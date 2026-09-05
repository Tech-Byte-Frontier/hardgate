#[path = "support/init_experience.rs"]
mod support;
use hardgate::commands::init::cmd_init_with_options;
use std::fs;
use support::{assert_commands, load_written, options, with_root};

#[test]
fn root_python_and_javascript_pair_every_available_command() {
    with_root("paired-complete", |root| {
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname='paired'\n[tool.ruff]\n[tool.pytest.ini_options]\n",
        )
        .unwrap();
        fs::write(root.join("package.json"), r#"{"packageManager":"npm@12.0.2","scripts":{"format:check":"fmt --check","format":"fmt","lint":"lint","test":"test"}}"#).unwrap();
        cmd_init_with_options(options("balanced")).unwrap();
        let config = load_written(root);
        assert_commands(
            &config,
            [
                "sh -c 'ruff format --check . && npm run format:check'",
                "sh -c 'ruff format . && npm run format'",
                "sh -c 'ruff check . && npm run lint'",
                "sh -c 'pytest && npm run test'",
            ],
        );
        assert_eq!(config.orchestration.timeout_secs, Some(300));
    });
}

#[test]
fn paired_projects_never_advertise_a_one_sided_command() {
    for missing in ["format:check", "lint", "test"] {
        with_root(&format!("paired-missing-{missing}"), |root| {
            fs::write(
                root.join("pyproject.toml"),
                "[project]\nname='paired'\n[tool.ruff]\n[tool.pytest.ini_options]\n",
            )
            .unwrap();
            let mut scripts = serde_json::json!({"format:check":"fmt --check", "format":"fmt", "lint":"lint", "test":"test"});
            scripts.as_object_mut().unwrap().remove(missing);
            fs::write(
                root.join("package.json"),
                serde_json::json!({"packageManager":"npm@12.0.2", "scripts":scripts}).to_string(),
            )
            .unwrap();
            cmd_init_with_options(options("balanced")).unwrap();
            let config = load_written(root);
            let command = match missing {
                "format:check" => config.orchestration.format_check,
                "lint" => config.orchestration.lint,
                _ => config.orchestration.test_cmd,
            };
            assert!(command.is_none(), "one-sided {missing}");
            let written = fs::read_to_string(root.join("hardgate.toml")).unwrap();
            assert!(written.contains("combined orchestration requires"));
        });
    }
}
