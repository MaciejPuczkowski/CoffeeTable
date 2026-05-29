use ignore::WalkBuilder;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use regex::RegexBuilder;
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const MAX_HITS: usize = 500;
const MAX_LINE_LEN: usize = 400;

#[derive(Clone)]
pub struct Hit {
    pub path: PathBuf,
    pub row: usize,
    pub col: usize,
    pub text: String,
}

pub struct GrepView {
    pub root: PathBuf,
    pub pattern: String,
    pub hits: Vec<Hit>,
    pub list_state: ListState,
    pub error: Option<String>,
    pub excludes: Vec<String>,
}

impl GrepView {
    pub fn new(root: PathBuf, excludes: Vec<String>) -> Self {
        Self {
            root,
            pattern: String::new(),
            hits: Vec::new(),
            list_state: ListState::default(),
            error: None,
            excludes,
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.pattern.push(c);
        self.search();
    }

    pub fn pop_char(&mut self) {
        self.pattern.pop();
        self.search();
    }

    pub fn move_down(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state
            .select(Some((i + 1).min(self.hits.len() - 1)));
    }

    pub fn move_up(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(1)));
    }

    pub fn selected_hit(&self) -> Option<&Hit> {
        self.hits.get(self.list_state.selected()?)
    }

    fn search(&mut self) {
        self.hits.clear();
        self.error = None;
        if self.pattern.len() < 2 {
            self.list_state.select(None);
            return;
        }
        let re = match RegexBuilder::new(&self.pattern)
            .case_insensitive(true)
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("regex: {}", e));
                return;
            }
        };
        let exc: std::collections::HashSet<String> = self.excludes.iter().cloned().collect();
        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .max_filesize(Some(1 * 1024 * 1024))
            .filter_entry(move |entry| {
                let name = entry.file_name().to_string_lossy();
                !exc.contains(name.as_ref())
            })
            .build();
        for entry in walker.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            if self.hits.len() >= MAX_HITS {
                break;
            }
            scan_file(entry.path(), &re, &mut self.hits);
        }
        self.list_state
            .select(if self.hits.is_empty() { None } else { Some(0) });
    }
}

fn scan_file(path: &Path, re: &regex::Regex, out: &mut Vec<Hit>) {
    let Ok(file) = std::fs::File::open(path) else { return };
    let reader = BufReader::new(file);
    for (i, line) in reader.lines().enumerate() {
        if out.len() >= MAX_HITS {
            return;
        }
        let Ok(mut line) = line else { return };
        if line.len() > MAX_LINE_LEN {
            line.truncate(MAX_LINE_LEN);
        }
        if let Some(m) = re.find(&line) {
            out.push(Hit {
                path: path.to_path_buf(),
                row: i,
                col: line[..m.start()].chars().count(),
                text: line,
            });
        }
    }
}

pub struct GrepWidget<'a> {
    pub view: &'a mut GrepView,
}

impl<'a> Widget for GrepWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Grep ");
        let inner = outer.inner(area);
        outer.render(area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(inner);

        let input = Paragraph::new(self.view.pattern.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Regex (case-insensitive) "),
        );
        input.render(chunks[0], buf);

        let root = self.view.root.clone();
        let items: Vec<ListItem> = self
            .view
            .hits
            .iter()
            .map(|h| {
                let rel = h
                    .path
                    .strip_prefix(&root)
                    .unwrap_or(&h.path)
                    .to_string_lossy()
                    .to_string();
                ListItem::new(Line::from(vec![
                    Span::styled(rel, Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!(":{}:{}  ", h.row + 1, h.col + 1),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(h.text.clone()),
                ]))
            })
            .collect();
        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        StatefulWidget::render(list, chunks[1], buf, &mut self.view.list_state);

        let footer = self
            .view
            .error
            .clone()
            .unwrap_or_else(|| "Type to filter • Enter open • Esc cancel".into());
        let style = if self.view.error.is_some() {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        Paragraph::new(footer).style(style).render(chunks[2], buf);
    }
}
