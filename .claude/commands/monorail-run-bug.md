---
description: Run a Linear Type A (bug / small change) ticket end-to-end without human intervention. Orchestrates implement → self-review loop → lint/test loop → open PR → CI-fix loop. Emits a final MONORAIL_RESULT line for the daemon. Invoke as `/monorail-run-bug TICKET` (e.g., `/monorail-run-bug RDM-5`).
---

# monorail-run-bug

You are the Type A orchestrator. Your job is to drive a single Linear ticket end-to-end through the bug-fix pipeline and emit a structured terminal result the daemon can parse.

**Announce at start:** "Running monorail-run-bug for `<TICKET>`."

## Inputs

- `<TICKET>` — Linear ticket key (e.g., `RDM-5`), passed as the slash-command argument
- Current working directory — when invoked by the daemon, this is already the per-ticket worktree. When invoked manually (`/monorail-run-bug TICKET` from any cwd inside the repo), Phase 0 below ensures a worktree exists.
- Environment — `LINEAR_API_KEY` may be set (used by Linear MCP if invoked); `GITHUB_TOKEN` consumed by `gh`.

## Hard contract

1. You MUST NOT edit files outside the resolved per-ticket worktree (the `worktree` value established by Phase 0). The daemon (when present) enforces this with a post-flight `git status` check across all worktrees in the Job; any cross-repo leak overrides your outcome to `escalated` with reason `CrossRepoLeak`.
2. You MUST emit exactly one final line starting with `MONORAIL_RESULT: ` followed by a JSON object (schema below) before exiting.
3. Each step is performed by a dedicated agent. You never do step work directly — you delegate via the Task tool and act on the agent's structured return.

## Phase sequence

```
0. triage — confirm criteria exist   (inline; Linear MCP read; NO worktree yet)
1. setup worktree                    (inline; uses wt + git)
2. implement                         (agent: monorail-implement)
3. self-review loop, max 5 attempts  (agents: monorail-self-review + monorail-fix-finding)
4. lint/test loop, max 5 attempts    (agent: monorail-lint-test)
5. acceptance verification           (agent: monorail-verify-acceptance)
6. open PR                           (agent: monorail-open-pr)
7. CI-fix loop, max 3 attempts       (agent: monorail-ci-fix)
```

**Why triage runs first.** Acceptance criteria are the basis on which
Phase 5 judges whether the change is Done. If a ticket has none, there
is no objective basis to start work. Triage uses only Linear MCP — no
worktree needed — so it's the cheapest possible failure point. Failing
here means no worktree is created, no compute is spent, and the user
gets a clear Linear comment explaining what to add. Setup, implement,
and everything after only happen once we know the work is well-formed.

### Phase 0 — Triage: confirm acceptance criteria exist

**Do this BEFORE creating any worktree.** Triage uses only Linear MCP; no filesystem changes happen here. Failing fast at this point means no worktree is left behind, no compute is wasted, and the user gets a clear comment in Linear telling them what to add.

```
1. If Linear MCP is not available in this session:
       MONORAIL_RESULT: {"outcome": "failed", "phase": "triage",
         "pr_url": null, "summary": "...",
         "reason": "linear_mcp_unavailable",
         "attempts": {}, "verification": null}
       exit.

2. Fetch the ticket. Use the MCP tool whose name resolves to
   "get issue by identifier" (exact tool name varies by MCP server;
   the official Linear MCP exposes one).
   - If the fetch errors (auth, network, ticket not found):
         MONORAIL_RESULT: {"outcome": "failed", "phase": "triage",
           "reason": "ticket_fetch_failed: <error>", ...}

3. Examine the ticket body. Look for a heading "## Acceptance Criteria"
   followed by at least one non-empty bullet. Use a permissive match:
   - heading text "Acceptance Criteria" (case-insensitive, possibly
     followed by " (EARS)" or similar parenthetical)
   - at least one line under it starting with "-" or "*" with
     non-whitespace content

4. If the section is missing or empty:
       a. Post a Linear comment to the ticket explaining why monorail
          cannot proceed:

              monorail cannot start: this ticket has no
              `## Acceptance Criteria` section. Please add EARS-style
              bullets describing the expected behaviour, then re-run.

              Example:
              ```
              ## Acceptance Criteria
              - The README.md file shall exist at the repository root.
              - When `cargo test` runs, all tests shall pass.
              ```

       b. Emit:
              MONORAIL_RESULT: {"outcome": "escalated", "phase": "triage",
                "pr_url": null,
                "summary": "ticket missing acceptance criteria",
                "reason": "needs_acceptance_criteria",
                "attempts": {}, "verification": null}
       c. Exit. **Do NOT proceed to Phase 1 (worktree setup).**

