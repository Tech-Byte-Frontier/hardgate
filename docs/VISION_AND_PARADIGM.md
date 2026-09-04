# Vision and paradigm: deterministic policy for agent-assisted code

Autonomous coding agents make producing plausible code inexpensive. The scarce resource is review time: a maintainer must decide whether a change respects the repository's boundaries and has enough evidence behind it. Hardgate treats that decision as a policy problem, not a prompt-writing problem.

```text
probabilistic agent -> deterministic local policy -> actionable report
                         | budgets
                         | anti-gaming
                         | architecture
                         | evidence
```

## What agents can optimize around

An agent asked only for a green command can choose shortcuts that are difficult to review:

1. add suppression directives instead of addressing a diagnostic;
2. grow a function or file until the surrounding design is opaque;
3. copy an existing block rather than discover a shared abstraction;
4. cross a UI/domain boundary because a direct import is shorter;
5. present a coverage or mutation number without proving that the report is current.

Hardgate does not infer intent. It makes the policy explicit and records what was actually inspected. Suppression findings come from source text; budgets come from measured bytes, lines, and parsed functions; invariant findings come from configured import/call/token rules; clone findings come from verified token windows.

## The local policy model

### Structural budgets

Physical file and function ceilings make growth visible at the point of change. Tree-sitter metrics provide cyclomatic and cognitive contributors, Halstead difficulty, ABC score, parameter count, statement count, body lines, and nesting depth for the supported parser targets. Teams can scale the thresholds with a preset, but a threshold remains a hard policy once configured.

### Anti-gaming

The anti-gaming scanner recognizes common suppression directives and project-forbidden tokens in safety-checked files. A project can disable the scanner, but then it has explicitly chosen not to enforce that evidence. There is no hidden allow-list or inline approval channel in the current configuration.

### Architecture

Declarative invariant rules keep high-risk calls and imports near the boundary they protect. They are intentionally local checks: a rule says which paths and tokens are forbidden, and the report points to the exact line. They complement a compiler or a dedicated dependency tool rather than pretending to replace either.

### Evidence

Coverage and mutation are optional evidence engines. An enabled coverage policy requires an LCOV report; an enabled mutation policy requires a supported JSON report. Strict mode treats missing, unreadable, or malformed evidence as a finding, while disabled policies do not consume old files. check --all can run commands configured by the repository, but Hardgate never claims that an unconfigured test command ran.

Native hardgate mutate adds an executable feedback loop: baseline first, bounded AST mutants, explicit outcome categories, timeout handling, and byte-for-byte restoration. Report evaluation and native execution are separate paths. A Stryker report can be evaluated without Hardgate invoking Stryker; the native runner does not claim Stryker compatibility.

### Reviewable output

The same report can be rendered for a human terminal, an agent's context window, or automation. A failing location includes the metric or policy, actual value, configured limit, and a refactoring direction. Advisories keep exclusions and partial-gate scope visible without silently turning them into pass criteria.

## Presets are policy bundles

- strict-agent supplies the tightest structural budgets and strict evidence handling.
- balanced supplies scaled budgets while retaining ordinary violations as failures.
- legacy-migration currently scales budgets and reports evidence failures as advisories; it is not a reference-branch ratchet.
- custom lets a repository state its own values.

A merge-base baseline/ratchet, changed-hunk attribution, and diff-coverage/new-clone evidence are stabilization targets. They are not part of the current preset behavior and must not be described as active until their implementations and regression proofs land.

## What Hardgate is (and is not)

Hardgate is a deterministic local policy and reporting layer for agent-assisted repositories. It is complementary to language compilers, formatters, linters, coverage providers, mutation runners, clone tools, and hosted dashboards. It does not make a quality claim merely because a command returned zero: the relevant engine must be enabled, its inputs must be present, and its report must be free of violations.
