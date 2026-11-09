"""Inventor part (.ipt) feature tree -> Gusset feature-tree JSON (clean-room prototype).

Usage: python3 -I iptfeat.py OUTDIR DUMPDIR/part...   (DUMPDIR from extract.py)
Writes OUTDIR/<part>.json (gusset-feature-tree v2) and prints a summary.
All lengths in the file are cm; output is mm. See LAYOUT.md for the byte layout."""
import sys, os, struct, json, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dc import DC, ref, features, ordered_features, slots, param, placement, seq, skdir
import collections

CM = 10.0
G = lambda d, i: d.g[i][:8]


def flags(d, i):
    return struct.unpack_from('<I', d.r[i], d.body(i))[0]


def owner(d, i):
    return ref(struct.unpack_from('<I', d.r[i], d.body(i) + 4)[0], d.n)


def first_list(d, i, start=None):
    """(items, end offset) of the first 0x30000002 list at/after start."""
    r = d.r[i]; k = r.find(b'\x02\x00\x00\x30', d.body(i) if start is None else start)
    if k < 0: return [], None
    n = struct.unpack_from('<I', r, k + 4)[0]
    if n == 0: return [], k + 8
    items = [ref(struct.unpack_from('<I', r, k + 16 + 4 * j)[0], d.n) for j in range(n)]
    return items, k + 16 + 4 * n


def frame(M):
    o = [M[3] * CM, M[7] * CM, M[11] * CM]
    x = [M[0], M[4], M[8]]; y = [M[1], M[5], M[9]]
    return o, x, y


def cross(a, b): return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
def dot(a, b): return sum(p*q for p, q in zip(a, b))
def rnd(v, n=9): return float(round(v, n)) + 0.0


