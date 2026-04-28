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
        // For each kind we care about, pick the state with the lowest `position`
        // — same rule Linear's UI uses to order states within a kind group.
        // Missing positions sort last; ties preserve input order.
        Self {
            started: lowest_position(&states, "started"),
            completed: lowest_position(&states, "completed"),
        }
    }

    pub fn for_kind(&self, kind: StateKind) -> Option<&WorkflowState> {
        match kind {
            StateKind::Started => self.started.as_ref(),
            StateKind::Completed => self.completed.as_ref(),
        }
    }
}

fn lowest_position(states: &[WorkflowState], kind: &str) -> Option<WorkflowState> {
    states
        .iter()
        .filter(|s| s.kind == kind)
        .min_by(|a, b| {
            let ap = a.position.unwrap_or(f64::INFINITY);
            let bp = b.position.unwrap_or(f64::INFINITY);
            ap.total_cmp(&bp)
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str, name: &str, kind: &str) -> WorkflowState {
        ws_at(id, name, kind, None)
    }

    fn ws_at(id: &str, name: &str, kind: &str, position: Option<f64>) -> WorkflowState {
        WorkflowState {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            position,
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
    fn position_orders_states_when_input_order_disagrees() {
        // Mirrors what Linear's API actually returned for reedom's workspace:
        // "In Review" came before "In Progress" in the response, but
        // "In Progress" has the lower position and is what the UI shows first.
        let r = LinearStateResolver::from_states(vec![
            ws_at("s1", "In Review", "started", Some(4.0)),
            ws_at("s2", "Todo", "unstarted", Some(2.0)),
            ws_at("s3", "In Progress", "started", Some(3.0)),
            ws_at("s4", "Done", "completed", Some(5.0)),
        ]);
        assert_eq!(r.for_kind(StateKind::Started).unwrap().id, "s3");
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
