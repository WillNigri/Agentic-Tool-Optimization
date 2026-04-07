/**
 * Skill Packs API Routes
 *
 * Endpoints for creating and managing skill collections.
 * Supports import/export for easy sharing.
 */

import { Router } from 'express';
import { nanoid } from 'nanoid';
import { z } from 'zod';
import { query, queryOne, queryAll, transaction } from '../services/db.js';
import { requireAuth, optionalAuth } from '../middleware/auth.js';

const router = Router();

// Validation schemas
const CreatePackSchema = z.object({
  name: z.string().min(1).max(100),
  description: z.string().max(1000).optional(),
  skillIds: z.array(z.string()).min(1).max(50),
});

const UpdatePackSchema = z.object({
  name: z.string().min(1).max(100).optional(),
  description: z.string().max(1000).optional(),
});

const ImportPackSchema = z.object({
  name: z.string().min(1).max(100),
  description: z.string().max(1000).optional(),
  skills: z.array(z.object({
    name: z.string(),
    description: z.string().optional(),
    content: z.string(),
    version: z.string().default('1.0.0'),
    runtime: z.enum(['claude', 'codex', 'hermes', 'openclaw', 'universal']).default('claude'),
    category: z.string().default('general'),
    tags: z.array(z.string()).default([]),
  })).min(1).max(50),
});

/**
 * GET /packs
 * List published skill packs
 */
