use super::lcov_details::{DetailValidation, RecordDetails};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Coverage counters and line-hit data for one LCOV source record.
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
        coverage_percent(self.lines_hit, self.lines_found)
    }

    pub fn function_coverage_percent(&self) -> f64 {
        coverage_percent(self.functions_hit, self.functions_found)
    }

    pub fn branch_coverage_percent(&self) -> f64 {
        coverage_percent(self.branches_hit, self.branches_found)
    }
}

fn coverage_percent(hit: usize, found: usize) -> f64 {
    if found == 0 {
        0.0
    } else {
        (hit as f64 / found as f64) * 100.0
    }
}

/// Parse an LCOV report with the configured metric requirements.
pub(crate) fn parse_report(
    report_path: &Path,
    require_functions: bool,
    require_branches: bool,
) -> Result<HashMap<PathBuf, FileCoverage>> {
    let content = fs::read_to_string(report_path)
        .with_context(|| format!("Unable to read LCOV report `{}`", report_path.display()))?;
    if content.trim().is_empty() {
        bail!("LCOV report is empty");
    }

    let mut records = LcovRecords::new(require_functions, require_branches);
    for (line_number, line) in content.lines().enumerate() {
        records
            .ingest(line.trim())
            .with_context(|| format!("Invalid LCOV record at line {}", line_number + 1))?;
    }
    records.finish()
}

#[derive(Default)]
struct LcovRecords {
    completed: HashMap<PathBuf, FileCoverage>,
    seen_paths: HashSet<String>,
    current: Option<RecordBuilder>,
    require_functions: bool,
    require_branches: bool,
}

impl LcovRecords {
    fn new(require_functions: bool, require_branches: bool) -> Self {
        Self {
            require_functions,
            require_branches,
            ..Default::default()
        }
    }

    fn ingest(&mut self, line: &str) -> Result<()> {
        reject_malformed_marker(line)?;
        if line.is_empty() || line.starts_with('#') {
            if let Some(current) = self.current.as_mut() {
                current.saw_metric = true;
            }
            return Ok(());
        }
        if self.ingest_metadata(line)? {
            return Ok(());
        }
        if let Some(path) = line.strip_prefix("SF:") {
            return self.start(path);
        }
        if line == "end_of_record" {
            return self.end();
        }
        if let Some(tag) = metric_tag(line) {
            let Some(current) = self.current.as_mut() else {
                bail!("LCOV {tag} metric appears outside a source record");
            };
            current.ingest_metric(tag, line)?;
        } else {
            return reject_unknown_tag(line);
        }
        Ok(())
    }

    fn ingest_metadata(&mut self, line: &str) -> Result<bool> {
        if line == "TN" {
            bail!("Malformed LCOV TN metadata");
        }
        if line.starts_with("TN:") {
            if self.current.is_some() {
                bail!("LCOV TN metadata must appear outside a source record");
            }
            return Ok(true);
        }
        if line == "VER" {
            bail!("Malformed LCOV VER metadata");
        }
        let Some(value) = line.strip_prefix("VER:") else {
            return Ok(false);
        };
        let Some(current) = self.current.as_mut() else {
            bail!("LCOV VER metadata must immediately follow SF");
        };
        current.ingest_version(value)?;
        Ok(true)
    }

    fn start(&mut self, path: &str) -> Result<()> {
        if self.current.is_some() {
            bail!("LCOV record started before the previous record ended");
        }
        let path = path.trim();
        if path.is_empty() {
            bail!("LCOV source path is empty");
        }
        if path.contains('\0') {
            bail!("LCOV source path contains a NUL byte");
        }
        self.current = Some(RecordBuilder::new(PathBuf::from(path)));
        Ok(())
    }

    fn end(&mut self) -> Result<()> {
        let Some(current) = self.current.take() else {
            bail!("LCOV record ended without a matching source record");
        };
        let coverage = current.finish(self.require_functions, self.require_branches)?;
        let path_key = lexical_record_key(&coverage.file_path);
        if self.completed.contains_key(&coverage.file_path) || !self.seen_paths.insert(path_key) {
            bail!(
                "LCOV source record is duplicated for `{}`",
                coverage.file_path.display()
            );
        }
        self.completed.insert(coverage.file_path.clone(), coverage);
        Ok(())
    }

    fn finish(self) -> Result<HashMap<PathBuf, FileCoverage>> {
        if self.current.is_some() {
            bail!("LCOV report ended before `end_of_record`");
        }
        if self.completed.is_empty() {
            bail!("LCOV report contains no source records");
        }
        Ok(self.completed)
    }
}

