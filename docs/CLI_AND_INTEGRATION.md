# CLI reference and agent integration

All commands run from the current repository. The CLI loads `hardgate.toml`, or strict-agent defaults when the file is absent. A report exits non-zero when it contains a violation; advisories are visible context and do not by themselves change the verdict.

## Command semantics

### `hardgate init`

Write a commented `hardgate.toml` template.

```sh
hardgate init
hardgate init --preset strict-agent
hardgate init --preset balanced
hardgate init --preset legacy-migration
```

The generated template explicitly disables coverage, mutation, orchestration, and dead-code execution. Enable each only after supplying its evidence or command.

### `hardgate check`

Run static engines over discovered inventory files:

- file bytes/physical lines and Tree-sitter function budgets;
- suppression and custom-token checks;
- declarative import/call/token invariants;
- bounded token-stream clone detection;
- optional dead-code analysis when enabled in policy;
- optional LCOV and mutation-report evaluation when those policies are enabled.

```sh
hardgate check
hardgate check --diff
hardgate check --all
hardgate check --dead-code
hardgate check --coverage-report coverage/lcov.info
hardgate check --format agent
hardgate check --format json
hardgate check --compact
hardgate check --summary
hardgate check src/routes/revenue.ts
```

`--diff` gets changed/staged inventory files from Git. Clone detection still builds a full repository index and retains only matches touching changed files. A missing Git repository or Git command is an evidence failure.

`--all` adds the configured `[orchestration]` format-check, lint, and test commands. It does not discover or invent commands, and it does not execute native mutation testing. `--dead-code` requests that analysis even when the config switch is false. A CLI coverage path is considered only when `[coverage].enabled = true`; similarly, mutation report paths are considered only when `[mutation].enabled = true`.

### `hardgate verify`

Run the static gate over the full discovered tree, then evaluate enabled LCOV and mutation reports.

```sh
hardgate verify
hardgate verify --coverage-report coverage/lcov.info \
  --mutation-report reports/stryker-mutation.json
hardgate verify --format agent
hardgate verify --format json --summary
hardgate verify packages/backend
```

`verify` ingests reports; it does not run a test suite, invoke Stryker, or run the native AST mutator. Enabled coverage accepts LCOV. Enabled mutation accepts Stryker-shaped, cargo-mutants-shaped, or generic outcome-count JSON. In strict mode, missing/unreadable/malformed required reports are blocking findings. Disabled evidence engines ignore stale report files.

### `hardgate mutate`

Run the built-in AST mutation loop against classified production sources.

```sh
hardgate mutate --diff
hardgate mutate --scoped src/services/auth.ts --timeout 5 --max-mutants 20
hardgate mutate --scoped src/services/auth.ts \
  --test-cmd 'pnpm test {file}' --format agent
hardgate mutate --json
```

The runner:

1. discovers source-role files with a supported AST mutator;
2. resolves one test command per target (or uses `--test-cmd`);
3. runs each unmutated baseline before generating mutants;
4. mutates binary operators and boolean literals, one mutant at a time;
5. classifies killed, survived, timeout, compile-error, runner-error, equivalent, and unviable outcomes;
6. restores and verifies original bytes after every mutant.

A failed baseline stops the run. A selection with no viable mutation points fails; zero viable mutants never scores as a pass. `mutate` is independent of report ingestion and does not invoke Stryker.

### `hardgate scan <file>`

Analyze one file with the same role-aware safety and AST metrics used by the static gate.

```sh
hardgate scan src/services/auth.ts
hardgate scan --format json --summary src/services/auth.ts
```

Unsupported inventory formats can still be inspected for file/safety policy, but they do not produce function metrics. Missing files fail loudly.

### `hardgate fmt`

Run the configured formatter command.

```sh
hardgate fmt
hardgate fmt --check
```

`fmt --check` uses `[orchestration].format_check`; `fmt` uses `format`, falling back to `format_check` when no format command is present. Hardgate prepends a repository-local `node_modules/.bin` to `PATH` and otherwise uses the process environment.

### `hardgate mcp`

Launch the embedded Model Context Protocol server over standard input/output:

```sh
hardgate mcp
```

The server speaks newline-delimited or `Content-Length`-framed JSON-RPC. It exposes static-analysis tools only:

| Tool | Arguments | Behavior |
| --- | --- | --- |
| `hardgate_check` | optional `paths: string[]` | role-aware static report and clone checks |
| `hardgate_scan_file` | required `path: string` | one-file safety and AST report |
| `hardgate_get_metrics` | required `path`, `symbol` | metrics for one named function |

The MCP process does not run orchestration, coverage, mutation, or dead-code commands.

## Output modes

`check`, `scan`, and `verify` accept `--format terminal|agent|json|compact|summary`, plus `--json`, `--compact`/`--no-snippets`, and `--summary`. `mutate` accepts terminal, agent, or JSON output. JSON is the machine-readable report; agent output is structured Markdown with actionable locations.

## Agent integration

Register the stdio process with an MCP-capable client:

```json
{
  "mcpServers": {
    "hardgate": {
      "command": "hardgate",
      "args": ["mcp"]
    }
  }
}
```

For a hook or an agent instruction, use a command that matches the evidence you intend to collect:

```text
After editing, run:
hardgate check --diff --format agent

For a repository-wide static report plus configured format/lint/test commands:
hardgate check --all --format agent

For report evidence already generated by the project:
hardgate verify --format agent
```

A partial command must be described as partial. In particular, `check` does not imply that tests, coverage, mutation, formatting, or dead-code evidence ran unless their policy/command was explicitly enabled.
