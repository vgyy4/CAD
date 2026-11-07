#!/usr/bin/env python3
"""Export an Onshape Part Studio's feature list to a Gusset `.gtree.json` (ADR 0042, v1).

UNVERIFIED - not yet run against Onshape. Read-only: only GET requests plus one POST to
the FeatureScript evaluator (which does not modify the document).

Usage:  python gusset_export.py <document URL> -o out.gtree.json
Keys:   ONSHAPE_ACCESS_KEY / ONSHAPE_SECRET_KEY environment variables (never printed).
"""
import argparse
import base64
import hashlib
import hmac
import json
import math
import os
import re
import secrets
import string
import sys
import urllib.error
import urllib.request
from email.utils import formatdate

API_VERSION = "v6"
EXPORTER = "plugins/onshape 0.1.0"

# ---------------------------------------------------------------- signing


def make_nonce(n=25):
    alphabet = string.ascii_letters + string.digits
    return "".join(secrets.choice(alphabet) for _ in range(n))


def http_date():
    return formatdate(timeval=None, localtime=False, usegmt=True)


def sign_request(method, path, query, nonce, date, content_type, access_key, secret_key):
    """Onshape API-key auth: HMAC-SHA256 over
    (method \\n nonce \\n date \\n content-type \\n path \\n query \\n), lowercased,
    keyed with the secret key, base64. `query` is the raw query string without '?'."""
    msg = ("%s\n%s\n%s\n%s\n%s\n%s\n" % (method, nonce, date, content_type, path, query)).lower()
    digest = hmac.new(secret_key.encode("utf-8"), msg.encode("utf-8"), hashlib.sha256).digest()
    signature = base64.b64encode(digest).decode("ascii")
    return {
        "On-Nonce": nonce,
        "Date": date,
        "Content-Type": content_type,
        "Authorization": "On %s:HmacSHA256:%s" % (access_key, signature),
        "Accept": "application/json;charset=UTF-8; qs=0.09",
    }


class Client:
    def __init__(self, base, access_key, secret_key):
        self.base = base.rstrip("/")
        self._ak = access_key
        self._sk = secret_key

    def request(self, method, path, query="", body=None):
        ctype = "application/json"
        headers = sign_request(method, path, query, make_nonce(), http_date(), ctype, self._ak, self._sk)
        url = self.base + path + ("?" + query if query else "")
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            # status only: never echo headers (they carry the signature)
            raise RuntimeError("HTTP %d for %s %s" % (e.code, method, path))

    def get(self, path, query=""):
        return self.request("GET", path, query)

    def post(self, path, body, query=""):
        return self.request("POST", path, query, body)


# ---------------------------------------------------------------- URL


def parse_document_url(url):
    m = re.match(r"^(https?://[^/]+)/documents/([0-9a-f]+)/([wvm])/([0-9a-f]+)/e/([0-9a-f]+)", url.strip())
    if not m:
        raise ValueError("not an Onshape document URL: expected .../documents/<did>/<w|v|m>/<id>/e/<eid>")
    return {"base": m.group(1), "did": m.group(2), "wvm": m.group(3), "wvmid": m.group(4), "eid": m.group(5)}


def ps_path(ref, suffix):
    return "/api/%s/partstudios/d/%s/%s/%s/e/%s/%s" % (
        API_VERSION, ref["did"], ref["wvm"], ref["wvmid"], ref["eid"], suffix)


# ---------------------------------------------------------------- units

_LEN = {"mm": 1.0, "millimeter": 1.0, "cm": 10.0, "centimeter": 10.0, "m": 1000.0, "meter": 1000.0,
        "in": 25.4, "inch": 25.4, "ft": 304.8, "foot": 304.8, "yd": 914.4, "yard": 914.4}
_ANG = {"deg": 1.0, "degree": 1.0, "rad": 180.0 / math.pi, "radian": 180.0 / math.pi}
_NUM = r"([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)"


def _parse_expr(expr):
    m = re.match(r"^\s*" + _NUM + r"\s*([A-Za-z]*)\s*$", expr or "")
    return (float(m.group(1)), m.group(2).lower()) if m else None


def quantity_to(q, kind):
    """BTMParameterQuantity message -> mm or degrees. Literal expressions with a unit are
    trusted; anything else (formulas, #variables) uses `value`, which is SI (m / rad)."""
    if q is None:
        return None
    table = _ANG if kind == "angle" else _LEN
    p = _parse_expr(q.get("expression"))
    if p and p[1] in table:
        return p[0] * table[p[1]]
    v = q.get("value")
    if v is None:
        return None
    return float(v) * (180.0 / math.pi if kind == "angle" else 1000.0)


