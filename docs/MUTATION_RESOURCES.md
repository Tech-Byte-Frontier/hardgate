# Workload resources

These safeguards apply to the unreleased 0.6.0 CLI. Resource exhaustion is
incomplete evaluation, never a passing gate or a killed-mutant credit.

## Complete CLI boundary

Workload commands run inside verified cgroup-v2 limits on Linux. Hardgate reuses
an already bounded ancestor or creates an owned systemd user scope, discovering
the owner-validated user runtime directory even when an agent omits its bus
environment. The kernel settings are checked; an environment marker alone cannot
bypass admission. A missing manager or failed containment refuses work with exit 2.

The default scope allows at most two CPUs, with a lower quota on one- and two-CPU
hosts, and low CPU scheduling weight. Memory is capped at the smaller of 4 GiB and
one quarter of host RAM; `memory.high` is 80% of that cap. Swap is disabled and
there is a 256-task ceiling. The entire process tree shares these limits, including
formatters, linters, test runners and detached subprocesses. Common worker settings
are capped at two and smaller settings are retained. Analysis also uses at most
two Rayon workers; requesting more workers is an error, not a policy override.

A per-user slot and reserved scope prevent independent invocations from multiplying
the resource allowance. Nested commands reuse their inherited boundary. Cancellation
stops the owned scope; a killed supervisor cannot create an unrestricted orphan.
The scope has a 30-minute runtime ceiling and a remaining scope blocks new work.
Hard memory-limit, OOM, and task-limit events invalidate evidence before report publication. Normal `memory.high` reclaim events alone do not invalidate a completed evaluation; live PSI checks still stop sustained pressure.

`scripts/coverage.sh` and `scripts/self-gate.sh` require the same kernel limits.
Use `scripts/with-resource-limits.sh COMMAND [ARG ...]` for maintenance builds and
other checks. It verifies the boundary and rejects changed resource-event counters;
worker environment hints alone are insufficient.

Full workload containment currently requires Linux cgroup v2 and, when no suitable
boundary is inherited, systemd 254 or later with an accessible user manager.
Platforms without an enforced backend refuse workload commands. Help, version and
completion generation remain available. This is a deliberate fail-closed boundary,
not a claim that worker settings provide equivalent protection on macOS.

Mutation execution now belongs to specialist tools. The removed native mutant
generator, test-command resolver and source-mutation API are not supported.
