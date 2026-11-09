#!/usr/bin/env python3
"""Clean-room prototype: Fusion 360 design timeline (sketches + features) -> Gusset feature-tree JSON.

    python3 -I f3dfeat.py OUTDIR sample.f3d|.f3z ...

Reads only the ZIP entries Design1/{MetaStream,BulkStream}.dat (and FusionDesignSegmentType1/ if
framed the same way). See LAYOUT.md for the byte layout this relies on. Samples are untrusted
data: everything is bounded, nothing is executed.
"""
import sys, os, json, math, struct, hashlib, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import f3dzip, fdesign

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
    m = re.compile(rb'([\x01-\x05])\x00\x00\x00([\x01-\x04])\x00\x00\x00([\x00-\x03])\x00\x00\x00[\x00\x01]\x01', re.S).search(rec, b, b + 64)
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

def extrude_json(d, fid, name, sketch_names):
    e = extrude_raw(d, fid)
    j = dict(type='extrude', name=name, source_id=fid)
    if not e: j['error'] = 'layout not recognised'; return j
    j['sketch'] = sketch_names.get(e['sketch']) if e['sketch'] else None
    j['sketch_source_id'] = e['sketch']
    j['profiles'] = 'all'
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

def design_json(d):
    feats = []
    sketch_names = {}
    tl, extra = timeline(d)
    used_names = set()
    for fid in tl + extra:
        base, name = feature_name(d, fid)
        c = cls(d, fid)
        if name in used_names:   # names restart per component: keep them unique in the tree
            k = 2
            while '%s (%d)' % (name, k) in used_names: k += 1
            name = '%s (%d)' % (name, k)
        used_names.add(name)
        if c == C['SKETCH_FEAT']:
            j, sid = sketch_json(d, fid, name)
            if j: feats.append(j); sketch_names[sid] = name
            else: feats.append(dict(type='sketch', name=name, error='sketch data not found'))
        elif c == C['EXTRUDE']:
            feats.append(extrude_json(d, fid, name, sketch_names))
        elif c.startswith(C['FILLET']):
            j = simple_json(d, fid, 'fillet', name, {'radius': ('Radius', mm)})
            j['edges'] = []; j['edges_note'] = 'edge references not decoded (EntityTracking records)'
            feats.append(j)
        elif c.startswith(C['CHAMFER']):
            feats.append(simple_json(d, fid, 'chamfer', name, {'distance': ('Distance', mm)}))
        elif c.startswith(C['REVOLVE']):
            j = simple_json(d, fid, 'revolve', name, {'angle': ('AlongAngle', lambda v: round(math.degrees(v), 9))})
            sk = profile_sketch(d, fid)
            j['sketch'] = sketch_names.get(sk) if sk else None; j['sketch_source_id'] = sk; j['profiles'] = 'all'
            j['axis'] = None; j['axis_note'] = 'axis is a topological reference (EntityTracking recipe), not decoded'
            feats.append(j)
        else:
            feats.append(dict(type='unsupported', fusion_type=base or c[:8], name=name, class_guid=c, source_id=fid))
        if fid in extra: feats[-1]['in_timeline'] = False
    return dict(format='gusset-feature-tree', version=2, source=dict(app='fusion'), features=feats)

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

if __name__ == '__main__':
    main()
