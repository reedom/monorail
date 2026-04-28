use crate::domain::{Phase, TicketKey};
use crate::engine::{Engine, ImplContext};
use crate::error::Result;
use crate::state::SqliteState;
use std::path::Path;

pub async fn run_implement<E: Engine + ?Sized>(
    state: &SqliteState,
    engine: &E,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
    instructions: &str,
) -> Result<()> {
    state
        .update_repo_task_phase(repo_task_id, Phase::Implementing)
        .await?;
    state
        .append_event(
            ticket,
            "phase_change",
            &serde_json::json!({"to":"implementing"}),
        )
        .await?;

    let ctx = ImplContext {
        worktree: worktree.to_path_buf(),
        ticket: ticket.as_str().to_string(),
        instructions: instructions.to_string(),
        anchors: vec![],
    };
    let result = engine.implement(ctx).await?;
    state
        .append_event(
            ticket,
            "implement_done",
            &serde_json::json!({
                "summary": result.summary,
            }),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::engine::{ImplResult, MockEngine};
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[tokio::test]
    async fn implement_advances_phase_and_records_event() {
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

        let mut engine = MockEngine::new();
        engine.expect_implement().returning(|_| {
            Box::pin(async {
                Ok(ImplResult {
                    summary: "ok".into(),
                })
            })
        });

        run_implement(&st, &engine, &ticket, id, &PathBuf::from("/tmp"), "do it")
            .await
            .unwrap();

        let rows = st.list_repo_tasks(&ticket).await.unwrap();
        assert_eq!(rows[0].phase, "implementing");
        assert_eq!(st.count_events(&ticket).await.unwrap(), 2);
    }
}
