# Workload resources

These safeguards apply to evidence producers, policies that set
`orchestration.require_isolation = true`, and maintenance runners. Ordinary
project checks and formatting run natively on macOS and Linux; native execution
retains timeouts, process-group cleanup, worker defaults, and check-input
verification, but does not enforce OS CPU/memory or filesystem isolation.
Static scans, policy-only checks without generated freshness, saved reports,
and static MCP tools run locally on macOS and Linux. Resource exhaustion is
incomplete evaluation, never a passing gate or a killed-mutant credit.

## Project-tool execution boundary

Commands requiring isolation run inside verified cgroup-v2 limits on Linux. Hardgate reuses
an already bounded ancestor or creates an owned systemd user scope, discovering
the owner-validated user runtime directory even when an agent omits its bus
environment. The kernel settings are checked; an environment marker alone cannot
bypass admission. A missing manager or failed containment refuses work with exit 2.

The default workload allowance is half the available CPUs, rounded down with a
minimum of one and a maximum of 64. Set `--workload-jobs N` or
`HARDGATE_WORKLOAD_JOBS=N` (1–64) for an explicit allowance; the CLI option takes
precedence. This changes execution capacity, not quality requirements. A selected
allowance is inherited unchanged when Hardgate reexecutes inside its scope.

For `N` jobs, the scope has an `N`-CPU quota and a task ceiling of
`max(256, 128*N)`, counting both processes and threads. Memory is capped at the
smaller of one quarter of host RAM and `clamp(N, 4, 16)` GiB by default.
`--workload-memory-mib` / `HARDGATE_WORKLOAD_MEMORY_MIB` (1024–65536)
can select an independent ceiling, still capped at one quarter of host RAM.
`memory.high` is 80% of that cap. Swap remains disabled and CPU scheduling weight remains low.
The entire process tree shares these limits, including detached subprocesses.
Hardgate prints the actual admitted kernel limits before running tools; tighter
inherited limits remain enforced. Resource failures identify memory versus task
events and include current, peak and maximum usage when available.

Common build/test worker settings are capped at the selected allowance; smaller
settings are retained. Stryker evidence also sizes runners from current memory
headroom, subtracting the live reserve and a 512 MiB coordinator allowance.
`--mutation-worker-memory-mib` / `HARDGATE_MUTATION_WORKER_MEMORY_MIB`
sets the per-runner estimate (1024–16384, default 2048 MiB). Concurrency cannot
exceed the resulting memory capacity, CPU allowance, or a smaller explicit project
setting. No fitting runner is a setup failure; the live pressure guard remains
mandatory because a planning estimate is not a guaranteed bound on each runner.
Analysis uses the available workload CPU allowance; `--threads` can select fewer Rayon workers.

```sh
hardgate --workload-jobs 8 check --format agent
hardgate --workload-jobs 8 evidence stryker
HARDGATE_WORKLOAD_JOBS=8 scripts/with-resource-limits.sh cargo test
```

A shared per-user flock slot prevents independent invocations and maintenance
runners from multiplying the resource allowance. Each outer invocation creates
a unique scope identity, so stale unit cleanup cannot collide with a subsequent
launch. Overlapping commands wait up to 30 minutes and print a waiting message;
cancellation while queued starts no workload. When the slot becomes available,
Hardgate reports queued time separately from execution. A timeout reports workload contention.
Nested commands reuse their inherited boundary. Cancellation stops only the owned
scope. The scope leader inherits the workload lock, so terminating its outer
supervisor does not release the slot while that leader remains live. The scope
has a 30-minute runtime ceiling. Mutation's separate serialization lease remains.
Hard memory-limit, OOM, and task-limit events invalidate evidence before report publication. Normal `memory.high` reclaim events alone do not invalidate a completed evaluation; live PSI checks still stop sustained pressure.

`scripts/coverage.sh` and `scripts/self-gate.sh` require the same kernel limits.
Use `scripts/with-resource-limits.sh COMMAND [ARG ...]` for maintenance builds and
other checks. It verifies the boundary and rejects changed resource-event counters;
worker environment hints alone are insufficient.

Full workload containment currently requires Linux cgroup v2 and, when no suitable
boundary is inherited, systemd 254 or later with an accessible user manager.
Platforms without an enforced backend refuse evidence production and explicitly
isolated project-tool execution with setup guidance. Native checks remain
available. Static analysis bounds Rayon workers by the available workload CPU allowance;
this worker limit does not claim OS containment.

Mutation execution now belongs to specialist tools. The removed native mutant
generator, test-command resolver and source-mutation API are not supported.
