"""Ground truth for extrude face generators: ACIS faces tagged with the generator text
(generic_tag_attrib_def) whose vertices lie on exactly one profile curve's swept surface."""
import sys,os,hashlib,collections,math; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
import f3dzip,fdesign,f3dfeat as F,naming,regions,sab,edgeprobe as E

def on_sweep(G,sg,pts,tol=2e-5):
    """All points lie on the surface swept by segment sg (line: plane; arc: cylinder)."""
    pl=G['pl']; s=G['sketch']; q=sg['curve']
    # to sketch coordinates
    loc=[]
    for p in pts:
        w=E.sub(p,pl['origin']); loc.append((E.dot(w,pl['x']),E.dot(w,pl['y']),E.dot(w,pl['z'])))
    if G['t0'] is not None:
        lo,hi=min(G['t0'],G['t1']),max(G['t0'],G['t1'])
        if any(z<lo-1e-5 or z>hi+1e-5 for x,y,z in loc): return False
    if q in s['lines']:
        a,b=sg['poly'][0],sg['poly'][-1]
        dx,dy=b[0]-a[0],b[1]-a[1]; L=math.hypot(dx,dy)
        if L<1e-12: return False
        return all(abs((x-a[0])*dy-(y-a[1])*dx)/L<tol for x,y,z in loc)
    if q in s['arcs']:
        A=s['arcs'][q]; c=A['center']; r=A['radius']
        return all(abs(math.hypot(x-c[0],y-c[1])-r)<tol for x,y,z in loc)
    return False

def truth(G,faces,gens):
    out={}
    for g in gens:
        cands=set()
        for bi,t,surf,vs in faces:
            if t!=g or len(vs)<2: continue
            hit=[sg['curve'] for sg in G['segs'] if on_sweep(G,sg,vs)]
            if len(hit)==1: cands.add(hit[0])
        if len(cands)==1: out[g]=cands.pop()
    return out

if __name__=='__main__':
    seen=set(); stats=collections.Counter(); rows=[]
    for p in sys.argv[1:]:
        for k,files in f3dzip.designs(p).items():
            faces=None
            for pre,(m,b) in fdesign.find_streams(files).items():
                if 'Active' not in pre or not ('Design1' in pre or 'DesignSeg' in pre): continue
                h=hashlib.md5(b).hexdigest()
                if h in seen: continue
                seen.add(h)
                d=fdesign.Design(m,b)
                om=naming.op_map(F,d); gl=naming.gen_lists(F,d)
                for op,gens in gl.items():
                    fid=om.get(op)
                    if fid is None or F.cls(d,fid)!=F.C['EXTRUDE']: continue
                    G=E.ext_geom(d,fid)
                    if not G: continue
                    if faces is None:
                        faces=[]
                        for n,bb in files.items():
                            if n.endswith('.smb') and 'Active' in n:
                                try: faces+=sab.tagged_faces(bb)
                                except Exception: pass
                    sides=sorted([g for g in gens if g not in ('1','2')],key=int)
                    T=truth(G,faces,sides)
                    stats['extrude ops']+=1; stats['side gens']+=len(sides); stats['side gens resolved']+=len(T)
                    rows.append((os.path.basename(p)[:14],fid,F.feature_name(d,fid)[1],op,sides,[sg['curve'] for sg in G['segs']],T))
    for r in rows: print(*r)
    print(dict(stats))
