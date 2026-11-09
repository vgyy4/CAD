"""Fillet/chamfer edges from the result body: faces carrying an NMx_blend_tag attribute whose
4th int is the feature's id (u32 at offset 18 of its DC record). The original edge is
rebuilt from the two support faces next to each blend face (planes only so far)."""
import math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from acis import Acis

def sub(a, b): return [a[i] - b[i] for i in range(3)]
def add(a, b): return [a[i] + b[i] for i in range(3)]
def mul(a, s): return [x * s for x in a]
def dot(a, b): return sum(a[i] * b[i] for i in range(3))
def cross(a, b): return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
def unit(a):
    l = math.sqrt(dot(a, a)); return [x / l for x in a] if l > 0 else a


class Body:
    def __init__(self, blob):
        self.a = a = Acis(blob)
        self.faces = [k for k, t in enumerate(a.types) if a.is_(k, 'face')]

    def surf(self, f):
        a = self.a
        for y in a.refs[f]:
            if 0 <= y < len(a.types) and a.types[y].endswith('surface'): return y
        return None

    def kind(self, f):
        s = self.surf(f)
        if s is None: return None, None
        t = self.a.types[s]; a = self.a
        if t.endswith('plane-surface'): return 'plane', (a.pos[s][0], unit(a.vec[s][0]))
        if t.endswith('cone-surface'):
            d = a.dbls[s]
            if len(d) >= 3 and abs(d[1]) < 1e-12:
                return 'cyl', (a.pos[s][0], unit(a.vec[s][0]), math.sqrt(dot(a.vec[s][1], a.vec[s][1])))
            return 'cone', None
        if t.endswith('torus-surface'):
            return 'torus', (a.pos[s][0], unit(a.vec[s][0]), a.dbls[s][0], abs(a.dbls[s][1]))
        return t, None

    def coedges(self, f):
        a = self.a; out = []
        loops = [y for y in a.refs[f] if a.is_(y, 'loop')]
        while loops and len(loops) < 64:
            nx = [y for y in a.refs[loops[-1]][:3] if a.is_(y, 'loop') and y not in loops]
            if not nx: break
            loops.append(nx[0])
        seen = set()
        for l in loops:
            # loop: attr, ?, next loop, first coedge, face
            st = [y for y in a.refs[l] if a.is_(y, 'coedge')]
            while st:
                c = st.pop()
                if c in seen: continue
                seen.add(c); out.append(c)
                for y in a.refs[c][2:4]:
                    if a.is_(y, 'coedge'): st.append(y)
        return out

    def neighbours(self, f):
        a = self.a; out = []
        for c in self.coedges(f):
            p = a.refs[c][4] if len(a.refs[c]) > 4 else -1
            if not a.is_(p, 'coedge'): continue
            l = [y for y in a.refs[p] if a.is_(y, 'loop')]
            if not l: continue
            fs = [y for y in a.refs[l[0]] if a.is_(y, 'face')]
            e = [y for y in a.refs[c] if a.is_(y, 'edge')]
            if fs: out.append((fs[0], e[0] if e else None))
        return out

    def verts(self, f):
        a = self.a; pts = []
        for c in self.coedges(f):
            for e in a.refs[c]:
                if a.is_(e, 'edge'):
                    for v in a.refs[e]:
                        if a.is_(v, 'vertex'):
                            p = a.point_of_vertex(v)
                            if p: pts.append(p)
        return pts

    def tagged(self, fid, name='NMx_blend_tag'):
        a = self.a; out = []
        for k, t in enumerate(a.types):
            if t.startswith(name) and len(a.ints[k]) > 3 and a.ints[k][3] == fid:
                own = [y for y in a.refs[k] if a.is_(y, 'face')]
                if own: out.append(own[0])
        return out


def line_of_planes(p1, n1, p2, n2, near):
    u = cross(n1, n2); l = math.sqrt(dot(u, u))
    if l < 1e-6: return None
    u = mul(u, 1 / l)
    d1 = dot(n1, p1); d2 = dot(n2, p2)
    # point p = near + a n1 + b n2 + c u with n1.p=d1, n2.p=d2, minimise distance to near
    g = dot(n1, n2); det = 1 - g * g
    r1 = d1 - dot(n1, near); r2 = d2 - dot(n2, near)
    a_ = (r1 - g * r2) / det; b_ = (r2 - g * r1) / det
    return add(near, add(mul(n1, a_), mul(n2, b_))), u


