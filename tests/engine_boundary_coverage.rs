use hardgate::config::{CoverageConfig, OrchestrationConfig};
use hardgate::engines::OrchestrationEngine;
use hardgate::engines::coverage::{CoverageScorer, FileCoverage};
use hardgate::engines::orchestration::OrchestrationStep;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn orchestration(
    format_check: Option<&str>,
    format: Option<&str>,
    lint: Option<&str>,
    test_cmd: Option<&str>,
    timeout_secs: Option<u64>,
) -> OrchestrationConfig {
    OrchestrationConfig {
        format_check: format_check.map(str::to_owned),
        format: format.map(str::to_owned),
        lint: lint.map(str::to_owned),
        test_cmd: test_cmd.map(str::to_owned),
        timeout_secs,
    }
}

fn step(command: &str) -> OrchestrationStep<'_> {
    OrchestrationStep {
        step: "boundary",
        command,
        recommendation: "repair the boundary command",
    }
}

fn report(details: &str, counts: &str) -> String {
    format!("TN:\nSF:src/lib.rs\n{details}DA:1,1\nLF:1\nLH:1\n{counts}\nend_of_record\n")
}

fn parse_report(
    body: &str,
    require_functions: bool,
    require_branches: bool,
) -> anyhow::Result<HashMap<PathBuf, FileCoverage>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "hardgate-engine-boundary-{}-{}.info",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, body)?;
    let config = CoverageConfig {
        enabled: true,
        min_function_percent: require_functions.then_some(1.0),
        min_branch_percent: require_branches.then_some(1.0),
        ..CoverageConfig::default()
    };
    let result = CoverageScorer::new(&config).parse_lcov(&path);
    let _ = std::fs::remove_file(path);
    result
}

#[test]
fn orchestration_presence_and_format_fallbacks_are_explicit() {
    let empty = OrchestrationEngine::new(&OrchestrationConfig::default());
    assert!(!empty.has_orchestration());
    assert!(empty.run_format_check(Path::new(".")).is_none());
    assert!(empty.run_format(Path::new(".")).is_none());
    assert!(empty.run_lint(Path::new(".")).is_none());
    assert!(empty.run_tests(Path::new(".")).is_none());

    for config in [
        orchestration(Some("true"), None, None, None, Some(1)),
        orchestration(None, Some("true"), None, None, Some(1)),
        orchestration(None, None, Some("true"), None, Some(1)),
        orchestration(None, None, None, Some("true"), Some(1)),
    ] {
        assert!(OrchestrationEngine::new(&config).has_orchestration());
    }

    let fallback = OrchestrationEngine::new(&orchestration(
        Some("printf check"),
        None,
        None,
        None,
        Some(1),
    ));
    assert_eq!(
        fallback.run_format(Path::new(".")).unwrap().unwrap().output,
        "check"
    );
    let explicit = OrchestrationEngine::new(&orchestration(
        Some("printf check"),
        Some("printf format"),
        None,
        None,
        Some(1),
    ));
    assert_eq!(
        explicit.run_format(Path::new(".")).unwrap().unwrap().output,
        "format"
    );
}

#[test]
fn orchestration_empty_runner_and_exit_paths_keep_diagnostics() {
    let engine = OrchestrationEngine::new(&orchestration(None, None, None, None, Some(1)));
    let empty = engine
        .run_step(step(""), Path::new("."))
        .expect_err("empty commands must fail closed");
    assert_eq!(empty.exit_code, None);
    assert!(empty.output.contains("Empty command string"));
    assert!(empty.recommendation.contains("boundary"));

    let missing = engine
        .run_step(step("hardgate-boundary-command-is-missing"), Path::new("."))
        .expect_err("missing executables must fail closed");
    assert_eq!(missing.exit_code, None);
    assert!(missing.output.contains("Failed to execute"));
    assert!(missing.recommendation.contains("installed"));

    let exited = engine
        .run_step(
            step("sh -c 'printf boundary-failure >&2; exit 9'"),
            Path::new("."),
        )
        .expect_err("nonzero commands must become violations");
    assert_eq!(exited.exit_code, Some(9));
    assert!(exited.output.contains("boundary-failure"));
    assert!(exited.recommendation.contains("boundary command"));
}

#[test]
fn orchestration_collects_success_failure_and_disabled_steps() {
    let config = orchestration(
        Some("printf format-boundary"),
        None,
        Some("sh -c 'printf lint-boundary >&2; exit 4'"),
        None,
        Some(1),
    );
    let (results, violations) = OrchestrationEngine::new(&config).run_all_checks(Path::new("."));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].step, "format_check");
    assert_eq!(results[0].output, "format-boundary");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].step, "lint");
    assert_eq!(violations[0].exit_code, Some(4));
    assert!(violations[0].output.contains("lint-boundary"));
    assert!(violations[0].recommendation.contains("linter"));
}

#[cfg(unix)]
#[test]
fn orchestration_timeout_keeps_cleanup_evidence() {
    let config = orchestration(Some("sleep 2"), None, None, None, Some(1));
    let violation = OrchestrationEngine::new(&config)
        .run_format_check(Path::new("."))
        .unwrap()
        .expect_err("long-running commands must time out");
    assert!(violation.output.contains("timed out"));
    assert!(violation.output.contains("process group"));
    assert!(violation.recommendation.contains("timeout_secs"));
}

#[test]
fn lcov_details_keep_ambiguous_names_and_reject_malformed_fields() {
    let named = report(
        "FN:1,lambda,variant\nFNDA:0,lambda,variant\nFNF:1\nFNH:0\n",
        "",
    );
    let parsed =
        parse_report(&named, true, false).expect("comma names are valid function identities");
    assert_eq!(parsed[&PathBuf::from("src/lib.rs")].functions_found, 1);

    let projected = report(
        "FN:1,first\nFN:2,second\nFNDA:0,first\nFNDA:0,second\n",
        "FNF:1\nFNH:1\n",
    );
    assert!(parse_report(&projected, true, false).is_ok());
    let missing_function_counts = parse_report(
        &report("FN:1,unbounded\nFNDA:0,unbounded\n", ""),
        false,
        false,
    )
    .expect_err("function details require aggregate counts");
    assert!(
        missing_function_counts
            .to_string()
            .contains("matching FNF/FNH")
    );

    for (details, expected) in [
        ("FN:1,\0\nFNDA:0,\0\nFNF:1\nFNH:0\n", "function name"),
        ("BRDA:1,,0,1\n", "BRDA block field"),
        ("BRDA:1,0,\0,1\n", "BRDA branch field"),
        ("BRDA:1,0,,1\n", "BRDA branch field"),
    ] {
        let error = parse_report(&report(details, ""), false, false)
            .expect_err("malformed detail records must fail closed")
            .to_string();
        assert!(error.contains(expected), "{error}");
    }

    let branch_error = parse_report(&report("BRDA:1,0,0,1\n", ""), false, false)
        .expect_err("branch details require aggregate counts");
    assert!(branch_error.to_string().contains("matching BRF/BRH"));
    let not_taken = parse_report(&report("BRDA:1,0,0,-\n", "BRF:1\nBRH:0"), false, true)
        .expect("not-taken branches are valid LCOV details");
    let coverage = &not_taken[&PathBuf::from("src/lib.rs")];
    assert_eq!((coverage.branches_found, coverage.branches_hit), (1, 0));
}
