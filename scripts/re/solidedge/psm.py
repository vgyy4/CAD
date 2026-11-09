import sys,os,re,struct
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from dump import streams
def typetable(t):
    """entries: guid(16) idx(u32) u16=1 u16=0xffff u32=1 str16(name) str16(dll); name/dll are u16 len + bytes + NUL"""
    out={}
    p=t.find(b'\x01\x00\xff\xff[\x01-\x20]\x00\x00\x00')
    pos=[m.start() for m in re.finditer(rb'\x01\x00\xff\xff[\x01-\x20]\x00\x00\x00',t)]
    for p in pos:
        idx=struct.unpack_from('<I',t,p-4)[0]; guid=t[p-20:p-4]
        q=p+8; n=struct.unpack_from('<H',t,q)[0]; name=t[q+2:q+2+n-1].decode('latin1'); q+=2+n
        m=struct.unpack_from('<H',t,q)[0]; dll=t[q+2:q+2+m-1].decode('latin1')
        out[idx]=(name,dll,guid.hex())
    return out
if __name__=='__main__':
    s=streams(sys.argv[1]); tt=typetable(s['PSMtypetable'])
    for k in sorted(tt): print(k,tt[k][0],tt[k][1])

def objects(c):
    """segment PSMcluster0 into objects: 'QQ' u16 kind u32 serial; serial must increase (filters false 'QQ' hits in payloads)."""
    cand=[]
    last=0
    for m in re.finditer(rb'QQ',c):
        p=m.start()
        if p+8>len(c): continue
        kind,sid=struct.unpack_from('<HI',c,p+2)
        if sid>last and sid-last<5000:
            cand.append((p,kind,sid)); last=sid
    out=[]
    for i,(p,k,sid) in enumerate(cand):
        e=cand[i+1][0] if i+1<len(cand) else len(c)
        out.append((sid,k,p,e))
    return out
