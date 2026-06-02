use crate::project::{Feature, ProjectMeta};
use crate::views::editor::EditorView;
use crate::views::feature_form::FeatureForm;
use ratatui::widgets::ListState;

pub fn feature_filename(feature: &Feature) -> String {
    format!("feature_{}_{}.md", feature.id, slugify(&feature.title))
}

pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

pub fn meta_section_body(meta: &ProjectMeta, section: &str) -> String {
    match section {
        "about" => render_or_empty("About", &meta.description),
        "conventions" => render_or_empty("Conventions", &meta.conventions),
        "ai_hints" => render_or_empty("AI Hints", &meta.ai_hints),
        "ai_notes" => render_or_empty("AI Notes", &meta.ai_notes),
        _ => String::new(),
    }
}

pub fn feature_markdown(feature: &Feature) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# [{}] {}\n\n",
        feature.status.label(),
        feature.title
    ));
    if !feature.description.trim().is_empty() {
        out.push_str("## Description\n\n");
        out.push_str(feature.description.trim());
        out.push_str("\n\n");
    }
    if !feature.steps.is_empty() {
        out.push_str("## Steps\n\n");
        for step in &feature.steps {
            out.push_str(&format!("- {} {}\n", step.status.glyph(), step.summary));
        }
        out.push('\n');
    }
    if !feature.comments.is_empty() {
        out.push_str("## Messages\n\n");
        for comment in &feature.comments {
            out.push_str(&format!(
                "- [{} • {}] {}\n",
                comment.kind.label(),
                comment.status.label(),
                comment.message.replace('\n', " ")
            ));
        }
        out.push('\n');
    }
    out
}

pub fn index_markdown(project_name: &str, features: &[Feature]) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} — context index\n\n", project_name));
    out.push_str("## Sections\n\n");
    out.push_str("- about.md — short project description\n");
    out.push_str("- conventions.md — coding/process conventions\n");
    out.push_str("- ai_hints.md — instructions the user wrote for AI agents\n");
    out.push_str("- ai_notes.md — running notes (you may append/replace via the CLI)\n\n");
    out.push_str("## Features\n\n");
    if features.is_empty() {
        out.push_str("_(none yet)_\n");
    } else {
        for feature in features {
            out.push_str(&format!(
                "- {} — id={} — [{}] {}\n",
                feature_filename(feature),
                feature.id,
                feature.status.label(),
                feature.title
            ));
        }
    }
    out
}

fn render_or_empty(title: &str, body: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    let trimmed = body.trim();
    if trimmed.is_empty() {
        out.push_str("_(empty)_\n");
    } else {
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSection {
    About,
    Conventions,
    AiHints,
    AiNotes,
}

impl ProjectSection {
    pub fn label(self) -> &'static str {
        match self {
            ProjectSection::About => "About",
            ProjectSection::Conventions => "Conventions",
            ProjectSection::AiHints => "AI Hints",
            ProjectSection::AiNotes => "AI Notes",
        }
    }

    pub fn all() -> &'static [ProjectSection] {
        &[
            ProjectSection::About,
            ProjectSection::Conventions,
            ProjectSection::AiHints,
            ProjectSection::AiNotes,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSelection {
    Meta(ProjectSection),
    NewFeature,
    Feature(usize),
}

pub struct ProjectViewModel {
    pub meta: ProjectMeta,
    pub features: Vec<Feature>,
    pub selection: ProjectSelection,
    pub list_state: ListState,
    pub editor: Option<EditorView>,
    pub editing_section: Option<ProjectSection>,
    pub feature_form: Option<FeatureForm>,
    pub preview_scroll: u16,
}

impl ProjectViewModel {
    pub fn new(meta: ProjectMeta, features: Vec<Feature>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            meta,
            features,
            selection: ProjectSelection::Meta(ProjectSection::About),
            list_state,
            editor: None,
            editing_section: None,
            feature_form: None,
            preview_scroll: 0,
        }
    }

    pub fn scroll_preview(&mut self, delta: i32) {
        if delta < 0 {
            let d = (-delta) as u16;
            self.preview_scroll = self.preview_scroll.saturating_sub(d);
        } else {
            self.preview_scroll = self.preview_scroll.saturating_add(delta as u16);
        }
    }

    pub fn rows(&self) -> usize {
        ProjectSection::all().len() + 1 + self.features.len()
    }

    pub fn move_down(&mut self) {
        let total = self.rows();
        if total == 0 {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0);
        if cur + 1 < total {
            self.list_state.select(Some(cur + 1));
        }
        self.sync_selection_from_list();
    }

    pub fn move_up(&mut self) {
        let cur = self.list_state.selected().unwrap_or(0);
        if cur > 0 {
            self.list_state.select(Some(cur - 1));
        }
        self.sync_selection_from_list();
    }

    pub fn jump_top(&mut self) {
        self.list_state.select(Some(0));
        self.sync_selection_from_list();
    }

    pub fn jump_bottom(&mut self) {
        let total = self.rows();
        if total > 0 {
            self.list_state.select(Some(total - 1));
        }
        self.sync_selection_from_list();
    }

    pub fn sync_selection_from_list(&mut self) {
        let idx = self.list_state.selected().unwrap_or(0);
        let sections = ProjectSection::all();
        let prev = self.selection;
        if idx < sections.len() {
            self.selection = ProjectSelection::Meta(sections[idx]);
        } else if idx == sections.len() {
            self.selection = ProjectSelection::NewFeature;
        } else {
            let feat_idx = idx - sections.len() - 1;
            if feat_idx < self.features.len() {
                self.selection = ProjectSelection::Feature(feat_idx);
            }
        }
        if self.selection != prev {
            self.preview_scroll = 0;
        }
    }

}
