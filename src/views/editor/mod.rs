use crate::syntax::Highlighter;
use anyhow::{Context, Result};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use std::{path::PathBuf, time::SystemTime};

mod edit;
mod modes;
mod motions;
mod mouse;
mod types;
mod widget;

pub use types::{COMMANDS, EditorMode, GitView, WrapMode, filter_commands};
pub use widget::{EditorWidget, render_command_line};

use types::{Snapshot, YankRegister};

pub struct EditorView {
    pub path: PathBuf,
    pub lines: Vec<Vec<char>>,
    pub cursor: (usize, usize),
    pub mode: EditorMode,
    pub anchor: Option<(usize, usize)>,
    pub search: String,
    pub last_search: Option<String>,
    pub yank: YankRegister,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub viewport_rows: u16,
    pub modified: bool,
    pub status: String,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    pub pending_g: bool,
    pub pending_d: bool,
    pub pending_y: bool,
    pub preferred_col: usize,
    pub focused: bool,
    pub did_save: bool,
    pub request_focus_tree: bool,
    pub git_view: GitView,
    pub working_lines: Option<Vec<Vec<char>>>,
    pub working_cursor: Option<(usize, usize)>,
    pub working_scroll: Option<(usize, usize)>,
    pub last_render_area: Option<Rect>,
    pub pill_working: Option<Rect>,
    pub pill_head: Option<Rect>,
    pub pill_diff: Option<Rect>,
    pub gutter_width: u16,
    pub highlighter: Highlighter,
    pub wrap_mode: WrapMode,
    pub disk_mtime: Option<SystemTime>,
    pub disk_conflict_warned: bool,
}

impl EditorView {
    pub fn from_content(path: PathBuf, raw: String) -> Result<Self> {
        let mut lines = parse_lines(&raw);
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let highlighter = Highlighter::for_extension(&ext);
        let disk_mtime = file_mtime(&path);
        Ok(Self {
            path,
            lines,
            cursor: (0, 0),
            mode: EditorMode::Normal,
            anchor: None,
            search: String::new(),
            last_search: None,
            yank: YankRegister::default(),
            scroll_row: 0,
            scroll_col: 0,
            viewport_rows: 0,
            modified: false,
            status: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            pending_g: false,
            pending_d: false,
            pending_y: false,
            preferred_col: 0,
            focused: true,
            did_save: false,
            request_focus_tree: false,
            git_view: GitView::Working,
            working_lines: None,
            working_cursor: None,
            working_scroll: None,
            last_render_area: None,
            pill_working: None,
            pill_head: None,
            pill_diff: None,
            gutter_width: 0,
            highlighter,
            wrap_mode: WrapMode::Off,
            disk_mtime,
            disk_conflict_warned: false,
        })
    }

    pub fn cycle_wrap(&mut self) {
        self.wrap_mode = self.wrap_mode.cycle();
        self.scroll_col = 0;
        self.status = self.wrap_mode.label().to_string();
    }

    pub fn show_alt_view(&mut self, view: GitView, raw: String) {
        if self.git_view == GitView::Working {
            self.working_lines = Some(self.lines.clone());
            self.working_cursor = Some(self.cursor);
            self.working_scroll = Some((self.scroll_row, self.scroll_col));
        }
        let mut lines = parse_lines(&raw);
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        self.lines = lines;
        self.cursor = (0, 0);
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.preferred_col = 0;
        self.mode = EditorMode::Normal;
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.git_view = view;
        let ext = if matches!(view, GitView::Diff) {
            "diff"
        } else {
            self.path.extension().and_then(|s| s.to_str()).unwrap_or("")
        };
        self.highlighter = Highlighter::for_extension(ext);
    }

    pub fn show_working(&mut self) {
        if matches!(self.git_view, GitView::Working) {
            return;
        }
        if let Some(saved) = self.working_lines.take() {
            self.lines = saved;
        }
        if let Some(cur) = self.working_cursor.take() {
            self.cursor = cur;
            self.preferred_col = cur.1;
        }
        if let Some((r, c)) = self.working_scroll.take() {
            self.scroll_row = r;
            self.scroll_col = c;
        }
        self.git_view = GitView::Working;
        let ext = self.path.extension().and_then(|s| s.to_str()).unwrap_or("");
        self.highlighter = Highlighter::for_extension(ext);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(self.mode, EditorMode::Search) {
            self.status.clear();
        }
        if matches!(self.git_view, GitView::Head | GitView::Diff) {
            self.readonly_key(key);
        } else {
            match self.mode {
                EditorMode::Normal => self.normal_key(key),
                EditorMode::Insert => self.insert_key(key),
                EditorMode::Visual | EditorMode::VisualLine => self.visual_key(key),
                EditorMode::Search => self.search_key(key),
            }
        }
        self.clamp_cursor();
    }

