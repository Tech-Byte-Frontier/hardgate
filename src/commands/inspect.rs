use crate::config::ConfigContext;
use anyhow::Result;
use std::io::{self, Write};

/// Inspect validated effective policy without running any configured command.
pub fn cmd_config(context: &ConfigContext, format: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    if format == "json" {
        let value = serde_json::json!({
            "schema_version": 1,
            "command": "config",
            "status": "passed",
            "passed": true,
            "exit_code": 0,
            "config_identity": crate::diagnostics::execution::ConfigIdentity::from_context(context)?,
            "config_path": context.config_path,
            "root": context.root,
            "invocation_dir": context.invocation_dir,
            "effective": context.config,
        });
        serde_json::to_writer_pretty(&mut out, &value)?;
        writeln!(out)?;
    } else {
        writeln!(out, "# Configuration root: {}", context.root.display())?;
        match &context.config_path {
            Some(path) => writeln!(out, "# Policy: {}", path.display())?,
            None => writeln!(out, "# Policy: implicit strict-agent defaults")?,
        }
        write!(out, "{}", toml::to_string_pretty(&context.config)?)?;
    }
    Ok(())
}
