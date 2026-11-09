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
            if (t.startswith(name) or t.startswith('NMx_edge_blend_tag')) and len(a.ints[k]) > 3 and a.ints[k][3] == fid:
                own = [y for y in a.refs[k] if a.is_(y, 'face')]
                if own: out.append(own[0])
        return out



def _basis(u):
    a = [1.0, 0.0, 0.0] if abs(u[0]) < 0.9 else [0.0, 1.0, 0.0]
    e1 = unit(cross(u, a)); e2 = cross(u, e1)
    return e1, e2


def _inter(s1, s2, o):
    """Intersections of two 2D curves ('line', n, d) n.x = d or ('circ', c, R); nearest to o first."""
    pts = []
    if s1[0] == 'line' and s2[0] == 'line':
        n1, d1 = s1[1], s1[2]; n2, d2 = s2[1], s2[2]
        det = n1[0] * n2[1] - n1[1] * n2[0]
        if abs(det) < 1e-9: return []
        pts.append(((d1 * n2[1] - d2 * n1[1]) / det, (n1[0] * d2 - n2[0] * d1) / det))
    elif s1[0] == 'line' or s2[0] == 'line':
        L, Cc = (s1, s2) if s1[0] == 'line' else (s2, s1)
        n, d = L[1], L[2]; c, R = Cc[1], Cc[2]
        h = n[0] * c[0] + n[1] * c[1] - d     # signed distance of the centre to the line
        if abs(h) > R + 1e-9: return []
        t = math.sqrt(max(0.0, R * R - h * h))
        foot = (c[0] - n[0] * h, c[1] - n[1] * h); tg = (-n[1], n[0])
        pts += [(foot[0] + tg[0] * t, foot[1] + tg[1] * t), (foot[0] - tg[0] * t, foot[1] - tg[1] * t)]
    else:
        c1, r1 = s1[1], s1[2]; c2, r2 = s2[1], s2[2]
        dx, dy = c2[0] - c1[0], c2[1] - c1[1]; dd = math.hypot(dx, dy)
        if dd < 1e-9 or dd > r1 + r2 + 1e-9 or dd < abs(r1 - r2) - 1e-9: return []
        a = (r1 * r1 - r2 * r2 + dd * dd) / (2 * dd); h = math.sqrt(max(0.0, r1 * r1 - a * a))
        mx, my = c1[0] + dx * a / dd, c1[1] + dy * a / dd
        pts += [(mx - dy * h / dd, my + dx * h / dd), (mx + dy * h / dd, my - dx * h / dd)]
    return sorted(pts, key=lambda q: math.hypot(q[0] - o[0], q[1] - o[1]))


def cyl_edge(nb, C, ax, rad):
    """Straight edge (point, direction) a cylindrical blend replaced: the two neighbouring support faces
    (planes parallel to the blend axis, cylinders coaxial-parallel) tangent to the blend circle, intersected
    in the cross-section."""
    u = unit(ax); e1, e2 = _basis(u)
    P2 = lambda q: (dot(sub(q, C), e1), dot(sub(q, C), e2))
    sup = []
    for g, (kk, dd) in nb:
        if kk == 'plane':
            p, n = dd
            if abs(dot(n, u)) > 1e-6: continue
            h = dot(sub(p, C), n)
            if abs(abs(h) - rad) < 1e-6:
                n2 = (dot(n, e1), dot(n, e2)); sup.append(('line', n2, n2[0] * P2(p)[0] + n2[1] * P2(p)[1], g))
        elif kk == 'cyl':
            p, n, R = dd
            if abs(abs(dot(n, u)) - 1) > 1e-6: continue
            c2 = P2(p); dist = math.hypot(*c2)
            if min(abs(dist - (R + rad)), abs(dist - abs(R - rad))) < 1e-6 and (dist > 1e-9):
                sup.append(('circ', c2, R, g))
    seen = set(); uniq = []
    for s_ in sup:
        if s_[3] not in seen: seen.add(s_[3]); uniq.append(s_)
    if len(uniq) != 2:
        return 'cylinder blend with %d usable supports' % len(uniq)
    pts = _inter(uniq[0][:3], uniq[1][:3], (0.0, 0.0))
    if not pts: return 'supports do not meet'
    q = pts[0]
    return add(C, add(mul(e1, q[0]), mul(e2, q[1]))), u



def edge_points(B, e):
    a = B.a
    return [a.point_of_vertex(v) for v in a.refs[e] if a.is_(v, 'vertex')]


def rev_axis(B, g):
    """(point on axis, unit direction) of a surface of revolution face (cyl/cone/torus) or None."""
    s = B.surf(g)
    if s is None: return None
    t = B.a.types[s]
    if t.endswith('cone-surface') or t.endswith('torus-surface'):
        return B.a.pos[s][0], unit(B.a.vec[s][0])
    return None


