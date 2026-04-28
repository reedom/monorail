# monorail Design Document

- **Date**: 2026-04-27
- **Status**: Draft (post-brainstorming)
- **Repo (working dir)**: `monorail`

> Naming note: All repository identifiers in this document use placeholder
> names (`acme/core-api`, `acme/web-app`, `acme/proto-schema`, etc.) and the
> example ticket prefix `ACM-`. Real organizational names are intentionally
> excluded so this design can be published.

---

## 1. Vision

monorail is a Linear-driven autonomous development pipeline. Given a Linear
ticket, it picks the work up, plans (with or without a human), implements,
self-reviews, runs lint/test loops, opens a PR, fixes CI failures, and
optionally auto-merges — all without human intervention except where the
ticket explicitly requires design dialogue.

Two work archetypes are first-class:

- **Type A — Bug / small change**: end-to-end autonomous from ticket to PR
  (or merge).
- **Type B — Feature / design-required**: human-in-the-loop planning first,
  then autonomous from impl through PR (or merge).

monorail is **not** a chat agent and **not** a spec-coherence engine. It is a
job runner whose unit of work is a Linear ticket and whose terminal output is
a green PR.

## 2. Goals and Non-Goals

### Goals
- Reliable, observable end-to-end execution for Type A.
- Smooth human-in-the-loop planning for Type B without breaking the autonomous
  downstream pipeline.
- First-class **multi-repo** ticket support (one ticket touching N repos).
- Run identically as a long-lived daemon (container on GCP) and as a local
  manual CLI.
- Engine-agnostic core: Claude Code is the first-class adapter; other engines
  are pluggable.
- Channel-agnostic human I/O: Linear comments are the default; Slack / CLI
  prompt are pluggable adapters.
- Documentation freshness without polluting feature PRs.

### Non-Goals
- Replacing the IDE / interactive coding session.
- Generating product specs (Linear is the source of truth for requirements).
- Cross-engine quality parity at v1 (only Claude Code adapter is fully
  supported initially).
- Spec-driven coherence management in the codd sense (different problem).

## 3. Core Concepts

### 3.1 Job

A `Job` is the unit of execution. It corresponds 1:1 to a Linear ticket and
spans 1..N repositories.

```rust
struct Job {
    ticket: TicketKey,           // e.g. "ACM-123"
    work_type: WorkType,         // Bug | Feature
    repos: Vec<RepoTask>,        // multi-repo DAG nodes
    state: JobState,             // global state (see 6.1)
    plan: Plan,                  // parsed from Linear ticket body
    human_channel: ChannelId,    // selected HumanChannel adapter
    engine: EngineId,            // selected Engine adapter
    questions: VecDeque<Question>,
    auto_merge: bool,            // from monorail:auto-merge label
    created_at: DateTime,
    updated_at: DateTime,
}

struct RepoTask {
    org: String,                 // e.g. "acme"
    repo: String,                // e.g. "core-api"
    branch: String,              // == ticket key, e.g. "ACM-123"
    worktree_path: PathBuf,      // resolved via ghq + wt convention
    anchors: Vec<PathBuf>,       // worktree-relative; starting points
                                 // for .monorail/ resolution (§13.1).
                                 // Empty == derived (Plan YAML or diff
                                 // common ancestor or worktree root).
    phase: Phase,                // per-repo phase (independent)
    deps: Vec<RepoRef>,          // upstream RepoTask deps in this Job
    wait_for: Option<WaitCondition>,
    pr_url: Option<Url>,         // set after PR opened
    review_attempts: u8,
    lint_test_attempts: u8,
    ci_fix_attempts: u8,
}

enum Phase {
    Pending,
    Planning,
    Implementing,
    SelfReviewing,
    LintTesting,
    PrOpened,
    CiFixing,
    Merged,
    Aborted,
    Escalated { reason: EscalationReason },
}
```

### 3.2 Adapter pattern

Three pluggable surfaces:

