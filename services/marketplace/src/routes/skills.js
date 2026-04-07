/**
 * Skills API Routes
 *
 * Endpoints for skill discovery, submission, and management.
 */

import { Router } from 'express';
import { nanoid } from 'nanoid';
import crypto from 'crypto';
import { z } from 'zod';
import { query, queryOne, queryAll, transaction } from '../services/db.js';
import { requireAuth, optionalAuth } from '../middleware/auth.js';

const router = Router();

// Validation schemas
const SubmitSkillSchema = z.object({
  name: z.string().min(1).max(100),
  description: z.string().max(500).optional(),
  longDescription: z.string().max(10000).optional(),
  category: z.string().default('general'),
  tags: z.array(z.string()).default([]),
  license: z.string().default('MIT'),
  repositoryUrl: z.string().url().optional(),
  homepageUrl: z.string().url().optional(),
  runtime: z.enum(['claude', 'codex', 'hermes', 'openclaw', 'universal']).default('claude'),
  content: z.string().min(1).max(100000), // The SKILL.md content
  version: z.string().default('1.0.0'),
  changelog: z.string().optional(),
});

const SearchSchema = z.object({
  q: z.string().optional(),
  category: z.string().optional(),
  runtime: z.string().optional(),
  tags: z.string().optional(), // comma-separated
  sort: z.enum(['downloads', 'rating', 'newest', 'updated']).default('downloads'),
  limit: z.coerce.number().min(1).max(100).default(20),
  offset: z.coerce.number().min(0).default(0),
});

/**
 * GET /skills
 * Search and list published skills
 */
