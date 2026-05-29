use crate::{git::GitStatus, icons};
use std::collections::HashSet;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub struct ChangesView {
    pub root: PathBuf,
    pub items: Vec<ChangesItem>,
    pub list_state: ListState,
    pub selected_path: Option<PathBuf>,
    pub last_render_area: Option<ratatui::layout::Rect>,
    pub collapsed: HashSet<PathBuf>,
    pub last_status: HashMap<PathBuf, GitStatus>,
}

pub enum ChangesAction {
    None,
    OpenFile(PathBuf),
}

#[derive(Clone)]
pub enum ChangesItem {
    Header(String),
    Dir {
        name: String,
        depth: u16,
        path: PathBuf,
    },
    File {
        name: String,
        path: PathBuf,
        status: GitStatus,
        depth: u16,
    },
}

impl ChangesView {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            items: Vec::new(),
            list_state: ListState::default(),
            selected_path: None,
            last_render_area: None,
            collapsed: HashSet::new(),
            last_status: HashMap::new(),
        }
    }

    pub fn mouse_select(&mut self, row: u16) -> ChangesAction {
        let Some(area) = self.last_render_area else {
            return ChangesAction::None;
        };
        if row <= area.y || row >= area.y + area.height.saturating_sub(1) {
            return ChangesAction::None;
        }
        let local = (row - area.y - 1) as usize;
        let idx = self.list_state.offset() + local;
        if idx >= self.items.len() {
            return ChangesAction::None;
        }
        match self.items[idx].clone() {
            ChangesItem::File { path, .. } => {
                self.list_state.select(Some(idx));
                self.selected_path = Some(path.clone());
                ChangesAction::OpenFile(path)
            }
            ChangesItem::Dir { path, .. } => {
                self.list_state.select(Some(idx));
                self.selected_path = Some(path.clone());
                if self.collapsed.contains(&path) {
                    self.collapsed.remove(&path);
                } else {
                    self.collapsed.insert(path.clone());
                }
                self.rebuild_items();
                self.select_path(&path);
                ChangesAction::None
            }
            _ => ChangesAction::None,
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

    pub fn set_status(&mut self, status: &HashMap<PathBuf, GitStatus>) {
        self.last_status = status.clone();
        self.rebuild_items();
    }

    fn rebuild_items(&mut self) {
        let prev = self.selected_path.clone();
        let mut staged: Vec<(PathBuf, GitStatus)> = Vec::new();
        let mut unstaged: Vec<(PathBuf, GitStatus)> = Vec::new();
        let mut counts = SummaryCounts::default();
        for (p, s) in &self.last_status {
            match s {
                GitStatus::Staged => {
                    staged.push((p.clone(), *s));
                    counts.staged += 1;
                }
                GitStatus::Modified => {
                    unstaged.push((p.clone(), *s));
                    counts.modified += 1;
                }
                GitStatus::Deleted => {
                    unstaged.push((p.clone(), *s));
                    counts.deleted += 1;
                }
                GitStatus::Untracked => {
                    unstaged.push((p.clone(), *s));
                    counts.untracked += 1;
                }
            }
        }

        let root = self.root.clone();
        let mut items = Vec::new();
        items.push(ChangesItem::Header(counts.summary()));
        push_section(&mut items, "Staged", &mut staged, &root, &self.collapsed);
        push_section(&mut items, "Unstaged", &mut unstaged, &root, &self.collapsed);
        self.items = items;
        self.restore_selection(prev);
    }

    pub fn toggle_or_open(&mut self) -> ChangesAction {
        let Some(idx) = self.list_state.selected() else {
            return ChangesAction::None;
        };
        match self.items.get(idx).cloned() {
            Some(ChangesItem::Dir { path, .. }) => {
                if self.collapsed.contains(&path) {
                    self.collapsed.remove(&path);
                } else {
                    self.collapsed.insert(path.clone());
                }
                self.rebuild_items();
                self.select_path(&path);
                ChangesAction::None
            }
            Some(ChangesItem::File { path, .. }) => ChangesAction::OpenFile(path),
            _ => ChangesAction::None,
        }
    }

    fn select_path(&mut self, target: &Path) {
        if let Some(i) = self.items.iter().position(|it| match it {
            ChangesItem::File { path, .. } => path == target,
            ChangesItem::Dir { path, .. } => path == target,
            _ => false,
        }) {
            self.list_state.select(Some(i));
            self.selected_path = Some(target.to_path_buf());
        }
    }

    pub fn select_path_external(&mut self, target: &Path) {
        self.select_path(target);
    }

    fn restore_selection(&mut self, prev: Option<PathBuf>) {
        if let Some(p) = prev {
            if let Some(i) = self.items.iter().position(|it| match it {
                ChangesItem::File { path, .. } => *path == p,
                ChangesItem::Dir { path, .. } => *path == p,
                _ => false,
            }) {
                self.list_state.select(Some(i));
                self.selected_path = Some(p);
                return;
            }
        }
        if let Some(i) = self.first_selectable() {
            self.list_state.select(Some(i));
            self.selected_path = match &self.items[i] {
                ChangesItem::File { path, .. } => Some(path.clone()),
                ChangesItem::Dir { path, .. } => Some(path.clone()),
                _ => None,
            };
        } else {
            self.list_state.select(None);
            self.selected_path = None;
        }
    }

    fn first_selectable(&self) -> Option<usize> {
        self.items
            .iter()
            .position(|it| !matches!(it, ChangesItem::Header(_)))
    }
}

#[derive(Default)]
struct SummaryCounts {
    staged: usize,
    modified: usize,
    deleted: usize,
    untracked: usize,
}

impl SummaryCounts {
    fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.staged > 0 {
            parts.push(format!("{} staged", self.staged));
        }
        if self.modified > 0 {
            parts.push(format!("{} modified", self.modified));
        }
        if self.deleted > 0 {
            parts.push(format!("{} deleted", self.deleted));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }
        if parts.is_empty() {
            "no changes".into()
        } else {
            parts.join(" · ")
        }
    }
}

