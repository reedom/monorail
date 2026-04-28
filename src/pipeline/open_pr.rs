use crate::domain::{Phase, TicketKey};
use crate::error::Result;
use crate::state::SqliteState;
use crate::tools::GhTool;
use std::path::Path;
use url::Url;

pub async fn run_open_pr<G: GhTool + ?Sized>(
    state: &SqliteState,
    gh: &G,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
    title: &str,
    body: &str,
) -> Result<Url> {
    let url = gh.pr_create(worktree, title, body).await?;
    state.set_pr_url(repo_task_id, &url).await?;
    state
        .update_repo_task_phase(repo_task_id, Phase::PrOpened)
        .await?;
    state
        .append_event(
            ticket,
            "pr_opened",
            &serde_json::json!({"url": url.to_string()}),
        )
        .await?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::tools::CheckRun;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct StubGh;
    #[async_trait]
    impl GhTool for StubGh {
        async fn pr_create(&self, _w: &Path, _t: &str, _b: &str) -> Result<Url> {
            Ok(Url::parse("https://github.com/acme/core-api/pull/123").unwrap())
        }
        async fn checks_for_pr(&self, _w: &Path) -> Result<Vec<CheckRun>> {
            Ok(vec![])
        }
        async fn check_run_log(&self, _w: &Path, _n: &str) -> Result<String> {
            Ok("".into())
        }
    }

    #[tokio::test]
    async fn opens_pr_and_persists_url() {
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
            phase: Phase::LintTesting,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        let url = run_open_pr(
            &st,
            &StubGh,
            &ticket,
            id,
            &PathBuf::from("/tmp"),
            "title",
            "body",
        )
        .await
        .unwrap();
        assert_eq!(url.path(), "/acme/core-api/pull/123");
        let rows = st.list_repo_tasks(&ticket).await.unwrap();
        assert_eq!(rows[0].phase, "pr-opened");
        assert!(rows[0].pr_url.is_some());
    }
}
