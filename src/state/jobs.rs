use crate::domain::{Job, JobState, TicketKey, WorkType};
use crate::error::Result;
use crate::state::SqliteState;
use chrono::Utc;

impl SqliteState {
    pub async fn insert_job(&self, job: &Job) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO jobs (ticket, work_type, state, auto_merge, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(job.ticket.as_str())
        .bind(work_type_str(job.work_type))
        .bind(state_str(job.state))
        .bind(job.auto_merge as i64)
        .bind(job.created_at.to_rfc3339())
        .bind(job.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(dead_code)] // TUI / ops queries (later plan)
    pub async fn get_job(&self, ticket: &TicketKey) -> Result<Option<JobRow>> {
        let row: Option<JobRow> = sqlx::query_as(
            r#"SELECT ticket, work_type, state, auto_merge, created_at, updated_at
               FROM jobs WHERE ticket = ?"#,
        )
        .bind(ticket.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn update_job_state(&self, ticket: &TicketKey, new_state: JobState) -> Result<()> {
        sqlx::query(
            r#"UPDATE jobs SET state = ?, updated_at = ? WHERE ticket = ?"#,
        )
        .bind(state_str(new_state))
        .bind(Utc::now().to_rfc3339())
        .bind(ticket.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[allow(dead_code)] // TUI / ops queries (later plan)
#[derive(Debug, sqlx::FromRow)]
pub struct JobRow {
    pub ticket: String,
    pub work_type: String,
    pub state: String,
    pub auto_merge: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn work_type_str(w: WorkType) -> &'static str {
    match w { WorkType::Bug => "bug", WorkType::Feature => "feature" }
}
fn state_str(s: JobState) -> &'static str {
    match s {
        JobState::Active => "active",
        JobState::Escalated => "escalated",
        JobState::Done => "done",
        JobState::Aborted => "aborted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TicketKey;
    use tempfile::TempDir;

    async fn fresh_state() -> (TempDir, SqliteState) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("t.db");
        (dir, SqliteState::open(&db).await.unwrap())
    }

    #[tokio::test]
    async fn insert_then_get_returns_row() {
        let (_d, st) = fresh_state().await;
        let job = Job {
            ticket: TicketKey::parse("ACM-7").unwrap(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let row = st.get_job(&job.ticket).await.unwrap().unwrap();
        assert_eq!(row.ticket, "ACM-7");
        assert_eq!(row.work_type, "bug");
        assert_eq!(row.state, "active");
    }

    #[tokio::test]
    async fn update_state_persists() {
        let (_d, st) = fresh_state().await;
        let job = Job {
            ticket: TicketKey::parse("ACM-8").unwrap(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        st.update_job_state(&job.ticket, JobState::Done).await.unwrap();
        let row = st.get_job(&job.ticket).await.unwrap().unwrap();
        assert_eq!(row.state, "done");
    }
}
