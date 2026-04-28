use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[async_trait]
pub trait WtTool: Send + Sync {
    /// Create (or switch to) a worktree at `{repo_path}/../{repo}.{branch_sanitized}`.
    /// Returns the worktree path.
    async fn switch_create(&self, repo_path: &Path, branch: &str) -> Result<PathBuf>;

    async fn remove(&self, worktree_path: &Path) -> Result<()>;
}

pub struct RealWt;

#[async_trait]
impl WtTool for RealWt {
    async fn switch_create(&self, repo_path: &Path, branch: &str) -> Result<PathBuf> {
        let out = Command::new("wt")
            .arg("-C").arg(repo_path)
            .args(["switch", "--create", branch])
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "wt",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        let parent = repo_path.parent()
            .ok_or_else(|| MonorailError::ExternalTool {
                tool: "wt",
                message: "repo_path has no parent".into(),
            })?;
        let repo_name = repo_path.file_name()
            .ok_or_else(|| MonorailError::ExternalTool {
                tool: "wt",
                message: "repo_path has no file name".into(),
            })?
            .to_string_lossy()
            .to_string();
        let sanitized: String = branch.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        Ok(parent.join(format!("{repo_name}.{sanitized}")))
    }

    async fn remove(&self, worktree_path: &Path) -> Result<()> {
        let out = Command::new("wt")
            .arg("-C").arg(worktree_path)
            .args(["remove"])
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "wt",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sanitization_replaces_slashes() {
        let branch = "ACM-1/x";
        let s: String = branch.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        assert_eq!(s, "ACM-1-x");
    }
}
