use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Pending,
    Planning,
    Implementing,
    SelfReviewing,
    LintTesting,
    PrOpened,
    CiFixing,
    Merged,
    Aborted,
    Escalated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationReason {
    SelfReviewMaxed,
    LintTestMaxed,
    CiFixMaxed,
    CrossRepoLeak,
    EngineFailure,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkType {
    Bug,
    Feature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Active,
    Escalated,
    Done,
    Aborted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_serializes_kebab_case() {
        let s = serde_json::to_string(&Phase::SelfReviewing).unwrap();
        assert_eq!(s, "\"self-reviewing\"");
    }

    #[test]
    fn escalation_reason_round_trip() {
        let r = EscalationReason::CrossRepoLeak;
        let s = serde_json::to_string(&r).unwrap();
        let r2: EscalationReason = serde_json::from_str(&s).unwrap();
        assert_eq!(r, r2);
    }

    #[test]
    fn work_type_parses_bug() {
        let w: WorkType = serde_json::from_str("\"bug\"").unwrap();
        assert_eq!(w, WorkType::Bug);
    }

    #[test]
    fn planning_serializes_kebab_case() {
        let s = serde_json::to_string(&Phase::Planning).unwrap();
        assert_eq!(s, "\"planning\"");
    }
}
