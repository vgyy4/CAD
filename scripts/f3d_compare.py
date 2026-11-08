#!/usr/bin/env python3
"""Per-body check of Gusset's Fusion import against the STEP its author exported.

    GUSSET_F3D_DUMP=<dir> GUSSET_F3D_FAST=1 GUSSET_F3D_SAMPLES=<samples> \
        cargo test -p gusset-cli --test fusion_samples -- --nocapture
    python3 -I scripts/f3d_compare.py <samples>/<name>.step <dir>/<name>.f3z [tolerance-mm]

The test writes the STEP text of every body it imports (placed as the import places it) to
<dir>/<file>/NNNN.step. This script reads the author's STEP (assembly tree: NEXT_ASSEMBLY_USAGE_
OCCURRENCE with ITEM_DEFINED_TRANSFORMATION), puts every solid at its world position, and
matches the solids by the box of their vertices. It prints how many of the author's solids have a
body of ours within the tolerance (default 0.05 mm) and how many are within 0.5 mm by centre.
Needs numpy. The samples are untrusted data: this only reads them as text.
"""
import re,sys,os,glob
import numpy as np
def load(path):
    t=open(path,errors='replace').read().replace('\n','')
    ents={}
    for m in re.finditer(r'#(\d+)\s*=\s*(.*?);(?=\s*#|\s*ENDSEC)',t):
        body=m.group(2).strip()
        mm=re.match(r'([A-Z_0-9]+)\s*\((.*)\)$',body,re.S)
        if mm: ents[int(m.group(1))]=(mm.group(1),mm.group(2))
        else: ents[int(m.group(1))]=('COMPLEX',body)
    return ents
def refs(s): return [int(x) for x in re.findall(r'#(\d+)',s)]
def axis(ents,i):
    _,a=ents[i]; r=refs(a)
    p=[float(x) for x in re.findall(r'[-+0-9.E]+',ents[r[0]][1].split('(',1)[1])]
    d=lambda j:np.array([float(x) for x in re.findall(r'-?[0-9.]+(?:E[-+]?\d+)?',ents[j][1].split('(',1)[1])])
    z=d(r[1]); x=d(r[2]); z/=np.linalg.norm(z); x/=np.linalg.norm(x); y=np.cross(z,x)
    M=np.eye(4); M[:3,0]=x;M[:3,1]=y;M[:3,2]=z;M[:3,3]=p; return M
def tree(path):
    e=load(path)
    pd={}
    for k,(n,a) in e.items():
        if n=='PRODUCT_DEFINITION':
            pdf=refs(a)[0]; prod=refs(e[pdf][1])[0]
            pd[k]=re.match(r"'([^']*)'",e[prod][1]).group(1)
    nauo={}
    for k,(n,a) in e.items():
        if n=='NEXT_ASSEMBLY_USAGE_OCCURRENCE':
            r=refs(a); nm=re.findall(r"'([^']*)'",a)
            nauo[k]=(nm[0],r[0],r[1])
    out=[]
    for k,(n,a) in e.items():
        if n=='CONTEXT_DEPENDENT_SHAPE_REPRESENTATION':
            r=refs(a)   # rep_relationship, pds
            rr=e[r[0]]; 
            pds=e[r[1]]; pr=refs(pds[1])  # PRODUCT_DEFINITION_SHAPE -> NAUO
            nid=[x for x in pr if x in nauo]
            ids=[x for x in refs(rr[1]) if e.get(x,('',''))[0]=='ITEM_DEFINED_TRANSFORMATION']
            if not ids and rr[0]=='COMPLEX': ids=[x for x in refs(rr[1]) if e[x][0]=='ITEM_DEFINED_TRANSFORMATION']
            if nid and ids:
                ir=refs(e[ids[0]][1])
                A=axis(e,ir[0]);B=axis(e,ir[1])
                out.append((nauo[nid[0]],A,B))
    return e,pd,out

TOPO={'MANIFOLD_SOLID_BREP','CLOSED_SHELL','ADVANCED_FACE','FACE_OUTER_BOUND','FACE_BOUND','EDGE_LOOP','ORIENTED_EDGE','EDGE_CURVE','VERTEX_POINT','OPEN_SHELL','SHELL_BASED_SURFACE_MODEL'}
def num3(s):
    return np.array([float(x) for x in re.findall(r'-?\d+\.?\d*(?:[Ee][-+]?\d+)?',s)][:3])
