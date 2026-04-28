use thiserror::Error;

#[derive(Debug, Error)]
pub enum MonorailError {
    #[error("invalid ticket key: {0}")]
    InvalidTicketKey(String),
    #[error("missing label: {0}")]
    MissingLabel(&'static str),
    #[error("ticket rejected at triage: {0}")]
    TriageRejected(String),
    #[error("phase aborted: {0}")]
    PhaseAborted(String),
    #[error("escalated: {0}")]
    Escalated(String),
    #[error("external tool failed: {tool}: {message}")]
    ExternalTool { tool: &'static str, message: String },
    #[error("linear api error: {0}")]
    Linear(String),
    #[error("state error: {0}")]
    State(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(String),
}

pub type Result<T> = std::result::Result<T, MonorailError>;
