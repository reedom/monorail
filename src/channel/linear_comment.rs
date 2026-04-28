use crate::channel::{HumanChannel, NotifyContext};
use crate::domain::Question;
use crate::error::Result;
use crate::linear::LinearClient;
use async_trait::async_trait;
use std::sync::Arc;

pub struct LinearCommentChannel {
    pub client: Arc<LinearClient>,
}

#[async_trait]
impl HumanChannel for LinearCommentChannel {
    async fn notify(&self, ctx: NotifyContext) -> Result<()> {
        let issue = self.client.get_issue(ctx.ticket.as_str()).await?;
        self.client.post_comment(&issue.id, &ctx.body).await?;
        Ok(())
    }

    async fn post_question(&self, q: Question) -> Result<String> {
        let issue = self.client.get_issue(q.ticket.as_str()).await?;
        let comment = self.client.post_comment(&issue.id, &q.prompt).await?;
        Ok(comment.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TicketKey;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn notify_posts_comment_after_get_issue() {
        let server = MockServer::start().await;
        let resp = serde_json::json!({
            "data": {
                "issue": {
                    "id": "iss-1", "identifier": "ACM-1", "title": "t",
                    "description": null,
                    "labels": { "nodes": [] },
                    "state": {"id":"s","name":"Backlog","type":"backlog"},
                    "team": {"id":"team-1"}
                },
                "commentCreate": {
                    "success": true,
                    "comment": { "id": "c-1", "body": "hello" }
                }
            }
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&server)
            .await;
        let lc = Arc::new(LinearClient::new(format!("{}/graphql", server.uri()), "k").unwrap());
        let ch = LinearCommentChannel { client: lc };
        ch.notify(NotifyContext {
            ticket: TicketKey::parse("ACM-1").unwrap(),
            body: "hello".into(),
        })
        .await
        .unwrap();
    }
}
