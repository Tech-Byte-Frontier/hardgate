use super::paths::CoveragePathIndex;
use super::{CoverageScorer, CoverageViolation, FileCoverage};
use crate::engines::complexity::FunctionMetrics;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Inputs that determine which report records participate in scoring.
pub struct CoverageEvaluationScope<'a> {
    pub root: &'a Path,
    pub source_files: Option<&'a [PathBuf]>,
}

struct ScoreContext<'a> {
    coverage_map: &'a HashMap<PathBuf, FileCoverage>,
    functions: &'a [FunctionMetrics],
    index: &'a CoveragePathIndex,
    source_keys: Option<&'a BTreeSet<String>>,
    root: &'a Path,
    violations: &'a mut Vec<CoverageViolation>,
}

struct SourceRecordInput<'a, 'b, 'c> {
    files: &'a [PathBuf],
    coverage_map: &'b HashMap<PathBuf, FileCoverage>,
    index: &'b CoveragePathIndex,
    min_line_percent: Option<f64>,
    violations: &'c mut Vec<CoverageViolation>,
}

impl CoverageScorer {
    pub(crate) fn evaluate_internal(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        functions: &[FunctionMetrics],
        scope: CoverageEvaluationScope<'_>,
    ) -> Vec<CoverageViolation> {
        let index = CoveragePathIndex::new(coverage_map, scope.root);
        let mut violations = Vec::new();
        let (source_keys, records) = match scope.source_files {
            Some(files) => source_records(SourceRecordInput {
                files,
                coverage_map,
                index: &index,
                min_line_percent: self.config.min_line_percent,
                violations: &mut violations,
            }),
            None => (None, index.unique_records(coverage_map)),
        };
        self.evaluate_global_floors(&records, &mut violations);
        let mut context = ScoreContext {
            coverage_map,
            functions,
            index: &index,
            source_keys: source_keys.as_ref(),
            root: scope.root,
            violations: &mut violations,
        };
        evaluate_missing_function_files(&self.config, &mut context);
        evaluate_function_crap(self.config.max_crap_score, &mut context);
        evaluate_critical_paths(&mut context, self.config.critical_paths.as_deref());
        violations
    }

    fn evaluate_global_floors(
        &self,
        records: &[&FileCoverage],
        violations: &mut Vec<CoverageViolation>,
    ) {
        let Some(totals) = coverage_totals(records) else {
            violations.push(super::count_overflow_violation(Path::new("workspace")));
            return;
        };
        for (metric, floor, hit, found, recommendation) in coverage_floors(&self.config, totals) {
            let Some(limit) = floor else { continue };
            let actual = super::calc_pct(hit, found);
            if actual < limit {
                violations.push(CoverageViolation {
                    file: PathBuf::from("workspace"),
                    function_name: None,
                    metric: metric.to_string(),
                    actual,
                    limit,
                    message: format!("{metric} {actual:.1}% is below floor {limit:.1}%"),
                    recommendation: recommendation.to_string(),
                });
            }
        }
    }
}

fn source_records<'a>(
    input: SourceRecordInput<'_, 'a, '_>,
) -> (Option<BTreeSet<String>>, Vec<&'a FileCoverage>) {
    let mut keys = BTreeSet::new();
    let mut records = Vec::new();
    for file in input.files {
        let Some(key) = input.index.key(file) else {
            missing_source_violation(
                file,
                "is outside the repository root",
                input.min_line_percent,
                input.violations,
            );
            continue;
        };
        if !keys.insert(key) {
            continue;
        }
        if let Some((_, coverage)) = input.index.resolve(input.coverage_map, file) {
            records.push(coverage);
        } else {
            let reason = if input.index.is_ambiguous(file) {
                "has ambiguous duplicate report paths"
            } else {
                "is absent from the required report"
            };
            missing_source_violation(file, reason, input.min_line_percent, input.violations);
        }
    }
    (Some(keys), records)
}

fn missing_source_violation(
    file: &Path,
    reason: &str,
    min_line_percent: Option<f64>,
    violations: &mut Vec<CoverageViolation>,
) {
    violations.push(CoverageViolation {
        file: file.to_path_buf(),
        function_name: None,
        metric: "Missing Source Coverage".to_string(),
        actual: 0.0,
        limit: min_line_percent.unwrap_or(0.0),
        message: format!(
            "Required coverage report has no exact record for `{}`: {reason}",
            file.display()
        ),
        recommendation: "Regenerate LCOV with an exact repository-relative source path."
            .to_string(),
    });
}

fn coverage_totals(records: &[&FileCoverage]) -> Option<[usize; 6]> {
    records
        .iter()
        .try_fold([0usize; 6], |mut totals, coverage| {
            let values = [
                coverage.lines_found,
                coverage.lines_hit,
                coverage.functions_found,
                coverage.functions_hit,
                coverage.branches_found,
                coverage.branches_hit,
            ];
            for (total, value) in totals.iter_mut().zip(values) {
                *total = (*total).checked_add(value)?;
            }
            Some(totals)
        })
}

