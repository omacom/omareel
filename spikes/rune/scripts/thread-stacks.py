# thread-stacks.py (n1x analysis helper) <run> <comm> <from s> <to s>: inclusive CPU ms per function on threads named <comm>, and the busy timeline.
import collections, json, os, re, subprocess, sys
run, comm, t0, t1 = sys.argv[1], sys.argv[2], float(sys.argv[3]), float(sys.argv[4])
start = json.load(open(os.path.join(run, 'start.json')))['mono']
out = subprocess.run([os.path.expanduser('~/.cache/omareel-spikes/tools/perf'), 'script', '-i', os.path.join(run, 'perf.data'), '--comms', comm,
                      '-F', 'tid,time,ip,sym,dso', '--no-inline'], capture_output=True, text=True, errors='replace').stdout
inc, buckets, n = collections.Counter(), collections.Counter(), 0
for block in out.split('\n\n'):
    lines = block.strip('\n').split('\n')
    m = re.match(r'\s*(\d+)\s+(\d+\.\d+):', lines[0]) if lines and lines[0] else None
    if not m: continue
    t = float(m[2]) - start
    if not t0 <= t <= t1: continue
    n += 1; buckets[int((t - t0) / 0.025)] += 1
    names = set()
    for l in lines[1:]:
        f = re.match(r'^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)$', l)
        if f:
            sym = re.sub(r'\+0x[0-9a-f]+$', '', f[1]); sym = re.sub(r'\(.*', '', sym) if not sym.startswith('[') else sym
            names.add(sym if sym != '[unknown]' else '[' + os.path.basename(f[2]) + ']')
    inc.update(names)
period = 1000 / 2999
print(f'{comm} {t0}-{t1}s: {n} samples = {n * period:.0f} ms on CPU')
print('busy per 25 ms:', ' '.join(f'{t0 + b * 0.025:.3f}:{c * period / 25 * 100:.0f}%' for b, c in sorted(buckets.items())))
for name, c in inc.most_common(int(sys.argv[5]) if len(sys.argv) > 5 else 40):
    print(f'  {c * period:7.1f}  {name[:150]}')
