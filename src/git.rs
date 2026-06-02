use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    Untracked,
    Modified,
    Staged,
    Deleted,
}

pub fn fetch_status(root: &Path) -> HashMap<PathBuf, GitStatus> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1"])
        .output();
    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }
    parse_porcelain(&output.stdout, root)
}

fn parse_porcelain(bytes: &[u8], root: &Path) -> HashMap<PathBuf, GitStatus> {
    let text = String::from_utf8_lossy(bytes);
    let mut map = HashMap::new();
    for line in text.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 3 {
            continue;
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let rest = &line[3..];
        let name = parse_path_field(rest);
        let path = root.join(&name);
        let status = if x == '?' && y == '?' {
            GitStatus::Untracked
        } else if x == 'D' || y == 'D' {
            GitStatus::Deleted
        } else if y != ' ' {
            GitStatus::Modified
        } else if x != ' ' {
            GitStatus::Staged
        } else {
            continue;
        };
        map.insert(path, status);
    }
    map
}

fn parse_path_field(field: &str) -> String {
    let target = match field.split_once(" -> ") {
        Some((_, new_name)) => new_name,
        None => field,
    };
    let trimmed = target.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

pub fn show_head(repo: &Path, rel: &Path) -> Option<String> {
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("show")
        .arg(format!("HEAD:{}", rel_str))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn file_diff_head(repo: &Path, rel: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "HEAD", "--"])
        .arg(rel)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn working_diff(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn untracked_files(repo: &Path) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "--others", "--exclude-standard"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn staged_diff(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--staged"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn commit_with_message(repo: &Path, message: &str) -> Result<(), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("commit")
        .arg("-m")
        .arg(message)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(())
}

pub fn stage(repo: &Path, rel: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("add")
        .arg("--")
        .arg(rel)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn unstage(repo: &Path, rel: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["restore", "--staged", "--"])
        .arg(rel)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn stage_all(repo: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["add", "-A"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn unstage_all(repo: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["restore", "--staged", "."])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn any_staged_changes(repo: &Path) -> bool {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain"])
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0] as char;
        if x != ' ' && x != '?' {
            return true;
        }
    }
    false
}

pub fn has_staged_changes(repo: &Path, rel: &Path) -> bool {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain", "--"])
        .arg(rel)
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0] as char;
        if x != ' ' && x != '?' {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub head_ref: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    pub is_locked: bool,
}

pub fn current_branch(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || s == "HEAD" {
        None
    } else {
        Some(s)
    }
}

pub fn list_branches(repo: &Path) -> Vec<BranchInfo> {
    let cur = current_branch(repo).unwrap_or_default();
    let mut out: Vec<BranchInfo> = Vec::new();
    if let Ok(o) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)%09%(upstream:short)",
            "refs/heads/",
        ])
        .output()
    {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let mut parts = line.splitn(2, '\t');
                let name = parts.next().unwrap_or("").to_string();
                let upstream = parts
                    .next()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                if name.is_empty() {
                    continue;
                }
                out.push(BranchInfo {
                    is_current: name == cur,
                    name,
                    is_remote: false,
                    upstream,
                });
            }
        }
    }
    if let Ok(o) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/remotes/",
        ])
        .output()
    {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let name = line.trim().to_string();
                if name.is_empty() || name.ends_with("/HEAD") {
                    continue;
                }
                let bare = name.split_once('/').map(|(_, r)| r).unwrap_or(&name);
                if out.iter().any(|b| b.name == bare) {
                    continue;
                }
                out.push(BranchInfo {
                    name,
                    is_current: false,
                    is_remote: true,
                    upstream: None,
                });
            }
        }
    }
    out
}

pub fn branch_commits(repo: &Path, branch: &str, limit: usize) -> Vec<CommitInfo> {
    let n = format!("-n{}", limit);
    let pretty = "--pretty=format:%H%x09%h%x09%an%x09%ad%x09%s";
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", &n, "--date=short", pretty, branch])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let parts: Vec<&str> = line.splitn(5, '\t').collect();
        if parts.len() < 5 {
            continue;
        }
        commits.push(CommitInfo {
            sha: parts[0].to_string(),
            short_sha: parts[1].to_string(),
            author: parts[2].to_string(),
            date: parts[3].to_string(),
            summary: parts[4].to_string(),
        });
    }
    commits
}

