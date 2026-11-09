"""Per-class readers (layouts inferred from sample bytes; see LAYOUT.md)."""
from walker import reader, Fail, READERS

def read_top(a):
    n = a.u16()
    top = [a.obj() for _ in range(n)]
    if a.p != len(a.d):
        raise Fail('trailing %d bytes at %x' % (len(a.d) - a.p, a.p))
    return top

@reader('moNodeName_c')
def r_nodename(a, o):
    o.f['name'] = a.cstr()
    o.f['u'] = a.u32()

def feature_base(a, o):
    """Common head of every tree node (folders, features, ref geometry, sketches)."""
    nn = a.obj({'moNodeName_c'})
    o.f['name'] = nn['name'] if nn is not None else None
    o.f['flags'] = a.u32()
    o.f['id'] = a.u32()
    o.f['head'] = a.raw(59)
    o.f['comment'] = a.cstr()
    o.f['mid'] = a.raw(46)
    o.f['ts'] = (a.u32(), a.u32())
    o.f['ver'] = a.u16()

@reader('moCommentsFolder_c', 'moSensorFolder_c', 'moDocsFolder_c')
def r_folder(a, o):
    feature_base(a, o)
    o.f['flag2'] = a.u16()
    o.f['items'] = a.objlist()

@reader('moFavoriteFolder_c', 'moHistoryFolder_c')
def r_folder2(a, o):
    feature_base(a, o)
    o.f['flag2'] = a.u16()
    o.f['x'] = a.u16()
    o.f['items'] = a.objlist()

@reader('moHistoryFeatItemData_c')
def r_hfid(a, o):
    o.f['a'] = a.raw(18)
    o.f['comp'] = a.obj()

@reader('moUnitComponent_c')
def r_unitcomp(a, o):
    o.f['ver'] = a.u16()
    o.f['raw'] = a.raw(10)

@reader('moCompFeature_c')
def r_compfeature(a, o):
    o.f['unit'] = a.obj({'moUnitComponent_c'})
    o.f['raw'] = a.raw(31)
    o.f['fid'] = a.u32(); o.f['ts'] = a.u32()

@reader('moDetailCabinet_c')
def r_cabinet(a, o):
    feature_base(a, o)
    o.f['flag2'] = a.u16()
    o.f['items'] = a.objlist()
    o.f['u'] = a.u16()
    o.f['scales'] = (a.f64(), a.f64())
    o.f['v'] = a.u16()

@reader('moSurfaceBodyFolder_c', 'moSolidBodyFolder_c')
def r_bodyfolder(a, o):
    feature_base(a, o)
    o.f['flag2'] = a.u16()
    o.f['raw'] = a.raw(12)
    o.f['ref'] = a.obj()

@reader('moEqnFolder_c')
def r_eqnfolder(a, o):
    feature_base(a, o)
    o.f['flag2'] = a.u16()
    o.f['items'] = a.objlist()

@reader('moMaterialFolder_c')
def r_matfolder(a, o):
    feature_base(a, o)
    o.f['flag2'] = a.u16()
    o.f['u'] = a.u16()
    o.f['s'] = a.cstr()
    o.f['v'] = a.u32()

@reader('moRefPlane_c')
def r_refplane(a, o):
    feature_base(a, o)
    o.f['raw'] = a.raw(116)
    o.f['label'] = (a.f64(), a.f64(), a.f64())
    o.f['u'] = a.u32()
    o.f['raw2'] = a.raw(6)
    o.f['data'] = a.obj()

def plane_data(a, o):
    o.f['origin'] = a.f64s(3)
    o.f['normal'] = a.f64s(3)
    o.f['hasrot'] = a.u8()
    o.f['rot'] = a.f64s(9) if o.f['hasrot'] else None
    o.f['trans'] = a.f64s(3)
    o.f['scale'] = a.f64()
    o.f['b0'] = a.u8()
    o.f['extent'] = a.f64s(4)
    o.f['tail'] = a.raw(114 - 1 - 32 - 8 - 24)

@reader('moDefaultRefPlnData_c')
def r_defplane(a, o):
    plane_data(a, o)

# ---------------------------------------------------------------- components
def comp_base(a, o):
    o.f['unit'] = a.obj({'moUnitComponent_c'})
    o.f['craw'] = a.raw(31)
    o.f['cid'] = a.u32(); o.f['cts'] = a.u32()

@reader('moCompSketchEntHandle_c')
def r_compskent(a, o):
    comp_base(a, o)
    o.f['handle'] = a.obj()

@reader('sgPointHandle', 'sgLineHandle', 'sgArcHandle')
def r_handle(a, o):
    o.f['ent'] = a.u16()
    o.f['m1'] = a.i32()
    o.f['z'] = a.u32()

@reader('sgExtEnt_c')
def r_extent(a, o):
    o.f['ref'] = a.obj()

@reader('moSketchExtRef_w')
def r_skextref(a, o):
    o.f['comp'] = a.obj()
    o.f['raw'] = a.raw(60)
    o.f['refname'] = a.cstr()

# ---------------------------------------------------------------- sketches
ENT_HDR = bytes.fromhex('ffff1f0001ffffffffffffffff000080bf')

def entity_common(a, e):
    a.expect(ENT_HDR, 'entity header')
    e['X'] = a.u32()
    e['h'] = (a.u16(), a.u16(), a.u16(), a.u16())
    e['kind'] = a.u16()
    a.f32()
    e['ext'] = a.obj()
    e['flags'] = a.u16()
    a.u8()
    e['w'] = (a.f64(), a.f64())

def sk_point(a, e):
    e['ptype'] = a.u16()
    e['x'] = a.f64(); e['y'] = a.f64()
    e['A'] = a.u16(); e['n'] = a.u16()
    e['h1'] = a.obj(); e['h2'] = a.obj()
    e['B'] = a.u16()
    a.expect('feffffff', 'point trailer')
    e['tail'] = a.raw(34)
    e['last'] = a.u16()

def sk_line(a, e):
    e['s'] = a.u16(); e['e'] = a.u16(); e['one'] = a.u16()
    e['m1'] = a.f64()
    e['tail'] = a.u16()

def sk_arc(a, e):
    e['s'] = a.u16(); e['e'] = a.u16(); e['one'] = a.u16()
    e['m1'] = a.f64()
    e['dir'] = a.i32(); e['c'] = a.u16()
    e['m2'] = [a.i32() for _ in range(4)]
    e['tail'] = a.u16()

def sk_list(a, fn, kind):
    n = a.u32()
    if n > 10000: raise Fail('bad sketch list count %d at %x' % (n, a.p - 4))
    out = []
    for i in range(n):
        e = {'pos': a.p}
        e['x0'] = a.u16(); e['a'] = a.u32(); e['id'] = a.u32()
        entity_common(a, e)
        fn(a, e)
        e['kind_name'] = kind
        out.append(e)
    return out

@reader('sgSketch')
def r_sgsketch(a, o):
    o.f['points'] = sk_list(a, sk_point, 'point')
    o.f['lines'] = sk_list(a, sk_line, 'line')
    o.f['l2'] = a.u32()
    if o.f['l2']: raise Fail('unknown sketch list 2 non-empty at %x' % a.p)
    o.f['arcs'] = sk_list(a, sk_arc, 'arc')
    o.f['rest_at'] = a.p
    raise Fail('sgSketch: lists done at %x' % a.p)

@reader('moOriginProfileFeature_c', 'moProfileFeature_c')
def r_profile(a, o):
    feature_base(a, o)
    o.f['raw'] = a.raw(14)
    o.f['sketch'] = a.obj({'sgSketch'})
