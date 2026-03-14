#![deny(clippy::unwrap_used)]
#![deny(dead_code)]
#![deny(unused_variables)]

use clap::Parser;
use eyre::{Context, Result};
use log::{debug, info};
use std::fs;
use std::path::PathBuf;

use forge::cli::Cli;
use forge::config::ForgeConfig;

fn resolve_log_level(cli_level: Option<&str>, config_level: Option<&str>) -> String {
    // Priority: CLI > env var > config > default
    if let Some(level) = cli_level {
        return level.to_string();
    }
    if let Ok(level) = std::env::var("FORGE_LOG_LEVEL") {
        return level;
    }
    if let Some(level) = config_level {
        return level.to_string();
    }
    "info".to_string()
}

fn setup_logging(level: &str) -> Result<()> {
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

    env_logger::Builder::new()
        .target(env_logger::Target::Pipe(target))
        .parse_filters(level)
        .format_timestamp_secs()
        .init();

    info!(
        "Logging initialized at level={}, writing to: {}",
        level,
        log_file.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Init must run before config loading since it creates the config
    if let forge::cli::Command::Init { force } = &cli.command {
        return forge::init::init(*force);
    }

    let config = ForgeConfig::load(cli.config.as_ref()).context("Failed to load forge configuration")?;

    let log_level = resolve_log_level(cli.log_level.as_deref(), config.log_level.as_deref());
    setup_logging(&log_level).context("Failed to setup logging")?;

    debug!(
        "cli: config={:?}, verbose={}, log_level={:?}, command={:?}",
        cli.config, cli.verbose, cli.log_level, log_level
    );
    debug!("config: {:?}", config);

    forge::run_command(&cli.command, &config)?;

    Ok(())
}
