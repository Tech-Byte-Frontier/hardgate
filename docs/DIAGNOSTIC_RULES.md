# Diagnostic rule IDs

`diagnostics` converts the blocking arrays in a `GateReport` into a stable,
machine-readable stream of `RuleDiagnostic` values. Findings are emitted in
this order: budget, suppression, complexity, invariant, clone, coverage,
mutation, and orchestration. Findings within one array retain the
array order from the report.

Rule IDs are compatibility keys. They are literal `HG-*` values selected from
the producer's metric or kind; paths, line numbers, fingerprints, and mutable
messages never participate in an ID. Existing IDs must remain unchanged once
published. A newly introduced metric or kind uses that category's fallback ID
until it receives an explicit public ID, so unknown input cannot create a new
unstable identifier.

The current public IDs are:

| Category | IDs |
| --- | --- |
| budget | `HG-BUDGET-FILE-BYTE-SIZE`, `HG-BUDGET-PHYSICAL-LINES`, `HG-BUDGET-UNKNOWN-METRIC` |
| suppression | `HG-SUPPRESSION-FORBIDDEN` |
| complexity | `HG-COMPLEXITY-CYCLOMATIC`, `HG-COMPLEXITY-PARAMETERS`, `HG-COMPLEXITY-FUNCTION-LINES`, `HG-COMPLEXITY-NESTING`, `HG-COMPLEXITY-STATEMENTS`, `HG-COMPLEXITY-UNKNOWN-METRIC` |
| invariant | `HG-INVARIANT-DISALLOWED-IMPORT`, `HG-INVARIANT-DISALLOWED-CALL`, `HG-INVARIANT-DISALLOWED-TOKEN`, `HG-INVARIANT-UNKNOWN-KIND` |
| clone | `HG-CLONE-DUPLICATE-BLOCK` |
| coverage | `HG-COVERAGE-COUNT-OVERFLOW`, `HG-COVERAGE-GLOBAL-LINES`, `HG-COVERAGE-GLOBAL-FUNCTIONS`, `HG-COVERAGE-GLOBAL-BRANCHES`, `HG-COVERAGE-MISSING-SOURCE`, `HG-COVERAGE-MISSING-CRITICAL-PATH`, `HG-COVERAGE-CRITICAL-PATH`, `HG-COVERAGE-MISSING-DIFF`, `HG-COVERAGE-DIFF-LINES`, `HG-COVERAGE-UNKNOWN-METRIC` |
| mutation | `HG-MUTATION-KILL-RATE`, `HG-MUTATION-TIMEOUTS`, `HG-MUTATION-COMPILE-ERRORS`, `HG-MUTATION-RUNNER-ERRORS`, `HG-MUTATION-UNVIABLE`, `HG-MUTATION-UNKNOWN-METRIC` |
| orchestration | `HG-ORCHESTRATION-FORMAT-CHECK`, `HG-ORCHESTRATION-FORMAT`, `HG-ORCHESTRATION-LINT`, `HG-ORCHESTRATION-TEST`, `HG-ORCHESTRATION-GENERATED-FRESHNESS`, `HG-ORCHESTRATION-COVERAGE-DIFF`, `HG-ORCHESTRATION-COVERAGE-REPORT`, `HG-ORCHESTRATION-COVERAGE-SOURCE-CLASSIFICATION`, `HG-ORCHESTRATION-DEAD-CODE-CONTEXT`, `HG-ORCHESTRATION-READ-CLONE-INDEX`, `HG-ORCHESTRATION-CLONE-INDEX`, `HG-ORCHESTRATION-READ-SOURCE`, `HG-ORCHESTRATION-PARSE-SOURCE`, `HG-ORCHESTRATION-CLASSIFY-SOURCE`, `HG-ORCHESTRATION-UNSUPPORTED-SOURCE`, `HG-ORCHESTRATION-MUTATION-REPORT`, `HG-ORCHESTRATION-LEGACY-RATCHET`, `HG-ORCHESTRATION-UNKNOWN-STEP` |

Locations carry only coordinates present in the source violation. Budget,
coverage, and mutation findings therefore have file-only locations;
orchestration findings have no location. Clone findings carry both source
ranges in `locations`, and their rule ID stays unchanged when either path or
line range changes.
