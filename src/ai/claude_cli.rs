use super::{
    AiProvider, CommitPlan, commit_plan_prompt, commit_prompt, parse_commit_plan,
};
use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::process::{Command, Stdio};

pub struct ClaudeCli {
    binary: String,
    model: Option<String>,
    extra_args: Vec<String>,
}

impl ClaudeCli {
    pub fn new(binary: String, model: Option<String>, extra_args: Vec<String>) -> Self {
        Self { binary, model, extra_args }
    }
}

impl ClaudeCli {
    fn run(&self, prompt: &str) -> Result<String> {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&self.binary);
            c
        } else {
            Command::new(&self.binary)
        };
        cmd.arg("-p").arg("--output-format").arg("text");
        if let Some(model) = &self.model {
            cmd.args(["--model", model]);
        }
        for arg in &self.extra_args {
            cmd.arg(arg);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to launch `{}`", self.binary))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .context("write prompt to claude stdin")?;
        }
        let output = child.wait_with_output().context("wait for claude")?;
        if !output.status.success() {
            return Err(anyhow!(
                "claude exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err(anyhow!("claude returned an empty response"));
        }
        Ok(text)
    }
}

impl AiProvider for ClaudeCli {
    fn generate_commit_message(&self, diff: &str) -> Result<String> {
        self.run(&commit_prompt(diff))
    }

    fn generate_commit_plan(&self, diff: &str, untracked: &[String]) -> Result<Vec<CommitPlan>> {
        let raw = self.run(&commit_plan_prompt(diff, untracked))?;
        parse_commit_plan(&raw)
    }
}