pub fn commit_show(repo: &Path, sha: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["show", "--stat", "--patch", sha])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run_git_output(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    Ok(if !stdout.is_empty() { stdout } else { stderr })
}

pub fn checkout(repo: &Path, branch: &str) -> Result<String, String> {
    let target = branch.strip_prefix("origin/").unwrap_or(branch);
    run_git_output(repo, &["checkout", target])
}

pub fn pull(repo: &Path) -> Result<String, String> {
    run_git_output(repo, &["pull", "--ff-only"])
}

pub fn push(repo: &Path) -> Result<String, String> {
    run_git_output(repo, &["push"])
}

pub fn push_set_upstream(repo: &Path, branch: &str) -> Result<String, String> {
    run_git_output(repo, &["push", "-u", "origin", branch])
}

pub fn merge(repo: &Path, branch: &str) -> Result<String, String> {
    let target = branch.strip_prefix("origin/").unwrap_or(branch);
    run_git_output(repo, &["merge", "--no-ff", target])
}

pub fn list_prs(repo: &Path, head_branch: Option<&str>) -> Result<Vec<PrInfo>, String> {
    let mut args: Vec<String> = vec![
        "pr".into(),
        "list".into(),
        "--json".into(),
        "number,title,state,author,headRefName,url".into(),
        "--limit".into(),
        "30".into(),
        "--state".into(),
        "all".into(),
    ];
    if let Some(b) = head_branch {
        args.push("--head".into());
        args.push(b.to_string());
    }
    let out = Command::new("gh")
        .current_dir(repo)
        .args(&args)
        .output()
        .map_err(|e| format!("gh not available: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_gh_prs(&String::from_utf8_lossy(&out.stdout))
}

fn parse_gh_prs(json: &str) -> Result<Vec<PrInfo>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let arr = v.as_array().ok_or_else(|| "expected JSON array".to_string())?;
    let mut prs = Vec::new();
    for item in arr {
        prs.push(PrInfo {
            number: item.get("number").and_then(|x| x.as_i64()).unwrap_or(0),
            title: item
                .get("title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            state: item
                .get("state")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            author: item
                .get("author")
                .and_then(|a| a.get("login").and_then(|x| x.as_str()))
                .unwrap_or("")
                .to_string(),
            head_ref: item
                .get("headRefName")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            url: item
                .get("url")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        });
    }
    Ok(prs)
}

pub fn pr_view(repo: &Path, number: i64) -> Result<String, String> {
    let n = number.to_string();
    let out = Command::new("gh")
        .current_dir(repo)
        .args(["pr", "view", &n, "--comments"])
        .output()
        .map_err(|e| format!("gh not available: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn pr_create(repo: &Path, title: &str, body: &str) -> Result<String, String> {
    let out = Command::new("gh")
        .current_dir(repo)
        .args(["pr", "create", "--title", title, "--body", body])
        .output()
        .map_err(|e| format!("gh not available: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn list_worktrees(repo: &Path) -> Vec<WorktreeInfo> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "list", "--porcelain"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_worktree_porcelain(&String::from_utf8_lossy(&out.stdout))
}

fn parse_worktree_porcelain(text: &str) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    let mut cur: Option<WorktreeInfo> = None;
    for line in text.lines() {
        if line.is_empty() {
            if let Some(w) = cur.take() {
                out.push(w);
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                out.push(w);
            }
            cur = Some(WorktreeInfo {
                path: PathBuf::from(path.trim()),
                branch: None,
                head: None,
                is_bare: false,
                is_detached: false,
                is_locked: false,
            });
        } else if let Some(w) = cur.as_mut() {
            if let Some(sha) = line.strip_prefix("HEAD ") {
                w.head = Some(sha.trim().to_string());
            } else if let Some(branch) = line.strip_prefix("branch ") {
                let b = branch.trim().strip_prefix("refs/heads/").unwrap_or(branch.trim());
                w.branch = Some(b.to_string());
            } else if line == "bare" {
                w.is_bare = true;
            } else if line == "detached" {
                w.is_detached = true;
            } else if line.starts_with("locked") {
                w.is_locked = true;
            }
        }
    }
    if let Some(w) = cur.take() {
        out.push(w);
    }
    out
}

pub fn add_worktree(repo: &Path, path: &Path, branch: &str, create_branch: bool) -> Result<String, String> {
    let path_str = path.to_string_lossy().to_string();
    let args: Vec<&str> = if create_branch {
        vec!["worktree", "add", "-b", branch, &path_str]
    } else {
        vec!["worktree", "add", &path_str, branch]
    };
    run_git_output(repo, &args)
}

pub fn remove_worktree(repo: &Path, path: &Path, force: bool) -> Result<String, String> {
    let path_str = path.to_string_lossy().to_string();
    let args: Vec<&str> = if force {
        vec!["worktree", "remove", "--force", &path_str]
    } else {
        vec!["worktree", "remove", &path_str]
    };
    run_git_output(repo, &args)
}

pub fn detect_github_url(project_path: &Path) -> Option<String> {
    let config = std::fs::read_to_string(project_path.join(".git").join("config")).ok()?;
    extract_origin_url(&config).and_then(|raw| normalize_github_url(&raw))
}

fn extract_origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if let Some(section) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_origin = is_origin_section(section);
            continue;
        }
        if in_origin {
            if let Some(url) = strip_url_assignment(trimmed) {
                return Some(url);
            }
        }
    }
    None
}

fn is_origin_section(section: &str) -> bool {
    let normalized = section.replace('\'', "\"");
    normalized.trim() == "remote \"origin\""
}

fn strip_url_assignment(line: &str) -> Option<String> {
    let rest = line.strip_prefix("url")?.trim_start();
    let value = rest.strip_prefix('=')?.trim();
    Some(value.to_string())
}

fn normalize_github_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if let Some(rest) = without_git.strip_prefix("git@github.com:") {
        return Some(format!("https://github.com/{}", rest));
    }
    if let Some(rest) = without_git.strip_prefix("ssh://git@github.com/") {
        return Some(format!("https://github.com/{}", rest));
    }
    if without_git.starts_with("https://github.com/")
        || without_git.starts_with("http://github.com/")
    {
        return Some(without_git.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_remote() {
        let cfg = r#"
[core]
    bare = false
[remote "origin"]
    url = git@github.com:acme/widget.git
    fetch = +refs/heads/*:refs/remotes/origin/*
"#;
        let raw = extract_origin_url(cfg).unwrap();
        assert_eq!(
            normalize_github_url(&raw).unwrap(),
            "https://github.com/acme/widget"
        );
    }

    #[test]
    fn parses_https_remote() {
        let cfg = "[remote \"origin\"]\n    url = https://github.com/foo/bar.git\n";
        let raw = extract_origin_url(cfg).unwrap();
        assert_eq!(
            normalize_github_url(&raw).unwrap(),
            "https://github.com/foo/bar"
        );
    }

    #[test]
    fn ignores_non_origin_remotes() {
        let cfg = r#"
[remote "upstream"]
    url = git@github.com:upstream/repo.git
[remote "origin"]
    url = git@github.com:me/repo.git
"#;
        let raw = extract_origin_url(cfg).unwrap();
        assert_eq!(
            normalize_github_url(&raw).unwrap(),
            "https://github.com/me/repo"
        );
    }

    #[test]
    fn returns_none_for_non_github() {
        let cfg = "[remote \"origin\"]\n    url = git@gitlab.com:me/repo.git\n";
        let raw = extract_origin_url(cfg).unwrap();
        assert_eq!(normalize_github_url(&raw), None);
    }

    #[test]
    fn parses_worktree_porcelain() {
        let text = "worktree /repo/main\nHEAD abc123\nbranch refs/heads/main\n\n\
                    worktree /repo/wt-foo\nHEAD def456\nbranch refs/heads/feature/foo\nlocked\n\n\
                    worktree /repo/wt-detached\nHEAD 999000\ndetached\n";
        let wts = parse_worktree_porcelain(text);
        assert_eq!(wts.len(), 3);
        assert_eq!(wts[0].path, PathBuf::from("/repo/main"));
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert!(!wts[0].is_locked);
        assert_eq!(wts[1].branch.as_deref(), Some("feature/foo"));
        assert!(wts[1].is_locked);
        assert!(wts[2].is_detached);
        assert!(wts[2].branch.is_none());
    }
}
