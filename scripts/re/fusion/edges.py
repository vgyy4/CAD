"""Fillet/chamfer edges from edge recipes: faces named (generator, creating operation) -> geometry.
Only extrude faces are modelled: caps (planes) and sides swept from sketch lines (planes) or arcs
(cylinders)."""
import math, fdesign, naming, extnaming
from edgeprobe import sub, add, mul, dot, norm, cross, world, ext_geom, edge_recipes_of

def unit(v):
    n = norm(v); return mul(v, 1 / n) if n > 1e-15 else None

class Naming:
    def __init__(self, F, d, files=None):
        self.F = F; self.d = d
        self.om = naming.op_map(F, d)
        self.geo = {}; self.gmap = {}
        self.truth_faces = None; self.files = files; self.how = {}
    def extrude(self, fid):
        if fid not in self.geo:
            G = ext_geom(self.d, fid)
            self.geo[fid] = G
            self.gmap[fid] = extnaming.gen_map(G['segs']) if G else None
        return self.geo[fid], self.gmap[fid]
    def truth(self, fid, G, g):
        if not self.files: return None
        if self.truth_faces is None:
            import sab
            self.truth_faces = []
            for n, b in self.files.items():
                if n.endswith('.smb') and 'Active' in n:
                    try: self.truth_faces += sab.tagged_faces(b)
                    except Exception: pass
        import genmap
        return genmap.truth(G, self.truth_faces, [g]).get(g)
    def face(self, entry):
        """Geometry of a face named (generator, [ops])."""
        g, ops = entry
        if not ops: return None
        fid = self.om.get(ops[0])
        if fid is None or self.F.cls(self.d, fid) != self.F.C['EXTRUDE']: return None
        G, M = self.extrude(fid)
        if not G: return None
        pl = G['pl']; z = pl['z']
        if g == '1' or g == '2':
            t = G['t0'] if g == '1' else G['t1']
            if t is None: return None
            return dict(kind='plane', n=z, p=world(pl, (0, 0), t), feature=fid, what='cap' + g)
        # the ACIS face carrying this generator tag, when it survives in the stored bodies, names
        # the curve directly; otherwise the numbering rule (confirmed for outer loops only)
        T = self.truth(fid, G, g)
        if T is not None:
            sg = next(x for x in G['segs'] if x['curve'] == T); self.how['acis tag'] = self.how.get('acis tag', 0) + 1
        else:
            if any(x['flag'] != 1 for x in G['segs']) or not M or g not in M: return None
            sg, fw = M[g]; self.how['rule'] = self.how.get('rule', 0) + 1
        q = sg['curve']; s = G['sketch']
        if q in s['lines']:
            a, b = sg['poly'][0], sg['poly'][-1]
            pa = world(pl, a, 0.0); pb = world(pl, b, 0.0)
            n = unit(cross(z, sub(pb, pa)))
            return dict(kind='plane', n=n, p=pa, feature=fid, what='side', seg=sg, t0=G['t0'], t1=G['t1'], pl=pl)
        if q in s['arcs']:
            A = s['arcs'][q]
            return dict(kind='cyl', c=world(pl, A['center'][:2], 0), axis=z, r=A['radius'], feature=fid, what='side', seg=sg, t0=G['t0'], t1=G['t1'], pl=pl)
        return None

def plane_plane(f0, f1):
    u = cross(f0['n'], f1['n'])
    if norm(u) < 1e-9: return None
    u = unit(u)
    # point on both planes: solve with the third direction u
    n0, n1 = f0['n'], f1['n']; d0, d1 = dot(n0, f0['p']), dot(n1, f1['p'])
    c = cross(n1, u); e = cross(u, n0)
    den = dot(n0, c)
    if abs(den) < 1e-12: return None
    P = mul(add(mul(c, d0), mul(e, d1)), 1 / den)
    return P, u

def edge_midpoint(N, faces):
    """(midpoint, kind) of the edge between faces[0] and faces[1], bounded by faces[2:], or None."""
    fs = [N.face(f[0]) if f else None for f in faces]
    if len(fs) < 2 or not fs[0] or not fs[1]: return None
    a, b = fs[0], fs[1]
    if a['kind'] == 'plane' and b['kind'] == 'plane':
        L = plane_plane(a, b)
        if not L: return None
        P, u = L
        ts = []
        for f in fs[2:]:
            if f and f['kind'] == 'plane' and abs(dot(f['n'], u)) > 1e-9:
                ts.append(dot(f['n'], sub(f['p'], P)) / dot(f['n'], u))
        if len(ts) >= 2 and max(ts) - min(ts) > 1e-7:
            t = (min(ts) + max(ts)) / 2
            return add(P, mul(u, t)), 'line', (add(P, mul(u, min(ts))), add(P, mul(u, max(ts))))
        return None
    # cap x arc side: the arc piece at the cap
    for cap, cyl in ((a, b), (b, a)):
        if cap['kind'] == 'plane' and cyl['kind'] == 'cyl' and cap.get('what', '').startswith('cap') and abs(abs(dot(cap['n'], cyl['axis'])) - 1) < 1e-9:
            sg = cyl['seg']; poly = sg['poly']
            # mid point of the piece along its polyline
            Ls = [0]
            for p, q in zip(poly, poly[1:]): Ls.append(Ls[-1] + math.dist(p, q))
            half = Ls[-1] / 2
            for k in range(len(poly) - 1):
                if Ls[k + 1] >= half:
                    t = (half - Ls[k]) / max(Ls[k + 1] - Ls[k], 1e-15)
                    m2 = (poly[k][0] + t * (poly[k + 1][0] - poly[k][0]), poly[k][1] + t * (poly[k + 1][1] - poly[k][1]))
                    break
            # project to the true circle
            A = N.extrude(cyl['feature'])[0]['sketch']['arcs'][sg['curve']]
            c = A['center']; r = A['radius']; v = (m2[0] - c[0], m2[1] - c[1]); l = math.hypot(*v)
            if l > 0: m2 = (c[0] + v[0] / l * r, c[1] + v[1] / l * r)
            tcap = dot(cap['n'], sub(cap['p'], cyl['pl']['origin'])) / dot(cap['n'], cyl['pl']['z'])
            return world(cyl['pl'], m2, tcap), 'arc', None
    return None
