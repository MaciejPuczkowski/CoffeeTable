use crate::project::{CommentKind, CommentStatus, Feature, FeatureStatus, StepStatus};
use crate::views::editor::EditorView;
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFocus {
    Title,
    Status,
    Description,
    Step(usize),
    NewStep,
    Comment(usize),
    NewComment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormPage {
    Details,
    Comments,
}


#[derive(Debug, Clone)]
pub struct StepDraft {
    pub id: Option<i64>,
    pub summary: String,
    pub status: StepStatus,
    pub deleted: bool,
}

#[derive(Debug, Clone)]
pub struct CommentDraft {
    pub id: Option<i64>,
    pub message: String,
    pub status: CommentStatus,
    pub kind: CommentKind,
    pub deleted: bool,
}

pub struct FeatureForm {
    pub feature_id: Option<i64>,
    pub title: String,
    pub status: FeatureStatus,
    pub status_cursor: FeatureStatus,
    pub description: String,
    pub steps: Vec<StepDraft>,
    pub comments: Vec<CommentDraft>,
    pub new_step_buf: String,
    pub new_comment_buf: String,
    pub page: FormPage,
    pub focus: FormFocus,
    pub cursor: usize,
    pub editor: Option<EditorView>,
    pub dirty: bool,
}

impl FeatureForm {
    pub fn for_new() -> Self {
        Self {
            feature_id: None,
            title: String::new(),
            status: FeatureStatus::Idea,
            status_cursor: FeatureStatus::Idea,
            description: String::new(),
            steps: Vec::new(),
            comments: Vec::new(),
            new_step_buf: String::new(),
            new_comment_buf: String::new(),
            page: FormPage::Details,
            focus: FormFocus::Title,
            cursor: 0,
            editor: None,
            dirty: false,
        }
    }

    pub fn for_existing(feature: &Feature) -> Self {
        Self {
            feature_id: Some(feature.id),
            title: feature.title.clone(),
            status: feature.status,
            status_cursor: feature.status,
            description: feature.description.clone(),
            steps: feature
                .steps
                .iter()
                .map(|s| StepDraft {
                    id: Some(s.id),
                    summary: s.summary.clone(),
                    status: s.status,
                    deleted: false,
                })
                .collect(),
            comments: feature
                .comments
                .iter()
                .map(|c| CommentDraft {
                    id: Some(c.id),
                    message: c.message.clone(),
                    status: c.status,
                    kind: c.kind,
                    deleted: false,
                })
                .collect(),
            new_step_buf: String::new(),
            new_comment_buf: String::new(),
            page: FormPage::Details,
            focus: FormFocus::Title,
            cursor: feature.title.chars().count(),
            editor: None,
            dirty: false,
        }
    }

    pub fn switch_to_details(&mut self) {
        if matches!(self.page, FormPage::Details) {
            return;
        }
        self.commit_pending_buffers_on_leave();
        self.editor = None;
        self.page = FormPage::Details;
        self.focus = FormFocus::Title;
        self.reset_cursor_to_end();
        self.sync_status_cursor();
    }

    pub fn switch_to_comments(&mut self) {
        if matches!(self.page, FormPage::Comments) {
            return;
        }
        self.commit_pending_buffers_on_leave();
        self.editor = None;
        self.page = FormPage::Comments;
        self.focus = match self.first_visible_comment() {
            Some(i) => FormFocus::Comment(i),
            None => FormFocus::NewComment,
        };
        self.reset_cursor_to_end();
    }

    pub fn toggle_page(&mut self) {
        match self.page {
            FormPage::Details => self.switch_to_comments(),
            FormPage::Comments => self.switch_to_details(),
        }
    }

    pub fn click_focus(&mut self, target: FormFocus) {
        if self.focus == target {
            return;
        }
        self.commit_pending_buffers_on_leave();
        self.editor = None;
        self.focus = target;
        self.reset_cursor_to_end();
        self.sync_status_cursor();
    }

    pub fn set_status(&mut self, status: FeatureStatus) {
        self.status_cursor = status;
        if self.status != status {
            self.status = status;
            self.dirty = true;
        }
    }

    pub fn description_editing(&self) -> bool {
        self.editor.is_some() && matches!(self.focus, FormFocus::Description)
    }

    pub fn current_text(&self) -> Option<&str> {
        match self.focus {
            FormFocus::Title => Some(&self.title),
            FormFocus::Step(i) => self.steps.get(i).map(|s| s.summary.as_str()),
            FormFocus::NewStep => Some(self.new_step_buf.as_str()),
            FormFocus::Comment(i) => self.comments.get(i).map(|c| c.message.as_str()),
            FormFocus::NewComment => Some(self.new_comment_buf.as_str()),
            _ => None,
        }
    }

    fn current_text_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            FormFocus::Title => Some(&mut self.title),
            FormFocus::Step(i) => self.steps.get_mut(i).map(|s| &mut s.summary),
            FormFocus::NewStep => Some(&mut self.new_step_buf),
            FormFocus::Comment(i) => self.comments.get_mut(i).map(|c| &mut c.message),
            FormFocus::NewComment => Some(&mut self.new_comment_buf),
            _ => None,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        let cursor = self.cursor;
        if let Some(text) = self.current_text_mut() {
            let byte_pos = char_to_byte(text, cursor);
            text.insert(byte_pos, ch);
        }
        self.cursor = cursor + 1;
        self.dirty = true;
    }

    pub fn delete_char_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let cursor = self.cursor;
        if let Some(text) = self.current_text_mut() {
            let start = char_to_byte(text, cursor - 1);
            let end = char_to_byte(text, cursor);
            text.replace_range(start..end, "");
        }
        self.cursor = cursor - 1;
        self.dirty = true;
    }

    pub fn move_caret_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_caret_right(&mut self) {
        let len = self.current_text().map(|t| t.chars().count()).unwrap_or(0);
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    pub fn move_caret_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_caret_end(&mut self) {
        self.cursor = self.current_text().map(|t| t.chars().count()).unwrap_or(0);
    }

    pub fn commit_new_step(&mut self) -> Option<usize> {
        let trimmed = self.new_step_buf.trim().to_string();
        self.new_step_buf.clear();
        if matches!(self.focus, FormFocus::NewStep) {
            self.cursor = 0;
        }
        if trimmed.is_empty() {
            return None;
        }
        self.steps.push(StepDraft {
            id: None,
            summary: trimmed,
            status: StepStatus::Todo,
            deleted: false,
        });
        self.dirty = true;
        Some(self.steps.len() - 1)
    }

    pub fn commit_new_comment(&mut self) -> Option<usize> {
        let trimmed = self.new_comment_buf.trim().to_string();
        self.new_comment_buf.clear();
        if matches!(self.focus, FormFocus::NewComment) {
            self.cursor = 0;
        }
        if trimmed.is_empty() {
            return None;
        }
        self.comments.push(CommentDraft {
            id: None,
            message: trimmed,
            status: CommentStatus::Queued,
            kind: CommentKind::Note,
            deleted: false,
        });
        self.dirty = true;
        Some(self.comments.len() - 1)
    }

    pub fn cycle_comment_kind(&mut self) {
        if let FormFocus::Comment(i) = self.focus {
            if let Some(c) = self.comments.get_mut(i) {
                c.kind = c.kind.cycle();
                self.dirty = true;
            }
        }
    }

    fn first_visible_step(&self) -> Option<usize> {
        self.steps.iter().position(|s| !s.deleted)
    }

    fn last_visible_step(&self) -> Option<usize> {
        self.steps.iter().rposition(|s| !s.deleted)
    }

    fn next_visible_step(&self, from: usize) -> Option<usize> {
        self.steps
            .iter()
            .enumerate()
            .skip(from + 1)
            .find(|(_, s)| !s.deleted)
            .map(|(i, _)| i)
    }

    fn prev_visible_step(&self, from: usize) -> Option<usize> {
        self.steps
            .iter()
            .enumerate()
            .take(from)
            .rev()
            .find(|(_, s)| !s.deleted)
            .map(|(i, _)| i)
    }

    fn first_visible_comment(&self) -> Option<usize> {
        self.comments.iter().position(|c| !c.deleted)
    }

    fn last_visible_comment(&self) -> Option<usize> {
        self.comments.iter().rposition(|c| !c.deleted)
    }

    fn next_visible_comment(&self, from: usize) -> Option<usize> {
        self.comments
            .iter()
            .enumerate()
            .skip(from + 1)
            .find(|(_, c)| !c.deleted)
            .map(|(i, _)| i)
    }

    fn prev_visible_comment(&self, from: usize) -> Option<usize> {
        self.comments
            .iter()
            .enumerate()
            .take(from)
            .rev()
            .find(|(_, c)| !c.deleted)
            .map(|(i, _)| i)
    }

    fn compute_next(&self, cur: FormFocus) -> FormFocus {
        match cur {
            FormFocus::Title => FormFocus::Status,
            FormFocus::Status => FormFocus::Description,
            FormFocus::Description => match self.first_visible_step() {
                Some(i) => FormFocus::Step(i),
                None => FormFocus::NewStep,
            },
            FormFocus::Step(i) => match self.next_visible_step(i) {
                Some(next) => FormFocus::Step(next),
                None => FormFocus::NewStep,
            },
            FormFocus::NewStep => FormFocus::Title,
            FormFocus::Comment(i) => match self.next_visible_comment(i) {
                Some(next) => FormFocus::Comment(next),
                None => FormFocus::NewComment,
            },
            FormFocus::NewComment => match self.first_visible_comment() {
                Some(i) => FormFocus::Comment(i),
                None => FormFocus::NewComment,
            },
        }
    }

    fn compute_prev(&self, cur: FormFocus) -> FormFocus {
        match cur {
            FormFocus::Title => FormFocus::NewStep,
            FormFocus::Status => FormFocus::Title,
            FormFocus::Description => FormFocus::Status,
            FormFocus::Step(i) => match self.prev_visible_step(i) {
                Some(prev) => FormFocus::Step(prev),
                None => FormFocus::Description,
            },
            FormFocus::NewStep => match self.last_visible_step() {
                Some(last) => FormFocus::Step(last),
                None => FormFocus::Description,
            },
            FormFocus::Comment(i) => match self.prev_visible_comment(i) {
                Some(prev) => FormFocus::Comment(prev),
                None => FormFocus::NewComment,
            },
            FormFocus::NewComment => match self.last_visible_comment() {
                Some(last) => FormFocus::Comment(last),
                None => FormFocus::NewComment,
            },
        }
    }

    pub fn focus_next(&mut self) {
        self.commit_pending_buffers_on_leave();
        self.editor = None;
        self.focus = self.compute_next(self.focus);
        self.reset_cursor_to_end();
        self.sync_status_cursor();
    }

    pub fn focus_prev(&mut self) {
        self.commit_pending_buffers_on_leave();
        self.editor = None;
        self.focus = self.compute_prev(self.focus);
        self.reset_cursor_to_end();
        self.sync_status_cursor();
    }

    fn commit_pending_buffers_on_leave(&mut self) {
        match self.focus {
            FormFocus::NewStep => {
                let _ = self.commit_new_step();
            }
            FormFocus::NewComment => {
                let _ = self.commit_new_comment();
            }
            FormFocus::Description => {
                self.commit_description_editor();
            }
            _ => {}
        }
    }

    fn reset_cursor_to_end(&mut self) {
        self.cursor = self.current_text().map(|t| t.chars().count()).unwrap_or(0);
    }

    fn sync_status_cursor(&mut self) {
        if matches!(self.focus, FormFocus::Status) {
            self.status_cursor = self.status;
        }
    }

    pub fn status_cursor_right(&mut self) {
        self.status_cursor = self.status_cursor.next();
    }

    pub fn status_cursor_left(&mut self) {
        self.status_cursor = self.status_cursor.prev();
    }

    pub fn apply_status_cursor(&mut self) {
        if self.status != self.status_cursor {
            self.status = self.status_cursor;
            self.dirty = true;
        }
    }

    pub fn cycle_status(&mut self) {
        self.status = self.status.next();
        self.status_cursor = self.status;
        self.dirty = true;
    }

    pub fn cycle_step_status(&mut self) {
        if let FormFocus::Step(i) = self.focus {
            if let Some(step) = self.steps.get_mut(i) {
                step.status = step.status.cycle();
                self.dirty = true;
            }
        }
    }

    pub fn delete_focused(&mut self) {
        let next_target = match self.focus {
            FormFocus::Step(i) => {
                let removed = remove_or_mark_deleted_step(&mut self.steps, i);
                if removed {
                    self.dirty = true;
                    Some(self.compute_next_after_step_removed(i))
                } else {
                    None
                }
            }
            FormFocus::Comment(i) => {
                let removed = remove_or_mark_deleted_comment(&mut self.comments, i);
                if removed {
                    self.dirty = true;
                    Some(self.compute_next_after_comment_removed(i))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(target) = next_target {
            self.focus = target;
            self.reset_cursor_to_end();
        }
    }

    fn compute_next_after_step_removed(&self, removed_idx: usize) -> FormFocus {
        if let Some(next) = self.next_visible_step(removed_idx) {
            return FormFocus::Step(next);
        }
        if let Some(prev) = self.prev_visible_step(removed_idx) {
            return FormFocus::Step(prev);
        }
        FormFocus::NewStep
    }

    fn compute_next_after_comment_removed(&self, removed_idx: usize) -> FormFocus {
        if let Some(next) = self.next_visible_comment(removed_idx) {
            return FormFocus::Comment(next);
        }
        if let Some(prev) = self.prev_visible_comment(removed_idx) {
            return FormFocus::Comment(prev);
        }
        FormFocus::NewComment
    }

    pub fn open_description_editor(&mut self) -> Result<()> {
        if !matches!(self.focus, FormFocus::Description) {
            return Ok(());
        }
        let path = std::env::temp_dir().join("coffeetable_feature_description.tmp");
        let mut editor = EditorView::from_content(path, self.description.clone())?;
        editor.mode = crate::views::editor::EditorMode::Insert;
        editor.cursor.0 = editor.lines.len().saturating_sub(1);
        editor.cursor.1 = editor.lines.last().map(|l| l.len()).unwrap_or(0);
        self.editor = Some(editor);
        Ok(())
    }

    pub fn commit_description_editor(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        let text = editor_text(&editor);
        if text != self.description {
            self.description = text;
            self.dirty = true;
        }
    }
}

fn editor_text(editor: &EditorView) -> String {
    editor
        .lines
        .iter()
        .map(|l| l.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

fn remove_or_mark_deleted_step(steps: &mut Vec<StepDraft>, i: usize) -> bool {
    let Some(step) = steps.get_mut(i) else { return false };
    if step.id.is_some() {
        step.deleted = true;
    } else {
        steps.remove(i);
    }
    true
}

fn remove_or_mark_deleted_comment(comments: &mut Vec<CommentDraft>, i: usize) -> bool {
    let Some(comment) = comments.get_mut(i) else { return false };
    if comment.id.is_some() {
        comment.deleted = true;
    } else {
        comments.remove(i);
    }
    true
}
