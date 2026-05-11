// v2.3.7 Phase 4 — Ops recipes storage + types.
// v2.3.15 Phase 4.9 — Types extracted to the `ato-recipes` shared crate
//                     to stop desktop/CLI drift. This file now only
//                     handles desktop-side storage (SQLite + JSON mirror).
//
// Recipes are user-authored trigger→action workflows.
//
// SOURCE OF TRUTH: the `ops_recipes` SQLite table. Always.
// JSON SNAPSHOT (best-effort): `~/.ato/recipes/<slug>.json` is written
//   after each successful DB write so the user can `ls` to see what
//   exists. It is NOT a hand-editable surface in this phase — edits
//   to the JSON files are not reconciled back to SQLite. If the JSON
//   write fails, the DB write is NOT rolled back; the recipe still
//   works, the JSON is just stale.

use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

// Re-export shared types so existing call sites (`crate::recipes::*`)
// keep working without churn. The shared crate is the source of truth.
pub use ato_recipes::{
    action_type_name, builtin_templates, template_by_slug, trigger_type_name,
    validate_slug as shared_validate_slug, CreateRecipeInput, OpsRecipe, RecipeAction,
    RecipeTemplate, RecipeTrigger,
};

type Result<T> = std::result::Result<T, String>;

fn to_string_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Local wrapper preserving the desktop crate's existing call shape.
/// Forwards to ato_recipes::validate_slug — the source of truth.
pub fn validate_slug(slug: &str) -> Result<()> {
    shared_validate_slug(slug)
}

// ─── Storage paths ────────────────────────────────────────────────────

/// Directory where recipe JSON files mirror the SQLite rows. Created
/// on first write. Files: ~/.ato/recipes/<slug>.json.
pub fn recipes_dir() -> PathBuf {
    let mut p = crate::home_dir();
    p.push(".ato");
    p.push("recipes");
    p
}

fn recipe_json_path(slug: &str) -> PathBuf {
    let mut p = recipes_dir();
    p.push(format!("{}.json", slug));
    p
}

// ─── CRUD ─────────────────────────────────────────────────────────────

pub fn create(conn: &Connection, input: CreateRecipeInput) -> Result<OpsRecipe> {
    validate_slug(&input.slug)?;
    // Reject duplicates at the slug boundary.
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM ops_recipes WHERE slug = ?1 LIMIT 1",
            [&input.slug],
            |r| r.get(0),
        )
        .ok();
    if existing.is_some() {
        return Err(format!("Recipe with slug '{}' already exists.", input.slug));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let trigger_type = trigger_type_name(&input.trigger);
    let action_type = action_type_name(&input.action);
    let trigger_config = serde_json::to_string(&input.trigger).map_err(to_string_err)?;
    let action_config = serde_json::to_string(&input.action).map_err(to_string_err)?;

    conn.execute(
        "INSERT INTO ops_recipes (id, slug, name, description, trigger_type, trigger_config, action_type, action_config, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            id,
            input.slug,
            input.name,
            input.description,
            trigger_type,
            trigger_config,
            action_type,
            action_config,
            input.enabled as i64,
            now,
            now,
        ],
    )
    .map_err(to_string_err)?;

    let recipe = OpsRecipe {
        id,
        slug: input.slug.clone(),
        name: input.name,
        description: input.description,
        trigger: input.trigger,
        action: input.action,
        enabled: input.enabled,
        created_at: now.clone(),
        updated_at: now,
    };
    // JSON mirror is best-effort. A failed mirror write does NOT
    // unwind the SQLite create — the recipe is real either way,
    // the snapshot will refresh on next mutation.
    if let Err(e) = write_json_mirror(&recipe) {
        eprintln!("warning: ops_recipes json mirror write failed for '{}': {}", recipe.slug, e);
    }
    Ok(recipe)
}

