# monorail Product Specification — EARS

- **Status:** Draft (synthesized 2026-04-30 from the design doc, the small-daemon/skill-first pivot, and the ROADMAP)
- **Sources of authority (in override order, latest wins):**
  1. `docs/ROADMAP.md` — Architecture decisions table (2026-04-30 amendments)
  2. `docs/superpowers/specs/2026-04-28-small-daemon-skill-first.md`
  3. `docs/superpowers/specs/2026-04-27-monorail-design.md`
- **Convention:** All criteria use EARS Ubiquitous (`The X shall Y.`) or Event-driven (`When …, the X shall Y.`) patterns. State-driven, Optional, and Unwanted patterns are flagged inline if used.

This document is the canonical product-level acceptance specification for monorail. Linear ticket bodies state deltas against these criteria; the daemon and orchestrators verify against them.

## 1. Glossary (read-only — definitions, not criteria)

- **Job** — one Linear ticket's worth of work, spanning 1..N RepoTasks.
- **RepoTask** — work in one repository / worktree for one Job.
- **Daemon** — long-running Rust binary (or one-shot `monorail run`) that triages, schedules, and observes Jobs.
- **Orchestrator command** — `/monorail-run-bug`, `/monorail-run-feature`, or `/monorail-plan`, invoked as `claude -p "/<command> <TICKET>"`.
- **Step agent** — single-step Claude Code agent invoked by an orchestrator (`monorail-implement`, `-self-review`, `-fix-finding`, `-lint-test`, `-verify-acceptance`, `-open-pr`, `-ci-fix`, `-plan-with-human`).
- **MONORAIL_RESULT** — the JSON contract emitted by an orchestrator on its final stdout line.

## 2. Triage and ticket admission

- The triager shall ignore any Linear ticket that carries neither `monorail:type/bug` nor `monorail:type/feature`.
- When a ticket carries `monorail:type/bug`, the triager shall classify it as Type A (no human planning).
- When a ticket carries `monorail:type/feature`, the triager shall classify it as Type B (human-in-the-loop planning permitted).
- The triager shall accept the `monorail:auto-merge` label only as an additive flag, independent of the type label.
- When a Type A ticket lacks an `## Acceptance Criteria` section in its body, the triager shall reject the ticket with reason `needs_acceptance_criteria` and shall post a Linear comment requesting one.
- When a Type A ticket touches more than one repository and its body lacks a `## Monorail Plan` YAML section, the triager shall reject the ticket with a Linear comment requesting either a GitHub link or a plan section.
- When a Type B ticket lacks an `## Acceptance Criteria` or `## Monorail Plan` section, the triager shall accept the ticket and the `monorail-plan-with-human` agent shall elicit and write the missing sections back to the ticket body before leaving the planning phase.
- When a Type B ticket already contains complete `## Acceptance Criteria` and `## Monorail Plan` sections, the `monorail-plan-with-human` agent shall pass through the planning phase without posting any Q&A comments.

## 3. Acceptance criteria (EARS) on tickets

- Every monorail-eligible Linear ticket shall include an `## Acceptance Criteria` section in its body.
- Bullets in a ticket's `## Acceptance Criteria` section shall use EARS Ubiquitous or Event-driven patterns.
- The `monorail-verify-acceptance` agent shall produce one report entry per criterion containing `criterion`, `satisfied` (`yes` | `partial` | `no`), `code_evidence`, `test_evidence`, and `score`.
- When `monorail-verify-acceptance` runs in `review` mode, it shall require `code_evidence` for each criterion to count it as satisfied and shall treat `test_evidence` as informational.
- When `monorail-verify-acceptance` runs in `verify` mode, it shall require both `code_evidence` and `test_evidence` to be non-empty for any criterion to be marked `satisfied="yes"`.
- When a criterion in `verify` mode has only `code_evidence`, the agent shall mark it `satisfied="partial"` and shall treat partial as failure unless the ticket carries the `monorail:no-test-required` label.
- The `monorail-verify-acceptance` agent shall set `all_satisfied=true` only when every criterion in the chosen mode meets that mode's threshold.
- monorail v1 shall not write to project-level EARS specifications; project-level propagation is deferred under roadmap ID `project-spec-sync`.

## 4. Orchestrator commands