fn coverage_floors(
    config: &crate::config::CoverageConfig,
    totals: [usize; 6],
) -> [(&'static str, Option<f64>, usize, usize, &'static str); 3] {
    [
        (
            "Global Line Coverage",
            config.min_line_percent,
            totals[1],
            totals[0],
            "Add tests to exercise uncovered lines.",
        ),
        (
            "Global Function Coverage",
            config.min_function_percent,
            totals[3],
            totals[2],
            "Add tests exercising newly added functions.",
        ),
        (
            "Global Branch Coverage",
            config.min_branch_percent,
            totals[5],
            totals[4],
            "Add tests targeting branch conditions.",
        ),
    ]
}

fn evaluate_missing_function_files(
    config: &crate::config::CoverageConfig,
    context: &mut ScoreContext<'_>,
) {
    if context.source_keys.is_some() {
        return;
    }
    let mut missing = HashSet::new();
    for function in context.functions {
        if !function_in_scope(context.index, context.source_keys, &function.file)
            || context
                .index
                .resolve(context.coverage_map, &function.file)
                .is_some()
            || !missing.insert(function.file.clone())
        {
            continue;
        }
        context.violations.push(CoverageViolation {
            file: function.file.clone(),
            function_name: None,
            metric: "Missing Source Coverage".to_string(),
            actual: 0.0,
            limit: config.min_line_percent.unwrap_or(0.0),
            message: format!(
                "Required coverage report has no exact record for `{}`",
                function.file.display()
            ),
            recommendation: "Instrument this classified source and regenerate LCOV.".to_string(),
        });
    }
}

fn evaluate_function_crap(max_crap: Option<f64>, context: &mut ScoreContext<'_>) {
    let max_crap = max_crap.unwrap_or(25.0);
    for function in context.functions {
        if !function_in_scope(context.index, context.source_keys, &function.file) {
            continue;
        }
        let Some((_, coverage)) = context.index.resolve(context.coverage_map, &function.file)
        else {
            continue;
        };
        let coverage_ratio =
            calculate_function_coverage_ratio(coverage, function.start_line, function.end_line);
        let complexity = function.cyclomatic as f64;
        let crap = complexity.powi(2) * (1.0 - coverage_ratio).powi(3) + complexity;
        if crap > max_crap {
            context.violations.push(CoverageViolation {
                file: function.file.clone(),
                function_name: Some(function.name.clone()),
                metric: "CRAP Score".to_string(),
                actual: crap,
                limit: max_crap,
                message: format!(
                    "CRAP score for `{}` is {crap:.1} (limit: {max_crap:.1}). Complexity: {}, Coverage: {:.1}%",
                    function.name,
                    function.cyclomatic,
                    coverage_ratio * 100.0
                ),
                recommendation: format!(
                    "Write tests covering lines {}-{} in `{}` or reduce complexity.",
                    function.start_line,
                    function.end_line,
                    function.file.display()
                ),
            });
        }
    }
}

fn evaluate_critical_paths(context: &mut ScoreContext<'_>, critical_paths: Option<&[String]>) {
    let Some(critical_paths) = critical_paths else {
        return;
    };
    for critical_path in critical_paths {
        let path = Path::new(critical_path);
        let Some((report_path, coverage)) = context.index.resolve(context.coverage_map, path)
        else {
            context.violations.push(CoverageViolation {
                file: PathBuf::from(critical_path),
                function_name: None,
                metric: "Missing Critical Path".to_string(),
                actual: 0.0,
                limit: 100.0,
                message: format!(
                    "Critical path `{critical_path}` is absent or ambiguous in the required coverage report"
                ),
                recommendation: "Instrument the critical path and regenerate LCOV.".to_string(),
            });
            continue;
        };
        let actual = coverage.line_coverage_percent();
        if actual >= 100.0 {
            continue;
        }
        let display = super::paths::normalized_repository_key(report_path, context.root)
            .map(PathBuf::from)
            .unwrap_or_else(|| report_path.to_path_buf());
        context.violations.push(CoverageViolation {
            file: display,
            function_name: None,
            metric: "Critical Path 100% Coverage".to_string(),
            actual,
            limit: 100.0,
            message: format!(
                "Critical path `{critical_path}` has {actual:.1}% coverage (requires 100.0%)"
            ),
            recommendation: "Ensure 100% test coverage for this critical module.".to_string(),
        });
    }
}

fn function_in_scope(
    index: &CoveragePathIndex,
    source_keys: Option<&BTreeSet<String>>,
    path: &Path,
) -> bool {
    source_keys
        .map(|keys| index.key(path).is_some_and(|key| keys.contains(&key)))
        .unwrap_or(true)
}

fn calculate_function_coverage_ratio(
    coverage: &FileCoverage,
    start_line: usize,
    end_line: usize,
) -> f64 {
    if end_line < start_line {
        return 0.0;
    }
    let (executable, hit) = coverage
        .line_hits
        .iter()
        .filter(|&(&line, _)| (start_line..=end_line).contains(&line))
        .fold((0usize, 0usize), |(executable, hit), (_, hits)| {
            let executable = executable.saturating_add(1);
            let hit = hit.saturating_add(usize::from(*hits > 0));
            (executable, hit)
        });
    super::calc_ratio(hit, executable)
}
