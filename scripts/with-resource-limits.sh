#!/bin/sh
# Keep maintenance builds, coverage, mutation, and their descendants in one boundary.
set -eu
if [ "$#" -eq 0 ]; then
  echo 'usage: scripts/with-resource-limits.sh COMMAND [ARG ...]' >&2
  exit 2
fi
if ! node scripts/check-resource-boundary.mjs >/dev/null 2>&1; then
  if [ "${HARDGATE_RESOURCE_SCRIPT_CHILD:-}" = 1 ]; then
    echo 'hardgate: kernel resource limits were not established; command was not started' >&2
    exit 2
  fi
  limits=$(node scripts/check-resource-boundary.mjs --limits)
  quota=${limits%% *}
  memory_and_high=${limits#* }
  memory=${memory_and_high%% *}
  high=${memory_and_high#* }
  export XDG_RUNTIME_DIR=/run/user/$(id -u)
  unset DBUS_SESSION_BUS_ADDRESS
  export HARDGATE_RESOURCE_SCRIPT_CHILD=1
  exec systemd-run --user --scope --quiet --collect --expand-environment=no \
    --unit=hardgate-workload.scope \
    --property="CPUQuota=$quota%" --property=CPUWeight=25 \
    --property="MemoryMax=$memory" --property="MemoryHigh=$high" \
    --property=MemorySwapMax=0 --property=TasksMax=256 \
    --property=OOMPolicy=kill --property=KillMode=control-group \
    --property=TimeoutStopSec=1s --property=RuntimeMaxSec=1800s \
    -- "$0" "$@"
fi
. scripts/resource-worker-env.sh
before=$(node scripts/check-resource-boundary.mjs --events)
status=0
"$@" || status=$?
after=$(node scripts/check-resource-boundary.mjs --events)
if [ "$before" != "$after" ]; then
  echo 'hardgate: resource-limit events occurred; maintenance evidence is incomplete' >&2
  exit 2
fi
exit "$status"
