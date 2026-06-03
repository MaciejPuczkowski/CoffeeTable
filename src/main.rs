mod agent_cli;
mod ai;
mod app;
mod clipboard;
mod config;
mod db;
mod discovery;
mod git;
mod github;
mod icons;
mod log;
mod project;
mod runtime;
mod syntax;
mod token_usage;
mod ui;
mod views;

use anyhow::Result;
use app::App;
use config::Paths;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use db::Db;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, time::Duration};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("agent") {
        return agent_cli::run(&args[1..]);
    }
    let paths = Paths::resolve()?;
    install_panic_hook(paths.data_dir.clone());
    let db = Db::open(&paths.db_file)?;
    let mut app = App::new(db, paths)?;
    app.restore_all_agents()?;

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;

    if let Err(e) = &result {
        eprintln!("coffeetable error: {e:?}");
    }
    app.persist_all()?;
    result
}

fn install_panic_hook(data_dir: std::path::PathBuf) {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::create_dir_all(&data_dir);
        let path = data_dir.join("coffeetable-error.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "--- panic @ unix {} ---", now);
            let _ = writeln!(f, "{info}");
            let _ = writeln!(f, "{}", std::backtrace::Backtrace::force_capture());
            let _ = writeln!(f);
        }
        original(info);
    }));
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        app.tick();
        terminal.draw(|f| ui::render(app, f))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if let Err(e) = app.on_key(key) {
                            app.log_error(format!("on_key: {:#}", e));
                            app.status = format!("Error: {:#}", e);
                        }
                    }
                }
                Event::Mouse(m) => {
                    if let Err(e) = app.on_mouse(m) {
                        app.log_error(format!("on_mouse: {:#}", e));
                        app.status = format!("Error: {:#}", e);
                    }
                }
                Event::Paste(text) => app.on_paste(text),
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
