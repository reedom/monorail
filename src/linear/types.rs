use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Vec<Label>,
    pub state: WorkflowState,
    pub team_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Label {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    // Linear's display-order field. Lower = earlier. Optional so ISSUE_QUERY,
    // which doesn't request it, still parses (Issue.state has no position).
    #[serde(default)]
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Comment {
    pub id: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_issue_with_labels() {
        let s = r#"{
            "id": "abc",
            "identifier": "ACM-1",
            "title": "fix login",
            "description": "...",
            "labels": [
              {"id":"l1","name":"monorail:type/bug"}
            ],
            "state": {"id":"s1","name":"Backlog","type":"backlog"},
            "team_id": "team-1"
        }"#;
        let issue: Issue = serde_json::from_str(s).unwrap();
        assert_eq!(issue.identifier, "ACM-1");
        assert_eq!(issue.labels.len(), 1);
        assert_eq!(issue.labels[0].name, "monorail:type/bug");
    }
}
