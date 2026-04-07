-- Marketplace Database Schema
-- Phase 5: v0.7.0 — Marketplace Backend

-- Published skills in the marketplace
CREATE TABLE IF NOT EXISTS marketplace_skills (
  id TEXT PRIMARY KEY,
  author_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  description TEXT,
  long_description TEXT,
  category TEXT NOT NULL DEFAULT 'general',
  tags TEXT[] DEFAULT '{}',
  license TEXT DEFAULT 'MIT',
  repository_url TEXT,
  homepage_url TEXT,
  icon_url TEXT,
  runtime TEXT NOT NULL DEFAULT 'claude', -- claude, codex, hermes, openclaw, universal
  is_published BOOLEAN DEFAULT false,
  is_featured BOOLEAN DEFAULT false,
  is_verified BOOLEAN DEFAULT false,
  total_downloads INTEGER DEFAULT 0,
  total_ratings INTEGER DEFAULT 0,
  average_rating NUMERIC(3,2) DEFAULT 0.00,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Version history for skills
CREATE TABLE IF NOT EXISTS marketplace_skill_versions (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL REFERENCES marketplace_skills(id) ON DELETE CASCADE,
  version TEXT NOT NULL, -- semver: 1.0.0, 1.0.1, etc.
  content TEXT NOT NULL, -- The actual SKILL.md content
  frontmatter JSONB DEFAULT '{}', -- Parsed frontmatter
  changelog TEXT,
  file_hash TEXT, -- SHA-256 of content for integrity
  token_count INTEGER DEFAULT 0,
  is_latest BOOLEAN DEFAULT false,
  downloads INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  UNIQUE(skill_id, version)
);

-- User ratings and reviews
CREATE TABLE IF NOT EXISTS marketplace_ratings (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL REFERENCES marketplace_skills(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  version_id TEXT REFERENCES marketplace_skill_versions(id) ON DELETE SET NULL,
  rating INTEGER NOT NULL CHECK (rating >= 1 AND rating <= 5),
  title TEXT,
  review TEXT,
  is_verified_download BOOLEAN DEFAULT false, -- Did user actually download it?
  helpful_count INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW(),
  UNIQUE(skill_id, user_id) -- One review per user per skill
);

-- Track downloads for analytics
CREATE TABLE IF NOT EXISTS marketplace_downloads (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL REFERENCES marketplace_skills(id) ON DELETE CASCADE,
  version_id TEXT NOT NULL REFERENCES marketplace_skill_versions(id) ON DELETE CASCADE,
  user_id TEXT REFERENCES users(id) ON DELETE SET NULL, -- NULL for anonymous
  ip_hash TEXT, -- Hashed IP for anonymous counting
  downloaded_at TIMESTAMPTZ DEFAULT NOW()
);

-- Skill packs (collections)
CREATE TABLE IF NOT EXISTS marketplace_skill_packs (
  id TEXT PRIMARY KEY,
  author_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  description TEXT,
  icon_url TEXT,
  is_published BOOLEAN DEFAULT false,
  is_featured BOOLEAN DEFAULT false,
  total_downloads INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Skills in packs (many-to-many)
CREATE TABLE IF NOT EXISTS marketplace_skill_pack_items (
  id TEXT PRIMARY KEY,
  pack_id TEXT NOT NULL REFERENCES marketplace_skill_packs(id) ON DELETE CASCADE,
  skill_id TEXT NOT NULL REFERENCES marketplace_skills(id) ON DELETE CASCADE,
  version_id TEXT REFERENCES marketplace_skill_versions(id) ON DELETE SET NULL, -- NULL = latest
  position INTEGER DEFAULT 0, -- For ordering
  added_at TIMESTAMPTZ DEFAULT NOW(),
  UNIQUE(pack_id, skill_id)
);

-- User's installed skills (for tracking and update notifications)
CREATE TABLE IF NOT EXISTS user_installed_skills (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  skill_id TEXT NOT NULL REFERENCES marketplace_skills(id) ON DELETE CASCADE,
  version_id TEXT NOT NULL REFERENCES marketplace_skill_versions(id) ON DELETE CASCADE,
  installed_version TEXT NOT NULL,
  installed_at TIMESTAMPTZ DEFAULT NOW(),
  last_checked_at TIMESTAMPTZ DEFAULT NOW(),
  auto_update BOOLEAN DEFAULT false,
  UNIQUE(user_id, skill_id)
);

-- Review helpfulness votes
CREATE TABLE IF NOT EXISTS marketplace_rating_votes (
  id TEXT PRIMARY KEY,
  rating_id TEXT NOT NULL REFERENCES marketplace_ratings(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  is_helpful BOOLEAN NOT NULL,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  UNIQUE(rating_id, user_id)
);

-- Skill report/flags for moderation
CREATE TABLE IF NOT EXISTS marketplace_skill_reports (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL REFERENCES marketplace_skills(id) ON DELETE CASCADE,
  reporter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  reason TEXT NOT NULL, -- 'malicious', 'inappropriate', 'copyright', 'spam', 'other'
  description TEXT,
  status TEXT DEFAULT 'pending', -- 'pending', 'reviewed', 'resolved', 'dismissed'
  reviewed_by TEXT REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_author ON marketplace_skills(author_id);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_category ON marketplace_skills(category);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_runtime ON marketplace_skills(runtime);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_published ON marketplace_skills(is_published);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_featured ON marketplace_skills(is_featured);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_rating ON marketplace_skills(average_rating DESC);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_downloads ON marketplace_skills(total_downloads DESC);
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_slug ON marketplace_skills(slug);

CREATE INDEX IF NOT EXISTS idx_skill_versions_skill ON marketplace_skill_versions(skill_id);
CREATE INDEX IF NOT EXISTS idx_skill_versions_latest ON marketplace_skill_versions(skill_id, is_latest);

CREATE INDEX IF NOT EXISTS idx_ratings_skill ON marketplace_ratings(skill_id);
CREATE INDEX IF NOT EXISTS idx_ratings_user ON marketplace_ratings(user_id);

CREATE INDEX IF NOT EXISTS idx_downloads_skill ON marketplace_downloads(skill_id);
CREATE INDEX IF NOT EXISTS idx_downloads_date ON marketplace_downloads(downloaded_at);

CREATE INDEX IF NOT EXISTS idx_installed_user ON user_installed_skills(user_id);
CREATE INDEX IF NOT EXISTS idx_installed_skill ON user_installed_skills(skill_id);

-- Full text search on skills
CREATE INDEX IF NOT EXISTS idx_marketplace_skills_search ON marketplace_skills
  USING GIN (to_tsvector('english', name || ' ' || COALESCE(description, '') || ' ' || COALESCE(long_description, '')));

-- Function to update skill rating stats
CREATE OR REPLACE FUNCTION update_skill_rating_stats()
RETURNS TRIGGER AS $$
BEGIN
  UPDATE marketplace_skills
  SET
    total_ratings = (SELECT COUNT(*) FROM marketplace_ratings WHERE skill_id = COALESCE(NEW.skill_id, OLD.skill_id)),
    average_rating = (SELECT COALESCE(AVG(rating), 0) FROM marketplace_ratings WHERE skill_id = COALESCE(NEW.skill_id, OLD.skill_id)),
    updated_at = NOW()
  WHERE id = COALESCE(NEW.skill_id, OLD.skill_id);
  RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-update rating stats
DROP TRIGGER IF EXISTS trigger_update_skill_rating_stats ON marketplace_ratings;
CREATE TRIGGER trigger_update_skill_rating_stats
  AFTER INSERT OR UPDATE OR DELETE ON marketplace_ratings
  FOR EACH ROW
  EXECUTE FUNCTION update_skill_rating_stats();

-- Function to increment download count
CREATE OR REPLACE FUNCTION increment_download_count()
RETURNS TRIGGER AS $$
BEGIN
  UPDATE marketplace_skills SET total_downloads = total_downloads + 1, updated_at = NOW() WHERE id = NEW.skill_id;
  UPDATE marketplace_skill_versions SET downloads = downloads + 1 WHERE id = NEW.version_id;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-increment downloads
DROP TRIGGER IF EXISTS trigger_increment_downloads ON marketplace_downloads;
CREATE TRIGGER trigger_increment_downloads
  AFTER INSERT ON marketplace_downloads
  FOR EACH ROW
  EXECUTE FUNCTION increment_download_count();

-- Function to update helpful count on ratings
CREATE OR REPLACE FUNCTION update_rating_helpful_count()
RETURNS TRIGGER AS $$
BEGIN
  UPDATE marketplace_ratings
  SET helpful_count = (
    SELECT COUNT(*) FROM marketplace_rating_votes
    WHERE rating_id = COALESCE(NEW.rating_id, OLD.rating_id) AND is_helpful = true
  )
  WHERE id = COALESCE(NEW.rating_id, OLD.rating_id);
  RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

-- Trigger for helpful votes
DROP TRIGGER IF EXISTS trigger_update_helpful_count ON marketplace_rating_votes;
CREATE TRIGGER trigger_update_helpful_count
  AFTER INSERT OR UPDATE OR DELETE ON marketplace_rating_votes
  FOR EACH ROW
  EXECUTE FUNCTION update_rating_helpful_count();