fn push_section(
    items: &mut Vec<ChangesItem>,
    title: &str,
    files: &mut Vec<(PathBuf, GitStatus)>,
    root: &Path,
    collapsed: &HashSet<PathBuf>,
) {
    if files.is_empty() {
        return;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    items.push(ChangesItem::Header(format!("{} ({})", title, files.len())));
    let mut current: Vec<String> = Vec::new();
    let mut path_stack: Vec<PathBuf> = Vec::new();
    for (path, status) in files.iter() {
        let rel = path.strip_prefix(root).unwrap_or(path);
        let components: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if components.is_empty() {
            continue;
        }
        let (dirs, filename) = components.split_at(components.len() - 1);
        let common = current
            .iter()
            .zip(dirs.iter())
            .take_while(|(a, b)| a == b)
            .count();
        current.truncate(common);
        path_stack.truncate(common);

        let mut current_path = if common == 0 {
            root.to_path_buf()
        } else {
            path_stack[common - 1].clone()
        };
        let mut hidden_by_collapse = path_stack.iter().any(|p| collapsed.contains(p));

        for d in dirs.iter().skip(common) {
            current_path = current_path.join(d);
            if !hidden_by_collapse {
                items.push(ChangesItem::Dir {
                    name: d.clone(),
                    depth: current.len() as u16,
                    path: current_path.clone(),
                });
            }
            current.push(d.clone());
            path_stack.push(current_path.clone());
            if collapsed.contains(&current_path) {
                hidden_by_collapse = true;
            }
        }

        if hidden_by_collapse {
            continue;
        }
        items.push(ChangesItem::File {
            name: filename[0].clone(),
            path: path.clone(),
            status: *status,
            depth: dirs.len() as u16,
        });
    }
}

impl ChangesView {
    pub fn move_down(&mut self) {
        let Some(curr) = self.list_state.selected() else {
            if let Some(i) = self.first_selectable() {
                self.list_state.select(Some(i));
                self.sync_selected();
            }
            return;
        };
        for i in (curr + 1)..self.items.len() {
            if !matches!(self.items[i], ChangesItem::Header(_)) {
                self.list_state.select(Some(i));
                self.sync_selected();
                return;
            }
        }
    }

    pub fn move_up(&mut self) {
        let Some(curr) = self.list_state.selected() else {
            return;
        };
        for i in (0..curr).rev() {
            if !matches!(self.items[i], ChangesItem::Header(_)) {
                self.list_state.select(Some(i));
                self.sync_selected();
                return;
            }
        }
    }

    pub fn jump_top(&mut self) {
        if let Some(i) = self.first_selectable() {
            self.list_state.select(Some(i));
            self.sync_selected();
        }
    }

    pub fn jump_bottom(&mut self) {
        if let Some(i) = self
            .items
            .iter()
            .rposition(|it| !matches!(it, ChangesItem::Header(_)))
        {
            self.list_state.select(Some(i));
            self.sync_selected();
        }
    }

    fn sync_selected(&mut self) {
        self.selected_path = self
            .list_state
            .selected()
            .and_then(|i| self.items.get(i))
            .and_then(|it| match it {
                ChangesItem::File { path, .. } => Some(path.clone()),
                ChangesItem::Dir { path, .. } => Some(path.clone()),
                _ => None,
            });
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_path.as_deref()
    }
}

pub struct ChangesWidget<'a> {
    pub view: &'a mut ChangesView,
    pub title: String,
    pub focused: bool,
}

