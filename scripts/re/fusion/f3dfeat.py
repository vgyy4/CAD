#!/usr/bin/env python3
"""Clean-room prototype: Fusion 360 design timeline (sketches + features) -> Gusset feature-tree JSON.

    python3 -I f3dfeat.py OUTDIR sample.f3d|.f3z ...

Reads only the ZIP entries Design1/{MetaStream,BulkStream}.dat (and FusionDesignSegmentType1/ if
framed the same way). See LAYOUT.md for the byte layout this relies on. Samples are untrusted
data: everything is bounded, nothing is executed.
"""
import sys, os, json, math, struct, hashlib, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import f3dzip, fdesign, regions, naming

# Class GUIDs (stable across streams; the per-stream tag is 256 + index in the MetaStream class table)
C = dict(
    TIMELINE='2F4C1849-1A5A-4F6C-A086-8DD445CBF94B',
    SKETCH_FEAT='8DA771B7-52ED-42FC-94E2-2353FE141373',
    SKETCH='44A64366-4BD3-4B24-881A-F94C206E8F2D',
    SKETCH_BASE='37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9',
    SK_XFORM='F47A46FB-DA27-4AFA-8A72-1B38FA596E23',
    POINT='C2CEDAE7-1716-47C1-B7B1-07B70081D0FB',
    LINE='DCA267ED-D615-4934-B64F-AD805E8003E2',
    ARC='F0130424-8B7E-4092-93C9-1CA807482534',
    EXTRUDE='DD405BC2-D673-44F0-8833-5CB2A1C186C7',
    PROFILE='4BD53E5A',
    PARAM_HOLDER='D91D429C-1BB5-4640-B49F-69AF14F463E9',
    PARAM='7A6A3D31-BE74-4E19-9115-20642944C2E4',
    FILLET='A07D5F17',
    CHAMFER='F757D611',
    REVOLVE='E3849A15',
)
ZERO_UUID = '00000000-0000-0000-0000-000000000000'

def cls(d, i): return d.cls.get(i, '')
def is_cls(d, i, key): return cls(d, i).startswith(C[key][:8])

def tag_class(d, tag):
    k = tag - 256
    cl = d.meta['classes']
    return cl[k][0] if 0 <= k < len(cl) else None

def section_bodies(d, i):
    """[(class guid, body offset, end)] for every section of record i (see LAYOUT.md 'Records')."""
    rec = d.rec(i)
    secs = fdesign.sections(rec)
    out = []
    for k, (o, tag) in enumerate(secs):
        end = secs[k + 1][0] if k + 1 < len(secs) else len(rec)
        if k == 0:
            try: _, _, _, _, body = fdesign.parse_header(rec)
            except Exception: body = None
        else:
            body = o + 7 + 8 + 4
        out.append((tag_class(d, tag), body, end))
    return out

def find_section(d, i, guid):
    for c, body, end in section_bodies(d, i):
        if c == guid and body is not None: return body, end
    return None

def dbl(rec, o, n):
    if o is None or o < 0 or o + 8 * n > len(rec): return None
    v = struct.unpack_from('<%dd' % n, rec, o)
    return list(v) if all(math.isfinite(x) and abs(x) < 1e9 for x in v) else None

def u64ref(rec, o):
    """A reference: 01 + u64 id (+ 00). None when absent."""
    if o + 9 <= len(rec) and rec[o] == 1: return struct.unpack_from('<Q', rec, o + 1)[0]
    return None

# ----- sketch entities -------------------------------------------------------------------
def point(d, i):
    s = find_section(d, i, C['POINT'])
    if not s: return None
    rec = d.rec(i); b, _ = s
    # body: ref to the point's curve list, 9 bytes, then x y z (cm, sketch space)
    # (9 bytes in most class versions, 10 in one older typed-reference version: take the offset
    #  whose values are sane: |v| < 1e5 cm, no denormal garbage, z == 0)
    r = fdesign.ref_end(d, rec, b)
    if not r: return None
    for gap in (9, 10, 8, 11):
        v = dbl(rec, r[1] + gap, 3)
        if v and v[2] == 0.0 and all(x == 0.0 or 1e-200 < abs(x) < 1e5 for x in v): return v
    return None

def line(d, i):
    s = find_section(d, i, C['LINE'])
    if not s: return None
    rec = d.rec(i); b, e = s
    v = dbl(rec, b, 9)   # origin, vector (end - start), unit direction; newer versions add 3 more
    if not v: return None
    pts = [q for o, q in fdesign.refs_at(d, rec[:e], b + 72) if has_point(d, q)][:2]
    return dict(origin=v[0:3], vector=v[3:6], end_ref=pts[0] if pts else None,
                start_ref=pts[1] if len(pts) > 1 else None)

def has_point(d, q):
    return any(c == C['POINT'] for c, _, _ in section_bodies(d, q))

def arc(d, i):
    s = find_section(d, i, C['ARC'])
    if not s: return None
    rec = d.rec(i); b, e = s
    v = dbl(rec, b, 12)
    if not v: return None
    refs = [q for o, q in fdesign.refs_at(d, rec[:e], b + 96) if has_point(d, q)][:3]
    return dict(center=v[0:3], normal=v[3:6], xdir=v[6:9], radius=v[9], a0=v[10], a1=v[11], refs=refs)

