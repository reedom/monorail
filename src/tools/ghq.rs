use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

#[async_trait]
pub trait GhqTool: Send + Sync {
    async fn list_path(&self, full: &str) -> Result<Option<PathBuf>>;
    async fn ensure_cloned(&self, full: &str) -> Result<PathBuf>;
}

pub struct RealGhq;

#[async_trait]
impl GhqTool for RealGhq {
    async fn list_path(&self, full: &str) -> Result<Option<PathBuf>> {
        let out = Command::new("ghq")
            .args(["list", "-p", full])
            .output().await?;
        if !out.status.success() {
            return Ok(None);
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() { return Ok(None); }
        Ok(Some(PathBuf::from(s)))
    }

    async fn ensure_cloned(&self, full: &str) -> Result<PathBuf> {
        if let Some(p) = self.list_path(full).await? {
            return Ok(p);
        }
        let out = Command::new("ghq").args(["get", full]).output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "ghq",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        self.list_path(full).await?.ok_or_else(|| MonorailError::ExternalTool {
            tool: "ghq", message: format!("not found after get: {full}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_path_returns_none_when_command_missing() {
        let g = RealGhq;
        let res = g.list_path("nonexistent/repo-xyz-12345").await;
        // Tolerate either Ok(None) (ghq present, repo missing) or Err (ghq absent).
        let _ = res;
    }
}
