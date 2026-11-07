"""GussetExport: Fusion (Autodesk Fusion 360) add-in, exports the timeline to .gtree.json.

UNVERIFIED: written against the published Fusion API documentation, never run against
Fusion. Format: docs/adr/0042-feature-tree-interchange.md, version 1.

Thin dumper: it only READS the design. Unit conversion (Fusion API lengths are cm ->
mm, angles are radians -> degrees) and world coordinates are the only processing; every
mapping decision belongs to the Rust importer. Each feature is exported inside its own
try/except; a failure becomes an "unsupported" entry so the rest of the file survives.
"""
import json
import math
import traceback

import adsk.core
import adsk.fusion

EXPORTER = "plugins/fusion 0.1.0"
CM = 10.0  # Fusion API internal length unit is cm; the format is mm

_handlers = []
_cmd_id = "GussetExportCmd"


# ---------------------------------------------------------------- helpers
def _kind(obj):
    """Short class name of an API object, e.g. 'ExtrudeFeature' (from objectType)."""
    try:
        return obj.objectType.split("::")[-1]
    except Exception:
        return type(obj).__name__


def _mm(v):
    return round(v * CM, 9)


def _deg(v):
    return round(math.degrees(v), 9)


def _pt(p):
    """Point3D (cm, model space) -> [x, y, z] mm."""
    return [_mm(p.x), _mm(p.y), _mm(p.z)]


def _vec(v):
    return [round(v.x, 9), round(v.y, 9), round(v.z, 9)]


def _unsupported(name, original_type, reason=None):
    d = {"type": "unsupported", "name": name, "original_type": original_type}
    if reason:
        d["reason"] = reason  # informational extra; the importer may ignore it
    return d


def _param_name(model_param, user_names):
    """Name of the user parameter driving `model_param`, if its expression is exactly one."""
    try:
        expr = model_param.expression.strip()
        if expr in user_names:
            return expr
    except Exception:
        pass
    return None


def _user_parameters(design):
    out = []
    ups = design.userParameters
    for i in range(ups.count):
        p = ups.item(i)
        unit = (p.unit or "").strip()
        if unit in ("deg", "rad", "degree", "radian"):
            kind, value = "angle", _deg(p.value)  # API value is radians
        elif unit == "" or unit == "Unitless":
            kind, value = "number", p.value
        else:
            kind, value = "length", _mm(p.value)  # API value is cm
        out.append({"name": p.name, "expression": p.expression,
                    "value": value, "kind": kind})
    return out


# ---------------------------------------------------------------- sketches
class SketchInfo:
    """Entity-token -> id map of an exported sketch, so later features can refer to its lines."""

    def __init__(self, sketch):
        self.sketch = sketch
        self.ids = {}

    def id_of(self, entity):
        try:
            return self.ids.get(entity.entityToken)
        except Exception:
            return None