def xform(d, i):
    """Sketch placement: 16 doubles, row-major 4x4, columns = sketch x, y, normal, origin (cm)."""
    rec = d.rec(i)
    for o in range(15, min(len(rec) - 127, 64)):
        m = dbl(rec, o, 16)
        if not m: continue
        if abs(m[12]) > 1e-9 or abs(m[13]) > 1e-9 or abs(m[14]) > 1e-9 or abs(m[15] - 1) > 1e-9: continue
        cols = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]]
        if all(abs(sum(c * c for c in col) - 1) < 1e-6 for col in cols):
            return dict(x=cols[0], y=cols[1], z=cols[2], origin=[m[3], m[7], m[11]], at=o)
    # short form (no matrix): u32 0, u8, u8, then 01 00 and a ref to the component -> identity (assumed)
    if len(rec) < 100 and rec[21:23] == b'\x01\x00':
        return dict(x=[1, 0, 0], y=[0, 1, 0], z=[0, 0, 1], origin=[0, 0, 0], identity_assumed=True)
    return None

def sketch(d, sid):
    cache = d.__dict__.setdefault('_sk', {})
    if sid not in cache: cache[sid] = _sketch(d, sid)
    return cache[sid]

def _sketch(d, sid):
    rec = d.rec(sid)
    s = find_section(d, sid, C['SKETCH_BASE'])
    out = dict(id=sid, plane=None, points={}, lines={}, arcs={}, other={})
    if not s: return out
    b, e = s
    r = fdesign.ref_end(d, rec, b)
    xf = r[0] if r else None
    if xf is not None and cls(d, xf) == C['SK_XFORM']: out['plane'] = xform(d, xf)
    ents = set()
    for o, st, ids in fdesign.ref_lists(d, rec[:e], b):
        ents.update(ids)
    for q in sorted(ents):
        secs = {c for c, _, _ in section_bodies(d, q)}
        if C['POINT'] in secs:
            p = point(d, q)
            if p: out['points'][q] = p
        elif C['LINE'] in secs:
            l = line(d, q)
            if l: out['lines'][q] = l
        elif C['ARC'] in secs:
            a = arc(d, q)
            if a: out['arcs'][q] = a
        else:
            out['other'][q] = cls(d, q)
    return out

# ----- features --------------------------------------------------------------------------
NAME_RE = re.compile(r'^[A-Za-z][A-Za-z0-9 _-]*$')
UUID_RE = re.compile(r'^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$')
def feature_name(d, i):
    """(base, display name): a UUID string (zero in newer files), the base name, u32 index, 4 bytes,
    then the user's name (u32 length 0 when none)."""
    rec = d.rec(i); ws = fdesign.wstrings(rec)
    for j, (o, e, s) in enumerate(ws):
        if UUID_RE.match(s) and j + 1 < len(ws) and ws[j + 1][0] >= e and ws[j + 1][0] - e < 1024:
            o2, e2, base = ws[j + 1]
            if UUID_RE.match(base) or base.startswith('Prism') or not NAME_RE.match(base): continue
            if e2 + 12 > len(rec): return base, base
            n = struct.unpack_from('<I', rec, e2)[0]
            name = (base + str(n)) if n < 100000 else base
            L = struct.unpack_from('<I', rec, e2 + 8)[0]
            if 0 < L < 256 and e2 + 12 + 2 * L <= len(rec):
                try:
                    custom = rec[e2 + 12:e2 + 12 + 2 * L].decode('utf-16le')
                    if custom.isprintable(): name = custom
                except UnicodeDecodeError: pass
            return base, name
    return None, None

def params_of(d, i):
    """{name: param dict} for every model parameter a feature references (D91D429C -> 7A6A3D31)."""
    out = {}
    for o, q in fdesign.refs_at(d, d.rec(i)):
        if cls(d, q) == C['PARAM_HOLDER']:
            for o2, q2 in fdesign.refs_at(d, d.rec(q)):
                if cls(d, q2) == C['PARAM']:
                    p = fdesign.param(d, q2)
                    if p and p.get('name'): out.setdefault(p['name'], p)
    return out

def param_values(d, i, names):
    """All values of parameters with these role names that feature i references."""
    out = []
    for o, q in fdesign.refs_at(d, d.rec(i)):
        if cls(d, q) == C['PARAM_HOLDER']:
            for o2, q2 in fdesign.refs_at(d, d.rec(q)):
                if cls(d, q2) == C['PARAM']:
                    p = fdesign.param(d, q2)
                    if p and p.get('name') in names and p['value'] is not None and p['value'] not in out: out.append(p['value'])
    return out

def find_rigid_dir(rec, b, span=40):
    """The unit axis vector (3 doubles) after the extrude's enums."""
    for o in range(b, b + span):
        v = dbl(rec, o, 3)
        if v and abs(math.sqrt(sum(x * x for x in v)) - 1) < 1e-6 and max(abs(x) for x in v) > 0.5:
            return v, o
    return None, None

def profile_sketch(d, i):
    """The sketch a feature's profile (4BD53E5A record) names: its UTF-16 decimal text is the sketch's object id."""
    for o, q in fdesign.refs_at(d, d.rec(i)):
        if cls(d, q).startswith(C['PROFILE']):
            for w in fdesign.wstrings(d.rec(q)):
                if w[2].isdigit() and int(w[2]) in d.objs and cls(d, int(w[2])) == C['SKETCH']:
                    return int(w[2])
    return None

