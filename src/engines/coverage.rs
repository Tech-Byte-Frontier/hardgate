use crate::config::CoverageConfig;
use crate::engines::complexity::FunctionMetrics;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        let mut map = HashMap::new();
        let mut current = FileCoverage::default();
        let mut in_file = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("SF:") {
                current = FileCoverage::default();
                current.file_path = PathBuf::from(rest);
                in_file = true;
            } else if trimmed == "end_of_record" && in_file {
                map.insert(current.file_path.clone(), current.clone());
                in_file = false;
            } else if in_file {
                parse_lcov_metric_line(&mut current, trimmed);
            }
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
        self.evaluate_function_crap(coverage_map, functions, &mut violations);
        self.evaluate_critical_paths(coverage_map, root, &mut violations);
        violations
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
                    message: format!("Global line coverage {:.1}% is below floor {:.1}%", line_pct, min_line),
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
                    message: format!("Global function coverage {:.1}% is below floor {:.1}%", fn_pct, min_fn),
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
                    message: format!("Global branch coverage {:.1}% is below floor {:.1}%", br_pct, min_br),
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
            let cov_opt = coverage_map.iter().find(|(path, _)| {
                path.to_string_lossy().ends_with(func.file.to_string_lossy().as_ref())
            });

            if let Some((_, cov)) = cov_opt {
                let cov_ratio = calculate_function_coverage_ratio(cov, func.start_line, func.end_line);
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
                let matching = coverage_map.iter().find(|(p, _)| {
                    p.to_string_lossy().ends_with(cp.as_str())
                });
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
                            message: format!("Critical path `{}` has {:.1}% coverage (requires 100.0%)", cp, pct),
                            recommendation: "Ensure 100% test coverage for this critical module.".to_string(),
                        });
                    }
                }
            }
        }
    }
}

fn parse_lcov_metric_line(cov: &mut FileCoverage, line: &str) {
    if let Some(rest) = line.strip_prefix("DA:") {
        parse_da_line(cov, rest);
    } else {
        parse_count_line(cov, line);
    }
}

fn parse_da_line(cov: &mut FileCoverage, rest: &str) {
    let Some((l_str, h_str)) = rest.split_once(',') else { return };
    let (Ok(line_num), Ok(hits)) = (l_str.parse::<usize>(), h_str.parse::<usize>()) else { return };
    cov.line_hits.insert(line_num, hits);
}

fn parse_count_line(cov: &mut FileCoverage, line: &str) {
    let Some((tag, val_str)) = line.split_once(':') else { return };
    let Ok(val) = val_str.parse::<usize>() else { return };
    match tag {
        "LF" => cov.lines_found = val,
        "LH" => cov.lines_hit = val,
        "FNF" => cov.functions_found = val,
        "FNH" => cov.functions_hit = val,
        "BRF" => cov.branches_found = val,
        "BRH" => cov.branches_hit = val,
        _ => {}
    }
}

fn calculate_function_coverage_ratio(cov: &FileCoverage, start_line: usize, end_line: usize) -> f64 {
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
        1.0
    }
}

fn calc_pct(hit: usize, found: usize) -> f64 {
    if found == 0 {
        100.0
    } else {
        (hit as f64 / found as f64) * 100.0
    }
}
