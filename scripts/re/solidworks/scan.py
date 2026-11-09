import re,struct,sys
DECL=re.compile(rb'\xff\xff([\x00-\x09])\x00([\x02-\x40])\x00([A-Za-z_][A-Za-z0-9_]{1,63})')
def decls(d):
    out=[]
    for m in DECL.finditer(d):
        l=m.group(2)[0]; nm=m.group(3)
        if len(nm)>=l and re.fullmatch(rb'[A-Za-z_][A-Za-z0-9_]*',nm[:l]):
            out.append((m.start(),m.group(1)[0],nm[:l].decode(),m.start()+6+l))
    return out
def scan(d,base,known=None):
    """known: dict idx->name for preexisting classes. returns events"""
    ds=decls(d); cls=dict(known or {}); nxt=base+1; ev=[]
    di=0; i=0
    # positions of decls
    dpos={p:(s,n,e) for p,s,n,e in ds}
    while i<len(d)-1:
        if i in dpos:
            s,n,e=dpos[i]; cls[nxt]=n; ev.append((i,'decl',nxt,n)); nxt+=2; i=e; continue
        t=struct.unpack_from('<H',d,i)[0]
        if t&0x8000 and t!=0xffff and (t&0x7fff) in cls:
            ev.append((i,'new',nxt,cls[t&0x7fff])); nxt+=1; i+=2; continue
        i+=1
    return ev,cls,nxt
if __name__=='__main__':
    d=open(sys.argv[1],'rb').read(); base=int(sys.argv[2],0)
    ev,cls,n=scan(d,base)
    for e in ev[:int(sys.argv[3]) if len(sys.argv)>3 else 80]: print(hex(e[0]),e[1],hex(e[2]),e[3])
    print('final',hex(n))
