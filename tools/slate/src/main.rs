/// slate — the SlateOS CLI build tool.
///
/// Entry point for building, flashing, and configuring SlateOS.
/// Intended to be installed via:
///   curl -sL slateos.org/go | sh
///
/// Subcommands:
///   build   — compile the shell crates (and optionally a rootfs)
///   flash   — flash a built image onto a connected device
///   config  — print or edit the current slate CLI configuration
///   status  — show build state and available shell crates
mod build;
mod check;
mod config;
mod cross;
mod dev;
mod device;
mod flash;
mod info;
mod init;
mod services;
mod status;
mod workspace;

use anyhow::Result;
use clap::{Parser, Subcommand};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// SlateOS build tool.
#[derive(Debug, Parser)]
#[command(
    name = "slate",
    about = "Build, flash, and configure SlateOS",
    version,
    propagate_version = true
)]
struct Cli {
    /// Increase log verbosity (-v = DEBUG, -vv = TRACE).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build the SlateOS shell crates (and optionally a full rootfs).
    Build(build::BuildArgs),

    /// Flash a built image onto a connected device.
    Flash(flash::FlashArgs),

    /// Print or edit the slate CLI configuration.
    Config(config::ConfigArgs),

    /// Show current build status and available shell crates.
    Status(status::StatusArgs),

    /// List and inspect arkhe service definitions.
    Services(services::ServicesArgs),

    /// Run check + clippy + tests in one command.
    Check(check::CheckArgs),

    /// Start a development loop (rebuild on file change).
    Dev(dev::DevArgs),

    /// Print system and project diagnostic info.
    Info(info::InfoArgs),

    /// Set up a local development environment for a device.
    Init(init::InitArgs),
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise tracing: -v → DEBUG, -vv → TRACE, default → INFO.
    let level = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(format!("slate={level}"))
        .without_time()
        .init();

    match cli.command {
        Command::Build(args) => build::run(args),
        Command::Flash(args) => flash::run(args),
        Command::Config(args) => config::run(args),
        Command::Status(args) => status::run(args),
        Command::Services(args) => services::run(args),
        Command::Check(args) => check::run(args),
        Command::Dev(args) => dev::run(args),
        Command::Info(args) => info::run(args),
        Command::Init(args) => init::run(args),
    }
}
