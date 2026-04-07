/**
 * Versions API Routes
 *
 * Endpoints for managing skill versions and checking for updates.
 */

import { Router } from 'express';
import { nanoid } from 'nanoid';
import crypto from 'crypto';
import { z } from 'zod';
import { query, queryOne, queryAll, transaction } from '../services/db.js';
import { requireAuth, optionalAuth } from '../middleware/auth.js';

const router = Router();

// Validation schemas
const CreateVersionSchema = z.object({
  content: z.string().min(1).max(100000),
  version: z.string().regex(/^\d+\.\d+\.\d+$/, 'Version must be semver format (e.g., 1.0.0)'),
  changelog: z.string().max(5000).optional(),
});

const CheckUpdatesSchema = z.object({
  skills: z.array(z.object({
    skillId: z.string(),
    currentVersion: z.string(),
  })),
});

/**
 * GET /versions/skill/:skillId
 * Get all versions of a skill
 */
router.get('/skill/:skillId', optionalAuth, async (req, res, next) => {
  try {
    const { skillId } = req.params;

    // Check skill exists and user has access
    const skill = await queryOne(
      `SELECT * FROM marketplace_skills
       WHERE (id = $1 OR slug = $1)
         AND (is_published = true OR author_id = $2)`,
      [skillId, req.user?.id || '']
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found' } });
    }

    const versions = await queryAll(
      `SELECT id, version, changelog, token_count, downloads, is_latest, created_at
       FROM marketplace_skill_versions
       WHERE skill_id = $1
       ORDER BY created_at DESC`,
      [skill.id]
    );

    res.json({
      data: {
        skill: { id: skill.id, name: skill.name, slug: skill.slug },
        versions,
      }
    });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /versions/:versionId
 * Get specific version details
 */
router.get('/:versionId', optionalAuth, async (req, res, next) => {
  try {
    const { versionId } = req.params;

    const version = await queryOne(
      `SELECT v.*, s.name as skill_name, s.slug as skill_slug, s.author_id
       FROM marketplace_skill_versions v
       JOIN marketplace_skills s ON v.skill_id = s.id
       WHERE v.id = $1
         AND (s.is_published = true OR s.author_id = $2)`,
      [versionId, req.user?.id || '']
    );

    if (!version) {
      return res.status(404).json({ error: { message: 'Version not found' } });
    }

    res.json({ data: version });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /versions/skill/:skillId
 * Create a new version of a skill
 */
router.post('/skill/:skillId', requireAuth, async (req, res, next) => {
  try {
    const { skillId } = req.params;
    const data = CreateVersionSchema.parse(req.body);

    // Check ownership
    const skill = await queryOne(
      'SELECT * FROM marketplace_skills WHERE id = $1 AND author_id = $2',
      [skillId, req.user.id]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found or not owned by you' } });
    }

    // Check version doesn't exist
    const existingVersion = await queryOne(
      'SELECT id FROM marketplace_skill_versions WHERE skill_id = $1 AND version = $2',
      [skillId, data.version]
    );

    if (existingVersion) {
      return res.status(400).json({ error: { message: `Version ${data.version} already exists` } });
    }

    // Compare versions to ensure new is greater than current latest
    const latestVersion = await queryOne(
      'SELECT version FROM marketplace_skill_versions WHERE skill_id = $1 AND is_latest = true',
      [skillId]
    );

    if (latestVersion) {
      const [latestMajor, latestMinor, latestPatch] = latestVersion.version.split('.').map(Number);
      const [newMajor, newMinor, newPatch] = data.version.split('.').map(Number);

      const isGreater =
        newMajor > latestMajor ||
        (newMajor === latestMajor && newMinor > latestMinor) ||
        (newMajor === latestMajor && newMinor === latestMinor && newPatch > latestPatch);

      if (!isGreater) {
        return res.status(400).json({
          error: { message: `New version (${data.version}) must be greater than current version (${latestVersion.version})` }
        });
      }
    }

    // Hash content for integrity
    const fileHash = crypto.createHash('sha256').update(data.content).digest('hex');
    const tokenCount = Math.ceil(data.content.length / 4);

    // Parse frontmatter
    let frontmatter = {};
    const frontmatterMatch = data.content.match(/^---\n([\s\S]*?)\n---/);
    if (frontmatterMatch) {
      try {
        frontmatterMatch[1].split('\n').forEach(line => {
          const [key, ...valueParts] = line.split(':');
          if (key && valueParts.length) {
            frontmatter[key.trim()] = valueParts.join(':').trim();
          }
        });
      } catch (e) {
        // Ignore parse errors
      }
    }

    const versionId = nanoid(12);

    await transaction(async (client) => {
      // Unset current latest
      await client.query(
        'UPDATE marketplace_skill_versions SET is_latest = false WHERE skill_id = $1',
        [skillId]
      );

      // Create new version
      await client.query(
        `INSERT INTO marketplace_skill_versions
          (id, skill_id, version, content, frontmatter, changelog, file_hash, token_count, is_latest)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)`,
        [versionId, skillId, data.version, data.content, JSON.stringify(frontmatter), data.changelog, fileHash, tokenCount]
      );

      // Update skill updated_at
      await client.query(
        'UPDATE marketplace_skills SET updated_at = NOW() WHERE id = $1',
        [skillId]
      );
    });

    const newVersion = await queryOne(
      'SELECT * FROM marketplace_skill_versions WHERE id = $1',
      [versionId]
    );

    res.status(201).json({ data: newVersion });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * POST /versions/check-updates
 * Check for updates for installed skills
 */
router.post('/check-updates', optionalAuth, async (req, res, next) => {
  try {
    const data = CheckUpdatesSchema.parse(req.body);

    if (data.skills.length === 0) {
      return res.json({ data: { updates: [] } });
    }

    const updates = [];

    for (const { skillId, currentVersion } of data.skills) {
      const latest = await queryOne(
        `SELECT v.version, v.changelog, v.created_at, s.name, s.slug
         FROM marketplace_skill_versions v
         JOIN marketplace_skills s ON v.skill_id = s.id
         WHERE (s.id = $1 OR s.slug = $1) AND v.is_latest = true AND s.is_published = true`,
        [skillId]
      );

      if (!latest) continue;

      // Compare versions
      const [currentMajor, currentMinor, currentPatch] = currentVersion.split('.').map(Number);
      const [latestMajor, latestMinor, latestPatch] = latest.version.split('.').map(Number);

      const hasUpdate =
        latestMajor > currentMajor ||
        (latestMajor === currentMajor && latestMinor > currentMinor) ||
        (latestMajor === currentMajor && latestMinor === currentMinor && latestPatch > currentPatch);

      if (hasUpdate) {
        updates.push({
          skillId,
          skillName: latest.name,
          skillSlug: latest.slug,
          currentVersion,
          latestVersion: latest.version,
          changelog: latest.changelog,
          releasedAt: latest.created_at,
          updateType:
            latestMajor > currentMajor ? 'major' :
            latestMinor > currentMinor ? 'minor' : 'patch',
        });
      }
    }

    res.json({ data: { updates } });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * GET /versions/user/installed
 * Get user's installed skills with update status
 */
router.get('/user/installed', requireAuth, async (req, res, next) => {
  try {
    const installed = await queryAll(
      `SELECT
        i.id, i.installed_version, i.installed_at, i.auto_update,
        s.id as skill_id, s.name, s.slug, s.description, s.runtime,
        v.version as latest_version, v.changelog as latest_changelog
       FROM user_installed_skills i
       JOIN marketplace_skills s ON i.skill_id = s.id
       JOIN marketplace_skill_versions v ON s.id = v.skill_id AND v.is_latest = true
       WHERE i.user_id = $1
       ORDER BY i.installed_at DESC`,
      [req.user.id]
    );

    // Add update status
    const withUpdateStatus = installed.map(skill => {
      const [currentMajor, currentMinor, currentPatch] = skill.installed_version.split('.').map(Number);
      const [latestMajor, latestMinor, latestPatch] = skill.latest_version.split('.').map(Number);

      const hasUpdate =
        latestMajor > currentMajor ||
        (latestMajor === currentMajor && latestMinor > currentMinor) ||
        (latestMajor === currentMajor && latestMinor === currentMinor && latestPatch > currentPatch);

      return {
        ...skill,
        hasUpdate,
        updateType: hasUpdate ? (
          latestMajor > currentMajor ? 'major' :
          latestMinor > currentMinor ? 'minor' : 'patch'
        ) : null,
      };
    });

    res.json({ data: withUpdateStatus });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /versions/user/install
 * Track that a user installed a skill
 */
router.post('/user/install', requireAuth, async (req, res, next) => {
  try {
    const { skillId, version } = req.body;

    if (!skillId || !version) {
      return res.status(400).json({ error: { message: 'skillId and version are required' } });
    }

    // Get version info
    const versionInfo = await queryOne(
      `SELECT v.id, v.version, s.id as skill_id
       FROM marketplace_skill_versions v
       JOIN marketplace_skills s ON v.skill_id = s.id
       WHERE (s.id = $1 OR s.slug = $1) AND v.version = $2`,
      [skillId, version]
    );

    if (!versionInfo) {
      return res.status(404).json({ error: { message: 'Skill version not found' } });
    }

    // Check if already installed
    const existing = await queryOne(
      'SELECT id FROM user_installed_skills WHERE user_id = $1 AND skill_id = $2',
      [req.user.id, versionInfo.skill_id]
    );

    if (existing) {
      // Update existing
      await query(
        `UPDATE user_installed_skills
         SET version_id = $1, installed_version = $2, last_checked_at = NOW()
         WHERE id = $3`,
        [versionInfo.id, version, existing.id]
      );
    } else {
      // Create new
      await query(
        `INSERT INTO user_installed_skills
          (id, user_id, skill_id, version_id, installed_version)
         VALUES ($1, $2, $3, $4, $5)`,
        [nanoid(12), req.user.id, versionInfo.skill_id, versionInfo.id, version]
      );
    }

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

/**
 * DELETE /versions/user/uninstall/:skillId
 * Remove skill from user's installed list
 */
router.delete('/user/uninstall/:skillId', requireAuth, async (req, res, next) => {
  try {
    const { skillId } = req.params;

    await query(
      'DELETE FROM user_installed_skills WHERE user_id = $1 AND skill_id = $2',
      [req.user.id, skillId]
    );

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

/**
 * PUT /versions/user/auto-update/:skillId
 * Toggle auto-update for a skill
 */
router.put('/user/auto-update/:skillId', requireAuth, async (req, res, next) => {
  try {
    const { skillId } = req.params;
    const { autoUpdate } = req.body;

    if (typeof autoUpdate !== 'boolean') {
      return res.status(400).json({ error: { message: 'autoUpdate must be a boolean' } });
    }

    await query(
      'UPDATE user_installed_skills SET auto_update = $1 WHERE user_id = $2 AND skill_id = $3',
      [autoUpdate, req.user.id, skillId]
    );

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

export default router;
