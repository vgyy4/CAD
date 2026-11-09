import sys,os,math,json
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from seraw import *
def sub(a,b): return [a[i]-b[i] for i in range(3)]
def cross(a,b): return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
def dot(a,b): return sum(x*y for x,y in zip(a,b))
def norm(a):
    l=math.sqrt(dot(a,a)); return [x/l for x in a]
def profiles(s):
    """-> {attrset_serial: dict(ents=[...])}: each entity dict(kind,curve fields,verts=[(x,y,z)...])"""
    n,key,mod=decode(s['SKETCH.P_B'])
    def attr(owner,name):
        for i,(t,f) in n.items():
            if t=='ATTRIBUTE' and f['owner'][1]==owner:
                dn=n[f['definition'][1]][1]['identifier'][1]
                if bytes(n[dn][1]['string']).decode()==name: return [n[p[1]][1].get('values') for p in f['fields'] if p[1] in n]
    out={}
    for i,(t,f) in n.items():
        if t!='BODY': continue
        pm=attr(i,'SolidEdge/ProfileMemberIDs')
        if not pm: continue
        pid=pm[0][0]; ents=[]
        sh=n[f['region'][1]][1]['shell'][1]
        while sh:
            e=n[sh][1]['edge'][1]
            while e:
                ef=n[e][1]; cu=n[ef['curve'][1]][1]; ct=n[ef['curve'][1]][0]
                verts=[]; fin=ef['fin'][1]
                if fin:
                    for fn_ in (fin,n[fin][1]['other'][1]):
                        vx=n[fn_][1]['vertex'][1]
                        verts.append(n[n[vx][1]['point'][1]][1]['pvec'] if vx else None)
                ents.append(dict(kind=ct,cu=cu,verts=verts,sense=ef and None))
                e=ef['next'][1]
            sh=n[sh][1]['next'][1]
        out[pid]=ents
    return out
def plane_of(ents):
    """plane frame (origin,x,y,normal) in metres from the entities."""
    for e in ents:
        if e['kind']=='CIRCLE':
            c=e['cu']; nrm=list(c['normal']); x=list(c['x_axis']); return list(c['centre']),x,cross(nrm,x),nrm
    # lines: gather points
    pts=[v for e in ents for v in e['verts'] if v]
    dirs=[e['cu']['direction'] for e in ents if e['kind']=='LINE']
    nrm=None
    if not dirs: raise ValueError('no lines/circles')
    for i in range(len(dirs)):
        for j in range(i+1,len(dirs)):
            c=cross(dirs[i],dirs[j])
            if dot(c,c)>1e-6: nrm=norm(c); break
        if nrm: break
    if nrm is None:
        # collinear lines: no plane
        raise ValueError('collinear sketch')
    x=norm(list(dirs[0])); y=cross(nrm,x)
    return list(ents[0]['cu']['pvec']),x,y,nrm
def r12(v): return round(v,9)+0.0
def sketch_json(name,ents,plane=None):
    o,x,y,nrm=plane or plane_of(ents)
    pts=[];curves=[]
    def pid(p):
        u=dot(sub(p,o),x)*1000; v=dot(sub(p,o),y)*1000
        for q in pts:
            if abs(q['x']-u)<1e-6 and abs(q['y']-v)<1e-6: return q['id']
        pts.append(dict(id='p%d'%len(pts),x=r12(u),y=r12(v))); return pts[-1]['id']
    for e in ents:
        k='c%d'%len(curves)
        if e['kind']=='LINE':
            a,b=e['verts'][0],e['verts'][1]
            if a and b: curves.append(dict(id=k,type='line',start=pid(a),end=pid(b)))
        elif e['kind']=='CIRCLE':
            c=e['cu']; cen=pid(c['centre'])
            if not e['verts'] or not e['verts'][0]:
                curves.append(dict(id=k,type='circle',center=cen,radius=r12(c['radius']*1000)))
            else:
                a,b=e['verts'][0],e['verts'][1]
                # ccw about the sketch normal?
                ccw = dot(cross(c['normal'],[1,0,0]),nrm) # placeholder, orientation resolved below
                curves.append(dict(id=k,type='arc',center=cen,start=pid(a),end=pid(b)))
    return dict(type='sketch',name=name,plane=dict(origin=[r12(t*1000) for t in o],x_axis=[r12(t) for t in x],y_axis=[r12(t) for t in y]),points=pts,curves=curves)
if __name__=='__main__':
    s,c,ob,d=load(sys.argv[1]); pr=profiles(s)
    for k,e in pr.items():
        j=sketch_json('p%d'%k,e); print(k,json.dumps(j)[:600])
