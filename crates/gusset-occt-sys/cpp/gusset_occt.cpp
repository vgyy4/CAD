// Gusset ⇄ OCCT shim implementation. See gusset_occt.h for the contract.
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// This file only *uses* OCCT through its public headers (permitted by the Open CASCADE
// exception to the LGPL-2.1); OCCT itself is linked dynamically.

#include "gusset_occt.h"

#include <cmath>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <sstream>
#include <string>
#include <vector>

#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <GeomAbs_JoinType.hxx>
#include <gp_Trsf.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepLProp_SLProps.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRep_Builder.hxx>
#include <BRep_Tool.hxx>
#include <BinTools.hxx>
#include <Bnd_Box.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GProp_GProps.hxx>
#include <IGESControl_Controller.hxx>
#include <IGESControl_Reader.hxx>
#include <IGESControl_Writer.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <NCollection_IndexedDataMap.hxx>
#include <NCollection_IndexedMap.hxx>
#include <NCollection_List.hxx>
#include <Poly_PolygonOnTriangulation.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPCAFControl_Writer.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopAbs.hxx>
#include <TopExp.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Vertex.hxx>
#include <TopoDS_Wire.hxx>
#include <TopTools_ShapeMapHasher.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Dir.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>

struct GsShape {
  TopoDS_Shape s;
};