def variable_ref(q):
    e = (q or {}).get("expression") or ""
    m = re.match(r"^\s*#([A-Za-z_][A-Za-z0-9_]*)\s*$", e)
    return m.group(1) if m else None


# ---------------------------------------------------------------- feature helpers


def params_of(msg):
    out = {}
    for p in msg.get("parameters", []):
        pm = p.get("message", {})
        if "parameterId" in pm:
            out[pm["parameterId"]] = pm
    return out


def query_ids(pm):
    """All deterministic ids in a BTMParameterQueryList message."""
    ids = []
    for q in (pm or {}).get("queries", []):
        ids.extend(q.get("message", {}).get("geometryIds", []) or [])
        ids.extend(q.get("message", {}).get("deterministicIds", []) or [])
    return ids


def query_feature_ids(pm):
    out = []
    for q in (pm or {}).get("queries", []):
        fid = q.get("message", {}).get("featureId")
        if fid:
            out.append(fid)
    return out


def sub(a, b):
    return [x - y for x, y in zip(a, b)]


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


# ---------------------------------------------------------------- sketches


def plane_from_info(info):
    """Frame from the sketch info `transform` (4x4 sketch->world, row-major, metres; also
    accepts {"matrix": [...]}). Returns (origin_mm, x_axis, y_axis)."""
    t = info.get("transform")
    if isinstance(t, dict):
        t = t.get("matrix") or t.get("m")
    if not t or len(t) < 12:
        raise ValueError("sketch info has no transform")
    origin = [t[3] * 1000.0, t[7] * 1000.0, t[11] * 1000.0]
    xa = [t[0], t[4], t[8]]
    ya = [t[1], t[5], t[9]]
    return origin, xa, ya


class PointTable:
    def __init__(self, origin, xa, ya):
        self.o, self.xa, self.ya = origin, xa, ya
        self.points, self._idx = [], {}

    def add(self, world_mm, construction=False):
        d = sub(world_mm, self.o)
        u, v = dot(d, self.xa), dot(d, self.ya)
        key = (round(u, 4), round(v, 4))
        if key not in self._idx:
            pid = "p%d" % (len(self.points) + 1)
            self._idx[key] = pid
            self.points.append({"id": pid, "x": round(u, 6), "y": round(v, 6), "construction": bool(construction)})
        return self._idx[key]


def _mm(p):
    return [c * 1000.0 for c in p]


def convert_sketch(name, info):
    origin, xa, ya = plane_from_info(info)
    pt = PointTable(origin, xa, ya)
    curves, line_ids = [], {}
    for n, e in enumerate(info.get("geomEntities", []), 1):
        kind = e.get("entityType")
        g = e.get("geometry", {})
        cons = bool(e.get("isConstruction", False))
        cid = "c%d" % n
        if kind == "line":
            curves.append({"id": cid, "type": "line", "start": pt.add(_mm(e["startPoint"]), cons),
                           "end": pt.add(_mm(e["endPoint"]), cons), "construction": cons})
            line_ids[e.get("entityId")] = cid
        elif kind == "circle":
            c = _mm([g["xCenter"], g["yCenter"], g["zCenter"]])
            curves.append({"id": cid, "type": "circle", "center": pt.add(c, cons),
                           "radius": g["radius"] * 1000.0, "construction": cons})
        elif kind == "arc":
            c = _mm([g["xCenter"], g["yCenter"], g["zCenter"]])
            s, t = e["startPoint"], e["endPoint"]
            if g.get("clockwise"):  # ADR: arcs run counter-clockwise start -> end
                s, t = t, s
            curves.append({"id": cid, "type": "arc", "center": pt.add(c, cons), "start": pt.add(_mm(s), cons),
                           "end": pt.add(_mm(t), cons), "construction": cons})
    skipped = [str(e.get("entityType")) for e in info.get("geomEntities", [])
               if e.get("entityType") not in ("line", "circle", "arc")]
    sk = {"type": "sketch", "name": name,
          "plane": {"origin": [round(c, 6) for c in origin], "x_axis": xa, "y_axis": ya},
          "points": pt.points, "curves": curves, "constraints": []}
    if skipped:
        sk["note"] = "skipped sketch entities: " + ", ".join(sorted(set(skipped)))
    return sk, line_ids


# ---------------------------------------------------------------- FeatureScript

FS_SCRIPT = """function(context is Context, queries is map)
{
    var out = "";
    for (var e in evaluateQuery(context, qEntityFilter(queries.ents, EntityType.EDGE)))
    {
        var l = evEdgeTangentLine(context, { "edge" : e, "parameter" : 0.5 });
        out = out ~ toString(l.origin[0] / meter) ~ "," ~ toString(l.origin[1] / meter) ~ "," ~ toString(l.origin[2] / meter)
            ~ "," ~ toString(l.direction[0]) ~ "," ~ toString(l.direction[1]) ~ "," ~ toString(l.direction[2]) ~ ";";
    }
    return out;
}
"""


