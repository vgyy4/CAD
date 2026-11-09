"""Sketch geometry from a moProfileFeature_c span (pattern-anchored, no object model needed).

Layout (see LAYOUT.md, confirmed on the Slip-Pad parts):
  sgSketch body: u32 nPoints, then nPoints items; then further lists (lines, arcs, ...),
  each 'u32 count' + items.  Item = u16 x0, u32 a, u32 id, entity.
  entity = ENT_HDR(17) u32 X, u16, u16, u16, u16, u16 kind(0 point / 1 segment), f32 -1,
           obj ext (null unless the entity is a reference to outside geometry),
           u16 flags (0x04 normal, 0x0c construction, 0x05 ...), u8, f64 0.0, f64 1.0, payload
  point payload: u16 ptype (0x1e point, 0x1f origin point), f64 x, f64 y (metres), ...
  line payload : u16 start index, u16 end index (0-based into the point list), u16 1, u16 0, f64 -1, u16
  arc payload  : u16 start, u16 end, u16 1, u16 0, f64 -1, i32 dir (+1 ccw / -1 cw), u16 centre index,
                 4 x i32 -2, ...
"""
import struct, re
ENT_HDR = bytes.fromhex('ffff1f0001ffffffffffffffff000080bf')
ENT_HDR3 = bytes.fromhex('ffff1f0003ffffffffffffffff000080bf')
HDR_RE = re.compile(rb'\xff\xff\x1f\x00[\x01\x03].{8}\x00\x00\x80\xbf', re.S)

def find_headers(d, a, b):
    """entity headers: FF FF 1F 00 <ver 1|3> <8 bytes, usually FF*8> 00 00 80 BF"""
    return [m.start() for m in HDR_RE.finditer(d, a, b)]

ONE = bytes.fromhex('0000000000000000000000000000f03f')   # f64 0.0, f64 1.0

def _payload(d, h):
    """offset of the type-specific payload of the entity whose header is at h.
    Header byte 4 is a format version: 1 (2013-2015 files) or 3 (newer files, 8 extra bytes)."""
    extra = 8 if d[h + 4] == 3 else 0
    ext = struct.unpack_from('<H', d, h + 17 + 18)[0]
    if ext == 0:
        p = h + 17 + 23
        if d[p:p + 16] == ONE: return p + 16 + extra, False
    # external reference: an object chain sits in the 'ext' slot; find the 0.0/1.0 pair after it
    k = d.find(ONE, h + 17 + 18, h + 2000)
    if k < 0: return None, True
    return k + 16 + extra, True

def parse_sketch(d, body, end):
    """body: offset of the sgSketch body (after its class tag); end: end of the feature span."""
    npts = struct.unpack_from('<I', d, body)[0]
    hdrs = []
    hdrs = find_headers(d, body, end)
    if npts > len(hdrs): raise ValueError('point count %d > records %d' % (npts, len(hdrs)))
    pts, segs, warn = [], [], []
    for k, h in enumerate(hdrs):
        kind = struct.unpack_from('<H', d, h + 17 + 12)[0]
        flags = struct.unpack_from('<H', d, h + 17 + 20)[0]
        rid = struct.unpack_from('<I', d, h - 4)[0]
        aval = struct.unpack_from('<I', d, h - 8)[0]
        p, ext = _payload(d, h)
        if p is None: warn.append('record %x: no payload' % h); continue
        if k < npts:
            ptype = struct.unpack_from('<H', d, p)[0]
            if kind != 0 or ptype not in (0x1e, 0x1f):
                warn.append('record %x: expected point, kind %d type %x' % (h, kind, ptype)); pts.append(None); continue
            x, y = struct.unpack_from('<dd', d, p + 2)
            A = struct.unpack_from('<H', d, p + 18)[0]
            pts.append(dict(id=rid, x=x, y=y, ext=ext, origin=ptype == 0x1f, flags=flags, A=A))
        else:
            s, e, one = struct.unpack_from('<HHH', d, p)
            m1 = struct.unpack_from('<d', d, p + 8)[0]
            if one not in (0, 1) or m1 != -1.0:
                warn.append('record %x: not a line/arc record (kind %d)' % (h, kind)); continue
            if d[p + 22:p + 38] == b'\xfe\xff\xff\xff' * 4:
                dirn, c = struct.unpack_from('<iH', d, p + 16)
                segs.append(dict(type='arc', id=rid, a=aval, s=s, e=e, c=c, dir=dirn, flags=flags, construction=bool(flags & 9)))
            else:
                segs.append(dict(type='line', id=rid, a=aval, s=s, e=e, flags=flags, construction=bool(flags & 9) or one == 0, centerline=one == 0))
    return pts, segs, warn

