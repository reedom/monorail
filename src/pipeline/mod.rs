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

use crate::channel::HumanChannel;
use crate::domain::{EscalationReason, TicketKey};
use crate::engine::Engine;
use crate::error::Result;
use crate::escalate::escalate;
use crate::linear::{LinearClient, LinearStateResolver, StateKind};
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
    pub linear: &'a LinearClient,
    pub ticket: &'a TicketKey,
    pub repo_task_id: i64,
    pub worktree: &'a Path,
    pub instructions: &'a str,
    pub pr_title: &'a str,
    pub pr_body: &'a str,
    pub poll_interval: Duration,
}

pub enum TypeARunOutcome {
    Merged,
    PrGreen,
    Escalated(EscalationReason),
}

pub async fn run_type_a<E: Engine + ?Sized, V: Verifier + ?Sized, G: GhTool + ?Sized, C: HumanChannel + ?Sized>(
    args: TypeARunArgs<'_, E, V, G, C>,
) -> Result<TypeARunOutcome> {
    let issue = args.linear.get_issue(args.ticket.as_str()).await?;
    let states = args.linear.list_issue_statuses(&issue.team_id).await?;
    let resolver = LinearStateResolver::from_states(states);

    set_linear_state(args.state, args.linear, args.ticket, &issue.id, &resolver, StateKind::Started).await?;

    run_implement(args.state, args.engine, args.ticket, args.repo_task_id, args.worktree, args.instructions).await?;

    match run_self_review(args.state, args.engine, args.ticket, args.repo_task_id, args.worktree).await? {
        SelfReviewOutcome::Clean => {}
        SelfReviewOutcome::Stuck => {}
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
        CiFixOutcome::Green => {
            set_linear_state(args.state, args.linear, args.ticket, &issue.id, &resolver, StateKind::Completed).await?;
            Ok(TypeARunOutcome::PrGreen)
        }
        CiFixOutcome::Escalated(r) => {
            escalate(args.state, args.channel, args.ticket, args.repo_task_id, r, "CI fix maxed").await?;
            Ok(TypeARunOutcome::Escalated(r))
        }
    }
}

async fn set_linear_state(
    state: &SqliteState,
    linear: &LinearClient,
    ticket: &TicketKey,
    issue_id: &str,
    resolver: &LinearStateResolver,
    kind: StateKind,
) -> Result<()> {
    match resolver.for_kind(kind) {
        Some(ws) => {
            linear.set_state(issue_id, &ws.id).await?;
            state.append_event(ticket, "linear_state_change", &serde_json::json!({
                "kind": kind.as_str(),
                "state_id": ws.id,
                "state_name": ws.name,
            })).await?;
        }
        None => {
            state.append_event(ticket, "linear_state_skip", &serde_json::json!({
                "kind": kind.as_str(),
                "reason": "no matching workflow state on team",
            })).await?;
        }
    }
    Ok(())
}
