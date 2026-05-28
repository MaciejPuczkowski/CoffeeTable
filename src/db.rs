use crate::project::{FileTreeState, Project};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    github_url TEXT,
    last_opened_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_state (
    project_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (project_id, key),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
"#;

const KEY_FILE_TREE: &str = "file_tree";
const KEY_OPEN_PROJECTS: &str = "open_projects";
const KEY_ACTIVE_PROJECT: &str = "active_project";
const KEY_ROOTS: &str = "roots";

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("could not open db at {}", path.display()))?;
        conn.execute_batch(SCHEMA).context("could not init schema")?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(Self { conn })
    }

    pub fn upsert_project(&self, name: &str, path: &Path, github_url: Option<&str>) -> Result<i64> {
        let path_str = path.to_string_lossy();
        self.conn.execute(
            "INSERT INTO projects (name, path, github_url) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                github_url = COALESCE(excluded.github_url, projects.github_url)",
            params![name, path_str, github_url],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM projects WHERE path = ?1",
            params![path_str],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, path, github_url FROM projects ORDER BY last_opened_at DESC")?;
        let rows = stmt.query_map([], |row| {
            let path_str: String = row.get(2)?;
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: PathBuf::from(path_str),
                github_url: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn touch_project(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET last_opened_at = strftime('%s','now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn load_file_tree_state(&self, project_id: i64) -> Result<FileTreeState> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM project_state WHERE project_id = ?1 AND key = ?2",
                params![project_id, KEY_FILE_TREE],
                |r| r.get(0),
            )
            .optional()?;
        match raw {
            Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            None => Ok(FileTreeState::default()),
        }
    }

    pub fn save_file_tree_state(&self, project_id: i64, state: &FileTreeState) -> Result<()> {
        let json = serde_json::to_string(state)?;
        self.conn.execute(
            "INSERT INTO project_state (project_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id, key) DO UPDATE SET value = excluded.value",
            params![project_id, KEY_FILE_TREE, json],
        )?;
        Ok(())
    }

    pub fn load_open_projects(&self) -> Result<(Vec<i64>, Option<i64>)> {
        let open_raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                params![KEY_OPEN_PROJECTS],
                |r| r.get(0),
            )
            .optional()?;
        let active_raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                params![KEY_ACTIVE_PROJECT],
                |r| r.get(0),
            )
            .optional()?;
        let open: Vec<i64> = open_raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let active: Option<i64> = active_raw.and_then(|s| s.parse().ok());
        Ok((open, active))
    }

    pub fn save_open_projects(&self, open: &[i64], active: Option<i64>) -> Result<()> {
        let json = serde_json::to_string(open)?;
        self.set_app_state(KEY_OPEN_PROJECTS, &json)?;
        match active {
            Some(id) => self.set_app_state(KEY_ACTIVE_PROJECT, &id.to_string())?,
            None => {
                self.conn
                    .execute("DELETE FROM app_state WHERE key = ?1", params![KEY_ACTIVE_PROJECT])?;
            }
        }
        Ok(())
    }

    pub fn load_roots_or_seed(&self, defaults: &[&str]) -> Result<Vec<PathBuf>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                params![KEY_ROOTS],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(s) = raw {
            let v: Vec<String> = serde_json::from_str(&s).unwrap_or_default();
            return Ok(v.into_iter().map(PathBuf::from).collect());
        }
        let seeded: Vec<PathBuf> = defaults.iter().map(PathBuf::from).collect();
        self.save_roots(&seeded)?;
        Ok(seeded)
    }

    pub fn save_roots(&self, roots: &[PathBuf]) -> Result<()> {
        let strs: Vec<String> = roots
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let json = serde_json::to_string(&strs)?;
        self.set_app_state(KEY_ROOTS, &json)?;
        Ok(())
    }

    fn set_app_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
