use crate::domain::TicketKey;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub ticket: TicketKey,
    pub prompt: String,
    pub posted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub question_id: String,
    pub body: String,
    pub answered_at: DateTime<Utc>,
}
