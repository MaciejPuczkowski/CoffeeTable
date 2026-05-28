use crate::project::Project;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    Browse,
    AddProject,
    Roots,
    AddRoot,
}

pub enum PickerItem {
    Header(String),
    Saved(Project),
    Discovered { name: String, path: PathBuf },
}

pub struct ProjectPicker {
    pub mode: PickerMode,
    pub items: Vec<PickerItem>,
    pub roots: Vec<PathBuf>,
    pub list_state: ListState,
    pub roots_state: ListState,
    pub input: String,
    pub error: Option<String>,
}

impl ProjectPicker {
    pub fn new(
        saved: Vec<Project>,
        discovered: Vec<(String, PathBuf)>,
        roots: Vec<PathBuf>,
    ) -> Self {
        let items = build_items(saved, discovered);
        let mut list_state = ListState::default();
        list_state.select(first_selectable_idx(&items));
        let mut roots_state = ListState::default();
        if !roots.is_empty() {
            roots_state.select(Some(0));
        }
        Self {
            mode: PickerMode::Browse,
            items,
            roots,
            list_state,
            roots_state,
            input: String::new(),
            error: None,
        }
    }

    pub fn refresh(&mut self, saved: Vec<Project>, discovered: Vec<(String, PathBuf)>) {
        let prev_path = self.selected_item().and_then(item_path);
        self.items = build_items(saved, discovered);
        let new_sel = prev_path
            .and_then(|p| {
                self.items
                    .iter()
                    .position(|it| item_path(it).as_ref() == Some(&p))
            })
            .or_else(|| first_selectable_idx(&self.items));
        self.list_state.select(new_sel);
    }

    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots;
        match self.roots_state.selected() {
            Some(s) if s >= self.roots.len() => {
                if self.roots.is_empty() {
                    self.roots_state.select(None);
                } else {
                    self.roots_state.select(Some(self.roots.len() - 1));
                }
            }
            None if !self.roots.is_empty() => self.roots_state.select(Some(0)),
            _ => {}
        }
    }

    pub fn move_down(&mut self) {
        let curr = self.list_state.selected().unwrap_or(0);
        for i in (curr + 1)..self.items.len() {
            if !matches!(self.items[i], PickerItem::Header(_)) {
                self.list_state.select(Some(i));
                return;
            }
        }
    }

    pub fn move_up(&mut self) {
        let Some(curr) = self.list_state.selected() else {
            return;
        };
        for i in (0..curr).rev() {
            if !matches!(self.items[i], PickerItem::Header(_)) {
                self.list_state.select(Some(i));
                return;
            }
        }
    }

    pub fn selected_item(&self) -> Option<&PickerItem> {
        self.list_state.selected().and_then(|i| self.items.get(i))
    }

    pub fn move_root_down(&mut self) {
        if self.roots.is_empty() {
            return;
        }
        let i = self.roots_state.selected().unwrap_or(0);
        self.roots_state
            .select(Some((i + 1).min(self.roots.len() - 1)));
    }

    pub fn move_root_up(&mut self) {
        if self.roots.is_empty() {
            return;
        }
        let i = self.roots_state.selected().unwrap_or(0);
        self.roots_state.select(Some(i.saturating_sub(1)));
    }

    pub fn selected_root(&self) -> Option<&PathBuf> {
        self.roots_state.selected().and_then(|i| self.roots.get(i))
    }

    pub fn begin_add_project(&mut self) {
        self.mode = PickerMode::AddProject;
        self.input.clear();
        self.error = None;
    }

    pub fn begin_add_root(&mut self) {
        self.mode = PickerMode::AddRoot;
        self.input.clear();
        self.error = None;
    }

    pub fn open_roots(&mut self) {
        self.mode = PickerMode::Roots;
        self.input.clear();
        self.error = None;
    }

    pub fn open_browse(&mut self) {
        self.mode = PickerMode::Browse;
        self.input.clear();
        self.error = None;
    }

    pub fn cancel_input(&mut self) {
        match self.mode {
            PickerMode::AddProject => self.mode = PickerMode::Browse,
            PickerMode::AddRoot => self.mode = PickerMode::Roots,
            _ => {}
        }
        self.input.clear();
        self.error = None;
    }

    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
        self.error = None;
    }

    pub fn pop_char(&mut self) {
        self.input.pop();
        self.error = None;
    }

    pub fn confirm_add_project(&mut self) -> Option<(String, PathBuf)> {
        let (name, path) = self.validate_dir_input()?;
        let name = name.unwrap_or_else(|| path.to_string_lossy().into_owned());
        Some((name, path))
    }

    pub fn confirm_add_root(&mut self) -> Option<PathBuf> {
        let (_, path) = self.validate_dir_input()?;
        Some(path)
    }

    fn validate_dir_input(&mut self) -> Option<(Option<String>, PathBuf)> {
        let trimmed = self.input.trim();
        if trimmed.is_empty() {
            self.error = Some("Path cannot be empty.".into());
            return None;
        }
        let path = PathBuf::from(trimmed);
        if !path.is_dir() {
            self.error = Some(format!("Directory does not exist: {}", path.display()));
            return None;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned());
        Some((name, path))
    }
}

