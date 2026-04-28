# monorail Roadmap

Canonical list of plans, deferred items, and current state. Each completed plan updates this file.

**Rule:** every spec/plan must reference items by their roadmap ID (e.g., `multi-repo`, `tui`), not by plan number. Plan numbers are mutable; IDs are stable.

---

## Status snapshot

| Plan | Title | Status | Branch / merge |
|---|---|---|---|
| Plan 1 | Type A single-repo end-to-end | shipped | `main` (merge `6cf8376`) |
| Plan 2 | Linear status sync + Plan-1 polish | shipped | `main` (merge `47239a7`) |

**Tests on main:** 51 unit + 3 e2e = 54 passing. Build warnings: 0.
**Tests on `claude/practical-dhawan-7e6e37` (current pivot branch):** 52 unit + 3 e2e + 2 ignored live tests.

---

## Architecture decisions

| Date | Decision | Spec |
|---|---|---|
| 2026-04-28 | **Small-daemon / skill-first.** Daemon shrinks to triage + state + worktree + multi-repo gating + Linear job-level sync. Two orchestrator skills (`monorail:run-bug`, `monorail:run-feature`) live in this repo's plugin and drive step agents. Loops (self-review, lint/test, ci-fix) move from Rust pipeline into skills. Skill ↔ Linear via official Linear MCP (user-configured). | `docs/superpowers/specs/2026-04-28-small-daemon-skill-first.md` |

This decision **amends sections of `2026-04-27-monorail-design.md`** — see that spec's §3.2, §6.3, §7.2-7.4, §10, §12, §13.1. The original design doc is not deleted; the pivot spec calls out which parts are superseded.

---

## Deferred items (canonical list)

Each row is a unit of future work. The "depends on" column tracks roadmap IDs that must land first.

| ID | Description | Spec § | Depends on | Notes |
|---|---|---|---|---|
| `monorail-plugin-skills` | `monorail:run-bug` and `monorail:run-feature` orchestrator skills under `.claude/plugins/monorail/skills/` | pivot spec §3, §4 | — | Two skills only; loops live inside them. |
| `monorail-plugin-agents` | Step agents (`monorail-implement`, `-self-review`, `-fix-finding`, `-lint-test`, `-open-pr`, `-ci-fix`, `-plan-with-human`) under `.claude/plugins/monorail/agents/` | pivot spec §4, §5 | `monorail-plugin-skills` | Each agent is a single-step worker; orchestration stays in skill. |
| `daemon-skill-contract` | Wire daemon to invoke a skill via `claude -p "/monorail:run-bug TICKET"` and parse `MONORAIL_RESULT:` JSON; replace per-phase Engine trait calls | pivot spec §6 | `monorail-plugin-skills` | Engine trait shrinks from 5 methods to 1-2. |
| `pipeline-prune` | Remove `src/pipeline/{self_review,lint_test,ci_fix}.rs`, per-phase counters in `repo_tasks`, prompt strings in `engine/claude_code.rs` | pivot spec §10 | `daemon-skill-contract` | Wait until skill route is reliable; current code stays as safety net. |
| `type-b-planning` | Type B human planning loop, implemented as `monorail:run-feature` skill + `monorail-plan-with-human` agent | pivot spec §3, §4 | `monorail-plugin-skills` | Skill talks to Linear via MCP for the Q&A thread. |
| `multi-repo` | One Job → many RepoTasks; parse multiple `Repo:` lines or DAG with `after:` / `wait_for:`; per-repo isolation hard contract | original §5.1, §7.5, §7.6 | `daemon-skill-contract` | The original cross-repo motivation. `RepoTask.anchors` field reserved. |
| `auto-merge` | Consume `monorail:auto-merge` label after CI green | original §6.1 | `daemon-skill-contract` | Label parsed today; never acted on. Daemon decides merge after seeing skill outcome `pr_opened` + CI green. |
| `worktree-cleanup` | Call `WtTool::remove` after merge / abandonment | — | `auto-merge` (some flows) | Trait method exists, never called. |
| `global-config` | `~/.config/monorail/config.toml` + state-name overrides for Linear sync | original §13.2 | — | Replaces env-var-only config. Unblocks Linear state-name overrides. |
| `linear-state-overrides` | Allow naming specific Linear states (e.g., `LINEAR_STATE_IN_PROGRESS=Doing`) | original §6.3 hybrid | `global-config` | Mostly addressed by position-sort (commit `c4318e6`); only needed if a workspace has multiple started states with confusing names. |
| `tui` | ratatui observer + intervention UI (sshable) | original §12 | — | Reads from SQLite `events` table. Multi-repo aware. Phase counters less granular under skill model. |
| `doc-subsystem` | Frontmatter-managed agent-facing docs (skills/prompts/hooks index) | original §9 | — | Hybrid trigger model. Less coupled to monorail under skill-first. |
| `container` | Container image for daemon mode (GCP) | original §11.1 | `daemon-skill-contract` | Plus systemd-style supervisor. Image must include `claude` CLI + Linear MCP config. |
| `concurrency` | Multiple Jobs simultaneously; per-repo lock to enforce isolation | original §11.3 | `multi-repo` | SQLite write contention bound. |
| `engine-alts` | Codex / API / composite engines | original §3.2 (adapter pattern), §16 (out of scope v1) | `daemon-skill-contract` | Trait already exists. Under skill-first, alternative engines also need a skill-execution surface. |
| `phase-linear-extras` | Per-phase Linear status (e.g., move to `In Review` on PR opened) | original §6.3 | `linear-state-overrides`, `daemon-skill-contract` | Requires skill to emit intermediate phase events to daemon. |
| `engine-permission-policy` | Replace temporary `--permission-mode bypassPermissions` patch with proper allowlist via `.claude/settings.json` checked in | — | `daemon-skill-contract` | Adapter currently has a debug bypass on `claude/practical-dhawan-7e6e37` only; do not merge to main as-is. |
| `layered-monorail-conf` | ~~Walk-up `.monorail/` resolution~~ | original §13.1 | — | **Likely obsolete under skill-first.** Skills inspect `CLAUDE.md` / `AGENTS.md` / build files directly (original §13.1 already named this as the primary path). Keep the row only for `.monorail/hooks/` if pre/post-skill side effects become useful. |

