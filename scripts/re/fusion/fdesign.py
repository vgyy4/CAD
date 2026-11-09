"""Framing of Fusion design streams: MetaStream (class table + object index) and BulkStream (object records)."""
import struct, re
class R:
    def __init__(s,b,i=0): s.b=b; s.i=i
    def need(s,n):
        if s.i+n>len(s.b): raise ValueError('eof')
    def u8(s): s.need(1); v=s.b[s.i]; s.i+=1; return v
    def u32(s): s.need(4); v=struct.unpack_from('<I',s.b,s.i)[0]; s.i+=4; return v
    def u64(s): s.need(8); v=struct.unpack_from('<Q',s.b,s.i)[0]; s.i+=8; return v
    def f64(s): s.need(8); v=struct.unpack_from('<d',s.b,s.i)[0]; s.i+=8; return v
    def str(s):
        n=s.u32()
        if n>1<<20: raise ValueError('str len')
        s.need(n); v=s.b[s.i:s.i+n].decode('latin1'); s.i+=n; return v
    def wstr(s):
        n=s.u32()
        if n>1<<20: raise ValueError('wstr len')
        s.need(2*n); v=s.b[s.i:s.i+2*n].decode('utf-16le','replace'); s.i+=2*n; return v

def parse_meta(m):
    """-> dict(classes=[(guid,parent,ver,module,[ids])], roots=[ids], index={id:off}, index2={id:off}, header_end)"""
    r=R(m)
    # header: str segment name, u32, wstr uuid; then (2023-24 files) the segment name again and
    # 3 u32, or (2025 files) u32 1234, u32 8, u32, 4 bytes, str segment type, str module, 2 u32.
    # Either way the class table follows: u32 count, then entries starting with a 36-char GUID.
    seg=r.str(); r.u32(); r.wstr()
    hdr_start=r.i
    GUID=re.compile(rb'[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}')
    for j in range(hdr_start, min(hdr_start+256, len(m)-48)):
        if m[j+4:j+8]==b'\x24\x00\x00\x00' and GUID.match(m, j+8) and 0<struct.unpack_from('<I',m,j)[0]<100000:
            break
    else:
        raise ValueError('no class table')
    hdr=list(m[hdr_start:j])
    r.i=j
    n=r.u32()
    classes=[]
    for _ in range(n):
        c=r.str(); p=r.str(); v=r.u32(); mod=r.str(); cnt=r.u32()
        if cnt>10**7: raise ValueError('cnt')
        ids=[r.u64() for _ in range(cnt)]
        classes.append((c,p,v,mod,ids))
    n=r.u32(); roots=[r.u64() for _ in range(n)]
    n=r.u32(); index={}
    order=[]
    for _ in range(n):
        i=r.u64(); o=r.u64(); index[i]=o; order.append(i)
    n=r.u32(); index2={}
    for _ in range(n):
        i=r.u64(); o=r.u64(); index2[i]=o
    return dict(seg=seg,hdr=hdr,classes=classes,roots=roots,index=index,order=order,index2=index2,end=r.i,len=len(m))

class Design:
    def __init__(s, meta, bulk):
        s.meta=parse_meta(meta); s.bulk=bulk
        s.cls={}; s.module={}
        s.parent={}
        for c,p,v,mod,ids in s.meta['classes']:
            s.parent[c]=p; s.module[c]=mod
            for i in ids: s.cls[i]=c
        idx=s.meta['index']
        offs=sorted(set(idx.values()))+[len(bulk)]
        nxt={o:offs[k+1] for k,o in enumerate(offs[:-1])}
        s.objs={}
        for i,o in idx.items():
            s.objs[i]=(o,nxt[o])
    def rec(s,i):
        o,e=s.objs[i]; return s.bulk[o:e]
    def header(s,i):
        """record header: str version, u64 id"""
        r=R(s.rec(i)); v=r.str(); oid=r.u64(); return v,oid,r.i
    def ancestors(s,c):
        out=[]
        while c in s.parent and len(out)<50:
            out.append(c); c=s.parent[c]
        return out