```rust
trait Engine {
    async fn plan(&self, ctx: PlanContext) -> Result<Plan>;
    async fn implement(&self, ctx: ImplContext) -> Result<ImplResult>;
    async fn review(&self, ctx: ReviewContext) -> Result<Vec<Finding>>;
    async fn analyze_finding(&self, f: &Finding, ctx: &ReviewContext)
        -> Result<RootCauseAnalysis>;     // returns: requires_fix + reason
    async fn apply_fix(&self, analysis: &RootCauseAnalysis, ctx: &ReviewContext)
        -> Result<FixOutcome>;
    async fn fix_failure(&self, ctx: FailureContext) -> Result<FixOutcome>;
        // used by lint/test loop and CI-fix loop
}

trait HumanChannel {
    async fn post_question(&self, q: Question) -> Result<MessageRef>;
    async fn poll_answers(&self) -> Result<Vec<Answer>>;
    async fn notify(&self, ctx: NotifyContext) -> Result<()>;
}

trait WorktreeBackend {
    fn create(&self, repo: &RepoRef, branch: &str) -> Result<PathBuf>;
    fn remove(&self, path: &Path, force: bool) -> Result<()>;
    fn list(&self) -> Result<Vec<WorktreeInfo>>;
}
```

v1 adapters:

| Trait | First-class | Future |
|---|---|---|
| `Engine` | `ClaudeCodeAdapter` (CLI subprocess) | `CodexAdapter`, `AnthropicApiAdapter`, `CompositeAdapter` |
| `HumanChannel` | `LinearCommentChannel` | `SlackChannel`, `CliPromptChannel`, `GitHubIssueChannel` |
| `WorktreeBackend` | `WorktrunkBackend` (`wt` CLI) | `ContainerBackend` |

## 4. Architecture

```
                                                ┌──────────────┐
                                                │  Linear API  │
                                                └──────┬───────┘
                                                       │ poll/webhook
                                                       ▼
┌──────────────────────────────────────────────────────────────────┐
│                       monorail core (Rust)                       │
│                                                                  │
│  ┌────────────┐     ┌──────────────┐     ┌──────────────────┐   │
│  │  Triager   │────▶│  Job Runner  │────▶│  Phase Pipeline  │   │
│  └────────────┘     └──────┬───────┘     └────────┬─────────┘   │
│                            │                      │             │
│                            ▼                      ▼             │
│                     ┌────────────┐         ┌────────────┐       │
│                     │   State    │         │  Engine    │──────┐│
│                     │  (SQLite)  │         │  Adapter   │      ││
│                     └────────────┘         └────────────┘      ││
│                            ▲                                   ││
│         ┌──────────────────┘                                   ││
│         │                                                      ││
│  ┌──────┴───────┐    ┌──────────────────┐                      ││
│  │ HumanChannel │    │ Worktree Backend │                      ││
│  │   Adapter    │    │  (worktrunk/wt)  │                      ││
│  └──────┬───────┘    └────────┬─────────┘                      ││
│         │                     │                                ││
└─────────┼─────────────────────┼────────────────────────────────┘│
          │                     │                                 │
          ▼                     ▼                                 ▼
   ┌────────────┐       ┌──────────────┐                ┌─────────────────┐
   │ Linear /   │       │ ghq + wt     │                │  claude/codex   │
   │ Slack /CLI │       │ filesystem   │                │  CLI subprocess │
   └────────────┘       └──────────────┘                └─────────────────┘
                                                                 │
   ┌────────────────────────────────────────────────────────┐    │
   │  TUI (ratatui)  ◀───── reads SQLite + tail logs ◀──────┘    │
   └────────────────────────────────────────────────────────┘    │
                                                                 │
                              git push / PR / CI               ◀─┘
                                       │
                                       ▼
                              ┌────────────────┐
                              │  GitHub API    │
                              └────────────────┘
```

### 4.1 Components

- **Triager** — listens to Linear (poll or webhook), classifies tickets by
  label, materializes a `Job`, persists it.
- **Job Runner** — schedules jobs respecting concurrency limits, dispatches to
  Phase Pipeline.
- **Phase Pipeline** — drives a job through phases (planning → impl →
  self-review → lint/test → PR → CI fix → merged). Per-repo phase advancement
  with dependency gating.
- **State** — SQLite database: jobs, repos, phases, questions, answers,
  attempts, escalations, audit trail.
- **TUI** — read-only-ish view over SQLite + log tailing; can write resume /
  abort / answer commands back via a small command channel.

### 4.2 Process model

monorail is a single binary with subcommands:

```
monorail daemon          # long-running: poll Linear, run jobs
monorail run <TICKET>    # one-shot: run a single ticket end-to-end
monorail tui             # connect to running daemon (or start ad-hoc)
monorail status          # text status of jobs
monorail answer <TICKET> # supply answer to pending question (CLI channel)
monorail resume <TICKET> # resume escalated job
monorail abort <TICKET>  # abort a job, optionally remove worktree
```