def _orthonormal(v):
    if any(not (abs(x) <= 1.0000001) for x in v): return False
    for r in range(3):
        if abs(v[3*r]**2 + v[3*r+1]**2 + v[3*r+2]**2 - 1) > 1e-9: return False
    dot = lambda a, b: sum(v[3*a+k] * v[3*b+k] for k in range(3))
    return abs(dot(0, 1)) < 1e-9 and abs(dot(0, 2)) < 1e-9 and abs(dot(1, 2)) < 1e-9

def _frame(R, t):
    """Stored transform: v_sketch = R^T v_model + t  (R rows as stored, its columns are the
    sketch axes in model space).  Returns origin (model), x axis, y axis, normal."""
    col = lambda j: [R[3*i+j] for i in range(3)]
    x, y, z = col(0), col(1), col(2)
    o = [-(x[i]*t[0] + y[i]*t[1] + z[i]*t[2]) for i in range(3)]
    return o, x, y, z

IDENT = [1.0, 0, 0, 0, 1.0, 0, 0, 0, 1.0]

def find_plane(d, a, b, unitcomp_idx):
    """Sketch plane inside a sketch span [a,b).
    1) moCompRefPlane_c: <unitcomp obj> 02 00 + 10x00, 31 bytes, u32 plane-feature-id, u32 stamp,
       u16 0, u32 k, u8 hasrot, [9 f64 rotation], 3 f64 t, f64 scale(1.0)
    2) plane data (moDefaultRefPlnData_c/moFaceRefPlnData_c/...): 3 f64 origin, 3 f64 normal,
       u8 hasrot, [9 f64], 3 f64 t, f64 1.0"""
    tags = [b'moUnitComponent_c\x02\x00' + bytes(10)]
    if unitcomp_idx is not None: tags.append(struct.pack('<H', 0x8000 | unitcomp_idx) + b'\x02\x00' + bytes(10))
    hits = []
    for tag in tags:
        i = a
        while True:
            i = d.find(tag, i, b)
            if i < 0: break
            hits.append(i + len(tag) - 12); i += 1
    cands = []
    for i0 in sorted(hits):
        i = i0 - 2
        p = i + 14 + 31
        cid, ts = struct.unpack_from('<II', d, p); p += 8
        if d[p:p+2] == b'\x00\x00':
            k = struct.unpack_from('<I', d, p + 2)[0]; hr = d[p + 6]; q = p + 7
            if hr in (0, 1) and k < 16:
                R = list(struct.unpack_from('<9d', d, q)) if hr else IDENT
                if hr: q += 72
                t = struct.unpack_from('<3d', d, q); sc = struct.unpack_from('<d', d, q + 24)[0]
                if sc == 1.0 and _orthonormal(R) and all(abs(x) < 10 for x in t):
                    cands.append((i, 'compRefPlane', cid, R, t))
    # plane data blocks
    for i in range(a, b - 160):
        hr = d[i + 48]
        if hr not in (0, 1): continue
        try:
            nrm = struct.unpack_from('<3d', d, i + 24)
        except struct.error:
            break
        if not all(abs(x) <= 1.0000001 for x in nrm) or abs(sum(x*x for x in nrm) - 1) > 1e-9: continue
        org = struct.unpack_from('<3d', d, i)
        if not all(abs(x) < 10 for x in org): continue
        R = list(struct.unpack_from('<9d', d, i + 49)) if hr else IDENT
        q = i + 49 + (72 if hr else 0)
        if not _orthonormal(R): continue
        if any(abs(R[3*r+2] - nrm[r]) > 1e-9 for r in range(3)): continue   # column 2 == normal
        t = struct.unpack_from('<3d', d, q); sc = struct.unpack_from('<d', d, q + 24)[0]
        if sc != 1.0 or not all(abs(x) < 10 for x in t): continue
        cands.append((i, 'planeData', None, R, t))
    if not cands: return None
    cands.sort(key=lambda c: c[0])
    i, kind, cid, R, t = cands[0]
    if kind == 'planeData':
        # A sketch on a face: the first block is the face's plane data; the sketch's own frame
        # (same normal, axes possibly turned) follows as '... 01 00 00 00 <12 x 00> 01 <R> <t> 1.0'.
        # Confirmed on FrameBracket Sketch11 (box) - the face block's axes are turned 180 deg there.
        j = i + 49
        while True:
            j = d.find(b'\x01\x00\x00\x00' + bytes(12) + b'\x01', j, b)
            if j < 0: break
            q = j + 17
            R2 = list(struct.unpack_from('<9d', d, q))
            if _orthonormal(R2) and all(abs(R2[3*r+2] - R[3*r+2]) < 1e-9 for r in range(3)):
                t2 = struct.unpack_from('<3d', d, q + 72); sc = struct.unpack_from('<d', d, q + 96)[0]
                if sc == 1.0:
                    R, t, kind = R2, t2, 'faceSketch'
                    break
            j += 1
    o, x, y, z = _frame(R, t)
    return dict(at=i, kind=kind, plane_feature_id=cid, R=R, t=list(t), origin=o, x_axis=x, y_axis=y, normal=z)
