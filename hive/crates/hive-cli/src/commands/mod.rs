mod init;
mod config;
mod status;
mod exec;
mod merge;
mod rfc;
mod approve;
mod check;
mod report;
mod claim;
mod isolate;
mod launch;
mod cleanup;
mod doctor;
mod graph;
mod show;
mod list_tasks;
mod audit;

use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "hive", version, about = "Multi-agent orchestration harness")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new hive project in the current git repository
    Init,

    /// Show or manage configuration
    Config {
        /// Display merged config with source annotations
        #[arg(long)]
        show: bool,
    },

    /// Show task status overview
    Status,

    /// Orchestrate full execution chain for approved tasks
    Exec,

    /// Merge completed task branches
    Merge {
        /// Task ID to merge
        #[arg(long)]
        task: Option<String>,

        /// Merge all completed tasks in dependency order
        #[arg(long)]
        all: bool,

        /// Merge mode: pr (default) or direct
        #[arg(long, default_value = "pr")]
        mode: String,
    },

    /// Generate RFC document for a draft
    Rfc {
        /// Draft ID
        #[arg(long)]
        draft: String,
    },

    /// Approve a draft for execution
    Approve {
        /// Draft ID
        #[arg(long)]
        draft: String,
    },

    /// Verify acceptance criteria for a task
    Check {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// Process result.md from a completed worker
    Report {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// Claim a task for execution
    Claim {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// Create an isolated worktree for a task
    Isolate {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// Launch an agent in a task's worktree
    Launch {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// Clean up worktree after task completion
    Cleanup {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// Diagnose environment and project health
    Doctor,

    /// Display task dependency graph
    Graph,

    /// Show detailed task information
    Show {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// List all tasks with optional filters
    ListTasks {
        /// Filter by state
        #[arg(long)]
        state: Option<String>,
    },

    /// Query audit log
    Audit {
        /// Task ID (omit for global audit)
        #[arg(long)]
        task: Option<String>,
    },
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init => init::run(),
        Command::Config { show } => config::run(show),
        Command::Status => status::run(),
        Command::Exec => exec::run(),
        Command::Merge { task, all, mode } => merge::run(task, all, mode),
        Command::Rfc { draft } => rfc::run(draft),
        Command::Approve { draft } => approve::run(draft),
        Command::Check { task } => check::run(task),
        Command::Report { task } => report::run(task),
        Command::Claim { task } => claim::run(task),
        Command::Isolate { task } => isolate::run(task),
        Command::Launch { task } => launch::run(task),
        Command::Cleanup { task } => cleanup::run(task),
        Command::Doctor => doctor::run(),
        Command::Graph => graph::run(),
        Command::Show { task } => show::run(task),
        Command::ListTasks { state } => list_tasks::run(state),
        Command::Audit { task } => audit::run(task),
    }
}
