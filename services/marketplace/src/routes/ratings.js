/**
 * Ratings & Reviews API Routes
 *
 * Endpoints for rating skills and managing reviews.
 */

import { Router } from 'express';
import { nanoid } from 'nanoid';
import { z } from 'zod';
import { query, queryOne, queryAll } from '../services/db.js';
import { requireAuth, optionalAuth } from '../middleware/auth.js';

const router = Router();

// Validation schemas
const CreateRatingSchema = z.object({
  skillId: z.string().min(1),
  rating: z.number().int().min(1).max(5),
  title: z.string().max(200).optional(),
  review: z.string().max(5000).optional(),
});

const UpdateRatingSchema = z.object({
  rating: z.number().int().min(1).max(5).optional(),
  title: z.string().max(200).optional(),
  review: z.string().max(5000).optional(),
});

/**
 * GET /ratings/skill/:skillId
 * Get all ratings for a skill
 */
router.get('/skill/:skillId', optionalAuth, async (req, res, next) => {
  try {
    const { skillId } = req.params;
    const { sort = 'helpful', limit = 20, offset = 0 } = req.query;

    let orderBy = 'ORDER BY ';
    switch (sort) {
      case 'newest':
        orderBy += 'r.created_at DESC';
        break;
      case 'highest':
        orderBy += 'r.rating DESC, r.helpful_count DESC';
        break;
      case 'lowest':
        orderBy += 'r.rating ASC, r.helpful_count DESC';
        break;
      case 'helpful':
      default:
        orderBy += 'r.helpful_count DESC, r.created_at DESC';
    }

    // Get ratings with user info
    const ratings = await queryAll(
      `SELECT
        r.id, r.rating, r.title, r.review, r.is_verified_download,
        r.helpful_count, r.created_at, r.updated_at,
        u.id as user_id, u.name as user_name,
        v.version as rated_version
      FROM marketplace_ratings r
      JOIN users u ON r.user_id = u.id
      LEFT JOIN marketplace_skill_versions v ON r.version_id = v.id
      WHERE r.skill_id = $1
      ${orderBy}
      LIMIT $2 OFFSET $3`,
      [skillId, parseInt(limit), parseInt(offset)]
    );

    // Get rating distribution
    const distribution = await queryAll(
      `SELECT rating, COUNT(*) as count
       FROM marketplace_ratings
       WHERE skill_id = $1
       GROUP BY rating
       ORDER BY rating DESC`,
      [skillId]
    );

    // Fill in missing ratings
    const dist = { 5: 0, 4: 0, 3: 0, 2: 0, 1: 0 };
    distribution.forEach(d => { dist[d.rating] = parseInt(d.count); });

    // Get total count
    const total = await queryOne(
      'SELECT COUNT(*) as count FROM marketplace_ratings WHERE skill_id = $1',
      [skillId]
    );

    // Check if current user has rated
    let userRating = null;
    if (req.user) {
      userRating = await queryOne(
        'SELECT * FROM marketplace_ratings WHERE skill_id = $1 AND user_id = $2',
        [skillId, req.user.id]
      );
    }

    res.json({
      data: {
        ratings,
        distribution: dist,
        total: parseInt(total.count),
        userRating,
      }
    });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /ratings
 * Create or update a rating for a skill
 */
router.post('/', requireAuth, async (req, res, next) => {
  try {
    const data = CreateRatingSchema.parse(req.body);

    // Check skill exists
    const skill = await queryOne(
      'SELECT id FROM marketplace_skills WHERE id = $1 AND is_published = true',
      [data.skillId]
    );

    if (!skill) {
      return res.status(404).json({ error: { message: 'Skill not found' } });
    }

    // Check if user has downloaded this skill
    const download = await queryOne(
      'SELECT id, version_id FROM marketplace_downloads WHERE skill_id = $1 AND user_id = $2 ORDER BY downloaded_at DESC LIMIT 1',
      [data.skillId, req.user.id]
    );

    // Check if user already rated
    const existingRating = await queryOne(
      'SELECT id FROM marketplace_ratings WHERE skill_id = $1 AND user_id = $2',
      [data.skillId, req.user.id]
    );

    if (existingRating) {
      // Update existing rating
      await query(
        `UPDATE marketplace_ratings
         SET rating = $1, title = $2, review = $3, updated_at = NOW()
         WHERE id = $4`,
        [data.rating, data.title, data.review, existingRating.id]
      );

      const updated = await queryOne(
        'SELECT * FROM marketplace_ratings WHERE id = $1',
        [existingRating.id]
      );

      return res.json({ data: updated });
    }

    // Create new rating
    const ratingId = nanoid(12);

    await query(
      `INSERT INTO marketplace_ratings
        (id, skill_id, user_id, version_id, rating, title, review, is_verified_download)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
      [
        ratingId,
        data.skillId,
        req.user.id,
        download?.version_id || null,
        data.rating,
        data.title,
        data.review,
        !!download,
      ]
    );

    const newRating = await queryOne(
      'SELECT * FROM marketplace_ratings WHERE id = $1',
      [ratingId]
    );

    res.status(201).json({ data: newRating });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * PUT /ratings/:id
 * Update a rating
 */
router.put('/:id', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;
    const data = UpdateRatingSchema.parse(req.body);

    // Check ownership
    const rating = await queryOne(
      'SELECT * FROM marketplace_ratings WHERE id = $1 AND user_id = $2',
      [id, req.user.id]
    );

    if (!rating) {
      return res.status(404).json({ error: { message: 'Rating not found or not owned by you' } });
    }

    const updates = [];
    const params = [];
    let i = 1;

    if (data.rating !== undefined) {
      updates.push(`rating = $${i++}`);
      params.push(data.rating);
    }
    if (data.title !== undefined) {
      updates.push(`title = $${i++}`);
      params.push(data.title);
    }
    if (data.review !== undefined) {
      updates.push(`review = $${i++}`);
      params.push(data.review);
    }

    if (updates.length === 0) {
      return res.json({ data: rating });
    }

    updates.push('updated_at = NOW()');
    params.push(id);

    await query(
      `UPDATE marketplace_ratings SET ${updates.join(', ')} WHERE id = $${i}`,
      params
    );

    const updated = await queryOne('SELECT * FROM marketplace_ratings WHERE id = $1', [id]);
    res.json({ data: updated });
  } catch (err) {
    if (err instanceof z.ZodError) {
      return res.status(400).json({ error: { message: 'Validation error', details: err.errors } });
    }
    next(err);
  }
});

/**
 * DELETE /ratings/:id
 * Delete a rating
 */
router.delete('/:id', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    const rating = await queryOne(
      'SELECT * FROM marketplace_ratings WHERE id = $1 AND user_id = $2',
      [id, req.user.id]
    );

    if (!rating) {
      return res.status(404).json({ error: { message: 'Rating not found or not owned by you' } });
    }

    await query('DELETE FROM marketplace_ratings WHERE id = $1', [id]);

    res.json({ success: true });
  } catch (err) {
    next(err);
  }
});

/**
 * POST /ratings/:id/helpful
 * Mark a rating as helpful or not helpful
 */
router.post('/:id/helpful', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;
    const { isHelpful } = req.body;

    if (typeof isHelpful !== 'boolean') {
      return res.status(400).json({ error: { message: 'isHelpful must be a boolean' } });
    }

    // Check rating exists
    const rating = await queryOne('SELECT * FROM marketplace_ratings WHERE id = $1', [id]);

    if (!rating) {
      return res.status(404).json({ error: { message: 'Rating not found' } });
    }

    // Can't vote on own rating
    if (rating.user_id === req.user.id) {
      return res.status(400).json({ error: { message: 'Cannot vote on your own rating' } });
    }

    // Check for existing vote
    const existingVote = await queryOne(
      'SELECT * FROM marketplace_rating_votes WHERE rating_id = $1 AND user_id = $2',
      [id, req.user.id]
    );

    if (existingVote) {
      // Update vote
      await query(
        'UPDATE marketplace_rating_votes SET is_helpful = $1 WHERE id = $2',
        [isHelpful, existingVote.id]
      );
    } else {
      // Create vote
      await query(
        'INSERT INTO marketplace_rating_votes (id, rating_id, user_id, is_helpful) VALUES ($1, $2, $3, $4)',
        [nanoid(12), id, req.user.id, isHelpful]
      );
    }

    // Get updated helpful count
    const updated = await queryOne('SELECT helpful_count FROM marketplace_ratings WHERE id = $1', [id]);

    res.json({ data: { helpfulCount: updated.helpful_count } });
  } catch (err) {
    next(err);
  }
});

/**
 * DELETE /ratings/:id/helpful
 * Remove helpful vote
 */
router.delete('/:id/helpful', requireAuth, async (req, res, next) => {
  try {
    const { id } = req.params;

    await query(
      'DELETE FROM marketplace_rating_votes WHERE rating_id = $1 AND user_id = $2',
      [id, req.user.id]
    );

    const updated = await queryOne('SELECT helpful_count FROM marketplace_ratings WHERE id = $1', [id]);

    res.json({ data: { helpfulCount: updated?.helpful_count || 0 } });
  } catch (err) {
    next(err);
  }
});

/**
 * GET /ratings/user/mine
 * Get ratings made by current user
 */
router.get('/user/mine', requireAuth, async (req, res, next) => {
  try {
    const ratings = await queryAll(
      `SELECT r.*, s.name as skill_name, s.slug as skill_slug
       FROM marketplace_ratings r
       JOIN marketplace_skills s ON r.skill_id = s.id
       WHERE r.user_id = $1
       ORDER BY r.created_at DESC`,
      [req.user.id]
    );

    res.json({ data: ratings });
  } catch (err) {
    next(err);
  }
});

export default router;