namespace {

using ShapeMap = NCollection_IndexedMap<TopoDS_Shape, TopTools_ShapeMapHasher>;
using ShapeList = NCollection_List<TopoDS_Shape>;
using AncMap = NCollection_IndexedDataMap<TopoDS_Shape, ShapeList, TopTools_ShapeMapHasher>;

thread_local std::string g_err;

// OCCT prints progress/statistics to stdout by default; the app's stdout belongs to the
// CLI and the MCP stdio transport, so remove the default printers once.
void quiet_occt() {
  static bool done = [] {
    const occ::handle<Message_Messenger>& m = Message::DefaultMessenger();
    m->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    return true;
  }();
  (void)done;
}

int fail(const std::string& msg) {
  g_err = msg;
  return GS_ERR;
}

template <typename T>
void to_buf(const std::vector<T>& v, GsBuf* out) {
  out->ptr = nullptr;
  out->len = 0;
  if (v.empty()) return;
  void* p = std::malloc(v.size() * sizeof(T));
  if (!p) throw std::bad_alloc();
  std::memcpy(p, v.data(), v.size() * sizeof(T));
  out->ptr = p;
  out->len = v.size();
}

void bytes_to_buf(const std::string& s, GsBuf* out) {
  std::vector<uint8_t> v(s.begin(), s.end());
  to_buf(v, out);
}

#define GS_GUARD_BEGIN \
  quiet_occt();        \
  try {
#define GS_GUARD_END                                                \
  }                                                                 \
  catch (const Standard_Failure& e) {                               \
    const char* w = e.what();                                       \
    return fail(std::string("geometry kernel error: ") +            \
                (w && *w ? w : "unspecified OCCT failure"));        \
  }                                                                 \
  catch (const std::bad_alloc&) {                                   \
    return fail("out of memory in the geometry kernel");            \
  }                                                                 \
  catch (const std::exception& e) {                                 \
    return fail(std::string("geometry kernel error: ") + e.what()); \
  }                                                                 \
  catch (...) {                                                     \
    return fail("unknown geometry kernel error");                   \
  }

struct Frame {
  gp_Pnt o;
  gp_Dir x, y, n;
  gp_Pnt at(double u, double v) const {
    return gp_Pnt(o.X() + x.X() * u + y.X() * v, o.Y() + x.Y() * u + y.Y() * v, o.Z() + x.Z() * u + y.Z() * v);
  }
};

Frame make_frame(const GsProfile* p, double offset) {
  gp_Dir x(p->x[0], p->x[1], p->x[2]);
  gp_Dir y(p->y[0], p->y[1], p->y[2]);
  gp_Dir n = x.Crossed(y);
  gp_Pnt o(p->origin[0] + n.X() * offset, p->origin[1] + n.Y() * offset, p->origin[2] + n.Z() * offset);
  return Frame{o, x, y, n};
}

// Signed area of a loop in profile coordinates, arcs sampled.
double loop_area(const GsProfile* p, const GsLoop& l) {
  std::vector<std::pair<double, double>> pts;
  for (uint32_t i = 0; i < l.count; ++i) {
    const GsSeg& s = p->segs[l.first + i];
    if (s.kind == GS_SEG_LINE) {
      pts.emplace_back(s.a[0], s.a[1]);
    } else if (s.kind == GS_SEG_CIRCLE) {
      for (int k = 0; k < 32; ++k) {
        double t = 2.0 * M_PI * k / 32.0;
        pts.emplace_back(s.c[0] + s.r * std::cos(t), s.c[1] + s.r * std::sin(t));
      }
    } else {
      double a0 = std::atan2(s.a[1] - s.c[1], s.a[0] - s.c[0]);
      double a1 = std::atan2(s.b[1] - s.c[1], s.b[0] - s.c[0]);
      double r = std::hypot(s.a[0] - s.c[0], s.a[1] - s.c[1]);
      double sweep = a1 - a0;
      if (s.ccw) {
        while (sweep <= 0) sweep += 2 * M_PI;
      } else {
        while (sweep >= 0) sweep -= 2 * M_PI;
      }
      for (int k = 0; k < 16; ++k) {
        double t = a0 + sweep * k / 16.0;
        pts.emplace_back(s.c[0] + r * std::cos(t), s.c[1] + r * std::sin(t));
      }
    }
  }
  double a = 0;
  for (size_t i = 0; i < pts.size(); ++i) {
    const auto& u = pts[i];
    const auto& w = pts[(i + 1) % pts.size()];
    a += u.first * w.second - w.first * u.second;
  }
  return a * 0.5;
}

struct TaggedEdge {
  TopoDS_Edge e;
  uint64_t tag;
};

// Builds one planar face per region, recording (edge, tag) pairs.
std::vector<TopoDS_Face> build_faces(const GsProfile* p, const Frame& f, std::vector<TaggedEdge>& tagged,
                                     std::vector<uint32_t>& face_region) {
  if (!p->segs || !p->loops || p->nloops == 0) throw Standard_Failure("profile has no loops");
  uint32_t nregions = 0;
  for (size_t i = 0; i < p->nloops; ++i) nregions = std::max(nregions, p->loops[i].region + 1);
  std::vector<TopoDS_Wire> outer(nregions);
  std::vector<std::vector<TopoDS_Wire>> holes(nregions);
  std::vector<bool> has_outer(nregions, false);
  gp_Pln pln(gp_Ax3(f.o, f.n, f.x));
  for (size_t li = 0; li < p->nloops; ++li) {
    const GsLoop& l = p->loops[li];
    if (l.count == 0 || size_t(l.first) + l.count > p->nsegs) throw Standard_Failure("profile loop out of range");
    BRepBuilderAPI_MakeWire mw;
    for (uint32_t i = 0; i < l.count; ++i) {
      const GsSeg& s = p->segs[l.first + i];
      TopoDS_Edge e;
      if (s.kind == GS_SEG_LINE) {
        BRepBuilderAPI_MakeEdge me(f.at(s.a[0], s.a[1]), f.at(s.b[0], s.b[1]));
        if (!me.IsDone()) throw Standard_Failure("degenerate line in profile");
        e = me.Edge();
      } else if (s.kind == GS_SEG_CIRCLE) {
        if (!(s.r > 0)) throw Standard_Failure("circle with non-positive radius in profile");
        gp_Circ c(gp_Ax2(f.at(s.c[0], s.c[1]), f.n, f.x), s.r);
        e = BRepBuilderAPI_MakeEdge(c).Edge();
      } else if (s.kind == GS_SEG_ARC) {
        double r = std::hypot(s.a[0] - s.c[0], s.a[1] - s.c[1]);
        if (!(r > 0)) throw Standard_Failure("arc with zero radius in profile");
        gp_Dir axis = s.ccw ? f.n : f.n.Reversed();
        gp_Circ c(gp_Ax2(f.at(s.c[0], s.c[1]), axis, f.x), r);
        BRepBuilderAPI_MakeEdge me(c, f.at(s.a[0], s.a[1]), f.at(s.b[0], s.b[1]));
        if (!me.IsDone()) throw Standard_Failure("could not build an arc of the profile");
        e = me.Edge();
      } else {
        throw Standard_Failure("unknown profile segment kind");
      }
      mw.Add(e);
      if (!mw.IsDone()) throw Standard_Failure("profile loop is not connected");
      // MakeWire may replace the edge (to share vertices); tag the edge actually used.
      tagged.push_back(TaggedEdge{mw.Edge(), s.tag});
    }
    TopoDS_Wire w = mw.Wire();
    double area = loop_area(p, l);
    bool ccw = area > 0;
    if (l.outer) {
      if (!ccw) w.Reverse();
      outer[l.region] = w;
      has_outer[l.region] = true;
    } else {
      if (ccw) w.Reverse();
      holes[l.region].push_back(w);
    }
  }
  std::vector<TopoDS_Face> faces;
  for (uint32_t r = 0; r < nregions; ++r) {
    if (!has_outer[r]) throw Standard_Failure("profile region has no outer loop");
    BRepBuilderAPI_MakeFace mf(pln, outer[r], true);
    for (const auto& h : holes[r]) mf.Add(h);
    if (!mf.IsDone()) throw Standard_Failure("could not build a face from the profile");
    faces.push_back(mf.Face());
    face_region.push_back(r);
  }
  return faces;
}

void push_origin(std::vector<int64_t>& v, int64_t face, int64_t kind, int64_t a, int64_t b) {
  v.push_back(face);
  v.push_back(kind);
  v.push_back(a);
  v.push_back(b);
}

void check_valid(const TopoDS_Shape& s, const char* what) {
  BRepCheck_Analyzer an(s);
  if (!an.IsValid()) throw Standard_Failure((std::string(what) + " produced an invalid solid").c_str());
}

// Records Input/Modified history of every face of input `input_idx`.
template <typename Op>
void record_input_faces(Op& op, const TopoDS_Shape& input, int64_t input_idx, const ShapeMap& out_faces,
                        std::vector<int64_t>& origins) {
  ShapeMap in_faces;
  TopExp::MapShapes(input, TopAbs_FACE, in_faces);
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    const TopoDS_Shape& g = in_faces(i);
    const ShapeList& mods = op.Modified(g);
    if (mods.IsEmpty()) {
      int idx = out_faces.FindIndex(g);
      if (idx > 0) push_origin(origins, idx - 1, GS_ORIGIN_INPUT, input_idx, i - 1);
    } else {
      for (ShapeList::Iterator it(mods); it.More(); it.Next()) {
        int idx = out_faces.FindIndex(it.Value());
        if (idx > 0) push_origin(origins, idx - 1, GS_ORIGIN_INPUT, input_idx, i - 1);
      }
    }
  }
}