    pub fn save(&mut self) -> Result<()> {
        let body: String = self
            .lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&self.path, body)
            .with_context(|| format!("write {}", self.path.display()))?;
        self.modified = false;
        self.did_save = true;
        self.disk_mtime = file_mtime(&self.path);
        self.disk_conflict_warned = false;
        self.status = format!("\"{}\" written", self.path.display());
        Ok(())
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Visual => "VISUAL",
            EditorMode::VisualLine => "V-LINE",
            EditorMode::Search => "SEARCH",
        }
    }

    pub fn ensure_cursor_visible(&mut self) {
        if self.viewport_rows == 0 {
            return;
        }
        let h = self.viewport_rows as usize;
        if self.cursor.0 < self.scroll_row {
            self.scroll_row = self.cursor.0;
        }
        if self.wrap_mode.width().is_some() {
            while self.scroll_row < self.cursor.0
                && self.display_rows(self.scroll_row, self.cursor.0 + 1) > h
            {
                self.scroll_row += 1;
            }
        } else if self.cursor.0 >= self.scroll_row + h {
            self.scroll_row = self.cursor.0 + 1 - h;
        }
    }

    fn display_rows(&self, start: usize, end: usize) -> usize {
        let Some(w) = self.wrap_mode.width() else {
            return end.saturating_sub(start);
        };
        let mut total = 0usize;
        for i in start..end.min(self.lines.len()) {
            let len = self.lines[i].len();
            let chunks = if len == 0 { 1 } else { len.div_ceil(w) };
            total += chunks;
        }
        total
    }

    pub(super) fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map(|l| l.len()).unwrap_or(0)
    }

    pub(super) fn clamp_cursor(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        if self.cursor.0 >= self.lines.len() {
            self.cursor.0 = self.lines.len() - 1;
        }
        let line_len = self.line_len(self.cursor.0);
        let max = if self.mode == EditorMode::Insert || line_len == 0 {
            line_len
        } else {
            line_len - 1
        };
        if self.cursor.1 > max {
            self.cursor.1 = max;
        }
    }

    pub fn reload(&mut self, force: bool) {
        if self.modified && !force {
            self.status = "Unsaved changes (e! to discard)".into();
            return;
        }
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => {
                let mut lines = parse_lines(&raw);
                if lines.is_empty() {
                    lines.push(Vec::new());
                }
                self.lines = lines;
                self.cursor = (0, 0);
                self.preferred_col = 0;
                self.modified = false;
                self.undo.clear();
                self.redo.clear();
                self.disk_mtime = file_mtime(&self.path);
                self.disk_conflict_warned = false;
                self.status = format!("\"{}\" reloaded", self.path.display());
            }
            Err(e) => self.status = format!("Reload failed: {}", e),
        }
    }

    pub fn poll_disk(&mut self) -> bool {
        if !matches!(self.git_view, GitView::Working) {
            return false;
        }
        let Some(current) = file_mtime(&self.path) else {
            return false;
        };
        let prev = match self.disk_mtime {
            Some(t) => t,
            None => {
                self.disk_mtime = Some(current);
                return false;
            }
        };
        if current <= prev {
            return false;
        }
        if self.modified {
            if !self.disk_conflict_warned {
                self.disk_conflict_warned = true;
                self.status = format!(
                    "{} changed on disk — unsaved buffer kept (\":e!\" to reload)",
                    self.path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                );
            }
            return false;
        }
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => {
                let mut new_lines = parse_lines(&raw);
                if new_lines.is_empty() {
                    new_lines.push(Vec::new());
                }
                let cursor = self.cursor;
                self.lines = new_lines;
                if self.cursor.0 >= self.lines.len() {
                    self.cursor.0 = self.lines.len().saturating_sub(1);
                }
                let max_col = self.lines.get(self.cursor.0).map(|l| l.len()).unwrap_or(0);
                if self.cursor.1 > max_col {
                    self.cursor.1 = max_col;
                }
                let _ = cursor;
                self.disk_mtime = Some(current);
                self.disk_conflict_warned = false;
                true
            }
            Err(_) => false,
        }
    }
}

fn file_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn parse_lines(raw: &str) -> Vec<Vec<char>> {
    raw.split('\n')
        .map(|s| s.trim_end_matches('\r').chars().collect())
        .collect()
}