def _export_sketch(sketch, user_names):
    info = SketchInfo(sketch)
    # Matrix3D sketch->model space: origin (cm) and unit axes.
    _ok, origin, xd, yd, _zd = sketch.transform.getAsCoordinateSystem()
    out = {"type": "sketch", "name": sketch.name,
           "plane": {"origin": _pt(origin), "x_axis": _vec(xd), "y_axis": _vec(yd)},
           "points": [], "curves": [], "constraints": []}
    warnings = []

    def reg(entity, prefix, n):
        i = "%s%d" % (prefix, n)
        info.ids[entity.entityToken] = i
        return i

    pts = sketch.sketchPoints
    for i in range(pts.count):
        p = pts.item(i)
        g = p.geometry  # Point3D in sketch coordinates, cm
        out["points"].append({"id": reg(p, "p", i + 1), "x": _mm(g.x), "y": _mm(g.y),
                              "construction": bool(getattr(p, "isConstruction", False))})

    n = 0
    curves = sketch.sketchCurves
    for i in range(curves.sketchLines.count):
        c = curves.sketchLines.item(i)
        n += 1
        out["curves"].append({"id": reg(c, "l", n), "type": "line",
                              "start": info.id_of(c.startSketchPoint),
                              "end": info.id_of(c.endSketchPoint),
                              "construction": bool(c.isConstruction)})
    for i in range(curves.sketchCircles.count):
        c = curves.sketchCircles.item(i)
        n += 1
        out["curves"].append({"id": reg(c, "c", n), "type": "circle",
                              "center": info.id_of(c.centerSketchPoint),
                              "radius": _mm(c.radius),
                              "construction": bool(c.isConstruction)})
    for i in range(curves.sketchArcs.count):
        c = curves.sketchArcs.item(i)
        n += 1
        # SketchArc runs counter-clockwise from start to end about the sketch normal.
        out["curves"].append({"id": reg(c, "a", n), "type": "arc",
                              "center": info.id_of(c.centerSketchPoint),
                              "start": info.id_of(c.startSketchPoint),
                              "end": info.id_of(c.endSketchPoint),
                              "construction": bool(c.isConstruction)})
    # Ellipses, splines, conics etc. are not in format version 1.
    skipped = 0
    for coll in ("sketchEllipses", "sketchEllipticalArcs", "sketchFittedSplines",
                 "sketchFixedSplines", "sketchConicCurves"):
        try:
            skipped += getattr(curves, coll).count
        except Exception:
            pass
    if skipped:
        warnings.append("%d curve(s) of unsupported type left out of %s" % (skipped, sketch.name))

    try:
        _export_constraints(sketch, info, out["constraints"], user_names)
    except Exception as e:
        warnings.append("constraints of %s not exported: %s" % (sketch.name, e))
    return out, info, warnings


def _export_constraints(sketch, info, out, user_names):
    """Map known constraint/dimension classes; skip the rest silently."""
    gc = sketch.geometricConstraints
    for i in range(gc.count):
        c = gc.item(i)
        k = _kind(c)
        try:
            t, ents = None, None
            if k == "HorizontalConstraint":
                t, ents = "horizontal", [c.line]
            elif k == "VerticalConstraint":
                t, ents = "vertical", [c.line]
            elif k == "ParallelConstraint":
                t, ents = "parallel", [c.lineOne, c.lineTwo]
            elif k == "PerpendicularConstraint":
                t, ents = "perpendicular", [c.lineOne, c.lineTwo]
            elif k == "TangentConstraint":
                t, ents = "tangent", [c.curveOne, c.curveTwo]
            elif k == "EqualConstraint":
                t, ents = "equal", [c.curveOne, c.curveTwo]
            elif k == "CoincidentConstraint":
                # point on point, or point on a curve (on_line / on_circle by curve class)
                other = c.entity
                ok = _kind(other)
                t = ("coincident" if ok == "SketchPoint"
                     else "on_line" if ok == "SketchLine" else "on_circle")
                ents = [c.point, other]
            elif k == "MidPointConstraint":
                t, ents = "midpoint", [c.point, c.midPointCurve]
            elif k == "SymmetryConstraint":
                t, ents = "symmetric", [c.entityOne, c.entityTwo, c.symmetryLine]
            elif k == "FixedConstraint":
                t, ents = "fixed", [c.entity]
            if t is None:
                continue
            ids = [info.id_of(e) for e in ents]
            if any(x is None for x in ids):
                continue
            out.append({"type": t, "entities": ids})
        except Exception:
            continue

    dims = sketch.sketchDimensions
    for i in range(dims.count):
        d = dims.item(i)
        k = _kind(d)
        try:
            prm = d.parameter
            if k == "SketchLinearDimension":
                # between two entities (points), or the length of one line
                if d.entityTwo is not None:
                    ids = [info.id_of(d.entityOne), info.id_of(d.entityTwo)]
                elif _kind(d.entityOne) == "SketchLine":
                    ids = [info.id_of(d.entityOne.startSketchPoint),
                           info.id_of(d.entityOne.endSketchPoint)]
                else:
                    continue
                c = {"type": "distance", "entities": ids, "value": _mm(prm.value)}
            elif k == "SketchRadialDimension":
                c = {"type": "radius", "entities": [info.id_of(d.entity)], "value": _mm(prm.value)}
            elif k == "SketchDiameterDimension":
                c = {"type": "diameter", "entities": [info.id_of(d.entity)], "value": _mm(prm.value)}
            elif k == "SketchAngularDimension":
                c = {"type": "angle", "entities": [info.id_of(d.lineOne), info.id_of(d.lineTwo)],
                     "value": _deg(prm.value)}
            else:
                continue
            if any(x is None for x in c["entities"]):
                continue
            pname = _param_name(prm, user_names)
            if pname:
                c["parameter"] = pname
            out.append(c)
        except Exception:
            continue