template <typename Op>
void record_generated(Op& op, const TopoDS_Shape& input, TopAbs_ShapeEnum type, int64_t kind, int64_t input_idx,
                      const ShapeMap& out_faces, std::vector<int64_t>& origins) {
  ShapeMap in_sub;
  TopExp::MapShapes(input, type, in_sub);
  for (int i = 1; i <= in_sub.Extent(); ++i) {
    const ShapeList& gen = op.Generated(in_sub(i));
    for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
      if (it.Value().ShapeType() != TopAbs_FACE) continue;
      int idx = out_faces.FindIndex(it.Value());
      if (idx > 0) push_origin(origins, idx - 1, kind, input_idx, i - 1);
    }
  }
}

TopoDS_Edge edge_by_index(const TopoDS_Shape& s, uint32_t i) {
  ShapeMap edges;
  TopExp::MapShapes(s, TopAbs_EDGE, edges);
  if (int(i) >= edges.Extent()) throw Standard_Failure("edge index out of range");
  return TopoDS::Edge(edges(int(i) + 1));
}

int surface_kind(const TopoDS_Face& f) {
  BRepAdaptor_Surface s(f, false);
  switch (s.GetType()) {
    case GeomAbs_Plane: return 0;
    case GeomAbs_Cylinder: return 1;
    case GeomAbs_Cone: return 2;
    case GeomAbs_Sphere: return 3;
    case GeomAbs_Torus: return 4;
    case GeomAbs_BSplineSurface:
    case GeomAbs_BezierSurface: return 5;
    default: return 6;
  }
}

int curve_kind(const TopoDS_Edge& e) {
  if (BRep_Tool::Degenerated(e)) return 4;
  BRepAdaptor_Curve c(e);
  switch (c.GetType()) {
    case GeomAbs_Line: return 0;
    case GeomAbs_Circle: return 1;
    case GeomAbs_Ellipse: return 2;
    case GeomAbs_BSplineCurve:
    case GeomAbs_BezierCurve: return 3;
    default: return 4;
  }
}

std::vector<GsShape*> read_with(XSControl_Reader& r, const uint8_t* data, size_t len, const char* name) {
  std::string buf(reinterpret_cast<const char*>(data), len);
  std::istringstream is(buf);
  IFSelect_ReturnStatus st = r.ReadStream(name, is);
  if (st != IFSelect_RetDone) throw Standard_Failure("the file could not be parsed");
  r.TransferRoots();
  std::vector<GsShape*> out;
  for (int i = 1; i <= r.NbShapes(); ++i) {
    TopoDS_Shape s = r.Shape(i);
    if (!s.IsNull()) out.push_back(new GsShape{s});
  }
  return out;
}

int shapes_out(std::vector<GsShape*>& v, GsShape*** out, size_t* n) {
  *n = v.size();
  *out = nullptr;
  if (v.empty()) return GS_OK;
  auto** arr = static_cast<GsShape**>(std::malloc(v.size() * sizeof(GsShape*)));
  if (!arr) {
    for (auto* s : v) delete s;
    throw std::bad_alloc();
  }
  for (size_t i = 0; i < v.size(); ++i) arr[i] = v[i];
  *out = arr;
  return GS_OK;
}

template <typename Mk>
int blend(const GsShape* s, const uint32_t* edges, size_t nedges, double size, bool fillet, GsShape** out,
          GsBuf* origins) {
  if (!s || !out || !origins || (!edges && nedges)) return fail("null argument");
  if (nedges == 0) return fail("no edges selected");
  if (!std::isfinite(size) || size <= 0) return fail(fillet ? "fillet radius must be positive" : "chamfer distance must be positive");
  Mk mk(s->s);
  for (size_t i = 0; i < nedges; ++i) mk.Add(size, edge_by_index(s->s, edges[i]));
  mk.Build();
  if (!mk.IsDone()) return fail(fillet ? "fillet failed" : "chamfer failed");
  TopoDS_Shape result = mk.Shape();
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  record_input_faces(mk, s->s, 0, out_faces, o);
  record_generated(mk, s->s, TopAbs_EDGE, GS_ORIGIN_FROM_EDGE, 0, out_faces, o);
  record_generated(mk, s->s, TopAbs_VERTEX, GS_ORIGIN_FROM_VERTEX, 0, out_faces, o);
  check_valid(result, fillet ? "fillet" : "chamfer");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
}

}  // namespace