def extrude_raw(d, i):
    rec = d.rec(i)
    try: _, _, _, attrs, b = fdesign.parse_header(rec)
    except Exception: return None
    b = max(b - 8, 15)
    # u32 operation, u32 extent, u32 kind, then 00 01 (position varies with the class version)
    ENUM = re.compile(rb'([\x01-\x05])\x00\x00\x00([\x01-\x04])\x00\x00\x00([\x00-\x03])\x00\x00\x00[\x00\x01]\x01', re.S)
    m = ENUM.search(rec, b, b + 64)
    if not m and b'SpecifiedColor' in rec[:200]:
        # the colour attribute is a nested object of variable size: search after its appearance name
        k = rec.find('PrismOpaque'.encode('utf-16le'))
        if k < 0: k = rec.find(b'P\x00r\x00i\x00s\x00m\x00')
        if k > 0: m = ENUM.search(rec, k, k + 200)
    if not m: return None
    op, ext, kind = m.group(1)[0], m.group(2)[0], m.group(3)[0]
    vec, at = find_rigid_dir(rec, m.end())
    b = m.start() - 3
    prof = None
    for o, q in fdesign.refs_at(d, rec):
        if cls(d, q).startswith(C['PROFILE']):
            for w in fdesign.wstrings(d.rec(q)):
                if w[2].isdigit() and int(w[2]) in d.objs and cls(d, int(w[2])) == C['SKETCH']:
                    prof = int(w[2])
    return dict(flag=rec[m.start() - 1], op=op, extent=ext, kind=kind, vec=vec, sketch=prof, params=params_of(d, i))


# ----- round 4: holes, pattern directions, mirror planes (LAYOUT.md section 12) --------------
def under(d, fid, want, depth=4, via=None):
    """Records of class prefix `want` reachable from fid through child records (breadth first)."""
    out = []; seen = {fid}; front = [fid]
    for _ in range(depth):
        nxt = []
        for x in front:
            for o, q in fdesign.refs_at(d, d.rec(x)):
                if q in seen: continue
                seen.add(q); c = cls(d, q)[:8]
                if c == want[:8]: out.append(q)
                elif via is None or c in via: nxt.append(q)
        front = nxt
    return out

def finite_doubles(rec, o, n, lim=1e6):
    v = dbl(rec, o, n)
    return v if v and all(abs(x) < lim for x in v) else None

def plane_frame(d, q):
    """CC54ECAD: a plane as origin, x axis, y axis (3 doubles each, cm); normal = x cross y."""
    rec = d.rec(q)
    for o in range(15, min(len(rec) - 72, 40)):
        v = finite_doubles(rec, o, 9)
        if not v: continue
        x, y = v[3:6], v[6:9]
        if abs(sum(a * a for a in x) - 1) < 1e-9 and abs(sum(a * a for a in y) - 1) < 1e-9 and abs(sum(a * b for a, b in zip(x, y))) < 1e-9:
            n = [x[1]*y[2]-x[2]*y[1], x[2]*y[0]-x[0]*y[2], x[0]*y[1]-x[1]*y[0]]
            return dict(origin=v[0:3], x=x, y=y, normal=n)
    return None

def point3(d, q):
    """994C518A: a 3D point (x y z f64 cm) right after the 6-byte record preamble."""
    rec = d.rec(q)
    for o in (21, 22, 20, 23):
        v = finite_doubles(rec, o, 3)
        if v and any(abs(a) > 1e-12 for a in v): return v
    return None

def hole_json(d, fid, name):
    ps = params_of(d, fid)
    j = dict(type='hole', name=name, source_id=fid)
    pts = []
    for q in under(d, fid, '994C518A', 4):
        p = point3(d, q)
        if p and p not in pts: pts.append(p)
    frames = [plane_frame(d, q) for q in under(d, fid, 'CC54ECAD', 4)]
    frames = [f for f in frames if f]
    if not pts or not frames:
        j['type'] = 'unsupported'; j['fusion_type'] = 'Hole'; j['error'] = 'hole position / face not found'
        return j
    fr = frames[0]
    j['face'] = dict(point=[mm(x) for x in pts[0]], normal=[round(x, 9) for x in fr['normal']])
    j['positions'] = [[mm(x) for x in p] for p in pts]
    diam = ps.get('HoleDiameter')
    if not diam or diam['value'] is None:
        j['type'] = 'unsupported'; j['fusion_type'] = 'Hole'; j['error'] = 'no HoleDiameter'
        return j
    j['diameter'] = mm(diam['value'])
    dep = ps.get('HoleDepth')
    if dep and dep['value'] is not None: j['depth'] = mm(dep['value'])
    else: j['through'] = True; j['extent_note'] = 'GUESS: no HoleDepth parameter => through (no sample)'
    tip = ps.get('TipAngle')
    if tip and tip['value'] is not None: j['point_angle'] = round(math.degrees(tip['value']), 6)
    j['variant'] = 'simple'
    # counterbore / countersink role names are not in any sample: GUESS by substring
    for k, p in ps.items():
        kl = k.lower()
        if p['value'] is None: continue
        if 'bore' in kl and 'diam' in kl: j['cbore_diameter'] = mm(p['value']); j['variant'] = 'counterbore'
        elif 'bore' in kl and 'depth' in kl: j['cbore_depth'] = mm(p['value'])
        elif 'sink' in kl and 'diam' in kl: j['csink_diameter'] = mm(p['value']); j['variant'] = 'countersink'
        elif 'sink' in kl and 'angle' in kl: j['csink_angle'] = round(math.degrees(p['value']), 6)
    if j['variant'] != 'simple': j['variant_note'] = 'GUESS: role names not seen in any sample'
    return j

def pattern_dirs(d, fid):
    """[(point cm, vector cm, flags)] of the F2A7590D direction records of a pattern, in U, V order.
    Layout: 15-byte preamble, u32 flags at 21 (8 = as stored, 16 = reversed), then point(3) and
    vector(3) doubles at 25; the vector is the direction (sign included)."""
    out = []
    for o, q in fdesign.refs_at(d, d.rec(fid)):
        if cls(d, q)[:8] != 'F2A7590D': continue
        rec = d.rec(q)
        v = finite_doubles(rec, 25, 6)
        if not v: continue
        n = math.sqrt(sum(a * a for a in v[3:6]))
        if n < 1e-12: continue
        out.append(dict(point=v[0:3], dir=[a / n for a in v[3:6]], length=n, flags=struct.unpack_from('<I', rec, 21)[0], source_id=q))
    return out

