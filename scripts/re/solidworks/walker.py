"""Clean-room walker for the SolidWorks MFC-CArchive-style object stream
(Config-0-ResolvedFeatures).  Derived only from sample bytes.

Framing (confirmed on the samples):
  tag u16 == 0xFFFF : new class: u16 schema, u16 namelen, ascii name; then the object body.
                      The class gets the next map index, the object the one after.
  tag & 0x8000      : new object of an already declared class (index = tag & 0x7FFF);
                      0x7FFF + u32 is the big form (not seen yet).
  tag == 0          : null pointer.
  other tag         : back-reference to an earlier object (map index).
  The map is shared with the Config-0 stream: ResolvedFeatures continues where Config-0
  left off, so its first index is (number of map entries in Config-0)+1.
CString: u8 len (0xFF -> u16, 0xFFFF -> u32); 0xFF 0xFFFE prefix = UTF-16LE, length follows again.
"""
import struct, sys, os

class Fail(Exception):
    pass

class Obj:
    __slots__ = ('cls', 'idx', 'pos', 'end', 'f')
    def __init__(self, cls, idx, pos):
        self.cls = cls; self.idx = idx; self.pos = pos; self.end = None; self.f = {}
    def __getitem__(self, k): return self.f[k]
    def get(self, k, dflt=None): return self.f.get(k, dflt)
    def __repr__(self): return '<%s#%d@%x>' % (self.cls, self.idx, self.pos)

class Archive:
    def __init__(self, data, base=0, classes=None, objects=None, readers=None, lenient=False):
        self.d = data; self.p = 0
        self.next = base + 1; self.base = base
        self.cls = dict(classes or {})       # idx -> class name
        self.objs = dict(objects or {})      # idx -> Obj
        self.readers = readers if readers is not None else READERS
        self.stack = []
        self.all = []                        # every object, in stream order
        self.lenient = lenient
        self.unknown = {}
    # primitives
    def need(self, n):
        if self.p + n > len(self.d): raise Fail('eof at %x need %d' % (self.p, n))
    def u8(self):
        self.need(1); v = self.d[self.p]; self.p += 1; return v
    def u16(self):
        self.need(2); v = struct.unpack_from('<H', self.d, self.p)[0]; self.p += 2; return v
    def i16(self):
        self.need(2); v = struct.unpack_from('<h', self.d, self.p)[0]; self.p += 2; return v
    def u32(self):
        self.need(4); v = struct.unpack_from('<I', self.d, self.p)[0]; self.p += 4; return v
    def i32(self):
        self.need(4); v = struct.unpack_from('<i', self.d, self.p)[0]; self.p += 4; return v
    def f32(self):
        self.need(4); v = struct.unpack_from('<f', self.d, self.p)[0]; self.p += 4; return v
    def f64(self):
        self.need(8); v = struct.unpack_from('<d', self.d, self.p)[0]; self.p += 8; return v
    def f64s(self, n): return [self.f64() for _ in range(n)]
    def raw(self, n):
        self.need(n); v = self.d[self.p:self.p + n]; self.p += n; return v
    def peek16(self, off=0):
        return struct.unpack_from('<H', self.d, self.p + off)[0] if self.p + off + 2 <= len(self.d) else None
    def expect(self, bs, what=''):
        if isinstance(bs, str): bs = bytes.fromhex(bs.replace(' ', ''))
        got = self.raw(len(bs))
        if got != bs:
            self.p -= len(bs)
            raise Fail('expected %s %s at %x, got %s' % (what, bs.hex(), self.p, got.hex()))
    def zeros(self, n):
        b = self.raw(n)
        if any(b):
            self.p -= n; raise Fail('expected %d zero bytes at %x: %s' % (n, self.p, b.hex()))
    def cstr(self):
        n = self.u8(); uni = False
        if n == 0xFF:
            n = self.u16()
            if n == 0xFFFE:
                uni = True; n = self.u8()
                if n == 0xFF:
                    n = self.u16()
                    if n == 0xFFFF: n = self.u32()
            elif n == 0xFFFF:
                n = self.u32()
        if uni: return self.raw(2 * n).decode('utf-16le', 'replace')
        return self.raw(n).decode('latin-1')
    # objects
    def obj(self, want=None):
        pos = self.p
        tag = self.u16()
        if tag == 0: return None
        if tag == 0xFFFF:
            schema = self.u16(); ln = self.u16(); name = self.raw(ln).decode('latin-1')
            if not name.replace('_', 'a').isalnum(): raise Fail('bad class name at %x' % pos)
            self.cls[self.next] = name; self.next += 1
        elif tag & 0x8000:
            ci = tag & 0x7FFF
            if ci == 0x7FFF: ci = self.u32() & 0x7FFFFFFF
            if ci not in self.cls: raise Fail('unknown class index %d (0x%x) at %x' % (ci, ci, pos))
            name = self.cls[ci]
        else:
            oi = tag
            if oi == 0x7FFF: oi = self.u32()
            if oi not in self.objs:
                if oi <= self.base:   # object that lives in the Config-0 part of the archive
                    return ('config0', oi)
                raise Fail('bad back-reference %d (0x%x) at %x' % (oi, oi, pos))
            return self.objs[oi]
        o = Obj(name, self.next, pos); self.objs[self.next] = o; self.next += 1
        if want and name not in want:
            raise Fail('expected %s, got %s at %x' % (want, name, pos))
        rd = self.readers.get(name)
        if rd is None:
            raise Fail('no reader for class %s at %x' % (name, pos))
        self.stack.append(o)
        rd(self, o)
        self.stack.pop()
        o.end = self.p
        self.all.append(o)
        return o
    def objlist(self):
        """MFC-style collection: u16 count then objects."""
        n = self.u16()
        return [self.obj() for _ in range(n)]
    def objlist32(self):
        n = self.u32()
        if n > 100000: raise Fail('implausible count %d at %x' % (n, self.p - 4))
        return [self.obj() for _ in range(n)]

READERS = {}
def reader(*names):
    def deco(fn):
        for n in names: READERS[n] = fn
        return fn
    return deco

def hexdump(d, a, n=96):
    out = []
    for i in range(a, min(a + n, len(d)), 16):
        c = d[i:i + 16]
        out.append('%06x %-48s %s' % (i, ' '.join('%02x' % x for x in c), ''.join(chr(x) if 32 <= x < 127 else '.' for x in c)))
    return '\n'.join(out)
