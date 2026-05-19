// commands/recipes.rs — Ops recipes Tauri command surface.
//
// PR 20 of the commands.rs split (see COMMANDS_SPLIT_PLAN.md).
//
// Scope (7 commands):
//   - recipes_list             — list installed recipes
//   - recipes_get              — fetch one by slug
//   - recipes_create           — install a recipe from a CreateRecipeInput
//   - recipes_set_enabled      — enable/disable without delete
//   - recipes_delete           — remove a recipe
//   - recipes_templates        — built-in template registry (read-only)
//   - recipes_install_template — instantiate a template into a working recipe
//
// All commands are thin wrappers around `crate::recipes::*`. The execution
// engine (events::bus subscriber + action dispatcher) lives in
// crate::recipes and is owned there, not here. The `ato recipes runs`
// CLI lives in apps/cli/src/commands/recipes.rs (CLI side) and reads
// the ops_recipe_runs audit table directly.

use rusqlite::Connection;

use crate::{
    get_db_path,
    recipes::{self, CreateRecipeInput, OpsRecipe, RecipeTemplate},
};

#[tauri::command]
pub fn recipes_list() -> Result<Vec<OpsRecipe>, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    recipes::list(&conn)
}

#[tauri::command]
pub fn recipes_get(slug: String) -> Result<Option<OpsRecipe>, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    recipes::get(&conn, &slug)
}

#[tauri::command]
pub fn recipes_create(input: CreateRecipeInput) -> Result<OpsRecipe, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    recipes::create(&conn, input)
}

#[tauri::command]
pub fn recipes_set_enabled(slug: String, enabled: bool) -> Result<OpsRecipe, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    recipes::set_enabled(&conn, &slug, enabled)
}

#[tauri::command]
pub fn recipes_delete(slug: String) -> Result<bool, String> {
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    recipes::delete(&conn, &slug)
}

#[tauri::command]
pub fn recipes_templates() -> Vec<RecipeTemplate> {
    recipes::builtin_templates()
}

#[tauri::command]
pub fn recipes_install_template(
    slug: String,
    rename_to: Option<String>,
) -> Result<OpsRecipe, String> {
    let template = recipes::template_by_slug(&slug)
        .ok_or_else(|| format!("No template with slug '{}'.", slug))?;
    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let install_slug = rename_to.unwrap_or_else(|| template.slug.clone());
    let input = CreateRecipeInput {
        slug: install_slug,
        name: template.name,
        description: Some(template.description),
        trigger: template.trigger,
        action: template.action,
        enabled: true,
    };
    recipes::create(&conn, input)
}
