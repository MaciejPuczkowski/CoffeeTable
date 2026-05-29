use crate::project::{Feature, ProjectMeta};
use crate::views::editor::EditorView;
use crate::views::feature_form::FeatureForm;
use ratatui::widgets::ListState;

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
        if idx < sections.len() {
            self.selection = ProjectSelection::Meta(sections[idx]);
            return;
        }
        if idx == sections.len() {
            self.selection = ProjectSelection::NewFeature;
            return;
        }
        let feat_idx = idx - sections.len() - 1;
        if feat_idx < self.features.len() {
            self.selection = ProjectSelection::Feature(feat_idx);
        }
    }

}
