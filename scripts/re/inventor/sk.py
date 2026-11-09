def backrefs(d):
    br = [[] for _ in range(d.n)]
    for i in range(d.n):
        for k, x in d.refs(i): br[x].append(i)
    return br
