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
0. precheck — Linear MCP + ticket      (inline; no worktree yet)
1. setup worktree                      (inline; same as /monorail-run-bug Phase 1)
2. plan-with-human                     (agent: monorail-plan-with-human)
3. implement                           (agent: monorail-implement)
4. acceptance review (gate)            (agent: monorail-verify-acceptance, mode=review)
5. self-review loop, max 5             (agents: monorail-self-review + monorail-fix-finding)
6. lint/test loop, max 5               (agent: monorail-lint-test)
7. acceptance verification (final)     (agent: monorail-verify-acceptance, mode=verify)
8. open PR                             (agent: monorail-open-pr)
9. CI-fix loop, max 3                  (agent: monorail-ci-fix)
```

### Phase 0 — Precheck

Type B tickets typically arrive without acceptance criteria — that's the point of the planning phase. So the cheap precheck here is narrower than run-bug's triage: it only confirms that Linear MCP is reachable and the ticket itself can be fetched. The plan-with-human phase will write criteria as part of approval.

```
1. If Linear MCP is not available, emit:
       MONORAIL_RESULT: {"outcome": "failed", "phase": "triage",
         "reason": "linear_mcp_unavailable", ...}
       exit. Do NOT create a worktree.
2. Fetch the ticket. If fetch errors (auth, network, not found), emit
   `phase=triage, reason=ticket_fetch_failed: <error>` and exit.
3. Cache the ticket body for plan-with-human to use as starting context.
```

### Phase 1 — Setup worktree

Identical to Phase 1 of `/monorail-run-bug`. See `monorail-run-bug.md` §"Phase 1 — Setup worktree". The worktree must exist before plan-with-human runs, since the plan agent reads project context (CLAUDE.md, repo structure) to draft the proposal.

### Phase 2 — Plan with human

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

### Phases 3–9

Once `approved=true`:

- Pass `instructions` AND `acceptance_criteria` from the plan agent's return into Phase 3 (`monorail-implement`) — same input shape as run-bug.
- From there onward, execute the same loop logic as `/monorail-run-bug`'s Phases 2–8: implement → review-mode acceptance gate (Phase 4) → self-review → lint-test → verify-mode final acceptance check → open PR → CI-fix.
- Both acceptance phases call `monorail-verify-acceptance`; only the `mode` differs (`review` for the gate, `verify` for the final).

**Why precheck-then-setup-then-plan.** Type B tickets often start without acceptance criteria, so a strict triage like run-bug's would always fail here. The precheck (Phase 0) is cheap — just "Linear MCP works, ticket exists" — and runs without a worktree, so a network-blocked or wrong-ticket session fails before any filesystem effect. Plan-with-human (Phase 2) IS the criteria-creating step: by the time it returns approved, the ticket body has both `## Monorail Plan` and `## Acceptance Criteria` sections, and `acceptance_criteria` is captured for downstream phases.

**Why two acceptance passes.** Same as run-bug — the review-mode gate fails fast if implementation misses a criterion (saves self-review and lint-test cost), and the final verify-mode check is the rigorous Done gate.

## Final result

Same `MONORAIL_RESULT` schema as `monorail-run-bug`. The `phase` field for this command can be `"setup"`, `"plan"`, or any of the run-bug post-triage phases (`"implement" | "self_review" | "lint_test" | "verify" | "open_pr" | "ci_fix"`). Note that `"triage"` does not appear in run-feature outcomes — that gating role is performed by `"plan"`.

## Notes

- **Linear MCP must be configured.** If unavailable, abort immediately with `outcome=failed`, `phase=plan`, `reason=linear_mcp_unavailable`. The skill does not have a graceful fallback for the Q&A channel.
- **Plan YAML is the source of truth.** After approval, the plan is written to the Linear ticket; if the daemon resumes a job mid-feature, this skill re-reads the plan from the ticket body rather than re-asking the human.