def pattern_symmetric(d, fid):
    """(u, v) symmetric flags: three doubles after the first ff ff ff ff of the pattern's EA0E146E record."""
    for o, q in fdesign.refs_at(d, d.rec(fid)):
        if cls(d, q)[:8] != 'EA0E146E': continue
        rec = d.rec(q); k = rec.find(b'\xff\xff\xff\xff')
        if k < 0: continue
        v = finite_doubles(rec, k + 4, 3)
        if v and all(abs(a) in (0.0, 1.0) for a in v): return [bool(a) for a in v]
    return None

def mirror_plane(d, fid):
    for q in under(d, fid, 'CC54ECAD', 4):
        f = plane_frame(d, q)
        if f: return dict(origin=[mm(a) for a in f['origin']], normal=[round(a, 9) for a in f['normal']])
    return None

# ----- timeline and JSON -----------------------------------------------------------------
OPS = {1: 'join', 2: 'cut', 3: 'intersect', 4: 'new'}   # 1/2/4 seen with evidence; 3 a guess

# Classes whose records carry a feature name (UUID + base name + index) in the samples' timelines
FEATURE_PREFIXES = {'78B9DEFC', 'DD405BC2', '8DA771B7', 'A07D5F17', 'F757D611', 'F4163A07', 'D1728651',
    'AA8B3D40', 'D087EFE5', '428B5C6E', 'D869265F', 'AE12AD3F', '64E40C8E', 'E8063467', 'E8816367',
    'D7015B81', '8DFDD543', '02E8AA49', 'FCBB1707', '2A94257F', '1339A425', 'D45FD6C4', '8EE00B00',
    '4A782808', 'B6678572', 'D3DC9613', 'E3849A15', '1C037A07', '21B67709', '11F1A5CE', 'B0EEF272',
    '0EE4A83E', '54F9ACE8'}
def timeline(d):
    """Feature ids in timeline order: the ref lists of the 2F4C1849 record(s). Features that are not
    in a timeline (designs without captured history keep some) follow in compute order (the ref
    list of B65B5682), flagged by the caller."""
    out = []
    for i in d.meta['order']:
        if cls(d, i) == C['TIMELINE']:
            for o, st, ids in fdesign.ref_lists(d, d.rec(i)):
                for q in ids:
                    if q not in out: out.append(q)
    extra = []
    for i in d.meta['order']:
        if cls(d, i).startswith('B65B5682'):
            for o, st, ids in fdesign.ref_lists(d, d.rec(i)):
                for q in ids:
                    if q in out or q in extra: continue
                    if cls(d, q)[:8] in FEATURE_PREFIXES and feature_name(d, q)[0]:
                        extra.append(q)
    return out, extra

def mm(v): return round(v * 10.0, 6)

def ccw_arc(a, pts):
    """Arc -> (center id, start id, end id) counterclockwise about the sketch normal (+z)."""
    c, e, s = (a['refs'] + [None, None, None])[:3]   # stored order: center, end, start (see LAYOUT.md)
    if a['normal'][2] < 0: s, e = e, s
    return c, s, e

def sketch_json(d, fid, name):
    sid = None
    for o, q in fdesign.refs_at(d, d.rec(fid)):
        if cls(d, q) == C['SKETCH']: sid = q; break
    if sid is None: return None, None
    s = sketch(d, sid)
    pl = s['plane']
    j = dict(type='sketch', name=name, source_id=sid)
    if pl:
        j['plane'] = dict(origin=[mm(x) for x in pl['origin']], x_axis=pl['x'], y_axis=pl['y'])
        if pl.get('identity_assumed'): j['plane_note'] = 'no matrix stored; identity (component XY) assumed'
    else:
        j['plane'] = None
    j['points'] = [dict(id='p%d' % q, x=mm(p[0]), y=mm(p[1])) for q, p in s['points'].items()]
    curves = []
    for q, l in s['lines'].items():
        if l['start_ref'] in s['points'] and l['end_ref'] in s['points']:
            curves.append(dict(id='c%d' % q, type='line', start='p%d' % l['start_ref'], end='p%d' % l['end_ref']))
    for q, a in s['arcs'].items():
        full = abs(abs(a['a1'] - a['a0']) - 2 * math.pi) < 1e-9
        c, st, en = ccw_arc(a, s['points'])
        if full and c in s['points']:
            curves.append(dict(id='c%d' % q, type='circle', center='p%d' % c, radius=mm(a['radius'])))
        elif c in s['points'] and st in s['points'] and en in s['points']:
            curves.append(dict(id='c%d' % q, type='arc', center='p%d' % c, start='p%d' % st, end='p%d' % en))
    j['curves'] = curves
    if s['other']:
        kinds = {}
        for q, c in s['other'].items(): kinds[c[:8]] = kinds.get(c[:8], 0) + 1
        j['unsupported_entities'] = kinds   # dimensions, constraints, splines, text...
    return j, sid

