use crate::github::{PrSummary, RunInfo, WorkflowInfo};
use ratatui::layout::Rect;
use std::path::PathBuf;

const RUN_LIMIT: usize = 30;
const PR_LIMIT: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhPane {
    Runs,
    Prs,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailsMode {
    Empty,
    RunLog,
    PrView,
}

pub struct GithubView {
    pub root: PathBuf,
    pub repo_configured: bool,
    pub runs: Vec<RunInfo>,
    pub prs: Vec<PrSummary>,
    pub workflows: Vec<WorkflowInfo>,
    pub focus: GhPane,
    pub last_list_pane: GhPane,
    pub run_idx: usize,
    pub pr_idx: usize,
    pub details_mode: DetailsMode,
    pub details_text: String,
    pub details_title: String,
    pub details_scroll: u16,
    pub last_error: Option<String>,
    pub runs_area: Rect,
    pub prs_area: Rect,
    pub details_area: Rect,
}

impl GithubView {
    pub fn new(root: PathBuf) -> Self {
        let repo_configured = crate::git::detect_github_url(&root).is_some();
        let mut v = Self {
            root,
            repo_configured,
            runs: Vec::new(),
            prs: Vec::new(),
            workflows: Vec::new(),
            focus: GhPane::Runs,
            last_list_pane: GhPane::Runs,
            run_idx: 0,
            pr_idx: 0,
            details_mode: DetailsMode::Empty,
            details_text: String::new(),
            details_title: "Details".into(),
            details_scroll: 0,
            last_error: None,
            runs_area: Rect::default(),
            prs_area: Rect::default(),
            details_area: Rect::default(),
        };
        if v.repo_configured {
            v.refresh_all();
        } else {
            v.details_title = "GitHub".into();
            v.details_text = "This repository has no GitHub `origin` remote.\n\nAdd one with:\n  git remote add origin git@github.com:owner/repo.git\n\nThen press 'r' to refresh.".into();
        }
        v
    }

    pub fn refresh_all(&mut self) {
        if !self.repo_configured {
            return;
        }
        self.last_error = None;
        match crate::github::list_runs(&self.root, None, RUN_LIMIT) {
            Ok(list) => self.runs = list,
            Err(e) => {
                self.runs.clear();
                self.last_error = Some(format!("runs: {}", e));
            }
        }
        match crate::github::list_all_prs(&self.root, PR_LIMIT) {
            Ok(list) => self.prs = list,
            Err(e) => {
                self.prs.clear();
                if self.last_error.is_none() {
                    self.last_error = Some(format!("prs: {}", e));
                }
            }
        }
        if self.workflows.is_empty() {
            if let Ok(list) = crate::github::list_workflows(&self.root) {
                self.workflows = list;
            }
        }
        if self.run_idx >= self.runs.len() {
            self.run_idx = self.runs.len().saturating_sub(1);
        }
        if self.pr_idx >= self.prs.len() {
            self.pr_idx = self.prs.len().saturating_sub(1);
        }
        self.refresh_summary_for_focus();
    }

    fn refresh_summary_for_focus(&mut self) {
        if let Some(err) = self.last_error.clone() {
            self.details_mode = DetailsMode::Empty;
            self.details_title = "gh error".into();
            self.details_text = err;
            self.details_scroll = 0;
            return;
        }
        match self.focus {
            GhPane::Runs => self.show_run_summary(),
            GhPane::Prs => self.show_pr_summary(),
            GhPane::Details => {}
        }
    }

    fn show_run_summary(&mut self) {
        self.details_mode = DetailsMode::Empty;
        self.details_scroll = 0;
        if let Some(r) = self.runs.get(self.run_idx) {
            self.details_title = format!("Run #{}", r.id);
            let conclusion = if r.conclusion.is_empty() { "—" } else { r.conclusion.as_str() };
            self.details_text = format!(
                "Workflow:    {}\nStatus:      {}\nConclusion:  {}\nBranch:      {}\nEvent:       {}\nCreated:     {}\nURL:         {}\n\nTitle:\n  {}\n\n(Enter to load full log)",
                r.workflow_name,
                r.status,
                conclusion,
                r.head_branch,
                r.event,
                r.created_at,
                r.url,
                r.display_title,
            );
        } else {
            self.details_title = "Runs".into();
            self.details_text = "(no workflow runs)".into();
        }
    }

    fn show_pr_summary(&mut self) {
        self.details_mode = DetailsMode::Empty;
        self.details_scroll = 0;
        if let Some(p) = self.prs.get(self.pr_idx) {
            self.details_title = format!("PR #{}", p.number);
            let draft = if p.draft { " [draft]" } else { "" };
            self.details_text = format!(
                "Title:    {}{}\nState:    {}\nAuthor:   @{}\nHead:     {}\nBase:     {}\nUpdated:  {}\nURL:      {}\n\n(Enter to load full PR view)",
                p.title, draft, p.state, p.author, p.head_ref, p.base_ref, p.updated_at, p.url,
            );
        } else {
            self.details_title = "PRs".into();
            self.details_text = "(no pull requests)".into();
        }
    }

    pub fn cycle_pane(&mut self, forward: bool) {
        self.focus = match (self.focus, forward) {
            (GhPane::Runs, true) => GhPane::Prs,
            (GhPane::Prs, true) => GhPane::Details,
            (GhPane::Details, true) => GhPane::Runs,
            (GhPane::Runs, false) => GhPane::Details,
            (GhPane::Prs, false) => GhPane::Runs,
            (GhPane::Details, false) => GhPane::Prs,
        };
        if !matches!(self.focus, GhPane::Details) {
            self.last_list_pane = self.focus;
            self.refresh_summary_for_focus();
        }
    }

    pub fn move_down(&mut self) {
        match self.focus {
            GhPane::Runs => {
                if self.run_idx + 1 < self.runs.len() {
                    self.run_idx += 1;
                    self.show_run_summary();
                }
            }
            GhPane::Prs => {
                if self.pr_idx + 1 < self.prs.len() {
                    self.pr_idx += 1;
                    self.show_pr_summary();
                }
            }
            GhPane::Details => self.details_scroll = self.details_scroll.saturating_add(1),
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            GhPane::Runs => {
                if self.run_idx > 0 {
                    self.run_idx -= 1;
                    self.show_run_summary();
                }
            }
            GhPane::Prs => {
                if self.pr_idx > 0 {
                    self.pr_idx -= 1;
                    self.show_pr_summary();
                }
            }
            GhPane::Details => self.details_scroll = self.details_scroll.saturating_sub(1),
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            GhPane::Runs => {
                self.run_idx = 0;
                self.show_run_summary();
            }
            GhPane::Prs => {
                self.pr_idx = 0;
                self.show_pr_summary();
            }
            GhPane::Details => self.details_scroll = 0,
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            GhPane::Runs => {
                if !self.runs.is_empty() {
                    self.run_idx = self.runs.len() - 1;
                    self.show_run_summary();
                }
            }
            GhPane::Prs => {
                if !self.prs.is_empty() {
                    self.pr_idx = self.prs.len() - 1;
                    self.show_pr_summary();
                }
            }
            GhPane::Details => self.details_scroll = u16::MAX / 2,
        }
    }

    pub fn activate(&mut self) {
        match self.focus {
            GhPane::Runs => self.load_selected_run_log(),
            GhPane::Prs => self.load_selected_pr_view(),
            GhPane::Details => {}
        }
    }

    fn load_selected_run_log(&mut self) {
        let Some(r) = self.runs.get(self.run_idx).cloned() else {
            return;
        };
        self.last_list_pane = GhPane::Runs;
        self.details_scroll = 0;
        self.details_mode = DetailsMode::RunLog;
        self.details_title = format!("Run #{} log — {}", r.id, r.workflow_name);
        match crate::github::run_logs(&self.root, r.id) {
            Ok(s) => {
                self.details_text = if s.trim().is_empty() {
                    "(empty log — run may still be queued)".into()
                } else {
                    s
                };
            }
            Err(e) => self.details_text = format!("gh error: {}", e),
        }
        self.focus = GhPane::Details;
    }

    fn load_selected_pr_view(&mut self) {
        let Some(p) = self.prs.get(self.pr_idx).cloned() else {
            return;
        };
        self.last_list_pane = GhPane::Prs;
        self.details_scroll = 0;
        self.details_mode = DetailsMode::PrView;
        self.details_title = format!("PR #{} — {}", p.number, p.title);
        match crate::git::pr_view(&self.root, p.number) {
            Ok(s) => self.details_text = s,
            Err(e) => self.details_text = format!("gh error: {}", e),
        }
        self.focus = GhPane::Details;
    }

    pub fn back(&mut self) {
        if matches!(self.focus, GhPane::Details) {
            self.focus = self.last_list_pane;
        }
        if !matches!(self.details_mode, DetailsMode::Empty) {
            self.details_mode = DetailsMode::Empty;
            self.details_scroll = 0;
            self.refresh_summary_for_focus();
        }
    }

    pub fn rerun_selected(&mut self, failed_only: bool) -> Result<String, String> {
        let id = self
            .runs
            .get(self.run_idx)
            .map(|r| r.id)
            .ok_or_else(|| "No run selected".to_string())?;
        let out = crate::github::run_rerun(&self.root, id, failed_only)?;
        self.refresh_all();
        Ok(if out.trim().is_empty() {
            format!("Re-ran run #{}", id)
        } else {
            out
        })
    }

    pub fn cancel_selected_run(&mut self) -> Result<String, String> {
        let id = self
            .runs
            .get(self.run_idx)
            .map(|r| r.id)
            .ok_or_else(|| "No run selected".to_string())?;
        let out = crate::github::run_cancel(&self.root, id)?;
        self.refresh_all();
        Ok(if out.trim().is_empty() {
            format!("Cancelled run #{}", id)
        } else {
            out
        })
    }

    pub fn checkout_selected_pr(&mut self) -> Result<String, String> {
        let n = self
            .prs
            .get(self.pr_idx)
            .map(|p| p.number)
            .ok_or_else(|| "No PR selected".to_string())?;
        crate::github::pr_checkout(&self.root, n)
    }
}
