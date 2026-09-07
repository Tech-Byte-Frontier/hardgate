//! Preserve unexecuted Stryker locations separately from compiler failures.
use super::gatekeeper::MutationViolation;
use serde_json::Value;
use std::path::Path;

pub(super) fn locations(report: &Value) -> Vec<String> {
    let Some(files) = report.get("files").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut locations = Vec::new();
    for (file, entry) in files {
        for mutant in entry
            .get("mutants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let status = mutant
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let normalized = status
                .to_ascii_lowercase()
                .replace(['-', ' ', '/', '_'], "");
            if matches!(
                normalized.as_str(),
                "nocoverage" | "notcovered" | "ignored" | "pending" | "notrun"
            ) {
                let start = &mutant["location"]["start"];
                let line = start["line"]
                    .as_u64()
                    .map_or("?".into(), |value| value.to_string());
                let column = start["column"]
                    .as_u64()
                    .map_or("?".into(), |value| value.to_string());
                let id = mutant["id"].as_str().unwrap_or("unknown");
                locations.push(format!("{file}:{line}:{column} (mutant {id}, {status})"));
            }
        }
    }
    locations
}

pub(super) fn violation(
    path: &Path,
    locations: &[String],
    stats: &super::gatekeeper::MutationStats,
    floor: f64,
) -> MutationViolation {
    let score = stats.score_percent();
    let viable = stats.killed > 0 || stats.survived > 0;
    MutationViolation {
        report_file: path.to_path_buf(),
        metric: "Mutation Unexecuted Mutants".into(),
        actual: locations.len() as f64,
        limit: 0.0,
        message: format!(
            "Score {} ({score:.2}%, floor {floor:.2}%); evidence incomplete: {} unexecuted mutants.",
            if viable && score >= floor {
                "passed"
            } else {
                "failed"
            },
            locations.len()
        ),
        recommendation: format!(
            "Execute these mutants with tests, then regenerate with `hardgate evidence stryker`: {}. NoCoverage is missing execution evidence, even when the score passes.",
            locations.join(", ")
        ),
    }
}
