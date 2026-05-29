use crate::{git::GitStatus, project::FileTreeState};
use anyhow::Result;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub struct FileTreeView {
    pub root: PathBuf,
    expanded: HashSet<PathBuf>,
    cache: HashMap<PathBuf, Vec<DirEntry>>,
    visible: Vec<VisibleNode>,
    list_state: ListState,
    selected_path: Option<PathBuf>,
    git_status: HashMap<PathBuf, GitStatus>,
    pub last_render_area: Option<Rect>,
    pub filter: String,
}

#[derive(Clone)]
struct DirEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

#[derive(Clone)]
struct VisibleNode {
    path: PathBuf,
    name: String,
    depth: u16,
    is_dir: bool,
    is_expanded: bool,
}

impl FileTreeView {
    pub fn new(root: PathBuf, state: FileTreeState) -> Result<Self> {
        let mut view = Self {
            root: root.clone(),
            expanded: state.expanded.into_iter().collect(),
            cache: HashMap::new(),
            visible: Vec::new(),
            list_state: ListState::default(),
            selected_path: state.selected_path,
            git_status: HashMap::new(),
            last_render_area: None,
            filter: String::new(),
        };
        view.expanded.insert(view.root.clone());
        view.rebuild_visible();
        view.restore_selection();
        Ok(view)
    }

    pub fn set_git_status(&mut self, status: HashMap<PathBuf, GitStatus>) {
        self.git_status = status;
    }

    pub fn git_status_for(&self, path: &Path) -> Option<GitStatus> {
        self.git_status.get(path).copied()
    }

    pub fn set_filter(&mut self, query: String) {
        let prev_selected = self.selected_path.clone();
        self.filter = query;
        self.rebuild_visible();
        if let Some(p) = prev_selected {
            self.reselect(&p);
        } else if !self.visible.is_empty() {
            self.list_state.select(Some(0));
            self.selected_path = Some(self.visible[0].path.clone());
        }
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.rebuild_visible();
        self.restore_selection();
    }

    pub fn mouse_select(&mut self, row: u16) -> Action {
        let Some(area) = self.last_render_area else {
            return Action::None;
        };
        if row <= area.y || row >= area.y + area.height.saturating_sub(1) {
            return Action::None;
        }
        let local = (row - area.y - 1) as usize;
        let idx = self.list_state.offset() + local;
        if idx >= self.visible.len() {
            return Action::None;
        }
        let node = self.visible[idx].clone();
        self.list_state.select(Some(idx));
        self.selected_path = Some(node.path.clone());
        if node.is_dir {
            if self.expanded.contains(&node.path) {
                self.expanded.remove(&node.path);
            } else {
                self.expanded.insert(node.path.clone());
            }
            self.rebuild_visible();
            self.reselect(&node.path);
            Action::None
        } else {
            Action::OpenFile(node.path)
        }
    }

    pub fn mouse_scroll(&mut self, delta: i32) {
        let steps = delta.unsigned_abs() as usize;
        for _ in 0..steps {
            if delta > 0 {
                self.move_down();
            } else {
                self.move_up();
            }
        }
    }

    pub fn snapshot_state(&self) -> FileTreeState {
        FileTreeState {
            selected_path: self.selected_path.clone(),
            expanded: self.expanded.iter().cloned().collect(),
            scroll_offset: self.list_state.offset(),
        }
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_path.as_deref()
    }

