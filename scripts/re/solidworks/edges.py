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
HDR = re.compile(rb'\x01\x00\x00\x00\x00\x00\x00\x00(.)\x00\x00\x00\x00(.)\x00\x00(....)' + re.escape(SENT) + rb'\x00\x00', re.S)

def edge_records(d, a, b):
    """All edge records in [a,b): list of (pos, [(tag, fid, idx, size)])."""
    out = []
    for m in HDR.finditer(d, a, b):
        n = m.group(1)[0]; p = m.end(); reps = []
        if n > 8: continue
        for k in range(n):
            if p + 20 > len(d): break
            t, z, t2, br, fid, st, idx = struct.unpack_from('<HHHHIII', d, p)
            if not t & 0x8000 or z != 0: break
            p += 20
            ext = False
            nxt = struct.unpack_from('<H', d, p)[0]
            if (k < n - 1 and not nxt & 0x8000) or (k == n - 1 and struct.unpack_from('<I', d, p)[0] < 16 and idx in (0, 1)):
                p += 4; ext = True
            reps.append(dict(tag=t & 0x7fff, fid=fid, idx=idx, ext=ext))
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

def midpoint(rec, feats):
    """The first two references are the edge's two faces, the last two the faces that bound
    its end vertices.  feats: fid -> dict(pts, segs, plane, distance(mm, signed), symmetric)."""
    reps = rec[1]
    if len(reps) != 4: return None
    cap_tags = {r['tag'] for r in reps if r['ext']}
    f1, f2 = reps[0], reps[1]
    f = feats.get(f1['fid'])
    if not f or f2['fid'] != f1['fid']: return None
    pts, segs = f['pts'], f['segs']
    def seg(r):
        c = [s for s in segs if s.get('a') == r['idx'] and not s.get('construction')]
        return c[0] if len(c) == 1 else None
    d = f['distance']
    def height(cap):
        if f['symmetric']: return d / 2 if cap == 1 else -d / 2
        return d if cap == 1 else 0.0
    c1, c2 = f1['tag'] in cap_tags, f2['tag'] in cap_tags
    if not c1 and not c2:
        A, B = seg(f1), seg(f2)
        if not A or not B: return None
        common = {A['s'], A['e']} & {B['s'], B['e']}
        if len(common) != 1: return None
        P = pts[common.pop()]
        h = 0.0 if f['symmetric'] else d / 2.0
        return lift(f, P['x'] * 1000, P['y'] * 1000, h)
    if c1 != c2:
        side, cap = (f2, f1) if c1 else (f1, f2)
        A = seg(side)
        if not A: return None
        m = seg_mid(pts, A)
        if m is None: return None
        return lift(f, m[0] * 1000, m[1] * 1000, height(cap['idx']))
    return None

def lift(f, x, y, h):
    pl = f['plane']; o = pl['origin']; xa = pl['x_axis']; ya = pl['y_axis']
    n = [xa[1]*ya[2]-xa[2]*ya[1], xa[2]*ya[0]-xa[0]*ya[2], xa[0]*ya[1]-xa[1]*ya[0]]
    return [round(o[i] + x * xa[i] + y * ya[i] + h * n[i], 6) for i in range(3)]