# ---------------------------------------------------------------- profiles
def _loop_segments(loop):
    """Sketch-space segments (cm) of a profile loop. ProfileCurve.geometry is in sketch
    space; arcs/circles are sampled through their evaluator."""
    segs = []
    for j in range(loop.profileCurves.count):
        g = loop.profileCurves.item(j).geometry
        if _kind(g) == "Line3D":
            segs.append(((g.startPoint.x, g.startPoint.y), (g.endPoint.x, g.endPoint.y)))
            continue
        ev = g.evaluator
        _ok, lo, hi = ev.getParameterExtents()
        _ok, ps = ev.getPointsAtParameters([lo + (hi - lo) * s / 16.0 for s in range(17)])
        pp = [(p.x, p.y) for p in ps]
        segs.extend(zip(pp, pp[1:]))
    return segs


def _interior_point(profile, sketch):
    """A point inside the profile, in sketch coordinates (mm).

    First choice: the area centroid mapped into sketch coordinates (inverse of the sketch
    transform). A non-convex profile's centroid can fall outside it, and a hole can
    swallow it, so when loop geometry can be sampled we cut the profile with the
    horizontal line through the centroid (even-odd over all loops) and return the middle
    of the interval containing the centroid, else of the widest interval. If anything
    fails the plain centroid is returned.
    """
    props = profile.areaProperties(adsk.fusion.CalculationAccuracy.LowCalculationAccuracy)
    inv = sketch.transform.copy()
    inv.invert()
    c = props.centroid.copy()
    c.transformBy(inv)
    cu, cv = c.x, c.y
    best = (cu, cv)
    try:
        xs = []
        for li in range(profile.profileLoops.count):
            for (x1, y1), (x2, y2) in _loop_segments(profile.profileLoops.item(li)):
                if (y1 - cv) * (y2 - cv) < 0:
                    xs.append(x1 + (cv - y1) * (x2 - x1) / (y2 - y1))
        xs.sort()
        spans = [(xs[i], xs[i + 1]) for i in range(0, len(xs) - 1, 2)]
        if spans:
            inside = [s for s in spans if s[0] <= cu <= s[1]]
            if not inside:
                inside = [max(spans, key=lambda s: s[1] - s[0])]
            lo, hi = inside[0]
            best = ((lo + hi) / 2.0, cv)
    except Exception:
        pass
    return [_mm(best[0]), _mm(best[1])]


def _profile_items(feature):
    prof = feature.profile  # a Profile or an ObjectCollection of them
    if _kind(prof) == "Profile":
        return [prof]
    return [prof.item(i) for i in range(prof.count)]


def _profiles_of(items, sketch):
    if not items:
        raise ValueError("no profiles")
    if len(items) == sketch.profiles.count:
        return "all"
    return [{"point": _interior_point(p, sketch)} for p in items]


# ---------------------------------------------------------------- features
_OPS = {
    "NewBodyFeatureOperation": "new",
    "NewComponentFeatureOperation": "new",
    "JoinFeatureOperation": "join",
    "CutFeatureOperation": "cut",
    "IntersectFeatureOperation": "intersect",
}


def _operation(feature):
    for name, val in _OPS.items():
        if getattr(adsk.fusion.FeatureOperations, name, None) == feature.operation:
            return val
    raise ValueError("operation %s" % feature.operation)


def _common(f, sketches):
    items = _profile_items(f)
    if not items:
        raise ValueError("no profiles")
    sk = items[0].parentSketch
    if sk.name not in sketches:
        raise ValueError("profile sketch %s not exported" % sk.name)
    return sk, items


