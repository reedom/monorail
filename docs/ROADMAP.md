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

---

## Deferred items (canonical list)

Each row is a unit of future work. The "depends on" column tracks roadmap IDs that must land first.

| ID | Description | Spec § | Depends on | Notes |
|---|---|---|---|---|
| `type-b-planning` | Human-conversation planning loop before implement; `Phase::Planning`, `Question`/`Answer` types already declared | §1 vision (work archetype B), §7.1 | — | Resumable wait-state in SQLite. Linear comment thread = Q/A channel. |
| `multi-repo` | One Job → many RepoTasks; parse multiple `Repo:` lines or DAG with `after:` / `wait_for:`; per-repo isolation hard contract | §5.1, §7.5, §7.6 | — | The original cross-repo motivation. `RepoTask.anchors` field reserved. |
| `auto-merge` | Consume `monorail:auto-merge` label after CI green | §6.1 | — | Label parsed today; never acted on. Small. |
| `worktree-cleanup` | Call `WtTool::remove` after merge / abandonment | — | `auto-merge` (some flows) | Trait method exists, never called. |
| `layered-monorail-conf` | Walk-up `.monorail/` resolution: prompts (concat), hooks (execute), skills (first-match) | §13.1 | — | Per-repo customization without `monorail.toml`. |
| `global-config` | `~/.config/monorail/config.toml` + state-name overrides for Linear sync | §13.2 | — | Replaces env-var-only config. Unblocks Linear state-name overrides. |
| `tui` | ratatui observer + intervention UI (sshable) | §12 | — | Reads from SQLite `events` table. Multi-repo aware. |
| `doc-subsystem` | Frontmatter-managed agent-facing docs (skills/prompts/hooks index) | §9 | `layered-monorail-conf` | Hybrid trigger model. |
| `container` | Container image for daemon mode (GCP) | §11.1 | — | Plus systemd-style supervisor. |
| `concurrency` | Multiple Jobs simultaneously; per-repo lock to enforce isolation | §11.3 | `multi-repo` | SQLite write contention bound. |
| `engine-alts` | Codex / API / composite engines | §3.2 (adapter pattern), §16 (out of scope v1) | — | Trait already exists. |
| `linear-state-overrides` | Allow naming specific Linear states (e.g., `LINEAR_STATE_IN_PROGRESS=Doing`) | §6.3 hybrid | `global-config` | Promotes Plan 2's type-only discovery to (c) hybrid. |
| `phase-linear-extras` | Per-phase Linear status (e.g., move to `In Review` on PR opened) | §6.3 | `linear-state-overrides` | Adds per-phase mapping beyond started/completed. |

---

## Open questions (no plan yet)

- **Multi-repo prompts:** when one ticket spans repo A and repo B, where does the prompt live? §13.1 layered resolution helps but doesn't fully resolve the question — see Plan 1 brainstorm thread on this. Capture decision in `multi-repo` spec.
- **Concurrency model:** single-process tokio scheduler, or multi-process daemon? Affects `container` and `tui` design.
- **Doc trigger hybrid:** what events fire doc regeneration? §9.3 has options; pick before starting `doc-subsystem`.

---

## Provisional plan numbering

Order is by payoff and dependency. Reorder freely; each plan claims a number when it's brainstormed.

| # | Tentative scope | Roadmap IDs |
|---|---|---|
| Plan 3 | Type B planning loop | `type-b-planning` |
| Plan 4 | Multi-repo + DAG + per-repo isolation | `multi-repo` (likely splits into 4a/4b) |
| Plan 5 | Auto-merge + worktree cleanup | `auto-merge`, `worktree-cleanup` |
| Plan 6 | Layered `.monorail/` + global config | `layered-monorail-conf`, `global-config`, `linear-state-overrides` |
| Plan 7 | TUI | `tui` |
| Plan 8 | Doc subsystem | `doc-subsystem` |
| Plan 9 | Container + concurrency | `container`, `concurrency` |
| Plan 10+ | Engine alternatives, phase-linear-extras | `engine-alts`, `phase-linear-extras` |

This supersedes the per-plan footnotes in earlier specs that named "Plan 3 = config" / "Plan 4 = TUI" / "Plan 5 = container" / "Plan 6 = doc subsystem". Those numbers shifted when Plan 2 became Linear status sync.

---

## How to use this file

- **Starting a new plan:** copy the relevant deferred row(s) into the new spec; reference by roadmap ID. Don't add net-new scope without adding a roadmap row first.
- **Finishing a plan:** update the status snapshot, remove the consumed deferred rows (or leave a "covered by Plan N" note if a row only partially landed).
- **Discovered work mid-plan:** add a new deferred row immediately rather than expanding the current plan.
