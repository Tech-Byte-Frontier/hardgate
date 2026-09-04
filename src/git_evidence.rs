use crate::discovery::{SKIPPED_DIRS, is_inventory_file};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
pub type ChangedLineMap = BTreeMap<PathBuf, BTreeSet<usize>>;
pub type ChangedLines = ChangedLineMap;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub merge_base: String,
    pub changed_lines: ChangedLineMap,
    pub changed_files: BTreeSet<PathBuf>,
    pub rename_lineage: BTreeMap<PathBuf, PathBuf>,
}
impl ChangeSet {
    pub fn touches(&self, path: &Path, start: usize, end: usize) -> bool {
        touches(&self.changed_lines, path, start, end)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySnapshot {
    pub commit: String,
    pub files: BTreeMap<PathBuf, String>,
}
impl RepositorySnapshot {
    pub fn get(&self, path: &Path) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEvidence {
    pub change_set: ChangeSet,
    pub snapshot: RepositorySnapshot,
}
pub type GitEvidence = ReferenceEvidence;
pub fn load_reference(root: &Path, reference: &str) -> Result<ReferenceEvidence> {
    let merge_base = resolve_merge_base(root, reference)?;
    let status = parse_status(&run_git(
        root,
        &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
    )?)?;
    let rename_lineage = parse_diff_status(&run_git(
        root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            &merge_base,
            "--",
        ],
    )?)?;
    let diff = run_git(
        root,
        &[
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            &merge_base,
            "--",
        ],
    )?;
    let mut changed_lines = parse_diff(&diff)?;
    let mut changed_files = status.inventory_paths.clone();
    add_untracked_lines(
        root,
        &status.untracked,
        &mut changed_lines,
        &mut changed_files,
    )?;
    validate_worktree_inventory(root, &status)?;
    changed_files.extend(changed_lines.keys().cloned());
    let snapshot = load_snapshot(root, &merge_base)?;
    Ok(ReferenceEvidence {
        change_set: ChangeSet {
            merge_base,
            changed_lines,
            changed_files,
            rename_lineage,
        },
        snapshot,
    })
}
pub fn touches(map: &ChangedLineMap, path: &Path, start: usize, end: usize) -> bool {
    if start > end {
        return false;
    }
    let Some(lines) = map.get(path) else {
        return false;
    };
    lines.range(start..=end).next().is_some()
}
fn resolve_merge_base(root: &Path, reference: &str) -> Result<String> {
    if reference.is_empty() || reference.starts_with('-') {
        bail!("Git reference must be a non-empty ref name");
    }
    let output = run_git(root, &["merge-base", "HEAD", reference])?;
    let text = String::from_utf8(output).context("Git returned a non-UTF-8 merge base")?;
    let commit = text.trim();
    if commit.is_empty() || commit.contains(char::is_whitespace) || !is_hex_hash(commit) {
        bail!("Git returned a malformed merge-base commit");
    }
    Ok(commit.to_string())
}
fn is_hex_hash(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("Failed to execute `git {}`", args.join(" ")))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        bail!("`git {}` failed: {}", args.join(" "), error.trim());
    }
    Ok(output.stdout)
}
#[derive(Default)]
struct StatusSummary {
    inventory_paths: BTreeSet<PathBuf>,
    untracked: BTreeSet<PathBuf>,
}
fn parse_status(output: &[u8]) -> Result<StatusSummary> {
    let records = split_nul_records(output, "Git status")?;
    let mut summary = StatusSummary::default();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.len() < 4 || record[2] != b' ' {
            bail!("Git returned a malformed porcelain status record");
        }
        let marker = [record[0], record[1]];
        let path = normalize_bytes(&record[3..], "Git status path")?;
        if is_inventory_path(&path) {
            summary.inventory_paths.insert(path.clone());
            if marker == *b"??" {
                summary.untracked.insert(path);
            }
        }
        if marker.contains(&b'R') || marker.contains(&b'C') {
            index += 1;
            if index >= records.len() {
                bail!("Git returned a malformed rename status record");
            }
            let _old_path = normalize_bytes(records[index], "Git rename path")?;
        }
        index += 1;
    }
    Ok(summary)
}
fn parse_diff_status(output: &[u8]) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let records = split_nul_records(output, "Git diff status")?;
    let mut lineage = BTreeMap::new();
    let mut index = 0;
    while index < records.len() {
        let status = records[index];
        if !valid_diff_status(status) {
            bail!("Git returned a malformed diff status record");
        }
        let marker = *status
            .first()
            .context("Git returned an empty diff status record")?;
        index += 1;
        if marker == b'R' || marker == b'C' {
            let old = records
                .get(index)
                .context("Git returned a malformed rename diff status")?;
            let new = records
                .get(index + 1)
                .context("Git returned a malformed rename diff status")?;
            let old = normalize_bytes(old, "Git rename baseline path")?;
            let new = normalize_bytes(new, "Git rename current path")?;
            if is_inventory_path(&old) && is_inventory_path(&new) {
                lineage.insert(new, old);
            }
            index += 2;
        } else {
            let _path = records
                .get(index)
                .context("Git returned a malformed diff status")?;
            index += 1;
        }
    }
    Ok(lineage)
}
fn valid_diff_status(status: &[u8]) -> bool {
    let Some(marker) = status.first().copied() else {
        return false;
    };
    if !matches!(
        marker,
        b'A' | b'C' | b'D' | b'M' | b'R' | b'T' | b'U' | b'X' | b'B'
    ) {
        return false;
    }
    if marker == b'R' || marker == b'C' {
        status.len() == 4 && status[1..].iter().all(u8::is_ascii_digit)
    } else {
        status.len() == 1
    }
}
fn split_nul_records<'a>(output: &'a [u8], label: &str) -> Result<Vec<&'a [u8]>> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    if !output.ends_with(&[0]) {
        bail!("{label} output was not NUL terminated");
    }
    Ok(output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect())
}
fn normalize_bytes(bytes: &[u8], label: &str) -> Result<PathBuf> {
    let text =
        String::from_utf8(bytes.to_vec()).with_context(|| format!("{label} was not UTF-8"))?;
    normalize_path(&text)
}
fn normalize_path(raw: &str) -> Result<PathBuf> {
    if raw.is_empty() {
        bail!("Git returned an empty path");
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        bail!("Git returned an absolute path: {raw}");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("Git returned a non-relative path: {raw}");
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("Git returned an empty path");
    }
    Ok(normalized)
}
fn is_inventory_path(path: &Path) -> bool {
    !in_skipped_dir(path) && is_inventory_file(path)
}
fn in_skipped_dir(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| SKIPPED_DIRS.contains(&part))
    })
}
fn add_untracked_lines(
    root: &Path,
    paths: &BTreeSet<PathBuf>,
    changed_lines: &mut ChangedLineMap,
    changed_files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for path in paths {
        let content = fs::read(root.join(path)).with_context(|| {
            format!(
                "Unable to read untracked inventory file `{}`",
                path.display()
            )
        })?;
        let text = String::from_utf8(content).with_context(|| {
            format!(
                "Untracked inventory file `{}` was not UTF-8",
                path.display()
            )
        })?;
        changed_files.insert(path.clone());
        let count = text.lines().count();
        let lines = changed_lines.entry(path.clone()).or_default();
        for line in 1..=count {
            lines.insert(line);
        }
    }
    Ok(())
}
fn validate_worktree_inventory(root: &Path, status: &StatusSummary) -> Result<()> {
    for path in &status.inventory_paths {
        if status.untracked.contains(path) {
            continue;
        }
        let target = root.join(path);
        if !target.exists() {
            continue;
        }
        let content = fs::read(target).with_context(|| {
            format!("Unable to read changed inventory file `{}`", path.display())
        })?;
        String::from_utf8(content).with_context(|| {
            format!("Changed inventory file `{}` was not UTF-8", path.display())
        })?;
    }
    Ok(())
}
fn parse_diff(output: &[u8]) -> Result<ChangedLineMap> {
    let text = String::from_utf8(output.to_vec()).context("Git diff output was not UTF-8")?;
    let mut parser = DiffParser::default();
    for line in text.split('\n') {
        parser.consume(line)?;
    }
    Ok(parser.lines)
}
#[derive(Default)]
struct DiffParser {
    old_path: Option<PathBuf>,
    new_path: Option<PathBuf>,
    in_hunk: bool,
    lines: ChangedLineMap,
}
impl DiffParser {
    fn consume(&mut self, line: &str) -> Result<()> {
        if line.starts_with("diff --git ") {
            self.old_path = None;
            self.new_path = None;
            self.in_hunk = false;
            return Ok(());
        }
        if !self.in_hunk {
            if let Some(rest) = line.strip_prefix("--- ") {
                self.old_path = parse_diff_path(rest, "a/")?;
                return Ok(());
            }
            if let Some(rest) = line.strip_prefix("+++ ") {
                self.new_path = parse_diff_path(rest, "b/")?;
                return Ok(());
            }
        }
        if line.starts_with("@@") {
            let (start, count) = parse_hunk_header(line)?;
            let path = self
                .new_path
                .as_ref()
                .or(self.old_path.as_ref())
                .context("Git hunk had no file path")?;
            self.in_hunk = true;
            if is_inventory_path(path) {
                add_hunk_lines(&mut self.lines, path, start, count)?;
            }
        }
        Ok(())
    }
}
fn parse_hunk_header(line: &str) -> Result<(usize, usize)> {
    let body = line
        .strip_prefix("@@ ")
        .and_then(|value| value.split_once(" @@").map(|(range, _)| range))
        .context("Git returned a malformed diff hunk header")?;
    let mut ranges = body.split_whitespace();
    let _old = ranges
        .next()
        .and_then(|value| value.strip_prefix('-'))
        .and_then(parse_range)
        .context("Git returned a malformed old diff range")?;
    let new = ranges
        .next()
        .and_then(|value| value.strip_prefix('+'))
        .and_then(parse_range)
        .context("Git returned a malformed new diff range")?;
    if ranges.next().is_some() {
        bail!("Git returned a malformed diff hunk header");
    }
    Ok(new)
}
fn parse_range(value: &str) -> Option<(usize, usize)> {
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}
fn add_hunk_lines(map: &mut ChangedLineMap, path: &Path, start: usize, count: usize) -> Result<()> {
    let first = if count == 0 { start.max(1) } else { start };
    if count > 0 && first == 0 {
        bail!("Git returned a zero-based non-empty diff range");
    }
    let last = if count == 0 {
        first
    } else {
        first
            .checked_add(count - 1)
            .context("Git diff hunk line range overflowed")?
    };
    map.entry(path.to_path_buf())
        .or_default()
        .extend(first..=last);
    Ok(())
}
fn parse_diff_path(rest: &str, prefix: &str) -> Result<Option<PathBuf>> {
    let token = if rest.starts_with('"') {
        let (decoded, consumed) = decode_quoted_path(rest)?;
        let trailing = &rest[consumed..];
        if !trailing.is_empty() && trailing != "\t" {
            bail!("Git returned a malformed quoted diff path");
        }
        decoded
    } else {
        rest.strip_suffix('\t').unwrap_or(rest).to_string()
    };
    if token == "/dev/null" {
        return Ok(None);
    }
    let path = token
        .strip_prefix(prefix)
        .with_context(|| format!("Git diff path lacked `{prefix}` prefix"))?;
    Ok(Some(normalize_path(path)?))
}
fn decode_quoted_path(input: &str) -> Result<(String, usize)> {
    let bytes = input.as_bytes();
    let mut output = Vec::new();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                let text = String::from_utf8(output).context("Quoted Git path was not UTF-8")?;
                return Ok((text, index + 1));
            }
            b'\\' => index = decode_escape(bytes, index, &mut output)?,
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    bail!("Git returned an unterminated quoted diff path")
}
fn decode_escape(bytes: &[u8], slash: usize, output: &mut Vec<u8>) -> Result<usize> {
    let next = *bytes
        .get(slash + 1)
        .context("Git returned an incomplete quoted path escape")?;
    if let Some(byte) = simple_escape(next) {
        output.push(byte);
        return Ok(slash + 2);
    }
    if !(b'0'..=b'7').contains(&next) {
        bail!("Git returned an unknown quoted path escape");
    }
    decode_octal_escape(bytes, slash, output)
}
fn simple_escape(byte: u8) -> Option<u8> {
    const ESCAPES: &[(u8, u8)] = &[
        (b'"', b'"'),
        (b'\\', b'\\'),
        (b'a', 7),
        (b'b', 8),
        (b't', 9),
        (b'n', 10),
        (b'v', 11),
        (b'f', 12),
        (b'r', 13),
    ];
    ESCAPES
        .iter()
        .find(|(escaped, _)| *escaped == byte)
        .map(|(_, value)| *value)
}
fn decode_octal_escape(bytes: &[u8], slash: usize, output: &mut Vec<u8>) -> Result<usize> {
    let mut value = 0_u8;
    let mut index = slash + 1;
    for _ in 0..3 {
        let Some(byte) = bytes.get(index) else { break };
        if !(b'0'..=b'7').contains(byte) {
            break;
        }
        value = value
            .checked_mul(8)
            .and_then(|value| value.checked_add(byte - b'0'))
            .context("Git quoted path octal escape overflowed")?;
        index += 1;
    }
    output.push(value);
    Ok(index)
}
fn load_snapshot(root: &Path, commit: &str) -> Result<RepositorySnapshot> {
    let listing = run_git(root, &["ls-tree", "-rz", "--name-only", commit, "--"])?;
    let paths = split_nul_records(&listing, "Git tree")?;
    let mut files = BTreeMap::new();
    for raw_path in paths {
        let path = normalize_bytes(raw_path, "Git tree path")?;
        if !is_inventory_path(&path) {
            continue;
        }
        let spec = format!("{commit}:{}", path.to_string_lossy());
        let blob = run_git(root, &["cat-file", "blob", &spec])?;
        let contents = String::from_utf8(blob)
            .with_context(|| format!("Baseline blob `{}` was not UTF-8", path.display()))?;
        files.insert(path, contents);
    }
    Ok(RepositorySnapshot {
        commit: commit.to_string(),
        files,
    })
}
