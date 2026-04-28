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

    async fn review(&self, ctx: ReviewContext) -> Result<Vec<Finding>> {
        let prompt = format!(
            "Run /pr-review-toolkit:review-pr against the current worktree changes for ticket {ticket}. \
             At the end, output a JSON array of findings on a single line prefixed with `FINDINGS_JSON: `. \
             Each finding has: id (stable hash of file+line+rule), file, line (or null), \
             severity (critical|high|medium|low|info), rule (or null), message.",
            ticket = ctx.ticket,
        );
        let raw = self.run(&ctx.worktree, &prompt).await?;
        let line = raw.lines().rev()
            .find(|l| l.contains("FINDINGS_JSON:"))
            .ok_or_else(|| MonorailError::PhaseAborted("no FINDINGS_JSON line in review output".into()))?;
        let json_part = line.split_once("FINDINGS_JSON:")
            .map(|(_, j)| j.trim())
            .unwrap_or("[]");
        let findings: Vec<Finding> = serde_json::from_str(json_part)
            .map_err(|e| MonorailError::Serde(format!("findings parse: {e}; raw: {json_part}")))?;
        Ok(findings)
    }

    async fn analyze_finding(
        &self,
        finding: Finding,
        ctx: ReviewContext,
    ) -> Result<RootCauseAnalysis> {
        let prompt = format!(
            "Analyze the root cause of the review finding below in the worktree at {wt} for ticket {ticket}. \
             Decide: does this finding REQUIRE a fix, or can it be dismissed (e.g., intentional, false positive)? \
             Output exactly two lines:\n\
             DECISION: <fix|dismiss>\n\
             REASON: <one sentence>\n\n\
             Finding:\n{f}",
            wt = ctx.worktree.display(),
            ticket = ctx.ticket,
            f = serde_json::to_string_pretty(&finding).unwrap_or_default(),
        );
        let out = self.run(&ctx.worktree, &prompt).await?;
        let mut decision = None;
        let mut reason = String::new();
        for line in out.lines() {
            if let Some(rest) = line.strip_prefix("DECISION:") {
                decision = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("REASON:") {
                reason = rest.trim().to_string();
            }
        }
        let requires_fix = matches!(decision.as_deref(), Some("fix"));
        Ok(RootCauseAnalysis {
            finding_id: finding.id,
            requires_fix,
            reason,
        })
    }

    async fn apply_fix(
        &self,
        analysis: RootCauseAnalysis,
        ctx: ReviewContext,
    ) -> Result<FixOutcome> {
        let prompt = format!(
            "Apply the fix for finding id={fid} in the worktree at {wt}. \
             Reason from analysis: {reason}. \
             Do NOT edit files outside this worktree. \
             Reply with exactly one line: APPLIED or NOT_APPLIED, then a short reason on the same line.",
            fid = analysis.finding_id,
            wt = ctx.worktree.display(),
            reason = analysis.reason,
        );
        let out = self.run(&ctx.worktree, &prompt).await?;
        let applied = out.contains("APPLIED") && !out.contains("NOT_APPLIED");
        Ok(FixOutcome { applied, message: out })
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

    #[test]
    fn parses_findings_json_line() {
        let raw = "preamble\nMore text\nFINDINGS_JSON: [\
            {\"id\":\"f1\",\"file\":\"a.rs\",\"line\":10,\"severity\":\"high\",\"rule\":null,\"message\":\"x\"}]";
        let line = raw.lines().rev().find(|l| l.contains("FINDINGS_JSON:")).unwrap();
        let part = line.split_once("FINDINGS_JSON:").unwrap().1.trim();
        let v: Vec<Finding> = serde_json::from_str(part).unwrap();
        assert_eq!(v[0].id, "f1");
    }
}
