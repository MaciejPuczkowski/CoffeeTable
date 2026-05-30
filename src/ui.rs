use crate::{
    app::{AiCommitState, App, AppMode, Focus, LeftPaneMode},
    views::{
        changes::ChangesWidget,
        editor::{COMMANDS, EditorMode, EditorWidget, filter_commands, render_command_line},
        file_tree::FileTreeWidget,
        grep::GrepWidget,
        project_picker::{PickerMode, ProjectPickerWidget},
    },
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render(app: &mut App, frame: &mut Frame<'_>) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(app, frame, chunks[0]);
    render_view_tabs(app, frame, chunks[1]);
    let view_mode = app
        .open_projects
        .get(app.active_index)
        .and_then(|p| app.project_views.get(&p.id))
        .map(|s| s.view_mode)
        .unwrap_or(crate::app::ViewMode::Editor);
    let body_area = split_off_agent_lane(app, frame, chunks[2]);
    match view_mode {
        crate::app::ViewMode::Terminal => render_terminal_body(app, frame, body_area),
        crate::app::ViewMode::Agents => render_agents_body(app, frame, body_area),
        crate::app::ViewMode::Project => render_project_body(app, frame, body_area),
        crate::app::ViewMode::Git => render_git_body(app, frame, body_area),
        crate::app::ViewMode::Editor => render_body(app, frame, body_area),
    }
    render_command_or_status(app, frame, chunks[3]);
    render_footer(app, frame, chunks[4]);

    match app.mode {
        AppMode::Picker => render_picker_overlay(app, frame, area),
        AppMode::Grep => render_grep_overlay(app, frame, area),
        AppMode::OpenConfirm => render_open_confirm_overlay(app, frame, area),
        AppMode::ConfirmDeleteFeature => render_delete_feature_overlay(app, frame, area),
        AppMode::Palette => render_command_palette_overlay(app, frame, area),
        AppMode::AiCommit => render_ai_commit_overlay(app, frame, area),
        AppMode::Normal | AppMode::ExplorerFilter => {}
    }
    if app.leader_pending {
        render_leader_overlay(frame, area);
    }
    if app.terminal_prefix {
        render_terminal_leader_overlay(frame, area);
    }
    if app.help_visible {
        render_help_overlay(app, frame, area);
    }
}

fn render_command_palette_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(palette) = app.palette.as_ref() else { return };
    let filtered = filter_commands(&palette.query);
    let total_rows = filtered.len() as u16 + 6;
    let popup = centered_rect_fixed(70, total_rows.min(area.height.saturating_sub(2)).max(7), area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Command ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let input = Paragraph::new(format!(":{}", palette.query))
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
    frame.render_widget(input, chunks[0]);

    let sel = if filtered.is_empty() {
        0
    } else {
        palette.selection.min(filtered.len() - 1)
    };
    let lines: Vec<Line> = if filtered.is_empty() {
        vec![Line::from(Span::styled(
            "  (no matching command — Enter runs literal text)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        filtered
            .iter()
            .enumerate()
            .map(|(row, &idx)| {
                let cmd = &COMMANDS[idx];
                let aliases = if cmd.aliases.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", cmd.aliases.join(", "))
                };
                let marker = if row == sel { "▶ " } else { "  " };
                let row_style = if row == sel {
                    Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!(":{:<5}", cmd.key),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(cmd.description.to_string(), Style::default()),
                    Span::styled(aliases, Style::default().fg(Color::DarkGray)),
                ])
                .style(row_style)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), chunks[1]);

    frame.render_widget(
        Paragraph::new("↑/↓ select • Enter run • ! suffix forces • Esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

fn render_terminal_leader_overlay(frame: &mut Frame<'_>, area: Rect) {
    let entries: &[(&str, &str)] = &[
        ("d", "Detach (back to Editor view)"),
        ("n", "New tab in this view"),
        ("l", "Next tab"),
        ("h", "Previous tab"),
        ("x", "Close current tab"),
        ("Space", "Send literal Ctrl+Space"),
        ("p", "Projects (picker)"),
        ("f", "Find file"),
        ("g", "Grep"),
        ("c", "Changes ↔ tree (toggle)"),
        ("e", "Explorer (focus tree)"),
        ("b", "Buffer (focus editor)"),
        ("w", "Show Working copy (editor)"),
        ("C", "AI commit"),
        ("t", "Terminal (focus or create)"),
        ("T", "New terminal"),
        ("P", "Project view"),
        ("G", "Git view (branches + commits)"),
        ("a", "Agent for selected feature"),
        ("L", "Toggle Agents lane (right side)"),
        ("z", "Toggle wrap (none / 120 / 80)"),
        ("?", "Help"),
        ("q", "Quit"),
        ("Esc", "Cancel prefix"),
    ];
    let popup_height = entries.len() as u16 + 4;
    let popup = bottom_centered(60, popup_height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Terminal <leader>  (Ctrl+Space) ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines: Vec<Line> = Vec::with_capacity(entries.len() + 2);
    lines.push(Line::from(""));
    for (key, desc) in entries {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<6}", key),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(desc.to_string()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "  Press the listed key, or any other to cancel",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_leader_overlay(frame: &mut Frame<'_>, area: Rect) {
    let entries: &[(&str, &str)] = &[
        ("p", "Projects (picker)"),
        ("f", "Find file"),
        ("g", "Grep"),
        ("c", "Changes ↔ tree (toggle)"),
        ("e", "Explorer (focus tree)"),
        ("b", "Buffer (focus editor)"),
        ("w", "Show Working copy (editor)"),
        ("h", "Show HEAD version (editor)"),
        ("d", "Show Diff vs HEAD (editor)"),
        ("C", "AI commit (generate message, review, commit)"),
        ("t", "Terminal (focus existing or create first)"),
        ("T", "New terminal (always create)"),
        ("P", "Project view (meta + features)"),
        ("G", "Git view (branches + commits)"),
        ("a", "Agent for selected feature"),
        ("L", "Toggle Agents lane (right side)"),
        ("z", "Toggle wrap (none / 120 / 80)"),
        ("?", "Help"),
        ("q", "Quit"),
    ];
    let popup_height = entries.len() as u16 + 4;
    let popup = bottom_centered(60, popup_height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" <leader>  (Space) ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::with_capacity(entries.len() + 2);
    lines.push(Line::from(""));
    for (key, desc) in entries {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<6}", key),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(desc.to_string()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "  Esc cancel",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn bottom_centered(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + area.height.saturating_sub(h + 2);
    Rect::new(x, y, w, h)
}

fn render_view_tabs(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    app.view_tabs_area = area;
    app.view_tab_rects.clear();
    let active_view = app
        .open_projects
        .get(app.active_index)
        .and_then(|p| app.project_views.get(&p.id))
        .map(|s| s.view_mode)
        .unwrap_or(crate::app::ViewMode::Editor);
    let mut spans: Vec<Span> = Vec::new();
    let mut x = area.x + 1;
    for (mode, label) in &[
        (crate::app::ViewMode::Editor, "Editor"),
        (crate::app::ViewMode::Terminal, "Terminal"),
        (crate::app::ViewMode::Agents, "Agents"),
        (crate::app::ViewMode::Project, "Project"),
        (crate::app::ViewMode::Git, "Git"),
    ] {
        let text = format!(" {} ", label);
        let w = text.chars().count() as u16;
        let style = if active_view == *mode {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        app.view_tab_rects
            .push((*mode, Rect::new(x, area.y, w, 1)));
        spans.push(Span::styled(text, style));
        x += w;
        spans.push(Span::raw("  "));
        x += 2;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_project_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    use crate::project::{CommentStatus, StepStatus};
    use crate::views::project_view::{ProjectSection, ProjectSelection};
    use ratatui::widgets::{List, ListItem, StatefulWidget};

    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        return;
    };
    let needs_load = app
        .project_views
        .get(&project.id)
        .map(|s| s.project_view.is_none())
        .unwrap_or(false);
    if needs_load {
        app.ensure_project_view_loaded();
    }
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Min(20)])
        .split(area);
    app.left_pane_area = chunks[0];
    app.right_pane_area = chunks[1];
    app.project_list_inner = Block::default().borders(Borders::ALL).inner(chunks[0]);

    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let focus = state.focus;
    let Some(model) = state.project_view.as_mut() else {
        let p = Paragraph::new("Project view unavailable (db error).")
            .block(Block::default().borders(Borders::ALL).title(" Project "));
        frame.render_widget(p, area);
        return;
    };

    let in_form = model.feature_form.is_some();
    let tree_focused = matches!(focus, Focus::Tree);
    let editor_focused = matches!(focus, Focus::Editor);
    let _ = in_form;

    let sections = ProjectSection::all();
    let mut items: Vec<ListItem> = Vec::with_capacity(model.rows());
    for s in sections {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!("  {}", s.label()),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])));
    }
    items.push(ListItem::new(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "+ New Feature",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ])));
    let form_overlay = model.feature_form.as_ref().and_then(|f| {
        f.feature_id.map(|id| (id, f.status, f.title.clone()))
    });
    for f in &model.features {
        let (status, title) = match form_overlay {
            Some((fid, st, ref t)) if fid == f.id => (st, t.clone()),
            _ => (f.status, f.title.clone()),
        };
        let badge_color = feature_status_color(status);
        let mut title_style = Style::default();
        if status.is_closed() {
            title_style = title_style
                .fg(Color::DarkGray)
                .add_modifier(Modifier::CROSSED_OUT);
        }
        items.push(ListItem::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("[{}] ", status.label()),
                Style::default().fg(badge_color),
            ),
            Span::styled(title, title_style),
        ])));
    }

    let list_border = if tree_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(list_border)
                .title(format!(" {} ", project.name)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(list, chunks[0], frame.buffer_mut(), &mut model.list_state);

    if model.feature_form.is_some() {
        let rects = render_feature_form(model, chunks[1], frame, editor_focused);
        app.feature_form_tab_rects = rects.tabs;
        app.feature_form_field_rects = rects.fields;
        app.feature_form_status_rects = rects.statuses;
        return;
    }
    app.feature_form_tab_rects.clear();
    app.feature_form_field_rects.clear();
    app.feature_form_status_rects.clear();

    if let Some(editor) = model.editor.as_mut() {
        editor.focused = editor_focused;
        let widget = crate::views::editor::EditorWidget {
            view: editor,
            area_title: "edit (Ctrl+S save, Esc/Backspace save+close)".into(),
            git_status: None,
        };
        frame.render_widget(widget, chunks[1]);
        return;
    }

    let detail_border = if editor_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(detail_border);
    let inner = detail_block.inner(chunks[1]);
    frame.render_widget(detail_block, chunks[1]);

    let mut lines: Vec<Line> = Vec::new();
    match model.selection {
        ProjectSelection::NewFeature => {
            lines.push(Line::from(Span::styled(
                "  + New Feature",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Press i/Enter to open the new-feature form.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  First line = title. Lines below = description.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  Ctrl+S saves.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        ProjectSelection::Meta(section) => {
            lines.push(Line::from(Span::styled(
                format!("  {}", section.label()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            let body = match section {
                ProjectSection::About => model.meta.description.clone(),
                ProjectSection::Conventions => model.meta.conventions.clone(),
                ProjectSection::AiHints => model.meta.ai_hints.clone(),
                ProjectSection::AiNotes => model.meta.ai_notes.clone(),
            };
            if body.trim().is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (empty — press i/Enter to edit)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for l in body.lines() {
                    lines.push(Line::from(format!("  {}", l)));
                }
            }
        }
        ProjectSelection::Feature(i) => {
            if let Some(feature) = model.features.get(i) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {} ", feature.title),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("[{}]", feature.status.label()),
                        Style::default().fg(feature_status_color(feature.status)),
                    ),
                ]));
                lines.push(Line::from(""));
                if feature.description.trim().is_empty() {
                    lines.push(Line::from(Span::styled(
                        "  (no description — press i/Enter to open form)",
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    for l in feature.description.lines() {
                        lines.push(Line::from(format!("  {}", l)));
                    }
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Steps",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )));
                if feature.steps.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (none yet)",
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    for step in &feature.steps {
                        let style = match step.status {
                            StepStatus::Done => Style::default().fg(Color::Green),
                            StepStatus::InProgress => Style::default().fg(Color::Yellow),
                            StepStatus::Todo => Style::default(),
                        };
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(format!("{} ", step.status.glyph()), style),
                            Span::raw(step.summary.clone()),
                        ]));
                    }
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Messages",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )));
                if feature.comments.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (none yet)",
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    for c in &feature.comments {
                        let badge_style = match c.status {
                            CommentStatus::Queued => Style::default().fg(Color::Yellow),
                            CommentStatus::Sent => Style::default().fg(Color::Cyan),
                            CommentStatus::Done => Style::default().fg(Color::Green),
                        };
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(format!("[{}] ", c.status.label()), badge_style),
                            Span::raw(c.message.clone()),
                        ]));
                    }
                }
            }
        }
    }
    let hints = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  i/Enter/l open form • n new feature • x cycle status • D delete feature",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "  ? for full help • Ctrl+J/K switch view",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    lines.extend(hints);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_git_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    use crate::views::git::{DetailsMode, GitPane};
    use ratatui::widgets::{List, ListItem, StatefulWidget, ListState};

    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        return;
    };
    let needs_load = app
        .project_views
        .get(&project.id)
        .map(|s| s.git_view.is_none())
        .unwrap_or(false);
    if needs_load {
        app.ensure_git_view_loaded();
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Min(20)])
        .split(area);
    app.left_pane_area = chunks[0];
    app.right_pane_area = chunks[1];

    let left_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Min(3)])
        .split(chunks[0]);

    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let Some(view) = state.git_view.as_mut() else {
        let p = Paragraph::new("Git view unavailable.")
            .block(Block::default().borders(Borders::ALL).title(" Git "));
        frame.render_widget(p, area);
        return;
    };
    view.branches_area = left_split[0];
    view.commits_area = left_split[1];
    view.details_area = chunks[1];

    let branches_focused = matches!(view.focus, GitPane::Branches);
    let commits_focused = matches!(view.focus, GitPane::Commits);
    let details_focused = matches!(view.focus, GitPane::Details);

    let cur_label = view
        .current_branch
        .clone()
        .unwrap_or_else(|| "(detached)".into());
    let branch_title = format!(" Branches — on {} ", cur_label);
    let mut branch_items: Vec<ListItem> = Vec::with_capacity(view.branches.len());
    for b in &view.branches {
        let marker = if b.is_current { "● " } else { "  " };
        let mut spans: Vec<Span> = Vec::new();
        let marker_color = if b.is_current { Color::Green } else { Color::DarkGray };
        spans.push(Span::styled(marker, Style::default().fg(marker_color)));
        let name_style = if b.is_remote {
            Style::default().fg(Color::Cyan)
        } else if b.is_current {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(b.name.clone(), name_style));
        if let Some(up) = &b.upstream {
            spans.push(Span::styled(
                format!("  ↑ {}", up),
                Style::default().fg(Color::DarkGray),
            ));
        }
        branch_items.push(ListItem::new(Line::from(spans)));
    }
    if branch_items.is_empty() {
        branch_items.push(ListItem::new(Line::from(Span::styled(
            "  (no branches — not a git repo?)",
            Style::default().fg(Color::DarkGray),
        ))));
    }
    let branches_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border(branches_focused))
        .title(branch_title);
    let mut branch_state = ListState::default();
    branch_state.select(Some(view.branch_idx));
    let branches_list = List::new(branch_items)
        .block(branches_block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(branches_list, left_split[0], frame.buffer_mut(), &mut branch_state);

    let commits_title = match view.selected_branch() {
        Some(b) => format!(" Commits — {} ", b.name),
        None => " Commits ".to_string(),
    };
    let mut commit_items: Vec<ListItem> = Vec::with_capacity(view.commits.len());
    for c in &view.commits {
        commit_items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("{} ", c.short_sha),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!("{} ", c.date),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(c.summary.clone()),
            Span::styled(
                format!("  ({})", c.author),
                Style::default().fg(Color::DarkGray),
            ),
        ])));
    }
    if commit_items.is_empty() {
        commit_items.push(ListItem::new(Line::from(Span::styled(
            "  (no commits)",
            Style::default().fg(Color::DarkGray),
        ))));
    }
    let commits_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border(commits_focused))
        .title(commits_title);
    let mut commit_state = ListState::default();
    commit_state.select(Some(view.commit_idx));
    let commits_list = List::new(commit_items)
        .block(commits_block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(commits_list, left_split[1], frame.buffer_mut(), &mut commit_state);

    let title_prefix = match view.details_mode {
        DetailsMode::Commit => "",
        DetailsMode::PrList => "[PRs] ",
        DetailsMode::PrView => "[PR] ",
    };
    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border(details_focused))
        .title(format!(" {}{} ", title_prefix, view.details_title));
    let inner = details_block.inner(chunks[1]);
    frame.render_widget(details_block, chunks[1]);
    let lines: Vec<Line> = view
        .details_text
        .lines()
        .map(|l| Line::from(diff_line_style(l)))
        .collect();
    let para = Paragraph::new(lines).scroll((view.details_scroll, 0));
    frame.render_widget(para, inner);
}

