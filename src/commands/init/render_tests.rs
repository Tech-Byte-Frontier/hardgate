use super::*;

#[test]
fn reference_comments_and_empty_orchestration_are_explicit() {
    assert!(reference_comment(ReferenceStatus::NotApplicable).is_empty());
    assert!(reference_comment(ReferenceStatus::Available).contains("currently resolvable"));
    assert!(reference_comment(ReferenceStatus::Missing).contains("not resolvable"));
    assert!(reference_comment(ReferenceStatus::Unknown).contains("could not be checked"));

    let mut output = String::new();
    append_orchestration(&mut output, &OrchestrationConfig::default());
    assert!(output.is_empty());
}

#[test]
fn orchestration_rendering_quotes_commands_and_preserves_timeout() {
    let orchestration = OrchestrationConfig {
        format_check: Some("format --check".to_string()),
        format: Some("format".to_string()),
        lint: Some("lint".to_string()),
        test_cmd: Some("test".to_string()),
        timeout_secs: Some(300),
        ..Default::default()
    };
    let mut output = String::new();
    append_orchestration(&mut output, &orchestration);
    assert!(output.starts_with("\n[orchestration]\n"));
    for line in [
        "format_check = \"format --check\"",
        "format = \"format\"",
        "lint = \"lint\"",
        "test_cmd = \"test\"",
        "timeout_secs = 300",
    ] {
        assert!(output.contains(line), "missing {line}: {output}");
    }

    append_command(&mut output, "empty", None);
    assert!(!output.contains("empty ="));
    append_command(&mut output, "quoted", Some("echo 'hello'"));
    assert!(output.contains("quoted = \"echo 'hello'\""));
}

#[test]
fn effective_config_applies_fallback_timeout_and_all_overrides() {
    let detection = Detection {
        ecosystem: super::super::detect::Ecosystem::Unknown,
        orchestration: OrchestrationConfig::default(),
        missing_setup: Vec::new(),
        notes: Vec::new(),
    };
    let options = InitOptions {
        preset: "balanced".to_string(),
        preview: false,
        full: false,
        format_check: Some("custom check".to_string()),
        format: Some("custom format".to_string()),
        lint: Some("custom lint".to_string()),
    };
    let config = effective_config(Preset::Balanced, &detection, &options);
    assert_eq!(config.orchestration.timeout_secs, Some(300));
    assert_eq!(
        config.orchestration.format_check.as_deref(),
        Some("custom check")
    );
    assert_eq!(
        config.orchestration.format.as_deref(),
        Some("custom format")
    );
    assert_eq!(config.orchestration.lint.as_deref(), Some("custom lint"));
}
