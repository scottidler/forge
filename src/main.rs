#![deny(clippy::unwrap_used)]
#![deny(dead_code)]
#![deny(unused_variables)]

use clap::Parser;
use eyre::{Context, Result};
use log::info;
use std::fs;
use std::path::PathBuf;

use forge::cli::Cli;
use forge::config::ForgeConfig;

fn setup_logging() -> Result<()> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("forge")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let log_file = log_dir.join("forge.log");

    let target = Box::new(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context("Failed to open log file")?,
    );

    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(target))
        .init();

    info!("Logging initialized, writing to: {}", log_file.display());
    Ok(())
}

fn main() -> Result<()> {
    setup_logging().context("Failed to setup logging")?;

    let cli = Cli::parse();

    // Init must run before config loading since it creates the config
    if let forge::cli::Command::Init { force } = &cli.command {
        return forge::init::init(*force);
    }

    let config = ForgeConfig::load(cli.config.as_ref()).context("Failed to load forge configuration")?;

    info!("Starting with config from: {:?}", cli.config);

    forge::run_command(&cli.command, &config)?;

    Ok(())
}
