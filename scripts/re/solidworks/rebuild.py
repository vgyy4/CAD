"""Rebuild each tree with the copied gusset binary and compare to the Parasolid body.
Usage: python3 -I rebuild.py truth/truth.json out/*.json"""
import sys, os, json, subprocess, re, shutil
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from stlbox import stl
HERE = os.path.dirname(os.path.abspath(__file__))
truth = json.load(open(sys.argv[1]))
res = {}
for fn in sys.argv[2:]:
    name = os.path.basename(fn)[:-5]
    if name == 'summary' or name not in truth: continue
    t = json.load(open(fn))
    unresolved = [f['name'] for f in t['features'] if f.get('edges_unresolved') or f['type'] == 'unsupported'
                  or (f['type'] == 'extrude' and f.get('distance') is None) or (f['type'] == 'sketch' and f['plane'].get('unresolved'))]
    d = os.path.join(HERE, 'rebuild'); os.makedirs(d, exist_ok=True)
    g = os.path.join(d, name + '.gtree.json'); shutil.copy(fn, g)
    stlp = os.path.join(d, name + '.stl')
    if os.path.exists(stlp): os.remove(stlp)
    p = subprocess.run([os.path.join(HERE, 'bin/gusset'), 'build', os.path.join(HERE, 'truth/empty.gus.txt'), '--import', g, '--export', stlp],
                       capture_output=True, text=True, timeout=600)
    out = p.stdout + p.stderr
    bad = [l.strip() for l in out.splitlines() if l.strip().startswith(('skip', 'fail', 'error', 'Error')) or 'not imported' in l]
    vols = [float(x) for x in re.findall(r'body .*?: volume (-?[\d.]+)', out)]
    tv = truth[name].get('parasolid_volume', [None])[0]
    r = dict(unresolved=unresolved, problems=bad[:6], volume=sum(vols) if vols else None, truth=tv)
    if os.path.exists(stlp) and vols:
        lo, hi, _ = stl(stlp)
        r['box_err'] = max(max(abs(a - b) for a, b in zip(lo, truth[name]['min'])), max(abs(a - b) for a, b in zip(hi, truth[name]['max'])))
        r['vol_rel'] = abs(r['volume'] - tv) / abs(tv) if tv else None
    r['match'] = bool(r.get('vol_rel') is not None and r['vol_rel'] < 1e-4 and r['box_err'] < 0.01)
    res[name] = r
    print('%-32s %-5s vol %s/%s rel %s box %s unresolved %d %s' % (name, 'MATCH' if r['match'] else '', r['volume'], tv,
          '%.2e' % r['vol_rel'] if r.get('vol_rel') is not None else '-', '%.3f' % r['box_err'] if 'box_err' in r else '-',
          len(unresolved), '; '.join(bad)[:150]))
json.dump(res, open(os.path.join(HERE, 'rebuild', 'results.json'), 'w'), indent=1)
z = [k for k, v in res.items() if not v['unresolved']]
print('parts with zero unresolved features:', len(z), '; of those matching:', sum(res[k]['match'] for k in z), '; matching overall:', sum(v['match'] for v in res.values()))
