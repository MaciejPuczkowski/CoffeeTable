use crate::git::{BranchInfo, CommitInfo, PrInfo, WorktreeInfo};
use ratatui::layout::Rect;
use std::path::PathBuf;

const COMMIT_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitPane {
    Branches,
    Commits,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailsMode {
    Commit,
    PrList,
    PrView,
    Worktrees,
}

pub struct GitTreeView {
    pub root: PathBuf,
    pub branches: Vec<BranchInfo>,
    pub commits: Vec<CommitInfo>,
    pub prs: Vec<PrInfo>,
    pub worktrees: Vec<WorktreeInfo>,
    pub current_branch: Option<String>,
    pub focus: GitPane,
    pub branch_idx: usize,
    pub commit_idx: usize,
    pub pr_idx: usize,
    pub worktree_idx: usize,
    pub details_mode: DetailsMode,
    pub details_text: String,
    pub details_scroll: u16,
    pub details_title: String,
    pub branches_area: Rect,
    pub commits_area: Rect,
    pub details_area: Rect,
}

impl GitTreeView {
    pub fn new(root: PathBuf) -> Self {
        let mut v = Self {
            root,
            branches: Vec::new(),
            commits: Vec::new(),
            prs: Vec::new(),
            worktrees: Vec::new(),
            current_branch: None,
            focus: GitPane::Branches,
            branch_idx: 0,
            commit_idx: 0,
            pr_idx: 0,
            worktree_idx: 0,
            details_mode: DetailsMode::Commit,
            details_text: String::new(),
            details_scroll: 0,
            details_title: "Commit details".into(),
            branches_area: Rect::default(),
            commits_area: Rect::default(),
            details_area: Rect::default(),
        };
        v.refresh_all();
        v
    }

    pub fn refresh_all(&mut self) {
        self.current_branch = crate::git::current_branch(&self.root);
        let prev_name = self
            .branches
            .get(self.branch_idx)
            .map(|b| b.name.clone());
        self.branches = crate::git::list_branches(&self.root);
        if self.branches.is_empty() {
            self.commits.clear();
            self.details_text.clear();
            return;
        }
        self.branch_idx = prev_name
            .and_then(|name| self.branches.iter().position(|b| b.name == name))
            .unwrap_or(0)
            .min(self.branches.len().saturating_sub(1));
        self.load_commits_for_selected_branch();
        self.load_selected_commit_details();
    }

    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.branches.get(self.branch_idx)
    }

    fn load_commits_for_selected_branch(&mut self) {
        if let Some(b) = self.branches.get(self.branch_idx) {
            self.commits = crate::git::branch_commits(&self.root, &b.name, COMMIT_LIMIT);
            self.commit_idx = 0;
        } else {
            self.commits.clear();
        }
    }

    fn load_selected_commit_details(&mut self) {
        self.details_mode = DetailsMode::Commit;
        self.details_scroll = 0;
        if let Some(c) = self.commits.get(self.commit_idx) {
            self.details_title = format!("Commit {}", c.short_sha);
            self.details_text = crate::git::commit_show(&self.root, &c.sha)
                .unwrap_or_else(|| "(failed to load commit)".to_string());
        } else {
            self.details_title = "Commit details".into();
            self.details_text.clear();
        }
    }

    pub fn load_prs_for_branch(&mut self) -> Result<(), String> {
        let branch = self.branches.get(self.branch_idx).map(|b| {
            b.name
                .strip_prefix("origin/")
                .unwrap_or(&b.name)
                .to_string()
        });
        let label = branch.clone().unwrap_or_else(|| "(no branch)".into());
        self.details_scroll = 0;
        self.details_mode = DetailsMode::PrList;
        self.pr_idx = 0;
        self.details_title = format!("PRs for {}", label);
        match crate::git::list_prs(&self.root, branch.as_deref()) {
            Ok(list) => {
                self.prs = list;
                self.details_text = self.render_pr_list_text();
                Ok(())
            }
            Err(e) => {
                self.prs.clear();
                self.details_text = format!("gh error: {}", e);
                Err(e)
            }
        }
    }

    fn render_pr_list_text(&self) -> String {
        if self.prs.is_empty() {
            return "(no PRs found for this branch)".to_string();
        }
        let mut out = String::new();
        out.push_str("PRs (Enter to view comments)\n\n");
        for (i, pr) in self.prs.iter().enumerate() {
            let marker = if i == self.pr_idx { "▶" } else { " " };
            out.push_str(&format!(
                "{} #{:<5} [{}] {}\n      head: {} • by @{}\n      {}\n",
                marker, pr.number, pr.state, pr.title, pr.head_ref, pr.author, pr.url
            ));
        }
        out
    }

    pub fn load_selected_pr_view(&mut self) -> Result<(), String> {
        let Some(pr) = self.prs.get(self.pr_idx).cloned() else {
            return Err("No PR selected".into());
        };
        self.details_scroll = 0;
        self.details_mode = DetailsMode::PrView;
        self.details_title = format!("PR #{} — {}", pr.number, pr.title);
        match crate::git::pr_view(&self.root, pr.number) {
            Ok(s) => {
                self.details_text = s;
                Ok(())
            }
            Err(e) => {
                self.details_text = format!("gh error: {}", e);
                Err(e)
            }
        }
    }

    pub fn back_to_pr_list(&mut self) {
        match self.details_mode {
            DetailsMode::PrView => {
                self.details_mode = DetailsMode::PrList;
                self.details_scroll = 0;
                self.details_text = self.render_pr_list_text();
                let label = self
                    .branches
                    .get(self.branch_idx)
                    .map(|b| b.name.strip_prefix("origin/").unwrap_or(&b.name).to_string())
                    .unwrap_or_default();
                self.details_title = format!("PRs for {}", label);
            }
            DetailsMode::PrList | DetailsMode::Worktrees => self.load_selected_commit_details(),
            DetailsMode::Commit => {}
        }
    }

    pub fn load_worktrees(&mut self) {
        self.worktrees = crate::git::list_worktrees(&self.root);
        if self.worktree_idx >= self.worktrees.len() {
            self.worktree_idx = self.worktrees.len().saturating_sub(1);
        }
        self.details_mode = DetailsMode::Worktrees;
        self.details_scroll = 0;
        self.details_title = "Worktrees".into();
        self.details_text = self.render_worktrees_text();
    }

    fn render_worktrees_text(&self) -> String {
        if self.worktrees.is_empty() {
            return "(no worktrees — press 'n' to create one)".to_string();
        }
        let mut out = String::new();
        out.push_str("Worktrees (Enter to open as project • n new • D delete)\n\n");
        for (i, wt) in self.worktrees.iter().enumerate() {
            let marker = if i == self.worktree_idx { "▶" } else { " " };
            let branch = wt
                .branch
                .clone()
                .unwrap_or_else(|| if wt.is_detached { "(detached)".into() } else { "(none)".into() });
            let lock = if wt.is_locked { " [locked]" } else { "" };
            let bare = if wt.is_bare { " [bare]" } else { "" };
            let head = wt.head.as_deref().map(|h| &h[..h.len().min(7)]).unwrap_or("-------");
            out.push_str(&format!(
                "{} {}{}{}\n      branch: {}  head: {}\n",
                marker,
                wt.path.display(),
                lock,
                bare,
                branch,
                head,
            ));
        }
        out
    }

    pub fn selected_worktree(&self) -> Option<&WorktreeInfo> {
        self.worktrees.get(self.worktree_idx)
    }

    pub fn remove_selected_worktree(&mut self, force: bool) -> Result<String, String> {
        let path = self
            .worktrees
            .get(self.worktree_idx)
            .map(|w| w.path.clone())
            .ok_or_else(|| "No worktree selected".to_string())?;
        let result = crate::git::remove_worktree(&self.root, &path, force);
        if result.is_ok() {
            self.load_worktrees();
        }
        result
    }

    pub fn cycle_pane(&mut self, forward: bool) {
        self.focus = match (self.focus, forward) {
            (GitPane::Branches, true) => GitPane::Commits,
            (GitPane::Commits, true) => GitPane::Details,
            (GitPane::Details, true) => GitPane::Branches,
            (GitPane::Branches, false) => GitPane::Details,
            (GitPane::Commits, false) => GitPane::Branches,
            (GitPane::Details, false) => GitPane::Commits,
        };
    }

    pub fn move_down(&mut self) {
        match self.focus {
            GitPane::Branches => {
                if self.branch_idx + 1 < self.branches.len() {
                    self.branch_idx += 1;
                    self.load_commits_for_selected_branch();
                    self.load_selected_commit_details();
                }
            }
            GitPane::Commits => {
                if self.commit_idx + 1 < self.commits.len() {
                    self.commit_idx += 1;
                    self.load_selected_commit_details();
                }
            }
            GitPane::Details => self.details_move_down(),
        }
    }

    fn details_move_down(&mut self) {
        match self.details_mode {
            DetailsMode::PrList if !self.prs.is_empty() => {
                if self.pr_idx + 1 < self.prs.len() {
                    self.pr_idx += 1;
                    self.details_text = self.render_pr_list_text();
                }
            }
            DetailsMode::Worktrees if !self.worktrees.is_empty() => {
                if self.worktree_idx + 1 < self.worktrees.len() {
                    self.worktree_idx += 1;
                    self.details_text = self.render_worktrees_text();
                }
            }
            _ => self.details_scroll = self.details_scroll.saturating_add(1),
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            GitPane::Branches => {
                if self.branch_idx > 0 {
                    self.branch_idx -= 1;
                    self.load_commits_for_selected_branch();
                    self.load_selected_commit_details();
                }
            }
            GitPane::Commits => {
                if self.commit_idx > 0 {
                    self.commit_idx -= 1;
                    self.load_selected_commit_details();
                }
            }
            GitPane::Details => self.details_move_up(),
        }
    }

    fn details_move_up(&mut self) {
        match self.details_mode {
            DetailsMode::PrList if !self.prs.is_empty() => {
                if self.pr_idx > 0 {
                    self.pr_idx -= 1;
                    self.details_text = self.render_pr_list_text();
                }
            }
            DetailsMode::Worktrees if !self.worktrees.is_empty() => {
                if self.worktree_idx > 0 {
                    self.worktree_idx -= 1;
                    self.details_text = self.render_worktrees_text();
                }
            }
            _ => self.details_scroll = self.details_scroll.saturating_sub(1),
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            GitPane::Branches => {
                self.branch_idx = 0;
                self.load_commits_for_selected_branch();
                self.load_selected_commit_details();
            }
            GitPane::Commits => {
                self.commit_idx = 0;
                self.load_selected_commit_details();
            }
            GitPane::Details => self.details_scroll = 0,
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            GitPane::Branches => {
                if !self.branches.is_empty() {
                    self.branch_idx = self.branches.len() - 1;
                    self.load_commits_for_selected_branch();
                    self.load_selected_commit_details();
                }
            }
            GitPane::Commits => {
                if !self.commits.is_empty() {
                    self.commit_idx = self.commits.len() - 1;
                    self.load_selected_commit_details();
                }
            }
            GitPane::Details => self.details_scroll = u16::MAX / 2,
        }
    }

    pub fn activate(&mut self) {
        match self.focus {
            GitPane::Branches => self.focus = GitPane::Commits,
            GitPane::Commits => {
                self.load_selected_commit_details();
                self.focus = GitPane::Details;
            }
            GitPane::Details => {
                if matches!(self.details_mode, DetailsMode::PrList) && !self.prs.is_empty() {
                    let _ = self.load_selected_pr_view();
                }
            }
        }
    }

    pub fn checkout_selected(&mut self) -> Result<String, String> {
        let name = self
            .branches
            .get(self.branch_idx)
            .map(|b| b.name.clone())
            .ok_or_else(|| "No branch selected".to_string())?;
        let out = crate::git::checkout(&self.root, &name)?;
        self.refresh_all();
        Ok(out)
    }

    pub fn pull(&mut self) -> Result<String, String> {
        let out = crate::git::pull(&self.root)?;
        self.refresh_all();
        Ok(out)
    }

    pub fn push(&mut self) -> Result<String, String> {
        match crate::git::push(&self.root) {
            Ok(s) => Ok(s),
            Err(e) => {
                let needs_upstream = e.contains("no upstream")
                    || e.contains("--set-upstream")
                    || e.contains("has no upstream branch");
                if needs_upstream {
                    if let Some(branch) = self.current_branch.clone() {
                        return crate::git::push_set_upstream(&self.root, &branch);
                    }
                }
                Err(e)
            }
        }
    }

    pub fn merge_selected_into_current(&mut self) -> Result<String, String> {
        let b = self
            .branches
            .get(self.branch_idx)
            .ok_or_else(|| "No branch selected".to_string())?;
        if b.is_current {
            return Err("Selected branch is the current branch".into());
        }
        let name = b.name.clone();
        let out = crate::git::merge(&self.root, &name)?;
        self.refresh_all();
        Ok(out)
    }

    pub fn create_pr_for_current(&mut self) -> Result<String, String> {
        let branch = self
            .current_branch
            .clone()
            .ok_or_else(|| "Detached HEAD".to_string())?;
        let title = crate::git::branch_commits(&self.root, &branch, 1)
            .first()
            .map(|c| c.summary.clone())
            .unwrap_or_else(|| format!("PR from {}", branch));
        crate::git::pr_create(&self.root, &title, "")
    }
}
