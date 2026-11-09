"""Profiles of a feature -> one interior point per region (sketch coordinates, cm)."""
import math, struct, fdesign, prof

def crv_key(d, q):
    try: a = fdesign.parse_header(d.rec(q))[3]
    except Exception: return None
    if 'crv_primary_id' not in a: return None
    return (int.from_bytes(a['crv_primary_id'][1], 'little'), int.from_bytes(a['crv_secondary_id'][1], 'little'))

def curve_polyline(F, d, s, q):
    """Sketch curve q as a polyline [(x,y)...] from start to end (cm), or None."""
    P = s['points']
    if q in s['lines']:
        l = s['lines'][q]
        a, b = l['origin'], [l['origin'][k] + l['vector'][k] for k in range(3)]
        return [(a[0], a[1]), (b[0], b[1])]
    if q in s['arcs']:
        a = s['arcs'][q]
        n = a['normal']; xd = a['xdir']
        yd = [n[1]*xd[2]-n[2]*xd[1], n[2]*xd[0]-n[0]*xd[2], n[0]*xd[1]-n[1]*xd[0]]
        a0, a1 = a['a0'], a['a1']
        m = max(8, int(abs(a1 - a0) / (2 * math.pi) * 64))
        out = []
        for k in range(m + 1):
            t = a0 + (a1 - a0) * k / m
            out.append((a['center'][0] + a['radius'] * (math.cos(t) * xd[0] + math.sin(t) * yd[0]),
                        a['center'][1] + a['radius'] * (math.cos(t) * xd[1] + math.sin(t) * yd[1])))
        return out
    # other curve classes (splines): chord through the point references, in record order
    pts = [P[r][:2] for o, r in fdesign.refs_at(d, d.rec(q)) if r in P]
    uniq = []
    for p in pts:
        if p not in uniq: uniq.append(p)
    return uniq if len(uniq) >= 2 else None

GAPS = []
def _seg_x(p1, p2, q1, q2):
    """Intersection parameter t on p1-p2 with segment q1-q2 (proper or touching), or None."""
    rx, ry = p2[0]-p1[0], p2[1]-p1[1]; sx, sy = q2[0]-q1[0], q2[1]-q1[1]
    den = rx*sy - ry*sx
    if abs(den) < 1e-15: return None
    qpx, qpy = q1[0]-p1[0], q1[1]-p1[1]
    t = (qpx*sy - qpy*sx)/den; u = (qpx*ry - qpy*rx)/den
    e = 1e-9
    if -e <= t <= 1+e and -e <= u <= 1+e: return t
    return None

def split_params(poly, others):
    """Arc-length fractions (0..1) where poly is crossed or touched by any of the other polylines."""
    L = [0.0]
    for a, b in zip(poly, poly[1:]): L.append(L[-1] + math.dist(a, b))
    tot = L[-1] or 1.0
    out = []
    for o in others:
        for k, (a, b) in enumerate(zip(poly, poly[1:])):
            for c, dd in zip(o, o[1:]):
                t = _seg_x(a, b, c, dd)
                if t is not None: out.append((L[k] + t*(L[k+1]-L[k]))/tot)
    out.sort(); res = []
    for f in out:
        if not res or f - res[-1] > 1e-6: res.append(f)
    return res

def sub_poly(poly, f0, f1):
    L = [0.0]
    for a, b in zip(poly, poly[1:]): L.append(L[-1] + math.dist(a, b))
    tot = L[-1] or 1.0
    def at(f):
        x = f*tot
        for k in range(len(poly)-1):
            if L[k+1] >= x - 1e-15:
                seg = L[k+1]-L[k]; t = 0 if seg == 0 else (x-L[k])/seg
                return (poly[k][0]+t*(poly[k+1][0]-poly[k][0]), poly[k][1]+t*(poly[k+1][1]-poly[k][1]))
        return poly[-1]
    pts = [at(f0)] + [p for p, l in zip(poly, L) if f0*tot < l < f1*tot] + [at(f1)]
    return pts

def piece(poly, closed, others, a, b):
    """Piece a (1-based) of b of a curve split at its crossings with the other curves; None if the
    crossings do not give b pieces."""
    fs = [f for f in split_params(poly, others) if (1e-6 < f < 1-1e-6) or closed]
    if closed:
        fs = sorted(set(round(f % 1.0, 9) for f in fs))
        if len(fs) != b: return None
        if b == 1: return poly
        f0, f1 = fs[a-1], fs[a % b]
        if f1 <= f0: return sub_poly(poly, f0, 1.0) + sub_poly(poly, 0.0, f1)[1:]
        return sub_poly(poly, f0, f1)
    cuts = [0.0] + fs + [1.0]
    if len(cuts) - 1 != b: return None
    return sub_poly(poly, cuts[a-1], cuts[a])