Both modes share the same pipeline implementation. The difference is whether
the **Triager** is active: `daemon` polls Linear and ingests new tickets;
`run <TICKET>` does not poll — it accepts the ticket key, materializes the
Job once, drives it to a terminal state, and exits.

### 4.3 External tool dependencies

monorail is deliberately **not** a reimplementation of git, GitHub, or
worktree management. It shells out to mature CLIs. All of the following
must be present and authenticated in any environment monorail runs in
(daemon container, local install):

| Tool | Used for | Required |
|---|---|---|
| `git` | git operations not covered by `wt` | yes |
| `gh` (GitHub CLI) | PR open, PR comment, CI status / logs, merge | yes |
| `ghq` | resolving `<org>/<repo>` → local path, lazy clone | yes |
| `wt` (worktrunk) | per-job worktree create/remove with conventions | yes |
| `claude` (Claude Code CLI) | first-class Engine adapter | yes (for `ClaudeCodeAdapter`) |

If any required tool is missing or not authenticated at startup, monorail
fails fast with an actionable message naming the missing tool. The
container image bakes them all in (§11.1).

## 5. Repository and Worktree Conventions

### 5.1 Multi-repo registry via ghq

monorail does not maintain its own repo registry. It assumes `ghq` is
installed and uses it to resolve `org/repo` to a local path:

```
ghq list -p <org>/<repo>
  → /home/user/ghq/github.com/<org>/<repo>
```

If a repo is not yet cloned, monorail runs `ghq get <org>/<repo>` lazily.

### 5.2 Worktree convention via worktrunk

monorail invokes `wt` (worktrunk) to create per-job worktrees. Per the user's
worktrunk config (`{{ repo_path }}/../{{ repo }}.{{ branch | sanitize }}`),
this produces:

```
~/ghq/github.com/<org>/<repo>             # base repo (default branch)
~/ghq/github.com/<org>/<repo>.<TICKET>    # monorail worktree (branch=TICKET)
```

Branch name is **always** the Linear ticket key (`ACM-123`).

### 5.3 Document reference resolution

In any documentation, references use `org/repo` form. When monorail (or any
agent reading docs) needs to resolve a reference within an active job's
context:

1. Try `<org>/<repo>.<TICKET>` (the in-progress worktree).
2. Fall back to `<org>/<repo>` (the base repo).

This lets a single doc reference point at the latest in-flight changes during
a job, and at mainline at all other times.

### 5.4 Linear ticket key universality

Linear enforces `<TEAM_PREFIX>-<NUMBER>` for all issues. monorail relies on
this. If a future system without ticket numbers is integrated, an adapter
must synthesize a stable identifier in the same form.

## 6. Triage and Linear Conventions

### 6.1 Labels (the only opt-in mechanism)

```
monorail:type/bug         Type A — autonomous, no human planning
monorail:type/feature     Type B — human planning required first
monorail:auto-merge       After CI green, merge automatically
```

A ticket with neither `type/bug` nor `type/feature` is **ignored**. Auto-merge
is a separate, additive label so it cannot be set by mistake by typing the
wrong type.

### 6.2 Linear ticket body — structured plan section

The ticket body MAY include a YAML block describing the multi-repo plan:

````markdown
## Monorail Plan

```yaml
repos:
  - org: acme
    name: proto-schema
    role: schema-source

  - org: acme
    name: core-api
    after: acme/proto-schema
    wait_for:
      type: ci-preview-branch
      ref_pattern: "preview/{ticket}"
    consumes:
      from: acme/proto-schema
      via: preview-branch

  - org: acme
    name: web-app
    after: acme/core-api
    anchors:
      - apps/api          # subproject inside the bigmono web-app
      - apps/web          # another subproject worked on in the same job
```
````

Rules:

