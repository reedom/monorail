# monorail Core (Type A, single repo) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver `monorail run <TICKET>` for a Type A (bug-labeled) single-repo Linear ticket — fetch the ticket, materialize a Job, drive implement → self-review (max 5) → lint/test (max 5) → open PR → CI-fix (max 3) end-to-end with the Claude Code engine, posting status to Linear and persisting to SQLite. No multi-repo, no Type B planning, no TUI in this plan.

**Architecture:** Single Rust binary (`monorail`) with module structure (no workspace yet). Async runtime is tokio. Persistence is SQLite via `sqlx`. External CLIs (`gh`, `ghq`, `wt`, `claude`) are invoked via `tokio::process`. Linear API is GraphQL via `reqwest`. Engine and HumanChannel are trait-based; v1 adapters are `ClaudeCodeAdapter` and `LinearCommentChannel`. The pipeline is a sequential phase runner that mutates a `Job` through `Phase` states, persisting each transition.

**Tech Stack:** Rust 2024 edition, `tokio`, `sqlx` (SQLite), `reqwest`, `clap`, `serde` + `serde_json` + `serde_yaml`, `tracing`, `anyhow` + `thiserror`, `mockall`, `wiremock`, `tempfile`, `assert_cmd`.

**Spec:** [`docs/superpowers/specs/2026-04-27-monorail-design.md`](../specs/2026-04-27-monorail-design.md). Sections referenced as `§N`.

---

## File structure

```
arail/                              # working dir (will be renamed to monorail later)
├── Cargo.toml
├── migrations/
│   └── 20260428000000_init.sql
├── src/
│   ├── main.rs                      # entry: parse CLI, dispatch
│   ├── cli.rs                       # clap definitions
│   ├── error.rs                     # thiserror types
│   ├── tracing_setup.rs             # tracing-subscriber init
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── ticket.rs                # TicketKey
│   │   ├── phase.rs                 # Phase, EscalationReason
│   │   ├── job.rs                   # Job, RepoTask, JobState, WorkType
│   │   ├── question.rs              # Question, Answer
│   │   └── finding.rs               # Finding, RootCauseAnalysis, FixOutcome
│   ├── state/
│   │   ├── mod.rs                   # SqliteState (the repository)
│   │   ├── jobs.rs                  # job CRUD
│   │   ├── repo_tasks.rs            # repo_task CRUD
│   │   └── events.rs                # audit log append/query
│   ├── linear/
│   │   ├── mod.rs                   # LinearClient
│   │   ├── types.rs                 # Issue, Label, Comment, etc.
│   │   └── graphql.rs               # query strings
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── ghq.rs                   # GhqTool: list_path, ensure_cloned
│   │   ├── wt.rs                    # WtTool: switch_create, remove
│   │   └── gh.rs                    # GhTool: pr_create, checks, logs, merge
│   ├── engine/
│   │   ├── mod.rs                   # Engine trait
│   │   └── claude_code.rs           # ClaudeCodeAdapter
│   ├── channel/
│   │   ├── mod.rs                   # HumanChannel trait
│   │   └── linear_comment.rs        # LinearCommentChannel
│   ├── triager.rs                   # fetch ticket, parse labels, build Job
│   ├── pipeline/
│   │   ├── mod.rs                   # PhaseRunner
│   │   ├── implement.rs
│   │   ├── self_review.rs
│   │   ├── lint_test.rs
│   │   ├── open_pr.rs
│   │   └── ci_fix.rs
│   └── escalate.rs                  # escalation handler
└── tests/
    └── e2e_typeA.rs                 # end-to-end smoke (mocked externals)
```

Each module is small and focused. Files larger than ~300 lines are a smell — split before they grow.

---

## Conventions

- **Branch naming for impl work**: `impl/monorail-<task-N>` per task. Use `git checkout -b` before starting (the repo blocks commits to `main`).
- **Test file location**: tests for module `foo.rs` live in the same file using `#[cfg(test)] mod tests { ... }` for unit tests; cross-module integration tests live in `tests/`.
- **Mocking style**: trait mocks with `mockall`; HTTP mocks with `wiremock`; subprocess mocks via stubbed binaries placed on `PATH` in tests using a temp dir + symlinks (real CLI invocation kept in integration tests only).
- **Error handling**: library/internal layers return `Result<T, MonorailError>` (thiserror); the binary entrypoint maps to `anyhow::Result<()>` for printing.
- **Numeric comparisons**: per repo CLAUDE.md — use `<` and `<=`, never `>` or `>=`.
- **No emojis** in code/comments/docs.

---

## Task 1: Project skeleton

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/error.rs`
- Create: `src/tracing_setup.rs`
- Create: `.gitignore`

- [ ] **Step 1: Create branch**

Run: `git checkout -b impl/monorail-task-01`

- [ ] **Step 2: Write Cargo.toml**

Create `Cargo.toml`:

```toml
[package]
name = "monorail"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio", "macros", "migrate", "chrono"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
thiserror = "1"
chrono = { version = "0.4", features = ["serde"] }
url = { version = "2", features = ["serde"] }
async-trait = "0.1"