def sketch_json(d, s):
    """Sketch record s -> dict (mm) plus helper data."""
    rs = [x for k, x in d.refs(s)]
    P = [x for x in rs if d.role[x] == 'SkPlacement']
    M = placement(d, P[0]) if P else [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
    o, x, y = frame(M)
    pts, curves, ids = [], [], {}
    for i in range(d.n):
        ro = d.role[i]
        if ro not in ('SkPoint', 'SkLine', 'SkCircle') or owner(d, i) != s: continue
        b = d.body(i); f = flags(d, i)
        if ro == 'SkPoint':
            u, v = struct.unpack_from('<2d', d.r[i], b + 8)
            ids[i] = 'p%d' % i
            pts.append({'id': ids[i], 'x': rnd(u * CM), 'y': rnd(v * CM), **({'construction': True} if f & 0x40 else {})})
    for i in range(d.n):
        ro = d.role[i]
        if ro not in ('SkLine', 'SkCircle') or owner(d, i) != s: continue
        f = flags(d, i); con = bool(f & 0x80000)
        items, end = first_list(d, i)
        if ro == 'SkLine':
            if len(items) >= 2 and all(t in ids for t in items[:2]):
                c = {'id': 'l%d' % i, 'type': 'line', 'start': ids[items[0]], 'end': ids[items[1]]}
            else: continue
        else:
            r = d.r[i]
            rad = struct.unpack_from('<d', r, len(r) - 9)[0]
            cen = ref(struct.unpack_from('<I', r, len(r) - 13)[0], d.n)
            if cen not in ids: continue
            if len(items) == 2 and all(t in ids for t in items):
                c = {'id': 'a%d' % i, 'type': 'arc', 'center': ids[cen], 'start': ids[items[0]], 'end': ids[items[1]]}
            else:
                c = {'id': 'c%d' % i, 'type': 'circle', 'center': ids[cen], 'radius': rnd(rad * CM)}
        if con: c['construction'] = True
        curves.append(c)
    return {'type': 'sketch', 'name': d.name(s) or 'Sketch%d' % s,
            'plane': {'origin': [rnd(v) for v in o], 'x_axis': [rnd(v) for v in x], 'y_axis': [rnd(v) for v in y]},
            'points': pts, 'curves': curves}, M


def sk_dir_flag(d, i):
    return d.r[i][-1] if i is not None else None


def profile_of(d, br, lst):
    """f9884c43 'profile' record that lists the loops list lst -> sketch record."""
    for j in br[lst]:
        if G(d, j) == 'f9884c43':
            for k, x in d.refs(j):
                if d.role[x] == 'Sketch': return x
    return None


EXT = {1: 'distance', 2: 'symmetric', 3: 'full', 5: 'through_all'}
OPS = {1: "new", 2: "cut", 3: "join", 4: "intersect"}


def extent(d, i):
    r = d.r[i]; return struct.unpack_from('<H', r, len(r) - 2)[0]


def op_of(d, i):
    r = d.r[i]; return struct.unpack_from('<H', r, len(r) - 2)[0]


def pval(d, s, j):
    if j < len(s) and s[j] is not None and d.role[s[j]] == 'Param': return param(d, s[j])[1]
    return None


def hole(d, fi, s, nm):
    """Hole feature: slot 0 type (u32; bit 0x20000 = counterbore, guessed), 1 diameter, 2 depth,
    3 cbore diameter, 4 cbore depth, 5 csink angle, 6 drill point angle (rad), 7 list of
    centre sketch points, 8 placement (SkPlacement of the centres' sketch), 9 extent
    (1 distance, 5 through all), 16 direction (SkDir, outward face normal), 20 csink diameter."""
    items, _ = first_list(d, s[7])
    M = placement(d, s[8]); o, x, y = frame(M)
    pos = []
    for it in items:
        if it is None or d.role[it] != 'SkPoint': continue
        u, v = struct.unpack_from('<2d', d.r[it], d.body(it) + 8)
        pos.append([rnd(o[k] + (u * x[k] + v * y[k]) * CM) for k in range(3)])
    n = cross(x, y)
    nd = skdir(d, s[16])[2:] if s[16] is not None else n
    typ = struct.unpack_from('<I', d.r[s[0]], d.body(s[0]))[0]
    f = {'type': 'hole', 'name': nm, 'face': {'point': pos[0] if pos else o, 'normal': [rnd(c) for c in nd]},
         'positions': pos, 'diameter': rnd(pval(d, s, 1) * CM)}
    if typ & 0x20000:
        f['variant'] = 'counterbore'; f['cbore_diameter'] = rnd(pval(d, s, 3) * CM); f['cbore_depth'] = rnd(pval(d, s, 4) * CM)
    ex = extent(d, s[9])
    if ex == 5: f['through'] = True
    else:
        f['depth'] = rnd(pval(d, s, 2) * CM)
        pa = pval(d, s, 6)
        if pa: f['point_angle'] = rnd(math.degrees(pa), 6)
    f['_inventor'] = {'type_code': typ, 'extent': ex}
    return f


def hole_fallback(f, target):
    """The same hole as plain cuts (exact geometry, less editable): through holes as a circle
    sketch cut through all; blind ones as one revolve cut per position (drill point included)."""
    n = f['face']['normal']; ln = math.sqrt(dot(n, n)); n = [c / ln for c in n]
    a = [1.0, 0.0, 0.0] if abs(n[0]) < 0.9 else [0.0, 1.0, 0.0]
    x = cross(a, n); lx = math.sqrt(dot(x, x)); x = [c / lx for c in x]
    y = cross(n, x)
    r = f['diameter'] / 2; out = []
    if f.get('through'):
        o = f['positions'][0]
        pts = []; cur = []
        for k, P in enumerate(f['positions']):
            q = [P[i] - o[i] for i in range(3)]
            pts.append({'id': 'h%d' % k, 'x': rnd(dot(q, x)), 'y': rnd(dot(q, y))})
            cur.append({'id': 'hc%d' % k, 'type': 'circle', 'center': 'h%d' % k, 'radius': r})
        sk = f['name'] + '_sketch'
        out.append({'type': 'sketch', 'name': sk, 'plane': {'origin': o, 'x_axis': x, 'y_axis': y}, 'points': pts, 'curves': cur})
        out.append({'type': 'extrude', 'name': f['name'], 'sketch': sk, 'profiles': 'all', 'operation': 'cut', 'target': target,
                    'distance': -1e4, 'symmetric': False, 'from_hole': True})
        return out
    depth = f['depth']; tip = 0.0
    if f.get('point_angle'): tip = r / math.tan(math.radians(f['point_angle']) / 2)
    prof = [(0.0, 0.0)]
    if f.get('variant') == 'counterbore':
        R = f['cbore_diameter'] / 2; cd = f['cbore_depth']
        prof += [(R, 0.0), (R, -cd), (r, -cd)]
    else:
        prof += [(r, 0.0)]
    prof += [(r, -depth), (0.0, -depth - tip)]
    for k, P in enumerate(f['positions']):
        sk = '%s_%d_sketch' % (f['name'], k)
        pts = [{'id': 'q%d' % j, 'x': rnd(u), 'y': rnd(v)} for j, (u, v) in enumerate(prof)]
        cur = [{'id': 'e%d' % j, 'type': 'line', 'start': 'q%d' % j, 'end': 'q%d' % ((j + 1) % len(prof))} for j in range(len(prof))]
        out.append({'type': 'sketch', 'name': sk, 'plane': {'origin': P, 'x_axis': x, 'y_axis': n}, 'points': pts, 'curves': cur})
        out.append({'type': 'revolve', 'name': '%s_%d' % (f['name'], k), 'sketch': sk, 'profiles': 'all', 'operation': 'cut',
                    'target': target, 'axis': {'line': 'e%d' % (len(prof) - 1)}, 'angle': 360.0, 'from_hole': f['name']})
    return out


_bodies = {}
def result_body(D):
    if D not in _bodies:
        from blobs import part_blobs
        from blend import Body
        b = part_blobs(D).get('result')
        _bodies[D] = Body(b[1]) if b else None
    return _bodies[D]


def blend_feature(d, fi, s, nm, typ, D):
    """Fillet/chamfer: edges rebuilt from the result body's blend faces tagged with this
    feature's id; size from the fillet's radius parameter / the chamfer's distance parameter."""
    from blend import edge_mid, local_path, EDGE_INFO
    B = result_body(D)
    if B is None: return {'type': 'unsupported', 'name': nm, 'original_type': typ}
    faces = B.tagged(seq(d, fi))
    mids = []; why = []
    radii = set()
    for f in faces:
        k, data = B.kind(f)
        if k == 'sphere-surface': continue          # corner patch
        if k == 'plane' and typ == 'fillet': continue   # a planar face carrying a fillet's tag is a leftover, not a blend
        m, w = edge_mid(B, f)
        if m is None: why.append(w); continue
        lp = local_path(m, EDGE_INFO[0])
        info = EDGE_INFO[0]; full = None
        if info and info[0] == 'circle':
            from blend import ref_dir, sub as bsub, mul as bmul
            full = [rnd(c * CM, 6) for c in bsub(info[1], bmul(ref_dir(info[2]), info[3]))]
        mids.append(([rnd(c * CM, 6) for c in m], [[rnd(c * CM, 6) for c in q] for q in lp] if lp else None, full))
        if k == 'cyl': radii.add(round(data[2] * CM, 6))
        if k == 'torus': radii.add(round(data[3] * CM, 6))
    if not faces: why.append('no blend faces tagged with this feature in the result body')
    if why:
        return {'type': 'unsupported', 'name': nm, 'original_type': typ, 'reason': '; '.join(sorted(set(why)))}
    # de-duplicate (a split face gives the same edge twice)
    uniq = []
    for m, lp, full in mids:
        if all(math.dist(m, q['midpoint']) > 1e-6 for q in uniq):
            uniq.append({'midpoint': m, **({'path': lp} if lp else {}), **({'_full': full} if full else {})})
    if typ == 'fillet':
        # radius: the parameter of the first edge set (aad64116 record -> Param)
        rad = None
        it, _ = first_list(d, s[0])
        for x in it:
            if x is None: continue
            for k2, y in d.refs(x):
                if d.role[y] == 'Param': rad = param(d, y)[1] * CM; break
            if rad: break
        if rad is None and len(radii) == 1: rad = radii.pop()
        if len(it) > 1 or (radii and any(abs(r - rad) > 1e-6 for r in radii)):
            return {'type': 'unsupported', 'name': nm, 'original_type': 'fillet with several radii'}
        return {'type': 'fillet', 'name': nm, 'edges': uniq, 'radius': rnd(rad)}
    else:
        ctype = struct.unpack_from('<H', d.r[s[4]], d.body(s[4]))[0] if s[4] is not None else None
        dist = pval(d, s, 2)
        f = {'type': 'chamfer', 'name': nm, 'edges': uniq, 'distance': rnd(dist * CM),
             '_inventor': {'chamfer_type': ctype, 'd2': pval(d, s, 3), 'angle': pval(d, s, 10)}}
        return f


PAT_SPACING_IS_TOTAL = False


def pattern_feature(d, fi, nm, feats):
    """Rectangular pattern, one direction. Refs of the record: the patterned features (part or
    pattern features, in order), `a244457b` direction (three triples at the record end: start
    point, vector, end point, cm, then u8 1 and two refs), `bb1dd5df` with the count as its last
    u32, and the spacing Param (cm)."""
    names = {i: n for i, n, t in feats}
    tgt = [names[x] for k, x in d.refs(fi) if x != fi and x in names]
    dirs = [x for k, x in d.refs(fi) if d.g[x].startswith('a244457b')]
    cnt = [x for k, x in d.refs(fi) if d.g[x].startswith('bb1dd5df') and 1 < struct.unpack_from('<I', d.r[x], len(d.r[x]) - 4)[0] < 1000]
    par = [x for k, x in d.refs(fi) if d.role[x] == 'Param']
    if not (tgt and dirs and cnt and par):
        return {'type': 'unsupported', 'name': nm, 'original_type': 'rect_pattern (direction, count or spacing not found)'}
    r = d.r[dirs[0]]; n = len(r)
    A, V, B = [struct.unpack_from('<3d', r, n - 9 - 24 * k) for k in (3, 2, 1)]
    ln = math.sqrt(dot(V, V))
    count = struct.unpack_from('<I', d.r[cnt[0]], len(d.r[cnt[0]]) - 4)[0]
    sp = param(d, par[0])[1] * CM
    if PAT_SPACING_IS_TOTAL: sp = sp / (count - 1)
    flags = [d.r[x][-1] for k, x in d.refs(fi) if d.g[x].startswith('90874d28')]
    sym = bool(flags and flags[-1])      # guess: last flag = midplane (symmetric about the original)
    # guess (L-Bracket, confirmed by volume + hole centres on 2 patterns): the pattern runs against the edge vector
    u = [-c / ln for c in V]
    return {'type': 'pattern_linear', 'name': nm, 'features': tgt, 'direction': [rnd(c) for c in u],
            'spacing': rnd(sp), 'count': count, '_midplane': sym, '_inventor': {'A': A, 'V': V, 'B': B}}



def thicken_feature(d, fi, s, nm, D, out):
    """Thicken of a side face of the base block by a distance = an extrude (join) of that face's
    outline. Distance: the Param under the face reference (slot 0 -> bf8b8868 -> Param). The cap is the
    thicken-tagged planar face of the result lying `distance` outside the first extrusion's bounding box.
    GUESS (unproven if the part has other failures): outline = the base block's side."""
    from blend import Body
    B = result_body(D)
    if B is None or s[0] is None: return {'type': 'unsupported', 'name': nm, 'original_type': 'thicken'}
    dist = None
    for k1, x1 in d.refs(s[0]):
        for k2, x2 in d.refs(x1):
            if d.role[x2] == 'Param': dist = param(d, x2)[1] * CM
    first = next((o for o in out if o['type'] == 'extrude' and o.get('operation') == 'new'), None)
    sk = next((o for o in out if o['type'] == 'sketch' and first and o['name'] == first['sketch']), None)
    if dist is None or not first or not sk:
        return {'type': 'unsupported', 'name': nm, 'original_type': 'thicken (no distance or base)'}
    pl = sk['plane']; o = pl['origin']; X = pl['x_axis']; Y = pl['y_axis']; N = cross(X, Y)
    pts3 = []
    for p in sk['points']:
        P = [o[i] + p['x'] * X[i] + p['y'] * Y[i] for i in range(3)]
        pts3.append(P); pts3.append([P[i] + N[i] * first['distance'] for i in range(3)])
    if first.get('symmetric'): pts3 += [[P[i] - N[i] * first['distance'] for i in range(3)] for P in pts3[::2]]
    lo = [min(p[i] for p in pts3) for i in range(3)]; hi = [max(p[i] for p in pts3) for i in range(3)]
    fid = seq(d, fi); a = B.a; cands = []
    for k, t in enumerate(a.types):
        if t.startswith('NMx_Brep_Feature_tag') and len(a.ints[k]) > 3 and a.ints[k][3] == fid:
            for y in a.refs[k]:
                if a.is_(y, 'face'):
                    kd, data = B.kind(y)
                    if kd == 'plane':
                        P, n = data
                        for ax in range(3):
                            if abs(abs(n[ax]) - 1) < 1e-6:
                                sg = 1 if n[ax] > 0 else -1; c = P[ax] * CM
                                ext = hi[ax] if sg > 0 else lo[ax]
                                if abs(sg * (c - ext) - dist) < 1e-4: cands.append((ax, sg, c))
    cands = sorted(set(cands))
    if len(cands) != 1: return {'type': 'unsupported', 'name': nm, 'original_type': 'thicken (cap face not found: %d)' % len(cands)}
    ax, sg, c = cands[0]
    j, k = [i for i in range(3) if i != ax]
    ej = [1.0 if i == j else 0.0 for i in range(3)]; ek = [1.0 if i == k else 0.0 for i in range(3)]
    if dot(cross(ej, ek), [sg if i == ax else 0 for i in range(3)]) < 0: ek = [-v for v in ek]
    org = [(lo[i] + hi[i]) / 2 for i in range(3)]; org[ax] = c
    hj = (hi[j] - lo[j]) / 2; hk = (hi[k] - lo[k]) / 2
    ps = [(-hj, -hk), (hj, -hk), (hj, hk), (-hj, hk)]
    ssk = {'type': 'sketch', 'name': nm + '_sketch', 'plane': {'origin': [rnd(v) for v in org], 'x_axis': ej, 'y_axis': ek},
           'points': [{'id': 't%d' % i, 'x': rnd(u), 'y': rnd(v)} for i, (u, v) in enumerate(ps)],
           'curves': [{'id': 'tl%d' % i, 'type': 'line', 'start': 't%d' % i, 'end': 't%d' % ((i + 1) % 4)} for i in range(4)]}
    out.append(ssk)
    return {'type': 'extrude', 'name': nm, 'sketch': ssk['name'], 'profiles': 'all', 'operation': 'join', 'target': first['name'],
            'distance': rnd(-dist), 'symmetric': False}


def decode(D):
    d = DC(D)
    from sk import backrefs
    br = backrefs(d)
    feats = ordered_features(d)
    out = []; done_sk = {}; report = []
    from acis import Acis
    from profiles import loop_tags, to2d, picks
    hb = [i for i in range(d.n) if d.role[i] == 'HistoryBlob']
    ac = None
    if hb:
        r = d.r[hb[0]]; k = r.find(b'ASM BinaryFile')
        try: ac = Acis(r[k:])
        except Exception as e: ac = None
    body_made = False
    for fi, nm, typ in feats:
        s = slots(d, fi)
        if d.r[fi][10] & 0x04:   # suppressed (flag byte 10, bit 2)
            out.append({'type': 'unsupported', 'name': nm, 'original_type': '%s (suppressed in the source)' % typ}); continue
        if typ in ('extrude', 'revolve'):
            sk = profile_of(d, br, s[1]) if s[1] is not None else None
            if sk is None:
                out.append({'type': 'unsupported', 'name': nm, 'original_type': typ + ' (no sketch found)'}); continue
            if sk not in done_sk:
                js, M = sketch_json(d, sk); done_sk[sk] = (js, M); out.append(js)
            js, M = done_sk[sk]
            o, x, y = frame(M); nrm = cross(x, y)
            op = OPS.get(op_of(d, s[0]), 'join') if s[0] is not None else 'join'
            if body_made and op == 'new': op = 'join'   # a second body: guessed as a join (multi-body not decoded)
            if not body_made and op == 'join': op = 'new'
            f = {'type': typ, 'name': nm, 'sketch': js['name'], 'profiles': 'all', 'operation': op}
            tags = loop_tags(d, s[1]) if ac else None
            if tags:
                l2 = []
                for fl, tg in tags:
                    ls = ac.loops_of_body(tg)
                    if ls is None: l2 = None; break
                    l2 += [[to2d(p, M)[:2] for p in l] for l in ls]
                pk = picks(js, l2) if l2 else None
                if pk: f['profiles'] = [{'point': p} for p in pk]
            if op != 'new': f['target'] = first_body
            if typ == 'extrude':
                ex = EXT.get(extent(d, s[6]), 'ext%d' % extent(d, s[6]))
                rev = sk_dir_flag(d, s[3]) == 1
                dist = param(d, s[4])[1] * CM
                if ex == 'symmetric':
                    f['distance'] = rnd(dist); f['symmetric'] = True
                elif ex == 'distance':
                    f['distance'] = rnd(-dist if rev else dist); f['symmetric'] = False
                elif ex == 'through_all':
                    f['distance'] = rnd(-1e4 if rev else 1e4); f['symmetric'] = False; f['through_all'] = True
                else:
                    out.append({'type': 'unsupported', 'name': nm, 'original_type': 'extrude extent ' + ex}); continue
            else:
                ax = s[2]
                r = d.r[ax]; k = r.find(b'\x02\x00\x00\x30', d.body(ax))
                p0 = struct.unpack_from('<3d', r, k + 8); v = struct.unpack_from('<3d', r, k + 32)
                ln = math.sqrt(dot(v, v))
                f['axis'] = {'origin': [rnd(c * CM) for c in p0], 'direction': [rnd(c / ln) for c in v]}
                ex = EXT.get(extent(d, s[3]), 'ext%d' % extent(d, s[3]))
                if ex == 'full': f['angle'] = 360.0
                else: f['angle'] = rnd(math.degrees(param(d, s[4])[1]))
            out.append(f)
            if not body_made: body_made = True; first_body = nm
        elif typ in ('fillet', 'chamfer'):
            out.append(blend_feature(d, fi, s, nm, typ, D))
        elif typ == 'thicken':
            out.append(thicken_feature(d, fi, s, nm, D, out))
        elif typ == 'rect_pattern':
            pf = pattern_feature(d, fi, nm, feats)
            if pf.get('_midplane'):
                sh = [-c * pf['spacing'] * (pf['count'] - 1) / 2 for c in pf['direction']]
                tgt = [o for o in out if o.get('name') in pf['features'] and o['type'] == 'hole']
                if len(tgt) == len(pf['features']):
                    for o in tgt:
                        o['positions'] = [[rnd(p[i] + sh[i]) for i in range(3)] for p in o['positions']]
                        o['face']['point'] = [rnd(o['face']['point'][i] + sh[i]) for i in range(3)]
                        if '_fallback' in o: o['_fallback'] = hole_fallback(o, first_body)
                else:
                    pf = {'type': 'unsupported', 'name': nm, 'original_type': 'rect_pattern (midplane of a non-hole feature)'}
            out.append(pf)
        elif typ == 'hole':
            f = hole(d, fi, s, nm)
            if body_made: f['_fallback'] = hole_fallback(f, first_body)
            out.append(f)
        else:
            out.append({'type': 'unsupported', 'name': nm, 'original_type': typ})
    return {'format': 'gusset-feature-tree', 'version': 2, 'source': {'app': 'inventor'}, 'features': out}


if __name__ == '__main__':
    od = sys.argv[1]; os.makedirs(od, exist_ok=True)
    for D in sys.argv[2:]:
        name = os.path.basename(D.rstrip('/'))
        try:
            t = decode(D)
        except Exception as e:
            import traceback; traceback.print_exc(); print(name, 'ERROR', e); continue
        json.dump(t, open(os.path.join(od, name + '.json'), 'w'), indent=1)
        print(name, ' '.join('%s:%s' % (f['type'], f.get('name')) for f in t['features'] if f['type'] != 'sketch'))
