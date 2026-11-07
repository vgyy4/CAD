"""Offline tests: request signing and JSON conversion from hand-written API responses.

Run from this directory:  python3 -m unittest
The canned responses follow the documented response shapes of the Onshape REST API v6;
they are NOT recordings of a real server (the exporter is UNVERIFIED).
"""
import base64
import hashlib
import hmac
import json
import os
import unittest

import gusset_export as ge

HERE = os.path.dirname(os.path.abspath(__file__))
IDENTITY = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
# sketch on the Front plane (XZ): x -> world X, y -> world Z
FRONT = [1, 0, 0, 0, 0, 0, -1, 0, 0, 1, 0, 0, 0, 0, 0, 1]


def line(eid, a, b, cons=False):
    return {"entityId": eid, "entityType": "line", "isConstruction": cons,
            "geometry": {"type": "line"}, "startPoint": a, "endPoint": b}


def quantity(pid, value, expr, units=""):
    return {"typeName": "BTMParameterQuantity", "message": {
        "parameterId": pid, "value": value, "expression": expr, "units": units, "isInteger": False}}


def enum(pid, v):
    return {"typeName": "BTMParameterEnum", "message": {"parameterId": pid, "value": v}}


def qlist(pid, **msg):
    return {"typeName": "BTMParameterQueryList", "message": {
        "parameterId": pid, "queries": [{"typeName": "BTMIndividualQuery-138", "message": msg}]}}


def feature(ftype, fid, name, params, **extra):
    m = {"featureType": ftype, "featureId": fid, "name": name, "suppressed": False, "parameters": params}
    m.update(extra)
    return {"typeName": "BTMFeature-134", "message": m}


FEATURES = {"features": [
    feature("assignVariable", "Fv", "Variable 1",
            [{"typeName": "BTMParameterString", "message": {"parameterId": "name", "value": "width"}},
             quantity("value", 0.04, "40 mm", "meter")]),
    feature("newSketch", "FS1", "Sketch 1", []),
    feature("extrude", "FE1", "Extrude 1", [
        enum("operationType", "NEW"), enum("endBound", "BLIND"),
        qlist("entities", featureId="FS1", deterministicIds=["JHR"]),
        quantity("depth", 0.01, "10 mm", "meter")]),
    feature("fillet", "FF1", "Fillet 1", [
        qlist("entities", deterministicIds=["JKD"]), quantity("radius", 0.002, "2 mm", "meter")]),
    feature("newSketch", "FS2", "Sketch 2", []),
    feature("revolve", "FR1", "Revolve 1", [
        enum("operationType", "ADD"), enum("revolveType", "FULL"),
        qlist("entities", featureId="FS2", deterministicIds=["JIR"]),
        qlist("axis", featureId="FS2", deterministicIds=["AX1"])]),
    feature("newSketch", "FS3", "Sketch 3", []),
    feature("hole", "FH1", "Hole 1", [
        enum("style", "SIMPLE"), enum("endStyle", "THROUGH"),
        qlist("locations", featureId="FS3", deterministicIds=["PT1"]),
        quantity("holeDiameter", 0.004, "4 mm", "meter")]),
    feature("linearPattern", "FP1", "Linear pattern 1", [
        enum("patternType", "FEATURE"),
        qlist("entities", featureId="FH1"), qlist("directionQuery", deterministicIds=["DIR"]),
        quantity("distance", 0.01, "10 mm", "meter"), quantity("instanceCount", 2, "2")]),
    feature("loft", "FL1", "Loft 1", []),
    feature("chamfer", "FC1", "Suppressed chamfer", [], suppressed=True),
]}

# sketch on the plane z = 10 mm holding one point at (10, 10)
TOP = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0.01, 0, 0, 0, 1]

SKETCHES = {"sketches": [
    {"featureId": "FS3", "transform": {"matrix": TOP}, "geomEntities": [
        {"entityId": "PT1", "entityType": "point", "isConstruction": False, "point": [0.01, 0.01, 0.01]}]},
    {"featureId": "FS1", "transform": {"matrix": IDENTITY}, "geomEntities": [
        line("L1", [0, 0, 0], [0.04, 0, 0]), line("L2", [0.04, 0, 0], [0.04, 0.02, 0]),
        line("L3", [0.04, 0.02, 0], [0, 0.02, 0]), line("L4", [0, 0.02, 0], [0, 0, 0])]},
    {"featureId": "FS2", "transform": {"matrix": FRONT}, "geomEntities": [
        line("AX1", [0, 0, 0], [0, 0, 0.02], True),
        line("R1", [0.005, 0, 0], [0.015, 0, 0]), line("R2", [0.015, 0, 0], [0.015, 0, 0.005]),
        line("R3", [0.015, 0, 0.005], [0.005, 0, 0.005]), line("R4", [0.005, 0, 0.005], [0.005, 0, 0])]},
]}