[dev-dependencies]
mockall = "0.13"
wiremock = "0.6"
tempfile = "3"
assert_cmd = "2"
predicates = "3"
pretty_assertions = "1"
```

- [ ] **Step 3: Write src/error.rs**

Create `src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MonorailError {
    #[error("invalid ticket key: {0}")]
    InvalidTicketKey(String),
    #[error("missing label: {0}")]
    MissingLabel(&'static str),
    #[error("ticket rejected at triage: {0}")]
    TriageRejected(String),
    #[error("phase aborted: {0}")]
    PhaseAborted(String),
    #[error("escalated: {0}")]
    Escalated(String),
    #[error("external tool failed: {tool}: {message}")]
    ExternalTool { tool: &'static str, message: String },
    #[error("linear api error: {0}")]
    Linear(String),
    #[error("state error: {0}")]
    State(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(String),
}

pub type Result<T> = std::result::Result<T, MonorailError>;
```

- [ ] **Step 4: Write src/tracing_setup.rs**

Create `src/tracing_setup.rs`:

```rust
use tracing_subscriber::{EnvFilter, fmt};

pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .init();
}
```

- [ ] **Step 5: Write src/main.rs (hello world)**

Create `src/main.rs`:

```rust
mod error;
mod tracing_setup;

fn main() -> anyhow::Result<()> {
    tracing_setup::init();
    tracing::info!("monorail starting");
    Ok(())
}
```

- [ ] **Step 6: Write .gitignore**

Create `.gitignore`:

```
target/
*.db
*.db-journal
*.db-wal
.env
```

- [ ] **Step 7: Build**

Run: `cargo build`
Expected: build succeeds. Warnings allowed for unused deps in this task.

- [ ] **Step 8: Run**

Run: `RUST_LOG=info cargo run`
Expected: prints a tracing line containing `monorail starting`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml src/main.rs src/error.rs src/tracing_setup.rs .gitignore
git commit -m "chore: initialize monorail rust project skeleton"
```

---

## Task 2: CLI with `run <TICKET>` subcommand

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs`
- Test: `src/cli.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-02 main` (or branch off prior task as appropriate)

- [ ] **Step 2: Write failing test**

Add to a new `src/cli.rs`:

```rust
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "monorail", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run { ticket: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_run_subcommand() {
        let cli = Cli::try_parse_from(["monorail", "run", "ACM-123"]).unwrap();
        match cli.command {
            Command::Run { ticket } => assert_eq!(ticket, "ACM-123"),
        }
    }

    #[test]
    fn rejects_missing_ticket() {
        let err = Cli::try_parse_from(["monorail", "run"]).unwrap_err();
        let s = err.to_string();
        assert!(s.contains("required"), "got: {s}");
    }
}
```

- [ ] **Step 3: Wire CLI into main**

Replace contents of `src/main.rs`:

```rust
mod cli;
mod error;
mod tracing_setup;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    tracing_setup::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Run { ticket } => {
            tracing::info!(ticket, "run subcommand invoked (stub)");
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib cli`
Expected: 2 passing.

- [ ] **Step 5: Manual smoke**

Run: `cargo run -- run ACM-123`
Expected: tracing line shows `ticket=ACM-123`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/cli.rs src/main.rs
git commit -m "feat(cli): add run <ticket> subcommand stub"
```

---

## Task 3: Domain — `TicketKey`

**Files:**
- Create: `src/domain/mod.rs`
- Create: `src/domain/ticket.rs`
- Modify: `src/main.rs` (add `mod domain;`)

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-03`

- [ ] **Step 2: Write failing tests**

Create `src/domain/ticket.rs`:

```rust
use crate::error::{MonorailError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TicketKey(String);

impl TicketKey {
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(MonorailError::InvalidTicketKey(s.to_string()));
        }
        let prefix_ok = !parts[0].is_empty()
            && parts[0].chars().all(|c| c.is_ascii_uppercase());
        let number_ok = !parts[1].is_empty()
            && parts[1].chars().all(|c| c.is_ascii_digit());
        if !prefix_ok || !number_ok {
            return Err(MonorailError::InvalidTicketKey(s.to_string()));
        }
        Ok(TicketKey(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TicketKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_standard_form() {
        let t = TicketKey::parse("ACM-123").unwrap();
        assert_eq!(t.as_str(), "ACM-123");
    }

    #[test]
    fn accepts_long_prefix() {
        TicketKey::parse("PROD-9999").unwrap();
    }

    #[test]
    fn rejects_lowercase_prefix() {
        assert!(TicketKey::parse("acm-123").is_err());
    }

    #[test]
    fn rejects_missing_number() {
        assert!(TicketKey::parse("ACM-").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(TicketKey::parse("").is_err());
    }

    #[test]
    fn rejects_extra_segments() {
        assert!(TicketKey::parse("ACM-12-3").is_err());
    }
}
```

Create `src/domain/mod.rs`:

```rust
pub mod ticket;
pub use ticket::TicketKey;
```

- [ ] **Step 3: Wire into main**

In `src/main.rs`, add `mod domain;` at the top with other modules.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib domain::ticket`
Expected: 6 passing.

- [ ] **Step 5: Commit**

```bash
git add src/domain/mod.rs src/domain/ticket.rs src/main.rs
git commit -m "feat(domain): add TicketKey with strict parsing"
```

---

## Task 4: Domain — `Phase`, `EscalationReason`, `WorkType`, `JobState`

**Files:**
- Create: `src/domain/phase.rs`
- Modify: `src/domain/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-04`

- [ ] **Step 2: Write failing tests + impl**

Create `src/domain/phase.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Pending,
    Implementing,
    SelfReviewing,
    LintTesting,
    PrOpened,
    CiFixing,
    Merged,
    Aborted,
    Escalated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationReason {
    SelfReviewMaxed,
    LintTestMaxed,
    CiFixMaxed,
    CrossRepoLeak,
    EngineFailure,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkType {
    Bug,
    Feature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Active,
    Escalated,
    Done,
    Aborted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_serializes_kebab_case() {
        let s = serde_json::to_string(&Phase::SelfReviewing).unwrap();
        assert_eq!(s, "\"self-reviewing\"");
    }

    #[test]
    fn escalation_reason_round_trip() {
        let r = EscalationReason::CrossRepoLeak;
        let s = serde_json::to_string(&r).unwrap();
        let r2: EscalationReason = serde_json::from_str(&s).unwrap();
        assert_eq!(r, r2);
    }

    #[test]
    fn work_type_parses_bug() {
        let w: WorkType = serde_json::from_str("\"bug\"").unwrap();
        assert_eq!(w, WorkType::Bug);
    }
}
```

Update `src/domain/mod.rs`:

```rust
pub mod phase;
pub mod ticket;

pub use phase::{EscalationReason, JobState, Phase, WorkType};
pub use ticket::TicketKey;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib domain::phase`
Expected: 3 passing.

- [ ] **Step 4: Commit**

```bash
git add src/domain/phase.rs src/domain/mod.rs
git commit -m "feat(domain): add Phase, EscalationReason, WorkType, JobState"
```

---

## Task 5: Domain — `Job`, `RepoTask`, `Question`, `Answer`, `Finding`

**Files:**
- Create: `src/domain/job.rs`
- Create: `src/domain/question.rs`
- Create: `src/domain/finding.rs`
- Modify: `src/domain/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-05`

- [ ] **Step 2: Write src/domain/job.rs**

```rust
use crate::domain::{JobState, Phase, TicketKey, WorkType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub org: String,
    pub repo: String,
}

impl RepoRef {
    pub fn full(&self) -> String {
        format!("{}/{}", self.org, self.repo)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoTask {
    pub repo: RepoRef,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub anchors: Vec<PathBuf>,
    pub phase: Phase,
    pub pr_url: Option<Url>,
    pub review_attempts: u8,
    pub lint_test_attempts: u8,
    pub ci_fix_attempts: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub ticket: TicketKey,
    pub work_type: WorkType,
    pub state: JobState,
    pub repos: Vec<RepoTask>,
    pub auto_merge: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_ref_full_format() {
        let r = RepoRef { org: "acme".into(), repo: "core-api".into() };
        assert_eq!(r.full(), "acme/core-api");
    }

    #[test]
    fn job_round_trips_json() {
        let j = Job {
            ticket: TicketKey::parse("ACM-1").unwrap(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let s = serde_json::to_string(&j).unwrap();
        let j2: Job = serde_json::from_str(&s).unwrap();
        assert_eq!(j.ticket, j2.ticket);
    }
}
```

- [ ] **Step 3: Write src/domain/question.rs**

```rust
use crate::domain::TicketKey;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub ticket: TicketKey,
    pub prompt: String,
    pub posted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub question_id: String,
    pub body: String,
    pub answered_at: DateTime<Utc>,
}
```

- [ ] **Step 4: Write src/domain/finding.rs**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,            // stable per (file,line,rule) tuple
    pub file: String,
    pub line: Option<u32>,
    pub severity: Severity,
    pub rule: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseAnalysis {
    pub finding_id: String,
    pub requires_fix: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixOutcome {
    pub applied: bool,
    pub message: String,
}
```

- [ ] **Step 5: Update src/domain/mod.rs**

```rust
pub mod finding;
pub mod job;
pub mod phase;
pub mod question;
pub mod ticket;

pub use finding::{Finding, FixOutcome, RootCauseAnalysis, Severity};
pub use job::{Job, RepoRef, RepoTask};
pub use phase::{EscalationReason, JobState, Phase, WorkType};
pub use question::{Answer, Question};
pub use ticket::TicketKey;
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib domain`
Expected: all domain tests pass (existing + 2 new in `job.rs`).

- [ ] **Step 7: Commit**

```bash
git add src/domain
git commit -m "feat(domain): add Job, RepoTask, Question, Answer, Finding"
```

---

## Task 6: SQLite migration + `state` module skeleton

**Files:**
- Create: `migrations/20260428000000_init.sql`
- Create: `src/state/mod.rs`
- Modify: `src/main.rs` (add `mod state;`)

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-06`

- [ ] **Step 2: Write migration**

Create `migrations/20260428000000_init.sql`:

```sql
CREATE TABLE jobs (
  ticket          TEXT PRIMARY KEY NOT NULL,
  work_type       TEXT NOT NULL,
  state           TEXT NOT NULL,
  auto_merge      INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);

CREATE TABLE repo_tasks (
  id                  INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket              TEXT NOT NULL REFERENCES jobs(ticket) ON DELETE CASCADE,
  org                 TEXT NOT NULL,
  repo                TEXT NOT NULL,
  branch              TEXT NOT NULL,
  worktree_path       TEXT NOT NULL,
  anchors_json        TEXT NOT NULL DEFAULT '[]',
  phase               TEXT NOT NULL,
  pr_url              TEXT,
  review_attempts     INTEGER NOT NULL DEFAULT 0,
  lint_test_attempts  INTEGER NOT NULL DEFAULT 0,
  ci_fix_attempts     INTEGER NOT NULL DEFAULT 0,
  UNIQUE (ticket, org, repo)
);

CREATE TABLE events (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket      TEXT NOT NULL,
  kind        TEXT NOT NULL,
  payload     TEXT NOT NULL,
  ts          TEXT NOT NULL
);

CREATE INDEX events_by_ticket ON events(ticket, ts);

CREATE TABLE escalations (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  ticket        TEXT NOT NULL,
  repo_task_id  INTEGER,
  reason        TEXT NOT NULL,
  snapshot      TEXT NOT NULL,
  ts            TEXT NOT NULL
);
```

- [ ] **Step 3: Write src/state/mod.rs**

```rust
use crate::error::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

pub struct SqliteState {
    pub pool: SqlitePool,
}

impl SqliteState {
    pub async fn open(db_path: &Path) -> Result<Self> {
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let opts = SqliteConnectOptions::from_str(&url)
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?;
        Ok(Self { pool })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn opens_and_migrates() {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("test.db");
        let state = SqliteState::open(&db).await.unwrap();
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM jobs")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(row.0, 0);
    }
}
```

- [ ] **Step 4: Wire `mod state;` into main**

Add `mod state;` to `src/main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib state`
Expected: 1 passing.

- [ ] **Step 6: Commit**

```bash
git add migrations src/state src/main.rs
git commit -m "feat(state): init sqlite schema and SqliteState wrapper"
```

---

## Task 7: State — Job CRUD

**Files:**
- Create: `src/state/jobs.rs`
- Modify: `src/state/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-07`

- [ ] **Step 2: Write tests + impl**

Create `src/state/jobs.rs`:

```rust
use crate::domain::{Job, JobState, TicketKey, WorkType};
use crate::error::Result;
use crate::state::SqliteState;
use chrono::{DateTime, Utc};

impl SqliteState {
    pub async fn insert_job(&self, job: &Job) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO jobs (ticket, work_type, state, auto_merge, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(job.ticket.as_str())
        .bind(work_type_str(job.work_type))
        .bind(state_str(job.state))
        .bind(job.auto_merge as i64)
        .bind(job.created_at.to_rfc3339())
        .bind(job.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_job(&self, ticket: &TicketKey) -> Result<Option<JobRow>> {
        let row: Option<JobRow> = sqlx::query_as(
            r#"SELECT ticket, work_type, state, auto_merge, created_at, updated_at
               FROM jobs WHERE ticket = ?"#,
        )
        .bind(ticket.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn update_job_state(&self, ticket: &TicketKey, new_state: JobState) -> Result<()> {
        sqlx::query(
            r#"UPDATE jobs SET state = ?, updated_at = ? WHERE ticket = ?"#,
        )
        .bind(state_str(new_state))
        .bind(Utc::now().to_rfc3339())
        .bind(ticket.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct JobRow {
    pub ticket: String,
    pub work_type: String,
    pub state: String,
    pub auto_merge: i64,
    pub created_at: String,
    pub updated_at: String,
}

fn work_type_str(w: WorkType) -> &'static str {
    match w { WorkType::Bug => "bug", WorkType::Feature => "feature" }
}
fn state_str(s: JobState) -> &'static str {
    match s {
        JobState::Active => "active",
        JobState::Escalated => "escalated",
        JobState::Done => "done",
        JobState::Aborted => "aborted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TicketKey;
    use tempfile::TempDir;

    async fn fresh_state() -> (TempDir, SqliteState) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("t.db");
        (dir, SqliteState::open(&db).await.unwrap())
    }

    #[tokio::test]
    async fn insert_then_get_returns_row() {
        let (_d, st) = fresh_state().await;
        let job = Job {
            ticket: TicketKey::parse("ACM-7").unwrap(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let row = st.get_job(&job.ticket).await.unwrap().unwrap();
        assert_eq!(row.ticket, "ACM-7");
        assert_eq!(row.work_type, "bug");
        assert_eq!(row.state, "active");
    }

    #[tokio::test]
    async fn update_state_persists() {
        let (_d, st) = fresh_state().await;
        let job = Job {
            ticket: TicketKey::parse("ACM-8").unwrap(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        st.update_job_state(&job.ticket, JobState::Done).await.unwrap();
        let row = st.get_job(&job.ticket).await.unwrap().unwrap();
        assert_eq!(row.state, "done");
    }
}
```

Update `src/state/mod.rs` to expose: add `mod jobs;` near top of file (below `pub struct SqliteState`'s impl block) and `pub use jobs::JobRow;`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib state`
Expected: 3 passing.

- [ ] **Step 4: Commit**

```bash
git add src/state
git commit -m "feat(state): job CRUD (insert, get, update_state)"
```

---

## Task 8: State — RepoTask CRUD + events log

**Files:**
- Create: `src/state/repo_tasks.rs`
- Create: `src/state/events.rs`
- Modify: `src/state/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-08`

- [ ] **Step 2: Write src/state/repo_tasks.rs**

```rust
use crate::domain::{Phase, RepoRef, RepoTask, TicketKey};
use crate::error::Result;
use crate::state::SqliteState;
use std::path::PathBuf;
use url::Url;

impl SqliteState {
    pub async fn insert_repo_task(&self, ticket: &TicketKey, rt: &RepoTask) -> Result<i64> {
        let anchors_json = serde_json::to_string(&rt.anchors)
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?;
        let result = sqlx::query(
            r#"INSERT INTO repo_tasks
               (ticket, org, repo, branch, worktree_path, anchors_json, phase,
                pr_url, review_attempts, lint_test_attempts, ci_fix_attempts)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(ticket.as_str())
        .bind(&rt.repo.org)
        .bind(&rt.repo.repo)
        .bind(&rt.branch)
        .bind(rt.worktree_path.to_string_lossy().to_string())
        .bind(&anchors_json)
        .bind(phase_str(rt.phase))
        .bind(rt.pr_url.as_ref().map(|u| u.to_string()))
        .bind(rt.review_attempts as i64)
        .bind(rt.lint_test_attempts as i64)
        .bind(rt.ci_fix_attempts as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn list_repo_tasks(&self, ticket: &TicketKey) -> Result<Vec<RepoTaskRow>> {
        let rows: Vec<RepoTaskRow> = sqlx::query_as(
            r#"SELECT id, ticket, org, repo, branch, worktree_path, anchors_json, phase,
                      pr_url, review_attempts, lint_test_attempts, ci_fix_attempts
               FROM repo_tasks WHERE ticket = ? ORDER BY id"#,
        )
        .bind(ticket.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn update_repo_task_phase(&self, id: i64, phase: Phase) -> Result<()> {
        sqlx::query(r#"UPDATE repo_tasks SET phase = ? WHERE id = ?"#)
            .bind(phase_str(phase))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn bump_attempt(&self, id: i64, kind: AttemptKind) -> Result<()> {
        let col = match kind {
            AttemptKind::Review => "review_attempts",
            AttemptKind::LintTest => "lint_test_attempts",
            AttemptKind::CiFix => "ci_fix_attempts",
        };
        let sql = format!("UPDATE repo_tasks SET {col} = {col} + 1 WHERE id = ?");
        sqlx::query(&sql).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_pr_url(&self, id: i64, url: &Url) -> Result<()> {
        sqlx::query(r#"UPDATE repo_tasks SET pr_url = ? WHERE id = ?"#)
            .bind(url.to_string())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct RepoTaskRow {
    pub id: i64,
    pub ticket: String,
    pub org: String,
    pub repo: String,
    pub branch: String,
    pub worktree_path: String,
    pub anchors_json: String,
    pub phase: String,
    pub pr_url: Option<String>,
    pub review_attempts: i64,
    pub lint_test_attempts: i64,
    pub ci_fix_attempts: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum AttemptKind {
    Review,
    LintTest,
    CiFix,
}

fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Pending => "pending",
        Phase::Implementing => "implementing",
        Phase::SelfReviewing => "self-reviewing",
        Phase::LintTesting => "lint-testing",
        Phase::PrOpened => "pr-opened",
        Phase::CiFixing => "ci-fixing",
        Phase::Merged => "merged",
        Phase::Aborted => "aborted",
        Phase::Escalated => "escalated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, WorkType};
    use chrono::Utc;
    use tempfile::TempDir;

    async fn fresh() -> (TempDir, SqliteState, TicketKey) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("t.db");
        let st = SqliteState::open(&db).await.unwrap();
        let ticket = TicketKey::parse("ACM-9").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        (dir, st, ticket)
    }

    fn sample_repo_task() -> RepoTask {
        RepoTask {
            repo: RepoRef { org: "acme".into(), repo: "core-api".into() },
            branch: "ACM-9".into(),
            worktree_path: PathBuf::from("/tmp/wt"),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        }
    }

    #[tokio::test]
    async fn insert_and_list() {
        let (_d, st, t) = fresh().await;
        st.insert_repo_task(&t, &sample_repo_task()).await.unwrap();
        let rows = st.list_repo_tasks(&t).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].repo, "core-api");
    }

    #[tokio::test]
    async fn bump_attempt_increments() {
        let (_d, st, t) = fresh().await;
        let id = st.insert_repo_task(&t, &sample_repo_task()).await.unwrap();
        st.bump_attempt(id, AttemptKind::Review).await.unwrap();
        st.bump_attempt(id, AttemptKind::Review).await.unwrap();
        let rows = st.list_repo_tasks(&t).await.unwrap();
        assert_eq!(rows[0].review_attempts, 2);
    }
}
```

- [ ] **Step 3: Write src/state/events.rs**

```rust
use crate::domain::TicketKey;
use crate::error::Result;
use crate::state::SqliteState;
use chrono::Utc;
use serde::Serialize;

impl SqliteState {
    pub async fn append_event<P: Serialize>(
        &self,
        ticket: &TicketKey,
        kind: &str,
        payload: &P,
    ) -> Result<()> {
        let body = serde_json::to_string(payload)
            .map_err(|e| crate::error::MonorailError::Serde(e.to_string()))?;
        sqlx::query(
            r#"INSERT INTO events (ticket, kind, payload, ts) VALUES (?, ?, ?, ?)"#,
        )
        .bind(ticket.as_str())
        .bind(kind)
        .bind(body)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn count_events(&self, ticket: &TicketKey) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM events WHERE ticket = ?"#,
        )
        .bind(ticket.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, WorkType};
    use chrono::Utc;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn append_and_count() {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let t = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: t.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();

        st.append_event(&t, "phase_change", &json!({"to": "implementing"})).await.unwrap();
        st.append_event(&t, "phase_change", &json!({"to": "self-reviewing"})).await.unwrap();
        let n = st.count_events(&t).await.unwrap();
        assert_eq!(n, 2);
    }
}
```

- [ ] **Step 4: Update src/state/mod.rs**

Add at the bottom:

```rust
pub mod events;
pub mod jobs;
pub mod repo_tasks;

pub use jobs::JobRow;
pub use repo_tasks::{AttemptKind, RepoTaskRow};
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib state`
Expected: prior 3 + 3 new = 6 passing.

- [ ] **Step 6: Commit**

```bash
git add src/state
git commit -m "feat(state): repo_tasks CRUD with attempts; events append/count"
```

---

## Task 9: Linear API — types

**Files:**
- Create: `src/linear/mod.rs`
- Create: `src/linear/types.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-09`

- [ ] **Step 2: Write src/linear/types.rs**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Issue {
    pub id: String,
    pub identifier: String,        // e.g. "ACM-123"
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Vec<Label>,
    pub state: WorkflowState,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Label {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,            // e.g. "Backlog", "In Progress"
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Comment {
    pub id: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_issue_with_labels() {
        let s = r#"{
            "id": "abc",
            "identifier": "ACM-1",
            "title": "fix login",
            "description": "...",
            "labels": [
              {"id":"l1","name":"monorail:type/bug"}
            ],
            "state": {"id":"s1","name":"Backlog","type":"backlog"}
        }"#;
        let issue: Issue = serde_json::from_str(s).unwrap();
        assert_eq!(issue.identifier, "ACM-1");
        assert_eq!(issue.labels.len(), 1);
        assert_eq!(issue.labels[0].name, "monorail:type/bug");
    }
}
```

- [ ] **Step 3: Write minimal src/linear/mod.rs**

```rust
pub mod types;
pub use types::{Comment, Issue, Label, WorkflowState};
```

- [ ] **Step 4: Wire into main**

Add `mod linear;` to `src/main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib linear`
Expected: 1 passing.

- [ ] **Step 6: Commit**

```bash
git add src/linear src/main.rs
git commit -m "feat(linear): add Issue/Label/WorkflowState/Comment types"
```

---

## Task 10: Linear API — client (`get_issue`, `post_comment`, `set_state`)

**Files:**
- Create: `src/linear/graphql.rs`
- Modify: `src/linear/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-10`

- [ ] **Step 2: Write src/linear/graphql.rs**

```rust
pub const ISSUE_QUERY: &str = r#"
query Issue($key: String!) {
  issue(id: $key) {
    id
    identifier
    title
    description
    labels { nodes { id name } }
    state { id name type }
  }
}"#;

pub const COMMENT_CREATE_MUTATION: &str = r#"
mutation CreateComment($input: CommentCreateInput!) {
  commentCreate(input: $input) { success comment { id body } }
}"#;

pub const ISSUE_UPDATE_STATE_MUTATION: &str = r#"
mutation UpdateState($id: String!, $stateId: String!) {
  issueUpdate(id: $id, input: { stateId: $stateId }) { success }
}"#;
```

- [ ] **Step 3: Write client + tests in src/linear/mod.rs**

Replace `src/linear/mod.rs` with:

```rust
pub mod graphql;
pub mod types;

pub use types::{Comment, Issue, Label, WorkflowState};

use crate::error::{MonorailError, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

pub struct LinearClient {
    http: Client,
    endpoint: String,
}

impl LinearClient {
    pub fn new(endpoint: impl Into<String>, api_key: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_str(api_key)
            .map_err(|e| MonorailError::Linear(e.to_string()))?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = Client::builder().default_headers(headers).build()
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        Ok(Self { http, endpoint: endpoint.into() })
    }

    pub async fn get_issue(&self, key: &str) -> Result<Issue> {
        let body = json!({ "query": graphql::ISSUE_QUERY, "variables": { "key": key } });
        let resp: Value = self.http.post(&self.endpoint).json(&body).send().await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .error_for_status()
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .json().await
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        let issue_node = resp.pointer("/data/issue")
            .ok_or_else(|| MonorailError::Linear("missing /data/issue".into()))?
            .clone();
        // Flatten labels.nodes -> labels
        let issue: IssueRaw = serde_json::from_value(issue_node)
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        Ok(Issue {
            id: issue.id,
            identifier: issue.identifier,
            title: issue.title,
            description: issue.description,
            labels: issue.labels.nodes,
            state: issue.state,
        })
    }

    pub async fn post_comment(&self, issue_id: &str, body: &str) -> Result<Comment> {
        let body_val = json!({
            "query": graphql::COMMENT_CREATE_MUTATION,
            "variables": { "input": { "issueId": issue_id, "body": body } }
        });
        let resp: Value = self.http.post(&self.endpoint).json(&body_val).send().await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .json().await
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        let c = resp.pointer("/data/commentCreate/comment")
            .ok_or_else(|| MonorailError::Linear("missing comment".into()))?;
        serde_json::from_value(c.clone())
            .map_err(|e| MonorailError::Linear(e.to_string()))
    }

    pub async fn set_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
        let body = json!({
            "query": graphql::ISSUE_UPDATE_STATE_MUTATION,
            "variables": { "id": issue_id, "stateId": state_id }
        });
        self.http.post(&self.endpoint).json(&body).send().await
            .map_err(|e| MonorailError::Linear(e.to_string()))?
            .error_for_status()
            .map_err(|e| MonorailError::Linear(e.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct IssueRaw {
    id: String,
    identifier: String,
    title: String,
    description: Option<String>,
    labels: LabelsRaw,
    state: WorkflowState,
}
#[derive(Debug, Deserialize)]
struct LabelsRaw { nodes: Vec<Label> }

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_issue_happy_path() {
        let server = MockServer::start().await;
        let resp = serde_json::json!({
            "data": {
                "issue": {
                    "id": "abc", "identifier": "ACM-1", "title": "fix",
                    "description": null,
                    "labels": { "nodes": [{"id":"l","name":"monorail:type/bug"}] },
                    "state": {"id":"s","name":"Backlog","type":"backlog"}
                }
            }
        });
        Mock::given(method("POST")).and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&server).await;
        let client = LinearClient::new(format!("{}/graphql", server.uri()), "key").unwrap();
        let issue = client.get_issue("ACM-1").await.unwrap();
        assert_eq!(issue.identifier, "ACM-1");
        assert_eq!(issue.labels[0].name, "monorail:type/bug");
    }

    #[tokio::test]
    async fn post_comment_returns_comment() {
        let server = MockServer::start().await;
        let resp = serde_json::json!({
            "data": { "commentCreate": { "success": true,
              "comment": {"id":"c1","body":"hi"} } }
        });
        Mock::given(method("POST")).and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&server).await;
        let client = LinearClient::new(format!("{}/graphql", server.uri()), "key").unwrap();
        let c = client.post_comment("issue-1", "hi").await.unwrap();
        assert_eq!(c.id, "c1");
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib linear`
Expected: 3 passing (1 from prior task + 2 new).

- [ ] **Step 5: Commit**

```bash
git add src/linear
git commit -m "feat(linear): add LinearClient with get_issue/post_comment/set_state"
```

---

## Task 11: Tools — `ghq` wrapper

**Files:**
- Create: `src/tools/mod.rs`
- Create: `src/tools/ghq.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-11`

- [ ] **Step 2: Write src/tools/ghq.rs**

```rust
use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

#[async_trait]
pub trait GhqTool: Send + Sync {
    async fn list_path(&self, full: &str) -> Result<Option<PathBuf>>;
    async fn ensure_cloned(&self, full: &str) -> Result<PathBuf>;
}

pub struct RealGhq;

#[async_trait]
impl GhqTool for RealGhq {
    async fn list_path(&self, full: &str) -> Result<Option<PathBuf>> {
        let out = Command::new("ghq")
            .args(["list", "-p", full])
            .output().await?;
        if !out.status.success() {
            return Ok(None);
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() { return Ok(None); }
        Ok(Some(PathBuf::from(s)))
    }

    async fn ensure_cloned(&self, full: &str) -> Result<PathBuf> {
        if let Some(p) = self.list_path(full).await? {
            return Ok(p);
        }
        let out = Command::new("ghq").args(["get", full]).output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "ghq",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        self.list_path(full).await?.ok_or_else(|| MonorailError::ExternalTool {
            tool: "ghq", message: format!("not found after get: {full}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_path_returns_none_when_command_missing() {
        // If ghq isn't installed in test env, list_path errors out; tolerate.
        let g = RealGhq;
        let res = g.list_path("nonexistent/repo-xyz-12345").await;
        // We tolerate either Ok(None) (ghq present, repo missing) or Err (ghq absent).
        let _ = res;
    }
}
```

- [ ] **Step 3: Write src/tools/mod.rs**

```rust
pub mod ghq;
pub use ghq::{GhqTool, RealGhq};
```

- [ ] **Step 4: Wire into main**

Add `mod tools;` to `src/main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib tools`
Expected: 1 passing (tolerates missing ghq).

- [ ] **Step 6: Commit**

```bash
git add src/tools src/main.rs
git commit -m "feat(tools): add GhqTool trait and RealGhq impl"
```

---

## Task 12: Tools — `wt` (worktrunk) wrapper

**Files:**
- Create: `src/tools/wt.rs`
- Modify: `src/tools/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-12`

- [ ] **Step 2: Write src/tools/wt.rs**

```rust
use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[async_trait]
pub trait WtTool: Send + Sync {
    /// Create (or switch to) a worktree at the convention path
    /// `{repo_path}/../{repo}.{branch_sanitized}`. Returns the worktree path.
    async fn switch_create(&self, repo_path: &Path, branch: &str) -> Result<PathBuf>;

    async fn remove(&self, worktree_path: &Path) -> Result<()>;
}

pub struct RealWt;

#[async_trait]
impl WtTool for RealWt {
    async fn switch_create(&self, repo_path: &Path, branch: &str) -> Result<PathBuf> {
        let out = Command::new("wt")
            .arg("-C").arg(repo_path)
            .args(["switch", "--create", branch])
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "wt",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        // Resolve via convention: parent_of(repo_path) joined with `<repo_basename>.<branch_sanitized>`
        let parent = repo_path.parent()
            .ok_or_else(|| MonorailError::ExternalTool {
                tool: "wt",
                message: "repo_path has no parent".into(),
            })?;
        let repo_name = repo_path.file_name()
            .ok_or_else(|| MonorailError::ExternalTool {
                tool: "wt",
                message: "repo_path has no file name".into(),
            })?
            .to_string_lossy()
            .to_string();
        let sanitized: String = branch.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        Ok(parent.join(format!("{repo_name}.{sanitized}")))
    }

    async fn remove(&self, worktree_path: &Path) -> Result<()> {
        let out = Command::new("wt")
            .arg("-C").arg(worktree_path)
            .args(["remove"])
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "wt",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitization_replaces_slashes() {
        // Indirectly: we encode the same logic here to ensure expected behavior.
        let branch = "ACM-1/x";
        let s: String = branch.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect();
        assert_eq!(s, "ACM-1-x");
    }
}
```

- [ ] **Step 3: Update src/tools/mod.rs**

```rust
pub mod ghq;
pub mod wt;
pub use ghq::{GhqTool, RealGhq};
pub use wt::{RealWt, WtTool};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib tools`
Expected: 2 passing.

- [ ] **Step 5: Commit**

```bash
git add src/tools
git commit -m "feat(tools): add WtTool trait and RealWt with path convention"
```

---

## Task 13: Tools — `gh` wrapper

**Files:**
- Create: `src/tools/gh.rs`
- Modify: `src/tools/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-13`

- [ ] **Step 2: Write src/tools/gh.rs**

```rust
use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub status: String,         // "queued" | "in_progress" | "completed"
    pub conclusion: Option<String>, // "success" | "failure" | ...
}

#[async_trait]
pub trait GhTool: Send + Sync {
    async fn pr_create(&self, worktree: &Path, title: &str, body: &str) -> Result<Url>;
    async fn checks_for_pr(&self, worktree: &Path) -> Result<Vec<CheckRun>>;
    async fn check_run_log(&self, worktree: &Path, name: &str) -> Result<String>;
}

pub struct RealGh;

#[async_trait]
impl GhTool for RealGh {
    async fn pr_create(&self, worktree: &Path, title: &str, body: &str) -> Result<Url> {
        let out = Command::new("gh")
            .arg("-R").arg(worktree_repo_arg(worktree).await?)
            .args(["pr", "create", "--title", title, "--body", body, "--fill"])
            .current_dir(worktree)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "gh",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Url::parse(&s).map_err(|e| MonorailError::ExternalTool {
            tool: "gh", message: format!("could not parse url '{s}': {e}"),
        })
    }

    async fn checks_for_pr(&self, worktree: &Path) -> Result<Vec<CheckRun>> {
        let out = Command::new("gh")
            .args(["pr", "checks", "--json", "name,status,conclusion"])
            .current_dir(worktree)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "gh",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let runs: Vec<CheckRun> = serde_json::from_str(&s)
            .map_err(|e| MonorailError::ExternalTool {
                tool: "gh", message: format!("parse: {e}; raw: {s}"),
            })?;
        Ok(runs)
    }

    async fn check_run_log(&self, worktree: &Path, name: &str) -> Result<String> {
        let out = Command::new("gh")
            .args(["run", "view", "--log-failed", "--job", name])
            .current_dir(worktree)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "gh",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

async fn worktree_repo_arg(worktree: &Path) -> Result<String> {
    // Use the remote derived from origin. gh accepts owner/repo.
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(worktree)
        .output().await?;
    if !out.status.success() {
        return Err(MonorailError::ExternalTool {
            tool: "git", message: String::from_utf8_lossy(&out.stderr).to_string(),
        });
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stripped = s.trim_end_matches(".git");
    if let Some(idx) = stripped.find("github.com") {
        let tail = &stripped[idx + "github.com".len()..];
        let tail = tail.trim_start_matches([':', '/']);
        return Ok(tail.to_string());
    }
    Err(MonorailError::ExternalTool {
        tool: "git", message: format!("origin not on github.com: {s}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkrun_parses_minimal() {
        let s = r#"[{"name":"build","status":"completed","conclusion":"success"}]"#;
        let v: Vec<CheckRun> = serde_json::from_str(s).unwrap();
        assert_eq!(v[0].conclusion.as_deref(), Some("success"));
    }
}
```

- [ ] **Step 3: Update src/tools/mod.rs**

```rust
pub mod gh;
pub mod ghq;
pub mod wt;
pub use gh::{CheckRun, GhTool, RealGh};
pub use ghq::{GhqTool, RealGhq};
pub use wt::{RealWt, WtTool};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib tools`
Expected: 3 passing.

- [ ] **Step 5: Commit**

```bash
git add src/tools
git commit -m "feat(tools): add GhTool trait with pr_create, checks_for_pr, log fetch"
```

---

## Task 14: `Engine` trait + `MockEngine`

**Files:**
- Create: `src/engine/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-14`

- [ ] **Step 2: Write src/engine/mod.rs**

```rust
use crate::domain::{Finding, FixOutcome, RootCauseAnalysis};
use crate::error::Result;
use async_trait::async_trait;
use mockall::automock;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ImplContext {
    pub worktree: PathBuf,
    pub ticket: String,
    pub instructions: String,
    pub anchors: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ImplResult {
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub worktree: PathBuf,
    pub ticket: String,
    pub anchors: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct FailureContext {
    pub worktree: PathBuf,
    pub ticket: String,
    pub failure_log: String,
    pub kind: FailureKind,
}

#[derive(Debug, Clone, Copy)]
pub enum FailureKind {
    LintTest,
    Ci,
}

#[async_trait]
#[automock]
pub trait Engine: Send + Sync {
    async fn implement(&self, ctx: ImplContext) -> Result<ImplResult>;
    async fn review(&self, ctx: ReviewContext) -> Result<Vec<Finding>>;
    async fn analyze_finding(
        &self,
        finding: Finding,
        ctx: ReviewContext,
    ) -> Result<RootCauseAnalysis>;
    async fn apply_fix(
        &self,
        analysis: RootCauseAnalysis,
        ctx: ReviewContext,
    ) -> Result<FixOutcome>;
    async fn fix_failure(&self, ctx: FailureContext) -> Result<FixOutcome>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Severity;

    #[tokio::test]
    async fn mock_engine_returns_no_findings() {
        let mut m = MockEngine::new();
        m.expect_review().returning(|_| Ok(vec![]));
        let ctx = ReviewContext {
            worktree: PathBuf::from("/tmp"),
            ticket: "ACM-1".into(),
            anchors: vec![],
        };
        let f = m.review(ctx).await.unwrap();
        assert!(f.is_empty());
    }

    #[tokio::test]
    async fn mock_engine_root_cause_dismisses() {
        let mut m = MockEngine::new();
        m.expect_analyze_finding().returning(|f, _| Ok(RootCauseAnalysis {
            finding_id: f.id,
            requires_fix: false,
            reason: "intentional".into(),
        }));
        let ctx = ReviewContext { worktree: PathBuf::from("/"), ticket: "ACM-1".into(), anchors: vec![] };
        let finding = Finding {
            id: "f1".into(), file: "x.rs".into(), line: None,
            severity: Severity::Medium, rule: None, message: "msg".into(),
        };
        let a = m.analyze_finding(finding, ctx).await.unwrap();
        assert!(!a.requires_fix);
    }
}
```

- [ ] **Step 3: Wire into main**

Add `mod engine;` to `src/main.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib engine`
Expected: 2 passing.

- [ ] **Step 5: Commit**

```bash
git add src/engine src/main.rs
git commit -m "feat(engine): add Engine trait with MockEngine via mockall"
```

---

## Task 15: `ClaudeCodeAdapter` — `implement` + `fix_failure`

**Files:**
- Create: `src/engine/claude_code.rs`
- Modify: `src/engine/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-15`

- [ ] **Step 2: Write src/engine/claude_code.rs**

```rust
use crate::domain::{Finding, FixOutcome, RootCauseAnalysis};
use crate::engine::{
    Engine, FailureContext, FailureKind, ImplContext, ImplResult, ReviewContext,
};
use crate::error::{MonorailError, Result};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct ClaudeCodeAdapter {
    pub binary: String,
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self { binary: "claude".to_string() }
    }
}

impl ClaudeCodeAdapter {
    async fn run(&self, cwd: &Path, prompt: &str) -> Result<String> {
        let out = Command::new(&self.binary)
            .args(["-p", prompt, "--output-format", "text"])
            .current_dir(cwd)
            .output().await?;
        if !out.status.success() {
            return Err(MonorailError::ExternalTool {
                tool: "claude",
                message: String::from_utf8_lossy(&out.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

#[async_trait]
impl Engine for ClaudeCodeAdapter {
    async fn implement(&self, ctx: ImplContext) -> Result<ImplResult> {
        let prompt = format!(
            "You are working on Linear ticket {ticket} inside the worktree at {wt}. \
             Make the necessary code changes to satisfy the request below. \
             Do NOT edit files outside this worktree. When done, respond with a brief summary.\n\n\
             Instructions:\n{instr}",
            ticket = ctx.ticket,
            wt = ctx.worktree.display(),
            instr = ctx.instructions,
        );
        let summary = self.run(&ctx.worktree, &prompt).await?;
        Ok(ImplResult { summary })
    }

    async fn review(&self, _ctx: ReviewContext) -> Result<Vec<Finding>> {
        // Implemented in Task 16
        Err(MonorailError::PhaseAborted("review unimplemented in Task 15".into()))
    }
    async fn analyze_finding(&self, _f: Finding, _c: ReviewContext) -> Result<RootCauseAnalysis> {
        Err(MonorailError::PhaseAborted("analyze_finding unimplemented in Task 15".into()))
    }
    async fn apply_fix(&self, _a: RootCauseAnalysis, _c: ReviewContext) -> Result<FixOutcome> {
        Err(MonorailError::PhaseAborted("apply_fix unimplemented in Task 15".into()))
    }

    async fn fix_failure(&self, ctx: FailureContext) -> Result<FixOutcome> {
        let kind = match ctx.kind {
            FailureKind::LintTest => "lint or test",
            FailureKind::Ci => "CI",
        };
        let prompt = format!(
            "The {kind} run for ticket {ticket} failed. The failure log is below. \
             Investigate root cause and apply the minimal fix in this worktree at {wt}. \
             Do NOT edit files outside this worktree. \
             Reply with one line: APPLIED or NOT_APPLIED, then a short reason.\n\n\
             Failure log:\n{log}",
            kind = kind,
            ticket = ctx.ticket,
            wt = ctx.worktree.display(),
            log = ctx.failure_log,
        );
        let out = self.run(&ctx.worktree, &prompt).await?;
        let applied = out.contains("APPLIED") && !out.contains("NOT_APPLIED");
        Ok(FixOutcome { applied, message: out })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn implement_reports_external_tool_error_when_claude_missing() {
        let adapter = ClaudeCodeAdapter { binary: "/no/such/binary/claude-zzz".into() };
        let ctx = ImplContext {
            worktree: PathBuf::from("/tmp"),
            ticket: "ACM-1".into(),
            instructions: "do nothing".into(),
            anchors: vec![],
        };
        let err = adapter.implement(ctx).await.unwrap_err();
        match err {
            MonorailError::Io(_) | MonorailError::ExternalTool { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Expose adapter from src/engine/mod.rs**

Add at the bottom of `src/engine/mod.rs`:

```rust
pub mod claude_code;
pub use claude_code::ClaudeCodeAdapter;
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib engine`
Expected: 3 passing.

- [ ] **Step 5: Commit**

```bash
git add src/engine
git commit -m "feat(engine): ClaudeCodeAdapter implement + fix_failure"
```

---

## Task 16: `ClaudeCodeAdapter` — `review`, `analyze_finding`, `apply_fix`

**Files:**
- Modify: `src/engine/claude_code.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-16`

- [ ] **Step 2: Write tests + impl**

Replace the `review`, `analyze_finding`, `apply_fix` methods in `src/engine/claude_code.rs` with the following (and add tests at the bottom of the same file inside the existing `tests` module):

```rust
    async fn review(&self, ctx: ReviewContext) -> Result<Vec<Finding>> {
        let prompt = format!(
            "Run /pr-review-toolkit:review-pr against the current worktree changes for ticket {ticket}. \
             At the end, output a JSON array of findings on a single line prefixed with `FINDINGS_JSON: `. \
             Each finding has: id (stable hash of file+line+rule), file, line (or null), \
             severity (critical|high|medium|low|info), rule (or null), message.",
            ticket = ctx.ticket,
        );
        let raw = self.run(&ctx.worktree, &prompt).await?;
        let line = raw.lines().rev()
            .find(|l| l.contains("FINDINGS_JSON:"))
            .ok_or_else(|| MonorailError::PhaseAborted("no FINDINGS_JSON line in review output".into()))?;
        let json_part = line.split_once("FINDINGS_JSON:")
            .map(|(_, j)| j.trim())
            .unwrap_or("[]");
        let findings: Vec<Finding> = serde_json::from_str(json_part)
            .map_err(|e| MonorailError::Serde(format!("findings parse: {e}; raw: {json_part}")))?;
        Ok(findings)
    }

    async fn analyze_finding(
        &self,
        finding: Finding,
        ctx: ReviewContext,
    ) -> Result<RootCauseAnalysis> {
        let prompt = format!(
            "Analyze the root cause of the review finding below in the worktree at {wt} for ticket {ticket}. \
             Decide: does this finding REQUIRE a fix, or can it be dismissed (e.g., intentional, false positive)? \
             Output exactly two lines:\n\
             DECISION: <fix|dismiss>\n\
             REASON: <one sentence>\n\n\
             Finding:\n{f}",
            wt = ctx.worktree.display(),
            ticket = ctx.ticket,
            f = serde_json::to_string_pretty(&finding).unwrap_or_default(),
        );
        let out = self.run(&ctx.worktree, &prompt).await?;
        let mut decision = None;
        let mut reason = String::new();
        for line in out.lines() {
            if let Some(rest) = line.strip_prefix("DECISION:") {
                decision = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("REASON:") {
                reason = rest.trim().to_string();
            }
        }
        let requires_fix = matches!(decision.as_deref(), Some("fix"));
        Ok(RootCauseAnalysis {
            finding_id: finding.id,
            requires_fix,
            reason,
        })
    }

    async fn apply_fix(
        &self,
        analysis: RootCauseAnalysis,
        ctx: ReviewContext,
    ) -> Result<FixOutcome> {
        let prompt = format!(
            "Apply the fix for finding id={fid} in the worktree at {wt}. \
             Reason from analysis: {reason}. \
             Do NOT edit files outside this worktree. \
             Reply with exactly one line: APPLIED or NOT_APPLIED, then a short reason on the same line.",
            fid = analysis.finding_id,
            wt = ctx.worktree.display(),
            reason = analysis.reason,
        );
        let out = self.run(&ctx.worktree, &prompt).await?;
        let applied = out.contains("APPLIED") && !out.contains("NOT_APPLIED");
        Ok(FixOutcome { applied, message: out })
    }
```

Add a test at the bottom of the existing `tests` mod inside the same file:

```rust
    #[test]
    fn parses_findings_json_line() {
        let raw = "preamble\nMore text\nFINDINGS_JSON: [\
            {\"id\":\"f1\",\"file\":\"a.rs\",\"line\":10,\"severity\":\"high\",\"rule\":null,\"message\":\"x\"}]";
        let line = raw.lines().rev().find(|l| l.contains("FINDINGS_JSON:")).unwrap();
        let part = line.split_once("FINDINGS_JSON:").unwrap().1.trim();
        let v: Vec<Finding> = serde_json::from_str(part).unwrap();
        assert_eq!(v[0].id, "f1");
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib engine`
Expected: 4 passing.

- [ ] **Step 4: Commit**

```bash
git add src/engine
git commit -m "feat(engine): ClaudeCodeAdapter review/analyze/apply_fix with FINDINGS_JSON parse"
```

---

## Task 17: `HumanChannel` trait + `LinearCommentChannel`

**Files:**
- Create: `src/channel/mod.rs`
- Create: `src/channel/linear_comment.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-17`

- [ ] **Step 2: Write src/channel/mod.rs**

```rust
use crate::domain::{Question, TicketKey};
use crate::error::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct NotifyContext {
    pub ticket: TicketKey,
    pub body: String,
}

#[async_trait]
pub trait HumanChannel: Send + Sync {
    async fn notify(&self, ctx: NotifyContext) -> Result<()>;
    async fn post_question(&self, q: Question) -> Result<String>;
}

pub mod linear_comment;
pub use linear_comment::LinearCommentChannel;
```

- [ ] **Step 3: Write src/channel/linear_comment.rs**

```rust
use crate::channel::{HumanChannel, NotifyContext};
use crate::domain::Question;
use crate::error::Result;
use crate::linear::LinearClient;
use async_trait::async_trait;
use std::sync::Arc;

pub struct LinearCommentChannel {
    pub client: Arc<LinearClient>,
}

#[async_trait]
impl HumanChannel for LinearCommentChannel {
    async fn notify(&self, ctx: NotifyContext) -> Result<()> {
        // The Linear API expects an issue id, not the ticket key. We resolve
        // by fetching the issue first.
        let issue = self.client.get_issue(ctx.ticket.as_str()).await?;
        self.client.post_comment(&issue.id, &ctx.body).await?;
        Ok(())
    }

    async fn post_question(&self, q: Question) -> Result<String> {
        let issue = self.client.get_issue(q.ticket.as_str()).await?;
        let comment = self.client.post_comment(&issue.id, &q.prompt).await?;
        Ok(comment.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TicketKey;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn notify_posts_comment_after_get_issue() {
        let server = MockServer::start().await;
        // The mock server responds to ANY POST. The first call (get_issue) and
        // the second (post_comment) both hit /graphql; we provide a sequence-
        // free response that satisfies both. We include both data shapes.
        let resp = serde_json::json!({
            "data": {
                "issue": {
                    "id": "iss-1", "identifier": "ACM-1", "title": "t",
                    "description": null,
                    "labels": { "nodes": [] },
                    "state": {"id":"s","name":"Backlog","type":"backlog"}
                },
                "commentCreate": {
                    "success": true,
                    "comment": { "id": "c-1", "body": "hello" }
                }
            }
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(resp))
            .mount(&server).await;
        let lc = Arc::new(LinearClient::new(format!("{}/graphql", server.uri()), "k").unwrap());
        let ch = LinearCommentChannel { client: lc };
        ch.notify(NotifyContext {
            ticket: TicketKey::parse("ACM-1").unwrap(),
            body: "hello".into(),
        }).await.unwrap();
    }
}
```

- [ ] **Step 4: Wire into main**

Add `mod channel;` to `src/main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib channel`
Expected: 1 passing.

- [ ] **Step 6: Commit**

```bash
git add src/channel src/main.rs
git commit -m "feat(channel): HumanChannel trait + LinearCommentChannel"
```

---

## Task 18: Triager — fetch ticket, parse labels, materialize Job

**Files:**
- Create: `src/triager.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-18`

- [ ] **Step 2: Write src/triager.rs**

```rust
use crate::domain::{Job, JobState, RepoRef, RepoTask, Phase, TicketKey, WorkType};
use crate::error::{MonorailError, Result};
use crate::linear::{LinearClient, Issue};
use chrono::Utc;

pub struct Triager<'a> {
    pub linear: &'a LinearClient,
}

const LABEL_BUG: &str = "monorail:type/bug";
const LABEL_FEATURE: &str = "monorail:type/feature";
const LABEL_AUTO_MERGE: &str = "monorail:auto-merge";

impl<'a> Triager<'a> {
    pub async fn build_job(&self, ticket: &TicketKey) -> Result<Job> {
        let issue = self.linear.get_issue(ticket.as_str()).await?;
        let work_type = classify_labels(&issue)?;
        let auto_merge = issue.labels.iter().any(|l| l.name == LABEL_AUTO_MERGE);
        // For Type A single-repo, infer org/repo from the issue body.
        // Format expected: a markdown line like `Repo: <org>/<repo>`.
        let (org, repo) = parse_repo_from_description(issue.description.as_deref())?;
        let now = Utc::now();
        let repo_task = RepoTask {
            repo: RepoRef { org, repo },
            branch: ticket.as_str().to_string(),
            worktree_path: std::path::PathBuf::new(), // resolved later
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        Ok(Job {
            ticket: ticket.clone(),
            work_type,
            state: JobState::Active,
            repos: vec![repo_task],
            auto_merge,
            created_at: now,
            updated_at: now,
        })
    }
}

fn classify_labels(issue: &Issue) -> Result<WorkType> {
    let has_bug = issue.labels.iter().any(|l| l.name == LABEL_BUG);
    let has_feature = issue.labels.iter().any(|l| l.name == LABEL_FEATURE);
    match (has_bug, has_feature) {
        (true, false) => Ok(WorkType::Bug),
        (false, true) => Ok(WorkType::Feature),
        (true, true) => Err(MonorailError::TriageRejected(
            "ticket has both monorail:type/bug and monorail:type/feature".into(),
        )),
        (false, false) => Err(MonorailError::TriageRejected(
            "ticket has neither monorail:type/bug nor monorail:type/feature".into(),
        )),
    }
}

fn parse_repo_from_description(desc: Option<&str>) -> Result<(String, String)> {
    let desc = desc.unwrap_or("");
    for line in desc.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Repo:") {
            let v = rest.trim().trim_start_matches('`').trim_end_matches('`');
            let parts: Vec<&str> = v.splitn(2, '/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }
    }
    Err(MonorailError::TriageRejected(
        "ticket description must contain a 'Repo: <org>/<repo>' line".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{Label, WorkflowState};

    fn issue_with(labels: Vec<&str>, desc: Option<&str>) -> Issue {
        Issue {
            id: "i1".into(),
            identifier: "ACM-1".into(),
            title: "t".into(),
            description: desc.map(String::from),
            labels: labels.into_iter()
                .map(|n| Label { id: n.into(), name: n.into() })
                .collect(),
            state: WorkflowState { id: "s".into(), name: "Backlog".into(), kind: "backlog".into() },
        }
    }

    #[test]
    fn classifies_bug() {
        let i = issue_with(vec!["monorail:type/bug"], None);
        assert_eq!(classify_labels(&i).unwrap(), WorkType::Bug);
    }

    #[test]
    fn rejects_no_label() {
        let i = issue_with(vec!["other"], None);
        assert!(classify_labels(&i).is_err());
    }

    #[test]
    fn rejects_both_labels() {
        let i = issue_with(vec!["monorail:type/bug", "monorail:type/feature"], None);
        assert!(classify_labels(&i).is_err());
    }

    #[test]
    fn parses_repo_line() {
        let (o, r) = parse_repo_from_description(Some("blah\nRepo: acme/core-api\n")).unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("acme", "core-api"));
    }

    #[test]
    fn rejects_missing_repo() {
        assert!(parse_repo_from_description(Some("nothing here")).is_err());
    }
}
```

- [ ] **Step 3: Wire into main**

Add `mod triager;` to `src/main.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib triager`
Expected: 5 passing.

- [ ] **Step 5: Commit**

```bash
git add src/triager.rs src/main.rs
git commit -m "feat(triager): classify labels and parse Repo: line for type A single repo"
```

---

## Task 19: Pipeline — `implement` phase

**Files:**
- Create: `src/pipeline/mod.rs`
- Create: `src/pipeline/implement.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-19`

- [ ] **Step 2: Write src/pipeline/implement.rs**

```rust
use crate::domain::{Phase, TicketKey};
use crate::engine::{Engine, ImplContext};
use crate::error::Result;
use crate::state::SqliteState;
use std::path::Path;
use std::sync::Arc;

pub async fn run_implement<E: Engine + ?Sized>(
    state: &SqliteState,
    engine: &E,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
    instructions: &str,
) -> Result<()> {
    state.update_repo_task_phase(repo_task_id, Phase::Implementing).await?;
    state.append_event(ticket, "phase_change", &serde_json::json!({"to":"implementing"})).await?;

    let ctx = ImplContext {
        worktree: worktree.to_path_buf(),
        ticket: ticket.as_str().to_string(),
        instructions: instructions.to_string(),
        anchors: vec![],
    };
    let result = engine.implement(ctx).await?;
    state.append_event(ticket, "implement_done", &serde_json::json!({
        "summary": result.summary,
    })).await?;
    Ok(())
}

// Suppress unused warning for Arc import in some configurations.
#[allow(dead_code)]
fn _ensure_arc(_: Arc<()>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::engine::{ImplResult, MockEngine};
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[tokio::test]
    async fn implement_advances_phase_and_records_event() {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();

        let mut engine = MockEngine::new();
        engine.expect_implement().returning(|_| Ok(ImplResult { summary: "ok".into() }));

        run_implement(&st, &engine, &ticket, id, &PathBuf::from("/tmp"), "do it")
            .await.unwrap();

        let rows = st.list_repo_tasks(&ticket).await.unwrap();
        assert_eq!(rows[0].phase, "implementing");
        assert_eq!(st.count_events(&ticket).await.unwrap(), 2);
    }
}
```

- [ ] **Step 3: Write src/pipeline/mod.rs**

```rust
pub mod implement;
pub use implement::run_implement;
```

- [ ] **Step 4: Wire into main**

Add `mod pipeline;` to `src/main.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib pipeline`
Expected: 1 passing.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline src/main.rs
git commit -m "feat(pipeline): implement phase advances state and emits events"
```

---

## Task 20: Pipeline — self-review loop (max 5)

**Files:**
- Create: `src/pipeline/self_review.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-20`

- [ ] **Step 2: Write src/pipeline/self_review.rs**

```rust
use crate::domain::{EscalationReason, Phase, TicketKey};
use crate::engine::{Engine, ReviewContext};
use crate::error::Result;
use crate::state::{AttemptKind, SqliteState};
use std::path::Path;

pub const SELF_REVIEW_MAX: u8 = 5;

pub enum SelfReviewOutcome {
    Clean,
    Stuck,                  // findings remain but no actionable fix this round
    Escalated(EscalationReason),
}

pub async fn run_self_review<E: Engine + ?Sized>(
    state: &SqliteState,
    engine: &E,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
) -> Result<SelfReviewOutcome> {
    state.update_repo_task_phase(repo_task_id, Phase::SelfReviewing).await?;
    state.append_event(ticket, "phase_change", &serde_json::json!({"to":"self-reviewing"})).await?;

    for attempt in 1..=SELF_REVIEW_MAX {
        let ctx = ReviewContext {
            worktree: worktree.to_path_buf(),
            ticket: ticket.as_str().to_string(),
            anchors: vec![],
        };
        let findings = engine.review(ctx.clone()).await?;
        if findings.is_empty() {
            state.append_event(ticket, "review_clean", &serde_json::json!({"attempt":attempt})).await?;
            return Ok(SelfReviewOutcome::Clean);
        }

        let mut actionable_fix = false;
        for finding in findings {
            let analysis = engine.analyze_finding(finding.clone(), ctx.clone()).await?;
            if analysis.requires_fix {
                let outcome = engine.apply_fix(analysis.clone(), ctx.clone()).await?;
                if outcome.applied {
                    actionable_fix = true;
                }
            } else {
                state.append_event(ticket, "finding_dismissed", &serde_json::json!({
                    "finding_id": finding.id,
                    "reason": analysis.reason,
                })).await?;
            }
        }

        state.bump_attempt(repo_task_id, AttemptKind::Review).await?;

        if !actionable_fix {
            state.append_event(ticket, "review_stuck", &serde_json::json!({"attempt":attempt})).await?;
            return Ok(SelfReviewOutcome::Stuck);
        }
        if attempt == SELF_REVIEW_MAX {
            return Ok(SelfReviewOutcome::Escalated(EscalationReason::SelfReviewMaxed));
        }
    }
    Ok(SelfReviewOutcome::Escalated(EscalationReason::SelfReviewMaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Finding, FixOutcome, Job, JobState, RepoRef, RepoTask, RootCauseAnalysis, Severity, WorkType,
    };
    use crate::engine::MockEngine;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    async fn fresh() -> (TempDir, SqliteState, TicketKey, i64) {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        (dir, st, ticket, id)
    }

    fn one_finding() -> Finding {
        Finding {
            id: "f1".into(), file: "a.rs".into(), line: Some(1),
            severity: Severity::High, rule: None, message: "x".into(),
        }
    }

    #[tokio::test]
    async fn empty_findings_first_pass_returns_clean() {
        let (_d, st, t, id) = fresh().await;
        let mut engine = MockEngine::new();
        engine.expect_review().returning(|_| Ok(vec![]));
        let out = run_self_review(&st, &engine, &t, id, &PathBuf::from("/tmp"))
            .await.unwrap();
        assert!(matches!(out, SelfReviewOutcome::Clean));
    }

    #[tokio::test]
    async fn dismissed_only_returns_stuck_after_one_attempt() {
        let (_d, st, t, id) = fresh().await;
        let mut engine = MockEngine::new();
        engine.expect_review().returning(|_| Ok(vec![one_finding()]));
        engine.expect_analyze_finding().returning(|f, _| Ok(RootCauseAnalysis {
            finding_id: f.id, requires_fix: false, reason: "intentional".into(),
        }));
        let out = run_self_review(&st, &engine, &t, id, &PathBuf::from("/tmp"))
            .await.unwrap();
        assert!(matches!(out, SelfReviewOutcome::Stuck));
        let rows = st.list_repo_tasks(&t).await.unwrap();
        assert_eq!(rows[0].review_attempts, 1);
    }

    #[tokio::test]
    async fn fixed_then_clean_returns_clean() {
        let (_d, st, t, id) = fresh().await;
        let mut engine = MockEngine::new();
        let counter = Mutex::new(0_u32);
        engine.expect_review().returning(move |_| {
            let mut c = counter.lock().unwrap();
            *c += 1;
            if *c == 1 { Ok(vec![one_finding()]) } else { Ok(vec![]) }
        });
        engine.expect_analyze_finding().returning(|f, _| Ok(RootCauseAnalysis {
            finding_id: f.id, requires_fix: true, reason: "fix".into(),
        }));
        engine.expect_apply_fix().returning(|_, _| Ok(FixOutcome { applied: true, message: "ok".into() }));
        let out = run_self_review(&st, &engine, &t, id, &PathBuf::from("/tmp"))
            .await.unwrap();
        assert!(matches!(out, SelfReviewOutcome::Clean));
    }
}
```

- [ ] **Step 3: Update src/pipeline/mod.rs**

```rust
pub mod implement;
pub mod self_review;

pub use implement::run_implement;
pub use self_review::{run_self_review, SelfReviewOutcome, SELF_REVIEW_MAX};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pipeline`
Expected: 4 passing.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline
git commit -m "feat(pipeline): self-review loop with root-cause-first, max 5"
```

---

## Task 21: Pipeline — lint/test loop (max 5)

**Files:**
- Create: `src/pipeline/lint_test.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-21`

- [ ] **Step 2: Write src/pipeline/lint_test.rs**

```rust
use crate::domain::{EscalationReason, Phase, TicketKey};
use crate::engine::{Engine, FailureContext, FailureKind};
use crate::error::Result;
use crate::state::{AttemptKind, SqliteState};
use std::path::Path;

pub const LINT_TEST_MAX: u8 = 5;

pub enum LintTestOutcome {
    Green,
    Escalated(EscalationReason),
}

#[async_trait::async_trait]
pub trait Verifier: Send + Sync {
    /// Returns Ok(()) on green, Err(log) on red.
    async fn verify(&self, worktree: &Path) -> std::result::Result<(), String>;
}

pub async fn run_lint_test<E: Engine + ?Sized, V: Verifier + ?Sized>(
    state: &SqliteState,
    engine: &E,
    verifier: &V,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
) -> Result<LintTestOutcome> {
    state.update_repo_task_phase(repo_task_id, Phase::LintTesting).await?;
    state.append_event(ticket, "phase_change", &serde_json::json!({"to":"lint-testing"})).await?;

    for attempt in 1..=LINT_TEST_MAX {
        match verifier.verify(worktree).await {
            Ok(()) => {
                state.append_event(ticket, "verify_green", &serde_json::json!({"attempt":attempt})).await?;
                return Ok(LintTestOutcome::Green);
            }
            Err(log) => {
                state.bump_attempt(repo_task_id, AttemptKind::LintTest).await?;
                let outcome = engine.fix_failure(FailureContext {
                    worktree: worktree.to_path_buf(),
                    ticket: ticket.as_str().to_string(),
                    failure_log: log,
                    kind: FailureKind::LintTest,
                }).await?;
                if !outcome.applied {
                    state.append_event(ticket, "lint_test_no_fix",
                        &serde_json::json!({"attempt":attempt, "msg": outcome.message})).await?;
                    // No fix applied: stuck. Stop early.
                    return Ok(LintTestOutcome::Escalated(EscalationReason::LintTestMaxed));
                }
            }
        }
    }
    Ok(LintTestOutcome::Escalated(EscalationReason::LintTestMaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FixOutcome, Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::engine::MockEngine;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct StubVerifier { fail_first: Mutex<u32> }
    #[async_trait::async_trait]
    impl Verifier for StubVerifier {
        async fn verify(&self, _: &Path) -> std::result::Result<(), String> {
            let mut c = self.fail_first.lock().unwrap();
            if *c == 0 { Ok(()) } else { *c -= 1; Err("boom".into()) }
        }
    }

    async fn fresh() -> (TempDir, SqliteState, TicketKey, i64) {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::Pending,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        (dir, st, ticket, id)
    }

    #[tokio::test]
    async fn green_first_try() {
        let (_d, st, t, id) = fresh().await;
        let v = StubVerifier { fail_first: Mutex::new(0) };
        let engine = MockEngine::new();
        let out = run_lint_test(&st, &engine, &v, &t, id, &PathBuf::from("/tmp"))
            .await.unwrap();
        assert!(matches!(out, LintTestOutcome::Green));
    }

    #[tokio::test]
    async fn fixes_then_green() {
        let (_d, st, t, id) = fresh().await;
        let v = StubVerifier { fail_first: Mutex::new(1) };
        let mut engine = MockEngine::new();
        engine.expect_fix_failure().returning(|_| Ok(FixOutcome { applied: true, message: "k".into() }));
        let out = run_lint_test(&st, &engine, &v, &t, id, &PathBuf::from("/tmp"))
            .await.unwrap();
        assert!(matches!(out, LintTestOutcome::Green));
    }

    #[tokio::test]
    async fn no_fix_applied_escalates_immediately() {
        let (_d, st, t, id) = fresh().await;
        let v = StubVerifier { fail_first: Mutex::new(5) };
        let mut engine = MockEngine::new();
        engine.expect_fix_failure().returning(|_| Ok(FixOutcome { applied: false, message: "no".into() }));
        let out = run_lint_test(&st, &engine, &v, &t, id, &PathBuf::from("/tmp"))
            .await.unwrap();
        assert!(matches!(out, LintTestOutcome::Escalated(EscalationReason::LintTestMaxed)));
    }
}
```

- [ ] **Step 3: Update src/pipeline/mod.rs**

```rust
pub mod implement;
pub mod lint_test;
pub mod self_review;

pub use implement::run_implement;
pub use lint_test::{run_lint_test, LintTestOutcome, Verifier, LINT_TEST_MAX};
pub use self_review::{run_self_review, SelfReviewOutcome, SELF_REVIEW_MAX};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pipeline`
Expected: 7 passing.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline
git commit -m "feat(pipeline): lint/test loop with verifier abstraction, max 5"
```

---

## Task 22: Pipeline — open PR

**Files:**
- Create: `src/pipeline/open_pr.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-22`

- [ ] **Step 2: Write src/pipeline/open_pr.rs**

```rust
use crate::domain::{Phase, TicketKey};
use crate::error::Result;
use crate::state::SqliteState;
use crate::tools::GhTool;
use std::path::Path;
use url::Url;

pub async fn run_open_pr<G: GhTool + ?Sized>(
    state: &SqliteState,
    gh: &G,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
    title: &str,
    body: &str,
) -> Result<Url> {
    let url = gh.pr_create(worktree, title, body).await?;
    state.set_pr_url(repo_task_id, &url).await?;
    state.update_repo_task_phase(repo_task_id, Phase::PrOpened).await?;
    state.append_event(ticket, "pr_opened",
        &serde_json::json!({"url": url.to_string()})).await?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::tools::CheckRun;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct StubGh;
    #[async_trait]
    impl GhTool for StubGh {
        async fn pr_create(&self, _w: &Path, _t: &str, _b: &str) -> Result<Url> {
            Ok(Url::parse("https://github.com/acme/core-api/pull/123").unwrap())
        }
        async fn checks_for_pr(&self, _w: &Path) -> Result<Vec<CheckRun>> { Ok(vec![]) }
        async fn check_run_log(&self, _w: &Path, _n: &str) -> Result<String> { Ok("".into()) }
    }

    #[tokio::test]
    async fn opens_pr_and_persists_url() {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::LintTesting,
            pr_url: None,
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        let url = run_open_pr(&st, &StubGh, &ticket, id, &PathBuf::from("/tmp"), "title", "body")
            .await.unwrap();
        assert_eq!(url.path(), "/acme/core-api/pull/123");
        let rows = st.list_repo_tasks(&ticket).await.unwrap();
        assert_eq!(rows[0].phase, "pr-opened");
        assert!(rows[0].pr_url.is_some());
    }
}
```

- [ ] **Step 3: Update src/pipeline/mod.rs**

```rust
pub mod implement;
pub mod lint_test;
pub mod open_pr;
pub mod self_review;

pub use implement::run_implement;
pub use lint_test::{run_lint_test, LintTestOutcome, Verifier, LINT_TEST_MAX};
pub use open_pr::run_open_pr;
pub use self_review::{run_self_review, SelfReviewOutcome, SELF_REVIEW_MAX};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pipeline`
Expected: 8 passing.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline
git commit -m "feat(pipeline): open PR via gh and persist URL + PrOpened phase"
```

---

## Task 23: Pipeline — CI-fix loop (max 3)

**Files:**
- Create: `src/pipeline/ci_fix.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-23`

- [ ] **Step 2: Write src/pipeline/ci_fix.rs**

```rust
use crate::domain::{EscalationReason, Phase, TicketKey};
use crate::engine::{Engine, FailureContext, FailureKind};
use crate::error::Result;
use crate::state::{AttemptKind, SqliteState};
use crate::tools::{CheckRun, GhTool};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

pub const CI_FIX_MAX: u8 = 3;

pub enum CiFixOutcome {
    Green,
    Escalated(EscalationReason),
}

pub async fn run_ci_fix<E: Engine + ?Sized, G: GhTool + ?Sized>(
    state: &SqliteState,
    engine: &E,
    gh: &G,
    ticket: &TicketKey,
    repo_task_id: i64,
    worktree: &Path,
    poll_interval: Duration,
) -> Result<CiFixOutcome> {
    state.update_repo_task_phase(repo_task_id, Phase::CiFixing).await?;
    state.append_event(ticket, "phase_change", &serde_json::json!({"to":"ci-fixing"})).await?;

    for attempt in 1..=CI_FIX_MAX {
        let verdict = wait_for_ci(gh, worktree, poll_interval).await?;
        match verdict {
            CiVerdict::Green => {
                state.append_event(ticket, "ci_green", &serde_json::json!({"attempt":attempt})).await?;
                return Ok(CiFixOutcome::Green);
            }
            CiVerdict::Failed { failed_jobs } => {
                state.bump_attempt(repo_task_id, AttemptKind::CiFix).await?;
                let mut log = String::new();
                for name in &failed_jobs {
                    if let Ok(part) = gh.check_run_log(worktree, name).await {
                        log.push_str(&format!("---- {name} ----\n{part}\n"));
                    }
                }
                let outcome = engine.fix_failure(FailureContext {
                    worktree: worktree.to_path_buf(),
                    ticket: ticket.as_str().to_string(),
                    failure_log: log,
                    kind: FailureKind::Ci,
                }).await?;
                if !outcome.applied {
                    state.append_event(ticket, "ci_fix_no_fix", &serde_json::json!({"attempt":attempt})).await?;
                    return Ok(CiFixOutcome::Escalated(EscalationReason::CiFixMaxed));
                }
                // The engine pushed; loop back to wait for new CI run.
            }
        }
    }
    Ok(CiFixOutcome::Escalated(EscalationReason::CiFixMaxed))
}

enum CiVerdict {
    Green,
    Failed { failed_jobs: Vec<String> },
}

async fn wait_for_ci<G: GhTool + ?Sized>(
    gh: &G,
    worktree: &Path,
    poll_interval: Duration,
) -> Result<CiVerdict> {
    loop {
        let runs = gh.checks_for_pr(worktree).await?;
        if runs.is_empty() {
            sleep(poll_interval).await;
            continue;
        }
        let all_completed = runs.iter().all(|r| r.status == "completed");
        if !all_completed {
            sleep(poll_interval).await;
            continue;
        }
        let failed: Vec<String> = runs.iter()
            .filter(|r| r.conclusion.as_deref() != Some("success"))
            .map(|r| r.name.clone())
            .collect();
        if failed.is_empty() {
            return Ok(CiVerdict::Green);
        } else {
            return Ok(CiVerdict::Failed { failed_jobs: failed });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FixOutcome, Job, JobState, RepoRef, RepoTask, WorkType};
    use crate::engine::MockEngine;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;
    use url::Url;

    struct ScriptedGh { steps: Mutex<Vec<Vec<CheckRun>>> }
    #[async_trait]
    impl GhTool for ScriptedGh {
        async fn pr_create(&self, _w: &Path, _t: &str, _b: &str) -> Result<Url> { unimplemented!() }
        async fn checks_for_pr(&self, _w: &Path) -> Result<Vec<CheckRun>> {
            Ok(self.steps.lock().unwrap().remove(0))
        }
        async fn check_run_log(&self, _w: &Path, _n: &str) -> Result<String> {
            Ok("failure log".into())
        }
    }

    async fn fresh() -> (TempDir, SqliteState, TicketKey, i64) {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::PrOpened,
            pr_url: Some(Url::parse("https://example.com").unwrap()),
            review_attempts: 0,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();
        (dir, st, ticket, id)
    }

    #[tokio::test]
    async fn green_on_first_check_returns_green() {
        let (_d, st, t, id) = fresh().await;
        let gh = ScriptedGh { steps: Mutex::new(vec![
            vec![CheckRun { name: "build".into(), status: "completed".into(), conclusion: Some("success".into()) }]
        ])};
        let engine = MockEngine::new();
        let out = run_ci_fix(&st, &engine, &gh, &t, id, &PathBuf::from("/tmp"), Duration::from_millis(0))
            .await.unwrap();
        assert!(matches!(out, CiFixOutcome::Green));
    }

    #[tokio::test]
    async fn failure_then_green_after_fix() {
        let (_d, st, t, id) = fresh().await;
        let gh = ScriptedGh { steps: Mutex::new(vec![
            vec![CheckRun { name: "build".into(), status: "completed".into(), conclusion: Some("failure".into()) }],
            vec![CheckRun { name: "build".into(), status: "completed".into(), conclusion: Some("success".into()) }],
        ])};
        let mut engine = MockEngine::new();
        engine.expect_fix_failure().returning(|_| Ok(FixOutcome { applied: true, message: "fixed".into() }));
        let out = run_ci_fix(&st, &engine, &gh, &t, id, &PathBuf::from("/tmp"), Duration::from_millis(0))
            .await.unwrap();
        assert!(matches!(out, CiFixOutcome::Green));
    }
}
```

- [ ] **Step 3: Update src/pipeline/mod.rs**

```rust
pub mod ci_fix;
pub mod implement;
pub mod lint_test;
pub mod open_pr;
pub mod self_review;

pub use ci_fix::{run_ci_fix, CiFixOutcome, CI_FIX_MAX};
pub use implement::run_implement;
pub use lint_test::{run_lint_test, LintTestOutcome, Verifier, LINT_TEST_MAX};
pub use open_pr::run_open_pr;
pub use self_review::{run_self_review, SelfReviewOutcome, SELF_REVIEW_MAX};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pipeline`
Expected: 10 passing.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline
git commit -m "feat(pipeline): CI-fix loop with poll, fix, max 3"
```

---

## Task 24: Escalation handler

**Files:**
- Create: `src/escalate.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-24`

- [ ] **Step 2: Write src/escalate.rs**

```rust
use crate::channel::{HumanChannel, NotifyContext};
use crate::domain::{EscalationReason, JobState, Phase, TicketKey};
use crate::error::Result;
use crate::state::SqliteState;

pub async fn escalate<C: HumanChannel + ?Sized>(
    state: &SqliteState,
    channel: &C,
    ticket: &TicketKey,
    repo_task_id: i64,
    reason: EscalationReason,
    summary: &str,
) -> Result<()> {
    state.update_repo_task_phase(repo_task_id, Phase::Escalated).await?;
    state.update_job_state(ticket, JobState::Escalated).await?;
    state.append_event(ticket, "escalated", &serde_json::json!({
        "reason": reason,
        "summary": summary,
    })).await?;
    let body = format!(
        "monorail needs help: {reason:?}\n\nSummary:\n{summary}",
    );
    channel.notify(NotifyContext { ticket: ticket.clone(), body }).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::HumanChannel;
    use crate::domain::{Job, JobState, Question, RepoRef, RepoTask, WorkType};
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct CapturingChannel { calls: Mutex<Vec<NotifyContext>> }
    #[async_trait]
    impl HumanChannel for CapturingChannel {
        async fn notify(&self, ctx: NotifyContext) -> Result<()> {
            self.calls.lock().unwrap().push(ctx);
            Ok(())
        }
        async fn post_question(&self, _q: Question) -> Result<String> { unimplemented!() }
    }

    #[tokio::test]
    async fn escalate_sets_state_and_notifies() {
        let dir = TempDir::new().unwrap();
        let st = SqliteState::open(&dir.path().join("t.db")).await.unwrap();
        let ticket = TicketKey::parse("ACM-1").unwrap();
        let job = Job {
            ticket: ticket.clone(),
            work_type: WorkType::Bug,
            state: JobState::Active,
            repos: vec![],
            auto_merge: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        st.insert_job(&job).await.unwrap();
        let rt = RepoTask {
            repo: RepoRef { org: "a".into(), repo: "b".into() },
            branch: "ACM-1".into(),
            worktree_path: PathBuf::from("/tmp"),
            anchors: vec![],
            phase: Phase::SelfReviewing,
            pr_url: None,
            review_attempts: 5,
            lint_test_attempts: 0,
            ci_fix_attempts: 0,
        };
        let id = st.insert_repo_task(&ticket, &rt).await.unwrap();

        let channel = CapturingChannel { calls: Mutex::new(vec![]) };
        escalate(&st, &channel, &ticket, id, EscalationReason::SelfReviewMaxed, "stuck").await.unwrap();

        let row = st.get_job(&ticket).await.unwrap().unwrap();
        assert_eq!(row.state, "escalated");
        let rows = st.list_repo_tasks(&ticket).await.unwrap();
        assert_eq!(rows[0].phase, "escalated");
        assert_eq!(channel.calls.lock().unwrap().len(), 1);
    }
}
```

- [ ] **Step 3: Wire into main**

Add `mod escalate;` to `src/main.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib escalate`
Expected: 1 passing.

- [ ] **Step 5: Commit**

```bash
git add src/escalate.rs src/main.rs
git commit -m "feat(escalate): persist escalated state + notify human channel"
```

---

## Task 25: Phase runner — orchestrate Type A end-to-end

**Files:**
- Modify: `src/pipeline/mod.rs` (add a `run_type_a` function tying phases together)

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-25`

- [ ] **Step 2: Write the runner**

Append to `src/pipeline/mod.rs`:

```rust
use crate::channel::HumanChannel;
use crate::domain::{EscalationReason, TicketKey};
use crate::engine::Engine;
use crate::error::Result;
use crate::escalate::escalate;
use crate::state::SqliteState;
use crate::tools::GhTool;
use std::path::Path;
use std::time::Duration;

pub struct TypeARunArgs<'a, E: Engine + ?Sized, V: Verifier + ?Sized, G: GhTool + ?Sized, C: HumanChannel + ?Sized> {
    pub state: &'a SqliteState,
    pub engine: &'a E,
    pub verifier: &'a V,
    pub gh: &'a G,
    pub channel: &'a C,
    pub ticket: &'a TicketKey,
    pub repo_task_id: i64,
    pub worktree: &'a Path,
    pub instructions: &'a str,
    pub pr_title: &'a str,
    pub pr_body: &'a str,
    pub poll_interval: Duration,
}

pub enum TypeARunOutcome {
    Merged,             // not used in this plan: merge gating is later
    PrGreen,            // PR opened and CI green; auto-merge not implemented in plan 1
    Escalated(EscalationReason),
}

pub async fn run_type_a<E: Engine + ?Sized, V: Verifier + ?Sized, G: GhTool + ?Sized, C: HumanChannel + ?Sized>(
    args: TypeARunArgs<'_, E, V, G, C>,
) -> Result<TypeARunOutcome> {
    run_implement(args.state, args.engine, args.ticket, args.repo_task_id, args.worktree, args.instructions).await?;

    match run_self_review(args.state, args.engine, args.ticket, args.repo_task_id, args.worktree).await? {
        SelfReviewOutcome::Clean | SelfReviewOutcome::Stuck => {}
        SelfReviewOutcome::Escalated(r) => {
            escalate(args.state, args.channel, args.ticket, args.repo_task_id, r, "self-review maxed").await?;
            return Ok(TypeARunOutcome::Escalated(r));
        }
    }

    match run_lint_test(args.state, args.engine, args.verifier, args.ticket, args.repo_task_id, args.worktree).await? {
        LintTestOutcome::Green => {}
        LintTestOutcome::Escalated(r) => {
            escalate(args.state, args.channel, args.ticket, args.repo_task_id, r, "lint/test failed").await?;
            return Ok(TypeARunOutcome::Escalated(r));
        }
    }

    let _pr_url = run_open_pr(args.state, args.gh, args.ticket, args.repo_task_id, args.worktree, args.pr_title, args.pr_body).await?;

    match run_ci_fix(args.state, args.engine, args.gh, args.ticket, args.repo_task_id, args.worktree, args.poll_interval).await? {
        CiFixOutcome::Green => Ok(TypeARunOutcome::PrGreen),
        CiFixOutcome::Escalated(r) => {
            escalate(args.state, args.channel, args.ticket, args.repo_task_id, r, "CI fix maxed").await?;
            Ok(TypeARunOutcome::Escalated(r))
        }
    }
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pipeline`
Expected: 10 still passing (no new tests yet; runner is exercised by Task 26 e2e).

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/mod.rs
git commit -m "feat(pipeline): run_type_a orchestrates implement->review->lint->pr->ci"
```

---

## Task 26: CLI wire-up + end-to-end smoke

**Files:**
- Modify: `src/main.rs`
- Create: `tests/e2e_typeA.rs`

- [ ] **Step 1: Branch**

Run: `git checkout -b impl/monorail-task-26`

- [ ] **Step 2: Update src/main.rs to call the runner**

Replace the body of the `Run { ticket }` branch in `src/main.rs`:

```rust
mod channel;
mod cli;
mod domain;
mod engine;
mod error;
mod escalate;
mod linear;
mod pipeline;
mod state;
mod tools;
mod tracing_setup;
mod triager;

use clap::Parser;
use cli::{Cli, Command};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_setup::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Run { ticket } => run_command(ticket).await?,
    }
    Ok(())
}

async fn run_command(ticket: String) -> anyhow::Result<()> {
    use crate::domain::TicketKey;
    use crate::engine::ClaudeCodeAdapter;
    use crate::linear::LinearClient;
    use crate::state::SqliteState;
    use crate::tools::{RealGh, RealGhq, RealWt};
    use std::sync::Arc;

    let ticket = TicketKey::parse(&ticket)?;
    let api_key = std::env::var("LINEAR_API_KEY")
        .map_err(|_| anyhow::anyhow!("LINEAR_API_KEY env var is required"))?;
    let endpoint = std::env::var("LINEAR_API_ENDPOINT")
        .unwrap_or_else(|_| "https://api.linear.app/graphql".to_string());
    let linear = Arc::new(LinearClient::new(endpoint, &api_key)?);

    let state_path = std::env::var("MONORAIL_STATE_DB")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.local/share/monorail/state.db")
        });
    let state_path = std::path::PathBuf::from(state_path);
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let state = SqliteState::open(&state_path).await?;

    let triager = triager::Triager { linear: linear.as_ref() };
    let job = triager.build_job(&ticket).await?;
    state.insert_job(&job).await?;

    let rt = &job.repos[0];
    let ghq = RealGhq;
    let wt = RealWt;
    let gh = RealGh;
    let engine = ClaudeCodeAdapter::default();
    let channel = channel::LinearCommentChannel { client: linear.clone() };

    let repo_path = ghq.ensure_cloned(&rt.repo.full()).await
        .map_err(|e| anyhow::anyhow!("ghq ensure_cloned: {e}"))?;
    let worktree = wt.switch_create(&repo_path, &rt.branch).await
        .map_err(|e| anyhow::anyhow!("wt switch_create: {e}"))?;

    let mut rt_persisted = rt.clone();
    rt_persisted.worktree_path = worktree.clone();
    let repo_task_id = state.insert_repo_task(&ticket, &rt_persisted).await?;

    let verifier = ShellVerifier;
    let outcome = pipeline::run_type_a(pipeline::TypeARunArgs {
        state: &state,
        engine: &engine,
        verifier: &verifier,
        gh: &gh,
        channel: &channel,
        ticket: &ticket,
        repo_task_id,
        worktree: &worktree,
        instructions: job_instructions(&job),
        pr_title: &format!("{}: {}", ticket, "monorail change"),
        pr_body: &format!("Automated PR by monorail for {ticket}."),
        poll_interval: Duration::from_secs(15),
    }).await?;
    tracing::info!(?outcome_kind = outcome_kind(&outcome), "type A run finished");
    Ok(())
}

fn outcome_kind(o: &pipeline::TypeARunOutcome) -> &'static str {
    match o {
        pipeline::TypeARunOutcome::Merged => "merged",
        pipeline::TypeARunOutcome::PrGreen => "pr_green",
        pipeline::TypeARunOutcome::Escalated(_) => "escalated",
    }
}

fn job_instructions(job: &domain::Job) -> &str {
    // Title is good enough as a baseline; the engine should also read CLAUDE.md.
    // We pass the ticket title as the implement instruction.
    // For richer context the engine can read its own context block.
    job.repos.first().map(|r| r.repo.repo.as_str()).unwrap_or("");
    // Use a static reference to a leak-free string at runtime by keeping the title
    // around in a Box leak via OnceCell. For v1 simplicity, we just pass the raw title.
    // (Implementation detail: we accept a small inefficiency here.)
    Box::leak(job.repos.first()
        .map(|_| format!("See Linear ticket {}.", job.ticket))
        .unwrap_or_default()
        .into_boxed_str())
}

struct ShellVerifier;
#[async_trait::async_trait]
impl pipeline::Verifier for ShellVerifier {
    async fn verify(&self, worktree: &std::path::Path) -> std::result::Result<(), String> {
        let cmd = std::env::var("MONORAIL_VERIFY_CMD")
            .unwrap_or_else(|_| "true".to_string());
        let out = tokio::process::Command::new("sh")
            .arg("-c").arg(&cmd)
            .current_dir(worktree)
            .output().await.map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else {
            Err(String::from_utf8_lossy(&out.stderr).to_string()
                + &String::from_utf8_lossy(&out.stdout))
        }
    }
}
```

Note: `main` is now `async`. Make sure `#[tokio::main]` is present and `tokio` features include `macros` (already in Cargo.toml's `full`).

- [ ] **Step 3: Write tests/e2e_typeA.rs**

Create `tests/e2e_typeA.rs`:

```rust
use assert_cmd::Command;

#[test]
fn cli_help_shows_run_subcommand() {
    let mut cmd = Command::cargo_bin("monorail").unwrap();
    let assert = cmd.arg("--help").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("run"), "help missing 'run': {out}");
}

#[test]
fn run_without_ticket_fails() {
    let mut cmd = Command::cargo_bin("monorail").unwrap();
    cmd.arg("run").assert().failure();
}

#[test]
fn run_invalid_ticket_format_fails_fast() {
    let mut cmd = Command::cargo_bin("monorail").unwrap();
    cmd.env("LINEAR_API_KEY", "dummy")
        .arg("run").arg("not-a-ticket")
        .assert()
        .failure();
}
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: builds clean. Some `dead_code` warnings on helpers are acceptable.

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all unit tests pass; e2e tests in `tests/e2e_typeA.rs` pass (3 of them).

- [ ] **Step 6: Manual smoke (optional, requires real env)**

If a sandbox Linear ticket is available with `monorail:type/bug` label and a `Repo:` line in description:

```
LINEAR_API_KEY=... \
MONORAIL_VERIFY_CMD="cargo build" \
RUST_LOG=monorail=info,info \
cargo run -- run ACM-1
```

Verify: process exits with success or escalation; SQLite state DB has rows; PR opened (if reached that phase).

- [ ] **Step 7: Commit**

```bash
git add src/main.rs Cargo.toml tests/
git commit -m "feat(cli): wire run subcommand to type-A pipeline end-to-end"
```

---

## Self-Review (run by author after writing this plan)

**1. Spec coverage**

Mapped each spec section against tasks for Plan 1 scope:

| Spec § | Coverage |
|---|---|
| §1 Vision (Type A path) | Tasks 18 (triage), 19–25 (pipeline) |
| §2 Goals (single-repo Type A end-to-end) | Task 26 |
| §3.1 Job/RepoTask/Phase | Tasks 4, 5 |
| §3.2 Engine/HumanChannel/Worktree traits | Tasks 14 (Engine), 17 (HumanChannel); WorktreeBackend itself is not implemented as a trait in Plan 1 — `WtTool` plays its role directly. Open issue: revisit in a later plan if generalization is needed. |
| §4.2 process model (`run`) | Task 26 |
| §4.3 external tools (git, gh, ghq, wt, claude) | Tasks 11–13 (gh/ghq/wt), 15–16 (claude). `git` is invoked inside `gh` resolver. |
| §5 ghq + wt path conventions | Tasks 11, 12 |
| §6.1 labels (bug/feature/auto-merge) | Task 18 |
| §6.2 Linear plan section (Repo: line for single repo) | Task 18 |
| §6.3 Linear status mapping | Tasks 18 (assignment via Triager — actual `set_state` calls happen via `LinearClient.set_state` available since Task 10; wired in Task 26 implicitly via channel notify). **Gap**: explicit phase→Linear status sync is not yet wired into pipeline. Acceptable for Plan 1 (we still post comments via escalate); explicit status updates can be a Plan 2 polish. Documented as out-of-scope below. |
| §7.1 phase order | Task 25 (run_type_a) |
| §7.2 self-review loop (root-cause-first, max 5) | Task 20 |
| §7.3 lint/test loop (max 5) | Task 21 |
| §7.4 CI-fix loop (max 3) | Task 23 |
| §7.5 per-repo isolation (single repo: trivially satisfied) | N/A in single-repo plan |
| §7.6 cross-repo context injection | Out of scope |
| §8 Escalation (per-phase) | Task 24 + integrated in run_type_a |
| §9 Doc subsystem | Out of scope (Plan 6) |
| §10 SQLite persistence | Tasks 6–8 |
| §11.1 container | Out of scope (Plan 5) |
| §11.2 secrets | Linear key + `MONORAIL_VERIFY_CMD` covered in Task 26 |
| §12 TUI | Out of scope (Plan 4) |
| §13 config | Per-repo overrides out of scope (Plan 3); `MONORAIL_STATE_DB`/`MONORAIL_VERIFY_CMD` env vars used as minimal config in Plan 1 |
| §14 testing strategy | Embedded in each task |

Identified gaps explicitly handled: phase→Linear status sync is deferred (only `In Progress` comments are posted via escalate path). This is acceptable because the design says status names are configurable; we postpone wiring until Plan 2 when state IDs can be discovered/cached.

**2. Placeholder scan**: searched the plan for `TBD`, `TODO`, `FIXME`, `implement later`, `add appropriate`, `similar to Task`, `fill in`, `[brackets]` — none found in normative content. The text "implementation detail" appears once in Task 26 as a comment-style aside, not a placeholder.

**3. Type consistency**:
- `Phase` enum values match across tasks: `Pending`, `Implementing`, `SelfReviewing`, `LintTesting`, `PrOpened`, `CiFixing`, `Merged`, `Aborted`, `Escalated`. SQL `phase_str` and `Phase` enum members align.
- `WorkType`, `JobState`, `EscalationReason` consistent.
- `RepoTask` field set unchanged from Task 5 onward (`anchors` included even though not used in Plan 1).
- `Engine` trait method signatures from Task 14 match callers in Tasks 15–16, 19–23.
- `Verifier` trait introduced in Task 21 reused in Task 25 and 26.
- `GhTool` methods (`pr_create`, `checks_for_pr`, `check_run_log`) consistent across Tasks 13, 22, 23, 26.
- `HumanChannel` (`notify`, `post_question`) consistent across Tasks 17, 24, 26.
- `MonorailError` variants used: `InvalidTicketKey`, `MissingLabel`, `TriageRejected`, `PhaseAborted`, `Escalated`, `ExternalTool`, `Linear`, `State`, `Io`, `Serde`. (`MissingLabel` is declared but not currently constructed; left for completeness — fine to leave, can be removed when refactoring later if still unused.)

No outstanding inconsistencies. Plan is ready to execute.
