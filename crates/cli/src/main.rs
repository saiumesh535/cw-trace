//! `cw-trace`: CDK-aware CloudWatch log + X-Ray trace debugger.
//!
//! Thin CLI over the workspace core crates. `logs` and `trace` subcommands
//! land in M2; until then `status` proves the arg plumbing.

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cw-trace", version, about = "CDK-aware CloudWatch log + X-Ray trace debugger")]
struct Cli {
    /// AWS profile (defaults to the standard credential chain).
    #[arg(long, global = true)]
    profile: Option<String>,

    /// AWS region (defaults to the standard credential chain).
    #[arg(long, global = true)]
    region: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show tool status (placeholder until M2).
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status => {
            println!("cw-trace {}", env!("CARGO_PKG_VERSION"));
            println!(
                "profile: {}",
                cli.profile.as_deref().unwrap_or("(default chain)")
            );
            println!(
                "region: {}",
                cli.region.as_deref().unwrap_or("(default chain)")
            );
        }
    }
    Ok(())
}
