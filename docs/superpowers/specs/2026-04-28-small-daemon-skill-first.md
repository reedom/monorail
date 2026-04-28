# monorail Architecture Pivot — Small Daemon, Skill-First

- **Date**: 2026-04-28
- **Status**: Accepted (decision recorded in [ROADMAP.md](../../ROADMAP.md#architecture-decisions))
- **Amends**: `docs/superpowers/specs/2026-04-27-monorail-design.md` §3.2, §6.3, §7.2-7.4, §10, §12, §13.1

> Naming note: This spec uses the same placeholder repo names (`acme/...`)
> and ticket prefix (`ACM-`) as the original design doc.

---

## 1. Why this pivot

### 1.1 What we observed

Plans 1 and 2 built a Rust pipeline that drives `claude -p ...` subprocesses
through phases (implement → self-review → lint/test → PR → ci-fix). Each
phase has its own loop, its own attempt counter, its own prompt template
hardcoded in `src/engine/claude_code.rs` (~150 lines of `format!` strings),
and its own pipeline module under `src/pipeline/`.

Two friction points surfaced during live testing of `RDM-5` against the
real Linear API:

1. **Permission policy.** `claude -p` in headless mode requires either an
   explicit allowlist or a permission bypass to do any meaningful work.
   The current code passes nothing, so a temporary
   `--permission-mode bypassPermissions` patch was added on this pivot
   branch as a placeholder. A "real" permission policy is non-trivial
   because the daemon's loops can't predict every tool the engine will
   need across a multi-attempt review-and-fix flow.
2. **Loop duplication.** Claude Code already runs review loops, test
   loops, and CI-fix loops natively when prompted to. The Rust pipeline
   re-implements these in coarse form via repeated subprocess calls
   with separate prompts. Each iteration loses context; each prompt
   reinstates it from scratch via `format!`.

### 1.2 What this implies

The Rust pipeline is doing work that Claude Code is structurally better
at. The daemon's value is elsewhere — the parts that **must run when no
Claude Code session is alive**:

- Polling Linear for new tickets at scheduled intervals (daemon mode).
- Maintaining durable state across machine restarts (SQLite).
- Materializing worktrees and enforcing per-repo isolation.
- Coordinating multi-repo DAGs (Job → N RepoTasks).
- Setting Linear workflow states at the macro boundaries of a job.
- Running concurrently (multiple jobs in flight).

Everything else — the actual code-changing work and the per-step quality
loops — fits naturally in a Claude Code session driven by skills and
agents.

### 1.3 The pivot

Move the loops and prompts into Claude Code skills and agents in this
repo's plugin (`.claude/plugins/monorail/`). The daemon dispatches one
skill per Job archetype (bug / feature) and observes its terminal
outcome via a structured contract. The pipeline modules in Rust stay as
a safety net until the skill route is proven; then they're pruned.

This is **not** a code rewrite. The Linear client, state machine,
triager, worktree backend, and multi-repo scaffolding all stay. The
Engine adapter shrinks from a 5-method interface to a 1-2 method
"invoke skill, parse result" surface.

---

## 2. New architecture

### 2.1 Three layers

```
┌──────────────────────────────────────────────────────────┐
│  Daemon (Rust binary)                                    │
│    • Triager (poll Linear, build Job)                    │
│    • State (SQLite: jobs, repo_tasks, events)            │
│    • Worktree (wt + ghq)                                 │
│    • Multi-repo DAG (deps, wait_for, isolation)          │
│    • Linear sync at job-level (started / completed)      │
│    • Engine adapter: invokes skill, parses result        │
└────────────────────────┬─────────────────────────────────┘
                         │ claude -p "/monorail:run-bug TICKET"
                         ▼
┌──────────────────────────────────────────────────────────┐
│  Skill (orchestrator, per Job archetype)                 │
│    • monorail:run-bug    (Type A, no human)              │
│    • monorail:run-feature (Type B, with human)           │
│  Responsibilities:                                       │
│    • Loop control (self-review max 5, lint/test max 5,   │
│      ci-fix max 3)                                       │
│    • Phase sequencing                                    │
│    • Verify-cmd discovery (CLAUDE.md, Makefile, ...)     │
│    • Emit MONORAIL_RESULT: {...} on completion           │
└────────────────────────┬─────────────────────────────────┘
                         │ Task tool
                         ▼
┌──────────────────────────────────────────────────────────┐
│  Agents (workers, single-step)                           │
│    monorail-implement, monorail-self-review,             │
│    monorail-fix-finding, monorail-lint-test,             │
│    monorail-open-pr, monorail-ci-fix,                    │
│    monorail-plan-with-human                              │
│  Each agent has a tight system prompt and tool scope     │
│  for one step. Fresh context per invocation.             │
└──────────────────────────────────────────────────────────┘
```

### 2.2 What stays in the daemon (unchanged)

- `src/triager.rs` — label parsing, work-type classification, Job
  materialization from Linear.
- `src/linear/` — `LinearClient`, `LinearStateResolver`, `WorkflowState`.
  Used for daemon polling and job-level status sync (NOT used by skills;
  skills go through Linear MCP).
- `src/state/` — SQLite schema for jobs, repo_tasks, events. Per-phase
  attempt counters (`review_attempts`, `lint_test_attempts`,
  `ci_fix_attempts`) are deferred for prune (§10) but stay during
  migration as safety-net columns.
- `src/tools/{ghq,wt,gh}.rs` — external CLI wrappers.
- `src/domain/` — types like `Job`, `RepoTask`, `Phase`, `Finding`,
  `RootCauseAnalysis`. `Phase` shrinks (§10) but is not deleted.
- Multi-repo DAG resolution + per-repo isolation post-flight check
  (original §7.5).
- Linear status sync at job-level (§8 of this spec).

### 2.3 What moves out of the daemon

| From | To | Notes |
|---|---|---|
| `src/engine/claude_code.rs` prompt strings (~150 lines of `format!`) | Skill / agent markdown files | The prompts become the skill/agent system prompts directly. |
| `src/pipeline/self_review.rs` loop body | `monorail:run-bug` skill | Skill calls `monorail-self-review` agent; if findings exist, calls `monorail-fix-finding` agent per finding; loops up to 5 times. |
| `src/pipeline/lint_test.rs` loop body | `monorail:run-bug` skill | Skill calls `monorail-lint-test` agent; agent runs verify cmd and fixes failures internally; skill bounds outer attempts. |
| `src/pipeline/ci_fix.rs` loop body | `monorail:run-bug` skill | Same shape as lint/test, but reads CI logs via `gh` (skill calls `monorail-ci-fix` agent). |
| `Engine` trait's 5 methods (`implement`, `review`, `analyze_finding`, `apply_fix`, `fix_failure`) | 1-2 methods (`run_skill`) | See §6 for the new contract. |

---

## 3. Skill catalog

Two skills, both top-level orchestrators. Names use the `monorail:`
namespace (entire skill name, including the colon, is the activation
trigger).

### 3.1 `monorail:run-bug`

**Purpose**: Run a Type A ticket end-to-end without human intervention.

**Phases (sequential)**:

1. **Implement** — invoke `monorail-implement` agent with the worktree
   path, ticket key, and ticket description. Agent makes changes, returns
   summary.
2. **Self-review loop (max 5)**:
   1. Invoke `monorail-self-review` agent → returns list of findings.
   2. For each finding, invoke `monorail-fix-finding` agent → returns
      `applied | dismissed`.
   3. If any fix was applied, repeat from (1). If none applied, exit
      loop. If 5 iterations reached with fixes still being applied,
      escalate.
3. **Lint/test loop (max 5)** — invoke `monorail-lint-test` agent. Agent
   discovers verify command, runs it, fixes failures, returns
   `green | red`. If red after 5 attempts, escalate.
4. **Open PR** — invoke `monorail-open-pr` agent. Returns `{pr_url}`.
5. **CI fix loop (max 3)** — invoke `monorail-ci-fix` agent. Polls
   GitHub Actions until checks finish, fixes failures, pushes. If
   checks fail after 3 attempts, escalate (PR remains open per original
   §8 contract).

**Result**: emits `MONORAIL_RESULT: {...}` (§6.2) and exits.

### 3.2 `monorail:run-feature`

**Purpose**: Run a Type B ticket with a human-in-the-loop planning phase.

**Phases**:

1. **Plan with human** — invoke `monorail-plan-with-human` agent. Agent
   uses Linear MCP to post questions as ticket comments and poll for
   replies. When the human approves a plan, agent writes the plan back
   to the ticket body as a `## Monorail Plan` YAML section (per
   original §6.2) and exits.
2. **Phases 2–5** — identical to `monorail:run-bug` from the implement
   phase onward.

**Result**: same `MONORAIL_RESULT` shape as `run-bug`.

---

## 4. Agent catalog

Each agent is a single-step worker. Tools are scoped tightly per agent.
All agents accept context from the orchestrating skill (worktree path,
ticket key, plan excerpt, prior step results) and return a structured
result the skill can act on.

| Agent | Inputs | Output | Tools |
|---|---|---|---|
| `monorail-implement` | worktree path, ticket key, instructions | summary text | Read, Edit, Write, Bash, Grep, Glob |
| `monorail-self-review` | worktree path, ticket key | JSON list of findings (id, file, line, severity, message) | Read, Grep, Bash(`git diff`) |
| `monorail-fix-finding` | worktree path, finding object | `{ applied: bool, reason: string }` | Read, Edit, Write, Bash |
| `monorail-lint-test` | worktree path, prior failure log (optional) | `{ outcome: "green"|"red", log: string }` | Read, Edit, Write, Bash |
| `monorail-open-pr` | worktree path, ticket key, summary | `{ pr_url: string }` | Bash(`gh pr create`, `git push`) |
| `monorail-ci-fix` | worktree path, ticket key, pr_url | `{ outcome: "green"|"red", attempts: int }` | Read, Edit, Write, Bash(`gh`) |
| `monorail-plan-with-human` | ticket key | `{ plan_yaml: string, approved: bool }` | Read, Bash, Linear MCP tools |

Agents do **not** know about each other. The skill is the only coordinator.

### 4.1 Verify-cmd discovery

`monorail-lint-test` (and `monorail-ci-fix` to a lesser extent) needs to
know the project's verify command. Discovery, in priority order:

1. `CLAUDE.md` / `AGENTS.md` at the worktree root for explicit verify
   commands.
2. `Makefile` target named `verify` / `test` / `check`.
3. `package.json` scripts (`test`, `lint`, `typecheck`).
4. `Cargo.toml` → `cargo test && cargo clippy -- -D warnings`.
5. `pyproject.toml` → `pytest` / `ruff` / `mypy` per project hints.

This obsoletes original §13.1's `.monorail/prompts/<phase>.md` plan —
the agent reads what already exists for AI consumers, no per-repo
monorail-specific config required.

---

## 5. File layout

All skills and agents are checked into this repo at:

```
.claude/
└── plugins/
    └── monorail/
        ├── plugin.json                        # plugin manifest
        ├── skills/
        │   ├── run-bug.md
        │   └── run-feature.md
        └── agents/
            ├── implement.md
            ├── self-review.md
            ├── fix-finding.md
            ├── lint-test.md
            ├── open-pr.md
            ├── ci-fix.md
            └── plan-with-human.md
```

Worktrees inherit this directory automatically (it's checked into git).
When the daemon runs `claude -p "/monorail:run-bug RDM-5"` with cwd set
to a worktree, Claude Code auto-discovers the plugin from
`<worktree>/.claude/plugins/monorail/`.

**Linear MCP** is **not** committed. It's a per-developer / per-machine
setup (official Linear MCP at `https://mcp.linear.app`). README will
document how to configure it. The `monorail-plan-with-human` agent
expects Linear MCP tools to be available; if not, it errors fast.

---

## 6. Daemon ↔ Skill contract

### 6.1 Invocation

The daemon's Engine adapter calls:

```
claude -p "/monorail:run-bug ACM-123" \
       --permission-mode bypassPermissions \
       --output-format text
```

Working directory is the worktree path. Environment includes
`LINEAR_API_KEY` (passed through to Linear MCP if configured),
`GITHUB_TOKEN` (consumed by `gh`), and any custom env from the daemon
config.

### 6.2 Result format

The skill's final stdout line MUST start with `MONORAIL_RESULT:` followed
by a JSON object. Schema:

```json
{
  "outcome": "pr_opened" | "merged" | "escalated" | "failed",
  "phase":   "plan" | "implement" | "self_review" | "lint_test" |
             "open_pr" | "ci_fix" | null,
  "pr_url":  "https://github.com/..." | null,
  "summary": "human-readable single-paragraph summary",
  "reason":  "non-null only when outcome ∈ {escalated, failed}",
  "attempts": { "self_review": 2, "lint_test": 1, "ci_fix": 0 }
}
```

`phase` is the phase the skill was *in* when it terminated:

- `outcome=pr_opened`: phase=`open_pr` (or `ci_fix` if ci-fix completed but the daemon hasn't merged yet).
- `outcome=escalated`: phase=the phase that gave up. `reason` describes why.
- `outcome=failed`: phase=where execution failed. `reason` is the error.
- `outcome=merged`: only if the skill performed an auto-merge itself, which v1 does not — daemon does merges. So in v1 the skill never returns `merged`.

### 6.3 Exit code

| Code | Meaning |
|---|---|
| 0 | `outcome ∈ {pr_opened, merged}` |
| 1 | `outcome=escalated` |
| 2 | `outcome=failed` |
| 3+ | Reserved |

The daemon parses the last `MONORAIL_RESULT:` line for definitive
outcome. Exit code is a coarse guardrail (e.g., for shell-level CI).

### 6.4 What the daemon does with the result

| Result | Daemon action |
|---|---|
| `pr_opened` | Set Linear state to `started`-or-keep, record PR URL, emit `pr_opened` event. If `auto_merge` label set, daemon initiates merge after CI green (separate from skill). |
| `merged` (v1: never) | Set Linear `completed`. |
| `escalated` | Leave job in `Escalated` state, post Linear comment with `reason`, surface in TUI. Do NOT close PR (per original §8 — pre-PR escalations have no PR; CI-fix escalation keeps the PR open). |
| `failed` | Treat as `escalated` with a failure-flavored comment. Distinguish for telemetry. |

### 6.5 Per-repo isolation enforcement (post-flight)

Original §7.5 hard contract still applies. After the skill returns,
the daemon runs `git status` (or equivalent) over **all** worktrees in
the Job. If any worktree other than the assigned one shows changes, the
skill outcome is overridden to `escalated` with reason
`CrossRepoLeak`. The leaked changes are stashed for inspection. This
is the daemon's safety belt against a misbehaving skill.

---

## 7. Skill ↔ Linear via MCP

### 7.1 Decision

The skill (specifically the `monorail-plan-with-human` agent) talks to
Linear via the **official Linear MCP server** at `https://mcp.linear.app`.
Configuration is per-developer / per-machine; the user adds it to their
Claude Code MCP settings once.

### 7.2 Why MCP, not a `monorail` CLI subcommand

Considered alternatives:
- (a) Skill posts raw GraphQL via `curl` from Bash → re-implements retry
  / error handling / type structure inside skill prompts. Fragile.
- (b) Add `monorail linear post-comment ...` CLI subcommands → reuses
  the proven Rust `LinearClient` and live-test coverage, but creates a
  new facade and version-skew risk between daemon and skill.
- (c) Linear MCP → no new code, standard tool surface, both daemon-free
  and self-contained from the skill's perspective.

(c) won. The Rust `LinearClient` stays exclusive to daemon use
(polling, status sync) — no facade.

### 7.3 What the skill actually does with Linear

- `monorail-plan-with-human` agent: post questions as ticket comments,
  poll for replies, post the agreed-upon plan YAML back to the ticket
  body. Uses Linear MCP `create_comment`, `list_comments`, `update_issue`
  (tool names depend on the MCP server's surface; the agent prompt
  abstracts over them).
- All other agents: do **not** touch Linear. Implementation, review,
  tests, PR, and CI fixes operate on the worktree and GitHub only.

---

## 8. Linear status sync (job-level only)

Original §6.3 mapped phases to Linear states:

| original phase | Linear status |
|---|---|
| picked-up | `In Progress` |
| pr-opened | `In Review` |
| merged | `Done` |

Under skill-first, the daemon doesn't observe phase transitions
granularly — it only sees the skill's terminal outcome. So Linear sync
collapses to:

| Daemon event | Linear status |
|---|---|
| Skill dispatched | `started` (e.g., `In Progress`) |
| Skill returned `pr_opened` (auto_merge=false) | unchanged (PR is opened; "Done" is set when the user merges) |
| Skill returned `pr_opened` AND daemon completes auto-merge | `completed` (e.g., `Done`) |
| Skill returned `escalated` / `failed` | unchanged; comment posted with reason |

If `phase-linear-extras` (e.g., setting `In Review` on PR opened) is
later wanted, the skill emits intermediate phase events back to the
daemon (mechanism TBD — see §13).

The position-sort fix in commit `c4318e6` already addresses the most
common state-mapping concern (multiple states share `kind=started`,
pick the lowest-position one to match Linear's UI).

---

## 9. Per-repo isolation (still daemon-enforced)

§6.5 above re-states this: the daemon's post-flight `git status` check
across all Job worktrees is the contract enforcement mechanism. The
skill prompts include the original §7.6 read-only context block
("other repos in this job"), but the **enforcement** is daemon-side, not
trust-based.

This separation is exactly the value the daemon retains in the
skill-first world: cross-repo coordination and isolation are jobs no
single Claude Code session can do reliably alone.

---

## 10. State machine simplification

Per-phase attempt counters in `repo_tasks` (`review_attempts`,
`lint_test_attempts`, `ci_fix_attempts`) become advisory only. The
authoritative attempt counts now live inside the skill's loop control
and arrive in `MONORAIL_RESULT.attempts`.

`Phase` enum (`Pending`, `Planning`, `Implementing`, `SelfReviewing`,
`LintTesting`, `PrOpened`, `CiFixing`, `Merged`, `Aborted`, `Escalated`)
collapses for daemon purposes:

- Daemon-observable phases: `Pending`, `Dispatched`, `PrOpened`,
  `Merged`, `Escalated`, `Aborted`. Internal phases (`Implementing`,
  `SelfReviewing`, `LintTesting`, `CiFixing`) become invisible to the
  daemon — they live inside the skill's session.
- The full enum stays in code as a safety net during migration. Once
  `pipeline-prune` lands, the unused variants get `#[allow(dead_code)]`
  and eventually removed.

---

## 11. Migration plan

This pivot does not require rewriting Plans 1 and 2. The migration is
incremental:

1. **(this spec)** Land the architecture pivot in docs.
2. **Plan 3** Build `.claude/plugins/monorail/` skeleton with
   `monorail:run-bug` skill and the Type A subset of agents. Implement
   `daemon-skill-contract` (`Engine::run_skill`). Run RDM-5 end-to-end
   via the new path against the live API. Existing Rust pipeline stays
   in place, unused but not deleted.
3. **Plan 4** Add `monorail:run-feature` and `monorail-plan-with-human`.
   Type B becomes available.
4. **Plan 5** `pipeline-prune` + `engine-permission-policy`. Delete the
   old pipeline modules and prompt strings. Replace bypass-perm patch
   with a proper `.claude/settings.json` allowlist (or, if skills set
   their own permissions inline via frontmatter, drop the daemon-side
   permission flag entirely).
5. **Plans 6+** continue per ROADMAP.

Until Plan 5 lands, both routes coexist: the daemon could be
configured (env var or feature flag) to dispatch via skill or via the
old pipeline. This is the safety net.

---

## 12. ROADMAP impact

See [ROADMAP.md](../../ROADMAP.md). Concretely:

- Architecture decisions table: this spec is recorded with date
  2026-04-28.
- New deferred rows: `monorail-plugin-skills`, `monorail-plugin-agents`,
  `daemon-skill-contract`, `pipeline-prune`,
  `engine-permission-policy`.
- `type-b-planning` redefined to depend on `monorail-plugin-skills`.
- Most other deferred rows now depend on `daemon-skill-contract`
  (multi-repo, auto-merge, container, engine-alts, phase-linear-extras).
- `layered-monorail-conf` marked likely-obsolete.
- Plan numbering: Plan 3 = skill scaffold, Plan 4 = Type B, Plan 5 =
  prune, Plan 6 = multi-repo, Plan 7 = auto-merge + cleanup, Plan 8 =
  config, Plan 9 = TUI, Plan 10 = doc, Plan 11 = container, Plan 12+ =
  alts / extras.

---

## 13. Open questions / out of scope

- **Skill→daemon intermediate events.** If `phase-linear-extras` is
  wanted (e.g., set `In Review` the moment PR opens, before skill
  exits), the skill needs to emit phase markers the daemon can observe
  before final exit. Options: stderr line markers parsed live, a small
  `monorail event <kind> <json>` CLI subcommand, an MCP tool the
  daemon hosts. Defer until the feature is concretely requested.
- **Skill cancellation / timeout.** The daemon needs to be able to
  abort a hung skill (e.g., a CI poll that won't terminate). v1
  approach: SIGTERM the `claude` subprocess after a configurable wall-clock
  timeout; the worktree is left intact; job marked escalated. Document
  this contract before Plan 3 ships.
- **Permissions policy specifics.** Once skills are real, refine
  `.claude/settings.json` allowlist instead of bypass. Capture in
  `engine-permission-policy`.
- **Skill testing.** Skill prompts can be unit-tested against fixture
  worktrees (snapshot the worktree state, run the skill, assert
  output). Frame this in Plan 3 as a recurring concern.
- **Multi-engine future.** `engine-alts` (Codex / API) implies the
  skill abstraction must be expressible in those engines too. Most
  likely each engine adapter "executes" the skill markdown its own way.
  Defer the trait shape until needed.

---

## 14. Sections of original design doc this supersedes

| Original §  | What changes |
|---|---|
| §3.2 Adapter pattern | `Engine` trait shrinks to `run_skill` + (optional) `cancel`. Old methods (`implement`, `review`, `analyze_finding`, `apply_fix`, `fix_failure`) are removed when `pipeline-prune` lands. |
| §6.3 Phase ↔ Linear status mapping | Daemon sets `started` and `completed` only. Per-phase mappings become a future feature (`phase-linear-extras`). |
| §7.2 Self-review loop | Loop body moves to `monorail:run-bug` skill. Daemon no longer increments per-phase counters. |
| §7.3 Lint/test loop | Same as §7.2; loop moves to skill. |
| §7.4 CI-fix loop | Same as §7.2; loop moves to skill. Daemon still observes terminal outcome. |
| §10 Persistence and State | `repo_tasks.review_attempts`, `lint_test_attempts`, `ci_fix_attempts` become advisory. `Phase` enum's internal variants (`Implementing` etc.) become daemon-invisible. |
| §12 TUI design | Per-phase counters less granular. Detail view shows `MONORAIL_RESULT.attempts` instead of column data. |
| §13.1 Per-repo configuration | `.monorail/prompts/<phase>.md` is unneeded — skill agents read CLAUDE.md / AGENTS.md / build files directly (§4.1). `.monorail/hooks/` may still be useful and is kept under review. |

The original design doc is **not deleted**. Its non-affected sections
(§1 vision, §2 goals, §4 architecture diagram, §5 worktree conventions,
§6.1 labels, §6.2 plan section, §7.1 phase order, §7.5 isolation, §7.6
cross-repo context, §8 escalation, §11 runtime, §13.2 global config,
§14 testing, §15-16) remain canonical.
