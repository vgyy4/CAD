"""Fillet / chamfer edge references (moEdgeRef_c) -> edge midpoints.

Layout (from Base_loadCell_v1 Fillet1, see LAYOUT.md):
  edge record = <moEdgeRef_c tag> u32 1, u32 0, u8 nReps, u8 0, u8 0, u8 0, u8 kind, u16 0,
                u32 id, f64 1e100-ish sentinel x2, u16 0, then nReps face references.
  face reference = <rep class tag> u16 0 <tag> u16, u32 feature-id, u32 stamp, u32 index [u32 0]
    * side face of an extrusion (moFromSktEnt*SurfIdRep_c): index = the 'a' key of the sketch
      segment it was swept from (the same keys the moSketchChain_c lists in loop order)
    * cap face (moEndFace*SurfIdRep_c): index 0 = start cap (sketch plane), 1 = end cap.
  An edge between two side faces lists [sideA, sideB, cap, cap]; an edge on a cap lists
  [side, cap, side, side] (guess - only the first form was seen resolved).
"""
import re, struct
SENT = b'\x7d\xc3\x94\x25\xad\x49\xb2\x54' * 2
HDR = re.compile(rb'\x01\x00{7}([\x01-\x08])\x00{4,6}([\x01-\x04])\x00\x00(....)' + re.escape(SENT) + rb'\x00\x00', re.S)

def edge_records(d, a, b):
    """All edge records in [a,b): list of (pos, [refs]).  A reference is a 20-byte core
    (tag, 0, tag, u16, fid, stamp, index) followed by 0..2 extra u32 depending on its class
    (cap references: one; '3Int' side references: two (-1, 0))."""
    out = []
    for m in HDR.finditer(d, a, b):
        n = m.group(1)[0]; p = m.end(); reps = []
        for k in range(n):
            if p + 20 > len(d): break
            t, z, t2, br, fid, st, idx = struct.unpack_from('<HHHHIII', d, p)
            if not t & 0x8000 or z != 0: break
            p += 20; extra = 0
            while extra < 2 and p + 4 <= len(d):
                nxt = struct.unpack_from('<HH', d, p)
                if k < n - 1 and nxt[0] & 0x8000 and nxt[1] == 0: break
                w = struct.unpack_from('<I', d, p)[0]
                if k == n - 1 and not (w < 16 or w == 0xFFFFFFFF): break
                if k == n - 1 and extra == 0 and w not in (0, 1, 0xFFFFFFFF): break
                p += 4; extra += 1
                if k == n - 1 and w != 0xFFFFFFFF: break
            reps.append(dict(tag=t & 0x7fff, fid=fid, idx=idx, ext=extra == 1 and k < n - 1, extra=extra))
        if len(reps) == n: out.append((m.start(), reps))
    return out

def selection(recs):
    """The edge list the user picked: the first run of records spaced by one record each."""
    if not recs: return []
    run = [recs[0]]
    for r in recs[1:]:
        if r[0] - run[-1][0] <= 0xA0: run.append(r)
        else: break
    return run

def seg_mid(pts, s):
    import math
    P, Q = pts[s['s']], pts[s['e']]
    if s['type'] == 'line':
        return ((P['x'] + Q['x']) / 2, (P['y'] + Q['y']) / 2)
    C = pts[s['c']]
    r = math.hypot(P['x'] - C['x'], P['y'] - C['y'])
    a0 = math.atan2(P['y'] - C['y'], P['x'] - C['x']); a1 = math.atan2(Q['y'] - C['y'], Q['x'] - C['x'])
    if s['s'] == s['e']: return None                      # full circle: no single midpoint
    if s['dir'] > 0:
        while a1 <= a0: a1 += 2 * math.pi
    else:
        while a1 >= a0: a1 -= 2 * math.pi
    am = (a0 + a1) / 2
    return (C['x'] + r * math.cos(am), C['y'] + r * math.sin(am))

def cap_tags_of(records):
    """Classes of cap references: those followed by one extra u32, or whose index is only ever
    0/1 while bounding a vertical edge (the '3Int' caps carry two extra words like the sides)."""
    caps = set(); idx = {}
    for _, reps in records:
        for r in reps:
            idx.setdefault(r['tag'], set()).add(r['idx'])
            if r['ext']: caps.add(r['tag'])
    for _, reps in records:
        if len(reps) == 4 and reps[2]['tag'] == reps[3]['tag'] and {reps[2]['idx'], reps[3]['idx']} == {0, 1} \
                and reps[0]['tag'] != reps[2]['tag'] and idx[reps[2]['tag']] <= {0, 1}:
            caps.add(reps[2]['tag'])
    return caps