fn build_items(saved: Vec<Project>, discovered: Vec<(String, PathBuf)>) -> Vec<PickerItem> {
    let mut out = Vec::new();
    out.push(PickerItem::Header(format!("Saved ({})", saved.len())));
    for p in saved {
        out.push(PickerItem::Saved(p));
    }
    out.push(PickerItem::Header(format!(
        "Discovered ({})",
        discovered.len()
    )));
    for (name, path) in discovered {
        out.push(PickerItem::Discovered { name, path });
    }
    out
}

fn first_selectable_idx(items: &[PickerItem]) -> Option<usize> {
    items
        .iter()
        .position(|i| !matches!(i, PickerItem::Header(_)))
}

fn item_path(item: &PickerItem) -> Option<PathBuf> {
    match item {
        PickerItem::Saved(p) => Some(p.path.clone()),
        PickerItem::Discovered { path, .. } => Some(path.clone()),
        PickerItem::Header(_) => None,
    }
}

pub struct ProjectPickerWidget<'a> {
    pub picker: &'a mut ProjectPicker,
}

impl<'a> Widget for ProjectPickerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = match self.picker.mode {
            PickerMode::Browse | PickerMode::AddProject => " Projects ",
            PickerMode::Roots | PickerMode::AddRoot => " Root directories ",
        };
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(title);
        let inner = outer.inner(area);
        outer.render(area, buf);

        let show_input = matches!(
            self.picker.mode,
            PickerMode::AddProject | PickerMode::AddRoot
        );
        let chunks = if show_input {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(3),
                    Constraint::Length(2),
                ])
                .split(inner)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(inner)
        };

        match self.picker.mode {
            PickerMode::Browse | PickerMode::AddProject => {
                render_items(self.picker, chunks[0], buf)
            }
            PickerMode::Roots | PickerMode::AddRoot => render_roots(self.picker, chunks[0], buf),
        }
        if show_input {
            render_input(self.picker, chunks[1], buf);
            render_footer(self.picker, chunks[2], buf);
        } else {
            render_footer(self.picker, chunks[1], buf);
        }
    }
}

fn render_items(picker: &mut ProjectPicker, area: Rect, buf: &mut Buffer) {
    let header_style = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
    let items: Vec<ListItem> = picker
        .items
        .iter()
        .map(|it| match it {
            PickerItem::Header(t) => ListItem::new(Line::from(Span::styled(
                format!("── {} ──", t),
                header_style,
            ))),
            PickerItem::Saved(p) => {
                let mut spans = vec![
                    Span::raw("  "),
                    Span::styled(
                        p.name.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        p.path.display().to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];
                if let Some(url) = &p.github_url {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(url.clone(), Style::default().fg(Color::Blue)));
                }
                ListItem::new(Line::from(spans))
            }
            PickerItem::Discovered { name, path } => ListItem::new(Line::from(vec![
                Span::styled("+ ", Style::default().fg(Color::Green)),
                Span::styled(name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(
                    path.display().to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
        })
        .collect();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(list, area, buf, &mut picker.list_state);
}

fn render_roots(picker: &mut ProjectPicker, area: Rect, buf: &mut Buffer) {
    let items: Vec<ListItem> = if picker.roots.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  (no roots configured) — press  n  to add",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        picker
            .roots
            .iter()
            .map(|p| {
                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(p.display().to_string()),
                ]))
            })
            .collect()
    };
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(list, area, buf, &mut picker.roots_state);
}

fn render_input(picker: &ProjectPicker, area: Rect, buf: &mut Buffer) {
    let title = match picker.mode {
        PickerMode::AddProject => " New project path ",
        PickerMode::AddRoot => " New root path ",
        _ => " Input ",
    };
    let para = Paragraph::new(picker.input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(title),
    );
    para.render(area, buf);
}

fn render_footer(picker: &ProjectPicker, area: Rect, buf: &mut Buffer) {
    let (text, style) = if let Some(err) = &picker.error {
        (err.clone(), Style::default().fg(Color::Red))
    } else {
        let help = match picker.mode {
            PickerMode::Browse => {
                "Enter open • n add • r roots • s rescan • d delete • ? help • Esc close"
            }
            PickerMode::AddProject => "Enter confirm • Esc cancel",
            PickerMode::Roots => "n add root • d delete • ? help • Esc back to projects",
            PickerMode::AddRoot => "Enter confirm • Esc cancel",
        };
        (help.to_string(), Style::default().fg(Color::DarkGray))
    };
    Paragraph::new(text).style(style).render(area, buf);
}
