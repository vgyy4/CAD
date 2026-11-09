import subprocess, json, sys, os
HERE = os.path.dirname(os.path.abspath(__file__))
G = os.path.join(HERE, '..', 'bin', 'gusset')
def session(calls, model=os.path.join(HERE, 'truth/empty.gus'), timeout=300):
    p = subprocess.Popen([G, 'mcp', model, '--no-attach'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    def rpc(i, m, params=None):
        p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': i, 'method': m, 'params': params or {}}) + '\n'); p.stdin.flush()
        while True:
            l = p.stdout.readline()
            if not l: return None
            j = json.loads(l)
            if j.get('id') == i: return j
    rpc(1, 'initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 't', 'version': '0'}})
    p.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n'); p.stdin.flush()
    out = [rpc(10 + i, 'tools/call', c) for i, c in enumerate(calls)]
    p.kill(); return out
if __name__ == '__main__':
    if sys.argv[1] == 'list':
        p = subprocess.Popen([G, 'mcp', os.path.join(HERE, 'truth/empty.gus'), '--no-attach'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        for i, (m, pr) in enumerate([('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 't', 'version': '0'}}), ('tools/list', {})]):
            p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': i + 1, 'method': m, 'params': pr}) + '\n'); p.stdin.flush()
            r = json.loads(p.stdout.readline())
        print([t['name'] for t in r['result']['tools']][:80]); p.kill()

import shutil, tempfile
def imp(gtree):
    d = tempfile.mkdtemp(dir=HERE + '/truth'); shutil.copy(gtree, d + '/t.gtree.json'); shutil.copy(HERE + '/truth/empty.gus', d + '/m.gus')
    p = subprocess.Popen([G, 'mcp', d + '/m.gus', '--no-attach'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    def rpc(i, m, params=None):
        p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': i, 'method': m, 'params': params or {}}) + '\n'); p.stdin.flush()
        while True:
            l = p.stdout.readline()
            if not l: return None
            j = json.loads(l)
            if j.get('id') == i: return j
    rpc(1, 'initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 't', 'version': '0'}})
    p.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n'); p.stdin.flush()
    r = rpc(2, 'tools/call', {'name': 'run', 'arguments': {'name': 'import.file', 'args': {'path': 't.gtree.json'}}})
    p.kill(); shutil.rmtree(d, ignore_errors=True)
    t = r['result']['content'][0]['text']
    try: return json.loads(t)
    except Exception: return t