def _sub(a, b): return [a[i] - b[i] for i in range(3)]
def _dot(a, b): return sum(a[i] * b[i] for i in range(3))

def _poly(f, s):
    """3D polyline (mm, at the sketch plane) of a sketch segment of feature f."""
    import math
    pts = f['pts']; P, Q = pts[s['s']], pts[s['e']]
    if s['type'] == 'line':
        uv = [(P['x'], P['y']), (Q['x'], Q['y'])]
    else:
        C = pts[s['c']]; r = math.hypot(P['x'] - C['x'], P['y'] - C['y'])
        a0 = math.atan2(P['y'] - C['y'], P['x'] - C['x']); a1 = math.atan2(Q['y'] - C['y'], Q['x'] - C['x'])
        if s['s'] == s['e']: a1 = a0 + 2 * math.pi * (1 if s['dir'] > 0 else -1)
        elif s['dir'] > 0:
            while a1 <= a0: a1 += 2 * math.pi
        else:
            while a1 >= a0: a1 -= 2 * math.pi
        n = 2000
        uv = [(C['x'] + r * math.cos(a0 + (a1 - a0) * k / n), C['y'] + r * math.sin(a0 + (a1 - a0) * k / n)) for k in range(n + 1)]
    return [lift(f, u * 1000, v * 1000, 0.0) for u, v in uv]

def _to_plane(P, f, along, h=0.0):
    """Move P along `along` onto the plane of f at height h."""
    n = normal(f); O = lift(f, 0.0, 0.0, h)
    den = _dot(along, n)
    if abs(den) < 1e-9: return None
    t = _dot(_sub(O, P), n) / den
    return [P[i] + t * along[i] for i in range(3)]

def _flat(P, n):
    """2D coordinates of P in a frame normal to n (for intersecting parallel sweeps)."""
    import math
    x = [1.0, 0.0, 0.0] if abs(n[0]) < 0.9 else [0.0, 1.0, 0.0]
    d = _dot(x, n); x = [x[i] - d * n[i] for i in range(3)]; L = math.sqrt(_dot(x, x)); x = [v / L for v in x]
    y = [n[1]*x[2]-n[2]*x[1], n[2]*x[0]-n[0]*x[2], n[0]*x[1]-n[1]*x[0]]
    return (_dot(P, x), _dot(P, y))

def _cross_params(A, B, n):
    """Parameters (polyline arc-length fractions on A) where A and B cross, seen along n."""
    import math
    a2 = [_flat(P, n) for P in A]; b2 = [_flat(P, n) for P in B]
    L = [0.0]
    for k in range(1, len(a2)): L.append(L[-1] + math.dist(a2[k - 1], a2[k]))
    out = []
    for k in range(1, len(a2)):
        p, q = a2[k - 1], a2[k]
        for m in range(1, len(b2)):
            r, s = b2[m - 1], b2[m]
            den = (q[0]-p[0])*(s[1]-r[1]) - (q[1]-p[1])*(s[0]-r[0])
            if abs(den) < 1e-15: continue
            t = ((r[0]-p[0])*(s[1]-r[1]) - (r[1]-p[1])*(s[0]-r[0])) / den
            u = ((r[0]-p[0])*(q[1]-p[1]) - (r[1]-p[1])*(q[0]-p[0])) / den
            if -1e-9 <= t <= 1 + 1e-9 and -1e-9 <= u <= 1 + 1e-9:
                out.append((L[k - 1] + t * (L[k] - L[k - 1])) / L[-1] if L[-1] else 0.0)
    return out

def _at(A, frac):
    import math
    L = [0.0]
    for k in range(1, len(A)): L.append(L[-1] + math.dist(A[k - 1], A[k]))
    target = frac * L[-1]
    for k in range(1, len(A)):
        if L[k] >= target:
            t = (target - L[k - 1]) / (L[k] - L[k - 1]) if L[k] > L[k - 1] else 0.0
            return [A[k - 1][i] + t * (A[k][i] - A[k - 1][i]) for i in range(3)]
    return A[-1]

