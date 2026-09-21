//! taba — command-line interface for human operators.
//!
//! The `taba` binary is the primary user-facing entry point for
//! authoring units, managing composition, inspecting status, and
//! querying audit trails. For M5, the CLI operates in **local mode**
//! — it uses the library crates directly with an in-memory graph
//! persisted to a local state directory.

use clap::Parser;
use taba_cli::commands::{AuditCommand, Cli, Command, UnitCommand};
use taba_cli::error::CliError;

#[tokio::main]
async fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { force } => taba_cli::commands::run_init(cli.state_dir, force),
        Command::Apply { file, dry_run } => {
            taba_cli::commands::run_apply(cli.state_dir, &file, dry_run).await
        }
        Command::Unit { sub } => match sub {
            UnitCommand::List => taba_cli::commands::run_unit_list(cli.state_dir, cli.format).await,
            UnitCommand::Inspect { id } => {
                taba_cli::commands::run_unit_inspect(cli.state_dir, &id, cli.format).await
            }
            UnitCommand::Validate { file } => taba_cli::commands::run_unit_validate(&file),
            UnitCommand::Archive { id } => {
                taba_cli::commands::run_unit_archive(cli.state_dir, &id).await
            }
        },
        Command::Status => taba_cli::commands::run_status(cli.state_dir, cli.format).await,
        Command::Compose => taba_cli::commands::run_compose(cli.state_dir, cli.format).await,
        Command::Reconcile => taba_cli::commands::run_reconcile(cli.state_dir).await,
        Command::Daemon { interval } => {
            taba_cli::commands::run_daemon(cli.state_dir, &interval).await
        }
        Command::Audit { sub } => match sub {
            AuditCommand::Provenance { unit_id } => {
                taba_cli::commands::run_audit_provenance(cli.state_dir, &unit_id, cli.format).await
            }
            AuditCommand::Trails => {
                taba_cli::commands::run_audit_trails(cli.state_dir, cli.format).await
            }
        },
        Command::Push { file, cache_dir } => taba_cli::commands::run_push(&file, cache_dir),
    }
}
