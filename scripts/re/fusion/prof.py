"""Profile records (0D57BD2F): the sketch regions a feature uses, as loops of sketch curves named by
their crv_primary/secondary ids. After the owner reference (to the 4BD53E5A record) and one byte:
u32 nregions, nregions x Region;
Region := u32 nloops, nloops x Loop, u32 nchildren, nchildren x Region   (children = nested regions)
Loop   := u32 n, n x entries {u32 kind, u64 crv_primary_id, u64 crv_secondary_id, u32 x, u32 a,
          u32 b, 8 bytes}, u8 flag (1 = outer)
The record's next section (str tag) must follow exactly."""
import struct, re, fdesign
SEC=re.compile(rb'\x03\x00\x00\x00[0-9]{3}')
class Bad(Exception): pass
def _region(rec, o, depth):
    if depth>32: raise Bad
    nl=struct.unpack_from('<I',rec,o)[0]; o+=4
    if not 0<nl<10000: raise Bad
    loops=[]
    for _ in range(nl):
        n=struct.unpack_from('<I',rec,o)[0]; o+=4
        if not 0<n<100000: raise Bad
        cs=[]
        for k in range(n):
            t,p,s,x,a,bn=struct.unpack_from('<IQQIII',rec,o)
            if t>16 or p>1<<40: raise Bad
            cs.append(dict(kind=t,primary=p,secondary=s,x=x,a=a,b=bn,tail=rec[o+32:o+40]))
            o+=40
        flag=rec[o]; o+=1
        loops.append(dict(flag=flag, curves=cs))
    nc=struct.unpack_from('<I',rec,o)[0]; o+=4
    if nc>10000: raise Bad
    kids=[]
    for _ in range(nc):
        k,o=_region(rec,o,depth+1); kids.append(k)
    return dict(loops=loops, children=kids), o
def parse_profiles(d, i):
    rec=d.rec(i)
    try: _,_,_,_,b=fdesign.parse_header(rec)
    except Exception: return None
    for j in range(max(b-4,15), min(b+16, len(rec))):
        r=fdesign.ref_end(d, rec, j)
        if not r: continue
        for skip in (1,0,2):
            try:
                o=r[1]+skip
                n=struct.unpack_from('<I',rec,o)[0]; o+=4
                if not 0<n<100000: continue
                regs=[]
                for _ in range(n):
                    g,o=_region(rec,o,0); regs.append(g)
                if SEC.match(rec,o) or o==len(rec): return regs
            except (struct.error, Bad, IndexError):
                continue
    return None