fn pane_border(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn diff_line_style(line: &str) -> Vec<Span<'static>> {
    let owned = line.to_string();
    if owned.starts_with("+++") || owned.starts_with("---") {
        vec![Span::styled(owned, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]
    } else if owned.starts_with("@@") {
        vec![Span::styled(owned, Style::default().fg(Color::Magenta))]
    } else if owned.starts_with('+') {
        vec![Span::styled(owned, Style::default().fg(Color::Green))]
    } else if owned.starts_with('-') {
        vec![Span::styled(owned, Style::default().fg(Color::Red))]
    } else if owned.starts_with("commit ") {
        vec![Span::styled(owned, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]
    } else if owned.starts_with("Author:") || owned.starts_with("Date:") {
        vec![Span::styled(owned, Style::default().fg(Color::DarkGray))]
    } else {
        vec![Span::raw(owned)]
    }
}

fn comment_kind_glyph(kind: crate::project::CommentKind) -> &'static str {
    use crate::project::CommentKind;
    match kind {
        CommentKind::Note => "◦",
        CommentKind::Request => "▶",
        CommentKind::Response => "◀",
    }
}

fn feature_status_color(status: crate::project::FeatureStatus) -> Color {
    use crate::project::FeatureStatus;
    match status {
        FeatureStatus::Idea => Color::Cyan,
        FeatureStatus::Todo => Color::Yellow,
        FeatureStatus::InProgress => Color::LightYellow,
        FeatureStatus::InReview => Color::Magenta,
        FeatureStatus::Done => Color::Green,
        FeatureStatus::Cancelled => Color::DarkGray,
    }
}

pub struct FeatureFormRects {
    pub tabs: Vec<(crate::views::feature_form::FormPage, Rect)>,
    pub fields: Vec<(crate::views::feature_form::FormFocus, Rect)>,
    pub statuses: Vec<(crate::project::FeatureStatus, Rect)>,
}

fn render_feature_form(
    model: &mut crate::views::project_view::ProjectViewModel,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) -> FeatureFormRects {
    use crate::views::feature_form::{FormFocus, FormPage};
    let mut rects = FeatureFormRects {
        tabs: Vec::new(),
        fields: Vec::new(),
        statuses: Vec::new(),
    };
    let Some(form) = model.feature_form.as_mut() else { return rects };

    let outer_border = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let outer_title = if form.feature_id.is_some() {
        " Feature ".to_string()
    } else {
        " New Feature ".to_string()
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(outer_border)
        .title(outer_title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(inner);
    rects.tabs = render_form_tabs(form, chunks[0], frame);

    match form.page {
        FormPage::Details => {
            let title_focused = matches!(form.focus, FormFocus::Title);
            let status_focused = matches!(form.focus, FormFocus::Status);
            let desc_focused = matches!(form.focus, FormFocus::Description);
            let steps_focused = matches!(form.focus, FormFocus::Step(_) | FormFocus::NewStep);

            let details = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Ratio(1, 2),
                    Constraint::Ratio(1, 2),
                ])
                .split(chunks[1]);
            render_form_title(form, details[0], frame, title_focused);
            rects.fields.push((FormFocus::Title, details[0]));
            rects.statuses = render_form_status(form, details[1], frame, status_focused);
            rects.fields.push((FormFocus::Status, details[1]));
            render_form_description(form, details[2], frame, desc_focused);
            rects.fields.push((FormFocus::Description, details[2]));
            let step_rects = render_form_steps(form, details[3], frame, steps_focused);
            rects.fields.extend(step_rects);
        }
        FormPage::Comments => {
            let comments_focused = matches!(
                form.focus,
                FormFocus::Comment(_) | FormFocus::NewComment
            );
            let comment_rects = render_form_comments(form, chunks[1], frame, comments_focused);
            rects.fields.extend(comment_rects);
        }
    }
    rects
}

fn render_form_tabs(
    form: &crate::views::feature_form::FeatureForm,
    area: Rect,
    frame: &mut Frame<'_>,
) -> Vec<(crate::views::feature_form::FormPage, Rect)> {
    use crate::views::feature_form::FormPage;
    let visible_comments = form.comments.iter().filter(|c| !c.deleted).count();
    let pages: [(FormPage, String); 2] = [
        (FormPage::Details, " 1·Details ".to_string()),
        (
            FormPage::Comments,
            format!(" 2·Messages ({}) ", visible_comments),
        ),
    ];
    let mut spans: Vec<Span> = Vec::new();
    let mut rects: Vec<(FormPage, Rect)> = Vec::new();
    let mut x = area.x;
    let space = Span::raw(" ");
    spans.push(space.clone());
    x = x.saturating_add(1);
    for (page, label) in pages {
        let active = page == form.page;
        let style = if active {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let w = label.chars().count() as u16;
        rects.push((page, Rect::new(x, area.y, w, 1)));
        spans.push(Span::styled(label, style));
        x = x.saturating_add(w);
        spans.push(space.clone());
        x = x.saturating_add(1);
    }
    spans.push(Span::styled(
        " Tab to switch",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    rects
}

fn render_form_title(
    form: &crate::views::feature_form::FeatureForm,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) {
    let block = field_block("Title", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let placeholder = "(type a title)";
    let text = form.title.as_str();
    if focused {
        render_inline_input(frame, inner, text, form.cursor, placeholder);
    } else if text.trim().is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                placeholder,
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
    } else {
        frame.render_widget(Paragraph::new(Line::from(text.to_string())), inner);
    }
}

fn render_form_status(
    form: &crate::views::feature_form::FeatureForm,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) -> Vec<(crate::project::FeatureStatus, Rect)> {
    use crate::project::FeatureStatus;
    let block = field_block("Status", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans: Vec<Span> = Vec::new();
    let mut rects: Vec<(FeatureStatus, Rect)> = Vec::new();
    let mut x = inner.x;
    spans.push(Span::raw(" "));
    x = x.saturating_add(1);
    for (i, st) in FeatureStatus::all().iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
            x = x.saturating_add(1);
        }
        let is_current = *st == form.status;
        let is_cursor = focused && *st == form.status_cursor;
        let label = format!(" {} ", st.label());
        let color = feature_status_color(*st);
        let style = if is_cursor {
            Style::default()
                .bg(color)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let w = label.chars().count() as u16;
        rects.push((*st, Rect::new(x, inner.y, w, 1)));
        spans.push(Span::styled(label, style));
        x = x.saturating_add(w);
        if is_current && !is_cursor {
            spans.push(Span::styled("◆", Style::default().fg(color)));
            x = x.saturating_add(1);
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    rects
}

fn render_form_description(
    form: &mut crate::views::feature_form::FeatureForm,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) {
    if focused && form.description_editing() {
        if let Some(editor) = form.editor.as_mut() {
            editor.focused = true;
            let widget = crate::views::editor::EditorWidget {
                view: editor,
                area_title: "Description (Esc/Backspace to commit)".into(),
                git_status: None,
            };
            frame.render_widget(widget, area);
            return;
        }
    }
    let block = field_block("Description (i/Enter to edit)", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if form.description.trim().is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "(empty — i/Enter to edit)",
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
    } else {
        let lines: Vec<Line> = form
            .description
            .lines()
            .map(|l| Line::from(l.to_string()))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn render_form_steps(
    form: &crate::views::feature_form::FeatureForm,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) -> Vec<(crate::views::feature_form::FormFocus, Rect)> {
    use crate::project::StepStatus;
    use crate::views::feature_form::FormFocus;
    let block = field_block("Steps (Ctrl+T cycle • Ctrl+D delete)", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let active_step = match form.focus {
        FormFocus::Step(i) => Some(i),
        _ => None,
    };
    let on_new = matches!(form.focus, FormFocus::NewStep);
    let mut rects: Vec<(FormFocus, Rect)> = Vec::new();

    let mut row: u16 = 0;
    for (i, step) in form.steps.iter().enumerate() {
        if step.deleted {
            continue;
        }
        if row >= inner.height {
            break;
        }
        let is_active = Some(i) == active_step;
        let prefix = if is_active { "▶ " } else { "  " };
        let glyph_style = match step.status {
            StepStatus::Done => Style::default().fg(Color::Green),
            StepStatus::InProgress => Style::default().fg(Color::Yellow),
            StepStatus::Todo => Style::default(),
        };
        let line_area = Rect {
            x: inner.x,
            y: inner.y + row,
            width: inner.width,
            height: 1,
        };
        let head = format!("{}{} ", prefix, step.status.glyph());
        let head_w = head.chars().count() as u16;
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(head, glyph_style)])),
            line_area,
        );
        let text_area = Rect {
            x: inner.x.saturating_add(head_w),
            y: inner.y + row,
            width: inner.width.saturating_sub(head_w),
            height: 1,
        };
        if is_active {
            render_inline_input(frame, text_area, &step.summary, form.cursor, "(step text)");
        } else {
            frame.render_widget(
                Paragraph::new(Line::from(step.summary.clone())),
                text_area,
            );
        }
        rects.push((FormFocus::Step(i), line_area));
        row += 1;
    }
    if row < inner.height {
        let line_area = Rect {
            x: inner.x,
            y: inner.y + row,
            width: inner.width,
            height: 1,
        };
        let prefix = if on_new { "▶ + " } else { "  + " };
        let head_w = prefix.chars().count() as u16;
        let prefix_style = if on_new {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(prefix, prefix_style)])),
            line_area,
        );
        let text_area = Rect {
            x: inner.x.saturating_add(head_w),
            y: inner.y + row,
            width: inner.width.saturating_sub(head_w),
            height: 1,
        };
        if on_new {
            render_inline_input(
                frame,
                text_area,
                &form.new_step_buf,
                form.cursor,
                "new step…",
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "new step",
                    Style::default().fg(Color::DarkGray),
                )),
                text_area,
            );
        }
        rects.push((FormFocus::NewStep, line_area));
    }
    rects
}

