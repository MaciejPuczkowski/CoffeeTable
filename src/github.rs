use std::{path::Path, process::Command};

#[derive(Debug, Clone)]
pub struct WorkflowInfo {
    pub id: i64,
    pub name: String,
    pub state: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RunInfo {
    pub id: i64,
    pub workflow_name: String,
    pub display_title: String,
    pub status: String,
    pub conclusion: String,
    pub event: String,
    pub head_branch: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct PrSummary {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub head_ref: String,
    pub base_ref: String,
    pub url: String,
    pub draft: bool,
    pub updated_at: String,
}

pub fn list_workflows(repo: &Path) -> Result<Vec<WorkflowInfo>, String> {
    let out = run_gh(
        repo,
        &[
            "workflow",
            "list",
            "--json",
            "id,name,state,path",
            "--limit",
            "100",
        ],
    )?;
    parse_workflows(&out)
}

pub fn list_runs(
    repo: &Path,
    workflow_id: Option<i64>,
    limit: usize,
) -> Result<Vec<RunInfo>, String> {
    let limit_str = limit.to_string();
    let mut args: Vec<String> = vec![
        "run".into(),
        "list".into(),
        "--json".into(),
        "databaseId,displayTitle,name,status,conclusion,event,headBranch,createdAt,url".into(),
        "--limit".into(),
        limit_str,
    ];
    if let Some(id) = workflow_id {
        args.push("--workflow".into());
        args.push(id.to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_gh(repo, &refs)?;
    parse_runs(&out)
}

pub fn run_logs(repo: &Path, id: i64) -> Result<String, String> {
    let n = id.to_string();
    run_gh(repo, &["run", "view", &n, "--log"])
}

pub fn run_rerun(repo: &Path, id: i64, failed_only: bool) -> Result<String, String> {
    let n = id.to_string();
    if failed_only {
        run_gh(repo, &["run", "rerun", &n, "--failed"])
    } else {
        run_gh(repo, &["run", "rerun", &n])
    }
}

pub fn run_cancel(repo: &Path, id: i64) -> Result<String, String> {
    let n = id.to_string();
    run_gh(repo, &["run", "cancel", &n])
}

pub fn list_all_prs(repo: &Path, limit: usize) -> Result<Vec<PrSummary>, String> {
    let limit_str = limit.to_string();
    let out = run_gh(
        repo,
        &[
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            &limit_str,
            "--json",
            "number,title,state,author,headRefName,baseRefName,url,isDraft,updatedAt",
        ],
    )?;
    parse_prs(&out)
}

pub fn pr_checkout(repo: &Path, number: i64) -> Result<String, String> {
    let n = number.to_string();
    run_gh(repo, &["pr", "checkout", &n])
}

fn run_gh(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .current_dir(repo)
        .args(args)
        .output()
        .map_err(|e| format!("gh not available: {}", e))?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        });
    }
    Ok(stdout)
}

fn parse_workflows(json: &str) -> Result<Vec<WorkflowInfo>, String> {
    let arr = parse_array(json)?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        out.push(WorkflowInfo {
            id: json_i64(&item, "id"),
            name: json_str(&item, "name"),
            state: json_str(&item, "state"),
            path: json_str(&item, "path"),
        });
    }
    Ok(out)
}

fn parse_runs(json: &str) -> Result<Vec<RunInfo>, String> {
    let arr = parse_array(json)?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        out.push(RunInfo {
            id: json_i64(&item, "databaseId"),
            workflow_name: json_str(&item, "name"),
            display_title: json_str(&item, "displayTitle"),
            status: json_str(&item, "status"),
            conclusion: json_str(&item, "conclusion"),
            event: json_str(&item, "event"),
            head_branch: json_str(&item, "headBranch"),
            created_at: json_str(&item, "createdAt"),
            url: json_str(&item, "url"),
        });
    }
    Ok(out)
}

fn parse_prs(json: &str) -> Result<Vec<PrSummary>, String> {
    let arr = parse_array(json)?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let author = item
            .get("author")
            .and_then(|a| a.get("login").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string();
        out.push(PrSummary {
            number: json_i64(&item, "number"),
            title: json_str(&item, "title"),
            state: json_str(&item, "state"),
            author,
            head_ref: json_str(&item, "headRefName"),
            base_ref: json_str(&item, "baseRefName"),
            url: json_str(&item, "url"),
            draft: item.get("isDraft").and_then(|x| x.as_bool()).unwrap_or(false),
            updated_at: json_str(&item, "updatedAt"),
        });
    }
    Ok(out)
}

fn parse_array(json: &str) -> Result<Vec<serde_json::Value>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let arr = v.as_array().ok_or_else(|| "expected JSON array".to_string())?;
    Ok(arr.clone())
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn json_i64(v: &serde_json::Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}
