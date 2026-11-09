"""Profile picks: which regions of a sketch an extrude/revolve uses, from its loops in the
history blob (bodies by tag), as one sketch-plane point (mm) per Gusset region."""
import math, struct, collections, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dc import ref


def loop_tags(d, lst):
    """Loops list (22947391) -> [(flag byte, body tag)] via a477243b -> cc0f7521."""
    items = []
    r = d.r[lst]; k = r.find(b'\x02\x00\x00\x30', d.body(lst)); n = struct.unpack_from('<I', r, k + 4)[0]
    for j in range(n):
        x = ref(struct.unpack_from('<I', r, k + 16 + 4 * j)[0], d.n)
        if x is None or not d.g[x].startswith('a477243b'): return None
        rr = d.r[x]; fl = rr[13]
        c = [y for kk, y in d.refs(x) if d.g[y].startswith('cc0f7521')]
        if not c: return None
        cr = d.r[c[-1]]; hb = [kk for kk, y in d.refs(c[-1]) if d.role[y] == 'HistoryBlob']
        if not hb: return None
        tag = struct.unpack_from('<I', cr, hb[0] + 4)[0]
        items.append((fl, tag))
    return items


def to2d(p, M):
    o = (M[3], M[7], M[11]); x = (M[0], M[4], M[8]); y = (M[1], M[5], M[9])
    q = [p[i] - o[i] for i in range(3)]
    return (sum(q[i] * x[i] for i in range(3)) * 10, sum(q[i] * y[i] for i in range(3)) * 10,
            sum(q[i] * (x[(i+1)%3]*y[(i+2)%3]-x[(i+2)%3]*y[(i+1)%3]) for i in range(3)) * 10)


def inside(poly, p):
    c = False; n = len(poly)
    for i in range(n):
        a = poly[i]; b = poly[(i + 1) % n]
        if (a[1] > p[1]) != (b[1] > p[1]):
            xx = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
            if p[0] < xx: c = not c
    return c


def sample_curves(js):
    """Sketch curves (non-construction) as 2D polylines (mm)."""
    P = {p['id']: (p['x'], p['y']) for p in js['points']}
    out = []
    for c in js['curves']:
        if c.get('construction'): continue
        if c['type'] == 'line':
            out.append([P[c['start']], P[c['end']]])
        elif c['type'] == 'circle':
            cx, cy = P[c['center']]; r = c['radius']
            out.append([(cx + r * math.cos(2 * math.pi * k / 96), cy + r * math.sin(2 * math.pi * k / 96)) for k in range(97)])
        elif c['type'] == 'arc':
            cx, cy = P[c['center']]; s = P[c['start']]; e = P[c['end']]
            r = math.dist((cx, cy), s); a0 = math.atan2(s[1] - cy, s[0] - cx); a1 = math.atan2(e[1] - cy, e[0] - cx)
            while a1 <= a0 + 1e-12: a1 += 2 * math.pi
            n = max(4, int(48 * (a1 - a0) / math.pi))
            out.append([(cx + r * math.cos(a0 + (a1 - a0) * k / n), cy + r * math.sin(a0 + (a1 - a0) * k / n)) for k in range(n + 1)])
    return out


def picks(js, loops2d, N=500):
    """Grid flood fill over the sketch's curves; one point per region inside the profile
    (odd number of profile loops around it)."""
    polys = sample_curves(js)
    allp = [p for pl in polys for p in pl] + [p for l in loops2d for p in l]
    if not allp: return None
    x0 = min(p[0] for p in allp); x1 = max(p[0] for p in allp); y0 = min(p[1] for p in allp); y1 = max(p[1] for p in allp)
    span = max(x1 - x0, y1 - y0, 1e-6); x0 -= span * 0.02; y0 -= span * 0.02; h = span * 1.04 / N
    nx = int((x1 - x0) / h) + 3; ny = int((y1 - y0) / h) + 3
    wall = bytearray(nx * ny)
    for pl in polys:
        for a, b in zip(pl, pl[1:]):
            L = math.dist(a, b); m = max(1, int(L / (h * 0.25)))
            for k in range(m + 1):
                x = a[0] + (b[0] - a[0]) * k / m; y = a[1] + (b[1] - a[1]) * k / m
                i = int((x - x0) / h); j = int((y - y0) / h)
                if 0 <= i < nx and 0 <= j < ny: wall[j * nx + i] = 1
    comp = [-1] * (nx * ny); out = []
    for s in range(nx * ny):
        if wall[s] or comp[s] >= 0: continue
        cid = len(out); q = collections.deque([s]); comp[s] = cid; cells = []; border = False
        while q:
            c = q.popleft(); cells.append(c); i = c % nx; j = c // nx
            if i == 0 or j == 0 or i == nx - 1 or j == ny - 1: border = True
            for di, dj in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                ii = i + di; jj = j + dj
                if 0 <= ii < nx and 0 <= jj < ny:
                    t = jj * nx + ii
                    if not wall[t] and comp[t] < 0: comp[t] = cid; q.append(t)
        out.append((border, cells))
    res = []
    for cid, (border, cells) in enumerate(out):
        if border or len(cells) < 4: continue
        # deepest cell: multi-source BFS from the component's boundary
        cs = set(cells); dist = {}
        q = collections.deque()
        for c in cells:
            i = c % nx; j = c // nx
            if any((jj * nx + ii) not in cs for ii, jj in ((i+1, j), (i-1, j), (i, j+1), (i, j-1))):
                dist[c] = 0; q.append(c)
        while q:
            c = q.popleft(); i = c % nx; j = c // nx
            for ii, jj in ((i+1, j), (i-1, j), (i, j+1), (i, j-1)):
                t = jj * nx + ii
                if t in cs and t not in dist: dist[t] = dist[c] + 1; q.append(t)
        best = max(cells, key=lambda c: dist.get(c, 0))
        p = (x0 + (best % nx + 0.5) * h, y0 + (best // nx + 0.5) * h)
        k = sum(1 for l in loops2d if len(l) > 2 and inside(l, p))
        if k % 2 == 1: res.append([round(p[0], 6), round(p[1], 6)])
    return res
