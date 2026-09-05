use super::*;
use std::collections::BTreeSet;
use std::fs;

fn fixture(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hardgate-init-detect-fixture-{label}-{}",
        std::process::id()
    ));
    let _ = root.exists().then(|| fs::remove_dir_all(&root));
    fs::create_dir_all(&root).unwrap();
    root
}

fn cleanup(root: std::path::PathBuf) {
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn detection_inventory_handles_duplicates_depth_and_symlinks() {
    let root = fixture("inventory");
    let mut inventory = ManifestInventory::default();
    for name in [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "setup.py",
        "requirements.txt",
        "go.mod",
        "ignored.txt",
    ] {
        record_manifest(&root.join(name), &mut inventory);
    }
    assert_eq!(inventory.cargo.len(), 1);
    assert_eq!(inventory.packages.len(), 1);
    assert_eq!(inventory.python.len(), 3);
    assert_eq!(inventory.go.len(), 1);

    let file = root.join("file");
    fs::write(&file, "content").unwrap();
    collect_manifests_at(&file, 1, &mut ManifestInventory::default());
    collect_manifests_at(&root, 0, &mut ManifestInventory::default());
    assert!(is_pruned_directory(std::path::Path::new("node_modules")));
    assert!(!is_pruned_directory(std::path::Path::new("src")));
    fs::write(root.join("marker"), "marker").unwrap();
    assert!(has_any(&root, &["marker"]));
    assert!(!has_any(&root, &["missing"]));

    #[cfg(unix)]
    {
        let scan = root.join("symlink-scan");
        let target = root.parent().unwrap().join(format!(
            "hardgate-init-detect-target-{}",
            std::process::id()
        ));
        fs::create_dir_all(&scan).unwrap();
        let _ = fs::remove_dir_all(&target);
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("package.json"), "{}").unwrap();
        std::os::unix::fs::symlink(&target, scan.join("linked")).unwrap();
        let mut links = ManifestInventory::default();
        collect_manifests_at(&scan, 2, &mut links);
        assert!(links.packages.is_empty());
        fs::remove_dir_all(target).unwrap();
    }
    cleanup(root);
}

#[test]
fn detection_messages_are_deduplicated() {
    let mut detection = Detection::new(Ecosystem::Unknown);
    detection.add_missing("missing");
    detection.add_missing("missing");
    detection.add_note("note");
    detection.add_note("note");
    assert_eq!(detection.missing_setup, vec!["missing".to_string()]);
    assert_eq!(detection.notes, vec!["note".to_string()]);
}

#[test]
fn classification_and_toml_helpers_cover_each_ecosystem() {
    let root = fixture("classification");
    let inventories = [
        (ManifestInventory::default(), Ecosystem::Unknown),
        (
            ManifestInventory {
                cargo: vec![root.join("Cargo.toml")],
                ..ManifestInventory::default()
            },
            Ecosystem::Rust,
        ),
        (
            ManifestInventory {
                packages: vec![root.join("package.json")],
                ..ManifestInventory::default()
            },
            Ecosystem::JavaScript,
        ),
        (
            ManifestInventory {
                python: vec![root.join("pyproject.toml")],
                ..ManifestInventory::default()
            },
            Ecosystem::Python,
        ),
        (
            ManifestInventory {
                go: vec![root.join("go.mod")],
                ..ManifestInventory::default()
            },
            Ecosystem::Go,
        ),
        (
            ManifestInventory {
                cargo: vec![root.join("Cargo.toml")],
                packages: vec![root.join("package.json")],
                ..ManifestInventory::default()
            },
            Ecosystem::Ambiguous,
        ),
    ];
    for (inventory, expected) in inventories {
        assert_eq!(classify(&inventory), expected);
    }

    let valid = "[tool]\n[tool.ruff]\nline-length = 88\n";
    assert!(has_toml_table(valid, &["tool", "ruff"]));
    assert!(!has_toml_table(valid, &["tool", "black"]));
    assert!(!has_toml_table("[tool\n", &["tool"]));
    cleanup(root);
}

