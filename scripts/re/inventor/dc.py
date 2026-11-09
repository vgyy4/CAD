"""PmDCSegment model: records, header parsing, refs, class names (from sample bytes)."""
import sys,os,struct,collections
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from rse import segments,typed
CLS={ # GUID prefix -> role (derived from samples; see LAYOUT.md)
 '2b48a42b':'Name','90874d91':'PartFeature','20673244':'PatternFeature','90874d11':'Sketch',
 'ce52df35':'SkPoint','ce52df3a':'SkLine','ce52df3b':'SkCircle','ce52df42':'WorkPlane',
 '8ef06c89':'WorkAxis','ce52df3e':'WorkPoint','90874d26':'Param','90874d18':'SkPlacement',
 'ce52df40':'SkDir','3d64ccf0':'SkOnPlane','90874d23':'SkOnFace','90874d63':'Doc',
 'f645595c':'HistoryBlob','90874d62':'ConstraintSet',
}
def ref(v,n):
    return (v&0x7fffffff)-1 if v&0x80000000 and 0<(v&0x7fffffff)<=n else None
class DC:
    def __init__(self,D):
        segs=segments(D); meta,b,_=segs['PmDCSegment']
        self.T,self.cl=typed(meta,b); self.n=len(self.T)
        self.g=[t[3] for t in self.T]; self.r=[t[4] for t in self.T]
        self.role=[CLS.get(x[:8],x[:8]) for x in self.g]
        self.ver=None
        for i,ro in enumerate(self.role):
            if ro=='PartFeature': self.ver=struct.unpack_from('<I',self.r[i],22)[0]; break
    def refs(self,i):
        r=self.r[i]; out=[]
        for k in range(0,len(r)-3):
            x=ref(struct.unpack_from('<I',r,k)[0],self.n)
            if x is not None and r[k+3]&0x80: out.append((k,x))
        return out
    def body(self,i):
        """Offset where the class body starts: header = u32 0,u16,u32 nameref,4 flag bytes,
        u32 owner, u32 seq, then optional u32 0xffffffff (any number) and u32 version."""
        r=self.r[i]; k=22
        if self.ver:
            for j in range(3):
                q=k+4*j
                if q+4<=len(r) and struct.unpack_from('<I',r,q)[0]==self.ver and all(struct.unpack_from('<I',r,k+4*t)[0] in (0,0xffffffff) for t in range(j)):
                    return q+4
        while k+4<=len(r) and struct.unpack_from('<I',r,k)[0]==0xffffffff: k+=4
        return k
    def name(self,i):
        """Name record pointing at i (Name records hold u32 ref at 14)."""
        r=self.r[i]; v=ref(struct.unpack_from('<I',r,6)[0],self.n) if len(r)>=10 else None
        if v is not None and self.role[v]=='Name': return self.namestr(v)
        return None
    def namestr(self,j):
        r=self.r[j]
        for q in range(0x1a,len(r)-4):
            L=struct.unpack_from('<I',r,q)[0]
            if 0<L<200 and q+4+2*L<=len(r):
                try: s=r[q+4:q+4+2*L].decode('utf-16le')
                except Exception: continue
                tail=r[q+4+2*L:]
                if s.isprintable() and (tail.strip(b'\0')==b'' or len(tail)==16): return s
        return None
    def nametype(self,j):
        r=self.r[j]; s=self.namestr(j)
        if s is None: return None
        import uuid
        tail=r[-16:]
        return str(uuid.UUID(bytes_le=tail)) if tail.strip(b'\0') and r[-16-2:-16]!=b'\0\0' else None
    def dbl(self,i,off,n): return list(struct.unpack_from('<%dd'%n,self.r[i],off))

FEATTYPES={'0ca91131':'extrude','0054015f':'revolve','1f757d1a':'hole','28aa1906':'thread',
 '7281f4ec':'thicken','767ac2da':'rect_pattern','f1f715dc':'fillet','f900713f':'chamfer'}
def placement(d,i):
    """4x4 row-major matrix of a SkPlacement record: u32 mask (bit k: entry k is +-1;
    bit 16+k: entry k is 0; both: -1), then f64 for every other entry, in order, to the end
    of the record. The mask's offset is found from the record length."""
    r=d.r[i]
    for b in range(22,40,4):
        if b+4>len(r): break
        m=struct.unpack_from('<I',r,b)[0]
        n=sum(1 for e in range(16) if not ((m>>e)&1 or (m>>(16+e))&1))
        if b+4+8*n==len(r) and (m>>15)&1: break
    else:
        b=d.body(i); m=struct.unpack_from('<I',r,b)[0]
    k=b+4; M=[]
    for e in range(16):
        one=(m>>e)&1; zero=(m>>(16+e))&1
        if one and zero: M.append(-1.0)
        elif one: M.append(1.0)
        elif zero: M.append(0.0)
        else: M.append(struct.unpack_from('<d',r,k)[0]); k+=8
    return M
def skdir(d,i):
    """SkDir: five f64 at the end of the record; the last three are the sketch normal."""
    r=d.r[i]; return list(struct.unpack_from('<5d',r,len(r)-40))
def slots(d,i):
    """Feature slot list: the first 0x30000002 list in the body (u32 tag, u32 n, u32 n, u32 0x10?)."""
    r=d.r[i]; b=d.body(i)
    k=r.find(b'\x02\x00\x00\x30',b)
    n=struct.unpack_from('<I',r,k+4)[0]
    out=[]
    for j in range(n):
        v=struct.unpack_from('<I',r,k+16+4*j)[0]
        out.append(ref(v,d.n) if v else None)
    return out
def param(d,i):
    """Parameter record: ... u32 n, UTF-16 name (n chars), u32 0, u32, u32, f64 value, f64 value,
    u16 0, u16 0xffff. Anchored at the end (the header before the name varies by version)."""
    r=d.r[i]; val=struct.unpack_from('<d',r,len(r)-20)[0]
    name=''
    for q in range(22,len(r)-36):
        L=struct.unpack_from('<I',r,q)[0]
        if 0<L<64 and q+4+2*L+12==len(r)-20:
            try: name=r[q+4:q+4+2*L].decode('utf-16le')
            except Exception: pass
            break
    return name,val
def features(d):
    out=[]
    for i in range(d.n):
        if d.role[i] in ('PartFeature','PatternFeature'):
            nm=None; typ=None
            v=ref(struct.unpack_from('<I',d.r[i],6)[0],d.n)
            if v is not None and d.role[v]=='Name':
                nm=d.namestr(v); t=d.nametype(v); typ=FEATTYPES.get(t[:8],t) if t else None
            out.append((i,nm,typ))
    return out

def seq(d, i):
    return struct.unpack_from('<I', d.r[i], 18)[0]

def ordered_features(d):
    """Features in history order: the order of their ids (u32 at offset 18) in the
    452121b6 record (one per part)."""
    F = features(d)
    ids = {seq(d, i): (i, nm, t) for i, nm, t in F}
    o = [i for i in range(d.n) if d.g[i].startswith('452121b6')]
    if not o: return F
    r = d.r[o[0]]; out = []
    for k in range(0, len(r) - 3):
        v = struct.unpack_from('<I', r, k)[0]
        if v in ids and ids[v] not in out: out.append(ids[v])
    return out + [f for f in F if f not in out]
