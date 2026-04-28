# monorail Plan 2 — Linear status sync + Plan-1 polish

**Date:** 2026-04-28
**Status:** Approved for planning
**Scope:** Wire Linear workflow-state updates into the Type A pipeline; clean up known follow-ups left from Plan 1.

## 1. Goal

Make Plan 1's Type A pipeline production-shaped: surface progress on Linear by transitioning the issue's workflow state automatically, and clear known polish items.

## 2. Non-goals

- Configurable state-name overrides (Plan 3 / config layer)
- Cross-Job caching of resolved state IDs (cache scoped to a single Job)
- Type B planning, multi-repo, TUI (later plans)

## 3. Linear status sync

### 3.1 Discovery model

Linear workflow states are configurable per team. Every state carries a universal `type` field: `backlog | unstarted | started | completed | canceled | triage`.

monorail uses the type to select target states without hardcoding names. At Job start it queries `list_issue_statuses(team_id)` once and caches the result. For a target type it picks the first state Linear returns.

A `LinearStateResolver` holds:

```
started:   Option<state_id>
completed: Option<state_id>
```

If a target type has no matching state on the team, the field is `None` — sync for that transition is silently skipped, never fails.

### 3.2 Transitions wired

Two macro transitions in `run_type_a`:

| When | Target type | Side effect on miss |
|---|---|---|
| Before `run_implement` (first action of the runner) | `started` | emit `linear_state_skip` event with reason |
| After `run_ci_fix` returns `Green` | `completed` | emit `linear_state_skip` event with reason |

Each successful transition emits a `linear_state_change` event with the new state id and name.

Escalations remain comment-only (`escalate` posts via `LinearCommentChannel`). The work isn't canceled — it's paused for human input — so we leave the issue's state as `started`.

### 3.3 GraphQL changes

- Extend `Issue` GraphQL fragment with `team { id }`. Add `team_id: String` to the `Issue` Rust struct.
- New `list_issue_statuses(team_id: &str) -> Result<Vec<WorkflowState>>` on `LinearClient`. Re-uses existing `WorkflowState { id, name, kind }` (`kind` already maps to Linear's `type`).

### 3.4 Wiring

`TypeARunArgs` gains a `linear: &'a LinearClient` field. The runner builds the resolver before the first phase call and consults it at the two transition points.

## 4. Polish items

### 4.1 `SelfReviewOutcome::Stuck` is silently swallowed

Today `run_type_a` matches `Clean | Stuck => {}` and proceeds with no event. Operators reading the SQLite event log can't see "stuck-but-proceed" runs.

Fix: split the match arms. `Stuck` emits a `review_stuck_proceed` event before falling through.

### 4.2 `claude_code.rs:86` `unwrap_or_default` on serde

A `serde_json::to_string_pretty(&finding).unwrap_or_default()` silently substitutes an empty string if serialization fails, corrupting the prompt.

Fix: propagate as `MonorailError::Serde` via `?`. (`Finding` always serializes today, but the silent fallback is inconsistent with every other serde call site.)

### 4.3 `bump_attempt` runtime SQL string interpolation

`format!("UPDATE repo_tasks SET {col} = {col} + 1 WHERE id = ?")` builds the query at runtime from a controlled `match`. No injection risk today, but a future change to the match could break it silently.

Fix: replace with three pre-built `static` query strings selected by `match` on `AttemptKind`. Behavior unchanged, but the column name lives in the SQL constant rather than being interpolated.

### 4.4 Unused error variants

- `MonorailError::MissingLabel` — declared in Plan 1, never constructed. The label rejection path uses `TriageRejected`.
- `MonorailError::Escalated` — declared, never constructed. Escalation surfaces via `EscalationReason`, not as an error variant.

Fix: remove both. If a future plan needs them, re-add at that time.

### 4.5 Dead-code warning noise

26 dead-code warnings on items reserved for later plans (`Question`, `Answer`, the `JobRow`/`RepoTaskRow` getters, `Phase::Merged`, `set_state` after Plan 2 wires it, etc.). They drown out new-warning signal.

Fix: targeted `#[allow(dead_code)]` on items demonstrably reserved for later plans, with a one-line comment naming the plan that consumes them. Items used by Plan 2 (e.g., `set_state`) get unlocked naturally.

## 5. Tests

- **Unit (LinearStateResolver):** mock GraphQL response with: both states present, only `started`, only `completed`, neither. Assert `for_type()` returns the right Option.
- **Unit (`bump_attempt`):** existing tests must still pass after the static-query refactor.
- **Unit (`run_type_a` Stuck branch):** new test asserts `review_stuck_proceed` event present after a Stuck self-review.
- **e2e:** unchanged (the existing 3 tests don't touch Linear).

The Linear-state wiring itself doesn't get a unit test in the runner because `run_type_a` already takes traits — adding stub LinearClient calls would require a new injection seam. Manual verification on a real ticket is acceptable for v1; the resolver's unit tests cover the only nontrivial logic.

## 6. Out-of-scope details

- A `LinearClient` trait abstraction (would unlock unit-testing the runner's status calls). Defer until something else also needs it.
- Per-Job state-resolver cache invalidation (resolver is built fresh per Job, so this can't drift).

## 7. Risks

- **No `started` or `completed` state on the team:** Linear teams in practice have both, but custom workflows could omit them. We skip silently and log — operators notice via missing state changes, not via a crashed pipeline. Acceptable.
- **Multiple `started` states (e.g., "In Progress" and "In Review"):** we pick whichever Linear returns first. Documented; configurable in Plan 3.

## 8. Estimated task count

8 tasks. See implementation plan.
