use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub github_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileTreeState {
    pub selected_path: Option<PathBuf>,
    pub expanded: Vec<PathBuf>,
    pub scroll_offset: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectMeta {
    pub description: String,
    pub conventions: String,
    pub ai_hints: String,
    pub ai_notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStatus {
    Idea,
    Todo,
    InProgress,
    InReview,
    Done,
    Cancelled,
}

impl FeatureStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureStatus::Idea => "idea",
            FeatureStatus::Todo => "todo",
            FeatureStatus::InProgress => "in_progress",
            FeatureStatus::InReview => "in_review",
            FeatureStatus::Done => "done",
            FeatureStatus::Cancelled => "cancelled",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "todo" | "planning" => FeatureStatus::Todo,
            "in_progress" => FeatureStatus::InProgress,
            "in_review" | "review" => FeatureStatus::InReview,
            "done" => FeatureStatus::Done,
            "cancelled" => FeatureStatus::Cancelled,
            _ => FeatureStatus::Idea,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FeatureStatus::Idea => "Idea",
            FeatureStatus::Todo => "Todo",
            FeatureStatus::InProgress => "In progress",
            FeatureStatus::InReview => "In review",
            FeatureStatus::Done => "Done",
            FeatureStatus::Cancelled => "Cancelled",
        }
    }
    pub fn next(self) -> Self {
        match self {
            FeatureStatus::Idea => FeatureStatus::Todo,
            FeatureStatus::Todo => FeatureStatus::InProgress,
            FeatureStatus::InProgress => FeatureStatus::InReview,
            FeatureStatus::InReview => FeatureStatus::Done,
            FeatureStatus::Done => FeatureStatus::Cancelled,
            FeatureStatus::Cancelled => FeatureStatus::Idea,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            FeatureStatus::Idea => FeatureStatus::Cancelled,
            FeatureStatus::Todo => FeatureStatus::Idea,
            FeatureStatus::InProgress => FeatureStatus::Todo,
            FeatureStatus::InReview => FeatureStatus::InProgress,
            FeatureStatus::Done => FeatureStatus::InReview,
            FeatureStatus::Cancelled => FeatureStatus::Done,
        }
    }
    pub fn all() -> &'static [FeatureStatus] {
        &[
            FeatureStatus::Idea,
            FeatureStatus::Todo,
            FeatureStatus::InProgress,
            FeatureStatus::InReview,
            FeatureStatus::Done,
            FeatureStatus::Cancelled,
        ]
    }
    pub fn is_closed(self) -> bool {
        matches!(self, FeatureStatus::Done | FeatureStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Todo,
    InProgress,
    Done,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            StepStatus::Todo => "todo",
            StepStatus::InProgress => "in_progress",
            StepStatus::Done => "done",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => StepStatus::InProgress,
            "done" => StepStatus::Done,
            _ => StepStatus::Todo,
        }
    }
    pub fn glyph(self) -> &'static str {
        match self {
            StepStatus::Todo => "☐",
            StepStatus::InProgress => "◐",
            StepStatus::Done => "✓",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            StepStatus::Todo => StepStatus::InProgress,
            StepStatus::InProgress => StepStatus::Done,
            StepStatus::Done => StepStatus::Todo,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentStatus {
    Queued,
    Sent,
    Done,
}

impl CommentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CommentStatus::Queued => "queued",
            CommentStatus::Sent => "sent",
            CommentStatus::Done => "done",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "sent" => CommentStatus::Sent,
            "done" => CommentStatus::Done,
            _ => CommentStatus::Queued,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            CommentStatus::Queued => "queued",
            CommentStatus::Sent => "sent",
            CommentStatus::Done => "done",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Feature {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub description: String,
    pub status: FeatureStatus,
    pub order_idx: i64,
    pub steps: Vec<FeatureStep>,
    pub comments: Vec<FeatureComment>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FeatureStep {
    pub id: i64,
    pub feature_id: i64,
    pub summary: String,
    pub status: StepStatus,
    pub order_idx: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FeatureComment {
    pub id: i64,
    pub feature_id: i64,
    pub message: String,
    pub status: CommentStatus,
    pub created_at: i64,
}