def _snap(P, f, s):
    """Put a point computed on a sampled arc back onto the exact circle."""
    import math
    if s['type'] != 'arc': return P
    C = f['pts'][s['c']]; Q = f['pts'][s['s']]
    r = math.hypot(Q['x'] - C['x'], Q['y'] - C['y']) * 1000
    n = normal(f); C3 = lift(f, C['x'] * 1000, C['y'] * 1000, 0.0)
    v = _sub(P, C3); h = _dot(v, n); w = [v[i] - h * n[i] for i in range(3)]; L = math.sqrt(_dot(w, w))
    return [C3[i] + w[i] * r / L + h * n[i] for i in range(3)] if L else P

def midpoint(rec, feats, cap_tags):
    """The first two references are the edge's two faces, the last two the faces that bound
    its end vertices.  feats: fid -> dict(pts, segs, plane, distance(mm, signed), symmetric,
    through).  Side faces are sketch segments swept along the feature normal; caps are planes."""
    reps = rec[1]
    if len(reps) != 4: return None
    def face(r):
        f = feats.get(r['fid'])
        if f is None: return None
        if r['tag'] in cap_tags: return ('cap', f, r['idx'])
        c = [s for s in f['segs'] if s.get('a') == r['idx'] and not s.get('construction')]
        return ('side', f, c[0]) if len(c) == 1 else None
    F = [face(r) for r in reps]
    if F[0] is None or F[1] is None: return None
    sides = [x for x in F[:2] if x[0] == 'side']; caps = [x for x in F[:2] if x[0] == 'cap']
    if len(sides) == 1 and len(caps) == 1:
        _, f, s = sides[0]; _, g, c = caps[0]
        A = _poly(f, s)
        lo, hi = 0.0, 1.0
        cuts = []
        for b in F[2:]:
            if b and b[0] == 'side' and b[1] is not f:
                cuts += _cross_params(A, _poly(b[1], b[2]), normal(f))
        if len(cuts) >= 2: lo, hi = min(cuts), max(cuts)
        M = _at(A, (lo + hi) / 2)
        M = _to_plane(M, g, normal(f), cap_height(g, c))
        return [round(v, 6) for v in _snap(M, f, s)] if M else None
    if len(sides) == 2:
        (_, f, A), (_, g, B) = sides
        if f is g:
            common = {A['s'], A['e']} & {B['s'], B['e']}
            if len(common) != 1: return None
            P = f['pts'][common.pop()]; P0 = lift(f, P['x'] * 1000, P['y'] * 1000, 0.0)
        else:
            if abs(abs(_dot(normal(f), normal(g))) - 1) > 1e-9: return None
            pa = _poly(f, A); ts = _cross_params(pa, _poly(g, B), normal(f))
            if len(ts) != 1: return None
            P0 = _snap(_at(pa, ts[0]), f, A)
        # height: halfway between the bounding caps, or between this feature's own caps
        bc = [b for b in F[2:] if b and b[0] == 'cap']
        if len(bc) == 2 and bc[0][1] is bc[1][1] and not bc[0][1].get('through'):
            g2 = bc[0][1]; hs = [cap_height(g2, b[2]) for b in bc]
        else:
            g2 = next((x for x in (f, g) if not x.get('through')), None)
            if g2 is None: return None
            hs = [cap_height(g2, 0), cap_height(g2, 1)]
        M = _to_plane(P0, g2, normal(f), (hs[0] + hs[1]) / 2)
        return [round(v, 6) for v in M] if M else None
    return None

def normal(f):
    xa = f['plane']['x_axis']; ya = f['plane']['y_axis']
    return [xa[1]*ya[2]-xa[2]*ya[1], xa[2]*ya[0]-xa[0]*ya[2], xa[0]*ya[1]-xa[1]*ya[0]]

def cap_height(f, cap):
    d = f['distance']
    if f['symmetric']: return d / 2 if cap == 1 else -d / 2
    return d if cap == 1 else 0.0

def lift(f, x, y, h):
    pl = f['plane']; o = pl['origin']; xa = pl['x_axis']; ya = pl['y_axis']
    n = [xa[1]*ya[2]-xa[2]*ya[1], xa[2]*ya[0]-xa[0]*ya[2], xa[0]*ya[1]-xa[1]*ya[0]]
    return [round(o[i] + x * xa[i] + y * ya[i] + h * n[i], 6) for i in range(3)]
