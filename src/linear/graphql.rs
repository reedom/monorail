pub const ISSUE_QUERY: &str = r#"
query Issue($key: String!) {
  issue(id: $key) {
    id
    identifier
    title
    description
    labels { nodes { id name } }
    state { id name type }
  }
}"#;

pub const COMMENT_CREATE_MUTATION: &str = r#"
mutation CreateComment($input: CommentCreateInput!) {
  commentCreate(input: $input) { success comment { id body } }
}"#;

pub const ISSUE_UPDATE_STATE_MUTATION: &str = r#"
mutation UpdateState($id: String!, $stateId: String!) {
  issueUpdate(id: $id, input: { stateId: $stateId }) { success }
}"#;
