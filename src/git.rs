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
}
