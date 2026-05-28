use super::EditorView;
use super::types::EditorMode;

impl EditorView {
    pub fn mouse_press(&mut self, col: u16, row: u16) {
        let Some(pos) = self.mouse_to_cursor(col, row) else { return };
        self.cursor = pos;
        self.preferred_col = pos.1;
        self.anchor = Some(pos);
        self.mode = EditorMode::Visual;
    }

    pub fn mouse_drag(&mut self, col: u16, row: u16) {
        let Some(pos) = self.mouse_to_cursor(col, row) else { return };
        self.cursor = pos;
        self.preferred_col = pos.1;
    }

    pub fn mouse_release(&mut self) {
        if self.mode != EditorMode::Visual {
            return;
        }
        let anchor = self.anchor.unwrap_or(self.cursor);
        if anchor == self.cursor {
            self.mode = EditorMode::Normal;
            self.anchor = None;
        } else {
            self.yank_selection();
        }
    }

    pub fn mouse_scroll(&mut self, delta: i32) {
        let steps = delta.unsigned_abs() as usize;
        if delta > 0 {
            let max = self.lines.len().saturating_sub(1);
            self.scroll_row = (self.scroll_row + steps).min(max);
        } else {
            self.scroll_row = self.scroll_row.saturating_sub(steps);
        }
    }

    pub fn mouse_scroll_horizontal(&mut self, delta: i32) {
        let steps = delta.unsigned_abs() as usize;
        if delta > 0 {
            self.scroll_col = self.scroll_col.saturating_add(steps);
        } else {
            self.scroll_col = self.scroll_col.saturating_sub(steps);
        }
    }

    fn mouse_to_cursor(&self, col: u16, row: u16) -> Option<(usize, usize)> {
        let area = self.last_render_area?;
        if col <= area.x || row <= area.y {
            return None;
        }
        if col >= area.x + area.width.saturating_sub(1) {
            return None;
        }
        if row >= area.y + area.height.saturating_sub(1) {
            return None;
        }
        let dy = (row - area.y - 1) as usize;
        let dx = col - area.x;
        let body_start_x = 1 + self.gutter_width + 1;
        if dx < body_start_x {
            return None;
        }
        let col_in_body = (dx - body_start_x) as usize + self.scroll_col;
        let line_idx = self.scroll_row + dy;
        if line_idx >= self.lines.len() {
            return None;
        }
        let line_len = self.line_len(line_idx);
        Some((line_idx, col_in_body.min(line_len)))
    }
}
