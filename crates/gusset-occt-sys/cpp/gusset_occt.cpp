// Gusset ⇄ OCCT shim implementation. See gusset_occt.h for the contract.
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// This file only *uses* OCCT through its public headers (permitted by the Open CASCADE
// exception to the LGPL-2.1); OCCT itself is linked dynamically.

#include "gusset_occt.h"

#include <climits>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <sstream>
#include <string>
#include <vector>

#include <BRepAdaptor_CompCurve.hxx>
#include <BRepBuilderAPI_TransitionMode.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepTools.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GeomAPI_Interpolate.hxx>
#include <GeomConvert_CompCurveToBSplineCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <NCollection_HArray1.hxx>
#include <TopExp_Explorer.hxx>
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
#include <BRepAlgo_Image.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffset_MakeOffset.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <GeomAbs_JoinType.hxx>
#include <gp_Trsf.hxx>
#include <BRepAlgoAPI_Check.hxx>
#include <BRepTools_Modifier.hxx>
#include <BRepTools_TrsfModification.hxx>
#include <GeomLib_Tool.hxx>
#include <GeomProjLib.hxx>
#include <Geom2d_Curve.hxx>
#include <Geom_Curve.hxx>
#include <Geom_Surface.hxx>
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
#include <Geom_BezierCurve.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <Standard_Version.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopAbs.hxx>
#include <TopExp.hxx>
#include <OSD_Parallel.hxx>
#include <array>
#include <Geom_Plane.hxx>
#include <atomic>
#include <TopTools_IndexedMapOfShape.hxx>
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
    } else if (s.kind == GS_SEG_BEZIER) {
      if (s.pole_count == 4 && p->poles && size_t(s.pole_first) + 4 <= p->npoles) {
        const double* q = p->poles + 2 * size_t(s.pole_first);
        for (int j = 0; j < 12; ++j) {
          double t = j / 12.0, u = 1.0 - t;
          double w0 = u * u * u, w1 = 3 * u * u * t, w2 = 3 * u * t * t, w3 = t * t * t;
          pts.emplace_back(w0 * q[0] + w1 * q[2] + w2 * q[4] + w3 * q[6], w0 * q[1] + w1 * q[3] + w2 * q[5] + w3 * q[7]);
        }
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

Handle(Geom_BezierCurve) bezier_curve(const GsProfile* p, const GsSeg& s, const Frame& f) {
  if (s.pole_count != 4 || !p->poles || size_t(s.pole_first) + 4 > p->npoles)
    throw Standard_Failure("Bezier segment has an invalid control polygon");
  TColgp_Array1OfPnt poles(1, 4);
  for (int i = 0; i < 4; ++i) {
    const double* q = p->poles + 2 * (size_t(s.pole_first) + size_t(i));
    poles.SetValue(i + 1, f.at(q[0], q[1]));
  }
  return new Geom_BezierCurve(poles);
}

// One edge of a profile or path segment, placed in the frame `f`.
TopoDS_Edge make_seg_edge(const GsProfile* p, const GsSeg& s, const Frame& f) {
  TopoDS_Edge e;
  if (s.kind == GS_SEG_LINE) {
    BRepBuilderAPI_MakeEdge me(f.at(s.a[0], s.a[1]), f.at(s.b[0], s.b[1]));
    if (!me.IsDone()) throw Standard_Failure("degenerate line in profile");
    e = me.Edge();
  } else if (s.kind == GS_SEG_CIRCLE) {
    if (!(s.r > 0)) throw Standard_Failure("circle with non-positive radius in profile");
    gp_Circ c(gp_Ax2(f.at(s.c[0], s.c[1]), f.n, f.x), s.r);
    e = BRepBuilderAPI_MakeEdge(c).Edge();
  } else if (s.kind == GS_SEG_BEZIER) {
    BRepBuilderAPI_MakeEdge me(bezier_curve(p, s, f));
    if (!me.IsDone()) throw Standard_Failure("could not build a spline edge of the profile");
    e = me.Edge();
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
  return e;
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
      TopoDS_Edge e = make_seg_edge(p, s, f);
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
  // Sub-shapes are checked on all cores; the verdict does not depend on the order.
  BRepCheck_Analyzer an(s, true, true);
  if (!an.IsValid()) throw Standard_Failure((std::string(what) + " produced an invalid solid").c_str());
}

// For operations whose result is valid by construction when the input is (the prism of a
// planar profile face whose loops the sketch arrangement already checked): every edge
// bounds exactly two face sides, i.e. the shells are closed. Linear in the shape size;
// BRepCheck_Analyzer took 1-2 s on a gear with 280-edge caps (dogfooding F3-5).
void check_closed(const TopoDS_Shape& s, const char* what) {
  TopTools_IndexedMapOfShape edges;
  std::vector<int> uses;
  bool any_solid = false;
  for (TopExp_Explorer sx(s, TopAbs_SOLID); sx.More(); sx.Next()) any_solid = true;
  for (TopExp_Explorer fx(s, TopAbs_FACE); fx.More(); fx.Next()) {
    for (TopExp_Explorer ex(fx.Current(), TopAbs_EDGE); ex.More(); ex.Next()) {
      const TopoDS_Edge& e = TopoDS::Edge(ex.Current());
      if (BRep_Tool::Degenerated(e)) continue;
      int i = edges.Add(e);
      if (size_t(i) > uses.size()) uses.resize(size_t(i), 0);
      uses[size_t(i) - 1] += 1;
    }
  }
  bool closed = any_solid && !uses.empty();
  for (int n : uses) closed = closed && n == 2;
  if (!closed) throw Standard_Failure((std::string(what) + " produced an open or invalid solid").c_str());
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
  // Compare unsigned: an index above INT_MAX must not wrap negative (security review V6).
  if (edges.Extent() <= 0 || i >= uint32_t(edges.Extent())) throw Standard_Failure("edge index out of range");
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

// Does the shape have a face that is not a plane, cylinder, cone, sphere or torus?
bool has_freeform_face(const TopoDS_Shape& s) {
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
    switch (BRepAdaptor_Surface(TopoDS::Face(ex.Current()), false).GetType()) {
      case GeomAbs_Plane:
      case GeomAbs_Cylinder:
      case GeomAbs_Cone:
      case GeomAbs_Sphere:
      case GeomAbs_Torus: break;
      default: return true;
    }
  }
  return false;
}

// ---------------------------------------------------------------- sweep and loft helpers

// How far a path may start from the profile plane (mm).
const double kPathTol = 1e-3;
// The spline-segment bits of an entity tag (gusset-naming `with_segment`): the Bezier
// segments of one spline share the other bits.
const uint64_t kSegMask = 0xfffULL << 52;

// An origin record in terms of shapes; converted to face indices once the result is known.
struct Rec {
  TopoDS_Shape face;
  int64_t kind, a, b;
};

void emit_recs(const std::vector<Rec>& recs, const ShapeMap& out_faces, std::vector<int64_t>& o) {
  for (const auto& r : recs) {
    int idx = out_faces.FindIndex(r.face);
    if (idx > 0) push_origin(o, idx - 1, r.kind, r.a, r.b);
  }
}

double signed_volume(const TopoDS_Shape& s) {
  GProp_GProps g;
  BRepGProp::VolumeProperties(s, g);
  return g.Mass();
}

std::vector<TopoDS_Edge> ordered_edges(const TopoDS_Wire& w) {
  std::vector<TopoDS_Edge> v;
  for (BRepTools_WireExplorer ex(w); ex.More(); ex.Next()) v.push_back(ex.Current());
  ShapeMap all;
  TopExp::MapShapes(w, TopAbs_EDGE, all);
  if (int(v.size()) != all.Extent()) throw Standard_Failure("the path branches or has a gap, so it is not one chain");
  return v;
}

gp_Pnt edge_start(const TopoDS_Edge& e) { return BRep_Tool::Pnt(TopExp::FirstVertex(e, true)); }
gp_Pnt edge_end(const TopoDS_Edge& e) { return BRep_Tool::Pnt(TopExp::LastVertex(e, true)); }

TopoDS_Wire wire_from_edges(const std::vector<TopoDS_Edge>& es) {
  BRepBuilderAPI_MakeWire mw;
  for (const auto& e : es) {
    mw.Add(e);
    if (!mw.IsDone()) throw Standard_Failure("the path curves are not connected end to end");
  }
  return mw.Wire();
}

// The edges of a chain of sketch curves; the Bezier segments of one spline become one edge.
std::vector<TopoDS_Edge> curve_edges(const GsProfile* p) {
  if (!p->segs || !p->loops || p->nloops != 1) throw Standard_Failure("the path must be one chain of curves");
  const GsLoop& l = p->loops[0];
  if (l.count == 0 || size_t(l.first) + l.count > p->nsegs) throw Standard_Failure("path out of range");
  Frame f = make_frame(p, 0.0);
  std::vector<TopoDS_Edge> out;
  uint32_t i = 0;
  while (i < l.count) {
    const GsSeg& s = p->segs[l.first + i];
    if (s.kind != GS_SEG_BEZIER) {
      out.push_back(make_seg_edge(p, s, f));
      ++i;
      continue;
    }
    uint64_t base = s.tag & ~kSegMask;
    GeomConvert_CompCurveToBSplineCurve cc(bezier_curve(p, s, f));
    uint32_t j = i + 1;
    for (; j < l.count; ++j) {
      const GsSeg& t = p->segs[l.first + j];
      if (t.kind != GS_SEG_BEZIER || (t.tag & ~kSegMask) != base) break;
      if (!cc.Add(bezier_curve(p, t, f), 1e-6)) throw Standard_Failure("could not join the segments of a spline in the path");
    }
    BRepBuilderAPI_MakeEdge me(cc.BSplineCurve());
    if (!me.IsDone()) throw Standard_Failure("could not build a spline edge of the path");
    out.push_back(me.Edge());
    i = j;
  }
  return out;
}

struct Spine {
  TopoDS_Wire wire;
  std::vector<TopoDS_Edge> edges;  // in travel order, each oriented along the travel
  bool closed = false;
};

// The spine of a sweep from sketch curves or from edges of a shape, directed so that it
// starts on the profile plane (a closed path stays as it is).
Spine make_spine(const GsProfile* curves, const GsShape* shape, const uint32_t* eidx, size_t n, const Frame& pf) {
  TopoDS_Wire w;
  if (curves) {
    w = wire_from_edges(curve_edges(curves));
  } else {
    if (!shape || !eidx || n == 0) throw Standard_Failure("the path has no edges");
    ShapeList l;
    for (size_t i = 0; i < n; ++i) l.Append(edge_by_index(shape->s, eidx[i]));
    BRepBuilderAPI_MakeWire mw;
    mw.Add(l);
    if (!mw.IsDone()) throw Standard_Failure("the selected edges do not form one connected chain");
    w = mw.Wire();
  }
  Spine sp;
  sp.edges = ordered_edges(w);
  if (sp.edges.empty()) throw Standard_Failure("the path has no edges");
  gp_Pnt p0 = edge_start(sp.edges.front());
  gp_Pnt p1 = edge_end(sp.edges.back());
  sp.closed = p0.Distance(p1) < 1e-6;
  if (!sp.closed) {
    auto off = [&](const gp_Pnt& q) { return std::fabs(gp_Vec(pf.o, q).Dot(gp_Vec(pf.n))); };
    double d0 = off(p0), d1 = off(p1);
    if (std::min(d0, d1) > kPathTol) {
      std::ostringstream m;
      m << "the path does not start on the profile plane (its nearest end is " << std::min(d0, d1) << " mm away)";
      throw Standard_Failure(m.str().c_str());
    }
    if (d1 < d0) {
      std::vector<TopoDS_Edge> rev;
      for (size_t i = sp.edges.size(); i-- > 0;) rev.push_back(TopoDS::Edge(sp.edges[i].Reversed()));
      sp.edges = rev;
      w = wire_from_edges(sp.edges);
    }
  }
  sp.wire = w;
  return sp;
}

// An auxiliary spine that makes a sweep turn about the path: the point at distance `r`
// from the path in a rotation-minimising frame, turned by `twist` * (arc length fraction).
// The sweep's trihedron points at this curve, so the profile follows the turn.
TopoDS_Wire twist_guide(const Spine& sp, const Frame& pf, double twist) {
  BRepAdaptor_CompCurve cc(sp.wire);
  double len = GCPnts_AbscissaPoint::Length(cc);
  if (!(len > 1e-6)) throw Standard_Failure("the path is too short");
  // Sized in floating point and clamped before converting (review H2: a huge twist
  // overflowed the int conversion).
  double md = std::ceil(std::fabs(twist) / 0.05) + 6.0 * double(sp.edges.size()) + 48.0;
  int m = int(std::min(md, 600.0));
  double r = std::min(std::max(0.05 * len, 0.05), 5.0);
  std::vector<gp_Pnt> pts;
  std::vector<gp_Vec> tan;
  std::vector<double> prm;  // the path's own parameter at each station
  for (int i = 0; i <= m; ++i) {
    GCPnts_AbscissaPoint ap(cc, len * i / m, cc.FirstParameter());
    gp_Pnt p;
    gp_Vec d;
    cc.D1(ap.Parameter(), p, d);
    if (d.Magnitude() < 1e-12) throw Standard_Failure("the path has a zero-length tangent");
    d.Normalize();
    pts.push_back(p);
    tan.push_back(d);
    prm.push_back(ap.Parameter());
  }
  gp_Vec hint(pf.x);
  gp_Vec e1 = hint - tan[0] * hint.Dot(tan[0]);
  if (e1.Magnitude() < 1e-9) e1 = gp_Vec(pf.y) - tan[0] * gp_Vec(pf.y).Dot(tan[0]);
  e1.Normalize();
  occ::handle<NCollection_HArray1<gp_Pnt>> hp = new NCollection_HArray1<gp_Pnt>(1, m + 1);
  for (int i = 0; i <= m; ++i) {
    if (i > 0) {
      gp_Vec a = tan[i - 1], b = tan[i];
      gp_Vec ax = a.Crossed(b);
      double sn = ax.Magnitude(), cs = a.Dot(b);
      if (sn > 1e-14) {
        ax /= sn;
        gp_Trsf tr;
        tr.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(ax)), std::atan2(sn, cs));
        e1.Transform(tr);
      }
      e1 = e1 - tan[i] * e1.Dot(tan[i]);
      e1.Normalize();
    }
    gp_Vec e2 = tan[i].Crossed(e1);
    double ph = twist * i / m;
    gp_Vec o = e1 * (r * std::cos(ph)) + e2 * (r * std::sin(ph));
    hp->SetValue(i + 1, gp_Pnt(pts[i].X() + o.X(), pts[i].Y() + o.Y(), pts[i].Z() + o.Z()));
  }
  // The guide shares the path's parameter, so the sweep pairs them station by station.
  occ::handle<NCollection_HArray1<double>> hq = new NCollection_HArray1<double>(1, m + 1);
  for (int i = 0; i <= m; ++i) hq->SetValue(i + 1, prm[i]);
  GeomAPI_Interpolate ip(hp, hq, false, 1e-9);
  ip.Perform();
  if (!ip.IsDone()) throw Standard_Failure("could not build the twist guide of the sweep");
  return BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(ip.Curve())).Wire();
}

