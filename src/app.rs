use crate::{
    config::{self, Settings},
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
    path::PathBuf,
};

pub enum AppMode {
    Normal,
    Picker,
    Grep,
    OpenConfirm,
    Palette,
    ExplorerFilter,
    AiCommit,
}

pub enum AiCommitState {
    Loading {
        rx: std::sync::mpsc::Receiver<Result<String, String>>,
        spinner: usize,
    },
    Reviewing {
        message: String,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftPaneMode {
    Tree,
    Changes,
}

pub struct ProjectViewState {
    pub tree: FileTreeView,
    pub changes: ChangesView,
    pub left_pane: LeftPaneMode,
    pub editor: Option<EditorView>,
    pub focus: Focus,
    pub preferred_git_view: GitView,
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
    pub roots: Vec<PathBuf>,
    pub search_excludes: Vec<String>,
    pub ai_config: crate::config::AiConfig,
    pub open_projects: Vec<Project>,
    pub active_index: usize,
    pub project_views: HashMap<i64, ProjectViewState>,
    pub mode: AppMode,
    pub picker: Option<ProjectPicker>,
    pub grep: Option<GrepView>,
    pub palette: Option<PaletteState>,
    pub ai_commit: Option<AiCommitOverlay>,
    pub pending_open: Option<PendingOpen>,
    pub should_quit: bool,
    pub status: String,
    pub help_visible: bool,
    pub leader_pending: bool,
    pub tabs_area: Rect,
    pub tab_rects: Vec<Rect>,
    pub left_pane_area: Rect,
    pub right_pane_area: Rect,
}

impl App {
    pub fn new(db: Db, settings_path: PathBuf) -> Result<Self> {
        let settings = Settings::load_or_seed(&settings_path)?;
        let roots = settings.roots;
        let search_excludes = settings.search_excludes;
        let ai_config = settings.ai;
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
        for p in &open_projects {
            let state = db.load_file_tree_state(p.id)?;
            let status = git::fetch_status(&p.path);
            let mut tree = FileTreeView::new(p.path.clone(), state)?;
            tree.set_git_status(status.clone());
            let mut changes = ChangesView::new(p.path.clone());
            changes.set_status(&status);
            project_views.insert(
                p.id,
                ProjectViewState {
                    tree,
                    changes,
                    left_pane: LeftPaneMode::Tree,
                    editor: None,
                    focus: Focus::Tree,
                    preferred_git_view: GitView::Working,
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
            roots,
            search_excludes,
            ai_config,
            open_projects,
            active_index,
            project_views,
            mode,
            picker,
            grep: None,
            palette: None,
            ai_commit: None,
            pending_open: None,
            should_quit: false,
            status: String::new(),
            help_visible: false,
            leader_pending: false,
            tabs_area: Rect::default(),
            tab_rects: Vec::new(),
            left_pane_area: Rect::default(),
            right_pane_area: Rect::default(),
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
        if self.is_help_key(key) {
            self.help_visible = true;
            return Ok(());
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.copy_current_context();
            return Ok(());
        }
        if key.code == KeyCode::Char(' ')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && self.should_activate_leader()
        {
            self.leader_pending = true;
            return Ok(());
        }
        if key.code == KeyCode::Char(':')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && self.should_open_palette()
        {
            self.open_palette();
            return Ok(());
        }
        match self.mode {
            AppMode::Picker => self.on_key_picker(key)?,
            AppMode::Grep => self.on_key_grep(key)?,
            AppMode::OpenConfirm => self.on_key_open_confirm(key)?,
            AppMode::Palette => self.on_key_palette(key)?,
            AppMode::ExplorerFilter => self.on_key_explorer_filter(key)?,
            AppMode::AiCommit => self.on_key_ai_commit(key)?,
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
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
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
        };
        s.save(&self.settings_path)
    }

    fn refresh_git_status(&mut self) -> Result<()> {
        let Some(project) = self.open_projects.get(self.active_index).cloned() else {
            return Ok(());
        };
        let status = git::fetch_status(&project.path);
        if let Some(state) = self.project_views.get_mut(&project.id) {
            state.tree.set_git_status(status.clone());
            state.changes.set_status(&status);
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

        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_tabs {
                    if let Some(i) = self.tab_at(col, row) {
                        self.active_index = i;
                        let _ = self.persist_open_projects();
                    }
                } else if in_left {
                    self.click_left_pane(col, row)?;
                } else if in_right {
                    self.click_right_pane(col, row);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if in_right {
                    if let Some(s) = self.active_state() {
                        if let Some(e) = s.editor.as_mut() {
                            e.mouse_drag(col, row);
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if in_right {
                    if let Some(s) = self.active_state() {
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

    fn click_right_pane(&mut self, col: u16, row: u16) {
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
            if let Some(s) = self.active_state() {
                s.focus = Focus::Editor;
            }
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
                s.focus = Focus::Editor;
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
            | AppMode::Palette
            | AppMode::ExplorerFilter
            | AppMode::AiCommit => false,
            AppMode::Normal => {
                if let Some(state) = self.active_state_ref() {
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

    fn handle_global_normal(&mut self, key: KeyEvent) -> Result<bool> {
        match (key.code, key.modifiers) {
            (KeyCode::Tab, _) => {
                self.next_project();
                Ok(true)
            }
            (KeyCode::BackTab, _) => {
                self.prev_project();
                Ok(true)
            }
            _ => Ok(false),
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
    }

    fn poll_ai_commit(&mut self) {
        let Some(overlay) = self.ai_commit.as_mut() else { return };
        let new_state = if let AiCommitState::Loading { rx, spinner } = &mut overlay.state {
            *spinner = spinner.wrapping_add(1);
            match rx.try_recv() {
                Ok(Ok(msg)) => Some(AiCommitState::Reviewing { message: msg }),
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
        let Some(diff) = git::staged_diff(&project.path) else {
            self.status = "git diff --staged failed".into();
            return Ok(());
        };
        if diff.trim().is_empty() {
            self.status = "Nothing staged — stage changes first (c on a file)".into();
            return Ok(());
        }
        let provider = match crate::ai::build_provider(&self.ai_config) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("AI provider error: {}", e);
                return Ok(());
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = provider
                .generate_commit_message(&diff)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
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
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.ai_commit = None;
                self.mode = AppMode::Normal;
                self.status = "AI commit cancelled".into();
            }
            (KeyCode::Char('y'), _) | (KeyCode::Enter, _) => {
                if let AiCommitState::Reviewing { message } = &overlay.state {
                    let msg = message.clone();
                    let project_path = overlay.project_path.clone();
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    match git::commit_with_message(&project_path, &msg) {
                        Ok(_) => {
                            let first_line =
                                msg.lines().next().unwrap_or("").to_string();
                            self.status = format!("Committed: {}", first_line);
                            let _ = self.refresh_git_status();
                        }
                        Err(e) => {
                            self.status = format!("git commit failed: {}", e.lines().next().unwrap_or(""))
                        }
                    }
                }
            }
            (KeyCode::Char('r'), _) => {
                if matches!(
                    overlay.state,
                    AiCommitState::Reviewing { .. } | AiCommitState::Error(_)
                ) {
                    self.ai_commit = None;
                    self.mode = AppMode::Normal;
                    self.start_ai_commit()?;
                }
            }
            _ => {}
        }
        Ok(())
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
        let path = self.settings_path.clone();
        self.open_file_in_editor(path)
    }

    fn reload_settings_from_disk(&mut self) {
        match Settings::load_or_seed(&self.settings_path) {
            Ok(s) => {
                self.roots = s.roots;
                self.search_excludes = s.search_excludes;
                self.status = "Settings reloaded".into();
            }
            Err(e) => {
                self.status = format!("Settings reload failed: {}", e);
            }
        }
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
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => picker.push_char(c),
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
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => picker.push_char(c),
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
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
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
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => grep.push_char(c),
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
            self.project_views.insert(
                project.id,
                ProjectViewState {
                    tree,
                    changes,
                    left_pane: LeftPaneMode::Tree,
                    editor: None,
                    focus: Focus::Tree,
                    preferred_git_view: GitView::Working,
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
            self.grep = Some(GrepView::new(p.path, self.search_excludes.clone()));
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
