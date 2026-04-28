use crate::domain::{EscalationReason, Phase, TicketKey};
use crate::engine::{Engine, FailureContext, FailureKind};
use crate::error::Result;
use crate::state::{AttemptKind, SqliteState};
use std::path::Path;

pub const LINT_TEST_MAX: u8 = 5;

pub enum LintTestOutcome {
    Green,
    Escalated(EscalationReason),
}

#[async_trait::async_trait]
pub trait Verifier: Send + Sync {
    async fn verify(&self, worktree: &Path) -> std::result::Result<(), String>;
}

pub async fn run_lint_test<E: Engine + ?Sized, V: Verifier + ?Sized>(
    state: &SqliteState,
    engine: &E,
    verifier: &V,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
) -> Result<LintTestOutcome> {
    state
        .update_repo_task_phase(repo_task_id, Phase::LintTesting)
        .await?;
    state
        .append_event(
            ticket,
            "phase_change",
            &serde_json::json!({"to":"lint-testing"}),
        )
        .await?;

    for attempt in 1..=LINT_TEST_MAX {
        match verifier.verify(worktree).await {
            Ok(()) => {
                state
                    .append_event(
                        ticket,
                        "verify_green",
                        &serde_json::json!({"attempt":attempt}),
                    )
                    .await?;
                return Ok(LintTestOutcome::Green);
            }
            Err(log) => {
                state
                    .bump_attempt(repo_task_id, AttemptKind::LintTest)
                    .await?;
                let outcome = engine
                    .fix_failure(FailureContext {
                        worktree: worktree.to_path_buf(),
                        ticket: ticket.as_str().to_string(),
                        failure_log: log,
                        kind: FailureKind::LintTest,
                    })
                    .await?;
                if !outcome.applied {
                    state
                        .append_event(
                            ticket,
                            "lint_test_no_fix",
                            &serde_json::json!({"attempt":attempt, "msg": outcome.message}),
                        )
                        .await?;
                    return Ok(LintTestOutcome::Escalated(EscalationReason::LintTestMaxed));
                }
            }
        }
    }
    Ok(LintTestOutcome::Escalated(EscalationReason::LintTestMaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FixOutcome, Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::engine::MockEngine;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct StubVerifier {
        fail_first: Mutex<u32>,
    }
    #[async_trait::async_trait]
    impl Verifier for StubVerifier {
        async fn verify(&self, _: &Path) -> std::result::Result<(), String> {
            let mut c = self.fail_first.lock().unwrap();
            if *c == 0 {
                Ok(())
            } else {
                *c -= 1;
                Err("boom".into())
            }
        }
    }

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

    #[tokio::test]
    async fn green_first_try() {
        let (_d, st, t, id) = fresh().await;
        let v = StubVerifier {
            fail_first: Mutex::new(0),
        };
        let engine = MockEngine::new();
        let out = run_lint_test(&st, &engine, &v, &t, id, &PathBuf::from("/tmp"))
            .await
            .unwrap();
        assert!(matches!(out, LintTestOutcome::Green));
    }

    #[tokio::test]
    async fn fixes_then_green() {
        let (_d, st, t, id) = fresh().await;
        let v = StubVerifier {
            fail_first: Mutex::new(1),
        };
        let mut engine = MockEngine::new();
        engine.expect_fix_failure().returning(|_| {
            Box::pin(async {
                Ok(FixOutcome {
                    applied: true,
                    message: "k".into(),
                })
            })
        });
        let out = run_lint_test(&st, &engine, &v, &t, id, &PathBuf::from("/tmp"))
            .await
            .unwrap();
        assert!(matches!(out, LintTestOutcome::Green));
    }

    #[tokio::test]
    async fn no_fix_applied_escalates_immediately() {
        let (_d, st, t, id) = fresh().await;
        let v = StubVerifier {
            fail_first: Mutex::new(5),
        };
        let mut engine = MockEngine::new();
        engine.expect_fix_failure().returning(|_| {
            Box::pin(async {
                Ok(FixOutcome {
                    applied: false,
                    message: "no".into(),
                })
            })
        });
        let out = run_lint_test(&st, &engine, &v, &t, id, &PathBuf::from("/tmp"))
            .await
            .unwrap();
        assert!(matches!(
            out,
            LintTestOutcome::Escalated(EscalationReason::LintTestMaxed)
        ));
    }
}
