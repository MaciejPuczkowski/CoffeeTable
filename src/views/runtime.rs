use crate::runtime::{LogKind, Runtime, ServiceStatus, human_bytes};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

pub struct RuntimePanesRects {
    pub list: Rect,
    pub log: Rect,
}

pub fn render_runtime(
    runtime: &Runtime,
    frame: &mut Frame<'_>,
    area: Rect,
    project_name: &str,
) -> RuntimePanesRects {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(20)])
        .split(area);
    render_service_list(runtime, frame, chunks[0], project_name);
    render_log(runtime, frame, chunks[1]);
    RuntimePanesRects {
        list: chunks[0],
        log: chunks[1],
    }
}

fn render_service_list(runtime: &Runtime, frame: &mut Frame<'_>, area: Rect, project_name: &str) {
    let title = format!(" Runtime — {} ", project_name);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);

    if runtime.services.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines = empty_state_lines(runtime);
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let mut items: Vec<ListItem> = Vec::with_capacity(runtime.services.len());
    for service in &runtime.services {
        let badge = status_badge(service.status);
        let resources = if service.pid.is_some() {
            format!(
                " pid {:<6} cpu {:>5.1}%  ram {:>6}",
                service.pid.unwrap_or(0),
                service.cpu_pct,
                human_bytes(service.ram_bytes)
            )
        } else {
            String::new()
        };
        let filtered = runtime
            .filter
            .as_ref()
            .map(|f| f == &service.config.name)
            .unwrap_or(false);
        let name_style = if filtered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        let mut header = Line::from(vec![
            badge,
            Span::raw(" "),
            Span::styled(service.config.name.clone(), name_style),
        ]);
        if filtered {
            header.spans.push(Span::styled(
                "  ◀ filter",
                Style::default().fg(Color::Cyan),
            ));
        }
        let detail = Line::from(Span::styled(
            resources,
            Style::default().fg(Color::DarkGray),
        ));
        let cmd_line = Line::from(Span::styled(
            format!("   {}", service.config.command),
            Style::default().fg(Color::DarkGray),
        ));
        items.push(ListItem::new(vec![header, detail, cmd_line]));
    }
    let mut list_state = ListState::default();
    if !runtime.services.is_empty() {
        list_state.select(Some(runtime.selected.min(runtime.services.len() - 1)));
    }
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_log(runtime: &Runtime, frame: &mut Frame<'_>, area: Rect) {
    let filter_label = match &runtime.filter {
        Some(name) => format!(" Output — {} ", name),
        None => " Output — all services ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(filter_label);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let log_snapshot: Vec<crate::runtime::LogLine> = {
        let Ok(log) = runtime.log.lock() else {
            return;
        };
        log.iter()
            .filter(|line| match &runtime.filter {
                Some(f) => &line.service == f,
                None => true,
            })
            .cloned()
            .collect()
    };
    let max_rows = inner.height as usize;
    let start = log_snapshot.len().saturating_sub(max_rows);
    let visible = &log_snapshot[start..];
    let name_width = runtime
        .services
        .iter()
        .map(|s| s.config.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(16);
    let mut lines: Vec<Line> = Vec::with_capacity(visible.len());
    for entry in visible {
        let service_style = service_color(&entry.service, &runtime.services);
        let body_style = match entry.kind {
            LogKind::Stderr => Style::default().fg(Color::LightRed),
            LogKind::System => Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            LogKind::Stdout => Style::default(),
        };
        let mut name = entry.service.clone();
        if name.chars().count() > name_width && name_width > 1 {
            let mut truncated: String = name.chars().take(name_width.saturating_sub(1)).collect();
            truncated.push('…');
            name = truncated;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{:<width$}", name, width = name_width), service_style),
            Span::raw(" │ "),
            Span::styled(entry.text.clone(), body_style),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no output yet — press r to run all, b to build)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn empty_state_lines(runtime: &Runtime) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(err) = &runtime.last_load_error {
        lines.push(Line::from(Span::styled(
            "  Runtime config error:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        for chunk in err.lines() {
            lines.push(Line::from(format!("    {}", chunk)));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Fix the YAML in `:S` settings then save (Ctrl+S), or press `e` here to re-apply.",
            Style::default().fg(Color::DarkGray),
        )));
        return lines;
    }
    if !runtime.config_exists() {
        lines.push(Line::from(Span::styled(
            "  No services configured for this project.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Open settings with `:S`, add a `runtime:` block in the project (right) pane, then save (Ctrl+S).",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Example:",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        for sample in EXAMPLE_YAML.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {}", sample),
                Style::default().fg(Color::Cyan),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  After saving settings, press `e` here to re-apply (auto-applied on settings save).",
            Style::default().fg(Color::DarkGray),
        )));
        return lines;
    }
    lines.push(Line::from(Span::styled(
        "  Runtime config is empty (no services).",
        Style::default().fg(Color::DarkGray),
    )));
    lines
}

fn status_badge(status: ServiceStatus) -> Span<'static> {
    let (label, color) = match status {
        ServiceStatus::Idle => ("idle    ", Color::DarkGray),
        ServiceStatus::Building => ("building", Color::Yellow),
        ServiceStatus::Running => ("running ", Color::Green),
        ServiceStatus::Stopped => ("stopped ", Color::DarkGray),
        ServiceStatus::Exited(0) => ("exit 0  ", Color::DarkGray),
        ServiceStatus::Exited(_) => ("exit ≠0 ", Color::LightRed),
        ServiceStatus::Failed => ("failed  ", Color::Red),
    };
    let _ = status.label();
    Span::styled(
        format!("[{}]", label),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn service_color(service: &str, services: &[crate::runtime::ServiceProcess]) -> Style {
    let idx = services
        .iter()
        .position(|s| s.config.name == service)
        .unwrap_or(0);
    const PALETTE: &[Color] = &[
        Color::Cyan,
        Color::Magenta,
        Color::Green,
        Color::Yellow,
        Color::LightBlue,
        Color::LightRed,
        Color::LightGreen,
        Color::LightMagenta,
    ];
    Style::default()
        .fg(PALETTE[idx % PALETTE.len()])
        .add_modifier(Modifier::BOLD)
}

pub fn service_index_at_row(area: Rect, row: u16, service_count: usize) -> Option<usize> {
    if service_count == 0 {
        return None;
    }
    if row < area.y || row >= area.y + area.height {
        return None;
    }
    let inner_y = area.y + 1;
    let inner_h = area.height.saturating_sub(2);
    if row < inner_y || row >= inner_y + inner_h {
        return None;
    }
    let rel = (row - inner_y) as usize;
    let idx = rel / 3;
    if idx >= service_count {
        None
    } else {
        Some(idx)
    }
}

const EXAMPLE_YAML: &str = "\
runtime:
  services:
    - name: api
      command: dotnet run --project src/Api
      build: dotnet build src/Api
    - name: web
      command: npm run dev
      cwd: web
      depends_on: [api]
";
