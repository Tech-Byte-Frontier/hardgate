# Native mutation resources

These safeguards apply to the unreleased 0.6.0 native `hardgate mutate` runner.
Mutation-report ingestion does not start workloads.

Hardgate holds one mutation slot per operating-system user across projects,
including snapshot creation, the unmutated baseline, and every mutant. A second
invocation waits without starting its test command. Ctrl-C cancels the wait.
Nested mutation from a mutation test command fails before copying or writing
source, preventing recursive builds and lock deadlocks.

Snapshot files are copied and verified in 64 KiB buffers. Native source input,
protected snapshots and candidate generation also have memory admission limits:
up to 8 MiB per source, 128 MiB of protected source bytes and 100,000 candidates,
reduced further by the available command budget. Exceeding a limit aborts with
incomplete evidence; it does not silently truncate the representative selection. Cargo test commands
build in the snapshot's fresh target directory. Allow enough temporary disk
space and time for a cold build; select tests that exercise the mutated scope.

## Worker defaults

The runner caps common Cargo, Rust test, Rayon, Go, OpenMP, BLAS, CMake and Make
worker settings at two (or one on a single-CPU environment), preserving a smaller
explicit limit where supported. These environment defaults do not override every
test framework or an explicit command-line worker option. Configure a small
worker count in frameworks that use their own settings.

## Linux memory checks

Before admission and during copying/test execution, Hardgate reads available
host memory, applicable cgroup-v2 limits, and memory pressure stall information.
It retains a reserve of 10% of effective total memory, clamped to 256 MiB–2 GiB.
It refuses or stops work below that reserve, at 1% full memory pressure over ten
seconds, or at 10% some memory pressure over ten seconds. Sampling is throttled
to 250 ms; it cannot prevent every abrupt memory spike or stop another project.

On an eligible Linux host with systemd 254 or later and an accessible user
manager, each test command also runs in an owned transient scope. Its aggregate
memory cap is the smallest of 8 GiB, one quarter of effective total memory, and
half the available memory after the reserve. Admission requires at least 64 MiB.
`memory.high` is 80% of that cap (limits round down to 64 KiB). Swap is disabled.
The scope has a two-core CPU quota (one core when applicable), a 256-task limit,
whole-cgroup cleanup, and a runtime limit. Scope startup has a separate bounded
handshake before the test-command timeout starts. Kernel settings, peak memory, memory
events and process-limit events are verified before accepting a test outcome. Resource-limit events, missing evidence
or a contained timeout are blocking failures; they earn no mutation credit.
A test command that leaves background descendants is also incomplete: cleanup
stops those processes, and the command must be rerun with proper child shutdown.

Scope commands retain the caller's namespaces, credentials and environment.
Without the required user-manager/runtime connection, Hardgate reports that
only sampled memory checks are available. Once a managed command is selected,
a manager or evidence error fails closed rather than retrying the command
without its memory cap.

## macOS and recovery

macOS retains the per-user slot, common worker limits and owned process-group
cleanup. Linux memory telemetry and aggregate cgroup containment are unavailable;
the command diagnostics state that limit explicitly.

When the guard aborts, close competing workloads or narrow the test command to
relevant tests, then rerun. Keep complete test-suite validation as a separate
serialized check. Do not count a resource-aborted run as a passing mutation gate.

SIGINT/SIGTERM stop owned commands and restore the private workspace. SIGKILL or
a host restart can leave temporary files; a managed Linux command additionally
has its scope runtime limit. Inspect and stop any remaining owned processes
before removing their exact temporary directory. Original project source remains
outside the native CLI mutation workspace.