fn render_form_comments(
    form: &mut crate::views::feature_form::FeatureForm,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) -> Vec<(crate::views::feature_form::FormFocus, Rect)> {
    use crate::project::CommentStatus;
    use crate::views::feature_form::{EditorTarget, FormFocus};
    if focused && form.message_editing() {
        let target = form.editor_target();
        if let (Some(editor), Some(target)) = (form.editor.as_mut(), target) {
            editor.focused = true;
            let title = match target {
                EditorTarget::NewComment => "New message (Esc/Backspace to commit)".to_string(),
                _ => "Message (Esc/Backspace to commit)".to_string(),
            };
            let widget = crate::views::editor::EditorWidget {
                view: editor,
                area_title: title,
                git_status: None,
            };
            frame.render_widget(widget, area);
            let rect = match target {
                EditorTarget::Comment(i) => (FormFocus::Comment(i), area),
                _ => (FormFocus::NewComment, area),
            };
            return vec![rect];
        }
    }
    let block = field_block("Messages (i/Enter to edit • Ctrl+T status • Ctrl+K kind • Ctrl+D delete)", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let active_comment = match form.focus {
        FormFocus::Comment(i) => Some(i),
        _ => None,
    };
    let on_new = matches!(form.focus, FormFocus::NewComment);
    let mut rects: Vec<(FormFocus, Rect)> = Vec::new();

    let mut row: u16 = 0;
    for (i, comment) in form.comments.iter().enumerate() {
        if comment.deleted {
            continue;
        }
        if row >= inner.height {
            break;
        }
        let is_active = Some(i) == active_comment;
        let prefix = if is_active { "▶ " } else { "  " };
        let badge_style = match comment.status {
            CommentStatus::Queued => Style::default().fg(Color::Yellow),
            CommentStatus::Sent => Style::default().fg(Color::Cyan),
            CommentStatus::Done => Style::default().fg(Color::Green),
        };
        let head = format!(
            "{}{} [{}] ",
            prefix,
            comment_kind_glyph(comment.kind),
            comment.status.label()
        );
        let head_w = head.chars().count() as u16;
        let line_area = Rect {
            x: inner.x,
            y: inner.y + row,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(head, badge_style)])),
            line_area,
        );
        let text_area = Rect {
            x: inner.x.saturating_add(head_w),
            y: inner.y + row,
            width: inner.width.saturating_sub(head_w),
            height: 1,
        };
        let first_line = comment.message.lines().next().unwrap_or("");
        if is_active {
            render_inline_input(frame, text_area, first_line, form.cursor, "(comment)");
        } else {
            let suffix = if comment.message.lines().count() > 1 {
                "  …"
            } else {
                ""
            };
            frame.render_widget(
                Paragraph::new(Line::from(format!("{}{}", first_line, suffix))),
                text_area,
            );
        }
        rects.push((FormFocus::Comment(i), line_area));
        row += 1;
    }
    if row < inner.height {
        let line_area = Rect {
            x: inner.x,
            y: inner.y + row,
            width: inner.width,
            height: 1,
        };
        let prefix = if on_new { "▶ + " } else { "  + " };
        let head_w = prefix.chars().count() as u16;
        let prefix_style = if on_new {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(prefix, prefix_style)])),
            line_area,
        );
        let text_area = Rect {
            x: inner.x.saturating_add(head_w),
            y: inner.y + row,
            width: inner.width.saturating_sub(head_w),
            height: 1,
        };
        if on_new {
            render_inline_input(
                frame,
                text_area,
                &form.new_comment_buf,
                form.cursor,
                "new comment…",
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "new comment",
                    Style::default().fg(Color::DarkGray),
                )),
                text_area,
            );
        }
        rects.push((FormFocus::NewComment, line_area));
    }
    rects
}

