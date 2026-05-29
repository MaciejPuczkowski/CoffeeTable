mod claude_cli;

use crate::config::AiConfig;
use anyhow::{Result, anyhow};

pub use claude_cli::ClaudeCli;

pub trait AiProvider: Send {
    fn generate_commit_message(&self, diff: &str) -> Result<String>;
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