def extrude_json(d, fid, name, sketch_names, EN=None):
    e = extrude_raw(d, fid)
    j = dict(type='extrude', name=name, source_id=fid)
    if not e: j['error'] = 'layout not recognised'; return j
    j['sketch'] = sketch_names.get(e['sketch']) if e['sketch'] else None
    j['sketch_source_id'] = e['sketch']
    j['profiles'] = 'all'
    if e['sketch']:
        rg = regions.feature_regions(sys.modules[__name__], d, fid, sketch(d, e['sketch']))
        if rg:
            j['profiles'] = [dict(point=[mm(r['point'][0]), mm(r['point'][1])]) for r in rg]
            j['_profile_bbox_mm'] = [[mm(v) for v in r['bbox']] for r in rg]
        else:
            j['profiles'] = None; j['profile_note'] = 'profile regions not decoded'
    ps = e['params']
    along = ps.get('AlongDistance'); against = ps.get('AgainstDistance')
    sign = 1.0
    if e['vec'] and e['sketch']:
        pl = sketch(d, e['sketch'])['plane']
        if pl: sign = 1.0 if sum(a * b for a, b in zip(pl['z'], e['vec'])) >= 0 else -1.0
    j['distance'] = mm(along['value']) * sign if along and along['value'] is not None else None
    if along: j['distance_expr'] = along['expr']
    j['symmetric'] = e['extent'] == 3
    if e['extent'] == 2 and against:
        j['two_sided'] = True; j['distance2'] = mm(against['value']) * -sign; j['distance2_expr'] = against['expr']
    if j['distance'] is None and 'Side1Offset' in ps and EN is not None:
        # to-object: the first face of the extrude's bounded_face_recipe, if it is a plane parallel to the sketch
        import edgeprobe as EP
        off = ps['Side1Offset']['value'] or 0.0
        for rid in EP.edge_recipes_of(d, fid, 3):
            kind, faces, _ = naming.recipe(sys.modules[__name__], d, rid)
            if kind != 'bounded_face_recipe_data' or not faces: continue
            f = EN.face(faces[0][0])
            pl = sketch(d, e['sketch'])['plane'] if e['sketch'] else None
            if f and f['kind'] == 'plane' and pl and abs(abs(sum(a * b for a, b in zip(f['n'], pl['z']))) - 1) < 1e-9:
                t = sum(f['n'][k] * (f['p'][k] - pl['origin'][k]) for k in range(3)) / sum(f['n'][k] * pl['z'][k] for k in range(3))
                j['distance'] = mm(t + (off if t >= 0 else -off)); j['extent_from'] = 'to object (face %s of %s)' % (faces[0][0][0], f['feature'])
            break
    if j['distance'] is None and not ps.get('AlongDistance') and 'Side1Offset' not in ps and e['sketch'] and CURRENT_FILES:
        # through all: the cut walls leave copies of the profile's straight edges in the body at the
        # entry and exit faces; their offsets along the normal give the side and the reach
        t = through_reach(d, fid, e)
        if t:
            j['distance'] = mm(t); j['extent_from'] = 'through all (reach measured on the stored body)'
    if j['distance'] is None: j['extent'] = 'not a distance (to object / through all)'
    j['operation'] = OPS.get(e['op'], 'unknown(%d)' % e['op'])
    j['target'] = None
    taper = ps.get('TaperAngle')
    if taper and taper['value']: j['taper_deg'] = round(math.degrees(taper['value']), 6)
    if e['sketch'] is None: j['profile_note'] = 'profile is not a sketch profile (face or PCB data)'
    return j

def simple_json(d, fid, typ, name, keys):
    ps = params_of(d, fid)
    j = dict(type=typ, name=name, source_id=fid)
    for k, (pname, conv) in keys.items():
        if pname in ps and ps[pname]['value'] is not None:
            j[k] = conv(ps[pname]['value']); j[k + '_expr'] = ps[pname]['expr']
    return j

# Timeline entries that create no body geometry (confirmed by class: they reference no body record
# D3937028 and are not referenced as a body's creator in any D26351F0 record of the corpus).
NON_GEOMETRIC = {
    '78B9DEFC': 'CreateComponent', '54F9ACE8': 'component activation', '02E8AA49': 'JointOrigin',
    '8DFDD543': 'Assemble (joint)', 'AA8B3D40': 'As-built joint', '8EE00B00': 'Position (assembly)',
    '0EE4A83E': 'Canvas (image)', '1339A425': 'Decal', 'D869265F': 'WorkPlane (construction)',
    'B0EEF272': 'PCB link (bodies come from the extrudes that follow)',
}
PATTERNS = {'AE12AD3F': 'pattern_linear', 'E8816367': 'pattern_linear', '11F1A5CE': 'pattern_circular'}
MIRROR = 'D1728651'