fn render_inline_input(
    frame: &mut Frame<'_>,
    area: Rect,
    text: &str,
    cursor: usize,
    placeholder: &str,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let width = area.width as usize;
    let scroll = if cursor + 1 > width {
        cursor + 1 - width
    } else {
        0
    };
    let buf = frame.buffer_mut();
    let show_placeholder = total == 0;
    for col in 0..width {
        let x = area.x + col as u16;
        let abs = scroll + col;
        let mut style = Style::default();
        let ch = if show_placeholder && col < placeholder.chars().count() {
            style = style.fg(Color::DarkGray);
            placeholder.chars().nth(col).unwrap_or(' ')
        } else if abs < total {
            chars[abs]
        } else {
            ' '
        };
        if abs == cursor {
            style = style.add_modifier(Modifier::REVERSED);
        }
        buf[(x, area.y)].set_char(ch).set_style(style);
    }
}

fn field_block(title: &str, focused: bool) -> Block<'_> {
    let style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(format!(" {} ", title))
}

fn split_off_agent_lane(app: &mut App, frame: &mut Frame<'_>, area: Rect) -> Rect {
    if !app.agent_lane_visible || area.width < 36 {
        app.agent_lane_area = Rect::default();
        app.agent_lane_tile_rects.clear();
        return area;
    }
    let lane_width: u16 = 36u16.min(area.width / 3).max(28);
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(24), Constraint::Length(lane_width)])
        .split(area);
    render_agent_lane(app, frame, split[1]);
    split[0]
}