- The `/monorail-run-bug` and `/monorail-run-feature` orchestrator definitions shall live under `.claude/commands/`, and the step-agent definitions shall live under `.claude/agents/`, both auto-discovered without a plugin install.
- When `/monorail-run-bug` is invoked, the orchestrator shall execute these phases in order: implement; self-review loop; acceptance review; self-review loop; lint/test loop; acceptance verification; open PR; CI-fix loop.
- When `/monorail-run-feature` is invoked, the orchestrator shall first execute a plan-with-human phase via the `monorail-plan-with-human` agent and then proceed identically to `/monorail-run-bug` from the implement phase onward.
- When the plan-with-human phase concludes with the human's approval, the orchestrator shall write the agreed `## Acceptance Criteria` and `## Monorail Plan` sections back to the ticket body before leaving the phase.
- The `/monorail-plan` command shall wrap only the `monorail-plan-with-human` agent and shall not invoke any other step agent.
- When `/monorail-plan` is invoked, the command shall run the Q&A planning loop via Linear MCP and shall exit immediately after writing the agreed `## Acceptance Criteria` and `## Monorail Plan` sections back to the ticket body.
- The `/monorail-plan` command shall not transition the Linear ticket's workflow state.
- When `/monorail-plan` is invoked on a ticket whose `## Acceptance Criteria` and `## Monorail Plan` sections are already complete, the command shall complete with no Linear comments posted.
- The orchestrator shall bound each self-review loop to 5 iterations, the lint/test loop to 5 iterations, and the CI-fix loop to 3 iterations.
- When the acceptance review phase finds any criterion without `code_evidence`, the orchestrator shall escalate with `phase=verify`, `reason=implementation_misses_criteria` and shall not enter subsequent loops.
- When the acceptance verification phase produces `all_satisfied=false`, the orchestrator shall escalate with `phase=verify`, `reason=criteria_unmet` and shall not open a PR.
- The orchestrator shall not push any commits or open any PR in any pre-PR phase that ends in escalation.
- When the CI-fix loop exhausts its 3 attempts without green checks, the orchestrator shall escalate while leaving the PR open.
- When `monorail-open-pr` opens a PR, it shall embed the final acceptance-verification report into the PR description.
- When step agents need a project verify command, the orchestrator's agents shall consult sources in priority order: `CLAUDE.md` / `AGENTS.md`; then `Makefile`; then `package.json` scripts; then `Cargo.toml`; then `pyproject.toml`.

## 5. Step agents

- Each step agent shall be invoked with a fresh Claude Code context per orchestrator call.
- The orchestrator shall be the only coordinator between step agents; step agents shall not invoke each other.
- The `monorail-implement` agent shall use the read-write tools (Read, Edit, Write, Bash, Grep, Glob) within the assigned worktree.
- The `monorail-self-review` agent shall return a JSON list of findings with `id`, `file`, `line`, `severity`, and `message`.
- The `monorail-fix-finding` agent shall return `{ applied: bool, reason: string }` for each finding it processes.
- The `monorail-lint-test` agent shall return `{ outcome: "green"|"red", log: string }`.
- The `monorail-ci-fix` agent shall return `{ outcome: "green"|"red", attempts: int }`.
- The `monorail-open-pr` agent shall return `{ pr_url: string }` after a successful push and `gh pr create`.
- The `monorail-plan-with-human` agent shall return `{ plan_yaml: string, approved: bool }`.

## 6. Daemon ↔ orchestrator contract (`MONORAIL_RESULT`)

- The daemon's `Engine` adapter shall expose a single invocation entry that runs `claude -p "/<command> <TICKET>"` with the worktree as cwd and shall parse `MONORAIL_RESULT` from the orchestrator's stdout.
- The orchestrator's final stdout line shall begin with the literal `MONORAIL_RESULT:` followed by a JSON object.
- The `MONORAIL_RESULT` JSON shall contain the fields `outcome`, `phase`, `pr_url`, `summary`, `reason`, `attempts`, and `verification`.
- The `outcome` field shall be one of `pr_opened`, `merged`, `escalated`, or `failed`.
- The `phase` field shall name the phase the orchestrator was in when it terminated.
- The `attempts` field shall record per-loop counters at minimum for `self_review`, `lint_test`, and `ci_fix`.
- The `verification` field shall contain `all_satisfied` (boolean) and `report` (per-criterion array as defined in §3).
- The orchestrator shall exit with code 0 when `outcome` is `pr_opened` or `merged`, code 1 when `outcome` is `escalated`, and code 2 when `outcome` is `failed`.
- monorail v1's orchestrator shall not return `outcome=merged`; the daemon owns merge.

## 7. Linear status synchronization (split ownership)

- When the orchestrator is dispatched, the orchestrator shall transition the Linear ticket to a state of type `started` (e.g., `In Progress`) before invoking `monorail-implement`.
- When `monorail-open-pr` successfully opens a PR, the agent shall transition the Linear ticket to a state of type `started` whose label maps to "in review" (e.g., `In Review`).
- When the daemon completes a merge of the orchestrator's PR, the daemon shall transition the Linear ticket to a state of type `completed` (e.g., `Done`).
- When the daemon observes the PR being closed without merge, the daemon shall transition the Linear ticket to a state of type `canceled` (e.g., `Canceled`).
- When the daemon receives `outcome=pr_opened` with `verification.all_satisfied=false`, the daemon shall not transition the Linear ticket to `completed` regardless of CI status, and shall post a Linear comment listing the unsatisfied criteria.
- The orchestrator and daemon shall both soft-fail on Linear MCP / Linear API errors during status transitions and shall continue rather than abort.
- When a target state type does not exist on the team, the resolving side shall skip the transition silently and shall emit a `linear_state_skip` event with reason.
- When multiple states share a `kind=started` on a team, the resolver shall pick the lowest-position state.

## 8. Skill ↔ Linear via MCP