extern "C" {

const char* gs_last_error(void) { return g_err.c_str(); }

const char* gs_occt_version(void) { return OCC_VERSION_COMPLETE; }

void gs_shape_free(GsShape* s) { delete s; }

void gs_buf_free(GsBuf* b) {
  if (b && b->ptr) std::free(b->ptr);
  if (b) {
    b->ptr = nullptr;
    b->len = 0;
  }
}

void gs_shape_array_free(GsShape** shapes, size_t n) {
  if (!shapes) return;
  for (size_t i = 0; i < n; ++i) delete shapes[i];
  std::free(shapes);
}

int gs_extrude(const GsProfile* p, double distance, int symmetric, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !out || !origins) return fail("null argument");
  if (!std::isfinite(distance) || std::fabs(distance) < 1e-9) return fail("extrude distance must be non-zero");
  double offset = symmetric ? -std::fabs(distance) / 2.0 : 0.0;
  double len = symmetric ? std::fabs(distance) : distance;
  Frame f = make_frame(p, offset);
  std::vector<TaggedEdge> tagged;
  std::vector<uint32_t> face_region;
  std::vector<TopoDS_Face> faces = build_faces(p, f, tagged, face_region);
  gp_Vec v(f.n.X() * len, f.n.Y() * len, f.n.Z() * len);

  std::vector<TopoDS_Shape> solids;
  std::vector<BRepPrimAPI_MakePrism*> ops;
  for (const auto& face : faces) {
    auto* mk = new BRepPrimAPI_MakePrism(face, v, true);
    mk->Build();
    if (!mk->IsDone()) {
      for (auto* o : ops) delete o;
      delete mk;
      return fail("extrusion failed");
    }
    ops.push_back(mk);
    solids.push_back(mk->Shape());
  }
  TopoDS_Shape result;
  if (solids.size() == 1) {
    result = solids[0];
  } else {
    TopoDS_Compound comp;
    BRep_Builder b;
    b.MakeCompound(comp);
    for (const auto& s : solids) b.Add(comp, s);
    result = comp;
  }
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (size_t ri = 0; ri < ops.size(); ++ri) {
    BRepPrimAPI_MakePrism* mk = ops[ri];
    for (const auto& te : tagged) {
      const ShapeList& gen = mk->Generated(te.e);
      for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
        int idx = out_faces.FindIndex(it.Value());
        if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_LATERAL, int64_t(te.tag), 0);
      }
    }
    // Start cap = the profile face itself; end cap = its translated copy.
    int a = out_faces.FindIndex(mk->FirstShape(faces[ri]));
    int b = out_faces.FindIndex(mk->LastShape(faces[ri]));
    if (a > 0) push_origin(o, a - 1, GS_ORIGIN_CAP, 0, face_region[ri]);
    if (b > 0) push_origin(o, b - 1, GS_ORIGIN_CAP, 1, face_region[ri]);
  }
  for (auto* op : ops) delete op;
  check_valid(result, "extrusion");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_revolve(const GsProfile* p, const double axis_origin[2], const double axis_dir[2], double angle,
               GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !out || !origins || !axis_origin || !axis_dir) return fail("null argument");
  if (!std::isfinite(angle) || angle <= 1e-9 || angle > 2 * M_PI + 1e-9)
    return fail("revolve angle must be between 0 and 360 degrees");
  Frame f = make_frame(p, 0.0);
  std::vector<TaggedEdge> tagged;
  std::vector<uint32_t> face_region;
  std::vector<TopoDS_Face> faces = build_faces(p, f, tagged, face_region);
  gp_Pnt ao = f.at(axis_origin[0], axis_origin[1]);
  gp_Vec ad(f.x.X() * axis_dir[0] + f.y.X() * axis_dir[1], f.x.Y() * axis_dir[0] + f.y.Y() * axis_dir[1],
            f.x.Z() * axis_dir[0] + f.y.Z() * axis_dir[1]);
  if (ad.Magnitude() < 1e-12) return fail("revolve axis has zero length");
  gp_Ax1 axis(ao, gp_Dir(ad));
  bool full = angle >= 2 * M_PI - 1e-9;

  std::vector<TopoDS_Shape> solids;
  std::vector<BRepPrimAPI_MakeRevol*> ops;
  for (const auto& face : faces) {
    auto* mk = full ? new BRepPrimAPI_MakeRevol(face, axis, true) : new BRepPrimAPI_MakeRevol(face, axis, angle, true);
    mk->Build();
    if (!mk->IsDone()) {
      for (auto* o : ops) delete o;
      delete mk;
      return fail("revolve failed (does the profile cross the axis?)");
    }
    ops.push_back(mk);
    solids.push_back(mk->Shape());
  }
  TopoDS_Shape result;
  if (solids.size() == 1) {
    result = solids[0];
  } else {
    TopoDS_Compound comp;
    BRep_Builder b;
    b.MakeCompound(comp);
    for (const auto& s : solids) b.Add(comp, s);
    result = comp;
  }
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (size_t ri = 0; ri < ops.size(); ++ri) {
    BRepPrimAPI_MakeRevol* mk = ops[ri];
    for (const auto& te : tagged) {
      const ShapeList& gen = mk->Generated(te.e);
      for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
        int idx = out_faces.FindIndex(it.Value());
        if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_LATERAL, int64_t(te.tag), 0);
      }
    }
    if (!full) {
      int a = out_faces.FindIndex(mk->FirstShape(faces[ri]));
      int b = out_faces.FindIndex(mk->LastShape(faces[ri]));
      if (a > 0) push_origin(o, a - 1, GS_ORIGIN_CAP, 0, face_region[ri]);
      if (b > 0) push_origin(o, b - 1, GS_ORIGIN_CAP, 1, face_region[ri]);
    }
  }
  for (auto* op : ops) delete op;
  check_valid(result, "revolve");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_boolean(int op, const GsShape* target, const GsShape* tool, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!target || !tool || !out || !origins) return fail("null argument");
  ShapeList args, tools;
  args.Append(target->s);
  tools.Append(tool->s);
  BRepAlgoAPI_BooleanOperation* b = nullptr;
  BRepAlgoAPI_Fuse fuse;
  BRepAlgoAPI_Cut cut;
  BRepAlgoAPI_Common common;
  switch (op) {
    case GS_BOOL_UNION: b = &fuse; break;
    case GS_BOOL_CUT: b = &cut; break;
    case GS_BOOL_INTERSECT: b = &common; break;
    default: return fail("unknown boolean operation");
  }
  b->SetArguments(args);
  b->SetTools(tools);
  b->SetRunParallel(false);
  b->Build();
  if (!b->IsDone() || b->HasErrors()) return fail("boolean operation failed");
  b->SimplifyResult(true, true);
  TopoDS_Shape result = b->Shape();
  if (result.IsNull()) return fail("boolean operation produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  if (out_faces.Extent() == 0) return fail("boolean operation removed all material");
  std::vector<int64_t> o;
  record_input_faces(*b, target->s, 0, out_faces, o);
  record_input_faces(*b, tool->s, 1, out_faces, o);
  check_valid(result, "boolean operation");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_fillet(const GsShape* s, const uint32_t* edges, size_t nedges, double radius, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  return blend<BRepFilletAPI_MakeFillet>(s, edges, nedges, radius, true, out, origins);
  GS_GUARD_END
}

int gs_chamfer(const GsShape* s, const uint32_t* edges, size_t nedges, double dist, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  return blend<BRepFilletAPI_MakeChamfer>(s, edges, nedges, dist, false, out, origins);
  GS_GUARD_END
}

int gs_shell(const GsShape* s, const uint32_t* faces, size_t nfaces, double thickness, GsShape** out,
             GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || (!faces && nfaces)) return fail("null argument");
  if (!std::isfinite(thickness) || std::fabs(thickness) < 1e-9) return fail("shell thickness must be non-zero");
  ShapeMap in_faces;
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  ShapeList closing;
  for (size_t i = 0; i < nfaces; ++i) {
    if (int(faces[i]) >= in_faces.Extent()) return fail("face index out of range");
    closing.Append(in_faces(int(faces[i]) + 1));
  }
  // OCCT: a negative offset builds the wall inside the solid.
  BRepOffsetAPI_MakeThickSolid mk;
  mk.MakeThickSolidByJoin(s->s, closing, -thickness, 1e-4, BRepOffset_Skin, false, false, GeomAbs_Intersection);
  if (!mk.IsDone()) return fail("shell failed");
  TopoDS_Shape result = mk.Shape();
  if (result.IsNull()) return fail("shell produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    bool removed = false;
    for (size_t k = 0; k < nfaces; ++k) removed = removed || faces[k] == uint32_t(i - 1);
    const TopoDS_Shape& g = in_faces(i);
    if (removed) {
      // The ring left where a removed face was inherits that face's identity.
      const ShapeList& mods = mk.Modified(g);
      for (ShapeList::Iterator it(mods); it.More(); it.Next()) {
        int idx = out_faces.FindIndex(it.Value());
        if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
      }
      const ShapeList& gen = mk.Generated(g);
      for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
        if (it.Value().ShapeType() != TopAbs_FACE) continue;
        int idx = out_faces.FindIndex(it.Value());
        if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
      }
      continue;
    }
    {
      const ShapeList& mods = mk.Modified(g);
      if (mods.IsEmpty()) {
        int idx = out_faces.FindIndex(g);
        if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
      } else {
        for (ShapeList::Iterator it(mods); it.More(); it.Next()) {
          int idx = out_faces.FindIndex(it.Value());
          if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
        }
      }
    }
    const ShapeList& gen = mk.Generated(g);
    for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
      if (it.Value().ShapeType() != TopAbs_FACE) continue;
      int idx = out_faces.FindIndex(it.Value());
      if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_OFFSET, 0, i - 1);
    }
  }
  check_valid(result, "shell");
  // Offsets that collapse (walls thicker than the part allows) can still give a "valid"
  // but wrong solid; require the volume to move the right way.
  {
    GProp_GProps before, after;
    BRepGProp::VolumeProperties(s->s, before);
    BRepGProp::VolumeProperties(result, after);
    double vb = before.Mass(), va = after.Mass();
    if (!(va > 1e-9) || (thickness > 0 && !(va < vb - 1e-9)))
      throw Standard_Failure("the walls are too thick for this part");
  }
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_transform(int kind, const double params[7], const GsShape* s, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!params || !s || !out || !origins) return fail("null argument");
  for (int i = 0; i < 7; ++i)
    if (!std::isfinite(params[i])) return fail("transform has non-finite parameters");
  gp_Trsf t;
  switch (kind) {
    case GS_XF_MIRROR: {
      if (std::hypot(params[3], std::hypot(params[4], params[5])) < 1e-12) return fail("mirror plane has no normal");
      t.SetMirror(gp_Ax2(gp_Pnt(params[0], params[1], params[2]), gp_Dir(params[3], params[4], params[5])));
      break;
    }
    case GS_XF_TRANSLATE: t.SetTranslation(gp_Vec(params[0], params[1], params[2])); break;
    case GS_XF_ROTATE: {
      if (std::hypot(params[3], std::hypot(params[4], params[5])) < 1e-12)
        return fail("rotation axis has no direction");
      t.SetRotation(gp_Ax1(gp_Pnt(params[0], params[1], params[2]), gp_Dir(params[3], params[4], params[5])),
                    params[6]);
      break;
    }
    default: return fail("unknown transform");
  }
  BRepBuilderAPI_Transform xf(s->s, t, true);
  if (!xf.IsDone()) return fail("transform failed");
  TopoDS_Shape result = xf.Shape();
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  ShapeMap in_faces;
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    int idx = out_faces.FindIndex(xf.ModifiedShape(in_faces(i)));
    if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
  }
  check_valid(result, "transform");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_compound(const GsShape* const* shapes, size_t n, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!out || !origins || (!shapes && n)) return fail("null argument");
  TopoDS_Compound comp;
  BRep_Builder b;
  b.MakeCompound(comp);
  for (size_t i = 0; i < n; ++i) b.Add(comp, shapes[i]->s);
  ShapeMap out_faces;
  TopExp::MapShapes(comp, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (size_t i = 0; i < n; ++i) {
    ShapeMap in_faces;
    TopExp::MapShapes(shapes[i]->s, TopAbs_FACE, in_faces);
    for (int k = 1; k <= in_faces.Extent(); ++k) {
      int idx = out_faces.FindIndex(in_faces(k));
      if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, int64_t(i), k - 1);
    }
  }
  to_buf(o, origins);
  *out = new GsShape{comp};
  return GS_OK;
  GS_GUARD_END
}

