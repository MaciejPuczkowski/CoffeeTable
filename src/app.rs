use crate::{
    config,
    db::Db,
    discovery, git,
    project::Project,
    views::{
        changes::ChangesView,
        editor::{EditorMode, EditorRequest, EditorView},
        file_finder::FileFinder,
        file_tree::{Action as FileTreeAction, FileTreeView},
        grep::GrepView,
        project_picker::{PickerItem, PickerMode, ProjectPicker},
    },
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

pub enum AppMode {
    Normal,
    Picker,
    FileFinder,
    Grep,
    OpenConfirm,
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
    pub roots: Vec<PathBuf>,
    pub open_projects: Vec<Project>,
    pub active_index: usize,
    pub project_views: HashMap<i64, ProjectViewState>,
    pub mode: AppMode,
    pub picker: Option<ProjectPicker>,
    pub file_finder: Option<FileFinder>,
    pub grep: Option<GrepView>,
    pub pending_open: Option<PendingOpen>,
    pub should_quit: bool,
    pub status: String,
    pub help_visible: bool,
    pub leader_pending: bool,
}

impl App {
    pub fn new(db: Db) -> Result<Self> {
        let roots = db.load_roots_or_seed(config::DEFAULT_ROOTS)?;
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
            roots,
            open_projects,
            active_index,
            project_views,
            mode,
            picker,
            file_finder: None,
            grep: None,
            pending_open: None,
            should_quit: false,
            status: String::new(),
            help_visible: false,
            leader_pending: false,
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
            self.should_quit = true;
            return Ok(());
        }
        if key.code == KeyCode::Char(' ')
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && self.should_activate_leader()
        {
            self.leader_pending = true;
            return Ok(());
        }
        match self.mode {
            AppMode::Picker => self.on_key_picker(key)?,
            AppMode::FileFinder => self.on_key_file_finder(key)?,
            AppMode::Grep => self.on_key_grep(key)?,
            AppMode::OpenConfirm => self.on_key_open_confirm(key)?,
            AppMode::Normal => self.on_key_normal(key)?,
        }
        Ok(())
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
                Some(EditorMode::Insert)
                | Some(EditorMode::Command)
                | Some(EditorMode::Search) => false,
                _ => true,
            },
        }
    }

    fn handle_leader_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Char('p') => self.open_picker()?,
            KeyCode::Char('f') => self.open_file_finder(),
            KeyCode::Char('g') => self.open_grep(),
            KeyCode::Char('w') => self.toggle_focus(),
            KeyCode::Char('e') => self.focus_tree(),
            KeyCode::Char('b') => self.focus_editor(),
            KeyCode::Char('c') => self.toggle_left_pane(),
            KeyCode::Char('?') | KeyCode::Char('h') => self.help_visible = true,
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
            AppMode::FileFinder | AppMode::Grep | AppMode::OpenConfirm => false,
            AppMode::Normal => {
                if let Some(state) = self.active_state_ref() {
                    if state.focus == Focus::Editor {
                        if let Some(e) = &state.editor {
                            if !matches!(e.mode, EditorMode::Normal | EditorMode::Visual | EditorMode::VisualLine) {
                                return false;
                            }
                            return false;
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
                    let path = self
                        .active_state_ref()
                        .and_then(|s| s.changes.selected_path().map(|p| p.to_path_buf()));
                    if let Some(p) = path {
                        if p.is_file() {
                            self.open_file_in_editor(p)?;
                        }
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
            _ => {}
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
        let close = editor.close_requested;
        let quit = editor.quit_app_requested;
        let request = editor.pending_request.take();
        let did_save = std::mem::replace(&mut editor.did_save, false);
        let status = std::mem::take(&mut editor.status);
        if !status.is_empty() {
            self.status = status;
        }
        if did_save {
            let _ = self.refresh_git_status();
        }
        if close {
            if let Some(s) = self.active_state() {
                s.editor = None;
                s.focus = Focus::Tree;
            }
        }
        if quit {
            self.should_quit = true;
        }
        if let Some(req) = request {
            match req {
                EditorRequest::OpenFinder => self.open_file_finder(),
                EditorRequest::OpenGrep => self.open_grep(),
                EditorRequest::OpenPicker => self.open_picker()?,
                EditorRequest::FocusTree => self.focus_tree(),
                EditorRequest::FocusEditor => self.focus_editor(),
                EditorRequest::ShowHelp => self.help_visible = true,
            }
        }
        Ok(())
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
        }
    }

    fn toggle_focus(&mut self) {
        if let Some(state) = self.active_state() {
            state.focus = match state.focus {
                Focus::Tree => {
                    if state.editor.is_some() {
                        Focus::Editor
                    } else {
                        Focus::Tree
                    }
                }
                Focus::Editor => Focus::Tree,
            };
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
                    self.db.save_roots(&self.roots)?;
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
                    self.db.save_roots(&self.roots)?;
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

    fn on_key_file_finder(&mut self, key: KeyEvent) -> Result<()> {
        let mut consumed = true;
        {
            let Some(finder) = self.file_finder.as_mut() else {
                return Ok(());
            };
            match (key.code, key.modifiers) {
                (KeyCode::Down, _) => finder.move_down(),
                (KeyCode::Up, _) => finder.move_up(),
                (KeyCode::Backspace, _) => finder.pop_char(),
                (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => finder.push_char(c),
                _ => consumed = false,
            }
        }
        if consumed {
            return Ok(());
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.file_finder = None;
                self.mode = AppMode::Normal;
            }
            (KeyCode::Enter, _) => {
                let path = self
                    .file_finder
                    .as_ref()
                    .and_then(|f| f.selected_path().map(|p| p.to_path_buf()));
                if let Some(p) = path {
                    self.file_finder = None;
                    self.mode = AppMode::Normal;
                    self.open_file_in_editor(p)?;
                }
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

    fn open_file_finder(&mut self) {
        if let Some(p) = self.active_project().cloned() {
            self.file_finder = Some(FileFinder::new(p.path));
            self.mode = AppMode::FileFinder;
        }
    }

    fn open_grep(&mut self) {
        if let Some(p) = self.active_project().cloned() {
            self.grep = Some(GrepView::new(p.path));
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
