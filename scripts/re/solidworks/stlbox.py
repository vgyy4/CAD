import struct,sys,os,json
def stl(fn):
    d=open(fn,'rb').read()
    if d[:5]==b'solid' and b'facet' in d[:300]:
        vs=[tuple(map(float,l.split()[1:4])) for l in d.decode().splitlines() if l.strip().startswith('vertex')]
        tris=[vs[i:i+3] for i in range(0,len(vs),3)]
    else:
        n=struct.unpack_from('<I',d,80)[0]; tris=[]
        for i in range(n):
            v=struct.unpack_from('<12f',d,84+50*i); tris.append((v[3:6],v[6:9],v[9:12]))
    lo=[min(t[k][a] for t in tris for k in range(3)) for a in range(3)]
    hi=[max(t[k][a] for t in tris for k in range(3)) for a in range(3)]
    vol=0
    for a,b,c in tris:
        vol+=(a[0]*(b[1]*c[2]-b[2]*c[1])-a[1]*(b[0]*c[2]-b[2]*c[0])+a[2]*(b[0]*c[1]-b[1]*c[0]))/6
    return lo,hi,vol
if __name__ == '__main__':
    out={}
    for fn in sys.argv[1:]:
        try: lo,hi,v=stl(fn); out[os.path.basename(fn)[:-4]]={'min':lo,'max':hi,'stl_volume':v}
        except Exception as e: print(fn,e)
    for fn in sys.argv[1:]:
        log=fn[:-4]+'.log'
        if os.path.exists(log):
            import re
            vs=[float(x) for x in re.findall(r'volume (-?[\d.]+)',open(log).read())]
            k=os.path.basename(fn)[:-4]
            if k in out: out[k]['parasolid_volume']=vs
    json.dump(out,open(os.path.join(os.path.dirname(sys.argv[1]),'truth.json'),'w'),indent=1)
    for k,v in out.items(): print(k,[round(x,3) for x in v['min']],[round(x,3) for x in v['max']],round(v['stl_volume'],2))
