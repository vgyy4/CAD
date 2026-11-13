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
#include <GCPnts_QuasiUniformDeflection.hxx>
#include <HLRAlgo_Projector.hxx>
#include <HLRAlgo_EdgeIterator.hxx>
#include <HLRBRep.hxx>
#include <HLRBRep_Algo.hxx>
#include <HLRBRep_Data.hxx>
#include <HLRBRep_FaceIterator.hxx>
#include <HLRBRep_HLRToShape.hxx>
#include <HLRBRep_PolyAlgo.hxx>
#include <HLRBRep_PolyHLRToShape.hxx>
#include <BRepBuilderAPI_TransitionMode.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepLib.hxx>
#include <Geom2d_Line.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <BRepTools.hxx>
#include <BRepTools_History.hxx>
#include <GeomAPI_ProjectPointOnSurf.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepAlgoAPI_Section.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <BRepExtrema_DistShapeShape.hxx>
#include <BRepGProp_Face.hxx>
#include <GeomAdaptor_Curve.hxx>
#include <GeomConvert_CurveToAnaCurve.hxx>
#include <ShapeAnalysis_FreeBounds.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <TopTools_HSequenceOfShape.hxx>
#include <GCPnts_AbscissaPoint.hxx>
#include <GeomAPI_Interpolate.hxx>
#include <GeomConvert_CompCurveToBSplineCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <NCollection_HArray1.hxx>
#include <TopExp_Explorer.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <memory>
#include <ShapeFix_Shape.hxx>
#include <ShapeFix_ShapeTolerance.hxx>
#include <ShapeBuild_ReShape.hxx>
#include <Precision.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Defeaturing.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepAlgoAPI_Splitter.hxx>
#include <BRepBndLib.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_GTransform.hxx>
#include <BRepBuilderAPI_ModifyShape.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepAlgo_Image.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffset_MakeOffset.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_MakeOffsetShape.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <ShapeFix_Solid.hxx>
#include <ShapeFix_Edge.hxx>
#include <ShapeFix_Wire.hxx>
#include <ShapeExtend.hxx>
#include <ShapeUpgrade_FaceDivide.hxx>
#include <ShapeUpgrade_ShapeDivideAngle.hxx>
#include <ShapeUpgrade_SplitSurfaceAngle.hxx>
#include <ShapeUpgrade_WireDivide.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <gp_Lin.hxx>
#include <GeomAbs_JoinType.hxx>
#include <gp_GTrsf.hxx>
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
#include <BRepCheck_Result.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <ChFi3d.hxx>
#include <BRepGProp.hxx>
#include <BRepLib_ToolTriangulatedShape.hxx>
#include <BRepLProp_SLProps.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepPrimAPI_MakeHalfSpace.hxx>
#include <BRepClass3d_SolidClassifier.hxx>
#include <ElCLib.hxx>
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
#include <algorithm>
#include <map>
#include <unordered_map>
#include <Quantity_Color.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Geom_BezierCurve.hxx>
#include <TColStd_Array1OfReal.hxx>
#include <gp_Elips.hxx>
#include <Geom_Ellipse.hxx>
#include <Geom_SurfaceOfLinearExtrusion.hxx>
#include <Geom_SurfaceOfRevolution.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <Standard_Version.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopAbs.hxx>
#include <TopExp.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <OSD_Parallel.hxx>
#include <array>
#include <Geom_Plane.hxx>
#include <atomic>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Iterator.hxx>
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
    } else if (s.kind == GS_SEG_ELLIPSE || s.kind == GS_SEG_ELLIPSE_ARC) {
      // Sampled by eccentric anomaly in the ellipse's own frame.
      double t0 = 0, sweep = 2.0 * M_PI;
      auto param = [&](const double* q) {
        double dx = q[0] - s.c[0], dy = q[1] - s.c[1];
        return std::atan2((s.major[0] * dy - s.major[1] * dx) / s.rb, (s.major[0] * dx + s.major[1] * dy) / s.r);
      };
      int n = 32;
      if (s.kind == GS_SEG_ELLIPSE_ARC) {
        t0 = param(s.a);
        sweep = param(s.b) - t0;
        if (s.ccw) {
          while (sweep <= 0) sweep += 2 * M_PI;
        } else {
          while (sweep >= 0) sweep -= 2 * M_PI;
        }
        n = 16;
      }
      for (int k = 0; k < n; ++k) {
        double t = t0 + sweep * k / n;
        double x = s.r * std::cos(t), y = s.rb * std::sin(t);
        pts.emplace_back(s.c[0] + s.major[0] * x - s.major[1] * y, s.c[1] + s.major[1] * x + s.major[0] * y);
      }
    } else if (s.kind == GS_SEG_BEZIER) {
      size_t n = s.pole_count;
      if (n >= 2 && n <= 26 && p->poles && size_t(s.pole_first) + n <= p->npoles) {
        const double* q = p->poles + 2 * size_t(s.pole_first);
        const double* w = p->weights ? p->weights + size_t(s.pole_first) : nullptr;
        // De Casteljau in homogeneous coordinates.
        double h[26][3];
        for (int j = 0; j < 12; ++j) {
          double t = j / 12.0;
          for (size_t i = 0; i < n; ++i) {
            double wi = w ? w[i] : 1.0;
            h[i][0] = q[2 * i] * wi;
            h[i][1] = q[2 * i + 1] * wi;
            h[i][2] = wi;
          }
          for (size_t level = 1; level < n; ++level)
            for (size_t i = 0; i + level < n; ++i)
              for (int c = 0; c < 3; ++c) h[i][c] = (1 - t) * h[i][c] + t * h[i + 1][c];
          pts.emplace_back(h[0][0] / h[0][2], h[0][1] / h[0][2]);
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
  const int n = int(s.pole_count);
  if (n < 2 || n > 26 || !p->poles || size_t(s.pole_first) + size_t(n) > p->npoles)
    throw Standard_Failure("Bezier segment has an invalid control polygon");
  TColgp_Array1OfPnt poles(1, n);
  TColStd_Array1OfReal weights(1, n);
  bool rational = false;
  for (int i = 0; i < n; ++i) {
    const double* q = p->poles + 2 * (size_t(s.pole_first) + size_t(i));
    poles.SetValue(i + 1, f.at(q[0], q[1]));
    const double w = p->weights ? p->weights[size_t(s.pole_first) + size_t(i)] : 1.0;
    if (!(w > 0) || !std::isfinite(w)) throw Standard_Failure("Bezier segment has a weight that is not positive");
    weights.SetValue(i + 1, w);
    rational = rational || w != 1.0;
  }
  if (rational) return new Geom_BezierCurve(poles, weights);
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
  } else if (s.kind == GS_SEG_ELLIPSE || s.kind == GS_SEG_ELLIPSE_ARC) {
    if (!(s.r > 0) || !(s.rb > 0) || s.rb > s.r * (1 + 1e-12))
      throw Standard_Failure("ellipse with invalid semi-axes in profile");
    double ml = std::hypot(s.major[0], s.major[1]);
    if (!(ml > 0)) throw Standard_Failure("ellipse without a major axis in profile");
    gp_Pnt c = f.at(s.c[0], s.c[1]);
    gp_Pnt tip = f.at(s.c[0] + s.major[0] / ml, s.c[1] + s.major[1] / ml);
    gp_Dir xdir(gp_Vec(c, tip));
    if (s.kind == GS_SEG_ELLIPSE) {
      gp_Elips el(gp_Ax2(c, f.n, xdir), s.r, s.rb);
      e = BRepBuilderAPI_MakeEdge(el).Edge();
    } else {
      gp_Dir axis = s.ccw ? f.n : f.n.Reversed();
      gp_Elips el(gp_Ax2(c, axis, xdir), s.r, s.rb);
      BRepBuilderAPI_MakeEdge me(el, f.at(s.a[0], s.a[1]), f.at(s.b[0], s.b[1]));
      if (!me.IsDone()) throw Standard_Failure("could not build an elliptical arc of the profile");
      e = me.Edge();
    }
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
  bool even = closed;
  for (int n : uses) {
    closed = closed && n == 2;
    even = even && n % 2 == 0;
  }
  // A loop that touches itself at a point (a notch whose corner meets the outline, 2348's board)
  // makes an edge four faces share: closed, but not two sides per edge. Such a shape is judged
  // by the full analyzer instead.
  if (!closed && even) {
    check_valid(s, what);
    return;
  }
  if (!closed) throw Standard_Failure((std::string(what) + " produced an open or invalid solid").c_str());
}

// Records Input/Modified history of every face of input `input_idx`.
template <typename Op>
void record_input_faces(Op& op, const TopoDS_Shape& input, int64_t input_idx, const ShapeMap& out_faces,
                        std::vector<int64_t>& origins,
                        const Handle(ShapeBuild_ReShape)& ctx = Handle(ShapeBuild_ReShape)()) {
  // `ctx` maps the operation's faces to the ones of a shape fixed afterwards.
  auto find = [&](const TopoDS_Shape& f) { return out_faces.FindIndex(ctx.IsNull() ? f : ctx->Apply(f)); };
  ShapeMap in_faces;
  TopExp::MapShapes(input, TopAbs_FACE, in_faces);
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    const TopoDS_Shape& g = in_faces(i);
    const ShapeList& mods = op.Modified(g);
    if (mods.IsEmpty()) {
      int idx = find(g);
      if (idx > 0) push_origin(origins, idx - 1, GS_ORIGIN_INPUT, input_idx, i - 1);
    } else {
      for (ShapeList::Iterator it(mods); it.More(); it.Next()) {
        int idx = find(it.Value());
        if (idx > 0) push_origin(origins, idx - 1, GS_ORIGIN_INPUT, input_idx, i - 1);
      }
    }
  }
}

template <typename Op>
void record_generated(Op& op, const TopoDS_Shape& input, TopAbs_ShapeEnum type, int64_t kind, int64_t input_idx,
                      const ShapeMap& out_faces, std::vector<int64_t>& origins,
                      const Handle(ShapeBuild_ReShape)& ctx = Handle(ShapeBuild_ReShape)()) {
  ShapeMap in_sub;
  TopExp::MapShapes(input, type, in_sub);
  for (int i = 1; i <= in_sub.Extent(); ++i) {
    const ShapeList& gen = op.Generated(in_sub(i));
    for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
      if (it.Value().ShapeType() != TopAbs_FACE) continue;
      int idx = out_faces.FindIndex(ctx.IsNull() ? it.Value() : ctx->Apply(it.Value()));
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

// Whether every fault BRepCheck finds in `made` lies in a part of it that `in` already had,
// unchanged (the same vertex, edge, wire or face): faults a blend inherits, not ones it made.
// 3350's threaded rod has helix edges a little off their faces (the thread's B-spline fits), and
// any blend of it, far from the thread, keeps those edges as they were.
bool faults_inherited(const TopoDS_Shape& made, const TopoDS_Shape& in) {
  BRepCheck_Analyzer an(made, true, true);
  if (an.IsValid()) return true;
  ShapeMap old;
  TopExp::MapShapes(in, old);
  for (TopAbs_ShapeEnum t : {TopAbs_VERTEX, TopAbs_EDGE, TopAbs_WIRE, TopAbs_FACE, TopAbs_SHELL, TopAbs_SOLID}) {
    for (TopExp_Explorer ex(made, t); ex.More(); ex.Next()) {
      Handle(BRepCheck_Result) r = an.Result(ex.Current());
      if (r.IsNull()) continue;
      bool ok = true;
      for (r->InitContextIterator(); ok && r->MoreShapeInContext(); r->NextShapeInContext())
        for (const BRepCheck_Status st : r->StatusOnShape())
          if (st != BRepCheck_NoError) ok = false;
      for (const BRepCheck_Status st : r->Status())
        if (st != BRepCheck_NoError) ok = false;
      if (!ok && (t == TopAbs_SHELL || t == TopAbs_SOLID || !old.Contains(ex.Current()))) return false;
    }
  }
  return true;
}

// A blend's shape, or its repair where BRepCheck rejects it (tolerances and small seams that
// ShapeFix mends, as FreeCAD's fillet does when its own validity test fails); `ctx` then maps
// the blend's faces to the repaired shape's. A shape whose only faults are ones the blended
// shape `in` already had is kept as it is ([`faults_inherited`]).
TopoDS_Shape valid_blend(const TopoDS_Shape& made, bool fillet, Handle(ShapeBuild_ReShape)& ctx,
                         const TopoDS_Shape& in) {
  TopoDS_Shape result = made;
  if (!BRepCheck_Analyzer(result, true, true).IsValid() &&
      (BRepCheck_Analyzer(in, true, true).IsValid() || !faults_inherited(made, in))) {
    ShapeFix_ShapeTolerance tol;
    tol.LimitTolerance(result, Precision::Confusion(), Precision::Confusion(), TopAbs_SHAPE);
    ShapeFix_Shape fix(result);
    fix.Perform();
    TopoDS_Shape fixed = fix.Shape();
    if (!fixed.IsNull() && BRepCheck_Analyzer(fixed, true, true).IsValid()) {
      result = fixed;
      ctx = fix.Context();
    } else {
      throw Standard_Failure((std::string(fillet ? "fillet" : "chamfer") + " produced an invalid solid").c_str());
    }
  }
  return result;
}

// The origins of a blend's faces from its history `h` (anything with Modified and Generated).
template <typename H>
void blend_origins(H& h, const TopoDS_Shape& in, const TopoDS_Shape& result, const Handle(ShapeBuild_ReShape)& ctx,
                   std::vector<int64_t>& o) {
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  record_input_faces(h, in, 0, out_faces, o, ctx);
  record_generated(h, in, TopAbs_EDGE, GS_ORIGIN_FROM_EDGE, 0, out_faces, o, ctx);
  record_generated(h, in, TopAbs_VERTEX, GS_ORIGIN_FROM_VERTEX, 0, out_faces, o, ctx);
}

// One edge of a blend: its size(s) and, for a chamfer with a reference face, that face.
struct BlendEdge {
  TopoDS_Edge e;
  TopoDS_Face ref;  // null: no reference face
  double a, b;      // fillet radius / chamfer distances (`a` on `ref`)
  int mode;         // 0: one size, 1: two distances, 2: distance `a` and angle `b`
};

void add_blend(BRepFilletAPI_MakeFillet& mk, const BlendEdge& x) { mk.Add(x.a, x.e); }
void add_blend(BRepFilletAPI_MakeChamfer& mk, const BlendEdge& x) {
  if (x.mode == 1) mk.Add(x.a, x.b, x.e, x.ref);
  else if (x.mode == 2) mk.AddDA(x.a, x.b, x.e, x.ref);
  else mk.Add(x.a, x.e);
}

// Do the edges touch (a common vertex) or bound a common face? Blends of edges that do
// neither are independent: made one after the other they give the shape made at once.
bool blends_independent(const TopoDS_Shape& s, const std::vector<BlendEdge>& es) {
  AncMap anc;
  TopExp::MapShapesAndAncestors(s, TopAbs_EDGE, TopAbs_FACE, anc);
  for (size_t i = 0; i < es.size(); ++i) {
    for (size_t j = i + 1; j < es.size(); ++j) {
      TopoDS_Vertex a0, a1, b0, b1;
      TopExp::Vertices(es[i].e, a0, a1);
      TopExp::Vertices(es[j].e, b0, b1);
      for (const TopoDS_Vertex& va : {a0, a1})
        for (const TopoDS_Vertex& vb : {b0, b1})
          if (!va.IsNull() && va.IsSame(vb)) return false;
      int ia = anc.FindIndex(es[i].e), ib = anc.FindIndex(es[j].e);
      if (ia <= 0 || ib <= 0) return false;
      for (ShapeList::Iterator x(anc(ia)); x.More(); x.Next())
        for (ShapeList::Iterator y(anc(ib)); y.More(); y.Next())
          if (x.Value().IsSame(y.Value())) return false;
    }
  }
  return true;
}

// The image of `x` after the passes so far: itself when untouched, its one modification,
// else null (removed or split).
TopoDS_Shape image_of(const BRepTools_History& h, const TopoDS_Shape& x) {
  if (x.IsNull() || h.IsRemoved(x)) return TopoDS_Shape();
  const ShapeList& m = h.Modified(x);
  if (m.IsEmpty()) return x;
  if (m.Extent() == 1) return m.First();
  return TopoDS_Shape();
}

// The edge of `s` with the ends and the middle of `e` (within 1e-7 mm), else null.
TopoDS_Shape same_edge(const TopoDS_Shape& s, const TopoDS_Edge& e) {
  auto key = [](const TopoDS_Edge& x, gp_Pnt& p0, gp_Pnt& p1, gp_Pnt& pm) {
    BRepAdaptor_Curve c(x);
    p0 = c.Value(c.FirstParameter());
    p1 = c.Value(c.LastParameter());
    pm = c.Value(0.5 * (c.FirstParameter() + c.LastParameter()));
  };
  gp_Pnt a0, a1, am;
  key(e, a0, a1, am);
  const double tol = 1e-7;
  TopoDS_Shape found;
  ShapeMap edges;
  TopExp::MapShapes(s, TopAbs_EDGE, edges);
  for (int i = 1; i <= edges.Extent(); ++i) {
    gp_Pnt b0, b1, bm;
    key(TopoDS::Edge(edges(i)), b0, b1, bm);
    bool ends = (a0.Distance(b0) < tol && a1.Distance(b1) < tol) || (a0.Distance(b1) < tol && a1.Distance(b0) < tol);
    if (!ends || am.Distance(bm) >= tol) continue;
    if (!found.IsNull()) return TopoDS_Shape();
    found = edges(i);
  }
  return found;
}

// A blend of independent edges made one edge at a time (OCCT fails on some sets that it
// blends edge by edge, e.g. the two bend edges of a bent bar). False when a pass fails;
// else `hist` holds the history from `in` to `result`.
template <typename Mk>
bool blend_in_passes(const TopoDS_Shape& in, const std::vector<BlendEdge>& es, TopoDS_Shape& result,
                     BRepTools_History& hist) {
  TopoDS_Shape cur = in;
  bool first = true;
  for (const auto& x0 : es) {
    BlendEdge x = x0;
    if (!first) {
      TopoDS_Shape e = image_of(hist, x0.e);
      if (e.IsNull()) {
        // A fillet rebuilds the edges around the faces it trims and calls the old ones
        // deleted: the edge is then the one of `cur` that lies where it lay.
        e = same_edge(cur, x0.e);
        if (e.IsNull()) return false;
        hist.ReplaceModified(x0.e, e);
      }
      if (e.ShapeType() != TopAbs_EDGE) return false;
      x.e = TopoDS::Edge(e);
      if (!x0.ref.IsNull()) {
        TopoDS_Shape r = image_of(hist, x0.ref);
        if (r.IsNull() || r.ShapeType() != TopAbs_FACE) return false;
        x.ref = TopoDS::Face(r);
      }
    }
    Mk mk(cur);
    add_blend(mk, x);
    mk.Build();
    if (!mk.IsDone()) return false;
    ShapeList args;
    args.Append(cur);
    if (first) hist = BRepTools_History(args, mk);
    else hist.Merge(args, mk);
    cur = mk.Shape();
    first = false;
  }
  result = cur;
  return !first;
}

// The edges of a fillet with the concave ones first, where it has both kinds; else empty.
// Fusion blends a concave and a convex edge of one face whose blends overlap (the face is
// narrower than both radii) as if the concave one came first: the convex blend then rolls on
// the concave blend's surface (3350's Fillet1: a 0.5 mm groove fillet and the rod's rim across a
// 0.942 mm step; its stored torus is the ball between the rod and the concave blend, as an edge
// by edge fillet in this order makes it, volume equal to 1e-8).
std::vector<BlendEdge> concave_first(const TopoDS_Shape& s, const std::vector<BlendEdge>& es) {
  AncMap anc;
  TopExp::MapShapesAndAncestors(s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<BlendEdge> concave, convex;
  for (const auto& x : es) {
    int i = anc.FindIndex(x.e);
    if (i <= 0 || anc(i).Extent() != 2) return {};
    const TopoDS_Face f1 = TopoDS::Face(anc(i).First()), f2 = TopoDS::Face(anc(i).Last());
    switch (ChFi3d::DefineConnectType(x.e, f1, f2, 1e-6, false)) {
      case ChFiDS_Concave: concave.push_back(x); break;
      case ChFiDS_Convex: convex.push_back(x); break;
      default: return {};
    }
  }
  if (concave.empty() || convex.empty()) return {};
  concave.insert(concave.end(), convex.begin(), convex.end());
  return concave;
}

// Defined below: the blend made by cutting away the corner of each edge.
int blend_by_corners(const GsShape* s, const std::vector<BlendEdge>& es, const std::vector<uint32_t>& idx, bool fillet,
                     GsShape** out, GsBuf* origins);
// Defined below: the blend of circles about one axis made on the solid of revolution of a
// half section and moved onto the shape as two booleans.
template <typename Mk>
int revolved_blend(const GsShape* s, const std::vector<BlendEdge>& es, const std::vector<uint32_t>& idx, bool fillet,
                   GsShape** out, GsBuf* origins);

// Blends the edges at once; where OCCT fails, independent edges one after another; failing
// that, the corners of convex edges between perpendicular faces cut away by tools whose
// removed volume is checked against the corners' own (`blend_by_corners`).
template <typename Mk>
int blend_edges(const GsShape* s, const std::vector<BlendEdge>& es, const std::vector<uint32_t>& idx, bool fillet,
                GsShape** out, GsBuf* origins) {
  std::string first_error = fillet ? "fillet failed" : "chamfer failed";
  // A fillet of concave and convex edges, the concave ones first (`concave_first`): where the
  // blend at once fails or is invalid (OCCT's answer to overlapping rounds, which ShapeFix can
  // turn into a valid solid of the wrong shape), before the repair.
  auto concave_passes = [&]() -> bool {
    if (!fillet || es.size() < 2) return false;
    try {
      const std::vector<BlendEdge> order = concave_first(s->s, es);
      TopoDS_Shape made;
      BRepTools_History hist;
      if (order.empty() || !blend_in_passes<Mk>(s->s, order, made, hist)) return false;
      Handle(ShapeBuild_ReShape) ctx;
      TopoDS_Shape result = valid_blend(made, fillet, ctx, s->s);
      std::vector<int64_t> o;
      blend_origins(hist, s->s, result, ctx, o);
      to_buf(o, origins);
      *out = new GsShape{result};
      return true;
    } catch (const Standard_Failure&) {
      return false;
    }
  };
  try {
    Mk mk(s->s);
    for (const auto& x : es) add_blend(mk, x);
    mk.Build();
    if (mk.IsDone()) {
      if (!BRepCheck_Analyzer(mk.Shape(), true, true).IsValid() && concave_passes()) return GS_OK;
      Handle(ShapeBuild_ReShape) ctx;
      TopoDS_Shape result = valid_blend(mk.Shape(), fillet, ctx, s->s);
      std::vector<int64_t> o;
      blend_origins(mk, s->s, result, ctx, o);
      to_buf(o, origins);
      *out = new GsShape{result};
      return GS_OK;
    }
  } catch (const Standard_Failure& e) {
    const char* w = e.what();
    if (w && *w) first_error = std::string("geometry kernel error: ") + w;
  }
  if (es.size() > 1 && blends_independent(s->s, es)) {
    try {
      TopoDS_Shape made;
      BRepTools_History hist;
      if (blend_in_passes<Mk>(s->s, es, made, hist)) {
        Handle(ShapeBuild_ReShape) ctx;
        TopoDS_Shape result = valid_blend(made, fillet, ctx, s->s);
        std::vector<int64_t> o;
        blend_origins(hist, s->s, result, ctx, o);
        to_buf(o, origins);
        *out = new GsShape{result};
        return GS_OK;
      }
    } catch (const Standard_Failure&) {
    }
  }
  if (concave_passes()) return GS_OK;
  try {
    if (blend_by_corners(s, es, idx, fillet, out, origins) == GS_OK) return GS_OK;
  } catch (const Standard_Failure&) {
  }
  try {
    if (revolved_blend<Mk>(s, es, idx, fillet, out, origins) == GS_OK) return GS_OK;
  } catch (const Standard_Failure&) {
  }
  return fail(first_error);
}

template <typename Mk>
int blend(const GsShape* s, const uint32_t* edges, size_t nedges, double size, bool fillet, GsShape** out,
          GsBuf* origins) {
  if (!s || !out || !origins || (!edges && nedges)) return fail("null argument");
  if (nedges == 0) return fail("no edges selected");
  if (!std::isfinite(size) || size <= 0) return fail(fillet ? "fillet radius must be positive" : "chamfer distance must be positive");
  std::vector<BlendEdge> es;
  std::vector<uint32_t> idx;
  for (size_t i = 0; i < nedges; ++i) {
    es.push_back(BlendEdge{edge_by_index(s->s, edges[i]), TopoDS_Face(), size, size, 0});
    idx.push_back(edges[i]);
  }
  return blend_edges<Mk>(s, es, idx, fillet, out, origins);
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

// Is there an extruded or revolved ellipse? Their faces are not analytic surfaces, and
// the default and the 1e-7 adaptive quadrature leave a relative error of 1e-5 .. 1e-8.
// A torus face trimmed by other faces (a revolved arc cut by a pad) is integrated with the
// default fixed quadrature to only about 1e-4 of the volume.
bool has_torus_face(const TopoDS_Shape& s) {
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
    if (BRepAdaptor_Surface(TopoDS::Face(ex.Current()), false).GetType() == GeomAbs_Torus) return true;
  }
  return false;
}

// Faces swept from an ellipse or from a rational curve (the spans of an exact B-spline
// sketch curve): the default adaptive tolerance leaves relative errors of 1e-7 to 1e-5.
bool has_elliptic_face(const TopoDS_Shape& s) {
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
    Handle(Geom_Surface) g = BRep_Tool::Surface(TopoDS::Face(ex.Current()));
    Handle(Geom_Curve) basis;
    if (Handle(Geom_SurfaceOfLinearExtrusion) e = Handle(Geom_SurfaceOfLinearExtrusion)::DownCast(g))
      basis = e->BasisCurve();
    else if (Handle(Geom_SurfaceOfRevolution) r = Handle(Geom_SurfaceOfRevolution)::DownCast(g))
      basis = r->BasisCurve();
    if (basis.IsNull()) continue;
    if (Handle(Geom_TrimmedCurve) t = Handle(Geom_TrimmedCurve)::DownCast(basis)) basis = t->BasisCurve();
    if (!Handle(Geom_Ellipse)::DownCast(basis).IsNull()) return true;
    Handle(Geom_BezierCurve) bz = Handle(Geom_BezierCurve)::DownCast(basis);
    if (!bz.IsNull() && bz->IsRational()) return true;
    Handle(Geom_BSplineCurve) bs = Handle(Geom_BSplineCurve)::DownCast(basis);
    if (!bs.IsNull() && bs->IsRational()) return true;
  }
  return false;
}

// A face that winds more than once around a periodic surface: a thread's root or crest, a
// strip of a cylinder between two helices of many turns (a Fusion modeled thread). The
// adaptive quadrature integrates its domain as if it were one period: 1145's root, nine turns,
// put the solid 1.1 mm3 off (4.5e-4), while the default quadrature (and Gauss-Kronrod, too slow
// on 5753's stored screws) agrees with the mesh to 2e-6. Only a cylinder or cone with one edge
// (a helix) going one and a half turns round it: a face's pcurve box alone can pass a turn.
bool is_wound_face(const TopoDS_Face& f) {
  BRepAdaptor_Surface sa(f, false);
  if (sa.GetType() != GeomAbs_Cylinder && sa.GetType() != GeomAbs_Cone) return false;
  for (TopExp_Explorer ee(f, TopAbs_EDGE); ee.More(); ee.Next()) {
    double u0 = 0, u1 = 0, v0 = 0, v1 = 0;
    BRepTools::UVBounds(f, TopoDS::Edge(ee.Current()), u0, u1, v0, v1);
    if (u1 - u0 > 3.0 * M_PI) return true;
  }
  return false;
}

bool has_wound_face(const TopoDS_Shape& s) {
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
    if (is_wound_face(TopoDS::Face(ex.Current()))) return true;
  }
  return false;
}

// Volume and area as `gs_mass` measures them. Torus and freeform faces are integrated
// adaptively (the default quadrature is off by a few tenths of a percent on twisted surfaces,
// by some 1e-7 on tori trimmed by planes), the rest by the default quadrature. A shape with a
// wound face is integrated face by face: the wound faces by the default quadrature, the others
// as above (the volume is a sum over faces, each face's part taken about the same origin: the
// faces go into two compounds). On the whole shape the default quadrature put 3350's stored nut,
// a revolved spline trimmed by the hexagon's sides beside a thread, 0.049 mm3 (1.4e-4) off.
// `area` may be null (the volume alone).
void measured_props(const TopoDS_Shape& s, GProp_GProps& vol, GProp_GProps* area) {
  if (!has_freeform_face(s) && !has_torus_face(s)) {
    BRepGProp::VolumeProperties(s, vol);
    if (area) BRepGProp::SurfaceProperties(s, *area);
    return;
  }
  const double eps = has_elliptic_face(s) ? 1e-10 : 1e-7;
  if (!has_wound_face(s)) {
    BRepGProp::VolumeProperties(s, vol, eps);
    if (area) BRepGProp::SurfaceProperties(s, *area, eps);
    return;
  }
  BRep_Builder b;
  TopoDS_Compound wound, rest;
  b.MakeCompound(wound);
  b.MakeCompound(rest);
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More(); ex.Next()) {
    const TopoDS_Face& f = TopoDS::Face(ex.Current());
    b.Add(is_wound_face(f) ? wound : rest, f);
  }
  GProp_GProps wv, rv;
  BRepGProp::VolumeProperties(wound, wv);
  BRepGProp::VolumeProperties(rest, rv, eps);
  wv.Add(rv);
  vol = wv;
  if (area) {
    GProp_GProps wa, ra;
    BRepGProp::SurfaceProperties(wound, wa);
    BRepGProp::SurfaceProperties(rest, ra, eps);
    wa.Add(ra);
    *area = wa;
  }
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

// The volume as `gs_mass` measures it (`measured_props`). The default quadrature is off by
// some 1e-7 of the volume on tori trimmed by planes, which is the tolerance the corner blends
// check their volumes to (3777's rim round, cut off by flats).
double measured_volume(const TopoDS_Shape& s) {
  GProp_GProps g;
  measured_props(s, g, nullptr);
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

// A helical sweep's side made by a profile line parallel to the axis lies on a cylinder about
// the axis (a helix is a straight line on the cylinder's parameter plane); MakePipeShell gives
// it as a B-spline approximation. Such a face is put on the exact cylinder, its edges kept and
// their pcurves projected anew, so a join with a coaxial cylinder of that radius makes one
// face of the two (3722's coil on its globe's base: the chamfers of the coil's ends run along
// that face). Kept only where the solid stays valid and of the same volume (within the
// sweep's own fit).
void exact_helix_cylinders(Swept& sw, const std::vector<TaggedEdge>& tagged, const gp_Ax1& axis) {
  const gp_Lin al(axis);
  const gp_Vec ad(axis.Direction());
  BRepBuilderAPI_Copy copy(sw.solid);
  TopoDS_Shape solid = copy.Shape();
  std::vector<Rec> recs = sw.recs;
  BRep_Builder b;
  bool changed = false;
  for (auto& r : recs) {
    if (copy.IsDone()) {
      const ShapeList& m = copy.Modified(r.face);
      if (!m.IsEmpty()) r.face = m.First();
    }
    if (r.kind != GS_ORIGIN_SWEPT || r.face.ShapeType() != TopAbs_FACE) continue;
    const TaggedEdge* te = nullptr;
    for (const auto& t : tagged) {
      if (int64_t(t.tag) == r.a) te = &t;
    }
    if (!te) continue;
    BRepAdaptor_Curve c(te->e);
    if (c.GetType() != GeomAbs_Line || !c.Line().Direction().IsParallel(axis.Direction(), 1e-9)) continue;
    const gp_Pnt p0 = c.Value(c.FirstParameter());
    const double rad = al.Distance(p0);
    if (rad < 1e-6) continue;
    TopoDS_Face f = TopoDS::Face(r.face);
    if (!f.Location().IsIdentity()) continue;
    TopLoc_Location old_loc;
    occ::handle<Geom_Surface> old = BRep_Tool::Surface(f, old_loc);
    if (old.IsNull() || !old_loc.IsIdentity()) continue;
    // The face must lie on the cylinder, and face the way its surface does.
    BRepAdaptor_Surface sa(f, false);
    bool on = true;
    for (int i = 0; i <= 4 && on; ++i) {
      for (int j = 0; j <= 4 && on; ++j) {
        const double u = sa.FirstUParameter() + (sa.LastUParameter() - sa.FirstUParameter()) * i / 4.0;
        const double v = sa.FirstVParameter() + (sa.LastVParameter() - sa.FirstVParameter()) * j / 4.0;
        on = std::fabs(al.Distance(sa.Value(u, v)) - rad) <= 1e-6 * (1.0 + rad);
      }
    }
    if (!on) continue;
    gp_Pnt pm;
    gp_Vec du, dv;
    sa.D1(0.5 * (sa.FirstUParameter() + sa.LastUParameter()), 0.5 * (sa.FirstVParameter() + sa.LastVParameter()), pm,
          du, dv);
    gp_Vec radial(al.Location(), pm);
    radial -= ad * radial.Dot(ad);
    gp_Vec start(al.Location(), p0);
    start -= ad * start.Dot(ad);
    gp_Ax3 frame(axis.Location(), axis.Direction(), gp_Dir(start));
    if (du.Crossed(dv).Dot(radial) < 0) frame.YReverse();
    occ::handle<Geom_CylindricalSurface> cyl = new Geom_CylindricalSurface(frame, rad);
    const double tol = BRep_Tool::Tolerance(f);
    b.UpdateFace(f, cyl, TopLoc_Location(), tol);
    for (TopExp_Explorer ee(f, TopAbs_EDGE); ee.More(); ee.Next()) {
      TopoDS_Edge e = TopoDS::Edge(ee.Current());
      b.UpdateEdge(e, occ::handle<Geom2d_Curve>(), old, old_loc, BRep_Tool::Tolerance(e));
      ShapeFix_Edge sfe;
      sfe.FixAddPCurve(e, f, false, 1e-7);
    }
    for (TopExp_Explorer we(f, TopAbs_WIRE); we.More(); we.Next()) {
      ShapeFix_Wire sfw(TopoDS::Wire(we.Current()), f, 1e-7);
      sfw.FixShifted();
    }
    for (TopExp_Explorer ee(f, TopAbs_EDGE); ee.More(); ee.Next()) {
      ShapeFix_Edge().FixSameParameter(TopoDS::Edge(ee.Current()));
    }
    changed = true;
  }
  if (!changed) return;
  // A face winding several turns round its cylinder is split into half turns: OCCT's booleans
  // drop a solid with one (a join lost the whole coil).
  ShapeUpgrade_ShapeDivideAngle divide(M_PI, solid);
  if (!divide.Perform()) return;
  solid = divide.Result();
  // A split face's pieces are what the reshape context gives for it. A face rebuilt round a
  // split edge (a cap beside the cylinder) is in neither the context nor the records: it is
  // matched by its surface, which it keeps.
  occ::handle<ShapeBuild_ReShape> ctx = divide.GetContext();
  std::vector<Rec> pieces;
  for (const auto& r : recs) {
    const TopoDS_Shape v = ctx.IsNull() ? r.face : ctx->Value(r.face);
    for (TopExp_Explorer ex(v, TopAbs_FACE); ex.More(); ex.Next()) pieces.push_back(Rec{ex.Current(), r.kind, r.a, r.b});
  }
  std::vector<Rec> split;
  for (TopExp_Explorer ex(solid, TopAbs_FACE); ex.More(); ex.Next()) {
    const Rec* found = nullptr;
    for (const auto& p : pieces) {
      if (p.face.IsSame(ex.Current())) found = &p;
    }
    if (!found) {
      TopLoc_Location loc;
      const occ::handle<Geom_Surface> surf = BRep_Tool::Surface(TopoDS::Face(ex.Current()), loc);
      for (const auto& r : recs) {
        if (r.face.ShapeType() != TopAbs_FACE) continue;
        TopLoc_Location rl;
        if (BRep_Tool::Surface(TopoDS::Face(r.face), rl) != surf || !(rl == loc)) continue;
        if (found && (found->kind != r.kind || found->a != r.a || found->b != r.b)) return;
        found = &r;
      }
    }
    if (!found) return;
    split.push_back(Rec{ex.Current(), found->kind, found->a, found->b});
  }
  recs = split;
  BRepLib::SameParameter(solid, 1e-6, true);
  if (!BRepCheck_Analyzer(solid).IsValid()) return;
  const double before = std::fabs(measured_volume(sw.solid)), after = std::fabs(measured_volume(solid));
  // The sweep's other faces are fitted (2e-6 of the volume measured, ADR 0034).
  if (std::fabs(after - before) > 1e-5 * before) return;
  sw.solid = solid;
  sw.recs = recs;
}

// Sweeps one closed profile wire along the spine into a solid.
Swept sweep_loop(const TopoDS_Wire& prof, const std::vector<TaggedEdge>& tagged, const Spine& sp, const Frame& pf,
                 int orientation, const TopoDS_Wire& guide, uint32_t region, const gp_Ax1* helix = nullptr) {
  BRepOffsetAPI_MakePipeShell mk(sp.wire);
  if (helix) {
    // A helix: the profile keeps its angle to the axis (ADR 0034), so a thread stays exact.
    mk.SetMode(helix->Direction());
  } else if (!guide.IsNull()) {
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
  if (helix) exact_helix_cylinders(out, tagged, *helix);
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

// Extrudes the profile from its plane until the face `face` of `target` (shifted by `offset`
// along the face normal toward the far side for planes), returning the swept tool solid.
// The prism is made longer than needed and split by the face's surface (a plane or cylinder
// extended past the face, any other surface as bounded); the piece holding the start cap is
// the result. The cut-off end must lie inside the face itself, or the call fails.
int gs_extrude_to_face(const GsProfile* p, const GsShape* target, uint32_t face, double offset, int reverse,
                       GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !target || !out || !origins) return fail("null argument");
  if (!std::isfinite(offset)) return fail("the offset must be finite");
  ShapeMap tfaces;
  TopExp::MapShapes(target->s, TopAbs_FACE, tfaces);
  if (face >= uint32_t(tfaces.Extent())) return fail("the face to extrude up to does not exist");
  TopoDS_Face tf = TopoDS::Face(tfaces(int(face) + 1));
  Frame f = make_frame(p, 0.0);
  const double sgn = reverse ? -1.0 : 1.0;
  gp_Dir dir(f.n.X() * sgn, f.n.Y() * sgn, f.n.Z() * sgn);
  std::vector<TaggedEdge> tagged;
  std::vector<uint32_t> face_region;
  std::vector<TopoDS_Face> faces = build_faces(p, f, tagged, face_region);

  Bnd_Box pb, ub;
  for (const auto& fc : faces) BRepBndLib::Add(fc, pb);
  ub = pb;
  BRepBndLib::Add(target->s, ub);
  if (pb.IsVoid() || ub.IsVoid()) return fail("the extrusion has no extent");
  auto corners = [](const Bnd_Box& b) {
    double x0, y0, z0, x1, y1, z1;
    b.Get(x0, y0, z0, x1, y1, z1);
    std::vector<gp_Pnt> c;
    for (int i = 0; i < 8; ++i) c.emplace_back((i & 1) ? x1 : x0, (i & 2) ? y1 : y0, (i & 4) ? z1 : z0);
    return c;
  };
  const std::vector<gp_Pnt> pcorners = corners(pb);
  const std::vector<gp_Pnt> ucorners = corners(ub);
  const double pdiag = std::sqrt(pb.SquareExtent());
  const double udiag = std::sqrt(ub.SquareExtent());

  BRepAdaptor_Surface sa(tf, true);
  TopoDS_Face tool_face, real_face = tf;
  double len = 0.0;
  if (sa.GetType() == GeomAbs_Plane) {
    gp_Pln pl = sa.Plane();
    gp_Dir pn = pl.Axis().Direction();
    double d = dir.Dot(pn);
    if (std::fabs(d) < 1e-9) return fail("the face is parallel to the extrusion direction, so the sweep never reaches it");
    gp_Vec shift(pn.X() * (d > 0 ? offset : -offset), pn.Y() * (d > 0 ? offset : -offset),
                 pn.Z() * (d > 0 ? offset : -offset));
    gp_Pnt o2 = pl.Location().Translated(shift);
    double tmin = 1e300, tmax = -1e300;
    for (const auto& c : pcorners) {
      double t = gp_Vec(c, o2).Dot(gp_Vec(pn)) / d;
      tmin = std::min(tmin, t);
      tmax = std::max(tmax, t);
    }
    if (tmax <= 1e-7) return fail("the face lies behind the sketch plane (reverse the direction)");
    if (tmin <= 1e-7) return fail("the face does not lie fully ahead of the sketch plane");
    len = tmax * 1.1 + 1.0;
    if (offset != 0.0) {
      gp_Trsf tr;
      tr.SetTranslation(shift);
      real_face = TopoDS::Face(BRepBuilderAPI_Transform(tf, tr, true).Shape());
    }
    gp_Pnt c0 = f.o;
    gp_Pnt proj = c0.Translated(gp_Vec(pn) * (gp_Vec(c0, o2).Dot(gp_Vec(pn))));
    double r = 2.0 * (tmax + pdiag) + 1.0;
    BRepBuilderAPI_MakeFace mf(gp_Pln(proj, pn), -r, r, -r, r);
    if (!mf.IsDone()) return fail("could not extend the face");
    tool_face = mf.Face();
  } else {
    if (std::fabs(offset) > 1e-12) return fail("an offset from the face needs a planar face");
    len = 2.0 * udiag + 1.0;
    occ::handle<Geom_Surface> surf = BRep_Tool::Surface(tf);
    double u0, u1, v0, v1;
    BRepTools::UVBounds(tf, u0, u1, v0, v1);
    if (sa.GetType() == GeomAbs_Cylinder) {
      gp_Cylinder cyl = sa.Cylinder();
      gp_Ax1 ax = cyl.Axis();
      double lo = 1e300, hi = -1e300;
      for (const auto& c : ucorners) {
        double v = gp_Vec(ax.Location(), c).Dot(gp_Vec(ax.Direction()));
        lo = std::min(lo, v);
        hi = std::max(hi, v);
      }
      occ::handle<Geom_CylindricalSurface> cs = new Geom_CylindricalSurface(cyl);
      BRepBuilderAPI_MakeFace mf(cs, 0.0, 2.0 * M_PI, lo - 1.0, hi + 1.0, 1e-7);
      if (!mf.IsDone()) return fail("could not extend the face");
      tool_face = mf.Face();
    } else {
      if (surf->IsUPeriodic()) {
        u0 = 0.0;
        u1 = surf->UPeriod();
      }
      if (surf->IsVPeriodic()) {
        v0 = 0.0;
        v1 = surf->VPeriod();
      }
      BRepBuilderAPI_MakeFace mf(surf, u0, u1, v0, v1, 1e-7);
      if (!mf.IsDone()) return fail("could not extend the face");
      tool_face = mf.Face();
    }
  }

  gp_Vec v(dir.X() * len, dir.Y() * len, dir.Z() * len);
  TopoDS_Compound comp;
  BRep_Builder bld;
  bld.MakeCompound(comp);
  std::vector<Rec> recs;
  TopoDS_Shape single;
  int nsolids = 0;
  for (size_t ri = 0; ri < faces.size(); ++ri) {
    BRepPrimAPI_MakePrism mk(faces[ri], v, true);
    mk.Build();
    if (!mk.IsDone()) return fail("extrusion failed");
    std::vector<Rec> rr;
    for (const auto& te : tagged) {
      const ShapeList& gen = mk.Generated(te.e);
      for (ShapeList::Iterator it(gen); it.More(); it.Next()) rr.push_back(Rec{it.Value(), GS_ORIGIN_LATERAL, int64_t(te.tag), 0});
    }
    TopoDS_Shape start = mk.FirstShape(faces[ri]);
    rr.push_back(Rec{start, GS_ORIGIN_CAP, 0, int64_t(face_region[ri])});

    ShapeList args, tools;
    args.Append(mk.Shape());
    tools.Append(tool_face);
    BRepAlgoAPI_Splitter sp;
    sp.SetArguments(args);
    sp.SetTools(tools);
    sp.Build();
    if (!sp.IsDone() || sp.HasErrors()) return fail("could not cut the extrusion at the face");
    ShapeList start_mods = sp.Modified(start);
    if (start_mods.IsEmpty()) start_mods.Append(start);
    TopoDS_Shape piece;
    for (TopExp_Explorer sx(sp.Shape(), TopAbs_SOLID); sx.More() && piece.IsNull(); sx.Next()) {
      ShapeMap pf;
      TopExp::MapShapes(sx.Current(), TopAbs_FACE, pf);
      for (ShapeList::Iterator it(start_mods); it.More(); it.Next())
        if (pf.Contains(it.Value())) piece = sx.Current();
    }
    if (piece.IsNull()) return fail("could not find the extrusion's start after cutting at the face");
    ShapeMap pf;
    TopExp::MapShapes(piece, TopAbs_FACE, pf);
    std::vector<TopoDS_Shape> caps;
    const ShapeList& tm = sp.Modified(tool_face);
    for (ShapeList::Iterator it(tm); it.More(); it.Next())
      if (pf.Contains(it.Value())) caps.push_back(it.Value());
    if (caps.empty()) return fail("the extrusion does not reach the face");
    // The end must lie inside the face: the cap and its overlap with the real face agree.
    for (const auto& cap : caps) {
      GProp_GProps g0, g1;
      BRepGProp::SurfaceProperties(cap, g0);
      double common_area = 0.0;
      BRepAlgoAPI_Common cm(cap, real_face);
      if (cm.IsDone() && !cm.HasErrors()) {
        BRepGProp::SurfaceProperties(cm.Shape(), g1);
        common_area = g1.Mass();
      }
      if (g0.Mass() - common_area > 1e-6 * std::max(1.0, g0.Mass()))
        return fail("the face does not cover the whole extrusion; pick a face that reaches past the profile");
    }
    carry_recs(sp, rr);
    for (const auto& r : rr)
      if (pf.Contains(r.face)) recs.push_back(r);
    for (const auto& cap : caps) recs.push_back(Rec{cap, GS_ORIGIN_CAP, 1, int64_t(face_region[ri])});
    bld.Add(comp, piece);
    single = piece;
    ++nsolids;
  }
  if (nsolids == 0) return fail("extrusion failed");
  TopoDS_Shape result = nsolids == 1 ? single : TopoDS_Shape(comp);
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  emit_recs(recs, out_faces, o);
  check_valid(result, "extrusion to a face");
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

// Sweeps the profile faces along `sp` into the result shape and its face origins.
int sweep_faces(const std::vector<TopoDS_Face>& faces, const std::vector<TaggedEdge>& tagged,
                const std::vector<uint32_t>& face_region, const Spine& sp, const Frame& f, int orientation,
                const TopoDS_Wire& guide, const gp_Ax1* helix, GsShape** out, GsBuf* origins) {
  std::vector<TopoDS_Shape> solids;
  std::vector<Rec> recs;
  for (size_t ri = 0; ri < faces.size(); ++ri) {
    TopoDS_Wire outer = BRepTools::OuterWire(faces[ri]);
    std::vector<TopoDS_Wire> holes;
    for (TopExp_Explorer ex(faces[ri], TopAbs_WIRE); ex.More(); ex.Next()) {
      if (!ex.Current().IsSame(outer)) holes.push_back(TopoDS::Wire(ex.Current()));
    }
    Swept body = sweep_loop(outer, tagged, sp, f, orientation, guide, face_region[ri], helix);
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
      Swept hole = sweep_loop(h, tagged, sp, f, orientation, guide, face_region[ri], helix);
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

  return sweep_faces(faces, tagged, face_region, sp, f, orientation, guide, nullptr, out, origins);
  GS_GUARD_END
}

// A helix about `h` = origin (3), axis (3), start direction (3), radius; `pitch` per turn
// (negative: left-handed), `height` along the axis (ADR 0034). An exact edge: a line on the
// cylinder's (angle, height) parameter plane.
int gs_sweep_helix(const GsProfile* p, const double* h, double pitch, double height, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !h || !out || !origins) return fail("null argument");
  for (int i = 0; i < 10; ++i) {
    if (!std::isfinite(h[i])) return fail("the helix has a non-finite value");
  }
  if (!std::isfinite(pitch) || !std::isfinite(height) || std::fabs(pitch) < 1e-6 || height <= 1e-6)
    return fail("a helix needs a non-zero pitch and a positive height");
  if (height / std::fabs(pitch) > 2000.0) return fail("the helix has more than 2000 turns");
  if (!(h[9] > 1e-6)) return fail("the helix radius must be positive (the profile sits on the axis)");
  gp_Pnt o(h[0], h[1], h[2]);
  gp_Vec av(h[3], h[4], h[5]), xv(h[6], h[7], h[8]);
  if (av.Magnitude() < 1e-12 || xv.Magnitude() < 1e-12) return fail("the helix axis or start direction is zero");
  gp_Dir axis(av);
  occ::handle<Geom_CylindricalSurface> cyl = new Geom_CylindricalSurface(gp_Ax3(o, axis, gp_Dir(xv)), h[9]);
  // Left-handed: the angle runs backwards; the helix always climbs the axis.
  gp_Dir2d d2(pitch < 0 ? -2.0 * M_PI : 2.0 * M_PI, std::fabs(pitch));
  double len = height / std::fabs(pitch) * std::sqrt(4.0 * M_PI * M_PI + pitch * pitch);
  occ::handle<Geom2d_Line> line = new Geom2d_Line(gp_Pnt2d(0.0, 0.0), d2);
  TopoDS_Edge e = BRepBuilderAPI_MakeEdge(line, cyl, 0.0, len).Edge();
  BRepLib::BuildCurves3d(e);
  Spine sp;
  sp.wire = BRepBuilderAPI_MakeWire(e).Wire();
  sp.edges.push_back(e);
  Frame f = make_frame(p, 0.0);
  std::vector<TaggedEdge> tagged;
  std::vector<uint32_t> face_region;
  std::vector<TopoDS_Face> faces = build_faces(p, f, tagged, face_region);
  const gp_Ax1 helix(o, axis);
  return sweep_faces(faces, tagged, face_region, sp, f, 0, TopoDS_Wire(), &helix, out, origins);
  GS_GUARD_END
}

// Whether every section is a uniformly scaled copy of the first about its own vertex
// centroid, vertex for vertex in wire order (e.g. a generated bevel gear's heel and toe).
// Such wires already correspond edge for edge, so ThruSections' compatibility pass, which
// re-splits tangent-continuous wires by polar projection in O(n^2) distance searches, can
// be skipped; any other loft keeps it.
static bool similar_wires(const std::vector<TopoDS_Wire>& ws) {
  if (ws.size() < 2) return false;
  auto verts = [](const TopoDS_Wire& w) {
    std::vector<gp_XYZ> v;
    for (BRepTools_WireExplorer ex(w); ex.More(); ex.Next()) v.push_back(BRep_Tool::Pnt(ex.CurrentVertex()).XYZ());
    return v;
  };
  auto centred = [](std::vector<gp_XYZ> v) {
    gp_XYZ c(0, 0, 0);
    for (const auto& p : v) c += p;
    c /= double(v.size());
    for (auto& p : v) p -= c;
    return v;
  };
  const std::vector<gp_XYZ> v0 = centred(verts(ws[0]));
  if (v0.size() < 3 || v0[0].Modulus() < 1e-9) return false;
  for (size_t k = 1; k < ws.size(); ++k) {
    const std::vector<gp_XYZ> vk = centred(verts(ws[k]));
    if (vk.size() != v0.size()) return false;
    const double s = vk[0].Modulus() / v0[0].Modulus();
    for (size_t i = 0; i < v0.size(); ++i) {
      if ((vk[i] - v0[i] * s).Modulus() > 1e-6 * (1.0 + v0[i].Modulus())) return false;
    }
  }
  return true;
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
  if (similar_wires(wires)) ts.CheckCompatibility(false);
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

// C++ helpers inside the extern "C" block: MSVC keeps C language linkage even in an anonymous
// namespace and refuses a C-linkage function that returns a C++ class (error C2526), which GCC
// and Clang accept; an explicit extern "C++" block gives them C++ linkage.
extern "C++" {
namespace {

// A face on a cylinder that runs on round past its seam between helical edges: a thread's root
// band (the core's cylinder split by the tooth), or a crest made exact by
// `exact_helix_cylinders` whose half turns a refined boolean merged back. OCCT's face builder
// cannot split such a face where another solid's edge crosses it (a partly threaded rod's
// shank: "unused edges" on the crest and root bands, and an empty fuse).
// Whether edge `e` of a cylinder face `f` is a helix on it: its curve on the face's parameter
// plane a straight line that turns at least a quarter round and climbs (an intersection with
// another cylinder or a plane slanting across, also neither line nor circle in space, is not:
// 3000's board, which no thread touches, changed volume when such faces were split).
bool helix_on(const TopoDS_Edge& e, const TopoDS_Face& f) {
  const GeomAbs_CurveType t = BRepAdaptor_Curve(e).GetType();
  if (t == GeomAbs_Line || t == GeomAbs_Circle) return false;
  double a = 0, b = 0;
  occ::handle<Geom2d_Curve> c = BRep_Tool::CurveOnSurface(e, f, a, b);
  if (c.IsNull()) return false;
  const gp_Pnt2d p0 = c->Value(a), p1 = c->Value(b);
  const double du = p1.X() - p0.X(), dv = p1.Y() - p0.Y();
  if (std::fabs(du) < 0.5 * M_PI || std::fabs(dv) < 1e-7) return false;
  const double len = std::hypot(du, dv);
  for (int k = 1; k < 8; ++k) {
    const double s = k / 8.0;
    const gp_Pnt2d q = c->Value(a + s * (b - a));
    // Distance from the chord, as a share of its length.
    const double off = std::fabs((q.X() - p0.X()) * dv - (q.Y() - p0.Y()) * du) / len;
    if (off > 1e-4 * len) return false;
  }
  return true;
}

bool wraps_between_helices(const TopoDS_Face& f) {
  if (BRepAdaptor_Surface(f, false).GetType() != GeomAbs_Cylinder) return false;
  bool seam = false, helix = false;
  for (TopExp_Explorer ex(f, TopAbs_EDGE); ex.More(); ex.Next()) {
    const TopoDS_Edge& e = TopoDS::Edge(ex.Current());
    if (BRep_Tool::Degenerated(e)) continue;
    seam = seam || BRep_Tool::IsClosed(e, f);
    helix = helix || helix_on(e, f);
  }
  return seam && helix;
}

// Splits only the faces `wraps_between_helices` accepts (into half turns); others stay whole.
class HelicalBandDivide : public ShapeUpgrade_FaceDivide {
 public:
  bool Perform(const double area = 0.) override {
    if (myFace.IsNull() || wraps_between_helices(myFace)) return ShapeUpgrade_FaceDivide::Perform(area);
    myStatus = ShapeExtend::EncodeStatus(ShapeExtend_OK);
    myResult = myFace;
    return false;
  }
};

// `s` with its helical bands (`wraps_between_helices`) split into half turns, and the split's
// history in `h`; `s` itself and a null `h` where it has none, or the split is not valid.
TopoDS_Shape split_helical_bands(const TopoDS_Shape& s, occ::handle<BRepTools_History>& h) {
  h.Nullify();
  bool any = false;
  for (TopExp_Explorer ex(s, TopAbs_FACE); ex.More() && !any; ex.Next())
    any = wraps_between_helices(TopoDS::Face(ex.Current()));
  if (!any) return s;
  ShapeUpgrade_ShapeDivideAngle divide(M_PI, s);
  occ::handle<HelicalBandDivide> tool = new HelicalBandDivide;
  tool->SetSplitSurfaceTool(new ShapeUpgrade_SplitSurfaceAngle(M_PI));
  tool->SetWireDivideTool(nullptr);
  divide.SetSplitFaceTool(tool);
  if (!divide.Perform() || divide.GetContext().IsNull()) return s;
  TopoDS_Shape out = divide.Result();
  BRepLib::SameParameter(out, 1e-6, true);
  if (!BRepCheck_Analyzer(out).IsValid()) return s;
  h = divide.GetContext()->History();
  return out;
}

// A boolean's history seen from its inputs before `split_helical_bands` (`split` null: not
// split).
struct AfterSplit {
  const occ::handle<BRepTools_History>& split;
  BRepAlgoAPI_BooleanOperation& op;
  ShapeList Modified(const TopoDS_Shape& g) const {
    ShapeList pieces, out;
    if (!split.IsNull() && split->IsSupportedType(g)) pieces = split->Modified(g);
    bool changed = !pieces.IsEmpty();
    if (pieces.IsEmpty()) pieces.Append(g);
    for (ShapeList::Iterator it(pieces); it.More(); it.Next()) {
      const ShapeList& m = op.Modified(it.Value());
      changed = changed || !m.IsEmpty();
      if (m.IsEmpty()) {
        if (!op.IsDeleted(it.Value())) out.Append(it.Value());
      } else {
        for (ShapeList::Iterator mt(m); mt.More(); mt.Next()) out.Append(mt.Value());
      }
    }
    return changed ? out : ShapeList();
  }
};

// The general boolean on the given arguments and tools. With `simplify` the result's
// coplanar faces and collinear edges are unified; unification can turn a valid result
// invalid (FreeCAD's examples: a pad touching a mirrored body), so callers retry without.
std::unique_ptr<BRepAlgoAPI_BooleanOperation> run_boolean(int op, const ShapeList& args, const ShapeList& tools,
                                                          bool simplify) {
  std::unique_ptr<BRepAlgoAPI_BooleanOperation> b;
  switch (op) {
    case GS_BOOL_UNION: b = std::make_unique<BRepAlgoAPI_Fuse>(); break;
    case GS_BOOL_CUT: b = std::make_unique<BRepAlgoAPI_Cut>(); break;
    case GS_BOOL_INTERSECT: b = std::make_unique<BRepAlgoAPI_Common>(); break;
    default: throw Standard_Failure("unknown boolean operation");
  }
  b->SetArguments(args);
  b->SetTools(tools);
  // Sequential on purpose: model-defining operations must give the same topology order
  // on every run and machine (spec "Determinism").
  b->SetRunParallel(false);
  b->Build();
  if (!b->IsDone() || b->HasErrors()) throw Standard_Failure("boolean operation failed");
  if (simplify) b->SimplifyResult(true, true);
  return b;
}

// The boolean with unification, or without it when unification breaks the result.
// With `refine` false the result is kept as the boolean made it (FreeCAD's Refine = false).
std::unique_ptr<BRepAlgoAPI_BooleanOperation> run_boolean_checked(int op, const ShapeList& args,
                                                                  const ShapeList& tools, bool refine = true) {
  if (refine) {
    auto unified = run_boolean(op, args, tools, true);
    const BRepCheck_Analyzer unified_check(unified->Shape(), Standard_True, Standard_True);
    if (unified_check.IsValid()) return unified;
  }
  auto unrefined = run_boolean(op, args, tools, false);
  const BRepCheck_Analyzer unrefined_check(unrefined->Shape(), Standard_True, Standard_True);
  if (unrefined_check.IsValid()) return unrefined;
  throw Standard_Failure("boolean operation produced an invalid solid");
}


// The outward normal of face `f` at its point nearest to `p`.
bool outward_normal(const TopoDS_Face& f, const gp_Pnt& p, gp_Dir& n) {
  GeomAPI_ProjectPointOnSurf proj(p, BRep_Tool::Surface(f));
  if (!proj.IsDone() || proj.NbPoints() < 1) return false;
  double u = 0, v = 0;
  proj.LowerDistanceParameters(u, v);
  BRepAdaptor_Surface sf(f, false);
  BRepLProp_SLProps props(sf, u, v, 1, 1e-9);
  if (!props.IsNormalDefined()) return false;
  n = props.Normal();
  if (f.Orientation() == TopAbs_REVERSED) n.Reverse();
  return true;
}

// The corner a blend takes off one edge, as a tool: `cutter` reaches past the faces (so the
// boolean meets no coincident boundary there), `ideal` is the corner alone, `face` the cutter's
// blend face.
struct Corner {
  TopoDS_Shape cutter;
  double ideal = 0;
  // The corner alone, as a solid (`ideal` is its volume).
  TopoDS_Shape exact;
  TopoDS_Shape face;
  // A concave edge: the blend fills the corner (the tool is added, not cut).
  bool concave = false;
  // A straight edge: its ends and direction (see `blend_by_corners`).
  bool line = false;
  gp_Pnt end0, end1;
  gp_Dir dir;
};

// The planes that end an arc's corner: at each end of the arc one more face meets it, a plane
// that crosses the arc (not along it) and has the material on the arc's side (3101: the top
// edges of a ring cut by four slots, a fillet whose radius is the ring's height). A blend
// that runs into such a face ends on it: the corner, revolved past the arc's ends, is trimmed
// by both planes. Each plane as a point and its outward normal; `tan_max` is the largest
// tangent of the angle between a plane and the plane through the axis at its end.
bool arc_ends(const TopoDS_Shape& body, const TopoDS_Edge& e, const std::vector<TopoDS_Face>& fs,
              std::vector<std::pair<gp_Pnt, gp_Dir>>& ends, double& tan_max) {
  AncMap vmap;
  TopExp::MapShapesAndAncestors(body, TopAbs_VERTEX, TopAbs_FACE, vmap);
  BRepAdaptor_Curve c(e);
  const double t0 = c.FirstParameter(), t1 = c.LastParameter();
  TopoDS_Vertex v0, v1;
  TopExp::Vertices(e, v0, v1);
  if (v0.IsNull() || v1.IsNull() || v0.IsSame(v1)) return false;
  tan_max = 0;
  for (const TopoDS_Vertex& v : {v0, v1}) {
    const gp_Pnt p = BRep_Tool::Pnt(v);
    const bool at_end = p.Distance(c.Value(t1)) < p.Distance(c.Value(t0));
    gp_Pnt q;
    gp_Vec t;
    c.D1(at_end ? t1 : t0, q, t);
    if (q.Distance(p) > 1e-6 || t.Magnitude() < 1e-12) return false;
    t.Normalize();
    int k = vmap.FindIndex(v);
    if (k <= 0) return false;
    std::vector<TopoDS_Face> others;
    for (ShapeList::Iterator it(vmap(k)); it.More(); it.Next()) {
      bool known = false;
      for (const auto& g : fs) known = known || g.IsSame(it.Value());
      for (const auto& g : others) known = known || g.IsSame(it.Value());
      if (!known) others.push_back(TopoDS::Face(it.Value()));
    }
    if (others.size() != 1) return false;
    if (BRepAdaptor_Surface(others[0], false).GetType() != GeomAbs_Plane) return false;
    gp_Dir n;
    if (!outward_normal(others[0], p, n)) return false;
    // Leaving the material at the arc's last point, entering it at its first.
    const double cs = gp_Vec(n).Dot(t) * (at_end ? 1.0 : -1.0);
    if (cs < 0.5) return false;
    tan_max = std::max(tan_max, std::sqrt(std::max(0.0, 1.0 - cs * cs)) / cs);
    ends.emplace_back(p, n);
  }
  return ends.size() == 2;
}

// For arc `i` of a fillet among other arcs of its circle rounded alike (3777 Fillet4: the two
// arcs of a disc's rim between two flats; Fusion's round is the whole rim's, one torus cut off by
// the flats): how far past its start and its end its corner is revolved, to the middle of the
// gap on each side, so that the arcs' corners meet there and together are the whole rim's
// corner trimmed by the planes the gaps lie under. False when the arc has no such neighbours
// or another arc overlaps it.
bool ring_turns(const std::vector<BlendEdge>& es, size_t i, double& turn0, double& turn1) {
  BRepAdaptor_Curve ci(es[i].e);
  if (ci.GetType() != GeomAbs_Circle) return false;
  const double t0 = ci.FirstParameter(), t1 = ci.LastParameter();
  if (std::fabs(t1 - t0 - 2 * M_PI) <= 1e-9) return false;
  const gp_Circ circ = ci.Circle();
  auto wrap = [](double x) {
    x = std::fmod(x, 2 * M_PI);
    return x < 0 ? x + 2 * M_PI : x;
  };
  double next = 2 * M_PI, prev = 2 * M_PI;
  bool any = false;
  for (size_t j = 0; j < es.size(); ++j) {
    if (j == i || std::fabs(es[j].a - es[i].a) > 1e-12) continue;
    BRepAdaptor_Curve cj(es[j].e);
    if (cj.GetType() != GeomAbs_Circle) continue;
    const gp_Circ o = cj.Circle();
    if (std::fabs(o.Radius() - circ.Radius()) > 1e-7 || o.Location().Distance(circ.Location()) > 1e-7 ||
        !o.Axis().IsParallel(circ.Axis(), 1e-9))
      continue;
    any = true;
    for (const double t : {cj.FirstParameter(), 0.5 * (cj.FirstParameter() + cj.LastParameter()), cj.LastParameter()}) {
      const double u = ElCLib::Parameter(circ, cj.Value(t));
      if (wrap(u - t0) < t1 - t0 - 1e-9 && wrap(u - t0) > 1e-9) return false;  // inside this arc
      next = std::min(next, wrap(u - t1));
      prev = std::min(prev, wrap(t0 - u));
    }
  }
  if (!any || !(next > 1e-6) || !(prev > 1e-6)) return false;
  turn0 = 0.5 * prev;
  turn1 = 0.5 * next;
  return true;
}

// The corner of a fillet of a straight concave edge between a plane and a cylinder about an
// axis along the edge (512 Fillet3: a 0.6 mm groove whose ends, the sides of the arms that cross
// it, are rounded with radius 0.3, which leaves nothing of the ends). The section is the same
// all along the edge: the region between the corner, the plane, the circle and the round of
// radius `r` that touches both on the open side. The tool fills it (a prism along the edge).
bool plane_cylinder_corner(const TopoDS_Shape& body, const std::vector<TopoDS_Face>& fs, double r,
                           const BRepAdaptor_Curve& c, Corner& out) {
  const double t0 = c.FirstParameter(), t1 = c.LastParameter();
  const gp_Pnt e0 = c.Value(t0), e1 = c.Value(t1);
  if (e0.Distance(e1) < 1e-9) return false;
  const gp_Dir along(gp_Vec(e0, e1));
  BRepAdaptor_Surface s0(fs[0], false), s1(fs[1], false);
  const bool plane_first = s0.GetType() == GeomAbs_Plane;
  const TopoDS_Face& pf = plane_first ? fs[0] : fs[1];
  const TopoDS_Face& cf = plane_first ? fs[1] : fs[0];
  const BRepAdaptor_Surface& ps = plane_first ? s0 : s1;
  const BRepAdaptor_Surface& cs = plane_first ? s1 : s0;
  if (ps.GetType() != GeomAbs_Plane || cs.GetType() != GeomAbs_Cylinder) return false;
  const gp_Cylinder cyl = cs.Cylinder();
  if (!cyl.Axis().Direction().IsParallel(along, 1e-9)) return false;
  if (std::fabs(ps.Plane().Axis().Direction().Dot(along)) > 1e-9) return false;
  const gp_Pnt mid = c.Value(0.5 * (t0 + t1));
  gp_Dir np, nc;
  if (!outward_normal(pf, mid, np) || !outward_normal(cf, mid, nc)) return false;
  // Concave: beside the edge along each face, the body goes on.
  const double eps = 1e-3 * r;
  BRepClass3d_SolidClassifier in(body, mid.Translated((gp_Vec(np) - gp_Vec(nc)) * eps), 1e-9);
  if (in.State() != TopAbs_IN) return false;
  // The section through the edge's first point, across it.
  const gp_Pnt p0 = e0;
  const gp_Pnt ctr = ElCLib::Value(ElCLib::Parameter(gp_Lin(cyl.Axis()), p0), gp_Lin(cyl.Axis()));
  const double R = cyl.Radius();
  if (std::fabs(ctr.Distance(p0) - R) > 1e-7) return false;
  // The round's centre: r off the plane on its open side, R + r from the axis where the
  // cylinder's open side is away from it, R - r where it is towards it.
  const bool open_out = gp_Vec(ctr, p0).Dot(gp_Vec(nc)) > 0;
  const double rc = open_out ? R + r : R - r;
  if (!(rc > 0)) return false;
  const gp_Vec u = gp_Vec(np).Crossed(gp_Vec(along));  // along the plane, across the edge
  const gp_Pnt q = p0.Translated(gp_Vec(np) * r);
  // |q + t u - ctr| = rc
  const gp_Vec w(ctr, q);
  const double bq = 2 * w.Dot(u), cq = w.Dot(w) - rc * rc;
  const double disc = bq * bq - 4 * cq;
  if (disc < 0) return false;
  // The root whose touching point on the plane lies on the plane face, nearest the edge.
  gp_Pnt o;
  bool found = false;
  for (double tt : {(-bq + std::sqrt(disc)) / 2, (-bq - std::sqrt(disc)) / 2}) {
    gp_Pnt oo = q.Translated(u * tt);
    // At a concave edge the plane face goes the way the cylinder face looks out.
    gp_Pnt touch = oo.Translated(-gp_Vec(np) * r);
    if (gp_Vec(p0, touch).Dot(gp_Vec(nc)) <= 1e-9) continue;
    if (found && oo.Distance(p0) >= o.Distance(p0)) continue;
    o = oo;
    found = true;
  }
  if (!found) return false;
  const gp_Pnt T1 = o.Translated(-gp_Vec(np) * r);
  gp_Vec oc(ctr, o);
  oc.Normalize();
  const gp_Pnt T2 = ctr.Translated(oc * R);
  auto arc_mid = [](const gp_Pnt& centre, double rad, const gp_Pnt& a, const gp_Pnt& b) {
    gp_Vec m = gp_Vec(centre, a).Normalized() + gp_Vec(centre, b).Normalized();
    if (m.Magnitude() < 1e-9) throw Standard_Failure("could not build the corner");
    return centre.Translated(m.Normalized() * rad);
  };
  GC_MakeArcOfCircle round(T1, arc_mid(o, r, T1, T2), T2);
  GC_MakeArcOfCircle wall(T2, arc_mid(ctr, R, T2, p0), p0);
  if (!round.IsDone() || !wall.IsDone()) throw Standard_Failure("could not build the corner");
  TopoDS_Edge blend = BRepBuilderAPI_MakeEdge(round.Value()).Edge();
  BRepBuilderAPI_MakeWire mw;
  mw.Add(blend);
  mw.Add(BRepBuilderAPI_MakeEdge(wall.Value()).Edge());
  mw.Add(BRepBuilderAPI_MakeEdge(p0, T1).Edge());
  if (!mw.IsDone()) throw Standard_Failure("could not build the corner");
  BRepBuilderAPI_MakeFace mf(mw.Wire(), true);
  if (!mf.IsDone()) throw Standard_Failure("could not build the corner");
  BRepPrimAPI_MakePrism mk(mf.Face(), gp_Vec(e0, e1));
  mk.Build();
  if (!mk.IsDone()) throw Standard_Failure("could not build the corner");
  out.cutter = mk.Shape();
  const ShapeList& g = mk.Generated(blend);
  if (!g.IsEmpty()) out.face = g.First();
  if (signed_volume(out.cutter) < 0) out.cutter.Reverse();
  out.exact = out.cutter;
  out.ideal = std::fabs(signed_volume(out.cutter));
  out.concave = true;
  return !out.face.IsNull() && out.ideal > 0;
}

// The corner of a convex edge between two perpendicular faces whose section is the same all
// along it: a straight edge between two planes (the corner is a prism) or a circle between a
// plane across its axis and a cylinder about it (the corner is revolved; an arc's corner ends
// on the planes `arc_ends` finds). `a` is measured on `x.ref` (or the first face), `b` on the
// other face. The cutter of a straight edge's corner may reach on past the edge's start by `e0`
// and past its end by `e1` (`exact` and `ideal` stay the corner along the edge).
bool corner_of(const TopoDS_Shape& body, const AncMap& anc, const BlendEdge& x, bool fillet, Corner& out,
               double e0 = 0, double e1 = 0, double turn0 = -1, double turn1 = -1) {
  int ei = anc.FindIndex(x.e);
  if (ei <= 0) return false;
  std::vector<TopoDS_Face> fs;
  for (ShapeList::Iterator it(anc(ei)); it.More(); it.Next()) {
    bool seen = false;
    for (const auto& g : fs) seen = seen || g.IsSame(it.Value());
    if (!seen) fs.push_back(TopoDS::Face(it.Value()));
  }
  if (fs.size() != 2) return false;
  if (!x.ref.IsNull() && fs[1].IsSame(x.ref)) std::swap(fs[0], fs[1]);
  if (!x.ref.IsNull() && !fs[0].IsSame(x.ref)) return false;
  double a = x.a, b = x.b;
  if (fillet || x.mode == 0) b = a;
  else if (x.mode != 1) return false;
  if (!(a > 0) || !(b > 0)) return false;
  BRepAdaptor_Curve c(x.e);
  const double t0 = c.FirstParameter(), t1 = c.LastParameter();
  const bool line = c.GetType() == GeomAbs_Line;
  const bool circle = c.GetType() == GeomAbs_Circle;
  if (!line && !circle) return false;
  bool whole = true;
  std::vector<std::pair<gp_Pnt, gp_Dir>> ends;
  double tan_max = 0;
  BRepAdaptor_Surface s0(fs[0], false), s1(fs[1], false);
  gp_Pnt p0 = c.Value(line ? t0 : 0.5 * (t0 + t1));
  gp_Dir n1, n2;
  if (!outward_normal(fs[0], p0, n1) || !outward_normal(fs[1], p0, n2)) return false;
  if (line && fillet && (s0.GetType() == GeomAbs_Cylinder || s1.GetType() == GeomAbs_Cylinder))
    return plane_cylinder_corner(body, fs, a, c, out);
  if (std::fabs(n1.Dot(n2)) > 1e-9) return false;
  // Concave: the material is on the outer side of both faces' corner (512: the top corners of
  // a window cut through a wall, rounded to a half circle). Only straight edges.
  {
    const double eps = 1e-3 * std::min(a, b);
    // Beside the corner, along a face: out of a convex edge's body, in a concave edge's.
    BRepClass3d_SolidClassifier in(body, c.Value(0.5 * (t0 + t1)).Translated((gp_Vec(n1) - gp_Vec(n2)) * eps), 1e-9);
    if (in.State() == TopAbs_IN) {
      if (!line) return false;
      out.concave = true;
      n1.Reverse();
      n2.Reverse();
    }
  }
  if (line) {
    if (s0.GetType() != GeomAbs_Plane || s1.GetType() != GeomAbs_Plane) return false;
  } else {
    // A whole circle or an arc between two end planes; one face a plane across its axis, the
    // other a cylinder about it.
    whole = std::fabs(t1 - t0 - 2 * M_PI) <= 1e-9;
    if (!whole && !arc_ends(body, x.e, fs, ends, tan_max)) return false;
    gp_Ax1 ax = c.Circle().Axis();
    const BRepAdaptor_Surface& pl = s0.GetType() == GeomAbs_Plane ? s0 : s1;
    const BRepAdaptor_Surface& cy = s0.GetType() == GeomAbs_Cylinder ? s0 : s1;
    if (pl.GetType() != GeomAbs_Plane || cy.GetType() != GeomAbs_Cylinder) return false;
    if (!pl.Plane().Axis().IsParallel(ax, 1e-9)) return false;
    gp_Cylinder cyl = cy.Cylinder();
    if (!cyl.Axis().IsParallel(ax, 1e-9) || gp_Lin(cyl.Axis()).Distance(ax.Location()) > 1e-7) return false;
    if (std::fabs(cyl.Radius() - c.Circle().Radius()) > 1e-7) return false;
  }
  // The section, in the plane of n1 and n2 at p0: inward along the first face is -n2.
  const gp_Vec v1(n1), v2(n2);
  const double d = 0.25 * std::min(a, b);
  // How far past an arc's ends its corner is revolved: past where the end planes leave the
  // section (measured at its point nearest the axis).
  // An arc of a ring (`ring_turns`) is revolved on to the middles of the gaps beside it instead.
  double reach0 = 0, reach1 = 0;
  if (!whole) {
    const double r_in = c.Circle().Radius() - std::max(a, b) - d;
    if (!(r_in > 1e-6) || !(tan_max < 2.0)) return false;
    reach0 = reach1 = 1.5 * (std::max(a, b) + d) * std::sqrt(2.0) * tan_max / r_in + 1e-3;
    if (turn0 >= 0 && turn1 >= 0) {
      reach0 = turn0;
      reach1 = turn1;
    }
    if (t1 - t0 + reach0 + reach1 >= 2 * M_PI) return false;
  }
  gp_Pnt A = p0.Translated(-v2 * a), B = p0.Translated(-v1 * b);
  auto section = [&](bool reach) -> std::pair<TopoDS_Face, TopoDS_Edge> {
    BRepBuilderAPI_MakeWire mw;
    TopoDS_Edge blend;
    if (fillet) {
      gp_Pnt o = p0.Translated(-(v1 + v2) * a);
      gp_Pnt m = o.Translated((v1 + v2).Normalized() * a);
      GC_MakeArcOfCircle arc(A, m, B);
      if (!arc.IsDone()) throw Standard_Failure("could not build the corner");
      blend = BRepBuilderAPI_MakeEdge(arc.Value()).Edge();
    } else {
      blend = BRepBuilderAPI_MakeEdge(A, B).Edge();
    }
    mw.Add(blend);
    if (reach) {
      gp_Pnt B2 = B.Translated(v2 * d), K = p0.Translated((v1 + v2) * d), A2 = A.Translated(v1 * d);
      mw.Add(BRepBuilderAPI_MakeEdge(B, B2).Edge());
      mw.Add(BRepBuilderAPI_MakeEdge(B2, K).Edge());
      mw.Add(BRepBuilderAPI_MakeEdge(K, A2).Edge());
      mw.Add(BRepBuilderAPI_MakeEdge(A2, A).Edge());
    } else {
      mw.Add(BRepBuilderAPI_MakeEdge(B, p0).Edge());
      mw.Add(BRepBuilderAPI_MakeEdge(p0, A).Edge());
    }
    if (!mw.IsDone()) throw Standard_Failure("could not build the corner");
    BRepBuilderAPI_MakeFace mf(mw.Wire(), true);
    if (!mf.IsDone()) throw Standard_Failure("could not build the corner");
    return {mf.Face(), blend};
  };
  if (line) {
    const gp_Vec along(c.Value(t0), c.Value(t1));
    if (!(along.Magnitude() > 0)) return false;
    out.line = true;
    out.end0 = c.Value(t0);
    out.end1 = c.Value(t1);
    out.dir = gp_Dir(along);
  } else if (e0 != 0 || e1 != 0) {
    return false;
  }
  auto solid_of = [&](const TopoDS_Face& f, const TopoDS_Edge& blend, TopoDS_Shape& face,
                      bool longer = false) -> TopoDS_Shape {
    TopoDS_Shape sh;
    if (line) {
      // The longer cutter starts `e0` before the edge and ends `e1` after it.
      const gp_Vec along(c.Value(t0), c.Value(t1));
      const double l0 = longer ? e0 : 0.0, l1 = longer ? e1 : 0.0;
      gp_Trsf back;
      back.SetTranslation(-gp_Vec(out.dir) * l0);
      const TopLoc_Location loc(back);
      BRepPrimAPI_MakePrism mk(f.Moved(loc), along + gp_Vec(out.dir) * (l0 + l1));
      mk.Build();
      if (!mk.IsDone()) throw Standard_Failure("could not build the corner");
      sh = mk.Shape();
      const ShapeList& g = mk.Generated(blend.Moved(loc));
      if (!g.IsEmpty()) face = g.First();
    } else if (whole) {
      BRepPrimAPI_MakeRevol mk(f, c.Circle().Axis(), 2 * M_PI);
      mk.Build();
      if (!mk.IsDone()) throw Standard_Failure("could not build the corner");
      sh = mk.Shape();
      const ShapeList& g = mk.Generated(blend);
      if (!g.IsEmpty()) face = g.First();
    } else {
      // The section is at the arc's middle: turned back to past its start, then revolved to
      // past its end.
      gp_Trsf back;
      back.SetRotation(c.Circle().Axis(), -(0.5 * (t1 - t0) + reach0));
      const TopLoc_Location loc(back);
      BRepPrimAPI_MakeRevol mk(f.Moved(loc), c.Circle().Axis(), t1 - t0 + reach0 + reach1);
      mk.Build();
      if (!mk.IsDone()) throw Standard_Failure("could not build the corner");
      sh = mk.Shape();
      const ShapeList& g = mk.Generated(blend.Moved(loc));
      if (!g.IsEmpty()) face = g.First();
    }
    return sh;
  };
  auto [fi, bi] = section(false);
  TopoDS_Shape unused;
  TopoDS_Shape exact = solid_of(fi, bi, unused);
  if (!whole) {
    // The corner trimmed by the end planes: the material the cut may take.
    for (const auto& [p, n] : ends) {
      TopoDS_Face pf = BRepBuilderAPI_MakeFace(gp_Pln(p, n)).Face();
      TopoDS_Shape half = BRepPrimAPI_MakeHalfSpace(pf, p.Translated(-gp_Vec(n))).Solid();
      BRepAlgoAPI_Common cm(exact, half);
      if (!cm.IsDone()) throw Standard_Failure("could not build the corner");
      exact = cm.Shape();
    }
  }
  if (signed_volume(exact) < 0) exact.Reverse();
  out.exact = exact;
  out.ideal = std::fabs(measured_volume(exact));
  auto [fc, bc] = section(true);
  out.cutter = solid_of(fc, bc, out.face, true);
  if (signed_volume(out.cutter) < 0) out.cutter.Reverse();
  return !out.face.IsNull() && out.ideal > 0;
}

// The corners cut away (or filled): the result, its face origins, the faces of it with no
// origin, and the corners. `ext`: per edge, how far past its start and its end its cutter
// reaches; where it reaches past an end the material removed may be more than the corners'
// (it is checked by the caller), else it must be exactly theirs.
struct CornerCut {
  TopoDS_Shape result;
  std::vector<int64_t> o;
  std::vector<TopoDS_Face> orphans;
  std::vector<Corner> cs;
  bool concave = false;
};

int mixed_corners(const GsShape* s, const std::vector<uint32_t>& idx, const ShapeList& tools, const ShapeList& cave,
                  double ideal, double ideal_cave, bool longer, CornerCut& cut);
template <typename Op, typename Gone>
int corner_origins(Op& op, Gone gone, const TopoDS_Shape& in, const TopoDS_Shape& result,
                   const std::vector<uint32_t>& idx, CornerCut& cut);

int cut_corners(const GsShape* s, const AncMap& anc, const std::vector<BlendEdge>& es,
                const std::vector<uint32_t>& idx, bool fillet, const std::vector<std::array<double, 2>>& ext,
                CornerCut& cut) {
  std::vector<Corner>& cs = cut.cs;
  ShapeList args, tools, cave;
  args.Append(s->s);
  double ideal = 0, ideal_cave = 0;
  bool longer = false;
  for (size_t i = 0; i < es.size(); ++i) {
    Corner c;
    double turn0 = -1, turn1 = -1;
    if (fillet && !ring_turns(es, i, turn0, turn1)) turn0 = turn1 = -1;
    if (!corner_of(s->s, anc, es[i], fillet, c, ext[i][0], ext[i][1], turn0, turn1)) return fail("no corner");
    longer = longer || ext[i][0] > 0 || ext[i][1] > 0;
    if (c.concave) {
      ideal_cave += c.ideal;
      cave.Append(c.cutter);
    } else {
      ideal += c.ideal;
      tools.Append(c.cutter);
    }
    cs.push_back(c);
  }
  if (!tools.IsEmpty() && !cave.IsEmpty()) return mixed_corners(s, idx, tools, cave, ideal, ideal_cave, longer, cut);
  const bool concave = !cave.IsEmpty();
  cut.concave = concave;
  if (concave) {
    tools = cave;
    ideal = ideal_cave;
  }
  // Chamfers of edges that meet at a corner of the body overlap there (258 Chamfer1 around the
  // top of a block): the material removed must be the union's volume, and corners of edges
  // that do not meet must not overlap at all (a blend wider than its face is refused).
  if (!fillet && cs.size() > 1 && cs.size() <= 32) {
    for (size_t i = 0; i < cs.size(); ++i)
      for (size_t j = i + 1; j < cs.size(); ++j) {
        TopoDS_Vertex v;
        if (TopExp::CommonVertex(es[i].e, es[j].e, v)) continue;
        BRepAlgoAPI_Common cm(cs[i].exact, cs[j].exact);
        if (!cm.IsDone() || std::fabs(signed_volume(cm.Shape())) > 1e-9 * std::max(1.0, ideal))
          return fail("the corners of edges that do not meet overlap");
      }
    ShapeList first, rest;
    first.Append(cs.front().exact);
    for (size_t i = 1; i < cs.size(); ++i) rest.Append(cs[i].exact);
    auto u = run_boolean_checked(GS_BOOL_UNION, first, rest);
    if (u->Shape().IsNull()) return fail("no union of the corners");
    ideal = std::fabs(signed_volume(u->Shape()));
  }
  auto b = run_boolean_checked(concave ? GS_BOOL_UNION : GS_BOOL_CUT, args, tools);
  TopoDS_Shape result = b->Shape();
  if (result.IsNull()) return fail("no result");
  const double before = std::fabs(measured_volume(s->s)), after = std::fabs(measured_volume(result));
  const double change = concave ? after - before : before - after;
  const double tol = 1e-7 * std::max(1.0, before);
  if (longer ? change < ideal - tol : std::fabs(change - ideal) > tol)
    return fail("the corners were not all material");
  return corner_origins(*b, [&](const TopoDS_Shape& f) { return b->IsDeleted(f); }, s->s, result, idx, cut);
}

// Two booleans: the convex corners cut away (`tools`), then the concave ones filled (`cave`),
// each changing the volume by exactly its corners' (2278 Fillet1: a convex and a concave corner
// 6 mm apart on one face, both rounded by 3 mm, which leaves nothing of that face; OCCT's
// fillet cannot end a blend on a face it uses up).
int mixed_corners(const GsShape* s, const std::vector<uint32_t>& idx, const ShapeList& tools, const ShapeList& cave,
                  double ideal, double ideal_cave, bool longer, CornerCut& cut) {
  ShapeList args;
  args.Append(s->s);
  auto b1 = run_boolean_checked(GS_BOOL_CUT, args, tools);
  const TopoDS_Shape mid = b1->Shape();
  if (mid.IsNull()) return fail("no result");
  ShapeList mids;
  mids.Append(mid);
  auto b2 = run_boolean_checked(GS_BOOL_UNION, mids, cave);
  const TopoDS_Shape result = b2->Shape();
  if (result.IsNull()) return fail("no result");
  const double before = std::fabs(measured_volume(s->s)), between = std::fabs(measured_volume(mid)),
               after = std::fabs(measured_volume(result));
  const double tol = 1e-7 * std::max(1.0, before);
  const double cut_away = before - between, filled = after - between;
  if (longer ? cut_away < ideal - tol : std::fabs(cut_away - ideal) > tol)
    return fail("the corners were not all material");
  if (std::fabs(filled - ideal_cave) > tol) return fail("the concave corners were not all empty");
  ShapeList a1 = args, a2 = mids;
  for (ShapeList::Iterator it(tools); it.More(); it.Next()) a1.Append(it.Value());
  for (ShapeList::Iterator it(cave); it.More(); it.Next()) a2.Append(it.Value());
  BRepTools_History h(a1, *b1);
  h.Merge(a2, *b2);
  // Concave corners were filled: no convex corner is cut on past its ends (`blend_by_corners`).
  cut.concave = true;
  return corner_origins(h, [&](const TopoDS_Shape& f) { return h.IsRemoved(f); }, s->s, result, idx, cut);
}

// The origins of the faces of `result`, made from `in` by `op` (a boolean or a history; `gone`
// says whether it removed a shape): the input's faces and each corner's blend face.
template <typename Op, typename Gone>
int corner_origins(Op& op, Gone gone, const TopoDS_Shape& in, const TopoDS_Shape& result,
                   const std::vector<uint32_t>& idx, CornerCut& cut) {
  const std::vector<Corner>& cs = cut.cs;
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t>& o = cut.o;
  record_input_faces(op, in, 0, out_faces, o);
  for (size_t i = 0; i < cs.size(); ++i) {
    const TopoDS_Shape& f = cs[i].face;
    std::vector<TopoDS_Shape> res;
    const ShapeList& mods = op.Modified(f);
    for (ShapeList::Iterator it(mods); it.More(); it.Next()) res.push_back(it.Value());
    if (mods.IsEmpty() && !gone(f)) res.push_back(f);
    bool found = false;
    for (const auto& g : res) {
      int k = out_faces.FindIndex(g);
      if (k <= 0) continue;
      found = true;
      push_origin(o, k - 1, GS_ORIGIN_FROM_EDGE, 0, idx[i]);
    }
    if (!found) return fail("a corner left no blend face");
  }
  std::vector<bool> has(size_t(out_faces.Extent()), false);
  for (size_t k = 0; k + 3 < o.size(); k += 4) {
    const int64_t f = o[k];
    if (f >= 0 && f < int64_t(has.size())) has[size_t(f)] = true;
  }
  for (int k = 1; k <= out_faces.Extent(); ++k)
    if (!has[size_t(k - 1)]) cut.orphans.push_back(TopoDS::Face(out_faces(k)));
  cut.result = result;
  return GS_OK;
}

// Which end of a straight corner a face lies across: 0 its start, 1 its end, -1 neither (a
// plane through the end point with the edge's direction as normal).
int corner_end_of(const Corner& c, const TopoDS_Face& f) {
  if (!c.line) return -1;
  BRepAdaptor_Surface sf(f, false);
  if (sf.GetType() != GeomAbs_Plane) return -1;
  const gp_Pln pl = sf.Plane();
  if (!pl.Axis().Direction().IsParallel(c.dir, 1e-9)) return -1;
  if (pl.Distance(c.end0) < 1e-7) return 0;
  if (pl.Distance(c.end1) < 1e-7) return 1;
  return -1;
}

// The blend as booleans: each edge's corner cut away (or, for concave edges, filled). Only
// where every edge has such a corner (convex ones cut first, then concave ones filled where
// there are both: `mixed_corners`) and the material removed (added) is exactly the corners' volume (so each corner lay wholly in the body, or wholly out of it, and
// no tool took or added anything else).
//
// Where the body goes on past a convex straight edge's end inside its corner (the edge ends at
// a face slanting away, as a chamfer next to an earlier chamfer's slope: 5358's Chamfer2), the
// cut leaves a face across that end with no origin. A blend ended by a third face is trimmed
// by it: the corner then runs on past that end until it leaves the body. The longer cut must
// still remove all of the corners along the edges and leave no face without an origin (so it
// ran out of the body before its far end).
int blend_by_corners(const GsShape* s, const std::vector<BlendEdge>& es, const std::vector<uint32_t>& idx, bool fillet,
                     GsShape** out, GsBuf* origins) {
  AncMap anc;
  TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<std::array<double, 2>> ext(es.size(), {0.0, 0.0});
  CornerCut cut;
  int st = cut_corners(s, anc, es, idx, fillet, ext, cut);
  if (st != GS_OK) return st;
  if (!cut.orphans.empty() && !cut.concave) {
    bool placed_all = true;
    for (const auto& f : cut.orphans) {
      bool placed = false;
      for (size_t i = 0; i < cut.cs.size(); ++i) {
        const int end = corner_end_of(cut.cs[i], f);
        if (end < 0) continue;
        const BlendEdge& x = es[i];
        ext[i][size_t(end)] = 4 * (x.a + (x.mode == 1 ? x.b : x.a));
        placed = true;
      }
      placed_all = placed_all && placed;
    }
    if (placed_all) {
      CornerCut longer;
      st = cut_corners(s, anc, es, idx, fillet, ext, longer);
      if (st != GS_OK) return st;
      if (!longer.orphans.empty()) return fail("a corner left a face with no origin");
      cut = std::move(longer);
    }
  }
  to_buf(cut.o, origins);
  *out = new GsShape{cut.result};
  return GS_OK;
}

// The images in `result` of `f` after `op` (a boolean or a history): its modifications, or
// itself when the operation kept it.
template <typename Op>
std::vector<TopoDS_Shape> images_after(Op& op, const TopoDS_Shape& f, bool gone) {
  std::vector<TopoDS_Shape> res;
  const ShapeList& mods = op.Modified(f);
  for (ShapeList::Iterator it(mods); it.More(); it.Next()) res.push_back(it.Value());
  if (mods.IsEmpty() && !gone) res.push_back(f);
  return res;
}

// A blend of whole circles about one axis, made where OCCT fails on the shape itself (916's
// Fillet1: three 0.5 mm rounds at the foot of a threaded rod, one of whose tangent lines lies
// on the square end of the thread's groove, where ChFi3d's first guess of the contact runs
// along the groove's boundary). The shape is axisymmetric about the circles: its section in a
// half plane through the axis, in a window a few blend sizes around the circles, is revolved
// into a solid of revolution with the same circles, and OCCT blends those (at once, or the
// concave ones first). What the blend took away and what it added (the solid less the blended
// one and the other way round) are cut from and joined to the shape. Kept only where the
// change stays inside the window and the shape's volume changes by exactly those two volumes
// (so the shape is the solid of revolution wherever the blend touched it); else the section is
// tried at other angles. The blend faces are the blended solid's faces made from the circles.
template <typename Mk>
int revolved_blend(const GsShape* s, const std::vector<BlendEdge>& es, const std::vector<uint32_t>& idx, bool fillet,
                   GsShape** out, GsBuf* origins) {
  if (es.empty()) return fail("no edges");
  // The circles and their axis.
  gp_Ax1 axis;
  double rlo = 1e300, rhi = 0, zlo = 1e300, zhi = -1e300, size = 0;
  for (size_t i = 0; i < es.size(); ++i) {
    const BlendEdge& x = es[i];
    if (!x.ref.IsNull() || x.mode != 0) return fail("not one size");
    BRepAdaptor_Curve c(x.e);
    if (c.GetType() != GeomAbs_Circle || std::fabs(c.LastParameter() - c.FirstParameter() - 2 * M_PI) > 1e-9)
      return fail("not a whole circle");
    const gp_Circ ci = c.Circle();
    if (i == 0) axis = ci.Axis();
    if (!ci.Axis().IsParallel(axis, 1e-9) || gp_Lin(axis).Distance(ci.Location()) > 1e-7)
      return fail("not about one axis");
    const double z = gp_Vec(axis.Location(), ci.Location()).Dot(gp_Vec(axis.Direction()));
    rlo = std::min(rlo, ci.Radius());
    rhi = std::max(rhi, ci.Radius());
    zlo = std::min(zlo, z);
    zhi = std::max(zhi, z);
    size = std::max(size, x.a);
  }
  const double m = 3.1 * size;
  const double w_r0 = std::max(0.0, rlo - m), w_r1 = rhi + m, w_z0 = zlo - m, w_z1 = zhi + m;
  const gp_Dir zd = axis.Direction();
  const gp_Dir x0 = gp_Ax2(axis.Location(), zd).XDirection();
  // A point at radius r and height z of the half plane at angle `th`.
  auto at = [&](double th, double r, double z) {
    gp_Vec x(x0);
    x.Rotate(axis, th);
    return axis.Location().Translated(x * r + gp_Vec(zd) * z);
  };
  // Is a point of a tool strictly inside the window (or on the axis, where the window starts at it)?
  auto inside = [&](const gp_Pnt& p) {
    const gp_Vec v(axis.Location(), p);
    const double z = v.Dot(gp_Vec(zd));
    const double r = (v - gp_Vec(zd) * z).Magnitude();
    const double eps = 1e-6;
    return z > w_z0 + eps && z < w_z1 - eps && r < w_r1 - eps && (w_r0 == 0 || r > w_r0 + eps);
  };
  auto tool_inside = [&](const TopoDS_Shape& t) {
    for (TopExp_Explorer ex(t, TopAbs_EDGE); ex.More(); ex.Next()) {
      const TopoDS_Edge& e = TopoDS::Edge(ex.Current());
      if (BRep_Tool::Degenerated(e)) continue;
      BRepAdaptor_Curve c(e);
      for (int k = 0; k <= 8; ++k)
        if (!inside(c.Value(c.FirstParameter() + (c.LastParameter() - c.FirstParameter()) * k / 8.0))) return false;
    }
    return true;
  };
  auto solids_of = [](const TopoDS_Shape& t) {
    ShapeList l;
    for (TopExp_Explorer ex(t, TopAbs_SOLID); ex.More(); ex.Next()) l.Append(ex.Current());
    return l;
  };
  std::string why = "no section";
  for (int k = 0; k < 6; ++k) {
    const double th = 0.3 + k * (2 * M_PI / 6);
    try {
      // The section in the window, revolved.
      BRepBuilderAPI_MakePolygon poly(at(th, w_r0, w_z0), at(th, w_r1, w_z0), at(th, w_r1, w_z1), at(th, w_r0, w_z1),
                                      true);
      if (!poly.IsDone()) continue;
      BRepBuilderAPI_MakeFace wf(poly.Wire(), true);
      if (!wf.IsDone()) continue;
      BRepAlgoAPI_Common sec(s->s, wf.Face());
      sec.SetRunParallel(false);
      sec.Build();
      if (!sec.IsDone() || sec.HasErrors()) continue;
      TopoDS_Compound faces;
      BRep_Builder bb;
      bb.MakeCompound(faces);
      int nf = 0;
      for (TopExp_Explorer ex(sec.Shape(), TopAbs_FACE); ex.More(); ex.Next(), ++nf) bb.Add(faces, ex.Current());
      if (nf != 1) {
        why = "the section is not one face";
        continue;
      }
      TopoDS_Face sf;
      for (TopExp_Explorer ex(faces, TopAbs_FACE); ex.More(); ex.Next()) sf = TopoDS::Face(ex.Current());
      BRepPrimAPI_MakeRevol rv(sf, axis, 2 * M_PI);
      rv.Build();
      if (!rv.IsDone()) continue;
      TopoDS_Shape sub;
      for (TopExp_Explorer ex(rv.Shape(), TopAbs_SOLID); ex.More(); ex.Next()) sub = ex.Current();
      if (sub.IsNull() || !BRepCheck_Analyzer(sub, true, true).IsValid()) {
        why = "the revolved section is not a valid solid";
        continue;
      }
      if (signed_volume(sub) < 0) sub.Reverse();
      // The circles on it.
      ShapeMap sub_edges;
      TopExp::MapShapes(sub, TopAbs_EDGE, sub_edges);
      std::vector<BlendEdge> ses;
      for (const auto& x : es) {
        const gp_Circ ci = BRepAdaptor_Curve(x.e).Circle();
        TopoDS_Edge found;
        int n = 0;
        for (int i = 1; i <= sub_edges.Extent(); ++i) {
          BRepAdaptor_Curve c(TopoDS::Edge(sub_edges(i)));
          if (c.GetType() != GeomAbs_Circle) continue;
          const gp_Circ o = c.Circle();
          if (std::fabs(o.Radius() - ci.Radius()) > 1e-7 || o.Location().Distance(ci.Location()) > 1e-7) continue;
          found = TopoDS::Edge(sub_edges(i));
          ++n;
        }
        if (n != 1) break;
        BlendEdge y = x;
        y.e = found;
        ses.push_back(y);
      }
      if (ses.size() != es.size()) {
        why = "the circles are not edges of the revolved section";
        continue;
      }
      // The blend of the solid of revolution, and the history of its faces.
      TopoDS_Shape blended;
      BRepTools_History bh;
      {
        Mk mk(sub);
        for (const auto& y : ses) add_blend(mk, y);
        mk.Build();
        ShapeList args;
        args.Append(sub);
        if (mk.IsDone() && BRepCheck_Analyzer(mk.Shape(), true, true).IsValid()) {
          blended = mk.Shape();
          bh = BRepTools_History(args, mk);
        }
      }
      if (blended.IsNull() && fillet) {
        const std::vector<BlendEdge> order = concave_first(sub, ses);
        TopoDS_Shape made;
        BRepTools_History hist;
        if (!order.empty() && blend_in_passes<Mk>(sub, order, made, hist) &&
            BRepCheck_Analyzer(made, true, true).IsValid()) {
          blended = made;
          bh = hist;
        }
      }
      if (blended.IsNull()) {
        why = "the solid of revolution does not blend";
        continue;
      }
      // What the blend took away and what it added.
      ShapeList lsub, lbl;
      lsub.Append(sub);
      lbl.Append(blended);
      auto taken = run_boolean(GS_BOOL_CUT, lsub, lbl, false);
      auto added = run_boolean(GS_BOOL_CUT, lbl, lsub, false);
      const ShapeList cut_tools = solids_of(taken->Shape()), add_tools = solids_of(added->Shape());
      if (cut_tools.IsEmpty() && add_tools.IsEmpty()) continue;
      if (!tool_inside(taken->Shape()) || !tool_inside(added->Shape())) {
        why = "the blend reaches the window's edge";
        continue;
      }
      const double v_cut = std::fabs(measured_volume(taken->Shape()));
      const double v_add = std::fabs(measured_volume(added->Shape()));
      // On the shape: the material taken away cut, then the material added joined.
      ShapeList args;
      args.Append(s->s);
      TopoDS_Shape mid = s->s;
      std::unique_ptr<BRepAlgoAPI_BooleanOperation> b1, b2;
      BRepTools_History h;
      bool first = true;
      if (!cut_tools.IsEmpty()) {
        b1 = run_boolean_checked(GS_BOOL_CUT, args, cut_tools);
        mid = b1->Shape();
        ShapeList a1 = args;
        for (ShapeList::Iterator it(cut_tools); it.More(); it.Next()) a1.Append(it.Value());
        h = BRepTools_History(a1, *b1);
        first = false;
      }
      TopoDS_Shape result = mid;
      if (!add_tools.IsEmpty()) {
        ShapeList mids;
        mids.Append(mid);
        b2 = run_boolean_checked(GS_BOOL_UNION, mids, add_tools);
        result = b2->Shape();
        ShapeList a2 = mids;
        for (ShapeList::Iterator it(add_tools); it.More(); it.Next()) a2.Append(it.Value());
        if (first) h = BRepTools_History(a2, *b2);
        else h.Merge(a2, *b2);
      }
      if (result.IsNull()) continue;
      const double before = std::fabs(measured_volume(s->s)), between = std::fabs(measured_volume(mid)),
                   after = std::fabs(measured_volume(result));
      const double tol = 1e-7 * std::max(1.0, before);
      if (std::fabs(before - between - v_cut) > tol || std::fabs(after - between - v_add) > tol) {
        why = "the shape is not the solid of revolution where the blend changes it";
        continue;
      }
      // The origins: the shape's faces, and the blend faces (made from the circles on the
      // solid of revolution, then in a tool, then in the result).
      ShapeMap out_faces;
      TopExp::MapShapes(result, TopAbs_FACE, out_faces);
      std::vector<int64_t> o;
      record_input_faces(h, s->s, 0, out_faces, o);
      bool all = true;
      for (size_t i = 0; i < ses.size() && all; ++i) {
        bool found = false;
        const ShapeList& gen = bh.Generated(ses[i].e);
        for (ShapeList::Iterator g(gen); g.More(); g.Next()) {
          if (g.Value().ShapeType() != TopAbs_FACE) continue;
          for (auto* op : {taken.get(), added.get()}) {
            for (const auto& t : images_after(*op, g.Value(), op->IsDeleted(g.Value()))) {
              for (const auto& r : images_after(h, t, h.IsRemoved(t))) {
                const int fi = out_faces.FindIndex(r);
                if (fi <= 0) continue;
                found = true;
                push_origin(o, fi - 1, GS_ORIGIN_FROM_EDGE, 0, idx[i]);
              }
            }
          }
        }
        all = found;
      }
      if (!all) {
        why = "a circle left no blend face";
        continue;
      }
      to_buf(o, origins);
      *out = new GsShape{result};
      return GS_OK;
    } catch (const Standard_Failure&) {
      why = "geometry kernel error";
    }
  }
  return fail(why);
}
}  // namespace
}  // extern "C++"

int gs_boolean(int op, const GsShape* target, const GsShape* tool, int refine, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!target || !tool || !out || !origins) return fail("null argument");
  ShapeList args, tools;
  occ::handle<BRepTools_History> split_target, split_tool;
  args.Append(split_helical_bands(target->s, split_target));
  const TopoDS_Shape tool_s = split_helical_bands(tool->s, split_tool);
  // A multi-region extrusion is a compound of prisms that may share faces (two sketch
  // regions with a common edge). As one tool such a compound is self-touching and the
  // result is invalid; as separate tools the general fuse splits the shared faces.
  // (Union and cut only: they act on the tools' union, which this does not change.)
  if (tool_s.ShapeType() == TopAbs_COMPOUND && (op == GS_BOOL_UNION || op == GS_BOOL_CUT)) {
    for (TopoDS_Iterator it(tool_s); it.More(); it.Next()) tools.Append(it.Value());
  } else {
    tools.Append(tool_s);
  }
  if (tools.IsEmpty()) return fail("the tool is empty");
  std::unique_ptr<BRepAlgoAPI_BooleanOperation> b;
  try {
    b = run_boolean_checked(op, args, tools, refine != 0);  // validated
  } catch (const Standard_Failure&) {
    // An intersection is the same either way round, and OCCT can fail one way only (3777:
    // two bars with flats on one cylinder, "Courbes non jointives" with one as the object).
    if (op != GS_BOOL_INTERSECT) throw;
    b = run_boolean_checked(op, tools, args, refine != 0);
  }
  TopoDS_Shape result = b->Shape();
  if (result.IsNull()) return fail("boolean operation produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  if (out_faces.Extent() == 0) return fail("boolean operation removed all material");
  std::vector<int64_t> o;
  AfterSplit ht{split_target, *b}, hs{split_tool, *b};
  record_input_faces(ht, target->s, 0, out_faces, o);
  record_input_faces(hs, tool->s, 1, out_faces, o);
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_boolean_many(int op, const GsShape* target, const GsShape* const* tools, size_t ntools, int refine,
                    GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!target || !out || !origins || (!tools && ntools)) return fail("null argument");
  if (ntools > size_t(INT_MAX) - 1) return fail("too many tools");
  for (size_t i = 0; i < ntools; ++i) {
    if (!tools[i]) return fail("null argument");
  }
  // One operation with all tools (review H3): the general fuse intersects every pair once,
  // instead of re-intersecting a growing result once per tool.
  ShapeList args, tool_list;
  std::vector<occ::handle<BRepTools_History>> splits(ntools + 1);
  args.Append(split_helical_bands(target->s, splits[0]));
  for (size_t i = 0; i < ntools; ++i) tool_list.Append(split_helical_bands(tools[i]->s, splits[i + 1]));
  if (op != GS_BOOL_UNION && op != GS_BOOL_CUT) return fail("only union and cut take several tools at once");
  auto b = run_boolean_checked(op, args, tool_list, refine != 0);
  TopoDS_Shape result = b->Shape();
  if (result.IsNull()) return fail("boolean operation produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  if (out_faces.Extent() == 0) return fail("boolean operation removed all material");
  std::vector<int64_t> o;
  AfterSplit ht{splits[0], *b};
  record_input_faces(ht, target->s, 0, out_faces, o);
  for (size_t i = 0; i < ntools; ++i) {
    AfterSplit hs{splits[i + 1], *b};
    record_input_faces(hs, tools[i]->s, int64_t(i) + 1, out_faces, o);
  }
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

int gs_chamfer_asym(const GsShape* s, const uint32_t* edges, const uint32_t* faces, size_t nedges, int mode, double a,
                    double b, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || (!edges && nedges) || (!faces && nedges)) return fail("null argument");
  if (nedges == 0) return fail("no edges selected");
  if (mode != 1 && mode != 2) return fail("unknown chamfer mode");
  if (!std::isfinite(a) || a <= 0) return fail("chamfer distance must be positive");
  if (mode == 1 && (!std::isfinite(b) || b <= 0)) return fail("chamfer distance must be positive");
  if (mode == 2 && (!std::isfinite(b) || b <= 0 || b >= M_PI / 2)) return fail("chamfer angle must be between 0 and 90 degrees");
  ShapeMap face_map;
  TopExp::MapShapes(s->s, TopAbs_FACE, face_map);
  std::vector<BlendEdge> es;
  std::vector<uint32_t> idx;
  for (size_t i = 0; i < nedges; ++i) {
    if (faces[i] >= uint32_t(face_map.Extent())) return fail("face index out of range");
    TopoDS_Edge e = edge_by_index(s->s, edges[i]);
    TopoDS_Face f = TopoDS::Face(face_map(int(faces[i]) + 1));
    es.push_back(BlendEdge{e, f, a, b, mode});
    idx.push_back(edges[i]);
  }
  return blend_edges<BRepFilletAPI_MakeChamfer>(s, es, idx, false, out, origins);
  GS_GUARD_END
}

namespace {

// Outward unit normal of a planar face, or false when the face is not planar.
bool planar_face(const TopoDS_Face& f, gp_Dir& n, gp_Pnt& p) {
  BRepAdaptor_Surface sf(f, false);
  if (sf.GetType() != GeomAbs_Plane) return false;
  gp_Pln pl = sf.Plane();
  n = pl.Axis().Direction();
  if (f.Orientation() == TopAbs_REVERSED) n.Reverse();
  p = pl.Location();
  return true;
}

// The two removal corners of a full round on `m` (ADR 0004 amendment): a prism along the
// round's axis whose section is the rectangle across the wall, from just outside the middle
// face down to the round's centre line, minus the half disc of the round.
struct RoundCutter {
  TopoDS_Shape shape;
  std::vector<uint32_t> edges;  // edges of the middle face shared with its two sides
};

RoundCutter full_round_cutter(const TopoDS_Shape& body, const ShapeMap& face_map, const AncMap& anc, uint32_t fi) {
  if (face_map.Extent() <= 0 || fi >= uint32_t(face_map.Extent())) throw Standard_Failure("face index out of range");
  TopoDS_Face m = TopoDS::Face(face_map(int(fi) + 1));
  gp_Dir nm;
  gp_Pnt pm;
  if (!planar_face(m, nm, pm)) throw Standard_Failure("a full round needs a planar middle face");
  // The planar neighbours of the middle face.
  std::vector<TopoDS_Face> nb;
  for (TopExp_Explorer ex(m, TopAbs_EDGE); ex.More(); ex.Next()) {
    int ei = anc.FindIndex(ex.Current());
    if (ei <= 0) continue;
    for (ShapeList::Iterator it(anc(ei)); it.More(); it.Next()) {
      if (it.Value().IsSame(m)) continue;
      bool seen = false;
      for (const auto& g : nb) seen = seen || g.IsSame(it.Value());
      if (!seen) nb.push_back(TopoDS::Face(it.Value()));
    }
  }
  // The pair of parallel sides with the smallest distance whose planes bound the wall.
  const double tol = 1e-7;
  int ia = -1, ib = -1;
  double best = 0;
  gp_Dir na, nbb;
  gp_Pnt pa, pb;
  for (size_t i = 0; i < nb.size(); ++i) {
    gp_Dir ni;
    gp_Pnt pi;
    if (!planar_face(nb[i], ni, pi)) continue;
    if (std::fabs(ni.Dot(nm)) > 1e-6) continue;
    for (size_t j = i + 1; j < nb.size(); ++j) {
      gp_Dir nj;
      gp_Pnt pj;
      if (!planar_face(nb[j], nj, pj)) continue;
      if (ni.Dot(nj) > -1.0 + tol) continue;
      double t = -gp_Vec(pi, pj).Dot(gp_Vec(ni));
      if (t < 1e-6) continue;
      if (ia < 0 || t < best - 1e-9) {
        ia = int(i);
        ib = int(j);
        best = t;
        na = ni;
        nbb = nj;
        pa = pi;
        pb = pj;
      }
    }
  }
  (void)nbb;
  (void)pb;
  if (ia < 0)
    throw Standard_Failure(
        "a full round needs a planar middle face between two parallel planar faces, perpendicular to both");
  const double t = best, r = t / 2;
  gp_Vec vu = gp_Vec(nm).Crossed(gp_Vec(na));
  gp_Dir u(vu);
  gp_Vec w = gp_Vec(na).Reversed();    // across the wall, from the first side
  gp_Vec dd = gp_Vec(nm).Reversed();   // into the material
  // A point on the middle plane and the first side's plane, at the origin along the axis.
  gp_Vec toa = gp_Vec(pm, pa);
  gp_Pnt o = pm.Translated(gp_Vec(na).Multiplied(toa.Dot(gp_Vec(na))));
  // Extent of the middle face along the axis.
  double umin = 1e300, umax = -1e300;
  auto take = [&](const gp_Pnt& p) {
    double s = gp_Vec(o, p).Dot(gp_Vec(u));
    umin = std::min(umin, s);
    umax = std::max(umax, s);
  };
  for (TopExp_Explorer ex(m, TopAbs_EDGE); ex.More(); ex.Next()) {
    const TopoDS_Edge& e = TopoDS::Edge(ex.Current());
    if (BRep_Tool::Degenerated(e)) continue;
    BRepAdaptor_Curve c(e);
    for (int k = 0; k <= 32; ++k) take(c.Value(c.FirstParameter() + (c.LastParameter() - c.FirstParameter()) * k / 32.0));
  }
  if (!(umax - umin > 1e-6)) throw Standard_Failure("the middle face has no extent");
  const double delta = 0.25 * r;
  auto P = [&](double x, double y) {
    return o.Translated(w.Multiplied(x) + dd.Multiplied(y) + gp_Vec(u).Multiplied(umin));
  };
  BRepBuilderAPI_MakeWire mw;
  mw.Add(BRepBuilderAPI_MakeEdge(P(0, -delta), P(t, -delta)).Edge());
  mw.Add(BRepBuilderAPI_MakeEdge(P(t, -delta), P(t, r)).Edge());
  GC_MakeArcOfCircle arc(P(t, r), P(r, 0), P(0, r));
  if (!arc.IsDone()) throw Standard_Failure("could not build the round");
  mw.Add(BRepBuilderAPI_MakeEdge(arc.Value()).Edge());
  mw.Add(BRepBuilderAPI_MakeEdge(P(0, r), P(0, -delta)).Edge());
  if (!mw.IsDone()) throw Standard_Failure("could not build the round");
  BRepBuilderAPI_MakeFace mf(mw.Wire(), true);
  if (!mf.IsDone()) throw Standard_Failure("could not build the round");
  BRepPrimAPI_MakePrism prism(mf.Face(), gp_Vec(u).Multiplied(umax - umin));
  prism.Build();
  if (!prism.IsDone()) throw Standard_Failure("could not build the round");
  RoundCutter rc;
  rc.shape = prism.Shape();
  if (signed_volume(rc.shape) < 0) rc.shape.Reverse();
  // The middle face's edges on the two sides.
  ShapeMap edge_map;
  TopExp::MapShapes(body, TopAbs_EDGE, edge_map);
  for (TopExp_Explorer ex(m, TopAbs_EDGE); ex.More(); ex.Next()) {
    int ei = anc.FindIndex(ex.Current());
    if (ei <= 0) continue;
    for (ShapeList::Iterator it(anc(ei)); it.More(); it.Next()) {
      if (it.Value().IsSame(nb[size_t(ia)]) || it.Value().IsSame(nb[size_t(ib)])) {
        int k = edge_map.FindIndex(ex.Current());
        if (k > 0) rc.edges.push_back(uint32_t(k - 1));
      }
    }
  }
  return rc;
}

// One end of a cylindrical face along its axis, for `offset_cylinders`: on a plane across the
// axis, or on a round (a torus about the axis, tangent to the face) that ends on such a plane.
struct CylinderEnd {
  bool round = false;
  double z = 0;               // where the face ends, along the axis
  double rc = 0, rho = 0;     // a round's centre circle radius and its radius
  double zp = 0;              // the plane the round ends on, along the axis
  std::vector<TopoDS_Face> tori;
};

// One cylindrical face offset as a boolean: the section between the face (and its rounds) and
// their offset places, turned about the axis.
struct CylinderOffset {
  TopoDS_Face face;
  gp_Cylinder cyl;
  double area = 0;  // the face's
  CylinderEnd ends[2];
  TopoDS_Shape ring;
  TopoDS_Shape outer;                 // the ring's face on the offset radius
  std::vector<TopoDS_Shape> rounds;  // the ring's faces on the moved rounds, per end (or null)
  std::vector<TopoDS_Face> planes;   // the planes around the face and its rounds
};

// Along the cylinder's axis from its position's origin.
double along(const gp_Cylinder& c, const gp_Pnt& p) {
  return gp_Vec(c.Position().Location(), p).Dot(gp_Vec(c.Position().Direction()));
}

// The faces of `body` across edge `e` from `f`.
TopoDS_Face face_across(const AncMap& anc, const TopoDS_Edge& e, const TopoDS_Face& f) {
  const int k = anc.FindIndex(e);
  if (k <= 0) return TopoDS_Face();
  for (ShapeList::Iterator it(anc(k)); it.More(); it.Next())
    if (!it.Value().IsSame(f)) return TopoDS::Face(it.Value());
  return TopoDS_Face();
}

// Is `pl` a plane across the cylinder's axis? Then `z` is where, along the axis.
bool plane_across(const gp_Cylinder& c, const TopoDS_Face& pl, double& z) {
  BRepAdaptor_Surface s(pl, false);
  if (s.GetType() != GeomAbs_Plane || !s.Plane().Axis().IsParallel(c.Axis(), 1e-9)) return false;
  z = along(c, s.Plane().Location());
  return true;
}

// Is `f` a cylinder whose every edge ends it along its axis, on a plane across the axis or on a
// tangent round that ends on such a plane, or runs along the axis between it and a plane along
// the axis? Then offsetting it only moves those rounds with it, and grows or shrinks its
// neighbouring planes.
bool offsettable_cylinder(const AncMap& anc, const TopoDS_Face& f, CylinderOffset& c) {
  BRepAdaptor_Surface sf(f, true);
  if (sf.GetType() != GeomAbs_Cylinder) return false;
  c.cyl = sf.Cylinder();
  const gp_Dir z = c.cyl.Axis().Direction();
  const gp_Lin axis(c.cyl.Axis());
  double u0 = 0, u1 = 0, v0 = 0, v1 = 0;
  BRepTools::UVBounds(f, u0, u1, v0, v1);
  c.ends[0].z = v0;
  c.ends[1].z = v1;
  bool seen[2] = {false, false};
  for (TopExp_Explorer ex(f, TopAbs_EDGE); ex.More(); ex.Next()) {
    const TopoDS_Edge e = TopoDS::Edge(ex.Current());
    if (BRep_Tool::Degenerated(e)) continue;
    const TopoDS_Face other = face_across(anc, e, f);
    if (other.IsNull()) continue;  // the seam
    BRepAdaptor_Curve cv(e);
    if (cv.GetType() == GeomAbs_Line) {
      BRepAdaptor_Surface so(other, false);
      if (!cv.Line().Direction().IsParallel(z, 1e-9) || so.GetType() != GeomAbs_Plane ||
          !so.Plane().Axis().Direction().IsNormal(z, 1e-9))
        return false;
      c.planes.push_back(other);
      continue;
    }
    if (cv.GetType() != GeomAbs_Circle || !cv.Circle().Axis().IsParallel(c.cyl.Axis(), 1e-9) ||
        axis.Distance(cv.Circle().Location()) > 1e-7)
      return false;
    const double ze = along(c.cyl, cv.Circle().Location());
    const int k = std::fabs(ze - v0) <= 1e-7 ? 0 : std::fabs(ze - v1) <= 1e-7 ? 1 : -1;
    double zp = 0;
    // Between the ends: a step of a plane across the axis (1054: the slots' roofs) whose
    // other side the ring passes through empty.
    if (k < 0) {
      if (!plane_across(c.cyl, other, zp)) return false;
      c.planes.push_back(other);
      continue;
    }
    CylinderEnd& end = c.ends[k];
    if (plane_across(c.cyl, other, zp)) {
      if (seen[k] && end.round) return false;
      seen[k] = true;
      c.planes.push_back(other);
      continue;
    }
    // A round: a torus about the axis, its centre circle at the edge's height, tangent to the
    // face, ending on a plane across the axis a radius away.
    BRepAdaptor_Surface so(other, false);
    if (so.GetType() != GeomAbs_Torus) return false;
    const gp_Torus t = so.Torus();
    if (!t.Axis().IsParallel(c.cyl.Axis(), 1e-9) || axis.Distance(t.Location()) > 1e-7) return false;
    const double rc = t.MajorRadius(), rho = t.MinorRadius();
    if (std::fabs(along(c.cyl, t.Location()) - ze) > 1e-7 || std::fabs(std::fabs(rc - c.cyl.Radius()) - rho) > 1e-7)
      return false;
    double zt = 0;
    bool ends_on_plane = false;
    for (TopExp_Explorer tx(other, TopAbs_EDGE); tx.More(); tx.Next()) {
      const TopoDS_Edge te = TopoDS::Edge(tx.Current());
      if (BRep_Tool::Degenerated(te)) continue;
      const TopoDS_Face beyond = face_across(anc, te, other);
      if (beyond.IsNull() || beyond.IsSame(f)) continue;
      BRepAdaptor_Curve tc(te);
      if (tc.GetType() != GeomAbs_Circle) return false;
      if (std::fabs(along(c.cyl, tc.Circle().Location()) - ze) <= 1e-7) {
        if (!beyond.IsSame(f)) return false;  // another face along the round's start
        continue;
      }
      double zq = 0;
      if (!plane_across(c.cyl, beyond, zq) || std::fabs(std::fabs(zq - ze) - rho) > 1e-7) return false;
      c.planes.push_back(beyond);
      if (ends_on_plane && std::fabs(zq - zt) > 1e-7) return false;
      zt = zq;
      ends_on_plane = true;
    }
    if (!ends_on_plane) return false;
    if (seen[k] && (!end.round || std::fabs(end.rc - rc) > 1e-7 || std::fabs(end.rho - rho) > 1e-7 ||
                    std::fabs(end.zp - zt) > 1e-7))
      return false;
    seen[k] = true;
    end.round = true;
    end.rc = rc;
    end.rho = rho;
    end.zp = zt;
    bool known = false;
    for (const auto& x : end.tori) known = known || x.IsSame(other);
    if (!known) end.tori.push_back(other);
  }
  if (!seen[0] || !seen[1]) return false;
  GProp_GProps g;
  BRepGProp::SurfaceProperties(f, g);
  c.area = g.Mass();
  return c.area > 0;
}

// Offsets cylindrical faces by `d` along their outward normals as booleans, where BRepOffset
// fails (3350: a pocket's two walls in a threaded body, whose every face BRepOffset offsets and
// fails on; 1054: a hole beside two slots, BRepOffset gives no shape). Each face's section (from
// its radius to the offset one, over its length; with a round at an end, from the round to the
// same round on the moved face, as a fillet of the moved edge makes it) is turned about the axis
// and cut away (an offset into the material) or added. The material taken (the empty space
// filled) must be bounded on the face's own cylinder by exactly the face, and on its rounds'
// tori by exactly the rounds (their areas), so nothing else changes but the planes around them,
// which grow or shrink.
int offset_cylinders(const GsShape* s, const std::vector<TopoDS_Face>& fs, const std::vector<uint32_t>& idx, double d,
                     GsShape** out, GsBuf* origins) {
  AncMap anc;
  TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<CylinderOffset> cs;
  ShapeList tools;
  for (const auto& f : fs) {
    CylinderOffset c;
    c.face = f;
    if (!offsettable_cylinder(anc, f, c)) return fail("not a cylinder between planes");
    // The outward normal points away from the axis on a shaft, towards it in a hole.
    double u0 = 0, u1 = 0, v0 = 0, v1 = 0;
    BRepTools::UVBounds(f, u0, u1, v0, v1);
    const gp_Pnt p = BRepAdaptor_Surface(f, false).Value(0.5 * (u0 + u1), 0.5 * (v0 + v1));
    gp_Dir n;
    if (!outward_normal(f, p, n)) return fail("no normal");
    const gp_Vec radial(gp_Lin(c.cyl.Axis()).Normal(p).Direction());
    const double away = gp_Vec(n).Dot(radial) > 0 ? 1.0 : -1.0;
    const double r = c.cyl.Radius(), r1 = r + away * d, shift = r1 - r;
    if (!(r1 > 1e-6)) return fail("the offset radius is not positive");
    for (const auto& e : c.ends)
      if (e.round && !(e.rc + shift > 1e-6 && std::fabs(shift) < e.rho)) return fail("the round would turn over");
    // The section in the half plane of the axis and the position's x direction: (radius,
    // height) -> point. Around: the old profile up one side, the new one down the other.
    const gp_Ax3& pos = c.cyl.Position();
    const gp_Pnt o = pos.Location();
    auto at = [&](double rr, double h) {
      return o.Translated(gp_Vec(pos.XDirection()) * rr + gp_Vec(pos.Direction()) * h);
    };
    BRepBuilderAPI_MakeWire mw;
    std::vector<TopoDS_Edge> walls, arcs(2);
    auto line = [&](const gp_Pnt& a, const gp_Pnt& b) {
      if (a.Distance(b) < 1e-9) return TopoDS_Edge();
      TopoDS_Edge e = BRepBuilderAPI_MakeEdge(a, b).Edge();
      mw.Add(e);
      return e;
    };
    auto arc = [&](double rc, double zc, double rho, double ra, double za, double rb, double zb) {
      // The quarter circle about (rc, zc) from (ra, za) to (rb, zb).
      gp_Vec2d m(ra - rc + rb - rc, za - zc + zb - zc);
      m.Normalize();
      GC_MakeArcOfCircle a(at(ra, za), at(rc + rho * m.X(), zc + rho * m.Y()), at(rb, zb));
      if (!a.IsDone()) throw Standard_Failure("no section");
      TopoDS_Edge e = BRepBuilderAPI_MakeEdge(a.Value()).Edge();
      mw.Add(e);
      return e;
    };
    // The profile at radius `rr` (the round's centre moved by `sh`), from end 0 to end 1, or back.
    auto profile = [&](double rr, double sh, bool forward, TopoDS_Edge& wall, TopoDS_Edge* rounds) {
      const CylinderEnd& a = c.ends[forward ? 0 : 1];
      const CylinderEnd& b = c.ends[forward ? 1 : 0];
      auto top = [&](const CylinderEnd& e) { return e.round ? e.rc + sh : rr; };
      auto htop = [&](const CylinderEnd& e) { return e.round ? e.zp : e.z; };
      gp_Pnt cur = at(top(a), htop(a));
      if (a.round) {
        rounds[forward ? 0 : 1] = arc(a.rc + sh, a.z, a.rho, a.rc + sh, a.zp, rr, a.z);
        cur = at(rr, a.z);
      }
      wall = line(cur, at(rr, b.z));
      if (b.round) rounds[forward ? 1 : 0] = arc(b.rc + sh, b.z, b.rho, rr, b.z, b.rc + sh, b.zp);
      return at(top(b), htop(b));
    };
    TopoDS_Edge old_wall, new_wall, old_rounds[2], new_rounds[2];
    const gp_Pnt start = at(c.ends[0].round ? c.ends[0].rc : r, c.ends[0].round ? c.ends[0].zp : c.ends[0].z);
    const gp_Pnt mid = profile(r, 0, true, old_wall, old_rounds);
    const gp_Pnt far_end = at(c.ends[1].round ? c.ends[1].rc + shift : r1, c.ends[1].round ? c.ends[1].zp : c.ends[1].z);
    line(mid, far_end);
    const gp_Pnt back = profile(r1, shift, false, new_wall, new_rounds);
    line(back, start);
    if (!mw.IsDone()) return fail("no section");
    BRepBuilderAPI_MakeFace mf(mw.Wire(), true);
    if (!mf.IsDone()) return fail("no section");
    BRepPrimAPI_MakeRevol rev(mf.Face(), pos.Axis(), 2 * M_PI);
    rev.Build();
    if (!rev.IsDone()) return fail("no ring");
    c.ring = rev.Shape();
    if (signed_volume(c.ring) < 0) c.ring.Reverse();
    // The ring's face turned from a section edge (the wire may hold copies of the edges made).
    auto made = [&](const TopoDS_Edge& e) {
      if (e.IsNull()) return TopoDS_Shape();
      auto mid_of = [](const TopoDS_Edge& x) {
        BRepAdaptor_Curve cv(x);
        return cv.Value(0.5 * (cv.FirstParameter() + cv.LastParameter()));
      };
      const gp_Pnt m = mid_of(e);
      for (TopExp_Explorer ex(mf.Face(), TopAbs_EDGE); ex.More(); ex.Next()) {
        if (mid_of(TopoDS::Edge(ex.Current())).Distance(m) > 1e-9) continue;
        const ShapeList& g = rev.Generated(ex.Current());
        if (!g.IsEmpty()) return g.First();
      }
      return TopoDS_Shape();
    };
    c.outer = made(new_wall);
    if (c.outer.IsNull()) return fail("no ring");
    for (int k = 0; k < 2; ++k) c.rounds.push_back(made(new_rounds[k]));
    tools.Append(c.ring);
    cs.push_back(c);
  }
  const bool into = d < 0;
  // What changes: the rings' material (into the body) or their empty space (out of it); bounded
  // on each face's cylinder by just that face, on its rounds' tori by just those rounds.
  double change = 0;
  for (const auto& c : cs) {
    std::unique_ptr<BRepAlgoAPI_BooleanOperation> part;
    if (into) part = std::make_unique<BRepAlgoAPI_Common>(c.ring, s->s);
    else part = std::make_unique<BRepAlgoAPI_Cut>(c.ring, s->s);
    if (!part->IsDone() || part->HasErrors()) return fail("the ring's material could not be found");
    const TopoDS_Shape x = part->Shape();
    change += std::fabs(signed_volume(x));
    double on = 0, want = c.area;
    std::vector<gp_Torus> tori;
    for (const auto& e : c.ends)
      for (const auto& t : e.tori) {
        GProp_GProps g;
        BRepGProp::SurfaceProperties(t, g);
        want += g.Mass();
        tori.push_back(BRepAdaptor_Surface(t, false).Torus());
      }
    const gp_Lin axis(c.cyl.Axis());
    for (TopExp_Explorer ex(x, TopAbs_FACE); ex.More(); ex.Next()) {
      BRepAdaptor_Surface sf(TopoDS::Face(ex.Current()), false);
      bool mine = false;
      if (sf.GetType() == GeomAbs_Cylinder) {
        const gp_Cylinder y = sf.Cylinder();
        mine = std::fabs(y.Radius() - c.cyl.Radius()) <= 1e-7 && y.Axis().IsParallel(c.cyl.Axis(), 1e-9) &&
               axis.Distance(y.Location()) <= 1e-7;
      } else if (sf.GetType() == GeomAbs_Torus) {
        const gp_Torus y = sf.Torus();
        for (const auto& t : tori)
          mine = mine || (std::fabs(y.MajorRadius() - t.MajorRadius()) <= 1e-7 &&
                          std::fabs(y.MinorRadius() - t.MinorRadius()) <= 1e-7 && y.Location().Distance(t.Location()) <= 1e-7);
      }
      if (!mine) continue;
      GProp_GProps g;
      BRepGProp::SurfaceProperties(ex.Current(), g);
      on += g.Mass();
    }
    if (std::fabs(on - want) > 1e-6 * std::max(1.0, want)) return fail("the offset reaches past the face");
  }
  ShapeList args;
  args.Append(s->s);
  auto b = run_boolean_checked(into ? GS_BOOL_CUT : GS_BOOL_UNION, args, tools);
  const TopoDS_Shape result = b->Shape();
  if (result.IsNull()) return fail("no result");
  const double before = std::fabs(signed_volume(s->s)), after = std::fabs(signed_volume(result));
  if (std::fabs(std::fabs(after - before) - change) > 1e-7 * std::max(1.0, before))
    return fail("the offset changed more than the rings");
  ShapeMap out_faces, in_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  std::vector<int64_t> o;
  record_input_faces(*b, s->s, 0, out_faces, o);
  // The moved face and rounds stand for the ones they replace.
  auto stands_for = [&](const TopoDS_Shape& made, const TopoDS_Shape& was) {
    const int fi = in_faces.FindIndex(was);
    if (made.IsNull() || fi <= 0) return false;
    std::vector<TopoDS_Shape> res;
    const ShapeList& mods = b->Modified(made);
    for (ShapeList::Iterator it(mods); it.More(); it.Next()) res.push_back(it.Value());
    if (mods.IsEmpty() && !b->IsDeleted(made)) res.push_back(made);
    bool found = false;
    for (const auto& g : res) {
      const int k = out_faces.FindIndex(g);
      if (k <= 0) continue;
      found = true;
      push_origin(o, k - 1, GS_ORIGIN_INPUT, 0, fi - 1);
    }
    return found;
  };
  for (const auto& c : cs) {
    if (!stands_for(c.outer, c.face)) return fail("the offset face is missing");
    for (int k = 0; k < 2; ++k)
      for (const auto& t : c.ends[k].tori) stands_for(c.rounds[size_t(k)], t);
  }
  // A piece of a ring's side left beside a plane it grows (the boolean's unification refused,
  // as on 3350's threaded body, whose own faults BRepCheck reports): that plane's.
  std::vector<bool> has(size_t(out_faces.Extent()), false);
  for (size_t k = 0; k + 3 < o.size(); k += 4)
    if (o[k] >= 0 && o[k] < int64_t(has.size())) has[size_t(o[k])] = true;
  for (int k = 1; k <= out_faces.Extent(); ++k) {
    if (has[size_t(k - 1)]) continue;
    BRepAdaptor_Surface sf(TopoDS::Face(out_faces(k)), false);
    if (sf.GetType() != GeomAbs_Plane) continue;
    const gp_Pln q = sf.Plane();
    bool done = false;
    for (const auto& c : cs)
      for (const auto& pl : c.planes) {
        if (done) break;
        const gp_Pln w = BRepAdaptor_Surface(pl, false).Plane();
        if (!w.Axis().IsParallel(q.Axis(), 1e-9) || w.Distance(q.Location()) > 1e-7) continue;
        const int fi = in_faces.FindIndex(pl);
        if (fi <= 0) continue;
        push_origin(o, k - 1, GS_ORIGIN_INPUT, 0, fi - 1);
        done = true;
      }
  }
  check_valid(result, "offset");
  to_buf(o, origins);
  *out = new GsShape{result};
  (void)idx;
  return GS_OK;
}

}  // namespace

// Replaces each middle face in `faces` by a half cylinder across the wall between its two
// parallel sides (a full round). The faces refer to `s`.
int gs_full_round(const GsShape* s, const uint32_t* faces, size_t nfaces, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || (!faces && nfaces)) return fail("null argument");
  if (nfaces == 0) return fail("no faces selected");
  ShapeMap face_map;
  TopExp::MapShapes(s->s, TopAbs_FACE, face_map);
  AncMap anc;
  TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
  std::vector<RoundCutter> cutters;
  ShapeList args, tools;
  args.Append(s->s);
  for (size_t i = 0; i < nfaces; ++i) {
    cutters.push_back(full_round_cutter(s->s, face_map, anc, faces[i]));
    tools.Append(cutters.back().shape);
  }
  auto b = run_boolean_checked(GS_BOOL_CUT, args, tools);
  TopoDS_Shape result = b->Shape();
  if (result.IsNull()) return fail("the full round produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  if (out_faces.Extent() == 0) return fail("the full round removed all material");
  for (size_t i = 0; i < nfaces; ++i)
    if (out_faces.Contains(face_map(int(faces[i]) + 1))) return fail("the full round did not replace the face");
  std::vector<int64_t> o;
  record_input_faces(*b, s->s, 0, out_faces, o);
  for (const auto& c : cutters) {
    bool found = false;
    ShapeMap cf;
    TopExp::MapShapes(c.shape, TopAbs_FACE, cf);
    for (int j = 1; j <= cf.Extent(); ++j) {
      if (surface_kind(TopoDS::Face(cf(j))) != 1) continue;
      const ShapeList& mods = b->Modified(cf(j));
      std::vector<TopoDS_Shape> res;
      for (ShapeList::Iterator it(mods); it.More(); it.Next()) res.push_back(it.Value());
      if (mods.IsEmpty() && !b->IsDeleted(cf(j))) res.push_back(cf(j));
      for (const auto& g : res) {
        int idx = out_faces.FindIndex(g);
        if (idx <= 0) continue;
        found = true;
        for (uint32_t e : c.edges) push_origin(o, idx - 1, GS_ORIGIN_FROM_EDGE, 0, e);
      }
    }
    if (!found) return fail("the full round produced no rounded face");
  }
  check_valid(result, "full round");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

// The shell of a solid of revolution (inward wall `d` > 0, opened at planar faces across the
// axis), for when OCCT's thick solid fails (3722's dome: a spindle torus whose offset meets the
// axis in a point). The cavity is the solid's section through the axis, carried on through the
// opened faces, offset inward by `d` in its plane and revolved; the shell is the solid less it.
// Null when the solid is not one of revolution (Pappus on two half sections), an opened face is
// not a plane across the axis, the section has a concave corner off the axis (where OCCT's 2D
// offset would round what the 3D one extends) or a hole, or OCCT's 2D offset fails.
static TopoDS_Shape revolved_cavity(const TopoDS_Shape& solid, const ShapeList& open, double d) {
  if (!(d > 0)) return TopoDS_Shape();
  // The axis: of the first face of revolution; every other such face on it, every plane across it.
  bool have = false;
  gp_Ax1 ax;
  for (TopExp_Explorer ex(solid, TopAbs_FACE); ex.More(); ex.Next()) {
    BRepAdaptor_Surface s(TopoDS::Face(ex.Current()), false);
    gp_Ax1 a;
    switch (s.GetType()) {
      case GeomAbs_Cylinder: a = s.Cylinder().Axis(); break;
      case GeomAbs_Cone: a = s.Cone().Axis(); break;
      case GeomAbs_Sphere: a = s.Sphere().Position().Axis(); break;
      case GeomAbs_Torus: a = s.Torus().Axis(); break;
      case GeomAbs_SurfaceOfRevolution: a = s.AxeOfRevolution(); break;
      case GeomAbs_Plane:
        if (have && std::fabs(std::fabs(s.Plane().Axis().Direction().Dot(ax.Direction())) - 1) > 1e-9)
          return TopoDS_Shape();
        continue;
      default: continue;
    }
    if (!have) {
      ax = a;
      have = true;
    } else if (!ax.IsParallel(a, 1e-9) || gp_Lin(ax).Distance(a.Location()) > 1e-7) {
      return TopoDS_Shape();
    }
  }
  if (!have) return TopoDS_Shape();
  const gp_Dir z = ax.Direction();
  const gp_Dir any = std::fabs(z.X()) < 0.9 ? gp_Dir(1, 0, 0) : gp_Dir(0, 1, 0);
  const gp_Dir n0(gp_Vec(z).Crossed(gp_Vec(any)));
  const double L = 1e5;
  // The meridian plane at `angle` about the axis: x radial, y along -z.
  auto meridian = [&](double angle) {
    gp_Dir n = n0.Rotated(ax, angle);
    return gp_Pln(gp_Ax3(ax.Location(), n, gp_Dir(gp_Vec(n).Crossed(gp_Vec(z)))));
  };
  GProp_GProps vp;
  BRepGProp::VolumeProperties(solid, vp);
  const double vol = vp.Mass();
  if (!(vol > 0)) return TopoDS_Shape();
  for (double angle : {0.0, 2.0}) {
    gp_Pln m = meridian(angle);
    TopoDS_Shape half = BRepAlgoAPI_Common(solid, BRepBuilderAPI_MakeFace(m, 0, L, -L, L).Face()).Shape();
    GProp_GProps sp;
    BRepGProp::SurfaceProperties(half, sp);
    double r = gp_Vec(ax.Location(), sp.CentreOfMass()).Dot(gp_Vec(m.Position().XDirection()));
    if (std::fabs(2 * M_PI * sp.Mass() * r - vol) > 1e-5 * vol) return TopoDS_Shape();
  }
  const gp_Pln mer = meridian(0.0);
  const gp_Dir rx = mer.Position().XDirection();
  const TopoDS_Face big = BRepBuilderAPI_MakeFace(mer, -L, L, -L, L).Face();
  TopoDS_Shape sec = BRepAlgoAPI_Common(solid, big).Shape();
  // Carry the section on through each opened face: a rectangle 2d deep beyond the face, along
  // the axis, on each piece of the face's trace.
  for (ShapeList::Iterator it(open); it.More(); it.Next()) {
    const TopoDS_Face of = TopoDS::Face(it.Value());
    BRepAdaptor_Surface s(of, false);
    // A plane across the axis: points of its surface at one height along the axis (a line
    // across the axis revolved stays a surface of revolution in OCCT's revolve).
    double fu0, fu1, fv0, fv1;
    BRepTools::UVBounds(of, fu0, fu1, fv0, fv1);
    const gp_Pnt base = s.Value(fu0, fv0);
    for (int a = 0; a <= 4; ++a)
      for (int b = 0; b <= 4; ++b) {
        const gp_Pnt q = s.Value(fu0 + (fu1 - fu0) * a / 4, fv0 + (fv1 - fv0) * b / 4);
        if (std::fabs(gp_Vec(base, q).Dot(gp_Vec(z))) > 1e-6) return TopoDS_Shape();
      }
    BRepGProp_Face gf(of);
    double u0, u1, v0, v1;
    gf.Bounds(u0, u1, v0, v1);
    gp_Pnt pp;
    gp_Vec nv;
    gf.Normal(0.5 * (u0 + u1), 0.5 * (v0 + v1), pp, nv);
    if (nv.Magnitude() < 1e-12) return TopoDS_Shape();
    const double up = gp_Dir(nv).Dot(z);
    if (std::fabs(std::fabs(up) - 1) > 1e-9) return TopoDS_Shape();
    const double h = gp_Vec(ax.Location(), pp).Dot(gp_Vec(z));
    const double s0 = up > 0 ? h : h - 2 * d, s1 = up > 0 ? h + 2 * d : h;
    TopoDS_Shape trace = BRepAlgoAPI_Section(of, big).Shape();
    int pieces = 0;
    for (TopExp_Explorer ee(trace, TopAbs_EDGE); ee.More(); ee.Next()) {
      TopoDS_Vertex a, b;
      TopExp::Vertices(TopoDS::Edge(ee.Current()), a, b);
      if (a.IsNull() || b.IsNull()) return TopoDS_Shape();
      double lo = gp_Vec(ax.Location(), BRep_Tool::Pnt(a)).Dot(gp_Vec(rx));
      double hi = gp_Vec(ax.Location(), BRep_Tool::Pnt(b)).Dot(gp_Vec(rx));
      if (lo > hi) std::swap(lo, hi);
      if (hi - lo < 1e-9) continue;
      gp_Pnt o = ax.Location();
      auto P = [&](double r, double t) { return o.Translated(gp_Vec(rx) * r + gp_Vec(z) * t); };
      BRepBuilderAPI_MakePolygon poly(P(lo, s0), P(hi, s0), P(hi, s1), P(lo, s1), true);
      if (!poly.IsDone()) return TopoDS_Shape();
      sec = BRepAlgoAPI_Fuse(sec, BRepBuilderAPI_MakeFace(mer, poly.Wire()).Face()).Shape();
      ++pieces;
    }
    if (pieces == 0) return TopoDS_Shape();
  }
  ShapeUpgrade_UnifySameDomain unify(sec, true, true, false);
  unify.Build();
  sec = unify.Shape();
  TopoDS_Shape cavity;
  const TopoDS_Face right = BRepBuilderAPI_MakeFace(mer, 0, L, -L, L).Face();
  for (TopExp_Explorer fe(sec, TopAbs_FACE); fe.More(); fe.Next()) {
    const TopoDS_Face sf = TopoDS::Face(fe.Current());
    int wires = 0;
    for (TopExp_Explorer we(sf, TopAbs_WIRE); we.More(); we.Next()) ++wires;
    if (wires != 1) return TopoDS_Shape();
    // The section's edges as lines and arcs (the boolean gives B-splines), joined anew.
    occ::handle<TopTools_HSequenceOfShape> edges = new TopTools_HSequenceOfShape, joined;
    for (BRepTools_WireExplorer we(BRepTools::OuterWire(sf), sf); we.More(); we.Next()) {
      TopoDS_Edge e = we.Current();
      double f0, f1;
      occ::handle<Geom_Curve> c = BRep_Tool::Curve(e, f0, f1);
      if (c.IsNull()) return TopoDS_Shape();
      GeomAdaptor_Curve gc(c);
      if (gc.GetType() != GeomAbs_Line && gc.GetType() != GeomAbs_Circle) {
        GeomConvert_CurveToAnaCurve conv(c);
        occ::handle<Geom_Curve> nc;
        double g0, g1;
        if (!conv.ConvertToAnalytical(1e-6, nc, f0, f1, g0, g1) || nc.IsNull()) return TopoDS_Shape();
        e = BRepBuilderAPI_MakeEdge(nc, g0, g1).Edge();
      }
      edges->Append(e);
    }
    ShapeAnalysis_FreeBounds::ConnectEdgesToWires(edges, 1e-6, false, joined);
    if (joined.IsNull() || joined->Length() != 1) return TopoDS_Shape();
    BRepBuilderAPI_MakeFace mf(mer, TopoDS::Wire(joined->Value(1)));
    if (!mf.IsDone()) return TopoDS_Shape();
    TopoDS_Face af = mf.Face();
    BRepCheck_Analyzer fa(af);
    if (!fa.IsValid()) return TopoDS_Shape();
    // A concave corner off the axis: the 2D offset rounds the wall's inside there, the 3D one
    // extends the faces; one on the axis (the dimple of a spindle torus) is rounded over a
    // width of microns at radius 0, which the volume does not see.
    bool concave = false;
    std::vector<TopoDS_Edge> ring;
    for (BRepTools_WireExplorer we(BRepTools::OuterWire(af), af); we.More(); we.Next()) ring.push_back(we.Current());
    for (size_t i = 0; i < ring.size(); ++i) {
      const TopoDS_Edge& e1 = ring[i];
      const TopoDS_Edge& e2 = ring[(i + 1) % ring.size()];
      BRepAdaptor_Curve c1(e1), c2(e2);
      double t1 = e1.Orientation() == TopAbs_REVERSED ? c1.FirstParameter() : c1.LastParameter();
      double t2 = e2.Orientation() == TopAbs_REVERSED ? c2.LastParameter() : c2.FirstParameter();
      gp_Pnt p;
      gp_Vec d1, d2;
      c1.D1(t1, p, d1);
      c2.D1(t2, p, d2);
      if (e1.Orientation() == TopAbs_REVERSED) d1.Reverse();
      if (e2.Orientation() == TopAbs_REVERSED) d2.Reverse();
      if (d1.Magnitude() < 1e-12 || d2.Magnitude() < 1e-12) return TopoDS_Shape();
      d1.Normalize();
      d2.Normalize();
      if (d1.Crossed(d2).Magnitude() < 1e-7 && d1.Dot(d2) > 0) continue;  // tangent
      gp_Vec into = d2 - d1;
      if (into.Magnitude() < 1e-12) return TopoDS_Shape();
      gp_Pnt q = p.Translated(into.Normalized() * (1e-3 * d));
      BRepClass_FaceClassifier fc(af, q, 1e-9);
      if (fc.State() == TopAbs_IN) continue;
      if (gp_Lin(ax).Distance(p) > 1e-7) return TopoDS_Shape();
      concave = true;
    }
    BRepOffsetAPI_MakeOffset mo(af, GeomAbs_Intersection);
    mo.Perform(-d);
    if (!mo.IsDone() && concave) {
      mo.Init(af, GeomAbs_Arc);
      mo.Perform(-d);
    }
    if (!mo.IsDone() || mo.Shape().IsNull()) return TopoDS_Shape();
    std::vector<TopoDS_Wire> ows;
    for (TopExp_Explorer we(mo.Shape(), TopAbs_WIRE); we.More(); we.Next()) ows.push_back(TopoDS::Wire(we.Current()));
    for (const TopoDS_Wire& ow : ows) {
      if (!BRep_Tool::IsClosed(ow)) return TopoDS_Shape();
      BRepBuilderAPI_MakeFace of(mer, ow);
      if (!of.IsDone()) return TopoDS_Shape();
      TopoDS_Shape halfs = BRepAlgoAPI_Common(of.Face(), right).Shape();
      for (TopExp_Explorer he(halfs, TopAbs_FACE); he.More(); he.Next()) {
        TopoDS_Shape rv = BRepPrimAPI_MakeRevol(he.Current(), ax, 2 * M_PI).Shape();
        cavity = cavity.IsNull() ? rv : BRepAlgoAPI_Fuse(cavity, rv).Shape();
      }
    }
  }
  return cavity;
}

// The shell as the solid less a cavity (see `revolved_cavity`): the solid's faces keep their
// identity where the cut leaves them (the opened faces become the rings left of them), and
// each face of the cavity is the offset of the solid's face `d` beyond it, outside the void.
static int shell_by_cavity(const TopoDS_Shape& solid, const ShapeMap& in_faces, const TopoDS_Shape& cavity, double d,
                           GsShape** out, GsBuf* origins) {
  BRepAlgoAPI_Cut cut(solid, cavity);
  if (!cut.IsDone() || cut.Shape().IsNull()) return fail("shell failed");
  const TopoDS_Shape result = cut.Shape();
  int nsolids = 0;
  for (TopExp_Explorer ex(result, TopAbs_SOLID); ex.More(); ex.Next()) ++nsolids;
  if (nsolids != 1) return fail("shell failed");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    const TopoDS_Shape& g = in_faces(i);
    int idx = out_faces.FindIndex(g);
    if (idx > 0) {
      push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
      continue;
    }
    for (ShapeList::Iterator it(cut.Modified(g)); it.More(); it.Next()) {
      int j = out_faces.FindIndex(it.Value());
      if (j > 0) push_origin(o, j - 1, GS_ORIGIN_INPUT, 0, i - 1);
    }
  }
  ShapeMap cav_faces;
  TopExp::MapShapes(cavity, TopAbs_FACE, cav_faces);
  for (int c = 1; c <= cav_faces.Extent(); ++c) {
    ShapeList images = cut.Modified(cav_faces(c));
    if (images.IsEmpty() && out_faces.Contains(cav_faces(c))) images.Append(cav_faces(c));
    for (ShapeList::Iterator it(images); it.More(); it.Next()) {
      int j = out_faces.FindIndex(it.Value());
      if (j <= 0) continue;
      const TopoDS_Face f = TopoDS::Face(it.Value());
      double u0, u1, v0, v1;
      BRepTools::UVBounds(f, u0, u1, v0, v1);
      BRepGProp_Face gf(f);
      gp_Pnt p;
      gp_Vec n;
      gf.Normal(0.5 * (u0 + u1), 0.5 * (v0 + v1), p, n);
      if (n.Magnitude() < 1e-12) continue;
      // The solid's face lies `d` from it along its normal, on the wall's side (whichever way
      // the normal points: the nearer face of the two sides).
      int best = 0;
      double best_d = 1e-5 * std::max(1.0, d);
      for (double side : {-d, d}) {
        const TopoDS_Vertex q = BRepBuilderAPI_MakeVertex(p.Translated(n.Normalized() * side)).Vertex();
        for (int i = 1; i <= in_faces.Extent(); ++i) {
          BRepExtrema_DistShapeShape dist(q, in_faces(i));
          if (dist.IsDone() && dist.Value() < best_d) {
            best_d = dist.Value();
            best = i;
          }
        }
      }
      if (best > 0) push_origin(o, j - 1, GS_ORIGIN_OFFSET, 0, best - 1);
    }
  }
  check_valid(result, "shell");
  GProp_GProps before, after;
  BRepGProp::VolumeProperties(solid, before);
  BRepGProp::VolumeProperties(result, after);
  if (!(after.Mass() > 1e-9) || !(after.Mass() < before.Mass() - 1e-9))
    throw Standard_Failure("the walls are too thick for this part");
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
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
  // OCCT: a negative offset builds the wall inside the solid. A body of several solids is
  // shelled solid by solid (OCCT fails on the compound): each solid with an opened face, and
  // every solid when no face is opened; a solid none of whose faces is opened stays as it is
  // (as in Fusion, whose shell acts on the bodies of its faces).
  std::vector<TopoDS_Shape> solids;
  for (TopExp_Explorer ex(s->s, TopAbs_SOLID); ex.More(); ex.Next()) solids.push_back(ex.Current());
  std::vector<std::unique_ptr<BRepOffsetAPI_MakeThickSolid>> mks;
  std::vector<TopoDS_Shape> parts;
  // The maker that shelled each input face's solid (-1: the solid was kept).
  std::vector<int> maker_of(size_t(in_faces.Extent()), -1);
  // Whether each maker's part is a hollow solid built from its closed offset (no face opened).
  std::vector<bool> hollow_maker;
  auto shell_part = [&](const TopoDS_Shape& part, const ShapeList& open) -> int {
    auto mk = std::make_unique<BRepOffsetAPI_MakeThickSolid>();
    mk->MakeThickSolidByJoin(part, open, -thickness, 1e-4, BRepOffset_Skin, false, false, GeomAbs_Intersection);
    if (!mk->IsDone() || mk->Shape().IsNull()) {
      // The same join with every pair of offset faces intersected (OCCT's slower, thorough
      // mode): it shells some filleted parts the default one fails on.
      mk = std::make_unique<BRepOffsetAPI_MakeThickSolid>();
      mk->MakeThickSolidByJoin(part, open, -thickness, 1e-4, BRepOffset_Skin, true, false, GeomAbs_Intersection);
    }
    if (!mk->IsDone() || mk->Shape().IsNull()) return -1;
    if (open.IsEmpty()) {
      // No face opened: OCCT gives the offset solid alone. The shell of a whole body is a
      // hollow solid: the outer of the two closed skins with the inner one as its void
      // (thickness > 0: the body's own skin outside, the offset one inside; < 0 the other way).
      TopoDS_Shape off = mk->Shape();
      std::vector<TopoDS_Shell> own, offs;
      for (TopExp_Explorer ex(part, TopAbs_SHELL); ex.More(); ex.Next()) own.push_back(TopoDS::Shell(ex.Current()));
      for (TopExp_Explorer ex(off, TopAbs_SHELL); ex.More(); ex.Next()) offs.push_back(TopoDS::Shell(ex.Current()));
      if (own.size() != 1 || offs.size() != 1) return -1;
      BRep_Builder hb;
      // A skin facing out (`out`) or in: a solid of it alone has a positive volume or a negative one.
      auto facing = [&hb](const TopoDS_Shell& sh, bool out) {
        TopoDS_Solid one;
        hb.MakeSolid(one);
        hb.Add(one, sh);
        GProp_GProps gp;
        BRepGProp::VolumeProperties(one, gp);
        return (gp.Mass() > 0) == out ? sh : TopoDS::Shell(sh.Reversed());
      };
      const TopoDS_Shell outer = facing(thickness > 0 ? own[0] : offs[0], true);
      const TopoDS_Shell inner = facing(thickness > 0 ? offs[0] : own[0], false);
      TopoDS_Solid hollow;
      hb.MakeSolid(hollow);
      hb.Add(hollow, outer);
      hb.Add(hollow, inner);
      parts.push_back(hollow);
      hollow_maker.push_back(true);
    } else {
      parts.push_back(mk->Shape());
      hollow_maker.push_back(false);
    }
    mks.push_back(std::move(mk));
    return int(mks.size()) - 1;
  };
  if (solids.size() <= 1) {
    // A solid of revolution OCCT cannot offset (it fails or throws on a point of the axis on a
    // face): its own cavity.
    const bool revolved = thickness > 0 && !closing.IsEmpty() && solids.size() == 1;
    int k = -1;
    try {
      k = shell_part(s->s, closing);
    } catch (const Standard_Failure&) {
      if (!revolved) throw;
    }
    if (k < 0 && revolved) {
      const TopoDS_Shape cavity = revolved_cavity(solids[0], closing, thickness);
      if (!cavity.IsNull()) return shell_by_cavity(solids[0], in_faces, cavity, thickness, out, origins);
    }
    if (k < 0) return fail("shell failed");
    for (auto& m : maker_of) m = k;
  } else {
    for (const auto& so : solids) {
      ShapeMap mine;
      TopExp::MapShapes(so, TopAbs_FACE, mine);
      ShapeList open;
      for (ShapeList::Iterator it(closing); it.More(); it.Next())
        if (mine.Contains(it.Value())) open.Append(it.Value());
      int k = -1;
      if (!open.IsEmpty() || nfaces == 0) {
        k = shell_part(so, open);
        if (k < 0) return fail("shell failed");
      } else {
        parts.push_back(so);
      }
      for (int j = 1; j <= mine.Extent(); ++j) {
        int fi = in_faces.FindIndex(mine(j));
        if (fi > 0) maker_of[size_t(fi - 1)] = k;
      }
    }
  }
  TopoDS_Shape result;
  if (parts.size() == 1) {
    result = parts[0];
  } else {
    TopoDS_Compound comp;
    BRep_Builder bb;
    bb.MakeCompound(comp);
    for (const auto& p : parts) bb.Add(comp, p);
    result = comp;
  }
  if (result.IsNull()) return fail("shell produced no result");
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    bool removed = false;
    for (size_t k = 0; k < nfaces; ++k) removed = removed || faces[k] == uint32_t(i - 1);
    const TopoDS_Shape& g = in_faces(i);
    const int k = maker_of[size_t(i - 1)];
    if (k < 0) {
      int idx = out_faces.FindIndex(g);
      if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
      continue;
    }
    BRepOffsetAPI_MakeThickSolid& mk = *mks[size_t(k)];
    if (hollow_maker[size_t(k)]) {
      // The body's own face stays as it is; what the offset made of it is the other skin's face.
      int idx = out_faces.FindIndex(g);
      if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, 0, i - 1);
      ShapeMap made;
      for (ShapeList::Iterator it(mk.Modified(g)); it.More(); it.Next()) made.Add(it.Value());
      for (ShapeList::Iterator it(mk.Generated(g)); it.More(); it.Next()) made.Add(it.Value());
      for (int m = 1; m <= made.Extent(); ++m) {
        if (made(m).ShapeType() != TopAbs_FACE || made(m).IsSame(g)) continue;
        int j = out_faces.FindIndex(made(m));
        if (j > 0) push_origin(o, j - 1, GS_ORIGIN_OFFSET, 0, i - 1);
      }
      continue;
    }
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
  std::vector<TopoDS_Face> fs;
  std::vector<uint32_t> idx;
  for (size_t i = 0; i < nfaces; ++i) {
    if (in_faces.Extent() <= 0 || faces[i] >= uint32_t(in_faces.Extent())) return fail("face index out of range");
    fs.push_back(TopoDS::Face(in_faces(int(faces[i]) + 1)));
    idx.push_back(faces[i]);
  }
  // Where BRepOffset fails, cylinders between planes are offset as booleans (`offset_cylinders`).
  // A cylinder with a tangent round at an end goes that way first: BRepOffset carries the offset
  // on along every tangent face (the round, then the plane the round ends on, and so on), where
  // Fusion moves the round with the face and keeps the plane (3350's pocket walls).
  bool rounds = false;
  {
    AncMap anc;
    TopExp::MapShapesAndAncestors(s->s, TopAbs_EDGE, TopAbs_FACE, anc);
    for (const auto& f : fs) {
      CylinderOffset c;
      if (offsettable_cylinder(anc, f, c)) rounds = rounds || c.ends[0].round || c.ends[1].round;
    }
  }
  if (rounds) {
    try {
      if (offset_cylinders(s, fs, idx, distance, out, origins) == GS_OK) return GS_OK;
    } catch (const Standard_Failure&) {
    }
  }
  std::string why;
  try {
    // Base offset 0 with an offset on the chosen faces only: the others stay, their
    // neighbours stretch (BRepOffset's per-face offsets).
    BRepOffset_MakeOffset mk;
    mk.Initialize(s->s, 0.0, 1e-7, BRepOffset_Skin, Standard_False, Standard_False, GeomAbs_Intersection);
    for (const auto& f : fs) mk.SetOffsetOnFace(f, distance);
    mk.MakeOffsetShape();
    if (!mk.IsDone()) throw Standard_Failure("the faces cannot be offset that far (the part would fold over)");
    TopoDS_Shape result = mk.Shape();
    if (result.IsNull()) throw Standard_Failure("offset produced no result");
    ShapeMap out_faces;
    TopExp::MapShapes(result, TopAbs_FACE, out_faces);
    // Modified() is empty for this algorithm; its face image maps every input face.
    const BRepAlgo_Image& image = mk.OffsetFacesFromShapes();
    std::vector<int64_t> o;
    for (int i = 1; i <= in_faces.Extent(); ++i) {
      const TopoDS_Shape& g = in_faces(i);
      if (!image.HasImage(g)) continue;
      for (ShapeList::Iterator it(image.Image(g)); it.More(); it.Next()) {
        int k = out_faces.FindIndex(it.Value());
        if (k > 0) push_origin(o, k - 1, GS_ORIGIN_INPUT, 0, i - 1);
      }
    }
    check_valid(result, "offset");
    to_buf(o, origins);
    *out = new GsShape{result};
    return GS_OK;
  } catch (const Standard_Failure& e) {
    why = e.what() ? e.what() : "offset failed";
  }
  try {
    if (offset_cylinders(s, fs, idx, distance, out, origins) == GS_OK) return GS_OK;
  } catch (const Standard_Failure&) {
  }
  return fail(why);
  GS_GUARD_END
}

static void input_origins(BRepBuilderAPI_MakeShape& mk, const TopoDS_Shape& in, const TopoDS_Shape& r, uint32_t input,
                          int kind, std::vector<int64_t>& o);

// Removes faces and lets the neighbours grow over the gap (ADR 0038, defeaturing).
int gs_delete_faces(const GsShape* s, const uint32_t* faces, size_t nfaces, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins || (!faces && nfaces)) return fail("null argument");
  if (nfaces == 0) return fail("no faces to delete");
  ShapeMap in_faces;
  TopExp::MapShapes(s->s, TopAbs_FACE, in_faces);
  BRepAlgoAPI_Defeaturing mk;
  mk.SetShape(s->s);
  mk.SetRunParallel(Standard_False);
  mk.SetToFillHistory(Standard_True);
  for (size_t i = 0; i < nfaces; ++i) {
    if (in_faces.Extent() <= 0 || faces[i] >= uint32_t(in_faces.Extent())) return fail("face index out of range");
    mk.AddFaceToRemove(in_faces(int(faces[i]) + 1));
  }
  mk.Build();
  if (!mk.IsDone() || mk.HasErrors())
    return fail("the faces cannot be removed: the faces around them do not close the gap");
  TopoDS_Shape result = mk.Shape();
  if (result.IsNull()) return fail("deleting faces produced no result");
  // The algorithm leaves a face it cannot remove in place (with a warning): say so.
  {
    ShapeMap kept;
    TopExp::MapShapes(result, TopAbs_FACE, kept);
    for (size_t i = 0; i < nfaces; ++i)
      if (kept.Contains(in_faces(int(faces[i]) + 1)))
        return fail("the faces cannot be removed: the faces around them do not close the gap");
  }
  std::vector<int64_t> o;
  input_origins(mk, s->s, result, 0, GS_ORIGIN_INPUT, o);
  check_valid(result, "delete faces");
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

// make_gtrsf returns a C++ class: extern "C++" keeps MSVC from giving it C linkage (C2526).
extern "C++" {
namespace {

// Whether a GS_XF_SCALE has the same factor on every axis (a similarity, kept analytic).
bool uniform_scale(const double params[7]) { return params[3] == params[4] && params[4] == params[5]; }

// Validates the parameters of a GS_XF_SCALE; an error message when they are no scaling.
const char* check_scale(const double params[7]) {
  for (int i = 0; i < 6; ++i)
    if (!std::isfinite(params[i])) return "scale has non-finite parameters";
  for (int i = 3; i < 6; ++i)
    if (!(params[i] > 1e-9)) return "scale factors must be positive";
  return nullptr;
}

// Scaling by different factors per axis: an affine map, so curved faces become B-splines.
gp_GTrsf make_gtrsf(const double params[7]) {
  gp_GTrsf g;
  for (int i = 0; i < 3; ++i) {
    g.SetValue(i + 1, i + 1, params[3 + i]);
    g.SetValue(i + 1, 4, params[i] * (1.0 - params[3 + i]));
  }
  return g;
}

}  // namespace
}  // extern "C++"

int gs_transform(int kind, const double params[7], const GsShape* s, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!params || !s || !out || !origins) return fail("null argument");
  TopoDS_Shape result;
  // The builder keeps the face history, so it lives to the end of the function.
  std::unique_ptr<BRepBuilderAPI_ModifyShape> builder;
  if (kind == GS_XF_SCALE) {
    if (const char* err = check_scale(params)) return fail(err);
    if (uniform_scale(params)) {
      gp_Trsf t;
      t.SetScale(gp_Pnt(params[0], params[1], params[2]), params[3]);
      builder.reset(new BRepBuilderAPI_Transform(s->s, t, true));
    } else {
      gp_GTrsf g = make_gtrsf(params);
      builder.reset(new BRepBuilderAPI_GTransform(s->s, g, true));
    }
  } else {
    gp_Trsf t;
    if (const char* err = make_trsf(kind, params, t)) return fail(err);
    builder.reset(new BRepBuilderAPI_Transform(s->s, t, true));
  }
  BRepBuilderAPI_ModifyShape& xf = *builder;
  if (!xf.IsDone()) return fail("transform failed");
  result = xf.Shape();
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
  measured_props(s->s, vol, &area);
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

namespace {

/* Appends the edges of `c` (in the projector's frame, z = 0) as HLR curves. */
void hlr_edges(const TopoDS_Shape& c, int visible, int cls, double defl, std::vector<double>& curves,
               std::vector<double>& points, double source = -1) {
  if (c.IsNull()) return;
  for (TopExp_Explorer ex(c, TopAbs_EDGE); ex.More(); ex.Next()) {
    TopoDS_Edge e = TopoDS::Edge(ex.Current());
    if (BRep_Tool::Degenerated(e)) continue;
    BRepAdaptor_Curve bc(e);
    double t0 = bc.FirstParameter(), t1 = bc.LastParameter();
    if (!(t1 > t0)) continue;
    bool rev = e.Orientation() == TopAbs_REVERSED;
    if (bc.GetType() == GeomAbs_Line) {
      gp_Pnt a = bc.Value(rev ? t1 : t0), b = bc.Value(rev ? t0 : t1);
      if (a.Distance(b) < 1e-9) continue;
      curves.insert(curves.end(), {double(visible), double(cls), 0, a.X(), a.Y(), b.X(), b.Y(), 0, 0, source});
      continue;
    }
    if (bc.GetType() == GeomAbs_Circle) {
      gp_Circ ci = bc.Circle();
      gp_Pnt a = bc.Value(t0), b = bc.Value(t1), cen = ci.Location();
      // The arc runs from a to b turning about the circle's axis: counterclockwise in the
      // view when that axis points at the viewer (+Z).
      bool ccw = ci.Axis().Direction().Z() > 0;
      if (rev) {
        std::swap(a, b);
        ccw = !ccw;
      }
      double a0 = std::atan2(a.Y() - cen.Y(), a.X() - cen.X());
      double a1 = std::atan2(b.Y() - cen.Y(), b.X() - cen.X());
      double span = t1 - t0;
      // Full circles: the end angle is one turn on.
      if (span > 2 * M_PI - 1e-9) a1 = a0 + (ccw ? 2 * M_PI : -2 * M_PI);
      curves.insert(curves.end(),
                    {double(visible), double(cls), 1, cen.X(), cen.Y(), ci.Radius(), a0, a1, double(ccw), source});
      continue;
    }
    GCPnts_QuasiUniformDeflection d(bc, defl, t0, t1);
    if (!d.IsDone() || d.NbPoints() < 2) continue;
    double first = double(points.size() / 2);
    int n = d.NbPoints();
    for (int k = 1; k <= n; ++k) {
      gp_Pnt p = d.Value(rev ? n + 1 - k : k);
      points.insert(points.end(), {p.X(), p.Y()});
    }
    curves.insert(curves.end(), {double(visible), double(cls), 2, first, double(n), 0, 0, 0, 0, source});
  }
}

/* HLRBRep_HLRToShape::InternalCompound (whole shape, 2D) with every fragment tagged by
 * the input edge it comes from (0-based in `em`, -1 for silhouettes and other new edges).
 * typ: 5 sharp, 3 smooth (G1), 2 outline. */
void hlr_collect(const occ::handle<HLRBRep_Data>& ds, int typ, bool visible, const ShapeMap& em, double defl,
                 std::vector<double>& curves, std::vector<double>& points) {
  int cls = typ == 5 ? 0 : typ == 3 ? 1 : 2;
  int ne = ds->NbEdges(), nf = ds->NbFaces();
  for (int ie = 1; ie <= ne; ++ie) {
    HLRBRep_EdgeData& ed = ds->EDataArray().ChangeValue(ie);
    if (ed.Selected() && !ed.Vertical()) {
      ed.Used(false);
      ed.HideCount(0);
    } else {
      ed.Used(true);
    }
  }
  auto source = [&](int ie) -> double {
    if (ie < 1 || ie > ds->EdgeMap().Extent()) return -1;
    int k = em.FindIndex(ds->EdgeMap().FindKey(ie));
    return k > 0 ? double(k - 1) : -1;
  };
  auto draw = [&](bool in_face, HLRBRep_EdgeData& ed, int ie) {
    bool todraw = in_face || (typ == 3 ? ed.Rg1Line() && !ed.RgNLine() : !ed.Rg1Line());
    if (!todraw) return;
    double sta, end;
    float tsta, tend;
    HLRAlgo_EdgeIterator it;
    BRep_Builder b;
    TopoDS_Compound part;
    b.MakeCompound(part);
    bool any = false;
    if (visible) {
      for (it.InitVisible(ed.Status()); it.MoreVisible(); it.NextVisible()) {
        it.Visible(sta, tsta, end, tend);
        TopoDS_Edge e = HLRBRep::MakeEdge(ed.Geometry(), sta, end);
        if (!e.IsNull()) b.Add(part, e), any = true;
      }
    } else {
      for (it.InitHidden(ed.Status()); it.MoreHidden(); it.NextHidden()) {
        it.Hidden(sta, tsta, end, tend);
        TopoDS_Edge e = HLRBRep::MakeEdge(ed.Geometry(), sta, end);
        if (!e.IsNull()) b.Add(part, e), any = true;
      }
    }
    if (any) hlr_edges(part, visible ? 1 : 0, cls, defl, curves, points, source(ie));
  };
  for (int f = 1; f <= nf; ++f) {
    HLRBRep_FaceIterator itf;
    for (itf.InitEdge(ds->FDataArray().ChangeValue(f)); itf.MoreEdge(); itf.NextEdge()) {
      int ie = itf.Edge();
      HLRBRep_EdgeData& edf = ds->EDataArray().ChangeValue(ie);
      if (edf.Used()) continue;
      bool todraw = typ == 2   ? itf.Internal()
                    : typ == 3 ? edf.Rg1Line() && !edf.RgNLine() && !itf.OutLine()
                               : !itf.IsoLine() && !itf.Internal() && (!edf.Rg1Line() || itf.OutLine());
      if (todraw) {
        draw(true, edf, ie);
        edf.Used(true);
      } else if ((typ > 4 || typ == 2) && edf.Rg1Line() && !itf.OutLine()) {
        int hc = edf.HideCount();
        if (hc > 0)
          edf.Used(true);
        else
          edf.HideCount(hc + 1);
      } else {
        edf.Used(true);
      }
    }
  }
  if (typ >= 3) {
    for (int ie = 1; ie <= ne; ++ie) {
      HLRBRep_EdgeData& ed = ds->EDataArray().ChangeValue(ie);
      if (!ed.Used()) {
        draw(false, ed, ie);
        ed.Used(true);
      }
    }
  }
}

}  // namespace

int gs_hlr(const GsShape* s, const double view[6], int exact, double deflection, GsBuf* curves, GsBuf* points) {
  GS_GUARD_BEGIN
  if (!s || !view || !curves || !points) return fail("null argument");
  if (!(deflection > 0)) return fail("the deflection must be positive");
  gp_Vec z(view[0], view[1], view[2]), x(view[3], view[4], view[5]);
  if (z.Magnitude() < 1e-12 || x.Magnitude() < 1e-12 || z.CrossMagnitude(x) < 1e-9 * z.Magnitude() * x.Magnitude())
    return fail("the view direction and its X axis must be independent");
  gp_Ax2 cs(gp_Pnt(0, 0, 0), gp_Dir(z), gp_Dir(x));
  HLRAlgo_Projector projector(cs);
  std::vector<double> cv, pts;
  if (exact) {
    occ::handle<HLRBRep_Algo> algo = new HLRBRep_Algo();
    algo->Add(s->s);
    algo->Projector(projector);
    algo->Update();
    algo->Hide();
    occ::handle<HLRBRep_Data> ds = algo->DataStructure();
    if (!ds.IsNull()) {
      ShapeMap em;
      TopExp::MapShapes(s->s, TopAbs_EDGE, em);
      ds->Projector().Scaled(true);
      for (int vis = 1; vis >= 0; --vis)
        for (int typ : {5, 3, 2}) hlr_collect(ds, typ, vis != 0, em, deflection, cv, pts);
      ds->Projector().Scaled(false);
    }
  } else {
    BRepMesh_IncrementalMesh mesher(s->s, deflection, false, 0.5, false);
    occ::handle<HLRBRep_PolyAlgo> algo = new HLRBRep_PolyAlgo();
    algo->Load(s->s);
    algo->Projector(projector);
    algo->Update();
    HLRBRep_PolyHLRToShape out;
    out.Update(algo);
    hlr_edges(out.VCompound(), 1, 0, deflection, cv, pts);
    hlr_edges(out.Rg1LineVCompound(), 1, 1, deflection, cv, pts);
    hlr_edges(out.OutLineVCompound(), 1, 2, deflection, cv, pts);
    hlr_edges(out.HCompound(), 0, 0, deflection, cv, pts);
    hlr_edges(out.Rg1LineHCompound(), 0, 1, deflection, cv, pts);
    hlr_edges(out.OutLineHCompound(), 0, 2, deflection, cv, pts);
  }
  to_buf(cv, curves);
  to_buf(pts, points);
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
  occ::handle<XCAFDoc_ColorTool> ct = XCAFDoc_DocumentTool::ColorTool(doc->Main());
  for (size_t i = 0; i < n; ++i) {
    // A compound of one shape (an imported body) is written as that shape: as a compound it
    // became an assembly holding a part named "SOLID", and its name and colours stayed on the
    // assembly, out of reach of readers (dogfooding F15-1).
    TopoDS_Shape shape = shapes[i]->s;
    while (shape.ShapeType() == TopAbs_COMPOUND) {
      TopoDS_Iterator it(shape);
      if (!it.More()) break;
      TopoDS_Shape only = it.Value();
      it.Next();
      if (it.More()) break;
      only.Move(shape.Location());
      shape = only;
    }
    TDF_Label l = st->AddShape(shape, false);
    if (!names || !names[i]) continue;
    // "name\x1f#rrggbb\x1fi=rrggbb,...": the body's colour and its faces' colours ride on
    // its name (ADR 0045; i is the face's index in TopExp::MapShapes order).
    std::string full(names[i]);
    std::string name = full, colour, faces;
    size_t sep = full.find('\x1f');
    if (sep != std::string::npos) {
      name = full.substr(0, sep);
      colour = full.substr(sep + 1);
      size_t sep2 = colour.find('\x1f');
      if (sep2 != std::string::npos) {
        faces = colour.substr(sep2 + 1);
        colour = colour.substr(0, sep2);
      }
    }
    TDataStd_Name::Set(l, TCollection_ExtendedString(name.c_str(), true));
    // A placed shape is added as a reference to (or an assembly of) the unplaced part: the part
    // takes the name and colours too, where readers look (dogfooding F15-1).
    TDF_Label part = l;
    TopoDS_Shape part_shape = shape;
    TDF_Label referred;
    if (XCAFDoc_ShapeTool::IsReference(l) && XCAFDoc_ShapeTool::GetReferredShape(l, referred)) {
      part = referred;
      part_shape = XCAFDoc_ShapeTool::GetShape(referred);
      TDataStd_Name::Set(part, TCollection_ExtendedString(name.c_str(), true));
    } else if (XCAFDoc_ShapeTool::IsAssembly(l)) {
      NCollection_Sequence<TDF_Label> comps;
      XCAFDoc_ShapeTool::GetComponents(l, comps);
      TDF_Label ref;
      if (comps.Length() == 1 && XCAFDoc_ShapeTool::GetReferredShape(comps.Value(1), ref)) {
        part = ref;
        part_shape = XCAFDoc_ShapeTool::GetShape(ref);
        TDataStd_Name::Set(part, TCollection_ExtendedString(name.c_str(), true));
      }
    }
    auto hex = [](const std::string& h, Quantity_Color& c) {
      if (h.size() != 6) return false;
      char* end = nullptr;
      unsigned long v = std::strtoul(h.c_str(), &end, 16);
      if (!end || *end != '\0') return false;
      c = Quantity_Color(((v >> 16) & 255) / 255.0, ((v >> 8) & 255) / 255.0, (v & 255) / 255.0, Quantity_TOC_sRGB);
      return true;
    };
    Quantity_Color c;
    if (colour.size() == 7 && colour[0] == '#' && hex(colour.substr(1), c)) {
      ct->SetColor(l, c, XCAFDoc_ColorSurf);
      if (part != l) ct->SetColor(part, c, XCAFDoc_ColorSurf);
    }
    if (!faces.empty()) {
      ShapeMap fm;
      TopExp::MapShapes(part_shape, TopAbs_FACE, fm);
      std::istringstream fs(faces);
      std::string e;
      while (std::getline(fs, e, ',')) {
        size_t eq = e.find('=');
        if (eq == std::string::npos) continue;
        char* end = nullptr;
        long fi = std::strtol(e.c_str(), &end, 10);
        if (end != e.c_str() + eq || fi < 0 || fi >= fm.Extent() || !hex(e.substr(eq + 1), c)) continue;
        TDF_Label fl = st->AddSubShape(part, fm(static_cast<int>(fi) + 1));
        if (!fl.IsNull()) ct->SetColor(fl, c, XCAFDoc_ColorSurf);
      }
    }
  }
  Interface_Static::SetCVal("write.step.unit", "MM");
  Interface_Static::SetCVal("write.step.product.name", "Gusset");
  STEPCAFControl_Writer w;
  w.SetNameMode(true);
  w.SetColorMode(true);
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

/* Names and colours of the shapes a STEP file holds (ADR 0045): the same root shapes as
 * gs_step_read, read through XDE. `meta` gets one line per shape, "name\t#rrggbb\tfaces\n",
 * with "-" for a missing colour; tabs and line breaks in names become spaces. A root without
 * a colour of its own takes the colour most of its faces have. `faces` lists the faces in
 * another colour than the shape's, "i=rrggbb,..." (i: the face's index in
 * TopExp::MapShapes order, as the kernel numbers faces; at most 10000), empty if none. */
int gs_step_read_meta(const uint8_t* data, size_t len, GsShape*** out_shapes, size_t* n, GsBuf* meta) {
  GS_GUARD_BEGIN
  if (!data || !out_shapes || !n || !meta) return fail("null argument");
  occ::handle<XCAFApp_Application> app = XCAFApp_Application::GetApplication();
  occ::handle<TDocStd_Document> doc;
  app->NewDocument("MDTV-XCAF", doc);
  STEPCAFControl_Reader r;
  r.SetColorMode(true);
  r.SetNameMode(true);
  std::string buf(reinterpret_cast<const char*>(data), len);
  std::istringstream is(buf);
  if (r.ReadStream("import.step", is) != IFSelect_RetDone) {
    app->Close(doc);
    throw Standard_Failure("the file could not be parsed");
  }
  if (!r.Transfer(doc)) {
    app->Close(doc);
    throw Standard_Failure("the file could not be translated");
  }
  occ::handle<XCAFDoc_ShapeTool> st = XCAFDoc_DocumentTool::ShapeTool(doc->Main());
  occ::handle<XCAFDoc_ColorTool> ct = XCAFDoc_DocumentTool::ColorTool(doc->Main());
  STEPControl_Reader& sr = r.ChangeReader();
  std::vector<GsShape*> v;
  std::string lines;
  auto rgb = [](const Quantity_Color& c) {
    double cr, cg, cb;
    c.Values(cr, cg, cb, Quantity_TOC_sRGB);
    auto byte = [](double x) { return static_cast<unsigned>(std::lround(std::clamp(x, 0.0, 1.0) * 255.0)); };
    return (byte(cr) << 16) | (byte(cg) << 8) | byte(cb);
  };
  // A label's name, or the name of the shape it refers to (an assembly component).
  auto name_of = [&](const TDF_Label& lab) {
    std::string name;
    occ::handle<TDataStd_Name> nm;
    TDF_Label l = lab;
    for (int k = 0; k < 2 && name.empty(); ++k) {
      if (l.FindAttribute(TDataStd_Name::GetID(), nm)) {
        TCollection_AsciiString a(nm->Get(), '?');
        name = a.ToCString();
      }
      TDF_Label ref;
      if (!XCAFDoc_ShapeTool::GetReferredShape(l, ref)) break;
      l = ref;
    }
    for (char& ch : name)
      if (ch == '\t' || ch == '\n' || ch == '\r') ch = ' ';
    return name;
  };
  // One pass over the document: names and colours by the shape they belong to (its
  // TShape, shared by every placed copy), so each lookup below is constant time.
  std::unordered_map<const TopoDS_TShape*, std::string> names;
  std::unordered_map<const TopoDS_TShape*, long> colours;
  auto label_colour = [&](const TDF_Label& l) -> long {
    Quantity_Color c;
    if (XCAFDoc_ColorTool::GetColor(l, XCAFDoc_ColorSurf, c) || XCAFDoc_ColorTool::GetColor(l, XCAFDoc_ColorGen, c))
      return rgb(c);
    return -1;
  };
  {
    NCollection_Sequence<TDF_Label> all;
    st->GetShapes(all);
    // Parts first, assemblies last, and an assembly's name and colour only for itself: a
    // solid takes its own part's name, never the name of an assembly holding it (dogfooding
    // F14-2).
    for (int pass = 0; pass < 2; ++pass)
    for (int i = 1; i <= all.Length(); ++i) {
      const TDF_Label& l = all.Value(i);
      bool assembly = XCAFDoc_ShapeTool::IsAssembly(l);
      if (assembly != (pass == 1)) continue;
      TopoDS_Shape sh = XCAFDoc_ShapeTool::GetShape(l);
      if (sh.IsNull()) continue;
      std::string n = name_of(l);
      long c = label_colour(l);
      auto note = [&](const TopoDS_Shape& x) {
        if (!n.empty()) names.emplace(x.TShape().get(), n);
        if (c >= 0) colours.emplace(x.TShape().get(), c);
      };
      note(sh);
      if (!assembly)
        for (TopExp_Explorer e(sh, TopAbs_SOLID); e.More(); e.Next()) note(e.Current());
      NCollection_Sequence<TDF_Label> subs;
      XCAFDoc_ShapeTool::GetSubShapes(l, subs);
      for (int j = 1; j <= subs.Length(); ++j) {
        long sc = label_colour(subs.Value(j));
        TopoDS_Shape x = XCAFDoc_ShapeTool::GetShape(subs.Value(j));
        if (sc >= 0 && !x.IsNull()) {
          colours[x.TShape().get()] = sc;
          for (TopExp_Explorer e(x, TopAbs_FACE); e.More(); e.Next()) colours.emplace(e.Current().TShape().get(), sc);
        }
      }
    }
  }
  auto colour_of = [&](const TopoDS_Shape& s) -> long {
    auto it = colours.find(s.TShape().get());
    if (it != colours.end()) return it->second;
    // The colour most of its faces have, uncoloured faces counted too (key -1): when most
    // faces have no colour but some do, the body takes the viewport's default colour and the
    // coloured faces are listed on their own (face colours review #1).
    std::map<long, int> count;
    bool any = false;
    for (TopExp_Explorer e(s, TopAbs_FACE); e.More(); e.Next()) {
      auto f = colours.find(e.Current().TShape().get());
      if (f != colours.end()) {
        ++count[f->second];
        any = true;
      } else {
        ++count[-1];
      }
    }
    long best = -1;
    int most = 0;
    for (const auto& [k, nface] : count)
      if (nface > most) {
        most = nface;
        best = k;
      }
    if (best < 0 && any) best = 0xb4bcc8;
    return best;
  };
  auto emit = [&](const TopoDS_Shape& s) {
    auto it = names.find(s.TShape().get());
    std::string name = it != names.end() ? it->second : std::string();
    long colour = colour_of(s);
    char hex[16];
    if (colour >= 0)
      std::snprintf(hex, sizeof hex, "#%06lx", colour);
    else
      std::snprintf(hex, sizeof hex, "-");
    std::string faces;
    if (colour >= 0) {
      ShapeMap fm;
      TopExp::MapShapes(s, TopAbs_FACE, fm);
      int listed = 0;
      for (int i = 1; i <= fm.Extent() && listed < 10000; ++i) {
        auto f = colours.find(fm(i).TShape().get());
        if (f == colours.end() || f->second == colour) continue;
        char e[32];
        std::snprintf(e, sizeof e, "%s%d=%06lx", listed ? "," : "", i - 1, f->second);
        faces += e;
        ++listed;
      }
    }
    lines += name + "\t" + hex + "\t" + faces + "\n";
    v.push_back(new GsShape{s});
  };
  for (int i = 1; i <= sr.NbShapes(); ++i) {
    TopoDS_Shape s = sr.Shape(i);
    if (s.IsNull()) continue;
    // An assembly (a compound of several solids) comes in as its solids, each with its
    // component's name and colour, when every face belongs to a solid and there are at
    // most 4096 of them; anything else stays one shape.
    std::vector<TopoDS_Shape> solids;
    ShapeMap in_solids, all_faces;
    TopExp::MapShapes(s, TopAbs_FACE, all_faces);
    for (TopExp_Explorer e(s, TopAbs_SOLID); e.More() && solids.size() <= 4096; e.Next()) {
      solids.push_back(e.Current());
      TopExp::MapShapes(e.Current(), TopAbs_FACE, in_solids);
    }
    // One solid inside a wrapper (a placed part, as Gusset writes imported bodies) comes in as
    // the solid too, so it takes its part's name and colour (dogfooding F15-1).
    if (!solids.empty() && solids.size() <= 4096 && in_solids.Extent() == all_faces.Extent() &&
        (solids.size() > 1 || !solids.front().IsSame(s) || s.ShapeType() != TopAbs_SOLID)) {
      for (const auto& so : solids) emit(so);
    } else {
      emit(s);
    }
  }
  app->Close(doc);
  bytes_to_buf(lines, meta);
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

/* OCCT's BRep text format ("CASCADE Topology V1..V3"), as FreeCAD stores shapes. Only
 * the sandboxed import worker calls this on file data (ADR 0033). */
int gs_brep_text_read(const uint8_t* data, size_t len, GsShape** out) {
  GS_GUARD_BEGIN
  if (!data || !out) return fail("null argument");
  std::string buf(reinterpret_cast<const char*>(data), len);
  std::istringstream is(buf);
  TopoDS_Shape s;
  BRep_Builder b;
  BRepTools::Read(s, is, b);
  if (s.IsNull()) return fail("the BRep text is empty or damaged");
  *out = new GsShape{s};
  return GS_OK;
  GS_GUARD_END
}

int gs_brep_text_write(const GsShape* s, GsBuf* out) {
  GS_GUARD_BEGIN
  if (!s || !out) return fail("null argument");
  std::ostringstream os;
  BRepTools::Write(s->s, os);
  bytes_to_buf(os.str(), out);
  return GS_OK;
  GS_GUARD_END
}

}  // extern "C"

/* ------------------------------------------------------------------ surfaces (ADR 0036) */

// Faces sewn into one shell when they share edges (each face stays when nothing is shared).
static TopoDS_Shape sew_faces(const std::vector<TopoDS_Shape>& faces, double tol) {
  BRepBuilderAPI_Sewing sew(tol);
  for (const auto& f : faces) sew.Add(f);
  sew.Perform();
  TopoDS_Shape r = sew.SewedShape();
  if (r.IsNull()) throw Standard_Failure("sewing produced no result");
  return r;
}

int gs_extrude_curves(const GsProfile* p, double distance, int symmetric, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !out || !origins) return fail("null argument");
  if (!std::isfinite(distance) || std::fabs(distance) < 1e-9) return fail("extrude distance must be non-zero");
  if (!p->segs || !p->loops || p->nloops == 0) return fail("no curves to extrude");
  double offset = symmetric ? -std::fabs(distance) / 2.0 : 0.0;
  double len = symmetric ? std::fabs(distance) : distance;
  Frame f = make_frame(p, offset);
  gp_Vec v(f.n.X() * len, f.n.Y() * len, f.n.Z() * len);
  std::vector<TopoDS_Shape> faces;
  std::vector<uint64_t> tags;
  for (size_t li = 0; li < p->nloops; ++li) {
    const GsLoop& l = p->loops[li];
    if (size_t(l.first) + l.count > p->nsegs) return fail("curves out of range");
    for (uint32_t i = 0; i < l.count; ++i) {
      const GsSeg& sg = p->segs[l.first + i];
      TopoDS_Edge e = make_seg_edge(p, sg, f);
      BRepPrimAPI_MakePrism mk(e, v, true);
      mk.Build();
      if (!mk.IsDone()) return fail("extruding a curve failed");
      for (TopExp_Explorer fx(mk.Shape(), TopAbs_FACE); fx.More(); fx.Next()) {
        faces.push_back(fx.Current());
        tags.push_back(sg.tag);
      }
    }
  }
  if (faces.empty()) return fail("no curves to extrude");
  TopoDS_Shape result = sew_faces(faces, 1e-6);
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  // Sewing keeps the faces' geometry: match each output face to its input by a point on it.
  for (int i = 1; i <= out_faces.Extent(); ++i) {
    GProp_GProps gp;
    BRepGProp::SurfaceProperties(out_faces(i), gp);
    gp_Pnt c = gp.CentreOfMass();
    double best = 1e300;
    size_t which = 0;
    for (size_t k = 0; k < faces.size(); ++k) {
      GProp_GProps g2;
      BRepGProp::SurfaceProperties(faces[k], g2);
      double d = g2.CentreOfMass().Distance(c);
      if (d < best) {
        best = d;
        which = k;
      }
    }
    push_origin(o, i - 1, GS_ORIGIN_LATERAL, int64_t(tags[which]), 0);
  }
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_patch(const GsProfile* p, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!p || !out || !origins) return fail("null argument");
  Frame f = make_frame(p, 0.0);
  std::vector<TaggedEdge> tagged;
  std::vector<uint32_t> face_region;
  std::vector<TopoDS_Face> faces = build_faces(p, f, tagged, face_region);
  if (faces.empty()) return fail("no region to fill");
  TopoDS_Shape result;
  if (faces.size() == 1) {
    result = faces[0];
  } else {
    TopoDS_Compound comp;
    BRep_Builder b;
    b.MakeCompound(comp);
    for (const auto& fc : faces) b.Add(comp, fc);
    result = comp;
  }
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (size_t i = 0; i < faces.size(); ++i) {
    int idx = out_faces.FindIndex(faces[i]);
    if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_CAP, 0, face_region[i]);
  }
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

// The input faces' images in `r` (by Modified, else unchanged).
static void input_origins(BRepBuilderAPI_MakeShape& mk, const TopoDS_Shape& in, const TopoDS_Shape& r, uint32_t input,
                          int kind, std::vector<int64_t>& o) {
  ShapeMap in_faces, out_faces;
  TopExp::MapShapes(in, TopAbs_FACE, in_faces);
  TopExp::MapShapes(r, TopAbs_FACE, out_faces);
  for (int i = 1; i <= in_faces.Extent(); ++i) {
    const ShapeList& mods = mk.Modified(in_faces(i));
    if (mods.IsEmpty()) {
      int idx = out_faces.FindIndex(in_faces(i));
      if (idx > 0) push_origin(o, idx - 1, kind, input, i - 1);
    }
    for (ShapeList::Iterator it(mods); it.More(); it.Next()) {
      int idx = out_faces.FindIndex(it.Value());
      if (idx > 0) push_origin(o, idx - 1, kind, input, i - 1);
    }
  }
}

// For every sub-shape of `type` in `base` (an offset copy of `orig`), the index (0-based)
// of the `orig` sub-shape it came from: by Modified, else the nearest by centre of mass.
static std::vector<int64_t> back_to_input(BRepBuilderAPI_MakeShape& mk, const TopoDS_Shape& orig,
                                          const TopoDS_Shape& base, TopAbs_ShapeEnum type) {
  ShapeMap in, at;
  TopExp::MapShapes(orig, type, in);
  TopExp::MapShapes(base, type, at);
  std::vector<int64_t> map(size_t(at.Extent()), -1);
  for (int i = 1; i <= in.Extent(); ++i) {
    const ShapeList& mods = mk.Modified(in(i));
    if (mods.IsEmpty()) {
      int idx = at.FindIndex(in(i));
      if (idx > 0) map[size_t(idx - 1)] = i - 1;
    }
    for (ShapeList::Iterator it(mods); it.More(); it.Next()) {
      int idx = at.FindIndex(it.Value());
      if (idx > 0) map[size_t(idx - 1)] = i - 1;
    }
  }
  auto centre = [type](const TopoDS_Shape& x) {
    GProp_GProps gp;
    if (type == TopAbs_FACE)
      BRepGProp::SurfaceProperties(x, gp);
    else
      BRepGProp::LinearProperties(x, gp);
    return gp.CentreOfMass();
  };
  for (int j = 1; j <= at.Extent(); ++j) {
    if (map[size_t(j - 1)] >= 0 || in.Extent() == 0) continue;
    gp_Pnt c = centre(at(j));
    double best = 1e300;
    for (int i = 1; i <= in.Extent(); ++i) {
      double d = centre(in(i)).Distance(c);
      if (d < best) {
        best = d;
        map[size_t(j - 1)] = i - 1;
      }
    }
  }
  return map;
}

int gs_thicken(const GsShape* s, double thickness, int both, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!s || !out || !origins) return fail("null argument");
  if (!std::isfinite(thickness) || std::fabs(thickness) < 1e-9) return fail("thickness must be non-zero");
  TopoDS_Shape base = s->s;
  // With both sides, the thickening starts from a half-offset copy; its faces and edges are
  // mapped back to the sheet's so origins name the input body's own topology.
  std::vector<int64_t> face_back, edge_back;
  if (both) {
    BRepOffsetAPI_MakeOffsetShape half;
    half.PerformBySimple(base, -thickness / 2.0);
    if (!half.IsDone()) return fail("thicken failed (offsetting the sheet)");
    base = half.Shape();
    face_back = back_to_input(half, s->s, base, TopAbs_FACE);
    edge_back = back_to_input(half, s->s, base, TopAbs_EDGE);
  }
  BRepOffsetAPI_MakeThickSolid mk;
  mk.MakeThickSolidBySimple(base, thickness);
  if (!mk.IsDone()) return fail("thicken failed");
  TopoDS_Shape result = mk.Shape();
  if (result.IsNull()) return fail("thicken produced no result");
  std::vector<int64_t> o;
  input_origins(mk, base, result, 0, GS_ORIGIN_INPUT, o);
  // The side walls along the sheet's free edges.
  {
    ShapeMap in_edges, out_faces;
    TopExp::MapShapes(base, TopAbs_EDGE, in_edges);
    TopExp::MapShapes(result, TopAbs_FACE, out_faces);
    for (int i = 1; i <= in_edges.Extent(); ++i) {
      const ShapeList& gen = mk.Generated(in_edges(i));
      for (ShapeList::Iterator it(gen); it.More(); it.Next()) {
        if (it.Value().ShapeType() != TopAbs_FACE) continue;
        int idx = out_faces.FindIndex(it.Value());
        if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_FROM_EDGE, 0, i - 1);
      }
    }
    // Faces still without an origin (OCCT keeps no history for them): the nearest input face.
    std::vector<bool> has(size_t(out_faces.Extent()), false);
    for (size_t k = 0; k + 3 < o.size(); k += 4) {
      int64_t f = o[k];
      if (f >= 0 && size_t(f) < has.size()) has[size_t(f)] = true;
    }
    ShapeMap in_faces;
    TopExp::MapShapes(base, TopAbs_FACE, in_faces);
    for (int i = 1; i <= out_faces.Extent(); ++i) {
      if (has[size_t(i - 1)] || in_faces.Extent() == 0) continue;
      GProp_GProps gp;
      BRepGProp::SurfaceProperties(out_faces(i), gp);
      double best = 1e300;
      int which = 1;
      for (int j = 1; j <= in_faces.Extent(); ++j) {
        GProp_GProps g2;
        BRepGProp::SurfaceProperties(in_faces(j), g2);
        double d = g2.CentreOfMass().Distance(gp.CentreOfMass());
        if (d < best) {
          best = d;
          which = j;
        }
      }
      push_origin(o, i - 1, GS_ORIGIN_INPUT, 0, which - 1);
    }
  }
  if (both) {
    std::vector<int64_t> kept;
    for (size_t k = 0; k + 3 < o.size(); k += 4) {
      const std::vector<int64_t>& back = o[k + 1] == GS_ORIGIN_FROM_EDGE ? edge_back : face_back;
      int64_t at = o[k + 3];
      if (at < 0 || size_t(at) >= back.size() || back[size_t(at)] < 0) continue;
      push_origin(kept, o[k], o[k + 1], o[k + 2], back[size_t(at)]);
    }
    o.swap(kept);
  }
  // MakeThickSolid's solid can come out inside out (its faces' normals inward: a negative
  // volume) depending on the side the sheet's normals face; turn it the right way out. The face
  // and edge indices are unchanged (shape maps ignore orientation).
  {
    GProp_GProps vol;
    BRepGProp::VolumeProperties(result, vol);
    if (vol.Mass() < 0) result.Reverse();
  }
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}

int gs_stitch(const GsShape* const* shapes, size_t n, double tol, GsShape** out, GsBuf* origins) {
  GS_GUARD_BEGIN
  if (!shapes || !out || !origins || n == 0) return fail("nothing to stitch");
  if (!std::isfinite(tol) || tol <= 0) return fail("tolerance must be positive");
  BRepBuilderAPI_Sewing sew(tol);
  for (size_t i = 0; i < n; ++i) {
    if (!shapes[i]) return fail("null shape");
    sew.Add(shapes[i]->s);
  }
  sew.Perform();
  TopoDS_Shape r = sew.SewedShape();
  if (r.IsNull()) return fail("stitching produced no result");
  // The result becomes a solid only when it is one closed shell holding every face: a
  // sheet that did not connect within the tolerance keeps the result a (sheet) compound
  // rather than being dropped.
  TopoDS_Shape result = r;
  {
    int shells = 0, all_faces = 0, shell_faces = 0;
    TopoDS_Shell only;
    for (TopExp_Explorer sx(r, TopAbs_SHELL); sx.More(); sx.Next()) {
      ++shells;
      only = TopoDS::Shell(sx.Current());
    }
    for (TopExp_Explorer fx(r, TopAbs_FACE); fx.More(); fx.Next()) ++all_faces;
    if (shells == 1)
      for (TopExp_Explorer fx(only, TopAbs_FACE); fx.More(); fx.Next()) ++shell_faces;
    if (shells == 1 && shell_faces == all_faces && BRep_Tool::IsClosed(only)) {
      BRepBuilderAPI_MakeSolid ms(only);
      if (ms.IsDone()) {
        ShapeFix_Solid fix;
        fix.Init(ms.Solid());
        fix.Perform();
        TopoDS_Shape solid = fix.Solid();
        if (!solid.IsNull()) result = solid;
      }
    }
  }
  ShapeMap out_faces;
  TopExp::MapShapes(result, TopAbs_FACE, out_faces);
  std::vector<int64_t> o;
  for (size_t k = 0; k < n; ++k) {
    ShapeMap in_faces;
    TopExp::MapShapes(shapes[k]->s, TopAbs_FACE, in_faces);
    for (int i = 1; i <= in_faces.Extent(); ++i) {
      TopoDS_Shape img = sew.IsModified(in_faces(i)) ? sew.Modified(in_faces(i)) : in_faces(i);
      int idx = out_faces.FindIndex(img);
      if (idx <= 0) {
        // After MakeSolid the faces are the same TShapes; compare without orientation.
        for (int j = 1; j <= out_faces.Extent() && idx <= 0; ++j)
          if (out_faces(j).IsSame(img)) idx = j;
      }
      if (idx > 0) push_origin(o, idx - 1, GS_ORIGIN_INPUT, int64_t(k), i - 1);
    }
  }
  // Faces whose history was lost (healing copies them): the input face at the same place.
  std::vector<bool> has(size_t(out_faces.Extent()), false);
  for (size_t q = 0; q + 3 < o.size(); q += 4)
    if (o[q] >= 0 && size_t(o[q]) < has.size()) has[size_t(o[q])] = true;
  for (int i = 1; i <= out_faces.Extent(); ++i) {
    if (has[size_t(i - 1)]) continue;
    GProp_GProps gp;
    BRepGProp::SurfaceProperties(out_faces(i), gp);
    double best = 1e300;
    int64_t bk = -1, bf = -1;
    for (size_t k = 0; k < n; ++k) {
      ShapeMap in_faces;
      TopExp::MapShapes(shapes[k]->s, TopAbs_FACE, in_faces);
      for (int j = 1; j <= in_faces.Extent(); ++j) {
        GProp_GProps g2;
        BRepGProp::SurfaceProperties(in_faces(j), g2);
        double d = g2.CentreOfMass().Distance(gp.CentreOfMass()) + std::fabs(g2.Mass() - gp.Mass());
        if (d < best) {
          best = d;
          bk = int64_t(k);
          bf = j - 1;
        }
      }
    }
    if (bk >= 0) push_origin(o, i - 1, GS_ORIGIN_INPUT, bk, bf);
  }
  to_buf(o, origins);
  *out = new GsShape{result};
  return GS_OK;
  GS_GUARD_END
}
