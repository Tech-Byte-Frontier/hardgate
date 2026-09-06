//! Controlled specialist protocol fixtures. Real compiler coverage and the
//! non-empty cargo-mutants sample remain separate release acceptance checks.
#[path = "fs.rs"]
mod fs_tests;

use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

pub(super) const SOURCE: &str = "pub fn answer() -> u32 { 42 }\n";
const RUNNER: &str = r#"#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
const mode = process.env.HARDGATE_FIXTURE_MODE || 'pass';
if (args.includes('--version')) {
  if (mode === 'version-fail') process.exit(9);
  if (mode !== 'version-empty') console.log('fixture-specialist 1.0');
  process.exit(0);
}
if (args.includes('clippy')) {
  const prefix = args.slice(0, args.includes('--') ? args.indexOf('--') : undefined);
  if (!prefix.includes('--message-format=json') || prefix.includes('short') || prefix.includes('--message-format=human')) process.exit(9);
  process.stdout.write(process.env.HARDGATE_FIXTURE_REPORT || '');
  process.exit(Number(process.env.HARDGATE_FIXTURE_EXIT || 0));
}
if (args.includes('--no-report') || (args.includes('test') && !args.includes('mutants'))) {
  process.exit(mode === 'baseline-fail' ? 1 : 0);
}
if (mode === 'timeout') setTimeout(() => {}, 10000);
else (async () => {
  if (mode === 'missing') return;
  let destination;
  if (args.includes('--output-path')) destination = args[args.indexOf('--output-path') + 1];
  else if (args.includes('--output')) {
    if (process.env.CARGO_MUTANTS_JOBS) throw Error('in-place jobs must be removed');
    destination = path.join(args[args.indexOf('--output') + 1], 'mutants.out/outcomes.json');
  } else {
    const directory = args.find(arg => arg.startsWith('--coverage.reportsDirectory='));
    if (directory) destination = path.join(directory.split('=').slice(1).join('='), 'lcov.info');
    else {
      const config = (await import(require('node:url').pathToFileURL(args[1]).href)).default;
      if (config.incremental || config.dryRunOnly || config.allowEmpty || config.inPlace || config.concurrency !== 1) throw Error('unsafe Stryker configuration');
      destination = config.jsonReporter.fileName;
    }
  }
  fs.mkdirSync(path.dirname(destination), {recursive: true});
  let report = process.env.HARDGATE_FIXTURE_REPORT.replaceAll('FIXTURE_ROOT', process.cwd());
  if (mode === 'empty') report = '';
  if (mode === 'malformed') report = 'not a report';
  fs.writeFileSync(destination, report);
  if (mode === 'unrestored') fs.writeFileSync('src/lib.rs', 'changed');
  process.exit(Number(process.env.HARDGATE_FIXTURE_EXIT || 0));
})().catch(error => { console.error(error); process.exit(9); });
"#;

pub(super) struct Project(pub(super) PathBuf);

