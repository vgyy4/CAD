"""Minimal binary ACIS (.smb) reader for ground-truth checks: records, per-body vertex boxes."""
import struct,re
SIZES={0x04:4,0x06:8,0x0c:4,0x13:24,0x14:24,0x15:4,0x0a:0,0x0b:0,0x0f:0,0x10:0,0x11:0,0x12:0,0x05:4,0x03:4,0x16:8}
def tokens(b,start,sizes=SIZES):
    i=start; rec=[]; out=[]
    while i<len(b):
        t=b[i]; i+=1
        if t in (0x07,0x0d,0x0e):
            L=b[i]; s=b[i+1:i+1+L]; i+=1+L; rec.append((t,s))
            if t==0x0e and False: pass
        elif t==0x08:
            L=struct.unpack_from('<H',b,i)[0]; i+=2+L; rec.append((t,None))
        elif t==0x09:
            L=struct.unpack_from('<I',b,i)[0]; i+=4+L; rec.append((t,None))
        elif t in sizes:
            n=sizes[t]; v=b[i:i+n]; i+=n; rec.append((t,v))
            if t==0x11: out.append(rec); rec=[]
        else:
            raise ValueError('tag %02x at %d'%(t,i-1))
    return out
def records(b):
    h=b.find(b'\x0d\x09asmheader')
    if h<0: raise ValueError('no header')
    if b.startswith(b'ASM BinaryFile8'):
        sz=dict(SIZES); sz[0x0c]=8; sz[0x04]=8; sz[0x15]=8
        return tokens(b,h,sz)
    return tokens(b,h)
def rtype(rec):
    names=[s.decode('latin1') for t,s in rec if t in (0x0d,0x0e) and s]
    return '-'.join(names)
def body_boxes(b):
    recs=records(b)
    types=[rtype(r) for r in recs]
    def ptrs(r): return [struct.unpack('<i' if len(v)==4 else '<q',v)[0] for t,v in r if t==0x0c]
    pos={}
    for k,r in enumerate(recs):
        if types[k].endswith('point'):
            for t,v in r:
                if t==0x13: pos[k]=struct.unpack('<3d',v)
    out=[]
    for k,t in enumerate(types):
        if t.endswith('body') and not t.endswith('_body'):
            seen={k}; st=[k]; pts=[]
            while st:
                x=st.pop()
                for p in ptrs(recs[x]):
                    if 0<=p<len(recs) and p not in seen and not types[p].endswith('body') and 'attrib' not in types[p] and 'transform' not in types[p]:
                        seen.add(p); st.append(p)
                        if p in pos: pts.append(pos[p])
            if pts:
                kinds={types[s].split('-')[-1] for s in seen}
                out.append(dict(solid=('face' in kinds and 'shell' in kinds), n=len(pts),
                    box=([min(q[i] for q in pts) for i in range(3)],[max(q[i] for q in pts) for i in range(3)])))
    return out
def points(b):
    out=[]
    for m in re.finditer(rb'\x0d\x05point',b):
        j=b.find(b'\x13',m.end(),m.end()+64)
        if j>=0 and j+25<=len(b):
            p=struct.unpack_from('<3d',b,j+1)
            if all(abs(x)<1e6 for x in p): out.append(p)
    return out
def bbox(pts):
    if not pts: return None
    return [min(p[k] for p in pts) for k in range(3)],[max(p[k] for p in pts) for k in range(3)]

def body_edges(b):
    """Per body: list of (p0, p1) vertex positions of its edges (cm, body coordinates)."""
    recs=records(b)
    types=[rtype(r) for r in recs]
    def ptrs(r): return [struct.unpack('<i' if len(v)==4 else '<q',v)[0] for t,v in r if t==0x0c]
    pos={}
    for k,r in enumerate(recs):
        if types[k].endswith('point'):
            for t,v in r:
                if t==0x13: pos[k]=struct.unpack('<3d',v); break
    vpt={}
    for k,t in enumerate(types):
        if t.endswith('vertex'):
            for p in ptrs(recs[k]):
                if p in pos: vpt[k]=pos[p]; break
    out=[]
    for k,t in enumerate(types):
        if t.endswith('body') and not t.endswith('_body'):
            seen={k}; st=[k]; edges=[]
            while st:
                x=st.pop()
                for p in ptrs(recs[x]):
                    if 0<=p<len(recs) and p not in seen and not types[p].endswith('body') and 'attrib' not in types[p] and 'transform' not in types[p]:
                        seen.add(p); st.append(p)
            for e in seen:
                if types[e].endswith('edge') and not types[e].endswith('coedge'):
                    vs=[vpt[p] for p in ptrs(recs[e]) if p in vpt]
                    if len(vs)>=2: edges.append((vs[0],vs[1],types[e]))
            if edges: out.append(edges)
    return out

