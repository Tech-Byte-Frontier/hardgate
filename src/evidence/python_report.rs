//! coverage.py omits empty LCOV counter pairs. Add a zero pair only when its
//! native JSON explicitly proves both counters zero; never infer execution.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::path::Path;

pub(super) fn normalize(content: &str, native: &Path) -> Result<String> {
    let native: Value = serde_json::from_slice(&std::fs::read(native)?)?;
    ensure!(
        native
            .pointer("/meta/branch_coverage")
            .and_then(Value::as_bool)
            == Some(true),
        "Python producer did not collect native branch execution"
    );
    let mut output = String::new();
    for section in content
        .split("end_of_record")
        .filter(|section| !section.trim().is_empty())
    {
        let source = section
            .lines()
            .find_map(|line| line.strip_prefix("SF:"))
            .context("Python LCOV record has no source")?;
        let file = native["files"]
            .get(source)
            .context("Python LCOV source has no native JSON counterpart")?;
        output.push_str(section.trim());
        output.push('\n');
        for (found, hit, total, covered) in counters(file)? {
            let found_present = section
                .lines()
                .any(|line| line.starts_with(&format!("{found}:")));
            let hit_present = section
                .lines()
                .any(|line| line.starts_with(&format!("{hit}:")));
            if !found_present || !hit_present {
                ensure!(
                    !found_present && !hit_present && total == 0 && covered == 0,
                    "native Python report omitted nonempty {found}/{hit} evidence"
                );
                output.push_str(&format!("{found}:0\n{hit}:0\n"));
            }
        }
        output.push_str("end_of_record\n");
    }
    Ok(output)
}

fn counters(file: &Value) -> Result<[(&'static str, &'static str, u64, u64); 3]> {
    let functions = file["functions"]
        .as_object()
        .context("Python evidence requires coverage.py function-region metadata")?;
    let mut function_count = 0;
    let mut function_hits = 0;
    for (name, region) in functions {
        if !name.is_empty() && count(region, "num_statements")? > 0 {
            function_count += 1;
            function_hits += u64::from(count(region, "covered_lines")? > 0);
        }
    }
    Ok([
        (
            "LF",
            "LH",
            count(file, "num_statements")?,
            count(file, "covered_lines")?,
        ),
        (
            "BRF",
            "BRH",
            count(file, "num_branches")?,
            count(file, "covered_branches")?,
        ),
        ("FNF", "FNH", function_count, function_hits),
    ])
}

fn count(file: &Value, key: &str) -> Result<u64> {
    file["summary"][key]
        .as_u64()
        .with_context(|| format!("native Python JSON lacks {key}"))
}

#[cfg(test)]
#[path = "python_report_tests.rs"]
mod tests;