const LANE_TILE_LINES: u16 = 3;
const LANE_TILE_GAP: u16 = 1;

fn render_agent_lane(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    use crate::views::agents::LaneStatus;
    app.agent_lane_area = area;
    app.agent_lane_tile_rects.clear();

    let total_agents: usize = app
        .project_views
        .values()
        .map(|s| s.agents.len())
        .sum();
    let title = if total_agents == 0 {
        " Lane ".to_string()
    } else {
        format!(" Lane ({}) ", total_agents)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if total_agents == 0 {
        let p = Paragraph::new(Line::from(vec![Span::styled(
            " (no active agents)",
            Style::default().fg(Color::DarkGray),
        )]));
        frame.render_widget(p, inner);
        return;
    }

    let active_pid = app.open_projects.get(app.active_index).map(|p| p.id);
    let active_view_mode = app
        .open_projects
        .get(app.active_index)
        .and_then(|p| app.project_views.get(&p.id))
        .map(|s| s.view_mode);
    let active_agent_idx = app
        .open_projects
        .get(app.active_index)
        .and_then(|p| app.project_views.get(&p.id))
        .and_then(|s| s.active_agent);

    let mut y = inner.y;
    let max_y = inner.y + inner.height;
    let projects_snapshot: Vec<(i64, String)> = app
        .open_projects
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    let inner_w = inner.width as usize;
    let text_w = inner_w.saturating_sub(3);

    for (pid, pname) in projects_snapshot {
        let Some(state) = app.project_views.get(&pid) else { continue };
        if state.agents.is_empty() {
            continue;
        }
        for (idx, agent) in state.agents.iter().enumerate() {
            if y + LANE_TILE_LINES > max_y {
                break;
            }
            let is_active = active_pid == Some(pid)
                && active_view_mode == Some(crate::app::ViewMode::Agents)
                && active_agent_idx == Some(idx);
            let status = agent.lane_status();
            let attention = agent.bells_pending > 0;
            let (state_icon, state_color, state_label) = match (attention, status) {
                (true, _) => ("!", Color::Yellow, "Attention"),
                (false, LaneStatus::Working) => ("●", Color::Green, "Working"),
                (false, LaneStatus::Idle) => ("○", Color::DarkGray, "Idle"),
            };
            let marker = if is_active { "▶ " } else { "  " };
            let row_style = if is_active {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let tile_rect = Rect::new(inner.x, y, inner.width, LANE_TILE_LINES);

            // Line 1: project name (with marker)
            let project_line = Rect::new(inner.x, y, inner.width, 1);
            let proj_text = truncate_with_ellipsis(&pname, text_w);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(marker, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        proj_text,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
                .style(row_style),
                project_line,
            );

            // Line 2: agent title (indented, truncated if needed)
            let title_line = Rect::new(inner.x, y + 1, inner.width, 1);
            let agent_text = truncate_with_ellipsis(&agent.name, text_w);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(agent_text),
                ]))
                .style(row_style),
                title_line,
            );

            // Line 3: state · age
            let status_line = Rect::new(inner.x, y + 2, inner.width, 1);
            let age = agent.activity_age_label();
            let mut spans: Vec<Span> = vec![
                Span::raw("  "),
                Span::styled(
                    format!("{} ", state_icon),
                    Style::default().fg(state_color),
                ),
                Span::styled(
                    state_label.to_string(),
                    Style::default().fg(state_color),
                ),
                Span::styled(
                    format!(" · {}", age),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if attention && agent.bells_pending > 1 {
                spans.push(Span::styled(
                    format!(" ({}×)", agent.bells_pending),
                    Style::default().fg(Color::Yellow),
                ));
            }
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(row_style),
                status_line,
            );

            app.agent_lane_tile_rects.push((pid, idx, tile_rect));
            y += LANE_TILE_LINES + LANE_TILE_GAP;
        }
    }
}

fn truncate_with_ellipsis(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut out: String = chars[..max - 1].iter().collect();
    out.push('…');
    out
}

