use crate::domain::{Finding, FixOutcome, RootCauseAnalysis};
use crate::error::Result;
use async_trait::async_trait;
use mockall::automock;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ImplContext {
    pub worktree: PathBuf,
    pub ticket: String,
    pub instructions: String,
    pub anchors: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ImplResult {
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub worktree: PathBuf,
    pub ticket: String,
    pub anchors: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct FailureContext {
    pub worktree: PathBuf,
    pub ticket: String,
    pub failure_log: String,
    pub kind: FailureKind,
}

#[derive(Debug, Clone, Copy)]
pub enum FailureKind {
    LintTest,
    Ci,
}

#[async_trait]
#[automock]
pub trait Engine: Send + Sync {
    async fn implement(&self, ctx: ImplContext) -> Result<ImplResult>;
    async fn review(&self, ctx: ReviewContext) -> Result<Vec<Finding>>;
    async fn analyze_finding(
        &self,
        finding: Finding,
        ctx: ReviewContext,
    ) -> Result<RootCauseAnalysis>;
    async fn apply_fix(
        &self,
        analysis: RootCauseAnalysis,
        ctx: ReviewContext,
    ) -> Result<FixOutcome>;
    async fn fix_failure(&self, ctx: FailureContext) -> Result<FixOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Severity;

    #[tokio::test]
    async fn mock_engine_returns_no_findings() {
        let mut m = MockEngine::new();
        m.expect_review()
            .returning(|_| Box::pin(async { Ok(vec![]) }));
        let ctx = ReviewContext {
            worktree: PathBuf::from("/tmp"),
            ticket: "ACM-1".into(),
            anchors: vec![],
        };
        let f = m.review(ctx).await.unwrap();
        assert!(f.is_empty());
    }

    #[tokio::test]
    async fn mock_engine_root_cause_dismisses() {
        let mut m = MockEngine::new();
        m.expect_analyze_finding().returning(|f, _| {
            Box::pin(async move {
                Ok(RootCauseAnalysis {
                    finding_id: f.id,
                    requires_fix: false,
                    reason: "intentional".into(),
                })
            })
        });
        let ctx = ReviewContext { worktree: PathBuf::from("/"), ticket: "ACM-1".into(), anchors: vec![] };
        let finding = Finding {
            id: "f1".into(), file: "x.rs".into(), line: None,
            severity: Severity::Medium, rule: None, message: "msg".into(),
        };
        let a = m.analyze_finding(finding, ctx).await.unwrap();
        assert!(!a.requires_fix);
    }
}