- For Type A: the plan section is **required** if more than one repo is
  involved. For single-repo tickets, monorail infers the repo from Linear's
  GitHub integration metadata (the ticket's linked repo). If neither is
  present, the ticket is **rejected** at triage with a Linear comment asking
  the author to add either the GitHub link or a `## Monorail Plan` section.
- For Type B: the plan section is **optional**. If absent, monorail's
  planning phase elicits it from the human and writes it back to the ticket
  before leaving the planning phase.
- `wait_for.type` initial set: `merged`, `ci-success`, `ci-preview-branch`,
  `release-published`, `manual`.
- `anchors` is **optional**, worktree-relative. Used as starting points for
  `.monorail/` resolution (§13.1). When omitted, monorail derives anchors
  from the diff's common ancestor (post-impl) or defaults to the worktree
  root (pre-impl).

### 6.3 Phase ↔ Linear status mapping

| monorail phase | Linear status (default mapping) | Comment posted |
|---|---|---|
| picked-up | `In Progress` | "monorail picked up this ticket (job <id>)" |
| planning (Type B) | `In Progress` | planning Q&A as comments |
| implementing | `In Progress` | (silent) |
| self-reviewing | `In Progress` | (silent) |
| lint/test | `In Progress` | (silent) |
| pr-opened | `In Review` | PR URL(s) |
| ci-fixing | `In Review` | (silent) |
| escalated | (unchanged) | "monorail needs help: <reason>" + context |
| merged | `Done` | merge SHAs and PR URL(s) |

Status names are configurable per workspace because Linear allows custom
workflow states.

## 7. Pipeline Phases and Loop Control

### 7.1 Phase order

```
Pending
  │
  ├─ (Type A) ──┐
  │             ▼
  │         Implementing
  │             │
  ├─ (Type B) ──▶  Planning ──▶ Implementing
                                    │
                                    ▼
                               Self-Reviewing  (loop, max 5)
                                    │
                                    ▼
                               Lint/Testing    (loop, max 5)
                                    │
                                    ▼
                                 PR Opened
                                    │
                                    ▼
                               CI Fixing       (loop, max 3)
                                    │
                              ┌─────┴─────┐
                              ▼           ▼
                          Merged     Escalated
```

### 7.2 Self-review loop (max 5)

```
attempt = 0
loop {
    attempt += 1
    findings = engine.review(ctx)               // e.g. /pr-review-toolkit:review-pr
    actionable_fix_made = false
    for finding in findings {
        analysis = engine.analyze_finding(finding, ctx)
        if analysis.requires_fix {
            outcome = engine.apply_fix(analysis, ctx)
            if outcome.applied { actionable_fix_made = true }
        } else {
            record_dismissed(finding, analysis.reason)
        }
    }
    if !actionable_fix_made { break }
    if attempt >= 5 { escalate(SelfReviewMaxed); break }
}
```

Critical: the loop only re-reviews when a real fix was made. Dismissed
findings (with recorded justification) do not trigger re-review.

### 7.3 Lint/test loop (max 5)

Same root-cause-first pattern. The verify command comes from the per-repo
`monorail.toml` (`verify_cmd = "make verify"` style).

### 7.4 CI-fix loop (max 3)

After PR is opened:

1. monorail subscribes to GitHub Actions check_run / check_suite events
   (via webhook in daemon mode, polling in one-shot mode).
2. On failure: fetch logs, root-cause, fix in worktree, push.
3. On success: if `monorail:auto-merge` set and PR is approval-ready, merge.

### 7.5 Multi-repo DAG execution and per-repo isolation

Each `RepoTask` advances independently up to `PrOpened`, gated by `deps` and
`wait_for`. Cross-repo merge ordering is determined by deps unless
`auto-merge` is off (in which case humans drive merges).

**Hard contract — per-repo isolation**:

- One phase invocation = one `RepoTask` = one worktree = one engine
  subprocess. The engine's working directory is the assigned worktree.
- An engine invocation **MUST NOT edit files outside its assigned
  worktree**. Cross-repo coordinated edits are split into separate
  `RepoTask`s scheduled by the DAG.
- This is enforced two ways:
  1. The phase system prompt explicitly tells the engine its scope and
     forbids editing outside it.
  2. **Post-flight verification**: when the engine returns, monorail runs
     `git status` (or equivalent) over **all** worktrees in the Job. If any
     worktree other than the assigned one shows changes, the phase is
     rejected and escalated as `CrossRepoLeak`. The leaked changes are
     stashed for inspection but not applied.
- This isolation is what lets the prompt-resolution rules (§13.1) work:
  each invocation has exactly one repo's `.monorail/` chain, with no
  ambiguity.

### 7.6 Cross-repo context injection

Per-repo isolation does not mean the engine is blind to its sibling
`RepoTask`s. monorail injects, into every phase invocation, a read-only
context block describing the rest of the Job:

```
## Job context (read-only — DO NOT edit these repos)

ticket: ACM-123
your assignment: acme/core-api  (this worktree)

other repos in this job:
  - acme/proto-schema  phase=Merged       PR=#456 (merged at abc123)
  - acme/web-app       phase=Pending      depends on acme/core-api

shared facts:
  - acme/proto-schema preview branch ready: preview/ACM-123 (commit def456)
  - the planned API change adds field `priority` to Task message
```

Sources for this block:

- monorail's `Job` and `RepoTask` state (phases, PR URLs, etc.).
- Cross-repo `consumes` / `wait_for` resolutions (e.g., the resolved
  preview branch ref).
- Plan YAML excerpts from the Linear ticket (so all engines see the same
  agreed plan).

This is **context**, not prompt-overrides. It is regenerated per
invocation from current state; it is not user-editable per repo.

## 8. Escalation Model

Escalation is **not** a single behavior. It is phase-dependent.

| Phase escalated from | Default action | TUI detail view shows |
|---|---|---|
| Planning | Pause, no PR. Worktree intact. | Q&A history, candidate plans |
| Implementing | Pause, no PR. Worktree intact. | Diff so far, attempt log |
| Self-Reviewing | Pause, no PR. | Unresolved findings + root-cause notes |
| Lint/Testing | Pause, no PR. | Failing command + last attempts |
| CI-Fixing | **PR remains open** (already pushed). | CI log link, attempt diffs |

The default contract is: **pre-PR phases never push**, **CI-fix phase keeps
the PR**. This guarantees feature PRs are not polluted by half-finished
attempts.

Escalation always:
1. Sets job state to `Escalated` with a reason enum.
2. Posts a Linear comment summarizing the situation and what monorail tried.
3. Surfaces the job in the TUI "Needs help" section.
4. Leaves the worktree intact for human inspection.

Resume paths:
- TUI / CLI: `monorail resume <TICKET>` with optional guidance prompt.
- Linear: a comment matching `monorail: resume` with optional inline guidance
  triggers resumption (daemon mode).

## 9. Documentation Subsystem

### 9.1 Why separate

Documentation maintenance must not pollute feature PRs (they create merge
conflicts when many PRs are in flight). It runs in CI on its own cadence and
writes to its own store.

### 9.2 Document model

All AI-facing docs are markdown files with **YAML front matter**, validated
by JSON Schema. Example front matter:

```yaml
---
id: module.core-api
title: Core API
layer: module                     # entry | global | domain | module | ref
owners: ["@team-backend"]
source_paths:
  - "apps/core-api/**"
depends_on:
  - "modules.shared"
entry_points:
  - "apps/core-api/src/main.rs"
last_reviewed: "2026-04-01"
---
```

Two doc streams:

- **Flow docs (specs, plans)** in Linear / Confluence-style sources →
  monorail's doc subsystem extracts and converts to **stock docs** (ADRs,
  module docs, glossary entries) over time.