int gs_topology(const GsShape* s, GsBuf* faces, GsBuf* edges, GsBuf* edge_adj, GsBuf* verts, GsBuf* counts) {
  GS_GUARD_BEGIN
  if (!s || !faces || !edges || !edge_adj || !verts || !counts) return fail("null argument");
  ShapeMap fm, em, vm, sm, shm;
  TopExp::MapShapes(s->s, TopAbs_FACE, fm);
  TopExp::MapShapes(s->s, TopAbs_EDGE, em);
  TopExp::MapShapes(s->s, TopAbs_VERTEX, vm);
  TopExp::MapShapes(s->s, TopAbs_SOLID, sm);
  TopExp::MapShapes(s->s, TopAbs_SHELL, shm);
  std::vector<double> fv;
  fv.reserve(size_t(fm.Extent()) * 8);
  for (int i = 1; i <= fm.Extent(); ++i) {
    TopoDS_Face f = TopoDS::Face(fm(i));
    GProp_GProps g;
    BRepGProp::SurfaceProperties(f, g);
    gp_Pnt c = g.CentreOfMass();
    BRepAdaptor_Surface sa(f, true);
    double u = 0.5 * (sa.FirstUParameter() + sa.LastUParameter());
    double v = 0.5 * (sa.FirstVParameter() + sa.LastVParameter());
    BRepLProp_SLProps props(sa, u, v, 1, 1e-7);
    gp_Dir n(0, 0, 1);
    if (props.IsNormalDefined()) n = props.Normal();
    if (f.Orientation() == TopAbs_REVERSED) n.Reverse();
    fv.insert(fv.end(), {double(surface_kind(f)), g.Mass(), c.X(), c.Y(), c.Z(), n.X(), n.Y(), n.Z()});
  }
  AncMap anc;
  TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<double> ev;
  std::vector<int64_t> adj;
  for (int i = 1; i <= em.Extent(); ++i) {
    TopoDS_Edge e = TopoDS::Edge(em(i));
    double length = 0;
    gp_Pnt mid(0, 0, 0);
    if (!BRep_Tool::Degenerated(e)) {
      BRepAdaptor_Curve c(e);
      length = GCPnts_AbscissaPoint::Length(c);
      mid = c.Value(0.5 * (c.FirstParameter() + c.LastParameter()));
    } else {
      TopoDS_Vertex v1, v2;
      TopExp::Vertices(e, v1, v2);
      if (!v1.IsNull()) mid = BRep_Tool::Pnt(v1);
    }
    ev.insert(ev.end(), {double(curve_kind(e)), length, mid.X(), mid.Y(), mid.Z()});
    std::vector<int64_t> fidx;
    int ai = anc.FindIndex(e);
    if (ai > 0) {
      for (ShapeList::Iterator it(anc(ai)); it.More(); it.Next()) {
        int fi = fm.FindIndex(it.Value());
        if (fi > 0) {
          int64_t x = fi - 1;
          bool dup = false;
          for (auto y : fidx) dup |= (y == x);
          if (!dup) fidx.push_back(x);
        }
      }
    }
    std::sort(fidx.begin(), fidx.end());
    TopoDS_Vertex v1, v2;
    TopExp::Vertices(e, v1, v2);
    int64_t a = v1.IsNull() ? -1 : vm.FindIndex(v1) - 1;
    int64_t b = v2.IsNull() ? -1 : vm.FindIndex(v2) - 1;
    int64_t nv = (a >= 0) + (b >= 0 && b != a);
    adj.insert(adj.end(), {int64_t(std::min<size_t>(fidx.size(), 2)), fidx.size() > 0 ? fidx[0] : -1,
                           fidx.size() > 1 ? fidx[1] : -1, nv, a, (b != a) ? b : -1});
  }
  std::vector<double> vv;
  for (int i = 1; i <= vm.Extent(); ++i) {
    gp_Pnt p = BRep_Tool::Pnt(TopoDS::Vertex(vm(i)));
    vv.insert(vv.end(), {p.X(), p.Y(), p.Z()});
  }
  std::vector<int64_t> cv = {sm.Extent(), shm.Extent()};
  to_buf(fv, faces);
  to_buf(ev, edges);
  to_buf(adj, edge_adj);
  to_buf(vv, verts);
  to_buf(cv, counts);
  return GS_OK;
  GS_GUARD_END
}

