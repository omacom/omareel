#!/usr/bin/env python3
"""Waits until the machine is quiet (other agents share it): under 8 % of all CPU time busy over
2 s, three times running. Gives up after `limit` seconds (default 600) and says so."""
import sys
import time


def busy(seconds=2.0):
    def read():
        fields = [int(v) for v in open('/proc/stat').readline().split()[1:]]
        return sum(fields), fields[3] + fields[4]
    t0, i0 = read()
    time.sleep(seconds)
    t1, i1 = read()
    return 1 - (i1 - i0) / max(1, t1 - t0)


limit = float(sys.argv[1]) if len(sys.argv) > 1 else 600
start, calm = time.monotonic(), 0
while time.monotonic() - start < limit:
    calm = calm + 1 if busy() < 0.08 else 0
    if calm >= 3:
        print(f'quiet after {time.monotonic() - start:.0f} s, load {open("/proc/loadavg").read().split()[0]}')
        sys.exit(0)
print(f'not quiet after {limit:.0f} s, load {open("/proc/loadavg").read().split()[0]}; measuring anyway')