router.get('/', optionalAuth, async (req, res, next) => {
  try {
    const params = SearchSchema.parse(req.query);

    let whereClause = 'WHERE is_published = true';
    const queryParams = [];
    let paramIndex = 1;

    // Full-text search
    if (params.q) {
      whereClause += ` AND to_tsvector('english', name || ' ' || COALESCE(description, '') || ' ' || COALESCE(long_description, '')) @@ plainto_tsquery('english', $${paramIndex})`;
      queryParams.push(params.q);
      paramIndex++;
    }

    // Category filter
    if (params.category) {
      whereClause += ` AND category = $${paramIndex}`;
      queryParams.push(params.category);
      paramIndex++;
    }

    // Runtime filter
    if (params.runtime) {
      whereClause += ` AND runtime = $${paramIndex}`;
      queryParams.push(params.runtime);
      paramIndex++;
    }

    // Tags filter
    if (params.tags) {
      const tagArray = params.tags.split(',').map(t => t.trim());
      whereClause += ` AND tags && $${paramIndex}`;
      queryParams.push(tagArray);
      paramIndex++;
    }

    // Sorting
    let orderBy = 'ORDER BY ';
    switch (params.sort) {
      case 'rating':
        orderBy += 'average_rating DESC, total_ratings DESC';
        break;
      case 'newest':
        orderBy += 'created_at DESC';
        break;
      case 'updated':
        orderBy += 'updated_at DESC';
        break;
      case 'downloads':
      default:
        orderBy += 'total_downloads DESC';
    }

    // Get total count
    const countResult = await queryOne(
      `SELECT COUNT(*) as total FROM marketplace_skills ${whereClause}`,
      queryParams
    );

    // Get skills
    const skills = await queryAll(
      `SELECT
        s.id, s.name, s.slug, s.description, s.category, s.tags, s.runtime,
        s.total_downloads, s.total_ratings, s.average_rating,
        s.is_featured, s.is_verified, s.icon_url,
        s.created_at, s.updated_at,
        u.name as author_name, u.id as author_id,
        v.version as latest_version
      FROM marketplace_skills s
      LEFT JOIN users u ON s.author_id = u.id
      LEFT JOIN marketplace_skill_versions v ON s.id = v.skill_id AND v.is_latest = true
      ${whereClause}
      ${orderBy}
      LIMIT $${paramIndex} OFFSET $${paramIndex + 1}`,
      [...queryParams, params.limit, params.offset]
    );

    res.json({
      data: skills,
      pagination: {
        total: parseInt(countResult.total),
        limit: params.limit,
        offset: params.offset,
      }
    });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /skills/featured
 * Get featured skills for homepage
 */
router.get('/featured', async (req, res, next) => {
  try {
    const skills = await queryAll(
      `SELECT
        s.id, s.name, s.slug, s.description, s.category, s.tags, s.runtime,
        s.total_downloads, s.average_rating, s.icon_url,
        u.name as author_name,
        v.version as latest_version
      FROM marketplace_skills s
      LEFT JOIN users u ON s.author_id = u.id
      LEFT JOIN marketplace_skill_versions v ON s.id = v.skill_id AND v.is_latest = true
      WHERE s.is_published = true AND s.is_featured = true
      ORDER BY s.total_downloads DESC
      LIMIT 12`
    );
    res.json({ data: skills });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /skills/categories
 * Get available categories with counts
 */
router.get('/categories', async (req, res, next) => {
  try {
    const categories = await queryAll(
      `SELECT category, COUNT(*) as count
       FROM marketplace_skills
       WHERE is_published = true
       GROUP BY category
       ORDER BY count DESC`
    );
    res.json({ data: categories });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /skills/:idOrSlug
 * Get skill details by ID or slug
 */
router.get('/:idOrSlug', optionalAuth, async (req, res, next) => {
  try {
    const { idOrSlug } = req.params;

    const skill = await queryOne(
      `SELECT
        s.*,
        u.name as author_name, u.email as author_email,
        v.version as latest_version, v.content as latest_content,
        v.frontmatter, v.token_count, v.changelog as latest_changelog
      FROM marketplace_skills s
      LEFT JOIN users u ON s.author_id = u.id
      LEFT JOIN marketplace_skill_versions v ON s.id = v.skill_id AND v.is_latest = true
      WHERE (s.id = $1 OR s.slug = $1)
        AND (s.is_published = true OR s.author_id = $2)`,
      [idOrSlug, req.user?.id || '']
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found' } });
    }

    res.json({ data: skill });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /skills
 * Submit a new skill to the marketplace
 */
router.post('/', requireAuth, async (req, res, next) => {
  try {
    const data = SubmitSkillSchema.parse(req.body);

    // Generate slug from name
    const baseSlug = data.name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');

    // Check for existing slug
    const existing = await queryOne(
      'SELECT id FROM marketplace_skills WHERE slug = $1',
      [baseSlug]
    );

    const slug = existing ? `${baseSlug}-${nanoid(6)}` : baseSlug;
    const skillId = nanoid(12);
    const versionId = nanoid(12);

    // Hash content for integrity
    const fileHash = crypto.createHash('sha256').update(data.content).digest('hex');

    // Estimate token count (rough: ~4 chars per token)
    const tokenCount = Math.ceil(data.content.length / 4);

    // Parse frontmatter if present
    let frontmatter = {};
    const frontmatterMatch = data.content.match(/^---\n([\s\S]*?)\n---/);
    if (frontmatterMatch) {
      try {
        // Simple YAML-like parsing
        frontmatterMatch[1].split('\n').forEach(line => {
          const [key, ...valueParts] = line.split(':');
          if (key && valueParts.length) {
            frontmatter[key.trim()] = valueParts.join(':').trim();
          }
        });
      } catch (e) {
        // Ignore frontmatter parse errors
      }
    }

    await transaction(async (client) => {
      // Create skill
      await client.query(
        `INSERT INTO marketplace_skills
          (id, author_id, name, slug, description, long_description, category, tags, license, repository_url, homepage_url, runtime, is_published)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)`,
        [
          skillId, req.user.id, data.name, slug, data.description, data.longDescription,
          data.category, data.tags, data.license, data.repositoryUrl, data.homepageUrl,
          data.runtime, false // Not published by default
        ]
      );

      // Create initial version
      await client.query(
        `INSERT INTO marketplace_skill_versions
          (id, skill_id, version, content, frontmatter, changelog, file_hash, token_count, is_latest)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)`,
        [versionId, skillId, data.version, data.content, JSON.stringify(frontmatter), data.changelog, fileHash, tokenCount]
      );
    });

    const skill = await queryOne(
      `SELECT s.*, v.version, v.token_count
       FROM marketplace_skills s
       JOIN marketplace_skill_versions v ON s.id = v.skill_id AND v.is_latest = true
       WHERE s.id = $1`,
      [skillId]
    );

    res.status(201).json({ data: skill });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * PUT /skills/:id
 * Update skill metadata (not content - use versions for that)
 */
router.put('/:id', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    // Verify ownership
    const skill = await queryOne(
      'SELECT * FROM marketplace_skills WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found or not owned by you' } });
    }

    const updates = {};
    const allowed = ['name', 'description', 'longDescription', 'category', 'tags', 'license', 'repositoryUrl', 'homepageUrl'];

    for (const key of allowed) {
      if (req.body[key] !== undefined) {
        updates[key] = req.body[key];
      }
    }

    if (Object.keys(updates).length === 0) {
      return res.json({ data: skill });
    }

    // Build update query
    const setClauses = [];
    const params = [];
    let i = 1;

    for (const [key, value] of Object.entries(updates)) {
      const dbKey = key.replace(/([A-Z])/g, '_$1').toLowerCase(); // camelCase to snake_case
      setClauses.push(`${dbKey} = $${i}`);
      params.push(value);
      i++;
    }

    setClauses.push('updated_at = NOW()');
    params.push(id);

    await query(
      `UPDATE marketplace_skills SET ${setClauses.join(', ')} WHERE id = $${i}`,
      params
    );

    const updated = await queryOne('SELECT * FROM marketplace_skills WHERE id = $1', [id]);
    res.json({ data: updated });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /skills/:id/publish
 * Publish a skill to the marketplace
 */
router.post('/:id/publish', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const skill = await queryOne(
      'SELECT * FROM marketplace_skills WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found or not owned by you' } });
    }

    await query(
      'UPDATE marketplace_skills SET is_published = true, updated_at = NOW() WHERE id = $1',
      [id]
    );

    res.json({ data: { ...skill, is_published: true } });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /skills/:id/unpublish
 * Remove a skill from the marketplace (still accessible by author)
 */
router.post('/:id/unpublish', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const skill = await queryOne(
      'SELECT * FROM marketplace_skills WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found or not owned by you' } });
    }

    await query(
      'UPDATE marketplace_skills SET is_published = false, updated_at = NOW() WHERE id = $1',
      [id]
    );

    res.json({ data: { ...skill, is_published: false } });
  } catch (err) {
    next(err);
  }
});

/**
 * DELETE /skills/:id
 * Delete a skill (soft delete by unpublishing, or hard delete if never published)
 */
router.delete('/:id', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const skill = await queryOne(
      'SELECT * FROM marketplace_skills WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found or not owned by you' } });
    }

    // If never downloaded, hard delete
    if (skill.total_downloads === 0) {
      await query('DELETE FROM marketplace_skills WHERE id = $1', [id]);
    } else {
      // Soft delete by unpublishing
      await query('UPDATE marketplace_skills SET is_published = false WHERE id = $1', [id]);
    }

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /skills/:id/download
 * Download a skill (tracks download count)
 */
router.get('/:id/download', optionalAuth, async (req, res, next) => {
  try {
    const { id } = req.params;
    const { version: requestedVersion } = req.query;

    const skill = await queryOne(
      'SELECT * FROM marketplace_skills WHERE (id = $1 OR slug = $1) AND is_published = true',
      [id]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found' } });
    }

    // Get version (latest if not specified)
    let versionQuery = 'SELECT * FROM marketplace_skill_versions WHERE skill_id = $1';
    const versionParams = [skill.id];

    if (requestedVersion) {
      versionQuery += ' AND version = $2';
      versionParams.push(requestedVersion);
    } else {
      versionQuery += ' AND is_latest = true';
    }

    const skillVersion = await queryOne(versionQuery, versionParams);

    if (!skillVersion) {
      return res.status(404).json({ error: { message: 'Version not found' } });
    }

    // Track download
    const ipHash = crypto
      .createHash('sha256')
      .update(req.ip || 'unknown')
      .digest('hex')
      .substring(0, 16);

    await query(
      `INSERT INTO marketplace_downloads (id, skill_id, version_id, user_id, ip_hash)
       VALUES ($1, $2, $3, $4, $5)`,
      [nanoid(12), skill.id, skillVersion.id, req.user?.id || null, ipHash]
    );

    res.json({
      data: {
        skill: {
          id: skill.id,
          name: skill.name,
          slug: skill.slug,
          author_id: skill.author_id,
          runtime: skill.runtime,
        },
        version: skillVersion.version,
        content: skillVersion.content,
        frontmatter: skillVersion.frontmatter,
        fileHash: skillVersion.file_hash,
        tokenCount: skillVersion.token_count,
      }
    });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /skills/user/mine
 * Get skills created by current user
 */
router.get('/user/mine', requireAuth, async (req, res, next) => {
  try {
    const skills = await queryAll(
      `SELECT s.*, v.version as latest_version, v.token_count
       FROM marketplace_skills s
       LEFT JOIN marketplace_skill_versions v ON s.id = v.skill_id AND v.is_latest = true
       WHERE s.author_id = $1
       ORDER BY s.updated_at DESC`,
      [req.user.id]
    );
    res.json({ data: skills });
  } catch (err) {
    next(err);
  }
});

export default router;
