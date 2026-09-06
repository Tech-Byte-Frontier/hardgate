use super::GateReport;

// Category order stays fixed; source locations and metric identity break ties.
pub(super) fn sort(report: &mut GateReport) {
    report
        .budget_violations
        .sort_by(|a, b| (&a.file, &a.metric, &a.message).cmp(&(&b.file, &b.metric, &b.message)));
    report.suppression_violations.sort_by(|a, b| {
        (&a.file, a.line_number, &a.token).cmp(&(&b.file, b.line_number, &b.token))
    });
    report.complexity_violations.sort_by(|a, b| {
        (
            &a.file,
            a.line_number,
            a.column_number,
            a.end_line,
            &a.function_name,
            &a.metric,
        )
            .cmp(&(
                &b.file,
                b.line_number,
                b.column_number,
                b.end_line,
                &b.function_name,
                &b.metric,
            ))
    });
    report.invariant_violations.sort_by(|a, b| {
        (&a.file, a.line_number, &a.rule_name, &a.offending_target).cmp(&(
            &b.file,
            b.line_number,
            &b.rule_name,
            &b.offending_target,
        ))
    });
    report.clone_violations.sort_by(|a, b| {
        (&a.file_a, a.lines_a, &a.file_b, a.lines_b, &a.fingerprint).cmp(&(
            &b.file_a,
            b.lines_a,
            &b.file_b,
            b.lines_b,
            &b.fingerprint,
        ))
    });
    report.coverage_violations.sort_by(|a, b| {
        (&a.file, &a.function_name, &a.metric).cmp(&(&b.file, &b.function_name, &b.metric))
    });
    report
        .mutation_violations
        .sort_by(|a, b| (&a.report_file, &a.metric).cmp(&(&b.report_file, &b.metric)));
    report.tool_diagnostics.sort_by(|a, b| {
        (&a.file, a.line, a.column, &a.rule, &a.message)
            .cmp(&(&b.file, b.line, b.column, &b.rule, &b.message))
    });
    report
        .orchestration_violations
        .sort_by(|a, b| (&a.step, &a.command, &a.output).cmp(&(&b.step, &b.command, &b.output)));
}