def trim_by_neighbours(pl, prev, nxt, a, b):
    """The piece of pl between its meeting points with the loop neighbours (piece a of b)."""
    fp = split_params(pl, [prev]) if prev else []
    fn = split_params(pl, [nxt]) if nxt else []
    cand = sorted(set(round(f, 9) for f in fp + fn))
    if a == 1:
        inner = [f for f in cand if f > 1e-6]
        return sub_poly(pl, 0.0, inner[0]) if inner else None
    if a == b:
        inner = [f for f in cand if f < 1 - 1e-6]
        return sub_poly(pl, inner[-1], 1.0) if inner else None
    best = None
    for f0 in fp:
        for f1 in fn:
            lo, hi = min(f0, f1), max(f0, f1)
            if hi - lo > 1e-6 and (best is None or hi - lo < best[1] - best[0]): best = (lo, hi)
    return sub_poly(pl, *best) if best else None

def chain(polys):
    """Join polylines end to end (either direction, nearest endpoint first) into one closed ring."""
    if not polys: return None
    polys = [list(p) for p in polys]
    allp = [q for p in polys for q in p]
    scale = max(1e-6, math.dist((min(q[0] for q in allp), min(q[1] for q in allp)),
                               (max(q[0] for q in allp), max(q[1] for q in allp))))
    tol = 1e-6 * scale + 1e-7
    if len(polys) == 1:
        r = polys[0]
        return r if math.dist(r[0], r[-1]) < tol * 10 or len(r) > 2 and math.dist(r[0], r[-1]) < 1e-3 * scale else None
    ring = polys.pop(0)
    flipped = False
    while polys:
        end = ring[-1]
        best = min([(math.dist(end, p[0]), k, False) for k, p in enumerate(polys)] +
                   [(math.dist(end, p[-1]), k, True) for k, p in enumerate(polys)])
        if best[0] > 1e-4 * scale:
            if not flipped:
                ring.reverse(); flipped = True; continue
            GAPS.append(best[0] / scale); return None
        p = polys.pop(best[1])
        ring += (list(reversed(p)) if best[2] else p)[1:]
    if math.dist(ring[0], ring[-1]) > 1e-4 * scale:
        GAPS.append(math.dist(ring[0], ring[-1]) / scale); return None
    return ring[:-1]

def inside(pt, ring):
    x, y = pt; c = False
    for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1]):
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1: c = not c
    return c

def seg_dist(p, a, b):
    ax, ay = a; bx, by = b; px, py = p
    dx, dy = bx - ax, by - ay; L = dx * dx + dy * dy
    t = 0 if L == 0 else max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / L))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)

def interior_point(outer, holes):
    """A point inside outer and outside every hole, as far from all edges as a grid search finds."""
    xs = [p[0] for p in outer]; ys = [p[1] for p in outer]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    best = None
    edges = [(a, b) for r in [outer] + holes for a, b in zip(r, r[1:] + r[:1])]
    for N in (24, 96):
        for i in range(1, N):
            for j in range(1, N):
                p = (x0 + (x1 - x0) * i / N, y0 + (y1 - y0) * j / N)
                if not inside(p, outer) or any(inside(p, h) for h in holes): continue
                dmin = min(seg_dist(p, a, b) for a, b in edges)
                if best is None or dmin > best[0]: best = (dmin, p)
        if best and best[0] > 1e-4 * max(x1 - x0, y1 - y0): break
    return best[1] if best else None

WHY = {}
def sketch_of_profile(F, d, q):
    for w in fdesign.wstrings(d.rec(q)):
        if w[2].isdigit() and int(w[2]) in d.objs and F.cls(d, int(w[2])) == F.C['SKETCH']: return int(w[2])
    return None
