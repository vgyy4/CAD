import sys,struct
def ole(d):
    ss=1<<struct.unpack_from('<H',d,30)[0]; mss=1<<struct.unpack_from('<H',d,32)[0]
    nfat,dirstart,_,cutoff,mfstart,nmf,difstart,ndif=struct.unpack_from('<IIIIIIII',d,44)
    sec=lambda i:d[(i+1)*ss:(i+2)*ss]
    difat=list(struct.unpack_from('<109I',d,76))
    n=difstart
    for _ in range(ndif):
        s=sec(n);v=struct.unpack('<%dI'%(ss//4),s);difat+=v[:-1];n=v[-1]
    fat=[]
    for f in difat[:nfat]:
        fat+=struct.unpack('<%dI'%(ss//4),sec(f))
    def chain(s):
        o=b'';seen=set()
        while s<0xFFFFFFF0:
            assert s not in seen;seen.add(s);o+=sec(s);s=fat[s]
        return o
    dirs=chain(dirstart)
    mf=b''
    if mfstart<0xFFFFFFF0:
        mfat=struct.unpack('<%dI'%(len(chain(mfstart))//4),chain(mfstart))
    ents=[]
    for i in range(0,len(dirs),128):
        e=dirs[i:i+128];nl=struct.unpack_from('<H',e,64)[0]
        name=e[:max(nl-2,0)].decode('utf-16le','replace');t=e[66];l,r,c=struct.unpack_from('<III',e,68);st,sz=struct.unpack_from('<IQ',e,116)
        ents.append((name,t,l,r,c,st,sz))
    root=ents[0]
    mini=chain(root[5]) if root[5]<0xFFFFFFF0 else b''
    def read(e):
        name,t,l,r,c,st,sz=e
        if sz<cutoff:
            o=b'';s=st
            while s<0xFFFFFFF0 and len(o)<sz:
                o+=mini[s*mss:(s+1)*mss];s=mfat[s]
            return o[:sz]
        return chain(st)[:sz]
    return ents,read
if __name__=='__main__':
    d=open(sys.argv[1],'rb').read()
    ents,read=ole(d)
    for e in ents:
        if e[1]==2: print(e[0],e[6],read(e)[:24].hex())