router.get('/', async (req, res, next) => {
  try {
    const { limit = 20, offset = 0, sort = 'downloads' } = req.query;

    let orderBy = 'ORDER BY ';
    switch (sort) {
      case 'newest':
        orderBy += 'p.created_at DESC';
        break;
      case 'downloads':
      default:
        orderBy += 'p.total_downloads DESC';
    }

    const packs = await queryAll(
      `SELECT
        p.id, p.name, p.slug, p.description, p.icon_url,
        p.total_downloads, p.created_at, p.updated_at,
        u.name as author_name,
        COUNT(pi.id) as skill_count
       FROM marketplace_skill_packs p
       JOIN users u ON p.author_id = u.id
       LEFT JOIN marketplace_skill_pack_items pi ON p.id = pi.pack_id
       WHERE p.is_published = true
       GROUP BY p.id, u.name
       ${orderBy}
       LIMIT $1 OFFSET $2`,
      [parseInt(limit), parseInt(offset)]
    );

    const total = await queryOne(
      'SELECT COUNT(*) as count FROM marketplace_skill_packs WHERE is_published = true'
    );

    res.json({
      data: packs,
      pagination: {
        total: parseInt(total.count),
        limit: parseInt(limit),
        offset: parseInt(offset),
      }
    });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /packs/featured
 * Get featured skill packs
 */
router.get('/featured', async (req, res, next) => {
  try {
    const packs = await queryAll(
      `SELECT
        p.id, p.name, p.slug, p.description, p.icon_url,
        p.total_downloads,
        u.name as author_name,
        COUNT(pi.id) as skill_count
       FROM marketplace_skill_packs p
       JOIN users u ON p.author_id = u.id
       LEFT JOIN marketplace_skill_pack_items pi ON p.id = pi.pack_id
       WHERE p.is_published = true AND p.is_featured = true
       GROUP BY p.id, u.name
       ORDER BY p.total_downloads DESC
       LIMIT 6`
    );

    res.json({ data: packs });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /packs/:idOrSlug
 * Get pack details with included skills
 */
router.get('/:idOrSlug', optionalAuth, async (req, res, next) => {
  try {
    const { idOrSlug } = req.params;

    const pack = await queryOne(
      `SELECT p.*, u.name as author_name
       FROM marketplace_skill_packs p
       JOIN users u ON p.author_id = u.id
       WHERE (p.id = $1 OR p.slug = $1)
         AND (p.is_published = true OR p.author_id = $2)`,
      [idOrSlug, req.user?.id || '']
    );

    if (!pack) {
      return res.status(404).json({ error: { message: 'Pack not found' } });
    }

    // Get skills in the pack
    const skills = await queryAll(
      `SELECT
        s.id, s.name, s.slug, s.description, s.runtime, s.average_rating,
        s.total_downloads, s.icon_url,
        pi.position,
        COALESCE(v.version, lv.version) as version
       FROM marketplace_skill_pack_items pi
       JOIN marketplace_skills s ON pi.skill_id = s.id
       LEFT JOIN marketplace_skill_versions v ON pi.version_id = v.id
       LEFT JOIN marketplace_skill_versions lv ON s.id = lv.skill_id AND lv.is_latest = true
       WHERE pi.pack_id = $1
       ORDER BY pi.position`,
      [pack.id]
    );

    res.json({
      data: {
        ...pack,
        skills,
      }
    });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /packs
 * Create a new skill pack from existing marketplace skills
 */
router.post('/', requireAuth, async (req, res, next) => {
  try {
    const data = CreatePackSchema.parse(req.body);

    // Generate slug
    const baseSlug = data.name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');

    const existing = await queryOne('SELECT id FROM marketplace_skill_packs WHERE slug = $1', [baseSlug]);
    const slug = existing ? `${baseSlug}-${nanoid(6)}` : baseSlug;

    const packId = nanoid(12);

    await transaction(async (client) => {
      // Create pack
      await client.query(
        `INSERT INTO marketplace_skill_packs (id, author_id, name, slug, description)
         VALUES ($1, $2, $3, $4, $5)`,
        [packId, req.user.id, data.name, slug, data.description]
      );

      // Add skills to pack
      for (let i = 0; i < data.skillIds.length; i++) {
        const skillId = data.skillIds[i];

        // Verify skill exists and is published
        const skill = await client.query(
          'SELECT id FROM marketplace_skills WHERE id = $1 AND is_published = true',
          [skillId]
        );

        if (skill.rows.length > 0) {
          await client.query(
            `INSERT INTO marketplace_skill_pack_items (id, pack_id, skill_id, position)
             VALUES ($1, $2, $3, $4)`,
            [nanoid(12), packId, skillId, i]
          );
        }
      }
    });

    const pack = await queryOne(
      `SELECT p.*, COUNT(pi.id) as skill_count
       FROM marketplace_skill_packs p
       LEFT JOIN marketplace_skill_pack_items pi ON p.id = pi.pack_id
       WHERE p.id = $1
       GROUP BY p.id`,
      [packId]
    );

    res.status(201).json({ data: pack });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * PUT /packs/:id
 * Update pack metadata
 */
router.put('/:id', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;
    const data = UpdatePackSchema.parse(req.body);

    const pack = await queryOne(
      'SELECT * FROM marketplace_skill_packs WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!pack) {
      return res.status(404).json({ error: { message: 'Pack not found or not owned by you' } });
    }

    const updates = [];
    const params = [];
    let i = 1;

    if (data.name !== undefined) {
      updates.push(`name = $${i++}`);
      params.push(data.name);
    }
    if (data.description !== undefined) {
      updates.push(`description = $${i++}`);
      params.push(data.description);
    }

    if (updates.length > 0) {
      updates.push('updated_at = NOW()');
      params.push(id);
      await query(`UPDATE marketplace_skill_packs SET ${updates.join(', ')} WHERE id = $${i}`, params);
    }

    const updated = await queryOne('SELECT * FROM marketplace_skill_packs WHERE id = $1', [id]);
    res.json({ data: updated });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * POST /packs/:id/publish
 * Publish a skill pack
 */
router.post('/:id/publish', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const pack = await queryOne(
      'SELECT * FROM marketplace_skill_packs WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!pack) {
      return res.status(404).json({ error: { message: 'Pack not found or not owned by you' } });
    }

    await query('UPDATE marketplace_skill_packs SET is_published = true, updated_at = NOW() WHERE id = $1', [id]);

    res.json({ data: { ...pack, is_published: true } });
  } catch (err) {
    next(err);
  }
});

/**
 * DELETE /packs/:id
 * Delete a skill pack
 */
router.delete('/:id', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const pack = await queryOne(
      'SELECT * FROM marketplace_skill_packs WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!pack) {
      return res.status(404).json({ error: { message: 'Pack not found or not owned by you' } });
    }

    await query('DELETE FROM marketplace_skill_packs WHERE id = $1', [id]);

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /packs/:id/export
 * Export a skill pack as JSON (includes all skill content)
 */
router.get('/:id/export', optionalAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const pack = await queryOne(
      `SELECT p.*, u.name as author_name
       FROM marketplace_skill_packs p
       JOIN users u ON p.author_id = u.id
       WHERE (p.id = $1 OR p.slug = $1) AND p.is_published = true`,
      [id]
    );

    if (!pack) {
      return res.status(404).json({ error: { message: 'Pack not found' } });
    }

    // Get skills with content
    const skills = await queryAll(
      `SELECT
        s.name, s.description, s.runtime, s.category, s.tags,
        v.version, v.content, v.frontmatter
       FROM marketplace_skill_pack_items pi
       JOIN marketplace_skills s ON pi.skill_id = s.id
       LEFT JOIN marketplace_skill_versions v ON pi.version_id = v.id
       LEFT JOIN marketplace_skill_versions lv ON s.id = lv.skill_id AND lv.is_latest = true
       WHERE pi.pack_id = $1
       ORDER BY pi.position`,
      [pack.id]
    );

    // Track download
    await query(
      `UPDATE marketplace_skill_packs
       SET total_downloads = total_downloads + 1, updated_at = NOW()
       WHERE id = $1`,
      [pack.id]
    );

    const exportData = {
      formatVersion: '1.0',
      exportedAt: new Date().toISOString(),
      pack: {
        name: pack.name,
        description: pack.description,
        author: pack.author_name,
      },
      skills: skills.map(s => ({
        name: s.name,
        description: s.description,
        runtime: s.runtime,
        category: s.category,
        tags: s.tags,
        version: s.version,
        content: s.content,
      })),
    };

    res.json({ data: exportData });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /packs/import
 * Import a skill pack from exported JSON
 * Creates new skills and pack under current user
 */
router.post('/import', requireAuth, async (req, res, next) => {
  try {
    const data = ImportPackSchema.parse(req.body);

    // Generate pack slug
    const baseSlug = data.name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');

    const existing = await queryOne('SELECT id FROM marketplace_skill_packs WHERE slug = $1', [baseSlug]);
    const packSlug = existing ? `${baseSlug}-${nanoid(6)}` : baseSlug;

    const packId = nanoid(12);
    const createdSkillIds = [];

    await transaction(async (client) => {
      // Create pack first
      await client.query(
        `INSERT INTO marketplace_skill_packs (id, author_id, name, slug, description)
         VALUES ($1, $2, $3, $4, $5)`,
        [packId, req.user.id, data.name, packSlug, data.description]
      );

      // Create each skill
      for (let i = 0; i < data.skills.length; i++) {
        const skill = data.skills[i];
        const skillId = nanoid(12);
        const versionId = nanoid(12);

        // Generate skill slug
        const skillBaseSlug = skill.name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, '-')
          .replace(/^-|-$/g, '');

        const skillExisting = await client.query('SELECT id FROM marketplace_skills WHERE slug = $1', [skillBaseSlug]);
        const skillSlug = skillExisting.rows.length > 0 ? `${skillBaseSlug}-${nanoid(6)}` : skillBaseSlug;

        // Hash content
        const crypto = await import('crypto');
        const fileHash = crypto.createHash('sha256').update(skill.content).digest('hex');
        const tokenCount = Math.ceil(skill.content.length / 4);

        // Create skill
        await client.query(
          `INSERT INTO marketplace_skills
            (id, author_id, name, slug, description, category, tags, runtime, is_published)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false)`,
          [skillId, req.user.id, skill.name, skillSlug, skill.description, skill.category, skill.tags, skill.runtime]
        );

        // Create version
        await client.query(
          `INSERT INTO marketplace_skill_versions
            (id, skill_id, version, content, file_hash, token_count, is_latest)
           VALUES ($1, $2, $3, $4, $5, $6, true)`,
          [versionId, skillId, skill.version, skill.content, fileHash, tokenCount]
        );

        // Add to pack
        await client.query(
          `INSERT INTO marketplace_skill_pack_items (id, pack_id, skill_id, version_id, position)
           VALUES ($1, $2, $3, $4, $5)`,
          [nanoid(12), packId, skillId, versionId, i]
        );

        createdSkillIds.push(skillId);
      }
    });

    const pack = await queryOne(
      `SELECT p.*, COUNT(pi.id) as skill_count
       FROM marketplace_skill_packs p
       LEFT JOIN marketplace_skill_pack_items pi ON p.id = pi.pack_id
       WHERE p.id = $1
       GROUP BY p.id`,
      [packId]
    );

    res.status(201).json({
      data: {
        pack,
        createdSkillIds,
      }
    });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * PUT /packs/:id/skills
 * Update skills in a pack (add/remove/reorder)
 */
router.put('/:id/skills', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;
    const { skillIds } = req.body;

    if (!Array.isArray(skillIds)) {
      return res.status(400).json({ error: { message: 'skillIds must be an array' } });
    }

    const pack = await queryOne(
      'SELECT * FROM marketplace_skill_packs WHERE id = $1 AND author_id = $2',
      [id, req.user.id]
    );

    if (!pack) {
      return res.status(404).json({ error: { message: 'Pack not found or not owned by you' } });
    }

    await transaction(async (client) => {
      // Remove all existing items
      await client.query('DELETE FROM marketplace_skill_pack_items WHERE pack_id = $1', [id]);

      // Add new items
      for (let i = 0; i < skillIds.length; i++) {
        const skillId = skillIds[i];

        // Verify skill exists
        const skill = await client.query('SELECT id FROM marketplace_skills WHERE id = $1', [skillId]);

        if (skill.rows.length > 0) {
          await client.query(
            `INSERT INTO marketplace_skill_pack_items (id, pack_id, skill_id, position)
             VALUES ($1, $2, $3, $4)`,
            [nanoid(12), id, skillId, i]
          );
        }
      }

      await client.query('UPDATE marketplace_skill_packs SET updated_at = NOW() WHERE id = $1', [id]);
    });

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /packs/user/mine
 * Get packs created by current user
 */
router.get('/user/mine', requireAuth, async (req, res, next) => {
  try {
    const packs = await queryAll(
      `SELECT p.*, COUNT(pi.id) as skill_count
       FROM marketplace_skill_packs p
       LEFT JOIN marketplace_skill_pack_items pi ON p.id = pi.pack_id
       WHERE p.author_id = $1
       GROUP BY p.id
       ORDER BY p.updated_at DESC`,
      [req.user.id]
    );

    res.json({ data: packs });
  } catch (err) {
    next(err);
  }
});

export default router;
