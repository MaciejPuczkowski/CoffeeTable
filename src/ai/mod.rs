mod claude_cli;

use crate::config::AiConfig;
use anyhow::{Result, anyhow};

pub use claude_cli::ClaudeCli;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CommitPlan {
    pub message: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct CommitPlanWrapper {
    commits: Vec<CommitPlan>,
}

pub trait AiProvider: Send {
    fn generate_commit_message(&self, diff: &str) -> Result<String>;
    fn generate_commit_plan(&self, diff: &str, untracked: &[String]) -> Result<Vec<CommitPlan>>;
}

pub fn parse_commit_plan(raw: &str) -> Result<Vec<CommitPlan>> {
    let trimmed = raw.trim();
    let json_start = trimmed.find('{');
    let json_end = trimmed.rfind('}');
    let json_slice = match (json_start, json_end) {
        (Some(a), Some(b)) if b >= a => &trimmed[a..=b],
        _ => trimmed,
    };
    let wrapper: CommitPlanWrapper = serde_json::from_str(json_slice)
        .map_err(|e| anyhow!("could not parse JSON commit plan: {}. Raw: {}", e, raw))?;
    if wrapper.commits.is_empty() {
        return Err(anyhow!("AI returned an empty commit plan"));
    }
    Ok(wrapper.commits)
}

pub fn build_provider(cfg: &AiConfig) -> Result<Box<dyn AiProvider>> {
    match cfg.provider.as_str() {
        "claude_cli" => Ok(Box::new(ClaudeCli::new(
            cfg.binary.clone(),
            cfg.model.clone(),
            cfg.extra_args.clone(),
        ))),
        other => Err(anyhow!(
            "Unknown AI provider '{}'. Only 'claude_cli' is wired in (API providers planned).",
            other
        )),
    }
}

pub fn commit_prompt(diff: &str) -> String {
    format!(
        "You are writing a git commit message for the staged diff below. \
Output ONLY the message text — no markdown, no quotes, no commentary. \
Use the imperative mood. Keep the subject line under 72 characters. \
If the change is non-trivial, follow the subject with a blank line and a short body.\n\n\
Staged diff:\n{}\n",
        diff
    )
}

pub fn commit_plan_prompt(diff: &str, untracked: &[String]) -> String {
    let untracked_text = if untracked.is_empty() {
        String::from("(none)")
    } else {
        untracked.join("\n")
    };
    format!(
        "You are organizing git commits. Below is the current unstaged diff and a list of \
untracked files (relative to the repo root). Propose how to split these changes into \
separate logical commits — typically 1 to 5 commits, each with a coherent purpose.\n\n\
Output ONLY a JSON object with this exact schema, no markdown fences, no commentary:\n\n\
{{\"commits\":[\n  {{\"message\":\"subject line\\n\\noptional body\",\"files\":[\"path/to/file1\"]}}\n]}}\n\n\
Rules:\n\
- File paths must be relative to the repo root, exactly as they appear below.\n\
- Each file must appear in at most one commit.\n\
- Use imperative mood; keep subject lines under 72 characters.\n\
- If a non-trivial body is warranted, include it in the message field (separated by a blank line).\n\n\
Unstaged diff:\n{}\n\n\
Untracked files:\n{}\n",
        diff, untracked_text
    )
}
