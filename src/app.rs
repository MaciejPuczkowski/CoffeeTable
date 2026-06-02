use crate::{
    config::{self, ProjectSettings, Settings},
    db::Db,
    discovery, git,
    project::Project,
    views::{
        changes::ChangesView,
        editor::{COMMANDS, EditorMode, EditorView, GitView, filter_commands},
        file_tree::{Action as FileTreeAction, FileTreeView},
        grep::GrepView,
        project_picker::{PickerItem, PickerMode, ProjectPicker},
    },
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

pub enum AppMode {
    Normal,
    Picker,
    Grep,
    OpenConfirm,
    ConfirmDeleteFeature,
    Palette,
    ExplorerFilter,
    AiCommit,
    AgentRename,
    WorktreePrompt,
    FilePrompt,
    ConfirmDeleteFile,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPane {
    Global,
    Project,
}

pub struct SettingsOverlay {
    pub global: EditorView,
    pub project: EditorView,
    pub focus: SettingsPane,
    pub project_id: Option<i64>,
    pub project_path: Option<PathBuf>,
    pub status: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilePromptKind {
    NewFile,
    NewDir,
    Rename,
}

pub struct FilePromptState {
    pub kind: FilePromptKind,
    pub parent: PathBuf,
    pub source: Option<PathBuf>,
    pub buffer: String,
    pub cursor: usize,
}

pub struct AgentRenameState {
    pub project_id: i64,
    pub agent_idx: usize,
    pub buffer: String,
    pub cursor: usize,
}

pub struct WorktreePromptState {
    pub project_id: i64,
    pub buffer: String,
    pub cursor: usize,
}

pub enum AiResult {
    Single(String),
    Plan(Vec<crate::ai::CommitPlan>),
}

pub enum AiCommitState {
    Loading {
        rx: std::sync::mpsc::Receiver<Result<AiResult, String>>,
        spinner: usize,
    },
    Reviewing {
        editor: EditorView,
    },
    ReviewingPlan {
        messages: Vec<EditorView>,
        files: Vec<Vec<String>>,
        current: usize,
    },
    Error(String),
}

pub struct AiCommitOverlay {
    pub state: AiCommitState,
    pub project_path: PathBuf,
}

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selection: usize,
}

pub struct PendingOpen {
    pub path: PathBuf,
    pub content: String,
    pub line_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Editor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusContext {
    ProjectTabs,
    ViewTabs,
    SubTabs,
    Body,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NavDir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftPaneMode {
    Tree,
    Changes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAction {
    Run,
    Stop,
    Build,
    Restart,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Editor,
    Terminal,
    Project,
    Agents,
    Git,
    Github,
    Runtime,
}

pub struct ProjectViewState {
    pub tree: FileTreeView,
    pub changes: ChangesView,
    pub left_pane: LeftPaneMode,
    pub editor: Option<EditorView>,
    pub focus: Focus,
    pub preferred_git_view: GitView,
    pub terminals: Vec<crate::views::terminal::TerminalView>,
    pub active_terminal: Option<usize>,
    pub agents: Vec<crate::views::agents::AgentSession>,
    pub active_agent: Option<usize>,
    pub agent_resumed_this_run: bool,
    pub view_mode: ViewMode,
    pub project_view: Option<crate::views::project_view::ProjectViewModel>,
    pub git_view: Option<crate::views::git::GitTreeView>,
    pub github_view: Option<crate::views::github::GithubView>,
    pub github_available: Option<bool>,
    pub runtime: Option<crate::runtime::Runtime>,
    pub branch: Option<String>,
}

impl ProjectViewState {
    pub fn set_editor(&mut self, view: EditorView) {
        self.tree.reveal_path(&view.path);
        self.editor = Some(view);
    }

    pub fn selected_path(&self) -> Option<&std::path::Path> {
        match self.left_pane {
            LeftPaneMode::Tree => self.tree.selected_path(),
            LeftPaneMode::Changes => self.changes.selected_path(),
        }
    }
}

pub struct App {
    pub db: Db,
    pub settings_path: PathBuf,
    pub paths: crate::config::Paths,
    pub roots: Vec<PathBuf>,
    pub search_excludes: Vec<String>,
    pub ai_config: crate::config::AiConfig,
    pub shell_config: crate::config::ShellConfig,
    pub project_settings_yaml: HashMap<i64, String>,
    pub runtime_yaml: HashMap<i64, String>,
    pub open_projects: Vec<Project>,
    pub active_index: usize,
    pub project_views: HashMap<i64, ProjectViewState>,
    pub mode: AppMode,
    pub picker: Option<ProjectPicker>,
    pub grep: Option<GrepView>,
    pub palette: Option<PaletteState>,
    pub ai_commit: Option<AiCommitOverlay>,
    pub settings_overlay: Option<SettingsOverlay>,
    pub pending_open: Option<PendingOpen>,
    pub pending_delete_feature: Option<(i64, String)>,
    pub should_quit: bool,
    pub status: String,
    pub help_visible: bool,
    pub leader_pending: bool,
    pub terminal_prefix: bool,
    pub tabs_area: Rect,
    pub tab_rects: Vec<Rect>,
    pub view_tabs_area: Rect,
    pub view_tab_rects: Vec<(ViewMode, Rect)>,
    pub terminal_tabs_area: Rect,
    pub terminal_tab_rects: Vec<Rect>,
    pub terminal_new_rect: Option<Rect>,
    pub agent_tabs_area: Rect,
    pub agent_tab_rects: Vec<Rect>,
    pub agent_new_rect: Option<Rect>,
    pub left_pane_area: Rect,
    pub right_pane_area: Rect,
    pub runtime_list_area: Rect,
    pub project_list_inner: Rect,
    pub feature_form_field_rects: Vec<(crate::views::feature_form::FormFocus, Rect)>,
    pub feature_form_status_rects: Vec<(crate::project::FeatureStatus, Rect)>,
    pub agent_lane_visible: bool,
    pub agent_lane_area: Rect,
    pub agent_lane_tile_rects: Vec<(i64, usize, Rect)>,
    pub agent_lane_width: u16,
    pub lane_dragging: bool,
    pub agent_rename: Option<AgentRenameState>,
    pub worktree_prompt: Option<WorktreePromptState>,
    pub file_prompt: Option<FilePromptState>,
    pub file_yank: Option<PathBuf>,
    pub pending_delete_file: Option<PathBuf>,
    pub split_pct: u16,
    pub split_dragging: bool,
    pub focus_context: FocusContext,
    pub tab_chord: Option<Instant>,
    pub last_disk_poll: Option<Instant>,
    pub token_usage: Arc<Mutex<crate::token_usage::TokenSnapshot>>,
}

impl App {
    pub fn new(db: Db, paths: crate::config::Paths) -> Result<Self> {
        let settings_path = paths.settings_file.clone();
        let settings = Settings::load_or_seed(&settings_path)?;
        let roots = settings.roots;
        let search_excludes = settings.search_excludes;
        let ai_config = settings.ai;
        let shell_config = settings.shell;
        let (open_ids, active_id) = db.load_open_projects()?;
        let all = db.list_projects()?;
        let open_projects: Vec<Project> = open_ids
            .into_iter()
            .filter_map(|id| all.iter().find(|p| p.id == id).cloned())
            .collect();
        let active_index = active_id
            .and_then(|id| open_projects.iter().position(|p| p.id == id))
            .unwrap_or(0);

        let mut project_views: HashMap<i64, ProjectViewState> = HashMap::new();
        let mut project_settings_yaml: HashMap<i64, String> = HashMap::new();
        for p in &open_projects {
            let state = db.load_file_tree_state(p.id)?;
            let status = git::fetch_status(&p.path);
            let mut tree = FileTreeView::new(p.path.clone(), state)?;
            tree.set_git_status(status.clone());
            let mut changes = ChangesView::new(p.path.clone());
            changes.set_status(&status);
            let yaml = db.load_project_settings_yaml(p.id)?.unwrap_or_default();
            project_settings_yaml.insert(p.id, yaml);
            let branch = git::current_branch(&p.path);
            project_views.insert(
                p.id,
                ProjectViewState {
                    tree,
                    changes,
                    left_pane: LeftPaneMode::Tree,
                    editor: None,
                    focus: Focus::Tree,
                    preferred_git_view: GitView::Working,
                    terminals: Vec::new(),
                    active_terminal: None,
                    agents: Vec::new(),
                    active_agent: None,
                    agent_resumed_this_run: false,
                    view_mode: ViewMode::Editor,
                    project_view: None,
                    git_view: None,
                    github_view: None,
                    github_available: None,
                    runtime: None,
                    branch,
                },
            );
        }

        let (mode, picker) = if open_projects.is_empty() {
            let saved = all;
            let discovered = discover_new(&roots, &saved);
            (
                AppMode::Picker,
                Some(ProjectPicker::new(saved, discovered, roots.clone())),
            )
        } else {
            (AppMode::Normal, None)
        };

        Ok(Self {
            db,
            settings_path,
            paths,
            roots,
            search_excludes,
            ai_config,
            shell_config,
            project_settings_yaml,
            runtime_yaml: HashMap::new(),
            open_projects,
            active_index,
            project_views,
            mode,
            picker,
            grep: None,
            palette: None,
            ai_commit: None,
            settings_overlay: None,
            pending_open: None,
            pending_delete_feature: None,
            should_quit: false,
            status: String::new(),
            help_visible: false,
            leader_pending: false,
            terminal_prefix: false,
            tabs_area: Rect::default(),
            tab_rects: Vec::new(),
            view_tabs_area: Rect::default(),
            view_tab_rects: Vec::new(),
            terminal_tabs_area: Rect::default(),
            terminal_tab_rects: Vec::new(),
            terminal_new_rect: None,
            agent_tabs_area: Rect::default(),
            agent_tab_rects: Vec::new(),
            agent_new_rect: None,
            left_pane_area: Rect::default(),
            right_pane_area: Rect::default(),
            runtime_list_area: Rect::default(),
            project_list_inner: Rect::default(),
            feature_form_field_rects: Vec::new(),
            feature_form_status_rects: Vec::new(),
            agent_lane_visible: true,
            agent_lane_area: Rect::default(),
            agent_lane_tile_rects: Vec::new(),
            agent_lane_width: 36,
            lane_dragging: false,
            agent_rename: None,
            worktree_prompt: None,
            file_prompt: None,
            file_yank: None,
            pending_delete_file: None,
            split_pct: 30,
            split_dragging: false,
            focus_context: FocusContext::Body,
            tab_chord: None,
            last_disk_poll: None,
            token_usage: {
                let state = Arc::new(Mutex::new(crate::token_usage::TokenSnapshot::default()));
                crate::token_usage::start_background_scanner(state.clone());
                state
            },
        })
    }

    pub fn active_project(&self) -> Option<&Project> {
        self.open_projects.get(self.active_index)
    }

    pub fn active_state(&mut self) -> Option<&mut ProjectViewState> {
        let id = self.open_projects.get(self.active_index)?.id;
        self.project_views.get_mut(&id)
    }

    pub fn active_state_ref(&self) -> Option<&ProjectViewState> {
        let id = self.open_projects.get(self.active_index)?.id;
        self.project_views.get(&id)
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.help_visible {
            self.help_visible = false;
            return Ok(());
        }
        if self.leader_pending {
            self.leader_pending = false;
            self.handle_leader_key(key)?;
            return Ok(());
        }
        if matches!(self.mode, AppMode::AgentRename) {
            return self.on_key_agent_rename(key);
        }
        if matches!(self.mode, AppMode::WorktreePrompt) {
            return self.on_key_worktree_prompt(key);
        }
        if matches!(self.mode, AppMode::FilePrompt) {
            return self.on_key_file_prompt(key);
        }
        if matches!(self.mode, AppMode::ConfirmDeleteFile) {
            return self.on_key_confirm_delete_file(key);
        }
        if self.is_help_key(key) {
            self.help_visible = true;
            return Ok(());
        }
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::SHIFT)
        {
            self.copy_current_context();
            return Ok(());
        }
        if matches!(self.mode, AppMode::Normal) {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('j') => {
                        self.cycle_view_mode(true);
                        return Ok(());
                    }
                    KeyCode::Char('k') => {
                        self.cycle_view_mode(false);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        if key.code == KeyCode::Char(' ')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && self.should_activate_leader()
        {
            self.leader_pending = true;
            return Ok(());
        }
        if key.code == KeyCode::Char('p')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.should_open_palette()
        {
            self.open_palette();
            return Ok(());
        }
        match self.mode {
            AppMode::Picker => self.on_key_picker(key)?,
            AppMode::Grep => self.on_key_grep(key)?,
            AppMode::OpenConfirm => self.on_key_open_confirm(key)?,
            AppMode::ConfirmDeleteFeature => self.on_key_confirm_delete_feature(key)?,
            AppMode::Palette => self.on_key_palette(key)?,
            AppMode::ExplorerFilter => self.on_key_explorer_filter(key)?,
            AppMode::AiCommit => self.on_key_ai_commit(key)?,
            AppMode::AgentRename => self.on_key_agent_rename(key)?,
            AppMode::WorktreePrompt => self.on_key_worktree_prompt(key)?,
            AppMode::FilePrompt => self.on_key_file_prompt(key)?,
            AppMode::ConfirmDeleteFile => self.on_key_confirm_delete_file(key)?,
            AppMode::Settings => self.on_key_settings(key)?,
            AppMode::Normal => self.on_key_normal(key)?,
        }
        Ok(())
    }

    fn should_open_palette(&self) -> bool {
        if !matches!(self.mode, AppMode::Normal) {
            return false;
        }
        let Some(state) = self.active_state_ref() else {
            return true;
        };
        if matches!(state.view_mode, ViewMode::Terminal) {
            return false;
        }
        match state.focus {
            Focus::Tree => true,
            Focus::Editor => match state.editor.as_ref().map(|e| e.mode) {
                Some(EditorMode::Insert | EditorMode::Search) => false,
                _ => true,
            },
        }
    }

    fn open_palette(&mut self) {
        self.palette = Some(PaletteState::default());
        self.mode = AppMode::Palette;
    }

    fn close_palette(&mut self) {
        self.palette = None;
        self.mode = AppMode::Normal;
    }

    fn on_key_palette(&mut self, key: KeyEvent) -> Result<()> {
        let Some(palette) = self.palette.as_mut() else {
            return Ok(());
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.close_palette(),
            (KeyCode::Enter, _) => self.run_palette_enter()?,
            (KeyCode::Backspace, _) => {
                if palette.query.pop().is_none() {
                    self.close_palette();
                }
                if let Some(p) = self.palette.as_mut() {
                    p.selection = 0;
                }
            }
            (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                let filtered = filter_commands(&palette.query);
                if !filtered.is_empty() {
                    palette.selection = (palette.selection + 1).min(filtered.len() - 1);
                }
            }
            (KeyCode::Up, _) | (KeyCode::BackTab, _) => {
                palette.selection = palette.selection.saturating_sub(1);
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => {
                palette.query.push(c);
                palette.selection = 0;
            }
            _ => {}
        }
        Ok(())
    }

    fn run_palette_enter(&mut self) -> Result<()> {
        let (raw, selection) = {
            let palette = self.palette.as_ref().expect("palette present");
            (palette.query.trim().to_string(), palette.selection)
        };
        let force = raw.ends_with('!');
        let filtered = filter_commands(&raw);
        let to_run = match filtered
            .get(selection.min(filtered.len().saturating_sub(1)))
            .copied()
        {
            Some(idx) => {
                let key = COMMANDS[idx].key.to_string();
                if force { format!("{}!", key) } else { key }
            }
            None => raw,
        };
        self.close_palette();
        self.run_palette_command(&to_run)?;
        Ok(())
    }

    fn run_palette_command(&mut self, cmd: &str) -> Result<()> {
        let (base, force) = match cmd.strip_suffix('!') {
            Some(rest) => (rest, true),
            None => (cmd, false),
        };
        let trimmed = base.trim();
        if let Some(rest) = match_runtime_keyword(trimmed, "run") {
            self.palette_runtime_action(RuntimeAction::Run, &rest);
            return Ok(());
        }
        if let Some(rest) = match_runtime_keyword(trimmed, "stop") {
            self.palette_runtime_action(RuntimeAction::Stop, &rest);
            return Ok(());
        }
        if let Some(rest) = match_runtime_keyword(trimmed, "build") {
            self.palette_runtime_action(RuntimeAction::Build, &rest);
            return Ok(());
        }
        if let Some(rest) = match_runtime_keyword(trimmed, "restart") {
            self.palette_runtime_action(RuntimeAction::Restart, &rest);
            return Ok(());
        }
        match base {
            "w" | "write" => self.palette_save(),
            "q" | "close" => self.palette_close_editor(force),
            "wq" | "x" => self.palette_save_and_close(),
            "Q" | "qa" | "quit" => self.palette_quit_app(force),
            "e" | "edit" | "reload" => self.palette_reload(force),
            "f" | "find" => self.open_explorer_filter(),
            "g" | "grep" => self.open_grep(),
            "p" | "projects" => self.open_picker()?,
            "t" | "tree" | "explorer" => self.focus_tree(),
            "b" | "buffer" => self.focus_editor(),
            "h" | "help" => self.help_visible = true,
            "S" | "settings" | "config" => self.open_settings_in_editor()?,
            "H" | "head" | "old" => self.palette_show_head(),
            "W" | "working" | "work" | "new" => self.palette_show_working(),
            "D" | "diff" => self.palette_show_diff(),
            "L" | "lane" => self.toggle_agent_lane(),
            "R" | "rename" => self.start_rename_active_agent(),
            "runtime" => {
                let _ = self.open_runtime_view();
            }
            "import-settings" | "psi" => self.palette_import_project_settings(),
            "export-settings" | "pse" => self.palette_export_project_settings(),
            "import-runtime" | "rti" => self.palette_import_runtime(),
            "export-runtime" | "rte" => self.palette_export_runtime(),
            "" => {}
            other => self.status = format!("Not a command: {}", other),
        }
        Ok(())
    }

    fn palette_save(&mut self) {
        let Some(state) = self.active_state() else {
            self.status = "No editor open".into();
            return;
        };
        let Some(editor) = state.editor.as_mut() else {
            self.status = "No editor open".into();
            return;
        };
        let path = editor.path.clone();
        if let Err(e) = editor.save() {
            self.status = format!("Save error: {}", e);
            return;
        }
        if path == self.settings_path {
            self.reload_settings_from_disk();
        } else {
            let _ = self.refresh_git_status();
        }
    }

    fn palette_close_editor(&mut self, force: bool) {
        let Some(state) = self.active_state() else {
            self.status = "No editor open".into();
            return;
        };
        let Some(editor) = state.editor.as_mut() else {
            self.status = "No editor open".into();
            return;
        };
        if !editor.modified || force {
            state.editor = None;
            state.focus = Focus::Tree;
        } else {
            self.status = "No write since last change (q! to force)".into();
        }
    }

    fn palette_save_and_close(&mut self) {
        self.palette_save();
        if self.status.starts_with("Save error") {
            return;
        }
        if let Some(state) = self.active_state() {
            state.editor = None;
            state.focus = Focus::Tree;
        }
    }

    fn palette_quit_app(&mut self, force: bool) {
        let modified = self
            .active_state_ref()
            .and_then(|s| s.editor.as_ref())
            .map(|e| e.modified)
            .unwrap_or(false);
        if !modified || force {
            self.should_quit = true;
        } else {
            self.status = "No write since last change (Q! to force)".into();
        }
    }

    fn palette_show_head(&mut self) {
        let Some((project_path, file_path)) = self.editor_file_in_project() else {
            return;
        };
        let Ok(rel) = file_path.strip_prefix(&project_path) else {
            self.status = "File is outside the active project".into();
            return;
        };
        match git::show_head(&project_path, rel) {
            Some(content) => {
                self.apply_alt_view(GitView::Head, content);
                self.remember_preferred_view(GitView::Head);
            }
            None => self.status = "Could not read HEAD version (file untracked?)".into(),
        }
    }

    fn palette_show_diff(&mut self) {
        let Some((project_path, file_path)) = self.editor_file_in_project() else {
            return;
        };
        let Ok(rel) = file_path.strip_prefix(&project_path) else {
            self.status = "File is outside the active project".into();
            return;
        };
        match git::file_diff_head(&project_path, rel) {
            Some(content) if !content.is_empty() => {
                self.apply_alt_view(GitView::Diff, prettify_diff(&content));
                self.remember_preferred_view(GitView::Diff);
            }
            Some(_) => self.status = "No changes against HEAD".into(),
            None => self.status = "Could not produce diff".into(),
        }
    }

    fn palette_show_working(&mut self) {
        let Some(state) = self.active_state() else {
            self.status = "No editor open".into();
            return;
        };
        let Some(editor) = state.editor.as_mut() else {
            self.status = "No editor open".into();
            return;
        };
        editor.show_working();
        self.remember_preferred_view(GitView::Working);
    }

    fn remember_preferred_view(&mut self, view: GitView) {
        if let Some(state) = self.active_state() {
            state.preferred_git_view = view;
        }
    }

    fn apply_preferred_view_in_changes(&mut self) {
        let preferred = self
            .active_state_ref()
            .map(|s| s.preferred_git_view)
            .unwrap_or(GitView::Working);
        let in_changes = self
            .active_state_ref()
            .map(|s| matches!(s.left_pane, LeftPaneMode::Changes))
            .unwrap_or(false);
        if !in_changes {
            return;
        }
        match preferred {
            GitView::Working => {}
            GitView::Head => self.palette_show_head(),
            GitView::Diff => self.palette_show_diff(),
        }
    }

    fn editor_file_in_project(&mut self) -> Option<(PathBuf, PathBuf)> {
        let project_path = self.active_project()?.path.clone();
        let file_path = self
            .active_state_ref()
            .and_then(|s| s.editor.as_ref())
            .map(|e| e.path.clone());
        match file_path {
            Some(p) => Some((project_path, p)),
            None => {
                self.status = "No editor open".into();
                None
            }
        }
    }

    fn apply_alt_view(&mut self, view: GitView, content: String) {
        if let Some(editor) = self.active_state().and_then(|s| s.editor.as_mut()) {
            editor.show_alt_view(view, content);
        }
    }

    fn palette_reload(&mut self, force: bool) {
        let Some(state) = self.active_state() else {
            self.status = "No editor open".into();
            return;
        };
        let Some(editor) = state.editor.as_mut() else {
            self.status = "No editor open".into();
            return;
        };
        editor.reload(force);
        let s = std::mem::take(&mut editor.status);
        if !s.is_empty() {
            self.status = s;
        }
    }

    fn should_activate_leader(&self) -> bool {
        if !matches!(self.mode, AppMode::Normal) {
            return false;
        }
        let Some(state) = self.active_state_ref() else {
            return true;
        };
        if matches!(state.view_mode, ViewMode::Terminal | ViewMode::Agents) {
            return false;
        }
        if matches!(state.view_mode, ViewMode::Project) {
            if let Some(model) = state.project_view.as_ref() {
                if let Some(form) = model.feature_form.as_ref() {
                    use crate::views::feature_form::FormFocus;
                    if matches!(
                        form.focus,
                        FormFocus::Title
                            | FormFocus::Step(_)
                            | FormFocus::NewStep
                            | FormFocus::Comment(_)
                            | FormFocus::NewComment
                    ) {
                        return false;
                    }
                    if let Some(e) = form.editor.as_ref() {
                        if matches!(e.mode, EditorMode::Insert | EditorMode::Search) {
                            return false;
                        }
                    }
                }
                if let Some(e) = model.editor.as_ref() {
                    if matches!(e.mode, EditorMode::Insert | EditorMode::Search) {
                        return false;
                    }
                }
            }
        }
        match state.focus {
            Focus::Tree => true,
            Focus::Editor => match state.editor.as_ref().map(|e| e.mode) {
                Some(EditorMode::Insert) | Some(EditorMode::Search) => false,
                _ => true,
            },
        }
    }

    fn copy_current_context(&mut self) {
        if !matches!(self.mode, AppMode::Normal) {
            return;
        }
        let focus = self.active_state_ref().map(|s| s.focus);
        match focus {
            Some(Focus::Editor) => self.copy_from_editor(),
            Some(Focus::Tree) => self.copy_selected_path(),
            None => {}
        }
    }

    fn copy_from_editor(&mut self) {
        let Some(state) = self.active_state() else { return };
        let Some(editor) = state.editor.as_mut() else { return };
        editor.copy_current();
        let status = std::mem::take(&mut editor.status);
        if !status.is_empty() {
            self.status = status;
        }
    }

    fn copy_selected_path(&mut self) {
        let path = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()));
        if let Some(p) = path {
            crate::clipboard::copy(&p.to_string_lossy());
            self.status = format!("Copied path: {}", p.display());
        }
    }

    fn handle_leader_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Char('p') => self.open_picker()?,
            KeyCode::Char('f') => self.open_explorer_filter(),
            KeyCode::Char('g') => self.open_grep(),
            KeyCode::Char('e') => self.focus_tree(),
            KeyCode::Char('b') => self.focus_editor(),
            KeyCode::Char('c') => self.toggle_left_pane(),
            KeyCode::Char('C') => self.start_ai_commit()?,
            KeyCode::Char('w') => self.palette_show_working(),
            KeyCode::Char('h') => self.palette_show_head(),
            KeyCode::Char('d') => self.palette_show_diff(),
            KeyCode::Char('t') => self.open_or_focus_terminal()?,
            KeyCode::Char('T') => self.new_terminal()?,
            KeyCode::Char('P') => self.open_project_view()?,
            KeyCode::Char('G') => self.open_git_view()?,
            KeyCode::Char('H') => self.open_github_view()?,
            KeyCode::Char('r') => self.open_runtime_view()?,
            KeyCode::Char('a') => self.agent_for_selected_feature()?,
            KeyCode::Char('L') => self.toggle_agent_lane(),
            KeyCode::Char('z') => self.cycle_editor_wrap(),
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char(' ') => {}
            _ => {
                self.status = format!("No leader binding: {:?}", key.code);
            }
        }
        Ok(())
    }

    fn focus_tree(&mut self) {
        if let Some(state) = self.active_state() {
            state.focus = Focus::Tree;
        }
    }

    fn toggle_left_pane(&mut self) {
        if let Some(state) = self.active_state() {
            state.left_pane = match state.left_pane {
                LeftPaneMode::Tree => LeftPaneMode::Changes,
                LeftPaneMode::Changes => LeftPaneMode::Tree,
            };
            state.focus = Focus::Tree;
        }
        let _ = self.refresh_git_status();
        let _ = self.preview_selected();
    }

    fn save_settings(&self) -> Result<()> {
        let s = Settings {
            roots: self.roots.clone(),
            search_excludes: self.search_excludes.clone(),
            ai: self.ai_config.clone(),
            shell: self.shell_config.clone(),
        };
        s.save(&self.settings_path)
    }

    fn refresh_git_status(&mut self) -> Result<()> {
        let Some(project) = self.open_projects.get(self.active_index).cloned() else {
            return Ok(());
        };
        let status = git::fetch_status(&project.path);
        let branch = git::current_branch(&project.path);
        if let Some(state) = self.project_views.get_mut(&project.id) {
            state.tree.set_git_status(status.clone());
            state.changes.set_status(&status);
            state.branch = branch;
        }
        Ok(())
    }

    fn focus_editor(&mut self) {
        if let Some(state) = self.active_state() {
            if state.editor.is_some() {
                state.focus = Focus::Editor;
            }
        }
    }

    fn cycle_editor_wrap(&mut self) {
        let label = self.active_state().and_then(|s| {
            s.editor.as_mut().map(|e| {
                e.cycle_wrap();
                e.wrap_mode.label()
            })
        });
        if let Some(label) = label {
            self.status = label.to_string();
        }
    }

    fn on_key_open_confirm(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) | (KeyCode::Enter, _) => {
                let pending = self.pending_open.take();
                self.mode = AppMode::Normal;
                if let Some(p) = pending {
                    self.finalize_open_file(p.path, p.content)?;
                }
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) | (KeyCode::Esc, _) => {
                self.pending_open = None;
                self.mode = AppMode::Normal;
                self.status = "Open cancelled".into();
            }
            _ => {}
        }
        Ok(())
    }

    fn on_key_confirm_delete_feature(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) | (KeyCode::Enter, _) => {
                self.confirm_delete_feature()?;
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) | (KeyCode::Esc, _) => {
                self.cancel_delete_feature();
            }
            _ => {}
        }
        Ok(())
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) -> Result<()> {
        if self.help_visible
            || self.leader_pending
            || !matches!(self.mode, AppMode::Normal)
        {
            return Ok(());
        }
        let col = ev.column;
        let row = ev.row;
        let in_tabs = contains(self.tabs_area, col, row);
        let in_left = contains(self.left_pane_area, col, row);
        let in_right = contains(self.right_pane_area, col, row);
        let in_lane = self.agent_lane_visible && contains(self.agent_lane_area, col, row);

        if self.handle_lane_mouse(ev, col, row) {
            return Ok(());
        }

        if in_lane {
            if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.click_agent_lane(col, row);
            }
            return Ok(());
        }

        if self.handle_split_mouse(ev, col, row) {
            return Ok(());
        }

        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_tabs {
                    if let Some(i) = self.tab_at(col, row) {
                        self.active_index = i;
                        let _ = self.persist_open_projects();
                    }
                } else if contains(self.view_tabs_area, col, row) {
                    for (mode, rect) in self.view_tab_rects.clone() {
                        if contains(rect, col, row) {
                            if let Some(state) = self.active_state() {
                                state.view_mode = mode;
                            }
                            if matches!(mode, ViewMode::Terminal) {
                                let need = self
                                    .active_state_ref()
                                    .map(|s| s.terminals.is_empty())
                                    .unwrap_or(false);
                                if need {
                                    let _ = self.new_terminal();
                                }
                            }
                            if matches!(mode, ViewMode::Agents) {
                                let _ = self.ensure_agents_restored();
                                let need = self
                                    .active_state_ref()
                                    .map(|s| s.agents.is_empty())
                                    .unwrap_or(false);
                                if need {
                                    let _ = self.new_agent();
                                }
                            }
                            return Ok(());
                        }
                    }
                } else if contains(self.agent_tabs_area, col, row)
                    && matches!(self.current_view_mode(), Some(ViewMode::Agents))
                {
                    if let Some(new_rect) = self.agent_new_rect {
                        if contains(new_rect, col, row) {
                            let _ = self.new_agent();
                            return Ok(());
                        }
                    }
                    let hits: Vec<(usize, Rect)> = self
                        .agent_tab_rects
                        .iter()
                        .enumerate()
                        .map(|(i, r)| (i, *r))
                        .collect();
                    for (idx, rect) in hits {
                        if contains(rect, col, row) {
                            if let Some(state) = self.active_state() {
                                state.active_agent = Some(idx);
                                state.view_mode = ViewMode::Agents;
                            }
                            return Ok(());
                        }
                    }
                } else if contains(self.terminal_tabs_area, col, row)
                    && matches!(self.current_view_mode(), Some(ViewMode::Terminal))
                {
                    if let Some(new_rect) = self.terminal_new_rect {
                        if contains(new_rect, col, row) {
                            let _ = self.new_terminal();
                            return Ok(());
                        }
                    }
                    let hits: Vec<(usize, Rect)> = self
                        .terminal_tab_rects
                        .iter()
                        .enumerate()
                        .map(|(i, r)| (i, *r))
                        .collect();
                    for (idx, rect) in hits {
                        if contains(rect, col, row) {
                            if let Some(state) = self.active_state() {
                                state.active_terminal = Some(idx);
                                state.view_mode = ViewMode::Terminal;
                            }
                            return Ok(());
                        }
                    }
                } else if self.try_pty_mouse_press(col, row) {
                    // routed to terminal/agent selection
                } else if in_left {
                    match self.current_view_mode() {
                        Some(ViewMode::Project) => self.click_project_list(col, row),
                        Some(ViewMode::Runtime) => self.click_runtime_list(col, row),
                        _ => self.click_left_pane(col, row)?,
                    }
                } else if in_right {
                    if matches!(self.current_view_mode(), Some(ViewMode::Project)) {
                        self.click_project_right(col, row);
                    } else {
                        self.click_right_pane(col, row);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.try_pty_mouse_drag(col, row) {
                    // routed
                } else if in_right {
                    if matches!(self.current_view_mode(), Some(ViewMode::Project)) {
                        if let Some(model) = self
                            .active_state()
                            .and_then(|s| s.project_view.as_mut())
                        {
                            if let Some(e) = model.editor.as_mut() {
                                e.mouse_drag(col, row);
                            }
                        }
                    } else if let Some(s) = self.active_state() {
                        if let Some(e) = s.editor.as_mut() {
                            e.mouse_drag(col, row);
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.try_pty_mouse_release() {
                    // routed
                } else if in_right {
                    if matches!(self.current_view_mode(), Some(ViewMode::Project)) {
                        let status = {
                            let Some(model) = self
                                .active_state()
                                .and_then(|s| s.project_view.as_mut())
                            else {
                                return Ok(());
                            };
                            let Some(e) = model.editor.as_mut() else { return Ok(()) };
                            e.mouse_release();
                            std::mem::take(&mut e.status)
                        };
                        if !status.is_empty() {
                            self.status = status;
                        }
                    } else if let Some(s) = self.active_state() {
                        if let Some(e) = s.editor.as_mut() {
                            e.mouse_release();
                            let status = std::mem::take(&mut e.status);
                            if !status.is_empty() {
                                self.status = status;
                            }
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => self.scroll_at(col, row, 3),
            MouseEventKind::ScrollUp => self.scroll_at(col, row, -3),
            MouseEventKind::ScrollRight => self.h_scroll_at(col, row, 3),
            MouseEventKind::ScrollLeft => self.h_scroll_at(col, row, -3),
            _ => {}
        }
        Ok(())
    }

    fn handle_lane_mouse(&mut self, ev: MouseEvent, col: u16, row: u16) -> bool {
        if !self.agent_lane_visible || self.agent_lane_area.width == 0 {
            if matches!(ev.kind, MouseEventKind::Up(MouseButton::Left)) && self.lane_dragging {
                self.lane_dragging = false;
                return true;
            }
            return false;
        }
        let lane = self.agent_lane_area;
        let in_rows = row >= lane.y && row < lane.y.saturating_add(lane.height);
        let boundary = lane.x;
        let near = in_rows && col + 1 >= boundary && col <= boundary;
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) if near => {
                self.lane_dragging = true;
                true
            }
            MouseEventKind::Drag(MouseButton::Left) if self.lane_dragging => {
                let max_x = lane.x.saturating_add(lane.width);
                let body_start = self
                    .left_pane_area
                    .x
                    .min(self.right_pane_area.x.min(self.tabs_area.x));
                let min_lane: u16 = 20;
                let max_lane = max_x.saturating_sub(body_start.saturating_add(24));
                let mut new_w = max_x.saturating_sub(col);
                if max_lane >= min_lane {
                    new_w = new_w.clamp(min_lane, max_lane);
                } else {
                    new_w = new_w.max(min_lane);
                }
                self.agent_lane_width = new_w;
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.lane_dragging => {
                self.lane_dragging = false;
                true
            }
            _ => false,
        }
    }

    fn handle_split_mouse(&mut self, ev: MouseEvent, col: u16, row: u16) -> bool {
        let left = self.left_pane_area;
        let right = self.right_pane_area;
        if left.width == 0 || right.width == 0 {
            if matches!(ev.kind, MouseEventKind::Up(MouseButton::Left)) && self.split_dragging {
                self.split_dragging = false;
                return true;
            }
            return false;
        }
        let boundary = left.x.saturating_add(left.width);
        let in_rows = row >= left.y && row < left.y.saturating_add(left.height);
        let near = in_rows && col + 1 >= boundary && col <= boundary;
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) if near => {
                self.split_dragging = true;
                true
            }
            MouseEventKind::Drag(MouseButton::Left) if self.split_dragging => {
                let total = left.width.saturating_add(right.width);
                if total > 0 {
                    let rel = col.saturating_sub(left.x).min(total);
                    let pct = ((rel as u32 * 100) / total as u32).clamp(10, 90) as u16;
                    self.split_pct = pct;
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.split_dragging => {
                self.split_dragging = false;
                true
            }
            _ => false,
        }
    }

    fn tab_at(&self, col: u16, row: u16) -> Option<usize> {
        for (i, r) in self.tab_rects.iter().enumerate() {
            if contains(*r, col, row) {
                return Some(i);
            }
        }
        None
    }

    fn click_left_pane(&mut self, col: u16, row: u16) -> Result<()> {
        let pane = self
            .active_state_ref()
            .map(|s| s.left_pane)
            .unwrap_or(LeftPaneMode::Tree);
        let _ = col;
        let action = if let Some(s) = self.active_state() {
            match pane {
                LeftPaneMode::Tree => match s.tree.mouse_select(row) {
                    FileTreeAction::OpenFile(p) => Some(p),
                    FileTreeAction::None => None,
                },
                LeftPaneMode::Changes => match s.changes.mouse_select(row) {
                    crate::views::changes::ChangesAction::OpenFile(p) => Some(p),
                    crate::views::changes::ChangesAction::None => None,
                },
            }
        } else {
            None
        };
        if let Some(path) = action {
            self.open_file_in_editor(path)?;
        } else {
            self.preview_selected()?;
        }
        if let Some(s) = self.active_state() {
            s.focus = Focus::Tree;
        }
        self.persist_active_tree()?;
        Ok(())
    }

    fn try_pty_mouse_press(&mut self, col: u16, row: u16) -> bool {
        let mode = self.current_view_mode();
        match mode {
            Some(ViewMode::Terminal) => {
                let Some(state) = self.active_state() else { return false };
                let Some(idx) = state.active_terminal else { return false };
                let Some(term) = state.terminals.get_mut(idx) else { return false };
                let Some(area) = term.last_render_area else { return false };
                if !contains(area, col, row) {
                    return false;
                }
                term.mouse_press(area, col, row);
                true
            }
            Some(ViewMode::Agents) => {
                let Some(state) = self.active_state() else { return false };
                let Some(idx) = state.active_agent else { return false };
                let Some(agent) = state.agents.get_mut(idx) else { return false };
                let Some(area) = agent.last_render_area else { return false };
                if !contains(area, col, row) {
                    return false;
                }
                agent.mouse_press(area, col, row);
                true
            }
            _ => false,
        }
    }

    fn try_pty_mouse_drag(&mut self, col: u16, row: u16) -> bool {
        let mode = self.current_view_mode();
        match mode {
            Some(ViewMode::Terminal) => {
                let Some(state) = self.active_state() else { return false };
                let Some(idx) = state.active_terminal else { return false };
                let Some(term) = state.terminals.get_mut(idx) else { return false };
                if term.drag_anchor.is_none() {
                    return false;
                }
                let Some(area) = term.last_render_area else { return false };
                term.mouse_drag(area, col, row);
                true
            }
            Some(ViewMode::Agents) => {
                let Some(state) = self.active_state() else { return false };
                let Some(idx) = state.active_agent else { return false };
                let Some(agent) = state.agents.get_mut(idx) else { return false };
                if agent.drag_anchor.is_none() {
                    return false;
                }
                let Some(area) = agent.last_render_area else { return false };
                agent.mouse_drag(area, col, row);
                true
            }
            _ => false,
        }
    }

    fn try_pty_mouse_release(&mut self) -> bool {
        let mode = self.current_view_mode();
        let text = match mode {
            Some(ViewMode::Terminal) => {
                let Some(state) = self.active_state() else { return false };
                let Some(idx) = state.active_terminal else { return false };
                let Some(term) = state.terminals.get_mut(idx) else { return false };
                if term.drag_anchor.is_none() && term.selection.is_none() {
                    return false;
                }
                term.mouse_release()
            }
            Some(ViewMode::Agents) => {
                let Some(state) = self.active_state() else { return false };
                let Some(idx) = state.active_agent else { return false };
                let Some(agent) = state.agents.get_mut(idx) else { return false };
                if agent.drag_anchor.is_none() && agent.selection.is_none() {
                    return false;
                }
                agent.mouse_release()
            }
            _ => return false,
        };
        if let Some(t) = text {
            let chars = t.chars().count();
            crate::clipboard::copy(&t);
            self.status = format!("Copied {} chars", chars);
        }
        true
    }

    fn click_agent_lane(&mut self, col: u16, row: u16) {
        let hit = self
            .agent_lane_tile_rects
            .iter()
            .find(|(_, _, r)| contains(*r, col, row))
            .map(|(pid, idx, _)| (*pid, *idx));
        let Some((pid, idx)) = hit else { return };
        if let Some(pos) = self.open_projects.iter().position(|p| p.id == pid) {
            self.active_index = pos;
        }
        if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Agents;
            if idx < state.agents.len() {
                state.active_agent = Some(idx);
            }
            if let Some(i) = state.active_agent {
                if let Some(a) = state.agents.get_mut(i) {
                    a.clear_attention();
                }
            }
        }
        let _ = self.persist_open_projects();
    }

    fn click_right_pane(&mut self, col: u16, row: u16) {
        if let Some(s) = self.active_state() {
            s.focus = Focus::Editor;
        }
        let pill_hit = self
            .active_state_ref()
            .and_then(|s| s.editor.as_ref())
            .and_then(|e| {
                if e.pill_working.map(|r| contains(r, col, row)).unwrap_or(false) {
                    return Some(0);
                }
                if e.pill_head.map(|r| contains(r, col, row)).unwrap_or(false) {
                    return Some(1);
                }
                if e.pill_diff.map(|r| contains(r, col, row)).unwrap_or(false) {
                    return Some(2);
                }
                None
            });
        if let Some(idx) = pill_hit {
            match idx {
                0 => self.palette_show_working(),
                1 => self.palette_show_head(),
                2 => self.palette_show_diff(),
                _ => {}
            }
            return;
        }
        if let Some(s) = self.active_state() {
            if let Some(editor) = s.editor.as_mut() {
                editor.mouse_press(col, row);
            }
        }
    }

    fn h_scroll_at(&mut self, col: u16, row: u16, delta: i32) {
        if !contains(self.right_pane_area, col, row) {
            return;
        }
        if let Some(s) = self.active_state() {
            if let Some(e) = s.editor.as_mut() {
                e.mouse_scroll_horizontal(delta);
            }
        }
    }

    fn scroll_at(&mut self, col: u16, row: u16, delta: i32) {
        let view_mode = self.current_view_mode();
        if matches!(view_mode, Some(ViewMode::Agents)) {
            if let Some(s) = self.active_state() {
                if let Some(i) = s.active_agent {
                    if let Some(agent) = s.agents.get_mut(i) {
                        agent.scroll(-delta);
                    }
                }
            }
            return;
        }
        if matches!(view_mode, Some(ViewMode::Terminal)) {
            if let Some(s) = self.active_state() {
                if let Some(i) = s.active_terminal {
                    if let Some(term) = s.terminals.get_mut(i) {
                        term.scroll(-delta);
                    }
                }
            }
            return;
        }
        if matches!(view_mode, Some(ViewMode::Project)) {
            if contains(self.right_pane_area, col, row) {
                if let Some(model) = self
                    .active_state()
                    .and_then(|s| s.project_view.as_mut())
                {
                    if let Some(e) = model.editor.as_mut() {
                        e.mouse_scroll(delta);
                        return;
                    }
                }
            }
            if contains(self.left_pane_area, col, row) {
                if delta > 0 {
                    for _ in 0..delta {
                        self.project_move_down();
                    }
                } else {
                    for _ in 0..(-delta) {
                        self.project_move_up();
                    }
                }
            }
            return;
        }
        if contains(self.right_pane_area, col, row) {
            if let Some(s) = self.active_state() {
                if let Some(e) = s.editor.as_mut() {
                    e.mouse_scroll(delta);
                }
            }
            return;
        }
        if contains(self.left_pane_area, col, row) {
            let pane = self
                .active_state_ref()
                .map(|s| s.left_pane)
                .unwrap_or(LeftPaneMode::Tree);
            if let Some(s) = self.active_state() {
                match pane {
                    LeftPaneMode::Tree => s.tree.mouse_scroll(delta),
                    LeftPaneMode::Changes => s.changes.mouse_scroll(delta),
                }
            }
            let _ = self.preview_selected();
        }
    }

    fn current_view_mode(&self) -> Option<ViewMode> {
        self.active_state_ref().map(|s| s.view_mode)
    }

    fn click_runtime_list(&mut self, col: u16, row: u16) {
        let area = self.runtime_list_area;
        let count = self
            .active_state_ref()
            .and_then(|s| s.runtime.as_ref())
            .map(|r| r.services.len())
            .unwrap_or(0);
        let Some(idx) = crate::views::runtime::service_index_at_row(area, row, count) else {
            return;
        };
        let _ = col;
        self.with_runtime(|r| r.select_index(idx));
    }

    fn click_project_list(&mut self, _col: u16, row: u16) {
        let inner = self.project_list_inner;
        if !contains(inner, _col, row) {
            return;
        }
        let target_idx = (row - inner.y) as usize;
        let (current_sel, rows_total) = {
            let Some(state) = self.active_state() else { return };
            let Some(model) = state.project_view.as_mut() else { return };
            (model.list_state.selected(), model.rows())
        };
        if target_idx >= rows_total {
            return;
        }
        let was_selected = current_sel == Some(target_idx);
        {
            let Some(state) = self.active_state() else { return };
            let Some(model) = state.project_view.as_mut() else { return };
            model.list_state.select(Some(target_idx));
            model.sync_selection_from_list();
            state.focus = Focus::Tree;
        }
        if was_selected {
            self.project_begin_edit();
            if let Some(state) = self.active_state() {
                if state.project_view.as_ref().and_then(|m| m.editor.as_ref()).is_some() {
                    state.focus = Focus::Editor;
                }
            }
        }
    }

    fn click_project_right(&mut self, col: u16, row: u16) {
        if let Some(state) = self.active_state() {
            state.focus = Focus::Editor;
        }
        let status_hit = self
            .feature_form_status_rects
            .iter()
            .any(|(_, r)| contains(*r, col, row));
        if status_hit {
            self.with_feature_form(|f| {
                f.click_focus(crate::views::feature_form::FormFocus::Status);
                f.cycle_status();
            });
            return;
        }
        let field_hit = self
            .feature_form_field_rects
            .iter()
            .find(|(_, r)| contains(*r, col, row))
            .map(|(f, _)| *f);
        if let Some(target) = field_hit {
            self.with_feature_form(|f| f.click_focus(target));
            return;
        }
        if let Some(state) = self.active_state() {
            if let Some(model) = state.project_view.as_mut() {
                if let Some(editor) = model.editor.as_mut() {
                    editor.mouse_press(col, row);
                }
            }
        }
    }

    fn is_help_key(&self, key: KeyEvent) -> bool {
        if key.code != KeyCode::Char('?') || key.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        match self.mode {
            AppMode::Picker => {
                if let Some(p) = &self.picker {
                    if matches!(p.mode, PickerMode::AddProject | PickerMode::AddRoot) {
                        return false;
                    }
                }
                true
            }
            AppMode::Grep
            | AppMode::OpenConfirm
            | AppMode::ConfirmDeleteFeature
            | AppMode::Palette
            | AppMode::ExplorerFilter
            | AppMode::AiCommit
            | AppMode::AgentRename
            | AppMode::WorktreePrompt
            | AppMode::FilePrompt
            | AppMode::ConfirmDeleteFile
            | AppMode::Settings => false,
            AppMode::Normal => {
                if let Some(state) = self.active_state_ref() {
                    if matches!(state.view_mode, ViewMode::Terminal | ViewMode::Agents) {
                        return false;
                    }
                    if state.focus == Focus::Editor {
                        if let Some(e) = &state.editor {
                            if matches!(e.mode, EditorMode::Insert | EditorMode::Search) {
                                return false;
                            }
                        }
                    }
                }
                true
            }
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent) -> Result<()> {
        if self.handle_tab_chord(key) {
            return Ok(());
        }
        if self.handle_context_nav(key)? {
            return Ok(());
        }
        let mode = self
            .active_state_ref()
            .map(|s| s.view_mode)
            .unwrap_or(ViewMode::Editor);
        match mode {
            ViewMode::Terminal => return self.on_key_terminal(key),
            ViewMode::Agents => return self.on_key_agents(key),
            ViewMode::Project => return self.on_key_project_view(key),
            ViewMode::Git => return self.on_key_git_view(key),
            ViewMode::Github => return self.on_key_github_view(key),
            ViewMode::Runtime => return self.on_key_runtime_view(key),
            ViewMode::Editor => {}
        }
        if self.handle_global_normal(key)? {
            return Ok(());
        }
        let focus = self
            .active_state_ref()
            .map(|s| s.focus)
            .unwrap_or(Focus::Tree);
        match focus {
            Focus::Tree => self.on_key_tree(key)?,
            Focus::Editor => self.on_key_editor(key)?,
        }
        Ok(())
    }

    fn cycle_view_mode(&mut self, forward: bool) {
        self.tab_chord = None;
        let order = self.view_mode_order();
        if let Some(state) = self.active_state() {
            let cur = order.iter().position(|m| *m == state.view_mode).unwrap_or(0);
            let next = if forward {
                (cur + 1) % order.len()
            } else {
                (cur + order.len() - 1) % order.len()
            };
            state.view_mode = order[next];
        }
        self.after_view_mode_change();
    }

    pub fn view_mode_order(&mut self) -> Vec<ViewMode> {
        let project_enabled = self
            .active_project()
            .map(|p| self.is_project_view_enabled(p.id))
            .unwrap_or(true);
        let mut order = vec![ViewMode::Editor, ViewMode::Terminal, ViewMode::Agents];
        if project_enabled {
            order.push(ViewMode::Project);
        }
        order.push(ViewMode::Runtime);
        order.push(ViewMode::Git);
        if self.github_available_for_active() {
            order.push(ViewMode::Github);
        }
        order
    }

    pub fn github_available_for_active(&mut self) -> bool {
        let Some(project) = self.active_project().cloned() else {
            return false;
        };
        if let Some(state) = self.project_views.get(&project.id) {
            if let Some(view) = &state.github_view {
                return view.repo_configured;
            }
            if let Some(cached) = state.github_available {
                return cached;
            }
        }
        let available = crate::git::detect_github_url(&project.path).is_some();
        if let Some(state) = self.project_views.get_mut(&project.id) {
            state.github_available = Some(available);
        }
        available
    }

    fn after_view_mode_change(&mut self) {
        let mode = self
            .active_state_ref()
            .map(|s| s.view_mode)
            .unwrap_or(ViewMode::Editor);
        match mode {
            ViewMode::Terminal => {
                let needs_spawn = self
                    .active_state_ref()
                    .map(|s| s.terminals.is_empty())
                    .unwrap_or(false);
                if needs_spawn {
                    let _ = self.new_terminal();
                }
            }
            ViewMode::Agents => {
                let _ = self.ensure_agents_restored();
                let needs_spawn = self
                    .active_state_ref()
                    .map(|s| s.agents.is_empty())
                    .unwrap_or(false);
                if needs_spawn {
                    let _ = self.new_agent();
                }
            }
            ViewMode::Project => {
                self.ensure_project_view_loaded();
            }
            ViewMode::Git => {
                self.ensure_git_view_loaded();
            }
            ViewMode::Github => {
                self.ensure_github_view_loaded();
            }
            ViewMode::Runtime => {
                self.ensure_runtime_loaded();
            }
            ViewMode::Editor => {}
        }
    }

    pub fn ensure_runtime_loaded(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            return;
        };
        let already = self
            .active_state_ref()
            .map(|s| s.runtime.is_some())
            .unwrap_or(false);
        if already {
            return;
        }
        let yaml = self.load_or_bootstrap_runtime_yaml(&project);
        let runtime = crate::runtime::Runtime::new(project.path.clone(), yaml);
        if let Some(state) = self.active_state() {
            state.runtime = Some(runtime);
        }
    }

    fn load_or_bootstrap_runtime_yaml(&mut self, project: &Project) -> String {
        if let Some(cached) = self.runtime_yaml.get(&project.id) {
            return cached.clone();
        }
        let db_yaml = self
            .db
            .load_project_runtime_yaml(project.id)
            .unwrap_or(None);
        if let Some(yaml) = db_yaml {
            self.runtime_yaml.insert(project.id, yaml.clone());
            return yaml;
        }
        let file_path = project.path.join(crate::runtime::RUNTIME_CONFIG_FILE);
        if let Ok(raw) = std::fs::read_to_string(&file_path) {
            if !raw.trim().is_empty() {
                if self
                    .db
                    .save_project_runtime_yaml(project.id, &raw)
                    .is_ok()
                {
                    self.status = format!(
                        "Bootstrapped runtime config from {} → DB",
                        crate::runtime::RUNTIME_CONFIG_FILE
                    );
                }
                self.runtime_yaml.insert(project.id, raw.clone());
                return raw;
            }
        }
        self.runtime_yaml.insert(project.id, String::new());
        String::new()
    }

    pub fn open_runtime_view(&mut self) -> Result<()> {
        if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Runtime;
        }
        self.ensure_runtime_loaded();
        Ok(())
    }

    fn on_key_runtime_view(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.with_runtime(|r| r.move_selection(1)),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.with_runtime(|r| r.move_selection(-1)),
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.with_runtime(|r| r.select_index(0));
            }
            (KeyCode::Char('G'), _) => {
                self.with_runtime(|r| {
                    let n = r.services.len();
                    if n > 0 {
                        r.select_index(n - 1);
                    }
                });
            }
            (KeyCode::Enter, _) | (KeyCode::Char('f'), _) => {
                self.with_runtime(|r| r.toggle_filter_selected());
            }
            (KeyCode::Esc, _) => self.with_runtime(|r| r.clear_filter()),
            (KeyCode::Char('c'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.with_runtime(|r| r.clear_log());
            }
            (KeyCode::Char('e'), _) => {
                self.reapply_runtime_yaml_from_cache();
                self.status = self.runtime_load_status();
            }
            (KeyCode::Char('r'), _) => self.runtime_action(RuntimeAction::Run, true),
            (KeyCode::Char('R'), _) => self.runtime_action(RuntimeAction::Run, false),
            (KeyCode::Char('s'), _) => self.runtime_action(RuntimeAction::Stop, true),
            (KeyCode::Char('S'), _) => self.runtime_action(RuntimeAction::Stop, false),
            (KeyCode::Char('b'), _) => self.runtime_action(RuntimeAction::Build, true),
            (KeyCode::Char('B'), _) => self.runtime_action(RuntimeAction::Build, false),
            (KeyCode::Char('x'), _) => self.runtime_action(RuntimeAction::Restart, true),
            (KeyCode::Char('X'), _) => self.runtime_action(RuntimeAction::Restart, false),
            _ => {}
        }
        Ok(())
    }

    fn runtime_action(&mut self, action: RuntimeAction, selected_only: bool) {
        let target = if selected_only {
            self.active_state()
                .and_then(|s| s.runtime.as_ref())
                .and_then(|r| r.selected_service_name())
        } else {
            None
        };
        let Some(runtime) = self.active_state().and_then(|s| s.runtime.as_mut()) else {
            return;
        };
        let target_ref = target.as_deref();
        match action {
            RuntimeAction::Run => runtime.run(target_ref),
            RuntimeAction::Stop => runtime.stop(target_ref),
            RuntimeAction::Build => runtime.build(target_ref),
            RuntimeAction::Restart => runtime.reload(target_ref),
        }
    }

    fn with_runtime(&mut self, mut f: impl FnMut(&mut crate::runtime::Runtime)) {
        if let Some(runtime) = self.active_state().and_then(|s| s.runtime.as_mut()) {
            f(runtime);
        }
    }

    fn runtime_load_status(&self) -> String {
        let Some(runtime) = self
            .active_state_ref()
            .and_then(|s| s.runtime.as_ref())
        else {
            return "Runtime not loaded".into();
        };
        if let Some(err) = &runtime.last_load_error {
            return format!("Runtime config error: {}", first_line(err));
        }
        format!("Runtime config reloaded ({} services)", runtime.services.len())
    }

    fn palette_runtime_action(&mut self, action: RuntimeAction, arg: &str) {
        let in_runtime = matches!(self.current_view_mode(), Some(ViewMode::Runtime));
        if !in_runtime {
            let _ = self.open_runtime_view();
        }
        let target = arg.trim();
        let target_opt = if target.is_empty() { None } else { Some(target.to_string()) };
        let Some(runtime) = self.active_state().and_then(|s| s.runtime.as_mut()) else {
            self.status = "Runtime view unavailable".into();
            return;
        };
        let target_ref = target_opt.as_deref();
        match action {
            RuntimeAction::Run => runtime.run(target_ref),
            RuntimeAction::Stop => runtime.stop(target_ref),
            RuntimeAction::Build => runtime.build(target_ref),
            RuntimeAction::Restart => runtime.reload(target_ref),
        }
    }

    pub fn ensure_git_view_loaded(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            return;
        };
        let already = self
            .active_state_ref()
            .map(|s| s.git_view.is_some())
            .unwrap_or(false);
        if already {
            return;
        }
        let view = crate::views::git::GitTreeView::new(project.path.clone());
        if let Some(state) = self.active_state() {
            state.git_view = Some(view);
        }
    }

    pub fn ensure_github_view_loaded(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            return;
        };
        let already = self
            .active_state_ref()
            .map(|s| s.github_view.is_some())
            .unwrap_or(false);
        if already {
            return;
        }
        let view = crate::views::github::GithubView::new(project.path.clone());
        if let Some(state) = self.active_state() {
            state.github_view = Some(view);
        }
    }

    pub fn ensure_project_view_loaded(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            return;
        };
        let already = self
            .active_state_ref()
            .map(|s| s.project_view.is_some())
            .unwrap_or(false);
        if already {
            return;
        }
        let meta = self.db.load_project_meta(project.id).unwrap_or_default();
        let features = self.db.list_features(project.id).unwrap_or_default();
        let model = crate::views::project_view::ProjectViewModel::new(meta, features);
        if let Some(state) = self.active_state() {
            state.project_view = Some(model);
        }
    }

    fn handle_global_normal(&mut self, _key: KeyEvent) -> Result<bool> {
        Ok(false)
    }

    pub fn has_subtabs(&self) -> bool {
        matches!(
            self.current_view_mode(),
            Some(ViewMode::Terminal | ViewMode::Agents)
        )
    }

    fn context_cycle(&self, forward: bool) -> FocusContext {
        let mut order: Vec<FocusContext> = vec![FocusContext::ProjectTabs, FocusContext::ViewTabs];
        if self.has_subtabs() {
            order.push(FocusContext::SubTabs);
        }
        order.push(FocusContext::Body);
        let cur = order
            .iter()
            .position(|c| *c == self.focus_context)
            .unwrap_or(order.len() - 1);
        let next = if forward {
            (cur + 1) % order.len()
        } else {
            (cur + order.len() - 1) % order.len()
        };
        order[next]
    }

    fn enter_focus_context(&mut self, ctx: FocusContext) {
        self.focus_context = ctx;
        if matches!(ctx, FocusContext::SubTabs) {
            let mode = self.current_view_mode();
            if matches!(mode, Some(ViewMode::Terminal)) {
                let need = self
                    .active_state_ref()
                    .map(|s| s.terminals.is_empty())
                    .unwrap_or(false);
                if need {
                    let _ = self.new_terminal();
                }
            } else if matches!(mode, Some(ViewMode::Agents)) {
                let _ = self.ensure_agents_restored();
                let need = self
                    .active_state_ref()
                    .map(|s| s.agents.is_empty())
                    .unwrap_or(false);
                if need {
                    let _ = self.new_agent();
                }
            }
        }
    }

    fn handle_tab_chord(&mut self, key: KeyEvent) -> bool {
        self.expire_tab_chord();
        let is_plain_tab = matches!(key.code, KeyCode::Tab)
            && !key.modifiers.intersects(
                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
            );
        if !is_plain_tab {
            self.tab_chord = None;
            return false;
        }
        if !self.tab_chord_eligible() {
            self.tab_chord = None;
            return false;
        }
        if self.tab_chord.take().is_some() {
            self.toggle_nav_mode();
            return true;
        }
        self.tab_chord = Some(Instant::now());
        match self.current_view_mode() {
            Some(ViewMode::Terminal) => self.write_to_active_terminal(&[b'\t']),
            Some(ViewMode::Agents) => self.write_to_active_agent(&[b'\t']),
            _ => {}
        }
        true
    }

    fn expire_tab_chord(&mut self) {
        if let Some(t0) = self.tab_chord {
            if t0.elapsed() >= std::time::Duration::from_millis(400) {
                self.tab_chord = None;
            }
        }
    }

    fn tab_chord_eligible(&self) -> bool {
        if !matches!(self.mode, AppMode::Normal) {
            return false;
        }
        let view = self.current_view_mode().unwrap_or(ViewMode::Editor);
        if matches!(view, ViewMode::Terminal | ViewMode::Agents) {
            return !self.terminal_prefix;
        }
        if matches!(view, ViewMode::Editor) {
            let editing = self
                .active_state_ref()
                .and_then(|s| s.editor.as_ref())
                .map(|e| matches!(e.mode, EditorMode::Insert | EditorMode::Search))
                .unwrap_or(false);
            return !editing;
        }
        if matches!(view, ViewMode::Project) {
            let in_form = self
                .active_state_ref()
                .and_then(|s| s.project_view.as_ref())
                .map(|m| m.editor.is_some() || m.feature_form.is_some())
                .unwrap_or(false);
            return !in_form;
        }
        true
    }

    fn toggle_nav_mode(&mut self) {
        if matches!(self.focus_context, FocusContext::Body) {
            self.enter_focus_context(FocusContext::ViewTabs);
            self.status = "Nav mode — arrows to navigate, Esc to exit".into();
        } else {
            self.focus_context = FocusContext::Body;
            self.status = "Nav mode off".into();
        }
    }

    fn handle_context_nav(&mut self, key: KeyEvent) -> Result<bool> {
        let view = self.current_view_mode().unwrap_or(ViewMode::Editor);
        let body_in_pty = matches!(view, ViewMode::Terminal | ViewMode::Agents);
        let in_editor_typing = if matches!(view, ViewMode::Editor) {
            self.active_state_ref()
                .and_then(|s| s.editor.as_ref())
                .map(|e| matches!(e.mode, EditorMode::Insert | EditorMode::Search))
                .unwrap_or(false)
        } else if matches!(view, ViewMode::Project) {
            self.active_state_ref()
                .and_then(|s| s.project_view.as_ref())
                .map(|m| m.editor.is_some() || m.feature_form.is_some())
                .unwrap_or(false)
        } else {
            false
        };

        let _ = (body_in_pty, in_editor_typing);
        if matches!(self.focus_context, FocusContext::Body) {
            return Ok(false);
        }

        let dir = Self::nav_direction(key);
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.focus_context = FocusContext::Body;
                Ok(true)
            }
            (KeyCode::Enter, _) => {
                self.context_activate();
                self.focus_context = FocusContext::Body;
                Ok(true)
            }
            _ => {
                if let Some(d) = dir {
                    match d {
                        NavDir::Left => self.context_nav_horizontal(false),
                        NavDir::Right => self.context_nav_horizontal(true),
                        NavDir::Up => self.context_nav_vertical(false),
                        NavDir::Down => self.context_nav_vertical(true),
                    }
                }
                Ok(true)
            }
        }
    }

    fn nav_direction(key: KeyEvent) -> Option<NavDir> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left => Some(NavDir::Left),
            KeyCode::Right => Some(NavDir::Right),
            KeyCode::Up => Some(NavDir::Up),
            KeyCode::Down => Some(NavDir::Down),
            KeyCode::Char('h') if ctrl => Some(NavDir::Left),
            KeyCode::Char('l') if ctrl => Some(NavDir::Right),
            KeyCode::Char('k') if ctrl => Some(NavDir::Up),
            KeyCode::Char('j') if ctrl => Some(NavDir::Down),
            _ => None,
        }
    }

    fn context_nav_horizontal(&mut self, forward: bool) {
        match self.focus_context {
            FocusContext::ProjectTabs => {
                if forward {
                    self.next_project();
                } else {
                    self.prev_project();
                }
            }
            FocusContext::ViewTabs => {
                self.cycle_view_mode(forward);
            }
            FocusContext::SubTabs => match self.current_view_mode() {
                Some(ViewMode::Terminal) => self.cycle_terminal(forward),
                Some(ViewMode::Agents) => self.cycle_agent(forward),
                _ => {}
            },
            FocusContext::Body => {}
        }
    }

    fn context_nav_vertical(&mut self, forward: bool) {
        if matches!(self.focus_context, FocusContext::Body) {
            return;
        }
        if forward {
            let next = self.context_cycle(true);
            self.enter_focus_context(next);
        } else {
            let next = self.context_cycle(false);
            self.enter_focus_context(next);
        }
    }

    fn context_activate(&mut self) {
        let mode = self.current_view_mode();
        if matches!(self.focus_context, FocusContext::ViewTabs) {
            if matches!(mode, Some(ViewMode::Terminal)) {
                let need = self
                    .active_state_ref()
                    .map(|s| s.terminals.is_empty())
                    .unwrap_or(false);
                if need {
                    let _ = self.new_terminal();
                }
            }
            if matches!(mode, Some(ViewMode::Agents)) {
                let _ = self.ensure_agents_restored();
                let need = self
                    .active_state_ref()
                    .map(|s| s.agents.is_empty())
                    .unwrap_or(false);
                if need {
                    let _ = self.new_agent();
                }
            }
        }
    }

    fn on_key_tree(&mut self, key: KeyEvent) -> Result<()> {
        let pane = self
            .active_state_ref()
            .map(|s| s.left_pane)
            .unwrap_or(LeftPaneMode::Tree);
        match (key.code, key.modifiers) {
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.left_pane_move_down(pane);
                self.persist_active_tree()?;
                self.preview_selected()?;
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                self.left_pane_move_up(pane);
                self.persist_active_tree()?;
                self.preview_selected()?;
            }
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.left_pane_jump_top(pane);
                self.persist_active_tree()?;
                self.preview_selected()?;
            }
            (KeyCode::Char('G'), _) => {
                self.left_pane_jump_bottom(pane);
                self.persist_active_tree()?;
                self.preview_selected()?;
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) => match pane {
                LeftPaneMode::Tree => {
                    let action = self.active_state().map(|s| s.tree.toggle_or_open());
                    if let Some(FileTreeAction::OpenFile(path)) = action {
                        self.open_file_in_editor(path)?;
                    }
                    self.persist_active_tree()?;
                }
                LeftPaneMode::Changes => {
                    let action = self.active_state().map(|s| s.changes.toggle_or_open());
                    if let Some(crate::views::changes::ChangesAction::OpenFile(p)) = action {
                        self.open_file_in_editor(p)?;
                    }
                }
            },
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    if let Some(s) = self.active_state() {
                        s.tree.collapse_or_parent();
                    }
                    self.persist_active_tree()?;
                }
            }
            (KeyCode::Char('e'), _) => {
                let path = self
                    .active_state_ref()
                    .and_then(|s| s.selected_path().map(|p| p.to_path_buf()));
                if let Some(p) = path {
                    if p.is_file() {
                        self.open_file_in_editor(p)?;
                    }
                }
            }
            (KeyCode::Char('y'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if matches!(pane, LeftPaneMode::Changes) {
                    self.toggle_stage_selected()?;
                }
            }
            (KeyCode::Char('Y'), _) => {
                if matches!(pane, LeftPaneMode::Changes) {
                    self.toggle_stage_all()?;
                }
            }
            (KeyCode::Char('a'), _) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    self.start_new_file();
                }
            }
            (KeyCode::Char('A'), _) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    self.start_new_dir();
                }
            }
            (KeyCode::Char('r'), _) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    self.start_rename_file();
                }
            }
            (KeyCode::Char('d'), _) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    self.start_delete_file();
                }
            }
            (KeyCode::Char('c'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    self.yank_selected_file();
                }
            }
            (KeyCode::Char('p'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if matches!(pane, LeftPaneMode::Tree) {
                    self.paste_yanked_file();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn toggle_stage_selected(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else {
            return Ok(());
        };
        let Some(path) = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()))
        else {
            return Ok(());
        };
        let rel = path
            .strip_prefix(&project.path)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let was_staged = git::has_staged_changes(&project.path, &rel);
        let ok = if was_staged {
            git::unstage(&project.path, &rel)
        } else {
            git::stage(&project.path, &rel)
        };
        if ok {
            self.status = if was_staged {
                format!("Unstaged {}", rel.display())
            } else {
                format!("Staged {}", rel.display())
            };
            self.refresh_git_status()?;
            if let Some(state) = self.active_state() {
                state.changes.select_path_external(&path);
            }
        } else {
            self.status = "git command failed".into();
        }
        Ok(())
    }

    pub fn tick(&mut self) {
        self.poll_ai_commit();
        self.poll_agent_lane();
        self.poll_disk_changes();
        self.poll_runtimes();
        self.sanitize_view_mode();
    }

    fn sanitize_view_mode(&mut self) {
        let Some(project_id) = self.active_project().map(|p| p.id) else { return };
        let valid = self.view_mode_order();
        if let Some(state) = self.project_views.get_mut(&project_id) {
            if !valid.contains(&state.view_mode) {
                state.view_mode = valid.first().copied().unwrap_or(ViewMode::Editor);
            }
        }
    }

    fn poll_runtimes(&mut self) {
        for state in self.project_views.values_mut() {
            if let Some(runtime) = state.runtime.as_mut() {
                runtime.tick();
            }
        }
    }

    fn poll_disk_changes(&mut self) {
        let now = Instant::now();
        let due = self
            .last_disk_poll
            .map(|t| now.duration_since(t).as_millis() >= 1000)
            .unwrap_or(true);
        if !due {
            return;
        }
        self.last_disk_poll = Some(now);
        for state in self.project_views.values_mut() {
            if let Some(editor) = state.editor.as_mut() {
                editor.poll_disk();
            }
            state.tree.refresh();
        }
        let _ = self.refresh_git_status();
    }

    fn poll_agent_lane(&mut self) {
        for state in self.project_views.values_mut() {
            for agent in state.agents.iter_mut() {
                agent.poll_status();
                agent.try_flush_initial_input();
            }
        }
        if matches!(self.current_view_mode(), Some(ViewMode::Agents)) {
            if let Some(state) = self.active_state() {
                if let Some(i) = state.active_agent {
                    if let Some(a) = state.agents.get_mut(i) {
                        a.clear_attention();
                    }
                }
            }
        }
    }

    pub fn start_rename_active_agent(&mut self) {
        let pid = match self.active_project().map(|p| p.id) {
            Some(id) => id,
            None => {
                self.status = "No active project".into();
                return;
            }
        };
        let (idx, current_name) = match self
            .project_views
            .get(&pid)
            .and_then(|s| s.active_agent.and_then(|i| s.agents.get(i).map(|a| (i, a.name.clone()))))
        {
            Some(v) => v,
            None => {
                self.status = "No active agent to rename".into();
                return;
            }
        };
        let cursor = current_name.chars().count();
        self.agent_rename = Some(AgentRenameState {
            project_id: pid,
            agent_idx: idx,
            buffer: current_name,
            cursor,
        });
        self.mode = AppMode::AgentRename;
    }

    fn cancel_agent_rename(&mut self) {
        self.agent_rename = None;
        self.mode = AppMode::Normal;
    }

    fn commit_agent_rename(&mut self) {
        let Some(st) = self.agent_rename.take() else {
            self.mode = AppMode::Normal;
            return;
        };
        self.mode = AppMode::Normal;
        let new_name = st.buffer.trim().to_string();
        if new_name.is_empty() {
            self.status = "Rename cancelled (empty name)".into();
            return;
        }
        if let Some(state) = self.project_views.get_mut(&st.project_id) {
            if let Some(agent) = state.agents.get_mut(st.agent_idx) {
                agent.name = new_name;
            }
        }
    }

    fn on_key_agent_rename(&mut self, key: KeyEvent) -> Result<()> {
        let Some(st) = self.agent_rename.as_mut() else {
            self.mode = AppMode::Normal;
            return Ok(());
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.cancel_agent_rename(),
            (KeyCode::Enter, _) => self.commit_agent_rename(),
            (KeyCode::Backspace, _) => {
                if st.cursor > 0 {
                    let chars: Vec<char> = st.buffer.chars().collect();
                    let new_chars: Vec<char> = chars
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| if i + 1 == st.cursor { None } else { Some(*c) })
                        .collect();
                    st.buffer = new_chars.iter().collect();
                    st.cursor -= 1;
                }
            }
            (KeyCode::Delete, _) => {
                let chars: Vec<char> = st.buffer.chars().collect();
                if st.cursor < chars.len() {
                    let new_chars: Vec<char> = chars
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| if i == st.cursor { None } else { Some(*c) })
                        .collect();
                    st.buffer = new_chars.iter().collect();
                }
            }
            (KeyCode::Left, _) => {
                if st.cursor > 0 {
                    st.cursor -= 1;
                }
            }
            (KeyCode::Right, _) => {
                let len = st.buffer.chars().count();
                if st.cursor < len {
                    st.cursor += 1;
                }
            }
            (KeyCode::Home, _) => st.cursor = 0,
            (KeyCode::End, _) => st.cursor = st.buffer.chars().count(),
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => {
                let mut chars: Vec<char> = st.buffer.chars().collect();
                chars.insert(st.cursor, c);
                st.buffer = chars.iter().collect();
                st.cursor += 1;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn toggle_agent_lane(&mut self) {
        self.agent_lane_visible = !self.agent_lane_visible;
        if !self.agent_lane_visible {
            self.agent_lane_area = Rect::default();
            self.agent_lane_tile_rects.clear();
        }
    }

    fn poll_ai_commit(&mut self) {
        let Some(overlay) = self.ai_commit.as_mut() else { return };
        let new_state = if let AiCommitState::Loading { rx, spinner } = &mut overlay.state {
            *spinner = spinner.wrapping_add(1);
            match rx.try_recv() {
                Ok(Ok(AiResult::Single(msg))) => Some(build_review_state(msg)),
                Ok(Ok(AiResult::Plan(plans))) => Some(build_plan_state(plans)),
                Ok(Err(e)) => Some(AiCommitState::Error(e)),
                Err(_) => None,
            }
        } else {
            None
        };
        if let Some(s) = new_state {
            overlay.state = s;
        }
    }

    fn start_ai_commit(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else {
            self.status = "No active project".into();
            return Ok(());
        };
        let staged = git::staged_diff(&project.path).unwrap_or_default();
        let ai_config = self.effective_ai_config(project.id);
        let provider = match crate::ai::build_provider(&ai_config) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("AI provider error: {}", e);
                return Ok(());
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        if !staged.trim().is_empty() {
            std::thread::spawn(move || {
                let result = provider
                    .generate_commit_message(&staged)
                    .map(AiResult::Single)
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
            });
        } else {
            let working = git::working_diff(&project.path).unwrap_or_default();
            let untracked = git::untracked_files(&project.path);
            if working.trim().is_empty() && untracked.is_empty() {
                self.status = "Nothing to commit".into();
                return Ok(());
            }
            std::thread::spawn(move || {
                let result = provider
                    .generate_commit_plan(&working, &untracked)
                    .map(AiResult::Plan)
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
            });
        }
        self.ai_commit = Some(AiCommitOverlay {
            state: AiCommitState::Loading { rx, spinner: 0 },
            project_path: project.path,
        });
        self.mode = AppMode::AiCommit;
        Ok(())
    }

    fn on_key_ai_commit(&mut self, key: KeyEvent) -> Result<()> {
        let Some(overlay) = self.ai_commit.as_mut() else {
            self.mode = AppMode::Normal;
            return Ok(());
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match &mut overlay.state {
            AiCommitState::Loading { .. } => {
                if key.code == KeyCode::Esc {
                    self.cancel_ai_commit();
                }
            }
            AiCommitState::Error(e) => match key.code {
                KeyCode::Char('y') => {
                    crate::clipboard::copy(e);
                    self.status = "Error copied to clipboard".into();
                }
                KeyCode::Char('r') => {
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    self.start_ai_commit()?;
                }
                KeyCode::Esc => self.cancel_ai_commit(),
                _ => {}
            },
            AiCommitState::Reviewing { editor } => {
                if ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Enter) {
                    let message = editor_text(editor);
                    let project_path = overlay.project_path.clone();
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    self.commit_message(&project_path, &message);
                    return Ok(());
                }
                if ctrl && matches!(key.code, KeyCode::Char('r')) {
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    self.start_ai_commit()?;
                    return Ok(());
                }
                if key.code == KeyCode::Esc && editor.mode == EditorMode::Normal {
                    self.cancel_ai_commit();
                    return Ok(());
                }
                editor.handle_key(key);
                let status = std::mem::take(&mut editor.status);
                if !status.is_empty() {
                    self.status = status;
                }
                editor.did_save = false;
                editor.request_focus_tree = false;
            }
            AiCommitState::ReviewingPlan {
                messages,
                files,
                current,
            } => {
                if ctrl && matches!(key.code, KeyCode::Char('n')) {
                    *current = (*current + 1).min(messages.len().saturating_sub(1));
                    return Ok(());
                }
                if ctrl && matches!(key.code, KeyCode::Char('p')) {
                    *current = current.saturating_sub(1);
                    return Ok(());
                }
                if ctrl && matches!(key.code, KeyCode::Char('r')) {
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    self.start_ai_commit()?;
                    return Ok(());
                }
                if ctrl && matches!(key.code, KeyCode::Char('s')) {
                    let plan_messages: Vec<String> =
                        messages.iter().map(editor_text).collect();
                    let plan_files = files.clone();
                    let project_path = overlay.project_path.clone();
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    self.execute_commit_plan(&project_path, &plan_messages, &plan_files);
                    return Ok(());
                }
                let cur = *current;
                let editor = match messages.get_mut(cur) {
                    Some(e) => e,
                    None => return Ok(()),
                };
                if key.code == KeyCode::Esc && editor.mode == EditorMode::Normal {
                    self.cancel_ai_commit();
                    return Ok(());
                }
                editor.handle_key(key);
                let status = std::mem::take(&mut editor.status);
                if !status.is_empty() {
                    self.status = status;
                }
                editor.did_save = false;
                editor.request_focus_tree = false;
            }
        }
        Ok(())
    }

    fn execute_commit_plan(
        &mut self,
        project_path: &PathBuf,
        messages: &[String],
        files: &[Vec<String>],
    ) {
        let mut total = 0usize;
        for (i, (msg, files)) in messages.iter().zip(files.iter()).enumerate() {
            let trimmed = msg.trim();
            if trimmed.is_empty() {
                self.status = format!("Commit {}: empty message — aborted", i + 1);
                let _ = self.refresh_git_status();
                return;
            }
            for f in files {
                let rel = std::path::PathBuf::from(f);
                if !git::stage(project_path, &rel) {
                    self.status = format!("git add {} failed at commit {}", f, i + 1);
                    let _ = self.refresh_git_status();
                    return;
                }
            }
            match git::commit_with_message(project_path, trimmed) {
                Ok(_) => total += 1,
                Err(e) => {
                    self.status = format!(
                        "commit {} failed: {}",
                        i + 1,
                        e.lines().next().unwrap_or("")
                    );
                    let _ = self.refresh_git_status();
                    return;
                }
            }
        }
        self.status = format!("Created {} commits", total);
        let _ = self.refresh_git_status();
    }

    fn cancel_ai_commit(&mut self) {
        self.ai_commit = None;
        self.mode = AppMode::Normal;
        self.status = "AI commit cancelled".into();
    }

    fn commit_message(&mut self, project_path: &PathBuf, message: &str) {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            self.status = "Empty commit message".into();
            return;
        }
        match git::commit_with_message(project_path, trimmed) {
            Ok(_) => {
                let first = trimmed.lines().next().unwrap_or("").to_string();
                self.status = format!("Committed: {}", first);
                let _ = self.refresh_git_status();
            }
            Err(e) => {
                self.status =
                    format!("git commit failed: {}", e.lines().next().unwrap_or(""));
            }
        }
    }

    fn toggle_stage_all(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else {
            return Ok(());
        };
        let any_staged = git::any_staged_changes(&project.path);
        let ok = if any_staged {
            git::unstage_all(&project.path)
        } else {
            git::stage_all(&project.path)
        };
        if ok {
            self.status = if any_staged {
                "Unstaged all".into()
            } else {
                "Staged all".into()
            };
            self.refresh_git_status()?;
        } else {
            self.status = "git command failed".into();
        }
        Ok(())
    }

    fn left_pane_move_down(&mut self, pane: LeftPaneMode) {
        if let Some(s) = self.active_state() {
            match pane {
                LeftPaneMode::Tree => s.tree.move_down(),
                LeftPaneMode::Changes => s.changes.move_down(),
            }
        }
    }

    fn left_pane_move_up(&mut self, pane: LeftPaneMode) {
        if let Some(s) = self.active_state() {
            match pane {
                LeftPaneMode::Tree => s.tree.move_up(),
                LeftPaneMode::Changes => s.changes.move_up(),
            }
        }
    }

    fn left_pane_jump_top(&mut self, pane: LeftPaneMode) {
        if let Some(s) = self.active_state() {
            match pane {
                LeftPaneMode::Tree => s.tree.jump_top(),
                LeftPaneMode::Changes => s.changes.jump_top(),
            }
        }
    }

    fn left_pane_jump_bottom(&mut self, pane: LeftPaneMode) {
        if let Some(s) = self.active_state() {
            match pane {
                LeftPaneMode::Tree => s.tree.jump_bottom(),
                LeftPaneMode::Changes => s.changes.jump_bottom(),
            }
        }
    }

    fn on_key_editor(&mut self, key: KeyEvent) -> Result<()> {
        let Some(state) = self.active_state() else {
            return Ok(());
        };
        let Some(editor) = state.editor.as_mut() else {
            state.focus = Focus::Tree;
            return Ok(());
        };
        editor.handle_key(key);
        let did_save = std::mem::replace(&mut editor.did_save, false);
        let request_focus_tree = std::mem::replace(&mut editor.request_focus_tree, false);
        let saved_path = if did_save { Some(editor.path.clone()) } else { None };
        let status = std::mem::take(&mut editor.status);
        if !status.is_empty() {
            self.status = status;
        }
        if let Some(path) = saved_path {
            if path == self.settings_path {
                self.reload_settings_from_disk();
            } else {
                let _ = self.refresh_git_status();
            }
        }
        if request_focus_tree {
            self.focus_tree();
        }
        Ok(())
    }

    fn open_settings_in_editor(&mut self) -> Result<()> {
        self.open_settings_overlay()
    }

    fn open_settings_overlay(&mut self) -> Result<()> {
        let global_content = std::fs::read_to_string(&self.settings_path).unwrap_or_default();
        let mut global = EditorView::from_content(self.settings_path.clone(), global_content)?;
        global.focused = true;

        let (project_id, project_path) = match self.active_project() {
            Some(p) => (Some(p.id), Some(p.path.clone())),
            None => (None, None),
        };
        let project_yaml = project_id
            .and_then(|id| self.project_settings_yaml.get(&id).cloned())
            .unwrap_or_default();
        let initial = if project_yaml.trim().is_empty() {
            ProjectSettings::empty_template()
        } else {
            project_yaml
        };
        let virtual_path = project_path
            .clone()
            .map(|p| p.join(config::PROJECT_SETTINGS_FILE))
            .unwrap_or_else(|| PathBuf::from(config::PROJECT_SETTINGS_FILE));
        let mut project = EditorView::from_content(virtual_path, initial)?;
        project.focused = false;

        self.settings_overlay = Some(SettingsOverlay {
            global,
            project,
            focus: SettingsPane::Global,
            project_id,
            project_path,
            status: String::new(),
        });
        self.mode = AppMode::Settings;
        Ok(())
    }

    fn close_settings_overlay(&mut self) {
        self.settings_overlay = None;
        self.mode = AppMode::Normal;
    }

    fn on_key_settings(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                let modified = self
                    .settings_overlay
                    .as_ref()
                    .map(|o| o.global.modified || o.project.modified)
                    .unwrap_or(false);
                if modified {
                    if let Some(overlay) = self.settings_overlay.as_mut() {
                        overlay.status =
                            "Unsaved changes — press Esc again to discard".into();
                        overlay.global.modified = false;
                        overlay.project.modified = false;
                    }
                } else {
                    self.close_settings_overlay();
                }
            }
            (KeyCode::Tab, m) | (KeyCode::BackTab, m)
                if !m.contains(KeyModifiers::CONTROL) =>
            {
                if let Some(overlay) = self.settings_overlay.as_mut() {
                    overlay.focus = match overlay.focus {
                        SettingsPane::Global => SettingsPane::Project,
                        SettingsPane::Project => SettingsPane::Global,
                    };
                    overlay.global.focused = matches!(overlay.focus, SettingsPane::Global);
                    overlay.project.focused = matches!(overlay.focus, SettingsPane::Project);
                    overlay.status.clear();
                }
            }
            (KeyCode::Char('s'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.save_settings_focused()?;
            }
            (KeyCode::Char('i'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.import_project_settings_from_file();
            }
            (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.export_project_settings_to_file();
            }
            _ => {
                if let Some(overlay) = self.settings_overlay.as_mut() {
                    match overlay.focus {
                        SettingsPane::Global => overlay.global.handle_key(key),
                        SettingsPane::Project => overlay.project.handle_key(key),
                    }
                    overlay.status.clear();
                }
            }
        }
        Ok(())
    }

    fn save_settings_focused(&mut self) -> Result<()> {
        let Some(overlay) = self.settings_overlay.as_mut() else {
            return Ok(());
        };
        match overlay.focus {
            SettingsPane::Global => {
                if let Err(e) = overlay.global.save() {
                    overlay.status = format!("Save error: {}", e);
                    return Ok(());
                }
                overlay.status = "Global settings saved".into();
                self.reload_settings_from_disk();
            }
            SettingsPane::Project => {
                let Some(project_id) = overlay.project_id else {
                    overlay.status = "No active project".into();
                    return Ok(());
                };
                let body: String = overlay
                    .project
                    .lines
                    .iter()
                    .map(|l| l.iter().collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Err(e) = ProjectSettings::from_yaml(&body) {
                    overlay.status = format!("Invalid YAML: {}", e);
                    return Ok(());
                }
                if let Err(e) = self.db.save_project_settings_yaml(project_id, &body) {
                    if let Some(o) = self.settings_overlay.as_mut() {
                        o.status = format!("DB error: {}", e);
                    }
                    return Ok(());
                }
                self.project_settings_yaml.insert(project_id, body);
                if let Some(o) = self.settings_overlay.as_mut() {
                    o.project.modified = false;
                    o.status = "Project settings saved to DB".into();
                }
            }
        }
        Ok(())
    }

    fn import_project_settings_from_file(&mut self) {
        let Some(overlay) = self.settings_overlay.as_mut() else { return };
        let Some(project_path) = overlay.project_path.clone() else {
            overlay.status = "No active project".into();
            return;
        };
        let Some(project_id) = overlay.project_id else {
            overlay.status = "No active project".into();
            return;
        };
        let path = project_path.join(config::PROJECT_SETTINGS_FILE);
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                overlay.status = format!("Read {}: {}", path.display(), e);
                return;
            }
        };
        if let Err(e) = ProjectSettings::from_yaml(&raw) {
            overlay.status = format!("Invalid YAML in {}: {}", path.display(), e);
            return;
        }
        if let Err(e) = self.db.save_project_settings_yaml(project_id, &raw) {
            if let Some(o) = self.settings_overlay.as_mut() {
                o.status = format!("DB error: {}", e);
            }
            return;
        }
        self.project_settings_yaml.insert(project_id, raw.clone());
        if let Some(o) = self.settings_overlay.as_mut() {
            match EditorView::from_content(o.project.path.clone(), raw) {
                Ok(mut view) => {
                    view.focused = matches!(o.focus, SettingsPane::Project);
                    o.project = view;
                }
                Err(e) => {
                    o.status = format!("Reload error: {}", e);
                    return;
                }
            }
            o.status = format!("Imported {} → DB", config::PROJECT_SETTINGS_FILE);
        }
    }

    fn palette_import_project_settings(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            self.status = "No active project".into();
            return;
        };
        let path = project.path.join(config::PROJECT_SETTINGS_FILE);
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Read {}: {}", path.display(), e);
                return;
            }
        };
        if let Err(e) = ProjectSettings::from_yaml(&raw) {
            self.status = format!("Invalid YAML in {}: {}", path.display(), e);
            return;
        }
        if let Err(e) = self.db.save_project_settings_yaml(project.id, &raw) {
            self.status = format!("DB error: {}", e);
            return;
        }
        self.project_settings_yaml.insert(project.id, raw);
        self.status = format!("Imported {} → DB", path.display());
    }

    fn palette_export_project_settings(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            self.status = "No active project".into();
            return;
        };
        let body = self
            .project_settings_yaml
            .get(&project.id)
            .cloned()
            .unwrap_or_default();
        if body.trim().is_empty() {
            self.status = "Project settings empty — nothing to export".into();
            return;
        }
        if let Err(e) = ProjectSettings::from_yaml(&body) {
            self.status = format!("Invalid project settings in DB: {}", e);
            return;
        }
        let path = project.path.join(config::PROJECT_SETTINGS_FILE);
        if let Err(e) = std::fs::write(&path, &body) {
            self.status = format!("Write {}: {}", path.display(), e);
            return;
        }
        self.status = format!("Exported DB → {}", path.display());
    }

    fn palette_import_runtime(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            self.status = "No active project".into();
            return;
        };
        let path = project.path.join(crate::runtime::RUNTIME_CONFIG_FILE);
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Read {}: {}", path.display(), e);
                return;
            }
        };
        if let Err(e) = crate::runtime::parse_runtime_yaml(&raw) {
            self.status = format!("Invalid YAML in {}: {}", path.display(), e);
            return;
        }
        if let Err(e) = self.db.save_project_runtime_yaml(project.id, &raw) {
            self.status = format!("DB error: {}", e);
            return;
        }
        self.runtime_yaml.insert(project.id, raw.clone());
        self.reapply_runtime_yaml_from_cache();
        self.status = format!("Imported {} → DB", path.display());
    }

    fn palette_export_runtime(&mut self) {
        let Some(project) = self.active_project().cloned() else {
            self.status = "No active project".into();
            return;
        };
        let body = self
            .runtime_yaml
            .get(&project.id)
            .cloned()
            .unwrap_or_default();
        if body.trim().is_empty() {
            self.status = "Runtime config empty — nothing to export".into();
            return;
        }
        let path = project.path.join(crate::runtime::RUNTIME_CONFIG_FILE);
        if let Err(e) = std::fs::write(&path, &body) {
            self.status = format!("Write {}: {}", path.display(), e);
            return;
        }
        self.status = format!("Exported DB → {}", path.display());
    }

    fn reapply_runtime_yaml_from_cache(&mut self) {
        let Some(project_id) = self.active_project().map(|p| p.id) else { return };
        let yaml = self
            .runtime_yaml
            .get(&project_id)
            .cloned()
            .unwrap_or_default();
        if let Some(state) = self.active_state() {
            if let Some(rt) = state.runtime.as_mut() {
                rt.apply_yaml(yaml);
            }
        }
    }

    fn export_project_settings_to_file(&mut self) {
        let Some(overlay) = self.settings_overlay.as_mut() else { return };
        let Some(project_path) = overlay.project_path.clone() else {
            overlay.status = "No active project".into();
            return;
        };
        let body: String = overlay
            .project
            .lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(e) = ProjectSettings::from_yaml(&body) {
            overlay.status = format!("Invalid YAML: {}", e);
            return;
        }
        let path = project_path.join(config::PROJECT_SETTINGS_FILE);
        if let Err(e) = std::fs::write(&path, &body) {
            overlay.status = format!("Write {}: {}", path.display(), e);
            return;
        }
        overlay.status = format!("Exported DB → {}", path.display());
    }

    fn reload_settings_from_disk(&mut self) {
        match Settings::load_or_seed(&self.settings_path) {
            Ok(s) => {
                self.roots = s.roots;
                self.search_excludes = s.search_excludes;
                self.ai_config = s.ai;
                self.shell_config = s.shell;
                self.status = "Settings reloaded".into();
            }
            Err(e) => {
                self.status = format!("Settings reload failed: {}", e);
            }
        }
    }

    fn global_settings_snapshot(&self) -> Settings {
        Settings {
            roots: self.roots.clone(),
            search_excludes: self.search_excludes.clone(),
            ai: self.ai_config.clone(),
            shell: self.shell_config.clone(),
        }
    }

    pub fn effective_settings_for(&self, project_id: i64) -> Settings {
        let global = self.global_settings_snapshot();
        let Some(raw) = self.project_settings_yaml.get(&project_id) else {
            return global;
        };
        match ProjectSettings::from_yaml(raw) {
            Ok(overrides) => global.with_project_overrides(&overrides),
            Err(_) => global,
        }
    }

    pub fn effective_ai_config(&self, project_id: i64) -> crate::config::AiConfig {
        self.effective_settings_for(project_id).ai
    }

    pub fn effective_shell_config(&self, project_id: i64) -> crate::config::ShellConfig {
        self.effective_settings_for(project_id).shell
    }

    pub fn effective_search_excludes(&self, project_id: i64) -> Vec<String> {
        self.effective_settings_for(project_id).search_excludes
    }

    pub fn is_project_view_enabled(&self, project_id: i64) -> bool {
        let Some(raw) = self.project_settings_yaml.get(&project_id) else {
            return true;
        };
        let Ok(overrides) = ProjectSettings::from_yaml(raw) else {
            return true;
        };
        overrides
            .views
            .and_then(|v| v.project)
            .unwrap_or(true)
    }

    fn open_file_in_editor(&mut self, path: PathBuf) -> Result<()> {
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Could not open {}: {}", path.display(), e);
                return Ok(());
            }
        };
        let line_count = content.bytes().filter(|&b| b == b'\n').count() + 1;
        if line_count > config::LARGE_FILE_LINE_THRESHOLD {
            self.pending_open = Some(PendingOpen {
                path,
                content,
                line_count,
            });
            self.mode = AppMode::OpenConfirm;
            return Ok(());
        }
        self.finalize_open_file(path, content)
    }

    fn finalize_open_file(&mut self, path: PathBuf, content: String) -> Result<()> {
        match EditorView::from_content(path.clone(), content) {
            Ok(view) => {
                if let Some(state) = self.active_state() {
                    state.set_editor(view);
                    state.focus = Focus::Editor;
                }
                self.status = format!("Opened {}", path.display());
                self.apply_preferred_view_in_changes();
            }
            Err(e) => {
                self.status = format!("Could not open {}: {}", path.display(), e);
            }
        }
        Ok(())
    }

    fn preview_selected(&mut self) -> Result<()> {
        let path = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()));
        if let Some(p) = path {
            if p.is_file() {
                self.try_preview_file(p);
            }
        }
        Ok(())
    }

    fn try_preview_file(&mut self, path: PathBuf) {
        let already_open = self
            .active_state_ref()
            .and_then(|s| s.editor.as_ref())
            .map(|e| e.path == path)
            .unwrap_or(false);
        if already_open {
            return;
        }
        let modified = self
            .active_state_ref()
            .and_then(|s| s.editor.as_ref())
            .map(|e| e.modified)
            .unwrap_or(false);
        if modified {
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => return,
        };
        if bytes.iter().take(8192).any(|&b| b == 0) {
            return;
        }
        let text = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return,
        };
        let line_count = text.bytes().filter(|&b| b == b'\n').count() + 1;
        if line_count > config::LARGE_FILE_LINE_THRESHOLD {
            return;
        }
        if let Ok(view) = EditorView::from_content(path, text) {
            if let Some(state) = self.active_state() {
                state.set_editor(view);
            }
            self.apply_preferred_view_in_changes();
        }
    }


    fn on_key_picker(&mut self, key: KeyEvent) -> Result<()> {
        let Some(mode) = self.picker.as_ref().map(|p| p.mode) else {
            return Ok(());
        };
        match mode {
            PickerMode::Browse => self.handle_browse(key),
            PickerMode::AddProject => self.handle_add_project(key),
            PickerMode::Roots => self.handle_roots(key),
            PickerMode::AddRoot => self.handle_add_root(key),
        }
    }

    fn handle_browse(&mut self, key: KeyEvent) -> Result<()> {
        let mut consumed = true;
        {
            let picker = self.picker.as_mut().expect("picker present");
            match (key.code, key.modifiers) {
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => picker.move_down(),
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => picker.move_up(),
                (KeyCode::Char('n'), _) => picker.begin_add_project(),
                (KeyCode::Char('r'), _) => picker.open_roots(),
                _ => consumed = false,
            }
        }
        if consumed {
            return Ok(());
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                if !self.open_projects.is_empty() {
                    self.mode = AppMode::Normal;
                    self.picker = None;
                }
            }
            (KeyCode::Char('s'), _) => self.rebuild_picker_data()?,
            (KeyCode::Char('d'), _) => self.delete_selected_saved()?,
            (KeyCode::Enter, _) => self.enter_selected_browse()?,
            _ => {}
        }
        Ok(())
    }

    fn handle_add_project(&mut self, key: KeyEvent) -> Result<()> {
        let mut consumed = true;
        {
            let picker = self.picker.as_mut().expect("picker present");
            match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => picker.cancel_input(),
                (KeyCode::Backspace, _) => picker.pop_char(),
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => picker.push_char(c),
                _ => consumed = false,
            }
        }
        if consumed {
            return Ok(());
        }
        if let (KeyCode::Enter, _) = (key.code, key.modifiers) {
            let confirmed = self.picker.as_mut().and_then(|p| p.confirm_add_project());
            if let Some((name, path)) = confirmed {
                self.add_and_open_project(name, path)?;
            }
        }
        Ok(())
    }

    fn handle_roots(&mut self, key: KeyEvent) -> Result<()> {
        let mut consumed = true;
        {
            let picker = self.picker.as_mut().expect("picker present");
            match (key.code, key.modifiers) {
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => picker.move_root_down(),
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => picker.move_root_up(),
                (KeyCode::Char('n'), _) => picker.begin_add_root(),
                _ => consumed = false,
            }
        }
        if consumed {
            return Ok(());
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                if let Some(p) = self.picker.as_mut() {
                    p.open_browse();
                }
                self.rebuild_picker_data()?;
            }
            (KeyCode::Char('d'), _) => {
                let target = self
                    .picker
                    .as_ref()
                    .and_then(|p| p.selected_root().cloned());
                if let Some(root) = target {
                    self.roots.retain(|r| r != &root);
                    self.save_settings()?;
                    if let Some(p) = self.picker.as_mut() {
                        p.set_roots(self.roots.clone());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_add_root(&mut self, key: KeyEvent) -> Result<()> {
        let mut consumed = true;
        {
            let picker = self.picker.as_mut().expect("picker present");
            match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => picker.cancel_input(),
                (KeyCode::Backspace, _) => picker.pop_char(),
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => picker.push_char(c),
                _ => consumed = false,
            }
        }
        if consumed {
            return Ok(());
        }
        if let (KeyCode::Enter, _) = (key.code, key.modifiers) {
            let confirmed = self.picker.as_mut().and_then(|p| p.confirm_add_root());
            if let Some(path) = confirmed {
                if !self.roots.contains(&path) {
                    self.roots.push(path);
                    self.save_settings()?;
                }
                if let Some(p) = self.picker.as_mut() {
                    p.set_roots(self.roots.clone());
                    p.open_roots();
                }
            }
        }
        Ok(())
    }

    fn enter_selected_browse(&mut self) -> Result<()> {
        let action = self
            .picker
            .as_ref()
            .and_then(|p| p.selected_item())
            .map(|item| match item {
                PickerItem::Saved(p) => BrowseEnter::OpenSaved(p.clone()),
                PickerItem::Discovered { name, path } => BrowseEnter::AddDiscovered {
                    name: name.clone(),
                    path: path.clone(),
                },
                PickerItem::Header(_) => BrowseEnter::None,
            });
        match action {
            Some(BrowseEnter::OpenSaved(p)) => self.open_project(p),
            Some(BrowseEnter::AddDiscovered { name, path }) => self.add_and_open_project(name, path),
            _ => Ok(()),
        }
    }

    fn delete_selected_saved(&mut self) -> Result<()> {
        let id = self
            .picker
            .as_ref()
            .and_then(|p| p.selected_item())
            .and_then(|it| match it {
                PickerItem::Saved(p) => Some(p.id),
                _ => None,
            });
        let Some(id) = id else { return Ok(()) };
        self.db.delete_project(id)?;
        self.open_projects.retain(|op| op.id != id);
        self.project_views.remove(&id);
        if self.active_index >= self.open_projects.len() && !self.open_projects.is_empty() {
            self.active_index = self.open_projects.len() - 1;
        }
        self.persist_open_projects()?;
        self.rebuild_picker_data()?;
        Ok(())
    }

    fn add_and_open_project(&mut self, name: String, path: PathBuf) -> Result<()> {
        let github_url = crate::git::detect_github_url(&path);
        let id = self
            .db
            .upsert_project(&name, &path, github_url.as_deref())?;
        self.status = match &github_url {
            Some(url) => format!("Added {} (GitHub: {})", name, url),
            None => format!("Added {} (no GitHub remote)", name),
        };
        let project = Project {
            id,
            name,
            path,
            github_url,
        };
        self.open_project(project)
    }

    pub fn open_worktree_path(&mut self, path: &Path) -> Result<()> {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self
            .open_projects
            .iter()
            .position(|p| paths_equal(&p.path, &canon))
        {
            self.active_index = i;
            self.persist_open_projects()?;
            self.status = format!("Switched to worktree {}", canon.display());
            return Ok(());
        }
        let name = canon
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("worktree")
            .to_string();
        self.add_and_open_project(name, canon)
    }

    fn rebuild_picker_data(&mut self) -> Result<()> {
        let saved = self.db.list_projects()?;
        let discovered = discover_new(&self.roots, &saved);
        if let Some(picker) = self.picker.as_mut() {
            picker.refresh(saved, discovered);
            picker.set_roots(self.roots.clone());
        }
        Ok(())
    }

    fn on_key_explorer_filter(&mut self, key: KeyEvent) -> Result<()> {
        let Some(state) = self.active_state() else {
            self.mode = AppMode::Normal;
            return Ok(());
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                state.tree.clear_filter();
                self.mode = AppMode::Normal;
            }
            (KeyCode::Enter, _) => {
                let path = state.tree.selected_path().map(|p| p.to_path_buf());
                self.mode = AppMode::Normal;
                if let Some(p) = path {
                    if p.is_file() {
                        self.open_file_in_editor(p)?;
                    }
                }
            }
            (KeyCode::Backspace, _) => {
                let mut q = state.tree.filter.clone();
                if q.pop().is_some() {
                    state.tree.set_filter(q);
                } else {
                    state.tree.clear_filter();
                    self.mode = AppMode::Normal;
                }
            }
            (KeyCode::Up, _) => state.tree.move_up(),
            (KeyCode::Down, _) => state.tree.move_down(),
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => {
                let mut q = state.tree.filter.clone();
                q.push(c);
                state.tree.set_filter(q);
            }
            _ => {}
        }
        Ok(())
    }

    fn on_key_grep(&mut self, key: KeyEvent) -> Result<()> {
        let mut consumed = true;
        {
            let Some(grep) = self.grep.as_mut() else {
                return Ok(());
            };
            match (key.code, key.modifiers) {
                (KeyCode::Down, _) => grep.move_down(),
                (KeyCode::Up, _) => grep.move_up(),
                (KeyCode::Backspace, _) => grep.pop_char(),
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => grep.push_char(c),
                _ => consumed = false,
            }
        }
        if consumed {
            return Ok(());
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.grep = None;
                self.mode = AppMode::Normal;
            }
            (KeyCode::Enter, _) => {
                let hit = self.grep.as_ref().and_then(|g| g.selected_hit().cloned());
                if let Some(hit) = hit {
                    self.grep = None;
                    self.mode = AppMode::Normal;
                    self.open_file_in_editor(hit.path.clone())?;
                    if let Some(state) = self.active_state() {
                        if let Some(editor) = state.editor.as_mut() {
                            editor.cursor = (hit.row, hit.col);
                            editor.preferred_col = hit.col;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn open_project(&mut self, mut project: Project) -> Result<()> {
        self.db.touch_project(project.id)?;
        if project.github_url.is_none() {
            if let Some(url) = crate::git::detect_github_url(&project.path) {
                self.db.upsert_project(&project.name, &project.path, Some(&url))?;
                project.github_url = Some(url);
            }
        }
        if let Some(i) = self.open_projects.iter().position(|p| p.id == project.id) {
            self.active_index = i;
            self.open_projects[i] = project;
        } else {
            let state = self.db.load_file_tree_state(project.id)?;
            let status = git::fetch_status(&project.path);
            let mut tree = FileTreeView::new(project.path.clone(), state)?;
            tree.set_git_status(status.clone());
            let mut changes = ChangesView::new(project.path.clone());
            changes.set_status(&status);
            let yaml = self
                .db
                .load_project_settings_yaml(project.id)?
                .unwrap_or_default();
            self.project_settings_yaml.insert(project.id, yaml);
            let branch = git::current_branch(&project.path);
            self.project_views.insert(
                project.id,
                ProjectViewState {
                    tree,
                    changes,
                    left_pane: LeftPaneMode::Tree,
                    editor: None,
                    focus: Focus::Tree,
                    preferred_git_view: GitView::Working,
                    terminals: Vec::new(),
                    active_terminal: None,
                    agents: Vec::new(),
                    active_agent: None,
                    agent_resumed_this_run: false,
                    view_mode: ViewMode::Editor,
                    project_view: None,
                    git_view: None,
                    github_view: None,
                    github_available: None,
                    runtime: None,
                    branch,
                },
            );
            self.open_projects.push(project);
            self.active_index = self.open_projects.len() - 1;
        }
        self.mode = AppMode::Normal;
        self.picker = None;
        self.persist_open_projects()?;
        Ok(())
    }

    fn open_picker(&mut self) -> Result<()> {
        let saved = self.db.list_projects()?;
        let discovered = discover_new(&self.roots, &saved);
        self.picker = Some(ProjectPicker::new(saved, discovered, self.roots.clone()));
        self.mode = AppMode::Picker;
        Ok(())
    }

    fn open_project_view(&mut self) -> Result<()> {
        if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Project;
        }
        self.ensure_project_view_loaded();
        Ok(())
    }

    fn open_git_view(&mut self) -> Result<()> {
        if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Git;
        }
        self.ensure_git_view_loaded();
        Ok(())
    }

    fn open_github_view(&mut self) -> Result<()> {
        if !self.github_available_for_active() {
            self.status = "GitHub view unavailable — no `origin` remote on github.com".into();
            return Ok(());
        }
        if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Github;
        }
        self.ensure_github_view_loaded();
        Ok(())
    }

    fn on_key_git_view(&mut self, key: KeyEvent) -> Result<()> {
        use crate::views::git::{DetailsMode, GitPane};
        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.with_git_view(|v| v.move_down()),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.with_git_view(|v| v.move_up()),
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.with_git_view(|v| v.jump_top());
            }
            (KeyCode::Char('G'), _) => self.with_git_view(|v| v.jump_bottom()),
            (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.with_git_view(|v| v.cycle_pane(true));
            }
            (KeyCode::Char('h'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.with_git_view(|v| v.cycle_pane(false));
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) | (KeyCode::Right, _) => {
                let on_worktree = self
                    .active_state_ref()
                    .and_then(|s| s.git_view.as_ref())
                    .map(|v| {
                        v.focus == GitPane::Details
                            && matches!(v.details_mode, DetailsMode::Worktrees)
                    })
                    .unwrap_or(false);
                if on_worktree {
                    self.git_view_open_selected_worktree()?;
                } else {
                    self.with_git_view(|v| v.activate());
                }
            }
            (KeyCode::Esc, _) | (KeyCode::Backspace, _) | (KeyCode::Char('h'), _)
            | (KeyCode::Left, _) => {
                let in_overlay = self
                    .active_state_ref()
                    .and_then(|s| s.git_view.as_ref())
                    .map(|v| !matches!(v.details_mode, DetailsMode::Commit))
                    .unwrap_or(false);
                if in_overlay {
                    self.with_git_view(|v| v.back_to_pr_list());
                }
            }
            (KeyCode::Char('r'), _) => {
                self.with_git_view(|v| v.refresh_all());
                let _ = self.refresh_git_status();
            }
            (KeyCode::Char('c'), _) => self.git_view_checkout(),
            (KeyCode::Char('p'), _) => self.git_view_push(),
            (KeyCode::Char('P'), _) => self.git_view_pull(),
            (KeyCode::Char('m'), _) => self.git_view_merge(),
            (KeyCode::Char('R'), _) => self.git_view_create_pr(),
            (KeyCode::Char('V'), _) => self.git_view_load_prs(),
            (KeyCode::Char('W'), _) => self.git_view_load_worktrees(),
            (KeyCode::Char('n'), _) => self.git_view_start_new_worktree(),
            (KeyCode::Char('D'), _) => self.git_view_remove_worktree(),
            _ => {}
        }
        Ok(())
    }

    fn with_git_view(&mut self, mut f: impl FnMut(&mut crate::views::git::GitTreeView)) {
        if let Some(view) = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
        {
            f(view);
        }
    }

    fn git_view_checkout(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.checkout_selected());
        match result {
            Some(Ok(out)) => {
                self.status = git_first_line(&out, "Checked out");
                let _ = self.refresh_git_status();
            }
            Some(Err(e)) => self.status = format!("checkout failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_pull(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.pull());
        match result {
            Some(Ok(out)) => {
                self.status = git_first_line(&out, "Pulled");
                let _ = self.refresh_git_status();
            }
            Some(Err(e)) => self.status = format!("pull failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_push(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.push());
        match result {
            Some(Ok(out)) => self.status = git_first_line(&out, "Pushed"),
            Some(Err(e)) => self.status = format!("push failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_merge(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.merge_selected_into_current());
        match result {
            Some(Ok(out)) => {
                self.status = git_first_line(&out, "Merged");
                let _ = self.refresh_git_status();
            }
            Some(Err(e)) => self.status = format!("merge failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_create_pr(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.create_pr_for_current());
        match result {
            Some(Ok(out)) => self.status = git_first_line(&out, "PR created"),
            Some(Err(e)) => self.status = format!("gh pr create failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_load_prs(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.load_prs_for_branch());
        match result {
            Some(Ok(_)) => self.status = "Loaded PRs".into(),
            Some(Err(e)) => self.status = format!("gh pr list failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_load_worktrees(&mut self) {
        let count = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| {
                v.load_worktrees();
                v.worktrees.len()
            });
        if let Some(n) = count {
            self.status = format!("Loaded {} worktrees", n);
        }
    }

    fn git_view_open_selected_worktree(&mut self) -> Result<()> {
        let path = self
            .active_state()
            .and_then(|s| s.git_view.as_ref())
            .and_then(|v| v.selected_worktree().map(|w| w.path.clone()));
        let Some(path) = path else {
            self.status = "No worktree selected".into();
            return Ok(());
        };
        self.open_worktree_path(&path)?;
        Ok(())
    }

    fn git_view_remove_worktree(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.git_view.as_mut())
            .map(|v| v.remove_selected_worktree(false));
        match result {
            Some(Ok(out)) => self.status = git_first_line(&out, "Worktree removed"),
            Some(Err(e)) => {
                let needs_force = e.contains("contains modified")
                    || e.contains("not clean")
                    || e.contains("locked")
                    || e.contains("use --force");
                if needs_force {
                    let forced = self
                        .active_state()
                        .and_then(|s| s.git_view.as_mut())
                        .map(|v| v.remove_selected_worktree(true));
                    match forced {
                        Some(Ok(out)) => self.status = format!("force-removed: {}", git_first_line(&out, "ok")),
                        Some(Err(e2)) => self.status = format!("remove failed: {}", git_first_line(&e2, "")),
                        None => {}
                    }
                } else {
                    self.status = format!("remove failed: {}", git_first_line(&e, ""));
                }
            }
            None => {}
        }
    }

    fn on_key_github_view(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.with_github_view(|v| v.move_down()),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.with_github_view(|v| v.move_up()),
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.with_github_view(|v| v.jump_top());
            }
            (KeyCode::Char('G'), _) => self.with_github_view(|v| v.jump_bottom()),
            (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.with_github_view(|v| v.cycle_pane(true));
            }
            (KeyCode::Char('h'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.with_github_view(|v| v.cycle_pane(false));
            }
            (KeyCode::Enter, _) | (KeyCode::Char('l'), _) | (KeyCode::Right, _) => {
                self.with_github_view(|v| v.activate());
            }
            (KeyCode::Esc, _) | (KeyCode::Backspace, _) | (KeyCode::Char('h'), _)
            | (KeyCode::Left, _) => self.with_github_view(|v| v.back()),
            (KeyCode::Char('r'), _) => {
                self.with_github_view(|v| v.refresh_all());
                self.status = "Refreshed GitHub view".into();
            }
            (KeyCode::Char('R'), _) => self.github_view_rerun(false),
            (KeyCode::Char('F'), _) => self.github_view_rerun(true),
            (KeyCode::Char('X'), _) => self.github_view_cancel(),
            (KeyCode::Char('c'), _) => self.github_view_checkout_pr(),
            _ => {}
        }
        Ok(())
    }

    fn with_github_view(&mut self, mut f: impl FnMut(&mut crate::views::github::GithubView)) {
        if let Some(view) = self.active_state().and_then(|s| s.github_view.as_mut()) {
            f(view);
        }
    }

    fn github_view_rerun(&mut self, failed_only: bool) {
        let result = self
            .active_state()
            .and_then(|s| s.github_view.as_mut())
            .map(|v| v.rerun_selected(failed_only));
        match result {
            Some(Ok(out)) => self.status = git_first_line(&out, "Re-ran"),
            Some(Err(e)) => self.status = format!("rerun failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn github_view_cancel(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.github_view.as_mut())
            .map(|v| v.cancel_selected_run());
        match result {
            Some(Ok(out)) => self.status = git_first_line(&out, "Cancelled"),
            Some(Err(e)) => self.status = format!("cancel failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn github_view_checkout_pr(&mut self) {
        let result = self
            .active_state()
            .and_then(|s| s.github_view.as_mut())
            .map(|v| v.checkout_selected_pr());
        match result {
            Some(Ok(out)) => {
                self.status = git_first_line(&out, "Checked out PR");
                let _ = self.refresh_git_status();
            }
            Some(Err(e)) => self.status = format!("pr checkout failed: {}", git_first_line(&e, "")),
            None => {}
        }
    }

    fn git_view_start_new_worktree(&mut self) {
        let Some(project_id) = self.active_project().map(|p| p.id) else {
            self.status = "No active project".into();
            return;
        };
        self.worktree_prompt = Some(WorktreePromptState {
            project_id,
            buffer: String::new(),
            cursor: 0,
        });
        self.mode = AppMode::WorktreePrompt;
    }

    fn cancel_worktree_prompt(&mut self) {
        self.worktree_prompt = None;
        self.mode = AppMode::Normal;
    }

    fn commit_worktree_prompt(&mut self) -> Result<()> {
        let Some(st) = self.worktree_prompt.take() else {
            self.mode = AppMode::Normal;
            return Ok(());
        };
        self.mode = AppMode::Normal;
        let branch = st.buffer.trim().to_string();
        if branch.is_empty() {
            self.status = "Worktree creation cancelled (empty branch)".into();
            return Ok(());
        }
        let project = self
            .open_projects
            .iter()
            .find(|p| p.id == st.project_id)
            .cloned();
        let Some(project) = project else {
            self.status = "Project no longer open".into();
            return Ok(());
        };
        let path = derive_worktree_path(&project.path, &branch);
        let result = crate::git::add_worktree(&project.path, &path, &branch, true);
        match result {
            Ok(out) => {
                self.status = git_first_line(&out, &format!("Worktree created at {}", path.display()));
                if let Some(view) = self
                    .project_views
                    .get_mut(&project.id)
                    .and_then(|s| s.git_view.as_mut())
                {
                    view.load_worktrees();
                }
            }
            Err(e) => {
                self.status = format!("worktree add failed: {}", git_first_line(&e, ""));
            }
        }
        Ok(())
    }

    fn on_key_worktree_prompt(&mut self, key: KeyEvent) -> Result<()> {
        let Some(st) = self.worktree_prompt.as_mut() else {
            self.mode = AppMode::Normal;
            return Ok(());
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.cancel_worktree_prompt(),
            (KeyCode::Enter, _) => self.commit_worktree_prompt()?,
            (KeyCode::Backspace, _) => {
                if st.cursor > 0 {
                    let chars: Vec<char> = st.buffer.chars().collect();
                    let new_chars: Vec<char> = chars
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| if i + 1 == st.cursor { None } else { Some(*c) })
                        .collect();
                    st.buffer = new_chars.iter().collect();
                    st.cursor -= 1;
                }
            }
            (KeyCode::Delete, _) => {
                let chars: Vec<char> = st.buffer.chars().collect();
                if st.cursor < chars.len() {
                    let new_chars: Vec<char> = chars
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| if i == st.cursor { None } else { Some(*c) })
                        .collect();
                    st.buffer = new_chars.iter().collect();
                }
            }
            (KeyCode::Left, _) => {
                if st.cursor > 0 {
                    st.cursor -= 1;
                }
            }
            (KeyCode::Right, _) => {
                let len = st.buffer.chars().count();
                if st.cursor < len {
                    st.cursor += 1;
                }
            }
            (KeyCode::Home, _) => st.cursor = 0,
            (KeyCode::End, _) => st.cursor = st.buffer.chars().count(),
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) => {
                let mut chars: Vec<char> = st.buffer.chars().collect();
                chars.insert(st.cursor, c);
                st.buffer = chars.iter().collect();
                st.cursor += 1;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn start_new_file(&mut self) {
        let Some(parent) = self.selected_dir_for_create() else {
            self.status = "No project open".into();
            return;
        };
        self.file_prompt = Some(FilePromptState {
            kind: FilePromptKind::NewFile,
            parent,
            source: None,
            buffer: String::new(),
            cursor: 0,
        });
        self.mode = AppMode::FilePrompt;
    }

    pub fn start_new_dir(&mut self) {
        let Some(parent) = self.selected_dir_for_create() else {
            self.status = "No project open".into();
            return;
        };
        self.file_prompt = Some(FilePromptState {
            kind: FilePromptKind::NewDir,
            parent,
            source: None,
            buffer: String::new(),
            cursor: 0,
        });
        self.mode = AppMode::FilePrompt;
    }

    pub fn start_rename_file(&mut self) {
        let Some(path) = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()))
        else {
            self.status = "No file selected".into();
            return;
        };
        let parent = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cursor = name.chars().count();
        self.file_prompt = Some(FilePromptState {
            kind: FilePromptKind::Rename,
            parent,
            source: Some(path),
            buffer: name,
            cursor,
        });
        self.mode = AppMode::FilePrompt;
    }

    fn cancel_file_prompt(&mut self) {
        self.file_prompt = None;
        self.mode = AppMode::Normal;
    }

    fn commit_file_prompt(&mut self) {
        let Some(st) = self.file_prompt.take() else {
            self.mode = AppMode::Normal;
            return;
        };
        self.mode = AppMode::Normal;
        let name = st.buffer.trim().to_string();
        if name.is_empty() {
            self.status = "Cancelled (empty name)".into();
            return;
        }
        let target = st.parent.join(&name);
        match st.kind {
            FilePromptKind::NewFile => match std::fs::write(&target, b"") {
                Ok(_) => {
                    self.status = format!("Created {}", display_relative(&target, &st.parent));
                    self.refresh_active_tree();
                    self.reveal_in_tree(&target);
                }
                Err(e) => self.status = format!("Create failed: {}", e),
            },
            FilePromptKind::NewDir => match std::fs::create_dir_all(&target) {
                Ok(_) => {
                    self.status = format!("Created {}/", display_relative(&target, &st.parent));
                    self.refresh_active_tree();
                    self.reveal_in_tree(&target);
                }
                Err(e) => self.status = format!("Create dir failed: {}", e),
            },
            FilePromptKind::Rename => {
                let Some(source) = st.source else {
                    self.status = "Rename: no source".into();
                    return;
                };
                if target == source {
                    return;
                }
                match std::fs::rename(&source, &target) {
                    Ok(_) => {
                        self.status =
                            format!("Renamed → {}", display_relative(&target, &st.parent));
                        self.refresh_active_tree();
                        self.reveal_in_tree(&target);
                    }
                    Err(e) => self.status = format!("Rename failed: {}", e),
                }
            }
        }
    }

    fn on_key_file_prompt(&mut self, key: KeyEvent) -> Result<()> {
        let Some(st) = self.file_prompt.as_mut() else {
            self.mode = AppMode::Normal;
            return Ok(());
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.cancel_file_prompt(),
            (KeyCode::Enter, _) => self.commit_file_prompt(),
            (KeyCode::Backspace, _) => {
                if st.cursor > 0 {
                    let chars: Vec<char> = st.buffer.chars().collect();
                    let new_chars: Vec<char> = chars
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| if i + 1 == st.cursor { None } else { Some(*c) })
                        .collect();
                    st.buffer = new_chars.iter().collect();
                    st.cursor -= 1;
                }
            }
            (KeyCode::Delete, _) => {
                let chars: Vec<char> = st.buffer.chars().collect();
                if st.cursor < chars.len() {
                    let new_chars: Vec<char> = chars
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| if i == st.cursor { None } else { Some(*c) })
                        .collect();
                    st.buffer = new_chars.iter().collect();
                }
            }
            (KeyCode::Left, _) => {
                if st.cursor > 0 {
                    st.cursor -= 1;
                }
            }
            (KeyCode::Right, _) => {
                let len = st.buffer.chars().count();
                if st.cursor < len {
                    st.cursor += 1;
                }
            }
            (KeyCode::Home, _) => st.cursor = 0,
            (KeyCode::End, _) => st.cursor = st.buffer.chars().count(),
            (KeyCode::Char(c), m)
                if !m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) =>
            {
                let mut chars: Vec<char> = st.buffer.chars().collect();
                chars.insert(st.cursor, c);
                st.buffer = chars.iter().collect();
                st.cursor += 1;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn yank_selected_file(&mut self) {
        let Some(path) = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()))
        else {
            self.status = "No file selected".into();
            return;
        };
        self.file_yank = Some(path.clone());
        self.status = format!(
            "Yanked {}",
            path.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }

    pub fn paste_yanked_file(&mut self) {
        let Some(source) = self.file_yank.clone() else {
            self.status = "Nothing to paste — yank a file with `c` first".into();
            return;
        };
        let Some(dest_dir) = self.selected_dir_for_create() else {
            self.status = "No paste destination".into();
            return;
        };
        let base_name = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let target = unique_target_path(&dest_dir, &base_name);
        let result = if source.is_dir() {
            copy_dir_recursive(&source, &target)
        } else {
            std::fs::copy(&source, &target).map(|_| ()).map_err(|e| e.to_string())
        };
        match result {
            Ok(_) => {
                self.status = format!("Copied → {}", display_relative(&target, &dest_dir));
                self.refresh_active_tree();
                self.reveal_in_tree(&target);
            }
            Err(e) => self.status = format!("Paste failed: {}", e),
        }
    }

    pub fn start_delete_file(&mut self) {
        let Some(path) = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()))
        else {
            self.status = "No file selected".into();
            return;
        };
        self.pending_delete_file = Some(path);
        self.mode = AppMode::ConfirmDeleteFile;
    }

    fn cancel_delete_file(&mut self) {
        self.pending_delete_file = None;
        self.mode = AppMode::Normal;
    }

    fn confirm_delete_file(&mut self) {
        let Some(path) = self.pending_delete_file.take() else {
            self.mode = AppMode::Normal;
            return;
        };
        self.mode = AppMode::Normal;
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(&path).map_err(|e| e.to_string())
        };
        match result {
            Ok(_) => {
                self.status = format!(
                    "Deleted {}",
                    path.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                );
                self.refresh_active_tree();
            }
            Err(e) => self.status = format!("Delete failed: {}", e),
        }
    }

    fn on_key_confirm_delete_file(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) | (KeyCode::Enter, _) => {
                self.confirm_delete_file();
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) | (KeyCode::Esc, _) => {
                self.cancel_delete_file();
            }
            _ => {}
        }
        Ok(())
    }

    fn selected_dir_for_create(&self) -> Option<PathBuf> {
        let project_root = self.active_project()?.path.clone();
        let selected = self
            .active_state_ref()
            .and_then(|s| s.selected_path().map(|p| p.to_path_buf()));
        match selected {
            Some(p) if p.is_dir() => Some(p),
            Some(p) => p.parent().map(|x| x.to_path_buf()),
            None => Some(project_root),
        }
    }

    fn refresh_active_tree(&mut self) {
        if let Some(state) = self.active_state() {
            state.tree.refresh();
        }
        let _ = self.refresh_git_status();
    }

    fn reveal_in_tree(&mut self, path: &Path) {
        if let Some(state) = self.active_state() {
            state.tree.reveal_path(path);
        }
    }

    fn open_or_focus_terminal(&mut self) -> Result<()> {
        let already_has = self
            .active_state_ref()
            .map(|s| !s.terminals.is_empty())
            .unwrap_or(false);
        if !already_has {
            self.new_terminal()?;
        } else if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Terminal;
        }
        Ok(())
    }

    fn new_terminal(&mut self) -> Result<()> {
        let Some((project_id, project_path)) = self
            .active_project()
            .map(|p| (p.id, p.path.clone()))
        else {
            self.status = "No active project".into();
            return Ok(());
        };
        let shell = self.effective_shell_config(project_id);
        match crate::views::terminal::TerminalView::spawn(&shell, &project_path, 24, 80) {
            Ok(term) => {
                if let Some(state) = self.active_state() {
                    state.terminals.push(term);
                    state.active_terminal = Some(state.terminals.len() - 1);
                    state.view_mode = ViewMode::Terminal;
                }
            }
            Err(e) => {
                self.status = format!("Failed to spawn terminal: {}", e);
            }
        }
        Ok(())
    }

    fn close_active_terminal(&mut self) {
        if let Some(state) = self.active_state() {
            if let Some(i) = state.active_terminal {
                if i < state.terminals.len() {
                    state.terminals.remove(i);
                }
                if state.terminals.is_empty() {
                    state.active_terminal = None;
                    state.view_mode = ViewMode::Editor;
                } else {
                    state.active_terminal = Some(i.min(state.terminals.len() - 1));
                }
            }
        }
    }

    fn cycle_terminal(&mut self, forward: bool) {
        if let Some(state) = self.active_state() {
            let n = state.terminals.len();
            if n == 0 {
                return;
            }
            let cur = state.active_terminal.unwrap_or(0);
            let next = if forward {
                (cur + 1) % n
            } else {
                (cur + n - 1) % n
            };
            state.active_terminal = Some(next);
        }
    }

    fn new_agent(&mut self) -> Result<()> {
        self.spawn_agent(None)
    }

    fn agent_for_selected_feature(&mut self) -> Result<()> {
        use crate::views::project_view::{ProjectSelection, feature_filename};
        let in_project = matches!(self.current_view_mode(), Some(ViewMode::Project));
        if !in_project {
            self.status = "Switch to Project view first".into();
            return Ok(());
        }
        let selection = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .map(|m| m.selection);
        let Some(ProjectSelection::Feature(i)) = selection else {
            self.status = "Select a feature first".into();
            return Ok(());
        };
        let feature_info = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .and_then(|m| m.features.get(i))
            .map(|f| (f.id, f.title.clone(), feature_filename(f)));
        let Some((feature_id, feature_title, feature_file)) = feature_info else {
            return Ok(());
        };
        let Some(project) = self.active_project().cloned() else { return Ok(()); };

        let context_dir = self.write_agent_project_context(project.id, &project.name)?;
        if let Some(state) = self.active_state() {
            state.view_mode = ViewMode::Agents;
        }
        self.ensure_agents_restored()?;
        let agent_name = format_feature_agent_name(feature_id, &feature_title);
        self.spawn_agent_for_project(&project, None, true, Some(agent_name))?;

        let message = format!(
            "Take feature #{id} \"{title}\". Start by reading {dir}/index.md, then {dir}/features/{file}. \
             Mark it in progress first (`coffeetable agent set-feature-status --feature-id {id} --status in_progress`), \
             then implement the Steps section. When done, set status to in_review and log the result with `coffeetable agent log-turn`.",
            id = feature_id,
            title = feature_title,
            dir = context_dir.display(),
            file = feature_file,
        );
        let mut bytes = bracketed_paste_bytes(&message);
        bytes.push(b'\r');
        if let Some(state) = self.active_state() {
            if let Some(i) = state.active_agent {
                if let Some(agent) = state.agents.get_mut(i) {
                    agent.queue_initial_input(bytes);
                }
            }
        }
        Ok(())
    }

    fn spawn_agent(&mut self, resume_session_id: Option<String>) -> Result<()> {
        let Some(project) = self.active_project().cloned() else {
            self.status = "No active project".into();
            return Ok(());
        };
        self.spawn_agent_for_project(&project, resume_session_id, true, None)
    }

    fn spawn_agent_for_project(
        &mut self,
        project: &Project,
        resume_session_id: Option<String>,
        focus_agents_view: bool,
        name_override: Option<String>,
    ) -> Result<()> {
        let ai = self.effective_ai_config(project.id);
        let context_dir = self.write_agent_project_context(project.id, &project.name)?;
        let prompt = self.agent_system_prompt(&project.name, project.id, &context_dir);
        let count = self
            .project_views
            .get(&project.id)
            .map(|s| s.agents.len())
            .unwrap_or(0);
        let suffix = if resume_session_id.is_some() { " (resumed)" } else { "" };
        let name = name_override
            .map(|n| if resume_session_id.is_some() { format!("{}{}", n, suffix) } else { n })
            .unwrap_or_else(|| format!("{} #{}{}", ai.provider, count + 1, suffix));
        match crate::views::agents::AgentSession::spawn(
            &ai,
            name,
            &project.path,
            Some(&prompt),
            resume_session_id.as_deref(),
            24,
            80,
        ) {
            Ok(agent) => {
                if let Some(state) = self.project_views.get_mut(&project.id) {
                    state.agents.push(agent);
                    let new_idx = state.agents.len() - 1;
                    if focus_agents_view {
                        state.active_agent = Some(new_idx);
                        state.view_mode = ViewMode::Agents;
                    } else if state.active_agent.is_none() {
                        state.active_agent = Some(new_idx);
                    }
                }
                self.persist_agent_sessions(project.id);
            }
            Err(e) => {
                self.status = format!("Failed to spawn agent: {}", e);
            }
        }
        Ok(())
    }

    pub fn restore_all_agents(&mut self) -> Result<()> {
        let projects: Vec<Project> = self.open_projects.clone();
        for p in projects {
            let already = self
                .project_views
                .get(&p.id)
                .map(|s| s.agent_resumed_this_run || !s.agents.is_empty())
                .unwrap_or(true);
            if already {
                continue;
            }
            if let Some(state) = self.project_views.get_mut(&p.id) {
                state.agent_resumed_this_run = true;
            }
            let saved = self.db.load_agent_sessions(p.id).unwrap_or_default();
            for id in saved {
                self.spawn_agent_for_project(&p, Some(id), false, None)?;
            }
        }
        Ok(())
    }

    fn ensure_agents_restored(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else { return Ok(()) };
        if self
            .active_state_ref()
            .map(|s| s.agent_resumed_this_run || !s.agents.is_empty())
            .unwrap_or(true)
        {
            return Ok(());
        }
        let saved = self.db.load_agent_sessions(project.id).unwrap_or_default();
        if let Some(state) = self.active_state() {
            state.agent_resumed_this_run = true;
        }
        if saved.is_empty() {
            return Ok(());
        }
        for id in saved {
            self.spawn_agent(Some(id))?;
        }
        Ok(())
    }

    fn persist_agent_sessions(&mut self, project_id: i64) {
        let ids: Vec<String> = self
            .project_views
            .get(&project_id)
            .map(|s| {
                s.agents
                    .iter()
                    .filter_map(|a| a.session_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let _ = self.db.save_agent_sessions(project_id, &ids);
    }

    fn capture_active_agent_session(&mut self) {
        let Some(project_id) = self.active_project().map(|p| p.id) else { return };
        let captured = {
            let Some(state) = self.active_state() else { return };
            let Some(i) = state.active_agent else { return };
            let Some(agent) = state.agents.get_mut(i) else { return };
            agent.try_capture_session_id()
        };
        if captured {
            self.persist_agent_sessions(project_id);
        }
    }

    fn write_agent_project_context(
        &mut self,
        project_id: i64,
        project_name: &str,
    ) -> Result<std::path::PathBuf> {
        let meta = self.db.load_project_meta(project_id)?;
        let features = self.db.list_features(project_id)?;
        let dir = self.paths.project_context_dir(project_id);
        Self::write_context_files(&dir, project_name, &meta, &features)?;
        Ok(dir)
    }

    fn write_context_files(
        dir: &std::path::Path,
        project_name: &str,
        meta: &crate::project::ProjectMeta,
        features: &[crate::project::Feature],
    ) -> Result<()> {
        use crate::views::project_view;
        std::fs::create_dir_all(dir)?;
        let features_dir = dir.join("features");
        std::fs::create_dir_all(&features_dir)?;
        std::fs::write(
            dir.join("index.md"),
            project_view::index_markdown(project_name, features),
        )?;
        for section in ["about", "conventions", "ai_hints", "ai_notes"] {
            let body = project_view::meta_section_body(meta, section);
            std::fs::write(dir.join(format!("{}.md", section)), body)?;
        }
        let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
        for f in features {
            let fname = project_view::feature_filename(f);
            std::fs::write(features_dir.join(&fname), project_view::feature_markdown(f))?;
            keep.insert(fname);
        }
        if let Ok(entries) = std::fs::read_dir(&features_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if !keep.contains(name) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(())
    }

    fn agent_system_prompt(
        &self,
        project_name: &str,
        project_id: i64,
        context_dir: &std::path::Path,
    ) -> String {
        format!(
            "You are running inside CoffeeTable as an agent attached to project \"{name}\" (project_id = {pid}).\n\n\
Always reply in the same natural language the user used in their most recent message. If they write in Polish, respond in Polish; if they switch to English, switch with them. Default to English only when the user's language is unclear.\n\n\
You start with no project metadata in context. The user will tell you which sections / features to read; do NOT preload them on your own.\n\n\
Available context files (read on demand, only when the user asks or it's clearly needed):\n  {dir}/index.md            — table of contents (list of features + their files)\n  {dir}/about.md            — project description\n  {dir}/conventions.md      — project conventions\n  {dir}/ai_hints.md         — instructions the user wrote for you\n  {dir}/ai_notes.md         — running notes you may also append to\n  {dir}/features/<file>.md  — one file per feature (see index.md for names + feature_id)\nThese files are regenerated whenever the user saves the Project tab — re-read if you need a refreshed view.\n\n\
Looking up a feature: when the user asks you to work on a feature (\"do feature X\", \"zrób feature X\", \"implement Y\", or similar), ALWAYS open {dir}/index.md first to find the feature's id and filename, then read {dir}/features/<file>.md before starting. Do not guess the path or the id.\n\n\
Taking a task: the moment you commit to working on a feature, mark it in progress before doing anything else:\n  coffeetable agent set-feature-status --feature-id <id> --status in_progress\nDo this even if the user only implicitly assigned the task. When the work is finished, set status to `in_review` (the user reviews and bumps to `done`). If you decide not to do it, leave status alone and explain why.\n\n\
You can write back into the project using the CLI binary `coffeetable` from the shell (run from any cwd):\n  coffeetable agent comment            --feature-id <id> --kind <note|request|response> --message <text>\n  coffeetable agent log-turn           --feature-id <id> --request <text> --response <text>\n  coffeetable agent set-feature-status --feature-id <id> --status <idea|todo|in_progress|in_review|done|cancelled>\n  coffeetable agent append-notes       --project-id {pid} --message <text>\n  coffeetable agent set-notes          --project-id {pid} --message <text>\n  coffeetable agent append-hints       --project-id {pid} --message <text>\n  coffeetable agent set-hints          --project-id {pid} --message <text>\nFor multi-line --message values, pass the text via a single shell-quoted argument.\n\n\
When you finish a meaningful exchange about a feature, log it as comments:\n  - kind=request for what the user asked\n  - kind=response for your summary/answer\n\
This keeps the Project tab as the source of truth for the conversation history.",
            name = project_name,
            pid = project_id,
            dir = context_dir.display(),
        )
    }

    fn close_active_agent(&mut self) {
        let project_id = self.active_project().map(|p| p.id);
        let mut closed_name = None;
        if let Some(state) = self.active_state() {
            if let Some(i) = state.active_agent {
                if i < state.agents.len() {
                    let mut session = state.agents.remove(i);
                    closed_name = Some(session.name.clone());
                    session.shutdown();
                }
                if state.agents.is_empty() {
                    state.active_agent = None;
                    state.view_mode = ViewMode::Editor;
                } else {
                    state.active_agent = Some(i.min(state.agents.len() - 1));
                }
            }
        }
        if let Some(id) = project_id {
            self.persist_agent_sessions(id);
        }
        if let Some(name) = closed_name {
            self.status = format!("Closed agent: {}", name);
        }
    }

    fn try_handle_terminal_prefix_global(&mut self, key: KeyEvent) -> Option<Result<()>> {
        let result = match key.code {
            KeyCode::Char('p') => self.open_picker(),
            KeyCode::Char('f') => {
                self.open_explorer_filter();
                Ok(())
            }
            KeyCode::Char('g') => {
                self.open_grep();
                Ok(())
            }
            KeyCode::Char('e') => {
                self.focus_tree();
                Ok(())
            }
            KeyCode::Char('b') => {
                self.focus_editor();
                Ok(())
            }
            KeyCode::Char('c') => {
                self.toggle_left_pane();
                Ok(())
            }
            KeyCode::Char('C') => self.start_ai_commit(),
            KeyCode::Char('w') => {
                self.palette_show_working();
                Ok(())
            }
            KeyCode::Char('t') => self.open_or_focus_terminal(),
            KeyCode::Char('T') => self.new_terminal(),
            KeyCode::Char('P') => self.open_project_view(),
            KeyCode::Char('G') => self.open_git_view(),
            KeyCode::Char('H') => self.open_github_view(),
            KeyCode::Char('r') => self.open_runtime_view(),
            KeyCode::Char('a') => self.agent_for_selected_feature(),
            KeyCode::Char('z') => {
                self.cycle_editor_wrap();
                Ok(())
            }
            KeyCode::Char('?') => {
                self.help_visible = true;
                Ok(())
            }
            _ => return None,
        };
        Some(result)
    }

    fn cycle_agent(&mut self, forward: bool) {
        if let Some(state) = self.active_state() {
            let n = state.agents.len();
            if n == 0 {
                return;
            }
            let cur = state.active_agent.unwrap_or(0);
            let next = if forward {
                (cur + 1) % n
            } else {
                (cur + n - 1) % n
            };
            state.active_agent = Some(next);
        }
    }

    fn write_to_active_agent(&mut self, bytes: &[u8]) {
        if let Some(state) = self.active_state() {
            if let Some(i) = state.active_agent {
                if let Some(agent) = state.agents.get_mut(i) {
                    agent.cancel_initial_input();
                    agent.clear_selection();
                    agent.reset_scrollback();
                    agent.write_bytes(bytes);
                }
            }
        }
        self.capture_active_agent_session();
    }

    fn paste_into_active_agent(&mut self) {
        let Some(text) = crate::clipboard::paste() else {
            self.status = "Clipboard is empty".into();
            return;
        };
        if text.is_empty() {
            self.status = "Clipboard is empty".into();
            return;
        }
        let bytes = bracketed_paste_bytes(&text);
        self.write_to_active_agent(&bytes);
        self.status = format!("Pasted {} chars", text.chars().count());
    }

    pub fn on_paste(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let Some(view) = self.current_view_mode() else { return };
        match view {
            ViewMode::Agents => {
                let bytes = bracketed_paste_bytes(&text);
                self.write_to_active_agent(&bytes);
                self.status = format!("Pasted {} chars", text.chars().count());
            }
            ViewMode::Terminal => {
                let bytes = bracketed_paste_bytes(&text);
                self.write_to_active_terminal(&bytes);
                self.status = format!("Pasted {} chars", text.chars().count());
            }
            _ => {}
        }
    }

    fn on_key_agents(&mut self, key: KeyEvent) -> Result<()> {
        if self.terminal_prefix {
            self.terminal_prefix = false;
            match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    if let Some(state) = self.active_state() {
                        state.view_mode = ViewMode::Editor;
                    }
                    return Ok(());
                }
                KeyCode::Char('n') => return self.new_agent(),
                KeyCode::Char('l') => {
                    self.cycle_agent(true);
                    return Ok(());
                }
                KeyCode::Char('h') => {
                    self.cycle_agent(false);
                    return Ok(());
                }
                KeyCode::Char('x') => {
                    self.close_active_agent();
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    self.start_rename_active_agent();
                    return Ok(());
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    self.paste_into_active_agent();
                    return Ok(());
                }
                KeyCode::Char(' ') => {
                    self.write_to_active_agent(&[0]);
                    return Ok(());
                }
                _ => {
                    if let Some(r) = self.try_handle_terminal_prefix_global(key) {
                        return r;
                    }
                    return Ok(());
                }
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            self.write_to_active_agent(&[0x03]);
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
        {
            self.paste_into_active_agent();
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char(' ') => {
                    self.terminal_prefix = true;
                    return Ok(());
                }
                KeyCode::Char('l') => {
                    self.cycle_agent(true);
                    return Ok(());
                }
                KeyCode::Char('h') => {
                    self.cycle_agent(false);
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some(bytes) = crate::views::terminal::key_to_bytes(key) {
            self.write_to_active_agent(&bytes);
        }
        Ok(())
    }

    fn on_key_terminal(&mut self, key: KeyEvent) -> Result<()> {
        if self.terminal_prefix {
            self.terminal_prefix = false;
            match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    if let Some(state) = self.active_state() {
                        state.view_mode = ViewMode::Editor;
                    }
                    return Ok(());
                }
                KeyCode::Char('n') => return self.new_terminal(),
                KeyCode::Char('l') => {
                    self.cycle_terminal(true);
                    return Ok(());
                }
                KeyCode::Char('h') => {
                    self.cycle_terminal(false);
                    return Ok(());
                }
                KeyCode::Char('x') => {
                    self.close_active_terminal();
                    return Ok(());
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    self.paste_into_active_terminal();
                    return Ok(());
                }
                KeyCode::Char(' ') => {
                    let bytes = vec![0];
                    self.write_to_active_terminal(&bytes);
                    return Ok(());
                }
                _ => {
                    if let Some(r) = self.try_handle_terminal_prefix_global(key) {
                        return r;
                    }
                    return Ok(());
                }
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            self.write_to_active_terminal(&[0x03]);
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
        {
            self.paste_into_active_terminal();
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char(' ') => {
                    self.terminal_prefix = true;
                    return Ok(());
                }
                KeyCode::Char('l') => {
                    self.cycle_terminal(true);
                    return Ok(());
                }
                KeyCode::Char('h') => {
                    self.cycle_terminal(false);
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some(bytes) = crate::views::terminal::key_to_bytes(key) {
            self.write_to_active_terminal(&bytes);
        }
        Ok(())
    }

    fn on_key_project_view(&mut self, key: KeyEvent) -> Result<()> {
        let is_editing = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .map(|m| m.editor.is_some() || m.feature_form.is_some())
            .unwrap_or(false);
        if is_editing {
            return self.on_key_project_editing(key);
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.project_scroll_preview(1)
            }
            (KeyCode::Char('y'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.project_scroll_preview(-1)
            }
            (KeyCode::PageDown, _) => self.project_scroll_preview(5),
            (KeyCode::PageUp, _) => self.project_scroll_preview(-5),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.project_move_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.project_move_up(),
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => self.project_jump_top(),
            (KeyCode::Char('G'), _) => self.project_jump_bottom(),
            (KeyCode::Char('i'), _) | (KeyCode::Enter, _) | (KeyCode::Char('l'), _)
            | (KeyCode::Right, _) => self.project_begin_edit(),
            (KeyCode::Char('n'), _) => self.project_add_feature()?,
            (KeyCode::Char('x'), _) => self.project_cycle_status()?,
            (KeyCode::Char('D'), _) => self.project_delete_selected()?,
            _ => {}
        }
        if let Some(state) = self.active_state() {
            let in_edit = state
                .project_view
                .as_ref()
                .map(|m| m.editor.is_some() || m.feature_form.is_some())
                .unwrap_or(false);
            state.focus = if in_edit { Focus::Editor } else { Focus::Tree };
        }
        Ok(())
    }

    fn on_key_project_editing(&mut self, key: KeyEvent) -> Result<()> {
        let has_form = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .map(|m| m.feature_form.is_some())
            .unwrap_or(false);
        if has_form {
            return self.on_key_feature_form(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('s')) {
            self.project_save_edit()?;
            return Ok(());
        }
        let close_after = {
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            let Some(editor) = model.editor.as_mut() else { return Ok(()) };
            let normal = editor.mode == EditorMode::Normal;
            let close_keys = matches!(key.code, KeyCode::Esc | KeyCode::Backspace);
            if normal && close_keys {
                true
            } else {
                editor.handle_key(key);
                editor.did_save = false;
                editor.request_focus_tree = false;
                false
            }
        };
        let pending_status = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
            .and_then(|m| m.editor.as_mut())
            .map(|e| std::mem::take(&mut e.status))
            .unwrap_or_default();
        if !pending_status.is_empty() {
            self.status = pending_status;
        }
        if close_after {
            self.project_save_edit()?;
        }
        Ok(())
    }

    fn on_key_feature_form(&mut self, key: KeyEvent) -> Result<()> {
        use crate::views::feature_form::FormFocus;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if ctrl && matches!(key.code, KeyCode::Char('s')) {
            self.project_save_edit()?;
            return Ok(());
        }
        if alt && matches!(key.code, KeyCode::Char('1')) {
            self.with_feature_form(|f| f.focus_title());
            return Ok(());
        }
        if alt && matches!(key.code, KeyCode::Char('2')) {
            self.with_feature_form(|f| f.focus_first_comment());
            return Ok(());
        }
        if ctrl {
            match key.code {
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    self.with_feature_form(|f| f.scroll_lines(1));
                    return Ok(());
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.with_feature_form(|f| f.scroll_lines(-1));
                    return Ok(());
                }
                _ => {}
            }
        }
        let editor_open = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .and_then(|m| m.feature_form.as_ref())
            .map(|f| f.editor_open())
            .unwrap_or(false);
        if editor_open {
            return self.on_key_feature_form_editor(key);
        }
        let focus = match self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .and_then(|m| m.feature_form.as_ref())
            .map(|f| f.focus)
        {
            Some(f) => f,
            None => return Ok(()),
        };
        if matches!(key.code, KeyCode::Esc) {
            self.project_close_form()?;
            return Ok(());
        }
        match focus {
            FormFocus::Status => self.handle_status_focus_key(key),
            FormFocus::Description => self.handle_description_focus_key(key)?,
            FormFocus::Title
            | FormFocus::Step(_)
            | FormFocus::NewStep
            | FormFocus::Comment(_)
            | FormFocus::NewComment => self.handle_text_focus_key(key, focus)?,
        }
        Ok(())
    }

    fn handle_status_focus_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.with_feature_form(|f| f.focus_next());
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                self.with_feature_form(|f| f.focus_prev());
            }
            (KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
                self.with_feature_form(|f| f.cycle_status_prev());
            }
            (KeyCode::Right, _) | (KeyCode::Char('l'), _) => {
                self.with_feature_form(|f| f.cycle_status());
            }
            (KeyCode::Enter, _)
            | (KeyCode::Char(' '), _)
            | (KeyCode::Char('x'), _) => {
                self.with_feature_form(|f| f.cycle_status());
            }
            _ => {}
        }
    }

    fn handle_description_focus_key(&mut self, key: KeyEvent) -> Result<()> {
        match (key.code, key.modifiers) {
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.with_feature_form(|f| f.focus_next());
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                self.with_feature_form(|f| f.focus_prev());
            }
            (KeyCode::Char('i'), _)
            | (KeyCode::Enter, _)
            | (KeyCode::Char('l'), _)
            | (KeyCode::Right, _) => {
                self.with_feature_form(|f| {
                    let _ = f.open_description_editor();
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_text_focus_key(
        &mut self,
        key: KeyEvent,
        focus: crate::views::feature_form::FormFocus,
    ) -> Result<()> {
        use crate::views::feature_form::FormFocus;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if ctrl && !alt {
            return self.handle_text_focus_ctrl_key(key, focus);
        }
        if matches!(focus, FormFocus::Comment(_) | FormFocus::NewComment) && !alt {
            return self.handle_message_focus_key(key);
        }
        match key.code {
            KeyCode::Down => {
                self.with_feature_form(|f| f.focus_next());
            }
            KeyCode::Up => {
                self.with_feature_form(|f| f.focus_prev());
            }
            KeyCode::Left => {
                self.with_feature_form(|f| f.move_caret_left());
            }
            KeyCode::Right => {
                self.with_feature_form(|f| f.move_caret_right());
            }
            KeyCode::Home => {
                self.with_feature_form(|f| f.move_caret_home());
            }
            KeyCode::End => {
                self.with_feature_form(|f| f.move_caret_end());
            }
            KeyCode::Backspace => {
                self.with_feature_form(|f| f.delete_char_backward());
            }
            KeyCode::Delete => {
                self.with_feature_form(|f| {
                    f.move_caret_right();
                    f.delete_char_backward();
                });
            }
            KeyCode::Enter => self.handle_text_focus_enter(focus),
            KeyCode::Char(c) => {
                self.with_feature_form(|f| f.insert_char(c));
            }
            _ => {}
        }
        let _ = FormFocus::Title;
        Ok(())
    }

    fn handle_message_focus_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.with_feature_form(|f| f.focus_next());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.with_feature_form(|f| f.focus_prev());
            }
            KeyCode::Enter
            | KeyCode::Char('i')
            | KeyCode::Char('a')
            | KeyCode::Char('o')
            | KeyCode::Char(' ') => {
                self.with_feature_form(|f| {
                    let _ = f.open_editor_for_focus();
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_text_focus_ctrl_key(
        &mut self,
        key: KeyEvent,
        focus: crate::views::feature_form::FormFocus,
    ) -> Result<()> {
        use crate::views::feature_form::FormFocus;
        match key.code {
            KeyCode::Char('t') | KeyCode::Char('T') => {
                if matches!(focus, FormFocus::Step(_)) {
                    self.with_feature_form(|f| f.cycle_step_status());
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if matches!(focus, FormFocus::Step(_) | FormFocus::Comment(_)) {
                    self.with_feature_form(|f| f.delete_focused());
                }
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                if matches!(focus, FormFocus::Comment(_)) {
                    self.with_feature_form(|f| f.cycle_comment_kind());
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_text_focus_enter(&mut self, focus: crate::views::feature_form::FormFocus) {
        use crate::views::feature_form::FormFocus;
        match focus {
            FormFocus::NewStep => {
                self.with_feature_form(|f| {
                    let _ = f.commit_new_step();
                });
            }
            FormFocus::NewComment => {
                self.with_feature_form(|f| {
                    let _ = f.commit_new_comment();
                });
            }
            _ => {
                self.with_feature_form(|f| f.focus_next());
            }
        }
    }

    fn on_key_feature_form_editor(&mut self, key: KeyEvent) -> Result<()> {
        let close_keys = matches!(key.code, KeyCode::Esc | KeyCode::Backspace);
        let normal_mode = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .and_then(|m| m.feature_form.as_ref())
            .and_then(|f| f.editor.as_ref())
            .map(|e| e.mode == EditorMode::Normal)
            .unwrap_or(false);
        if normal_mode && close_keys {
            self.with_feature_form(|f| f.commit_editor());
            return Ok(());
        }
        let pending_status = {
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            let Some(form) = model.feature_form.as_mut() else { return Ok(()) };
            let Some(editor) = form.editor.as_mut() else { return Ok(()) };
            editor.handle_key(key);
            editor.did_save = false;
            editor.request_focus_tree = false;
            std::mem::take(&mut editor.status)
        };
        if !pending_status.is_empty() {
            self.status = pending_status;
        }
        Ok(())
    }

    fn with_feature_form(&mut self, mut f: impl FnMut(&mut crate::views::feature_form::FeatureForm)) {
        if let Some(form) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
            .and_then(|m| m.feature_form.as_mut())
        {
            f(form);
        }
    }

    fn project_move_down(&mut self) {
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            model.move_down();
        }
    }

    fn project_move_up(&mut self) {
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            model.move_up();
        }
    }

    fn project_jump_top(&mut self) {
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            model.jump_top();
        }
    }

    fn project_jump_bottom(&mut self) {
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            model.jump_bottom();
        }
    }

    fn project_scroll_preview(&mut self, delta: i32) {
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            model.scroll_preview(delta);
        }
    }

    fn project_begin_edit(&mut self) {
        use crate::views::feature_form::FeatureForm;
        use crate::views::project_view::{ProjectSection, ProjectSelection};
        let Some(state) = self.active_state() else { return };
        let Some(model) = state.project_view.as_mut() else { return };
        match model.selection {
            ProjectSelection::Meta(section) => {
                let initial = match section {
                    ProjectSection::About => model.meta.description.clone(),
                    ProjectSection::Conventions => model.meta.conventions.clone(),
                    ProjectSection::AiHints => model.meta.ai_hints.clone(),
                    ProjectSection::AiNotes => model.meta.ai_notes.clone(),
                };
                let path = std::env::temp_dir().join("coffeetable_project_meta.md");
                if let Ok(mut view) = EditorView::from_content(path, initial) {
                    view.wrap_mode = crate::views::editor::WrapMode::Hard(80);
                    model.editor = Some(view);
                    model.editing_section = Some(section);
                }
            }
            ProjectSelection::NewFeature => {
                model.feature_form = Some(FeatureForm::for_new());
            }
            ProjectSelection::Feature(i) => {
                if let Some(feature) = model.features.get(i) {
                    model.feature_form = Some(FeatureForm::for_existing(feature));
                }
            }
        }
    }

    fn project_save_edit(&mut self) -> Result<()> {
        use crate::views::project_view::ProjectSelection;
        let Some(project) = self.active_project().cloned() else { return Ok(()) };
        let has_form = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .map(|m| m.feature_form.is_some())
            .unwrap_or(false);
        if has_form {
            return self.feature_form_save(project.id);
        }
        let (text, section) = {
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            let editor = match model.editor.take() {
                Some(e) => e,
                None => return Ok(()),
            };
            let text = editor_text(&editor);
            let section = model.editing_section.take();
            (text, section)
        };
        if let Some(s) = section {
            use crate::views::project_view::ProjectSection;
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            if !matches!(model.selection, ProjectSelection::Meta(_)) {
                return Ok(());
            }
            match s {
                ProjectSection::About => model.meta.description = text.clone(),
                ProjectSection::Conventions => model.meta.conventions = text.clone(),
                ProjectSection::AiHints => model.meta.ai_hints = text.clone(),
                ProjectSection::AiNotes => model.meta.ai_notes = text.clone(),
            }
            let meta = model.meta.clone();
            self.db.save_project_meta(project.id, &meta)?;
            self.status = "Saved".into();
        }
        Ok(())
    }

    fn feature_form_save(&mut self, project_id: i64) -> Result<()> {
        if let Some(form) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
            .and_then(|m| m.feature_form.as_mut())
        {
            if form.editor_open() {
                form.commit_editor();
            }
            let _ = form.commit_new_step();
            let _ = form.commit_new_comment();
        }
        let form = {
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            match model.feature_form.take() {
                Some(f) => f,
                None => return Ok(()),
            }
        };
        let feature_id = self.persist_feature_form(project_id, &form)?;
        self.reload_project_view()?;
        self.focus_feature_row(feature_id);
        self.status = "Feature saved".into();
        Ok(())
    }

    fn persist_feature_form(
        &mut self,
        project_id: i64,
        form: &crate::views::feature_form::FeatureForm,
    ) -> Result<i64> {
        let title = if form.title.trim().is_empty() {
            "Untitled feature".to_string()
        } else {
            form.title.clone()
        };
        let feature_id = match form.feature_id {
            Some(id) => {
                self.db
                    .update_feature(id, &title, &form.description, form.status)?;
                id
            }
            None => self.db.insert_feature(project_id, &title)?,
        };
        if form.feature_id.is_none() {
            self.db
                .update_feature(feature_id, &title, &form.description, form.status)?;
        }
        for step in &form.steps {
            match step.id {
                Some(id) if step.deleted => self.db.delete_step(id)?,
                Some(id) => self.db.update_step(id, &step.summary, step.status)?,
                None if !step.deleted && !step.summary.trim().is_empty() => {
                    let new_id = self.db.insert_step(feature_id, &step.summary)?;
                    if step.status != crate::project::StepStatus::Todo {
                        self.db.update_step(new_id, &step.summary, step.status)?;
                    }
                }
                None => {}
            }
        }
        for comment in &form.comments {
            match comment.id {
                Some(id) if comment.deleted => self.db.delete_comment(id)?,
                Some(id) => {
                    self.db
                        .update_comment(id, &comment.message, comment.status)?;
                    self.db.update_comment_kind(id, comment.kind)?;
                }
                None if !comment.deleted && !comment.message.trim().is_empty() => {
                    let new_id = self
                        .db
                        .insert_comment_with_kind(feature_id, &comment.message, comment.kind)?;
                    if comment.status != crate::project::CommentStatus::Queued {
                        self.db
                            .update_comment(new_id, &comment.message, comment.status)?;
                    }
                }
                None => {}
            }
        }
        Ok(feature_id)
    }

    fn focus_feature_row(&mut self, feature_id: i64) {
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            if let Some(pos) = model.features.iter().position(|f| f.id == feature_id) {
                let row = crate::views::project_view::ProjectSection::all().len() + 1 + pos;
                model.list_state.select(Some(row));
                model.sync_selection_from_list();
            }
        }
    }

    fn project_close_form(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else { return Ok(()) };
        let dirty = self
            .active_state_ref()
            .and_then(|s| s.project_view.as_ref())
            .and_then(|m| m.feature_form.as_ref())
            .map(|f| {
                f.dirty
                    || f.editor_open()
                    || !f.new_step_buf.trim().is_empty()
                    || !f.new_comment_buf.trim().is_empty()
            })
            .unwrap_or(false);
        if dirty {
            self.feature_form_save(project.id)?;
        } else if let Some(state) = self.active_state() {
            if let Some(model) = state.project_view.as_mut() {
                model.feature_form = None;
            }
        }
        Ok(())
    }

    fn project_add_feature(&mut self) -> Result<()> {
        use crate::views::project_view::{ProjectSection, ProjectSelection};
        let sections = ProjectSection::all().len();
        if let Some(model) = self
            .active_state()
            .and_then(|s| s.project_view.as_mut())
        {
            model.list_state.select(Some(sections));
            model.selection = ProjectSelection::NewFeature;
        }
        self.project_begin_edit();
        Ok(())
    }

    fn project_cycle_status(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else { return Ok(()) };
        let payload = {
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            let Some(i) = (match model.selection {
                crate::views::project_view::ProjectSelection::Feature(i) => Some(i),
                _ => None,
            }) else {
                return Ok(());
            };
            let Some(feature) = model.features.get_mut(i) else { return Ok(()) };
            feature.status = feature.status.next();
            (
                feature.id,
                feature.title.clone(),
                feature.description.clone(),
                feature.status,
            )
        };
        self.db
            .update_feature(payload.0, &payload.1, &payload.2, payload.3)?;
        let _ = project;
        Ok(())
    }

    fn project_delete_selected(&mut self) -> Result<()> {
        use crate::views::project_view::ProjectSelection;
        let target = {
            let Some(state) = self.active_state() else { return Ok(()) };
            let Some(model) = state.project_view.as_mut() else { return Ok(()) };
            match model.selection {
                ProjectSelection::Feature(i) => model
                    .features
                    .get(i)
                    .map(|f| (f.id, f.title.clone())),
                _ => None,
            }
        };
        if let Some((id, title)) = target {
            self.pending_delete_feature = Some((id, title));
            self.mode = AppMode::ConfirmDeleteFeature;
        }
        Ok(())
    }

    fn confirm_delete_feature(&mut self) -> Result<()> {
        if let Some((id, _)) = self.pending_delete_feature.take() {
            self.db.delete_feature(id)?;
            self.reload_project_view()?;
            self.status = "Feature deleted".into();
        }
        self.mode = AppMode::Normal;
        Ok(())
    }

    fn cancel_delete_feature(&mut self) {
        self.pending_delete_feature = None;
        self.mode = AppMode::Normal;
        self.status = "Delete cancelled".into();
    }

    fn reload_project_view(&mut self) -> Result<()> {
        let Some(project) = self.active_project().cloned() else { return Ok(()) };
        let meta = self.db.load_project_meta(project.id)?;
        let features = self.db.list_features(project.id)?;
        let has_agents = self
            .active_state_ref()
            .map(|s| !s.agents.is_empty())
            .unwrap_or(false);
        if let Some(state) = self.active_state() {
            if let Some(model) = state.project_view.as_mut() {
                let prev_sel = model.list_state.selected();
                model.meta = meta.clone();
                model.features = features.clone();
                let total = model.rows();
                if let Some(idx) = prev_sel {
                    model.list_state.select(Some(idx.min(total.saturating_sub(1))));
                }
                model.sync_selection_from_list();
            }
        }
        if has_agents {
            let dir = self.paths.project_context_dir(project.id);
            let _ = Self::write_context_files(&dir, &project.name, &meta, &features);
        }
        Ok(())
    }

    fn write_to_active_terminal(&mut self, bytes: &[u8]) {
        if let Some(state) = self.active_state() {
            if let Some(i) = state.active_terminal {
                if let Some(term) = state.terminals.get_mut(i) {
                    term.clear_selection();
                    term.reset_scrollback();
                    term.write_bytes(bytes);
                }
            }
        }
    }

    fn paste_into_active_terminal(&mut self) {
        let Some(text) = crate::clipboard::paste() else {
            self.status = "Clipboard is empty".into();
            return;
        };
        if text.is_empty() {
            self.status = "Clipboard is empty".into();
            return;
        }
        let bytes = bracketed_paste_bytes(&text);
        self.write_to_active_terminal(&bytes);
        self.status = format!("Pasted {} chars", text.chars().count());
    }

    fn open_explorer_filter(&mut self) {
        if self.active_state().is_none() {
            return;
        }
        if let Some(state) = self.active_state() {
            state.left_pane = LeftPaneMode::Tree;
            state.focus = Focus::Tree;
            state.tree.set_filter(String::new());
        }
        self.mode = AppMode::ExplorerFilter;
    }

    fn open_grep(&mut self) {
        if let Some(p) = self.active_project().cloned() {
            let excludes = self.effective_search_excludes(p.id);
            self.grep = Some(GrepView::new(p.path, excludes));
            self.mode = AppMode::Grep;
        }
    }

    fn next_project(&mut self) {
        if self.open_projects.is_empty() {
            return;
        }
        self.active_index = (self.active_index + 1) % self.open_projects.len();
        let _ = self.persist_open_projects();
    }

    fn prev_project(&mut self) {
        if self.open_projects.is_empty() {
            return;
        }
        if self.active_index == 0 {
            self.active_index = self.open_projects.len() - 1;
        } else {
            self.active_index -= 1;
        }
        let _ = self.persist_open_projects();
    }

    fn persist_active_tree(&self) -> Result<()> {
        if let Some(p) = self.open_projects.get(self.active_index) {
            if let Some(s) = self.project_views.get(&p.id) {
                self.db.save_file_tree_state(p.id, &s.tree.snapshot_state())?;
            }
        }
        Ok(())
    }

    fn persist_open_projects(&self) -> Result<()> {
        let ids: Vec<i64> = self.open_projects.iter().map(|p| p.id).collect();
        let active = self.open_projects.get(self.active_index).map(|p| p.id);
        self.db.save_open_projects(&ids, active)?;
        Ok(())
    }

    pub fn persist_all(&self) -> Result<()> {
        self.persist_open_projects()?;
        for p in &self.open_projects {
            if let Some(s) = self.project_views.get(&p.id) {
                self.db.save_file_tree_state(p.id, &s.tree.snapshot_state())?;
            }
        }
        Ok(())
    }
}

fn editor_text(editor: &EditorView) -> String {
    editor
        .lines
        .iter()
        .map(|l| l.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_review_state(message: String) -> AiCommitState {
    let path = std::env::temp_dir().join("coffeetable_commit_msg.txt");
    match EditorView::from_content(path, message.clone()) {
        Ok(view) => AiCommitState::Reviewing { editor: view },
        Err(_) => AiCommitState::Error(format!("Editor init failed for: {}", message)),
    }
}

fn build_plan_state(plans: Vec<crate::ai::CommitPlan>) -> AiCommitState {
    let mut messages = Vec::with_capacity(plans.len());
    let mut files = Vec::with_capacity(plans.len());
    for (i, plan) in plans.into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!("coffeetable_plan_{}.txt", i));
        match EditorView::from_content(path, plan.message) {
            Ok(view) => {
                messages.push(view);
                files.push(plan.files);
            }
            Err(e) => {
                return AiCommitState::Error(format!("Editor init failed: {}", e));
            }
        }
    }
    if messages.is_empty() {
        return AiCommitState::Error("AI returned no commits".into());
    }
    AiCommitState::ReviewingPlan {
        messages,
        files,
        current: 0,
    }
}

enum BrowseEnter {
    OpenSaved(Project),
    AddDiscovered { name: String, path: PathBuf },
    None,
}

fn prettify_diff(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut header_done = false;
    for line in raw.lines() {
        if !header_done {
            if line.starts_with("@@") {
                header_done = true;
            } else {
                continue;
            }
        }
        if let Some(formatted) = format_hunk_header(line) {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(formatted);
            continue;
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

fn format_hunk_header(line: &str) -> Option<String> {
    let rest = line.strip_prefix("@@")?;
    let (range, context) = rest.split_once("@@")?;
    let parts: Vec<&str> = range.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let old = parts[0].trim_start_matches('-');
    let new = parts[1].trim_start_matches('+');
    let old_label = format_range(old, '-');
    let new_label = format_range(new, '+');
    let mut header = format!("─── {}  →  {} ───", old_label, new_label);
    let context = context.trim();
    if !context.is_empty() {
        header.push_str("   ");
        header.push_str(context);
    }
    Some(header)
}

fn format_range(part: &str, sign: char) -> String {
    let (start, count) = match part.split_once(',') {
        Some((s, c)) => (s, c.parse::<usize>().unwrap_or(0)),
        None => (part, 1),
    };
    let start_n = start.parse::<usize>().unwrap_or(0);
    if count == 0 {
        format!("{} line {}", sign, start_n)
    } else if count == 1 {
        format!("{} L{}", sign, start_n)
    } else {
        let end = start_n + count.saturating_sub(1);
        format!("{} L{}-{}", sign, start_n, end)
    }
}

fn display_relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| {
            path.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        })
}

fn unique_target_path(dir: &Path, name: &str) -> PathBuf {
    let base = dir.join(name);
    if !base.exists() {
        return base;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)),
        _ => (name.to_string(), String::new()),
    };
    for i in 1..1000 {
        let candidate = dir.join(format!("{} copy{}{}", stem, if i == 1 { String::new() } else { format!(" {}", i) }, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{} copy.{}", stem, std::process::id()))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn bracketed_paste_bytes(text: &str) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::with_capacity(normalized.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(normalized.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

fn git_first_line(text: &str, fallback: &str) -> String {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.is_empty() {
        fallback.to_string()
    } else {
        first.to_string()
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or(text).to_string()
}

fn match_runtime_keyword(input: &str, keyword: &str) -> Option<String> {
    let rest = input.strip_prefix(keyword)?;
    if rest.is_empty() {
        return Some(String::new());
    }
    if rest.starts_with(char::is_whitespace) {
        return Some(rest.trim().to_string());
    }
    None
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let ca = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

pub fn preview_worktree_path(repo: &Path, branch: &str) -> PathBuf {
    derive_worktree_path(repo, branch)
}

fn derive_worktree_path(repo: &Path, branch: &str) -> PathBuf {
    let parent = repo.parent().unwrap_or(repo);
    let base = repo
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let safe: String = branch
        .chars()
        .map(|c| match c {
            '/' | '\\' | ' ' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            other => other,
        })
        .collect();
    parent.join(format!("{}-{}", base, safe))
}

fn format_feature_agent_name(feature_id: i64, feature_title: &str) -> String {
    const MAX: usize = 28;
    let title = feature_title.trim();
    let prefix = format!("#{} ", feature_id);
    let room = MAX.saturating_sub(prefix.chars().count());
    let chars: Vec<char> = title.chars().collect();
    let trimmed_title: String = if chars.len() <= room {
        chars.iter().collect()
    } else if room <= 1 {
        "…".to_string()
    } else {
        let mut s: String = chars[..room - 1].iter().collect();
        s.push('…');
        s
    };
    format!("{}{}", prefix, trimmed_title)
}

fn contains(rect: Rect, col: u16, row: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && col >= rect.x
        && col < rect.x + rect.width
        && row >= rect.y
        && row < rect.y + rect.height
}

fn discover_new(roots: &[PathBuf], saved: &[Project]) -> Vec<(String, PathBuf)> {
    let saved_keys: HashSet<String> = saved.iter().map(|p| discovery::canon_key(&p.path)).collect();
    let mut out = Vec::new();
    for path in discovery::scan_roots(roots) {
        if saved_keys.contains(&discovery::canon_key(&path)) {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push((name, path));
    }
    out
}
