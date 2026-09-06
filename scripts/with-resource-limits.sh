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
  exec node scripts/resource-scope.mjs "$0" "$@"
fi
node scripts/resource-scope.mjs --ack
unset HARDGATE_RESOURCE_SCRIPT_READY
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