def _export_extrude(f, sketches, user_names):
    sk, items = _common(f, sketches)
    if getattr(f, "hasTwoExtents", False):
        raise ValueError("two-sided extent")
    ext = f.extentOne
    ek = _kind(ext)
    entry = {"type": "extrude", "name": f.name, "sketch": sk.name,
             "profiles": _profiles_of(items, sk), "symmetric": False,
             "operation": _operation(f)}
    if ek == "DistanceExtentDefinition":
        # signed distance (sign = direction); with isSymmetric it is the total length
        prm = ext.distance
        entry["distance"] = _mm(prm.value)
        entry["symmetric"] = bool(getattr(ext, "isSymmetric", False))
    elif ek == "SymmetricExtentDefinition":
        # distance is half the length unless isFullLength; the format wants the total
        prm = ext.distance
        full = bool(getattr(ext, "isFullLength", False))
        entry["distance"] = _mm(prm.value if full else prm.value * 2.0)
        entry["symmetric"] = True
    else:
        raise ValueError("extent %s" % ek)  # to-object, all, ...
    pn = _param_name(prm, user_names)
    if pn:
        entry["distance_parameter"] = pn
    return entry


def _export_revolve(f, sketches, user_names):
    sk, items = _common(f, sketches)
    ext = f.extentDefinition
    if _kind(ext) != "AngleExtentDefinition":
        raise ValueError("extent %s" % _kind(ext))
    entry = {"type": "revolve", "name": f.name, "sketch": sk.name,
             "profiles": _profiles_of(items, sk),
             "angle": _deg(ext.angle.value), "operation": _operation(f)}
    axis = f.axis
    ak = _kind(axis)
    if ak == "SketchLine":
        lid = sketches[sk.name].id_of(axis)
        if lid is None:
            raise ValueError("axis line is not in the profile sketch")
        entry["axis"] = {"line": lid}
    elif ak == "ConstructionAxis":
        g = axis.geometry  # InfiniteLine3D
        entry["axis"] = {"origin": _pt(g.origin), "direction": _vec(g.direction)}
    elif ak == "BRepEdge" and _kind(axis.geometry) == "Line3D":
        g = axis.geometry
        d = g.startPoint.vectorTo(g.endPoint)
        d.normalize()
        entry["axis"] = {"origin": _pt(g.startPoint), "direction": _vec(d)}
    else:
        raise ValueError("axis %s" % ak)
    return entry


def _edge_midpoint(edge):
    ev = edge.evaluator
    _ok, lo, hi = ev.getParameterExtents()
    ok, p = ev.getPointAtParameter((lo + hi) / 2.0)
    if not ok:
        raise ValueError("no edge midpoint")
    return {"midpoint": _pt(p)}  # model space cm -> mm


def _edge_sets(f, ftype):
    """One output feature per edge set (each set has its own radius / distance)."""
    out = []
    sets = f.edgeSets
    for i in range(sets.count):
        s = sets.item(i)
        k = _kind(s)
        edges = [_edge_midpoint(s.edges.item(j)) for j in range(s.edges.count)]
        if ftype == "fillet" and k == "ConstantRadiusFilletEdgeSet":
            out.append({"type": "fillet", "edges": edges, "radius": _mm(s.radius.value)})
        elif ftype == "chamfer" and k == "EqualDistanceChamferEdgeSet":
            out.append({"type": "chamfer", "edges": edges, "distance": _mm(s.distance.value)})
        else:
            raise ValueError("edge set kind %s" % k)  # variable radius, two distances, ...
    if not out:
        raise ValueError("no edge sets")
    for i, e in enumerate(out):
        e["name"] = f.name if len(out) == 1 else "%s_%d" % (f.name, i + 1)
    return out


def _export_bodies(design):
    out = []
    bodies = design.rootComponent.bRepBodies
    for i in range(bodies.count):
        b = bodies.item(i)
        try:
            if not b.isSolid:
                continue
            bb = b.boundingBox
            out.append({"name": b.name,
                        "volume_mm3": round(b.physicalProperties.volume * CM ** 3, 6),  # cm3
                        "bbox_mm": [_pt(bb.minPoint), _pt(bb.maxPoint)]})
        except Exception:
            continue
    return out


