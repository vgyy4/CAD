"""Minimal ShapeManager (binary ACIS) topology reader for the history blob: bodies by tag,
their loops/wires as sampled 3D polylines (cm). Clean room; token layout from sab.py."""
import struct, math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sab


def tok(t, v):
    if t in (0x0c, 0x04, 0x15): return struct.unpack('<i', v)[0]
    if t == 0x06: return struct.unpack('<d', v)[0]
    if t in (0x13, 0x14): return struct.unpack('<3d', v)
    if t in (0x07, 0x0d, 0x0e): return v.decode('latin1')
    if t == 0x0a: return 'F'
    if t == 0x0b: return 'T'
    return None


class Acis:
    def __init__(self, blob):
        R = sab.records(blob)
        self.recs = R
        self.types = [sab.rtype(r) for r in R]
        self.refs = [[struct.unpack('<i', v)[0] for t, v in r if t == 0x0c] for r in R]
        self.ints = [[struct.unpack('<i', v)[0] for t, v in r if t == 0x04] for r in R]
        self.dbls = [[struct.unpack('<d', v)[0] for t, v in r if t == 0x06] for r in R]
        self.pos = [[struct.unpack('<3d', v) for t, v in r if t == 0x13] for r in R]
        self.vec = [[struct.unpack('<3d', v) for t, v in r if t == 0x14] for r in R]
        self.bodies = {}
        for k, t in enumerate(self.types):
            if t == 'body' and len(self.ints[k]) >= 1:
                self.bodies[self.ints[k][0]] = k

    def is_(self, k, name):
        return 0 <= k < len(self.types) and self.types[k].split('-')[-1] == name and not self.types[k].endswith('_' + name)

    def point_of_vertex(self, v):
        for p in self.refs[v]:
            if self.is_(p, 'point'): return self.pos[p][0]
        return None

    def curve_eval(self, c, t):
        ty = self.types[c]
        if ty.endswith('straight-curve'):
            p, = self.pos[c][:1]; d = self.vec[c][0]
            return tuple(p[i] + t * d[i] for i in range(3))
        if ty.endswith('ellipse-curve'):
            cen = self.pos[c][0]; n = self.vec[c][0]; maj = self.vec[c][1]; ratio = self.dbls[c][0] if self.dbls[c] else 1.0
            ln = math.sqrt(sum(x * x for x in n)); n = [x / ln for x in n]
            mn = [n[1] * maj[2] - n[2] * maj[1], n[2] * maj[0] - n[0] * maj[2], n[0] * maj[1] - n[1] * maj[0]]
            return tuple(cen[i] + maj[i] * math.cos(t) + ratio * mn[i] * math.sin(t) for i in range(3))
        return None

    def edge_samples(self, e, nseg=48):
        """Polyline of an edge from its start vertex to its end vertex."""
        rf = self.refs[e]; ds = self.dbls[e]
        verts = [x for x in rf if self.is_(x, 'vertex')]
        curve = [x for x in rf if 0 <= x < len(self.types) and self.types[x].endswith('curve')]
        p0 = self.point_of_vertex(verts[0]) if verts else None
        p1 = self.point_of_vertex(verts[1]) if len(verts) > 1 else p0
        if not curve or len(ds) < 2:
            return [p0, p1]
        c = curve[0]; t0, t1 = ds[0], ds[1]
        if self.types[c].endswith('straight-curve'):
            return [p0, p1]
        if not self.types[c].endswith('ellipse-curve'):
            return None
        # sense: the curve may run the other way along the edge
        q = self.curve_eval(c, t0)
        fwd = q is not None and p0 is not None and math.dist(q, p0) < 1e-6 * max(1.0, abs(t1 - t0))
        if not fwd:
            q2 = self.curve_eval(c, -t0)
            if q2 is not None and math.dist(q2, p0) < 1e-6:
                return [self.curve_eval(c, -(t0 + (t1 - t0) * k / nseg)) for k in range(nseg + 1)]
        return [self.curve_eval(c, t0 + (t1 - t0) * k / nseg) for k in range(nseg + 1)]

    def loops_of_body(self, tag):
        """Every loop (face loop or wire) of the body with this tag, as a list of 3D points (cm)."""
        b = self.bodies.get(tag)
        if b is None: return None
        seen = {b}; st = [b]; coedges = []
        while st:
            x = st.pop()
            for p in self.refs[x]:
                if 0 <= p < len(self.types) and p not in seen and not self.is_(p, 'body') and 'attrib' not in self.types[p]:
                    seen.add(p); st.append(p)
        # group coedges by owner loop/wire, follow 'next'
        own = {}
        for k in seen:
            if self.is_(k, 'coedge'):
                r = self.refs[k]
                # coedge: attr, ?, next, prev, partner, edge, (sense), owner
                owner = [x for x in r if self.is_(x, 'loop') or self.is_(x, 'wire')]
                own.setdefault(owner[0] if owner else -1, []).append(k)
        loops = []
        for o, cs in own.items():
            cs = set(cs)
            start = min(cs); cur = start; order = []
            while True:
                order.append(cur)
                r = self.refs[cur]; nxt = r[3] if len(r) > 3 else -1
                if nxt not in cs or nxt == start or len(order) > len(cs): break
                cur = nxt
            segs = []
            for ce in order:
                r = self.refs[ce]
                edge = [x for x in r if self.is_(x, 'edge')]
                if not edge: continue
                sm = self.edge_samples(edge[0])
                if sm is None or any(p is None for p in sm): return None
                segs.append(list(sm))
            pts = []
            for k, sm in enumerate(segs):
                if not pts:
                    pts = sm[:]; continue
                if len(pts) == len(segs[0]) and min(math.dist(pts[0], sm[0]), math.dist(pts[0], sm[-1])) < min(math.dist(pts[-1], sm[0]), math.dist(pts[-1], sm[-1])):
                    pts = pts[::-1]
                if math.dist(pts[-1], sm[0]) > math.dist(pts[-1], sm[-1]): sm = sm[::-1]
                pts.extend(sm[1:])
            loops.append(pts)
        return loops
