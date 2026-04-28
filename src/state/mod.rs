use crate::error::Result;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;

pub struct SqliteState {
    pub pool: SqlitePool,
}

impl SqliteState {
    pub async fn open(db_path: &Path) -> Result<Self> {
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let opts = SqliteConnectOptions::from_str(&url)
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?;
        Ok(Self { pool })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn opens_and_migrates() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("test.db");
        let state = SqliteState::open(&db).await.unwrap();
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM jobs")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(row.0, 0);
    }
}

pub mod events;
pub mod jobs;
pub mod repo_tasks;

pub use jobs::JobRow;
pub use repo_tasks::{AttemptKind, RepoTaskRow};