def profile_curve(B, g, O, ax, kind_data):
    """Profile of face g's surface in the (rho, h) half-plane about the axis (O, ax):
    ('line', (nx, ny), d) with n.(rho,h) = d, or ('circ', (rho_c, h_c), r). None if not coaxial."""
    k, data = kind_data
    def rh(p):
        q = sub(p, O); h = dot(q, ax); return (math.sqrt(max(0.0, dot(q, q) - h * h)), h)
    if k == 'plane':
        p, n = data
        if abs(abs(dot(n, ax)) - 1) > 1e-6: return None
        return ('line', (0.0, 1.0), rh(p)[1])
    s = B.surf(g)
    if k == 'cyl':
        C, u, R = data
        if abs(abs(dot(u, ax)) - 1) > 1e-6: return None
        if math.sqrt(max(0.0, dot(sub(C, O), sub(C, O)) - dot(sub(C, O), ax) ** 2)) > 1e-6: return None
        return ('line', (1.0, 0.0), R)
    if k == 'torus':
        C, u, Rt, rt = data
        if abs(abs(dot(u, ax)) - 1) > 1e-6: return None
        return ('circ', (Rt, rh(C)[1]), rt)
    if k == 'cone':
        pts = []
        for p in B.verts(g):
            c = rh(p)
            if all(math.hypot(c[0] - q[0], c[1] - q[1]) > 1e-6 for q in pts): pts.append(c)
        if len(pts) < 2: return None
        a_, b_ = pts[0], pts[1]
        for q in pts[1:]:
            if math.hypot(q[0] - a_[0], q[1] - a_[1]) > math.hypot(b_[0] - a_[0], b_[1] - a_[1]): b_ = q
        dx, dy = b_[0] - a_[0], b_[1] - a_[1]; L = math.hypot(dx, dy)
        n = (-dy / L, dx / L)
        # all vertices must lie on the line
        if any(abs(n[0] * (q[0] - a_[0]) + n[1] * (q[1] - a_[1])) > 1e-6 for q in pts): return None
        return ('line', n, n[0] * a_[0] + n[1] * a_[1])
    return None


def circle_edge(B, f, fid=None):
    """Circular edge a blend face replaced, from its two long-side neighbours (coaxial surfaces of
    revolution / planes across the axis), whatever the blend face's own surface is. Returns
    (centre, axis, R) or a reason string."""
    k, data = B.kind(f)
    nb = []
    for g, e in B.neighbours(f):
        if g == f or e is None: continue
        nb.append((g, e, B.kind(g)))
    ax = None
    for g, e, kd in [(f, None, (k, data))] + [(g, e, kd) for g, e, kd in nb]:
        r = rev_axis(B, g)
        if r: O, ax = r; break
    if ax is None: return 'no axis of revolution'
    rh = lambda p: (math.sqrt(max(0.0, dot(sub(p, O), sub(p, O)) - dot(sub(p, O), ax) ** 2)), dot(sub(p, O), ax))
    vs = B.verts(f)
    if not vs: return 'no vertices'
    mean = (sum(rh(v)[0] for v in vs) / len(vs), sum(rh(v)[1] for v in vs) / len(vs))
    sup = {}
    for g, e, kd in nb:
        pts = edge_points(B, e)
        cs = [rh(p) for p in pts]
        if not cs: continue
        if any(abs(c[0] - cs[0][0]) > 1e-5 or abs(c[1] - cs[0][1]) > 1e-5 for c in cs): continue   # an end edge, not a long side
        if kd[0] is None: continue
        pc = profile_curve(B, g, O, ax, kd)
        if pc is None: continue
        sup[g] = pc
    if len(sup) != 2: return '%d coaxial long-side neighbours' % len(sup)
    c1, c2 = list(sup.values())
    pts = _inter(c1, c2, mean)
    if not pts: return 'supports do not meet in the half-plane'
    rho, h = pts[0]
    if rho < 1e-9: return 'edge on the axis'
    return add(O, mul(ax, h)), ax, rho


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
    EDGE_INFO[0] = None
    k, data = B.kind(f)
    nb = [(g, B.kind(g)) for g, e in B.neighbours(f)]
    planes = {}
    for g, (kk, dd) in nb:
        if kk == 'plane': planes[g] = dd
    vs = B.verts(f)
    if not vs: return None, 'no vertices'
    if k == 'cyl':
        C, ax, rad = data
        r = cyl_edge(nb, C, ax, rad)
        if isinstance(r, str): return None, r
        p, u = r
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
    elif k in ('torus', 'cone', None) or k.endswith('surface') or k not in ('plane', 'cyl'):
        ce = circle_edge(B, f)
        if not isinstance(ce, str):
            cen, ax_, R_ = ce
            vs_ = B.verts(f)
            EDGE_INFO[0] = ('circle', cen, ax_, R_)
            return circ_mid(cen, ax_, R_, vs_), None
        if k not in ('torus', 'cone'): return None, 'blend surface %s (%s)' % (k, ce)
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
        cen = add(O, mul(ax, hp))
        EDGE_INFO[0] = ('circle', cen, ax, rc)
        return circ_mid(cen, ax, rc, vs), None
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
    EDGE_INFO[0] = ('line', u)
    return add(p, mul(u, (lo + hi) / 2)), None


EDGE_INFO = [None]   # set by edge_mid: tangent info of the last edge found


def local_path(m, info, eps=0.01):
    """Two points eps (cm) either side of m on the edge (line or circle), for the importer's
    `path` (it then picks the model edge that holds m and both points, whatever its extent)."""
    if info is None: return None
    if info[0] == 'line':
        u = info[1]; return [sub(m, mul(u, eps)), add(m, mul(u, eps))]
    _, cen, ax, R = info
    q = sub(m, cen); q = sub(q, mul(ax, dot(q, ax)))
    if math.sqrt(dot(q, q)) < 1e-9: return None
    a = unit(q); b = cross(unit(ax), a); d = eps / R
    pt = lambda t: add(cen, mul(add(mul(a, math.cos(t)), mul(b, math.sin(t))), R))
    return [pt(-d), pt(d)]


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
    angs = sorted(a % (2 * math.pi) for a in angs)
    ded = []
    for a in angs:
        if not ded or a - ded[-1] > 1e-6: ded.append(a)
    if len(ded) > 1 and ded[0] + 2 * math.pi - ded[-1] < 1e-6: ded.pop()   # -pi and +pi are one angle
    angs = ded
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
