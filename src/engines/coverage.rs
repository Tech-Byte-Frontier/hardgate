use crate::config::CoverageConfig;
use crate::engines::complexity::FunctionMetrics;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// One coverage or CRAP breach for a file or function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageViolation {
    pub file: PathBuf,
    pub function_name: Option<String>,
    pub metric: String,
    pub actual: f64,
    pub limit: f64,
    pub message: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Default)]
pub struct FileCoverage {
    pub file_path: PathBuf,
    pub lines_found: usize,
    pub lines_hit: usize,
    pub line_hits: HashMap<usize, usize>,
    pub functions_found: usize,
    pub functions_hit: usize,
    pub branches_found: usize,
    pub branches_hit: usize,
}

impl FileCoverage {
    pub fn line_coverage_percent(&self) -> f64 {
        if self.lines_found == 0 {
            100.0
        } else {
            (self.lines_hit as f64 / self.lines_found as f64) * 100.0
        }
    }

    pub fn function_coverage_percent(&self) -> f64 {
        if self.functions_found == 0 {
            100.0
        } else {
            (self.functions_hit as f64 / self.functions_found as f64) * 100.0
        }
    }

    pub fn branch_coverage_percent(&self) -> f64 {
        if self.branches_found == 0 {
            100.0
        } else {
            (self.branches_hit as f64 / self.branches_found as f64) * 100.0
        }
    }
}

pub struct CoverageScorer {
    config: CoverageConfig,
}

impl CoverageScorer {
    pub fn new(config: &CoverageConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn parse_lcov(&self, report_path: &Path) -> anyhow::Result<HashMap<PathBuf, FileCoverage>> {
        let content = fs::read_to_string(report_path)?;
        if content.trim().is_empty() {
            anyhow::bail!("LCOV report is empty");
        }
        let mut map = HashMap::new();
        let mut current = FileCoverage::default();
        let mut in_file = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("SF:") {
                if in_file {
                    anyhow::bail!("LCOV record started before the previous record ended");
                }
                if rest.trim().is_empty() {
                    anyhow::bail!("LCOV source path is empty");
                }
                current = FileCoverage::default();
                current.file_path = PathBuf::from(rest);
                in_file = true;
            } else if trimmed == "end_of_record" && in_file {
                map.insert(current.file_path.clone(), current.clone());
                in_file = false;
            } else if in_file {
                parse_lcov_metric_line(&mut current, trimmed)?;
            }
        }

        if in_file {
            anyhow::bail!("LCOV report ended before `end_of_record`");
        }
        if map.is_empty() {
            anyhow::bail!("LCOV report contains no source records");
        }

        Ok(map)
    }

    pub fn evaluate(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        functions: &[FunctionMetrics],
        root: &Path,
    ) -> Vec<CoverageViolation> {
        let mut violations = Vec::new();
        self.evaluate_global_floors(coverage_map, &mut violations);
        self.evaluate_missing_function_files(coverage_map, functions, &mut violations);
        self.evaluate_function_crap(coverage_map, functions, &mut violations);
        self.evaluate_critical_paths(coverage_map, root, &mut violations);
        violations
    }

    fn evaluate_missing_function_files(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        functions: &[FunctionMetrics],
        violations: &mut Vec<CoverageViolation>,
    ) {
        let mut missing = std::collections::HashSet::new();
        for function in functions {
            let present = coverage_map
                .keys()
                .any(|path| coverage_path_matches(path, &function.file));
            if !present && missing.insert(function.file.clone()) {
                violations.push(CoverageViolation {
                    file: function.file.clone(),
                    function_name: None,
                    metric: "Missing Source Coverage".to_string(),
                    actual: 0.0,
                    limit: self.config.min_line_percent.unwrap_or(0.0),
                    message: format!(
                        "Required coverage report has no record for `{}`",
                        function.file.display()
                    ),
                    recommendation:
                        "Instrument this classified source or explicitly change its coverage policy."
                            .to_string(),
                });
            }
        }
    }

