#[path = "support/reports.rs"]
mod reports;

use hardgate::GateReport;
use reports::failing_report;

#[test]
fn test_terminal_pass_report() {
    colored::control::set_override(false);
    let mut report = GateReport::new("demo".to_string());
    report.finalize(10, 50, 42);

    assert!(report.passed);
    let term = report.render_terminal();
    for needle in [
        "hardgate [demo]",
        "pass",
        "summary: 10 files, 50 functions in 42ms",
        "result: pass",
    ] {
        assert!(term.contains(needle), "missing {needle}");
    }
    assert!(!term.contains("error["));
    assert!(!term.contains("fail"));
}

#[test]
fn test_terminal_fail_report() {
    colored::control::set_override(false);
    let mut report = failing_report();
    report.finalize(3, 12, 7);

    assert!(!report.passed);
    let term = report.render_terminal();
    for needle in [
        "error[complexity]",
        "error[file-budget]",
        "-->",
        "help:",
        "src/main.rs",
        "src/big.rs",
        "result: fail (2 errors)",
    ] {
        assert!(term.contains(needle), "missing {needle}");
    }
    assert!(!term.contains("result: pass"));
}
