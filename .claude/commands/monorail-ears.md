---
description: Distill prose specs, plans, or Linear ticket bodies into EARS-style acceptance criteria. Auto-detects input — `<TICKET-KEY>` fetches via Linear MCP and writes back on approval; `<file-path>` reads the file and emits to stdout (no write); no argument auto-picks the most recent file under `docs/superpowers/specs/` or `docs/superpowers/plans/`. Invoke as `/monorail-ears [TICKET|file|]`.
---

# monorail-ears

You are the EARS distiller. Your job is to turn prose (a spec, a plan, or a Linear ticket body) into EARS-style acceptance criteria that the rest of the monorail pipeline (`monorail-implement`, `monorail-verify-acceptance`) can consume as a hard input contract.

**Announce at start:** "Running monorail-ears for `<argument-or-auto>`."

## Inputs

A single optional argument resolved into one of three input modes (auto-detected by shape):

| Argument shape | Mode | Source |
| -- | -- | -- |
| Looks like a Linear ticket key (regex `^[A-Z]+-[0-9]+$`, e.g., `RDM-12`) | **ticket** | Fetch ticket body via Linear MCP. |
| Existing path on disk (file or `./relative` / `/absolute`) | **file** | Read the file directly. |
| Empty / no argument | **auto-file** | Pick the most recently modified file under `docs/superpowers/specs/` or `docs/superpowers/plans/`. Treat as the **file** case from then on. |

If the argument is non-empty but matches neither shape (not a ticket key and not an existing path), abort with:

```
ERROR: argument "<arg>" is neither a Linear ticket key nor an existing file path.
       Use a ticket key like RDM-12, a path like docs/superpowers/specs/foo.md,
       or omit the argument to auto-pick the latest spec/plan.
```

Do not proceed with any write or fetch in that case.

## Hard contract

1. **Never write to project-level EARS docs in v1.** Even if you discover one (see "Project EARS doc discovery" below), it is read-only context only. Project-level write is tracked as `project-spec-sync` in `docs/ROADMAP.md` and is explicitly out of scope for this command.
2. **Never write to local files.** When the input is a file path (including the `auto-file` case), emit the distilled bullets to stdout only. The caller decides where to put them.
3. **Never write to Linear without explicit user approval.** When the input is a ticket key, you MUST propose the bullets and wait for the user's approval before any `save_issue` MCP call.
4. **If Linear MCP is unavailable and the input is a ticket key, abort.** Print a clear error and do not attempt any other write or fallback fetch.
5. **Distillation is human-confirmed.** No silent rewrites. Always show the proposal; on ticket input, ask for explicit approval.

## Workflow

### Step 1 — Resolve the argument

```
arg = $1 (the slash-command argument; may be empty)

if arg matches ^[A-Z]+-[0-9]+$:
    mode = "ticket"
    ticket_key = arg
elif arg is non-empty:
    if file exists at arg:
        mode = "file"
        file_path = arg
    else:
        abort with the "neither a ticket key nor an existing path" error above
else:
    mode = "auto-file"
    Find the most recently modified *.md under docs/superpowers/specs/ and
    docs/superpowers/plans/ (combined). Use Bash with `find` to stay portable
    across bash and zsh — relying on `ls *.md` would abort under zsh's default
    `nomatch` behavior whenever one of the directories is empty or missing:
        find docs/superpowers/specs docs/superpowers/plans \
             -maxdepth 1 -type f -name '*.md' 2>/dev/null \
          | xargs ls -t 2>/dev/null | head -1
    If no candidate exists, abort with:
        ERROR: no files found under docs/superpowers/specs/ or docs/superpowers/plans/.
               Pass an explicit path or ticket key.
    Otherwise: file_path = the picked file. Mode is now effectively "file".
```

Announce the resolved mode and source (e.g., "Mode: file. Source: `docs/superpowers/specs/2026-04-30-foo.md`.").

### Step 2 — Fetch the source body

**For `mode=ticket`:**

1. Confirm Linear MCP is available in this session by attempting a lightweight call (e.g., `list_teams` or `get_user(me)`). If the call fails or the MCP server is not reachable:

   ```
   ERROR: Linear MCP is unavailable. Cannot fetch ticket <ticket_key>.
          Run `/monorail-ears` again once Linear MCP is configured, or pass a
          file path instead.
   ```

   Abort. **Do not** attempt to read any other source as a fallback — silent fallback would write the wrong place.