def _unwrap_fs(res):
    r = res.get("result", res)
    while isinstance(r, dict) and "message" in r and "value" in r["message"]:
        r = r["message"]["value"]
    return r


def parse_fs_edges(text):
    """'ox,oy,oz,dx,dy,dz;...' (metres) -> list of (midpoint_mm, direction)."""
    out = []
    for rec in (text or "").split(";"):
        if not rec.strip():
            continue
        v = [float(x) for x in rec.split(",")]
        out.append((_mm(v[:3]), v[3:6]))
    return out


def resolve_edges(client, ref, ids):
    if not ids or client is None:
        return []
    body = {"script": FS_SCRIPT, "queries": [{"key": "ents", "value": [{"deterministicIds": ids}]}]}
    res = client.post(ps_path(ref, "featurescript"), body)
    return parse_fs_edges(_unwrap_fs(res))


# ---------------------------------------------------------------- features

OPS = {"NEW": "new", "ADD": "join", "REMOVE": "cut", "INTERSECT": "intersect"}


def unsupported(name, original, note=None):
    d = {"type": "unsupported", "name": name, "original_type": original}
    if note:
        d["note"] = note
    return d


def convert_extrude_like(kind, name, pr, sketch_names, lines_by_sketch, client, ref):
    fid = next((f for f in query_feature_ids(pr.get("entities")) if f in sketch_names), None)
    if fid is None:
        return unsupported(name, kind, "profile is not from a sketch of this studio")
    op = OPS.get(pr.get("operationType", {}).get("value", "NEW"))
    if op is None:
        return unsupported(name, kind, "operation type")
    f = {"type": kind, "name": name, "sketch": sketch_names[fid], "profiles": "all", "operation": op}
    if kind == "extrude":
        end = pr.get("endBound", {}).get("value", "BLIND")
        if end not in ("BLIND", "SYMMETRIC"):
            return unsupported(name, "extrude", "end condition " + str(end))
        q = pr.get("depth")
        d = quantity_to(q, "length")
        if d is None:
            return unsupported(name, "extrude", "no depth")
        if pr.get("oppositeDirection", {}).get("value"):
            d = -d
        f["distance"], f["symmetric"] = d, end == "SYMMETRIC"
        var = variable_ref(q)
        if var:
            f["distance_parameter"] = var
        return f
    rtype = pr.get("revolveType", {}).get("value", "FULL")
    if rtype not in ("FULL", "ONE_DIRECTION"):
        return unsupported(name, "revolve", "revolve type " + str(rtype))
    ang = 360.0 if rtype == "FULL" else quantity_to(pr.get("angle"), "angle")
    if ang is None:
        return unsupported(name, "revolve", "no angle")
    f["angle"], f["symmetric"] = ang, False
    ids = query_ids(pr.get("axis"))
    lines = lines_by_sketch.get(fid, {})
    hit = next((lines[i.split(".")[-1]] for i in ids if i.split(".")[-1] in lines), None)
    if hit:
        f["axis"] = {"line": hit}
    else:
        edges = resolve_edges(client, ref, ids)
        if len(edges) != 1:
            return unsupported(name, "revolve", "axis could not be resolved")
        f["axis"] = {"origin": edges[0][0], "direction": edges[0][1]}
    return f


def convert_edge_feature(kind, name, pr, client, ref):
    if kind == "chamfer" and pr.get("chamferType", {}).get("value", "EQUAL_OFFSETS") != "EQUAL_OFFSETS":
        return unsupported(name, kind, "only equal-distance chamfers are exported")
    edges = resolve_edges(client, ref, query_ids(pr.get("entities")))
    if not edges:
        return unsupported(name, kind, "edges could not be resolved to geometry")
    f = {"type": kind, "name": name, "edges": [{"midpoint": [round(c, 6) for c in m]} for m, _ in edges]}
    if kind == "fillet":
        r = quantity_to(pr.get("radius"), "length")
        if r is None:
            return unsupported(name, kind, "no radius")
        f["radius"] = r
    else:
        d = quantity_to(pr.get("width") or pr.get("distance"), "length")
        if d is None:
            return unsupported(name, kind, "no distance")
        f["distance"] = d
    return f


def convert_variable(msg):
    pr = params_of(msg)
    nm = pr.get("name", {}).get("value")
    q = pr.get("value")
    if not nm or q is None:
        return None
    units = (q.get("units") or "").lower()
    expr = q.get("expression") or ""
    p = _parse_expr(expr)
    unit = p[1] if p else units
    if unit in _ANG or "rad" in units or "deg" in units:
        kind = "angle"
    elif unit in _LEN or "meter" in units or "inch" in units:
        kind = "length"
    else:
        kind = "number"
    val = quantity_to(q, kind) if kind != "number" else (p[0] if p else q.get("value"))
    return {"name": nm, "expression": expr, "value": val, "kind": kind}


