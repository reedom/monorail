---
name: monorail:run-feature
description: Run a Linear Type B (feature / design-required) ticket end-to-end with a human-in-the-loop planning phase first, then identical to monorail:run-bug from implement onward. Emit a final MONORAIL_RESULT line. Use when invoked as `/monorail:run-feature TICKET`.
---

# monorail:run-feature

You are the Type B orchestrator. Identical to `monorail:run-bug` except for an **upfront human-planning phase** that happens before any code is written.

**Announce at start:** "Running monorail:run-feature for `<TICKET>`."

## Hard contract

Same as `monorail:run-bug` (no cross-worktree edits, MONORAIL_RESULT on stdout, delegate all step work to agents).

## Phase sequence

```
0. plan-with-human              (agent: monorail-plan-with-human)
1. implement                    (agent: monorail-implement, with the agreed plan as instructions)
2. self-review loop, max 5
3. lint/test loop, max 5
4. open PR
5. CI-fix loop, max 3
```

### Phase 0 — Plan with human

Invoke `monorail-plan-with-human` with `{ ticket: <TICKET> }`. This agent:

- Posts initial questions as Linear comments via Linear MCP (`create_comment`).
- Polls for human replies via Linear MCP (`list_comments` since timestamp).
- Iterates until a plan is agreed upon.
- Writes the agreed plan back to the Linear ticket body as a `## Monorail Plan` YAML section (per the original design doc §6.2).
- Returns `{ plan_yaml: string, approved: bool, instructions: string }`.

If `approved=false` after a configurable timeout (default 24 hours of no human reply), emit:

```json
MONORAIL_RESULT: {"outcome": "escalated", "phase": "plan", "pr_url": null, "summary": "...", "reason": "plan_not_approved_in_time", "attempts": {}}
```

### Phases 1–5

Once `approved=true`:

- Pass `instructions` from the plan agent's return into Phase 1 (`monorail-implement`).
- From there onward, execute exactly the same loop logic as `monorail:run-bug` (see `monorail:run-bug/SKILL.md`).

## Final result

Same `MONORAIL_RESULT` schema as `monorail:run-bug`. The `phase` field can additionally take the value `"plan"` if escalation happened during planning.

## Notes

- **Linear MCP must be configured.** If unavailable, abort immediately with `outcome=failed`, `phase=plan`, `reason=linear_mcp_unavailable`. The skill does not have a graceful fallback for the Q&A channel.
- **Plan YAML is the source of truth.** After approval, the plan is written to the Linear ticket; if the daemon resumes a job mid-feature, this skill re-reads the plan from the ticket body rather than re-asking the human.
