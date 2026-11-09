"""Rebuild each tree with the gusset binary and compare with the part's own import.
Usage: python3 -I rebuild.py truth/truth.json out/*.json"""
import sys, os, json, subprocess, re, shutil, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from stlbox import stl
HERE = os.path.dirname(os.path.abspath(__file__)); G='/home/user/CAD/target/debug/gusset'
truth = json.load(open(sys.argv[1])); res = {}
d = os.path.join(HERE, 'rebuild'); os.makedirs(d, exist_ok=True)
for fn in sys.argv[2:]:
    name = os.path.basename(fn)[:-5]
    if name.endswith('.gtree'): name = name[:-6]
    if name not in truth: continue
    t = json.load(open(fn)); T = truth[name]
    uns = [f.get('name') for f in t['features'] if f['type'] == 'unsupported']
    g = os.path.join(d, name + '.gtree.json'); shutil.copy(fn, g)
    stlp = os.path.join(d, name + '.stl')
    if os.path.exists(stlp): os.remove(stlp)
    p = subprocess.run([G, 'build', os.path.join(HERE, 'truth/empty.gus.txt'), '--import', g, '--export', stlp], capture_output=True, text=True, timeout=900)
    out = p.stdout + p.stderr
    open(os.path.join(d, name + '.log'), 'w').write(out)
    bad = [l.strip() for l in out.splitlines() if re.match(r'\s*(skip|fail|error|Error|warn)', l) or 'not imported' in l or 'skipped' in l]
    vols = [float(x) for x in re.findall(r'body .*?: volume (-?[\d.]+)', out)]
    r = dict(unsupported=uns, problems=bad[:8], volume=sum(vols) if vols else None, truth=T['volume'])
    if os.path.exists(stlp) and vols:
        lo, hi, _ = stl(stlp); diag = math.dist(T['min'], T['max'])
        r['box_err'] = max(max(abs(a - b) for a, b in zip(lo, T['min'])), max(abs(a - b) for a, b in zip(hi, T['max']))) / diag
        r['vol_rel'] = abs(r['volume'] - T['volume']) / abs(T['volume'])
    r['match'] = bool(r.get('vol_rel') is not None and r['vol_rel'] < 2e-5 and r['box_err'] < 1e-4)
    res[name] = r
    print('%-40s %-5s vol %s/%s rel %s box %s uns %d %s' % (name[:40], 'MATCH' if r['match'] else '', r['volume'], T['volume'],
          '%.2e' % r['vol_rel'] if r.get('vol_rel') is not None else '-', '%.1e' % r['box_err'] if 'box_err' in r else '-',
          len(uns), '; '.join(bad)[:300]), flush=True)
json.dump(res, open(os.path.join(d, 'results.json'), 'w'), indent=1)
print('matching:', sum(v['match'] for v in res.values()), 'of', len(res))
