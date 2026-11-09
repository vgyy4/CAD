import sys,os,re,struct,collections
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from load import rfc
def uctag(rf):
    C=collections.Counter(m.group(1) for m in re.finditer(rb'(.\x80)\x02\x00\x00{10}',rf,re.S))
    return C.most_common(3)
if __name__=='__main__':
    for p in sys.argv[1:]:
        rf,c0=rfc(p); print(p.split('/')[-2][11:25],p.split('/')[-1][:10],[(struct.unpack('<H',k)[0]&0x7fff,v) for k,v in uctag(rf)])