def used_curves(F, d, keymap, sid):
    """Sketch curves (record ids) that bound some profile of sketch sid anywhere in the design."""
    cache = d.__dict__.setdefault('_used', None)
    if cache is None:
        cache = {}
        for i in d.objs:
            if not F.cls(d, i).startswith('0D57BD2F'): continue
            own = [q for o, q in fdesign.refs_at(d, d.rec(i))[:2] if F.cls(d, q).startswith('4BD53E5A')]
            sk = sketch_of_profile(F, d, own[0]) if own else None
            if sk is None: continue
            for g in prof.parse_profiles(d, i) or []:
                for l in g['loops']:
                    for c in l['curves']: cache.setdefault(sk, set()).add((c['primary'], c['secondary']))
        d._used = cache
    keys = cache.get(sid, set())
    return {q for k, q in keymap.items() if k in keys}
def feature_regions(F, d, fid, s):
    r = _feature_regions(F, d, fid, s)
    return r if isinstance(r, list) else None
def _fail(why):
    WHY[why] = WHY.get(why, 0) + 1
    return why
def _feature_regions(F, d, fid, s):
    """[(x,y) cm] one interior point per profile region the feature uses, or None if not decodable."""
    rec = d.rec(fid)
    profs = [q for o, q in fdesign.refs_at(d, rec) if F.cls(d, q).startswith('4BD53E5A')]
    if not profs: return _fail('no profile record')
    keymap = {}
    for q in list(s['lines']) + list(s['arcs']) + list(s['other']):
        k = crv_key(d, q)
        if k: keymap[k] = q
    pts = []
    allpolys = None; used = None
    owners = set(profs)
    for i in d.objs:
        if not F.cls(d, i).startswith('0D57BD2F'): continue
        own = [q for o, q in fdesign.refs_at(d, d.rec(i))[:2]]
        if not owners.intersection(own): continue
        regs = prof.parse_profiles(d, i)
        if regs is None: return _fail('0D57BD2F layout')
        def process(g):
            nonlocal allpolys, used
            rings = []; segs = []
            for l in g['loops']:
                full = []
                for c in l['curves']:
                    q = keymap.get((c['primary'], c['secondary']))
                    pl = curve_polyline(F, d, s, q) if q else None
                    if not pl: return _fail('curve %s' % ('unknown' if not q else F.cls(d, q)[:8]))
                    full.append((q, c, pl))
                polys = []
                n = len(full)
                for k, (q, c, pl) in enumerate(full):
                    if c['b'] > 1:   # a piece of a curve cut by other sketch curves: piece a of b
                        if allpolys is None:
                            allpolys = {kk: curve_polyline(F, d, s, kk) for kk in keymap.values()}
                        if used is None: used = used_curves(F, d, keymap, s['id'])
                        closed = math.dist(pl[0], pl[-1]) < 1e-9
                        pc = piece(pl, closed, [v for kk, v in allpolys.items() if kk != q and v and kk in used], c['a'], c['b'])
                        if not pc and not closed:
                            pc = trim_by_neighbours(pl, full[k - 1][2] if n > 1 else None, full[(k + 1) % n][2] if n > 1 else None, c['a'], c['b'])
                        if not pc:
                            pc = piece(pl, closed, [v for kk, v in allpolys.items() if kk != q and v], c['a'], c['b'])
                        if not pc: return _fail('curve piece %d/%d not reproduced' % (c['a'], c['b']))
                        pl = pc
                    polys.append(pl)
                    segs.append(dict(curve=q, entry=c, poly=pl, flag=l['flag'], loopkey=(i, id(g), id(l))))
                ring = chain(polys)
                if not ring: return _fail('loop does not close')
                rings.append((l['flag'], ring))
            return rings, segs
        for g in regs:
            naming = None
            if g['children']:
                if g['loops']:
                    r = process(g)
                    if isinstance(r, str): return r
                    naming = r[1]
                else:
                    naming = []     # boundary not stored: face names of this extrude unknown
                parts = g['children']
            else:
                parts = [g]
            for k_, part in enumerate(parts):
                r = process(part)
                if isinstance(r, str): return r
                rings, segs = r
                outer = [x for f, x in rings if f == 1]
                holes = [x for f, x in rings if f != 1]
                if len(outer) != 1: return _fail('outer loops %d' % len(outer))
                p = interior_point(outer[0], holes)
                if p is None: return _fail('no interior point')
                xs=[q[0] for q in outer[0]]; ys=[q[1] for q in outer[0]]
                ent = dict(point=p, bbox=(min(xs),min(ys),max(xs),max(ys)), segments=segs)
                if naming is not None: ent['naming_segments'] = naming if k_ == 0 else []
                pts.append(ent)
    return pts or _fail('no 0D57BD2F for profile')
