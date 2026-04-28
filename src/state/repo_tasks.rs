use crate::domain::{Phase, RepoTask, TicketKey};
use crate::error::Result;
use crate::state::SqliteState;
use url::Url;

impl SqliteState {
    pub async fn insert_repo_task(&self, ticket: &TicketKey, rt: &RepoTask) -> Result<i64> {
        let anchors_json = serde_json::to_string(&rt.anchors)
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?;
        let result = sqlx::query(
            r#"INSERT INTO repo_tasks
               (ticket, org, repo, branch, worktree_path, anchors_json, phase,
                pr_url, review_attempts, lint_test_attempts, ci_fix_attempts)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(ticket.as_str())
        .bind(&rt.repo.org)
        .bind(&rt.repo.repo)
        .bind(&rt.branch)
        .bind(rt.worktree_path.to_string_lossy().to_string())
        .bind(&anchors_json)
        .bind(phase_str(rt.phase))
        .bind(rt.pr_url.as_ref().map(|u| u.to_string()))
        .bind(rt.review_attempts as i64)
        .bind(rt.lint_test_attempts as i64)
        .bind(rt.ci_fix_attempts as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    #[allow(dead_code)] // TUI / ops queries (later plan)
    pub async fn list_repo_tasks(&self, ticket: &TicketKey) -> Result<Vec<RepoTaskRow>> {
        let rows: Vec<RepoTaskRow> = sqlx::query_as(
            r#"SELECT id, ticket, org, repo, branch, worktree_path, anchors_json, phase,
                      pr_url, review_attempts, lint_test_attempts, ci_fix_attempts
               FROM repo_tasks WHERE ticket = ? ORDER BY id"#,
        )
        .bind(ticket.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn update_repo_task_phase(&self, id: i64, phase: Phase) -> Result<()> {
        sqlx::query(r#"UPDATE repo_tasks SET phase = ? WHERE id = ?"#)
            .bind(phase_str(phase))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn bump_attempt(&self, id: i64, kind: AttemptKind) -> Result<()> {
        const Q_REVIEW: &str =
            "UPDATE repo_tasks SET review_attempts = review_attempts + 1 WHERE id = ?";
        const Q_LINT_TEST: &str =
            "UPDATE repo_tasks SET lint_test_attempts = lint_test_attempts + 1 WHERE id = ?";
        const Q_CI_FIX: &str =
            "UPDATE repo_tasks SET ci_fix_attempts = ci_fix_attempts + 1 WHERE id = ?";
        let sql = match kind {
            AttemptKind::Review => Q_REVIEW,
            AttemptKind::LintTest => Q_LINT_TEST,
            AttemptKind::CiFix => Q_CI_FIX,
        };
        sqlx::query(sql).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_pr_url(&self, id: i64, url: &Url) -> Result<()> {
        sqlx::query(r#"UPDATE repo_tasks SET pr_url = ? WHERE id = ?"#)
            .bind(url.to_string())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[allow(dead_code)] // TUI / ops queries (later plan)
#[derive(Debug, sqlx::FromRow)]
pub struct RepoTaskRow {
    pub id: i64,
    pub ticket: String,
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub worktree_path: String,
    pub anchors_json: String,
    pub phase: String,
    pub pr_url: Option<String>,
    pub review_attempts: i64,
    pub lint_test_attempts: i64,
    pub ci_fix_attempts: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum AttemptKind {
    Review,
    LintTest,
    CiFix,
}

fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Pending => "pending",
        Phase::Planning => "planning",
        Phase::Implementing => "implementing",
        Phase::SelfReviewing => "self-reviewing",
        Phase::LintTesting => "lint-testing",
        Phase::PrOpened => "pr-opened",
        Phase::CiFixing => "ci-fixing",
        Phase::Merged => "merged",
        Phase::Aborted => "aborted",
        Phase::Escalated => "escalated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, RepoRef, WorkType};
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    async fn fresh() -> (TempDir, SqliteState, TicketKey) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("t.db");
        let st = SqliteState::open(&db).await.unwrap();
        let ticket = TicketKey::parse("ACM-9").unwrap();
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
        (dir, st, ticket)
    }

    fn sample_repo_task() -> RepoTask {
        RepoTask {
            repo: RepoRef { org: "acme".into(), repo: "core-api".into() },
            branch: "ACM-9".into(),
            worktree_path: PathBuf::from("/tmp/wt"),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        }
    }

    #[tokio::test]
    async fn insert_and_list() {
        let (_d, st, t) = fresh().await;
        st.insert_repo_task(&t, &sample_repo_task()).await.unwrap();
        let rows = st.list_repo_tasks(&t).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].repo, "core-api");
    }

    #[tokio::test]
    async fn bump_attempt_increments() {
        let (_d, st, t) = fresh().await;
        let id = st.insert_repo_task(&t, &sample_repo_task()).await.unwrap();
        st.bump_attempt(id, AttemptKind::Review).await.unwrap();
        st.bump_attempt(id, AttemptKind::Review).await.unwrap();
        let rows = st.list_repo_tasks(&t).await.unwrap();
        assert_eq!(rows[0].review_attempts, 2);
    }
}