- **Index** of all stock docs + relevant code metadata → consumed by AI
  agents (including monorail itself) for retrieval.

### 9.3 Triggers (the chosen hybrid)

| Trigger | Workflow | Frequency |
|---|---|---|
| Incremental on merge to main | `docs-index-incremental.yml` | every merge |
| Full rebuild | `docs-index-full.yml` (cron) | nightly |
| ADR extraction | `docs-adr-extract.yml` (cron) | nightly, opens a separate PR |
| Manual rebuild | `workflow_dispatch` and Linear comment `monorail: rebuild-docs` | on demand |

Rationale:
- Probabilistic 1/n triggering was rejected: AI freshness must be predictable.
- Cron-only was rejected: up to a day of staleness lets agents reason on
  outdated indexes — a known cause of broken PRs.
- Per-merge full rebuild was rejected: too heavy, slows merges.
- The hybrid mirrors the standard search-engine pattern (incremental updates +
  periodic full rebuild for drift correction).

### 9.4 Storage

Index artifacts (jsonl, sqlite, embeddings) are **not committed to the
product repo**. They live in:
- A dedicated index repo (e.g., `acme/docs-index`), or
- An external object store / vector DB.

This is what lets the doc workflows run without creating PR conflicts.

### 9.5 Format choice

**YAML front matter + JSON Schema validation**. Considered alternatives:
- Standalone `metadata.yaml` per doc — pair management is fragile.
- CUE — strong typing but high learning cost and tool dependency.
- TOML front matter — awkward for arrays.

