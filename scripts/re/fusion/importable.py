"""Per design: can the whole tree be rebuilt? (every feature fully decoded)"""
import json,glob,sys,collections
def blockers(f):
    t=f['type']
    if t=='sketch': return [] if f.get('plane') and not f.get('error') else ['sketch']
    if t=='extrude':
        b=[]
        if f.get('error'): b.append('extrude layout')
        if not isinstance(f.get('profiles'),list): b.append('extrude profiles')
        if f.get('distance') is None: b.append('extrude extent not distance')
        if f.get('two_sided'): b.append('two-sided')
        if f.get('taper_deg'): b.append('taper')
        if f.get('operation') in ('join','cut','intersect') and not isinstance(f.get('target'),str): b.append('extrude target')
        return b
    if t in ('fillet','chamfer'):
        b=[]
        if not f.get('edges') or f.get('edges_unresolved'): b.append(t+' edges')
        if f.get('radius' if t=='fillet' else 'distance') is None: b.append(t+' value')
        return b
    if t in ('hole','mirror'): return []
    if t=='pattern_linear': return [] if f.get('direction') and f.get('features') else ['pattern']
    if t=='revolve': return ['revolve axis']
    return ['unsupported '+(f.get('gtree_type') or f.get('fusion_type') or '?')]
C=collections.Counter(); rows=[]
for fn in sorted(glob.glob(sys.argv[1]+'/*.json')):
    j=json.load(open(fn))
    if not j['features']: continue
    bl=collections.Counter(b for f in j['features'] for b in blockers(f))
    unsup=sum(1 for f in j['features'] if f['type']=='unsupported')
    sc=j.get('single_component')
    C['designs with features']+=1; C['single-component']+=bool(sc)
    if sc and unsup==0: C['single-component & 0 unsupported']+=1
    if sc and not bl: C['single-component & fully rebuildable']+=1
    if not bl: C['fully rebuildable (any)']+=1; print('REBUILDABLE', fn.split('/')[-1])
    rows.append((fn.split('/')[-1][:55], 'single' if sc else 'multi', len(j['features']), unsup, dict(bl)))
for r in rows: print(*r)
print(dict(C))
