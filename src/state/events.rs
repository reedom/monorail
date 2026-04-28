use crate::domain::TicketKey;
use crate::error::Result;
use crate::state::SqliteState;
use chrono::Utc;
use serde::Serialize;

impl SqliteState {
    pub async fn append_event<P: Serialize>(
        &self,
        ticket: &TicketKey,
        kind: &str,
        payload: &P,
    ) -> Result<()> {
        let body = serde_json::to_string(payload)
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?;
        sqlx::query(
            r#"INSERT INTO events (ticket, kind, payload, ts) VALUES (?, ?, ?, ?)"#,
        )
        .bind(ticket.as_str())
        .bind(kind)
        .bind(body)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn count_events(&self, ticket: &TicketKey) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM events WHERE ticket = ?"#,
        )
        .bind(ticket.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, WorkType};
    use chrono::Utc;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn append_and_count() {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let t = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: t.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();

        st.append_event(&t, "phase_change", &json!({"to": "implementing"})).await.unwrap();
        st.append_event(&t, "phase_change", &json!({"to": "self-reviewing"})).await.unwrap();
        let n = st.count_events(&t).await.unwrap();
        assert_eq!(n, 2);
    }
}
