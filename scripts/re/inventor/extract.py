"""Inflate every RSeStorage B stream of the given .ipt files into OUT/<part>/<stream>.bin, and M streams raw.
Usage: python3 -I extract.py OUT file.ipt..."""
import sys,os,zlib
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from ole import ole
out=sys.argv[1]
for fn in sys.argv[2:]:
    d=open(fn,'rb').read(); ents,read=ole(d)
    part=os.path.basename(fn).rsplit('.',1)[0]
    od=os.path.join(out,part); os.makedirs(od,exist_ok=True)
    line=[]
    for e in ents:
        if e[1]!=2: continue
        raw=read(e); nm=e[0]
        if nm.startswith('B') and len(raw)>18:
            try: z=zlib.decompressobj(); data=z.decompress(raw[18:]); 
            except Exception as ex: data=None
            if data is not None:
                open(os.path.join(od,nm+'.bin'),'wb').write(data); line.append('%s:%d'%(nm,len(data))); continue
        safe=''.join(c if c.isalnum() else '_' for c in nm)
        open(os.path.join(od,'raw_'+safe),'wb').write(raw)
    print(part,' '.join(line))