def solids(e):
    """solid id -> Nx3 vertex array (local)"""
    pts={}
    out={}
    for k,(n,a) in e.items():
        if n=='MANIFOLD_SOLID_BREP' or n=='BREP_WITH_VOIDS':
            seen=set();stack=[k];vs=set()
            while stack:
                i=stack.pop()
                if i in seen or i not in e: continue
                seen.add(i)
                t,aa=e[i]
                if t not in TOPO and i!=k: continue
                if t=='VERTEX_POINT':
                    vs.add(refs(aa)[0]); continue
                stack.extend(refs(aa))
            arr=[]
            for p in vs:
                if p not in pts: pts[p]=num3(e[p][1].split('(',1)[1])
                arr.append(pts[p])
            if arr: out[k]=np.array(arr)
    return out
def gt(path):
    e,pd,out=tree(path)
    sol=solids(e)
    # rep -> product via SHAPE_DEFINITION_REPRESENTATION
    pds_prod={}
    for k,(n,a) in e.items():
        if n=='PRODUCT_DEFINITION_SHAPE':
            r=refs(a)
            for x in r:
                if x in pd: pds_prod[k]=x
    rep_prod={}
    for k,(n,a) in e.items():
        if n=='SHAPE_DEFINITION_REPRESENTATION':
            r=refs(a)
            if r[0] in pds_prod: rep_prod[r[1]]=pds_prod[r[0]]
    for k,(n,a) in e.items():
        if n=='SHAPE_REPRESENTATION_RELATIONSHIP' or (n=='COMPLEX' and 'SHAPE_REPRESENTATION_RELATIONSHIP' in a):
            r=[x for x in refs(a) if x in e]
            if len(r)>=2:
                if r[0] in rep_prod: rep_prod.setdefault(r[1],rep_prod[r[0]])
                elif r[1] in rep_prod: rep_prod.setdefault(r[0],rep_prod[r[1]])
    prod_sol={}
    for k,(n,a) in e.items():
        if n in('SHAPE_REPRESENTATION','ADVANCED_BREP_SHAPE_REPRESENTATION','MANIFOLD_SURFACE_SHAPE_REPRESENTATION') or n=='COMPLEX':
            if k in rep_prod:
                for x in refs(a):
                    if x in sol: prod_sol.setdefault(rep_prod[k],[]).append(x)
    children={}
    for (nm,par,chi),A,B in out:
        children.setdefault(par,[]).append((chi,B@np.linalg.inv(A)))
    allc={c for v in children.values() for c,_ in v}
    roots=[p for p in pd if p not in allc and (p in children or p in prod_sol)]
    res=[]
    def walk(p,T,depth=0):
        for s in prod_sol.get(p,[]):
            v=sol[s]; w=(T[:3,:3]@v.T).T+T[:3,3]; res.append(w)
        if depth>12: return
        for c,t in children.get(p,[]): walk(c,T@t,depth+1)
    for r in roots: walk(r,np.eye(4))
    return res
def ours(d):
    res=[]
    for f in sorted(glob.glob(d+'/*.step')):
        e=load(f)
        for s,v in solids(e).items(): res.append(v)
    return res
def sig(v): return np.concatenate([v.min(0),v.max(0)])
def main():
    g=gt(sys.argv[1]); o=ours(sys.argv[2])
    gs=[sig(v) for v in g]; os_=[sig(v) for v in o]
    tol=float(sys.argv[3]) if len(sys.argv)>3 else 0.05
    def cover(A,B):
        ok=0;near=[]
        for a in A:
            best=min((np.abs(a-b).max() for b in B),default=None)
            near.append(best)
            if best is not None and best<tol: ok+=1
        return ok,near
    okg,ng=cover(gs,os_)
    oko,no=cover(os_,gs)
    # looser: the centre of the vertex box within 0.5 mm (a body Fusion's STEP shapes slightly differently)
    def cen(a): return (a[:3]+a[3:])/2
    loose=sum(1 for a in gs if min((np.linalg.norm(cen(a)-cen(b)) for b in os_),default=9e9)<0.5)
    bad=[x for x in ng if x is not None and x>=tol]
    print(f'GT solids {len(gs)}: covered {okg} (centre within 0.5 mm: {loose}); ours {len(os_)}: matched {oko}'+('; GT misses: median %.2f mm, max %.2f'%(np.median(bad),max(bad)) if bad else ''))
if __name__=='__main__': main()
