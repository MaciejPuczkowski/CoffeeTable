use crate::{clipboard, syntax::Highlighter};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
    Search,
}

#[derive(Default, Clone)]
pub struct YankRegister {
    pub text: String,
    pub linewise: bool,
}

#[derive(Clone, Copy)]
pub enum EditorRequest {
    OpenFinder,
    OpenGrep,
    OpenPicker,
    FocusTree,
    FocusEditor,
    ShowHelp,
}

pub struct CommandDef {
    pub key: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
}

pub const COMMANDS: &[CommandDef] = &[
    CommandDef { key: "w", aliases: &["write"], description: "Save the current file" },
    CommandDef { key: "q", aliases: &["close"], description: "Close editor (q! to force)" },
    CommandDef { key: "x", aliases: &["wq"], description: "Save and close" },
    CommandDef { key: "e", aliases: &["edit", "reload"], description: "Reload from disk (e! to discard)" },
    CommandDef { key: "Q", aliases: &["qa", "quit"], description: "Quit application (Q! to force)" },
    CommandDef { key: "f", aliases: &["find"], description: "Find file in project" },
    CommandDef { key: "g", aliases: &["grep"], description: "Grep across project" },
    CommandDef { key: "p", aliases: &["projects"], description: "Open project picker" },
    CommandDef { key: "t", aliases: &["tree", "explorer"], description: "Focus file tree" },
    CommandDef { key: "b", aliases: &["buffer"], description: "Focus editor" },
    CommandDef { key: "h", aliases: &["help"], description: "Show help overlay" },
];

pub fn filter_commands(query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..COMMANDS.len()).collect();
    }
    let q = query.trim_end_matches('!').to_lowercase();
    if q.is_empty() {
        return (0..COMMANDS.len()).collect();
    }
    let mut scored: Vec<(i32, usize)> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let key_l = c.key.to_lowercase();
            if key_l == q { return Some((100, i)); }
            if key_l.starts_with(&q) { return Some((80, i)); }
            for a in c.aliases {
                let al = a.to_lowercase();
                if al == q { return Some((90, i)); }
                if al.starts_with(&q) { return Some((70, i)); }
            }
            if c.description.to_lowercase().contains(&q) { return Some((30, i)); }
            None
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, i)| i).collect()
}

#[derive(Clone)]
pub struct Snapshot {
    pub lines: Vec<Vec<char>>,
    pub cursor: (usize, usize),
}

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
    pub highlighter: Highlighter,
}

