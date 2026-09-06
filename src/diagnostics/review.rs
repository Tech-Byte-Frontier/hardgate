use super::GateReport;
use crate::engines::{ComplexityViolation, complexity::SizeBreakdown};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Related metric findings at one original function span. These observations
/// identify review work; they are not a count of independent proven bugs.
#[derive(Serialize)]
pub struct FunctionReview<'a> {
    pub file: &'a Path,
    pub function_name: &'a str,
    pub line: usize,
    pub end_line: usize,
    pub column: Option<usize>,
    pub size: Option<&'a SizeBreakdown>,
    pub metrics: Vec<&'a ComplexityViolation>,
}

impl FunctionReview<'_> {
    pub(crate) fn location(&self) -> String {
        let column = self
            .column
            .map(|column| format!(":{column}"))
            .unwrap_or_default();
        format!("{}:{}{column}", self.file.display(), self.line)
    }
}

impl GateReport {
    pub fn function_reviews(&self) -> Vec<FunctionReview<'_>> {
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        for finding in &self.complexity_violations {
            groups
                .entry((
                    &finding.file,
                    finding.line_number,
                    finding.column_number,
                    finding.end_line,
                    &finding.function_name,
                ))
                .or_default()
                .push(finding);
        }
        groups
            .into_iter()
            .map(
                |((file, line, column, end_line, name), metrics)| FunctionReview {
                    file,
                    function_name: name,
                    line,
                    end_line,
                    column: (column > 0).then_some(column),
                    size: metrics[0].size.as_ref(),
                    metrics,
                },
            )
            .collect()
    }

    pub(crate) fn file_size_description(&self, file: &Path) -> Option<String> {
        let descriptions = self
            .file_sizes
            .iter()
            .filter(|entry| entry.file == file)
            .map(|entry| format!("{:?}: {}", entry.role, entry.size.description()))
            .collect::<Vec<_>>();
        (!descriptions.is_empty()).then(|| descriptions.join("; "))
    }
}
