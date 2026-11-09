"""Prototype: SolidWorks part (OLE2 .SLDPRT) -> Gusset feature-tree JSON (version 2).

Usage: python3 -I swfeat.py OUTDIR PART.SLDPRT...
Clean-room: every layout used here was inferred from the sample bytes (see LAYOUT.md).
"""
import sys, os, re, json, struct, math, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dump import load
from scan import scan
from nodes2 import classify
from sketch import parse_sketch, find_plane, _orthonormal, _frame
from feat import params, endspec_candidates, endspec_decl
import edges as edgemod

PLANE_NAMES = {2: 'Front', 3: 'Top', 4: 'Right'}
EXTRUDE = ('moExtrusion_c', 'moICE_c', 'moCut_c')
REVOLVE = ('moRevolution_c', 'moRevCut_c')
BIG = 500.0
H_DEFAULT_CLEAR = None   # 0.15 gets motorClip from 7.0e-3 to 2.4e-3 only; not applied
END_TYPES = {0: 'blind', 1: 'through_all?', 2: 'through_all?', 6: 'mid_plane', 9: 'through_all_both'}

def mm(v): return round(v * 1000.0, 6)

def sketch_json(name, pts, segs, plane):
    o = plane['origin'] if plane else [0, 0, 0]
    js = dict(type='sketch', name=name,
              plane=dict(origin=[mm(x) for x in o],
                         x_axis=[round(x, 12) + 0.0 for x in (plane['x_axis'] if plane else [1, 0, 0])],
                         y_axis=[round(x, 12) + 0.0 for x in (plane['y_axis'] if plane else [0, 1, 0])]),
              points=[], curves=[])
    if plane is None: js['plane']['unresolved'] = True
    elif plane.get('plane_feature_id') in PLANE_NAMES: js['plane']['ref'] = PLANE_NAMES[plane['plane_feature_id']]
    pid = lambda i: 'p%d' % i
    for i, p in enumerate(pts):
        if p is None: continue
        q = dict(id=pid(i), x=mm(p['x']), y=mm(p['y']))
        if p.get('ext'): q['external'] = True
        js['points'].append(q)
    for k, s in enumerate(segs):
        cid = 'c%d' % k
        if max(s['s'], s['e'], s.get('c', 0)) >= len(pts) or any(pts[k] is None for k in (s['s'], s['e'], s.get('c', s['s']))):
            continue
        if s['type'] == 'line':
            c = dict(id=cid, type='line', start=pid(s['s']), end=pid(s['e']))
        else:
            if s['s'] == s['e']:
                P, C = pts[s['s']], pts[s['c']]
                c = dict(id=cid, type='circle', center=pid(s['c']), radius=mm(math.hypot(P['x'] - C['x'], P['y'] - C['y'])))
            else:
                a, b = (s['s'], s['e']) if s['dir'] > 0 else (s['e'], s['s'])   # make it counter-clockwise
                c = dict(id=cid, type='arc', center=pid(s['c']), start=pid(a), end=pid(b))
        if s.get('construction'): c['construction'] = True
        if s.get('centerline'): c['centerline'] = True
        js['curves'].append(c)
    return js

# ------------------------------------------------------------ profile area (for the volume check)
def loops_area(pts, segs):
    """Signed-area based profile area: sum |area| of closed loops, nested loops subtract (even-odd)."""
    use = [s for s in segs if not s.get('construction')]
    circles = [s for s in use if s['type'] == 'arc' and s['s'] == s['e']]
    other = [s for s in use if not (s['type'] == 'arc' and s['s'] == s['e'])]
    loops = []
    for c in circles:
        P, C = pts[c['s']], pts[c['c']]
        r = math.hypot(P['x'] - C['x'], P['y'] - C['y'])
        poly = [(C['x'] + r * math.cos(2 * math.pi * k / 128), C['y'] + r * math.sin(2 * math.pi * k / 128)) for k in range(128)]
        loops.append(poly)
    # chain the other segments into loops
    adj = collections.defaultdict(list)
    for i, s in enumerate(other):
        adj[s['s']].append(i); adj[s['e']].append(i)
    used = set()
    for i0, s0 in enumerate(other):
        if i0 in used: continue
        poly = []; cur = s0['s']; i = i0; ok = True
        while True:
            used.add(i); s = other[i]
            fwd = s['s'] == cur
            a, b = (s['s'], s['e']) if fwd else (s['e'], s['s'])
            poly += sample(pts, s, fwd)
            cur = b
            nxt = [j for j in adj[cur] if j not in used]
            if cur == s0['s']: break
            if not nxt: ok = False; break
            i = nxt[0]
        if ok and len(poly) >= 3: loops.append(poly)
        else: return None, 'open profile'
    if not loops: return None, 'no closed loop'
    areas = [abs(shoelace(p)) for p in loops]
    # nesting depth by point-in-polygon of a vertex
    total = 0.0
    for i, p in enumerate(loops):
        depth = sum(1 for j, q in enumerate(loops) if j != i and areas[j] > areas[i] and inside(p[0], q))
        total += areas[i] if depth % 2 == 0 else -areas[i]
    return total, None

