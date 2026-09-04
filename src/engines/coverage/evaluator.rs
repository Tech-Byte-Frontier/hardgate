use super::paths::CoveragePathIndex;
use super::{CoverageScorer, CoverageViolation, FileCoverage};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

impl CoverageScorer {
    pub(crate) fn evaluate_diff_coverage_legacy(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        changed_lines: &BTreeMap<PathBuf, BTreeSet<usize>>,
    ) -> Vec<CoverageViolation> {
        let mut violations = Vec::new();
        for (file, lines) in changed_lines {
            let Some(coverage) = unique_compatible_record(coverage_map, file) else {
                violations.push(missing_diff_violation(file, "has no coverage record"));
                continue;
            };
            let executable = lines
                .iter()
                .filter(|line| coverage.line_hits.contains_key(line))
                .count();
            if executable == 0 {
                continue;
            }
            let uncovered: Vec<_> = lines
                .iter()
                .filter(|line| coverage.line_hits.get(line) == Some(&0))
                .map(usize::to_string)
                .collect();
            if uncovered.is_empty() {
                continue;
            }
            let pct = super::calc_pct(executable - uncovered.len(), executable);
            violations.push(diff_violation(file, &uncovered, pct));
        }
        violations
    }

    /// Strict diff scoring: every supplied line is code-bearing and must have
    /// a DA entry. Missing entries and zero-hit entries are both blocking.
    pub(crate) fn evaluate_diff_coverage_strict_impl(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        changed_lines: &BTreeMap<PathBuf, BTreeSet<usize>>,
        root: &Path,
    ) -> Vec<CoverageViolation> {
        let index = CoveragePathIndex::new(coverage_map, root);
        let mut violations = Vec::new();
        for (file, lines) in changed_lines {
            let Some((_, coverage)) = index.resolve(coverage_map, file) else {
                violations.push(missing_diff_violation(file, "has no exact coverage record"));
                continue;
            };
            let Some(counts) = strict_line_counts(lines, coverage) else {
                violations.push(super::count_overflow_violation(file));
                continue;
            };
            emit_strict_violations(file, lines.len(), counts, &mut violations);
        }
        violations
    }
}

#[derive(Default)]
struct StrictLineCounts {
    covered: usize,
    uncovered: Vec<usize>,
    missing: Vec<usize>,
}

fn strict_line_counts(
    lines: &BTreeSet<usize>,
    coverage: &FileCoverage,
) -> Option<StrictLineCounts> {
    let mut counts = StrictLineCounts::default();
    for line in lines {
        match coverage.line_hits.get(line) {
            Some(hits) if *hits > 0 => {
                counts.covered = counts.covered.checked_add(1)?;
            }
            Some(_) => counts.uncovered.push(*line),
            None => counts.missing.push(*line),
        }
    }
    Some(counts)
}

fn emit_strict_violations(
    file: &Path,
    total: usize,
    counts: StrictLineCounts,
    violations: &mut Vec<CoverageViolation>,
) {
    if !counts.missing.is_empty() {
        violations.push(CoverageViolation {
            file: file.to_path_buf(),
            function_name: None,
            metric: "Missing Diff Coverage".to_string(),
            actual: 0.0,
            limit: 100.0,
            message: format!(
                "Changed code lines {} in `{}` have no DA entry",
                display_lines(&counts.missing),
                file.display()
            ),
            recommendation: "Regenerate LCOV so every changed code line has DA data.".to_string(),
        });
    }
    if counts.uncovered.is_empty() {
        return;
    }
    let pct = super::calc_pct(counts.covered, total);
    violations.push(diff_violation(file, &counts.uncovered, pct));
}

fn unique_compatible_record<'a>(
    coverage_map: &'a HashMap<PathBuf, FileCoverage>,
    changed_path: &Path,
) -> Option<&'a FileCoverage> {
    let mut matches: Vec<_> = coverage_map
        .iter()
        .filter(|(path, _)| compatibility_path_matches(path, changed_path))
        .collect();
    matches.sort_by_key(|(path, _)| *path);
    (matches.len() == 1).then(|| matches[0].1)
}

fn diff_violation(file: &Path, uncovered: &[impl ToString], pct: f64) -> CoverageViolation {
    let lines = uncovered
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    CoverageViolation {
        file: file.to_path_buf(),
        function_name: None,
        metric: "Diff Line Coverage".to_string(),
        actual: pct,
        limit: 100.0,
        message: format!(
            "Uncovered lines {lines} in `{}` ({pct:.1}%, need 100%)",
            file.display()
        ),
        recommendation: "Add tests covering the changed executable lines.".to_string(),
    }
}

fn compatibility_path_matches(report_path: &Path, changed_path: &Path) -> bool {
    let report = lexical_path(report_path);
    let changed = lexical_path(changed_path);
    !report.is_empty()
        && !changed.is_empty()
        && (report == changed
            || report.ends_with(&format!("/{changed}"))
            || changed.ends_with(&format!("/{report}")))
}

fn lexical_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn missing_diff_violation(file: &Path, reason: &str) -> CoverageViolation {
    CoverageViolation {
        file: file.to_path_buf(),
        function_name: None,
        metric: "Missing Diff Coverage".to_string(),
        actual: 0.0,
        limit: 100.0,
        message: format!("Changed source `{}` {reason}", file.display()),
        recommendation: "Regenerate LCOV for every changed source file.".to_string(),
    }
}

fn display_lines(lines: &[usize]) -> String {
    lines
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
