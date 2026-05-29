use super::EditorView;
use super::types::{EditorMode, GitView};
use crate::git::GitStatus;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};

const SELECTION_BG: Color = Color::Rgb(33, 66, 131);
const DIFF_ADDED_BG: Color = Color::Rgb(20, 55, 25);
const DIFF_REMOVED_BG: Color = Color::Rgb(70, 25, 25);
const DIFF_HUNK_BG: Color = Color::Rgb(45, 45, 90);

pub struct EditorWidget<'a> {
    pub view: &'a mut EditorView,
    pub area_title: String,
    pub git_status: Option<GitStatus>,
}

impl<'a> Widget for EditorWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.view.last_render_area = Some(area);
        let (title_line, pills) =
            build_title_with_pills(self.view, &self.area_title, self.git_status, area);
        self.view.pill_working = pills[0];
        self.view.pill_head = pills[1];
        self.view.pill_diff = pills[2];
        let inner = render_block_with_title(self.view, title_line, area, buf);
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

fn render_block_with_title(
    view: &EditorView,
    title_line: Line<'static>,
    area: Rect,
    buf: &mut Buffer,
) -> Rect {
    let border_style = if view.focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title_line);
    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

fn build_title_with_pills(
    view: &EditorView,
    title: &str,
    git_status: Option<GitStatus>,
    area: Rect,
) -> (Line<'static>, [Option<Rect>; 3]) {
    let mut tracker = TitleTracker::new(area.x + 1, area.y);
    tracker.push(Span::raw(" "));
    tracker.push(Span::styled(
        title.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    if view.modified && matches!(view.git_view, GitView::Working) {
        tracker.push(Span::styled(
            " [+]".to_string(),
            Style::default().fg(Color::Red),
        ));
    }
    let pills = append_status_badge(&mut tracker, view, git_status);
    tracker.push(Span::raw(" "));
    (Line::from(tracker.spans), pills)
}

struct TitleTracker {
    spans: Vec<Span<'static>>,
    x: u16,
    y: u16,
}

impl TitleTracker {
    fn new(x: u16, y: u16) -> Self {
        Self { spans: Vec::new(), x, y }
    }

    fn push(&mut self, span: Span<'static>) {
        self.x += span.content.chars().count() as u16;
        self.spans.push(span);
    }

    fn record(&mut self, span: Span<'static>) -> Rect {
        let w = span.content.chars().count() as u16;
        let rect = Rect::new(self.x, self.y, w, 1);
        self.x += w;
        self.spans.push(span);
        rect
    }
}

fn append_status_badge(
    tracker: &mut TitleTracker,
    view: &EditorView,
    git_status: Option<GitStatus>,
) -> [Option<Rect>; 3] {
    let in_alt_view = !matches!(view.git_view, GitView::Working);
    if !in_alt_view && matches!(git_status, Some(GitStatus::Untracked)) {
        tracker.push(Span::raw("   "));
        tracker.push(Span::styled(
            " untracked ".to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        return [None; 3];
    }
    let show_pills = in_alt_view
        || matches!(git_status, Some(GitStatus::Modified | GitStatus::Staged));
    if !show_pills {
        return [None; 3];
    }
    tracker.push(Span::raw("   "));
    let working_rect = tracker.record(view_pill('w', "Working", matches!(view.git_view, GitView::Working)));
    let head_rect = tracker.record(view_pill('h', "HEAD", matches!(view.git_view, GitView::Head)));
    let diff_rect = tracker.record(view_pill('d', "Diff", matches!(view.git_view, GitView::Diff)));
    [Some(working_rect), Some(head_rect), Some(diff_rect)]
}

fn view_pill(letter: char, label: &str, active: bool) -> Span<'static> {
    let text = format!(" {}·{} ", letter, label);
    if active {
        Span::styled(
            text,
            Style::default()
                .bg(Color::Yellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(text, Style::default().fg(Color::DarkGray))
    }
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
    let row_bg = diff_row_bg(view, row_idx);
    if let Some(bg) = row_bg {
        paint_row_background(buf, layout, cell_y, bg);
    }
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
                        style_for_cell(view, *style, row_idx, char_idx, selection, row_bg);
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

fn diff_row_bg(view: &EditorView, row_idx: usize) -> Option<Color> {
    if !matches!(view.git_view, GitView::Diff) {
        return None;
    }
    let line = view.lines.get(row_idx)?;
    let first = line.first().copied()?;
    match first {
        '+' => Some(DIFF_ADDED_BG),
        '-' => Some(DIFF_REMOVED_BG),
        '─' | '@' => Some(DIFF_HUNK_BG),
        _ => None,
    }
}

fn paint_row_background(buf: &mut Buffer, layout: &BodyLayout, cell_y: u16, bg: Color) {
    let style = Style::default().bg(bg);
    for dx in 0..layout.body_w {
        let x = layout.body_x + dx as u16;
        buf[(x, cell_y)].set_char(' ').set_style(style);
    }
}

fn style_for_cell(
    view: &EditorView,
    base: Style,
    row: usize,
    col: usize,
    selection: &SelectionFrame,
    row_bg: Option<Color>,
) -> Style {
    let mut style = base;
    if let Some(bg) = row_bg {
        style = style.bg(bg);
    }
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
    let text = match view.mode {
        EditorMode::Search => view.search.as_str(),
        _ => return,
    };
    let style = Style::default().fg(Color::Yellow);
    let line = format!("/{}", text);
    for (i, ch) in line.chars().enumerate() {
        if (i as u16) >= area.width {
            break;
        }
        buf[(area.x + i as u16, area.y)]
            .set_char(ch)
            .set_style(style);
    }
}
