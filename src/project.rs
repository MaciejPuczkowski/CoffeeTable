use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub github_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileTreeState {
    pub selected_path: Option<PathBuf>,
    pub expanded: Vec<PathBuf>,
    pub scroll_offset: usize,
}
