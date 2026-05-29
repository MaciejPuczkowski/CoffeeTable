use super::EditorView;
use super::types::EditorMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl EditorView {
    pub(super) fn normal_key(&mut self, key: KeyEvent) {
        let prev_g = std::mem::replace(&mut self.pending_g, false);
        let prev_d = std::mem::replace(&mut self.pending_d, false);
        let prev_y = std::mem::replace(&mut self.pending_y, false);
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {}
            (KeyCode::Backspace, _) => {
                self.request_focus_tree = true;
            }
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Char('l'), _) | (KeyCode::Right, _) => self.move_right(),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Char('0'), _) | (KeyCode::Home, _) => self.jump_line_start(),
            (KeyCode::Char('^'), _) => self.jump_first_non_ws(),
            (KeyCode::Char('$'), _) | (KeyCode::End, _) => self.jump_line_end(),
            (KeyCode::Char('g'), m) if !m.contains(KeyModifiers::CONTROL) => {
                if prev_g {
                    self.cursor = (0, 0);
                    self.preferred_col = 0;
                } else {
                    self.pending_g = true;
                }
            }
            (KeyCode::Char('G'), _) => self.jump_last_line(),
            (KeyCode::Char('w'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.motion_word_forward()
            }
            (KeyCode::Char('b'), _) => self.motion_word_back(),
            (KeyCode::Char('i'), _) => self.mode = EditorMode::Insert,
            (KeyCode::Char('I'), _) => self.enter_insert_at_first_non_ws(),
            (KeyCode::Char('a'), _) => self.enter_insert_after_cursor(),
            (KeyCode::Char('A'), _) => self.enter_insert_at_line_end(),
            (KeyCode::Char('o'), _) => self.open_line_below(),
            (KeyCode::Char('O'), _) => self.open_line_above(),
            (KeyCode::Char('x'), _) => self.delete_char_at_cursor(),
            (KeyCode::Char('X'), _) => self.delete_char_before_cursor(),
            (KeyCode::Char('d'), _) => {
                if prev_d {
                    self.delete_current_line();
                } else {
                    self.pending_d = true;
                }
            }
            (KeyCode::Char('D'), _) => self.delete_to_end_of_line(),
            (KeyCode::Char('y'), _) => {
                if prev_y {
                    self.yank_line();
                } else {
                    self.pending_y = true;
                }
            }
            (KeyCode::Char('p'), _) => self.paste_after(),
            (KeyCode::Char('P'), _) => self.paste_before(),
            (KeyCode::Char('v'), _) => self.enter_visual(EditorMode::Visual),
            (KeyCode::Char('V'), _) => self.enter_visual(EditorMode::VisualLine),
            (KeyCode::Char('u'), m) if !m.contains(KeyModifiers::CONTROL) => self.undo(),
            (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => self.redo(),
            (KeyCode::Char('/'), _) => self.enter_search(),
            (KeyCode::Char('n'), m) if !m.contains(KeyModifiers::CONTROL) => {
                self.repeat_search(true)
            }
            (KeyCode::Char('N'), _) => self.repeat_search(false),
            (KeyCode::Enter, _) => self.jump_next_line_indent(),
            _ => {}
        }
    }

    pub(super) fn insert_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.leave_insert(),
            (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Right, _) => self.insert_right(),
            (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Backspace, _) => self.backspace(),
            (KeyCode::Delete, _) => self.delete_char_at_cursor(),
            (KeyCode::Enter, _) => self.split_line_at_cursor(),
            (KeyCode::Tab, _) => self.insert_str("    "),
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => self.insert_char(c),
            _ => {}
        }
    }

    pub(super) fn visual_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.leave_visual(),
            (KeyCode::Backspace, _) => {
                self.request_focus_tree = true;
                self.leave_visual();
            }
            (KeyCode::Char('v'), _) => self.toggle_visual(EditorMode::Visual),
            (KeyCode::Char('V'), _) => self.toggle_visual(EditorMode::VisualLine),
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Char('l'), _) | (KeyCode::Right, _) => self.move_right(),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Char('0'), _) | (KeyCode::Home, _) => self.jump_line_start(),
            (KeyCode::Char('^'), _) => self.jump_first_non_ws(),
            (KeyCode::Char('$'), _) | (KeyCode::End, _) => self.jump_line_end(),
            (KeyCode::Char('w'), _) => self.motion_word_forward(),
            (KeyCode::Char('b'), _) => self.motion_word_back(),
            (KeyCode::Char('G'), _) => self.jump_last_line(),
            (KeyCode::Char('g'), _) => self.visual_gg(),
            (KeyCode::Char('y'), _) => self.yank_selection(),
            (KeyCode::Char('d'), _) | (KeyCode::Char('x'), _) => self.delete_selection(),
            _ => {}
        }
    }

    pub(super) fn readonly_key(&mut self, key: KeyEvent) {
        if self.mode == EditorMode::Search {
            self.search_key(key);
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {}
            (KeyCode::Backspace, _) => self.request_focus_tree = true,
            (KeyCode::Char('h'), _) | (KeyCode::Left, _) => self.move_left(),
            (KeyCode::Char('l'), _) | (KeyCode::Right, _) => self.move_right(),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.move_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.move_up(),
            (KeyCode::Char('0'), _) | (KeyCode::Home, _) => self.jump_line_start(),
            (KeyCode::Char('^'), _) => self.jump_first_non_ws(),
            (KeyCode::Char('$'), _) | (KeyCode::End, _) => self.jump_line_end(),
            (KeyCode::Char('w'), _) => self.motion_word_forward(),
            (KeyCode::Char('b'), _) => self.motion_word_back(),
            (KeyCode::Char('G'), _) => self.jump_last_line(),
            (KeyCode::Char('g'), _) => self.visual_gg(),
            (KeyCode::Char('/'), _) => self.enter_search(),
            (KeyCode::Char('n'), _) => self.repeat_search(true),
            (KeyCode::Char('N'), _) => self.repeat_search(false),
            _ => {}
        }
    }

    pub(super) fn search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.mode = EditorMode::Normal;
                self.search.clear();
            }
            (KeyCode::Enter, _) => self.run_search(),
            (KeyCode::Backspace, _) => {
                if self.search.pop().is_none() {
                    self.mode = EditorMode::Normal;
                }
            }
            (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) => self.search.push(c),
            _ => {}
        }
    }

    fn enter_insert_at_first_non_ws(&mut self) {
        self.jump_first_non_ws();
        self.mode = EditorMode::Insert;
    }

    fn enter_insert_after_cursor(&mut self) {
        let len = self.line_len(self.cursor.0);
        if self.cursor.1 < len {
            self.cursor.1 += 1;
        }
        self.mode = EditorMode::Insert;
    }

    fn enter_insert_at_line_end(&mut self) {
        self.cursor.1 = self.line_len(self.cursor.0);
        self.mode = EditorMode::Insert;
    }

    fn open_line_below(&mut self) {
        self.snapshot();
        let row = self.cursor.0 + 1;
        self.lines.insert(row, Vec::new());
        self.cursor = (row, 0);
        self.modified = true;
        self.mode = EditorMode::Insert;
    }

    fn open_line_above(&mut self) {
        self.snapshot();
        let row = self.cursor.0;
        self.lines.insert(row, Vec::new());
        self.cursor = (row, 0);
        self.modified = true;
        self.mode = EditorMode::Insert;
    }

    fn enter_visual(&mut self, mode: EditorMode) {
        self.anchor = Some(self.cursor);
        self.mode = mode;
    }

    fn enter_search(&mut self) {
        self.mode = EditorMode::Search;
        self.search.clear();
    }

    fn leave_insert(&mut self) {
        self.mode = EditorMode::Normal;
        if self.cursor.1 > 0 {
            self.cursor.1 -= 1;
        }
        self.preferred_col = self.cursor.1;
        if self.modified {
            if let Err(e) = self.save() {
                self.status = format!("Autosave failed: {}", e);
            }
        }
    }

    fn insert_right(&mut self) {
        let len = self.line_len(self.cursor.0);
        if self.cursor.1 < len {
            self.cursor.1 += 1;
        }
    }

    fn leave_visual(&mut self) {
        self.mode = EditorMode::Normal;
        self.anchor = None;
    }

    fn toggle_visual(&mut self, target: EditorMode) {
        if self.mode == target {
            self.leave_visual();
        } else {
            self.mode = target;
        }
    }

    fn visual_gg(&mut self) {
        if self.pending_g {
            self.cursor = (0, 0);
            self.preferred_col = 0;
            self.pending_g = false;
        } else {
            self.pending_g = true;
        }
    }

    fn run_search(&mut self) {
        let pattern = self.search.clone();
        self.mode = EditorMode::Normal;
        if !pattern.is_empty() {
            self.last_search = Some(pattern);
            self.repeat_search(true);
        }
    }
}
