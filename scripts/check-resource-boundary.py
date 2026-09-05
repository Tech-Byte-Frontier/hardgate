#!/usr/bin/env python3
"""Check real cgroup limits before repository maintenance tools start."""
import os
from pathlib import Path
import sys


def memory_limit():
    for line in Path('/proc/meminfo').read_text().splitlines():
        if line.startswith('MemTotal:'):
            return min(int(line.split()[1]) * 1024 // 4, 4 * 1024**3)
    raise ValueError('missing host memory size')


def bounded(directory, limit):
    def value(name):
        return (directory / name).read_text().strip()
    try:
        quota, period = value('cpu.max').split()
        maximum = int(value('memory.max'))
        high = int(value('memory.high'))
        return (0 < int(quota) <= 2 * int(period) and int(period) > 0
                and 0 < maximum <= limit and 0 < high <= maximum // 5 * 4
                and value('memory.swap.max') == '0'
                and 0 < int(value('pids.max')) <= 256)
    except (OSError, ValueError):
        return False


def boundary():
    limit = memory_limit()
    membership = next(line[3:] for line in Path('/proc/self/cgroup').read_text().splitlines()
                      if line.startswith('0::'))
    if not membership.startswith('/') or '..' in Path(membership).parts:
        raise ValueError('invalid cgroup membership')
    mount = Path('/sys/fs/cgroup')
    current = mount / membership.lstrip('/')
    while current != mount:
        if bounded(current, limit):
            return current
        current = current.parent
    if bounded(mount, limit):
        return mount
    raise ValueError('no enforced CPU, memory, swap and task boundary')


def event_counters(directory, name):
    counters = {}
    for line in (directory / name).read_text().splitlines():
        key, value = line.split()
        if key in counters or int(value) < 0:
            raise ValueError('invalid resource event counter')
        counters[key] = int(value)
    required = {'max', 'oom', 'oom_kill'} if name == 'memory.events' else {'max'}
    if not required.issubset(counters):
        raise ValueError('missing resource event counters')
    return counters


def print_events(directory, name):
    for key, value in sorted(event_counters(directory, name).items()):
        if key not in {'low', 'high', 'sock_throttled'}:
            print(f'{name} {key} {value}')


def main():
    if sys.argv[1:] == ['--limits']:
        memory = memory_limit()
        cpus = len(os.sched_getaffinity(0))
        quota = min(200, cpus * 50)
        print(f'{quota} {memory} {memory // 5 * 4}')
        return 0
    directory = boundary()
    if sys.argv[1:] == ['--events']:
        for name in ['memory.events', 'pids.events']:
            print_events(directory, name)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, StopIteration) as error:
        print(f'hardgate workload limits: {error}', file=sys.stderr)
        sys.exit(2)
