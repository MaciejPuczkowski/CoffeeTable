use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use std::path::{Path, PathBuf};

pub struct FileFinder {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub query: String,
    pub matches: Vec<(i64, usize)>,
    pub list_state: ListState,
}

impl FileFinder {
    pub fn new(root: PathBuf) -> Self {
        let files = enumerate_files(&root);
        let mut s = Self {
            root,
            files,
            query: String::new(),
            matches: Vec::new(),
            list_state: ListState::default(),
        };
        s.refresh();
        s
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.refresh();
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.refresh();
    }

    pub fn move_down(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state
            .select(Some((i + 1).min(self.matches.len() - 1)));
    }

    pub fn move_up(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(1)));
    }

    pub fn selected_path(&self) -> Option<&Path> {
        let (_, idx) = self.matches.get(self.list_state.selected()?)?;
        self.files.get(*idx).map(|p| p.as_path())
    }

    fn refresh(&mut self) {
        let matcher = SkimMatcherV2::default().ignore_case();
        let mut matches: Vec<(i64, usize)> = if self.query.is_empty() {
            self.files
                .iter()
                .enumerate()
                .take(500)
                .map(|(i, _)| (0i64, i))
                .collect()
        } else {
            self.files
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let rel = p
                        .strip_prefix(&self.root)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .to_string();
                    matcher
                        .fuzzy_match(&rel, &self.query)
                        .map(|score| (score, i))
                })
                .collect()
        };
        matches.sort_by(|a, b| b.0.cmp(&a.0));
        matches.truncate(500);
        self.matches = matches;
        if !self.matches.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }
}

fn enumerate_files(root: &Path) -> Vec<PathBuf> {
    use ignore::WalkBuilder;
    let mut out = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .ignore(true)
        .max_filesize(Some(2 * 1024 * 1024))
        .build();
    for entry in walker.flatten() {
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            out.push(entry.into_path());
        }
        if out.len() >= 20_000 {
            break;
        }
    }
    out
}

pub struct FileFinderWidget<'a> {
    pub finder: &'a mut FileFinder,
}

impl<'a> Widget for FileFinderWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Find file ");
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

        let input = Paragraph::new(self.finder.query.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Pattern "),
        );
        input.render(chunks[0], buf);

        let root = self.finder.root.clone();
        let items: Vec<ListItem> = self
            .finder
            .matches
            .iter()
            .filter_map(|(_, i)| self.finder.files.get(*i))
            .map(|p| {
                let rel = p.strip_prefix(&root).unwrap_or(p).to_string_lossy().to_string();
                ListItem::new(Line::from(Span::raw(rel)))
            })
            .collect();
        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        StatefulWidget::render(list, chunks[1], buf, &mut self.finder.list_state);

        Paragraph::new("Type to filter • Enter open • Esc cancel")
            .style(Style::default().fg(Color::DarkGray))
            .render(chunks[2], buf);
    }
}
