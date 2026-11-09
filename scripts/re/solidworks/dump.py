import sys,os,struct,zlib
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from ole import ole
from chunks import streams as newstreams
GUID=bytes.fromhex('231dd571da8148a2a85898b21b89ef99')
def unzlb(b):
    if b[:16]!=GUID: return None
    a,c=struct.unpack_from('<II',b,16)
    for off in (24,):
        for w in (15,-15):
            try: return zlib.decompress(b[off:],w)
            except Exception: pass
    return None
def load(path):
    d=open(path,'rb').read()
    out={}
    if d[:8]==bytes.fromhex('d0cf11e0a1b11ae1'):
        ents,read=ole(d)
        for e in ents:
            if e[1]==2:
                b=read(e); out[e[0]]=b
                z=unzlb(b)
                if z is not None: out[e[0]+'#unz']=z
    else:
        for k,v in newstreams(d).items(): out[k]=v
    return out
if __name__=='__main__':
    dst=sys.argv[1]
    for p in sys.argv[2:]:
        st=load(p); n=os.path.splitext(os.path.basename(p))[0]
        os.makedirs(os.path.join(dst,n),exist_ok=True)
        for k,v in st.items():
            open(os.path.join(dst,n,k.replace('/','_')),'wb').write(v)
        print(n,len(st))
