# monorail — AI agent guide

This file is the **thin constitution** for AI agents working in this repo.
Keep it short. Detail lives in `docs/`. If guidance grows, push it down into `docs/` and link from here.

## Read order (start here)

1. `docs/index.md` — entry index (links every other doc)
2. `docs/ai/entry.md` — what AI MUST/MUST NOT do in this repo
3. `docs/glossary.md` — Job, RepoTask, EARS, MONORAIL_RESULT, ticket states
4. `docs/ai/repo-map.md` — important files + symbols (don't grep blindly)
5. Then drill into the area you're touching:
   - architecture → `docs/architecture/`
   - contract change → `docs/contracts/`
   - past decision → `docs/decisions/adr/`
   - current/queued work → `docs/ROADMAP.md`, `docs/superpowers/{specs,plans}/`

## Documentation layout (compound docs)

The `docs/` tree is the **single source of truth** for cross-cutting knowledge. AI agents and humans use the same entry; AI tools (Claude Code, Copilot, Cursor) bridge to `docs/ai/entry.md`, never to ad-hoc files.

```
docs/
  index.md                     # single entry; links everything else
  glossary.md                  # domain terms — one place, no duplicates
  architecture/
    overview.md                # boundaries, non-goals, quality goals
    c4-context.md              # daemon ↔ Linear ↔ Claude Code commands
    c4-container.md            # daemon / commands / agents containers
    c4-component.md            # components inside each container
    runtime.md                 # run-bug, run-feature scenarios
    deployment.md              # install / config / env
  modules/
    <name>.md                  # one file per crate/module:
                               #   responsibilities | inputs/outputs |
                               #   allowed deps | forbidden deps | entry files
  contracts/
    monorail-result.md         # JSON contract: skill → daemon
    linear-states.md           # ticket-state mapping (skill vs. daemon ownership)
    ears.md                    # EARS acceptance-criteria contract
  decisions/adr/
    NNNN-slug.md               # one ADR per significant decision
  ai/
    entry.md                   # thin AI constitution: MUST/MUST NOT
    repo-map.md                # curated map of important files + symbols
    change-playbook.md         # "for change kind X, touch these files"
  superpowers/
    specs/                     # design specs (current + historical)
    plans/                     # implementation plans
  ROADMAP.md                   # canonical status + deferred-work list
```

### What goes where (knowledge placement rules)

| Knowledge type | Home | Notes |
|---|---|---|
| Module boundary / responsibility | `docs/modules/<name>.md` | Including forbidden deps. AI uses this to pick the right layer. |
| DSL / config **structure** | machine-readable schema (when added) | JSON Schema or equivalent — code is generated/validated from this. |
| DSL / config **semantics** (order, precedence, conflict, defaults) | `docs/contracts/<name>.md` | AI cannot reliably infer semantics from code; spell it out. |
| Cross-process contract (skill ↔ daemon, daemon ↔ Linear) | `docs/contracts/` | One file per contract. |
| Why a structure/constraint exists | `docs/decisions/adr/NNNN-*.md` | Prevents AI from "simplifying" away load-bearing constraints. |
| In-flight design / proposal | `docs/superpowers/specs/` | Promote conclusions into ADR once accepted. |
| Implementation plan for accepted design | `docs/superpowers/plans/` | Linked from ROADMAP. |
| Current status, deferred work | `docs/ROADMAP.md` | Canonical; ADRs reference roadmap IDs, not plan numbers. |
| AI-only operating rules | `docs/ai/entry.md` | Stays short. Long → split into `docs/ai/*.md`. |

### Document conventions

- **One file = one decision / one contract / one responsibility.** No mixed-purpose files.
- **Each doc starts with the same sections:** `Purpose` · `Definitions` · `Responsibilities` · `Non-goals` · `References`. AI looks in fixed places.
- **Normative vs. informative are separated.** Rules use MUST / MUST NOT / SHOULD. Explanations live in their own paragraph.
- **For DSL/config, write order, precedence, conflict resolution, and defaults explicitly.** Examples come in three flavors: minimal, typical, boundary.
- **Talk in file paths.** Every responsibility doc names its primary entry files.
- **Short over long.** Above ~400 lines, split. CLAUDE.md itself stays under ~150 lines.

### Tool bridges

AI tools should bridge to `docs/ai/entry.md`, not duplicate it:

- **Claude Code** — this `CLAUDE.md` is the bridge. Use `@docs/...` imports for detail.
- **Copilot** — `.github/copilot-instructions.md` should be a short summary of `docs/ai/entry.md`.
- **Cursor** — `.cursor/rules/` may auto-attach scoped slices (e.g. `contracts/*` on edits there).

### Keeping docs honest

- Treat docs as code: PR review, lint, CI checks.
- Touching `docs/contracts/` or schemas → update consumers in the same PR; add a conformance test if missing.
- Architectural pivots → write/update an ADR in the same PR (don't bury the reason in a commit message).
- Stale doc beats missing doc only if it's labeled stale. Otherwise delete it.

## Project-specific rules

- **Roadmap IDs are stable; plan numbers are not.** Specs and ADRs reference items by ID (e.g. `multi-repo`, `tui`), per `docs/ROADMAP.md`.
- **Linear status ownership is split:** orchestrator commands + `monorail-open-pr` write `In Progress` / `In Review`; the daemon owns merge/close → `Done` / `Canceled`. Both soft-fail on Linear MCP errors. (See ADR once written; meanwhile `docs/ROADMAP.md` 2026-04-30 row.)
- **Tickets MUST carry an `## Acceptance Criteria` (EARS) section** for triage. Daemon gates Linear `Done` on `MONORAIL_RESULT.verification.all_satisfied=true`.
- **No emojis in code, scripts, or docs.**

## When this file should change

Edit CLAUDE.md only when:
- A read-order entry moves or is renamed.
- A top-level rule is added or removed.
- The `docs/` layout itself changes.

Everything else — architecture, contracts, decisions, plans — goes in `docs/`. CLAUDE.md links to it; it does not restate it.
