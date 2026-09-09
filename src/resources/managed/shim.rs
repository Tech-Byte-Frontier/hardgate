pub(super) const SOURCE: &[u8] = br##"#!/bin/sh
umask 077

fail() {
    printf '%s\n' "mutation resource guard: $*" >&2
    exit 125
}

report=$1
limit=$2
high=$3
identity=$4
tasks=$5
shift 5
[ "$#" -gt 0 ] || fail "mutation executable is missing"

case "$limit" in
    ''|*[!0-9]*) fail "invalid memory.max limit" ;;
esac
case "$high" in
    ''|*[!0-9]*) fail "invalid memory.high limit" ;;
esac

cgroup_path=
found=0
while IFS=: read -r hierarchy controllers path
do
    [ -n "$hierarchy" ] || continue
    [ "$hierarchy" = 0 ] || continue
    [ "$found" = 0 ] || fail "duplicate unified cgroup membership"
    [ -z "$controllers" ] || fail "unified cgroup membership has controllers"
    [ -n "$path" ] || fail "unified cgroup path is empty"
    cgroup_path=$path
    found=1
done < /proc/self/cgroup
[ "$found" = 1 ] || fail "unified cgroup membership is unavailable"
case "$cgroup_path" in
    /*) ;;
    *) fail "unified cgroup path is not absolute" ;;
esac
case "$cgroup_path" in
    ..|../*|*/..|*/../*|*/.|*/./*|./*) fail "unified cgroup path contains traversal" ;;
esac

cgroup=/sys/fs/cgroup$cgroup_path
read_setting() {
    value=
    IFS= read -r value < "$1" || fail "cannot read $1"
    [ "$value" = "$2" ] || fail "$1 does not equal $2"
}
read_setting "$cgroup/memory.max" "$limit"
read_setting "$cgroup/memory.high" "$high"
read_setting "$cgroup/memory.swap.max" 0
read_setting "$cgroup/memory.oom.group" 1
read_setting "$cgroup/pids.max" "$tasks"

start_marker=$report.start
while :
do
    if [ -e "$start_marker" ]; then
        start_identity=
        IFS= read -r start_identity < "$start_marker" || fail "cannot read mutation start marker"
        [ "$start_identity" = "$identity" ] || fail "mutation start marker identity does not match"
        break
    fi
    sleep 0.02
done

check_processes() {
    found_self=0
    while IFS= read -r pid
    do
        [ -n "$pid" ] || continue
        case "$pid" in
            ''|*[!0-9]*) fail "cgroup.procs has an invalid process id" ;;
        esac
        if [ "$pid" = "$$" ]; then
            found_self=1
        else
            fail "mutation descendants remain in cgroup"
        fi
    done < "$cgroup/cgroup.procs" || fail "cannot read $cgroup/cgroup.procs"
    [ "$found_self" = 1 ] || fail "mutation shim is absent from cgroup"
}

set +e
"$@"
target_status=$?

check_processes
read_setting "$cgroup/memory.max" "$limit"
read_setting "$cgroup/memory.high" "$high"
read_setting "$cgroup/memory.swap.max" 0
read_setting "$cgroup/memory.oom.group" 1
read_setting "$cgroup/pids.max" "$tasks"

read_scalar() {
    value=
    IFS= read -r value < "$1" || fail "cannot read $1"
    case "$value" in
        ''|*[!0-9]*) fail "$1 is not a numeric counter" ;;
    esac
}
read_scalar "$cgroup/memory.peak"
peak=$value

set -C
: > "$report" || fail "cannot create mutation evidence report"
printf 'id=%s\n' "$identity" >> "$report" || fail "cannot write mutation evidence report"
printf 'limit=%s\n' "$limit" >> "$report" || fail "cannot write mutation evidence report"
printf 'high=%s\n' "$high" >> "$report" || fail "cannot write mutation evidence report"
printf 'status=%s\n' "$target_status" >> "$report" || fail "cannot write mutation evidence report"
printf 'peak=%s\n' "$peak" >> "$report" || fail "cannot write mutation evidence report"

pids_max_events=
pids_event_lines=0
while IFS=' ' read -r key value extra
do
    [ -n "$key" ] || fail "pids.events has an invalid line"
    [ -n "$value" ] || fail "pids.events has a missing counter"
    [ -z "$extra" ] || fail "pids.events has extra fields"
    [ "$key" = max ] || fail "pids.events has an invalid line"
    case "$value" in
        ''|*[!0-9]*) fail "pids.events has a nonnumeric counter" ;;
    esac
    pids_event_lines=$((pids_event_lines + 1))
    [ "$pids_event_lines" = 1 ] || fail "pids.events repeats max"
    pids_max_events=$value
done < "$cgroup/pids.events" || fail "cannot read $cgroup/pids.events"
[ "$pids_event_lines" = 1 ] || fail "pids.events is missing max"
[ "$pids_max_events" = 0 ] || fail "pids.events reports a nonzero max counter"
printf 'pids_max_events=%s\n' "$pids_max_events" >> "$report" || fail "cannot write mutation evidence report"

seen_high=0
seen_max=0
seen_oom=0
seen_oom_kill=0
while IFS=' ' read -r key value extra
do
    [ -n "$key" ] || continue
    [ -n "$value" ] || fail "memory.events has a missing counter"
    [ -z "$extra" ] || fail "memory.events has extra fields"
    case "$key" in
        ''|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_]*) fail "memory.events has an invalid key" ;;
    esac
    case "$value" in
        ''|*[!0-9]*) fail "memory.events has a nonnumeric counter" ;;
    esac
    case "$key" in
        high) [ "$seen_high" = 0 ] || fail "memory.events repeats high"; seen_high=1 ;;
        max) [ "$seen_max" = 0 ] || fail "memory.events repeats max"; seen_max=1 ;;
        oom) [ "$seen_oom" = 0 ] || fail "memory.events repeats oom"; seen_oom=1 ;;
        oom_kill) [ "$seen_oom_kill" = 0 ] || fail "memory.events repeats oom_kill"; seen_oom_kill=1 ;;
    esac
    printf 'events.%s=%s\n' "$key" "$value" >> "$report" || fail "cannot write mutation evidence report"
done < "$cgroup/memory.events"
[ "$seen_high" = 1 ] || fail "memory.events is missing high"
[ "$seen_max" = 1 ] || fail "memory.events is missing max"
[ "$seen_oom" = 1 ] || fail "memory.events is missing oom"
[ "$seen_oom_kill" = 1 ] || fail "memory.events is missing oom_kill"

exit "$target_status"
"##;