const char* pipe_error_text(BRepBuilderAPI_PipeError e) {
  switch (e) {
    case BRepBuilderAPI_PlaneNotIntersectGuide: return "the profile plane does not cross the path";
    case BRepBuilderAPI_ImpossibleContact: return "the profile cannot be placed on the path";
    default: return "the sweep failed (the path may bend too sharply for the profile, or be too short)";
  }
}

struct Swept {
  TopoDS_Shape solid;
  std::vector<Rec> recs;
};

// Sweeps one closed profile wire along the spine into a solid.
Swept sweep_loop(const TopoDS_Wire& prof, const std::vector<TaggedEdge>& tagged, const Spine& sp, const Frame& pf,
                 int orientation, const TopoDS_Wire& guide, uint32_t region) {
  BRepOffsetAPI_MakePipeShell mk(sp.wire);
  if (!guide.IsNull()) {
    // Parametric (not curvilinear) correspondence: the guide carries the path's parameter,
    // and OCCT's curvilinear pairing loses a few tenths of a percent of volume on twists.
    mk.SetMode(guide, false, BRepFill_NoContact);
  } else if (orientation == 0) {
    mk.SetMode(gp_Ax2(pf.o, pf.n, pf.x));
  } else {
    mk.SetMode(false);
  }
  mk.SetTransitionMode(BRepBuilderAPI_RightCorner);
  mk.Add(prof, false, false);
  mk.Build();
  if (!mk.IsDone()) throw Standard_Failure(pipe_error_text(mk.GetStatus()));
  if (!mk.MakeSolid()) throw Standard_Failure("the swept surface could not be closed into a solid");
  Swept out;
  out.solid = mk.Shape();
  double vol = signed_volume(out.solid);
  if (std::fabs(vol) < 1e-9) throw Standard_Failure("the sweep has no volume (does the path leave the profile plane?)");
  if (vol < 0) out.solid.Reverse();
  ShapeMap wire_edges;
  TopExp::MapShapes(prof, TopAbs_EDGE, wire_edges);
  for (const auto& te : tagged) {
    if (!wire_edges.Contains(te.e)) continue;
    int64_t j = 0;
    const ShapeList& gen = mk.Generated(te.e);
    for (ShapeList::Iterator it(gen); it.More(); it.Next(), ++j) {
      if (it.Value().ShapeType() == TopAbs_FACE) out.recs.push_back(Rec{it.Value(), GS_ORIGIN_SWEPT, int64_t(te.tag), j});
    }
  }
  if (!sp.closed) {
    TopoDS_Shape a = mk.FirstShape(), b = mk.LastShape();
    if (!a.IsNull() && a.ShapeType() == TopAbs_FACE) out.recs.push_back(Rec{a, GS_ORIGIN_CAP, 0, int64_t(region)});
    if (!b.IsNull() && b.ShapeType() == TopAbs_FACE) out.recs.push_back(Rec{b, GS_ORIGIN_CAP, 1, int64_t(region)});
  }
  return out;
}

