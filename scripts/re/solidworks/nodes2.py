import re,struct,sys,os
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
M=bytes.fromhex('00ffff000001ffffffffffffffff000080bf')
M3=bytes.fromhex('00ffff000003ffffffffffffffff000080bf')
def feature_nodes(d):
    m=re.search(rb'\xff\xff\x01\x00(..)(mo\w+?_c)(..)\xff\xfe\xff',d)
    nt=m.group(3)
    out=[]
    allp=[m.start() for m in re.finditer(re.escape(nt)+rb'\xff\xfe\xff',d)]+[len(d)]
    for i,p in enumerate(allp[:-1]):
        q=p+5
        L=d[q]; 
        if L==0xff: L=struct.unpack_from('<H',d,q+1)[0]; q+=2
        name=d[q+1:q+1+2*L].decode('utf-16le','replace'); e=q+1+2*L
        k=d.find(M,e)
        k3=d.find(M3,e)
        if k<0 or (0<=k3<k): k=k3
        if k<0 or k-e>1500: continue
        if struct.unpack_from('<I',d,e+8)[0]==0xffffffff: continue   # parameters carry id -1   # not a feature node (parameter etc.)
        flags,fid=struct.unpack_from('<II',d,e+4)
        post=k+46+8+2
        ver=struct.unpack_from('<H',d,post-2)[0]
        cls=None; tag=None; start=None
        j=d.rfind(b'\xff\xff\x01\x00',max(0,p-80),p)
        if j>=0 and j+6+d[j+4]==p: cls=d[j+6:p].decode(); start=j
        else:
            t=struct.unpack_from('<H',d,p-2)[0]
            if t&0x8000: tag=t&0x7fff; start=p-2
        out.append(dict(start=start,cls=cls,tag=tag,name=name,flags=flags,id=fid,post=post,ver=ver,head=d[e+12:k]))
    return out
if __name__=='__main__':
    import glob,collections
    for fn in sys.argv[1:]:
        d=open(fn,'rb').read()
        for n in feature_nodes(d):
            print('%-22s %-30s %-6s %-26s %s'%(fn.split('/')[-2][:22],n['cls'] or '',hex(n['tag']) if n['tag'] else '',n['name'][:26],d[n['post']:n['post']+20].hex()))

def sig(d,n):
    b=bytearray(d[n['post']:n['post']+16])
    return bytes(b)
def similarity(x,y):
    return sum(1 for a,b in zip(x,y) if a==b)
def classify(d,c0=None):
    """Map class-ref tags of feature nodes to declared class names."""
    ns=feature_nodes(d)
    est={}
    if c0 is not None:
        from scan import scan
        ev0,cls0,n0=scan(c0,0)
        ev,cls,n=scan(d,n0,cls0)
        est={e[0]:e[2] for e in ev if e[1]=='decl'}
    decl=[n for n in ns if n['cls']]
    tags={}
    for n in ns:
        if n['tag'] is not None: tags.setdefault(n['tag'],[]).append(n)
    mapping={}
    for t in sorted(tags):
        first=min(x['start'] for x in tags[t])
        best=None
        for c in decl:
            if c['start']>=first: continue
            strip=lambda s: ''.join(ch for ch in s if not ch.isdigit()).strip()
            sc=max(similarity(sig(d,c),sig(d,x)) + (8 if strip(c['name'])==strip(x['name']) else 0)
                   + (4 if len(c['head'])==len(x['head']) else 0) for x in tags[t])
            # monotone: class indices grow with declaration position
            if c['start'] in est: sc-=abs(est[c['start']]-t)/16.0
            key=(sc,c['start'])
            if best is None or key>best[0]: best=(key,c['cls'])
        mapping[t]=best[1] if best else None
    for n in ns:
        if n['tag'] is not None: n['cls']=mapping.get(n['tag'])
    return ns,mapping
