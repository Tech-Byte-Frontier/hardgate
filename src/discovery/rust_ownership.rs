//! Syntactic Rust test ownership for source policy, not dependency resolution.
mod cfg;
mod syntax;
mod targets;

use super::{ClassifiedFile, FileRole};
use crate::engines::complexity::SupportedLanguage;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

const PRODUCTION: u8 = 1;
const TEST: u8 = 2;

#[derive(Default)]
pub(crate) struct RustOwnership {
    files: BTreeMap<PathBuf, OwnedFile>,
}

struct OwnedFile {
    syntax: syntax::Syntax,
    reach: u8,
    explicit: bool,
}

pub(crate) struct RoleView {
    pub file: ClassifiedFile,
    pub text: String,
    pub syntax_source: Option<String>,
    pub bytes: usize,
    pub lines: usize,
}

impl RustOwnership {
    pub(crate) fn context_path(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
            || path.file_name().is_some_and(|name| name == "Cargo.toml")
    }

    pub(crate) fn from_root(
        root: &Path,
        config: &crate::config::HardgateConfig,
        replacements: &[(&ClassifiedFile, &str)],
    ) -> anyhow::Result<Self> {
        let discovered = super::discover_paths(super::DiscoverOptions {
            root,
            diff_only: false,
            exclusions: &[],
        })?;
        let classifier = super::classification::PreparedClassifier::new(&config.classification)?;
        let mut sources = BTreeMap::new();
        for path in discovered
            .files
            .into_iter()
            .filter(|path| Self::context_path(path))
        {
            let mut file = classifier.classify(path.strip_prefix(root).unwrap_or(&path));
            file.path = path.clone();
            let source = std::fs::read_to_string(&path)?;
            sources.insert(normalize(&path), (file, source));
        }
        for (file, source) in replacements {
            sources.insert(normalize(&file.path), ((*file).clone(), source.to_string()));
        }
        Ok(Self::from_inputs(
            &sources
                .values()
                .map(|(file, source)| (file, source.as_str()))
                .collect::<Vec<_>>(),
        ))
    }

    // Missing context must not turn a potentially shared module into test code.
    pub(crate) fn disable_module_proof(&mut self) {
        for file in self.files.values_mut() {
            if !file.syntax.file_test {
                file.reach |= PRODUCTION;
            }
        }
    }

    pub(crate) fn has_role(&self, file: &ClassifiedFile, role: FileRole) -> bool {
        self.file_role(file) == role
            || (role == FileRole::Test
                && file.role == FileRole::Source
                && self
                    .files
                    .get(&normalize(&file.path))
                    .is_some_and(|owned| !owned.explicit && !owned.syntax.test_ranges.is_empty()))
    }

    pub(crate) fn test_span(&self, file: &ClassifiedFile, span: Range<usize>) -> bool {
        self.file_role(file) == FileRole::Test
            || self.files.get(&normalize(&file.path)).is_some_and(|owned| {
                !owned.explicit
                    && owned
                        .syntax
                        .test_ranges
                        .iter()
                        .any(|range| range.start < span.end && span.start < range.end)
            })
    }

    /// One entry per physical line: test, production, or ambiguous (`None`).
    /// Compute once before scoring so line lookup stays constant-time.
    pub(crate) fn line_roles(&self, file: &ClassifiedFile, source: &str) -> Vec<Option<bool>> {
        if self.file_role(file) == FileRole::Test {
            return source.lines().map(|_| Some(true)).collect();
        }
        let ranges = self
            .files
            .get(&normalize(&file.path))
            .filter(|owned| !owned.explicit)
            .map(|owned| owned.syntax.test_ranges.as_slice())
            .unwrap_or(&[]);
        let mut offset = 0;
        let mut range_index = 0;
        source
            .split_inclusive('\n')
            .map(|text| {
                let mut production = false;
                let mut testing = false;
                for (index, byte) in text.bytes().enumerate() {
                    while ranges
                        .get(range_index)
                        .is_some_and(|range| offset + index >= range.end)
                    {
                        range_index += 1;
                    }
                    if byte.is_ascii_whitespace() {
                        continue;
                    }
                    if ranges
                        .get(range_index)
                        .is_some_and(|range| range.contains(&(offset + index)))
                    {
                        testing = true;
                    } else {
                        production = true;
                    }
                }
                offset += text.len();
                if production && testing {
                    None
                } else {
                    Some(testing)
                }
            })
            .collect()
    }