impl<'a> Widget for ChangesWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.view.last_render_area = Some(area);
        let border_style = if self.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let title = format!(" Changes — {} ", self.title);
        let header_style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);
        let row_width = area.width.saturating_sub(4) as usize;
        let items: Vec<ListItem> = if self.view.items.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "  (no changes)",
                Style::default().fg(Color::DarkGray),
            )))]
        } else {
            self.view
                .items
                .iter()
                .map(|it| match it {
                    ChangesItem::Header(t) => ListItem::new(Line::from(Span::styled(
                        format!("── {} ──", t),
                        header_style,
                    ))),
                    ChangesItem::Dir { name, depth, path } => {
                        let expanded = !self.view.collapsed.contains(path);
                        let indent = "  ".repeat(*depth as usize);
                        ListItem::new(Line::from(vec![
                            Span::raw(format!("  {}{}  ", indent, icons::folder(expanded))),
                            Span::raw(format!("{}/", name)),
                        ]))
                    }
                    ChangesItem::File {
                        name,
                        status,
                        depth,
                        ..
                    } => {
                        let indent = "  ".repeat(*depth as usize);
                        let mut name_style = match status {
                            GitStatus::Untracked => Style::default().fg(Color::Red),
                            GitStatus::Modified => Style::default().fg(Color::Yellow),
                            GitStatus::Staged => Style::default().fg(Color::Green),
                            GitStatus::Deleted => Style::default().fg(Color::Red),
                        };
                        if matches!(status, GitStatus::Deleted) {
                            name_style = name_style.add_modifier(Modifier::CROSSED_OUT);
                        }
                        let badge = match status {
                            GitStatus::Untracked => "??",
                            GitStatus::Modified => " M",
                            GitStatus::Staged => "A ",
                            GitStatus::Deleted => " D",
                        };
                        let badge_style = match status {
                            GitStatus::Untracked => Style::default().fg(Color::Red),
                            GitStatus::Modified => Style::default().fg(Color::Yellow),
                            GitStatus::Staged => Style::default().fg(Color::Green),
                            GitStatus::Deleted => Style::default().fg(Color::Red),
                        };
                        let icon = icons::for_file(name);
                        let left_text = format!("  {}{}  {}", indent, icon, name);
                        let used = left_text.chars().count() + badge.chars().count();
                        let pad = row_width.saturating_sub(used + 1).max(1);
                        ListItem::new(Line::from(vec![
                            Span::raw(format!("  {}{}  ", indent, icon)),
                            Span::styled(name.clone(), name_style),
                            Span::raw(" ".repeat(pad)),
                            Span::styled(badge.to_string(), badge_style),
                        ]))
                    }
                })
                .collect()
        };
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(title),
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
