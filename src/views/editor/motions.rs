use super::EditorView;
use super::types::EditorMode;

impl EditorView {
    pub(super) fn move_left(&mut self) {
        if self.cursor.1 > 0 {
            self.cursor.1 -= 1;
            self.preferred_col = self.cursor.1;
        }
    }

    pub(super) fn move_right(&mut self) {
        let len = self.line_len(self.cursor.0);
        let max = if self.mode == EditorMode::Insert {
            len
        } else {
            len.saturating_sub(1)
        };
        if self.cursor.1 < max {
            self.cursor.1 += 1;
            self.preferred_col = self.cursor.1;
        }
    }

    pub(super) fn move_down(&mut self) {
        if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.cursor.1 = self.preferred_col.min(self.line_len(self.cursor.0));
        }
    }

    pub(super) fn move_up(&mut self) {
        if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.cursor.1 = self.preferred_col.min(self.line_len(self.cursor.0));
        }
    }

    pub(super) fn jump_line_start(&mut self) {
        self.cursor.1 = 0;
        self.preferred_col = 0;
    }

    pub(super) fn jump_line_end(&mut self) {
        let len = self.line_len(self.cursor.0);
        self.cursor.1 = len.saturating_sub(1);
        self.preferred_col = usize::MAX;
    }

    pub(super) fn jump_first_non_ws(&mut self) {
        let line = &self.lines[self.cursor.0];
        let col = line.iter().position(|c| !c.is_whitespace()).unwrap_or(0);
        self.cursor.1 = col;
        self.preferred_col = col;
    }

    pub(super) fn jump_last_line(&mut self) {
        self.cursor.0 = self.lines.len().saturating_sub(1);
        self.cursor.1 = self.preferred_col.min(self.line_len(self.cursor.0));
    }

    pub(super) fn jump_next_line_indent(&mut self) {
        if self.cursor.0 + 1 < self.lines.len() {
            self.cursor.0 += 1;
            self.jump_first_non_ws();
        }
    }

    pub(super) fn motion_word_forward(&mut self) {
        let (mut r, mut c) = self.cursor;
        loop {
            let line = &self.lines[r];
            if c < line.len() {
                let cur = line[c];
                if is_word(cur) {
                    while c < line.len() && is_word(line[c]) {
                        c += 1;
                    }
                } else if !cur.is_whitespace() {
                    while c < line.len() && !is_word(line[c]) && !line[c].is_whitespace() {
                        c += 1;
                    }
                }
                while c < line.len() && line[c].is_whitespace() {
                    c += 1;
                }
                if c < line.len() {
                    break;
                }
            }
            if r + 1 >= self.lines.len() {
                c = self.line_len(r).saturating_sub(1);
                break;
            }
            r += 1;
            c = 0;
            let line = &self.lines[r];
            if let Some(p) = line.iter().position(|ch| !ch.is_whitespace()) {
                c = p;
                break;
            }
        }
        self.cursor = (r, c);
        self.preferred_col = c;
    }

    pub(super) fn motion_word_back(&mut self) {
        let (mut r, mut c) = self.cursor;
        loop {
            if c == 0 {
                if r == 0 {
                    break;
                }
                r -= 1;
                c = self.line_len(r);
            }
            let line = &self.lines[r];
            while c > 0
                && line
                    .get(c.saturating_sub(1))
                    .map(|ch| ch.is_whitespace())
                    .unwrap_or(false)
            {
                c -= 1;
            }
            if c == 0 {
                continue;
            }
            let kind = is_word(line[c - 1]);
            while c > 0 && is_word(line[c - 1]) == kind && !line[c - 1].is_whitespace() {
                c -= 1;
            }
            break;
        }
        self.cursor = (r, c);
        self.preferred_col = c;
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
