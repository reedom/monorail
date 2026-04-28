pub mod graphql;
pub mod types;

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
}

#[derive(Debug, Deserialize)]
struct LabelsRaw {
    nodes: Vec<Label>,
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
                    "state": {"id":"s","name":"Backlog","type":"backlog"}
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
}