The schema is the authoritative format definition; AI agents are instructed
to never write a doc without front matter and to validate before commit.

## 10. Persistence and State

Single SQLite database, default location:
- daemon: `/var/lib/monorail/state.db` inside the container; mounted volume
  for persistence.
- local CLI: `~/.local/share/monorail/state.db` (XDG).

Tables (schema sketch, not final):

```
jobs(ticket PK, work_type, state, plan_yaml, auto_merge, created_at, updated_at, ...)
repo_tasks(id PK, ticket FK, org, repo, branch, worktree_path, phase,
           review_attempts, lint_test_attempts, ci_fix_attempts, pr_url, ...)
deps(repo_task_id FK, depends_on_repo_task_id FK, wait_for_json)
questions(id PK, ticket FK, repo_task_id NULL, channel, payload_json,
          posted_at, answered_at NULL, answer_json NULL)
events(id PK, ticket FK, kind, payload_json, ts)        -- audit trail
escalations(id PK, ticket FK, repo_task_id NULL, reason, snapshot_json, ts)
```

Why SQLite: single-binary deploy, no external service, sufficient
write throughput for a small fleet of jobs, easy backup (just copy the file).

## 11. Runtime and Deployment

### 11.1 Container image

```
FROM rust:slim AS build
... build monorail ...

FROM debian:slim
COPY --from=build /monorail /usr/local/bin/monorail
RUN install_dependencies: \
    git, gh, ghq, wt (worktrunk), claude (Claude Code CLI), python3 (for some skills)
ENTRYPOINT ["monorail"]
```

The image bundles `claude` (Claude Code CLI) and dependencies needed by the
Engine adapter. The same image runs as:
- `docker run ... monorail daemon` on GCP (Cloud Run jobs / Compute Engine).
- `docker run -it ... monorail run <TICKET>` locally.
- `monorail` natively installed (Homebrew tap) for users who prefer non-container local.

### 11.2 Secrets

Required:

| Secret | Scope | Provided via |
|---|---|---|
| `ANTHROPIC_API_KEY` | Engine adapter | env / GCP Secret Manager |
| `LINEAR_API_KEY` | Triager + HumanChannel | env / GCP Secret Manager |
| `GITHUB_TOKEN` | gh CLI (PR open, CI poll, merge) | env / `gh auth login` |
| `SLACK_BOT_TOKEN` | optional Slack channel | env |

Rule: secrets never written to the SQLite DB or to logs. Logs scrub on a
configured allowlist of fields.

### 11.3 Concurrency

Daemon mode: configurable max concurrent jobs (default 3). Each job may run
multiple repo tasks in parallel. Engine concurrency is bounded separately
(default 2 concurrent Claude Code subprocesses) to manage API spend and CPU.

## 12. TUI Design

Built with `ratatui` + `crossterm`. Read-mostly with a small command surface.

Top-level view (job list):

```
┌ monorail ─────────────────────────────────────────────────────────────┐
│ STATE       TICKET    REPOS                              PHASE        │
│ Active      ACM-456   acme/core-api                      impl         │
│ Active      ACM-789   acme/core-api                      ci-fix 2/3   │
│                       acme/web-app                       impl         │
│ ▶ Help      ACM-123   acme/core-api                      review 5/5   │
│                       acme/proto-schema                  pr-opened    │
│ Done        ACM-100   acme/web-app                       merged       │
└───────────────────────────────────────────────────────────────────────┘
[Enter] detail   [r] resume   [a] abort   [o] open worktree   [q] quit
```

Detail view (escalation example):

```
ACM-123 — Help needed
─────────────────────────────────────────────────
Repos:
  ▶ acme/core-api      review 5/5     3 unresolved findings
    acme/proto-schema  pr-opened      CI green

Findings (acme/core-api):
  [1] handler/foo.rs:42  N+1 query risk
       Root cause: intentional caching, monorail dismissed
  [2] models/bar.rs:88   missing nullable check
       Root cause: 2 fix attempts failed, schema ambiguity
  [3] auth/middleware.rs:14  unsafe header parse
       Root cause: requires user judgment

Actions:
  [g] guidance prompt + retry
  [w] open worktree in $EDITOR
  [p] push current state as draft PR
  [m] mark finding #N as wontfix
```

