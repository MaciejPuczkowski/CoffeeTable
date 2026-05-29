use super::EditorView;
use super::types::{EditorMode, Snapshot, YankRegister};
use crate::clipboard;

impl EditorView {
    pub(super) fn insert_char(&mut self, c: char) {
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        if self.cursor.1 > line.len() {
            self.cursor.1 = line.len();
        }
        line.insert(self.cursor.1, c);
        self.cursor.1 += 1;
        self.preferred_col = self.cursor.1;
        self.modified = true;
    }

    pub(super) fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    pub(super) fn backspace(&mut self) {
        self.snapshot();
        if self.cursor.1 > 0 {
            let line = &mut self.lines[self.cursor.0];
            line.remove(self.cursor.1 - 1);
            self.cursor.1 -= 1;
        } else if self.cursor.0 > 0 {
            let removed = self.lines.remove(self.cursor.0);
            self.cursor.0 -= 1;
            self.cursor.1 = self.lines[self.cursor.0].len();
            self.lines[self.cursor.0].extend(removed);
        } else {
            return;
        }
        self.preferred_col = self.cursor.1;
        self.modified = true;
    }

    pub(super) fn split_line_at_cursor(&mut self) {
        self.snapshot();
        let row = self.cursor.0;
        let col = self.cursor.1;
        let line = &mut self.lines[row];
        let tail: Vec<char> = line.drain(col..).collect();
        self.lines.insert(row + 1, tail);
        self.cursor = (row + 1, 0);
        self.preferred_col = 0;
        self.modified = true;
    }

    pub(super) fn delete_char_at_cursor(&mut self) {
        let line_len = self.line_len(self.cursor.0);
        if line_len == 0 || self.cursor.1 >= line_len {
            return;
        }
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        let removed = line.remove(self.cursor.1);
        self.yank = YankRegister {
            text: removed.to_string(),
            linewise: false,
        };
        clipboard::copy(&self.yank.text);
        if self.cursor.1 >= line.len() && !line.is_empty() {
            self.cursor.1 = line.len() - 1;
        }
        self.modified = true;
    }

    pub(super) fn delete_char_before_cursor(&mut self) {
        if self.cursor.1 == 0 {
            return;
        }
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        line.remove(self.cursor.1 - 1);
        self.cursor.1 -= 1;
        self.modified = true;
    }

