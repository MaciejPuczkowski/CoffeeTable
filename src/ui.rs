use crate::{
    app::{App, AppMode, Focus, LeftPaneMode},
    views::{
        changes::ChangesWidget,
        editor::{COMMANDS, EditorMode, EditorWidget, filter_commands, render_command_line},
        file_finder::FileFinderWidget,
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
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(app, frame, chunks[0]);
    render_body(app, frame, chunks[1]);
    render_command_or_status(app, frame, chunks[2]);
    render_footer(app, frame, chunks[3]);

    match app.mode {
        AppMode::Picker => render_picker_overlay(app, frame, area),
        AppMode::FileFinder => render_finder_overlay(app, frame, area),
        AppMode::Grep => render_grep_overlay(app, frame, area),
        AppMode::OpenConfirm => render_open_confirm_overlay(app, frame, area),
        AppMode::Normal => {}
    }
    if editor_in_command_mode(app) {
        render_command_palette_overlay(app, frame, area);
    }
    if app.leader_pending {
        render_leader_overlay(frame, area);
    }
    if app.help_visible {
        render_help_overlay(app, frame, area);
    }
}

fn editor_in_command_mode(app: &App) -> bool {
    matches!(
        current_editor(app).map(|e| e.mode),
        Some(EditorMode::Command)
    )
}

fn render_command_palette_overlay(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let Some(editor) = current_editor(app) else { return };
    let filtered = filter_commands(&editor.command);
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

    let input = Paragraph::new(format!(":{}", editor.command))
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
        editor.command_selection.min(filtered.len() - 1)
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

fn render_leader_overlay(frame: &mut Frame<'_>, area: Rect) {
    let entries: &[(&str, &str)] = &[
        ("p", "Projects (picker)"),
        ("f", "Find file"),
        ("g", "Grep"),
        ("c", "Changes ↔ tree (toggle)"),
        ("e", "Explorer (focus tree)"),
        ("b", "Buffer (focus editor)"),
        ("w", "Toggle focus"),
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
    let Some(state) = app.project_views.get_mut(&project.id) else {
        return;
    };
    let tree_focused = normal_mode && state.focus == Focus::Tree;
    let editor_focused = normal_mode && state.focus == Focus::Editor;

    let title = format!(" {} ", project.path.display());
    let selected = state.selected_path().map(|p| p.to_path_buf());
    match state.left_pane {
        LeftPaneMode::Tree => {
            let tree_widget = FileTreeWidget {
                view: &mut state.tree,
                title,
                focused: tree_focused,
            };
            frame.render_widget(tree_widget, chunks[0]);
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
        let (marker, marker_style) = if is_dir {
            ("▸ ", Style::default().fg(Color::Cyan))
        } else {
            ("  ", Style::default().fg(Color::DarkGray))
        };
        let name_style = if is_dir {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let display = if is_dir {
            format!("{}/", name)
        } else {
            name.clone()
        };
        out.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(marker.to_string(), marker_style),
            Span::styled(display, name_style),
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
        AppMode::Normal => match current_focus(app) {
            Some(Focus::Editor) => {
                ":palette  •  i insert  •  v visual  •  /search  •  Backspace tree  •  Space menu  •  ? help"
                    .to_string()
            }
            _ => {
                "Space menu  •  ? help  •  Tab switch project  •  Space q quit".to_string()
            }
        },
        AppMode::Picker => "? help  •  Esc close".into(),
        AppMode::FileFinder | AppMode::Grep => "Type to filter  •  Enter open  •  Esc cancel".into(),
        AppMode::OpenConfirm => "y / Enter open  •  n / Esc cancel".into(),
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

fn render_finder_overlay(app: &mut App, frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup);
    if let Some(finder) = app.file_finder.as_mut() {
        frame.render_widget(FileFinderWidget { finder }, popup);
    }
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
                        ("Ctrl+C", "quit app"),
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
                        ("Ctrl+C", "quit app"),
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
        AppMode::FileFinder => (
            "Find file",
            vec![
                ("(text)", "fuzzy filter"),
                ("↑/↓", "navigate results"),
                ("Enter", "open in editor"),
                ("Esc", "cancel"),
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
        AppMode::Normal => {
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
                        ("Space c", "toggle Changes view ↔ Tree view"),
                        ("Space p / f / g", "projects / find file / grep"),
                        ("Space e / b / w", "focus tree / editor / toggle"),
                        ("Space q  /  Ctrl+C", "quit"),
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
                        ("Backspace", "focus the explorer (file stays open)"),
                        ("Esc (from Insert)", "back to normal — autosaves file"),
                        ("Space (normal/visual)", "leader menu"),
                        ("Space p / f / g", "projects / find file / grep"),
                        ("Space e / b / w", "focus tree / editor / toggle"),
                        ("Space q  /  Ctrl+C", "quit app"),
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