class Graph:
    """Body/feature/component relations of one design stream."""
    def __init__(self, d):
        self.d = d
        self.creator = {}      # D26351F0 body-link id -> creating feature id
        self.link_body = {}    # D26351F0 id -> body record (D3937028)
        self.comp_of_tl = {}   # C3423E04 -> component (E03784ED)
        self.tl_of_3a6d = {}   # 3A6D1E62 -> C3423E04
        for i in d.objs:
            c = cls(d, i)[:8]
            if c == 'D26351F0':
                rs = [q for o, q in fdesign.refs_at(d, d.rec(i))]
                for q in rs:
                    if cls(d, q)[:8] == 'D3937028': self.link_body.setdefault(i, q)
                    elif cls(d, q)[:8] not in ('C3423E04', '2C54F6D6'): self.creator.setdefault(i, q)
            elif c == 'E03784ED':
                for o, q in fdesign.refs_at(d, d.rec(i)):
                    if cls(d, q)[:8] == 'C3423E04': self.comp_of_tl.setdefault(q, i)
            elif c == 'C3423E04':
                for o, q in fdesign.refs_at(d, d.rec(i)):
                    if cls(d, q)[:8] == '3A6D1E62': self.tl_of_3a6d.setdefault(q, i)
    def component(self, fid):
        for o, q in reversed(fdesign.refs_at(self.d, self.d.rec(fid))):
            if cls(self.d, q)[:8] == '3A6D1E62' and q in self.tl_of_3a6d:
                return self.comp_of_tl.get(self.tl_of_3a6d[q])
        return None
    def body_links(self, fid, depth=2):
        """D26351F0 records reachable from a feature through its own child records."""
        d = self.d; seen = {fid}; front = [fid]; out = set()
        for _ in range(depth):
            nxt = []
            for x in front:
                for o, q in fdesign.refs_at(d, d.rec(x)):
                    if q in seen: continue
                    seen.add(q); c = cls(d, q)[:8]
                    if c == 'D26351F0': out.add(q)
                    elif c in ('9716F783', '2CA5A1CD', '5A1BF548', '5662F619', '0897AF07', 'F2A7590D', '8DB90DAE', 'EA0E146E'):
                        nxt.append(q)
            front = nxt
        return out
    def inputs(self, fid):
        """Features a pattern/mirror copies: 6CCF41D5 lists under its 5A1BF548 input records."""
        d = self.d; out = []
        for o, q in fdesign.refs_at(d, d.rec(fid)):
            if cls(d, q)[:8] != '5A1BF548': continue
            for o2, q2 in fdesign.refs_at(d, d.rec(q)):
                if cls(d, q2)[:8] != '6CCF41D5': continue
                for o3, q3 in fdesign.refs_at(d, d.rec(q2)):
                    if q3 != q and cls(d, q3)[:8] not in ('5A1BF548', '8DA771B7') and feature_name(d, q3)[0] and q3 not in out:
                        out.append(q3)
        return out

def component_name(d, c):
    ws = [w[2] for w in fdesign.wstrings(d.rec(c)) if not UUID_RE.match(w[2]) and not w[2].startswith('PrismMaterial')]
    return ws[-1] if ws else None

def component_bodies(d, c):
    out = []
    for o, q in fdesign.refs_at(d, d.rec(c)):
        if cls(d, q)[:8] == 'CD57BC48':
            for o2, q2 in fdesign.refs_at(d, d.rec(q)):
                for w in fdesign.wstrings(d.rec(q2)):
                    if w[2].startswith('BREP.') and w[2].endswith('.smb') and w[2] not in out: out.append(w[2])
    return out

def blend_edges(d, fid, N, names_):
    """[(value cm, [edge dicts])] per radius/distance set of a fillet or chamfer."""
    import edges as ED, edgeprobe as EP
    sets = [q for o, q in fdesign.refs_at(d, d.rec(fid)) if cls(d, q)[:8] == '4A557CE2']
    groups = []
    if sets:
        for q in sets:
            vals = param_values(d, q, names_)
            groups.append((vals[0] if vals else None, EP.input_recipes(d, q)))
    else:
        vals = param_values(d, fid, names_)
        groups.append((vals[0] if vals else None, EP.input_recipes(d, fid)))
    out = []
    for v, recs in groups:
        es = []; seen = set(); bad = 0
        for rid in dict.fromkeys(recs):
            kind, faces, _ = naming.recipe(sys.modules[__name__], d, rid)
            if kind != 'edge_recipe_data' or not faces: continue
            r = ED.edge_midpoint(N, faces)
            if not r: bad += 1; continue
            key = tuple(round(x, 6) for x in r[0])
            if key in seen: continue
            seen.add(key)
            es.append(dict(midpoint=[mm(x) for x in r[0]], curve=r[1]))
        out.append((v, es, bad))
    return out

def pattern_direction(d, fid, EN, inputs, spacing):
    """Unit direction of a linear pattern from its direction edge (F2A7590D -> 5662F619 -> edge recipe),
    signed so that the first copy of an input extrude's straight profile edges exists in a stored body."""
    import edges as ED, edgeprobe as EP
    u = None
    for o, q in fdesign.refs_at(d, d.rec(fid)):
        if cls(d, q)[:8] != 'F2A7590D': continue
        for o2, q2 in fdesign.refs_at(d, d.rec(q)):
            if cls(d, q2)[:8] != '5662F619': continue
            for o3, q3 in fdesign.refs_at(d, d.rec(q2)):
                if cls(d, q3)[:8] != '7ACC2A03': continue
                kind, faces, _ = naming.recipe(sys.modules[__name__], d, q3)
                if kind == 'edge_recipe_data' and faces and len(faces) >= 2:
                    a, b = EN.face(faces[0][0]), EN.face(faces[1][0])
                    if a and b and a['kind'] == 'plane' and b['kind'] == 'plane':
                        L = ED.plane_plane(a, b)
                        if L: u = L[1]
    if not u: return None
    sets = EP.body_edge_sets(CURRENT_FILES)
    for sgn in (1, -1):
        hits = 0; tried = 0
        for q in inputs:
            if cls(d, q) != C['EXTRUDE']: continue
            G = EP.ext_geom(d, q)
            if not G: continue
            for sg in G['segs']:
                if sg['curve'] not in G['sketch']['lines']: continue
                A = EP.world(G['pl'], sg['poly'][0], 0); B = EP.world(G['pl'], sg['poly'][-1], 0)
                sh = EP.mul(u, sgn * spacing)
                tried += 1
                if any(EP.colinear_exists(EP.add(A, sh), EP.add(B, sh), es, 1e-5) or
                       any(EP.colinear_exists(EP.add(EP.add(A, sh), EP.mul(G['pl']['z'], t)), EP.add(EP.add(B, sh), EP.mul(G['pl']['z'], t)), es, 1e-5)
                           for t in ([G['t1']] if G['t1'] else [])) for es in sets):
                    hits += 1
        if tried and hits == tried:
            return [round(x * sgn, 9) for x in u]
    return None