    pub(super) fn delete_current_line(&mut self) {
        self.snapshot();
        let text: String = self.lines[self.cursor.0].iter().collect();
        self.yank = YankRegister {
            text: text.clone(),
            linewise: true,
        };
        clipboard::copy(&format!("{}\n", text));
        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(self.cursor.0);
            if self.cursor.0 >= self.lines.len() {
                self.cursor.0 = self.lines.len() - 1;
            }
        }
        self.cursor.1 = 0;
        self.preferred_col = 0;
        self.modified = true;
    }

    pub(super) fn delete_to_end_of_line(&mut self) {
        self.snapshot();
        let line = &mut self.lines[self.cursor.0];
        let removed: String = line.drain(self.cursor.1..).collect();
        self.yank = YankRegister {
            text: removed.clone(),
            linewise: false,
        };
        clipboard::copy(&removed);
        if self.cursor.1 > 0 && self.cursor.1 >= line.len() {
            self.cursor.1 = line.len().saturating_sub(1);
        }
        self.modified = true;
    }

    pub fn copy_current(&mut self) {
        match self.mode {
            EditorMode::Visual | EditorMode::VisualLine => self.yank_selection(),
            _ => self.yank_line(),
        }
    }

    pub(super) fn yank_line(&mut self) {
        let text: String = self.lines[self.cursor.0].iter().collect();
        self.yank = YankRegister {
            text: text.clone(),
            linewise: true,
        };
        clipboard::copy(&format!("{}\n", text));
        self.status = "Yanked 1 line".into();
    }

    pub(super) fn yank_selection(&mut self) {
        let (start, end, linewise) = self.selection_range();
        let text = self.collect_range(start, end, linewise);
        self.yank = YankRegister {
            text: text.clone(),
            linewise,
        };
        clipboard::copy(&text);
        self.status = "Yanked".into();
        self.mode = EditorMode::Normal;
        self.anchor = None;
    }

    pub(super) fn delete_selection(&mut self) {
        let (start, end, linewise) = self.selection_range();
        let text = self.collect_range(start, end, linewise);
        self.snapshot();
        self.remove_range(start, end, linewise);
        self.yank = YankRegister { text, linewise };
        clipboard::copy(&self.yank.text);
        self.modified = true;
        self.mode = EditorMode::Normal;
        self.anchor = None;
    }

    pub(super) fn paste_after(&mut self) {
        if self.yank.text.is_empty() {
            return;
        }
        self.snapshot();
        if self.yank.linewise {
            let chunks: Vec<Vec<char>> = self
                .yank
                .text
                .trim_end_matches('\n')
                .split('\n')
                .map(|s| s.chars().collect())
                .collect();
            let row = self.cursor.0 + 1;
            for (i, line) in chunks.into_iter().enumerate() {
                self.lines.insert(row + i, line);
            }
            self.cursor = (row, 0);
            self.preferred_col = 0;
        } else {
            let line_len = self.line_len(self.cursor.0);
            let mut col = self.cursor.1;
            if line_len > 0 {
                col = (col + 1).min(line_len);
            }
            let text = self.yank.text.clone();
            self.insert_text_at(col, &text);
            self.cursor.1 = col + text.chars().count().saturating_sub(1);
        }
        self.modified = true;
    }

    pub(super) fn paste_before(&mut self) {
        if self.yank.text.is_empty() {
            return;
        }
        self.snapshot();
        if self.yank.linewise {
            let chunks: Vec<Vec<char>> = self
                .yank
                .text
                .trim_end_matches('\n')
                .split('\n')
                .map(|s| s.chars().collect())
                .collect();
            let row = self.cursor.0;
            for (i, line) in chunks.into_iter().enumerate() {
                self.lines.insert(row + i, line);
            }
            self.cursor = (row, 0);
            self.preferred_col = 0;
        } else {
            let col = self.cursor.1;
            let text = self.yank.text.clone();
            self.insert_text_at(col, &text);
        }
        self.modified = true;
    }

    fn insert_text_at(&mut self, col: usize, text: &str) {
        let row = self.cursor.0;
        let line = &mut self.lines[row];
        let col = col.min(line.len());
        for (i, c) in text.chars().enumerate() {
            line.insert(col + i, c);
        }
    }

    pub(super) fn selection_range(&self) -> ((usize, usize), (usize, usize), bool) {
        let a = self.anchor.unwrap_or(self.cursor);
        let b = self.cursor;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let linewise = self.mode == EditorMode::VisualLine;
        (start, end, linewise)
    }

    pub(super) fn collect_range(
        &self,
        start: (usize, usize),
        end: (usize, usize),
        linewise: bool,
    ) -> String {
        if linewise {
            let mut out = String::new();
            for r in start.0..=end.0 {
                let line: String = self.lines[r].iter().collect();
                out.push_str(&line);
                out.push('\n');
            }
            return out;
        }
        if start.0 == end.0 {
            let line = &self.lines[start.0];
            let s = start.1.min(line.len());
            let e = (end.1 + 1).min(line.len());
            return line[s..e].iter().collect();
        }
        let mut out = String::new();
        let first = &self.lines[start.0];
        let s = start.1.min(first.len());
        out.extend(first[s..].iter());
        out.push('\n');
        for r in start.0 + 1..end.0 {
            out.push_str(&self.lines[r].iter().collect::<String>());
            out.push('\n');
        }
        let last = &self.lines[end.0];
        let e = (end.1 + 1).min(last.len());
        out.extend(last[..e].iter());
        out
    }

    pub(super) fn remove_range(
        &mut self,
        start: (usize, usize),
        end: (usize, usize),
        linewise: bool,
    ) {
        if linewise {
            let count = end.0 - start.0 + 1;
            if count >= self.lines.len() {
                self.lines.clear();
                self.lines.push(Vec::new());
            } else {
                self.lines.drain(start.0..start.0 + count);
            }
            if start.0 >= self.lines.len() {
                self.cursor = (self.lines.len() - 1, 0);
            } else {
                self.cursor = (start.0, 0);
            }
            self.preferred_col = 0;
            return;
        }
        if start.0 == end.0 {
            let line = &mut self.lines[start.0];
            let s = start.1.min(line.len());
            let e = (end.1 + 1).min(line.len());
            line.drain(s..e);
            self.cursor = (start.0, s);
        } else {
            let first = &mut self.lines[start.0];
            let s = start.1.min(first.len());
            first.truncate(s);
            let last = self.lines.remove(end.0);
            let last_end = (end.1 + 1).min(last.len());
            let tail: Vec<char> = last[last_end..].to_vec();
            if end.0 - start.0 > 1 {
                self.lines.drain(start.0 + 1..end.0);
            }
            self.lines[start.0].extend(tail);
            self.cursor = (start.0, s);
        }
        self.preferred_col = self.cursor.1;
    }

    pub(super) fn snapshot(&mut self) {
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(super) fn undo(&mut self) {
        if let Some(snap) = self.undo.pop() {
            self.redo.push(Snapshot {
                lines: self.lines.clone(),
                cursor: self.cursor,
            });
            self.lines = snap.lines;
            self.cursor = snap.cursor;
            self.modified = true;
            self.status = "1 change undone".into();
        } else {
            self.status = "Already at oldest change".into();
        }
    }

    pub(super) fn redo(&mut self) {
        if let Some(snap) = self.redo.pop() {
            self.undo.push(Snapshot {
                lines: self.lines.clone(),
                cursor: self.cursor,
            });
            self.lines = snap.lines;
            self.cursor = snap.cursor;
            self.modified = true;
            self.status = "1 change redone".into();
        } else {
            self.status = "Already at newest change".into();
        }
    }

    pub(super) fn repeat_search(&mut self, forward: bool) {
        let Some(pattern) = self.last_search.clone() else {
            self.status = "No previous search".into();
            return;
        };
        if pattern.is_empty() {
            return;
        }
        let hits = self.find_all(&pattern);
        if hits.is_empty() {
            self.status = format!("Pattern not found: {}", pattern);
            return;
        }
        let cur = self.cursor;
        let next = if forward {
            hits.iter()
                .find(|(r, c)| (*r, *c) > cur)
                .copied()
                .unwrap_or(hits[0])
        } else {
            hits.iter()
                .rev()
                .find(|(r, c)| (*r, *c) < cur)
                .copied()
                .unwrap_or(*hits.last().unwrap())
        };
        self.cursor = next;
        self.preferred_col = next.1;
    }

    fn find_all(&self, pattern: &str) -> Vec<(usize, usize)> {
        let mut hits = Vec::new();
        let step = pattern
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
        for (r, line) in self.lines.iter().enumerate() {
            let s: String = line.iter().collect();
            let mut start = 0;
            while let Some(found) = s[start..].find(pattern) {
                let abs = start + found;
                let col = s[..abs].chars().count();
                hits.push((r, col));
                start = abs + step;
                if start > s.len() {
                    break;
                }
            }
        }
        hits
    }
}
