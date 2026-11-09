"""RSe segment reader: meta (record sizes, class GUIDs) + B stream records.
Usage as module. CLI: python3 -I rse.py dumpdir/part SEGNAME [first last]"""
import sys,os,struct,zlib,uuid
def inflate_meta(raw):
    n=struct.unpack_from('<I',raw,0)[0]; i=4+n+22; L=struct.unpack_from('<I',raw,i-4)[0]
    name=raw[i:i+2*L].decode('utf-16le')
    for j in range(i+2*L,len(raw)-2):
        if raw[j]==0x78 and raw[j+1] in (0x01,0x5e,0x9c,0xda):
            try:
                z=zlib.decompressobj(); d=z.decompress(raw[j:])
                if z.eof: return name,d
            except Exception: pass
    return name,None
def segments(D):
    out={}
    for f in sorted(os.listdir(D)):
        if f.startswith('raw_M'):
            name,meta=inflate_meta(open(os.path.join(D,f),'rb').read())
            bp=os.path.join(D,'B'+f[5:]+'.bin')
            out[name]=(meta,open(bp,'rb').read() if os.path.exists(bp) else None,f[5:])
    return out
def records(meta,b):
    n=struct.unpack_from('<I',meta,14)[0]; s=[x&0x7fffffff for x in struct.unpack_from('<%dI'%n,meta,18)]
    p=4; recs=[]
    for i in range(n):
        e=p+s[i]
        if e+9>len(b) or struct.unpack_from('<I',b,e)[0]!=s[i]: raise ValueError('framing broke at record %d'%i)
        recs.append((p,b[p:e],struct.unpack_from('<I',b,e+5)[0])); p=e+9
    if len(b)-p>64: raise ValueError('trailing %d'%(len(b)-p))
    return recs
def hexd(d,off=0):
    o=[]
    for i in range(0,len(d),16):
        c=d[i:i+16]; o.append('%06x  %-48s %s'%(off+i,' '.join('%02x'%x for x in c),''.join(chr(x) if 32<=x<127 else '.' for x in c)))
    return '\n'.join(o)
if __name__=='__main__':
    segs=segments(sys.argv[1]); meta,b,_=segs[sys.argv[2]]
    R=records(meta,b); a=int(sys.argv[3]) if len(sys.argv)>3 else 0; z=int(sys.argv[4]) if len(sys.argv)>4 else len(R)-1
    for i in range(a,z+1):
        p,r,t=R[i]; print('--- rec %d @%x len %d next %x'%(i,p,len(r),t)); print(hexd(r))

def classes(meta):
    """Class table: u32 count, then 28-byte entries (GUID, u16 version, u16 flag, u32 0, u32 n).
    It follows the bytes `00000000 xx06 0000 00000000 04000000` after the record-size array.
    A record's class is (u32 before it in the B stream) - 0x100, an index into this table."""
    n=struct.unpack_from('<I',meta,14)[0]; p0=18+4*n
    for q in range(p0+16,len(meta)-4):
        if meta[q-4:q]!=b'\x04\x00\x00\x00' or meta[q-10:q-4]!=b'\x00'*6: continue
        nc=struct.unpack_from('<I',meta,q)[0]
        if not (0<nc<5000 and q+4+28*nc<=len(meta)): continue
        if not all(struct.unpack_from('<H',meta,q+4+28*j+18)[0]<=3 and struct.unpack_from('<I',meta,q+4+28*j+20)[0]==0 for j in range(nc)): continue
        out=[]
        for j in range(nc):
            g=q+4+28*j; a,b2,c,d=struct.unpack_from('<HHII',meta,g+16)
            out.append((str(uuid.UUID(bytes_le=meta[g:g+16])),a,b2,d))
        return out,q
    raise ValueError('no class table')
def typed(meta,b):
    """records with class guid: list of (index, offset, class index, guid, bytes)."""
    cl,_=classes(meta); R=records(meta,b)
    out=[]; cur=struct.unpack_from('<I',b,0)[0]
    for i,(p,r,t) in enumerate(R):
        ci=cur-0x100; out.append((i,p,ci,cl[ci][0] if 0<=ci<len(cl) else None,r)); cur=t
    return out,cl
