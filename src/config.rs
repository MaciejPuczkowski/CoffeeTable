use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const LARGE_FILE_LINE_THRESHOLD: usize = 10_000;

pub struct Paths {
    pub db_file: PathBuf,
    pub settings_file: PathBuf,
    pub data_dir: PathBuf,
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
            data_dir,
        })
    }

    pub fn project_context_dir(&self, project_id: i64) -> PathBuf {
        self.data_dir
            .join("agents")
            .join(format!("project_{}", project_id))
    }

}

pub fn claude_projects_dir(cwd: &Path) -> Option<PathBuf> {
    let base = BaseDirs::new()?;
    Some(base.home_dir().join(".claude").join("projects").join(encode_cwd(cwd)))
}

fn encode_cwd(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c == ':' || c == '\\' || c == '/' { '-' } else { c })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_roots")]
    pub roots: Vec<PathBuf>,
    #[serde(default = "default_search_excludes")]
    pub search_excludes: Vec<String>,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub runtime: crate::runtime::RuntimeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default = "default_ai_provider")]
    pub provider: String,
    #[serde(default = "default_ai_binary")]
    pub binary: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_token_limit: Option<u64>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: default_ai_provider(),
            binary: default_ai_binary(),
            model: None,
            extra_args: Vec::new(),
            session_token_limit: None,
            weekly_token_limit: None,
        }
    }
}

fn default_ai_provider() -> String {
    "claude_cli".into()
}

fn default_ai_binary() -> String {
    "claude".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    #[serde(default = "default_shell_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            command: default_shell_command(),
            args: Vec::new(),
        }
    }
}

fn default_shell_command() -> String {
    if cfg!(target_os = "windows") {
        "powershell".into()
    } else {
        "bash".into()
    }
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
            ai: AiConfig::default(),
            shell: ShellConfig::default(),
            runtime: crate::runtime::RuntimeConfig::default(),
        }
    }

    pub fn with_project_overrides(&self, overrides: &ProjectSettings) -> Self {
        Self {
            roots: self.roots.clone(),
            search_excludes: overrides
                .search_excludes
                .clone()
                .unwrap_or_else(|| self.search_excludes.clone()),
            ai: overrides.ai.clone().unwrap_or_else(|| self.ai.clone()),
            shell: overrides.shell.clone().unwrap_or_else(|| self.shell.clone()),
            runtime: overrides
                .runtime
                .clone()
                .unwrap_or_else(|| self.runtime.clone()),
        }
    }
}

pub const PROJECT_SETTINGS_FILE: &str = "CoffeeTable.Settings.yaml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_excludes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<ShellConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<ViewsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<crate::runtime::RuntimeConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViewsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<bool>,
}

impl ProjectSettings {
    pub fn from_yaml(raw: &str) -> Result<Self> {
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_yaml::from_str(raw).context("could not parse project settings YAML")
    }

    pub fn empty_template() -> String {
        "# Project-scoped overrides for CoffeeTable.\n\
         # Unset keys fall back to the global settings on the left.\n\
         # Examples:\n\
         #\n\
         # search_excludes:\n\
         #   - node_modules\n\
         #   - target\n\
         #\n\
         # ai:\n\
         #   provider: claude_cli\n\
         #   binary: claude\n\
         #   model: claude-opus-4-7\n\
         #   extra_args: []\n\
         #   session_token_limit: 200000\n\
         #   weekly_token_limit: 5000000\n\
         #\n\
         # shell:\n\
         #   command: pwsh\n\
         #   args: []\n\
         #\n\
         # views:\n\
         #   project: false   # hide the Project (Kanban) tab for this project\n\
         #\n\
         # runtime:\n\
         #   services:\n\
         #     - name: api\n\
         #       command: cargo run -p api\n\
         #       cwd: services/api\n\
         #       build: cargo build -p api\n\
         #       depends_on: []\n\
         #       env:\n\
         #         RUST_LOG: debug\n"
            .into()
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
