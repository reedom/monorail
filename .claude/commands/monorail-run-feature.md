---
description: Run a Linear Type B (feature / design-required) ticket end-to-end with a human-in-the-loop planning phase first, then identical to monorail-run-bug from implement onward. Emits a final MONORAIL_RESULT line. Invoke as `/monorail-run-feature TICKET`.
---

# monorail-run-feature

You are the Type B orchestrator. Identical to `monorail-run-bug` except for an **upfront human-planning phase** that happens before any code is written.

**Announce at start:** "Running monorail-run-feature for `<TICKET>`."

## Hard contract

Same as `monorail-run-bug` (no cross-worktree edits, MONORAIL_RESULT on stdout, delegate all step work to agents).

## Phase sequence

```
0. setup worktree               (inline — same as /monorail-run-bug Phase 0)
1. plan-with-human              (agent: monorail-plan-with-human)
2. implement                    (agent: monorail-implement, with the agreed plan as instructions)
3. self-review loop, max 5
4. lint/test loop, max 5
5. acceptance verification      (agent: monorail-verify-acceptance)
6. open PR
7. CI-fix loop, max 3
```

### Phase 0 — Setup worktree

Identical to Phase 0 of `/monorail-run-bug`. See `monorail-run-bug.md` §"Phase 0 — Setup worktree". The worktree must exist before plan-with-human runs, since the plan agent reads project context (CLAUDE.md, repo structure) to draft the proposal.

### Phase 1 — Plan with human

This phase **replaces** the run-bug Phase 1 triage step: instead of rejecting tickets that lack `## Acceptance Criteria`, run-feature creates them through the human Q&A. The plan agent's contract guarantees that, before returning `approved=true`, the ticket body has both:

- a `## Monorail Plan` YAML block (per the original design doc §6.2), and
- a `## Acceptance Criteria` section with EARS-style bullets.

Invoke `monorail-plan-with-human` with `{ ticket: <TICKET> }`. The agent:

- Posts initial questions as Linear comments via Linear MCP.
- Polls for human replies and iterates until a plan + criteria are agreed.
- Writes both sections back to the Linear ticket body.
- Returns `{ plan_yaml, approved, instructions, acceptance_criteria }`.

If `approved=false` after a configurable timeout (default 24 hours of no human reply), or if the plan agent fails to write criteria for any reason, emit:

```json
MONORAIL_RESULT: {"outcome": "escalated", "phase": "plan", "pr_url": null,
  "summary": "...", "reason": "plan_not_approved_in_time" | "criteria_not_written",
  "attempts": {}, "verification": null}
```

### Phases 2–7

Once `approved=true`:

- Pass `instructions` AND `acceptance_criteria` from the plan agent's return into Phase 2 (`monorail-implement`) — same input shape as run-bug Phase 2.
- From there onward, execute exactly the same loop logic as `/monorail-run-bug`'s Phases 2–7 (see `monorail-run-bug.md`), including the acceptance-verification step at Phase 5.

**Why no separate triage in run-feature.** Type B tickets often start without acceptance criteria — that's the point of the planning phase. Plan-with-human IS the triage: it negotiates and writes the criteria. After Phase 1, criteria exist by construction; a separate triage step would be redundant.

## Final result

Same `MONORAIL_RESULT` schema as `monorail-run-bug`. The `phase` field for this command can be `"setup"`, `"plan"`, or any of the run-bug post-triage phases (`"implement" | "self_review" | "lint_test" | "verify" | "open_pr" | "ci_fix"`). Note that `"triage"` does not appear in run-feature outcomes — that gating role is performed by `"plan"`.

## Notes

- **Linear MCP must be configured.** If unavailable, abort immediately with `outcome=failed`, `phase=plan`, `reason=linear_mcp_unavailable`. The skill does not have a graceful fallback for the Q&A channel.
- **Plan YAML is the source of truth.** After approval, the plan is written to the Linear ticket; if the daemon resumes a job mid-feature, this skill re-reads the plan from the ticket body rather than re-asking the human.
