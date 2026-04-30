//! Structural contract tests for the `/monorail-ears` slash command.
//!
//! `/monorail-ears` is a prompt artifact (markdown), not executable code, so
//! these tests assert that the command file documents every acceptance
//! criterion it claims to satisfy. If the command markdown is rewritten in
//! a way that drops one of these contracts, the test fails — which is the
//! signal the verifier wants ("test_evidence per criterion").
//!
//! Each test below ties to one bullet from the ticket's
//! `## Acceptance Criteria`.

use std::fs;
use std::path::PathBuf;

fn command_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".claude/commands/monorail-ears.md")
}

fn read_command() -> String {
    let path = command_path();
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn assert_contains_all(haystack: &str, needles: &[&str], context: &str) {
    for n in needles {
        assert!(
            haystack.contains(n),
            "[{context}] command markdown is missing required phrase: {n:?}"
        );
    }
}

#[test]
fn command_file_exists_and_has_frontmatter() {
    let body = read_command();
    assert!(
        body.starts_with("---\n"),
        "command file must start with YAML frontmatter"
    );
    assert!(
        body.contains("description:"),
        "frontmatter must include a description field"
    );
}

#[test]
fn ticket_key_input_fetches_via_linear_mcp_and_proposes() {
    // Criterion: When the user invokes `/monorail-ears <ticket-key>`, the
    // command shall fetch the ticket via Linear MCP and propose EARS
    // bullets distilled from its body.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "Linear ticket key",
            "Linear MCP",
            "get_issue",
            "Propose",
        ],
        "ticket-key input proposes via Linear MCP",
    );
}

#[test]
fn file_path_input_emits_to_stdout_without_writing() {
    // Criterion: When the user invokes `/monorail-ears <file-path>`, the
    // command shall read the file and emit EARS bullets to stdout without
    // modifying any file.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "mode=file",
            "NEVER write",
            "stdout",
        ],
        "file-path input emits to stdout, never writes",
    );
}

#[test]
fn no_argument_picks_most_recent_spec_or_plan() {
    // Criterion: When the user invokes `/monorail-ears` with no argument,
    // the command shall auto-pick the most recent file under
    // `docs/superpowers/specs/` or `docs/superpowers/plans/` and treat it
    // as the input file.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "auto-file",
            "docs/superpowers/specs/",
            "docs/superpowers/plans/",
            "most recently modified",
        ],
        "no-argument auto-picks latest spec or plan",
    );
}

#[test]
fn existing_acceptance_criteria_triggers_merge_or_replace_prompt() {
    // Criterion: When the input source already contains a `## Acceptance
    // Criteria` section, the command shall display the existing bullets
    // and ask the user whether to merge or replace before writing.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "## Acceptance Criteria",
            "Existing Acceptance Criteria in source",
            "merge",
            "replace",
            "abort",
        ],
        "merge-vs-replace prompt for existing criteria",
    );
}

#[test]
fn ticket_mode_writes_back_only_after_approval() {
    // Criterion: When the user is given a ticket-key input and approves
    // the proposal, the command shall write the bullets back to the
    // Linear ticket body under a `## Acceptance Criteria` section via
    // Linear MCP.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "save_issue",
            "explicit approval",
            "Wrote acceptance criteria",
        ],
        "ticket-mode writeback gated on user approval",
    );
}

#[test]
fn linear_mcp_unavailable_aborts_for_ticket_input() {
    // Criterion: When Linear MCP is unavailable and the input is a
    // ticket-key, the command shall abort with a clear error and shall
    // not attempt any other write.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "Linear MCP is unavailable",
            "Do not",
            "fallback",
        ],
        "abort cleanly when Linear MCP is down",
    );
}

#[test]
fn prefers_ubiquitous_and_event_driven_ears_patterns() {
    // Criterion: The command shall preferentially use Ubiquitous
    // (`The X shall Y.`) and Event-driven (`When X, the X shall Y.`)
    // EARS patterns.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "Ubiquitous",
            "The X shall Y.",
            "Event-driven",
            "When <trigger>, the X shall Y.",
        ],
        "prefer Ubiquitous and Event-driven EARS",
    );
}

#[test]
fn never_writes_to_project_level_ears_docs_in_v1() {
    // Criterion: The command shall not write to project-level EARS docs
    // in v1.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "Never write to project-level EARS docs",
            "project-spec-sync",
            "read-only context",
        ],
        "no project-level EARS doc writes in v1",
    );
}

#[test]
fn project_ears_doc_hint_in_claude_md_or_agents_md_is_read() {
    // Criterion: When CLAUDE.md or AGENTS.md contains a hint pointing to
    // a project EARS doc, the command shall read that doc for context
    // before distilling.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "CLAUDE.md",
            "AGENTS.md",
            "EARS",
            "hint",
            "project_ears_doc",
        ],
        "read project EARS doc when hinted",
    );
}

#[test]
fn fallback_to_conventional_paths_when_no_hint() {
    // Criterion: When no hint is found, the command shall fall back to
    // conventional paths under `docs/superpowers/specs/` and
    // `docs/superpowers/plans/`.
    let body = read_command();
    assert_contains_all(
        &body,
        &[
            "no hint",
            "conventional locations",
            "docs/spec/EARS.md",
            "docs/superpowers/specs/",
            "docs/superpowers/plans/",
        ],
        "fall back to conventional paths",
    );
}
