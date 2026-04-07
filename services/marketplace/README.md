# ATO Marketplace Service

Skill marketplace backend for the Agentic Tool Optimization platform.

## Features

- **Skill Discovery**: Search, filter, and browse published skills
- **Skill Submission**: Authors can submit and version their skills
- **Ratings & Reviews**: Users can rate and review skills
- **Skill Packs**: Curated collections of skills
- **Import/Export**: Share skill packs as JSON
- **Version Tracking**: Track installed skills and check for updates

## Endpoints

### Skills

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/skills` | Search and list skills |
| GET | `/skills/featured` | Get featured skills |
| GET | `/skills/categories` | List categories with counts |
| GET | `/skills/:id` | Get skill details |
| POST | `/skills` | Submit a new skill |
| PUT | `/skills/:id` | Update skill metadata |
| POST | `/skills/:id/publish` | Publish a skill |
| POST | `/skills/:id/unpublish` | Unpublish a skill |
| DELETE | `/skills/:id` | Delete a skill |
| GET | `/skills/:id/download` | Download skill content |
| GET | `/skills/user/mine` | Get user's own skills |

### Ratings

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/ratings/skill/:id` | Get ratings for a skill |
| POST | `/ratings` | Create/update a rating |
| PUT | `/ratings/:id` | Update a rating |
| DELETE | `/ratings/:id` | Delete a rating |
| POST | `/ratings/:id/helpful` | Vote rating as helpful |
| GET | `/ratings/user/mine` | Get user's ratings |

### Versions

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/versions/skill/:id` | List skill versions |
| GET | `/versions/:id` | Get version details |
| POST | `/versions/skill/:id` | Create new version |
| POST | `/versions/check-updates` | Check for skill updates |
| GET | `/versions/user/installed` | Get installed skills |
| POST | `/versions/user/install` | Track skill installation |
| DELETE | `/versions/user/uninstall/:id` | Untrack skill |

### Skill Packs

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/packs` | List published packs |
| GET | `/packs/featured` | Get featured packs |
| GET | `/packs/:id` | Get pack details |
| POST | `/packs` | Create a pack |
| PUT | `/packs/:id` | Update pack metadata |
| POST | `/packs/:id/publish` | Publish a pack |
| DELETE | `/packs/:id` | Delete a pack |
| GET | `/packs/:id/export` | Export pack as JSON |
| POST | `/packs/import` | Import pack from JSON |
| PUT | `/packs/:id/skills` | Update skills in pack |
| GET | `/packs/user/mine` | Get user's packs |

## Setup

### Environment Variables

```bash
DATABASE_URL=postgresql://user:pass@localhost:5432/ato_cloud
JWT_SECRET=your-jwt-secret-min-32-chars
PORT=3007  # Optional, defaults to 3007
```

### Database Migration

Run the migration against your PostgreSQL database:

```bash
psql $DATABASE_URL < database/migrations/001_marketplace_schema.sql
```

### Running

```bash
# Install dependencies
npm install

# Development
npm run dev

# Production
npm start
```

## API Examples

### Search Skills

```bash
# Search for code review skills
curl "http://localhost:3007/skills?q=code+review&runtime=claude&sort=rating"

# Get featured skills
curl "http://localhost:3007/skills/featured"
```

### Submit a Skill

```bash
curl -X POST http://localhost:3007/skills \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Code Reviewer",
    "description": "AI-powered code review skill",
    "category": "development",
    "tags": ["code-review", "quality"],
    "runtime": "claude",
    "content": "---\nname: code-reviewer\n---\n\nYou are a code reviewer..."
  }'
```

### Rate a Skill

```bash
curl -X POST http://localhost:3007/ratings \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "skillId": "abc123",
    "rating": 5,
    "title": "Excellent!",
    "review": "This skill saved me hours..."
  }'
```

### Export/Import Skill Packs

```bash
# Export a pack
curl "http://localhost:3007/packs/my-pack/export" > pack.json

# Import a pack
curl -X POST http://localhost:3007/packs/import \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d @pack.json
```

## Integration with ato-cloud

This service is designed to run alongside the existing ato-cloud services. To integrate:

1. Add the migration to ato-cloud's `database/migrations/`
2. Add marketplace service to `start.sh`:
   ```bash
   node services/marketplace/src/index.js &
   ```
3. Update API gateway to proxy `/marketplace/*` to port 3007

## License

MIT