impl Project {
    pub(super) fn new() -> Self {
        let root = fs_tests::tempdir("evidence-protocol");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), SOURCE).unwrap();
        std::fs::write(root.join("index.js"), "export const answer = 42;\n").unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        std::fs::write(root.join("package.json"), "{\"private\":true}").unwrap();
        std::fs::write(root.join("hardgate.toml"), "[gate]\npreset='balanced'\n").unwrap();
        std::fs::write(
            root.join("stryker.config.json"),
            "{\"incremental\":true,\"dryRunOnly\":true,\"concurrency\":12}",
        )
        .unwrap();
        for name in [
            "tools/cargo",
            "node_modules/.bin/vitest",
            "node_modules/.bin/stryker",
        ] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, RUNNER).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self(root)
    }

    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hardgate"));
        command.current_dir(&self.0).env(
            "PATH",
            format!(
                "{}:{}",
                self.0.join("tools").display(),
                std::env::var("PATH").unwrap()
            ),
        );
        command.env("CARGO_MUTANTS_JOBS", "1");
        command
    }

    pub(super) fn producer_command(
        &self,
        producer: &str,
        report: &str,
        scenario: (&str, i32),
    ) -> Command {
        let mut command = self.command();
        command.args(["evidence", producer, "--timeout-secs", "1"]);
        if producer == "cargo-llvm-cov" {
            command.args(["--toolchain", "fixture-nightly"]);
        }
        command
            .env("HARDGATE_FIXTURE_REPORT", report)
            .env("HARDGATE_FIXTURE_MODE", scenario.0)
            .env("HARDGATE_FIXTURE_EXIT", scenario.1.to_string());
        command
    }

    pub(super) fn produce(&self, producer: &str, report: &str, scenario: (&str, i32)) -> Output {
        self.producer_command(producer, report, scenario)
            .output()
            .unwrap()
    }

    pub(super) fn report(&self, kind: &str) -> PathBuf {
        self.0
            .join(".hardgate/evidence")
            .join(if kind == "coverage" {
                "coverage.lcov"
            } else {
                "mutation.json"
            })
    }

    pub(super) fn receipt(&self, kind: &str) -> PathBuf {
        PathBuf::from(format!("{}.hardgate.json", self.report(kind).display()))
    }

    pub(super) fn nested_mutation_is_rejected(&self, producer: &str) -> bool {
        if std::env::var_os("HARDGATE_MUTATION_CHILD").is_none() {
            return false;
        }
        // A cargo-mutants workspace test is already inside the global lease.
        // The public contract here is rejection, not a second mutation run.
        let output = self.produce(producer, &mutation(true).to_string(), ("pass", 0));
        assert_exit(&output, 2);
        assert!(String::from_utf8_lossy(&output.stderr).contains("nested mutation is unsupported"));
        assert!(!self.receipt("mutation").exists());
        true
    }

    pub(super) fn check_report(&self, kind: &str) -> Value {
        let output = self
            .command()
            .args(["check", "--checks", "policy", "--json"])
            .arg(format!("--{kind}-report"))
            .arg(self.report(kind))
            .arg("src/lib.rs")
            .output()
            .unwrap();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["summary"]["analysis_blockers"], 0, "{report}");
        report
    }

    pub(super) fn verify(&self, kind: hardgate::evidence::EvidenceKind) -> anyhow::Result<()> {
        let name = if kind == hardgate::evidence::EvidenceKind::Coverage {
            "coverage"
        } else {
            "mutation"
        };
        hardgate::evidence::verify(
            &self.0,
            &self.report(name),
            kind,
            &hardgate::config::HardgateConfig::default(),
        )
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

pub(super) fn lcov() -> &'static str {
    "SF:FIXTURE_ROOT/src/lib.rs\nFN:1,answer\nFNDA:1,answer\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nBRF:0\nBRH:0\nend_of_record\n"
}

pub(super) fn mutation(caught: bool) -> Value {
    let phases = |failed| {
        json!([
            {"phase":"Build","process_status":"Success","argv":["cargo","test","--no-run"]},
            {"phase":"Test","process_status":if failed { json!({"Failure":101}) } else { json!("Success") },"argv":["cargo","test"]}
        ])
    };
    json!({"cargo_mutants_version":"27.1.0","end_time":"2026-09-06T00:00:00Z","total_mutants":1,"caught":u8::from(caught),"missed":u8::from(!caught),"timeout":0,"unviable":0,"success":0,"outcomes":[
        {"scenario":"Baseline","summary":"Success","phase_results":phases(false)},
        {"scenario":{"Mutant":{"file":"src/lib.rs","replacement":"0","span":{"start":{"line":1,"column":25},"end":{"line":1,"column":27}}}},"summary":if caught {"CaughtMutant"} else {"MissedMutant"},"phase_results":phases(caught)}
    ]})
}

pub(super) fn assert_exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
