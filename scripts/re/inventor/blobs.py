"""ShapeManager blobs of a part dump: {'result': (stream, bytes), 'history': ...}.
A blob is preceded by its record type (u32 at -8 before the magic): 0x031d result, 0x0357 history."""
import os, struct
def part_blobs(D):
    out = {}
    for f in sorted(os.listdir(D)):
        if not (f.startswith('B') and f.endswith('.bin')): continue
        data = open(os.path.join(D, f), 'rb').read()
        i = 0
        while True:
            k = data.find(b'ASM BinaryFile', i)
            if k < 0: break
            i = k + 1
            if k < 8: continue
            t = struct.unpack_from('<I', data, k - 8)[0]
            key = {0x031d: 'result', 0x0357: 'history'}.get(t)
            if key and key not in out: out[key] = (f, data[k:])
    return out