---

## Open questions (no plan yet)

- **Multi-repo prompts:** when one ticket spans repo A and repo B, where does the prompt live? Original §13.1 layered resolution helps but doesn't fully resolve the question. Under skill-first, the skill itself reads each repo's `CLAUDE.md` per worktree — re-evaluate before starting `multi-repo`.
- **Concurrency model:** single-process tokio scheduler, or multi-process daemon? Affects `container` and `tui` design.
- **Doc trigger hybrid:** what events fire doc regeneration? Original §9.3 has options; pick before starting `doc-subsystem`.
- **Skill→daemon intermediate events:** if `phase-linear-extras` is wanted, how does the skill emit incremental phase events back to daemon (stderr line markers? a file? a small `monorail event` CLI subcommand)? Defer until that plan is brainstormed.

---

## Provisional plan numbering

Order is by payoff and dependency. Reorder freely; each plan claims a number when it's brainstormed.

| # | Tentative scope | Roadmap IDs |
|---|---|---|
| Plan 3 | Skill scaffold + Type A end-to-end via skill | `monorail-plugin-skills` (run-bug only), `monorail-plugin-agents` (Type A subset), `daemon-skill-contract` |
| Plan 4 | Type B planning via skill | `type-b-planning`, `monorail-plugin-skills` (run-feature) |
| Plan 5 | Pipeline prune + permission policy | `pipeline-prune`, `engine-permission-policy` |
| Plan 6 | Multi-repo + DAG + per-repo isolation | `multi-repo` (likely splits into 6a/6b) |
| Plan 7 | Auto-merge + worktree cleanup | `auto-merge`, `worktree-cleanup` |
| Plan 8 | Global config | `global-config`, `linear-state-overrides` |
| Plan 9 | TUI | `tui` |
| Plan 10 | Doc subsystem | `doc-subsystem` |
| Plan 11 | Container + concurrency | `container`, `concurrency` |
| Plan 12+ | Engine alternatives, phase-linear-extras | `engine-alts`, `phase-linear-extras` |

This supersedes the prior plan numbering. The architecture pivot consumed the previous "Plan 3 = Type B planning" slot — Type B now requires `monorail-plugin-skills` to land first, so it shifts to Plan 4.

---

## How to use this file

- **Starting a new plan:** copy the relevant deferred row(s) into the new spec; reference by roadmap ID. Don't add net-new scope without adding a roadmap row first.
- **Finishing a plan:** update the status snapshot, remove the consumed deferred rows (or leave a "covered by Plan N" note if a row only partially landed).
- **Discovered work mid-plan:** add a new deferred row immediately rather than expanding the current plan.
- **Architecture decisions:** record in the "Architecture decisions" table with a spec link before propagating consequences across deferred rows.
