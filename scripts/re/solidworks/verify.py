"""Volume / bounding-box check of the prototype trees against the Parasolid body (truth/truth.json)."""
import sys, os, json, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from swfeat import loops_area

def pts_of(sk):
    return {p['id']: (p['x'], p['y']) for p in sk['points']}

def tree_to_internal(sk):
    """back to index form for loops_area"""
    ids = [p['id'] for p in sk['points']]
    idx = {pid: i for i, pid in enumerate(ids)}
    pts = [dict(x=p['x'], y=p['y']) for p in sk['points']]
    segs = []
    for c in sk['curves']:
        if c.get('construction'): continue
        if c['type'] == 'line': segs.append(dict(type='line', s=idx[c['start']], e=idx[c['end']]))
        elif c['type'] == 'arc': segs.append(dict(type='arc', s=idx[c['start']], e=idx[c['end']], c=idx[c['center']], dir=1))
        else:
            C = pts[idx[c['center']]]
            pts.append(dict(x=C['x'] + c['radius'], y=C['y'])); k = len(pts) - 1
            segs.append(dict(type='arc', s=k, e=k, c=idx[c['center']], dir=1))
    return pts, segs

def to3d(pl, x, y):
    return [pl['origin'][i] + x * pl['x_axis'][i] + y * pl['y_axis'][i] for i in range(3)]

def check(tree, truth):
    sk = {f['name']: f for f in tree['features'] if f['type'] == 'sketch'}
    solids = [f for f in tree['features'] if f['type'] in ('extrude', 'revolve')]
    vol = 0.0; pts3 = []; why = None
    for f in solids:
        s = sk.get(f['sketch'])
        if s is None: return None, None, 'missing sketch'
        pts, segs = tree_to_internal(s)
        area, err = loops_area(pts, segs)
        if err: return None, None, '%s: %s' % (f['name'], err)
        pl = s['plane']
        n = [pl['x_axis'][1]*pl['y_axis'][2]-pl['x_axis'][2]*pl['y_axis'][1],
             pl['x_axis'][2]*pl['y_axis'][0]-pl['x_axis'][0]*pl['y_axis'][2],
             pl['x_axis'][0]*pl['y_axis'][1]-pl['x_axis'][1]*pl['y_axis'][0]]
        if f['type'] == 'extrude':
            if f.get('distance') is None: return None, None, '%s: no distance' % f['name']
            dist = f['distance']
            sign = -1 if f['operation'] == 'cut' else 1
            if f['operation'] == 'cut': why = 'contains cuts (volume assumes the cut lies fully inside)'
            vol += sign * area * abs(dist)
            lo, hi = (-abs(dist) / 2, abs(dist) / 2) if f.get('symmetric') else (0, dist)
            if f['operation'] != 'cut':
                for poly in build_polys(pts, segs):
                    for (x, y) in poly:
                        for h in (lo, hi):
                            q = to3d(pl, x, y); pts3.append([q[i] + h * n[i] for i in range(3)])
        else:
            # Pappus about the axis line
            ax = f.get('axis')
            if not ax: return None, None, '%s: no axis' % f['name']
            c = next(c for c in s['curves'] if c['id'] == ax['line'])
            P = pts_of(s)[c['start']]; Q = pts_of(s)[c['end']]
            # centroid distance of the profile polygon from the axis
            import swfeat
            dx, dy = Q[0] - P[0], Q[1] - P[1]; L = math.hypot(dx, dy)
            # polygon centroid via sampled loops
            from swfeat import sample, shoelace
            # reuse loops_area by sampling: approximate centroid distance with fine grid
            xs = [p['x'] for p in pts]; ys = [p['y'] for p in pts]
            N = 400; acc = 0.0; tot = 0.0
            x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
            # grid integration using point-in-profile through loops_area polygons
            from swfeat import inside
            polys = build_polys(pts, segs)
            for i in range(N):
                for j in range(N):
                    X = x0 + (x1 - x0) * (i + .5) / N; Y = y0 + (y1 - y0) * (j + .5) / N
                    k = sum(1 for poly in polys if inside((X, Y), poly))
                    if k % 2:
                        r = abs((X - P[0]) * dy - (Y - P[1]) * dx) / L
                        acc += r; tot += 1
            cell = (x1 - x0) * (y1 - y0) / N / N
            v = 2 * math.pi * acc * cell * (f.get('angle', 360) / 360.0)
            vol += -v if f['operation'] == 'cut' else v
    if pts3:
        lo = [min(p[i] for p in pts3) for i in range(3)]; hi = [max(p[i] for p in pts3) for i in range(3)]
    else: lo = hi = None
    return vol, (lo, hi), why

def build_polys(pts, segs):
    import collections
    from swfeat import sample
    polys = []
    circles = [s for s in segs if s['type'] == 'arc' and s['s'] == s['e']]
    for c in circles:
        P, C = pts[c['s']], pts[c['c']]; r = math.hypot(P['x'] - C['x'], P['y'] - C['y'])
        polys.append([(C['x'] + r * math.cos(2 * math.pi * k / 128), C['y'] + r * math.sin(2 * math.pi * k / 128)) for k in range(128)])
    other = [s for s in segs if s not in circles]
    adj = collections.defaultdict(list)
    for i, s in enumerate(other): adj[s['s']].append(i); adj[s['e']].append(i)
    used = set()
    for i0, s0 in enumerate(other):
        if i0 in used: continue
        poly = []; cur = s0['s']; i = i0
        while True:
            used.add(i); s = other[i]; fwd = s['s'] == cur
            poly += sample(pts, s, fwd); cur = s['e'] if fwd else s['s']
            nxt = [j for j in adj[cur] if j not in used]
            if cur == s0['s'] or not nxt: break
            i = nxt[0]
        polys.append(poly)
    return polys

if __name__ == '__main__':
    truth = json.load(open(sys.argv[1]))
    rows = []
    for fn in sys.argv[2:]:
        name = os.path.basename(fn)[:-5]
        if name not in truth: continue
        tree = json.load(open(fn))
        try:
            v, box, why = check(tree, truth[name])
        except Exception as e:
            v, box, why = None, None, 'error %r' % e
        t = truth[name]; tv = t['parasolid_volume'][0] if t.get('parasolid_volume') else None
        line = '%-32s truth %10.3f' % (name, tv if tv is not None else float('nan'))
        if v is not None:
            line += '  proto %10.3f  ratio %.4f' % (v, v / tv if tv else float('nan'))
            if box and box[0]:
                err = max(max(abs(a - b) for a, b in zip(box[0], t['min'])), max(abs(a - b) for a, b in zip(box[1], t['max'])))
                line += '  bbox-err %.3f mm' % err
        line += '  ' + (why or '')
        print(line)