int gs_mass(const GsShape* s, GsBuf* mass) {
  GS_GUARD_BEGIN
  if (!s || !mass) return fail("null argument");
  GProp_GProps vol, area;
  BRepGProp::VolumeProperties(s->s, vol);
  BRepGProp::SurfaceProperties(s->s, area);
  gp_Pnt c = vol.CentreOfMass();
  std::vector<double> v = {vol.Mass(), area.Mass(), c.X(), c.Y(), c.Z()};
  to_buf(v, mass);
  return GS_OK;
  GS_GUARD_END
}

int gs_bbox(const GsShape* s, GsBuf* bbox) {
  GS_GUARD_BEGIN
  if (!s || !bbox) return fail("null argument");
  Bnd_Box b;
  BRepBndLib::AddOptimal(s->s, b, false, false);
  if (b.IsVoid()) return fail("shape is empty");
  double x0, y0, z0, x1, y1, z1;
  b.Get(x0, y0, z0, x1, y1, z1);
  std::vector<double> v = {x0, y0, z0, x1, y1, z1};
  to_buf(v, bbox);
  return GS_OK;
  GS_GUARD_END
}

int gs_check(const GsShape* s) {
  GS_GUARD_BEGIN
  if (!s) return fail("null argument");
  check_valid(s->s, "the operation");
  return GS_OK;
  GS_GUARD_END
}

