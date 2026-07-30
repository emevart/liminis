//! The `liminis` binary.
//!
//! One command so far. It reads a scenario config, applies defaults, and prints
//! the identity of the run it would have performed. There is no tick loop.

use anyhow::Result;
use clap::{Parser, Subcommand};
use liminis_core::{config, version::WORLD_FORMAT_VERSION};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "liminis",
    version,
    about = "Voxel simulator of a biological ecosystem"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read a scenario config and print the run identity.
    Run {
        /// Path to a scenario TOML file.
        #[arg(long, value_name = "PATH")]
        config: PathBuf,
        /// Seed for the run.
        #[arg(long, value_name = "N")]
        seed: u64,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Run { config, seed } => {
            let scenario = config::load(&config)?;
            let hash = config::config_hash(&scenario)?;
            println!(
                "seed={seed} config_hash={hash} world_format_version={WORLD_FORMAT_VERSION} code_version={}",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
    }
}
