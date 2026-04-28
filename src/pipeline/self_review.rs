use crate::domain::{EscalationReason, Phase, TicketKey};
use crate::engine::{Engine, ReviewContext};
use crate::error::Result;
use crate::state::{AttemptKind, SqliteState};
use std::path::Path;

pub const SELF_REVIEW_MAX: u8 = 5;

pub enum SelfReviewOutcome {
    Clean,
    Stuck,
    Escalated(EscalationReason),
}

pub async fn run_self_review<E: Engine + ?Sized>(
    state: &SqliteState,
    engine: &E,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
) -> Result<SelfReviewOutcome> {
    state
        .update_repo_task_phase(repo_task_id, Phase::SelfReviewing)
        .await?;
    state
        .append_event(
            ticket,
            "phase_change",
            &serde_json::json!({"to":"self-reviewing"}),
        )
        .await?;

    for attempt in 1..=SELF_REVIEW_MAX {
        let ctx = ReviewContext {
            worktree: worktree.to_path_buf(),
            ticket: ticket.as_str().to_string(),
            anchors: vec![],
        };
        let findings = engine.review(ctx.clone()).await?;
        if findings.is_empty() {
            state
                .append_event(
                    ticket,
                    "review_clean",
                    &serde_json::json!({"attempt":attempt}),
                )
                .await?;
            return Ok(SelfReviewOutcome::Clean);
        }

        let mut actionable_fix = false;
        for finding in findings {
            let analysis = engine
                .analyze_finding(finding.clone(), ctx.clone())
                .await?;
            if analysis.requires_fix {
                let outcome = engine.apply_fix(analysis.clone(), ctx.clone()).await?;
                if outcome.applied {
                    actionable_fix = true;
                }
            } else {
                state
                    .append_event(
                        ticket,
                        "finding_dismissed",
                        &serde_json::json!({
                            "finding_id": finding.id,
                            "reason": analysis.reason,
                        }),
                    )
                    .await?;
            }
        }

        state
            .bump_attempt(repo_task_id, AttemptKind::Review)
            .await?;

        if !actionable_fix {
            state
                .append_event(
                    ticket,
                    "review_stuck",
                    &serde_json::json!({"attempt":attempt}),
                )
                .await?;
            return Ok(SelfReviewOutcome::Stuck);
        }
        if attempt == SELF_REVIEW_MAX {
            return Ok(SelfReviewOutcome::Escalated(
                EscalationReason::SelfReviewMaxed,
            ));
        }
    }
    Ok(SelfReviewOutcome::Escalated(
        EscalationReason::SelfReviewMaxed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Finding, FixOutcome, Job, JobState, RepoRef, RepoTask, RootCauseAnalysis, Severity,
        WorkType,
    };
    use crate::engine::MockEngine;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    async fn fresh() -> (TempDir, SqliteState, TicketKey, i64) {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef {
                org: "a".into(),
                repo: "b".into(),
            },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        (dir, st, ticket, id)
    }

    fn one_finding() -> Finding {
        Finding {
            id: "f1".into(),
            file: "a.rs".into(),
            line: Some(1),
            severity: Severity::High,
            rule: None,
            message: "x".into(),
        }
    }

    #[tokio::test]
    async fn empty_findings_first_pass_returns_clean() {
        let (_d, st, t, id) = fresh().await;
        let mut engine = MockEngine::new();
        engine
            .expect_review()
            .returning(|_| Box::pin(async { Ok(vec![]) }));
        let out = run_self_review(&st, &engine, &t, id, &PathBuf::from("/tmp"))
            .await
            .unwrap();
        assert!(matches!(out, SelfReviewOutcome::Clean));
    }

    #[tokio::test]
    async fn dismissed_only_returns_stuck_after_one_attempt() {
        let (_d, st, t, id) = fresh().await;
        let mut engine = MockEngine::new();
        engine
            .expect_review()
            .returning(|_| Box::pin(async { Ok(vec![one_finding()]) }));
        engine.expect_analyze_finding().returning(|f, _| {
            Box::pin(async move {
                Ok(RootCauseAnalysis {
                    finding_id: f.id,
                    requires_fix: false,
                    reason: "intentional".into(),
                })
            })
        });
        let out = run_self_review(&st, &engine, &t, id, &PathBuf::from("/tmp"))
            .await
            .unwrap();
        assert!(matches!(out, SelfReviewOutcome::Stuck));
        let rows = st.list_repo_tasks(&t).await.unwrap();
        assert_eq!(rows[0].review_attempts, 1);
    }

    #[tokio::test]
    async fn fixed_then_clean_returns_clean() {
        let (_d, st, t, id) = fresh().await;
        let mut engine = MockEngine::new();
        let counter = std::sync::Arc::new(Mutex::new(0_u32));
        let counter2 = counter.clone();
        engine.expect_review().returning(move |_| {
            let counter = counter2.clone();
            Box::pin(async move {
                let mut c = counter.lock().unwrap();
                *c += 1;
                if *c == 1 {
                    Ok(vec![one_finding()])
                } else {
                    Ok(vec![])
                }
            })
        });
        engine.expect_analyze_finding().returning(|f, _| {
            Box::pin(async move {
                Ok(RootCauseAnalysis {
                    finding_id: f.id,
                    requires_fix: true,
                    reason: "fix".into(),
                })
            })
        });
        engine.expect_apply_fix().returning(|_, _| {
            Box::pin(async {
                Ok(FixOutcome {
                    applied: true,
                    message: "ok".into(),
                })
            })
        });
        let out = run_self_review(&st, &engine, &t, id, &PathBuf::from("/tmp"))
            .await
            .unwrap();
        assert!(matches!(out, SelfReviewOutcome::Clean));
    }
}
