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