2. Fetch the ticket via the Linear MCP `get_issue` tool. Pass the ticket key as the identifier. Capture the ticket body (markdown) into `source_body` and remember the ticket's internal UUID from the `id` field of the response (NOT the human-readable `identifier` field like `RDM-12`) — the `save_issue` call later needs this UUID `id`.

3. If fetch fails (auth, network, ticket not found), abort with:

   ```
   ERROR: failed to fetch <ticket_key>: <error message>
   ```

**For `mode=file` (including `auto-file`-resolved file):**

1. Read the file with the Read tool. Capture the contents into `source_body`.

### Step 3 — Project EARS doc discovery (read-only context)

This step finds a project-level EARS doc (if any) so the distillation can use consistent vocabulary. **Read-only.** Never write to whatever you find.

```
1. Read CLAUDE.md and AGENTS.md from the worktree root if they exist.
   Grep them for hints pointing at a project EARS doc. Look for:
     - lines mentioning "EARS" combined with a file path (e.g.,
       "Project EARS spec: docs/spec/EARS.md")
     - explicit "Acceptance criteria spec:" / "Project criteria:" markers
     - any other unambiguous path-shaped pointer
   If a hint is found, treat that path as project_ears_doc.

2. If no hint, fall back to conventional locations:
     a. docs/spec/EARS.md
     b. docs/superpowers/specs/  (treat as a directory of canonical specs)
     c. docs/superpowers/plans/  (lower priority — plans, not specs)
   For (a), read the file if it exists. For (b)/(c), DO NOT load every file —
   only enumerate filenames so you can scan titles for relevance. Load the
   contents of any file whose title or first heading appears semantically
   close to the source you're distilling.

3. If found, prepend the project EARS doc's bullets (just the EARS-style
   lines) to your working context as "project-level reference criteria".
   Use this to:
     - Match terminology (component names, action verbs).
     - Avoid contradictions.
     - Skip stating things that are already universally true at the project
       level (you're distilling deltas, not restating the whole spec).
   Mention in the proposal output that the doc was consulted (e.g.,
   "Consulted project EARS doc: docs/spec/EARS.md").

4. If nothing is found, proceed without project context. State this in
   the proposal output: "No project EARS doc found — distilled standalone."
```

### Step 4 — Detect existing `## Acceptance Criteria` in the source

Scan `source_body` for an existing `## Acceptance Criteria` (case-insensitive, possibly suffixed with " (EARS)" or similar) section. If found:

1. Extract the existing bullets verbatim.
2. Display them to the user in a fenced block labeled "Existing Acceptance Criteria in source".
3. Ask explicitly:

   ```
   The source already has an Acceptance Criteria section with N bullets.
   How should the distilled output relate to them?
     [m] merge   — keep existing bullets; append distilled additions
     [r] replace — discard existing bullets; use only the distilled set
     [a] abort   — stop without proposing or writing anything
   ```

4. Wait for the user's explicit choice. Do not assume a default. Do not perform any write before the choice is given. If `[a]`, exit cleanly with a one-line message; no fetch, no write.

### Step 5 — Distill prose → EARS

Convert `source_body` (and, if relevant, the merged-or-replaced existing bullets) into EARS-style bullets following these rules:

1. **Pattern preference (in order):**
   - **Ubiquitous:** `The X shall Y.` — for invariants always true.
   - **Event-driven:** `When <trigger>, the X shall Y.` — for behaviors triggered by an event or input.
   - Avoid State-driven (`While …`), Optional (`Where …`), and Unwanted (`If …, then the X shall Y.`) patterns in v1 unless the source clearly requires them. If you must use one, flag it in the proposal so the user can review.

2. **Verbatim-quote when possible.** If the source already contains a sentence in EARS shape, lift it verbatim rather than paraphrasing.

3. **Atomic bullets.** One observable behavior per bullet. If the source describes a compound behavior ("does X and also Y"), split it.

4. **Implementation-agnostic where possible.** Acceptance criteria describe behavior, not how to code it. ("The README shall list installation steps." not "The README shall be parsed by section-extractor.rs.")

5. **Use project terminology.** If the project EARS doc names a component `X`, use that exact name.

6. **Cap length per bullet.** Aim for one sentence; never more than two. If you can't fit it, the bullet is non-atomic — split.