CURRENT_FILES = None
def through_reach(d, fid, e):
    import edgeprobe as EP
    G = EP.ext_geom(d, fid)
    if not G: return None
    pl = G['pl']; z = pl['z']
    sets = EP.body_edge_sets(CURRENT_FILES)
    ts = set()
    for sg in G['segs']:
        if sg['curve'] not in G['sketch']['lines']: continue
        a, b = sg['poly'][0], sg['poly'][-1]
        A = EP.world(pl, a, 0); B = EP.world(pl, b, 0); u = EP.sub(B, A)
        for es in sets:
            for p0, p1, t_ in es:
                v = EP.sub(p1, p0)
                if EP.norm(v) < 1e-9 or abs(abs(EP.dot(v, u)) / (EP.norm(v) * EP.norm(u)) - 1) > 1e-9: continue
                w = EP.sub(p0, A); t = EP.dot(w, z)
                off = EP.sub(w, EP.mul(z, t))      # in-plane offset from the profile segment's line
                perp = EP.sub(off, EP.mul(u, EP.dot(off, u) / EP.dot(u, u)))
                if EP.norm(perp) < 1e-6 and abs(t) > 1e-6: ts.add(round(t, 7))
    pos = [t for t in ts if t > 0]; neg = [t for t in ts if t < 0]
    if pos and not neg: return max(pos)
    if neg and not pos: return min(neg)
    return None


