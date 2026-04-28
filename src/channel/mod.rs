use crate::domain::{Question, TicketKey};
use crate::error::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct NotifyContext {
    pub ticket: TicketKey,
    pub body: String,
}

#[async_trait]
pub trait HumanChannel: Send + Sync {
    async fn notify(&self, ctx: NotifyContext) -> Result<()>;
    #[allow(dead_code)] // Plan 3: Type B planning loop
    async fn post_question(&self, q: Question) -> Result<String>;
}

pub mod linear_comment;
pub use linear_comment::LinearCommentChannel;
