use crate::syntax::Highlighter;
use anyhow::{Context, Result};
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use std::path::PathBuf;

mod edit;
mod modes;
mod motions;
mod mouse;
mod types;
mod widget;

pub use types::{COMMANDS, EditorMode, EditorRequest, filter_commands};
pub use widget::{EditorWidget, render_command_line};

use types::{Snapshot, YankRegister};

pub struct EditorView {
    pub path: PathBuf,
    pub lines: Vec<Vec<char>>,
    pub cursor: (usize, usize),
    pub mode: EditorMode,
    pub anchor: Option<(usize, usize)>,
    pub command: String,
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
    pub close_requested: bool,
    pub quit_app_requested: bool,
    pub focused: bool,
    pub command_selection: usize,
    pub pending_request: Option<EditorRequest>,
    pub did_save: bool,
    pub last_render_area: Option<Rect>,
    pub gutter_width: u16,
    pub highlighter: Highlighter,
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
        Ok(Self {
            path,
            lines,
            cursor: (0, 0),
            mode: EditorMode::Normal,
            anchor: None,
            command: String::new(),
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
            close_requested: false,
            quit_app_requested: false,
            focused: true,
            command_selection: 0,
            pending_request: None,
            did_save: false,
            last_render_area: None,
            gutter_width: 0,
            highlighter,
        })
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(self.mode, EditorMode::Command | EditorMode::Search) {
            self.status.clear();
        }
        match self.mode {
            EditorMode::Normal => self.normal_key(key),
            EditorMode::Insert => self.insert_key(key),
            EditorMode::Visual | EditorMode::VisualLine => self.visual_key(key),
            EditorMode::Command => self.command_key(key),
            EditorMode::Search => self.search_key(key),
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
        self.status = format!("\"{}\" written", self.path.display());
        Ok(())
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Visual => "VISUAL",
            EditorMode::VisualLine => "V-LINE",
            EditorMode::Command => "COMMAND",
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
        } else if self.cursor.0 >= self.scroll_row + h {
            self.scroll_row = self.cursor.0 + 1 - h;
        }
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

    pub(super) fn reload(&mut self, force: bool) {
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
                self.status = format!("\"{}\" reloaded", self.path.display());
            }
            Err(e) => self.status = format!("Reload failed: {}", e),
        }
    }
}

fn parse_lines(raw: &str) -> Vec<Vec<char>> {
    raw.split('\n')
        .map(|s| s.trim_end_matches('\r').chars().collect())
        .collect()
}
