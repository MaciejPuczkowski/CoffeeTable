mod agent_cli;
mod ai;
mod app;
mod clipboard;
mod config;
mod db;
mod discovery;
mod git;
mod icons;
mod project;
mod syntax;
mod ui;
mod views;

use anyhow::Result;
use app::App;
use config::Paths;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
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

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
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
                        app.on_key(key)?;
                    }
                }
                Event::Mouse(m) => app.on_mouse(m)?,
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
