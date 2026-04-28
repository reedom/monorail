# Plan 2 — Linear status sync + Plan-1 polish

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire Linear workflow-state transitions into the Type A runner (started before implement, completed after CI green) and clean up known polish items left from Plan 1.

**Architecture:** Discover team workflow states once per Job via a new `list_issue_statuses` GraphQL query; pick first state by universal `type` (`started`, `completed`); cache in `LinearStateResolver`; consult at two macro transition points in `run_type_a`. Polish items are targeted local fixes.

**Tech Stack:** Same as Plan 1 — Rust 2024, tokio, sqlx (sqlite), reqwest, async-trait, mockall, wiremock.

**Spec:** `docs/superpowers/specs/2026-04-28-monorail-plan2-linear-status-sync.md`

**Branch:** Stay on `impl/monorail-plan2` (already created from main); commit each task there.

---

## Task 1: Add `team { id }` to Issue GraphQL + struct

**Files:**
- Modify: `src/linear/graphql.rs`
- Modify: `src/linear/types.rs`
- Modify: `src/linear/mod.rs`

- [ ] **Step 1: Extend ISSUE_QUERY**

In `src/linear/graphql.rs`, the existing `ISSUE_QUERY` requests `id identifier title description labels{nodes{id name}} state{id name type}`. Add `team { id }` to the field set.

- [ ] **Step 2: Add `team_id` to `Issue`**

In `src/linear/types.rs`, add `pub team_id: String` to the `Issue` struct. Place it after `state`.

- [ ] **Step 3: Update IssueRaw flattening in `LinearClient::get_issue`**

In `src/linear/mod.rs`, the private `IssueRaw` deserializes the GraphQL response and flattens `labels.nodes` into `labels`. Extend the same pattern for `team`: add a private `TeamRaw { id: String }` and a `team: TeamRaw` field on `IssueRaw`, then assign `team_id: raw.team.id` when constructing the public `Issue`.

- [ ] **Step 4: Update tests**

Update existing fixture JSON in `src/linear/mod.rs` tests and any other tests that build mock `Issue` responses (e.g., `src/channel/linear_comment.rs`, `src/triager.rs`). Add `"team": { "id": "team-1" }` to the response body. For Rust-side `Issue { ... }` constructions in tests, add `team_id: "team-1".into()`.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all 50 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/linear src/channel src/triager.rs
git commit -m "feat(linear): add team_id to Issue for state-resolver lookup"
```

---

## Task 2: `LinearClient::list_issue_statuses`

**Files:**
- Modify: `src/linear/graphql.rs`
- Modify: `src/linear/mod.rs`

- [ ] **Step 1: Add GraphQL query**

In `src/linear/graphql.rs`, add:

```rust
pub const ISSUE_STATUSES_QUERY: &str = r#"
query IssueStatuses($teamId: String!) {
  workflowStates(filter: { team: { id: { eq: $teamId } } }) {
    nodes { id name type }
  }
}
"#;
```

- [ ] **Step 2: Write the failing test**

In `src/linear/mod.rs` add to the existing `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn list_issue_statuses_returns_all_team_states() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "data": { "workflowStates": { "nodes": [
            {"id":"s1","name":"Backlog","type":"backlog"},
            {"id":"s2","name":"In Progress","type":"started"},
            {"id":"s3","name":"Done","type":"completed"},
        ]}}
    });
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server).await;
    let c = LinearClient::new(format!("{}/graphql", server.uri()), "k").unwrap();
    let states = c.list_issue_statuses("team-1").await.unwrap();
    assert_eq!(states.len(), 3);
    assert_eq!(states[1].kind, "started");
}
```

- [ ] **Step 3: Run test to confirm it fails**

Run: `cargo test list_issue_statuses_returns_all_team_states`
Expected: FAIL — `list_issue_statuses` not defined.

- [ ] **Step 4: Implement the method**

Add to `impl LinearClient`:

```rust
pub async fn list_issue_statuses(&self, team_id: &str) -> Result<Vec<WorkflowState>> {
    let body = serde_json::json!({
        "query": graphql::ISSUE_STATUSES_QUERY,
        "variables": { "teamId": team_id },
    });
    let resp: serde_json::Value = self.post_graphql(&body).await?;
    let nodes = resp.pointer("/data/workflowStates/nodes")
        .ok_or_else(|| MonorailError::Linear("missing workflowStates.nodes".into()))?;
    let states: Vec<WorkflowState> = serde_json::from_value(nodes.clone())?;
    Ok(states)
}
```

(Adapt `post_graphql` invocation to match how the existing `get_issue` / `post_comment` methods POST. If `post_graphql` doesn't exist, follow the same pattern those methods use.)

- [ ] **Step 5: Re-run test**

Run: `cargo test list_issue_statuses_returns_all_team_states`
Expected: PASS.

- [ ] **Step 6: Run full suite**

Run: `cargo test`
Expected: 51 passing.

- [ ] **Step 7: Commit**

```bash
git add src/linear
git commit -m "feat(linear): add list_issue_statuses for workflow-state discovery"
```

---

## Task 3: `LinearStateResolver`

**Files:**
- Create: `src/linear/state_resolver.rs`
- Modify: `src/linear/mod.rs` (add `pub mod state_resolver; pub use state_resolver::LinearStateResolver;`)

- [ ] **Step 1: Write the failing tests**

Create `src/linear/state_resolver.rs`:

```rust
use crate::linear::types::WorkflowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Started,
    Completed,
}