// The holes of a swept profile are swept along the same spine as its outer loop, so their
// end sections lie in the outer sweep's end planes, inside its caps. The holed solid is then
// the outer sweep's side faces, the holes' side faces turned inside out, and the two caps
// with the holes' end wires as inner loops: no boolean (a gear's bore cut took 2.6-3.4 s,
// dogfooding F3-5). Returns false when anything does not fit; the caller then cuts.
bool cut_coaxial_holes(const Swept& body, const std::vector<Swept>& holes, TopoDS_Shape& result,
                       std::vector<Rec>& recs) {
  auto caps_of = [](const Swept& sw, TopoDS_Face (&caps)[2]) {
    bool have[2] = {false, false};
    for (const auto& r : sw.recs) {
      if (r.kind == GS_ORIGIN_CAP && (r.a == 0 || r.a == 1) && r.face.ShapeType() == TopAbs_FACE) {
        caps[r.a] = TopoDS::Face(r.face);
        have[r.a] = true;
      }
    }
    return have[0] && have[1];
  };
  auto plane_normal = [](const TopoDS_Face& f, gp_Dir& n) {
    occ::handle<Geom_Plane> pl = occ::down_cast<Geom_Plane>(BRep_Tool::Surface(f));
    if (pl.IsNull()) return false;
    n = pl->Axis().Direction();
    return true;
  };
  TopoDS_Face caps[2];
  if (holes.empty() || !caps_of(body, caps)) return false;
  std::vector<std::array<TopoDS_Face, 2>> hole_caps;
  for (const auto& h : holes) {
    TopoDS_Face hc[2];
    if (!caps_of(h, hc)) return false;
    hole_caps.push_back({hc[0], hc[1]});
  }
  TopoDS_Face new_caps[2];
  for (int k = 0; k < 2; ++k) {
    TopoDS_Face base = TopoDS::Face(caps[k].Oriented(TopAbs_FORWARD));
    gp_Dir n;
    if (!plane_normal(base, n)) return false;
    BRepBuilderAPI_MakeFace mf(BRep_Tool::Surface(base), BRepTools::OuterWire(base), true);
    for (const auto& hc : hole_caps) {
      TopoDS_Face hf = TopoDS::Face(hc[size_t(k)].Oriented(TopAbs_FORWARD));
      gp_Dir hn;
      if (!plane_normal(hf, hn)) return false;
      // The hole's own cap runs its loop counter-clockwise about its normal; as an inner
      // loop of the cap it must run clockwise about the cap's normal.
      TopoDS_Wire w = BRepTools::OuterWire(hf);
      mf.Add(n.Dot(hn) > 0 ? TopoDS::Wire(w.Reversed()) : w);
    }
    if (!mf.IsDone()) return false;
    new_caps[k] = mf.Face();
  }
  BRep_Builder b;
  TopoDS_Shell shell;
  b.MakeShell(shell);
  for (TopExp_Explorer ex(body.solid, TopAbs_FACE); ex.More(); ex.Next()) {
    const TopoDS_Shape& f = ex.Current();
    int k = f.IsSame(caps[0]) ? 0 : f.IsSame(caps[1]) ? 1 : -1;
    b.Add(shell, k < 0 ? f : new_caps[k].Oriented(f.Orientation()));
  }
  for (size_t hi = 0; hi < holes.size(); ++hi) {
    for (TopExp_Explorer ex(holes[hi].solid, TopAbs_FACE); ex.More(); ex.Next()) {
      const TopoDS_Shape& f = ex.Current();
      if (f.IsSame(hole_caps[hi][0]) || f.IsSame(hole_caps[hi][1])) continue;
      b.Add(shell, f.Reversed());
    }
  }
  shell.Closed(true);
  TopoDS_Solid solid;
  b.MakeSolid(solid);
  b.Add(solid, shell);
  try {
    check_closed(solid, "sweep");
  } catch (const Standard_Failure&) {
    return false;
  }
  double expect = signed_volume(body.solid);
  for (const auto& h : holes) expect -= std::fabs(signed_volume(h.solid));
  double got = signed_volume(solid);
  if (!(expect > 0) || std::fabs(got - expect) > 1e-6 * expect) return false;
  recs.clear();
  for (Rec r : body.recs) {
    if (r.face.IsSame(caps[0])) r.face = new_caps[0];
    if (r.face.IsSame(caps[1])) r.face = new_caps[1];
    recs.push_back(r);
  }
  for (const auto& h : holes) {
    for (const auto& r : h.recs) {
      if (r.kind != GS_ORIGIN_CAP) recs.push_back(r);
    }
  }
  result = solid;
  return true;
}