    fn evaluate_global_floors(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        violations: &mut Vec<CoverageViolation>,
    ) {
        let mut lines_found = 0;
        let mut lines_hit = 0;
        let mut fn_found = 0;
        let mut fn_hit = 0;
        let mut br_found = 0;
        let mut br_hit = 0;

        for cov in coverage_map.values() {
            lines_found += cov.lines_found;
            lines_hit += cov.lines_hit;
            fn_found += cov.functions_found;
            fn_hit += cov.functions_hit;
            br_found += cov.branches_found;
            br_hit += cov.branches_hit;
        }

        let line_pct = calc_pct(lines_hit, lines_found);
        if let Some(min_line) = self.config.min_line_percent {
            if line_pct < min_line {
                violations.push(CoverageViolation {
                    file: PathBuf::from("workspace"),
                    function_name: None,
                    metric: "Global Line Coverage".to_string(),
                    actual: line_pct,
                    limit: min_line,
                    message: format!(
                        "Global line coverage {:.1}% is below floor {:.1}%",
                        line_pct, min_line
                    ),
                    recommendation: "Add tests to exercise uncovered lines.".to_string(),
                });
            }
        }

        let fn_pct = calc_pct(fn_hit, fn_found);
        if let Some(min_fn) = self.config.min_function_percent {
            if fn_pct < min_fn {
                violations.push(CoverageViolation {
                    file: PathBuf::from("workspace"),
                    function_name: None,
                    metric: "Global Function Coverage".to_string(),
                    actual: fn_pct,
                    limit: min_fn,
                    message: format!(
                        "Global function coverage {:.1}% is below floor {:.1}%",
                        fn_pct, min_fn
                    ),
                    recommendation: "Add tests exercising newly added functions.".to_string(),
                });
            }
        }

        let br_pct = calc_pct(br_hit, br_found);
        if let Some(min_br) = self.config.min_branch_percent {
            if br_pct < min_br {
                violations.push(CoverageViolation {
                    file: PathBuf::from("workspace"),
                    function_name: None,
                    metric: "Global Branch Coverage".to_string(),
                    actual: br_pct,
                    limit: min_br,
                    message: format!(
                        "Global branch coverage {:.1}% is below floor {:.1}%",
                        br_pct, min_br
                    ),
                    recommendation: "Add tests targeting branch conditions.".to_string(),
                });
            }
        }
    }

    fn evaluate_function_crap(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        functions: &[FunctionMetrics],
        violations: &mut Vec<CoverageViolation>,
    ) {
        let max_crap = self.config.max_crap_score.unwrap_or(25.0);

        for func in functions {
            let cov_opt = coverage_map
                .iter()
                .find(|(path, _)| coverage_path_matches(path, &func.file));

            if let Some((_, cov)) = cov_opt {
                let cov_ratio =
                    calculate_function_coverage_ratio(cov, func.start_line, func.end_line);
                let comp = func.cyclomatic as f64;
                let crap_score = comp.powi(2) * (1.0 - cov_ratio).powi(3) + comp;

                if crap_score > max_crap {
                    violations.push(CoverageViolation {
                        file: func.file.clone(),
                        function_name: Some(func.name.clone()),
                        metric: "CRAP Score".to_string(),
                        actual: crap_score,
                        limit: max_crap,
                        message: format!(
                            "CRAP score for `{}` is {:.1} (limit: {:.1}). Complexity: {}, Coverage: {:.1}%",
                            func.name, crap_score, max_crap, func.cyclomatic, cov_ratio * 100.0
                        ),
                        recommendation: format!(
                            "Write tests covering lines {}-{} in `{}` or reduce complexity.",
                            func.start_line, func.end_line, func.file.display()
                        ),
                    });
                }
            }
        }
    }