Multi-repo display: each repo gets its own row in detail. Per-repo phase
counters are visible. Dependency arrows are drawn in the detail view when
a repo is `waiting`.

## 13. Configuration Files

### 13.1 Per-repo configuration — convention-based, no `monorail.toml`

monorail does **not** require a per-repo config file. Encoding `verify` /
`build` / `test` / `lint` commands as a single block per repo does not fit
reality: many of monorail's target repositories are themselves monorepos
with many sub-projects, each with its own toolchain. A flat per-repo command
table would be wrong for half the cases.

Instead, the Engine adapter discovers what to run by reading what the repo
already documents for AI agents. In priority order:

1. **AI-facing entry docs**: `CLAUDE.md`, `AGENTS.md` (root), and any
   `docs/ai/*` index conventions (e.g., the layered structure from the
   aidocs design principles: entry → repo-map → service-map).
2. **Architecture / overview docs**: `docs/OVERVIEW.md`,
   `docs/ARCHITECTURE.md`, `docs/architecture/*`.
3. **Build signal files**: `package.json`, `Makefile`, `Cargo.toml`,
   `pnpm-workspace.yaml`, `pyproject.toml`, `go.mod`, `Justfile`, etc. —
   used to confirm the stack and likely commands.
4. **Convention overrides** (optional, all under `.monorail/` if present):
   - `.monorail/prompts/<phase>.md` — extra context for a phase
     (`plan`, `implement`, `review`, `lint-test`, `ci-fix`)
   - `.monorail/hooks/<event>.sh` — pre/post side-effect scripts
     (e.g., `pre-review.sh`, `post-implement.sh`). Run with cwd set to the
     **anchor** dir, with env vars `MONORAIL_TICKET`, `MONORAIL_PHASE`,
     `MONORAIL_REPO`, `MONORAIL_WORKTREE`, `MONORAIL_ANCHOR`,
     `MONORAIL_ATTEMPT`. Non-zero exit aborts the phase.
   - `.monorail/skills/<name>.md` — repo-local skill files surfaced to
     the engine

#### 13.1.1 Layered resolution algorithm

`.monorail/` directories may exist at three layers within a single
`RepoTask`'s scope:

