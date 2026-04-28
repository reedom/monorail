use crate::error::{MonorailError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TicketKey(String);

impl TicketKey {
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(MonorailError::InvalidTicketKey(s.to_string()));
        }
        let prefix_ok = !parts[0].is_empty()
            && parts[0].chars().all(|c| c.is_ascii_uppercase());
        let number_ok = !parts[1].is_empty()
            && parts[1].chars().all(|c| c.is_ascii_digit());
        if !prefix_ok || !number_ok {
            return Err(MonorailError::InvalidTicketKey(s.to_string()));
        }
        Ok(TicketKey(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TicketKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_standard_form() {
        let t = TicketKey::parse("ACM-123").unwrap();
        assert_eq!(t.as_str(), "ACM-123");
    }

    #[test]
    fn accepts_long_prefix() {
        TicketKey::parse("PROD-9999").unwrap();
    }

    #[test]
    fn rejects_lowercase_prefix() {
        assert!(TicketKey::parse("acm-123").is_err());
    }

    #[test]
    fn rejects_missing_number() {
        assert!(TicketKey::parse("ACM-").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(TicketKey::parse("").is_err());
    }

    #[test]
    fn rejects_extra_segments() {
        assert!(TicketKey::parse("ACM-12-3").is_err());
    }
}
