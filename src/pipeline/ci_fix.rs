use crate::domain::{EscalationReason, Phase, TicketKey};
use crate::engine::{Engine, FailureContext, FailureKind};
use crate::error::Result;
use crate::state::{AttemptKind, SqliteState};
use crate::tools::GhTool;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

pub const CI_FIX_MAX: u8 = 3;

pub enum CiFixOutcome {
    Green,
    Escalated(EscalationReason),
}

pub async fn run_ci_fix<E: Engine + ?Sized, G: GhTool + ?Sized>(
    state: &SqliteState,
    engine: &E,
    gh: &G,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
    poll_interval: Duration,
) -> Result<CiFixOutcome> {
    state.update_repo_task_phase(repo_task_id, Phase::CiFixing).await?;
    state.append_event(ticket, "phase_change", &serde_json::json!({"to":"ci-fixing"})).await?;

    for attempt in 1..=CI_FIX_MAX {
        let verdict = wait_for_ci(gh, worktree, poll_interval).await?;
        match verdict {
            CiVerdict::Green => {
                state.append_event(ticket, "ci_green", &serde_json::json!({"attempt":attempt})).await?;
                return Ok(CiFixOutcome::Green);
            }
            CiVerdict::Failed { failed_jobs } => {
                state.bump_attempt(repo_task_id, AttemptKind::CiFix).await?;
                let mut log = String::new();
                for name in &failed_jobs {
                    if let Ok(part) = gh.check_run_log(worktree, name).await {
                        log.push_str(&format!("---- {name} ----\n{part}\n"));
                    }
                }
                let outcome = engine.fix_failure(FailureContext {
                    worktree: worktree.to_path_buf(),
                    ticket: ticket.as_str().to_string(),
                    failure_log: log,
                    kind: FailureKind::Ci,
                }).await?;
                if !outcome.applied {
                    state.append_event(ticket, "ci_fix_no_fix", &serde_json::json!({"attempt":attempt})).await?;
                    return Ok(CiFixOutcome::Escalated(EscalationReason::CiFixMaxed));
                }
            }
        }
    }
    Ok(CiFixOutcome::Escalated(EscalationReason::CiFixMaxed))
}

enum CiVerdict {
    Green,
    Failed { failed_jobs: Vec<String> },
}

async fn wait_for_ci<G: GhTool + ?Sized>(
    gh: &G,
    worktree: &Path,
    poll_interval: Duration,
) -> Result<CiVerdict> {
    loop {
        let runs = gh.checks_for_pr(worktree).await?;
        if runs.is_empty() {
            sleep(poll_interval).await;
            continue;
        }
        let all_completed = runs.iter().all(|r| r.status == "completed");
        if !all_completed {
            sleep(poll_interval).await;
            continue;
        }
        let failed: Vec<String> = runs.iter()
            .filter(|r| r.conclusion.as_deref() != Some("success"))
            .map(|r| r.name.clone())
            .collect();
        if failed.is_empty() {
            return Ok(CiVerdict::Green);
        } else {
            return Ok(CiVerdict::Failed { failed_jobs: failed });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FixOutcome, Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::engine::MockEngine;
    use crate::tools::CheckRun;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;
    use url::Url;

    struct ScriptedGh { steps: Mutex<Vec<Vec<CheckRun>>> }
    #[async_trait]
    impl GhTool for ScriptedGh {
        async fn pr_create(&self, _w: &Path, _t: &str, _b: &str) -> Result<Url> { unimplemented!() }
        async fn checks_for_pr(&self, _w: &Path) -> Result<Vec<CheckRun>> {
            Ok(self.steps.lock().unwrap().remove(0))
        }
        async fn check_run_log(&self, _w: &Path, _n: &str) -> Result<String> {
            Ok("failure log".into())
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
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::PrOpened,
            pr_url: Some(Url::parse("https://example.com").unwrap()),
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        (dir, st, ticket, id)
    }

    #[tokio::test]
    async fn green_on_first_check_returns_green() {
        let (_d, st, t, id) = fresh().await;
        let gh = ScriptedGh { steps: Mutex::new(vec![
            vec![CheckRun { name: "build".into(), status: "completed".into(), conclusion: Some("success".into()) }]
        ])};
        let engine = MockEngine::new();
        let out = run_ci_fix(&st, &engine, &gh, &t, id, &PathBuf::from("/tmp"), Duration::from_millis(0))
            .await.unwrap();
        assert!(matches!(out, CiFixOutcome::Green));
    }

    #[tokio::test]
    async fn failure_then_green_after_fix() {
        let (_d, st, t, id) = fresh().await;
        let gh = ScriptedGh { steps: Mutex::new(vec![
            vec![CheckRun { name: "build".into(), status: "completed".into(), conclusion: Some("failure".into()) }],
            vec![CheckRun { name: "build".into(), status: "completed".into(), conclusion: Some("success".into()) }],
        ])};
        let mut engine = MockEngine::new();
        engine.expect_fix_failure()
            .returning(|_| Box::pin(async { Ok(FixOutcome { applied: true, message: "fixed".into() }) }));
        let out = run_ci_fix(&st, &engine, &gh, &t, id, &PathBuf::from("/tmp"), Duration::from_millis(0))
            .await.unwrap();
        assert!(matches!(out, CiFixOutcome::Green));
    }
}