    fn evaluate_critical_paths(
        &self,
        coverage_map: &HashMap<PathBuf, FileCoverage>,
        root: &Path,
        violations: &mut Vec<CoverageViolation>,
    ) {
        if let Some(ref critical_paths) = self.config.critical_paths {
            for cp in critical_paths {
                let cp_path = Path::new(cp);
                let matching = coverage_map
                    .iter()
                    .find(|(p, _)| coverage_path_matches(p, cp_path));
                if let Some((path, cov)) = matching {
                    let pct = cov.line_coverage_percent();
                    if pct < 100.0 {
                        let rel = path.strip_prefix(root).unwrap_or(path);
                        violations.push(CoverageViolation {
                            file: rel.to_path_buf(),
                            function_name: None,
                            metric: "Critical Path 100% Coverage".to_string(),
                            actual: pct,
                            limit: 100.0,
                            message: format!(
                                "Critical path `{}` has {:.1}% coverage (requires 100.0%)",
                                cp, pct
                            ),
                            recommendation: "Ensure 100% test coverage for this critical module."
                                .to_string(),
                        });
                    }
                } else {
                    violations.push(CoverageViolation {
                        file: PathBuf::from(cp),
                        function_name: None,
                        metric: "Missing Critical Path".to_string(),
                        actual: 0.0,
                        limit: 100.0,
                        message: format!(
                            "Critical path `{}` is absent from the required coverage report",
                            cp
                        ),
                        recommendation:
                            "Instrument the critical path and regenerate the coverage report."
                                .to_string(),
                    });
                }
            }
        }
    }
}

fn parse_lcov_metric_line(cov: &mut FileCoverage, line: &str) -> anyhow::Result<()> {
    if let Some(rest) = line.strip_prefix("DA:") {
        parse_da_line(cov, rest)?;
    } else {
        parse_count_line(cov, line)?;
    }
    Ok(())
}

fn parse_da_line(cov: &mut FileCoverage, rest: &str) -> anyhow::Result<()> {
    // DA:<line>,<hits>[,<checksum>] — checksum is optional (lcov --checksum).
    let mut parts = rest.split(',');
    let (Some(l_str), Some(h_str)) = (parts.next(), parts.next()) else {
        anyhow::bail!("Malformed LCOV DA record `{rest}`");
    };
    let (Ok(line_num), Ok(hits)) = (l_str.parse::<usize>(), h_str.parse::<usize>()) else {
        anyhow::bail!("Malformed LCOV DA values `{rest}`");
    };
    cov.line_hits.insert(line_num, hits);
    Ok(())
}

fn parse_count_line(cov: &mut FileCoverage, line: &str) -> anyhow::Result<()> {
    let Some((tag, val_str)) = line.split_once(':') else {
        return Ok(());
    };
    if !matches!(tag, "LF" | "LH" | "FNF" | "FNH" | "BRF" | "BRH") {
        return Ok(());
    }
    let Ok(val) = val_str.parse::<usize>() else {
        anyhow::bail!("Malformed LCOV {tag} count `{val_str}`");
    };
    match tag {
        "LF" => cov.lines_found = val,
        "LH" => cov.lines_hit = val,
        "FNF" => cov.functions_found = val,
        "FNH" => cov.functions_hit = val,
        "BRF" => cov.branches_found = val,
        "BRH" => cov.branches_hit = val,
        _ => {}
    }
    Ok(())
}

fn calculate_function_coverage_ratio(
    cov: &FileCoverage,
    start_line: usize,
    end_line: usize,
) -> f64 {
    let mut executable = 0;
    let mut hit = 0;

    for line in start_line..=end_line {
        if let Some(&hits) = cov.line_hits.get(&line) {
            executable += 1;
            if hits > 0 {
                hit += 1;
            }
        }
    }

    if executable > 0 {
        hit as f64 / executable as f64
    } else {
        // File is in the report but has no executable lines in this range:
        // treat as uncovered (0.0) instead of hiding it as 1.0.
        0.0
    }
}

fn normalize_cov_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    // Strip common absolute prefixes so `/repo/src/foo.rs` matches `src/foo.rs`.
    s.to_string()
}

fn coverage_path_matches(report_path: &Path, func_path: &Path) -> bool {
    let a = normalize_cov_path(report_path);
    let b = normalize_cov_path(func_path);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.ends_with(b.as_str()) || b.ends_with(a.as_str())
}

fn calc_pct(hit: usize, found: usize) -> f64 {
    if found == 0 {
        100.0
    } else {
        (hit as f64 / found as f64) * 100.0
    }
}
