import sys,os,re,struct,bisect
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from psm import streams,typetable,objects
from xt import decode
FN=rb'(JM(?:Protrusion|Cutout|RevolvedProtrusion|RevolvedCutout|Hole|Thread|UserDefinedPattern|Round|Chamfer|LoftedProtrusion|SweptProtrusion|Mirror)[A-Za-z]*Function)\x00'
def load(path):
    s=streams(path); c=s['PSMcluster0']; ob=objects(c); d={o[0]:o for o in ob}
    return s,c,ob,d
def refs(b):
    return [struct.unpack_from('<I',b,m.start()+8)[0] for m in re.finditer(rb'\x10\x00\x00\x00\x00\x00\x00\x80',b)]
def functions(c,ob):
    out=[]
    for sid,k,a,e in ob:
        b=c[a:e]; m=re.search(FN,b)
        if m and b[:2]==b'QQ': out.append((sid,m.group(1).decode(),a,e))
    return out
def relation(c,d,sid):
    _,k,a,e=d[sid]; b=c[a:e]
    if e-a==466: return struct.unpack_from('<d',b,402)[0]
def sketch_bodies(s):
    """SKETCH.P_B -> {profile_attrset_serial: [(kind, params)]} using BODY_ID_ATTRIB (attrset=id+1)"""
    if 'SKETCH.P_B' not in s: return {}
    n,key,mod=decode(s['SKETCH.P_B'])
    def attr(owner,name):
        out=[]
        for i,(t,f) in n.items():
            if t=='ATTRIBUTE' and f['owner'][1]==owner:
                dn=n[f['definition'][1]][1]['identifier'][1]; nm=bytes(n[dn][1]['string']).decode()
                if nm==name: out.append([n[p[1]][1].get('values') for p in f['fields'] if p[1] in n])
        return out
    res={}
    for i,(t,f) in n.items():
        if t!='BODY': continue
        ba=attr(i,'MONIKER/BODY_ID_ATTRIB')
        if not ba: continue
        bid=int(ba[0][0][0])
        # edges via region->shell->edge chain
        ents=[]; reg=f['region'][1]
        sh=n[reg][1]['shell'][1] if reg in n else 0
        while sh:
            e=n[sh][1]['edge'][1]
            while e:
                ef=n[e][1]; cu=n[ef['curve'][1]]
                fin=ef['fin'][1]; v=[]
                # vertices via fins
                seen=set(); fn_=fin
                for _ in range(2):
                    if fn_ and fn_ in n:
                        vx=n[fn_][1]['vertex'][1]
                        if vx and vx in n: v.append(n[n[vx][1]['point'][1]][1]['pvec'] if n[vx][1]['point'][1] in n else None)
                        fn_=n[fn_][1]['other'][1]
                ents.append((cu[0],cu[1],v,attr(e,'SolidEdge/ProfileEntityInfo'),n[ef['curve'][1]][1]['sense']))
                e=ef['next'][1]
            sh=n[sh][1]['next'][1]
        res[bid+1]=ents
    return res