class FakeClient:
    def __init__(self):
        self.posts = []

    def post(self, path, body, query=""):
        self.posts.append((path, body))
        ids = body["queries"][0]["value"][0]["deterministicIds"]
        if ids == ["DIR"]:
            return {"result": {"message": {"value": "0.0,0.0,0.01,1,0,0;"}}}
        if ids == ["TOP"]:  # a face: point, outward normal
            return {"result": {"message": {"value": "0.02,0.01,0.01,0,0,1;"}}}
        if ids == ["YZ"]:
            return {"result": {"message": {"value": "0,0.01,0.005,1,0,0;"}}}
        return {"result": {"typeName": "BTFSValueString", "message": {"value": "0.04,0.02,0.005,0,0,1;"}}}


REF = {"did": "d1", "wvm": "w", "wvmid": "w1", "eid": "e1"}
BODIES = [{"name": "Part 1", "volume_mm3": 7740.0886}, {"name": "Part 2", "volume_mm3": 2356.194}]


class SigningTest(unittest.TestCase):
    def test_signature_structure(self):
        h = ge.sign_request("GET", "/api/v6/documents/abc", "a=B&c=d", "NoNcE123", "Tue, 03 Nov 2026 10:00:00 GMT",
                            "application/json", "ACCESS", "SECRET")
        msg = "get\nnonce123\ntue, 03 nov 2026 10:00:00 gmt\napplication/json\n/api/v6/documents/abc\na=b&c=d\n"
        sig = base64.b64encode(hmac.new(b"SECRET", msg.encode(), hashlib.sha256).digest()).decode()
        self.assertEqual(h["Authorization"], "On ACCESS:HmacSHA256:" + sig)
        self.assertEqual(h["On-Nonce"], "NoNcE123")  # header keeps case, only the signed text is lowered
        self.assertEqual(h["Date"], "Tue, 03 Nov 2026 10:00:00 GMT")

    def test_nonce_and_date(self):
        self.assertEqual(len(ge.make_nonce()), 25)
        self.assertNotEqual(ge.make_nonce(), ge.make_nonce())
        self.assertTrue(ge.http_date().endswith("GMT"))

    def test_url(self):
        r = ge.parse_document_url("https://cad.onshape.com/documents/0123abcd/w/4567ef/e/89ab01?x=1")
        self.assertEqual((r["did"], r["wvm"], r["wvmid"], r["eid"]), ("0123abcd", "w", "4567ef", "89ab01"))
        with self.assertRaises(ValueError):
            ge.parse_document_url("https://example.com/")


