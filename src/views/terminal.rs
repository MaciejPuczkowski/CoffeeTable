use crate::config::ShellConfig;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct TerminalView {
    pub parser: Arc<Mutex<vt100::Parser>>,
    pub writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    pub last_rows: u16,
    pub last_cols: u16,
    pub scrollback: usize,
    pub selection: Option<PtySelection>,
    pub drag_anchor: Option<(u16, u16)>,
    pub last_render_area: Option<Rect>,
}

#[derive(Clone, Copy, Debug)]
pub struct PtySelection {
    pub start: (u16, u16),
    pub end: (u16, u16),
}

impl PtySelection {
    pub fn normalized(self) -> ((u16, u16), (u16, u16)) {
        let (a, b) = (self.start, self.end);
        let before = a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1);
        if before { (a, b) } else { (b, a) }
    }

    pub fn contains(self, row: u16, col: u16) -> bool {
        let (a, b) = self.normalized();
        if row < a.0 || row > b.0 {
            return false;
        }
        if a.0 == b.0 {
            return col >= a.1 && col <= b.1;
        }
        if row == a.0 {
            return col >= a.1;
        }
        if row == b.0 {
            return col <= b.1;
        }
        true
    }
}

impl TerminalView {
    pub fn spawn(shell: &ShellConfig, cwd: &Path, rows: u16, cols: u16) -> Result<Self> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;
        let mut cmd = CommandBuilder::new(&shell.command);
        for a in &shell.args {
            cmd.arg(a);
        }
        cmd.cwd(cwd);
        for (k, v) in std::env::vars() {
            cmd.env(k, v);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("spawn shell `{}`", shell.command))?;
        drop(pair.slave);

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 1000)));
        let writer = pair
            .master
            .take_writer()
            .context("pty take_writer")?;
        let reader = pair
            .master
            .try_clone_reader()
            .context("pty clone_reader")?;
        spawn_reader(reader, parser.clone());

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            _child: child,
            last_rows: rows,
            last_cols: cols,
            scrollback: 0,
            selection: None,
            drag_anchor: None,
            last_render_area: None,
        })
    }

    pub fn mouse_press(&mut self, area: Rect, col: u16, row: u16) {
        if let Some(cell) = screen_cell_from_xy(area, col, row) {
            self.drag_anchor = Some(cell);
            self.selection = Some(PtySelection { start: cell, end: cell });
        } else {
            self.drag_anchor = None;
            self.selection = None;
        }
    }

    pub fn mouse_drag(&mut self, area: Rect, col: u16, row: u16) {
        let Some(start) = self.drag_anchor else { return };
        let Some(end) = screen_cell_from_xy_clamped(area, col, row) else {
            return;
        };
        self.selection = Some(PtySelection { start, end });
    }

    pub fn mouse_release(&mut self) -> Option<String> {
        let sel = self.selection?;
        self.drag_anchor = None;
        if sel.start == sel.end {
            self.selection = None;
            return None;
        }
        let text = extract_pty_selection(&self.parser, sel);
        if text.trim().is_empty() {
            self.selection = None;
            return None;
        }
        Some(text)
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.drag_anchor = None;
    }

    pub fn scroll(&mut self, delta: i32) {
        self.scrollback = apply_scroll(&self.parser, self.scrollback, delta);
    }

    pub fn reset_scrollback(&mut self) {
        self.scrollback = 0;
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 || (rows == self.last_rows && cols == self.last_cols) {
            return;
        }
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
        self.last_rows = rows;
        self.last_cols = cols;
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }
}

