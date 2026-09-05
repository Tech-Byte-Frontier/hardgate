use super::InitOptions;
use super::detect::{Detection, ReferenceStatus};
use crate::config::{HardgateConfig, OrchestrationConfig, Preset};

pub(crate) struct RenderInput<'a> {
    pub(crate) config: &'a HardgateConfig,
    pub(crate) preset: Preset,
    pub(crate) detection: &'a Detection,
    pub(crate) missing_setup: &'a [String],
    pub(crate) reference_status: ReferenceStatus,
    pub(crate) full: bool,
}

pub(crate) fn effective_config(
    preset: Preset,
    detection: &Detection,
    options: &InitOptions,
) -> HardgateConfig {
    let mut config = preset.to_default_config();
    config.orchestration = detection.orchestration.clone();
    if let Some(command) = &options.format_check {
        config.orchestration.format_check = Some(command.clone());
    }
    if let Some(command) = &options.format {
        config.orchestration.format = Some(command.clone());
    }
    if let Some(command) = &options.lint {
        config.orchestration.lint = Some(command.clone());
    }
    config
}

pub(crate) fn render(input: RenderInput<'_>) -> String {
    if input.full {
        render_full(&input)
    } else {
        render_concise(&input)
    }
}

fn render_full(input: &RenderInput<'_>) -> String {
    let body = toml::to_string_pretty(input.config).expect("effective preset must serialize");
    let mut output = header(input.preset, true);
    output.push_str(&preset_guidance(input.preset));
    output.push_str(&detection_comments(input.detection, input.missing_setup));
    output.push_str(&reference_comment(input.reference_status));
    output.push('\n');
    output.push_str(&body);
    output
}

fn render_concise(input: &RenderInput<'_>) -> String {
    let mut output = header(input.preset, false);
    output.push_str(&preset_guidance(input.preset));
    output.push_str(&detection_comments(input.detection, input.missing_setup));
    output.push_str(&reference_comment(input.reference_status));
    output.push_str("\n[gate]\n");
    output.push_str(&format!(
        "preset = {:?}\n",
        super::preset_name(input.preset)
    ));
    append_orchestration(&mut output, &input.config.orchestration);
    output
}

fn header(preset: Preset, full: bool) -> String {
    let mode = if full {
        "expanded effective configuration"
    } else {
        "concise preset with project-specific overrides"
    };
    format!(
        "# Hardgate {mode}\n# Preset: {}\n# Generated without installing or executing project tools.\n",
        super::preset_name(preset)
    )
}

fn preset_guidance(preset: Preset) -> String {
    match preset {
        Preset::StrictAgent => {
            "# strict-agent keeps the strict structural thresholds (95% line/function,\n\
             # 90% branch coverage, and an 85% mutation floor) and requires evidence.\n\
             # Provide coverage/lcov.info and [mutation].reports before hardgate verify.\n\
             # A structural hardgate check is useful while evidence is being generated.\n"
                .to_string()
        }
        Preset::Balanced => {
            "# balanced is the structural starting point: coverage and mutation evidence\n\
             # are disabled until the project is ready to configure them.\n\
             # Next step: run hardgate check and then add project evidence deliberately.\n"
                .to_string()
        }
        Preset::LegacyMigration => {
            "# legacy-migration keeps a non-strict static ratchet against origin/main.\n\
             # The reference must resolve to a merge-base before the ratchet is useful.\n\
             # Fetch the reference branch when it is missing; current evidence remains required.\n"
                .to_string()
        }
        Preset::Custom => {
            "# custom starts from Hardgate's ordinary defaults; it is not an empty shell.\n\
             # Clone analysis, anti-gaming, and invariants remain enabled by their defaults.\n\
             # Add only the evidence and budgets this project is ready to own.\n"
                .to_string()
        }
    }
}

fn detection_comments(detection: &Detection, missing_setup: &[String]) -> String {
    let mut output = format!(
        "# Detected project kind: {}.\n",
        detection.ecosystem.label()
    );
    for note in &detection.notes {
        output.push_str(&format!("# {note}.\n"));
    }
    for missing in missing_setup {
        output.push_str(&format!("# Setup needed: {missing}.\n"));
    }
    output
}

fn reference_comment(status: ReferenceStatus) -> String {
    match status {
        ReferenceStatus::NotApplicable => String::new(),
        ReferenceStatus::Available => {
            "# Legacy reference check: origin/main is currently resolvable.\n".to_string()
        }
        ReferenceStatus::Missing => {
            "# Setup needed: origin/main is not resolvable; fetch or configure a valid reference before checking.\n"
                .to_string()
        }
        ReferenceStatus::Unknown => {
            "# Setup needed: legacy reference validity could not be checked; verify origin/main before checking.\n"
                .to_string()
        }
    }
}

fn append_orchestration(output: &mut String, orchestration: &OrchestrationConfig) {
    if orchestration.format_check.is_none()
        && orchestration.format.is_none()
        && orchestration.lint.is_none()
        && orchestration.test_cmd.is_none()
    {
        return;
    }
    output.push_str("\n[orchestration]\n");
    append_command(
        output,
        "format_check",
        orchestration.format_check.as_deref(),
    );
    append_command(output, "format", orchestration.format.as_deref());
    append_command(output, "lint", orchestration.lint.as_deref());
    append_command(output, "test_cmd", orchestration.test_cmd.as_deref());
}

fn append_command(output: &mut String, key: &str, command: Option<&str>) {
    if let Some(command) = command {
        let quoted = toml::Value::String(command.to_string()).to_string();
        output.push_str(&format!("{key} = {quoted}\n"));
    }
}
