use crate::project::{
    CommentKind, CommentStatus, Feature, FeatureComment, FeatureStatus, FeatureStep, FileTreeState,
    Project, ProjectMeta, StepStatus,
};
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

CREATE TABLE IF NOT EXISTS project_meta (
    project_id INTEGER PRIMARY KEY,
    description TEXT NOT NULL DEFAULT '',
    conventions TEXT NOT NULL DEFAULT '',
    ai_hints TEXT NOT NULL DEFAULT '',
    ai_notes TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS project_settings (
    project_id INTEGER PRIMARY KEY,
    yaml TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS project_runtime (
    project_id INTEGER PRIMARY KEY,
    yaml TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS features (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'idea',
    order_idx INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS feature_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feature_id INTEGER NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'todo',
    order_idx INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS feature_comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feature_id INTEGER NOT NULL,
    message TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    kind TEXT NOT NULL DEFAULT 'note',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE
);
"#;

fn migrate(conn: &Connection) -> Result<()> {
    let needs_kind: bool = {
        let mut stmt = conn.prepare("PRAGMA table_info(feature_comments)")?;
        let mut found = false;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let name: String = r.get(1)?;
            if name == "kind" {
                found = true;
                break;
            }
        }
        !found
    };
    if needs_kind {
        conn.execute(
            "ALTER TABLE feature_comments ADD COLUMN kind TEXT NOT NULL DEFAULT 'note'",
            [],
        )?;
    }
    Ok(())
}

const KEY_FILE_TREE: &str = "file_tree";
const KEY_OPEN_PROJECTS: &str = "open_projects";
const KEY_ACTIVE_PROJECT: &str = "active_project";
const KEY_AGENT_SESSIONS: &str = "agent_sessions";

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("could not open db at {}", path.display()))?;
        conn.execute_batch(SCHEMA).context("could not init schema")?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&conn).context("could not migrate schema")?;
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

    pub fn load_agent_sessions(&self, project_id: i64) -> Result<Vec<String>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM project_state WHERE project_id = ?1 AND key = ?2",
                params![project_id, KEY_AGENT_SESSIONS],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    pub fn save_agent_sessions(&self, project_id: i64, sessions: &[String]) -> Result<()> {
        let json = serde_json::to_string(sessions)?;
        self.conn.execute(
            "INSERT INTO project_state (project_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id, key) DO UPDATE SET value = excluded.value",
            params![project_id, KEY_AGENT_SESSIONS, json],
        )?;
        Ok(())
    }

    pub fn load_project_settings_yaml(&self, project_id: i64) -> Result<Option<String>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT yaml FROM project_settings WHERE project_id = ?1",
                params![project_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw)
    }

    pub fn save_project_settings_yaml(&self, project_id: i64, yaml: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO project_settings (project_id, yaml) VALUES (?1, ?2)
             ON CONFLICT(project_id) DO UPDATE SET yaml = excluded.yaml",
            params![project_id, yaml],
        )?;
        Ok(())
    }

    pub fn load_project_meta(&self, project_id: i64) -> Result<ProjectMeta> {
        let row: Option<(String, String, String, String)> = self
            .conn
            .query_row(
                "SELECT description, conventions, ai_hints, ai_notes FROM project_meta WHERE project_id = ?1",
                params![project_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        Ok(match row {
            Some((description, conventions, ai_hints, ai_notes)) => ProjectMeta {
                description,
                conventions,
                ai_hints,
                ai_notes,
            },
            None => ProjectMeta::default(),
        })
    }

    pub fn save_project_meta(&self, project_id: i64, meta: &ProjectMeta) -> Result<()> {
        self.conn.execute(
            "INSERT INTO project_meta (project_id, description, conventions, ai_hints, ai_notes)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id) DO UPDATE SET
                description = excluded.description,
                conventions = excluded.conventions,
                ai_hints = excluded.ai_hints,
                ai_notes = excluded.ai_notes",
            params![
                project_id,
                meta.description,
                meta.conventions,
                meta.ai_hints,
                meta.ai_notes
            ],
        )?;
        Ok(())
    }

    pub fn list_features(&self, project_id: i64) -> Result<Vec<Feature>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, status, order_idx FROM features
             WHERE project_id = ?1 ORDER BY order_idx, id",
        )?;
        let rows = stmt.query_map(params![project_id], |r| {
            Ok(Feature {
                id: r.get(0)?,
                project_id,
                title: r.get(1)?,
                description: r.get(2)?,
                status: FeatureStatus::from_str(&r.get::<_, String>(3)?),
                order_idx: r.get(4)?,
                steps: Vec::new(),
                comments: Vec::new(),
            })
        })?;
        let mut out: Vec<Feature> = Vec::new();
        for r in rows {
            out.push(r?);
        }
        for f in &mut out {
            f.steps = self.list_steps(f.id)?;
            f.comments = self.list_comments(f.id)?;
        }
        Ok(out)
    }

    pub fn insert_feature(&self, project_id: i64, title: &str) -> Result<i64> {
        let next_idx: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(order_idx), -1) + 1 FROM features WHERE project_id = ?1",
                params![project_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO features (project_id, title, order_idx) VALUES (?1, ?2, ?3)",
            params![project_id, title, next_idx],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_feature(
        &self,
        feature_id: i64,
        title: &str,
        description: &str,
        status: FeatureStatus,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE features SET title = ?1, description = ?2, status = ?3 WHERE id = ?4",
            params![title, description, status.as_str(), feature_id],
        )?;
        Ok(())
    }

    pub fn update_feature_status(&self, feature_id: i64, status: FeatureStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE features SET status = ?1 WHERE id = ?2",
            params![status.as_str(), feature_id],
        )?;
        Ok(())
    }

    pub fn delete_feature(&self, feature_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM features WHERE id = ?1", params![feature_id])?;
        Ok(())
    }

    pub fn list_steps(&self, feature_id: i64) -> Result<Vec<FeatureStep>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, summary, status, order_idx FROM feature_steps
             WHERE feature_id = ?1 ORDER BY order_idx, id",
        )?;
        let rows = stmt.query_map(params![feature_id], |r| {
            Ok(FeatureStep {
                id: r.get(0)?,
                feature_id,
                summary: r.get(1)?,
                status: StepStatus::from_str(&r.get::<_, String>(2)?),
                order_idx: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_step(&self, feature_id: i64, summary: &str) -> Result<i64> {
        let next_idx: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(order_idx), -1) + 1 FROM feature_steps WHERE feature_id = ?1",
                params![feature_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO feature_steps (feature_id, summary, order_idx) VALUES (?1, ?2, ?3)",
            params![feature_id, summary, next_idx],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_step(&self, step_id: i64, summary: &str, status: StepStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE feature_steps SET summary = ?1, status = ?2 WHERE id = ?3",
            params![summary, status.as_str(), step_id],
        )?;
        Ok(())
    }

    pub fn delete_step(&self, step_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM feature_steps WHERE id = ?1", params![step_id])?;
        Ok(())
    }

    pub fn list_comments(&self, feature_id: i64) -> Result<Vec<FeatureComment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, message, status, kind, created_at FROM feature_comments
             WHERE feature_id = ?1 ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map(params![feature_id], |r| {
            Ok(FeatureComment {
                id: r.get(0)?,
                feature_id,
                message: r.get(1)?,
                status: CommentStatus::from_str(&r.get::<_, String>(2)?),
                kind: CommentKind::from_str(&r.get::<_, String>(3)?),
                created_at: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_comment_with_kind(
        &self,
        feature_id: i64,
        message: &str,
        kind: CommentKind,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO feature_comments (feature_id, message, kind) VALUES (?1, ?2, ?3)",
            params![feature_id, message, kind.as_str()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_comment(&self, comment_id: i64, message: &str, status: CommentStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE feature_comments SET message = ?1, status = ?2 WHERE id = ?3",
            params![message, status.as_str(), comment_id],
        )?;
        Ok(())
    }

    pub fn update_comment_kind(&self, comment_id: i64, kind: CommentKind) -> Result<()> {
        self.conn.execute(
            "UPDATE feature_comments SET kind = ?1 WHERE id = ?2",
            params![kind.as_str(), comment_id],
        )?;
        Ok(())
    }

    pub fn delete_comment(&self, comment_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM feature_comments WHERE id = ?1",
            params![comment_id],
        )?;
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
