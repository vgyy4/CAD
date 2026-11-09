"""Ground truth: Gusset's import of each .ipt (result body): volume and STL box.
Usage: python3 -I truth.py OUTDIR part.ipt..."""
import sys,os,subprocess,re,json
sys.path.insert(0,os.path.dirname(os.path.abspath(__file__)))
from stlbox import stl
HERE=os.path.dirname(os.path.abspath(__file__)); G='/home/user/CAD/target/debug/gusset'
out=sys.argv[1]; os.makedirs(out,exist_ok=True); res={}
jp=os.path.join(out,'truth.json')
if os.path.exists(jp): res=json.load(open(jp))
for fn in sys.argv[2:]:
    name=os.path.basename(fn).rsplit('.',1)[0]
    if name in res: continue
    s=os.path.join(out,name+'.stl')
    p=subprocess.run([G,'build',os.path.join(HERE,'truth/empty.gus.txt'),'--import',fn,'--export',s],capture_output=True,text=True,timeout=900)
    o=p.stdout+p.stderr
    vols=[float(x) for x in re.findall(r'body .*?: volume (-?[\d.]+)',o)]
    r={'volume':sum(vols) if vols else None}
    if os.path.exists(s):
        lo,hi,v=stl(s); r.update(min=lo,max=hi,stl_volume=v); os.remove(s)
    res[name]=r; print(name,r.get('volume'),[round(x,3) for x in r.get('min',[])],[round(x,3) for x in r.get('max',[])],flush=True)
    json.dump(res,open(jp,'w'),indent=1)