class ConvertTest(unittest.TestCase):
    def setUp(self):
        self.client = FakeClient()
        self.tree = ge.convert(FEATURES, SKETCHES, self.client, REF, document="Bracket", bodies=BODIES)

    def test_units(self):
        self.assertAlmostEqual(ge.quantity_to({"value": 0.01, "expression": "x+1"}, "length"), 10.0)
        self.assertAlmostEqual(ge.quantity_to({"value": 0.0254, "expression": "1 in"}, "length"), 25.4)
        self.assertAlmostEqual(ge.quantity_to({"value": 3.141592653589793, "expression": "pi rad"}, "angle"), 180.0)
        self.assertEqual(ge.variable_ref({"expression": "#width"}), "width")

    def test_parameters_and_header(self):
        self.assertEqual(self.tree["format"], "gusset-feature-tree")
        self.assertEqual(self.tree["source"]["app"], "onshape")
        self.assertEqual(self.tree["parameters"],
                         [{"name": "width", "expression": "40 mm", "value": 40.0, "kind": "length"}])

    def test_sketch_and_extrude(self):
        sk, ex = self.tree["features"][0], self.tree["features"][1]
        self.assertEqual(sk["type"], "sketch")
        self.assertEqual(len(sk["points"]), 4)
        self.assertEqual(len(sk["curves"]), 4)
        self.assertEqual([p["x"] for p in sk["points"]], [0, 40, 40, 0])
        self.assertEqual([p["y"] for p in sk["points"]], [0, 0, 20, 20])
        self.assertEqual(ex["distance"], 10.0)
        self.assertFalse(ex["symmetric"])
        self.assertEqual(ex["operation"], "new")
        self.assertEqual(ex["sketch"], "Sketch 1")

    def test_fillet_uses_featurescript(self):
        f = self.tree["features"][2]
        self.assertEqual(f["type"], "fillet")
        self.assertEqual(f["radius"], 2.0)
        self.assertEqual(f["edges"], [{"midpoint": [40.0, 20.0, 5.0]}])
        path, body = self.client.posts[0]
        self.assertTrue(path.endswith("/e/e1/featurescript"))
        self.assertEqual(body["queries"][0]["value"][0]["deterministicIds"], ["JKD"])

    def test_revolve_axis_line_and_plane(self):
        sk, rev = self.tree["features"][3], self.tree["features"][4]
        self.assertEqual(sk["plane"]["y_axis"], [0, 0, 1])
        self.assertEqual(rev["type"], "revolve")
        self.assertEqual(rev["angle"], 360.0)
        self.assertEqual(rev["operation"], "join")
        self.assertEqual(rev["axis"], {"line": "c1"})
        self.assertTrue(sk["curves"][0]["construction"])
        # in-plane coordinates of the rectangle
        self.assertEqual([(p["x"], p["y"]) for p in sk["points"]][2:5], [(5, 0), (15, 0), (15, 5)])

    def test_unsupported_and_suppressed(self):
        last = self.tree["features"][-1]
        self.assertEqual(last, {"type": "unsupported", "name": "Loft 1", "original_type": "loft"})
        self.assertEqual(len(self.tree["features"]), 9)  # suppressed chamfer left out

    def test_hole_and_pattern(self):
        names = {f["name"]: f for f in self.tree["features"]}
        self.assertEqual(self.tree["version"], 2)
        h = names["Hole 1"]
        self.assertEqual((h["type"], h["variant"], h["through"], h["diameter"]), ("hole", "simple", True, 4.0))
        self.assertEqual(h["face"], {"point": [10.0, 10.0, 10.0], "normal": [0, 0, 1]})
        self.assertEqual(h["positions"], [[10.0, 10.0, 10.0]])
        p = names["Linear pattern 1"]
        self.assertEqual((p["type"], p["features"], p["count"], p["spacing"]), ("pattern_linear", ["Hole 1"], 2, 10.0))
        self.assertEqual(p["direction"], [1.0, 0.0, 0.0])

    def test_mirror_shell_and_circular_pattern(self):
        feats = {"features": [
            feature("extrude", "FE1", "Extrude 1", []),
            feature("mirror", "FM1", "Mirror 1", [
                qlist("entities", featureId="FE1"), qlist("mirrorPlane", deterministicIds=["YZ"])]),
            feature("shell", "FSH", "Shell 1", [
                qlist("entities", deterministicIds=["TOP"]), quantity("thickness", 0.002, "2 mm", "meter")]),
            feature("circularPattern", "FCP", "Circular pattern 1", [
                enum("patternType", "FEATURE"), qlist("entities", featureId="FE1"),
                qlist("axis", deterministicIds=["DIR"]), quantity("instanceCount", 4, "4")]),
            feature("mirror", "FM2", "Mirror of parts", [enum("patternType", "PART")]),
        ]}
        t = ge.convert(feats, {"sketches": []}, FakeClient(), REF)
        m, sh, cp = t["features"][1], t["features"][2], t["features"][3]
        self.assertEqual(m, {"type": "mirror", "name": "Mirror 1", "features": ["Extrude 1"],
                             "plane": {"origin": [0.0, 10.0, 5.0], "normal": [1, 0, 0]}})
        self.assertEqual(sh, {"type": "shell", "name": "Shell 1", "side": "inside", "thickness": 2.0,
                              "faces": [{"point": [20.0, 10.0, 10.0], "normal": [0, 0, 1]}]})
        self.assertEqual((cp["type"], cp["count"], cp["angle"], cp["features"]),
                         ("pattern_circular", 4, 360.0, ["Extrude 1"]))
        self.assertEqual(t["features"][4]["type"], "unsupported")  # mirror without features

    def test_symmetric_cut_and_opposite(self):
        feats = {"features": [FEATURES["features"][1], feature("extrude", "FX", "Cut", [
            enum("operationType", "REMOVE"), enum("endBound", "SYMMETRIC"),
            qlist("entities", featureId="FS1"), quantity("depth", 0.006, "#d", "meter")])]}
        t = ge.convert(feats, SKETCHES)
        ex = t["features"][1]
        self.assertEqual((ex["operation"], ex["symmetric"], ex["distance"]), ("cut", True, 6.0))
        self.assertEqual(ex["distance_parameter"], "d")

    def test_failure_becomes_unsupported(self):
        t = ge.convert(FEATURES, {"sketches": []}, self.client, REF)  # no sketch info
        self.assertEqual(t["features"][0]["type"], "unsupported")
        self.assertIn("export failed", t["features"][0]["note"])
        self.assertEqual(t["features"][1]["type"], "unsupported")  # extrude without its sketch

    def test_unresolvable_edges(self):
        class Empty(FakeClient):
            def post(self, path, body, query=""):
                return {"result": {"message": {"value": ""}}}
        t = ge.convert(FEATURES, SKETCHES, Empty(), REF)
        self.assertEqual(t["features"][2]["type"], "unsupported")

    def test_matches_sample(self):
        with open(os.path.join(HERE, "sample", "expected.gtree.json"), encoding="utf-8") as f:
            self.assertEqual(json.load(f), json.loads(json.dumps(self.tree)))


if __name__ == "__main__":
    unittest.main()