def build_tree(app, design):
    """Return (tree dict, list of warnings)."""
    warnings = []
    user_names = set(p["name"] for p in _user_parameters(design))
    sketches = {}
    features = []

    tl = design.timeline
    for i in range(min(tl.count, tl.markerPosition)):  # rolled-back items are left out
        item = tl.item(i)
        name, ot = "item%d" % i, "unknown"
        try:
            if item.isSuppressed:
                continue  # suppressed features are left out
            ent = item.entity
            name = ent.name
            ot = _kind(ent)
        except Exception:
            features.append(_unsupported(name, ot, "unreadable timeline item"))
            continue
        try:
            if ot == "Sketch":
                s, info, w = _export_sketch(ent, user_names)
                sketches[ent.name] = info
                features.append(s)
                warnings.extend(w)
            elif ot == "ExtrudeFeature":
                features.append(_export_extrude(ent, sketches, user_names))
            elif ot == "RevolveFeature":
                features.append(_export_revolve(ent, sketches, user_names))
            elif ot == "FilletFeature":
                features.extend(_edge_sets(ent, "fillet"))
            elif ot == "ChamferFeature":
                features.extend(_edge_sets(ent, "chamfer"))
            else:
                features.append(_unsupported(name, ot))
        except Exception as e:
            features.append(_unsupported(name, ot, str(e)))
            warnings.append("%s (%s): %s" % (name, ot, e))

    doc = app.activeDocument
    tree = {
        "format": "gusset-feature-tree", "version": 1,
        "source": {"app": "fusion", "app_version": app.version, "exporter": EXPORTER,
                   "document": doc.name if doc else design.rootComponent.name},
        "parameters": _user_parameters(design),
        "features": features,
        "bodies": _export_bodies(design),
    }
    return tree, warnings


def export_design(app, design, path):
    tree, warnings = build_tree(app, design)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(tree, fh, indent=2)
    return tree, warnings


# ---------------------------------------------------------------- UI
class _ExecuteHandler(adsk.core.CommandEventHandler):
    def notify(self, args):
        app = adsk.core.Application.get()
        ui = app.userInterface
        try:
            design = adsk.fusion.Design.cast(app.activeProduct)
            if not design:
                ui.messageBox("Open a Fusion design first.")
                return
            dlg = ui.createFileDialog()
            dlg.title = "Export Gusset feature tree"
            dlg.filter = "Gusset feature tree (*.gtree.json)"
            dlg.initialFilename = (app.activeDocument.name or "design") + ".gtree.json"
            if dlg.showSave() != adsk.core.DialogResults.DialogOK:
                return
            tree, warnings = export_design(app, design, dlg.filename)
            unsup = sum(1 for f in tree["features"] if f["type"] == "unsupported")
            msg = "Wrote %s\n%d features (%d unsupported)." % (
                dlg.filename, len(tree["features"]), unsup)
            if warnings:
                msg += "\n\n" + "\n".join(warnings[:10])
            ui.messageBox(msg)
        except Exception:
            ui.messageBox("Gusset export failed:\n" + traceback.format_exc())


class _CreatedHandler(adsk.core.CommandCreatedEventHandler):
    def notify(self, args):
        h = _ExecuteHandler()
        args.command.execute.add(h)
        _handlers.append(h)


def run(context):
    ui = None
    try:
        app = adsk.core.Application.get()
        ui = app.userInterface
        cmd = ui.commandDefinitions.addButtonDefinition(
            _cmd_id, "Export to Gusset", "Export the timeline as .gtree.json", "")
        h = _CreatedHandler()
        cmd.commandCreated.add(h)
        _handlers.append(h)
        panel = ui.allToolbarPanels.itemById("SolidScriptsAddinsPanel")
        panel.controls.addCommand(cmd)
    except Exception:
        if ui:
            ui.messageBox("GussetExport failed to start:\n" + traceback.format_exc())


def stop(context):
    try:
        ui = adsk.core.Application.get().userInterface
        panel = ui.allToolbarPanels.itemById("SolidScriptsAddinsPanel")
        ctl = panel.controls.itemById(_cmd_id)
        if ctl:
            ctl.deleteMe()
        d = ui.commandDefinitions.itemById(_cmd_id)
        if d:
            d.deleteMe()
    except Exception:
        pass
