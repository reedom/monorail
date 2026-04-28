use crate::domain::{Job, JobState, Phase, RepoRef, RepoTask, TicketKey, WorkType};
use crate::error::{MonorailError, Result};
use crate::linear::{Issue, LinearClient};
use chrono::Utc;

pub struct Triager<'a> {
    pub linear: &'a LinearClient,
}

const LABEL_BUG: &str = "monorail:type/bug";
const LABEL_FEATURE: &str = "monorail:type/feature";
const LABEL_AUTO_MERGE: &str = "monorail:auto-merge";

impl<'a> Triager<'a> {
    pub async fn build_job(&self, ticket: &TicketKey) -> Result<Job> {
        let issue = self.linear.get_issue(ticket.as_str()).await?;
        let work_type = classify_labels(&issue)?;
        let auto_merge = issue.labels.iter().any(|l| l.name == LABEL_AUTO_MERGE);
        let (org, repo) = parse_repo_from_description(issue.description.as_deref())?;
        let now = Utc::now();
        let repo_task = RepoTask {
            repo: RepoRef { org, repo },
            branch: ticket.as_str().to_string(),
            worktree_path: std::path::PathBuf::new(),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        Ok(Job {
            ticket: ticket.clone(),
            work_type,
            state: JobState::Active,
            repos: vec![repo_task],
            auto_merge,
            created_at: now,
            updated_at: now,
        })
    }
}

fn classify_labels(issue: &Issue) -> Result<WorkType> {
    let has_bug = issue.labels.iter().any(|l| l.name == LABEL_BUG);
    let has_feature = issue.labels.iter().any(|l| l.name == LABEL_FEATURE);
    match (has_bug, has_feature) {
        (true, false) => Ok(WorkType::Bug),
        (false, true) => Ok(WorkType::Feature),
        (true, true) => Err(MonorailError::TriageRejected(
            "ticket has both monorail:type/bug and monorail:type/feature".into(),
        )),
        (false, false) => Err(MonorailError::TriageRejected(
            "ticket has neither monorail:type/bug nor monorail:type/feature".into(),
        )),
    }
}

fn parse_repo_from_description(desc: Option<&str>) -> Result<(String, String)> {
    let desc = desc.unwrap_or("");
    for line in desc.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Repo:") {
            let v = rest.trim().trim_start_matches('`').trim_end_matches('`');
            let parts: Vec<&str> = v.splitn(2, '/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }
    }
    Err(MonorailError::TriageRejected(
        "ticket description must contain a 'Repo: <org>/<repo>' line".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{Label, WorkflowState};

    fn issue_with(labels: Vec<&str>, desc: Option<&str>) -> Issue {
        Issue {
            id: "i1".into(),
            identifier: "ACM-1".into(),
            title: "t".into(),
            description: desc.map(String::from),
            labels: labels
                .into_iter()
                .map(|n| Label {
                    id: n.into(),
                    name: n.into(),
                })
                .collect(),
            state: WorkflowState {
                id: "s".into(),
                name: "Backlog".into(),
                kind: "backlog".into(),
            },
        }
    }

    #[test]
    fn classifies_bug() {
        let i = issue_with(vec!["monorail:type/bug"], None);
        assert_eq!(classify_labels(&i).unwrap(), WorkType::Bug);
    }

    #[test]
    fn rejects_no_label() {
        let i = issue_with(vec!["other"], None);
        assert!(classify_labels(&i).is_err());
    }

    #[test]
    fn rejects_both_labels() {
        let i = issue_with(vec!["monorail:type/bug", "monorail:type/feature"], None);
        assert!(classify_labels(&i).is_err());
    }

    #[test]
    fn parses_repo_line() {
        let (o, r) = parse_repo_from_description(Some("blah\nRepo: acme/core-api\n")).unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("acme", "core-api"));
    }

    #[test]
    fn rejects_missing_repo() {
        assert!(parse_repo_from_description(Some("nothing here")).is_err());
    }
}
