use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const LARGE_FILE_LINE_THRESHOLD: usize = 10_000;

pub struct Paths {
    pub db_file: PathBuf,
    pub settings_file: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "coffeetable", "coffeetable")
            .context("could not resolve user data directory")?;
        let data_dir = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("could not create data dir {}", data_dir.display()))?;
        Ok(Self {
            db_file: data_dir.join("coffeetable.db"),
            settings_file: data_dir.join("settings.yaml"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_roots")]
    pub roots: Vec<PathBuf>,
    #[serde(default = "default_search_excludes")]
    pub search_excludes: Vec<String>,
}

impl Settings {
    pub fn load_or_seed(path: &Path) -> Result<Self> {
        if path.exists() {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("could not read settings {}", path.display()))?;
            let s: Settings = serde_yaml::from_str(&raw)
                .with_context(|| format!("could not parse settings {}", path.display()))?;
            return Ok(s);
        }
        let s = Self::defaults();
        s.save(path)?;
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let yaml = serde_yaml::to_string(self)
            .context("could not serialize settings")?;
        std::fs::write(path, yaml)
            .with_context(|| format!("could not write settings {}", path.display()))?;
        Ok(())
    }

    pub fn defaults() -> Self {
        Self {
            roots: default_roots(),
            search_excludes: default_search_excludes(),
        }
    }
}

fn default_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("C:/Workspace/PRV"),
        PathBuf::from("C:/Workspace/SL"),
    ]
}

fn default_search_excludes() -> Vec<String> {
    vec![
        "node_modules".into(),
        ".next".into(),
        ".git".into(),
        ".idea".into(),
        ".vscode".into(),
        "bin".into(),
        "obj".into(),
    ]
}