def tagged_faces(b):
    """[(body index, generic tag text, surface type, [vertex points])] for every face carrying a
    generic_tag_attrib_def attribute (Fusion's face generator name)."""
    recs=records(b)
    types=[rtype(r) for r in recs]
    def ptrs(r): return [struct.unpack('<i' if len(v)==4 else '<q',v)[0] for t,v in r if t==0x0c]
    pos={}
    for k,r in enumerate(recs):
        if types[k].endswith('point'):
            for t,v in r:
                if t==0x13: pos[k]=struct.unpack('<3d',v); break
    tag={}
    for k,r in enumerate(recs):
        if 'attrib' in types[k]:
            strs=[v.decode('latin1') for t,v in r if t==0x07 and v]
            if strs and strs[0]=='generic_tag_attrib_def':
                o=[p for p in ptrs(r) if 0<=p<len(recs) and 'attrib' not in types[p]]
                if o and types[o[0]].endswith('face'): tag[o[0]]=strs[-1]
    vpt={}
    for k,t in enumerate(types):
        if t.endswith('vertex'):
            for p in ptrs(recs[k]):
                if p in pos: vpt[k]=pos[p]; break
    ce_loop={}
    for k,t in enumerate(types):
        if t.endswith('coedge'):
            for p in ptrs(recs[k]):
                if 0<=p<len(recs) and types[p].endswith('loop'): ce_loop.setdefault(p,[]).append(k)
    def coedges_of_loop(loops):
        return [c for l in loops for c in ce_loop.get(l,[])]
    out=[]
    bi=0
    for k,t in enumerate(types):
        if not (t.endswith('body') and not t.endswith('_body')): continue
        seen={k}; st=[k]
        while st:
            x=st.pop()
            for p in ptrs(recs[x]):
                if 0<=p<len(recs) and p not in seen and not types[p].endswith('body') and 'attrib' not in types[p] and 'transform' not in types[p]:
                    seen.add(p); st.append(p)
        for f in seen:
            if f in tag:
                loops={p for p in ptrs(recs[f]) if 0<=p<len(recs) and types[p].endswith('loop')}
                vs=set()
                for ce in coedges_of_loop(loops):
                    for p in ptrs(recs[ce]):
                        if 0<=p<len(recs) and types[p].endswith('edge') and not types[p].endswith('coedge'):
                            for q in ptrs(recs[p]):
                                if q in vpt: vs.add(vpt[q])
                surf=[types[p] for p in ptrs(recs[f]) if 0<=p<len(recs) and types[p].endswith('surface')]
                out.append((bi,tag[f],surf[0] if surf else None,sorted(vs)))
        bi+=1
    return out

def cylinders(b):
    """[(root, unit axis, radius)] of every cylindrical cone-surface record."""
    out=[]
    for r in records(b):
        if not rtype(r).endswith('cone-surface'): continue
        pos=[struct.unpack('<3d',v) for t,v in r if t==0x13]
        vec=[struct.unpack('<3d',v) for t,v in r if t==0x14]
        dbl=[struct.unpack('<d',v)[0] for t,v in r if t==0x06]
        if not pos or len(vec)<2 or len(dbl)<3: continue
        sin,cos=dbl[1],dbl[2]
        if abs(sin)>1e-9: continue
        a=vec[0]; n=sum(x*x for x in a)**0.5
        rad=sum(x*x for x in vec[1])**0.5
        out.append((pos[0],tuple(x/n for x in a),rad))
    return out
