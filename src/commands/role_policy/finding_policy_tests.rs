use super::*;
use crate::config::Preset;

fn duplicate(lines: usize, tokens: usize) -> CloneViolation {
    CloneViolation {
        file_a: "src/first.rs".into(),
        file_b: "src/second.rs".into(),
        lines_a: (1, lines),
        lines_b: (1, lines),
        lines,
        tokens,
        fingerprint: "example".into(),
        message: "duplicate".into(),
        recommendation: "review".into(),
    }
}

#[test]
fn both_clone_failure_minima_are_inclusive_and_independent_of_detection() {
    let config = Preset::StrictAgent.to_default_config();
    for (lines, tokens, expected) in [
        (9, 100, Severity::Warning),
        (10, 99, Severity::Warning),
        (10, 100, Severity::Error),
        (11, 101, Severity::Error),
    ] {
        assert_eq!(
            clone_severity(&config, FileRole::Source, &duplicate(lines, tokens)),
            expected
        );
    }
    assert_eq!(
        clone_severity(&config, FileRole::Test, &duplicate(100, 1000)),
        Severity::Warning
    );
}

#[test]
fn explicit_clone_severities_and_custom_inheritance_are_respected() {
    let mut config = Preset::Custom.to_default_config();
    let finding = duplicate(5, 50);
    assert_eq!(
        clone_severity(&config, FileRole::Source, &finding),
        Severity::Error
    );
    assert_eq!(
        clone_severity(&config, FileRole::Config, &finding),
        Severity::Error
    );
    config.roles.source.clone_severity = Some(Severity::Warning);
    assert_eq!(
        clone_severity(&config, FileRole::Source, &finding),
        Severity::Warning
    );
    config.roles.source.clone_severity = Some(Severity::Ignore);
    assert_eq!(
        clone_severity(&config, FileRole::Source, &finding),
        Severity::Ignore
    );
    let (errors, warnings) = partition(vec![finding], |_| Severity::Ignore);
    assert!(errors.is_empty() && warnings.is_empty());
}
