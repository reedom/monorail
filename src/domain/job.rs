use crate::domain::{JobState, Phase, TicketKey, WorkType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub org: String,
    pub repo: String,
}

impl RepoRef {
    pub fn full(&self) -> String {
        format!("{}/{}", self.org, self.repo)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoTask {
    pub repo: RepoRef,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub anchors: Vec<PathBuf>,
    pub phase: Phase,
    pub pr_url: Option<Url>,
    pub review_attempts: u8,
    pub lint_test_attempts: u8,
    pub ci_fix_attempts: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub ticket: TicketKey,
    pub work_type: WorkType,
    pub state: JobState,
    pub repos: Vec<RepoTask>,
    pub auto_merge: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_ref_full_format() {
        let r = RepoRef { org: "acme".into(), repo: "core-api".into() };
        assert_eq!(r.full(), "acme/core-api");
    }

    #[test]
    fn job_round_trips_json() {
        let j = Job {
            ticket: TicketKey::parse("ACM-1").unwrap(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let s = serde_json::to_string(&j).unwrap();
        let j2: Job = serde_json::from_str(&s).unwrap();
        assert_eq!(j.ticket, j2.ticket);
    }
}
