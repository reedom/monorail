use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
}

#[async_trait]
pub trait GhTool: Send + Sync {
    async fn pr_create(&self, worktree: &Path, title: &str, body: &str) -> Result<Url>;
    async fn checks_for_pr(&self, worktree: &Path) -> Result<Vec<CheckRun>>;
    async fn check_run_log(&self, worktree: &Path, name: &str) -> Result<String>;
}

pub struct RealGh;

#[async_trait]
impl GhTool for RealGh {
    async fn pr_create(&self, worktree: &Path, title: &str, body: &str) -> Result<Url> {
        let out = Command::new("gh")
            .arg("-R").arg(worktree_repo_arg(worktree).await?)
            .args(["pr", "create", "--title", title, "--body", body, "--fill"])
            .current_dir(worktree)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "gh",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Url::parse(&s).map_err(|e| MonorailError::ExternalTool {
            tool: "gh", message: format!("could not parse url '{s}': {e}"),
        })
    }

    async fn checks_for_pr(&self, worktree: &Path) -> Result<Vec<CheckRun>> {
        let out = Command::new("gh")
            .args(["pr", "checks", "--json", "name,status,conclusion"])
            .current_dir(worktree)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "gh",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let runs: Vec<CheckRun> = serde_json::from_str(&s)
            .map_err(|e| MonorailError::ExternalTool {
                tool: "gh", message: format!("parse: {e}; raw: {s}"),
            })?;
        Ok(runs)
    }

    async fn check_run_log(&self, worktree: &Path, name: &str) -> Result<String> {
        let out = Command::new("gh")
            .args(["run", "view", "--log-failed", "--job", name])
            .current_dir(worktree)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "gh",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

async fn worktree_repo_arg(worktree: &Path) -> Result<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(worktree)
        .output().await?;
    if !out.status.success() {
        return Err(MonorailError::ExternalTool {
            tool: "git", message: String::from_utf8_lossy(&out.stderr).to_string(),
        });
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stripped = s.trim_end_matches(".git");
    if let Some(idx) = stripped.find("github.com") {
        let tail = &stripped[idx + "github.com".len()..];
        let tail = tail.trim_start_matches([':', '/']);
        return Ok(tail.to_string());
    }
    Err(MonorailError::ExternalTool {
        tool: "git", message: format!("origin not on github.com: {s}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkrun_parses_minimal() {
        let s = r#"[{"name":"build","status":"completed","conclusion":"success"}]"#;
        let v: Vec<CheckRun> = serde_json::from_str(s).unwrap();
        assert_eq!(v[0].conclusion.as_deref(), Some("success"));
    }
}
