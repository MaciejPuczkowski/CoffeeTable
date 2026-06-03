use crate::{
    app::{AiCommitState, App, AppMode, Focus, FocusContext, LeftPaneMode, SettingsPane},
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
        crate::app::ViewMode::Github => render_github_body(app, frame, body_area),
        crate::app::ViewMode::Runtime => render_runtime_body(app, frame, body_area),
        crate::app::ViewMode::Editor => render_body(app, frame, body_area),
    }
    render_command_or_status(app, frame, chunks[3]);
    render_footer(app, frame, chunks[4]);
    render_status_toolbar(app, frame, chunks[5]);

    match app.mode {
        AppMode::Picker => render_picker_overlay(app, frame, area),
        AppMode::Grep => render_grep_overlay(app, frame, area),
        AppMode::OpenConfirm => render_open_confirm_overlay(app, frame, area),
        AppMode::ConfirmDeleteFeature => render_delete_feature_overlay(app, frame, area),
        AppMode::Palette => render_command_palette_overlay(app, frame, area),
        AppMode::AiCommit => render_ai_commit_overlay(app, frame, area),
        AppMode::Settings => render_settings_overlay(app, frame, area),
        AppMode::LogView => render_log_overlay(app, frame, area),
        AppMode::AgentRename => render_agent_rename_overlay(app, frame, area),
        AppMode::WorktreePrompt => render_worktree_prompt_overlay(app, frame, area),
        AppMode::FilePrompt => render_file_prompt_overlay(app, frame, area),
        AppMode::ConfirmDeleteFile => render_delete_file_overlay(app, frame, area),
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
        ("r", "Rename current agent (Agents only)"),
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
        ("r", "Runtime view (services)"),
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
        ("r", "Runtime view (services)"),
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
    let focused = matches!(app.focus_context, FocusContext::ViewTabs);
    let github_available = app.github_available_for_active();
    let project_enabled = app
        .open_projects
        .get(app.active_index)
        .map(|p| app.is_project_view_enabled(p.id))
        .unwrap_or(true);
    let mut spans: Vec<Span> = Vec::new();
    let marker = if focused { "▶ " } else { "  " };
    spans.push(Span::styled(
        marker,
        Style::default().fg(if focused { Color::Yellow } else { Color::DarkGray }),
    ));
    let mut x = area.x + marker.chars().count() as u16;
    let mut tabs: Vec<(crate::app::ViewMode, &str)> = vec![
        (crate::app::ViewMode::Editor, "Editor"),
        (crate::app::ViewMode::Terminal, "Terminal"),
        (crate::app::ViewMode::Agents, "Agents"),
    ];
    if project_enabled {
        tabs.push((crate::app::ViewMode::Project, "Project"));
    }
    tabs.push((crate::app::ViewMode::Runtime, "Runtime"));
    tabs.push((crate::app::ViewMode::Git, "Git"));
    if github_available {
        tabs.push((crate::app::ViewMode::Github, "GitHub"));
    }
    for (mode, label) in &tabs {
        let text = format!(" {} ", label);
        let w = text.chars().count() as u16;
        let style = if active_view == *mode {
            let bg = if focused { Color::Yellow } else { Color::Yellow };
            let fg = if focused { Color::Black } else { Color::Black };
            Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD)
        } else if focused {
            Style::default().fg(Color::Yellow)
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
        .constraints([Constraint::Percentage(app.split_pct), Constraint::Min(20)])
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
        app.feature_form_field_rects = rects.fields;
        app.feature_form_status_rects = rects.statuses;
        return;
    }
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
                let wrap_width = (inner.width as usize).min(120).max(10);
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
                    for w in wrap_paragraph(&feature.description, wrap_width.saturating_sub(2)) {
                        lines.push(Line::from(format!("  {}", w)));
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
                        let head = format!("    [{}] ", c.status.label());
                        let head_len = head.chars().count();
                        let avail = wrap_width.saturating_sub(head_len).max(8);
                        let wrapped = wrap_paragraph(&c.message, avail);
                        if wrapped.is_empty() {
                            lines.push(Line::from(vec![Span::styled(head, badge_style)]));
                        } else {
                            for (j, w) in wrapped.into_iter().enumerate() {
                                if j == 0 {
                                    lines.push(Line::from(vec![
                                        Span::styled(head.clone(), badge_style),
                                        Span::raw(w),
                                    ]));
                                } else {
                                    let pad: String =
                                        std::iter::repeat(' ').take(head_len).collect();
                                    lines
                                        .push(Line::from(vec![Span::raw(pad), Span::raw(w)]));
                                }
                            }
                        }
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
            "  Ctrl+E/Y scroll preview • ? for full help",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    lines.extend(hints);
    let total = lines.len() as u16;
    let max_scroll = total.saturating_sub(inner.height);
    let scroll = model.preview_scroll.min(max_scroll);
    model.preview_scroll = scroll;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
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
        .constraints([Constraint::Percentage(app.split_pct), Constraint::Min(20)])
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
        DetailsMode::Worktrees => "[Worktrees] ",
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

fn render_github_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    use crate::views::github::GhPane;
    use ratatui::widgets::{List, ListItem, ListState, StatefulWidget};

    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        return;
    };
    let needs_load = app
        .project_views
        .get(&project.id)
        .map(|s| s.github_view.is_none())
        .unwrap_or(false);
    if needs_load {
        app.ensure_github_view_loaded();
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(app.split_pct), Constraint::Min(20)])
        .split(area);
    app.left_pane_area = chunks[0];
    app.right_pane_area = chunks[1];

    let left_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Min(3)])
        .split(chunks[0]);

    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let Some(view) = state.github_view.as_mut() else {
        let p = Paragraph::new("GitHub view unavailable.")
            .block(Block::default().borders(Borders::ALL).title(" GitHub "));
        frame.render_widget(p, area);
        return;
    };
    view.runs_area = left_split[0];
    view.prs_area = left_split[1];
    view.details_area = chunks[1];

    let runs_focused = matches!(view.focus, GhPane::Runs);
    let prs_focused = matches!(view.focus, GhPane::Prs);
    let details_focused = matches!(view.focus, GhPane::Details);

    let runs_title = if view.runs.is_empty() {
        " Workflow runs ".to_string()
    } else {
        format!(" Workflow runs ({}) ", view.runs.len())
    };
    let mut run_items: Vec<ListItem> = Vec::with_capacity(view.runs.len());
    for r in &view.runs {
        let (glyph, color) = run_status_style(&r.status, &r.conclusion);
        let workflow = truncate(&r.workflow_name, 22);
        let branch = truncate(&r.head_branch, 18);
        let when = format_when(&r.created_at);
        run_items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("{} ", glyph), Style::default().fg(color)),
            Span::styled(
                format!("{:<22} ", workflow),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{:<18} ", branch), Style::default().fg(Color::Cyan)),
            Span::styled(when, Style::default().fg(Color::DarkGray)),
        ])));
    }
    if run_items.is_empty() {
        let msg = if view.repo_configured {
            "  (no workflow runs — press 'r' to refresh)"
        } else {
            "  (not a GitHub repository)"
        };
        run_items.push(ListItem::new(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray),
        ))));
    }
    let runs_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border(runs_focused))
        .title(runs_title);
    let mut run_state = ListState::default();
    run_state.select(Some(view.run_idx));
    let runs_list = List::new(run_items)
        .block(runs_block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(runs_list, left_split[0], frame.buffer_mut(), &mut run_state);

    let prs_title = if view.prs.is_empty() {
        " Pull requests ".to_string()
    } else {
        format!(" Pull requests ({}) ", view.prs.len())
    };
    let mut pr_items: Vec<ListItem> = Vec::with_capacity(view.prs.len());
    for p in &view.prs {
        let (glyph, color) = pr_state_style(&p.state, p.draft);
        let title = truncate(&p.title, 60);
        pr_items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("{} ", glyph), Style::default().fg(color)),
            Span::styled(
                format!("#{:<5} ", p.number),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(title),
            Span::styled(
                format!("  @{}", p.author),
                Style::default().fg(Color::DarkGray),
            ),
        ])));
    }
    if pr_items.is_empty() {
        pr_items.push(ListItem::new(Line::from(Span::styled(
            "  (no pull requests)",
            Style::default().fg(Color::DarkGray),
        ))));
    }
    let prs_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border(prs_focused))
        .title(prs_title);
    let mut pr_state = ListState::default();
    pr_state.select(Some(view.pr_idx));
    let prs_list = List::new(pr_items)
        .block(prs_block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    StatefulWidget::render(prs_list, left_split[1], frame.buffer_mut(), &mut pr_state);

    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border(details_focused))
        .title(format!(" {} ", view.details_title));
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

