use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

pub const DEFAULT_ROOTS: &[&str] = &["C:/Workspace/PRV", "C:/Workspace/SL"];
pub const LARGE_FILE_LINE_THRESHOLD: usize = 10_000;

pub struct Paths {
    pub db_file: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "coffeetable", "coffeetable")
            .context("could not resolve user data directory")?;
        let data_dir = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("could not create data dir {}", data_dir.display()))?;
        let db_file = data_dir.join("coffeetable.db");
        Ok(Self { db_file })
    }
}
