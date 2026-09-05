use super::detect::{
    Detection, Ecosystem, ManifestInventory, detect_javascript, detect_root_python, root_manifest,
    root_python_manifest,
};
use std::path::Path;

fn supports_paired_commands(root: &Path, inventory: &ManifestInventory) -> bool {
    let root_js = root_manifest(root, &inventory.packages).is_some() || inventory.js_config;
    let root_py = root_python_manifest(root, inventory).is_some() || inventory.python_config;
    cfg!(unix) && root_js && root_py && inventory.cargo.is_empty() && inventory.go.is_empty()
}

pub(super) fn detect_ambiguous_ecosystems(
    root: &Path,
    inventory: &ManifestInventory,
    detection: &mut Detection,
) {
    if !supports_paired_commands(root, inventory) {
        detection.add_missing(
            "multiple supported ecosystems were detected; configure [orchestration] commands explicitly",
        );
        return;
    }
    let mut js_detect = Detection::new(Ecosystem::JavaScript);
    detect_javascript(root, inventory, &mut js_detect);

    let mut py_detect = Detection::new(Ecosystem::Python);
    detect_root_python(root, inventory, &mut py_detect);

    detection.orchestration.format_check = combine_commands(
        py_detect.orchestration.format_check,
        js_detect.orchestration.format_check,
    );
    detection.orchestration.format = combine_commands(
        py_detect.orchestration.format,
        js_detect.orchestration.format,
    );
    detection.orchestration.lint =
        combine_commands(py_detect.orchestration.lint, js_detect.orchestration.lint);
    detection.orchestration.test_cmd = combine_commands(
        py_detect.orchestration.test_cmd,
        js_detect.orchestration.test_cmd,
    );
    detection.orchestration.timeout_secs = Some(300);
    for missing in js_detect
        .missing_setup
        .into_iter()
        .chain(py_detect.missing_setup)
    {
        detection.add_missing(missing);
    }
    for note in js_detect.notes.into_iter().chain(py_detect.notes) {
        detection.add_note(note);
    }
    if detection.orchestration.format_check.is_none()
        || detection.orchestration.lint.is_none()
        || detection.orchestration.test_cmd.is_none()
    {
        detection.add_missing("combined orchestration requires a detected command for both ecosystems; configure missing commands explicitly");
    }
    detection.add_note(
        "Multi-ecosystem project detected (Python + JavaScript/TypeScript); paired orchestration commands run through POSIX sh",
    );
}

pub(super) fn combine_commands(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) => {
            let script = format!("{a} && {b}").replace('\'', "'\\''");
            Some(format!("sh -c '{script}'"))
        }
        _ => None,
    }
}
