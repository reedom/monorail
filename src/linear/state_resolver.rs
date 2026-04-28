use crate::linear::types::WorkflowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Started,
    Completed,
}

impl StateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StateKind::Started => "started",
            StateKind::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinearStateResolver {
    started: Option<WorkflowState>,
    completed: Option<WorkflowState>,
}

impl LinearStateResolver {
    pub fn from_states(states: Vec<WorkflowState>) -> Self {
        let started = states.iter().find(|s| s.kind == "started").cloned();
        let completed = states.iter().find(|s| s.kind == "completed").cloned();
        Self { started, completed }
    }

    pub fn for_kind(&self, kind: StateKind) -> Option<&WorkflowState> {
        match kind {
            StateKind::Started => self.started.as_ref(),
            StateKind::Completed => self.completed.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str, name: &str, kind: &str) -> WorkflowState {
        WorkflowState {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
        }
    }

    #[test]
    fn picks_first_started_and_completed() {
        let r = LinearStateResolver::from_states(vec![
            ws("s1", "Backlog", "backlog"),
            ws("s2", "In Progress", "started"),
            ws("s3", "In Review", "started"),
            ws("s4", "Done", "completed"),
        ]);
        assert_eq!(r.for_kind(StateKind::Started).unwrap().id, "s2");
        assert_eq!(r.for_kind(StateKind::Completed).unwrap().id, "s4");
    }

    #[test]
    fn missing_started_returns_none() {
        let r = LinearStateResolver::from_states(vec![ws("s1", "Done", "completed")]);
        assert!(r.for_kind(StateKind::Started).is_none());
        assert!(r.for_kind(StateKind::Completed).is_some());
    }

    #[test]
    fn empty_returns_none_for_both() {
        let r = LinearStateResolver::from_states(vec![]);
        assert!(r.for_kind(StateKind::Started).is_none());
        assert!(r.for_kind(StateKind::Completed).is_none());
    }
}
