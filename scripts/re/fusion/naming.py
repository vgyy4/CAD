"""Topological naming records: operation ids, generator lists, face/edge recipes."""
import struct, fdesign

def op_map(F, d):
    """5D89B935 -> {op id: feature id}. Two layouts: u32 n, n x (u64 op, u64 feature) (older), or
    u32 n, n x (u64 feature, u32 k, k x u64 op) (newer)."""
    out = {}
    for i in d.objs:
        if not F.cls(d, i).startswith('5D89B935'): continue
        r = d.rec(i)
        for j in range(15, min(len(r) - 4, 60)):
            n = struct.unpack_from('<I', r, j)[0]
            if not 0 < n < 100000: continue
            try:   # newer
                p = j + 4; res = {}; known = 0
                for _ in range(n):
                    f, k = struct.unpack_from('<QI', r, p); p += 12
                    if k > 10000 or f > 1 << 40: raise ValueError
                    ops = struct.unpack_from('<%dQ' % k, r, p); p += 8 * k
                    if any(o > 1 << 32 for o in ops): raise ValueError
                    if f in d.objs:
                        known += 1
                        for o in ops: res[o] = f
                if res and known * 2 >= n: out.update(res); break
            except (struct.error, ValueError):
                pass
            if j + 4 + 16 * n <= len(r):
                pairs = [struct.unpack_from('<QQ', r, j + 4 + 16 * k) for k in range(n)]
                if all(f in d.objs for o, f in pairs) and all(o < 1 << 32 for o, f in pairs):
                    for o, f in pairs: out[o] = f
                    break
    return out

def gen_lists(F, d):
    """ABA79BE7 (one per body): runs of {u32 n, n x str generator (decimal text), u64 op id}
    -> {op: [generators]} (found by scanning; the framing around the runs differs by version)."""
    out = {}
    for i in d.objs:
        if not F.cls(d, i).startswith('ABA79BE7'): continue
        r = d.rec(i); j = 15
        while j + 8 < len(r):
            try:
                n = struct.unpack_from('<I', r, j)[0]
                if not 0 < n < 10000: raise ValueError
                p = j + 4; gens = []
                for k in range(n):
                    g, p = _gen(r, p); gens.append(g)
                op = struct.unpack_from('<Q', r, p)[0]
                if op == 0 or op > 1 << 31: raise ValueError
                out.setdefault(op, [])
                for g in gens:
                    if g not in out[op]: out[op].append(g)
                j = p + 8; continue
            except (struct.error, ValueError, UnicodeDecodeError):
                j += 1
    return out

def _gen(r, p):
    L = struct.unpack_from('<I', r, p)[0]
    if not 0 < L < 12: raise ValueError
    g = r[p + 4:p + 4 + L].decode('ascii')
    if not g.lstrip('-').isdigit(): raise ValueError
    return g, p + 4 + L

def recipe(F, d, i):
    """7ACC2A03: (kind, faces). Header u32 1, u32 3, u32 nfaces; face = u32 k, k entries;
    entry = str generator (decimal text, may be negative), u32 0, u32 n, n x i32 operation ids
    (creator first, then operations that modified the face), u32 0."""
    r = d.rec(i)
    kind = None
    for name in (b'bounded_face_recipe_data', b'edge_recipe_data', b'face_recipe_data', b'body_recipe_data', b'vertex_recipe_data'):
        if name in r: kind = name.decode(); break
    for start in range(15, 40):
        try:
            p = start
            a, b, nf = struct.unpack_from('<III', r, p); p += 12
            if a != 1 or not 0 < nf < 1000: continue
            faces = []
            for _ in range(nf):
                k = struct.unpack_from('<I', r, p)[0]; p += 4
                if not 0 < k < 100: raise ValueError
                ents = []
                for _ in range(k):
                    g, p = _gen(r, p)
                    z = struct.unpack_from('<I', r, p)[0]; p += 4
                    if z > 100000: raise ValueError
                    n = struct.unpack_from('<I', r, p)[0]; p += 4
                    if n > 1000: raise ValueError
                    ops = list(struct.unpack_from('<%di' % n, r, p)); p += 4 * n
                    m = struct.unpack_from('<I', r, p)[0]; p += 4
                    if m != 0: raise ValueError
                    ents.append((g, ops))
                faces.append(ents)
            return kind, faces, b
        except (struct.error, ValueError, UnicodeDecodeError, IndexError):
            continue
    return kind, None, None
