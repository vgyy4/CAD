#!/usr/bin/env python3
"""List a design's timeline (name, class) and dump records of chosen classes.

    python3 -I tl.py sample.f3z [CLASS8 ...]      (no class: timeline of every design in the file)
With classes: for each feature of that class, print its record's refs (id:class8), doubles and u32s.
Clean-room helper: bounded, never executes sample data.
"""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import f3dzip, fdesign, f3dfeat
from f3dfeat import cls, feature_name, timeline

def dump(d, q, depth=0):
    rec = d.rec(q); out = []
    refs = fdesign.refs_at(d, rec)
    rs = {o: i for o, i in refs}
    j = 0
    s = []
    while j < len(rec):
        if j in rs:
            s.append('[%d:%s]' % (rs[j], cls(d, rs[j])[:8])); j = fdesign.ref_end(d, rec, j)[1]; continue
        j += 1
    return refs

def main():
    p = sys.argv[1]; want = sys.argv[2:]
    for k, files in f3dzip.designs(p).items():
        for pre, (m, b) in fdesign.find_streams(files).items():
            if not pre.startswith('FusionAssetName[Active]') or 'Design1' not in pre: continue
            d = fdesign.Design(m, b)
            tl, extra = timeline(d)
            print('==', k, pre[:40], len(tl), 'features')
            for q in tl + extra:
                c = cls(d, q)[:8]
                nm = feature_name(d, q)
                if not want:
                    print('%6d %s %s%s' % (q, c, nm, ' (extra)' if q in extra else ''))
                elif c in want:
                    rec = d.rec(q)
                    print('--', q, c, nm, len(rec), 'bytes')
                    refs = fdesign.refs_at(d, rec)
                    print('  refs:', ' '.join('%d@%d:%s' % (i, o, cls(d, i)[:8]) for o, i in refs))
                    print('  hex:', rec.hex())

def walk(d, q, depth=0, seen=None, maxd=3):
    """Print the record tree below q: class, parameters, refs (bounded)."""
    seen = seen if seen is not None else set()
    pad = '  ' * depth
    c = cls(d, q)[:8]
    if q in seen: print(pad + '%d:%s (seen)' % (q, c)); return
    seen.add(q)
    rec = d.rec(q)
    extra = ''
    if c == '7A6A3D31':
        pp = fdesign.param(d, q)
        extra = ' PARAM %r' % ({k: pp[k] for k in ('expr', 'name', 'unit', 'pname', 'value') if k in pp} if pp else None)
    print(pad + '%d:%s %dB%s' % (q, c, len(rec), extra))
    if depth >= maxd: return
    for o, i in fdesign.refs_at(d, rec):
        if cls(d, i)[:8] in ('3A6D1E62', '6CCF41D5', '5FBE538E', 'B7F34D7B'): continue
        walk(d, i, depth + 1, seen, maxd)

def tree(p, ids, maxd):
    for k, files in f3dzip.designs(p).items():
        for pre, (m, b) in fdesign.find_streams(files).items():
            if not pre.startswith('FusionAssetName[Active]') or 'Design1' not in pre: continue
            d = fdesign.Design(m, b)
            for q in ids:
                if q in d.objs:
                    print('##', k[:8], q); walk(d, q, 0, None, maxd)

def skeleton(p, cls8):
    """For each feature of class cls8: the record with refs shown as [id:class] and the bytes between."""
    for k, files in f3dzip.designs(p).items():
        for pre, (m, b) in fdesign.find_streams(files).items():
            if not pre.startswith('FusionAssetName[Active]') or 'Design1' not in pre: continue
            d = fdesign.Design(m, b)
            tl, extra = timeline(d)
            for q in tl + extra:
                if cls(d, q)[:8] != cls8: continue
                rec = d.rec(q); out = []; j = 0
                refs = {o: i for o, i in fdesign.refs_at(d, rec)}
                while j < len(rec):
                    if j in refs:
                        out.append('[%d:%s]' % (refs[j], cls(d, refs[j])[:8])); j = fdesign.ref_end(d, rec, j)[1]
                    else:
                        out.append('%02x' % rec[j]); j += 1
                print(k[:8], q, feature_name(d, q)[1], ' '.join(out))

if sys.argv[2:3] == ['skel']:
    skeleton(sys.argv[1], sys.argv[3])
elif sys.argv[2:3] == ['tree']:
    tree(sys.argv[1], [int(x) for x in sys.argv[4:]], int(sys.argv[3]))
else:
    main()
