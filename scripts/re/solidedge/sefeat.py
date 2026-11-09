"""Solid Edge .par -> Gusset feature-tree JSON v2 (prototype; python3 -I sefeat.py out_dir part.par...)"""
import sys,os,json,math,struct,itertools
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from sebuild import *
from sevars import variables
TYPES={'JMProtrusionFunction':'Protrusion','JMCutoutFunction':'Cutout','JMRevolvedCutoutFunction':'RevolvedCutout','JMRevolvedProtrusionFunction':'RevolvedProtrusion',
       'JMHoleFunction':'Hole','JMThreadFunction':'Thread','JMUserDefinedPatternFunction':'UserDefinedPattern'}
def after_bbox(b):
    i=b.find(b'\x5a\xbb\x04\x00')
    if i<0: return None
    for k in range(20,44,4):
        try: v=struct.unpack_from('<6d',b,i+4+k)
        except struct.error: continue
        if all(x==0 or 1e-9<abs(x)<10 for x in v) and all(v[j]<=v[j+3]+1e-12 for j in range(3)) and any(x!=0 for x in v): return v
def flag5(b):
    i=b.find(b'\x5a\xbb\x04\x00'); return struct.unpack_from('<I',b,i+4+16)[0] if i>=0 else None
def chain(fs):
    byid={f[0]:f for f in fs}; prev={}
    for f in fs:
        b=f[4]; r=refs(b); prev[f[0]]=r[0] if r else 0
    nxt={}
    for k,p in prev.items(): nxt.setdefault(p,[]).append(k)
    out=[]; cur=[k for k,p in prev.items() if p not in byid]
    seen=set()
    while cur:
        k=min(cur); cur=[x for x in cur if x!=k]
        if k in seen: continue
        seen.add(k); out.append(k); cur+=nxt.get(k,[])
    out+= [k for k in sorted(byid) if k not in seen]
    return out
def canon_plane(o,x,y,n):
    """axis-aligned canonical frame when n is a principal axis; origin = foot of world origin"""
    dn=dot(o,n); org=[n[i]*dn for i in range(3)]
    return org,x,y,n
def build(path):
    s,c,ob,d=load(path)
    fs=[(sid,nm,a,e,c[a:e]) for sid,nm,a,e in functions(c,ob)]
    order=chain(fs); byid={f[0]:f for f in fs}
    pr=profiles(s) if 'SKETCH.P_B' in s else {}
    vv={}
    for nm_,ser_,tg_,val_ in variables(c,d):
        if val_ is not None: vv.setdefault(tg_,{})[nm_]=r12(val_*1000)
    feats=[]; prev_bb=None; solid=False; first=None; notes=[]
    for sid in order:
        _,jn,a,e,b=byid[sid]; nm=TYPES.get(jn,jn); r=refs(b)
        bb=after_bbox(b)
        prof=next((x for x in r[4:12] if x in pr),None)
        rel=next((relation(c,d,x) for x in r[:6] if x in d and d[x][3]-d[x][2]==466),None)
        fname='%s_%d'%(nm,sid)
        if nm in('Protrusion','Cutout','RevolvedCutout','RevolvedProtrusion') and prof is not None:
            ents=pr[prof]; pl=plane_of(ents); o,x,y,n=pl
            sk=sketch_json('Sketch_%d'%prof,ents,pl); feats.append(sk)
            if nm in('Protrusion','Cutout') and rel is not None:
                cand=[]
                for sg in (1,-1):
                    p0=[o[i]+0 for i in range(3)]; p1=[o[i]+sg*rel*n[i] for i in range(3)]
                    cand.append((sg,p1))
                f5=flag5(b)
                tol=2e-6
                def inside(p,B): return all(B[j]-tol<=p[j]<=B[j+3]+tol for j in range(3))
                ok=[]
                for sg,p1 in cand:
                    if nm=='Protrusion' and bb and inside(p1,bb): ok.append(sg)
                    if nm=='Cutout' and prev_bb and inside(p1,prev_bb): ok.append(sg)
                if len(ok)==1: sg=ok[0]; how='bbox'
                else: sg=-1 if f5==1 else 1; how='flag5=%s'%f5
                op='new' if not solid else ('cut' if nm=='Cutout' else 'join')
                feats.append(dict(type='extrude',name=fname,sketch=sk['name'],profiles='all',distance=r12(sg*rel*1000),symmetric=False,operation=op,
                                  target=None if op=='new' else first,_dir_by=how))
                solid=True; first=first or fname
            elif nm.startswith('Revolved'):
                # axis: a profile line on a principal world axis through the origin (heuristic)
                ax=None
                for en in ents:
                    if en['kind']=='LINE':
                        dr=en['cu']['direction']; pv=en['cu']['pvec']
                        for a_ in range(3):
                            if abs(abs(dr[a_])-1)<1e-9 and all(abs(pv[k])<1e-9 for k in range(3) if k!=a_):
                                ax=dict(origin=[0,0,0],direction=[1.0 if k==a_ else 0.0 for k in range(3)])
                feats.append(dict(type='revolve',name=fname,sketch=sk['name'],profiles='all',angle=360.0,axis=ax,operation='new' if not solid else ('cut' if nm=='RevolvedCutout' else 'join'),
                                  target=None if not solid else first,_axis_heuristic=True))
                solid=True; first=first or fname
        else:
            feats.append(dict(type='unsupported',name=fname,original_type=jn,dims_mm=vv.get(sid,{})))
        if bb: prev_bb=bb
    # sketches only (sync parts)
    used={f['name'] for f in feats if f['type']=='sketch'}
    for k,en in pr.items():
        if 'Sketch_%d'%k not in used:
            try: feats.append(sketch_json('Sketch_%d'%k,en))
            except Exception as ex: feats.append(dict(type='unsupported',name='Sketch_%d'%k,original_type='sketch (%s)'%ex))
    return dict(format='gusset-feature-tree',version=2,source=dict(app='solidedge'),features=feats)
if __name__=='__main__':
    out=sys.argv[1]; os.makedirs(out,exist_ok=True)
    for p in sys.argv[2:]:
        t=build(p); n=os.path.basename(p)[:-4]
        json.dump(t,open(os.path.join(out,n+'.json'),'w'),indent=1)
        print(n,[ (f['type'],f.get('operation'),f.get('distance')) for f in t['features'] if f['type']!='sketch'][:14])
