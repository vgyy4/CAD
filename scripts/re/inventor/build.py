"""Decode, rebuild with gusset, and swap holes Gusset refuses for their plain-cut form.
Usage: python3 -I build.py OUTDIR truth.json DUMPDIR/part..."""
import sys, os, json, subprocess, re, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from iptfeat import decode
G = '/home/user/CAD/target/debug/gusset'; HERE = os.path.dirname(os.path.abspath(__file__))
def run(t, path):
    g = path + '.gtree.json'
    clean = {k: v for k, v in t.items()}
    clean['features'] = [{k: v for k, v in f.items() if not k.startswith('_')} for f in t['features']]
    json.dump(clean, open(g, 'w'), indent=1)
    p = subprocess.run([G, 'build', os.path.join(HERE, 'truth/empty.gus.txt'), '--import', g], capture_output=True, text=True, timeout=900)
    o = p.stdout + p.stderr
    ok = set(re.findall(r'^\s+ok\s+(\S.*?)\s*$', o, re.M))
    return ok, o
od = sys.argv[1]; os.makedirs(od, exist_ok=True)
for D in sys.argv[3:]:
    name = os.path.basename(D.rstrip('/'))
    try: t = decode(D)
    except Exception as e: print(name, 'DECODE ERROR', e); continue
    path = os.path.join(od, name)
    ok, o = run(t, path); changed = False
    for it in range(6):
        nf = []; ch = False
        for f in t['features']:
            if f['type'] == 'hole' and f['name'] not in ok and f.get('_fallback'):
                nf += f['_fallback']; ch = True
            else: nf.append(f)
        if not ch: break
        changed = True; t['features'] = nf; ok, o = run(t, path)
    open(path + '.log', 'w').write(o)
    print(name, 'fallback' if changed else '', 'not built:', [f.get('name') for f in t['features'] if f['type'] != 'unsupported' and f.get('name') not in ok and not f['name'].endswith('_sketch')])
