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
    use crate::tools::{GhqTool, RealGh, RealGhq, RealWt, WtTool};
    use std::sync::Arc;

    let ticket = TicketKey::parse(&ticket)?;
    let api_key = std::env::var("LINEAR_API_KEY")
        .map_err(|_| anyhow::anyhow!("LINEAR_API_KEY env var is required"))?;
    let endpoint = std::env::var("LINEAR_API_ENDPOINT")
        .unwrap_or_else(|_| "https://api.linear.app/graphql".to_string());
    let linear = Arc::new(LinearClient::new(endpoint, &api_key)?);

    let state_path = std::env::var("MONORAIL_STATE_DB").unwrap_or_else(|_| {
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

    let repo_path = ghq.ensure_cloned(&rt.repo.full()).await?;
    let worktree = wt.switch_create(&repo_path, &rt.branch).await?;

    let mut rt_persisted = rt.clone();
    rt_persisted.worktree_path = worktree.clone();
    let repo_task_id = state.insert_repo_task(&ticket, &rt_persisted).await?;

    let verifier = ShellVerifier;
    let instructions = format!("See Linear ticket {}.", ticket);
    let pr_title = format!("{}: monorail change", ticket);
    let pr_body = format!("Automated PR by monorail for {}.", ticket);

    let outcome = pipeline::run_type_a(pipeline::TypeARunArgs {
        state: &state,
        engine: &engine,
        verifier: &verifier,
        gh: &gh,
        channel: &channel,
        ticket: &ticket,
        repo_task_id,
        worktree: &worktree,
        instructions: &instructions,
        pr_title: &pr_title,
        pr_body: &pr_body,
        poll_interval: Duration::from_secs(15),
    })
    .await?;

    let kind = outcome_kind(&outcome);
    tracing::info!(outcome = %kind, "type A run finished");
    Ok(())
}

fn outcome_kind(o: &pipeline::TypeARunOutcome) -> &'static str {
    match o {
        pipeline::TypeARunOutcome::Merged => "merged",
        pipeline::TypeARunOutcome::PrGreen => "pr_green",
        pipeline::TypeARunOutcome::Escalated(_) => "escalated",
    }
}

struct ShellVerifier;

#[async_trait::async_trait]
impl pipeline::Verifier for ShellVerifier {
    async fn verify(&self, worktree: &std::path::Path) -> std::result::Result<(), String> {
        let cmd = std::env::var("MONORAIL_VERIFY_CMD").unwrap_or_else(|_| "true".to_string());
        let out = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(worktree)
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).to_string()
                + &String::from_utf8_lossy(&out.stdout))
        }
    }
}
