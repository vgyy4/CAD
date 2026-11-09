"""Rebuild growing prefixes of a tree and print the box after each solid feature."""
import json,sys,subprocess,re,os
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from stlbox import stl
t=json.load(open(sys.argv[1])); F=t['features']
for i,f in enumerate(F):
    if f['type'] in ('sketch','unsupported'): continue
    t2=dict(t); t2['features']=[g for g in F[:i+1] if g['type']!='unsupported']
    json.dump(t2,open('rebuild/p.gtree.json','w'))
    if os.path.exists('rebuild/p.stl'): os.remove('rebuild/p.stl')
    o=subprocess.run(['bin/gusset','build','truth/empty.gus.txt','--import','rebuild/p.gtree.json','--export','rebuild/p.stl'],capture_output=True,text=True)
    v=re.findall(r'volume ([\d.-]+)',o.stdout)
    box=None
    if os.path.exists('rebuild/p.stl'):
        lo,hi,_=stl('rebuild/p.stl'); box=[round(x,2) for x in lo+hi]
    print('%-22s %-8s %-10s %s %s'%(f['name'][:22],f['type'],v,box,(f.get('sketch'),f.get('distance'),f.get('operation'))))