fn run_status_style(status: &str, conclusion: &str) -> (&'static str, Color) {
    match status {
        "queued" | "waiting" | "requested" | "pending" => ("◌", Color::Cyan),
        "in_progress" => ("◐", Color::Yellow),
        "completed" => match conclusion {
            "success" => ("✓", Color::Green),
            "failure" | "timed_out" | "startup_failure" => ("✗", Color::Red),
            "cancelled" => ("⊘", Color::DarkGray),
            "skipped" | "neutral" | "stale" => ("·", Color::DarkGray),
            _ => ("?", Color::Magenta),
        },
        _ => ("?", Color::Magenta),
    }
}

fn pr_state_style(state: &str, draft: bool) -> (&'static str, Color) {
    if draft {
        return ("◇", Color::DarkGray);
    }
    match state {
        "OPEN" | "open" => ("●", Color::Green),
        "MERGED" | "merged" => ("●", Color::Magenta),
        "CLOSED" | "closed" => ("●", Color::Red),
        _ => ("●", Color::DarkGray),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let cut = max.saturating_sub(1);
    let mut out: String = chars.into_iter().take(cut).collect();
    out.push('…');
    out
}

fn format_when(iso: &str) -> String {
    if iso.len() >= 16 {
        iso[..16].replace('T', " ")
    } else {
        iso.to_string()
    }
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
    pub fields: Vec<(crate::views::feature_form::FormFocus, Rect)>,
    pub statuses: Vec<(crate::project::FeatureStatus, Rect)>,
}

fn render_feature_form(
    model: &mut crate::views::project_view::ProjectViewModel,
    area: Rect,
    frame: &mut Frame<'_>,
    focused: bool,
) -> FeatureFormRects {
    use crate::views::feature_form::{EditorTarget, FormFocus};
    let mut rects = FeatureFormRects {
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

    if inner.width < 10 || inner.height < 1 {
        return rects;
    }

    let layout = build_feature_layout(form, focused, inner.width);

    let total_height = layout.lines.len() as u16;
    let max_scroll = total_height.saturating_sub(inner.height);
    let scroll = form.scroll_offset.min(max_scroll);
    form.scroll_offset = scroll;

    let paragraph = Paragraph::new(layout.lines).scroll((scroll, 0));
    frame.render_widget(paragraph, inner);

    for (focus, vy, h) in layout.regions {
        if vy + h <= scroll || vy >= scroll + inner.height {
            continue;
        }
        let visible_top = vy.saturating_sub(scroll);
        let visible_bottom = (vy + h).saturating_sub(scroll).min(inner.height);
        let screen_y = inner.y + visible_top;
        let screen_h = visible_bottom - visible_top;
        if screen_h == 0 {
            continue;
        }
        rects.fields.push((
            focus,
            Rect::new(inner.x, screen_y, inner.width, screen_h),
        ));
    }

    for (st, vy, x_off, w) in layout.statuses {
        if vy < scroll || vy >= scroll + inner.height {
            continue;
        }
        let screen_y = inner.y + (vy - scroll);
        let screen_x = inner.x.saturating_add(x_off);
        let max_w = inner.width.saturating_sub(x_off);
        let width = w.min(max_w);
        if width == 0 {
            continue;
        }
        rects
            .statuses
            .push((st, Rect::new(screen_x, screen_y, width, 1)));
    }

    if let Some(target) = form.editor_target {
        let target_focus = match target {
            EditorTarget::Description => FormFocus::Description,
            EditorTarget::Comment(i) => FormFocus::Comment(i),
            EditorTarget::NewComment => FormFocus::NewComment,
        };
        let rect = rects
            .fields
            .iter()
            .find(|(f, _)| *f == target_focus)
            .map(|(_, r)| *r);
        if let (Some(rect), Some(editor)) = (rect, form.editor.as_mut()) {
            editor.focused = true;
            let widget = EditorWidget {
                view: editor,
                area_title: editor_overlay_title(target),
                git_status: None,
            };
            frame.render_widget(Clear, rect);
            frame.render_widget(widget, rect);
        }
    }

    rects
}

fn editor_overlay_title(target: crate::views::feature_form::EditorTarget) -> String {
    use crate::views::feature_form::EditorTarget;
    match target {
        EditorTarget::Description => "Description (Esc/Backspace to commit)".into(),
        EditorTarget::Comment(_) => "Message (Esc/Backspace to commit)".into(),
        EditorTarget::NewComment => "New message (Esc/Backspace to commit)".into(),
    }
}

struct FeatureLayout {
    lines: Vec<Line<'static>>,
    regions: Vec<(crate::views::feature_form::FormFocus, u16, u16)>,
    statuses: Vec<(crate::project::FeatureStatus, u16, u16, u16)>,
}

fn build_feature_layout(
    form: &crate::views::feature_form::FeatureForm,
    focused: bool,
    inner_width: u16,
) -> FeatureLayout {
    use crate::project::{CommentStatus, FeatureStatus, StepStatus};
    use crate::views::feature_form::FormFocus;

    let wrap_width = (inner_width as usize).min(120).max(10);
    let indent_lvl1: usize = 2;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut regions: Vec<(FormFocus, u16, u16)> = Vec::new();
    let mut statuses: Vec<(FeatureStatus, u16, u16, u16)> = Vec::new();

    let title_focused = focused && matches!(form.focus, FormFocus::Title);
    let status_focused = focused && matches!(form.focus, FormFocus::Status);
    let prefix = if title_focused {
        "▶ "
    } else if status_focused {
        "  "
    } else {
        "  "
    };
    let title_text = if form.title.is_empty() {
        "(untitled — Enter to edit)".to_string()
    } else {
        form.title.clone()
    };
    let title_style = if form.title.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let title_line_y = lines.len() as u16;
    let mut title_spans: Vec<Span<'static>> = Vec::new();
    title_spans.push(Span::raw(prefix.to_string()));
    let mut x_off = prefix.chars().count() as u16;
    if title_focused {
        push_cursor_spans(&mut title_spans, &title_text, form.cursor, title_style);
        x_off = x_off.saturating_add(title_text.chars().count() as u16);
    } else {
        title_spans.push(Span::styled(title_text.clone(), title_style));
        x_off = x_off.saturating_add(title_text.chars().count() as u16);
    }
    title_spans.push(Span::raw(" ".to_string()));
    x_off = x_off.saturating_add(1);
    let status_label = format!("[{}]", form.status.label());
    let status_color = feature_status_color(form.status);
    let status_w = status_label.chars().count() as u16;
    let status_style = if status_focused {
        Style::default()
            .bg(status_color)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(status_color).add_modifier(Modifier::BOLD)
    };
    title_spans.push(Span::styled(status_label, status_style));
    lines.push(Line::from(title_spans));
    regions.push((FormFocus::Title, title_line_y, 1));
    regions.push((FormFocus::Status, title_line_y, 1));
    statuses.push((form.status, title_line_y, x_off, status_w));

    lines.push(Line::from(""));

    let desc_focused = focused && matches!(form.focus, FormFocus::Description);
    let desc_y = lines.len() as u16;
    let prefix = if desc_focused { "▶ " } else { "  " };
    let cont_prefix = "  ";
    if form.description.trim().is_empty() {
        let placeholder = if desc_focused {
            "(empty — Enter to edit)".to_string()
        } else {
            "(no description — Enter to edit)".to_string()
        };
        lines.push(Line::from(vec![
            Span::raw(prefix.to_string()),
            Span::styled(placeholder, Style::default().fg(Color::DarkGray)),
        ]));
        regions.push((FormFocus::Description, desc_y, 1));
    } else {
        let wrapped = wrap_paragraph(&form.description, wrap_width.saturating_sub(indent_lvl1));
        let count = wrapped.len() as u16;
        for (i, w) in wrapped.into_iter().enumerate() {
            let p = if i == 0 { prefix } else { cont_prefix };
            lines.push(Line::from(vec![Span::raw(p.to_string()), Span::raw(w)]));
        }
        regions.push((FormFocus::Description, desc_y, count.max(1)));
    }

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  Steps".to_string(),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )));

    for (i, step) in form.steps.iter().enumerate() {
        if step.deleted {
            continue;
        }
        let step_focused = focused && matches!(form.focus, FormFocus::Step(idx) if idx == i);
        let prefix = if step_focused { "  ▶ " } else { "    " };
        let glyph_style = match step.status {
            StepStatus::Done => Style::default().fg(Color::Green),
            StepStatus::InProgress => Style::default().fg(Color::Yellow),
            StepStatus::Todo => Style::default(),
        };
        let head = format!("{}{} ", prefix, step.status.glyph());
        let head_indent = head.chars().count() as u16 as usize;
        let avail = wrap_width.saturating_sub(head_indent).max(8);
        let wrapped = wrap_paragraph(&step.summary, avail);
        let lines_for_step = wrapped.len().max(1);
        let step_y = lines.len() as u16;
        for (j, w) in wrapped.into_iter().enumerate() {
            if j == 0 {
                let mut spans: Vec<Span<'static>> = vec![Span::styled(head.clone(), glyph_style)];
                if step_focused && step.summary.chars().count() <= avail {
                    push_cursor_spans(&mut spans, &w, form.cursor, Style::default());
                } else {
                    spans.push(Span::raw(w));
                }
                lines.push(Line::from(spans));
            } else {
                let pad: String = std::iter::repeat(' ').take(head_indent).collect();
                lines.push(Line::from(vec![Span::raw(pad), Span::raw(w)]));
            }
        }
        regions.push((FormFocus::Step(i), step_y, lines_for_step as u16));
    }

    let ns_focused = focused && matches!(form.focus, FormFocus::NewStep);
    let ns_prefix = if ns_focused { "  ▶ + " } else { "    + " };
    let ns_y = lines.len() as u16;
    let head_style = if ns_focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let mut spans: Vec<Span<'static>> = vec![Span::styled(ns_prefix.to_string(), head_style)];
    if ns_focused {
        let buf = if form.new_step_buf.is_empty() {
            String::new()
        } else {
            form.new_step_buf.clone()
        };
        push_cursor_spans(&mut spans, &buf, form.cursor, Style::default());
        if buf.is_empty() && form.cursor == 0 {
            // cursor rendered already via push_cursor_spans
        }
    } else {
        spans.push(Span::styled(
            "new step".to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    lines.push(Line::from(spans));
    regions.push((FormFocus::NewStep, ns_y, 1));

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  Messages".to_string(),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )));

    for (i, comment) in form.comments.iter().enumerate() {
        if comment.deleted {
            continue;
        }
        let msg_focused = focused && matches!(form.focus, FormFocus::Comment(idx) if idx == i);
        let prefix = if msg_focused { "  ▶ " } else { "    " };
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
        let head_indent = head.chars().count();
        let avail = wrap_width.saturating_sub(head_indent).max(8);
        let wrapped = wrap_paragraph(&comment.message, avail);
        let mut height: u16 = 0;
        let msg_y = lines.len() as u16;
        if wrapped.is_empty() {
            lines.push(Line::from(vec![Span::styled(head.clone(), badge_style)]));
            height = 1;
        } else {
            for (j, w) in wrapped.into_iter().enumerate() {
                if j == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(head.clone(), badge_style),
                        Span::raw(w),
                    ]));
                } else {
                    let pad: String = std::iter::repeat(' ').take(head_indent).collect();
                    lines.push(Line::from(vec![Span::raw(pad), Span::raw(w)]));
                }
                height += 1;
            }
        }
        regions.push((FormFocus::Comment(i), msg_y, height.max(1)));
    }

    let nc_focused = focused && matches!(form.focus, FormFocus::NewComment);
    let nc_prefix = if nc_focused { "  ▶ + " } else { "    + " };
    let nc_y = lines.len() as u16;
    let head_style = if nc_focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let spans: Vec<Span<'static>> = if nc_focused {
        let hint = if form.new_comment_buf.trim().is_empty() {
            "(Enter to open editor)".to_string()
        } else {
            form.new_comment_buf.clone()
        };
        vec![
            Span::styled(nc_prefix.to_string(), head_style),
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ]
    } else {
        vec![
            Span::styled(nc_prefix.to_string(), head_style),
            Span::styled(
                "new message".to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]
    };
    lines.push(Line::from(spans));
    regions.push((FormFocus::NewComment, nc_y, 1));

    FeatureLayout {
        lines,
        regions,
        statuses,
    }
}

fn push_cursor_spans(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    cursor: usize,
    base: Style,
) {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let before: String = chars[..cursor].iter().collect();
    let at: String = if cursor < chars.len() {
        chars[cursor].to_string()
    } else {
        " ".to_string()
    };
    let after: String = if cursor + 1 < chars.len() {
        chars[cursor + 1..].iter().collect()
    } else {
        String::new()
    };
    if !before.is_empty() {
        spans.push(Span::styled(before, base));
    }
    spans.push(Span::styled(at, base.add_modifier(Modifier::REVERSED)));
    if !after.is_empty() {
        spans.push(Span::styled(after, base));
    }
}

fn wrap_paragraph(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if width == 0 {
        out.push(text.to_string());
        return out;
    }
    let paragraphs: Vec<&str> = text.split('\n').collect();
    for paragraph in paragraphs {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut cur = String::new();
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if cur.is_empty() {
                if word_len > width {
                    let chars: Vec<char> = word.chars().collect();
                    for chunk in chars.chunks(width) {
                        out.push(chunk.iter().collect());
                    }
                } else {
                    cur.push_str(word);
                }
            } else if cur.chars().count() + 1 + word_len <= width {
                cur.push(' ');
                cur.push_str(word);
            } else {
                out.push(std::mem::take(&mut cur));
                if word_len > width {
                    let chars: Vec<char> = word.chars().collect();
                    for chunk in chars.chunks(width) {
                        out.push(chunk.iter().collect());
                    }
                } else {
                    cur.push_str(word);
                }
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
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

fn split_off_agent_lane(app: &mut App, frame: &mut Frame<'_>, area: Rect) -> Rect {
    if !app.agent_lane_visible || area.width < 36 {
        app.agent_lane_area = Rect::default();
        app.agent_lane_tile_rects.clear();
        return area;
    }
    let max_lane = area.width.saturating_sub(24).max(20);
    let min_lane: u16 = 20;
    let lane_width = app.agent_lane_width.clamp(min_lane, max_lane);
    app.agent_lane_width = lane_width;
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

fn render_runtime_body(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let Some(project) = app.open_projects.get(app.active_index).cloned() else {
        return;
    };
    let needs_load = app
        .project_views
        .get(&project.id)
        .map(|s| s.runtime.is_none())
        .unwrap_or(false);
    if needs_load {
        app.ensure_runtime_loaded();
    }
    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let Some(runtime) = state.runtime.as_ref() else {
        return;
    };
    let rects =
        crate::views::runtime::render_runtime(runtime, frame, area, &project.name);
    app.left_pane_area = rects.list;
    app.right_pane_area = rects.log;
    app.runtime_list_area = rects.list;
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
    let focused = matches!(app.focus_context, FocusContext::SubTabs);

    let mut spans: Vec<Span> = Vec::new();
    let marker = if focused { "▶ " } else { "  " };
    spans.push(Span::styled(
        marker,
        Style::default().fg(if focused { Color::Yellow } else { Color::DarkGray }),
    ));
    let mut x = chunks[0].x + marker.chars().count() as u16;
    for i in 0..total {
        let text = format!(" {} ", i + 1);
        let w = text.chars().count() as u16;
        let style = if i == active {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else if focused {
            Style::default().fg(Color::Yellow)
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
    let focused = matches!(app.focus_context, FocusContext::SubTabs);

    let mut spans: Vec<Span> = Vec::new();
    let marker = if focused { "▶ " } else { "  " };
    spans.push(Span::styled(
        marker,
        Style::default().fg(if focused { Color::Yellow } else { Color::DarkGray }),
    ));
    let mut x = chunks[0].x + marker.chars().count() as u16;
    for (i, agent) in state.agents.iter().enumerate() {
        let text = format!(" {} ", agent.name);
        let w = text.chars().count() as u16;
        let style = if i == active {
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else if focused {
            Style::default().fg(Color::Yellow)
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
    let focused = matches!(app.focus_context, FocusContext::ProjectTabs);
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let title = if focused {
        " CoffeeTable ◀ "
    } else {
        " CoffeeTable "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);
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
            if focused {
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            }
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
        .constraints([Constraint::Percentage(app.split_pct), Constraint::Min(20)])
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
            let ft = d.file_type().ok()?;
            if ft.is_dir() && name == ".git" {
                return None;
            }
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
                "j/k move • Tab pane • Enter open • c checkout • p push • P pull • m merge • R PR • V PRs • W worktrees • n new • D del • r refresh • ? help".to_string()
            }
            Some(crate::app::ViewMode::Github) => {
                "j/k move • Tab pane • Enter open • r refresh • R rerun • F rerun-failed • X cancel • c checkout PR • ? help".to_string()
            }
            Some(crate::app::ViewMode::Terminal) => {
                "Ctrl+Space leader • Ctrl+L/H switch • Ctrl+Shift+C SIGINT • Ctrl+J/K view • ? help".to_string()
            }
            Some(crate::app::ViewMode::Runtime) => {
                "j/k move • Enter/f filter • r run • R run all • s stop • b build • x restart • e reload config • c clear log • ? help".to_string()
            }
            _ => match current_focus(app) {
                Some(Focus::Editor) => {
                    "Ctrl+P palette  •  i insert  •  v visual  •  /search  •  Backspace tree  •  Space menu  •  ? help"
                        .to_string()
                }
                _ => {
                    "Ctrl+P palette  •  Space menu  •  ? help  •  Tab switch  •  Space q quit".to_string()
                }
            },
        },
        AppMode::Picker => "? help  •  Esc close".into(),
        AppMode::Grep => "Type to filter  •  Enter open  •  Esc cancel".into(),
        AppMode::OpenConfirm => "y / Enter open  •  n / Esc cancel".into(),
        AppMode::ConfirmDeleteFeature => "y / Enter delete  •  n / Esc cancel".into(),
        AppMode::AgentRename => "Type to edit  •  Enter save  •  Esc cancel".into(),
        AppMode::WorktreePrompt => "Branch name for new worktree  •  Enter create  •  Esc cancel".into(),
        AppMode::FilePrompt => "Type a name  •  Enter confirm  •  Esc cancel".into(),
        AppMode::ConfirmDeleteFile => "y / Enter delete  •  n / Esc cancel".into(),
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
        AppMode::Settings => {
            "Tab pane  •  Ctrl+S save  •  Ctrl+I import  •  Ctrl+E export  •  Esc close".into()
        }
        AppMode::LogView => {
            "j/k scroll  •  g/G top/bottom  •  c clear  •  y copy  •  Esc/q close".into()
        }
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_status_toolbar(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let project = app.open_projects.get(app.active_index);
    let cwd_span = match project {
        Some(p) => Span::styled(
            format!(" {} ", p.path.display()),
            Style::default().fg(Color::Cyan),
        ),
        None => Span::styled(
            " (no project) ".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    };

    let git_spans = build_git_status_spans(app);
    let token_spans = build_token_spans(app);

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(cwd_span);
    if !git_spans.is_empty() {
        spans.push(Span::styled(
            "│".to_string(),
            Style::default().fg(Color::DarkGray),
        ));
        spans.extend(git_spans);
    }
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let token_used: usize = token_spans.iter().map(|s| s.content.chars().count()).sum();
    let total_w = area.width as usize;
    if total_w > used + token_used + 1 {
        let pad = total_w - used - token_used;
        spans.push(Span::raw(" ".repeat(pad)));
    } else {
        spans.push(Span::raw(" "));
    }
    spans.extend(token_spans);

    let line = Line::from(spans);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 20, 28))),
        area,
    );
}

fn build_git_status_spans(app: &App) -> Vec<Span<'static>> {
    let Some(project) = app.open_projects.get(app.active_index) else {
        return Vec::new();
    };
    let Some(state) = app.project_views.get(&project.id) else {
        return Vec::new();
    };
    let branch = state
        .branch
        .clone()
        .unwrap_or_else(|| "(no branch)".into());
    let statuses = state.tree.git_status();
    let mut modified = 0usize;
    let mut staged = 0usize;
    let mut untracked = 0usize;
    let mut deleted = 0usize;
    for (_, st) in statuses.iter() {
        match st {
            crate::git::GitStatus::Modified => modified += 1,
            crate::git::GitStatus::Staged => staged += 1,
            crate::git::GitStatus::Untracked => untracked += 1,
            crate::git::GitStatus::Deleted => deleted += 1,
        }
    }
    let mut spans = vec![Span::styled(
        format!(" {} ", branch),
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )];
    if modified == 0 && staged == 0 && untracked == 0 && deleted == 0 {
        spans.push(Span::styled(
            "clean ".to_string(),
            Style::default().fg(Color::Green),
        ));
    } else {
        if staged > 0 {
            spans.push(Span::styled(
                format!("●{} ", staged),
                Style::default().fg(Color::Green),
            ));
        }
        if modified > 0 {
            spans.push(Span::styled(
                format!("✚{} ", modified),
                Style::default().fg(Color::Yellow),
            ));
        }
        if deleted > 0 {
            spans.push(Span::styled(
                format!("✖{} ", deleted),
                Style::default().fg(Color::Red),
            ));
        }
        if untracked > 0 {
            spans.push(Span::styled(
                format!("?{} ", untracked),
                Style::default().fg(Color::Cyan),
            ));
        }
    }
    spans
}

fn build_token_spans(app: &App) -> Vec<Span<'static>> {
    use crate::token_usage::{format_compact, format_duration};
    let snapshot = app
        .token_usage
        .lock()
        .map(|s| *s)
        .unwrap_or_default();

    let limits = app
        .open_projects
        .get(app.active_index)
        .map(|p| {
            let s = app.effective_settings_for(p.id);
            (s.ai.session_token_limit, s.ai.weekly_token_limit)
        })
        .unwrap_or((None, None));

    if snapshot.last_scan_at.is_none() {
        return vec![Span::styled(
            " tokens: scanning… ".to_string(),
            Style::default().fg(Color::DarkGray),
        )];
    }
    if let Some(err) = snapshot.last_error {
        return vec![Span::styled(
            format!(" tokens: {} ", err),
            Style::default().fg(Color::Red),
        )];
    }

    let has_limits = limits.0.is_some() || limits.1.is_some();
    if has_limits {
        let session_span = token_window_span("5h", snapshot.session_tokens, limits.0);
        let weekly_span = token_window_span("7d", snapshot.weekly_tokens, limits.1);
        return vec![
            Span::styled(" tokens ".to_string(), Style::default().fg(Color::DarkGray)),
            session_span,
            Span::styled(" · ".to_string(), Style::default().fg(Color::DarkGray)),
            weekly_span,
            Span::raw(" "),
        ];
    }

    vec![
        Span::styled(" tokens ".to_string(), Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("5h {}", format_compact(snapshot.session_tokens)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(" · thinking ".to_string(), Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("5h {}", format_duration(snapshot.session_thinking_secs)),
            Style::default().fg(Color::Magenta),
        ),
        Span::raw(" "),
    ]
}

fn token_window_span(label: &str, used: u64, limit: Option<u64>) -> Span<'static> {
    use crate::token_usage::format_compact;
    match limit {
        Some(cap) if cap > 0 => {
            let left = cap.saturating_sub(used);
            let pct = (used as f64 / cap as f64).clamp(0.0, 1.0);
            let color = if pct >= 0.9 {
                Color::Red
            } else if pct >= 0.7 {
                Color::Yellow
            } else {
                Color::Green
            };
            Span::styled(
                format!("{} {} left ({:.0}%)", label, format_compact(left), pct * 100.0),
                Style::default().fg(color),
            )
        }
        _ => Span::styled(
            format!("{} {} used", label, format_compact(used)),
            Style::default().fg(Color::Cyan),
        ),
    }
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

fn render_settings_overlay(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let Some(overlay) = app.settings_overlay.as_mut() else { return };
    let popup = centered_rect(80, 80, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Settings ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    overlay.global.focused = matches!(overlay.focus, SettingsPane::Global);
    overlay.project.focused = matches!(overlay.focus, SettingsPane::Project);

    let global_title = format!("global · {}", overlay.global.path.display());
    let project_title = match overlay.project_id {
        Some(_) => "project · DB-backed".to_string(),
        None => "project · (no active project)".to_string(),
    };

    let left_widget = crate::views::editor::EditorWidget {
        view: &mut overlay.global,
        area_title: global_title,
        git_status: None,
    };
    frame.render_widget(left_widget, panes[0]);

    let right_widget = crate::views::editor::EditorWidget {
        view: &mut overlay.project,
        area_title: project_title,
        git_status: None,
    };
    frame.render_widget(right_widget, panes[1]);

    let status_line = if overlay.status.is_empty() {
        Line::from(Span::styled(
            "  Tab switch pane  •  Ctrl+S save  •  Ctrl+I import file  •  Ctrl+E export file  •  Esc close",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(Span::styled(
            format!("  {}", overlay.status),
            Style::default().fg(Color::Yellow),
        ))
    };
    frame.render_widget(Paragraph::new(status_line), chunks[1]);

    let hint = Line::from(Span::styled(
        "  Left pane writes to settings.yaml on disk. Right pane writes to DB; use import/export for the on-disk CoffeeTable.Settings.yaml.",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(hint), chunks[2]);
}

fn render_log_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(80, 70, area);
    frame.render_widget(Clear, popup);
    let title = format!(" Log ({} entries) ", app.log.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(inner);

    let body_h = chunks[0].height as usize;
    let total = app.log.len();
    let entries: Vec<&crate::log::LogEntry> = app.log.iter().collect();

    let lines: Vec<Line> = if entries.is_empty() {
        vec![Line::from(Span::styled(
            "  (no log entries yet)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        let scroll = app.log_scroll.min(total.saturating_sub(1));
        let start = scroll;
        let end = total.min(start + body_h);
        entries[start..end]
            .iter()
            .map(|e| {
                let (label, label_color) = match e.kind {
                    crate::log::LogKind::Info => ("INFO ", Color::Cyan),
                    crate::log::LogKind::Warn => ("WARN ", Color::Yellow),
                    crate::log::LogKind::Error => ("ERROR", Color::Red),
                };
                Line::from(vec![
                    Span::styled(
                        format!(" {:<10}", crate::log::relative_age(e.timestamp)),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{} ", label),
                        Style::default().fg(label_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(e.text.clone()),
                ])
            })
            .collect()
    };

    frame.render_widget(Paragraph::new(lines), chunks[0]);

    let footer = Line::from(Span::styled(
        "  j/k scroll · g/G top/bottom · c clear · y copy · Esc/q close",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(footer), chunks[1]);
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

fn render_agent_rename_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(st) = app.agent_rename.as_ref() else { return };
    let popup = centered_rect_fixed(60, 7, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Rename agent ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let input_inner = input_block.inner(chunks[0]);
    frame.render_widget(input_block, chunks[0]);
    render_inline_input(frame, input_inner, &st.buffer, st.cursor, "(new name)");

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]))
        .style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}

fn render_worktree_prompt_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(st) = app.worktree_prompt.as_ref() else { return };
    let popup = centered_rect_fixed(70, 9, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" New worktree ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let path_preview = app
        .open_projects
        .get(app.active_index)
        .map(|p| {
            let preview = crate::app::preview_worktree_path(&p.path, &st.buffer);
            format!("  Path: {}", preview.display())
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            path_preview,
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[0],
    );

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" branch ");
    let input_inner = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);
    render_inline_input(frame, input_inner, &st.buffer, st.cursor, "(new branch name)");

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" create branch + worktree  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]))
        .style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

fn render_file_prompt_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    use crate::app::FilePromptKind;
    let Some(st) = app.file_prompt.as_ref() else { return };
    let (title, label, hint) = match st.kind {
        FilePromptKind::NewFile => (" New file ", " name ", "(filename to create)"),
        FilePromptKind::NewDir => (" New directory ", " name ", "(directory name)"),
        FilePromptKind::Rename => (" Rename ", " name ", "(new name)"),
    };
    let popup = centered_rect_fixed(74, 9, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let preview = st.parent.join(if st.buffer.trim().is_empty() {
        "<name>".to_string()
    } else {
        st.buffer.clone()
    });
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  Path: {}", preview.display()),
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[0],
    );

    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(label);
    let input_inner = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);
    render_inline_input(frame, input_inner, &st.buffer, st.cursor, hint);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" confirm  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]))
        .style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

fn render_delete_file_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(path) = app.pending_delete_file.as_ref() else { return };
    let popup = centered_rect_fixed(74, 9, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" Delete file? ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let kind_note = if path.is_dir() {
        "  This will recursively delete the directory and its contents."
    } else {
        "  This will permanently delete the file."
    };
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Delete "),
            Span::styled(
                path.display().to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
        Line::from(Span::styled(
            kind_note,
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
        AppMode::Settings => (
            "Settings",
            vec![
                ("Tab / BackTab", "switch between global (left) and project (right) panes"),
                ("Ctrl+S", "save focused pane (left → disk, right → DB)"),
                ("Ctrl+I", "import CoffeeTable.Settings.yaml from project root into DB"),
                ("Ctrl+E", "export project settings from DB to CoffeeTable.Settings.yaml"),
                ("i / a / o", "vim-style edit in focused pane"),
                ("Esc", "close (press again to discard unsaved changes)"),
            ],
        ),
        AppMode::LogView => (
            "Log",
            vec![
                ("j / k", "scroll one line"),
                ("g / G", "jump to top / bottom"),
                ("c", "clear log"),
                ("y", "copy all log entries to clipboard"),
                ("Esc / q", "close"),
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
        AppMode::AgentRename => (
            "Rename agent",
            vec![
                ("(text)", "type the new agent name"),
                ("← / →  Home / End", "move cursor"),
                ("Backspace / Delete", "edit text"),
                ("Enter", "save"),
                ("Esc", "cancel"),
            ],
        ),
        AppMode::WorktreePrompt => (
            "New worktree",
            vec![
                ("(text)", "branch name (will be created if missing)"),
                ("← / →  Home / End", "move cursor"),
                ("Backspace / Delete", "edit text"),
                ("Enter", "create branch + worktree alongside repo"),
                ("Esc", "cancel"),
            ],
        ),
        AppMode::FilePrompt => (
            "File prompt",
            vec![
                ("(text)", "file or directory name"),
                ("← / →  Home / End", "move cursor"),
                ("Backspace / Delete", "edit text"),
                ("Enter", "create / rename"),
                ("Esc", "cancel"),
            ],
        ),
        AppMode::ConfirmDeleteFile => (
            "Delete file?",
            vec![
                ("y / Enter", "delete"),
                ("n / Esc", "cancel"),
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
            let in_github = app
                .open_projects
                .get(app.active_index)
                .and_then(|p| app.project_views.get(&p.id))
                .map(|s| matches!(s.view_mode, crate::app::ViewMode::Github))
                .unwrap_or(false);
            if in_github {
                return (
                    "GitHub",
                    vec![
                        ("j/k  ↓/↑", "navigate within focused pane"),
                        ("Tab / Shift+Tab", "cycle panes (runs → PRs → details)"),
                        ("Enter / l", "load full run log / PR view into details"),
                        ("Esc / Backspace", "back to list pane (clears details)"),
                        ("g / G", "jump to top / bottom of focused pane"),
                        ("r", "refresh runs + PRs"),
                        ("R", "re-run selected workflow run (all jobs)"),
                        ("F", "re-run only failed jobs of selected run"),
                        ("X", "cancel selected workflow run"),
                        ("c", "checkout selected PR (gh pr checkout)"),
                        ("Ctrl+J / Ctrl+K", "switch view"),
                    ],
                );
            }
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
                        ("W", "list worktrees in the details pane"),
                        ("n", "create a new worktree (prompts for branch)"),
                        ("D", "delete the selected worktree"),
                        ("Enter (worktree)", "open the worktree as a project tab"),
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
            let in_runtime = app
                .open_projects
                .get(app.active_index)
                .and_then(|p| app.project_views.get(&p.id))
                .map(|s| matches!(s.view_mode, crate::app::ViewMode::Runtime))
                .unwrap_or(false);
            if in_runtime {
                return (
                    "Runtime",
                    vec![
                        ("j/k  ↓/↑", "navigate services"),
                        ("g / G", "jump to first / last service"),
                        ("Enter / f", "toggle output filter to selected service"),
                        ("Esc", "clear output filter"),
                        ("c", "clear output buffer"),
                        ("e", "reload CoffeeTable.Runtime.yaml"),
                        ("r / R", "run selected / run all"),
                        ("s / S", "stop selected / stop all"),
                        ("b / B", "build selected / build all"),
                        ("x / X", "restart selected / restart all"),
                        (":run [name]", "palette command (also :stop, :build, :restart)"),
                        ("Ctrl+J / Ctrl+K", "switch view"),
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
                        ("Ctrl+P", "open command palette (works globally)"),
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
                        ("Ctrl+P", "open command palette (dropdown)"),
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