fn reject_malformed_marker(line: &str) -> Result<()> {
    if line == "SF" || (line.starts_with("end_of_record") && line != "end_of_record") {
        bail!("Malformed LCOV record marker `{line}`");
    }
    Ok(())
}

fn reject_unknown_tag(line: &str) -> Result<()> {
    let tag = line.split_once(':').map_or(line, |(tag, _)| tag);
    bail!("Unsupported or malformed LCOV tag `{tag}`")
}

struct RecordBuilder {
    coverage: FileCoverage,
    seen_counts: HashSet<&'static str>,
    seen_da: HashSet<usize>,
    details: RecordDetails,
    saw_version: bool,
    saw_metric: bool,
}

impl RecordBuilder {
    fn new(file_path: PathBuf) -> Self {
        Self {
            coverage: FileCoverage {
                file_path,
                ..Default::default()
            },
            seen_counts: HashSet::new(),
            seen_da: HashSet::new(),
            details: RecordDetails::default(),
            saw_version: false,
            saw_metric: false,
        }
    }

    fn ingest_version(&mut self, value: &str) -> Result<()> {
        if self.saw_version || self.saw_metric || value.trim().is_empty() {
            bail!("LCOV VER must be non-empty and immediately follow SF");
        }
        self.saw_version = true;
        Ok(())
    }

    fn ingest_metric(&mut self, tag: &'static str, line: &str) -> Result<()> {
        self.saw_metric = true;
        let rest = line
            .strip_prefix(tag)
            .and_then(|value| value.strip_prefix(':'))
            .ok_or_else(|| anyhow::anyhow!("Malformed LCOV {tag} metric `{line}`"))?;
        match tag {
            "DA" => self.ingest_da(rest),
            "LF" | "LH" | "FNF" | "FNH" | "BRF" | "BRH" => self.ingest_count(tag, rest),
            // Function/branch detail records are not needed for scoring, but
            // they are still recognized metrics and must remain record-bound.
            "FN" => self.details.ingest_fn(rest),
            "FNDA" => self.details.ingest_fnda(rest),
            "BRDA" => self.details.ingest_brda(rest),
            "FNL" | "FNA" | "MCDC" | "MRF" | "MRH" => {
                bail!("Unsupported LCOV {tag} metric")
            }
            _ => unreachable!("metric_tag only returns recognized metrics"),
        }
    }

    fn ingest_da(&mut self, rest: &str) -> Result<()> {
        let fields: Vec<_> = rest.split(',').collect();
        if !(2..=3).contains(&fields.len()) || fields.iter().any(|field| field.trim().is_empty()) {
            bail!("Malformed LCOV DA record `{rest}`");
        }
        let line_number = fields[0]
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Malformed LCOV DA line number `{}`", fields[0]))?;
        if line_number == 0 {
            bail!("LCOV DA line number must be greater than zero");
        }
        let hits = fields[1]
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Malformed LCOV DA hit count `{}`", fields[1]))?;
        if !self.seen_da.insert(line_number) {
            bail!("Duplicate LCOV DA line `{line_number}`");
        }
        self.coverage.line_hits.insert(line_number, hits);
        Ok(())
    }

    fn ingest_count(&mut self, tag: &'static str, rest: &str) -> Result<()> {
        ensure_unique_count(&mut self.seen_counts, tag)?;
        let value = rest
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Malformed LCOV {tag} count `{rest}`"))?;
        let setter = count_setter(tag).expect("ingest_count receives a known count tag");
        setter(&mut self.coverage, value);
        Ok(())
    }

    fn finish(self, require_functions: bool, require_branches: bool) -> Result<FileCoverage> {
        validate_lines(&self)?;
        validate_function_counts(&self, require_functions)?;
        validate_branch_counts(&self, require_branches)?;
        self.details.validate(DetailValidation {
            seen_counts: &self.seen_counts,
            functions_found: self.coverage.functions_found,
            functions_hit: self.coverage.functions_hit,
            require_functions,
            require_branches,
        })?;
        Ok(self.coverage)
    }
}

type CountSetter = fn(&mut FileCoverage, usize);

const COUNT_SETTERS: &[(&str, CountSetter)] = &[
    ("LF", |coverage, value| coverage.lines_found = value),
    ("LH", |coverage, value| coverage.lines_hit = value),
    ("FNF", |coverage, value| coverage.functions_found = value),
    ("FNH", |coverage, value| coverage.functions_hit = value),
    ("BRF", |coverage, value| coverage.branches_found = value),
    ("BRH", |coverage, value| coverage.branches_hit = value),
];

