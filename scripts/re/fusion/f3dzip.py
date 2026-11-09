"""In-memory reader for .f3d/.f3z ZIPs (deflate, stored, zstd method 93 via libzstd ctypes)."""
import zipfile, io, struct, zlib, ctypes, ctypes.util
_z = None
def _zstd(data, size):
    global _z
    if _z is None:
        _z = ctypes.CDLL('libzstd.so.1')
        _z.ZSTD_decompress.restype = ctypes.c_size_t
        _z.ZSTD_isError.restype = ctypes.c_uint
    if size > 512 << 20: raise ValueError('too big')
    buf = ctypes.create_string_buffer(size or 1)
    n = _z.ZSTD_decompress(buf, ctypes.c_size_t(size), data, ctypes.c_size_t(len(data)))
    if _z.ZSTD_isError(ctypes.c_size_t(n)): raise ValueError('zstd error')
    return buf.raw[:n]
def _read(z, info):
    if info.compress_type in (0, 8):
        return z.read(info)
    if info.compress_type == 93:
        f = z.fp; f.seek(info.header_offset)
        h = f.read(30)
        nl, el = struct.unpack('<HH', h[26:30])
        f.seek(info.header_offset + 30 + nl + el)
        return _zstd(f.read(info.compress_size), info.file_size)
    raise ValueError('compression %d' % info.compress_type)
def entries(data, prefix='', depth=0):
    """Yield (path, bytes) for every file; nested .f3d are expanded (path 'outer.f3d/inner')."""
    z = zipfile.ZipFile(io.BytesIO(data))
    for i in z.infolist():
        if i.filename.endswith('/'): continue
        b = _read(z, i)
        if i.filename.lower().endswith('.f3d') and depth < 2:
            yield from entries(b, prefix + i.filename + '/', depth + 1)
        else:
            yield prefix + i.filename, b
def designs(path):
    """Yield (design_path, {relname: bytes}) per design (.f3d) in a file."""
    groups = {}
    for n, b in entries(open(path, 'rb').read()):
        k = n.rsplit('.f3d/', 1)[0] + '.f3d' if '.f3d/' in n else ''
        rel = n.rsplit('.f3d/', 1)[1] if '.f3d/' in n else n
        groups.setdefault(k, {})[rel] = b
    return groups