pub fn apply_scroll(parser: &Arc<Mutex<vt100::Parser>>, current: usize, delta: i32) -> usize {
    let Ok(mut p) = parser.lock() else { return current };
    let requested = if delta >= 0 {
        current.saturating_add(delta as usize)
    } else {
        current.saturating_sub((-delta) as usize)
    };
    p.set_scrollback(requested);
    p.screen().scrollback()
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, parser: Arc<Mutex<vt100::Parser>>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut p) = parser.lock() {
                        p.process(&buf[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub fn key_to_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(b"\x7f".to_vec()),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => Some(b"\x1b".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::F(n) if (1..=4).contains(&n) => {
            Some(format!("\x1bO{}", (b'P' + n - 1) as char).into_bytes())
        }
        KeyCode::F(n) if (5..=12).contains(&n) => {
            let code = match n {
                5 => 15, 6 => 17, 7 => 18, 8 => 19, 9 => 20, 10 => 21, 11 => 23, 12 => 24,
                _ => return None,
            };
            Some(format!("\x1b[{}~", code).into_bytes())
        }
        KeyCode::Char(c) => {
            if ctrl && alt && !c.is_ascii() {
                return Some(c.to_string().into_bytes());
            }
            if ctrl && c.is_ascii() {
                let lower = c.to_ascii_lowercase();
                if ('a'..='z').contains(&lower) {
                    return Some(vec![lower as u8 - b'a' + 1]);
                }
                match lower {
                    '@' | ' ' => return Some(vec![0]),
                    '[' => return Some(vec![0x1b]),
                    '\\' => return Some(vec![0x1c]),
                    ']' => return Some(vec![0x1d]),
                    '^' => return Some(vec![0x1e]),
                    '_' => return Some(vec![0x1f]),
                    _ => {}
                }
            }
            if alt {
                let mut bytes = vec![0x1b];
                bytes.extend(c.to_string().bytes());
                return Some(bytes);
            }
            Some(c.to_string().into_bytes())
        }
        _ => None,
    }
}

pub struct TerminalWidget<'a> {
    pub view: &'a mut TerminalView,
    pub focused: bool,
}

impl<'a> Widget for TerminalWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.view.resize(area.height, area.width);
        if let Ok(mut p) = self.view.parser.lock() {
            p.set_scrollback(self.view.scrollback);
        }
        self.view.last_render_area = Some(area);
        render_pty_screen_sel(
            area,
            buf,
            &self.view.parser,
            self.focused,
            self.view.selection,
        );
    }
}

pub fn render_pty_screen_sel(
    area: Rect,
    buf: &mut Buffer,
    parser: &Arc<Mutex<vt100::Parser>>,
    focused: bool,
    selection: Option<PtySelection>,
) {
    let Ok(parser) = parser.lock() else { return };
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let max_rows = rows.min(area.height);
    let max_cols = cols.min(area.width);
    for r in 0..max_rows {
        for c in 0..max_cols {
            if let Some(cell) = screen.cell(r, c) {
                let mut style = Style::default();
                if let Some(fg) = vt_color_to_ratatui(cell.fgcolor()) {
                    style = style.fg(fg);
                }
                if let Some(bg) = vt_color_to_ratatui(cell.bgcolor()) {
                    style = style.bg(bg);
                }
                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                if let Some(sel) = selection {
                    if sel.contains(r, c) {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                }
                let ch = cell.contents().chars().next().unwrap_or(' ');
                let cell_pos = (area.x + c, area.y + r);
                buf[cell_pos].set_char(ch).set_style(style);
            }
        }
    }
    if focused && !screen.hide_cursor() {
        let (cy, cx) = screen.cursor_position();
        if cy < area.height && cx < area.width {
            let pos = (area.x + cx, area.y + cy);
            let mut style = buf[pos].style();
            style = style.add_modifier(Modifier::REVERSED);
            buf[pos].set_style(style);
        }
    }
}

fn vt_color_to_ratatui(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

pub fn screen_cell_from_xy(area: Rect, col: u16, row: u16) -> Option<(u16, u16)> {
    if col < area.x || row < area.y {
        return None;
    }
    let cx = col - area.x;
    let cy = row - area.y;
    if cx >= area.width || cy >= area.height {
        return None;
    }
    Some((cy, cx))
}

pub fn screen_cell_from_xy_clamped(area: Rect, col: u16, row: u16) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let cx = col.saturating_sub(area.x).min(area.width.saturating_sub(1));
    let cy = row.saturating_sub(area.y).min(area.height.saturating_sub(1));
    Some((cy, cx))
}

pub fn extract_pty_selection(parser: &Arc<Mutex<vt100::Parser>>, sel: PtySelection) -> String {
    let Ok(p) = parser.lock() else { return String::new() };
    let screen = p.screen();
    let (rows, cols) = screen.size();
    let (start, end) = sel.normalized();
    let mut out = String::new();
    for r in start.0..=end.0 {
        if r >= rows {
            break;
        }
        let (cs, ce) = if start.0 == end.0 {
            (start.1, end.1)
        } else if r == start.0 {
            (start.1, cols.saturating_sub(1))
        } else if r == end.0 {
            (0, end.1)
        } else {
            (0, cols.saturating_sub(1))
        };
        let mut line = String::new();
        let mut c = cs;
        while c <= ce && c < cols {
            if let Some(cell) = screen.cell(r, c) {
                let s = cell.contents();
                if s.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(&s);
                }
            } else {
                line.push(' ');
            }
            c += 1;
        }
        let trimmed = line.trim_end().to_string();
        out.push_str(&trimmed);
        if r != end.0 {
            out.push('\n');
        }
    }
    out
}