fn render_terminal_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        return;
    };
    app.terminal_tab_rects.clear();
    app.terminal_new_rect = None;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(area);
    app.terminal_tabs_area = chunks[0];

    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let total = state.terminals.len();
    let active = state.active_terminal.unwrap_or(0);

    let mut spans: Vec<Span> = Vec::new();
    let mut x = chunks[0].x + 1;
    for i in 0..total {
        let text = format!(" {} ", i + 1);
        let w = text.chars().count() as u16;
        let style = if i == active {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        app.terminal_tab_rects
            .push(Rect::new(x, chunks[0].y, w, 1));
        spans.push(Span::styled(text, style));
        x += w;
        spans.push(Span::raw(" "));
        x += 1;
    }
    let plus = " + ".to_string();
    let plus_w = plus.chars().count() as u16;
    let plus_rect = Rect::new(x, chunks[0].y, plus_w, 1);
    app.terminal_new_rect = Some(plus_rect);
    spans.push(Span::styled(
        plus,
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

    let body_area = chunks[1];
    let header_text = if total == 0 {
        format!(" Terminal — {} (no shell) ", project.name)
    } else {
        format!(" Terminal — {} ", project.name)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(header_text);
    let inner = block.inner(body_area);
    frame.render_widget(block, body_area);

    if total == 0 || active >= total {
        let para = Paragraph::new("No terminal — Ctrl+Space then n to create one")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, inner);
        return;
    }
    if let Some(term) = state.terminals.get_mut(active) {
        let widget = crate::views::terminal::TerminalWidget {
            view: term,
            focused: true,
        };
        frame.render_widget(widget, inner);
    }
}

fn render_agents_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        return;
    };
    app.agent_tab_rects.clear();
    app.agent_new_rect = None;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(area);
    app.agent_tabs_area = chunks[0];

    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let total = state.agents.len();
    let active = state.active_agent.unwrap_or(0);

    let mut spans: Vec<Span> = Vec::new();
    let mut x = chunks[0].x + 1;
    for (i, agent) in state.agents.iter().enumerate() {
        let text = format!(" {} ", agent.name);
        let w = text.chars().count() as u16;
        let style = if i == active {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        app.agent_tab_rects
            .push(Rect::new(x, chunks[0].y, w, 1));
        spans.push(Span::styled(text, style));
        x += w;
        spans.push(Span::raw(" "));
        x += 1;
    }
    let plus = " + ".to_string();
    let plus_w = plus.chars().count() as u16;
    let plus_rect = Rect::new(x, chunks[0].y, plus_w, 1);
    app.agent_new_rect = Some(plus_rect);
    spans.push(Span::styled(
        plus,
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

    let body_area = chunks[1];
    let header_text = if total == 0 {
        format!(" Agents — {} (no agent) ", project.name)
    } else {
        format!(" Agents — {} ", project.name)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(header_text);
    let inner = block.inner(body_area);
    frame.render_widget(block, body_area);

    if total == 0 || active >= total {
        let para = Paragraph::new("No agent — Ctrl+Space then n to start one")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, inner);
        return;
    }
    if let Some(agent) = state.agents.get_mut(active) {
        let widget = crate::views::agents::AgentWidget {
            session: agent,
            focused: true,
        };
        frame.render_widget(widget, inner);
    }
}

fn render_tabs(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    app.tabs_area = area;
    app.tab_rects.clear();
    let block = Block::default().borders(Borders::ALL).title(" CoffeeTable ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.open_projects.is_empty() {
        let para = Paragraph::new(Line::from(Span::styled(
            " (no open projects) ",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(para, inner);
        return;
    }

    let mut spans: Vec<Span> = Vec::new();
    let mut x = inner.x;
    let max_x = inner.x + inner.width;
    for (i, p) in app.open_projects.iter().enumerate() {
        let label = format!(" {} ", p.name);
        let label_w = label.chars().count() as u16;
        if x + label_w > max_x {
            break;
        }
        let style = if i == app.active_index {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        app.tab_rects
            .push(Rect::new(x, inner.y, label_w, inner.height));
        spans.push(Span::styled(label, style));
        x += label_w;
        if i + 1 < app.open_projects.len() && x + 3 <= max_x {
            spans.push(Span::styled(
                " │ ",
                Style::default().fg(Color::DarkGray),
            ));
            x += 3;
        }
    }
    let para = Paragraph::new(Line::from(spans));
    frame.render_widget(para, inner);
}

fn render_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        let empty = Paragraph::new("No project open — press Space then p to open the project picker.")
            .block(Block::default().borders(Borders::ALL).title(" CoffeeTable "));
        frame.render_widget(empty, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Min(20)])
        .split(area);
    app.left_pane_area = chunks[0];
    app.right_pane_area = chunks[1];

    let normal_mode = matches!(app.mode, AppMode::Normal);
    let filter_mode = matches!(app.mode, AppMode::ExplorerFilter);
    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let tree_focused = (normal_mode || filter_mode) && state.focus == Focus::Tree;
    let editor_focused = normal_mode && state.focus == Focus::Editor;

    let title = format!(" {} ", project.path.display());
    let selected = state.selected_path().map(|p| p.to_path_buf());
    match state.left_pane {
        LeftPaneMode::Tree => {
            let show_filter = filter_mode || !state.tree.filter.is_empty();
            let tree_area = if show_filter {
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(1)])
                    .split(chunks[0]);
                render_filter_input(split[0], &state.tree.filter, filter_mode, frame);
                split[1]
            } else {
                chunks[0]
            };
            let tree_widget = FileTreeWidget {
                view: &mut state.tree,
                title,
                focused: tree_focused,
            };
            frame.render_widget(tree_widget, tree_area);
        }
        LeftPaneMode::Changes => {
            let project_label = project
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.path.display().to_string());
            let widget = ChangesWidget {
                view: &mut state.changes,
                title: project_label,
                focused: tree_focused,
            };
            frame.render_widget(widget, chunks[0]);
        }
    }

    let selected_is_dir = selected.as_ref().map(|p| p.is_dir()).unwrap_or(false);
    if editor_focused && state.editor.is_some() {
        render_editor_pane(state, chunks[1], frame, true);
    } else if selected_is_dir {
        let p = selected.expect("dir selected");
        render_dir_preview(&p, chunks[1], frame);
    } else if state.editor.is_some() {
        render_editor_pane(state, chunks[1], frame, editor_focused);
    } else {
        render_empty_editor_panel(&project, frame, chunks[1]);
    }
}

fn render_editor_pane(
    state: &mut crate::app::ProjectViewState,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) {
    let editor_path = state
        .editor
        .as_ref()
        .expect("editor present")
        .path
        .clone();
    let git_status = state.tree.git_status_for(&editor_path);
    let editor = state.editor.as_mut().expect("editor present");
    editor.focused = focused;
    let title = editor
        .path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| editor.path.display().to_string());
    let widget = EditorWidget {
        view: editor,
        area_title: title,
        git_status,
    };
    frame.render_widget(widget, area);
}

const DIR_PREVIEW_MAX_DEPTH: u16 = 2;
const DIR_PREVIEW_MAX_LINES: usize = 500;

fn render_dir_preview(path: &std::path::Path, area: Rect, frame: &mut Frame<'_>) {
    let title = format!(" Directory: {} ", path.display());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    push_dir_entries(path, 0, DIR_PREVIEW_MAX_DEPTH, &mut lines);
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if lines.len() >= DIR_PREVIEW_MAX_LINES {
        lines.push(Line::from(Span::styled(
            "  …",
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn push_dir_entries(
    path: &std::path::Path,
    depth: u16,
    max_depth: u16,
    out: &mut Vec<Line<'static>>,
) {
    if out.len() >= DIR_PREVIEW_MAX_LINES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else { return };
    let mut entries: Vec<(bool, String, std::path::PathBuf)> = rd
        .flatten()
        .filter_map(|d| {
            let name = d.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".github" {
                return None;
            }
            let ft = d.file_type().ok()?;
            Some((ft.is_dir(), name, d.path()))
        })
        .collect();
    entries.sort_by(|a, b| match (a.0, b.0) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.1.to_lowercase().cmp(&b.1.to_lowercase()),
    });
    for (is_dir, name, child_path) in entries {
        if out.len() >= DIR_PREVIEW_MAX_LINES {
            return;
        }
        let indent = "  ".repeat(depth as usize + 1);
        let (icon, icon_color) = if is_dir {
            (crate::icons::folder(false), crate::icons::folder_color())
        } else {
            (
                crate::icons::for_file(&name),
                crate::icons::color_for_file(&name),
            )
        };
        let display = if is_dir {
            format!("{}/", name)
        } else {
            name.clone()
        };
        out.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(
                format!("{}  ", icon),
                Style::default().fg(icon_color),
            ),
            Span::raw(display),
        ]));
        if is_dir && depth + 1 < max_depth {
            push_dir_entries(&child_path, depth + 1, max_depth, out);
        }
    }
}

fn render_empty_editor_panel(
    project: &crate::project::Project,
    frame: &mut Frame<'_>,
    area: Rect,
) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Project: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                project.name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Path:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(project.path.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("GitHub:  ", Style::default().fg(Color::DarkGray)),
            Span::raw(project.github_url.clone().unwrap_or_else(|| "—".into())),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Open a file from the tree (Enter) or press Space then f to find one.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("?", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  show all shortcuts", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Info "),
    );
    frame.render_widget(p, area);
}

fn render_command_or_status(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let editor_cmd = app
        .open_projects
        .get(app.active_index)
        .and_then(|p| app.project_views.get(&p.id))
        .and_then(|s| s.editor.as_ref())
        .filter(|e| matches!(e.mode, EditorMode::Search));

    if let Some(editor) = editor_cmd {
        let buf = frame.buffer_mut();
        render_command_line(area, buf, editor);
        return;
    }

    let text = if !app.status.is_empty() {
        app.status.clone()
    } else if let Some(editor) = current_editor(app) {
        let pos = format!("Ln {}, Col {}", editor.cursor.0 + 1, editor.cursor.1 + 1);
        let mode = editor.mode_label();
        format!(" {}   {}   {}", mode, editor.path.display(), pos)
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_footer(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let text = match app.mode {
        AppMode::Normal => match current_view_mode(app) {
            Some(crate::app::ViewMode::Project) => {
                "j/k Tab move • i/Enter edit field • x cycle status • d cycle step • D delete • Ctrl+S save • Esc back • ? help"
                    .to_string()
            }
            Some(crate::app::ViewMode::Git) => {
                "j/k move • Tab pane • Enter open • c checkout • p push • P pull • m merge • R PR • V PRs • r refresh • ? help".to_string()
            }
            Some(crate::app::ViewMode::Terminal) => {
                "Ctrl+Space leader • Ctrl+L/H switch • Ctrl+Shift+C SIGINT • Ctrl+J/K view • ? help".to_string()
            }
            _ => match current_focus(app) {
                Some(Focus::Editor) => {
                    ":palette  •  i insert  •  v visual  •  /search  •  Backspace tree  •  Space menu  •  ? help"
                        .to_string()
                }
                _ => {
                    ":palette  •  Space menu  •  ? help  •  Tab switch  •  Space q quit".to_string()
                }
            },
        },
        AppMode::Picker => "? help  •  Esc close".into(),
        AppMode::Grep => "Type to filter  •  Enter open  •  Esc cancel".into(),
        AppMode::OpenConfirm => "y / Enter open  •  n / Esc cancel".into(),
        AppMode::ConfirmDeleteFeature => "y / Enter delete  •  n / Esc cancel".into(),
        AppMode::Palette => "↑/↓ select  •  Enter run  •  ! suffix forces  •  Esc cancel".into(),
        AppMode::ExplorerFilter => "Type to narrow  •  ↑/↓ select  •  Enter open  •  Esc clear+exit".into(),
        AppMode::AiCommit => {
            match app.ai_commit.as_ref().map(|o| &o.state) {
                Some(crate::app::AiCommitState::ReviewingPlan { .. }) => {
                    "Ctrl+N/P cycle  •  Ctrl+S commit all  •  Ctrl+R regen  •  Esc cancel".into()
                }
                _ => "i edit  •  Ctrl+S commit  •  Ctrl+R regen  •  Esc cancel".into(),
            }
        }
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn current_focus(app: &App) -> Option<Focus> {
    let id = app.open_projects.get(app.active_index)?.id;
    app.project_views.get(&id).map(|s| s.focus)
}

fn current_view_mode(app: &App) -> Option<crate::app::ViewMode> {
    let id = app.open_projects.get(app.active_index)?.id;
    app.project_views.get(&id).map(|s| s.view_mode)
}

fn current_editor<'a>(app: &'a App) -> Option<&'a crate::views::editor::EditorView> {
    let id = app.open_projects.get(app.active_index)?.id;
    app.project_views.get(&id)?.editor.as_ref()
}

fn render_picker_overlay(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(80, 70, area);
    frame.render_widget(Clear, popup);
    if let Some(picker) = app.picker.as_mut() {
        frame.render_widget(ProjectPickerWidget { picker }, popup);
    }
}

fn render_filter_input(area: Rect, query: &str, focused: bool, frame: &mut Frame<'_>) {
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title = if focused {
        " filter (type to narrow • Enter open • Esc clear) "
    } else {
        " filter "
    };
    let para = Paragraph::new(format!("/{}", query))
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        );
    frame.render_widget(para, area);
}

fn render_ai_commit_overlay(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let Some(overlay) = app.ai_commit.as_mut() else { return };
    let popup = centered_rect(70, 60, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" AI commit ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let provider_name = app.ai_config.provider.clone();
    match &mut overlay.state {
        AiCommitState::Loading { spinner, .. } => {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let sp = frames[*spinner % frames.len()];
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(sp.to_string(), Style::default().fg(Color::Yellow)),
                    Span::raw("  Generating commit message via "),
                    Span::styled(
                        provider_name,
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("..."),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        AiCommitState::Reviewing { editor } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(inner);
            editor.focused = true;
            let widget = crate::views::editor::EditorWidget {
                view: editor,
                area_title: "commit message".to_string(),
                git_status: None,
            };
            frame.render_widget(widget, chunks[0]);
            frame.render_widget(
                Paragraph::new(
                    "  i edit  •  Ctrl+S commit  •  Ctrl+R regenerate  •  Esc cancel",
                )
                .style(Style::default().fg(Color::DarkGray)),
                chunks[1],
            );
        }
        AiCommitState::ReviewingPlan {
            messages,
            files,
            current,
        } => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(3),
                    Constraint::Length(5),
                    Constraint::Length(1),
                ])
                .split(inner);
            let header = format!(
                "  Commit {} of {}",
                *current + 1,
                messages.len()
            );
            frame.render_widget(
                Paragraph::new(header).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                chunks[0],
            );
            if let Some(editor) = messages.get_mut(*current) {
                editor.focused = true;
                let widget = crate::views::editor::EditorWidget {
                    view: editor,
                    area_title: format!("message {}/{}", *current + 1, files.len()),
                    git_status: None,
                };
                frame.render_widget(widget, chunks[1]);
            }
            let file_lines = if let Some(list) = files.get(*current) {
                let mut lines: Vec<Line> = vec![Line::from(Span::styled(
                    "  Files:",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ))];
                for f in list.iter().take(3) {
                    lines.push(Line::from(format!("    • {}", f)));
                }
                if list.len() > 3 {
                    lines.push(Line::from(format!(
                        "    … +{} more",
                        list.len() - 3
                    )));
                }
                lines
            } else {
                Vec::new()
            };
            frame.render_widget(Paragraph::new(file_lines), chunks[2]);
            frame.render_widget(
                Paragraph::new(
                    "  Ctrl+N/P next/prev  •  Ctrl+S commit all  •  Ctrl+R regen  •  Esc cancel",
                )
                .style(Style::default().fg(Color::DarkGray)),
                chunks[3],
            );
        }
        AiCommitState::Error(e) => {
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        "Error: ",
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(e.clone()),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  y copy  •  r retry  •  Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
}

fn render_delete_feature_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some((_, title)) = app.pending_delete_feature.as_ref() else { return };
    let popup = centered_rect_fixed(70, 9, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Delete feature? ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Are you sure you want to delete "),
            Span::styled(
                format!("\"{}\"", title),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
        Line::from(Span::styled(
            "  This will also delete its steps and comments.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "y / Enter",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  delete     "),
            Span::styled(
                "n / Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  cancel"),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_open_confirm_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(pending) = app.pending_open.as_ref() else { return };
    let popup = centered_rect_fixed(70, 9, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Large file ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                pending.path.display().to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{} lines", pending.line_count),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " — over the {} line threshold; the editor may be slow.",
                    crate::config::LARGE_FILE_LINE_THRESHOLD
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "y / Enter",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  open anyway     "),
            Span::styled(
                "n / Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  cancel"),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_grep_overlay(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(85, 75, area);
    frame.render_widget(Clear, popup);
    if let Some(view) = app.grep.as_mut() {
        frame.render_widget(GrepWidget { view }, popup);
    }
}

fn render_help_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let (context, entries) = help_context(app);
    let popup = centered_rect(60, 75, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(format!(" Help — {} ", context));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::with_capacity(entries.len() + 2);
    lines.push(Line::from(""));
    for (key, desc) in entries {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<18}", key),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(desc.to_string()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn help_context(app: &App) -> (&'static str, Vec<(&'static str, &'static str)>) {
    match app.mode {
        AppMode::Picker => {
            let m = app.picker.as_ref().map(|p| p.mode);
            match m {
                Some(PickerMode::Browse) => (
                    "Project picker",
                    vec![
                        ("j/k  ↓/↑", "navigate"),
                        ("Enter", "open saved / add+open discovered"),
                        ("n", "add a new project by path"),
                        ("r", "manage root directories"),
                        ("s", "rescan roots for new repos"),
                        ("d", "delete selected (saved only)"),
                        ("Esc", "close picker"),
                        ("?", "toggle this help"),
                        ("Ctrl+C", "copy selection / current line / selected path"),
                    ],
                ),
                Some(PickerMode::Roots) => (
                    "Root directories",
                    vec![
                        ("j/k  ↓/↑", "navigate"),
                        ("n", "add a new root"),
                        ("d", "delete selected root"),
                        ("Esc", "back to projects (rescans)"),
                        ("?", "toggle this help"),
                        ("Ctrl+C", "copy selection / current line / selected path"),
                    ],
                ),
                _ => (
                    "Path input",
                    vec![
                        ("(text)", "type the path"),
                        ("Backspace", "delete a character"),
                        ("Enter", "confirm"),
                        ("Esc", "cancel"),
                    ],
                ),
            }
        }
        AppMode::ExplorerFilter => (
            "Explorer filter",
            vec![
                ("(text)", "narrow visible tree"),
                ("↑/↓", "navigate matches"),
                ("Enter", "open selected file"),
                ("Backspace", "delete a character (empty → exit)"),
                ("Esc", "clear filter and exit"),
            ],
        ),
        AppMode::Grep => (
            "Grep",
            vec![
                ("(text)", "regex (case-insensitive)"),
                ("↑/↓", "navigate results"),
                ("Enter", "open at hit"),
                ("Esc", "cancel"),
            ],
        ),
        AppMode::OpenConfirm => (
            "Large file",
            vec![
                ("y / Enter", "open anyway"),
                ("n / Esc", "cancel"),
            ],
        ),
        AppMode::ConfirmDeleteFeature => (
            "Delete feature",
            vec![
                ("y / Enter", "delete"),
                ("n / Esc", "cancel"),
            ],
        ),
        AppMode::AiCommit => (
            "AI commit",
            vec![
                ("i / a / o", "edit the current message in vim-style insert mode"),
                ("hjkl / w / b / gg / G", "navigate in normal mode"),
                ("yy / dd / p / v", "yank / delete / paste / visual"),
                ("Ctrl+S", "execute the commit(s) using current buffer text"),
                ("Ctrl+N / Ctrl+P (plan)", "next / previous commit in the plan"),
                ("Ctrl+R", "regenerate (discards your edits)"),
                ("Esc (normal mode)", "cancel overlay"),
                ("y (error state)", "copy error text to clipboard"),
            ],
        ),
        AppMode::Palette => (
            "Command palette",
            vec![
                ("(text)", "filter commands"),
                ("↑/↓  Tab/BackTab", "navigate"),
                ("Enter", "run highlighted command (or literal if none)"),
                ("! suffix", "force (q!, e!, Q!)"),
                ("Esc", "cancel"),
            ],
        ),
        AppMode::Normal => {
            let in_terminal = app
                .open_projects
                .get(app.active_index)
                .and_then(|p| app.project_views.get(&p.id))
                .map(|s| matches!(s.view_mode, crate::app::ViewMode::Terminal))
                .unwrap_or(false);
            let in_project = app
                .open_projects
                .get(app.active_index)
                .and_then(|p| app.project_views.get(&p.id))
                .map(|s| matches!(s.view_mode, crate::app::ViewMode::Project))
                .unwrap_or(false);
            let in_git = app
                .open_projects
                .get(app.active_index)
                .and_then(|p| app.project_views.get(&p.id))
                .map(|s| matches!(s.view_mode, crate::app::ViewMode::Git))
                .unwrap_or(false);
            if in_git {
                return (
                    "Git",
                    vec![
                        ("j/k  ↓/↑", "navigate within focused pane"),
                        ("Tab / Shift+Tab", "cycle panes (branches → commits → details)"),
                        ("Enter / l", "drill into selection (branch, commit, PR)"),
                        ("Esc / Backspace", "in PR view: back to PR list / back to commit"),
                        ("g / G", "jump to top / bottom of focused pane"),
                        ("c", "checkout selected branch"),
                        ("p", "push current branch (auto sets upstream)"),
                        ("P", "pull (ff-only) into current branch"),
                        ("m", "merge selected branch into current"),
                        ("R", "create a PR for the current branch (gh)"),
                        ("V", "list PRs for selected branch (gh)"),
                        ("r", "refresh branches + commits"),
                        ("Ctrl+J / Ctrl+K", "switch view"),
                    ],
                );
            }
            if in_project {
                return (
                    "Project",
                    vec![
                        ("j/k  ↓/↑", "navigate sections + features (list)"),
                        ("g / G", "jump to top / bottom"),
                        ("i / Enter / l", "open form for selected (Meta editor or Feature form)"),
                        ("n", "add new feature (opens empty form)"),
                        ("x", "cycle selected feature status (in list)"),
                        ("D", "delete selected feature (in list)"),
                        ("— in Feature form —", ""),
                        ("Tab / Shift+Tab", "next / previous field"),
                        ("j/k", "also navigate fields"),
                        ("i / Enter / l", "edit focused field (or cycle Status)"),
                        ("Enter (single-line field)", "commit & next"),
                        ("Esc (in field normal)", "commit field"),
                        ("x", "cycle feature status (Status field)"),
                        ("d", "cycle step status (Step field)"),
                        ("D", "delete focused step / comment"),
                        ("Ctrl+S", "save form to db"),
                        ("Esc / Backspace (nav)", "save & close form"),
                        ("Ctrl+J / Ctrl+K", "switch view (Editor/Terminal/Project)"),
                    ],
                );
            }
            if in_terminal {
                return (
                    "Terminal",
                    vec![
                        ("(any key)", "forward to PTY"),
                        ("Ctrl+J / Ctrl+K", "switch between Editor and Terminal views"),
                        ("Ctrl+L / Ctrl+H", "next / previous terminal in this project"),
                        ("Ctrl+Shift+C", "send SIGINT (interrupt) to the running process"),
                        ("Ctrl+Space", "prefix (next key triggers a terminal action)"),
                        ("Ctrl+Space d", "detach (back to Editor view)"),
                        ("Ctrl+Space n", "new terminal in this project"),
                        ("Ctrl+Space l / h", "next / previous terminal (same as Ctrl+L/H)"),
                        ("Ctrl+Space x", "close current terminal"),
                        ("Ctrl+Space Space", "send a literal Ctrl+Space to the shell"),
                        ("click tabs", "switch terminal by clicking on the numbered tabs"),
                    ],
                );
            }
            let focus = current_focus(app).unwrap_or(Focus::Tree);
            match focus {
                Focus::Tree => (
                    "Explorer",
                    vec![
                        ("j/k  ↓/↑", "navigate (auto-previews text files)"),
                        ("l  Enter", "expand directory / open + focus editor"),
                        ("h  ←", "collapse directory / go to parent (tree only)"),
                        ("e", "open selected file and focus editor"),
                        ("g / G", "jump to top / bottom"),
                        ("Tab / Shift+Tab", "next / previous project"),
                        (":", "open command palette (works globally)"),
                        ("Space c", "toggle Changes view ↔ Tree view"),
                        ("Space p / f / g", "projects / find file / grep"),
                        ("Space e / b / w", "focus tree / editor / toggle"),
                        ("Space q", "quit"),
                        ("Ctrl+C", "copy selected path to clipboard"),
                        ("?", "toggle this help"),
                        ("colors", "red untracked • yellow modified • green staged"),
                    ],
                ),
                Focus::Editor => (
                    "Editor (vim-style)",
                    vec![
                        ("h j k l / arrows", "navigate"),
                        ("w / b", "next / previous word"),
                        ("0 / ^ / $", "line start / first non-ws / end"),
                        ("gg / G", "first / last line"),
                        ("i / I / a / A", "insert at / start / after / end"),
                        ("o / O", "open line below / above"),
                        ("x / X", "delete char at / before cursor"),
                        ("dd / D", "delete line / to end of line"),
                        ("yy / p / P", "yank line / paste after / before"),
                        ("v / V", "visual char / line selection"),
                        ("y (visual) / d", "yank / delete selection"),
                        ("u / Ctrl+R", "undo / redo"),
                        ("/pattern  n / N", "search forward, next / prev"),
                        (":", "open command palette (dropdown)"),
                        (":S  /  :settings", "open settings.yaml (autoreloads on save)"),
                        ("Space h  /  :H", "show HEAD version of file (read-only)"),
                        ("Space d  /  :D", "show unified diff against HEAD (read-only)"),
                        ("Space w  /  :W", "back to working copy (editable)"),
                        ("click pill", "switch view by clicking on the title pills"),
                        ("Backspace", "focus the explorer (file stays open)"),
                        ("Esc (from Insert)", "back to normal — autosaves file"),
                        ("Space (normal/visual)", "leader menu"),
                        ("Space p / f / g", "projects / find file / grep"),
                        ("Space e / b / w", "focus tree / editor / toggle"),
                        ("Space q", "quit app"),
                        ("Ctrl+C", "copy selection or current line"),
                        ("?", "toggle this help"),
                    ],
                ),
            }
        }
    }
}

fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}
