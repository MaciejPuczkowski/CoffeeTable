use super::EditorView;
use super::types::EditorMode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Widget},
};

const SELECTION_BG: Color = Color::Rgb(33, 66, 131);

pub struct EditorWidget<'a> {
    pub view: &'a mut EditorView,
    pub area_title: String,
}

impl<'a> Widget for EditorWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.view.last_render_area = Some(area);
        let inner = render_block(&self.view, &self.area_title, area, buf);
        self.view.viewport_rows = inner.height;
        self.view.ensure_cursor_visible();

        let layout = BodyLayout::for_view(self.view, inner);
        let highlighted = self.view.render_visible_lines(layout.visible_range_end());
        let selection = SelectionFrame::for_mode(self.view);

        for (i, row_idx) in (self.view.scroll_row..layout.visible_range_end()).enumerate() {
            let cell_y = inner.y + i as u16;
            paint_gutter(buf, inner, &layout, row_idx, cell_y);
            paint_line(
                buf,
                self.view,
                &layout,
                row_idx,
                cell_y,
                highlighted.get(row_idx),
                &selection,
            );
        }
    }
}

fn render_block(view: &EditorView, title: &str, area: Rect, buf: &mut Buffer) -> Rect {
    let border_style = if view.focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let modified = if view.modified { " [+]" } else { "" };
    let title = format!(" {}{} ", title, modified);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);
    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

struct BodyLayout {
    inner: Rect,
    gutter_width: u16,
    body_x: u16,
    body_w: usize,
    scroll_row: usize,
    total_lines: usize,
}

impl BodyLayout {
    fn for_view(view: &mut EditorView, inner: Rect) -> Self {
        let total = view.lines.len();
        let gutter_width = total.to_string().len().max(2) as u16 + 1;
        view.gutter_width = gutter_width;
        let body_x = inner.x + gutter_width + 1;
        let body_w = inner.width.saturating_sub(gutter_width + 1) as usize;
        Self {
            inner,
            gutter_width,
            body_x,
            body_w,
            scroll_row: view.scroll_row,
            total_lines: total,
        }
    }

    fn visible_range_end(&self) -> usize {
        (self.scroll_row + self.inner.height as usize).min(self.total_lines)
    }
}

struct SelectionFrame {
    start: (usize, usize),
    end: (usize, usize),
    linewise: bool,
    active: bool,
}

impl SelectionFrame {
    fn for_mode(view: &EditorView) -> Self {
        match view.mode {
            EditorMode::Visual | EditorMode::VisualLine => {
                let (start, end, linewise) = view.selection_range();
                Self { start, end, linewise, active: true }
            }
            _ => Self {
                start: (0, 0),
                end: (0, 0),
                linewise: false,
                active: false,
            },
        }
    }

    fn contains(&self, row: usize, col: usize) -> bool {
        self.active && in_selection(row, col, self.start, self.end, self.linewise)
    }
}

impl EditorView {
    fn render_visible_lines(&self, end: usize) -> Vec<Vec<(Style, String)>> {
        let visible: Vec<String> = self
            .lines
            .iter()
            .take(end)
            .map(|l| l.iter().collect())
            .collect();
        self.highlighter.highlight_lines(&visible)
    }
}

fn paint_gutter(buf: &mut Buffer, inner: Rect, layout: &BodyLayout, row_idx: usize, cell_y: u16) {
    let gutter = format!(
        "{:>width$} ",
        row_idx + 1,
        width = layout.gutter_width as usize - 1
    );
    let style = Style::default().fg(Color::DarkGray);
    for (gi, ch) in gutter.chars().enumerate() {
        let x = inner.x + gi as u16;
        if x >= inner.x + inner.width {
            break;
        }
        buf[(x, cell_y)].set_char(ch).set_style(style);
    }
}

fn paint_line(
    buf: &mut Buffer,
    view: &EditorView,
    layout: &BodyLayout,
    row_idx: usize,
    cell_y: u16,
    spans: Option<&Vec<(Style, String)>>,
    selection: &SelectionFrame,
) {
    let scroll_col = view.scroll_col;
    let mut char_idx = 0usize;
    if let Some(spans) = spans {
        'spans: for (style, text) in spans {
            for ch in text.chars() {
                if char_idx >= scroll_col {
                    let disp_x = char_idx - scroll_col;
                    if disp_x >= layout.body_w {
                        break 'spans;
                    }
                    let cell_style =
                        style_for_cell(view, *style, row_idx, char_idx, selection);
                    buf[(layout.body_x + disp_x as u16, cell_y)]
                        .set_char(ch)
                        .set_style(cell_style);
                }
                char_idx += 1;
            }
        }
    }
    paint_trailing_cursor(buf, view, layout, row_idx, cell_y, char_idx);
    if selection.active && selection.linewise {
        paint_linewise_trailing(buf, view, layout, row_idx, cell_y, char_idx, selection);
    }
}

fn style_for_cell(
    view: &EditorView,
    base: Style,
    row: usize,
    col: usize,
    selection: &SelectionFrame,
) -> Style {
    let mut style = base;
    if selection.contains(row, col) {
        style = style.bg(SELECTION_BG);
    }
    if view.focused && row == view.cursor.0 && col == view.cursor.1 {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn paint_trailing_cursor(
    buf: &mut Buffer,
    view: &EditorView,
    layout: &BodyLayout,
    row_idx: usize,
    cell_y: u16,
    char_idx: usize,
) {
    if !view.focused
        || row_idx != view.cursor.0
        || view.cursor.1 < view.scroll_col
        || view.cursor.1 < char_idx
    {
        return;
    }
    let disp_x = view.cursor.1 - view.scroll_col;
    if disp_x >= layout.body_w {
        return;
    }
    buf[(layout.body_x + disp_x as u16, cell_y)]
        .set_char(' ')
        .set_style(Style::default().add_modifier(Modifier::REVERSED));
}

fn paint_linewise_trailing(
    buf: &mut Buffer,
    view: &EditorView,
    layout: &BodyLayout,
    row_idx: usize,
    cell_y: u16,
    char_idx: usize,
    selection: &SelectionFrame,
) {
    let line_len = view.lines[row_idx].len();
    let mut col_abs = line_len.max(char_idx);
    let scroll_col = view.scroll_col;
    while selection.contains(row_idx, col_abs) {
        if col_abs >= scroll_col {
            let disp_x = col_abs - scroll_col;
            if disp_x >= layout.body_w {
                break;
            }
            buf[(layout.body_x + disp_x as u16, cell_y)]
                .set_char(' ')
                .set_style(Style::default().bg(SELECTION_BG));
        }
        col_abs += 1;
        if col_abs - line_len > layout.body_w + scroll_col {
            break;
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