int gs_tessellate(const GsShape* s, double lin_defl, double ang_defl, GsBuf* positions, GsBuf* normals,
                  GsBuf* indices, GsBuf* face_ranges, GsBuf* edge_offsets, GsBuf* edge_points) {
  GS_GUARD_BEGIN
  if (!s || !positions || !normals || !indices || !face_ranges || !edge_offsets || !edge_points)
    return fail("null argument");
  if (!(lin_defl > 0) || !(ang_defl > 0)) return fail("tessellation tolerances must be positive");
  BRepMesh_IncrementalMesh mesher(s->s, lin_defl, false, ang_defl, false);
  ShapeMap fm, em;
  TopExp::MapShapes(s->s, TopAbs_FACE, fm);
  TopExp::MapShapes(s->s, TopAbs_EDGE, em);
  std::vector<float> pos, nrm;
  std::vector<uint32_t> idx, ranges;
  for (int i = 1; i <= fm.Extent(); ++i) {
    TopoDS_Face f = TopoDS::Face(fm(i));
    uint32_t start = uint32_t(idx.size() / 3);
    TopLoc_Location loc;
    const occ::handle<Poly_Triangulation>& tri = BRep_Tool::Triangulation(f, loc);
    if (!tri.IsNull()) {
      if (!tri->HasNormals()) BRepLib_ToolTriangulatedShape::ComputeNormals(f, tri);
      bool rev = f.Orientation() == TopAbs_REVERSED;
      gp_Trsf t = loc.Transformation();
      uint32_t base = uint32_t(pos.size() / 3);
      for (int k = 1; k <= tri->NbNodes(); ++k) {
        gp_Pnt p = tri->Node(k).Transformed(t);
        pos.insert(pos.end(), {float(p.X()), float(p.Y()), float(p.Z())});
        gp_Dir n = tri->Normal(k);
        gp_Vec nv(n);
        nv.Transform(t);
        if (rev) nv.Reverse();
        nrm.insert(nrm.end(), {float(nv.X()), float(nv.Y()), float(nv.Z())});
      }
      for (int k = 1; k <= tri->NbTriangles(); ++k) {
        int a, b, c;
        tri->Triangle(k).Get(a, b, c);
        if (rev) std::swap(b, c);
        idx.insert(idx.end(), {base + uint32_t(a - 1), base + uint32_t(b - 1), base + uint32_t(c - 1)});
      }
    }
    ranges.push_back(start);
    ranges.push_back(uint32_t(idx.size() / 3));
  }
  AncMap anc;
  TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<uint32_t> offs = {0};
  std::vector<float> pts;
  for (int i = 1; i <= em.Extent(); ++i) {
    TopoDS_Edge e = TopoDS::Edge(em(i));
    int ai = anc.FindIndex(e);
    if (ai > 0 && !anc(ai).IsEmpty() && !BRep_Tool::Degenerated(e)) {
      TopoDS_Face f = TopoDS::Face(anc(ai).First());
      TopLoc_Location loc;
      const occ::handle<Poly_Triangulation>& tri = BRep_Tool::Triangulation(f, loc);
      if (!tri.IsNull()) {
        const occ::handle<Poly_PolygonOnTriangulation>& poly = BRep_Tool::PolygonOnTriangulation(e, tri, loc);
        if (!poly.IsNull()) {
          gp_Trsf t = loc.Transformation();
          for (int k = 1; k <= poly->NbNodes(); ++k) {
            gp_Pnt p = tri->Node(poly->Node(k)).Transformed(t);
            pts.insert(pts.end(), {float(p.X()), float(p.Y()), float(p.Z())});
          }
        }
      }
    }
    offs.push_back(uint32_t(pts.size() / 3));
  }
  to_buf(pos, positions);
  to_buf(nrm, normals);
  to_buf(idx, indices);
  to_buf(ranges, face_ranges);
  to_buf(offs, edge_offsets);
  to_buf(pts, edge_points);
  return GS_OK;
  GS_GUARD_END
}

