use hardgate::commands::{run_static_gate, run_static_gate_at, run_static_gate_scoped};
use hardgate::config::HardgateConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
const WRAPPER_CHILD_ENV: &str = "HARDGATE_STATIC_API_WRAPPER_CHILD";
const WRAPPER_TEST_NAME: &str = "cwd_wrappers_preserve_source_and_policy_results";
const WRAPPER_SOURCE: &str =
    "fn choose(value: bool) -> bool {\n    if value { true } else { false }\n}\n";

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    fn new(label: &str) -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "hardgate-static-api-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config_without_clone_analysis() -> HardgateConfig {
    let mut config = HardgateConfig::default();
    config.clones.enabled = false;
    config
}

#[test]
fn root_explicit_gate_returns_owned_source_functions_and_findings() {
    let root = FixtureRoot::new("analysis");
    let relative = PathBuf::from("src/branch.rs");
    let source = "fn choose(value: bool) -> bool {\n    if value { true } else { false }\n}\n";
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join(&relative), source).unwrap();

    let mut config = config_without_clone_analysis();
    config.budgets.functions.max_cyclomatic = Some(1);
    let (report, files, loaded, functions) =
        run_static_gate_at(&config, false, std::slice::from_ref(&relative), root.path())
            .unwrap()
            .expect("the selected source file should produce an outcome");

    assert_eq!(files, vec![root.path().join(&relative)]);
    assert_eq!(
        loaded,
        vec![(root.path().join(&relative), source.to_string())]
    );
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "choose");
    assert_eq!(functions[0].file, relative);
    assert!(
        report
            .complexity_violations
            .iter()
            .any(|violation| { violation.function_name == "choose" && violation.file == relative })
    );

    fs::write(root.path().join(&relative), "// changed after analysis\n").unwrap();
    assert_eq!(loaded[0].1, source);
}

#[test]
fn root_explicit_gate_returns_none_for_an_empty_inventory_scope() {
    let root = FixtureRoot::new("empty");
    fs::write(root.path().join("notes.txt"), "not an inventory file\n").unwrap();

    let result =
        run_static_gate_at(&config_without_clone_analysis(), false, &[], root.path()).unwrap();

    assert!(result.is_none());
}

#[test]
fn root_explicit_gate_rejects_an_invalid_scope_path() {
    let root = FixtureRoot::new("invalid");
    let error = run_static_gate_at(
        &config_without_clone_analysis(),
        false,
        &[PathBuf::from("src/missing.rs")],
        root.path(),
    )
    .expect_err("missing explicit paths must fail closed");

    assert!(error.to_string().contains("Path not found"));
}

#[test]
fn cwd_wrappers_preserve_source_and_policy_results() {
    let relative = PathBuf::from("src/branch.rs");
    let mut config = config_without_clone_analysis();
    config.budgets.functions.max_cyclomatic = Some(1);

    if std::env::var_os(WRAPPER_CHILD_ENV).is_some() {
        assert_wrapper_outcome(
            run_static_gate(&config, false).unwrap(),
            &relative,
            WRAPPER_SOURCE,
        );
        assert_wrapper_outcome(
            run_static_gate_scoped(&config, false, std::slice::from_ref(&relative)).unwrap(),
            &relative,
            WRAPPER_SOURCE,
        );
        return;
    }

    let root = FixtureRoot::new("wrappers");
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join(&relative), WRAPPER_SOURCE).unwrap();

    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", WRAPPER_TEST_NAME, "--nocapture"])
        .env(WRAPPER_CHILD_ENV, "1")
        .current_dir(root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("static API wrapper child should spawn");
    let output = wait_for_child(child);
    assert!(
        output.status.success(),
        "static API wrapper child failed: {}",
        diagnostic(&output)
    );
}

fn assert_wrapper_outcome(
    outcome: hardgate::commands::StaticGateOutcome,
    relative: &Path,
    source: &str,
) {
    let (report, files, loaded, functions) = outcome.expect("wrapper should find the source file");
    assert_eq!(files.len(), 1);
    assert!(
        files[0].ends_with(relative),
        "unexpected source path: {:?}",
        files
    );
    assert_eq!(loaded.len(), 1);
    assert!(
        loaded[0].0.ends_with(relative),
        "unexpected loaded path: {:?}",
        loaded
    );
    assert_eq!(loaded[0].1, source);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "choose");
    assert_eq!(functions[0].file, relative);
    assert!(
        report
            .complexity_violations
            .iter()
            .any(|violation| { violation.function_name == "choose" && violation.file == relative })
    );
}

fn diagnostic(output: &Output) -> String {
    format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn wait_for_child(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child
                .wait_with_output()
                .expect("static API wrapper child output should be collected");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .expect("timed-out static API wrapper child should be reaped");
            panic!(
                "static API wrapper child did not terminate: {}",
                diagnostic(&output)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}
