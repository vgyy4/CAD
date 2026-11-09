"""Geometry helpers for the Fusion feature prototype (rewritten in round 4; the round-3 copy was lost)."""
import math, fdesign

def sub(a, b): return [x - y for x, y in zip(a, b)]
def add(a, b): return [x + y for x, y in zip(a, b)]
def mul(a, s): return [x * s for x in a]
def dot(a, b): return sum(x * y for x, y in zip(a, b))
def norm(a): return math.sqrt(dot(a, a))
def cross(a, b): return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]

def world(pl, p, t):
    """sketch point (x, y) at offset t along the sketch normal -> world (cm)."""
    o = pl['origin']
    return [o[k] + p[0]*pl['x'][k] + p[1]*pl['y'][k] + t*pl['z'][k] for k in range(3)]

def _F():
    import sys
    return sys.modules.get('f3dfeat') or sys.modules['__main__']

def ext_geom(d, fid):
    """dict(pl, z, sketch, segs, t0, t1) for an extrude: sketch placement, the profile segments in
    profile-record order (cm), and the cap offsets along the normal (t1 None when not a distance)."""
    F = _F()
    cache = d.__dict__.setdefault('_eg', {})
    if fid in cache: return cache[fid]
    cache[fid] = r = _ext_geom(F, d, fid)
    return r

def _ext_geom(F, d, fid):
    import regions
    e = F.extrude_raw(d, fid)
    if not e or not e['sketch']: return None
    s = F.sketch(d, e['sketch']); pl = s['plane']
    if not pl: return None
    rg = regions.feature_regions(F, d, fid, s)
    if not rg: return None
    segs = []; seen = set()
    for r in rg:
        for sg in (r.get('naming_segments') if r.get('naming_segments') else r['segments']):
            k = id(sg)
            if k in seen: continue
            seen.add(k); segs.append(sg)
    ps = e['params']
    al = ps.get('AlongDistance'); ag = ps.get('AgainstDistance')
    sign = 1.0
    if e['vec']: sign = 1.0 if dot(pl['z'], e['vec']) >= 0 else -1.0
    t0 = None; t1 = None
    if al and al['value'] is not None:
        a = al['value'] * sign
        if e['extent'] == 3: t0, t1 = -a / 2, a / 2
        elif e['extent'] == 2 and ag and ag['value'] is not None: t0, t1 = -ag['value'] * sign, a
        else: t0, t1 = 0.0, a
    return dict(pl=pl, z=pl['z'], sketch=s, segs=segs, t0=t0, t1=t1, op=e['op'], extent=e['extent'])

def _under(d, fid, classes, depth):
    F = _F(); out = []; seen = {fid}; front = [fid]
    for _ in range(depth):
        nxt = []
        for x in front:
            for o, q in fdesign.refs_at(d, d.rec(x)):
                if q in seen: continue
                seen.add(q); c = F.cls(d, q)[:8]
                if c == '7ACC2A03': out.append(q)
                elif c in classes: nxt.append(q)
        front = nxt
    return out

def edge_recipes_of(d, fid, depth=3):
    return _under(d, fid, ('2CA5A1CD', '5662F619', '9716F783', '4B31DA1B', '5A1BF548', '6CCF41D5', 'F2A7590D'), depth)

def input_recipes(d, fid):
    return _under(d, fid, ('2CA5A1CD', '5662F619'), 4)

_SETS = {}
def body_edge_sets(files):
    import sab
    k = id(files)
    if k in _SETS: return _SETS[k]
    sets = []
    for n, b in files.items():
        if n.endswith('.smb') or n.endswith('.smbh'):
            try: sets += sab.body_edges(b)
            except Exception: pass
    _SETS[k] = sets
    return sets

def colinear_exists(A, B, edges, tol):
    """An edge (p0, p1, ...) that contains the segment A-B."""
    u = sub(B, A); L = norm(u)
    if L < 1e-12: return False
    for e in edges:
        p0, p1 = e[0], e[1]
        ok = True
        for P in (A, B):
            w = sub(P, p0); v = sub(p1, p0); lv = norm(v)
            if lv < 1e-12: ok = False; break
            t = dot(w, v) / lv
            perp = norm(sub(w, mul(v, t / lv)))
            if perp > tol or t < -tol or t > lv + tol: ok = False; break
        if ok: return True
    return False