    pub(crate) fn from_inputs(inputs: &[(&ClassifiedFile, &str)]) -> Self {
        let roots = targets::cargo_roots(inputs);
        let mut incomplete = inputs.iter().any(|(file, source)| {
            file.path
                .file_name()
                .is_some_and(|name| name == "Cargo.toml")
                && toml::from_str::<toml::Value>(source).is_err()
        });
        let mut result = Self::default();
        let mut declared = BTreeMap::new();
        for (file, source) in inputs {
            if file
                .path
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("rs"))
            {
                continue;
            }
            let Ok(Some((SupportedLanguage::Rust, tree))) =
                SupportedLanguage::parse_file_checked(&file.path, source)
            else {
                incomplete = true;
                continue;
            };
            let path = normalize(&file.path);
            let is_root = roots.contains_key(&path) || conventional_root(&path);
            let syntax = syntax::analyze(&tree, source, &path, is_root);
            if file.role == FileRole::Test {
                declared.insert(path.clone(), TEST);
            }
            result.files.insert(
                path,
                OwnedFile {
                    syntax,
                    reach: 0,
                    explicit: file.reason.starts_with("custom classification rule "),
                },
            );
        }
        result.resolve(roots, declared);
        if incomplete
            || result
                .files
                .values()
                .any(|file| file.reach & PRODUCTION != 0 && file.syntax.uncertain_modules)
        {
            result.disable_module_proof();
        }
        result
    }

    fn resolve(&mut self, roots: BTreeMap<PathBuf, u8>, declared: BTreeMap<PathBuf, u8>) {
        let mut edges = BTreeMap::<PathBuf, Vec<(PathBuf, bool)>>::new();
        let mut incoming = BTreeSet::new();
        for (path, file) in &self.files {
            for module in &file.syntax.modules {
                let candidates = module
                    .candidates
                    .iter()
                    .map(|path| normalize(path))
                    .filter(|path| self.files.contains_key(path))
                    .collect::<Vec<_>>();
                if candidates.len() != 1 {
                    continue;
                }
                let target = candidates[0].clone();
                incoming.insert(target.clone());
                edges
                    .entry(path.clone())
                    .or_default()
                    .push((target, module.test_only));
            }
        }
        let mut pending = VecDeque::new();
        for (path, file) in &self.files {
            let role = roots
                .get(path)
                .copied()
                .or_else(|| declared.get(path).copied())
                .or_else(|| {
                    (!incoming.contains(path) || conventional_root(path)).then_some(PRODUCTION)
                });
            if let Some(role) = role {
                pending.push_back((path.clone(), role));
            }
            if file.syntax.file_test {
                pending.push_back((path.clone(), TEST));
            }
        }
        self.propagate(&edges, pending);
        // A disconnected cycle is not proof of test-only ownership.
        let unresolved = self
            .files
            .iter()
            .filter(|(_, file)| file.reach == 0)
            .map(|(path, _)| (path.clone(), PRODUCTION))
            .collect();
        self.propagate(&edges, unresolved);
    }

    fn propagate(
        &mut self,
        edges: &BTreeMap<PathBuf, Vec<(PathBuf, bool)>>,
        mut pending: VecDeque<(PathBuf, u8)>,
    ) {
        while let Some((path, mut role)) = pending.pop_front() {
            let Some(file) = self.files.get_mut(&path) else {
                continue;
            };
            if file.syntax.file_test {
                role = TEST;
            }
            if file.reach & role == role {
                continue;
            }
            file.reach |= role;
            for (target, test_only) in edges.get(&path).into_iter().flatten() {
                pending.push_back((target.clone(), if *test_only { TEST } else { role }));
            }
        }
    }

    pub(crate) fn file_role(&self, file: &ClassifiedFile) -> FileRole {
        if file.role == FileRole::Source
            && self
                .files
                .get(&normalize(&file.path))
                .is_some_and(|owned| owned.reach == TEST && !owned.explicit)
        {
            FileRole::Test
        } else {
            file.role
        }
    }

    pub(crate) fn views(&self, file: &ClassifiedFile, source: &str) -> Vec<RoleView> {
        let role = self.file_role(file);
        let ranges = self
            .files
            .get(&normalize(&file.path))
            .filter(|owned| !owned.explicit)
            .map(|owned| owned.syntax.test_ranges.as_slice())
            .unwrap_or(&[]);
        if role != FileRole::Source || ranges.is_empty() {
            let mut file = file.clone();
            file.role = role;
            return vec![RoleView {
                file,
                text: source.to_string(),
                syntax_source: None,
                bytes: source.len(),
                lines: source.lines().count(),
            }];
        }
        [FileRole::Source, FileRole::Test]
            .into_iter()
            .filter_map(|role| mask(file, source, ranges, role))
            .collect()
    }
}

fn mask(
    file: &ClassifiedFile,
    source: &str,
    ranges: &[Range<usize>],
    role: FileRole,
) -> Option<RoleView> {
    let mut text = source.as_bytes().to_vec();
    let mut size = RoleSize::default();
    let mut range_index = 0;
    for (index, byte) in source.bytes().enumerate() {
        while ranges
            .get(range_index)
            .is_some_and(|range| index >= range.end)
        {
            range_index += 1;
        }
        let testing = ranges
            .get(range_index)
            .is_some_and(|range| range.contains(&index));
        let selected = testing == (role == FileRole::Test);
        size.include(byte, selected);
        if !selected && byte != b'\n' && byte != b'\r' {
            text[index] = b' ';
        }
    }
    size.finish_line();
    if size.bytes == 0 {
        return None;
    }
    let mut file = file.clone();
    file.role = role;
    Some(RoleView {
        file,
        text: String::from_utf8(text).expect("mask replaces whole UTF-8 ranges"),
        syntax_source: Some(source.to_string()),
        bytes: size.bytes,
        lines: size.lines,
    })
}

#[derive(Default)]
struct RoleSize {
    bytes: usize,
    lines: usize,
    occupied: bool,
    substantive: bool,
    included: bool,
}

impl RoleSize {
    fn include(&mut self, byte: u8, selected: bool) {
        if selected {
            self.bytes += 1;
            self.included = true;
            self.occupied |= !byte.is_ascii_whitespace();
        }
        self.substantive |= !byte.is_ascii_whitespace();
        if byte == b'\n' {
            self.finish_line();
        }
    }

    fn finish_line(&mut self) {
        self.lines += usize::from(self.occupied || (!self.substantive && self.included));
        self.occupied = false;
        self.substantive = false;
        self.included = false;
    }
}

fn conventional_root(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("lib.rs" | "main.rs")
    )
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if result.file_name().is_some() => {
                result.pop();
            }
            part => result.push(part.as_os_str()),
        }
    }
    result
}