def convert(features_json, sketch_infos, client=None, ref=None, document="",
            app_version="Onshape REST " + API_VERSION, bodies=None):
    """Conversion of API responses to the tree (client is only used for edge/axis resolution)."""
    sketch_info = {s.get("featureId"): s for s in sketch_infos.get("sketches", [])}
    out = {"format": "gusset-feature-tree", "version": 1,
           "source": {"app": "onshape", "app_version": app_version, "exporter": EXPORTER, "document": document},
           "parameters": [], "features": [], "bodies": bodies or []}
    sketch_names, lines_by_sketch = {}, {}
    for feat in features_json.get("features", []):
        msg = feat.get("message", {})
        ftype, name = msg.get("featureType", feat.get("typeName", "?")), msg.get("name", "?")
        if msg.get("suppressed"):
            continue
        try:
            if ftype == "assignVariable":
                v = convert_variable(msg)
                if v:
                    out["parameters"].append(v)
                continue
            pr = params_of(msg)
            if ftype == "newSketch":
                info = sketch_info.get(msg.get("featureId"))
                if info is None:
                    raise ValueError("no sketch info")
                sk, lines = convert_sketch(name, info)
                sketch_names[msg["featureId"]] = name
                lines_by_sketch[msg["featureId"]] = lines
                out["features"].append(sk)
            elif ftype in ("extrude", "revolve"):
                out["features"].append(
                    convert_extrude_like(ftype, name, pr, sketch_names, lines_by_sketch, client, ref))
            elif ftype in ("fillet", "chamfer"):
                out["features"].append(convert_edge_feature(ftype, name, pr, client, ref))
            else:
                out["features"].append(unsupported(name, ftype))
        except Exception as e:  # per-feature isolation
            out["features"].append(unsupported(name, ftype, "export failed: %s" % type(e).__name__))
    return out


def collect_bodies(client, ref):
    bodies = []
    try:
        parts = client.get("/api/%s/parts/d/%s/%s/%s/e/%s" % (
            API_VERSION, ref["did"], ref["wvm"], ref["wvmid"], ref["eid"]))
        mp = client.get(ps_path(ref, "massproperties")).get("bodies", {})
        bb = client.get(ps_path(ref, "boundingboxes"))
        box = [[bb["lowX"] * 1000, bb["lowY"] * 1000, bb["lowZ"] * 1000],
               [bb["highX"] * 1000, bb["highY"] * 1000, bb["highZ"] * 1000]]
        for p in parts:
            props = mp.get(p.get("partId"), {})
            vol = (props.get("volume") or [None])[0]
            b = {"name": p.get("name", p.get("partId", "Part"))}
            if vol is not None:
                b["volume_mm3"] = vol * 1e9
            if len(parts) == 1:  # the API only gives the studio-wide box
                b["bbox_mm"] = box
            bodies.append(b)
    except Exception:
        pass
    return bodies


def export(url, access_key, secret_key):
    ref = parse_document_url(url)
    client = Client(ref["base"], access_key, secret_key)
    try:
        doc = client.get("/api/%s/documents/%s" % (API_VERSION, ref["did"])).get("name", "")
    except Exception:
        doc = ""
    feats = client.get(ps_path(ref, "features"))
    try:
        sketches = client.get(ps_path(ref, "sketches"))
    except Exception:
        sketches = {"sketches": []}
    return convert(feats, sketches, client, ref, document=doc, bodies=collect_bodies(client, ref))


def main(argv=None):
    ap = argparse.ArgumentParser(description="Export an Onshape Part Studio to .gtree.json")
    ap.add_argument("url")
    ap.add_argument("-o", "--output", required=True)
    a = ap.parse_args(argv)
    ak, sk = os.environ.get("ONSHAPE_ACCESS_KEY"), os.environ.get("ONSHAPE_SECRET_KEY")
    if not ak or not sk:
        print("set ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY", file=sys.stderr)
        return 2
    try:
        tree = export(a.url, ak, sk)
    except Exception as e:
        print("export failed: %s" % e, file=sys.stderr)
        return 1
    with open(a.output, "w", encoding="utf-8") as f:
        json.dump(tree, f, indent=2)
    n = sum(1 for x in tree["features"] if x["type"] == "unsupported")
    print("wrote %s: %d features (%d unsupported)" % (a.output, len(tree["features"]), n))
    return 0


if __name__ == "__main__":
    sys.exit(main())