5. If the section is present, capture the bullets verbatim into a
   variable `acceptance_criteria`, and the ticket title + description
   into `instructions`, for use in later phases.
```

After Phase 0, you have a verified set of `acceptance_criteria` and the ticket body cached in memory. Proceed to Phase 1.

### Phase 1 — Setup worktree

Now that triage has passed, materialize the per-ticket worktree. This phase is idempotent: existing worktrees are reused.

```
1. current_branch = `git rev-parse --abbrev-ref HEAD`
2. If current_branch == <TICKET>:
       worktree = `git rev-parse --show-toplevel`
       (We're already in the right worktree — daemon-prepared, or a manual rerun.)
   Else:
       a. Find the base repo path. Run `git worktree list --porcelain` from cwd
          and pick the entry whose path is the "main" worktree (the one whose
          branch is the repo's default — usually `main` or `master`).
          - If `git worktree list` shows only entries inside this current
            worktree's tree, fall back to `ghq list -p <org>/<repo>` once you
            know the org/repo (derive from `git remote get-url origin`).
       b. Run: `wt -C <base_repo_path> switch --create <TICKET>`
          - This creates the worktree if absent or switches to it if present.
          - The wt convention places it at `<base_parent>/<repo>.<TICKET>`
            (e.g., `~/ghq/github.com/reedom/monorail.RDM-5`).
       c. worktree = the resulting path. You can confirm via `wt list`.
3. From here on, every agent invocation passes `worktree` explicitly. Bash
   commands inside agents `cd` to that path or use `git -C "$worktree"`.
4. If any of the above fails (no `wt` on PATH, no permission to create, etc.),
   emit:

       MONORAIL_RESULT: {"outcome": "failed", "phase": "setup", "pr_url": null,
         "summary": "...", "reason": "<actual error>", "attempts": {}, "verification": null}

   and exit.
```

After Phase 1, `worktree` is the absolute path to the per-ticket worktree, and the branch in that worktree is `<TICKET>`.

### Phase 2 — Implement

Invoke `monorail-implement` with:
- `worktree`: the path resolved in Phase 1
- `ticket`: `<TICKET>`
- `instructions`: the Linear ticket title + description (already fetched in Phase 0; reuse that body)
- `acceptance_criteria`: the bullets captured in Phase 0, so the implementor knows what success looks like before writing a line of code

If the agent returns failure (cannot proceed, missing info, etc.), emit `outcome=escalated`, `phase=implement`, `reason=<agent's reason>` and exit.

### Phase 3 — Self-review loop

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

### Phase 4 — Lint/test loop

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

### Phase 5 — Acceptance verification

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

### Phase 6 — Open PR

Invoke `monorail-open-pr` with `{ worktree, ticket, summary, verification_report }` where `summary` is a one-paragraph synthesis of what implement + fixes accomplished, and `verification_report` is the report from Phase 5 (the open-pr agent embeds it into the PR body so reviewers see the same acceptance check the daemon will use). Returns `{ pr_url }`.

If the agent fails (push rejected, gh error, etc.), emit `outcome=failed`, `phase=open_pr`.

### Phase 7 — CI-fix loop

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
| `phase` | `"setup" \| "triage" \| "plan" \| "implement" \| "self_review" \| "lint_test" \| "verify" \| "open_pr" \| "ci_fix" \| null` | The phase the orchestrator was in when it terminated. `null` when outcome is `pr_opened` and CI-fix loop also ran. |
| `pr_url` | string \| null | Set after Phase 6 succeeds. |
| `summary` | string | One paragraph human-readable summary. |
| `reason` | string \| null | Non-null only when outcome ∈ {`escalated`, `failed`}. |
| `attempts` | object | Loop counts from each phase. |
| `verification` | object \| null | Full report from `monorail-verify-acceptance` (Phase 5). `null` if Phase 5 didn't run (e.g., escalated earlier). Daemon uses `verification.all_satisfied` to gate Linear `Done`. |

### Exit code

Use the surrounding shell to set exit code:
- `0` for `outcome ∈ {pr_opened, merged}`
- `1` for `outcome=escalated`
- `2` for `outcome=failed`

(Claude Code will default to 0 if no error occurred during your execution; the daemon parses the `MONORAIL_RESULT` line as the authoritative outcome regardless of exit code.)

## Notes on isolation

The Job-level read-only context block (other repos in this Job) is included in the implement agent's prompt by the daemon. You do not need to inject it. You may **read** files in sibling worktrees if necessary for context, but you MUST NOT write to them.
