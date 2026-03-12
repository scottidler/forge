use eyre::{Result, eyre};

use crate::config::ForgeConfig;

pub fn run_stage(config: &ForgeConfig, stage: Option<&str>, input: Option<&str>) -> Result<()> {
    let _ = (config, stage, input);
    Err(eyre!("run not yet implemented (Phase 3)"))
}
