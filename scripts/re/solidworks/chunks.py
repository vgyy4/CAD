import sys, struct, zlib, re, os, glob
MARK = b'\x14\x00\x06\x00\x08\x00'
def swap(b): return bytes(((x & 0x0f) << 4) | (x >> 4) for x in b)
def streams(data):
    out = {}; i = 0
    while True:
        i = data.find(MARK, i)
        if i < 0: break
        h = data[i+6:i+6+20]
        if len(h) < 20: break
        _k, crc, dsz, isz, nl = struct.unpack('<5I', h)
        if nl > 1024 or dsz > len(data): i += 1; continue
        name = swap(data[i+26:i+26+nl]).decode('latin1', 'replace')
        raw = data[i+26+nl:i+26+nl+dsz]
        try:
            inf = zlib.decompress(raw, -15)
            if len(inf) == isz and zlib.crc32(inf) == crc: out[name] = inf
        except Exception: pass
        i += 1
    return out
