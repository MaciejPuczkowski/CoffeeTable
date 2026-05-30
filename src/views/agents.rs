use crate::config::AiConfig;
use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const ACTIVITY_FRESH_SECS: u64 = 3;
const FOOTER_ROWS_TO_IGNORE: u16 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LaneStatus {
    Working,
    Idle,
}

pub struct AgentSession {
    pub name: String,
    pub parser: Arc<Mutex<vt100::Parser>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    pub last_rows: u16,
    pub last_cols: u16,
    pub scrollback: usize,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pre_session_files: HashSet<String>,
    last_screen_hash: u64,
    last_activity_at: Instant,
    last_bell_count: usize,
    pub bells_pending: usize,
}

impl AgentSession {
    pub fn spawn(
        ai: &AiConfig,
        name: String,
        cwd: &Path,
        system_prompt_extra: Option<&str>,
        resume_session: Option<&str>,
        rows: u16,
        cols: u16,
    ) -> Result<Self> {
        let pre_session_files = list_session_files(cwd);
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;
        let cmd = build_command(ai, cwd, system_prompt_extra, resume_session);
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("spawn agent `{}`", ai.binary))?;
        drop(pair.slave);

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let writer = pair.master.take_writer().context("pty take_writer")?;
        let reader = pair
            .master
            .try_clone_reader()
            .context("pty clone_reader")?;
        spawn_reader(reader, parser.clone());
        Ok(Self {
            name,
            parser,
            writer: Arc::new(Mutex::new(writer)),
            master: pair.master,
            _child: child,
            last_rows: rows,
            last_cols: cols,
            scrollback: 0,
            session_id: resume_session.map(|s| s.to_string()),
            cwd: cwd.to_path_buf(),
            pre_session_files,
            last_screen_hash: 0,
            last_activity_at: Instant::now(),
            last_bell_count: 0,
            bells_pending: 0,
        })
    }

    pub fn poll_status(&mut self) {
        let Ok(p) = self.parser.lock() else { return };
        let screen = p.screen();
        let (rows, cols) = screen.size();
        let body_end = rows.saturating_sub(FOOTER_ROWS_TO_IGNORE);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (i, line) in screen.rows(0, cols).enumerate() {
            if (i as u16) >= body_end {
                break;
            }
            line.hash(&mut hasher);
        }
        let hash = hasher.finish();
        let bells = screen.audible_bell_count();
        drop(p);
        if hash != self.last_screen_hash {
            self.last_screen_hash = hash;
            self.last_activity_at = Instant::now();
        }
        if bells > self.last_bell_count {
            self.bells_pending = self.bells_pending.saturating_add(bells - self.last_bell_count);
            self.last_bell_count = bells;
        }
    }

    pub fn clear_attention(&mut self) {
        self.bells_pending = 0;
    }

    pub fn lane_status(&self) -> LaneStatus {
        if self.last_activity_at.elapsed().as_secs() <= ACTIVITY_FRESH_SECS {
            LaneStatus::Working
        } else {
            LaneStatus::Idle
        }
    }

    pub fn activity_age_label(&self) -> String {
        let s = self.last_activity_at.elapsed().as_secs();
        if s < 60 {
            format!("{}s", s)
        } else if s < 3600 {
            format!("{}m", s / 60)
        } else {
            format!("{}h", s / 3600)
        }
    }

    pub fn try_capture_session_id(&mut self) -> bool {
        if self.session_id.is_some() {
            return false;
        }
        let Some(dir) = crate::config::claude_projects_dir(&self.cwd) else { return false };
        let Ok(entries) = std::fs::read_dir(&dir) else { return false };
        let mut best: Option<(String, std::time::SystemTime)> = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()) else { continue };
            if self.pre_session_files.contains(&stem) {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                best = Some((stem, mtime));
            }
        }
        if let Some((id, _)) = best {
            self.session_id = Some(id);
            return true;
        }
        false
    }

    pub fn scroll(&mut self, delta: i32) {
        self.scrollback =
            crate::views::terminal::apply_scroll(&self.parser, self.scrollback, delta);
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

    pub fn write_bytes(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }
}

fn build_command(
    ai: &AiConfig,
    cwd: &Path,
    system_prompt_extra: Option<&str>,
    resume_session: Option<&str>,
) -> CommandBuilder {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = CommandBuilder::new("cmd");
        c.arg("/C");
        c.arg(&ai.binary);
        c
    } else {
        CommandBuilder::new(&ai.binary)
    };
    if let Some(id) = resume_session {
        if ai.provider == "claude_cli" {
            cmd.arg("--resume");
            cmd.arg(id);
        }
    }
    if let Some(model) = &ai.model {
        cmd.arg("--model");
        cmd.arg(model);
    }
    if let Some(extra) = system_prompt_extra {
        if !extra.trim().is_empty() {
            cmd.arg("--append-system-prompt");
            cmd.arg(extra);
        }
    }
    for arg in &ai.extra_args {
        cmd.arg(arg);
    }
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd
}

fn list_session_files(cwd: &Path) -> HashSet<String> {
    let Some(dir) = crate::config::claude_projects_dir(cwd) else { return HashSet::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return HashSet::new() };
    let mut out = HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            out.insert(stem.to_string());
        }
    }
    out
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

pub struct AgentWidget<'a> {
    pub session: &'a mut AgentSession,
    pub focused: bool,
}

impl<'a> Widget for AgentWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.session.resize(area.height, area.width);
        if let Ok(mut p) = self.session.parser.lock() {
            p.set_scrollback(self.session.scrollback);
        }
        crate::views::terminal::render_pty_screen(area, buf, &self.session.parser, self.focused);
    }
}