| Layer | Path | Role |
|---|---|---|
| User | `~/.monorail/` | personal defaults across all repos |
| Repo | `<worktree-root>/.monorail/` | repo-wide defaults |
| Anchor | `<anchor-dir>/.monorail/` | subproject-specific (only inside this RepoTask's worktree) |

`<anchor-dir>` is taken from `RepoTask.anchors` (declared in Plan YAML or
derived: see §6.2 / §3.1). Multiple anchors → multiple anchor layers, each
resolved independently for the files within its subtree.

Resolution differs per asset type:

| Asset | Strategy | Order |
|---|---|---|
| `prompts/<phase>.md` | **layered concat** | user → repo → anchor (broad → narrow). Layers that exist are concatenated with separators; missing layers are skipped. |
| `hooks/<event>.sh` | **layered execute** | user → repo → anchor (broad → narrow). Each existing layer runs in order; any non-zero exit aborts. |
| `skills/<name>.md` | **first-match-wins** | anchor → repo → user (narrow → broad). The closest occurrence of a given filename is used; identical filenames at outer layers are ignored. |

For per-file decisions during a phase (e.g., when reviewing many files in
a multi-anchor RepoTask), each file is associated with the **closest
enclosing anchor** under the worktree; files outside any declared anchor
are processed under the repo+user layers only.

Other repos' `.monorail/` directories are never consulted for this
RepoTask. Per the §7.5 isolation contract, each engine invocation sees
exactly one repo's `.monorail/` chain.

Engine choice and concurrency are **operator concerns**, not repo concerns,
so they belong in the global config (§13.2), not here. There is no
`[engine] preferred` knob per repo.

### 13.2 Global `~/.config/monorail/config.toml`

```toml
[linear]
workspace = "acme"
status_map = { picked_up = "In Progress", in_review = "In Review", done = "Done" }

[engine.default]
adapter = "claude-code"

[channel.default]
adapter = "linear-comment"

[concurrency]
max_jobs = 3
max_engine_subprocesses = 2

[paths]
state_db = "~/.local/share/monorail/state.db"
log_dir = "~/.local/state/monorail/logs"
```

The global config is the only TOML monorail reads. Ticket-specific plans
live in the Linear ticket body (§6.2). Per-repo overrides live in
`.monorail/prompts/*.md` files inside the repo (§13.1).

## 14. Testing Strategy

- **Unit**: pure logic — plan parser, state machine transitions, dependency
  DAG resolver, label parser.
- **Integration**: with `MockEngine`, `MockHumanChannel`, in-memory SQLite,
  drive a full pipeline through phases including escalation and resume.
- **Adapter contract tests**: each adapter implementation must pass the same
  contract test suite (so Codex / API adapters can be added later with
  confidence).
- **End-to-end smoke** (manual or staged): tag a Linear ticket in a sandbox
  workspace, run `monorail run` against a sandbox repo, verify PR open and
  CI loop.

Coverage target: 80% for core (per project rules), best-effort for adapters
that wrap external CLIs (subprocess interaction).

## 15. Open Questions and Future Work

1. **Concurrency budgeting per Engine adapter**: Anthropic API rate limits
   vs. Claude Code CLI subprocess limits — needs measurement before tuning.
2. **Cross-repo PR coordination**: when N PRs must merge atomically (none
   independently mergeable), v1 escalates to human. A future "merge train"
   feature could automate this.
3. **Cost observability**: per-job token / dollar accounting. Probably an
   `events` table extension consuming Anthropic API usage headers.
4. **TUI-over-network**: when the daemon runs on GCP, the TUI must reach
   it. Options: ssh + run TUI inside the container vs. expose a small
   gRPC/HTTP service the TUI connects to. Probably ssh first.
5. **Slack channel adapter polish**: thread routing, file uploads for diffs.
6. **Spec-to-ADR extractor**: heuristics for what flow content becomes which
   stock doc type. Likely an LLM-driven pass with human approval gate.
7. **Folder rename**: the working directory is currently `arail`; should be
   renamed to `monorail` with the package and binary names aligned.
8. **Claude Code CLI authentication inside containers**: `claude` typically
   uses an interactive session-based auth. For headless container runs we
   need either an API-key mode for `claude` or to switch the daemon's
   default Engine adapter to `AnthropicApiAdapter` while keeping
   `ClaudeCodeAdapter` for local-CLI use. To resolve before v1 daemon ship.
9. **Daemon vs one-shot semantic boundary**: `monorail run <TICKET>` does NOT
   poll Linear; it accepts the ticket as input, runs to terminal state, then
   exits. The daemon polls Linear and runs many jobs. Both share the same
   pipeline implementation; the difference is whether the Triager is active.
   This is reflected in §4.2 but worth re-validating during implementation.

## 16. Out of Scope (v1)

- Non-Linear ticket sources (Jira, GitHub Issues, Notion).
- Engines other than Claude Code (Codex / API / Composite are deferred).
- HumanChannels other than Linear comments.
- Browser UI.
- Cross-repo atomic merge trains.
- Auto-rollback on production incident detection.

---

## Appendix A — Phase pseudocode

```rust
async fn run_repo_task(rt: &mut RepoTask, job: &Job) -> Result<()> {
    wait_for_deps(rt, job).await?;

    if matches!(job.work_type, WorkType::Feature) && rt.phase == Phase::Pending {
        rt.phase = Phase::Planning;
        plan_with_human(rt, job).await?;
    }

    rt.phase = Phase::Implementing;
    engine.implement(impl_ctx(rt)).await?;

    rt.phase = Phase::SelfReviewing;
    self_review_loop(rt).await?;        // max 5

    rt.phase = Phase::LintTesting;
    lint_test_loop(rt).await?;          // max 5

    rt.phase = Phase::PrOpened;
    open_pr(rt, job).await?;

    rt.phase = Phase::CiFixing;
    ci_fix_loop(rt).await?;             // max 3

    if job.auto_merge && all_repos_ready(job) {
        merge_repo(rt).await?;
        rt.phase = Phase::Merged;
    }
    Ok(())
}
```

## Appendix B — Why not codd?

codd-dev is a coherence-driven methodology: it generates and propagates
design docs, with the unit of work being a doc dependency graph. monorail's
unit of work is a Linear ticket; its job is to produce a green PR. The two
systems could compose in the future (codd as a doc-subsystem strategy under
monorail's docs CI), but they solve different problems and should not be
conflated.
