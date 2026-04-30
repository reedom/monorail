# monorail

A small Rust daemon that drives Linear tickets through an automated implementation pipeline. `monorail` reads a ticket, sets up an isolated git worktree, hands work off to a coding agent (Claude Code), and tracks the run in a local SQLite state store.

The current shipped scope is **Type A single-repo end-to-end** runs: triage a ticket, implement the change, self-review, run lint/tests, open a PR, and react to CI feedback. Larger work — multi-repo DAGs, Type B human-planning loops, a TUI, auto-merge — is tracked in [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Overview

monorail is built around a few small pieces:

- **CLI** (`src/cli.rs`) — `clap`-based entry point with a single `run TICKET` subcommand.
- **Triager** (`src/triager.rs`) — fetches the Linear ticket and builds a `Job` describing what to do.
- **State** (`src/state/`) — SQLite-backed persistence using `sqlx`. Migrations live in [`migrations/`](migrations).
- **Tools** (`src/tools/`) — thin wrappers over `ghq`, `git worktree`, and `gh` so the daemon can clone, branch, and open PRs.
- **Engine** (`src/engine/`) — adapter that shells out to the Claude Code CLI to run a step.
- **Pipeline** (`src/pipeline/`) — orchestrates the Type A flow (implement → self-review → lint/test → open PR → CI fix loop).
- **Channel** (`src/channel/`) — posts updates back to the Linear ticket as comments.

Under the **small-daemon / skill-first** pivot (see [`docs/ROADMAP.md`](docs/ROADMAP.md)), most loop logic is migrating from the Rust pipeline into Claude Code orchestrator skills under `.claude/`. The daemon's long-term job is triage, state, worktree setup, and gating.

## Installation

### Prerequisites

- **Rust** with edition 2024 support (a recent stable toolchain via [`rustup`](https://rustup.rs)).
- **SQLite** (only the C library; `sqlx` uses the `sqlite` feature in pure-Rust mode where possible).
- **External CLIs** on `PATH`:
  - [`ghq`](https://github.com/x-motemen/ghq) — repo cloning and path resolution.
  - [`git`](https://git-scm.com/) with `worktree` support.
  - [`gh`](https://cli.github.com/) — GitHub PR creation and CI status.
  - [`claude`](https://docs.anthropic.com/en/docs/claude-code) — the Claude Code CLI used by the engine adapter.

### Build from source

```sh
git clone https://github.com/reedom/monorail.git
cd monorail
cargo build --release
```

The resulting binary lives at `target/release/monorail`.

### Configuration

monorail reads configuration from environment variables:

| Variable | Purpose | Default |
|---|---|---|
| `LINEAR_API_KEY` | Personal API token used to call the Linear GraphQL API. **Required.** | — |
| `LINEAR_API_ENDPOINT` | Linear GraphQL endpoint. | `https://api.linear.app/graphql` |
| `MONORAIL_STATE_DB` | Path to the SQLite state database. | `$HOME/.local/share/monorail/state.db` |
| `MONORAIL_VERIFY_CMD` | Shell command run inside the worktree to verify a change (lint/tests/build). | `true` |

A local `.envrc` (used with [`direnv`](https://direnv.net/)) is the easiest way to keep these out of your shell history.

## Usage

Run monorail against a Linear ticket key:

```sh
monorail run RDM-5
```

What happens:

1. The ticket key is parsed and validated (`ACM-123`-style format).
2. The Linear ticket is fetched and a `Job` (with one `RepoTask`) is materialised.
3. The repo named in the ticket body is cloned via `ghq` (if missing) and a fresh worktree is created on a per-ticket branch.
4. The Type A pipeline drives the run: it asks the engine to implement the change, self-reviews, runs `MONORAIL_VERIFY_CMD`, opens a PR via `gh`, and waits on CI.
5. Progress is mirrored back to the Linear ticket as comments via the Linear comment channel.

The final outcome is logged as `merged`, `pr_green`, or `escalated`. Run state is persisted in SQLite so a follow-up invocation can resume where the previous one left off.

### Development

```sh
# Run the full test suite (unit + integration + e2e against fakes).
cargo test

# Apply database migrations against a scratch SQLite file.
sqlx migrate run --database-url sqlite://./scratch.db
```

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the canonical list of in-flight and deferred work, and `docs/superpowers/specs/` for design specs.

## License

Licensed under the terms in [`LICENSE`](LICENSE).
