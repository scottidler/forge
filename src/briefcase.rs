use eyre::{Result, eyre};

use crate::config::ForgeConfig;

pub fn unpack(config: &ForgeConfig, pipeline: &str, input: Option<&str>, slug: Option<&str>) -> Result<()> {
    let _ = (config, pipeline, input, slug);
    Err(eyre!("unpack not yet implemented (Phase 2)"))
}

pub fn pack(config: &ForgeConfig, abandon: bool) -> Result<()> {
    let _ = (config, abandon);
    Err(eyre!("pack not yet implemented (Phase 2)"))
}
