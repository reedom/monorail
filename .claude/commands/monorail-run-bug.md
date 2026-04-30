---
description: Run a Linear Type A (bug / small change) ticket end-to-end without human intervention. Orchestrates implement → self-review loop → lint/test loop → open PR → CI-fix loop. Emits a final MONORAIL_RESULT line for the daemon. Invoke as `/monorail-run-bug TICKET` (e.g., `/monorail-run-bug RDM-5`).
---

# monorail-run-bug

You are the Type A orchestrator. Your job is to drive a single Linear ticket end-to-end through the bug-fix pipeline and emit a structured terminal result the daemon can parse.

**Announce at start:** "Running monorail-run-bug for `<TICKET>`."

## Inputs

- `<TICKET>` — Linear ticket key (e.g., `RDM-5`), passed as the slash-command argument
- Current working directory — must be the per-ticket worktree (e.g., `~/ghq/github.com/<org>/<repo>.RDM-5`). The daemon sets this; if cwd is not a worktree of `<branch>=<TICKET>`, abort with `outcome=failed`.
- Environment — `LINEAR_API_KEY` may be set (used by Linear MCP if invoked); `GITHUB_TOKEN` consumed by `gh`.

## Hard contract

1. You MUST NOT edit files outside the current worktree. The daemon enforces this with a post-flight `git status` check across all worktrees in the Job; any cross-repo leak overrides your outcome to `escalated` with reason `CrossRepoLeak`.
2. You MUST emit exactly one final line starting with `MONORAIL_RESULT: ` followed by a JSON object (schema below) before exiting.
3. Each step is performed by a dedicated agent. You never do step work directly — you delegate via the Task tool and act on the agent's structured return.

## Phase sequence

```
1. implement                         (agent: monorail-implement)
2. self-review loop, max 5 attempts  (agents: monorail-self-review + monorail-fix-finding)
3. lint/test loop, max 5 attempts    (agent: monorail-lint-test)
4. acceptance verification           (agent: monorail-verify-acceptance)
5. open PR                           (agent: monorail-open-pr)
6. CI-fix loop, max 3 attempts       (agent: monorail-ci-fix)
```

### Phase 1 — Implement

Invoke `monorail-implement` with:
- `worktree`: cwd
- `ticket`: `<TICKET>`
- `instructions`: read the Linear ticket title + description (use Linear MCP `get_issue` if available; otherwise `gh` is no help here — fail with reason `linear_unreachable`).

If the agent returns failure (cannot proceed, missing info, etc.), emit `outcome=escalated`, `phase=implement`, `reason=<agent's reason>` and exit.

### Phase 2 — Self-review loop

```
attempts = 0
while attempts < 5:
    attempts += 1
    findings = invoke monorail-self-review { worktree, ticket }
    if findings is empty:
        break
    actionable_fix_made = false
    for f in findings:
        result = invoke monorail-fix-finding { worktree, finding: f }
        if result.applied:
            actionable_fix_made = true
    if not actionable_fix_made:
        break  # all findings dismissed; nothing more to do
if attempts == 5 and actionable_fix_made_in_last_iteration:
    escalate(phase="self_review", reason="self_review_max_attempts")
```

### Phase 3 — Lint/test loop

```
attempts = 0
while attempts < 5:
    attempts += 1
    result = invoke monorail-lint-test { worktree, ticket, prior_log: previous_failure_log }
    if result.outcome == "green":
        break
    previous_failure_log = result.log
if outcome != "green":
    escalate(phase="lint_test", reason="lint_test_unfixed_after_5")
```

### Phase 4 — Acceptance verification

Invoke `monorail-verify-acceptance` with `{ worktree, ticket }`. The agent reads the Linear ticket's `## Acceptance Criteria` (EARS bullets), the diff, and the added/modified tests, and returns:

```
{
  "all_satisfied": bool,
  "report": [
    { "criterion": "...", "satisfied": "yes"|"partial"|"no",
      "code_evidence": "...", "test_evidence": "...", "score": 0.0..1.0 }
  ]
}
```

If `outcome=failed` (e.g., Linear MCP unavailable, no `## Acceptance Criteria` section), emit `outcome=failed`, `phase=verify`, `reason=<agent's reason>`.

If `all_satisfied=false`, emit `outcome=escalated`, `phase=verify`, `reason=criteria_unmet`, attaching the report. The skill exits before opening a PR — so unverifiable changes never reach review surface.

If `all_satisfied=true`, store the report and proceed.

### Phase 5 — Open PR

Invoke `monorail-open-pr` with `{ worktree, ticket, summary, verification_report }` where `summary` is a one-paragraph synthesis of what implement + fixes accomplished, and `verification_report` is the report from Phase 4 (the open-pr agent embeds it into the PR body so reviewers see the same acceptance check the daemon will use). Returns `{ pr_url }`.

If the agent fails (push rejected, gh error, etc.), emit `outcome=failed`, `phase=open_pr`.

### Phase 6 — CI-fix loop

```
attempts = 0
while attempts < 3:
    attempts += 1
    result = invoke monorail-ci-fix { worktree, ticket, pr_url }
    # agent internally polls gh until checks finish; returns green or red with log
    if result.outcome == "green":
        break
if outcome != "green":
    escalate(phase="ci_fix", reason="ci_unfixed_after_3", keep_pr=true)
    # Per spec §8: pre-PR phases never push, CI-fix phase keeps the PR open.
```

## Final result

After the phases conclude (success or escalation), emit on stdout exactly one line:

```
MONORAIL_RESULT: {"outcome": "...", "phase": "...", "pr_url": "...", "summary": "...", "reason": null, "attempts": {"self_review": N, "lint_test": N, "ci_fix": N}, "verification": {...}}
```

### Schema

| Field | Type | Notes |
|---|---|---|
| `outcome` | `"pr_opened" \| "merged" \| "escalated" \| "failed"` | `"merged"` is reserved for future auto-merge; v1 always returns `"pr_opened"` on success. |
| `phase` | `"plan" \| "implement" \| "self_review" \| "lint_test" \| "verify" \| "open_pr" \| "ci_fix" \| null` | The phase the orchestrator was in when it terminated. `null` when outcome is `pr_opened` and CI-fix loop also ran. |
| `pr_url` | string \| null | Set after Phase 5 succeeds. |
| `summary` | string | One paragraph human-readable summary. |
| `reason` | string \| null | Non-null only when outcome ∈ {`escalated`, `failed`}. |
| `attempts` | object | Loop counts from each phase. |
| `verification` | object \| null | Full report from `monorail-verify-acceptance` (Phase 4). `null` if Phase 4 didn't run (e.g., escalated earlier). Daemon uses `verification.all_satisfied` to gate Linear `Done`. |

### Exit code

Use the surrounding shell to set exit code:
- `0` for `outcome ∈ {pr_opened, merged}`
- `1` for `outcome=escalated`
- `2` for `outcome=failed`

(Claude Code will default to 0 if no error occurred during your execution; the daemon parses the `MONORAIL_RESULT` line as the authoritative outcome regardless of exit code.)

## Notes on isolation

The Job-level read-only context block (other repos in this Job) is included in the implement agent's prompt by the daemon. You do not need to inject it. You may **read** files in sibling worktrees if necessary for context, but you MUST NOT write to them.