impl EditorView {
    pub fn from_content(path: PathBuf, raw: String) -> Result<Self> {
        let mut lines: Vec<Vec<char>> = raw
            .split('\n')
            .map(|s| s.trim_end_matches('\r').chars().collect())
            .collect();
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

    fn normal_key(&mut self, key: KeyEvent) {
        let prev_g = std::mem::replace(&mut self.pending_g, false);
        let prev_d = std::mem::replace(&mut self.pending_d, false);
        let prev_y = std::mem::replace(&mut self.pending_y, false);
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {}
            (KeyCode::Backspace, _) => {
                self.pending_request = Some(EditorRequest::FocusTree);
            }
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Char('l'), _) | (KeyCode::Right, _) => self.move_right(),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Char('0'), _) | (KeyCode::Home, _) => self.jump_line_start(),
            (KeyCode::Char('^'), _) => self.jump_first_non_ws(),
            (KeyCode::Char('$'), _) | (KeyCode::End, _) => self.jump_line_end(),
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if prev_g {
                    self.cursor = (0, 0);
                    self.preferred_col = 0;
                } else {
                    self.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) => self.jump_last_line(),
            (KeyCode::Char('w'), m) if !m.contains(KeyModifiers::CONTROL) => self.motion_word_forward(),
            (KeyCode::Char('b'), _) => self.motion_word_back(),
            (KeyCode::Char('i'), _) => self.mode = EditorMode::Insert,
            (KeyCode::Char('I'), _) => {
                self.jump_first_non_ws();
                self.mode = EditorMode::Insert;
            }
            (KeyCode::Char('a'), _) => {
                let len = self.line_len(self.cursor.0);
                if self.cursor.1 < len {
                    self.cursor.1 += 1;
                }
                self.mode = EditorMode::Insert;
            }
            (KeyCode::Char('A'), _) => {
                self.cursor.1 = self.line_len(self.cursor.0);
                self.mode = EditorMode::Insert;
            }
            (KeyCode::Char('o'), _) => {
                self.snapshot();
                let row = self.cursor.0 + 1;
                self.lines.insert(row, Vec::new());
                self.cursor = (row, 0);
                self.modified = true;
                self.mode = EditorMode::Insert;
            }
            (KeyCode::Char('O'), _) => {
                self.snapshot();
                let row = self.cursor.0;
                self.lines.insert(row, Vec::new());
                self.cursor = (row, 0);
                self.modified = true;
                self.mode = EditorMode::Insert;
            }
            (KeyCode::Char('x'), _) => self.delete_char_at_cursor(),
            (KeyCode::Char('X'), _) => self.delete_char_before_cursor(),
            (KeyCode::Char('d'), _) => {
                if prev_d {
                    self.delete_current_line();
                } else {
                    self.pending_d = true;
                }
            }
            (KeyCode::Char('D'), _) => self.delete_to_end_of_line(),
            (KeyCode::Char('y'), _) => {
                if prev_y {
                    self.yank_line();
                } else {
                    self.pending_y = true;
                }
            }
            (KeyCode::Char('p'), _) => self.paste_after(),
            (KeyCode::Char('P'), _) => self.paste_before(),
            (KeyCode::Char('v'), _) => {
                self.anchor = Some(self.cursor);
                self.mode = EditorMode::Visual;
            }
            (KeyCode::Char('V'), _) => {
                self.anchor = Some(self.cursor);
                self.mode = EditorMode::VisualLine;
            }
            (KeyCode::Char('u'), m) if !m.contains(KeyModifiers::CONTROL) => self.undo(),
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => self.redo(),
            (KeyCode::Char('/'), _) => {
                self.mode = EditorMode::Search;
                self.search.clear();
            }
            (KeyCode::Char('n'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.repeat_search(true)
            }
            (KeyCode::Char('N'), _) => self.repeat_search(false),
            (KeyCode::Char(':'), _) => {
                self.mode = EditorMode::Command;
                self.command.clear();
                self.command_selection = 0;
            }
            (KeyCode::Enter, _) => self.jump_next_line_indent(),
            _ => {}
        }
    }

    fn insert_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.mode = EditorMode::Normal;
                if self.cursor.1 > 0 {
                    self.cursor.1 -= 1;
                }
                self.preferred_col = self.cursor.1;
                if self.modified {
                    if let Err(e) = self.save() {
                        self.status = format!("Autosave failed: {}", e);
                    }
                }
            }
            (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Right, _) => {
                let len = self.line_len(self.cursor.0);
                if self.cursor.1 < len {
                    self.cursor.1 += 1;
                }
            }
            (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Backspace, _) => self.backspace(),
            (KeyCode::Delete, _) => self.delete_char_at_cursor(),
            (KeyCode::Enter, _) => self.split_line_at_cursor(),
            (KeyCode::Tab, _) => self.insert_str("    "),
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => self.insert_char(c),
            _ => {}
        }
    }

    fn visual_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.mode = EditorMode::Normal;
                self.anchor = None;
            }
            (KeyCode::Backspace, _) => {
                self.pending_request = Some(EditorRequest::FocusTree);
                self.mode = EditorMode::Normal;
                self.anchor = None;
            }
            (KeyCode::Char('v'), _) => {
                if self.mode == EditorMode::Visual {
                    self.mode = EditorMode::Normal;
                    self.anchor = None;
                } else {
                    self.mode = EditorMode::Visual;
                }
            }
            (KeyCode::Char('V'), _) => {
                if self.mode == EditorMode::VisualLine {
                    self.mode = EditorMode::Normal;
                    self.anchor = None;
                } else {
                    self.mode = EditorMode::VisualLine;
                }
            }
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Char('l'), _) | (KeyCode::Right, _) => self.move_right(),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Char('0'), _) | (KeyCode::Home, _) => self.jump_line_start(),
            (KeyCode::Char('^'), _) => self.jump_first_non_ws(),
            (KeyCode::Char('$'), _) | (KeyCode::End, _) => self.jump_line_end(),
            (KeyCode::Char('w'), _) => self.motion_word_forward(),
            (KeyCode::Char('b'), _) => self.motion_word_back(),
            (KeyCode::Char('G'), _) => self.jump_last_line(),
            (KeyCode::Char('g'), _) => {
                if self.pending_g {
                    self.cursor = (0, 0);
                    self.preferred_col = 0;
                    self.pending_g = false;
                } else {
                    self.pending_g = true;
                }
            }
            (KeyCode::Char('y'), _) => self.yank_selection(),
            (KeyCode::Char('d'), _) | (KeyCode::Char('x'), _) => self.delete_selection(),
            _ => {}
        }
    }

    fn command_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.mode = EditorMode::Normal;
                self.command.clear();
                self.command_selection = 0;
            }
            (KeyCode::Enter, _) => {
                let raw = self.command.trim().to_string();
                let force = raw.ends_with('!');
                let filtered = filter_commands(&raw);
                let to_run = match filtered
                    .get(self.command_selection.min(filtered.len().saturating_sub(1)))
                    .copied()
                {
                    Some(idx) => {
                        let key = COMMANDS[idx].key.to_string();
                        if force { format!("{}!", key) } else { key }
                    }
                    None => raw,
                };
                self.command.clear();
                self.command_selection = 0;
                self.mode = EditorMode::Normal;
                self.execute_command(&to_run);
            }
            (KeyCode::Backspace, _) => {
                if self.command.pop().is_none() {
                    self.mode = EditorMode::Normal;
                }
                self.command_selection = 0;
            }
            (KeyCode::Down, _) | (KeyCode::Tab, _) => {
                let filtered = filter_commands(&self.command);
                if !filtered.is_empty() {
                    self.command_selection =
                        (self.command_selection + 1).min(filtered.len() - 1);
                }
            }
            (KeyCode::Up, _) | (KeyCode::BackTab, _) => {
                self.command_selection = self.command_selection.saturating_sub(1);
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.command.push(c);
                self.command_selection = 0;
            }
            _ => {}
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.mode = EditorMode::Normal;
                self.search.clear();
            }
            (KeyCode::Enter, _) => {
                let pattern = self.search.clone();
                self.mode = EditorMode::Normal;
                if !pattern.is_empty() {
                    self.last_search = Some(pattern);
                    self.repeat_search(true);
                }
            }
            (KeyCode::Backspace, _) => {
                if self.search.pop().is_none() {
                    self.mode = EditorMode::Normal;
                }
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => self.search.push(c),
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        let (base, force) = match cmd.strip_suffix('!') {
            Some(rest) => (rest, true),
            None => (cmd, false),
        };
        match base {
            "w" | "write" => match self.save() {
                Ok(()) => {}
                Err(e) => self.status = format!("Error: {}", e),
            },
            "q" | "close" => {
                if !self.modified || force {
                    self.close_requested = true;
                } else {
                    self.status = "No write since last change (q! to force)".into();
                }
            }
            "wq" | "x" => match self.save() {
                Ok(()) => self.close_requested = true,
                Err(e) => self.status = format!("Error: {}", e),
            },
            "Q" | "qa" | "quit" => {
                if !self.modified || force {
                    self.quit_app_requested = true;
                } else {
                    self.status = "No write since last change (Q! to force)".into();
                }
            }
            "e" | "edit" | "reload" => self.reload(force),
            "f" | "find" => self.pending_request = Some(EditorRequest::OpenFinder),
            "g" | "grep" => self.pending_request = Some(EditorRequest::OpenGrep),
            "p" | "projects" => self.pending_request = Some(EditorRequest::OpenPicker),
            "t" | "tree" | "explorer" => self.pending_request = Some(EditorRequest::FocusTree),
            "b" | "buffer" => self.pending_request = Some(EditorRequest::FocusEditor),
            "h" | "help" => self.pending_request = Some(EditorRequest::ShowHelp),
            "" => {}
            other => {
                self.status = format!("Not an editor command: {}", other);
            }
        }
    }

    fn reload(&mut self, force: bool) {
        if self.modified && !force {
            self.status = "Unsaved changes (e! to discard)".into();
            return;
        }
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => {
                let mut lines: Vec<Vec<char>> = raw
                    .split('\n')
                    .map(|s| s.trim_end_matches('\r').chars().collect())
                    .collect();
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

    fn move_left(&mut self) {
        if self.cursor.1 > 0 {
            self.cursor.1 -= 1;
            self.preferred_col = self.cursor.1;
        }
    }

    fn move_right(&mut self) {
        let len = self.line_len(self.cursor.0);
        let max = if self.mode == EditorMode::Insert {
            len
        } else {
            len.saturating_sub(1)
        };
        if self.cursor.1 < max {
            self.cursor.1 += 1;
            self.preferred_col = self.cursor.1;
        }
    }

    fn move_down(&mut self) {
        if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.cursor.1 = self.preferred_col.min(self.line_len(self.cursor.0));
        }
    }

    fn move_up(&mut self) {
        if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.cursor.1 = self.preferred_col.min(self.line_len(self.cursor.0));
        }
    }

    fn jump_line_start(&mut self) {
        self.cursor.1 = 0;
        self.preferred_col = 0;
    }

    fn jump_line_end(&mut self) {
        let len = self.line_len(self.cursor.0);
        self.cursor.1 = len.saturating_sub(1);
        self.preferred_col = usize::MAX;
    }

    fn jump_first_non_ws(&mut self) {
        let line = &self.lines[self.cursor.0];
        let col = line.iter().position(|c| !c.is_whitespace()).unwrap_or(0);
        self.cursor.1 = col;
        self.preferred_col = col;
    }

    fn jump_last_line(&mut self) {
        self.cursor.0 = self.lines.len().saturating_sub(1);
        self.cursor.1 = self.preferred_col.min(self.line_len(self.cursor.0));
    }

    fn jump_next_line_indent(&mut self) {
        if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.jump_first_non_ws();
        }
    }

    fn motion_word_forward(&mut self) {
        let (mut r, mut c) = self.cursor;
        loop {
            let line = &self.lines[r];
            if c < line.len() {
                let cur = line[c];
                if is_word(cur) {
                    while c < line.len() && is_word(line[c]) {
                        c += 1;
                    }
                } else if !cur.is_whitespace() {
                    while c < line.len() && !is_word(line[c]) && !line[c].is_whitespace() {
                        c += 1;
                    }
                }
                while c < line.len() && line[c].is_whitespace() {
                    c += 1;
                }
                if c < line.len() {
                    break;
                }
            }
            if r + 1 >= self.lines.len() {
                c = self.line_len(r).saturating_sub(1);
                break;
            }
            r += 1;
            c = 0;
            let line = &self.lines[r];
            if let Some(p) = line.iter().position(|ch| !ch.is_whitespace()) {
                c = p;
                break;
            }
        }
        self.cursor = (r, c);
        self.preferred_col = c;
    }

    fn motion_word_back(&mut self) {
        let (mut r, mut c) = self.cursor;
        loop {
            if c == 0 {
                if r == 0 {
                    break;
                }
                r -= 1;
                c = self.line_len(r);
            }
            let line = &self.lines[r];
            while c > 0 && line.get(c.saturating_sub(1)).map(|ch| ch.is_whitespace()).unwrap_or(false) {
                c -= 1;
            }
            if c == 0 {
                continue;
            }
            let kind = is_word(line[c - 1]);
            while c > 0 && is_word(line[c - 1]) == kind && !line[c - 1].is_whitespace() {
                c -= 1;
            }
            break;
        }
        self.cursor = (r, c);
        self.preferred_col = c;
    }

    fn insert_char(&mut self, c: char) {
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        if self.cursor.1 > line.len() {
            self.cursor.1 = line.len();
        }
        line.insert(self.cursor.1, c);
        self.cursor.1 += 1;
        self.preferred_col = self.cursor.1;
        self.modified = true;
    }

    fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    fn backspace(&mut self) {
        self.snapshot();
        if self.cursor.1 > 0 {
            let line = &mut self.lines[self.cursor.0];
            line.remove(self.cursor.1 - 1);
            self.cursor.1 -= 1;
        } else if self.cursor.0 > 0 {
            let removed = self.lines.remove(self.cursor.0);
            self.cursor.0 -= 1;
            self.cursor.1 = self.lines[self.cursor.0].len();
            self.lines[self.cursor.0].extend(removed);
        } else {
            return;
        }
        self.preferred_col = self.cursor.1;
        self.modified = true;
    }

    fn split_line_at_cursor(&mut self) {
        self.snapshot();
        let row = self.cursor.0;
        let col = self.cursor.1;
        let line = &mut self.lines[row];
        let tail: Vec<char> = line.drain(col..).collect();
        self.lines.insert(row + 1, tail);
        self.cursor = (row + 1, 0);
        self.preferred_col = 0;
        self.modified = true;
    }

    fn delete_char_at_cursor(&mut self) {
        let line_len = self.line_len(self.cursor.0);
        if line_len == 0 || self.cursor.1 >= line_len {
            return;
        }
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        let removed = line.remove(self.cursor.1);
        self.yank = YankRegister {
            text: removed.to_string(),
            linewise: false,
        };
        clipboard::copy(&self.yank.text);
        if self.cursor.1 >= line.len() && line.len() > 0 {
            self.cursor.1 = line.len() - 1;
        }
        self.modified = true;
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor.1 == 0 {
            return;
        }
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        line.remove(self.cursor.1 - 1);
        self.cursor.1 -= 1;
        self.modified = true;
    }

    fn delete_current_line(&mut self) {
        self.snapshot();
        let text: String = self.lines[self.cursor.0].iter().collect();
        self.yank = YankRegister {
            text: text.clone(),
            linewise: true,
        };
        clipboard::copy(&format!("{}\n", text));
        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(self.cursor.0);
            if self.cursor.0 >= self.lines.len() {
                self.cursor.0 = self.lines.len() - 1;
            }
        }
        self.cursor.1 = 0;
        self.preferred_col = 0;
        self.modified = true;
    }

    fn delete_to_end_of_line(&mut self) {
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        let removed: String = line.drain(self.cursor.1..).collect();
        self.yank = YankRegister {
            text: removed.clone(),
            linewise: false,
        };
        clipboard::copy(&removed);
        if self.cursor.1 > 0 && self.cursor.1 >= line.len() {
            self.cursor.1 = line.len().saturating_sub(1);
        }
        self.modified = true;
    }

    fn yank_line(&mut self) {
        let text: String = self.lines[self.cursor.0].iter().collect();
        self.yank = YankRegister {
            text: text.clone(),
            linewise: true,
        };
        clipboard::copy(&format!("{}\n", text));
        self.status = "Yanked 1 line".into();
    }

    fn yank_selection(&mut self) {
        let (start, end, linewise) = self.selection_range();
        let text = self.collect_range(start, end, linewise);
        self.yank = YankRegister {
            text: text.clone(),
            linewise,
        };
        clipboard::copy(&text);
        self.status = "Yanked".into();
        self.mode = EditorMode::Normal;
        self.anchor = None;
    }

    fn delete_selection(&mut self) {
        let (start, end, linewise) = self.selection_range();
        let text = self.collect_range(start, end, linewise);
        self.snapshot();
        self.remove_range(start, end, linewise);
        self.yank = YankRegister { text, linewise };
        clipboard::copy(&self.yank.text);
        self.modified = true;
        self.mode = EditorMode::Normal;
        self.anchor = None;
    }

    fn paste_after(&mut self) {
        if self.yank.text.is_empty() {
            return;
        }
        self.snapshot();
        if self.yank.linewise {
            let chunks: Vec<Vec<char>> = self
                .yank
                .text
                .trim_end_matches('\n')
                .split('\n')
                .map(|s| s.chars().collect())
                .collect();
            let row = self.cursor.0 + 1;
            for (i, line) in chunks.into_iter().enumerate() {
                self.lines.insert(row + i, line);
            }
            self.cursor = (row, 0);
            self.preferred_col = 0;
        } else {
            let line_len = self.line_len(self.cursor.0);
            let mut col = self.cursor.1;
            if line_len > 0 {
                col = (col + 1).min(line_len);
            }
            self.insert_text_at(col, &self.yank.text.clone());
            self.cursor.1 = col + self.yank.text.chars().count().saturating_sub(1);
        }
        self.modified = true;
    }

    fn paste_before(&mut self) {
        if self.yank.text.is_empty() {
            return;
        }
        self.snapshot();
        if self.yank.linewise {
            let chunks: Vec<Vec<char>> = self
                .yank
                .text
                .trim_end_matches('\n')
                .split('\n')
                .map(|s| s.chars().collect())
                .collect();
            let row = self.cursor.0;
            for (i, line) in chunks.into_iter().enumerate() {
                self.lines.insert(row + i, line);
            }
            self.cursor = (row, 0);
            self.preferred_col = 0;
        } else {
            let col = self.cursor.1;
            self.insert_text_at(col, &self.yank.text.clone());
        }
        self.modified = true;
    }

    fn insert_text_at(&mut self, col: usize, text: &str) {
        let row = self.cursor.0;
        let line = &mut self.lines[row];
        let col = col.min(line.len());
        for (i, c) in text.chars().enumerate() {
            line.insert(col + i, c);
        }
    }

    fn selection_range(&self) -> ((usize, usize), (usize, usize), bool) {
        let a = self.anchor.unwrap_or(self.cursor);
        let b = self.cursor;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let linewise = self.mode == EditorMode::VisualLine;
        (start, end, linewise)
    }

    fn collect_range(
        &self,
        start: (usize, usize),
        end: (usize, usize),
        linewise: bool,
    ) -> String {
        if linewise {
            let mut out = String::new();
            for r in start.0..=end.0 {
                let line: String = self.lines[r].iter().collect();
                out.push_str(&line);
                out.push('\n');
            }
            return out;
        }
        if start.0 == end.0 {
            let line = &self.lines[start.0];
            let s = start.1.min(line.len());
            let e = (end.1 + 1).min(line.len());
            return line[s..e].iter().collect();
        }
        let mut out = String::new();
        let first = &self.lines[start.0];
        let s = start.1.min(first.len());
        out.extend(first[s..].iter());
        out.push('\n');
        for r in start.0 + 1..end.0 {
            out.push_str(&self.lines[r].iter().collect::<String>());
            out.push('\n');
        }
        let last = &self.lines[end.0];
        let e = (end.1 + 1).min(last.len());
        out.extend(last[..e].iter());
        out
    }

    fn remove_range(&mut self, start: (usize, usize), end: (usize, usize), linewise: bool) {
        if linewise {
            let count = end.0 - start.0 + 1;
            if count >= self.lines.len() {
                self.lines.clear();
                self.lines.push(Vec::new());
            } else {
                self.lines.drain(start.0..start.0 + count);
            }
            if start.0 >= self.lines.len() {
                self.cursor = (self.lines.len() - 1, 0);
            } else {
                self.cursor = (start.0, 0);
            }
            self.preferred_col = 0;
            return;
        }
        if start.0 == end.0 {
            let line = &mut self.lines[start.0];
            let s = start.1.min(line.len());
            let e = (end.1 + 1).min(line.len());
            line.drain(s..e);
            self.cursor = (start.0, s.min(line.len().saturating_sub(0)));
        } else {
            let first = &mut self.lines[start.0];
            let s = start.1.min(first.len());
            first.truncate(s);
            let last = self.lines.remove(end.0);
            let last_end = (end.1 + 1).min(last.len());
            let tail: Vec<char> = last[last_end..].to_vec();
            if end.0 - start.0 > 1 {
                self.lines.drain(start.0 + 1..end.0);
            }
            self.lines[start.0].extend(tail);
            self.cursor = (start.0, s);
        }
        self.preferred_col = self.cursor.1;
    }

    fn repeat_search(&mut self, forward: bool) {
        let Some(pattern) = self.last_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        if pattern.is_empty() {
            return;
        }
        let mut hits: Vec<(usize, usize)> = Vec::new();
        for (r, line) in self.lines.iter().enumerate() {
            let s: String = line.iter().collect();
            let mut start = 0;
            while let Some(found) = s[start..].find(pattern.as_str()) {
                let abs = start + found;
                let col = s[..abs].chars().count();
                hits.push((r, col));
                start = abs + pattern.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                if start > s.len() {
                    break;
                }
            }
        }
        if hits.is_empty() {
            self.status = format!("Pattern not found: {}", pattern);
            return;
        }
        let cur = self.cursor;
        let next = if forward {
            hits.iter()
                .find(|(r, c)| (*r, *c) > cur)
                .copied()
                .unwrap_or(hits[0])
        } else {
            hits.iter()
                .rev()
                .find(|(r, c)| (*r, *c) < cur)
                .copied()
                .unwrap_or(*hits.last().unwrap())
        };
        self.cursor = next;
        self.preferred_col = next.1;
    }

    fn snapshot(&mut self) {
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(snap) = self.undo.pop() {
            self.redo.push(Snapshot {
                lines: self.lines.clone(),
                cursor: self.cursor,
            });
            self.lines = snap.lines;
            self.cursor = snap.cursor;
            self.modified = true;
            self.status = "1 change undone".into();
        } else {
            self.status = "Already at oldest change".into();
        }
    }

    fn redo(&mut self) {
        if let Some(snap) = self.redo.pop() {
            self.undo.push(Snapshot {
                lines: self.lines.clone(),
                cursor: self.cursor,
            });
            self.lines = snap.lines;
            self.cursor = snap.cursor;
            self.modified = true;
            self.status = "1 change redone".into();
        } else {
            self.status = "Already at newest change".into();
        }
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map(|l| l.len()).unwrap_or(0)
    }

    fn clamp_cursor(&mut self) {
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
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub struct EditorWidget<'a> {
    pub view: &'a mut EditorView,
    pub area_title: String,
}

