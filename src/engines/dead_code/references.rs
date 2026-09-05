use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Occurrence<'a> {
    first_file: &'a Path,
    multiple_files: bool,
}

/// Index Unicode words once. This preserves the existing word-boundary
/// heuristic, including references in comments/strings, without scanning every
/// repository byte separately for each exported symbol.
pub(super) struct ReferenceIndex<'a> {
    words: HashMap<&'a str, Occurrence<'a>>,
}

impl<'a> ReferenceIndex<'a> {
    pub(super) fn build(contents: &'a [(PathBuf, &'a str)]) -> Self {
        static WORD: OnceLock<Regex> = OnceLock::new();
        let word = WORD.get_or_init(|| Regex::new(r"\w+").expect("valid word regex"));
        let mut words: HashMap<&str, Occurrence<'_>> = HashMap::new();
        for (path, content) in contents {
            for token in word.find_iter(content) {
                words
                    .entry(token.as_str())
                    .and_modify(|seen| {
                        seen.multiple_files |= seen.first_file != path;
                    })
                    .or_insert(Occurrence {
                        first_file: path,
                        multiple_files: false,
                    });
            }
        }
        Self { words }
    }

    pub(super) fn is_referenced(&self, symbol: &str, current_file: &Path) -> bool {
        symbol == "default"
            || symbol.starts_with('_')
            || self
                .words
                .get(symbol)
                .is_some_and(|seen| seen.multiple_files || seen.first_file != current_file)
    }
}