def utf16_strings(b, minlen=2):
    out=[]
    for m in re.finditer(rb'(?:[\x20-\x7e]\x00){%d,}'%minlen, b):
        s,e=m.span()
        # check length prefix
        L=struct.unpack_from('<I',b,s-4)[0] if s>=4 else -1
        out.append((s, m.group().decode('utf-16le'), L==(e-s)//2))
    return out

def find_streams(files):
    """{prefix: (meta,bulk)} for every segment in a design's files."""
    out={}
    for n,b in files.items():
        if n.endswith('/BulkStream.dat'):
            p=n[:-len('BulkStream.dat')]
            m=files.get(p+'MetaStream.dat')
            if m is not None: out[p]=(m,b)
    return out

GUID_STR=re.compile(rb'\x24\x00\x00\x00[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}')
def ref_end(d, rec, j):
    """If rec[j:] is a reference -> (id, offset after it). A reference is 01 + u64 id of an existing
    object, optionally followed by the target's class GUID as str (older streams: 'typed' refs)."""
    if j+9>len(rec) or rec[j]!=1: return None
    q=struct.unpack_from('<Q',rec,j+1)[0]
    if q not in d.objs or q==0: return None
    e=j+9
    if GUID_STR.match(rec, e): e+=40          # typed reference: 01 u64 str(class GUID)
    elif e<len(rec) and rec[e]==0: e+=1        # plain reference: 01 u64 00
    else: return None
    return q,e
def refs_at(d, rec, start=0):
    """All (offset, id) of references in rec (see ref_end)."""
    out=[]; j=start
    while j+10<=len(rec):
        r=ref_end(d,rec,j)
        if r: out.append((j,r[0])); j=r[1]; continue
        j+=1
    return out
def ref_lists(d, rec, start=0, minn=1):
    """(offset, stride, [ids]) for every u32 n followed by n references, each padded with the same
    number (0..3) of zero bytes."""
    out=[]; j=start
    while j+4<=len(rec):
        n=struct.unpack_from('<I',rec,j)[0]
        hit=None
        if minn<=n<=200000 and j+4<len(rec) and rec[j+4]==1:
            for pad in (1,2,0,3):
                o=j+4; ids=[]; ok=True; stride=None
                for k in range(n):
                    r=ref_end(d,rec,o)
                    if not r: ok=False; break
                    e=r[1]
                    if any(rec[e:e+pad]) or e+pad>len(rec): ok=False; break
                    if stride is None: stride=e+pad-o
                    ids.append(r[0]); o=e+pad
                if ok: hit=(j,stride,ids,o); break
        if hit:
            out.append(hit[:3]); j=hit[3]; continue
        j+=1
    return out
def sections(rec):
    """Split a record into sections: each starts with u32 3 + 3 ASCII digits + u64 id (same id as the record)."""
    _,oid=None,None
    r=R(rec); tag=r.str(); oid=r.u64()
    key=struct.pack('<Q',oid)
    out=[(0,int(tag))]
    j=r.i
    while True:
        j=rec.find(b'\x03\x00\x00\x00',j)
        if j<0 or j+15>len(rec): break
        t=rec[j+4:j+7]
        if t.isdigit() and rec[j+7:j+15]==key:
            out.append((j,int(t))); j+=15; continue
        j+=1
    return out

ATTR_SIZES={'IntrinsicMetaTypeuint64':8,'IntrinsicMetaTypeint64':8,'IntrinsicMetaTypedouble':8,
            'IntrinsicMetaTypeuint32':4,'IntrinsicMetaTypeint32':4,'IntrinsicMetaTypebool':1}
def _attrs(rec, i):
    r=R(rec,i); n=r.u32()
    if n>64: raise ValueError('attrs')
    attrs={}
    for _ in range(n):
        k=r.str(); t=r.str()
        if not k or not all(32<=ord(c)<127 for c in k): raise ValueError('key')
        if t=='IntrinsicMetaTypeIString': attrs[k]=(t,r.wstr()); continue
        if t.endswith('RefMetaType'): sz=10
        elif t in ATTR_SIZES: sz=ATTR_SIZES[t]
        elif t.startswith('EA46307F'): sz=16   # a colour (4 floats) in the samples
        else: raise ValueError('attr type '+t)
        r.need(sz); attrs[k]=(t,rec[r.i:r.i+sz]); r.i+=sz
    return attrs, r.i
def parse_header(rec):
    """tag, id, flags, attrs {key:(type,raw)}, body offset.
    After the id: u32 0, then u8 0 + u8 flag (2021+ classes) or just u8 flag (older class versions);
    flag 1 = u32 n + n attributes (str key, str type, value)."""
    r=R(rec); tag=int(r.str()); oid=r.u64(); z=r.u32(); o=r.i
    if o+2>len(rec): raise ValueError('eof')
    for flag_at in (o+1, o):
        if rec[flag_at]==1:
            try:
                attrs,body=_attrs(rec, flag_at+1)
                return tag,oid,(z,rec[o],rec[o+1]),attrs,body
            except (ValueError, struct.error):
                pass
    return tag,oid,(z,rec[o],rec[o+1]),{},o+2
def wstrings(rec, start=0, end=None):
    """Length-prefixed UTF-16LE strings (u32 count + chars) in order: (offset, end, text). Empty strings are not found."""
    out=[]; j=start; end=len(rec) if end is None else end
    while j+6<=end:
        n=struct.unpack_from('<I',rec,j)[0]
        if 1<=n<=4096 and j+4+2*n<=end:
            s=rec[j+4:j+4+2*n]
            if all(s[k+1]==0 and 32<=s[k]<127 for k in range(0,2*n,2)):
                out.append((j,j+4+2*n,s.decode('utf-16le'))); j+=4+2*n; continue
        j+=1
    return out
def f64(rec,o):
    return struct.unpack_from('<d',rec,o)[0] if 0<=o<=len(rec)-8 else None
def param(d, pid):
    """A 7A6A3D31 parameter: dict(expr, name, unit, pname, value[internal units])."""
    r=d.rec(pid); ws=wstrings(r)
    if len(ws)<2: return None
    p=dict(id=pid, expr=ws[0][2], strings=[w[2] for w in ws])
    last=ws[-1]
    p['value']=f64(r,last[1])
    p['name']=ws[1][2] if len(ws)>1 else None
    if len(ws)>=4: p['unit']=ws[2][2]; p['pname']=ws[3][2]
    elif len(ws)==3: p['pname']=ws[2][2]
    return p