def edge_mid(B, f, r=None):
    """Midpoint (cm) of the edge the blend face f replaced, or None with a reason."""
    k, data = B.kind(f)
    nb = [(g, B.kind(g)) for g, e in B.neighbours(f)]
    planes = {}
    for g, (kk, dd) in nb:
        if kk == 'plane': planes[g] = dd
    vs = B.verts(f)
    if not vs: return None, 'no vertices'
    if k == 'cyl':
        C, ax, rad = data
        sup = []
        for g, (p, n) in planes.items():
            if abs(dot(n, ax)) < 1e-6 and abs(abs(dot(sub(C, p), n)) - rad) < 1e-6: sup.append((p, n))
        if len(sup) != 2: return None, 'cylinder blend with %d planar supports' % len(sup)
        L = line_of_planes(sup[0][0], sup[0][1], sup[1][0], sup[1][1], C)
        if L is None: return None, 'parallel supports'
        p, u = L
    elif k == 'plane':
        Pc, nc = data
        cen = mul([sum(v[i] for v in vs) for i in range(3)], 1 / len(vs))
        best = None; ps = list(planes.values())
        for i in range(len(ps)):
            for j in range(i + 1, len(ps)):
                L = line_of_planes(ps[i][0], ps[i][1], ps[j][0], ps[j][1], cen)
                if L is None or abs(dot(L[1], nc)) > 1e-6: continue
                dd = math.dist(L[0], cen)
                if best is None or dd < best[0]: best = (dd, L)
        if best is None: return None, 'no support pair'
        p, u = best[1]
    elif k in ('torus', 'cone'):
        sref = B.surf(f); O = B.a.pos[sref][0]; ax = unit(B.a.vec[sref][0])
        rh = []
        for v in vs:
            q = sub(v, O); h = dot(q, ax); rr = math.sqrt(max(0.0, dot(q, q) - h * h)); rh.append((rr, h))
        hp = None; rc = None
        pls = [dd for g, (kk, dd) in nb if kk == 'plane' and abs(abs(dot(dd[1], ax)) - 1) < 1e-6]
        cyl = [dd for g, (kk, dd) in nb if kk == 'cyl' and abs(abs(dot(dd[1], ax)) - 1) < 1e-6]
        if pls: hp = dot(sub(pls[0][0], O), ax)
        elif k == 'torus':
            on = [h for rr, h in rh if abs(rr - data[2]) < 1e-6]
            if on: hp = on[0]
        if hp is not None:
            others = sorted(set(round(rr, 7) for rr, h in rh if abs(h - hp) > 1e-6))
            if len(others) == 1: rc = others[0]
        if rc is None and cyl:
            rc = cyl[0][2]
            if hp is None:
                hs = sorted(set(round(h, 7) for rr, h in rh if abs(rr - rc) > 1e-6))
                if len(hs) == 1: hp = hs[0]
        if hp is None or rc is None: return None, '%s blend: corner circle not found' % k
        return circ_mid(add(O, mul(ax, hp)), ax, rc, vs), None
    else:
        return None, 'blend surface %s' % k
    ts = [dot(sub(v, p), u) for v in vs]
    lo, hi = min(ts), max(ts)
    # ends: where the edge line meets the end faces (planes across it), when there are some
    ends = []
    for g, (kk, dd) in nb:
        if kk == 'plane' and abs(dot(dd[1], u)) > 1e-3:
            ends.append(dot(sub(dd[0], p), dd[1]) / dot(u, dd[1]))
    span = hi - lo
    near_lo = [t for t in ends if abs(t - lo) < 0.5 * span + 1e-9 and t <= (lo + hi) / 2]
    near_hi = [t for t in ends if abs(t - hi) < 0.5 * span + 1e-9 and t >= (lo + hi) / 2]
    if near_lo: lo = min(near_lo, key=lambda t: abs(t - lo))
    if near_hi: hi = min(near_hi, key=lambda t: abs(t - hi))
    return add(p, mul(u, (lo + hi) / 2)), None


def ref_dir(n):
    """Where Gusset puts the seam of a circle with this normal (found by experiment): the
    midpoint of a full circle is centre - R*ref."""
    n = unit(n)
    X = [1.0, 0.0, 0.0] if abs(n[0]) < 0.9 else [0.0, 1.0, 0.0]
    return unit(sub(X, mul(n, dot(X, n))))


def circ_mid(cen, ax, R, vs):
    ref = ref_dir(ax); oth = cross(unit(ax), ref)
    angs = []
    for v in vs:
        q = sub(v, cen); q = sub(q, mul(ax, dot(q, ax)))
        if math.sqrt(dot(q, q)) > 1e-9: angs.append(math.atan2(dot(q, oth), dot(q, ref)))
    angs = sorted(set(round(a, 9) for a in angs))
    if len(angs) < 2:
        return sub(cen, mul(ref, R))
    # the arc is the complement of the largest gap between vertex angles
    gaps = [(angs[(i + 1) % len(angs)] - angs[i]) % (2 * math.pi) for i in range(len(angs))]
    i = max(range(len(gaps)), key=lambda j: gaps[j])
    a0 = angs[(i + 1) % len(angs)]; span = 2 * math.pi - gaps[i]
    am = a0 + span / 2
    return add(cen, add(mul(ref, R * math.cos(am)), mul(oth, R * math.sin(am))))


def size_of(B, f, mid):
    """Fillet radius or chamfer leg (cm) from the blend face's geometry."""
    k, data = B.kind(f)
    if k == 'cyl': return data[2]
    if k == 'torus': return data[3]
    vs = B.verts(f)
    if k in ('plane', 'cone') and mid is not None and vs:
        return None
    return None
