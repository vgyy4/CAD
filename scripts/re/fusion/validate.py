"""Check decoded 'new body' extrudes against the ACIS bodies of the same design:
the box of (sketch curve points, swept along the normal by the distance) must equal some body's
vertex box (both in the component's own coordinates).  validate.py OUTDIR samples..."""
import sys,os,json,math,hashlib; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
import f3dzip,fdesign,sab,f3dfeat as F
outdir=sys.argv[1]
WANT=set(os.environ.get('OPS','new').split(','))
tot=ok=0; okplane=0; rows=[]; ncont=0
for p in sys.argv[2:]:
    for k,files in f3dzip.designs(p).items():
        base=os.path.basename(p)+('__'+os.path.basename(k) if k else '')
        jf=os.path.join(outdir,base+'.json')
        if not os.path.exists(jf): continue
        j=json.load(open(jf))
        boxes=[]
        for n,b in files.items():
            if n.endswith('.smb') and 'Active' in n:
                try: bbs=sab.body_boxes(b)
                except Exception: bbs=[]
                for bb in bbs: boxes.append((n.split('/')[-1][5:13],bb['box']))
        sk={f['source_id']:f for f in j['features'] if f['type']=='sketch' and 'source_id' in f}
        for f in j['features']:
            if f['type']!='extrude' or f.get('operation') not in WANT or f.get('distance') is None or not f.get('sketch_source_id'): continue
            s=sk.get(f['sketch_source_id'])
            if not s or not s.get('plane'): continue
            used=set()
            for c in s['curves']:
                for kk in ('start','end','center'): 
                    if kk in c: used.add(c[kk])
            P={q['id']:(q['x'],q['y']) for q in s['points'] if q['id'] in used}
            # circles: add extreme points
            ext=[]
            for c in s['curves']:
                if c['type']=='circle' and c['center'] in P:
                    cx,cy=P[c['center']]; r=c['radius']; ext+= [(cx-r,cy-r),(cx+r,cy+r)]
            pts2=[v for kk,v in P.items() if any(c.get('center')!=kk or c['type']!='circle' for c in s['curves'])]+ext
            if not pts2: continue
            pl=s['plane']; o=pl['origin']; X=pl['x_axis']; Y=pl['y_axis']
            Z=[X[1]*Y[2]-X[2]*Y[1],X[2]*Y[0]-X[0]*Y[2],X[0]*Y[1]-X[1]*Y[0]]
            dist=f['distance']; lo,hi=(0,dist) if not f['symmetric'] else (-dist/2,dist/2)
            W=[]
            for (x,y) in pts2:
                for t in (lo,hi):
                    W.append([o[i]+x*X[i]+y*Y[i]+t*Z[i] for i in range(3)])
            mn=[min(w[i] for w in W) for i in range(3)]; mx=[max(w[i] for w in W) for i in range(3)]
            best=min((max(max(abs(mn[i]-10*bb[0][i]),abs(mx[i]-10*bb[1][i])) for i in range(3)),nm) for nm,bb in boxes) if boxes else (None,None)
            # in-plane check: the extents along the two in-plane world axes the plane spans
            axes=[i for i in range(3) if abs(Z[i])<1e-9]
            inpl=min((max(max(abs(mn[i]-10*bb[0][i]),abs(mx[i]-10*bb[1][i])) for i in axes),nm) for nm,bb in boxes) if boxes and axes else (None,None)
            contained=any(all(10*bb[0][i]-0.01<=mn[i] and mx[i]<=10*bb[1][i]+0.01 for i in range(3)) for nm,bb in boxes)
            ncont+=contained
            tot+=1; good=best[0] is not None and best[0]<0.01; ok+=good
            okp=inpl[0] is not None and inpl[0]<0.01; okplane+=okp
            rows.append((base[:40],f['name'],f['distance'],f['symmetric'],[round(x,3) for x in mn],[round(x,3) for x in mx],best[0] if best[0] is None else round(best[0],4),'OK' if good else ('PLANE-OK' if okp else '')))
for r in rows: print(*r)
print('contained in a body box:',ncont); print('extrudes checked',tot,'matching a body box within 0.01 mm:',ok,'; in-plane extents matching a body:',okplane)