int gs_step_write(const GsShape* const* shapes, const char* const* names, size_t n, GsBuf* out) {
  GS_GUARD_BEGIN
  if (!out || (!shapes && n)) return fail("null argument");
  occ::handle<XCAFApp_Application> app = XCAFApp_Application::GetApplication();
  occ::handle<TDocStd_Document> doc;
  app->NewDocument("MDTV-XCAF", doc);
  occ::handle<XCAFDoc_ShapeTool> st = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
  for (size_t i = 0; i < n; ++i) {
    TDF_Label l = st->AddShape(shapes[i]->s, false);
    if (names && names[i]) TDataStd_Name::Set(l, TCollection_ExtendedString(names[i], true));
  }
  Interface_Static::SetCVal("write.step.unit", "MM");
  Interface_Static::SetCVal("write.step.product.name", "Gusset");
  STEPCAFControl_Writer w;
  w.SetNameMode(true);
  if (!w.Transfer(doc, STEPControl_AsIs)) {
    app->Close(doc);
    return fail("STEP export could not translate the model");
  }
  std::ostringstream os;
  IFSelect_ReturnStatus st2 = w.ChangeWriter().WriteStream(os);
  app->Close(doc);
  if (st2 != IFSelect_RetDone) return fail("STEP export failed");
  bytes_to_buf(os.str(), out);
  return GS_OK;
  GS_GUARD_END
}

int gs_iges_write(const GsShape* const* shapes, size_t n, GsBuf* out) {
  GS_GUARD_BEGIN
  if (!out || (!shapes && n)) return fail("null argument");
  IGESControl_Controller::Init();
  IGESControl_Writer w("MM", 1);
  for (size_t i = 0; i < n; ++i) w.AddShape(shapes[i]->s);
  w.ComputeModel();
  std::ostringstream os;
  if (!w.Write(os)) return fail("IGES export failed");
  bytes_to_buf(os.str(), out);
  return GS_OK;
  GS_GUARD_END
}

int gs_step_read(const uint8_t* data, size_t len, GsShape*** out_shapes, size_t* n) {
  GS_GUARD_BEGIN
  if (!data || !out_shapes || !n) return fail("null argument");
  STEPControl_Reader r;
  std::vector<GsShape*> v = read_with(r, data, len, "import.step");
  return shapes_out(v, out_shapes, n);
  GS_GUARD_END
}

/* IGES readers cannot read from a stream (IGESSelect_WorkLibrary has no ReadStream and
 * IGESFile_Read takes a file name), so this reads a file. The sandboxed worker is allowed
 * exactly this one file read-only. */
int gs_iges_read_file(const char* path, GsShape*** out_shapes, size_t* n) {
  GS_GUARD_BEGIN
  if (!path || !out_shapes || !n) return fail("null argument");
  IGESControl_Controller::Init();
  IGESControl_Reader r;
  IFSelect_ReturnStatus st = r.ReadFile(path);
  if (st != IFSelect_RetDone) throw Standard_Failure("the file could not be parsed");
  r.TransferRoots();
  std::vector<GsShape*> v;
  for (int i = 1; i <= r.NbShapes(); ++i) {
    TopoDS_Shape s = r.Shape(i);
    if (!s.IsNull()) v.push_back(new GsShape{s});
  }
  return shapes_out(v, out_shapes, n);
  GS_GUARD_END
}

int gs_brep_write(const GsShape* s, GsBuf* out) {
  GS_GUARD_BEGIN
  if (!s || !out) return fail("null argument");
  std::ostringstream os;
  BinTools::Write(s->s, os);
  bytes_to_buf(os.str(), out);
  return GS_OK;
  GS_GUARD_END
}

int gs_brep_read(const uint8_t* data, size_t len, GsShape** out) {
  GS_GUARD_BEGIN
  if (!data || !out) return fail("null argument");
  std::string buf(reinterpret_cast<const char*>(data), len);
  std::istringstream is(buf);
  TopoDS_Shape s;
  BinTools::Read(s, is);
  if (s.IsNull()) return fail("B-rep data is empty or damaged");
  *out = new GsShape{s};
  return GS_OK;
  GS_GUARD_END
}

}  // extern "C"