// The recs of faces that a boolean replaced or removed, carried over to the result.
void carry_recs(BRepAlgoAPI_BuilderAlgo& op, std::vector<Rec>& recs) {
  std::vector<Rec> out;
  for (const auto& r : recs) {
    if (op.IsDeleted(r.face)) continue;
    const ShapeList& mods = op.Modified(r.face);
    if (mods.IsEmpty()) {
      out.push_back(r);
      continue;
    }
    for (ShapeList::Iterator it(mods); it.More(); it.Next()) out.push_back(Rec{it.Value(), r.kind, r.a, r.b});
  }
  recs = out;
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
  check_closed(result, "extrusion");
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

int gs_sweep(const GsProfile* p, const GsProfile* path_curves, const GsShape* path_shape, const uint32_t* path_edges,
             size_t npath_edges, int orientation, double twist, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !out || !origins) return fail("null argument");
  if (!path_curves && !path_shape) return fail("the sweep has no path");
  if (!std::isfinite(twist)) return fail("sweep twist must be a finite angle");
  if (std::fabs(twist) > 100.0 * 2.0 * M_PI) return fail("sweep twist is over 100 turns");
  Frame f = make_frame(p, 0.0);
  std::vector<TaggedEdge> tagged;
  std::vector<uint32_t> face_region;
  std::vector<TopoDS_Face> faces = build_faces(p, f, tagged, face_region);
  Spine sp = make_spine(path_curves, path_shape, path_edges, npath_edges, f);
  TopoDS_Wire guide;
  if (std::fabs(twist) > 1e-12) {
    if (sp.closed) return fail("a twist needs an open path (a closed path has no start and end to turn between)");
    guide = twist_guide(sp, f, twist);
  }

  std::vector<TopoDS_Shape> solids;
  std::vector<Rec> recs;
  for (size_t ri = 0; ri < faces.size(); ++ri) {
    TopoDS_Wire outer = BRepTools::OuterWire(faces[ri]);
    std::vector<TopoDS_Wire> holes;
    for (TopExp_Explorer ex(faces[ri], TopAbs_WIRE); ex.More(); ex.Next()) {
      if (!ex.Current().IsSame(outer)) holes.push_back(TopoDS::Wire(ex.Current()));
    }
    Swept body = sweep_loop(outer, tagged, sp, f, orientation, guide, face_region[ri]);
    if (holes.empty()) {
      solids.push_back(body.solid);
      recs.insert(recs.end(), body.recs.begin(), body.recs.end());
      continue;
    }
    // Holes: sweep each hole loop on its own and subtract it from the outer solid.
    ShapeList tools;
    std::vector<Rec> all = body.recs;
    std::vector<Swept> swept_holes;
    for (const auto& h : holes) {
      Swept hole = sweep_loop(h, tagged, sp, f, orientation, guide, face_region[ri]);
      tools.Append(hole.solid);
      all.insert(all.end(), hole.recs.begin(), hole.recs.end());
      swept_holes.push_back(hole);
    }
    TopoDS_Shape direct;
    std::vector<Rec> direct_recs;
    if (!sp.closed && cut_coaxial_holes(body, swept_holes, direct, direct_recs)) {
      solids.push_back(direct);
      recs.insert(recs.end(), direct_recs.begin(), direct_recs.end());
      continue;
    }
    ShapeList args;
    args.Append(body.solid);
    BRepAlgoAPI_Cut cut;
    cut.SetArguments(args);
    cut.SetTools(tools);
    cut.SetRunParallel(false);
    cut.Build();
    if (!cut.IsDone() || cut.HasErrors()) return fail("could not cut the profile's holes out of the sweep");
    cut.SimplifyResult(true, true);
    carry_recs(cut, all);
    solids.push_back(cut.Shape());
    recs.insert(recs.end(), all.begin(), all.end());
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
  emit_recs(recs, out_faces, o);
  check_valid(result, "sweep");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_loft(const GsProfile* sections, size_t nsections, int ruled, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!sections || !out || !origins) return fail("null argument");
  if (nsections < 2) return fail("a loft needs at least two profiles");
  BRepOffsetAPI_ThruSections ts(true, ruled != 0);
  std::vector<std::vector<TaggedEdge>> tags(nsections);
  std::vector<TopoDS_Wire> wires;
  std::vector<gp_Dir> normals;
  std::vector<gp_Pnt> centres;
  for (size_t k = 0; k < nsections; ++k) {
    const GsProfile* sp = sections + k;
    Frame f = make_frame(sp, 0.0);
    std::vector<uint32_t> face_region;
    std::vector<TopoDS_Face> faces = build_faces(sp, f, tags[k], face_region);
    if (faces.size() != 1 || sp->nloops != 1)
      return fail("each loft profile must be one outline without holes (pick a single region)");
    TopoDS_Wire w = BRepTools::OuterWire(faces[0]);
    GProp_GProps g;
    BRepGProp::LinearProperties(w, g);
    wires.push_back(w);
    normals.push_back(f.n);
    centres.push_back(g.CentreOfMass());
  }
  // Profiles are counter-clockwise about their own normals; make them run the same way
  // round the direction of the loft so the sections do not twist against each other.
  gp_Vec d(centres.front(), centres.back());
  if (d.Magnitude() > 1e-9) {
    bool first_fwd = gp_Vec(normals.front()).Dot(d) >= 0;
    for (size_t k = 1; k < nsections; ++k) {
      if ((gp_Vec(normals[k]).Dot(d) >= 0) != first_fwd) wires[k].Reverse();
    }
  }
  for (const auto& w : wires) ts.AddWire(w);
  ts.Build();
  if (!ts.IsDone()) return fail("the profiles cannot be lofted (are they similar, in a sensible order and not twisted?)");
  TopoDS_Shape result = ts.Shape();
  if (result.IsNull()) return fail("the loft produced no shape");
  double vol = signed_volume(result);
  if (std::fabs(vol) < 1e-9) return fail("the loft has no volume (are all the profiles on the same plane?)");
  if (vol < 0) result.Reverse();
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<Rec> recs;
  for (size_t k = 0; k < nsections; ++k) {
    ShapeMap wire_edges;
    TopExp::MapShapes(wires[k], TopAbs_EDGE, wire_edges);
    for (const auto& te : tags[k]) {
      if (!wire_edges.Contains(te.e)) continue;
      TopoDS_Shape gf = ts.GeneratedFace(te.e);
      if (!gf.IsNull()) {
        recs.push_back(Rec{gf, GS_ORIGIN_SECTION, int64_t(te.tag), int64_t(k)});
        continue;
      }
      const ShapeList& gen = ts.Generated(te.e);
      for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
        if (it.Value().ShapeType() == TopAbs_FACE) recs.push_back(Rec{it.Value(), GS_ORIGIN_SECTION, int64_t(te.tag), int64_t(k)});
      }
    }
  }
  const TopoDS_Shape& a = ts.FirstShape();
  const TopoDS_Shape& b = ts.LastShape();
  if (!a.IsNull()) recs.push_back(Rec{a, GS_ORIGIN_CAP, 0, 0});
  if (!b.IsNull()) recs.push_back(Rec{b, GS_ORIGIN_CAP, 1, 0});
  std::vector<int64_t> o;
  emit_recs(recs, out_faces, o);
  check_valid(result, "loft");
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

int gs_boolean_many(int op, const GsShape* target, const GsShape* const* tools, size_t ntools, GsShape** out,
                    GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!target || !out || !origins || (!tools && ntools)) return fail("null argument");
  if (ntools > size_t(INT_MAX) - 1) return fail("too many tools");
  for (size_t i = 0; i < ntools; ++i) {
    if (!tools[i]) return fail("null argument");
  }
  // One operation with all tools (review H3): the general fuse intersects every pair once,
  // instead of re-intersecting a growing result once per tool.
  ShapeList args, tool_list;
  args.Append(target->s);
  for (size_t i = 0; i < ntools; ++i) tool_list.Append(tools[i]->s);
  BRepAlgoAPI_BooleanOperation* b = nullptr;
  BRepAlgoAPI_Fuse fuse;
  BRepAlgoAPI_Cut cut;
  switch (op) {
    case GS_BOOL_UNION: b = &fuse; break;
    case GS_BOOL_CUT: b = &cut; break;
    default: return fail("only union and cut take several tools at once");
  }
  b->SetArguments(args);
  b->SetTools(tool_list);
  // Sequential on purpose: model-defining operations must give the same topology order
  // on every run and machine (spec "Determinism").
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
  for (size_t i = 0; i < ntools; ++i) record_input_faces(*b, tools[i]->s, int64_t(i) + 1, out_faces, o);
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
    if (in_faces.Extent() <= 0 || faces[i] >= uint32_t(in_faces.Extent())) return fail("face index out of range");
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

int gs_draft(const GsShape* s, const uint32_t* faces, size_t nfaces, const double params[7], GsShape** out,
             GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || !params || (!faces && nfaces)) return fail("null argument");
  if (nfaces == 0) return fail("no faces to draft");
  for (int i = 0; i < 7; ++i)
    if (!std::isfinite(params[i])) return fail("draft has non-finite parameters");
  gp_Vec pull(params[0], params[1], params[2]);
  if (pull.Magnitude() < 1e-12) return fail("the pull direction is zero");
  const double angle = params[6];
  if (!(std::fabs(angle) > 1e-9 && std::fabs(angle) < M_PI / 2 - 1e-9))
    return fail("the draft angle must be between 0 and 90 degrees");
  const gp_Dir dir(pull);
  const gp_Pln neutral(gp_Pnt(params[3], params[4], params[5]), dir);
  ShapeMap in_faces;
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  BRepOffsetAPI_DraftAngle mk(s->s);
  for (size_t i = 0; i < nfaces; ++i) {
    if (in_faces.Extent() <= 0 || faces[i] >= uint32_t(in_faces.Extent())) return fail("face index out of range");
    // For OCCT a positive angle leans an outward face inward along the pull direction
    // (the part narrows towards it), the convention of DraftSpec (golden test).
    mk.Add(TopoDS::Face(in_faces(int(faces[i]) + 1)), dir, angle, neutral);
    if (!mk.AddDone())
      throw Standard_Failure(("face " + std::to_string(faces[i]) +
                              " cannot be drafted: it must be planar, cylindrical or conical and cross the "
                              "neutral plane")
                                 .c_str());
  }
  mk.Build();
  if (!mk.IsDone()) return fail("draft failed");
  TopoDS_Shape result = mk.Shape();
  if (result.IsNull()) return fail("draft produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  // A modification keeps one face per face; DraftAngle reports the drafted ones through
  // ModifiedShape, not Modified.
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    int idx = out_faces.FindIndex(mk.ModifiedShape(in_faces(i)));
    if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
  }
  check_valid(result, "draft");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_offset_faces(const GsShape* s, const uint32_t* faces, size_t nfaces, double distance, GsShape** out,
                    GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || (!faces && nfaces)) return fail("null argument");
  if (nfaces == 0) return fail("no faces to offset");
  if (!std::isfinite(distance) || std::fabs(distance) < 1e-9) return fail("the offset distance must be non-zero");
  ShapeMap in_faces;
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  // Base offset 0 with an offset on the chosen faces only: the others stay, their
  // neighbours stretch (BRepOffset's per-face offsets).
  BRepOffset_MakeOffset mk;
  mk.Initialize(s->s, 0.0, 1e-7, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Intersection);
  for (size_t i = 0; i < nfaces; ++i) {
    if (in_faces.Extent() <= 0 || faces[i] >= uint32_t(in_faces.Extent())) return fail("face index out of range");
    mk.SetOffsetOnFace(TopoDS::Face(in_faces(int(faces[i]) + 1)), distance);
  }
  mk.MakeOffsetShape();
  if (!mk.IsDone()) return fail("the faces cannot be offset that far (the part would fold over)");
  TopoDS_Shape result = mk.Shape();
  if (result.IsNull()) return fail("offset produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  // Modified() is empty for this algorithm; its face image maps every input face.
  const BRepAlgo_Image& image = mk.OffsetFacesFromShapes();
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    const TopoDS_Shape& g = in_faces(i);
    if (!image.HasImage(g)) continue;
    for (ShapeList::Iterator it(image.Image(g)); it.More(); it.Next()) {
      int idx = out_faces.FindIndex(it.Value());
      if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
    }
  }
  check_valid(result, "offset");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

namespace {

// The rigid motion (or mirror) of a GS_XF_* kind; an error message when it has none.
const char* make_trsf(int kind, const double params[7], gp_Trsf& t) {
  for (int i = 0; i < 7; ++i)
    if (!std::isfinite(params[i])) return "transform has non-finite parameters";
  switch (kind) {
    case GS_XF_MIRROR: {
      if (std::hypot(params[3], std::hypot(params[4], params[5])) < 1e-12) return "mirror plane has no normal";
      t.SetMirror(gp_Ax2(gp_Pnt(params[0], params[1], params[2]), gp_Dir(params[3], params[4], params[5])));
      return nullptr;
    }
    case GS_XF_TRANSLATE: t.SetTranslation(gp_Vec(params[0], params[1], params[2])); return nullptr;
    case GS_XF_ROTATE: {
      if (std::hypot(params[3], std::hypot(params[4], params[5])) < 1e-12) return "rotation axis has no direction";
      t.SetRotation(gp_Ax1(gp_Pnt(params[0], params[1], params[2]), gp_Dir(params[3], params[4], params[5])),
                    params[6]);
      return nullptr;
    }
    default: return "unknown transform";
  }
}

// A copy of the edge's 3D curve in world coordinates, moved by `t` (same parameters).
occ::handle<Geom_Curve> moved_curve(const TopoDS_Edge& e, const gp_Trsf& t, double& f, double& l) {
  occ::handle<Geom_Curve> c = BRep_Tool::Curve(e, f, l);
  if (c.IsNull()) return c;
  return occ::down_cast<Geom_Curve>(c->Transformed(t));
}

// Moves some faces of a solid rigidly (ADR 0031). The other faces keep their surfaces and
// the other edges their curves, so they must contain what moves onto them (checked by
// check_followers first): a hole slides along the plane it sits in, a side face moves
// along its normal while its neighbours stretch.
class MoveFacesModification : public BRepTools_Modification {
 public:
  MoveFacesModification(const gp_Trsf& t, const ShapeMap& faces, const ShapeMap& edges, const ShapeMap& vertices)
      : t_(t), trsf_(new BRepTools_TrsfModification(t)), faces_(faces), edges_(edges), vertices_(vertices) {}

  bool NewSurface(const TopoDS_Face& F, occ::handle<Geom_Surface>& S, TopLoc_Location& L, double& Tol,
                  bool& RevWires, bool& RevFace) override {
    return faces_.Contains(F) && trsf_->NewSurface(F, S, L, Tol, RevWires, RevFace);
  }
  bool NewCurve(const TopoDS_Edge& E, occ::handle<Geom_Curve>& C, TopLoc_Location& L, double& Tol) override {
    return edges_.Contains(E) && trsf_->NewCurve(E, C, L, Tol);
  }
  bool NewPoint(const TopoDS_Vertex& V, gp_Pnt& P, double& Tol) override {
    return vertices_.Contains(V) && trsf_->NewPoint(V, P, Tol);
  }
  bool NewCurve2d(const TopoDS_Edge& E, const TopoDS_Face& F, const TopoDS_Edge& NewE, const TopoDS_Face& NewF,
                  occ::handle<Geom2d_Curve>& C, double& Tol) override {
    if (faces_.Contains(F)) return trsf_->NewCurve2d(E, F, NewE, NewF, C, Tol);
    if (!edges_.Contains(E)) return false;
    // A moved edge on a face that stays: the moved curve's image on that face's surface.
    double f = 0, l = 0;
    occ::handle<Geom_Curve> c = moved_curve(E, t_, f, l);
    TopLoc_Location loc;
    occ::handle<Geom_Surface> surf = BRep_Tool::Surface(F, loc);
    if (c.IsNull() || surf.IsNull()) return false;
    if (!loc.IsIdentity()) c = occ::down_cast<Geom_Curve>(c->Transformed(loc.Transformation().Inverted()));
    Tol = BRep_Tool::Tolerance(E);
    double ptol = Tol;
    C = GeomProjLib::Curve2d(c, f, l, surf, ptol);
    if (C.IsNull()) throw Standard_Failure("a moved edge cannot be placed on the face next to it");
    Tol = std::max(Tol, ptol);
    return true;
  }
  bool NewParameter(const TopoDS_Vertex& V, const TopoDS_Edge& E, double& P, double& Tol) override {
    if (edges_.Contains(E)) return trsf_->NewParameter(V, E, P, Tol);
    if (!vertices_.Contains(V)) return false;
    // An edge that stays, with a moved end: where the moved point lies on its curve.
    TopLoc_Location loc;
    double f = 0, l = 0;
    occ::handle<Geom_Curve> c = BRep_Tool::Curve(E, loc, f, l);
    if (c.IsNull()) throw Standard_Failure("a moved vertex ends an edge without a curve");
    gp_Pnt p = BRep_Tool::Pnt(V).Transformed(t_);
    if (!loc.IsIdentity()) p.Transform(loc.Transformation().Inverted());
    Tol = BRep_Tool::Tolerance(V);
    double u = 0;
    if (!GeomLib_Tool::Parameter(c, p, std::max(Tol, 1e-6), u))
      throw Standard_Failure("a moved vertex leaves the edge next to it");
    if (c->IsPeriodic()) {
      const double old = BRep_Tool::Parameter(V, E);
      const double period = c->Period();
      u += period * std::round((old - u) / period);
    }
    P = u;
    return true;
  }
  GeomAbs_Shape Continuity(const TopoDS_Edge& E, const TopoDS_Face& F1, const TopoDS_Face& F2,
                           const TopoDS_Edge& NewE, const TopoDS_Face& NewF1, const TopoDS_Face& NewF2) override {
    return trsf_->Continuity(E, F1, F2, NewE, NewF1, NewF2);
  }

 private:
  gp_Trsf t_;
  occ::handle<BRepTools_TrsfModification> trsf_;
  const ShapeMap& faces_;
  const ShapeMap& edges_;
  const ShapeMap& vertices_;
};

// Throws unless every face that stays contains the moved edges on it and every edge that
// stays contains its moved ends; otherwise the faces around would have to change shape.
void check_followers(const TopoDS_Shape& s, const gp_Trsf& t, const ShapeMap& faces, const ShapeMap& edges,
                     const ShapeMap& vertices) {
  const char* hint =
      "the faces next to the moved ones would have to change shape: move them too, or move along the "
      "faces around (a hole along the plane it sits in), or use offset_face or draft";
  AncMap edge_faces;
  TopExp::MapShapesAndAncestors(s, TopAbs_EDGE, TopAbs_FACE, edge_faces);
  for (int i = 1; i <= edges.Extent(); ++i) {
    const TopoDS_Edge& e = TopoDS::Edge(edges(i));
    if (BRep_Tool::Degenerated(e)) continue;
    double f = 0, l = 0;
    occ::handle<Geom_Curve> c = moved_curve(e, t, f, l);
    if (c.IsNull()) throw Standard_Failure("a moved edge has no curve");
    const double tol = std::max(BRep_Tool::Tolerance(e), 1e-6);
    for (ShapeList::Iterator it(edge_faces.FindFromKey(e)); it.More(); it.Next()) {
      const TopoDS_Face& face = TopoDS::Face(it.Value());
      if (faces.Contains(face)) continue;
      occ::handle<Geom_Surface> surf = BRep_Tool::Surface(face);
      for (int k = 0; k <= 4; ++k) {
        double u = 0, v = 0;
        if (!GeomLib_Tool::Parameters(surf, c->Value(f + (l - f) * k / 4.0), tol, u, v))
          throw Standard_Failure(hint);
      }
    }
  }
  AncMap vertex_edges;
  TopExp::MapShapesAndAncestors(s, TopAbs_VERTEX, TopAbs_EDGE, vertex_edges);
  for (int i = 1; i <= vertices.Extent(); ++i) {
    const TopoDS_Vertex& v = TopoDS::Vertex(vertices(i));
    const gp_Pnt p = BRep_Tool::Pnt(v).Transformed(t);
    for (ShapeList::Iterator it(vertex_edges.FindFromKey(v)); it.More(); it.Next()) {
      const TopoDS_Edge& e = TopoDS::Edge(it.Value());
      if (edges.Contains(e)) continue;
      TopoDS_Vertex a, b;
      TopExp::Vertices(e, a, b);
      if (BRep_Tool::Degenerated(e) || a.IsSame(b)) throw Standard_Failure(hint);
      double f = 0, l = 0;
      occ::handle<Geom_Curve> c = BRep_Tool::Curve(e, f, l);
      double u = 0;
      if (c.IsNull() || !GeomLib_Tool::Parameter(c, p, std::max(BRep_Tool::Tolerance(e), 1e-6), u))
        throw Standard_Failure(hint);
    }
  }
}

}  // namespace

int gs_move_faces(const GsShape* s, const uint32_t* faces, size_t nfaces, const int* kinds, const double* params,
                  size_t nsteps, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || (!faces && nfaces) || (!kinds && nsteps) || (!params && nsteps))
    return fail("null argument");
  if (nfaces == 0) return fail("no faces to move");
  if (nsteps == 0) return fail("the move does nothing");
  // Steps apply in order: the composed motion is t_n * ... * t_1.
  gp_Trsf t;
  for (size_t i = 0; i < nsteps; ++i) {
    if (kinds[i] == GS_XF_MIRROR) return fail("faces can only be moved and turned, not mirrored");
    gp_Trsf step;
    if (const char* err = make_trsf(kinds[i], params + 7 * i, step)) return fail(err);
    t.PreMultiply(step);
  }
  bool moves = false;
  for (const gp_Pnt& p : {gp_Pnt(0, 0, 0), gp_Pnt(1000, 0, 0), gp_Pnt(0, 1000, 0), gp_Pnt(0, 0, 1000)})
    moves = moves || p.Distance(p.Transformed(t)) > 1e-9;
  if (!moves) return fail("the move does nothing");
  ShapeMap in_faces;
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  ShapeMap moved, edges, vertices;
  for (size_t i = 0; i < nfaces; ++i) {
    if (in_faces.Extent() <= 0 || faces[i] >= uint32_t(in_faces.Extent())) return fail("face index out of range");
    const TopoDS_Shape& f = in_faces(int(faces[i]) + 1);
    moved.Add(f);
    TopExp::MapShapes(f, TopAbs_EDGE, edges);
    TopExp::MapShapes(f, TopAbs_VERTEX, vertices);
  }
  if (moved.Extent() == in_faces.Extent()) return fail("every face would move: use move on the body");
  check_followers(s->s, t, moved, edges, vertices);
  occ::handle<MoveFacesModification> m = new MoveFacesModification(t, moved, edges, vertices);
  BRepTools_Modifier mod(s->s, m);
  if (!mod.IsDone()) return fail("move face failed");
  TopoDS_Shape result = mod.ModifiedShape(s->s);
  if (result.IsNull()) return fail("move face produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    int idx = out_faces.FindIndex(mod.ModifiedShape(in_faces(i)));
    if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
  }
  check_valid(result, "move face");
  // A moved face can pass through another one, which the face checks do not see.
  BRepAlgoAPI_Check self(result, false, true);
  if (!self.IsValid()) return fail("the moved faces would cut through the rest of the part");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_transform(int kind, const double params[7], const GsShape* s, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!params || !s || !out || !origins) return fail("null argument");
  gp_Trsf t;
  if (const char* err = make_trsf(kind, params, t)) return fail(err);
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
  // A rigid motion (move, rotate, mirror) of a valid shape is valid: no re-check (it took
  // as long as the operation that made the shape, F3-5).
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

// Kind, area, centroid and outward normal (at the parameter-space middle) of a face.
// Axis of a revolved face (cylinder, cone, torus) or a sphere's center and polar axis:
// out = [has, ox, oy, oz, dx, dy, dz] (joint frames, ADR 0026).
void face_axis(const TopoDS_Face& f, double* out) {
  BRepAdaptor_Surface a(f);
  gp_Ax1 ax;
  bool has = true;
  switch (a.GetType()) {
    case GeomAbs_Cylinder: ax = a.Cylinder().Axis(); break;
    case GeomAbs_Cone: ax = a.Cone().Axis(); break;
    case GeomAbs_Sphere: ax = a.Sphere().Position().Axis(); break;
    case GeomAbs_Torus: ax = a.Torus().Axis(); break;
    default: has = false;
  }
  const gp_Pnt& o = ax.Location();
  const gp_Dir& d = ax.Direction();
  const double v[7] = {has ? 1.0 : 0.0, o.X(), o.Y(), o.Z(), d.X(), d.Y(), d.Z()};
  for (int i = 0; i < 7; ++i) out[i] = has ? v[i] : 0.0;
}

// Center, normal and radius of a circular edge: out = [has, cx, cy, cz, nx, ny, nz, r].
void edge_circle(const TopoDS_Edge& e, double* out) {
  for (int i = 0; i < 8; ++i) out[i] = 0.0;
  if (BRep_Tool::Degenerated(e)) return;
  BRepAdaptor_Curve c(e);
  if (c.GetType() != GeomAbs_Circle) return;
  gp_Circ ci = c.Circle();
  const gp_Pnt& o = ci.Location();
  const gp_Dir& n = ci.Axis().Direction();
  const double v[8] = {1.0, o.X(), o.Y(), o.Z(), n.X(), n.Y(), n.Z(), ci.Radius()};
  for (int i = 0; i < 8; ++i) out[i] = v[i];
}

void measure_face(const TopoDS_Face& f, double* out) {
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
  const double vals[8] = {double(surface_kind(f)), g.Mass(), c.X(), c.Y(), c.Z(), n.X(), n.Y(), n.Z()};
  std::copy(vals, vals + 8, out);
}

// Length and midpoint of an edge (a degenerated edge: zero length at its vertex).
void measure_edge(const TopoDS_Edge& e, double* out) {
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
  const double vals[4] = {length, mid.X(), mid.Y(), mid.Z()};
  std::copy(vals, vals + 4, out);
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
  // Per-face and per-edge measures are independent: computed on all cores, each into its
  // own slot, so the result is identical to a sequential pass (determinism, PROMPT.md).
  std::vector<double> fv(size_t(fm.Extent()) * 15);
  std::atomic<bool> failed{false};
  OSD_Parallel::For(1, fm.Extent() + 1, [&](int i) {
    try {
      measure_face(TopoDS::Face(fm(i)), fv.data() + size_t(i - 1) * 15);
      face_axis(TopoDS::Face(fm(i)), fv.data() + size_t(i - 1) * 15 + 8);
    } catch (...) {
      failed = true;
    }
  });
  if (failed) throw Standard_Failure("could not measure a face of the shape");
  std::vector<double> lens(size_t(em.Extent()) * 4);
  OSD_Parallel::For(1, em.Extent() + 1, [&](int i) {
    try {
      measure_edge(TopoDS::Edge(em(i)), lens.data() + size_t(i - 1) * 4);
    } catch (...) {
      failed = true;
    }
  });
  if (failed) throw Standard_Failure("could not measure an edge of the shape");
  AncMap anc;
  TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<double> ev;
  std::vector<int64_t> adj;
  for (int i = 1; i <= em.Extent(); ++i) {
    TopoDS_Edge e = TopoDS::Edge(em(i));
    const double* l = lens.data() + size_t(i - 1) * 4;
    ev.insert(ev.end(), {double(curve_kind(e)), l[0], l[1], l[2], l[3]});
    double circle[8];
    edge_circle(e, circle);
    ev.insert(ev.end(), circle, circle + 8);
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
  if (has_freeform_face(s->s)) {
    // Swept, lofted and spline faces: the default quadrature is off by a few tenths of a
    // percent on twisted surfaces, so integrate adaptively.
    BRepGProp::VolumeProperties(s->s, vol, 1e-7);
    BRepGProp::SurfaceProperties(s->s, area, 1e-7);
  } else {
    BRepGProp::VolumeProperties(s->s, vol);
    BRepGProp::SurfaceProperties(s->s, area);
  }
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