7. **Skip obvious meta.** Don't restate things like "the code shall compile" or "tests shall pass" unless the source explicitly calls them out as criteria.

### Step 6 — Propose

Output the distilled bullets to stdout in this exact shape:

```
## Acceptance Criteria

- <bullet 1>
- <bullet 2>
- ...
```

Followed by a brief notes block explaining anything non-obvious — e.g., bullets that used a non-preferred EARS pattern, things from the source you deliberately omitted (and why), and the merge-vs-replace choice if applicable.

### Step 7 — Write back (ticket mode only)

**Only when `mode=ticket`:**

1. Ask the user for explicit approval:

   ```
   Write these bullets to <ticket_key> under `## Acceptance Criteria`?
     [y] yes — call Linear save_issue
     [n] no  — print the proposal and exit without writing
     [e] edit — let me edit the bullets first (return them to me, I'll resubmit)
   ```

2. On `[y]`:
   - Compose the new ticket body: take the original `source_body`, locate any existing `## Acceptance Criteria` section using the **same fuzzy-match rule as Step 4** (case-insensitive, optional ` (EARS)` or similar suffix), and replace-or-insert per the user's earlier merge/replace choice. If no such section exists in the original under that fuzzy match, append a new one at the end (after one blank line). Do not append a second section when a fuzzy-matching heading is already present — replacing or merging must target the existing heading verbatim.
   - Call Linear MCP `save_issue` with the ticket's internal UUID `id` (captured in Step 2, not the `identifier` like `RDM-12`) and the updated body.
   - On success: print `Wrote acceptance criteria to <ticket_key>.` and exit.
   - On failure: print the error and exit. Do NOT retry silently. Do NOT touch any other source.

3. On `[n]`: print "Skipping write." and exit. The proposal is already on stdout for the user to copy manually.

4. On `[e]`: surface the bullets back to the user as editable text, accept their revision, then return to the approval prompt with the new bullets. The user may iterate `[e]` as many times as they want; `[n]` always remains available as the exit-without-writing escape hatch.

**For `mode=file` (including auto-file): NEVER write.** The proposal printed in Step 6 is the only output. The user copies the bullets wherever they want them.

## Output contract

- `mode=ticket` + approved write: `Wrote acceptance criteria to <ticket_key>.`
- `mode=ticket` + declined: the proposal block, then `Skipping write.`
- `mode=file` / `auto-file`: the proposal block on stdout. Nothing else is touched.
- All abort paths: a single `ERROR: ...` line and exit. No partial writes.

## Notes on isolation

This command runs in the user's current cwd (typically the repo root or a worktree). For project-context discovery (Step 3), it only reads `CLAUDE.md`, `AGENTS.md`, `docs/spec/EARS.md`, and files under `docs/superpowers/specs/` or `docs/superpowers/plans/`. In `mode=file`, it additionally reads whatever path the user supplied as the argument — including absolute paths or paths outside the worktree — since the user explicitly chose that source. It writes nothing to disk. The only mutation it ever performs is a Linear `save_issue` call in `mode=ticket` after explicit user approval.

## Acceptance Criteria

- When the user invokes `/monorail-ears <ticket-key>`, the command shall fetch the ticket via Linear MCP and propose EARS bullets distilled from its body.
- When the user invokes `/monorail-ears <file-path>`, the command shall read the file and emit EARS bullets to stdout without modifying any file.
- When the user invokes `/monorail-ears` with no argument, the command shall auto-pick the most recent file under `docs/superpowers/specs/` or `docs/superpowers/plans/` and treat it as the input file.
- When the input source already contains a `## Acceptance Criteria` section, the command shall display the existing bullets and ask the user whether to merge or replace before writing.
- When the user is given a ticket-key input and approves the proposal, the command shall write the bullets back to the Linear ticket body under a `## Acceptance Criteria` section via Linear MCP.
- When Linear MCP is unavailable and the input is a ticket-key, the command shall abort with a clear error and shall not attempt any other write.
- The command shall preferentially use Ubiquitous (`The X shall Y.`) and Event-driven (`When X, the X shall Y.`) EARS patterns.
- The command shall not write to project-level EARS docs in v1.
- When `CLAUDE.md` or `AGENTS.md` contains a hint pointing to a project EARS doc, the command shall read that doc for context before distilling.
- When no hint is found, the command shall fall back to conventional paths under `docs/superpowers/specs/` and `docs/superpowers/plans/`.
