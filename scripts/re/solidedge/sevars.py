import sys,os,re,struct
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from seraw import *
def variables(c,d):
    """name table entries: u32 serial, u32 nchars, UTF-16LE name, u32 2, u32 1, u32 target. Returns [(name,serial,target,value)]"""
    out=[]
    for m in re.finditer(rb'(?:[\x21-\xff]\x00){4,}',c[:40000] if False else c):
        nm=m.group(); n=len(nm)//2; p=m.start()
        if p<8: continue
        ln,=struct.unpack_from('<I',c,p-4)
        if ln!=n: continue
        ser,=struct.unpack_from('<I',c,p-8)
        t=nm.decode('utf-16le')
        if '_' not in t or not t[-1].isalpha(): continue
        q=p+len(nm)
        a,b_,tg=struct.unpack_from('<III',c,q)
        if (a,b_)!=(2,1): continue
        val=None
        if ser in d and d[ser][3]-d[ser][2]==466: val=relation(c,d,ser)
        elif ser in d and d[ser][3]-d[ser][2]<=160:
            v=struct.unpack_from('<d',c,d[ser][2]+8)[0]
            if 1e-6<abs(v)<10: val=v
        out.append((t,ser,tg,val))
    return out
if __name__=='__main__':
    for f in sys.argv[1:]:
        s,c,ob,d=load(f); print('==',os.path.basename(f))
        for v in variables(c,d): print('  %-42s ser %5d feat %5d val %s'%(v[0],v[1],v[2],None if v[3] is None else round(v[3]*1000,6)))