fn count_setter(tag: &str) -> Option<CountSetter> {
    COUNT_SETTERS
        .iter()
        .find_map(|(candidate, setter)| (*candidate == tag).then_some(*setter))
}

fn ensure_unique_count(seen_counts: &mut HashSet<&'static str>, tag: &'static str) -> Result<()> {
    if seen_counts.insert(tag) {
        Ok(())
    } else {
        bail!("Duplicate LCOV {tag} count")
    }
}

fn validate_lines(builder: &RecordBuilder) -> Result<()> {
    require_counts(&builder.seen_counts, &["LF", "LH"], "LF/LH line counts")?;
    if builder.coverage.line_hits.is_empty() {
        bail!("LCOV source record contains no DA line data");
    }
    let detailed_lines = builder.coverage.line_hits.len();
    if builder.coverage.lines_found < detailed_lines {
        bail!(
            "LCOV LF:{} is less than {} unique DA lines",
            builder.coverage.lines_found,
            detailed_lines
        );
    }
    let hit_lines = builder
        .coverage
        .line_hits
        .values()
        .try_fold(0usize, |count, hits| {
            count.checked_add(usize::from(*hits > 0))
        })
        .ok_or_else(|| anyhow::anyhow!("LCOV hit-line count overflow"))?;
    if builder.coverage.lines_hit > builder.coverage.lines_found {
        bail!(
            "LCOV LH:{} exceeds LF:{}",
            builder.coverage.lines_hit,
            builder.coverage.lines_found
        );
    }
    if builder.coverage.lines_found == detailed_lines && builder.coverage.lines_hit != hit_lines {
        bail!(
            "LCOV LH:{} does not match {} DA lines with hits",
            builder.coverage.lines_hit,
            hit_lines
        );
    }
    Ok(())
}

fn validate_function_counts(builder: &RecordBuilder, required: bool) -> Result<()> {
    validate_counter_pair(
        builder,
        required,
        CounterPair {
            found_tag: "FNF",
            hit_tag: "FNH",
            label: "function counts",
            required_label: "required FNF/FNH counts",
            found: builder.coverage.functions_found,
            hit: builder.coverage.functions_hit,
            exceeds: "LCOV FNH exceeds FNF",
        },
    )
}

fn validate_branch_counts(builder: &RecordBuilder, required: bool) -> Result<()> {
    validate_counter_pair(
        builder,
        required,
        CounterPair {
            found_tag: "BRF",
            hit_tag: "BRH",
            label: "branch counts",
            required_label: "required BRF/BRH counts",
            found: builder.coverage.branches_found,
            hit: builder.coverage.branches_hit,
            exceeds: "LCOV BRH exceeds BRF",
        },
    )
}

struct CounterPair {
    found_tag: &'static str,
    hit_tag: &'static str,
    label: &'static str,
    required_label: &'static str,
    found: usize,
    hit: usize,
    exceeds: &'static str,
}

fn validate_counter_pair(builder: &RecordBuilder, required: bool, pair: CounterPair) -> Result<()> {
    validate_pair(
        &builder.seen_counts,
        pair.found_tag,
        pair.hit_tag,
        pair.label,
    )?;
    if required {
        require_counts(
            &builder.seen_counts,
            &[pair.found_tag, pair.hit_tag],
            pair.required_label,
        )?;
    }
    if pair.hit > pair.found {
        bail!("{}", pair.exceeds);
    }
    Ok(())
}

fn validate_pair(
    seen_counts: &HashSet<&'static str>,
    first: &'static str,
    second: &'static str,
    label: &str,
) -> Result<()> {
    if seen_counts.contains(first) == seen_counts.contains(second) {
        Ok(())
    } else {
        bail!("LCOV {first}/{second} {label} must be paired")
    }
}

fn require_counts(
    seen_counts: &HashSet<&'static str>,
    tags: &[&'static str],
    label: &str,
) -> Result<()> {
    if tags.iter().all(|tag| seen_counts.contains(tag)) {
        Ok(())
    } else {
        bail!("LCOV source record is missing {label}")
    }
}

fn metric_tag(line: &str) -> Option<&'static str> {
    let tag = line.split_once(':').map_or(line, |(tag, _)| tag);
    [
        "DA", "LF", "LH", "FN", "FNDA", "FNF", "FNH", "BRDA", "BRF", "BRH", "FNL", "FNA", "MCDC",
        "MRF", "MRH",
    ]
    .into_iter()
    .find(|candidate| *candidate == tag)
}

fn lexical_record_key(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let absolute = raw.starts_with('/');
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}