def sample(pts, s, fwd):
    P = pts[s['s']]; Q = pts[s['e']]
    if s['type'] == 'line':
        return [(P['x'], P['y'])] if fwd else [(Q['x'], Q['y'])]
    C = pts[s['c']]
    r = math.hypot(P['x'] - C['x'], P['y'] - C['y'])
    a0 = math.atan2(P['y'] - C['y'], P['x'] - C['x']); a1 = math.atan2(Q['y'] - C['y'], Q['x'] - C['x'])
    if s['dir'] > 0:
        while a1 <= a0: a1 += 2 * math.pi
    else:
        while a1 >= a0: a1 -= 2 * math.pi
    n = 48
    seq = [(C['x'] + r * math.cos(a0 + (a1 - a0) * k / n), C['y'] + r * math.sin(a0 + (a1 - a0) * k / n)) for k in range(n)]
    if fwd: return seq
    seq = [(C['x'] + r * math.cos(a1 + (a0 - a1) * k / n), C['y'] + r * math.sin(a1 + (a0 - a1) * k / n)) for k in range(n)]
    return seq

def shoelace(p):
    return 0.5 * sum(p[i][0] * p[(i + 1) % len(p)][1] - p[(i + 1) % len(p)][0] * p[i][1] for i in range(len(p)))

def inside(pt, poly):
    x, y = pt; c = False
    for i in range(len(poly)):
        x1, y1 = poly[i]; x2, y2 = poly[(i + 1) % len(poly)]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1: c = not c
    return c

def centroid_x_area(poly_pts):
    pass

def cross(a, b):
    return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]

def finish_hole(h, rep):
    """Hole-wizard parameters (names as the samples store them; values in m / rad)."""
    h.pop('_nsk', None)
    ps = {p['name']: p['value'] for p in h.pop('_params')}
    def get(*names):
        for k in names:
            if k in ps: return ps[k]
        return None
    dia = get('Hole Dia.', 'Thru Hole Dia.', 'Tap Drill Dia.')
    dep = get('Hole Depth', 'Thru Hole Depth', 'Tap Drill Depth')
    cs_d = get("C'Sink Dia.", "Near C'Sink Dia.")
    cs_a = get("C'Sink Angle", "Near C'Sink Angle")
    if dia: h['diameter'] = mm(dia)
    if dep: h['depth'] = mm(dep); h['point_angle'] = 118.0
    if cs_d:
        h['variant'] = 'countersink'; h['csink_diameter'] = mm(cs_d)
        h['csink_angle'] = round(math.degrees(cs_a), 9) if cs_a else 90.0
    else:
        h['variant'] = 'simple'
    if get('Head Clearance'): h['head_clearance'] = mm(get('Head Clearance'))
    elif cs_d and H_DEFAULT_CLEAR: h['head_clearance'] = H_DEFAULT_CLEAR
    if not h['positions'] or not dia or 'face' not in h:
        h['unresolved'] = True; rep['unsupported'].append('%s (hole not resolved)' % h['name'])

def expand_holes(feats):
    """Head clearance (confirmed by volume on motorClip): a cylinder of the countersink diameter,
    head_clearance deep, above the countersink.  Emitted as a 0.15 mm circle pocket per position,
    then one hole per position from the pocket floor (Gusset drops a hole whose positions lie on
    different faces, so each position gets its own hole)."""
    out = []
    for f in feats:
        if f['type'] != 'hole' or f.get('unresolved') or not f.get('head_clearance') or f.get('variant') != 'countersink':
            out.append(f); continue
        c = f.pop('head_clearance'); n = f['face']['normal']; R = f['csink_diameter'] / 2
        x = [1.0, 0.0, 0.0] if abs(n[0]) < 0.9 else [0.0, 1.0, 0.0]
        dd = sum(x[i] * n[i] for i in range(3)); x = [x[i] - dd * n[i] for i in range(3)]
        L = math.sqrt(sum(v * v for v in x)); x = [v / L for v in x]; y = cross(n, x)
        for k, P in enumerate(f['positions']):
            nm = '%s_%d' % (f['name'], k + 1)
            out.append(dict(type='sketch', name=nm + '_clearance', plane=dict(origin=P, x_axis=x, y_axis=y),
                            points=[dict(id='c', x=0.0, y=0.0)], curves=[dict(id='k', type='circle', center='c', radius=R)]))
            out.append(dict(type='extrude', name=nm + '_clearance_cut', sketch=nm + '_clearance', profiles='all',
                            distance=-c, operation='cut', target=f.get('target')))
            Q = [round(P[i] - c * n[i], 6) for i in range(3)]
            g = {k2: v for k2, v in f.items() if k2 not in ('positions', 'face', 'name')}
            g.update(name=nm, face=dict(point=Q, normal=n), positions=[Q])
            out.append(g)
    return out