def linear_pattern_chain(name, originals, j, dirs, sym):
    """Rectangular pattern -> chained pattern_linear features (U first; V takes originals + U results).
    A symmetric direction (original in the middle, odd count) becomes two patterns, + and - spacing."""
    nu = int(round(j.get('count') or 1)); nv = int(round(j.get('count_v') or 1))
    jobs = []
    if nu > 1: jobs.append(('U', dirs[0], j.get('spacing') or 0.0, nu, sym[0]))
    if nv > 1:
        dv = dirs[1] if len(dirs) > 1 else (dirs[0] if nu <= 1 else None)
        if dv is None: return None
        jobs.append(('V', dv, j.get('spacing_v') or 0.0, nv, sym[1]))
    if not jobs: return None
    out = []; made = []
    for tag, dr, sp, n, sy in jobs:
        parts = []
        if sy:
            if n % 2 == 0: return None
            parts = [(sp, (n + 1) // 2), (-sp, (n + 1) // 2)]
        else:
            parts = [(sp, n)]
        for k, (sp_, n_) in enumerate(parts):
            nm = name if not out else '%s %s%s' % (name, tag, '' if len(parts) == 1 else str(k + 1))
            if len(parts) > 1 and not out: nm = name
            out.append(dict(type='pattern_linear', name=nm, features=list(originals) + list(made),
                            direction=[round(a, 9) for a in dr['dir']], spacing=round(sp_, 9), count=n_,
                            target=None, pattern_axis=tag, symmetric=bool(sy),
                            note=('direction = the stored F2A7590D vector (sign included)' +
                                  ('; symmetric about the original: split into + and - halves' if sy else ''))))
        made += [o['name'] for o in out[len(out) - len(parts):]]
    return out

def design_json(d):
    feats = []; skipped = []
    sketch_names = {}
    tl, extra = timeline(d)
    used_names = set()
    G = Graph(d)
    import edges as ED
    EN = ED.Naming(sys.modules[__name__], d, CURRENT_FILES)
    names = {}
    comps = {}
    for fid in tl + extra:
        base, name = feature_name(d, fid)
        c = cls(d, fid)
        if name in used_names:   # names restart per component: keep them unique in the tree
            k = 2
            while '%s (%d)' % (name, k) in used_names: k += 1
            name = '%s (%d)' % (name, k)
        used_names.add(name)
        names[fid] = name
        comp = G.component(fid)
        if comp is not None and comp not in comps:
            comps[comp] = dict(name=component_name(d, comp), bodies=component_bodies(d, comp))
        nbefore = len(feats)
        if c[:8] in NON_GEOMETRIC:
            skipped.append(dict(name=name, fusion_type=base or c[:8], reason=NON_GEOMETRIC[c[:8]], source_id=fid))
            continue
        if c == C['SKETCH_FEAT']:
            j, sid = sketch_json(d, fid, name)
            if j: feats.append(j); sketch_names[sid] = name
            else: feats.append(dict(type='sketch', name=name, error='sketch data not found'))
        elif c == C['EXTRUDE']:
            j = extrude_json(d, fid, name, sketch_names, EN)
            links = G.body_links(fid)
            made = [names.get(G.creator.get(l), G.creator.get(l)) for l in links if G.creator.get(l) == fid]
            acted = sorted({names.get(G.creator.get(l), str(G.creator.get(l))) for l in links if G.creator.get(l) not in (None, fid)})
            if j.get('operation') in ('join', 'cut', 'intersect'):
                j['target'] = acted[0] if len(acted) == 1 else (acted or None)
            j['creates_bodies'] = len(made)
            feats.append(j)
        elif c.startswith(C['FILLET']) or c.startswith(C['CHAMFER']):
            fil = c.startswith(C['FILLET'])
            key = 'radius' if fil else 'distance'
            for k_, (v, es, bad) in enumerate(blend_edges(d, fid, EN, ('Radius',) if fil else ('Distance', 'leftDistance'))):
                j = dict(type='fillet' if fil else 'chamfer', name=name if k_ == 0 else '%s set %d' % (name, k_ + 1), source_id=fid)
                j[key] = mm(v) if v is not None else None
                j['edges'] = [dict(midpoint=e['midpoint']) for e in es]
                if bad: j['edges_unresolved'] = bad
                if not es and not bad: j['edges_note'] = 'no edge recipes found'
                feats.append(j)
        elif c.startswith(C['REVOLVE']):
            j = simple_json(d, fid, 'revolve', name, {'angle': ('AlongAngle', lambda v: round(math.degrees(v), 9))})
            sk = profile_sketch(d, fid)
            j['sketch'] = sketch_names.get(sk) if sk else None; j['sketch_source_id'] = sk; j['profiles'] = 'all'
            j['axis'] = None; j['axis_note'] = 'axis is a topological reference (EntityTracking recipe), not decoded'
            feats.append(j)
        elif c[:8] == '1C037A07':
            feats.append(hole_json(d, fid, name))
        elif c[:8] == MIRROR:
            ins = G.inputs(fid)
            j = dict(type='mirror', name=name, source_id=fid, features=[names.get(q, str(q)) for q in ins])
            pl_ = mirror_plane(d, fid)
            if pl_ and ins and all(names.get(q) for q in ins): j['plane'] = pl_
            else:
                j['type'] = 'unsupported'; j['gtree_type'] = 'mirror'; j['fusion_type'] = base
                j['note'] = 'mirror plane is not an origin/construction plane frame (CC54ECAD), or it mirrors bodies'
            feats.append(j)
        elif c[:8] in PATTERNS:
            typ = PATTERNS[c[:8]]
            ins = G.inputs(fid)
            ps = {}
            for o, q in fdesign.refs_at(d, d.rec(fid)):
                if cls(d, q)[:8] in ('D91D429C', 'A085449A'):
                    for o2, q2 in fdesign.refs_at(d, d.rec(q)):
                        if cls(d, q2) == C['PARAM']:
                            pp = fdesign.param(d, q2)
                            if pp: ps[pp['name']] = pp
            base_j = dict(type='unsupported', gtree_type=typ, fusion_type=base, name=name, source_id=fid,
                          features=[names.get(q, str(q)) for q in ins])
            if 'countU' in ps: base_j['count'] = ps['countU']['value']
            if 'CountV' in ps: base_j['count_v'] = ps['CountV']['value']
            if 'uSpaceDistance' in ps: base_j['spacing'] = mm(ps['uSpaceDistance']['value']); base_j['spacing_expr'] = ps['uSpaceDistance']['expr']
            if 'vSpaceDistance' in ps: base_j['spacing_v'] = mm(ps['vSpaceDistance']['value'])
            if 'TotalAngle' in ps: base_j['angle'] = math.degrees(ps['TotalAngle']['value'])
            dirs = pattern_dirs(d, fid) if typ == 'pattern_linear' else []
            if typ == 'pattern_linear' and ins and all(names.get(q) for q in ins) and dirs and 'countU' in ps:
                out_p = linear_pattern_chain(name, [names[q] for q in ins], base_j, dirs, pattern_symmetric(d, fid) or [False] * 3)
                if out_p:
                    for p_ in out_p: p_['source_id'] = fid
                    feats.extend(out_p); continue_ = True
                else: feats.append(base_j)
            else:
                base_j['note'] = 'pattern of bodies/components, circular, or no direction record: not converted'
                if dirs: base_j['direction'] = [round(a, 9) for a in dirs[0]['dir']]
                feats.append(base_j)
        else:
            j = dict(type='unsupported', fusion_type=base or c[:8], name=name, class_guid=c, source_id=fid)
            if c[:8] == 'D7015B81':
                j['creates_bodies'] = sum(1 for l, f in G.creator.items() if f == fid)
            feats.append(j)
        for f_ in feats[nbefore:]:
            if comp is not None: f_['component'] = comps[comp]['name']
            if fid in extra: f_['in_timeline'] = False
    out = dict(format='gusset-feature-tree', version=2, source=dict(app='fusion'), features=feats, skipped=skipped)
    allcomps = [i for i in d.objs if cls(d, i)[:8] == 'E03784ED']
    out['components'] = [dict(name=v['name'], source_id=k, bodies=v['bodies']) for k, v in comps.items()]
    with_bodies = [k for k, v in comps.items() if v['bodies']]
    out['single_component'] = len({f.get('component') for f in feats}) <= 1 and len(with_bodies) <= 1
    out['component_records'] = len(allcomps)
    return out

def main():
    out = sys.argv[1]; os.makedirs(out, exist_ok=True)
    seen = set()
    for p in sys.argv[2:]:
        for k, files in f3dzip.designs(p).items():
            for pre, (m, b) in fdesign.find_streams(files).items():
                if 'Design1' not in pre and 'FusionDesignSegmentType1' not in pre: continue
                if not pre.startswith('FusionAssetName[Active]'): continue
                h = hashlib.md5(b).hexdigest()
                if h in seen: continue
                seen.add(h)
                base = os.path.basename(p) + ('__' + os.path.basename(k) if k else '')
                try:
                    global CURRENT_FILES
                    CURRENT_FILES = files
                    d = fdesign.Design(m, b)
                    j = design_json(d)
                except Exception as ex:
                    j = dict(format='gusset-feature-tree', version=2, source=dict(app='fusion'), features=[],
                             error='%s: %s' % (type(ex).__name__, ex))
                j['source']['file'] = os.path.basename(p); j['source']['design'] = k or None
                with open(os.path.join(out, base + '.json'), 'w') as f: json.dump(j, f, indent=1)
                ft = {}
                for x in j['features']: ft[x['type']] = ft.get(x['type'], 0) + 1
                print('%-60s %s %s' % (base[:60], ft, j.get('error', '')))
    print('profile failures:', regions.WHY)

if __name__ == '__main__':
    main()