    pub fn move_down(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1).min(self.visible.len() - 1);
        self.list_state.select(Some(next));
        self.selected_path = Some(self.visible[next].path.clone());
    }

    pub fn move_up(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = i.saturating_sub(1);
        self.list_state.select(Some(next));
        self.selected_path = Some(self.visible[next].path.clone());
    }

    pub fn jump_top(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.list_state.select(Some(0));
        self.selected_path = Some(self.visible[0].path.clone());
    }

    pub fn jump_bottom(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        self.list_state.select(Some(last));
        self.selected_path = Some(self.visible[last].path.clone());
    }

    pub fn toggle_or_open(&mut self) -> Action {
        let Some(i) = self.list_state.selected() else {
            return Action::None;
        };
        let Some(node) = self.visible.get(i).cloned() else {
            return Action::None;
        };
        if node.is_dir {
            if self.expanded.contains(&node.path) {
                self.expanded.remove(&node.path);
            } else {
                self.expanded.insert(node.path.clone());
            }
            self.rebuild_visible();
            self.reselect(&node.path);
            Action::None
        } else {
            Action::OpenFile(node.path)
        }
    }

    pub fn collapse_or_parent(&mut self) {
        let Some(i) = self.list_state.selected() else { return };
        let Some(node) = self.visible.get(i).cloned() else { return };
        if node.is_dir && self.expanded.contains(&node.path) && node.path != self.root {
            self.expanded.remove(&node.path);
            self.rebuild_visible();
            self.reselect(&node.path);
            return;
        }
        if let Some(parent) = node.path.parent() {
            if parent.starts_with(&self.root) || parent == self.root {
                let parent = parent.to_path_buf();
                self.reselect(&parent);
            }
        }
    }

    pub fn reveal_path(&mut self, path: &Path) {
        if !path.starts_with(&self.root) {
            return;
        }
        let mut current = path.parent();
        while let Some(parent) = current {
            if parent == self.root {
                self.expanded.insert(parent.to_path_buf());
                break;
            }
            if !parent.starts_with(&self.root) {
                break;
            }
            self.expanded.insert(parent.to_path_buf());
            current = parent.parent();
        }
        self.rebuild_visible();
        self.reselect(path);
    }

    fn reselect(&mut self, path: &Path) {
        if let Some(i) = self.visible.iter().position(|n| n.path == path) {
            self.list_state.select(Some(i));
            self.selected_path = Some(path.to_path_buf());
        } else if !self.visible.is_empty() {
            self.list_state.select(Some(0));
            self.selected_path = Some(self.visible[0].path.clone());
        }
    }

    fn restore_selection(&mut self) {
        if let Some(p) = self.selected_path.clone() {
            if let Some(i) = self.visible.iter().position(|n| n.path == p) {
                self.list_state.select(Some(i));
                return;
            }
        }
        if !self.visible.is_empty() {
            self.list_state.select(Some(0));
            self.selected_path = Some(self.visible[0].path.clone());
        }
    }

    fn rebuild_visible(&mut self) {
        self.visible.clear();
        let root = self.root.clone();
        if self.filter.is_empty() {
            self.push_children(&root, 0);
        } else {
            let q = self.filter.to_lowercase();
            self.push_filtered(&root, 0, &q);
        }
    }

    fn push_children(&mut self, dir: &Path, depth: u16) {
        let entries = self.load_dir(dir);
        for e in entries {
            let is_expanded = e.is_dir && self.expanded.contains(&e.path);
            self.visible.push(VisibleNode {
                path: e.path.clone(),
                name: e.name.clone(),
                depth,
                is_dir: e.is_dir,
                is_expanded,
            });
            if is_expanded {
                let child = e.path.clone();
                self.push_children(&child, depth + 1);
            }
        }
    }

    fn push_filtered(&mut self, dir: &Path, depth: u16, query: &str) {
        let entries = self.load_dir(dir);
        for e in entries {
            if e.is_dir {
                if !self.subtree_has_match(&e.path, query) {
                    continue;
                }
                self.visible.push(VisibleNode {
                    path: e.path.clone(),
                    name: e.name.clone(),
                    depth,
                    is_dir: true,
                    is_expanded: true,
                });
                let child = e.path.clone();
                self.push_filtered(&child, depth + 1, query);
            } else if e.name.to_lowercase().contains(query) {
                self.visible.push(VisibleNode {
                    path: e.path.clone(),
                    name: e.name.clone(),
                    depth,
                    is_dir: false,
                    is_expanded: false,
                });
            }
        }
    }

    fn subtree_has_match(&mut self, dir: &Path, query: &str) -> bool {
        let entries = self.load_dir(dir);
        for e in entries {
            if e.is_dir {
                if self.subtree_has_match(&e.path, query) {
                    return true;
                }
            } else if e.name.to_lowercase().contains(query) {
                return true;
            }
        }
        false
    }

    fn load_dir(&mut self, dir: &Path) -> Vec<DirEntry> {
        if let Some(cached) = self.cache.get(dir) {
            return cached.clone();
        }
        let mut entries: Vec<DirEntry> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|r| r.ok())
                .filter_map(|d| {
                    let name = d.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') && name != ".github" {
                        return None;
                    }
                    let path = d.path();
                    let is_dir = d.file_type().ok()?.is_dir();
                    Some(DirEntry { name, path, is_dir })
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        self.cache.insert(dir.to_path_buf(), entries.clone());
        entries
    }
}

pub enum Action {
    None,
    OpenFile(PathBuf),
}

fn node_status(
    status: &HashMap<PathBuf, GitStatus>,
    path: &Path,
    is_dir: bool,
) -> Option<GitStatus> {
    if is_dir {
        let mut best: Option<GitStatus> = None;
        for (p, s) in status {
            if p.starts_with(path) {
                best = Some(match (best, *s) {
                    (None, s) => s,
                    (Some(GitStatus::Untracked), _) => GitStatus::Untracked,
                    (_, GitStatus::Untracked) => GitStatus::Untracked,
                    (Some(GitStatus::Modified), _) => GitStatus::Modified,
                    (_, GitStatus::Modified) => GitStatus::Modified,
                    (Some(GitStatus::Staged), GitStatus::Staged) => GitStatus::Staged,
                });
            }
        }
        best
    } else {
        status.get(path).copied()
    }
}

pub struct FileTreeWidget<'a> {
    pub view: &'a mut FileTreeView,
    pub title: String,
    pub focused: bool,
}

impl<'a> Widget for FileTreeWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.view.last_render_area = Some(area);
        let items: Vec<ListItem> = self
            .view
            .visible
            .iter()
            .map(|n| {
                let indent = "  ".repeat(n.depth as usize);
                let marker = if n.is_dir {
                    if n.is_expanded { "▾ " } else { "▸ " }
                } else {
                    "  "
                };
                let status = node_status(&self.view.git_status, &n.path, n.is_dir);
                let style = match (n.is_dir, status) {
                    (_, Some(GitStatus::Untracked)) => Style::default().fg(Color::Red),
                    (_, Some(GitStatus::Modified)) => Style::default().fg(Color::Yellow),
                    (_, Some(GitStatus::Staged)) => Style::default().fg(Color::Green),
                    (true, None) => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    (false, None) => Style::default(),
                };
                let style = if n.is_dir {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                };
                ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::raw(marker),
                    Span::styled(n.name.clone(), style),
                ]))
            })
            .collect();
        let border_style = if self.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(self.title),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        StatefulWidget::render(list, area, buf, &mut self.view.list_state);
    }
}
