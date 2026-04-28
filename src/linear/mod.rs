pub mod graphql;
pub mod state_resolver;
pub mod types;

pub use state_resolver::{LinearStateResolver, StateKind};
pub use types::{Comment, Issue, Label, WorkflowState};

use crate::error::{MonorailError, Result};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

pub struct LinearClient {
    http: Client,
    endpoint: String,
}

impl LinearClient {
    pub fn new(endpoint: impl Into<String>, api_key: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(api_key).map_err(|e| MonorailError::Linear(e.to_string()))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        Ok(Self {
            http,
            endpoint: endpoint.into(),
        })
    }

    pub async fn get_issue(&self, key: &str) -> Result<Issue> {
        let body = json!({ "query": graphql::ISSUE_QUERY, "variables": { "key": key } });
        let resp: Value = self
            .http
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .error_for_status()
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .json()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        let issue_node = resp
            .pointer("/data/issue")
            .ok_or_else(|| MonorailError::Linear("missing /data/issue".into()))?
            .clone();
        let issue: IssueRaw = serde_json::from_value(issue_node)
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        Ok(Issue {
            id: issue.id,
            identifier: issue.identifier,
            title: issue.title,
            description: issue.description,
            labels: issue.labels.nodes,
            state: issue.state,
            team_id: issue.team.id,
        })
    }

    pub async fn post_comment(&self, issue_id: &str, body: &str) -> Result<Comment> {
        let body_val = json!({
            "query": graphql::COMMENT_CREATE_MUTATION,
            "variables": { "input": { "issueId": issue_id, "body": body } }
        });
        let resp: Value = self
            .http
            .post(&self.endpoint)
            .json(&body_val)
            .send()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .json()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        let c = resp
            .pointer("/data/commentCreate/comment")
            .ok_or_else(|| MonorailError::Linear("missing comment".into()))?;
        serde_json::from_value(c.clone()).map_err(|e| MonorailError::Linear(e.to_string()))
    }

    pub async fn list_issue_statuses(&self, team_id: &str) -> Result<Vec<WorkflowState>> {
        let body = json!({
            "query": graphql::ISSUE_STATUSES_QUERY,
            "variables": { "teamId": team_id }
        });
        let resp: Value = self
            .http
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .error_for_status()
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .json()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        let nodes = resp
            .pointer("/data/workflowStates/nodes")
            .ok_or_else(|| MonorailError::Linear("missing workflowStates.nodes".into()))?
            .clone();
        serde_json::from_value(nodes).map_err(|e| MonorailError::Linear(e.to_string()))
    }

    pub async fn set_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
        let body = json!({
            "query": graphql::ISSUE_UPDATE_STATE_MUTATION,
            "variables": { "id": issue_id, "stateId": state_id }
        });
        self.http
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .error_for_status()
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct IssueRaw {
    id: String,
    identifier: String,
    title: String,
    description: Option<String>,
    labels: LabelsRaw,
    state: WorkflowState,
    team: TeamRaw,
}

#[derive(Debug, Deserialize)]
struct LabelsRaw {
    nodes: Vec<Label>,
}

#[derive(Debug, Deserialize)]
struct TeamRaw {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_issue_happy_path() {
        let server = MockServer::start().await;
        let resp = serde_json::json!({
            "data": {
                "issue": {
                    "id": "abc", "identifier": "ACM-1", "title": "fix",
                    "description": null,
                    "labels": { "nodes": [{"id":"l","name":"monorail:type/bug"}] },
                    "state": {"id":"s","name":"Backlog","type":"backlog"},
                    "team": {"id":"team-1"}
                }
            }
        });
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&server)
            .await;
        let client = LinearClient::new(format!("{}/graphql", server.uri()), "key").unwrap();
        let issue = client.get_issue("ACM-1").await.unwrap();
        assert_eq!(issue.identifier, "ACM-1");
        assert_eq!(issue.labels[0].name, "monorail:type/bug");
    }

    #[tokio::test]
    async fn list_issue_statuses_returns_all_team_states() {
        let server = MockServer::start().await;
        let body = serde_json::json!({
            "data": { "workflowStates": { "nodes": [
                {"id":"s1","name":"Backlog","type":"backlog"},
                {"id":"s2","name":"In Progress","type":"started"},
                {"id":"s3","name":"Done","type":"completed"},
            ]}}
        });
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let c = LinearClient::new(format!("{}/graphql", server.uri()), "k").unwrap();
        let states = c.list_issue_statuses("team-1").await.unwrap();
        assert_eq!(states.len(), 3);
        assert_eq!(states[1].kind, "started");
    }

