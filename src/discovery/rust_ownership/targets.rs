use super::{ClassifiedFile, PRODUCTION, TEST, normalize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Roots = BTreeMap<PathBuf, u8>;

// Targets establish possible compilation roots, including feature-gated bins.
// This does not predict which targets the configured specialist command ran.
pub(super) fn cargo_roots(inputs: &[(&ClassifiedFile, &str)]) -> Roots {
    let mut roots = Roots::new();
    for (file, source) in inputs {
        if file
            .path
            .file_name()
            .is_none_or(|name| name != "Cargo.toml")
        {
            continue;
        }
        let Ok(manifest) = toml::from_str::<toml::Value>(source) else {
            continue;
        };
        if manifest.get("package").is_none() {
            continue;
        }
        let directory = file.path.parent().unwrap_or(Path::new(""));
        add_manifest(
            &Manifest {
                directory,
                value: &manifest,
            },
            inputs,
            &mut roots,
        );
    }
    roots
}

struct Manifest<'a> {
    directory: &'a Path,
    value: &'a toml::Value,
}

struct TargetKind {
    section: &'static str,
    folder: &'static str,
    automatic: &'static str,
    role: u8,
}

fn add_manifest(manifest: &Manifest<'_>, inputs: &[(&ClassifiedFile, &str)], roots: &mut Roots) {
    for (section, folder, automatic, role) in [
        ("lib", "src", "autolib", PRODUCTION),
        ("bin", "src/bin", "autobins", PRODUCTION),
        ("example", "examples", "autoexamples", PRODUCTION),
        ("test", "tests", "autotests", TEST),
        ("bench", "benches", "autobenches", TEST),
    ] {
        TargetKind {
            section,
            folder,
            automatic,
            role,
        }
        .add_roots(manifest, inputs, roots);
    }
    // main.rs is conservative even when automatic inference is disabled.
    for path in ["src/main.rs", "build.rs"] {
        roots
            .entry(normalize(&manifest.directory.join(path)))
            .or_insert(PRODUCTION);
    }
    if let Some(path) = manifest.value["package"]
        .get("build")
        .and_then(toml::Value::as_str)
    {
        roots
            .entry(normalize(&manifest.directory.join(path)))
            .or_insert(PRODUCTION);
    }
}

impl TargetKind {
    fn add_roots(
        &self,
        manifest: &Manifest<'_>,
        inputs: &[(&ClassifiedFile, &str)],
        roots: &mut Roots,
    ) {
        if let Some(value) = manifest.value.get(self.section) {
            let entries = value
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(std::slice::from_ref(value));
            for target in entries {
                for path in self.declared_paths(target) {
                    *roots
                        .entry(normalize(&manifest.directory.join(path)))
                        .or_insert(0) |= self.role;
                }
            }
        }
        if manifest.value["package"]
            .get(self.automatic)
            .and_then(toml::Value::as_bool)
            == Some(false)
        {
            return;
        }
        for (candidate, _) in inputs {
            if self.matches_automatic(&candidate.path, manifest.directory) {
                *roots.entry(normalize(&candidate.path)).or_insert(0) |= self.role;
            }
        }
    }

    fn declared_paths(&self, target: &toml::Value) -> Vec<String> {
        if let Some(path) = target.get("path").and_then(toml::Value::as_str) {
            vec![path.to_string()]
        } else if self.section == "lib" {
            vec!["src/lib.rs".into()]
        } else if let Some(name) = target.get("name").and_then(toml::Value::as_str) {
            vec![
                format!("{}/{name}.rs", self.folder),
                format!("{}/{name}/main.rs", self.folder),
            ]
        } else {
            Vec::new()
        }
    }

    fn matches_automatic(&self, path: &Path, directory: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(directory.join(self.folder)) else {
            return false;
        };
        if path.extension().is_none_or(|extension| extension != "rs") {
            return false;
        }
        if self.section == "lib" {
            relative == Path::new("lib.rs")
        } else {
            relative.components().count() == 1
                || (relative.components().count() == 2
                    && relative.file_name().is_some_and(|name| name == "main.rs"))
        }
    }
}
