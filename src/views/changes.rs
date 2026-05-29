use crate::git::GitStatus;
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
        if let ChangesItem::File { path, .. } = self.items[idx].clone() {
            self.list_state.select(Some(idx));
            self.selected_path = Some(path.clone());
            ChangesAction::OpenFile(path)
        } else {
            ChangesAction::None
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
        let prev = self.selected_path.clone();
        let mut staged: Vec<(PathBuf, GitStatus)> = Vec::new();
        let mut modified: Vec<(PathBuf, GitStatus)> = Vec::new();
        let mut untracked: Vec<(PathBuf, GitStatus)> = Vec::new();
        for (p, s) in status {
            match s {
                GitStatus::Staged => staged.push((p.clone(), *s)),
                GitStatus::Modified => modified.push((p.clone(), *s)),
                GitStatus::Untracked => untracked.push((p.clone(), *s)),
            }
        }

        let root = self.root.clone();
        let mut items = Vec::new();
        push_section(&mut items, "Staged", &mut staged, &root);
        push_section(&mut items, "Modified", &mut modified, &root);
        push_section(&mut items, "Untracked", &mut untracked, &root);
        self.items = items;
        self.restore_selection(prev);
    }

    fn restore_selection(&mut self, prev: Option<PathBuf>) {
        if let Some(p) = prev {
            if let Some(i) = self.items.iter().position(|it| match it {
                ChangesItem::File { path, .. } => *path == p,
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
            .position(|it| matches!(it, ChangesItem::File { .. }))
    }
}

fn push_section(
    items: &mut Vec<ChangesItem>,
    title: &str,
    files: &mut Vec<(PathBuf, GitStatus)>,
    root: &Path,
) {
    if files.is_empty() {
        return;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    items.push(ChangesItem::Header(format!("{} ({})", title, files.len())));
    let mut current: Vec<String> = Vec::new();
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
        for d in dirs.iter().skip(common) {
            current.push(d.clone());
            items.push(ChangesItem::Dir {
                name: d.clone(),
                depth: (current.len() - 1) as u16,
            });
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
            if matches!(self.items[i], ChangesItem::File { .. }) {
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
            if matches!(self.items[i], ChangesItem::File { .. }) {
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
            .rposition(|it| matches!(it, ChangesItem::File { .. }))
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
                    ChangesItem::Dir { name, depth } => {
                        let indent = "  ".repeat(*depth as usize);
                        ListItem::new(Line::from(vec![
                            Span::raw(format!("  {}", indent)),
                            Span::styled(
                                format!("{}/", name),
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]))
                    }
                    ChangesItem::File {
                        name,
                        status,
                        depth,
                        ..
                    } => {
                        let indent = "  ".repeat(*depth as usize);
                        let style = match status {
                            GitStatus::Untracked => Style::default().fg(Color::Red),
                            GitStatus::Modified => Style::default().fg(Color::Yellow),
                            GitStatus::Staged => Style::default().fg(Color::Green),
                        };
                        let badge = match status {
                            GitStatus::Untracked => "??",
                            GitStatus::Modified => " M",
                            GitStatus::Staged => "A ",
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(badge.to_string(), style),
                            Span::raw(format!("  {}", indent)),
                            Span::styled(name.clone(), style),
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