#[test]
fn package_metadata_scripts_and_managers_filter_invalid_values() {
    let root = fixture("package");
    let package = root.join("package.json");
    fs::write(
        &package,
        r#"{"packageManager":"yarn@4","scripts":{"format":"format","empty":"   ","bad":false,"test":"test"}}"#,
    )
    .unwrap();
    let package_info = read_package(&package).unwrap();
    assert_eq!(package_info.manager, Some("yarn".to_string()));
    assert!(!package_info.manager_invalid);
    assert_eq!(
        package_info.scripts,
        BTreeSet::from(["format".to_string(), "test".to_string()])
    );
    for invalid in ["[]", "{"] {
        fs::write(&package, invalid).unwrap();
        assert!(read_package(&package).is_none());
    }

    let mut orchestration = OrchestrationConfig::default();
    set_script_commands(
        &mut orchestration,
        &BTreeSet::from([
            "format:check".to_string(),
            "format".to_string(),
            "lint".to_string(),
            "test".to_string(),
        ]),
        "yarn".to_string(),
    );
    assert_eq!(
        orchestration.format_check.as_deref(),
        Some("yarn run format:check")
    );
    assert_eq!(orchestration.format.as_deref(), Some("yarn run format"));
    assert_eq!(orchestration.lint.as_deref(), Some("yarn run lint"));
    assert_eq!(orchestration.test_cmd.as_deref(), Some("yarn run test"));
    assert_eq!(first_script(&BTreeSet::new(), &["format", "fmt"]), None);
    cleanup(root);
}

#[test]
fn lockfiles_and_roots_are_selected_deterministically() {
    let root = fixture("roots");
    assert_eq!(parse_manager("npm@10"), Some("npm".to_string()));
    assert_eq!(parse_manager("pnpm"), Some("pnpm".to_string()));
    assert_eq!(parse_manager("deno@2"), None);
    assert_eq!(parse_manager(""), None);
    assert_eq!(package_manager(&root), Some("npm".to_string()));
    fs::write(root.join("bun.lock"), "lock").unwrap();
    fs::write(root.join("bun.lockb"), "legacy").unwrap();
    assert_eq!(package_manager(&root), Some("bun".to_string()));
    fs::write(root.join("package-lock.json"), "{}").unwrap();
    assert_eq!(package_manager(&root), None);

    let nested = root.join("packages").join("app");
    fs::create_dir_all(&nested).unwrap();
    let root_path = root.join("package.json");
    let nested_path = nested.join("package.json");
    fs::write(&root_path, "{}").unwrap();
    fs::write(&nested_path, "{}").unwrap();
    let manifests = vec![nested_path.clone(), root_path.clone()];
    assert_eq!(root_manifest(&root, &manifests), Some(root_path.as_path()));
    assert_eq!(
        root_manifest(&nested, &manifests),
        Some(nested_path.as_path())
    );
    assert_eq!(root_manifest(&root.join("other"), &manifests), None);
    cleanup(root);
}

#[test]
fn command_detection_reports_ambiguity_and_nested_only_projects() {
    let root = fixture("commands");
    let mut ambiguous = Detection::new(Ecosystem::Ambiguous);
    add_unconfigured_commands(&mut ambiguous);
    assert!(ambiguous.missing_setup.is_empty());
    let mut unknown = Detection::new(Ecosystem::Unknown);
    add_unconfigured_commands(&mut unknown);
    assert_eq!(unknown.missing_setup.len(), 2);
    let mut configured = Detection::new(Ecosystem::Unknown);
    configured.orchestration.format = Some("format".to_string());
    configured.orchestration.lint = Some("lint".to_string());
    add_unconfigured_commands(&mut configured);
    assert!(configured.missing_setup.is_empty());

    let inventory = ManifestInventory {
        python: vec![root.join("nested/pyproject.toml")],
        ..ManifestInventory::default()
    };
    assert_eq!(root_python_manifest(&root, &inventory), None);
    let mut detection = Detection::new(Ecosystem::JavaScript);
    detect_javascript_without_manifest(&root, &ManifestInventory::default(), &mut detection);
    assert!(
        detection
            .missing_setup
            .iter()
            .any(|item| item.contains("package.json was found below"))
    );
    cleanup(root);
}