- The orchestrator and step agents shall communicate with Linear exclusively via the official Linear MCP server.
- The Rust `LinearClient` shall be reserved for daemon-side use (polling, status sync) and shall not be invoked from skills or step agents.

## 9. Worktrees and branching

- The branch name for any RepoTask shall be the Linear ticket key (e.g., `ACM-123`).
- monorail shall resolve `<org>/<repo>` to a local path via `ghq` and shall lazily run `ghq get <org>/<repo>` when the repo is not yet cloned.
- monorail shall create per-Job worktrees via `wt` (worktrunk) following the configured worktree-path convention.
- When a doc reference of the form `<org>/<repo>` is resolved during an active Job, the resolver shall first try `<org>/<repo>.<TICKET>` and shall fall back to `<org>/<repo>`.

## 10. Per-repo isolation (hard contract)

- Every step-agent invocation shall operate within a single assigned worktree.
- A step agent shall not edit files outside its assigned worktree.
- When the orchestrator returns, the daemon shall run `git status` over every worktree in the Job and shall override the orchestrator's outcome to `escalated` with reason `CrossRepoLeak` if any non-assigned worktree shows changes.
- The daemon shall stash leaked changes for inspection without applying them.
- Every phase invocation shall receive a read-only "Job context" block describing sibling RepoTasks (phase, PR URL, shared facts, plan excerpt).

## 11. External tool dependencies

- monorail shall fail fast at startup with an actionable message naming any missing or unauthenticated required tool.
- monorail shall require `git`, `gh`, `ghq`, `wt`, and `claude` to be present and authenticated in any environment in which it runs.

## 12. State persistence

- monorail shall persist Job, RepoTask, deps, questions, events, and escalations in a single SQLite database.
- monorail shall never write secrets to the SQLite database or to logs.
- monorail shall scrub log output on a configured allowlist of fields.
- The per-RepoTask attempt counters (`review_attempts`, `lint_test_attempts`, `ci_fix_attempts`) shall be advisory only; the authoritative attempt counts shall come from `MONORAIL_RESULT.attempts`.

## 13. Plan YAML and multi-repo (deferred — v1 spec, future implementation)

- A ticket's `## Monorail Plan` YAML block, when present, shall enumerate `repos[]` with `org`, `name`, optional `after`, `wait_for`, `consumes`, and `anchors`.
- The `wait_for.type` field shall be one of `merged`, `ci-success`, `ci-preview-branch`, `release-published`, or `manual`.
- When `anchors` is omitted, monorail shall derive anchors from the diff's common ancestor post-implementation or default to the worktree root pre-implementation.
- Each RepoTask shall advance independently up to `PrOpened`, gated by its declared `deps` and `wait_for`.
- monorail v1 shall not implement multi-repo execution; the contract above is the target shape under roadmap ID `multi-repo`.

## 14. Escalation

- When any pre-PR phase escalates, the orchestrator shall not push commits and shall not open a PR.
- When the CI-fix phase escalates, the orchestrator shall leave the PR open.
- On every escalation, the daemon shall post a Linear comment summarizing the situation, shall surface the Job in the TUI's "Needs help" view (where TUI is enabled), and shall leave the worktree intact.
- A user comment matching `monorail: resume` on the Linear ticket shall trigger resumption when the daemon is in daemon mode.

## 15. Concurrency

- monorail's daemon shall bound concurrent Jobs by a configurable `max_jobs` value (default 3).
- monorail shall bound concurrent engine subprocesses by a configurable `max_engine_subprocesses` value (default 2).

## 16. Configuration

- monorail shall read its global configuration from `~/.config/monorail/config.toml`.
- monorail shall not require any per-repo `monorail.toml` file; per-repo behavior shall be discovered from existing AI-facing docs and build-signal files.
- When `.monorail/hooks/<event>.sh` is present at user, repo, or anchor layer, monorail shall execute existing layers in user→repo→anchor order, and any non-zero exit shall abort the phase.

## 17. Out of scope (v1)

- Non-Linear ticket sources (Jira, GitHub Issues, Notion).
- Engines other than Claude Code.
- Human channels other than Linear comments.
- Browser UI.
- Cross-repo atomic merge trains.
- Auto-rollback on production-incident detection.
- Project-level EARS-spec propagation (`project-spec-sync`).

---

## Notes (informative)

- The orchestrator's phase order in §4 lists self-review twice deliberately, matching the pivot spec's §3.1: a first self-review immediately after implement (correctness fixes), then acceptance review as a fail-fast gate, then a second self-review focused on code-quality findings before lint/test.
- §7 reflects two amendments to the original §6.3 phase-↔-status map: the 2026-04-28 collapse (only `started` and `completed`) and the 2026-04-30 split (orchestrator owns synchronous `In Progress` / `In Review`; daemon owns asynchronous `Done` / `Canceled`).
- §13 (multi-repo) is included for forward compatibility — the criteria are normative for the eventual implementation but the v1 daemon does not exercise them.
- §6's `MONORAIL_RESULT` schema is the trust boundary on which §3's `verification.all_satisfied` is enforced for Linear `Done` gating.
