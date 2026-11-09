"""Prototype: SolidWorks part (OLE2 .SLDPRT) -> Gusset feature-tree JSON (version 2).

Usage: python3 -I swfeat.py OUTDIR PART.SLDPRT...
Clean-room: every layout used here was inferred from the sample bytes (see LAYOUT.md).
"""
import sys, os, re, json, struct, math, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dump import load
from scan import scan
from nodes2 import classify
from sketch import parse_sketch, find_plane
from feat import params, endspec_candidates, endspec_decl

PLANE_NAMES = {2: 'Front', 3: 'Top', 4: 'Right'}
EXTRUDE = ('moExtrusion_c', 'moICE_c', 'moCut_c')
REVOLVE = ('moRevolution_c', 'moRevCut_c')
END_TYPES = {0: 'blind', 1: 'through_all?', 2: 'through_all', 6: 'mid_plane'}

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
    last_sketch = None; sketch_geo = {}; solid = False
    # EndSpec class index (learned: most frequent EndSpec-shaped class-ref in extrude spans)
    C = collections.Counter()
    for n in ns:
        if n['cls'] in EXTRUDE + REVOLVE:
            a, b = span(n)
            for c in endspec_candidates(rf, a, b): C[c['cls']] += 1
    es_cls = [k for k, _ in C.most_common(2)]
    for n in ns:
        cls = n['cls']; a, b = span(n)
        rep['features'].append('%s:%s' % (cls, n['name']))
        if cls == 'moProfileFeature_c':
            rep['sketches'] += 1
            from sketch import ENT_HDR, ENT_HDR3
            h = min([x for x in (rf.find(ENT_HDR, n['post'], b), rf.find(ENT_HDR3, n['post'], b)) if x >= 0] or [b])
            body = h - 14
            try:
                pts, segs, warn = parse_sketch(rf, body, b)
            except Exception as e:
                notes.append('%s: sketch parse failed: %s' % (n['name'], e)); last_sketch = None; continue
            plane = find_plane(rf, a, b, uc)
            js = sketch_json(n['name'], pts, segs, plane)
            if plane is None: notes.append('%s: plane not found in span (back-referenced plane?)' % n['name'])
            else: rep['sketch_ok'] += 1
            feats.append(js); last_sketch = n['name']; sketch_geo[n['name']] = (pts, segs, plane)
        elif cls in REVOLVE and endspec_decl(rf, a, b, b'moRevEndSpec_c') is None and \
                any(c['cls'] in es_cls for c in endspec_candidates(rf, a, b)) and \
                not any(abs(p['value'] - 2 * math.pi) < 1e-9 for p in params(rf, a, b, nt)):
            notes.append('%s: tag mapped to %s but body looks like an extrude; treated as extrude' % (n['name'], cls))
            n['cls'] = cls = 'moICE_c'
        if cls in EXTRUDE:
            rep['extrudes'] += 1
            es = endspec_decl(rf, a, b)
            if es is None:
                cands = [c for c in endspec_candidates(rf, a, b) if c['cls'] in es_cls]
                es = cands[0] if cands else None
            ps = params(rf, a, b, nt)
            op = 'new' if not solid else ('cut' if (cls == 'moCut_c' or n['name'].lower().startswith('cut')) else 'join')
            js = dict(type='extrude', name=n['name'], sketch=last_sketch, profiles='all', operation=op,
                      target=None if op == 'new' else 'body1')
            typ = es['type'] if es else None
            js['end_condition'] = END_TYPES.get(typ, 'unknown(%s)' % typ)
            if typ in (0, 6) and ps:
                js['distance'] = mm(ps[0]['value']); js['symmetric'] = typ == 6
            elif typ == 2:
                js['distance'] = None; js['through_all'] = True; js['symmetric'] = False
            else:
                js['distance'] = mm(ps[0]['value']) if ps else None; js['symmetric'] = False
            if es and es.get('flag'): js['reverse_flag'] = True
            if js.get('distance') is not None or js.get('through_all'): rep['extrude_ok'] += 1
            else: rep['unsupported'].append('%s (end type %s)' % (n['name'], typ))
            feats.append(js); solid = True
        elif cls in REVOLVE:
            rep['revolves'] += 1
            ps = params(rf, a, b, nt)
            ang = [p for p in ps if abs(p['value']) <= 2 * math.pi + 1e-9]
            js = dict(type='revolve', name=n['name'], sketch=last_sketch, profiles='all',
                      operation='new' if not solid else ('cut' if cls == 'moRevCut_c' or n['name'].lower().startswith('cut') else 'join'),
                      angle=round(math.degrees(ang[0]['value']), 9) if ang else 360.0)
            g = sketch_geo.get(last_sketch)
            if g:
                cl = [k for k, s in enumerate(g[1]) if s['type'] == 'line' and s.get('centerline') and s['id'] != 0xFFFFFFFF]
                cl += [k for k, s in enumerate(g[1]) if s['type'] == 'line' and s.get('construction') and k not in cl]
                if cl: js['axis'] = dict(line='c%d' % cl[0])
                else: js['axis'] = None; notes.append('%s: no construction line for the axis' % n['name'])
            feats.append(js); solid = True
        elif cls in ('Fillet_c', 'Chamfer_c'):
            ps = params(rf, a, b, nt)
            js = dict(type='fillet' if cls == 'Fillet_c' else 'chamfer', name=n['name'], edges=[],
                      radius=mm(ps[0]['value']) if ps else None, edges_unresolved=True)
            feats.append(js); rep['unsupported'].append('%s (edges not decoded)' % n['name'])
        elif cls in ('moCommentsFolder_c', 'moFavoriteFolder_c', 'moHistoryFolder_c', 'moSensorFolder_c',
                     'moDocsFolder_c', 'moDetailCabinet_c', 'moSurfaceBodyFolder_c', 'moSolidBodyFolder_c',
                     'moEqnFolder_c', 'moMaterialFolder_c', 'moRefPlane_c', 'moOriginProfileFeature_c', 'moProfileFeature_c', None):
            continue
        else:
            rep['unsupported'].append('%s:%s' % (cls, n['name']))
    tree = {'format': 'gusset-feature-tree', 'version': 2, 'source': {'app': 'solidworks', 'file': os.path.basename(path)},
            'features': feats}
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
