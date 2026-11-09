"""Extrude face generators -> profile curves (rule derived from ACIS face tags, see LAYOUT.md 11).
Faces of an extrude are named by decimal text: "1" start cap (at t0), "2" end cap (at t1), then
"3", "4", ... one per profile curve: loops in profile-record order, each loop walked counter-
clockwise about the sketch normal (holes clockwise: guess), starting with the curve that, in that
walk, starts at the start point of the loop's highest-record-id curve."""
import math

def loop_ring(segs, ccw=True):
    segs = list(segs)
    if not segs: return None
    out = [(segs[0], True)]; used = {0}; cur = segs[0]['poly'][-1]
    while len(used) < len(segs):
        for k, sg in enumerate(segs):
            if k in used: continue
            if math.dist(sg['poly'][0], cur) < 1e-5: out.append((sg, True)); cur = sg['poly'][-1]; used.add(k); break
            if math.dist(sg['poly'][-1], cur) < 1e-5: out.append((sg, False)); cur = sg['poly'][0]; used.add(k); break
        else:
            return None
    pts = []
    for sg, fw in out: pts += sg['poly'] if fw else list(reversed(sg['poly']))
    A = sum(pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1] for i in range(len(pts))) / 2
    if (A > 0) != ccw:
        out = [(out[0][0], not out[0][1])] + [(x, not f) for x, f in reversed(out[1:])]
    return out

def start_of(x, f): return x['poly'][0] if f else x['poly'][-1]

def gen_map(segs):
    """{generator text: segment} for the side faces of an extrude of these profile segments."""
    loops = []
    for sg in segs:
        key = id(sg.get('loopkey')) if sg.get('loopkey') is not None else None
        if loops and loops[-1][0] == sg.get('loopkey'): loops[-1][1].append(sg)
        else: loops.append((sg.get('loopkey'), [sg]))
    out = {}; g = 3
    for key, ls in loops:
        ring = loop_ring(ls, ccw=(ls[0]['flag'] == 1))
        if ring is None: return None
        mx = max(ring, key=lambda t: t[0]['curve'])[0]
        anchor = mx['poly'][0]
        k = next((i for i, (x, f) in enumerate(ring) if math.dist(start_of(x, f), anchor) < 1e-6), 0)
        ring = ring[k:] + ring[:k]
        for x, f in ring:
            out[str(g)] = (x, f); g += 1
    return out