# ------------------------------------------------------------ per part
def convert(path):
    st = load(path)
    rf = st.get('Config-0-ResolvedFeatures') or st.get('Contents/Config-0-ResolvedFeatures')
    c0 = st.get('Config-0') or st.get('Contents/Config-0')
    if rf is None or c0 is None:
        return None, dict(status='no Config-0-ResolvedFeatures stream')
    ev0, cls0, n0 = scan(c0, 0)
    uc = [k for k, v in cls0.items() if v == 'moUnitComponent_c']
    uc = uc[0] if uc else None
    nt = re.search(rb'\xff\xff\x01\x00(..)(mo\w+?_c)(..)\xff\xfe\xff', rf).group(3)
    ns, mapping = classify(rf, None)
    starts = sorted(n['start'] for n in ns) + [len(rf)]
    span = lambda n: (n['start'], min(s for s in starts if s > n['start']))
    feats, notes = [], []
    rep = dict(features=[], sketches=0, sketch_ok=0, extrudes=0, extrude_ok=0, revolves=0, unsupported=[])
    last_sketch = None; sketch_geo = {}; solid = False; first_solid = None; by_fid = {}; sk_json = {}
    # EndSpec class index (learned: most frequent EndSpec-shaped class-ref in extrude spans)
    C = collections.Counter()
    for n in ns:
        if n['cls'] in EXTRUDE + REVOLVE:
            a, b = span(n)
            for c in endspec_candidates(rf, a, b):
                if c['cls'] < 0x3000 and c['X'] == 1: C[c['cls']] += 1
    es_cls = [k for k, _ in C.most_common(1)]
    cap_tags = edgemod.cap_tags_of(edgemod.edge_records(rf, 0, len(rf)))
    hole = None
    for ni, n in enumerate(ns):
        cls = n['cls']; a, b = span(n)
        rep['features'].append('%s:%s' % (cls, n['name']))
        if hole is not None and (cls != 'moProfileFeature_c' or hole['_nsk'] >= 2):
            finish_hole(hole, rep); hole = None
        if cls == 'moHoleWzd_c':
            j = ni + 1
            while j < len(ns) and ns[j]['cls'] == 'moProfileFeature_c': j += 1
            hb = ns[j]['start'] if j < len(ns) else len(rf)
            hole = dict(type='hole', name=n['name'], positions=[], _params=params(rf, a, hb, nt), _nsk=0)
            if solid: hole['target'] = first_solid
            feats.append(hole); continue
        if cls == 'moProfileFeature_c':
            rep['sketches'] += 1
            from sketch import find_headers
            hh = find_headers(rf, n['post'], b)
            h = hh[0] if hh else b
            body = h - 14
            try:
                pts, segs, warn = parse_sketch(rf, body, b) if hh else ([], [], [])
            except Exception as e:
                notes.append('%s: sketch parse failed: %s' % (n['name'], e)); last_sketch = None
                if hole is not None: hole['_nsk'] += 1
                continue
            plane = find_plane(rf, a, b, uc)
            js = sketch_json(n['name'], pts, segs, plane)
            if hole is not None:
                hole['_nsk'] += 1
                # hole-wizard sketches: the placement sketch holds the hole centres as stand-alone
                # sketch points (point field A == 1); the other one is the hole's section
                cen = [p for p in pts if p and p.get('A') == 1 and p['id'] != 0xFFFFFFFF and not p.get('ext')]
                if cen and not hole['positions'] and plane is not None:
                    pl = js['plane']; nrm = cross(pl['x_axis'], pl['y_axis'])
                    seen = []
                    for q in cen:
                        P = [round(pl['origin'][i] + q['x'] * 1000 * pl['x_axis'][i] + q['y'] * 1000 * pl['y_axis'][i], 6) for i in range(3)]
                        if all(max(abs(P[i] - o[i]) for i in range(3)) > 1e-6 for o in seen): seen.append(P)
                    hole['positions'] = seen
                    hole['face'] = dict(point=seen[0], normal=[round(x, 12) + 0.0 for x in nrm])
                feats.append(js); sketch_geo[n['name']] = (pts, segs, plane); sk_json[n['name']] = js
                continue
            if plane is None: notes.append('%s: plane not found in span (back-referenced plane?)' % n['name'])
            else: rep['sketch_ok'] += 1
            feats.append(js); last_sketch = n['name']; sketch_geo[n['name']] = (pts, segs, plane); sk_json[n['name']] = js
        elif cls in REVOLVE and endspec_decl(rf, a, b, b'moRevEndSpec_c') is None and \
                any(c['cls'] in es_cls for c in endspec_candidates(rf, a, b)) and \
                not any(abs(p['value'] - 2 * math.pi) < 1e-9 for p in params(rf, a, b, nt)):
            notes.append('%s: tag mapped to %s but body looks like an extrude; treated as extrude' % (n['name'], cls))
            n['cls'] = cls = 'moICE_c'
        if cls in EXTRUDE:
            rep['extrudes'] += 1
            dec = endspec_decl(rf, a, b)
            cands = ([dec] if dec else []) + [c for c in endspec_candidates(rf, a, b) if c['cls'] in es_cls]
            es = cands[0] if cands else None
            ps = params(rf, a, b, nt)
            op = 'new' if not solid else ('cut' if (cls == 'moCut_c' or n['name'].lower().startswith('cut')) else 'join')
            js = dict(type='extrude', name=n['name'], sketch=last_sketch, profiles='all', operation=op,
                      target=None if op == 'new' else first_solid)
            typ = es['type'] if es else None
            js['end_condition'] = END_TYPES.get(typ, 'unknown(%s)' % typ)
            if typ in (0, 6) and ps:
                js['distance'] = mm(ps[0]['value']); js['symmetric'] = typ == 6
            elif typ in (1, 2, 9) and es.get('flag') == 1 and op == 'cut':
                js['distance'] = 2 * BIG; js['through_all'] = True; js['symmetric'] = True   # flag 1 = both directions (confirmed: loadCell_v1, idler_single_v2)
            elif typ == 9:
                js['distance'] = 2 * BIG; js['through_all'] = True; js['symmetric'] = True   # confirmed on loadCell_v1
            elif typ == 2 or (typ == 1 and op == 'cut'):
                js['distance'] = BIG; js['through_all'] = True; js['symmetric'] = False
            else:
                js['distance'] = mm(ps[0]['value']) if ps else None; js['symmetric'] = False
            if op == 'cut' and js.get('distance') and not js.get('symmetric') and js['distance'] > 0:
                js['distance'] = -js['distance']   # SolidWorks cuts go into the material: against the sketch normal (confirmed by rebuild)
            js['sw_end'] = [dict(X=c['X'], type=c['type'], flag=c['flag']) for c in cands[:3]]
            if last_sketch in sketch_geo and js.get('distance') is not None:
                by_fid[n['id']] = dict(pts=sketch_geo[last_sketch][0], segs=sketch_geo[last_sketch][1],
                                       plane=sk_json[last_sketch]['plane'], distance=js['distance'], symmetric=js.get('symmetric'),
                                       through=bool(js.get('through_all')))
            if js.get('distance') is not None or js.get('through_all'): rep['extrude_ok'] += 1
            else: rep['unsupported'].append('%s (end type %s)' % (n['name'], typ))
            feats.append(js); solid = True; first_solid = first_solid or n['name']
        elif cls in REVOLVE:
            rep['revolves'] += 1
            ps = params(rf, a, b, nt)
            ang = [p for p in ps if abs(p['value']) <= 2 * math.pi + 1e-9]
            js = dict(type='revolve', name=n['name'], sketch=last_sketch, profiles='all',
                      operation='new' if not solid else ('cut' if cls == 'moRevCut_c' or n['name'].lower().startswith('cut') else 'join'),
                      angle=round(math.degrees(ang[0]['value']), 9) if ang else 360.0)
            if js['operation'] != 'new': js['target'] = first_solid
            g = sketch_geo.get(last_sketch)
            if g:
                cl = [k for k, s in enumerate(g[1]) if s['type'] == 'line' and s.get('centerline') and s['id'] != 0xFFFFFFFF]
                cl += [k for k, s in enumerate(g[1]) if s['type'] == 'line' and s.get('construction') and k not in cl]
                if cl: js['axis'] = dict(line='c%d' % cl[0])
                else: js['axis'] = None; notes.append('%s: no construction line for the axis' % n['name'])
            feats.append(js); solid = True; first_solid = first_solid or n['name']
        elif cls == 'moMirrorPattern_c':
            # The mirror plane is stored as a face reference (same layout as fillet edge
            # references); the bodies to mirror are back-references into Config-0, i.e. the
            # whole solid body.  Seen once (FrameBracket Mirror1: end cap of 'main flange').
            js = dict(type='mirror', name=n['name'], features=[], target=first_solid, merge=True)
            recs = edgemod.edge_records(rf, a, b)
            pl = None
            # the mirror plane's own frame: '<4 f64 display extents> 01 <R 3x3> <t> 1.0' (confirmed:
            # FrameBracket Mirror1 = Right plane, x = 0; the face list is the faces it was picked by)
            for i in range(a, b - 129):
                if rf[i] != 1: continue
                R = list(struct.unpack_from('<9d', rf, i + 1))
                if not _orthonormal(R): continue
                t = struct.unpack_from('<3d', rf, i + 73)
                if struct.unpack_from('<d', rf, i + 97)[0] != 1.0: continue
                o, x, y, z = _frame(R, t)
                pl = dict(origin=[round(v * 1000, 6) + 0.0 for v in o], normal=[round(v, 12) + 0.0 for v in z]); recs = []
                break
            if recs:
                r0 = recs[0][1][0]; f = by_fid.get(r0['fid'])
                if f is not None and r0['tag'] in cap_tags:
                    pl = dict(origin=edgemod.lift(f, 0.0, 0.0, edgemod.cap_height(f, r0['idx'])), normal=edgemod.normal(f))
            if pl: js['plane'] = pl
            else:
                js['unresolved'] = True; rep['unsupported'].append('%s (mirror plane not resolved)' % n['name'])
            feats.append(js)
        elif cls in ('Fillet_c', 'Chamfer_c'):
            ps = params(rf, a, b, nt)
            js = dict(type='fillet' if cls == 'Fillet_c' else 'chamfer', name=n['name'], edges=[],
                      radius=mm(ps[0]['value']) if ps else None)
            if cls == 'Chamfer_c': js['distance'] = js.pop('radius')
            sel = edgemod.selection(edgemod.edge_records(rf, a, b))
            mids = [edgemod.midpoint(r, by_fid, cap_tags) for r in sel]
            js['edges'] = [dict(midpoint=m) for m in mids if m is not None]
            stale = [r for r, m in zip(sel, mids) if m is None and all(x['fid'] in by_fid for x in r[1])]
            if stale: js['stale_edges'] = len(stale)   # references to faces the sketch no longer has (guess: superseded selections)
            if not js['edges'] or any(m is None and r not in stale for r, m in zip(sel, mids)):
                js['edges_unresolved'] = True
                rep['unsupported'].append('%s (%d of %d edges resolved)' % (n['name'], len(js['edges']), len(sel)))
            feats.append(js)
        elif cls in ('moCommentsFolder_c', 'moFavoriteFolder_c', 'moHistoryFolder_c', 'moSensorFolder_c',
                     'moDocsFolder_c', 'moDetailCabinet_c', 'moSurfaceBodyFolder_c', 'moSolidBodyFolder_c',
                     'moEqnFolder_c', 'moMaterialFolder_c', 'moRefPlane_c', 'moOriginProfileFeature_c', 'moProfileFeature_c', None):
            continue
        else:
            rep['unsupported'].append('%s:%s' % (cls, n['name']))
            feats.append(dict(type='unsupported', name=n['name'], original_type=cls))
    if hole is not None: finish_hole(hole, rep)
    tree = {'format': 'gusset-feature-tree', 'version': 2, 'source': {'app': 'solidworks', 'file': os.path.basename(path)},
            'features': expand_holes(feats)}
    rep['notes'] = notes
    return tree, rep

if __name__ == '__main__':
    out = sys.argv[1]; os.makedirs(out, exist_ok=True)
    summary = {}
    for path in sys.argv[2:]:
        name = os.path.splitext(os.path.basename(path))[0]
        try:
            tree, rep = convert(path)
        except Exception as e:
            tree, rep = None, dict(status='error: %r' % e)
        if tree is not None:
            json.dump(tree, open(os.path.join(out, name + '.json'), 'w'), indent=1)
        summary[name] = rep
        print(name, {k: v for k, v in rep.items() if k in ('sketches', 'sketch_ok', 'extrudes', 'extrude_ok', 'revolves', 'status')},
              len(rep.get('unsupported', [])), 'unsupported')
    json.dump(summary, open(os.path.join(out, 'summary.json'), 'w'), indent=1)