impl StateKind {
    fn as_str(&self) -> &'static str {
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
        WorkflowState { id: id.into(), name: name.into(), kind: kind.into() }
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
        let r = LinearStateResolver::from_states(vec![
            ws("s1", "Done", "completed"),
        ]);
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
```

Note `as_str` helper is used by Task 4 events; keep it private until then.

- [ ] **Step 2: Wire into linear module**

Add to `src/linear/mod.rs`:

```rust
pub mod state_resolver;
pub use state_resolver::{LinearStateResolver, StateKind};
```

- [ ] **Step 3: Run tests**

Run: `cargo test state_resolver`
Expected: 3 passing.
Run: `cargo test`
Expected: 54 passing.

- [ ] **Step 4: Commit**

```bash
git add src/linear
git commit -m "feat(linear): LinearStateResolver picks first started/completed state by type"
```

---

## Task 4: Wire status sync into `run_type_a`

**Files:**
- Modify: `src/pipeline/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Make `as_str` public on StateKind**

In `src/linear/state_resolver.rs`, change `fn as_str` to `pub fn as_str` so the runner can include the kind in event payloads.

- [ ] **Step 2: Add `linear` to TypeARunArgs**

In `src/pipeline/mod.rs`, the existing `TypeARunArgs` struct gets a new field:

```rust
pub linear: &'a crate::linear::LinearClient,
```

Add it after `channel`. The generic parameters and lifetime stay unchanged.

- [ ] **Step 3: Build the resolver in `run_type_a`**

At the top of `run_type_a` (before `run_implement`), insert:

```rust
let issue = args.linear.get_issue(args.ticket.as_str()).await?;
let states = args.linear.list_issue_statuses(&issue.team_id).await?;
let resolver = crate::linear::LinearStateResolver::from_states(states);

set_linear_state(args.state, args.linear, args.ticket, &issue.id, &resolver,
    crate::linear::StateKind::Started).await?;
```

- [ ] **Step 4: Add private `set_linear_state` helper at the bottom of `mod.rs`**

```rust
async fn set_linear_state(
    state: &crate::state::SqliteState,
    linear: &crate::linear::LinearClient,
    ticket: &crate::domain::TicketKey,
    issue_id: &str,
    resolver: &crate::linear::LinearStateResolver,
    kind: crate::linear::StateKind,
) -> crate::error::Result<()> {
    match resolver.for_kind(kind) {
        Some(ws) => {
            linear.set_state(issue_id, &ws.id).await?;
            state.append_event(ticket, "linear_state_change", &serde_json::json!({
                "kind": kind.as_str(),
                "state_id": ws.id,
                "state_name": ws.name,
            })).await?;
        }
        None => {
            state.append_event(ticket, "linear_state_skip", &serde_json::json!({
                "kind": kind.as_str(),
                "reason": "no matching workflow state on team",
            })).await?;
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Wire `Completed` after CI green**

In `run_type_a`'s final `match` on `CiFixOutcome`:

```rust
CiFixOutcome::Green => {
    set_linear_state(args.state, args.linear, args.ticket, &issue.id, &resolver,
        crate::linear::StateKind::Completed).await?;
    Ok(TypeARunOutcome::PrGreen)
}
```

The `Escalated` arm is unchanged — escalation stays comment-only.

- [ ] **Step 6: Update `src/main.rs`**

In `run_command`, add `linear: linear.as_ref(),` to the `TypeARunArgs { ... }` construction. Place it next to `channel`.

- [ ] **Step 7: Build**

Run: `cargo build`
Expected: green.

- [ ] **Step 8: Run tests**

Run: `cargo test`
Expected: 54 passing (no new tests; runner is wired but exercising it requires a real LinearClient mock that's out of scope).

- [ ] **Step 9: Commit**

```bash
git add src/pipeline src/main.rs src/linear/state_resolver.rs
git commit -m "feat(pipeline): wire Linear started/completed state transitions"
```

---

## Task 5: Emit `review_stuck_proceed` event

**Files:**
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Split the `Clean | Stuck` arm**

In `run_type_a`, replace:

```rust
SelfReviewOutcome::Clean | SelfReviewOutcome::Stuck => {}
```

with:

```rust
SelfReviewOutcome::Clean => {}
SelfReviewOutcome::Stuck => {
    args.state.append_event(args.ticket, "review_stuck_proceed",
        &serde_json::json!({})).await?;
}
```

- [ ] **Step 2: Build + test**

Run: `cargo build && cargo test`
Expected: green, 54 passing.

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/mod.rs
git commit -m "feat(pipeline): emit review_stuck_proceed event when self-review stuck"
```

---

## Task 6: Fix `claude_code.rs:86` serde fallback

**Files:**
- Modify: `src/engine/claude_code.rs`

- [ ] **Step 1: Locate the call site**

Search for `to_string_pretty(&finding).unwrap_or_default()` in `src/engine/claude_code.rs`. It's inside `analyze_finding` (or `apply_fix`); the prompt builder serializes the finding for inclusion.

- [ ] **Step 2: Replace with `?` propagation**

Change:

```rust
let finding_json = serde_json::to_string_pretty(&finding).unwrap_or_default();
```

to:

```rust
let finding_json = serde_json::to_string_pretty(&finding)?;
```

The function already returns `crate::error::Result<...>` and `MonorailError: From<serde_json::Error>` is wired (used elsewhere in the file), so `?` works.

- [ ] **Step 3: Build + test**

Run: `cargo build && cargo test`
Expected: green, 54 passing.

- [ ] **Step 4: Commit**

```bash
git add src/engine/claude_code.rs
git commit -m "fix(engine): propagate serde error instead of silent unwrap_or_default"
```

---

## Task 7: Refactor `bump_attempt` to static queries

**Files:**
- Modify: `src/state/repo_tasks.rs`

- [ ] **Step 1: Read current implementation**

The current code looks like:

```rust
let col = match kind {
    AttemptKind::Review => "review_attempts",
    AttemptKind::LintTest => "lint_test_attempts",
    AttemptKind::CiFix => "ci_fix_attempts",
};
let q = format!("UPDATE repo_tasks SET {col} = {col} + 1 WHERE id = ?");
sqlx::query(&q).bind(id).execute(&self.pool).await?;
```

- [ ] **Step 2: Replace with static query selection**

```rust
const Q_REVIEW: &str =
    "UPDATE repo_tasks SET review_attempts = review_attempts + 1 WHERE id = ?";
const Q_LINT_TEST: &str =
    "UPDATE repo_tasks SET lint_test_attempts = lint_test_attempts + 1 WHERE id = ?";
const Q_CI_FIX: &str =
    "UPDATE repo_tasks SET ci_fix_attempts = ci_fix_attempts + 1 WHERE id = ?";

let q = match kind {
    AttemptKind::Review => Q_REVIEW,
    AttemptKind::LintTest => Q_LINT_TEST,
    AttemptKind::CiFix => Q_CI_FIX,
};
sqlx::query(q).bind(id).execute(&self.pool).await?;
```

Place the consts at the top of the function or as module-level constants near `bump_attempt`.

- [ ] **Step 3: Build + test**

Run: `cargo build && cargo test`
Expected: green, 54 passing. Existing `bump_attempt_increments` test still passes.

- [ ] **Step 4: Commit**

```bash
git add src/state/repo_tasks.rs
git commit -m "refactor(state): bump_attempt uses static queries instead of runtime format"
```

---

## Task 8: Cleanup — unused error variants + dead-code allows

**Files:**
- Modify: `src/error.rs`
- Modify: various (targeted `#[allow(dead_code)]`)

- [ ] **Step 1: Remove `MissingLabel` and `Escalated` variants**

In `src/error.rs`, delete the `MissingLabel(String)` and `Escalated(...)` variants from `MonorailError`. Confirm no construction sites:

Run: `grep -rn "MonorailError::MissingLabel\|MonorailError::Escalated" src/ tests/`
Expected: no matches outside the enum definition.

- [ ] **Step 2: Build to confirm no callers**

Run: `cargo build`
Expected: green.

- [ ] **Step 3: Audit dead-code warnings**

Run: `cargo build 2>&1 | grep "warning:" | grep -v "^$"` and read the list. Some warnings are now resolvable (e.g., `set_state` is wired in Task 4 — should no longer warn). For items still pending later plans, add targeted `#[allow(dead_code)]` with a one-line comment naming the consuming plan:

Likely candidates:
- `Question`, `Answer` — Plan 3+ Type B planning
- `JobRow` getters not used by Plan 1/2 — Plan 4 TUI
- `RepoTaskRow` getters not used — Plan 4 TUI
- `Phase::Merged`, related state strings — auto-merge wiring (later)

For each, prefer `#[allow(dead_code)]` on the specific item rather than the module.

- [ ] **Step 4: Re-run build, count remaining warnings**

Run: `cargo build 2>&1 | grep -c "^warning:"`
Goal: drop from 26 to under 5. Remaining warnings should be from genuinely-pending integration points.

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: 54 passing.

- [ ] **Step 6: Commit**

```bash
git add src/
git commit -m "chore: remove unused error variants; targeted dead-code allows for deferred plans"
```

---

## Definition of done

- All 8 tasks committed on `impl/monorail-plan2`
- `cargo build` warnings under 5 (down from 26)
- `cargo test` 54+ passing (47 unit + 3 e2e + 4 new resolver tests)
- Linear's `started` state set before implement; `completed` set after CI green; both observable via `linear_state_change` events in SQLite
- `review_stuck_proceed` events visible after a Stuck self-review
- `claude_code.rs` no longer silently swallows serde errors
- `bump_attempt` uses static queries

## Self-review (author)

**1. Spec coverage:** every spec section maps to a task. Linear sync = Tasks 1-4. Polish 4.1 = Task 5. Polish 4.2 = Task 6. Polish 4.3 = Task 7. Polish 4.4 + 4.5 = Task 8.

**2. Placeholder scan:** no TBD/TODO. Task 6 says "locate the call site" because the exact line may shift post-merge; specific code shown.

**3. Type consistency:** `WorkflowState`, `StateKind`, `LinearStateResolver`, `Issue.team_id` all consistent across tasks. No method signature drift.

Plan ready to execute.
