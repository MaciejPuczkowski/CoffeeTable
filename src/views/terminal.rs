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
        })
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
        let Ok(parser) = self.view.parser.lock() else { return };
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
                    let ch = cell.contents().chars().next().unwrap_or(' ');
                    let cell_pos = (area.x + c, area.y + r);
                    buf[cell_pos].set_char(ch).set_style(style);
                }
            }
        }
        if self.focused && !screen.hide_cursor() {
            let (cy, cx) = screen.cursor_position();
            if cy < area.height && cx < area.width {
                let pos = (area.x + cx, area.y + cy);
                let mut style = buf[pos].style();
                style = style.add_modifier(Modifier::REVERSED);
                buf[pos].set_style(style);
            }
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
