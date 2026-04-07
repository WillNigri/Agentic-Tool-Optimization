/**
 * ATO Marketplace Service
 *
 * Provides REST API for the skill marketplace:
 * - Skill discovery and search
 * - Skill submission and versioning
 * - Ratings and reviews
 * - Skill packs (collections)
 * - Download tracking
 */

import express from 'express';
import skillsRouter from './routes/skills.js';
import ratingsRouter from './routes/ratings.js';
import packsRouter from './routes/packs.js';
import versionsRouter from './routes/versions.js';

const app = express();
const PORT = process.env.PORT || 3007;

// Middleware
app.use(express.json({ limit: '1mb' }));

// CORS headers (handled by gateway in production)
app.use((req, res, next) => {
  res.header('Access-Control-Allow-Origin', '*');
  res.header('Access-Control-Allow-Methods', 'GET, POST, PUT, DELETE, OPTIONS');
  res.header('Access-Control-Allow-Headers', 'Content-Type, Authorization');
  if (req.method === 'OPTIONS') {
    return res.sendStatus(200);
  }
  next();
});

// Health check
app.get('/health', (req, res) => {
  res.json({ status: 'ok', service: 'marketplace', version: '0.7.0' });
});

// API Routes
app.use('/skills', skillsRouter);
app.use('/ratings', ratingsRouter);
app.use('/packs', packsRouter);
app.use('/versions', versionsRouter);

// 404 handler
app.use((req, res) => {
  res.status(404).json({ error: { message: 'Not found' } });
});

// Error handler
app.use((err, req, res, next) => {
  console.error('[Marketplace Error]', err);
  res.status(err.status || 500).json({
    error: { message: err.message || 'Internal server error' }
  });
});

app.listen(PORT, () => {
  console.log(`[Marketplace] Service running on port ${PORT}`);
});

export default app;
