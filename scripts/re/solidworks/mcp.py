"""Import a .gtree.json through `gusset mcp` and print the import result (incl. skipped reasons).
python3 -I mcp.py file.gtree.json"""
import json, subprocess, sys, os, shutil
HERE = os.path.dirname(os.path.abspath(__file__))
def import_result(path):
    w = os.path.join(HERE, 'mcpw'); os.makedirs(w, exist_ok=True)
    dst = os.path.join(w, 'x.gtree.json'); shutil.copy(path, dst)
    shutil.copy(os.path.join(w, 'p.gus'), os.path.join(w, 'q.gus'))
    p = subprocess.Popen([os.path.join(HERE, 'bin/gusset'), 'mcp', os.path.join(w, 'q.gus'), '--no-attach'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True, cwd=w)
    msgs = [dict(jsonrpc='2.0', id=1, method='initialize', params=dict(protocolVersion='2024-11-05', capabilities={}, clientInfo=dict(name='t', version='0'))),
            dict(jsonrpc='2.0', method='notifications/initialized'),
            dict(jsonrpc='2.0', id=3, method='tools/call', params=dict(name='run', arguments=dict(name='import.file', args=dict(path='x.gtree.json'))))]
    out, err = p.communicate('\n'.join(json.dumps(m) for m in msgs) + '\n', timeout=600)
    for l in out.splitlines():
        try: j = json.loads(l)
        except Exception: continue
        if j.get('id') == 3: return j
    return dict(error=err[-800:])
if __name__ == '__main__':
    j = import_result(sys.argv[1])
    s = json.dumps(j)
    i = s.find('skipped')
    print(s[:300]); print(s[i - 50:i + 2500] if i >= 0 else 'no skipped field')
