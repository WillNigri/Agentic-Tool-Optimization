use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::DbState;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InputRecord {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub content: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn parse_tags(raw: Option<String>) -> Vec<String> {
    match raw {
        None => Vec::new(),
        Some(s) if s.trim().is_empty() => Vec::new(),
        Some(s) => {
            if let Ok(tags) = serde_json::from_str::<Vec<String>>(&s) {
                return tags
                    .into_iter()
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect();
            }
            s.split(',')
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty())
                .collect()
        }
    }
}

fn row_to_input(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn assemble_input(
    raw: (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    ),
) -> InputRecord {
    let (id, slug, name, content, kind, tags_raw, created_at, updated_at) = raw;
    InputRecord {
        id,
        slug,
        name,
        content,
        kind,
        tags: parse_tags(tags_raw),
        created_at,
        updated_at,
    }
}

const INPUT_SELECT: &str =
    "SELECT id, slug, name, content, kind, tags, created_at, updated_at FROM inputs";

fn id_or_slug_column(input: &str) -> &'static str {
    if uuid::Uuid::parse_str(input).is_ok() {
        "id"
    } else {
        "slug"
    }
}

#[tauri::command]
pub fn list_inputs(db: State<'_, DbState>) -> Result<Vec<InputRecord>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let sql = format!("{} ORDER BY updated_at DESC", INPUT_SELECT);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_input)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(assemble_input(row.map_err(|e| e.to_string())?));
    }
    Ok(out)
}

#[tauri::command]
pub fn get_input(db: State<'_, DbState>, id: String) -> Result<InputRecord, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let sql = format!("{} WHERE {} = ?1", INPUT_SELECT, id_or_slug_column(&id));
    let raw = conn
        .query_row(&sql, params![id], row_to_input)
        .map_err(|e| format!("input not found: {}", e))?;
    Ok(assemble_input(raw))
}
