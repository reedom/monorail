use crate::channel::{HumanChannel, NotifyContext};
use crate::domain::{EscalationReason, JobState, Phase, TicketKey};
use crate::error::Result;
use crate::state::SqliteState;

pub async fn escalate<C: HumanChannel + ?Sized>(
    state: &SqliteState,
    channel: &C,
    ticket: &TicketKey,
    repo_task_id: i64,
    reason: EscalationReason,
    summary: &str,
) -> Result<()> {
    state.update_repo_task_phase(repo_task_id, Phase::Escalated).await?;
    state.update_job_state(ticket, JobState::Escalated).await?;
    state.append_event(ticket, "escalated", &serde_json::json!({
        "reason": reason,
        "summary": summary,
    })).await?;
    let body = format!(
        "monorail needs help: {reason:?}\n\nSummary:\n{summary}",
    );
    channel.notify(NotifyContext { ticket: ticket.clone(), body }).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::HumanChannel;
    use crate::domain::{Job, JobState, Question, RepoRef, RepoTask, WorkType};
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct CapturingChannel { calls: Mutex<Vec<NotifyContext>> }
    #[async_trait]
    impl HumanChannel for CapturingChannel {
        async fn notify(&self, ctx: NotifyContext) -> Result<()> {
            self.calls.lock().unwrap().push(ctx);
            Ok(())
        }
        async fn post_question(&self, _q: Question) -> Result<String> { unimplemented!() }
    }

    #[tokio::test]
    async fn escalate_sets_state_and_notifies() {
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
            phase: Phase::SelfReviewing,
            pr_url: None,
            review_attempts: 5,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();

        let channel = CapturingChannel { calls: Mutex::new(vec![]) };
        escalate(&st, &channel, &ticket, id, EscalationReason::SelfReviewMaxed, "stuck").await.unwrap();

        let row = st.get_job(&ticket).await.unwrap().unwrap();
        assert_eq!(row.state, "escalated");
        let rows = st.list_repo_tasks(&ticket).await.unwrap();
        assert_eq!(rows[0].phase, "escalated");
        assert_eq!(channel.calls.lock().unwrap().len(), 1);
    }
}