pub fn list(conn: &Connection) -> Result<Vec<OpsRecipe>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
               FROM ops_recipes
              ORDER BY created_at DESC",
        )
        .map_err(to_string_err)?;
    let iter = stmt
        .query_map([], |r| {
            let trigger_json: String = r.get(4)?;
            let action_json: String = r.get(5)?;
            let enabled_int: i64 = r.get(6)?;
            // Deserialization happens outside the rusqlite Result; we
            // can't fail-with-string-error here, so we propagate via
            // a synthetic error type.
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                trigger_json,
                action_json,
                enabled_int,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
            ))
        })
        .map_err(to_string_err)?;

    let mut out: Vec<OpsRecipe> = Vec::new();
    for row in iter {
        let (id, slug, name, description, trigger_json, action_json, enabled_int, created_at, updated_at) =
            row.map_err(to_string_err)?;
        let trigger: RecipeTrigger = serde_json::from_str(&trigger_json).map_err(to_string_err)?;
        let action: RecipeAction = serde_json::from_str(&action_json).map_err(to_string_err)?;
        out.push(OpsRecipe {
            id,
            slug,
            name,
            description,
            trigger,
            action,
            enabled: enabled_int != 0,
            created_at,
            updated_at,
        });
    }
    Ok(out)
}

pub fn get(conn: &Connection, slug: &str) -> Result<Option<OpsRecipe>> {
    let row: Option<(String, String, String, Option<String>, String, String, i64, String, String)> = conn
        .query_row(
            "SELECT id, slug, name, description, trigger_config, action_config, enabled, created_at, updated_at
               FROM ops_recipes WHERE slug = ?1",
            [slug],
            |r| Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            )),
        )
        .ok();
    let Some((id, slug, name, description, trigger_json, action_json, enabled_int, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let trigger: RecipeTrigger = serde_json::from_str(&trigger_json).map_err(to_string_err)?;
    let action: RecipeAction = serde_json::from_str(&action_json).map_err(to_string_err)?;
    Ok(Some(OpsRecipe {
        id,
        slug,
        name,
        description,
        trigger,
        action,
        enabled: enabled_int != 0,
        created_at,
        updated_at,
    }))
}

pub fn set_enabled(conn: &Connection, slug: &str, enabled: bool) -> Result<OpsRecipe> {
    let now = chrono::Utc::now().to_rfc3339();
    let updated = conn
        .execute(
            "UPDATE ops_recipes SET enabled = ?1, updated_at = ?2 WHERE slug = ?3",
            rusqlite::params![enabled as i64, now, slug],
        )
        .map_err(to_string_err)?;
    if updated == 0 {
        return Err(format!("No recipe with slug '{}'.", slug));
    }
    let recipe = get(conn, slug)?.ok_or_else(|| "recipe vanished after update".to_string())?;
    if let Err(e) = write_json_mirror(&recipe) {
        eprintln!("warning: ops_recipes json mirror write failed for '{}': {}", recipe.slug, e);
    }
    Ok(recipe)
}

pub fn delete(conn: &Connection, slug: &str) -> Result<bool> {
    let n = conn
        .execute("DELETE FROM ops_recipes WHERE slug = ?1", [slug])
        .map_err(to_string_err)?;
    if n > 0 {
        let _ = fs::remove_file(recipe_json_path(slug));
        Ok(true)
    } else {
        Ok(false)
    }
}

// ─── JSON mirror ──────────────────────────────────────────────────────

fn write_json_mirror(recipe: &OpsRecipe) -> Result<()> {
    let dir = recipes_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(to_string_err)?;
    }
    let path = recipe_json_path(&recipe.slug);
    let json = serde_json::to_string_pretty(recipe).map_err(to_string_err)?;
    fs::write(path, json).map_err(to_string_err)?;
    Ok(())
}

// Type-name helpers, RecipeTemplate, builtin_templates, template_by_slug
// all live in the ato-recipes shared crate (re-exported above).
