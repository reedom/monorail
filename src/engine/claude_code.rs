use crate::domain::{Finding, FixOutcome, RootCauseAnalysis};
use crate::engine::{
    Engine, FailureContext, FailureKind, ImplContext, ImplResult, ReviewContext,
};
use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct ClaudeCodeAdapter {
    pub binary: String,
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self { binary: "claude".to_string() }
    }
}

impl ClaudeCodeAdapter {
    async fn run(&self, cwd: &Path, prompt: &str) -> Result<String> {
        let out = Command::new(&self.binary)
            .args(["-p", prompt, "--output-format", "text"])
            .current_dir(cwd)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "claude",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

#[async_trait]
impl Engine for ClaudeCodeAdapter {
    async fn implement(&self, ctx: ImplContext) -> Result<ImplResult> {
        let prompt = format!(
            "You are working on Linear ticket {ticket} inside the worktree at {wt}. \
             Make the necessary code changes to satisfy the request below. \
             Do NOT edit files outside this worktree. When done, respond with a brief summary.\n\n\
             Instructions:\n{instr}",
            ticket = ctx.ticket,
            wt = ctx.worktree.display(),
            instr = ctx.instructions,
        );
        let summary = self.run(&ctx.worktree, &prompt).await?;
        Ok(ImplResult { summary })
    }

    async fn review(&self, _ctx: ReviewContext) -> Result<Vec<Finding>> {
        Err(MonorailError::PhaseAborted("review unimplemented in Task 15".into()))
    }
    async fn analyze_finding(&self, _f: Finding, _c: ReviewContext) -> Result<RootCauseAnalysis> {
        Err(MonorailError::PhaseAborted("analyze_finding unimplemented in Task 15".into()))
    }
    async fn apply_fix(&self, _a: RootCauseAnalysis, _c: ReviewContext) -> Result<FixOutcome> {
        Err(MonorailError::PhaseAborted("apply_fix unimplemented in Task 15".into()))
    }

    async fn fix_failure(&self, ctx: FailureContext) -> Result<FixOutcome> {
        let kind = match ctx.kind {
            FailureKind::LintTest => "lint or test",
            FailureKind::Ci => "CI",
        };
        let prompt = format!(
            "The {kind} run for ticket {ticket} failed. The failure log is below. \
             Investigate root cause and apply the minimal fix in this worktree at {wt}. \
             Do NOT edit files outside this worktree. \
             Reply with one line: APPLIED or NOT_APPLIED, then a short reason.\n\n\
             Failure log:\n{log}",
            kind = kind,
            ticket = ctx.ticket,
            wt = ctx.worktree.display(),
            log = ctx.failure_log,
        );
        let out = self.run(&ctx.worktree, &prompt).await?;
        let applied = out.contains("APPLIED") && !out.contains("NOT_APPLIED");
        Ok(FixOutcome { applied, message: out })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn implement_reports_external_tool_error_when_claude_missing() {
        let adapter = ClaudeCodeAdapter { binary: "/no/such/binary/claude-zzz".into() };
        let ctx = ImplContext {
            worktree: PathBuf::from("/tmp"),
            ticket: "ACM-1".into(),
            instructions: "do nothing".into(),
            anchors: vec![],
        };
        let err = adapter.implement(ctx).await.unwrap_err();
        match err {
            MonorailError::Io(_) | MonorailError::ExternalTool { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
