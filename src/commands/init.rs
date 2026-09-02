use crate::config::{HardgateConfig, Preset};
use anyhow::Result;
use colored::*;
use std::fs;
use std::path::Path;

pub fn cmd_init(preset_str: &str) -> Result<()> {
    let target = Path::new("hardgate.toml");
    if target.exists() {
        println!(
            "{} `hardgate.toml` already exists in this directory.",
            "⚠️".yellow()
        );
        return Ok(());
    }

    let preset = match preset_str.to_lowercase().as_str() {
        "balanced" => Preset::Balanced,
        "legacy-migration" => Preset::LegacyMigration,
        "custom" => Preset::Custom,
        _ => Preset::StrictAgent,
    };

    let toml_content = HardgateConfig::generate_toml_template(preset);
    fs::write(target, toml_content)?;

    println!(
        "{} Initialized {} with preset [{}]",
        "✓".green(),
        "hardgate.toml".bold(),
        format!("{:?}", preset).bold()
    );
    Ok(())
}
