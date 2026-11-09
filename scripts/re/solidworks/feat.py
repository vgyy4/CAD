"""Pattern-anchored extraction of extrude / revolve / fillet parameters inside a feature span."""
import re, struct, collections
PARAM_TAIL = re.compile(rb'\x00\x00\x00\x00(.\x00\x00[\x40\xc0])\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00', re.S)

def params(d, a, b, nn_tag):
    """Dimension parameters (moLengthParameter_c / moAngleParameter_c ...) in [a,b):
    nodename tag, CString name, u32 0, u32 flags, u32 -1, u32 0, u16 0, f64 value."""
    out = []
    pat = re.escape(nn_tag) + rb'\xff\xfe\xff(.)'
    for m in re.finditer(pat, d[a:b], re.S):
        p = a + m.end(); n = m.group(1)[0]
        name = d[p:p + 2*n].decode('utf-16le', 'replace'); q = p + 2*n
        mm = PARAM_TAIL.match(d, q)
        if not mm: continue
        v = struct.unpack_from('<d', d, mm.end())[0]
        cls_tag = struct.unpack_from('<H', d, a + m.start() - 2)[0]
        out.append(dict(pos=a + m.start(), name=name, value=v, cls_tag=cls_tag))
    return out

def endspec_candidates(d, a, b):
    """EndSpec-like objects: class-ref tag, u16 0, u32 X(0/1), 10 x 00, u32 type, u32 flag, ..."""
    out = []
    for m in re.finditer(rb'(..)\x00\x00([\x00\x01])\x00\x00\x00' + bytes(10), d[a:b], re.S):
        t = struct.unpack('<H', m.group(1))[0]
        p = a + m.start()
        if t == 0xffff or not t & 0x8000: continue
        typ, flag = struct.unpack_from('<II', d, p + 18)
        if typ > 64 or flag > 1: continue
        out.append(dict(pos=p, cls=t & 0x7fff, X=m.group(2)[0], type=typ, flag=flag))
    return out

def endspec_decl(d, a, b, name=b'moEndSpec_c'):
    i = d.find(b'\xff\xff\x01\x00' + bytes([len(name), 0]) + name, a, b)
    if i < 0: return None
    p = i + 6 + len(name) - 2     # align like a tag (2 bytes before the body)
    X = struct.unpack_from('<I', d, p + 4)[0]
    typ, flag = struct.unpack_from('<II', d, p + 18)
    return dict(pos=i, cls=None, X=X, type=typ, flag=flag, decl=True)
