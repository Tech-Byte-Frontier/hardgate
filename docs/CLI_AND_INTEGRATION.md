# CLI Reference & Agent Integration

## 1. CLI Commands

### `hardgate init`
Initializes a new `hardgate.toml` in the current repository.

```sh
hardgate init
hardgate init --preset strict-agent
```

### `hardgate check`
Runs the fast, deterministic static gate. Executes in $< 100\text{ms}$ over thousands of files:
* Physical file and byte budgets
* Tree-sitter AST complexity checks (cyclomatic, cognitive, Halstead, ABC)
* Anti-gaming zero-suppression scan
* Architectural invariant boundary checks
* Token-stream clone detection

```sh
# Standard human terminal output
hardgate check

# Check only staged or modified git files
hardgate check --diff

# Output formatted specifically for an LLM agent context window
hardgate check --format agent
```

### `hardgate verify`
Runs the complete everyday quality gate, incorporating test coverage ingestion, per-function CRAP calculation, and mutation score evaluation.

```sh
hardgate verify
hardgate verify --coverage-report coverage/lcov.info --mutation-report mutants.json
```

### `hardgate scan <file>`
Immediately evaluates a single file and outputs its AST metrics, violations, and suppressions. Perfect for sub-second agent pre-flight checks.

```sh
hardgate scan src/services/auth.ts
```

### `hardgate mcp`
Launches Hardgate as a Model Context Protocol (MCP) server over standard input/output (`stdio`) or HTTP. Exposes tools directly to AI coding assistants.

```sh
hardgate mcp
```

---

## 2. Integrating with AI Coding Agents

### Claude Code (`CLAUDE.md` & Hooks)
To enforce Hardgate in Claude Code, add the check command to your project instructions:

```markdown
<!-- In CLAUDE.md -->
## Mandatory Pre-Flight Verification
After modifying code and before reporting completion, always run:
`hardgate check --format agent`

If Hardgate reports any violations (complexity, suppressions, or budget limits), 
you must refactor the implementation immediately. Never add suppression directives.
```

Configure a Claude Code hook in `.claude/settings.json` or `.hooks`:
```json
{
  "postToolExecution": {
    "command": "hardgate check --diff --format agent"
  }
}
```

### Model Context Protocol (MCP) Setup
To register Hardgate as a native tool for Cline, Cursor, Windsurf, or Claude Desktop, add it to your MCP settings file:

```json
{
  "mcpServers": {
    "hardgate": {
      "command": "hardgate",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

This exposes three tools to the agent:
1. `hardgate_check(paths?: string[])`: Runs the gate on specified files or the whole project.
2. `hardgate_scan_file(path: string)`: Inspects complexity and suppressions for a single file.
3. `hardgate_get_metrics(symbol: string, path: string)`: Returns cyclomatic, cognitive, and line metrics for a specific function.

### Cursor (`.cursorrules`)
Add the following to `.cursorrules`:
```text
All code must pass Hardgate (`hardgate check`).
Strict invariants:
- Functions must have cyclomatic complexity <= 10 and cognitive complexity <= 15.
- Zero suppression comments allowed (no @ts-ignore, no eslint-disable, no #[allow(...)]).
- Files must not exceed 400 lines.
Always run `hardgate check --format agent` to verify edits.
```

---

## 3. Git Hooks & CI/CD Integration

### Lefthook (`lefthook.yml`)
```yaml
pre-commit:
  parallel: true
  commands:
    hardgate:
      run: hardgate check --diff
```

### GitHub Actions Workflow (`.github/workflows/quality.yml`)
```yaml
name: Quality Gate

on:
  push:
    branches: [main]
  pull_request:

jobs:
  hardgate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install Hardgate
        run: cargo install hardgate --locked

      - name: Run Deterministic Quality Gate
        run: hardgate check

      # Optional full verification with test and coverage
      - name: Run Tests & Coverage
        run: pnpm test:coverage

      - name: Run Full Hardgate Verification
        run: hardgate verify --coverage-report coverage/lcov.info
```