impl<'a> Widget for EditorWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.view.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let modified = if self.view.modified { " [+]" } else { "" };
        let title = format!(" {}{} ", self.area_title, modified);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);
        let inner = block.inner(area);
        block.render(area, buf);

        self.view.viewport_rows = inner.height;
        self.view.ensure_cursor_visible();

        let total = self.view.lines.len();
        let gutter_width = total.to_string().len().max(2) as u16 + 1;
        let body_x = inner.x + gutter_width + 1;
        let body_w = inner.width.saturating_sub(gutter_width + 1) as usize;

        let visible_range_end = (self.view.scroll_row + inner.height as usize).min(total);
        let visible_lines_strings: Vec<String> = self
            .view
            .lines
            .iter()
            .take(visible_range_end)
            .map(|l| l.iter().collect())
            .collect();
        let highlighted = self.view.highlighter.highlight_lines(&visible_lines_strings);

        let (sel_start, sel_end, sel_linewise, sel_active) = match self.view.mode {
            EditorMode::Visual | EditorMode::VisualLine => {
                let (s, e, lw) = self.view.selection_range();
                (s, e, lw, true)
            }
            _ => ((0, 0), (0, 0), false, false),
        };

        for (i, row_idx) in (self.view.scroll_row..visible_range_end).enumerate() {
            let cell_y = inner.y + i as u16;
            let gutter = format!("{:>width$} ", row_idx + 1, width = gutter_width as usize - 1);
            for (gi, ch) in gutter.chars().enumerate() {
                if (inner.x + gi as u16) >= inner.x + inner.width {
                    break;
                }
                buf[(inner.x + gi as u16, cell_y)]
                    .set_char(ch)
                    .set_style(Style::default().fg(Color::DarkGray));
            }

            let row_highlight = highlighted.get(row_idx);
            let mut char_idx = 0usize;
            if let Some(spans) = row_highlight {
                for (style, text) in spans {
                    for ch in text.chars() {
                        if char_idx >= body_w {
                            break;
                        }
                        let mut cell_style = *style;
                        if sel_active && in_selection(row_idx, char_idx, sel_start, sel_end, sel_linewise) {
                            cell_style = cell_style.bg(Color::Rgb(33, 66, 131));
                        }
                        if self.view.focused
                            && row_idx == self.view.cursor.0
                            && char_idx == self.view.cursor.1
                        {
                            cell_style = cell_style.add_modifier(Modifier::REVERSED);
                        }
                        buf[(body_x + char_idx as u16, cell_y)]
                            .set_char(ch)
                            .set_style(cell_style);
                        char_idx += 1;
                    }
                    if char_idx >= body_w {
                        break;
                    }
                }
            }

            if self.view.focused
                && row_idx == self.view.cursor.0
                && char_idx == self.view.cursor.1
                && char_idx < body_w
            {
                buf[(body_x + char_idx as u16, cell_y)]
                    .set_char(' ')
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
            if sel_active {
                let line_len = self.view.lines[row_idx].len();
                let mut x = char_idx.max(line_len);
                while sel_linewise && x < body_w && in_selection(row_idx, x, sel_start, sel_end, true) {
                    buf[(body_x + x as u16, cell_y)]
                        .set_char(' ')
                        .set_style(Style::default().bg(Color::Rgb(33, 66, 131)));
                    x += 1;
                }
            }
        }
    }
}

fn in_selection(
    row: usize,
    col: usize,
    start: (usize, usize),
    end: (usize, usize),
    linewise: bool,
) -> bool {
    if linewise {
        return row >= start.0 && row <= end.0;
    }
    if row < start.0 || row > end.0 {
        return false;
    }
    if start.0 == end.0 {
        return col >= start.1 && col <= end.1;
    }
    if row == start.0 {
        return col >= start.1;
    }
    if row == end.0 {
        return col <= end.1;
    }
    true
}

pub fn render_command_line(area: Rect, buf: &mut Buffer, view: &EditorView) {
    let (prefix, text) = match view.mode {
        EditorMode::Command => (":", view.command.as_str()),
        EditorMode::Search => ("/", view.search.as_str()),
        _ => ("", ""),
    };
    let style = Style::default().fg(Color::Yellow);
    let line = format!("{}{}", prefix, text);
    for (i, ch) in line.chars().enumerate() {
        if (i as u16) >= area.width {
            break;
        }
        buf[(area.x + i as u16, area.y)]
            .set_char(ch)
            .set_style(style);
    }
}