    #[tokio::test]
    async fn post_comment_returns_comment() {
        let server = MockServer::start().await;
        let resp = serde_json::json!({
            "data": { "commentCreate": { "success": true,
              "comment": {"id":"c1","body":"hi"} } }
        });
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&server)
            .await;
        let client = LinearClient::new(format!("{}/graphql", server.uri()), "key").unwrap();
        let c = client.post_comment("issue-1", "hi").await.unwrap();
        assert_eq!(c.id, "c1");
    }

    // Live contract tests — opt-in, hit the real Linear API.
    //
    // Run with:
    //   cargo test --lib linear:: -- --ignored --nocapture
    //
    // Required env: LINEAR_API_KEY
    // Optional env: LINEAR_API_ENDPOINT, LINEAR_TEST_TICKET (default RDM-5)
    //
    // Read-only: no comments posted, no states changed.
    mod live {
        use super::super::state_resolver::{LinearStateResolver, StateKind};
        use super::*;

        const DEFAULT_TICKET: &str = "RDM-5";
        const DEFAULT_ENDPOINT: &str = "https://api.linear.app/graphql";

        fn live_client() -> (LinearClient, String) {
            let api_key = std::env::var("LINEAR_API_KEY")
                .expect("LINEAR_API_KEY required (set via .envrc + direnv allow)");
            let endpoint = std::env::var("LINEAR_API_ENDPOINT")
                .unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
            let ticket = std::env::var("LINEAR_TEST_TICKET")
                .unwrap_or_else(|_| DEFAULT_TICKET.into());
            (LinearClient::new(endpoint, &api_key).expect("LinearClient::new"), ticket)
        }

        #[tokio::test]
        #[ignore = "live Linear API"]
        async fn get_issue_returns_team_id() {
            let (client, ticket) = live_client();
            let issue = client.get_issue(&ticket).await.expect("get_issue");
            assert_eq!(issue.identifier, ticket);
            assert!(!issue.team_id.is_empty(), "team_id populated");
            assert!(!issue.id.is_empty(), "id populated");
            eprintln!(
                "issue id={} identifier={} team_id={} state={:?} title={:?}",
                issue.id, issue.identifier, issue.team_id, issue.state.name, issue.title
            );
            eprintln!("labels:");
            for l in &issue.labels {
                eprintln!("  - {} (id={})", l.name, l.id);
            }
        }

        #[tokio::test]
        #[ignore = "live Linear API"]
        async fn list_issue_statuses_includes_started_and_completed() {
            let (client, ticket) = live_client();
            let issue = client.get_issue(&ticket).await.expect("get_issue");
            let states = client
                .list_issue_statuses(&issue.team_id)
                .await
                .expect("list_issue_statuses");

            assert!(!states.is_empty(), "team has at least one workflow state");

            // Reorder for display so it matches Linear's status-picker UI:
            // group by kind (backlog → unstarted → started → completed → canceled),
            // then sort by `position` within each group.
            fn kind_priority(kind: &str) -> u8 {
                match kind {
                    "triage" => 0,
                    "backlog" => 1,
                    "unstarted" => 2,
                    "started" => 3,
                    "completed" => 4,
                    "canceled" => 5,
                    _ => 99,
                }
            }
            let mut ui_order: Vec<&WorkflowState> = states.iter().collect();
            ui_order.sort_by(|a, b| {
                kind_priority(&a.kind).cmp(&kind_priority(&b.kind)).then_with(|| {
                    a.position
                        .unwrap_or(f64::INFINITY)
                        .total_cmp(&b.position.unwrap_or(f64::INFINITY))
                })
            });
            eprintln!("states ({}, in UI order):", states.len());
            for s in &ui_order {
                eprintln!(
                    "  - kind={:<10} position={:<8} name={:?}",
                    s.kind,
                    s.position
                        .map(|p| format!("{p}"))
                        .unwrap_or_else(|| "-".into()),
                    s.name
                );
            }

            let resolver = LinearStateResolver::from_states(states);
            let started = resolver
                .for_kind(StateKind::Started)
                .expect("team must have a workflow state of type=started");
            let completed = resolver
                .for_kind(StateKind::Completed)
                .expect("team must have a workflow state of type=completed");
            eprintln!("picked started:   name={:?} position={:?}", started.name, started.position);
            eprintln!("picked completed: name={:?} position={:?}", completed.name, completed.position);
        }
    }
}
