// Deterministic reference records for OCCT 8.0.1's package GeomAdaptor (GeomAdaptor_Curve, _Surface, _TransformedCurve,
// _TransformedSurface, _SurfaceOfRevolution, _SurfaceOfLinearExtrusion, GeomAdaptor::MakeCurve / MakeSurface), see
// docs/api-geomadaptor.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal.
// The geometry the adaptors work on is a closed set of Geom curves (`def_gcurve`), Geom surfaces (`def_gsurface`) and 2D curves
// (`def_gcurve2d`), printed in full by the first records and referred to by index afterwards. A call that may fail is
// printed as `1 <results>` or `0` (it raised an exception): the exception classes are not compared.
#include <Adaptor2d_Curve2d.hxx>
#include <Adaptor3d_Curve.hxx>
#include <Adaptor3d_CurveOnSurface.hxx>
#include <Adaptor3d_Surface.hxx>
#include <Geom2dAdaptor_Curve.hxx>
#include <Geom2d_Circle.hxx>
#include <Geom2d_Line.hxx>
#include <GeomAbs_CurveType.hxx>
#include <GeomAbs_Shape.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <GeomAdaptor.hxx>
#include <GeomAdaptor_Curve.hxx>
#include <GeomAdaptor_Surface.hxx>
#include <GeomAdaptor_SurfaceOfLinearExtrusion.hxx>
#include <GeomAdaptor_SurfaceOfRevolution.hxx>
#include <GeomAdaptor_TransformedCurve.hxx>
#include <GeomAdaptor_TransformedSurface.hxx>
#include <GeomEval_RepCurveDesc.hxx>
#include <GeomEval_RepSurfaceDesc.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_Conic.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BezierSurface.hxx>
#include <Geom_Circle.hxx>
#include <Geom_ConicalSurface.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <Geom_Ellipse.hxx>
#include <Geom_Hyperbola.hxx>
#include <Geom_Line.hxx>
#include <Geom_OffsetCurve.hxx>
#include <Geom_OffsetSurface.hxx>
#include <Geom_Parabola.hxx>
#include <Geom_Plane.hxx>
#include <Geom_RectangularTrimmedSurface.hxx>
#include <Geom_SphericalSurface.hxx>
#include <Geom_SurfaceOfLinearExtrusion.hxx>
#include <Geom_SurfaceOfRevolution.hxx>
#include <Geom_ToroidalSurface.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips.hxx>
#include <gp_Hypr.hxx>
#include <gp_Lin.hxx>
#include <gp_Parab.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <functional>
#include <iostream>
#include <string>
#include <vector>

// ---- output -------------------------------------------------------------------------------------
static std::string line;
static uint64_t bitsOf(double x) {
  uint64_t b;
  std::memcpy(&b, &x, 8);
  return b;
}
static void F(double x) {
  char t[24];
  std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bitsOf(x));
  line += t;
}
static void I(long long v) { line += ' ' + std::to_string(v); }
static void B(bool v) { I(v ? 1 : 0); }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void begin(const char* name) {
  line = name;
  if (std::getenv("TR")) std::fprintf(stderr, "BEGIN %s\n", name);
}
static void bar() {
  line += " |";
  smash();
}
static void end() { std::cout << line << std::endl; }
#define CASE(...) \
  { \
    size_t keep_ = line.size(); \
    try { \
      __VA_ARGS__ \
    } catch (Standard_Failure&) { \
      line.resize(keep_); \
      line += " throws"; \
    } catch (std::exception&) { \
      line.resize(keep_); \
      line += " throws"; \
    } \
    end(); \
  }
// ATT(...): a call that may raise: ` 1 <what the body printed>`, or ` 0`
#define ATT(...) \
  { \
    size_t keepa_ = line.size(); \
    try { \
      line += " 1"; \
      __VA_ARGS__ \
    } catch (Standard_Failure&) { \
      line.resize(keepa_); \
      line += " 0"; \
    } catch (std::exception&) { \
      line.resize(keepa_); \
      line += " 0"; \
    } \
  }

static uint64_t S0 = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S0 ^= S0 << 13;
  S0 ^= S0 >> 7;
  S0 ^= S0 << 17;
  return (double)(S0 >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)

// ---- printing of gp values ---------------------------------------------------------------------------------------------
static void P2(const gp_Pnt2d& p) { F(p.X()); F(p.Y()); }
static void P3(const gp_Pnt& p) { F(p.X()); F(p.Y()); F(p.Z()); }
static void V3(const gp_Vec& v) { F(v.X()); F(v.Y()); F(v.Z()); }
static void D3(const gp_Dir& d) { F(d.X()); F(d.Y()); F(d.Z()); }
static void A1(const gp_Ax1& a) { P3(a.Location()); D3(a.Direction()); }
static void A2(const gp_Ax2& a) { P3(a.Location()); D3(a.Direction()); D3(a.XDirection()); D3(a.YDirection()); }
static void A3(const gp_Ax3& a) { P3(a.Location()); D3(a.Direction()); D3(a.XDirection()); D3(a.YDirection()); }
static void PrLin(const gp_Lin& l) { P3(l.Location()); D3(l.Direction()); }
static void PrCirc(const gp_Circ& c) { A2(c.Position()); F(c.Radius()); }
static void PrElips(const gp_Elips& e) { A2(e.Position()); F(e.MajorRadius()); F(e.MinorRadius()); }
static void PrHypr(const gp_Hypr& h) { A2(h.Position()); F(h.MajorRadius()); F(h.MinorRadius()); }
static void PrParab(const gp_Parab& p) { A2(p.Position()); F(p.Focal()); }
static void PrPln(const gp_Pln& p) { A3(p.Position()); }
static void PrCyl(const gp_Cylinder& c) { A3(c.Position()); F(c.Radius()); }
static void PrCone(const gp_Cone& c) { A3(c.Position()); F(c.RefRadius()); F(c.SemiAngle()); }
static void PrSph(const gp_Sphere& s) { A3(s.Position()); F(s.Radius()); }
static void PrTor(const gp_Torus& t) { A3(t.Position()); F(t.MajorRadius()); F(t.MinorRadius()); }

static const GeomAbs_Shape SHP[7] = {GeomAbs_C0, GeomAbs_G1, GeomAbs_C1, GeomAbs_G2, GeomAbs_C2, GeomAbs_C3, GeomAbs_CN};

// ---- the curves ------------------------------------------------------------------------------------------------------------
struct CS {
  int kind = 0;  // 0 Line, 1 Circle, 2 Ellipse, 3 Hyperbola, 4 Parabola, 5 Bezier, 6 BSpline, 7 Offset, 8 Trimmed
  double loc[3] = {0, 0, 0}, n[3] = {0, 0, 1}, x[3] = {1, 0, 0};
  double r1 = 1, r2 = 1;
  int basis = -1;
  double u1 = 0, u2 = 1;
  int deg = 1, per = 0;
  std::vector<gp_Pnt> poles;
  std::vector<double> w, k;
  std::vector<int> m;
  int rep = -1;  // index of the curve of a full evaluation representation
};
static const int NGC = 25;
static CS GC[NGC];
static occ::handle<Geom_Curve> CC[NGC];

static gp_Pnt rp() { return gp_Pnt(rv() * 1.5, rv() * 1.5, rv() * 1.5); }
static gp_Ax2 ax2Of(const CS& s) { return gp_Ax2(gp_Pnt(s.loc[0], s.loc[1], s.loc[2]), gp_Dir(s.n[0], s.n[1], s.n[2]), gp_Dir(s.x[0], s.x[1], s.x[2])); }
static void setAx(CS& s, double lx, double ly, double lz, double nx, double ny, double nz, double xx, double xy, double xz) {
  s.loc[0] = lx; s.loc[1] = ly; s.loc[2] = lz;
  s.n[0] = nx; s.n[1] = ny; s.n[2] = nz;
  s.x[0] = xx; s.x[1] = xy; s.x[2] = xz;
}
static NCollection_Array1<gp_Pnt> polyArr(const std::vector<gp_Pnt>& p) {
  NCollection_Array1<gp_Pnt> a(1, (int)p.size());
  for (size_t i = 0; i < p.size(); ++i) a.SetValue((int)i + 1, p[i]);
  return a;
}
static NCollection_Array1<double> dblArr(const std::vector<double>& p) {
  NCollection_Array1<double> a(1, (int)p.size());
  for (size_t i = 0; i < p.size(); ++i) a.SetValue((int)i + 1, p[i]);
  return a;
}
static NCollection_Array1<int> intArr(const std::vector<int>& p) {
  NCollection_Array1<int> a(1, (int)p.size());
  for (size_t i = 0; i < p.size(); ++i) a.SetValue((int)i + 1, p[i]);
  return a;
}

static void makeCurveSpecs() {
  // 0 line, 1 circle, 2 ellipse, 3 hyperbola, 4 parabola
  GC[0].kind = 0; setAx(GC[0], 0.1, -0.2, 0.3, 0.6, 0.0, 0.8, 1, 0, 0);
  GC[1].kind = 1; setAx(GC[1], 0.5, -0.3, 0.2, 0.2, 0.3, 0.9, 1.0, 0.0, -0.2); GC[1].r1 = 1.7;
  GC[2].kind = 2; setAx(GC[2], -0.4, 0.6, 0.1, 0.1, -0.5, 0.8, 0.9, 0.2, -0.3); GC[2].r1 = 2.2; GC[2].r2 = 1.3;
  GC[3].kind = 3; setAx(GC[3], 0.3, 0.3, -0.6, -0.3, 0.2, 0.9, 0.8, 0.1, 0.35); GC[3].r1 = 1.5; GC[3].r2 = 0.8;
  GC[4].kind = 4; setAx(GC[4], 0.0, 0.5, 0.5, 0.4, 0.4, 0.8, 1.0, -0.2, -0.5); GC[4].r1 = 0.9;
  // 5 Bezier, 6 rational Bezier
  GC[5].kind = 5; for (int i = 0; i < 4; ++i) GC[5].poles.push_back(rp());
  GC[6].kind = 5; for (int i = 0; i < 5; ++i) { GC[6].poles.push_back(rp()); GC[6].w.push_back(0.5 + rnd()); }
  // 7 BSpline (C1 and C2 knots), 8 periodic, 9 rational
  GC[7].kind = 6; GC[7].deg = 3; GC[7].k = {0.0, 1.5, 3.0, 4.5}; GC[7].m = {4, 2, 1, 4};
  for (int i = 0; i < 7; ++i) GC[7].poles.push_back(rp());
  GC[8].kind = 6; GC[8].deg = 3; GC[8].per = 1; GC[8].k = {0.0, 1.0, 2.0, 3.0, 4.0}; GC[8].m = {1, 1, 1, 1, 1};
  for (int i = 0; i < 4; ++i) GC[8].poles.push_back(rp());
  GC[9].kind = 6; GC[9].deg = 2; GC[9].k = {0.0, 1.0, 2.0, 3.0}; GC[9].m = {3, 1, 1, 3};
  for (int i = 0; i < 5; ++i) { GC[9].poles.push_back(rp()); GC[9].w.push_back(0.5 + rnd()); }
  // 10 offset of the circle, 11 offset of the B-spline, 12 / 13 trimmed circle / B-spline
  GC[10].kind = 7; GC[10].basis = 1; GC[10].r1 = 0.4; setAx(GC[10], 0, 0, 0, 0.2, 0.3, 0.9, 1, 0, 0);
  GC[11].kind = 7; GC[11].basis = 7; GC[11].r1 = -0.3; setAx(GC[11], 0, 0, 0, 0.0, 0.0, 1.0, 1, 0, 0);
  GC[12].kind = 8; GC[12].basis = 1; GC[12].u1 = 0.3; GC[12].u2 = 2.4;
  GC[13].kind = 8; GC[13].basis = 7; GC[13].u1 = 0.5; GC[13].u2 = 3.9;
  // 14 B-spline of degree 4 with every multiplicity
  GC[14].kind = 6; GC[14].deg = 4; GC[14].k = {0.0, 1.0, 2.0, 3.0, 4.0, 5.0}; GC[14].m = {5, 1, 2, 4, 1, 5};
  for (int i = 0; i < 13; ++i) GC[14].poles.push_back(rp());
  // 15 offset of the Bezier, 16 Bezier with a vanishing first derivative, 17 its offset
  GC[15].kind = 7; GC[15].basis = 5; GC[15].r1 = 0.25; setAx(GC[15], 0, 0, 0, 0.0, 0.0, 1.0, 1, 0, 0);
  GC[16].kind = 5; GC[16].poles = {gp_Pnt(0, 0, 0), gp_Pnt(0, 0, 0), gp_Pnt(1, 1, 0.2), gp_Pnt(2, 0, 0.4)};
  GC[17].kind = 7; GC[17].basis = 16; GC[17].r1 = 0.2; setAx(GC[17], 0, 0, 0, 0.0, 0.0, 1.0, 1, 0, 0);
  // 18 Bezier, 19 B-spline, 20 offset with an evaluation representation
  GC[18].kind = 5; for (int i = 0; i < 4; ++i) GC[18].poles.push_back(rp()); GC[18].rep = 7;
  GC[19].kind = 6; GC[19].deg = 2; GC[19].k = {0.0, 1.0, 2.5}; GC[19].m = {3, 1, 3};
  for (int i = 0; i < 4; ++i) GC[19].poles.push_back(rp());
  GC[19].rep = 1;
  GC[20].kind = 7; GC[20].basis = 5; GC[20].r1 = 0.15; setAx(GC[20], 0, 0, 0, 0.0, 1.0, 0.0, 1, 0, 0); GC[20].rep = 0;
  // 21: degree 4 with simple knots (C3), 22: degree 3 with simple knots (C2), 23 / 24: their offsets
  GC[21].kind = 6; GC[21].deg = 4; GC[21].k = {0.0, 1.0, 2.0, 3.0}; GC[21].m = {5, 1, 1, 5};
  for (int i = 0; i < 7; ++i) GC[21].poles.push_back(rp());
  GC[22].kind = 6; GC[22].deg = 3; GC[22].k = {0.0, 1.0, 2.0, 3.0}; GC[22].m = {4, 1, 1, 4};
  for (int i = 0; i < 6; ++i) GC[22].poles.push_back(rp());
  GC[23].kind = 7; GC[23].basis = 21; GC[23].r1 = 0.2; setAx(GC[23], 0, 0, 0, 0.0, 0.0, 1.0, 1, 0, 0);
  GC[24].kind = 7; GC[24].basis = 22; GC[24].r1 = -0.2; setAx(GC[24], 0, 0, 0, 0.0, 0.0, 1.0, 1, 0, 0);
}

static occ::handle<Geom_Curve> buildCurve(int i) {
  const CS& s = GC[i];
  occ::handle<Geom_Curve> c;
  switch (s.kind) {
    case 0: c = new Geom_Line(gp_Pnt(s.loc[0], s.loc[1], s.loc[2]), gp_Dir(s.n[0], s.n[1], s.n[2])); break;
    case 1: c = new Geom_Circle(ax2Of(s), s.r1); break;
    case 2: c = new Geom_Ellipse(ax2Of(s), s.r1, s.r2); break;
    case 3: c = new Geom_Hyperbola(ax2Of(s), s.r1, s.r2); break;
    case 4: c = new Geom_Parabola(ax2Of(s), s.r1); break;
    case 5:
      if (s.w.empty()) c = new Geom_BezierCurve(polyArr(s.poles));
      else c = new Geom_BezierCurve(polyArr(s.poles), dblArr(s.w));
      break;
    case 6:
      if (s.w.empty()) c = new Geom_BSplineCurve(polyArr(s.poles), dblArr(s.k), intArr(s.m), s.deg, s.per != 0);
      else c = new Geom_BSplineCurve(polyArr(s.poles), dblArr(s.w), dblArr(s.k), intArr(s.m), s.deg, s.per != 0);
      break;
    case 7: c = new Geom_OffsetCurve(CC[s.basis], s.r1, gp_Dir(s.n[0], s.n[1], s.n[2])); break;
    default: c = new Geom_TrimmedCurve(CC[s.basis], s.u1, s.u2); break;
  }
  if (s.rep >= 0) {
    occ::handle<GeomEval_RepCurveDesc::Full> d = new GeomEval_RepCurveDesc::Full();
    d->Representation = CC[s.rep];
    if (s.kind == 5) occ::down_cast<Geom_BezierCurve>(c)->SetEvalRepresentation(d);
    else if (s.kind == 6) occ::down_cast<Geom_BSplineCurve>(c)->SetEvalRepresentation(d);
    else occ::down_cast<Geom_OffsetCurve>(c)->SetEvalRepresentation(d);
  }
  return c;
}
static void prCS(const CS& s) {
  I(s.kind);
  for (double v : s.loc) F(v);
  for (double v : s.n) F(v);
  for (double v : s.x) F(v);
  F(s.r1); F(s.r2);
  I(s.basis); F(s.u1); F(s.u2);
  I(s.deg); I(s.per);
  I((long long)s.poles.size());
  for (const gp_Pnt& p : s.poles) P3(p);
  I((long long)s.w.size());
  for (double v : s.w) F(v);
  I((long long)s.k.size());
  for (double v : s.k) F(v);
  for (int v : s.m) I(v);
  I(s.rep);
}

// the parameter range of the adaptor for the range selector r (finite for every curve)
static void curveRange(int i, int r, double& f, double& l) {
  double cf = CC[i]->FirstParameter(), cl = CC[i]->LastParameter();
  if (Precision::IsInfinite(cf) || Precision::IsInfinite(cl)) {
    switch (r % 4) {
      case 0: f = -2.0; l = 3.0; break;
      case 1: f = -0.5; l = 4.0; break;
      case 2: f = 0.25; l = 2.0; break;
      default: f = -3.0; l = 0.5; break;
    }
    if (!Precision::IsInfinite(cf)) f = cf + (f + 3.0) * 0.1;
    if (!Precision::IsInfinite(cl)) l = cl - (4.0 - l) * 0.1;
    return;
  }
  double d = cl - cf;
  switch (r % 4) {
    case 0: f = cf; l = cl; break;
    case 1: f = cf + 0.2 * d; l = cl - 0.3 * d; break;
    case 2: f = cf + 0.1 * d; l = cl; break;
    default: f = cf; l = cf + 0.55 * d; break;
  }
}
static bool curveFull(int i, int r) {
  double cf = CC[i]->FirstParameter(), cl = CC[i]->LastParameter();
  return !(Precision::IsInfinite(cf) || Precision::IsInfinite(cl)) && r % 4 == 0;
}
static occ::handle<GeomAdaptor_Curve> loadCurve(int i, int r) {
  double f, l;
  curveRange(i, r, f, l);
  if (curveFull(i, r)) return new GeomAdaptor_Curve(CC[i]);
  return new GeomAdaptor_Curve(CC[i], f, l);
}

// ---- the surfaces ----------------------------------------------------------------------------------------------------------
struct SS {
  int kind = 0;  // 0 Plane, 1 Cylinder, 2 Cone, 3 Sphere, 4 Torus, 5 Bezier, 6 BSpline, 7 Revolution, 8 Extrusion, 9 Offset, 10 RTrimmed
  double loc[3] = {0, 0, 0}, n[3] = {0, 0, 1}, x[3] = {1, 0, 0};
  double r1 = 1, r2 = 1, ang = 0.4;
  int basis = -1;
  double u1 = 0, u2 = 1, v1 = 0, v2 = 1;
  int ud = 1, vd = 1, up = 0, vp = 0;
  int nu = 0, nv = 0;
  std::vector<gp_Pnt> poles;  // row by row
  std::vector<double> w;
  std::vector<double> uk, vk;
  std::vector<int> um, vm;
  int rep = -1;  // index of the surface of a full evaluation representation
};
static const int NGS = 33;
static SS GS[NGS];
static occ::handle<Geom_Surface> SC[NGS];

static void setAxS(SS& s, double lx, double ly, double lz, double nx, double ny, double nz, double xx, double xy, double xz) {
  s.loc[0] = lx; s.loc[1] = ly; s.loc[2] = lz;
  s.n[0] = nx; s.n[1] = ny; s.n[2] = nz;
  s.x[0] = xx; s.x[1] = xy; s.x[2] = xz;
}
static void gridPoles(SS& s, int nu, int nv, bool rational) {
  s.nu = nu; s.nv = nv;
  for (int i = 0; i < nu; ++i)
    for (int j = 0; j < nv; ++j) {
      s.poles.push_back(gp_Pnt(i * 1.1 + 0.3 * rv(), j * 0.9 + 0.3 * rv(), 0.6 * rv()));
      if (rational) s.w.push_back(0.6 + rnd());
    }
}
static void makeSurfaceSpecs() {
  GS[0].kind = 0; setAxS(GS[0], 0.2, -0.1, 0.4, 0.3, 0.2, 0.9, 1.0, 0.1, -0.4);
  GS[1].kind = 1; setAxS(GS[1], -0.3, 0.2, 0.1, 0.1, 0.9, 0.3, 1.0, 0.0, -0.3); GS[1].r1 = 1.3;
  GS[2].kind = 2; setAxS(GS[2], 0.0, 0.4, -0.2, 0.5, -0.2, 0.8, 0.7, 0.4, -0.5); GS[2].r1 = 0.8; GS[2].ang = 0.45;
  GS[3].kind = 3; setAxS(GS[3], 0.4, 0.1, 0.3, -0.2, 0.3, 0.9, 1.0, 0.3, 0.1); GS[3].r1 = 1.6;
  GS[4].kind = 4; setAxS(GS[4], -0.1, -0.4, 0.2, 0.4, 0.1, 0.9, 1.0, -0.3, -0.2); GS[4].r1 = 2.0; GS[4].r2 = 0.6;
  GS[5].kind = 5; gridPoles(GS[5], 3, 3, false);
  GS[6].kind = 5; gridPoles(GS[6], 3, 4, true);
  // 7: B-spline surface with inner knots
  GS[7].kind = 6; GS[7].ud = 3; GS[7].vd = 2; GS[7].uk = {0.0, 1.0, 2.5}; GS[7].um = {4, 1, 4}; GS[7].vk = {0.0, 1.0, 2.0}; GS[7].vm = {3, 1, 3};
  gridPoles(GS[7], 5, 4, false);
  // 8: periodic in u
  GS[8].kind = 6; GS[8].ud = 2; GS[8].vd = 2; GS[8].up = 1; GS[8].uk = {0.0, 1.0, 2.0, 3.0}; GS[8].um = {1, 1, 1, 1}; GS[8].vk = {0.0, 2.0}; GS[8].vm = {3, 3};
  gridPoles(GS[8], 3, 3, false);
  // 9: rational
  GS[9].kind = 6; GS[9].ud = 2; GS[9].vd = 1; GS[9].uk = {0.0, 1.0, 2.0}; GS[9].um = {3, 1, 3}; GS[9].vk = {0.0, 1.0, 3.0}; GS[9].vm = {2, 1, 2};
  gridPoles(GS[9], 4, 3, true);
  // 10: revolution of the B-spline curve, 11: of the line, 12: of the circle
  GS[10].kind = 7; GS[10].basis = 7; setAxS(GS[10], 0.2, 0.1, -0.3, 0.1, 0.2, 0.95, 1, 0, 0);
  GS[11].kind = 7; GS[11].basis = 0; setAxS(GS[11], 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1, 0, 0);
  GS[12].kind = 7; GS[12].basis = 1; setAxS(GS[12], 0.5, 2.5, 0.2, 0.2, 0.3, 0.9, 1, 0, 0);
  // 13: extrusion of the B-spline curve, 14: of the circle, 15: of the line
  GS[13].kind = 8; GS[13].basis = 7; setAxS(GS[13], 0, 0, 0, 0.3, -0.2, 0.9, 1, 0, 0);
  GS[14].kind = 8; GS[14].basis = 1; setAxS(GS[14], 0, 0, 0, 0.0, 0.1, 1.0, 1, 0, 0);
  GS[15].kind = 8; GS[15].basis = 0; setAxS(GS[15], 0, 0, 0, 0.2, 0.9, 0.1, 1, 0, 0);
  // 16..20: offsets of the plane, the sphere, the B-spline surface, the cylinder, the torus
  GS[16].kind = 9; GS[16].basis = 0; GS[16].r1 = 0.35;
  GS[17].kind = 9; GS[17].basis = 3; GS[17].r1 = -0.4;
  GS[18].kind = 9; GS[18].basis = 7; GS[18].r1 = 0.2;
  GS[19].kind = 9; GS[19].basis = 1; GS[19].r1 = 0.3;
  GS[20].kind = 9; GS[20].basis = 4; GS[20].r1 = -0.15;
  // 21 / 22: rectangular trimmed plane / B-spline surface
  GS[21].kind = 10; GS[21].basis = 0; GS[21].u1 = -1.0; GS[21].u2 = 2.0; GS[21].v1 = -0.5; GS[21].v2 = 1.5;
  GS[22].kind = 10; GS[22].basis = 7; GS[22].u1 = 0.3; GS[22].u2 = 2.0; GS[22].v1 = 0.2; GS[22].v2 = 1.7;
  // 23..27: evaluation representations
  GS[23].kind = 5; gridPoles(GS[23], 3, 3, false); GS[23].rep = 7;
  GS[24].kind = 6; GS[24].ud = 2; GS[24].vd = 2; GS[24].uk = {0.0, 2.0}; GS[24].um = {3, 3}; GS[24].vk = {0.0, 1.0, 2.0}; GS[24].vm = {3, 1, 3};
  gridPoles(GS[24], 3, 4, false); GS[24].rep = 5;
  GS[25].kind = 7; GS[25].basis = 0; setAxS(GS[25], 0, 0, 0, 0, 0, 1, 1, 0, 0); GS[25].rep = 1;
  GS[26].kind = 8; GS[26].basis = 1; setAxS(GS[26], 0, 0, 0, 0.0, 0.0, 1.0, 1, 0, 0); GS[26].rep = 0;
  GS[27].kind = 9; GS[27].basis = 0; GS[27].r1 = 0.5; GS[27].rep = 0;
  // 28: B-spline surface of degrees 3, 3 with simple knots (C2), 29 its offset, 30: degrees 4, 4 (C3), 31 its offset
  GS[28].kind = 6; GS[28].ud = 3; GS[28].vd = 3; GS[28].uk = {0.0, 1.0, 2.0}; GS[28].um = {4, 1, 4}; GS[28].vk = {0.0, 1.0, 2.0}; GS[28].vm = {4, 1, 4};
  gridPoles(GS[28], 5, 5, false);
  GS[29].kind = 9; GS[29].basis = 28; GS[29].r1 = 0.25;
  GS[30].kind = 6; GS[30].ud = 4; GS[30].vd = 4; GS[30].uk = {0.0, 1.0, 2.0}; GS[30].um = {5, 1, 5}; GS[30].vk = {0.0, 1.0, 2.0}; GS[30].vm = {5, 1, 5};
  gridPoles(GS[30], 6, 6, false);
  GS[31].kind = 9; GS[31].basis = 30; GS[31].r1 = -0.25;
  // 32: closed in both directions without being periodic
  GS[32].kind = 6; GS[32].ud = 2; GS[32].vd = 2; GS[32].uk = {0.0, 1.0}; GS[32].um = {3, 3}; GS[32].vk = {0.0, 1.0}; GS[32].vm = {3, 3};
  gridPoles(GS[32], 3, 3, false);
  for (int j = 0; j < 3; ++j) GS[32].poles[(size_t)(2 * 3 + j)] = GS[32].poles[(size_t)j];
  for (int i = 0; i < 3; ++i) GS[32].poles[(size_t)(i * 3 + 2)] = GS[32].poles[(size_t)(i * 3)];
}
static gp_Ax3 ax3Of(const SS& s) { return gp_Ax3(gp_Pnt(s.loc[0], s.loc[1], s.loc[2]), gp_Dir(s.n[0], s.n[1], s.n[2]), gp_Dir(s.x[0], s.x[1], s.x[2])); }
static NCollection_Array2<gp_Pnt> grid2(const SS& s) {
  NCollection_Array2<gp_Pnt> a(1, s.nu, 1, s.nv);
  for (int i = 0; i < s.nu; ++i)
    for (int j = 0; j < s.nv; ++j) a.SetValue(i + 1, j + 1, s.poles[(size_t)(i * s.nv + j)]);
  return a;
}
static NCollection_Array2<double> wgrid2(const SS& s) {
  NCollection_Array2<double> a(1, s.nu, 1, s.nv);
  for (int i = 0; i < s.nu; ++i)
    for (int j = 0; j < s.nv; ++j) a.SetValue(i + 1, j + 1, s.w[(size_t)(i * s.nv + j)]);
  return a;
}
static occ::handle<Geom_Surface> buildSurface(int i) {
  const SS& s = GS[i];
  occ::handle<Geom_Surface> c;
  switch (s.kind) {
    case 0: c = new Geom_Plane(ax3Of(s)); break;
    case 1: c = new Geom_CylindricalSurface(ax3Of(s), s.r1); break;
    case 2: c = new Geom_ConicalSurface(ax3Of(s), s.ang, s.r1); break;
    case 3: c = new Geom_SphericalSurface(ax3Of(s), s.r1); break;
    case 4: c = new Geom_ToroidalSurface(ax3Of(s), s.r1, s.r2); break;
    case 5:
      if (s.w.empty()) c = new Geom_BezierSurface(grid2(s));
      else c = new Geom_BezierSurface(grid2(s), wgrid2(s));
      break;
    case 6:
      if (s.w.empty())
        c = new Geom_BSplineSurface(grid2(s), dblArr(s.uk), dblArr(s.vk), intArr(s.um), intArr(s.vm), s.ud, s.vd, s.up != 0, s.vp != 0);
      else
        c = new Geom_BSplineSurface(grid2(s), wgrid2(s), dblArr(s.uk), dblArr(s.vk), intArr(s.um), intArr(s.vm), s.ud, s.vd, s.up != 0, s.vp != 0);
      break;
    case 7: c = new Geom_SurfaceOfRevolution(CC[s.basis], gp_Ax1(gp_Pnt(s.loc[0], s.loc[1], s.loc[2]), gp_Dir(s.n[0], s.n[1], s.n[2]))); break;
    case 8: c = new Geom_SurfaceOfLinearExtrusion(CC[s.basis], gp_Dir(s.n[0], s.n[1], s.n[2])); break;
    case 9: c = new Geom_OffsetSurface(SC[s.basis], s.r1); break;
    default: c = new Geom_RectangularTrimmedSurface(SC[s.basis], s.u1, s.u2, s.v1, s.v2); break;
  }
  if (s.rep >= 0) {
    occ::handle<GeomEval_RepSurfaceDesc::Full> d = new GeomEval_RepSurfaceDesc::Full();
    d->Representation = SC[s.rep];
    if (s.kind == 5) occ::down_cast<Geom_BezierSurface>(c)->SetEvalRepresentation(d);
    else if (s.kind == 6) occ::down_cast<Geom_BSplineSurface>(c)->SetEvalRepresentation(d);
    else if (s.kind == 7) occ::down_cast<Geom_SurfaceOfRevolution>(c)->SetEvalRepresentation(d);
    else if (s.kind == 8) occ::down_cast<Geom_SurfaceOfLinearExtrusion>(c)->SetEvalRepresentation(d);
    else occ::down_cast<Geom_OffsetSurface>(c)->SetEvalRepresentation(d);
  }
  return c;
}
static void prSS(const SS& s) {
  I(s.kind);
  for (double v : s.loc) F(v);
  for (double v : s.n) F(v);
  for (double v : s.x) F(v);
  F(s.r1); F(s.r2); F(s.ang);
  I(s.basis); F(s.u1); F(s.u2); F(s.v1); F(s.v2);
  I(s.ud); I(s.vd); I(s.up); I(s.vp);
  I(s.nu); I(s.nv);
  for (const gp_Pnt& p : s.poles) P3(p);
  I((long long)s.w.size());
  for (double v : s.w) F(v);
  I((long long)s.uk.size());
  for (double v : s.uk) F(v);
  for (int v : s.um) I(v);
  I((long long)s.vk.size());
  for (double v : s.vk) F(v);
  for (int v : s.vm) I(v);
  I(s.rep);
}
// the bounds of the adaptor for the range selector r
static void surfRange(int i, int r, double& u1, double& u2, double& v1, double& v2) {
  double a, b, c, d;
  SC[i]->Bounds(a, b, c, d);
  auto fin = [](double x) { return !Precision::IsInfinite(x); };
  double lo[2] = {a, c}, hi[2] = {b, d};
  double out[2][2];
  for (int k = 0; k < 2; ++k) {
    double f = lo[k], l = hi[k];
    if (!fin(f) || !fin(l)) {
      double ff = (k == 0) ? -2.0 : -1.5, ll = (k == 0) ? 3.0 : 2.5;
      if (fin(f)) ff = f + 0.1;
      if (fin(l)) ll = l - 0.1;
      f = ff; l = ll;
      switch (r % 4) { case 1: f += 0.4; l -= 0.7; break; case 2: f += 0.2; break; case 3: l -= 0.3; break; default: break; }
    } else {
      double dd = l - f;
      switch (r % 4) {
        case 0: break;
        case 1: f += 0.2 * dd; l -= 0.3 * dd; break;
        case 2: f += 0.1 * dd; break;
        default: l = f + 0.55 * dd; break;
      }
    }
    out[k][0] = f; out[k][1] = l;
  }
  u1 = out[0][0]; u2 = out[0][1]; v1 = out[1][0]; v2 = out[1][1];
}
static occ::handle<GeomAdaptor_Surface> loadSurface(int i, int r, double tolU, double tolV) {
  double u1, u2, v1, v2;
  surfRange(i, r, u1, u2, v1, v2);
  double a, b, c, d;
  SC[i]->Bounds(a, b, c, d);
  bool full = r % 4 == 0 && !Precision::IsInfinite(a) && !Precision::IsInfinite(b) && !Precision::IsInfinite(c) && !Precision::IsInfinite(d);
  if (full && tolU == 0.0 && tolV == 0.0) return new GeomAdaptor_Surface(SC[i]);
  return new GeomAdaptor_Surface(SC[i], u1, u2, v1, v2, tolU, tolV);
}

// ---- helpers for the curve families --------------------------------------------------------------------------------------
static void prCurveBasics(const Adaptor3d_Curve& a) {
  I((int)a.GetType());
  F(a.FirstParameter());
  F(a.LastParameter());
}
static void prD0(const Adaptor3d_Curve& a, double u) { P3(a.Value(u)); }
static void prCurveFull(const Adaptor3d_Curve& a) {
  prCurveBasics(a);
  B(a.IsClosed());
  B(a.IsPeriodic());
  ATT(F(a.Period());)
  ATT(I((int)a.Continuity());)
  ATT(F(a.Resolution(1.0e-3));)
}
static const double TFR[9][3] = {{0.0, 0.5, 1.0}, {0.31, 0.32, 0.9}, {1.0, 0.0, 0.5}, {0.5, 0.5, 0.5}, {0.25, 0.75, 0.26},
                                 {0.1, 0.999999, 1.0}, {0.5, 0.0, 1.0}, {0.9, 0.1, 0.9}, {0.0, 0.0, 0.0}};
// an interior knot of a B-spline curve (or the middle of the range)
static double knotOrMid(int c, double f, double l, int which) {
  const CS& s = GC[c];
  if (s.kind == 6 || (s.kind == 8 && GC[s.basis].kind == 6)) {
    const CS& b = s.kind == 6 ? s : GC[s.basis];
    std::vector<double> in;
    for (double k : b.k)
      if (k > f + 1.0e-3 && k < l - 1.0e-3) in.push_back(k);
    if (!in.empty()) return in[(size_t)which % in.size()];
  }
  return f + 0.5 * (l - f);
}

static void curveFamilies() {
  // -- the geometry itself
  for (int i = 0; i < NGC; ++i) {
    begin("def_gcurve"); I(i); bar();
    CASE(
      prCS(GC[i]);
      F(CC[i]->FirstParameter());
      F(CC[i]->LastParameter());
      double f, l;
      curveRange(i, 0, f, l);
      P3(CC[i]->EvalD0(f + 0.4 * (l - f)));)
  }
  // -- Load, the constructors, Reset
  for (int j = 0; j < 80; ++j) {
    int c = j % NGC, r = (j / NGC + j) % 4, v = (j / 3) % 5;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_load"); I(c); I(v); I(full); F(f); F(l); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a;
      auto load = [&](GeomAdaptor_Curve& x, int ci) {
        if (full) x.Load(CC[ci]);
        else {
          double ff, ll;
          curveRange(ci, r, ff, ll);
          x.Load(CC[ci], ff, ll);
        }
      };
      if (v == 0) { a = new GeomAdaptor_Curve(); load(*a, c); }
      else if (v == 1) { a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l); }
      else if (v == 2) { a = new GeomAdaptor_Curve(CC[c]); load(*a, c); }
      else if (v == 3) { a = new GeomAdaptor_Curve(CC[c], f, l); a->Reset(); }
      else { a = new GeomAdaptor_Curve(CC[(c + 1) % NGC]); load(*a, c); }
      if (v == 3) {
        prCurveBasics(*a);
        B(a->Curve().IsNull());
      } else {
        prCurveFull(*a);
        B(a->Curve().get() == CC[c].get());
      })
  }
  // -- invalid loads
  for (int j = 0; j < 12; ++j) {
    int c = (j * 5) % NGC, k = j % 4;
    double f, l;
    curveRange(c, j, f, l);
    begin("gac_load_bad"); I(c); I(k); F(f); F(l); bar();
    CASE(
      GeomAdaptor_Curve a;
      if (k == 0) a.Load(occ::handle<Geom_Curve>());
      else if (k == 1) a.Load(CC[c], l + 0.01, f);
      else if (k == 2) a.Load(CC[c], f, f - 0.5e-7);
      else a = GeomAdaptor_Curve(occ::handle<Geom_Curve>(), f, l);
      prCurveBasics(a);)
  }
  // -- evaluation
  for (int j = 0; j < 63; ++j) {
    int c = j % NGC, r = (j / NGC) % 4, pat = (j / 3 + j / 25) % 9;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double t[3];
    for (int k = 0; k < 3; ++k) t[k] = f + TFR[pat][k] * (l - f);
    if (j % 7 == 3) t[1] = knotOrMid(c, f, l, j);
    if (j % 7 == 5) { t[0] = knotOrMid(c, f, l, j); t[2] = knotOrMid(c, f, l, j + 1); }
    int ord = (j / 21 + j) % 4;
    begin("gac_eval"); I(ord); I(c); I(full); F(f); F(l); F(t[0]); F(t[1]); F(t[2]); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      for (double u : t) {
        std::function<void()> ev[4] = {
          [&] { ATT(P3(a->Value(u));) },
          [&] { ATT(gp_Pnt p; gp_Vec d1; a->D1(u, p, d1); P3(p); V3(d1);) },
          [&] { ATT(gp_Pnt p; gp_Vec d1, d2; a->D2(u, p, d1, d2); P3(p); V3(d1); V3(d2);) },
          [&] { ATT(gp_Pnt p; gp_Vec d1, d2, d3; a->D3(u, p, d1, d2, d3); P3(p); V3(d1); V3(d2); V3(d3);) }};
        for (int q = 0; q < 4; ++q) ev[(q + ord) % 4]();
        for (int n = 1; n <= 5; ++n) ATT(V3(a->DN(u, n));)
      })
  }
  // -- continuity intervals
  for (int j = 0; j < NGC * 7; ++j) {
    int c = j % NGC, s = j / NGC, r = (c * 3 + s) % 4;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_intervals"); I(c); I(full); F(f); F(l); I(s); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      ATT(I((int)a->Continuity());)
      ATT(I(a->NbIntervals(SHP[s]));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); a->Intervals(T, SHP[s]); for (int i = 1; i <= 8; ++i) F(T(i));))
  }
  // -- the typed accessors
  for (int j = 0; j < 63; ++j) {
    int c = j % NGC, r = (j / NGC) % 4;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_info"); I(c); I(full); F(f); F(l); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      prCurveBasics(*a);
      ATT(PrLin(a->Line());)
      ATT(PrCirc(a->Circle());)
      ATT(PrElips(a->Ellipse());)
      ATT(PrHypr(a->Hyperbola());)
      ATT(PrParab(a->Parabola());)
      ATT(I(a->Degree());)
      B(a->IsRational());
      ATT(I(a->NbPoles());)
      ATT(I(a->NbKnots());)
      ATT(occ::handle<Geom_BezierCurve> b = a->Bezier(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)););
      ATT(occ::handle<Geom_BSplineCurve> b = a->BSpline(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)); F(b->Knot(1)););
      ATT(occ::handle<Geom_OffsetCurve> b = a->OffsetCurve(); B(b.IsNull()); F(b->Offset()); D3(b->Direction());))
  }
  // -- resolution
  for (int j = 0; j < 42; ++j) {
    int c = j % NGC, r = (j / NGC) % 4;
    static const double R3[6] = {1.0e-3, 5.0, 1.0e-7, 0.5, 2.0, 100.0};
    double rr = R3[(j / 3) % 6];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_resolution"); I(c); I(full); F(f); F(l); F(rr); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      ATT(F(a->Resolution(rr));))
  }
  // -- Trim and ShallowCopy
  for (int j = 0; j < 42; ++j) {
    int c = j % NGC, r = (j / NGC) % 4;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double t1 = f + 0.2 * (l - f), t2 = f + 0.7 * (l - f);
    double tol = (j % 2) ? 1.0e-3 : 0.0;
    begin("gac_trim"); I(c); I(full); F(f); F(l); F(t1); F(t2); F(tol); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      occ::handle<Adaptor3d_Curve> t = a->Trim(t1, t2, tol);
      prCurveBasics(*t);
      ATT(prD0(*t, 0.5 * (t1 + t2));)
      occ::handle<Adaptor3d_Curve> cp = a->ShallowCopy();
      prCurveBasics(*cp);
      ATT(prD0(*cp, 0.5 * (f + l));)
      ATT(I(cp->NbIntervals(GeomAbs_C1));))
  }
}

// ---- helpers for the surface families ------------------------------------------------------------------------------------
static void prSurfBasics(const Adaptor3d_Surface& a) {
  I((int)a.GetType());
  F(a.FirstUParameter());
  F(a.LastUParameter());
  F(a.FirstVParameter());
  F(a.LastVParameter());
}
static void prSurfFull(const Adaptor3d_Surface& a) {
  prSurfBasics(a);
  B(a.IsUClosed());
  B(a.IsVClosed());
  B(a.IsUPeriodic());
  ATT(F(a.UPeriod());)
  B(a.IsVPeriodic());
  ATT(F(a.VPeriod());)
  ATT(I((int)a.UContinuity());)
  ATT(I((int)a.VContinuity());)
  ATT(F(a.UResolution(1.0e-3));)
  ATT(F(a.VResolution(1.0e-3));)
}
static void prCurveHandle(const occ::handle<Adaptor3d_Curve>& c) {
  B(c.IsNull());
  if (!c.IsNull()) {
    prCurveBasics(*c);
  }
}
static void prSurfHandle(const occ::handle<Adaptor3d_Surface>& c) {
  B(c.IsNull());
  if (!c.IsNull()) {
    prSurfBasics(*c);
  }
}
static const double SFR[10][2] = {{0.0, 0.0}, {1.0, 1.0}, {1.0, 0.0}, {0.5, 0.5}, {1.0e-4, 0.5},
                                  {0.5, 1.0 - 1.0e-4}, {0.3, 0.6}, {0.999, 0.001}, {0.25, 0.75}, {0.0, 1.0}};
static const double STOL[4][2] = {{0.0, 0.0}, {0.01, 0.01}, {0.0, 0.02}, {0.005, 0.0}};

static void surfaceFamilies() {
  for (int i = 0; i < NGS; ++i) {
    begin("def_gsurface"); I(i); bar();
    CASE(
      prSS(GS[i]);
      double a, b, c, d;
      SC[i]->Bounds(a, b, c, d);
      F(a); F(b); F(c); F(d);
      double u1, u2, v1, v2;
      surfRange(i, 0, u1, u2, v1, v2);
      P3(SC[i]->EvalD0(u1 + 0.4 * (u2 - u1), v1 + 0.3 * (v2 - v1)));
      F(u1); F(u2); F(v1); F(v2);)
  }
  // -- Load, the constructors
  for (int j = 0; j < 90; ++j) {
    int s = j % NGS, r = (j / NGS + j) % 4, v = (j / 3) % 5, tl = (j / 2 + j / 5) % 4;
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double a, b, c, d;
    SC[s]->Bounds(a, b, c, d);
    bool full = r % 4 == 0 && !Precision::IsInfinite(a) && !Precision::IsInfinite(b) && !Precision::IsInfinite(c) && !Precision::IsInfinite(d);
    full = full && tu == 0.0 && tv == 0.0;
    begin("gas_load"); I(s); I(v); I(full); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x;
      auto load = [&](GeomAdaptor_Surface& y, int si) {
        if (full) y.Load(SC[si]);
        else {
          double p1, p2, q1, q2;
          surfRange(si, r, p1, p2, q1, q2);
          y.Load(SC[si], p1, p2, q1, q2, tu, tv);
        }
      };
      if (v == 0) { x = new GeomAdaptor_Surface(); load(*x, s); }
      else if (v == 1) { x = full ? new GeomAdaptor_Surface(SC[s]) : new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2, tu, tv); }
      else if (v == 2) { x = new GeomAdaptor_Surface(SC[s]); load(*x, s); }
      else if (v == 3) { x = new GeomAdaptor_Surface(SC[(s + 1) % NGS]); load(*x, s); }
      else { x = new GeomAdaptor_Surface(); load(*x, s); occ::handle<Adaptor3d_Surface> cp = x->ShallowCopy(); prSurfBasics(*cp); }
      prSurfFull(*x);
      F(x->ToleranceU()); F(x->ToleranceV());
      B(x->Surface().get() == SC[s].get());)
  }
  // -- invalid loads
  for (int j = 0; j < 12; ++j) {
    int s = (j * 5) % NGS, k = j % 4;
    double u1, u2, v1, v2;
    surfRange(s, j, u1, u2, v1, v2);
    begin("gas_load_bad"); I(s); I(k); F(u1); F(u2); F(v1); F(v2); bar();
    CASE(
      GeomAdaptor_Surface x;
      if (k == 0) x.Load(occ::handle<Geom_Surface>());
      else if (k == 1) x.Load(SC[s], u2 + 0.1, u1, v1, v2);
      else if (k == 2) x.Load(SC[s], u1, u2, v2 + 0.1, v1);
      else x = GeomAdaptor_Surface(SC[s], u1, u1, v1, v1);
      prSurfBasics(x);)
  }
  // -- evaluation
  for (int j = 0; j < 58; ++j) {
    int s = j % NGS, r = (j / NGS + j / 11) % 4, tl = (j / 5 + j / 29) % 4;
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double pt[2][2];
    pt[0][0] = u1 + SFR[p1][0] * (u2 - u1); pt[0][1] = v1 + SFR[p1][1] * (v2 - v1);
    pt[1][0] = u1 + SFR[p2][0] * (u2 - u1); pt[1][1] = v1 + SFR[p2][1] * (v2 - v1);
    int ord = (j / 7 + j) % 4;
    begin("gas_eval"); I(ord); I(s); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2, tu, tv);
      for (auto& q : pt) {
        double u = q[0], v = q[1];
        std::function<void()> ev[4] = {
          [&] { ATT(P3(x->Value(u, v));) },
          [&] { ATT(gp_Pnt p; gp_Vec a, b; x->D1(u, v, p, a, b); P3(p); V3(a); V3(b);) },
          [&] { ATT(gp_Pnt p; gp_Vec a, b, c, d, e; x->D2(u, v, p, a, b, c, d, e); P3(p); V3(a); V3(b); V3(c); V3(d); V3(e);) },
          [&] { ATT(gp_Pnt p; gp_Vec a, b, c, d, e, f, g, h, k; x->D3(u, v, p, a, b, c, d, e, f, g, h, k); P3(p); V3(a); V3(b); V3(c);
                    V3(d); V3(e); V3(f); V3(g); V3(h); V3(k);) }};
        for (int q = 0; q < 4; ++q) ev[(q + ord) % 4]();
        static const int NN[8][2] = {{1, 0}, {0, 1}, {1, 1}, {2, 0}, {0, 2}, {2, 1}, {1, 2}, {3, 1}};
        for (auto& n : NN) ATT(V3(x->DN(u, v, n[0], n[1]));)
      })
  }
  // -- intervals
  for (int j = 0; j < NGS * 7; ++j) {
    int s = j % NGS, sh = j / NGS, r = (s * 3 + sh) % 4;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gas_intervals"); I(s); F(u1); F(u2); F(v1); F(v2); I(sh); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
      ATT(I(x->NbUIntervals(SHP[sh]));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); x->UIntervals(T, SHP[sh]); for (int i = 1; i <= 8; ++i) F(T(i));)
      ATT(I(x->NbVIntervals(SHP[sh]));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); x->VIntervals(T, SHP[sh]); for (int i = 1; i <= 8; ++i) F(T(i));))
  }
  // -- the typed accessors
  for (int j = 0; j < 84; ++j) {
    int s = j % NGS, r = (j / NGS) % 4;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gas_info"); I(s); F(u1); F(u2); F(v1); F(v2); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
      prSurfBasics(*x);
      ATT(PrPln(x->Plane());)
      ATT(PrCyl(x->Cylinder());)
      ATT(PrCone(x->Cone());)
      ATT(PrSph(x->Sphere());)
      ATT(PrTor(x->Torus());)
      ATT(I(x->UDegree());)
      ATT(I(x->NbUPoles());)
      ATT(I(x->VDegree());)
      ATT(I(x->NbVPoles());)
      ATT(I(x->NbUKnots());)
      ATT(I(x->NbVKnots());)
      B(x->IsURational());
      B(x->IsVRational());
      ATT(occ::handle<Geom_BezierSurface> b = x->Bezier(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(occ::handle<Geom_BSplineSurface> b = x->BSpline(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(A1(x->AxeOfRevolution());)
      ATT(D3(x->Direction());)
      ATT(prCurveHandle(x->BasisCurve());)
      ATT(prSurfHandle(x->BasisSurface());)
      ATT(F(x->OffsetValue());))
  }
  // -- resolution
  for (int j = 0; j < 56; ++j) {
    int s = j % NGS, r = (j / NGS) % 4;
    static const double R3[6] = {1.0e-3, 5.0, 1.0e-7, 0.5, 2.0, 100.0};
    double rr = R3[(j / 3) % 6];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gas_resolution"); I(s); F(u1); F(u2); F(v1); F(v2); F(rr); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
      ATT(F(x->UResolution(rr));)
      ATT(F(x->VResolution(rr));))
  }
  // -- UTrim, VTrim, ShallowCopy
  for (int j = 0; j < 42; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, tl = (j / 7) % 4;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double a1 = u1 + 0.2 * (u2 - u1), a2 = u1 + 0.8 * (u2 - u1), b1 = v1 + 0.3 * (v2 - v1), b2 = v1 + 0.7 * (v2 - v1);
    begin("gas_trim"); I(s); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv); F(a1); F(a2); F(b1); F(b2); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2, tu, tv);
      occ::handle<Adaptor3d_Surface> t = x->UTrim(a1, a2, 1.0e-3);
      prSurfBasics(*t);
      ATT(P3(t->Value(0.5 * (a1 + a2), 0.5 * (v1 + v2)));)
      occ::handle<Adaptor3d_Surface> w = x->VTrim(b1, b2, 2.0e-3);
      prSurfBasics(*w);
      ATT(P3(w->Value(0.5 * (u1 + u2), 0.5 * (b1 + b2)));)
      occ::handle<Adaptor3d_Surface> cp = x->ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(0.5 * (u1 + u2), 0.5 * (v1 + v2)));)
      ATT(I(cp->NbUIntervals(GeomAbs_C1)); I(cp->NbVIntervals(GeomAbs_C1));))
  }
}

// ---- the transformations -------------------------------------------------------------------------------------------------
struct TS {
  int kind = 0;  // 0 identity, 1 translation, 2 rotation, 3 scale, 4 point mirror, 5 axis mirror, 6 plane mirror
  double p[3] = {0, 0, 0}, a[3] = {0, 0, 1}, v = 0;
};
static const int NTS = 7;
static TS TSD[NTS];
static void makeTrsfSpecs() {
  TSD[0].kind = 0;
  TSD[1].kind = 1; TSD[1].a[0] = 0.3; TSD[1].a[1] = -0.2; TSD[1].a[2] = 0.5;
  TSD[2].kind = 2; TSD[2].p[0] = 0.2; TSD[2].p[1] = -0.1; TSD[2].p[2] = 0.3; TSD[2].a[0] = 0.3; TSD[2].a[1] = 0.2; TSD[2].a[2] = 1.0; TSD[2].v = 0.7;
  TSD[3].kind = 3; TSD[3].p[0] = 0.1; TSD[3].p[1] = 0.4; TSD[3].p[2] = -0.2; TSD[3].v = 1.5;
  TSD[4].kind = 4; TSD[4].p[0] = 0.5; TSD[4].p[1] = 0.1; TSD[4].p[2] = 0.2;
  TSD[5].kind = 5; TSD[5].p[0] = 0.0; TSD[5].p[1] = 0.3; TSD[5].p[2] = 0.1; TSD[5].a[0] = 1.0; TSD[5].a[1] = 0.2; TSD[5].a[2] = 0.0;
  TSD[6].kind = 6; TSD[6].p[0] = 0.2; TSD[6].p[1] = 0.2; TSD[6].p[2] = 0.0; TSD[6].a[0] = 0.1; TSD[6].a[1] = 0.1; TSD[6].a[2] = 1.0;
}
static gp_Trsf trsfOf(const TS& t) {
  gp_Trsf r;
  gp_Pnt p(t.p[0], t.p[1], t.p[2]);
  switch (t.kind) {
    case 1: r.SetTranslation(gp_Vec(t.a[0], t.a[1], t.a[2])); break;
    case 2: r.SetRotation(gp_Ax1(p, gp_Dir(t.a[0], t.a[1], t.a[2])), t.v); break;
    case 3: r.SetScale(p, t.v); break;
    case 4: r.SetMirror(p); break;
    case 5: r.SetMirror(gp_Ax1(p, gp_Dir(t.a[0], t.a[1], t.a[2]))); break;
    case 6: r.SetMirror(gp_Ax2(p, gp_Dir(t.a[0], t.a[1], t.a[2]))); break;
    default: break;
  }
  return r;
}
static void prTS(const TS& t) {
  I(t.kind);
  for (double v : t.p) F(v);
  for (double v : t.a) F(v);
  F(t.v);
}

// ---- the 2D curves of the curves on surfaces -----------------------------------------------------------------------------
struct G2 {
  int kind;  // 0 line, 1 circle
  double p[2], d[2], r;
  double f, l;  // the range of the adaptor
};
static const int NG2 = 3;
static const G2 G2D[NG2] = {{0, {0.2, 0.1}, {1.0, 0.5}, 0.0, 0.0, 1.5},
                            {1, {1.2, 1.0}, {1.0, 0.0}, 0.6, 0.0, 5.5},
                            {0, {0.3, -1.0}, {0.2, 1.0}, 0.0, 0.0, 2.0}};
static occ::handle<Geom2d_Curve> C2[NG2];
static void prG2(const G2& g) {
  I(g.kind); F(g.p[0]); F(g.p[1]); F(g.d[0]); F(g.d[1]); F(g.r); F(g.f); F(g.l);
}
static const int NCP = 5;
static const int COSP[NCP][2] = {{0, 7}, {1, 7}, {0, 0}, {2, 1}, {1, 4}};
static occ::handle<Adaptor3d_CurveOnSurface> makeCos(int k) {
  int g = COSP[k][0], s = COSP[k][1];
  occ::handle<Geom2dAdaptor_Curve> c2 = new Geom2dAdaptor_Curve(C2[g], G2D[g].f, G2D[g].l);
  double u1, u2, v1, v2;
  surfRange(s, 0, u1, u2, v1, v2);
  occ::handle<GeomAdaptor_Surface> sa = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
  return new Adaptor3d_CurveOnSurface(c2, sa);
}

// ---- the class of a Geom object ---------------------------------------------------------------------------------------------
static int curveTag(const occ::handle<Geom_Curve>& c) {
  if (c.IsNull()) return -1;
  if (!occ::down_cast<Geom_Line>(c).IsNull()) return 0;
  if (!occ::down_cast<Geom_Circle>(c).IsNull()) return 1;
  if (!occ::down_cast<Geom_Ellipse>(c).IsNull()) return 2;
  if (!occ::down_cast<Geom_Parabola>(c).IsNull()) return 3;
  if (!occ::down_cast<Geom_Hyperbola>(c).IsNull()) return 4;
  if (!occ::down_cast<Geom_BezierCurve>(c).IsNull()) return 5;
  if (!occ::down_cast<Geom_BSplineCurve>(c).IsNull()) return 6;
  if (!occ::down_cast<Geom_TrimmedCurve>(c).IsNull()) return 7;
  if (!occ::down_cast<Geom_OffsetCurve>(c).IsNull()) return 8;
  return 9;
}
static int surfTag(const occ::handle<Geom_Surface>& c) {
  if (c.IsNull()) return -1;
  if (!occ::down_cast<Geom_Plane>(c).IsNull()) return 0;
  if (!occ::down_cast<Geom_CylindricalSurface>(c).IsNull()) return 1;
  if (!occ::down_cast<Geom_ConicalSurface>(c).IsNull()) return 2;
  if (!occ::down_cast<Geom_SphericalSurface>(c).IsNull()) return 3;
  if (!occ::down_cast<Geom_ToroidalSurface>(c).IsNull()) return 4;
  if (!occ::down_cast<Geom_BezierSurface>(c).IsNull()) return 5;
  if (!occ::down_cast<Geom_BSplineSurface>(c).IsNull()) return 6;
  if (!occ::down_cast<Geom_SurfaceOfRevolution>(c).IsNull()) return 7;
  if (!occ::down_cast<Geom_SurfaceOfLinearExtrusion>(c).IsNull()) return 8;
  if (!occ::down_cast<Geom_OffsetSurface>(c).IsNull()) return 9;
  if (!occ::down_cast<Geom_RectangularTrimmedSurface>(c).IsNull()) return 10;
  return 11;
}

static void transformedFamilies() {
  // -- the 2D curves
  for (int i = 0; i < NG2; ++i) {
    begin("def_gcurve2d"); I(i); bar();
    CASE(prG2(G2D[i]); F(C2[i]->FirstParameter()); F(C2[i]->LastParameter()); P2(C2[i]->Value(0.5 * (G2D[i].f + G2D[i].l)));)
  }
  // -- GeomAdaptor_TransformedCurve: evaluation and the common data
  for (int j = 0; j < 50; ++j) {
    int cs = j % 4 == 3 ? (j / 4) % NCP : -1;
    int c = j % NGC, r = (j / NGC + j / 4) % 4, tk = (j / 2 + j / 9) % NTS;
    double f, l;
    bool full = false;
    if (cs >= 0) { f = G2D[COSP[cs][0]].f; l = G2D[COSP[cs][0]].l; }
    else { curveRange(c, r, f, l); full = curveFull(c, r); }
    int pat = (j / 3 + j) % 9;
    double t[2] = {f + TFR[pat][0] * (l - f), f + TFR[pat][1] * (l - f)};
    begin("gtc_eval"); prTS(TSD[tk]); I(cs); I(cs >= 0 ? COSP[cs][0] : -1); I(cs >= 0 ? COSP[cs][1] : -1); I(cs >= 0 ? -1 : c); I(full); F(f); F(l); F(t[0]); F(t[1]); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedCurve> a;
      if (cs >= 0) { a = new GeomAdaptor_TransformedCurve(); a->SetTrsf(tr); a->LoadCurveOnSurface(makeCos(cs)); }
      else if (full) a = new GeomAdaptor_TransformedCurve(CC[c], tr);
      else a = new GeomAdaptor_TransformedCurve(CC[c], f, l, tr);
      prCurveBasics(*a);
      B(a->Is3DCurve());
      B(a->IsCurveOnSurface());
      B(a->IsClosed());
      B(a->IsPeriodic());
      ATT(F(a->Period());)
      ATT(I((int)a->Continuity());)
      ATT(I(a->NbIntervals(GeomAbs_C1));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); a->Intervals(T, GeomAbs_C1); for (int i = 1; i <= 8; ++i) F(T(i));)
      ATT(F(a->Resolution(1.0e-3));)
      for (double u : t) {
        ATT(P3(a->Value(u));)
        ATT(gp_Pnt p; gp_Vec d1; a->D1(u, p, d1); P3(p); V3(d1);)
        ATT(gp_Pnt p; gp_Vec d1, d2; a->D2(u, p, d1, d2); P3(p); V3(d1); V3(d2);)
        ATT(gp_Pnt p; gp_Vec d1, d2, d3; a->D3(u, p, d1, d2, d3); P3(p); V3(d1); V3(d2); V3(d3);)
        for (int n = 1; n <= 3; ++n) ATT(V3(a->DN(u, n));)
      })
  }
  // -- GeomAdaptor_TransformedCurve: the typed accessors
  for (int j = 0; j < 63; ++j) {
    int cs = j % 5 == 4 ? (j / 5) % NCP : -1;
    int c = j % NGC, r = (j / NGC) % 4, tk = (j / 3 + j / 11) % NTS;
    double f, l;
    bool full = false;
    if (cs >= 0) { f = G2D[COSP[cs][0]].f; l = G2D[COSP[cs][0]].l; }
    else { curveRange(c, r, f, l); full = curveFull(c, r); }
    begin("gtc_info"); I(j); prTS(TSD[tk]); I(cs); I(cs >= 0 ? COSP[cs][0] : -1); I(cs >= 0 ? COSP[cs][1] : -1); I(cs >= 0 ? -1 : c); I(full); F(f); F(l); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedCurve> a;
      if (cs >= 0) { a = new GeomAdaptor_TransformedCurve(); a->SetTrsf(tr); a->LoadCurveOnSurface(makeCos(cs)); }
      else if (full) a = new GeomAdaptor_TransformedCurve(CC[c], tr);
      else a = new GeomAdaptor_TransformedCurve(CC[c], f, l, tr);
      prCurveBasics(*a);
      ATT(PrLin(a->Line());)
      ATT(PrCirc(a->Circle());)
      ATT(PrElips(a->Ellipse());)
      ATT(PrHypr(a->Hyperbola());)
      ATT(PrParab(a->Parabola());)
      ATT(I(a->Degree());)
      ATT(B(a->IsRational());)
      ATT(I(a->NbPoles());)
      ATT(I(a->NbKnots());)
      if (cs < 0) {
        ATT(occ::handle<Geom_BezierCurve> b = a->Bezier(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)););
        ATT(occ::handle<Geom_BSplineCurve> b = a->BSpline(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)); F(b->Knot(1)););
      }
      ATT(occ::handle<Geom_OffsetCurve> b = a->OffsetCurve(); B(b.IsNull()); F(b->Offset()); D3(b->Direction());)
      B(a->Curve().Curve().IsNull());
      B(a->GeomCurve().IsNull());)
  }
  // -- GeomAdaptor_TransformedCurve: copies, Trim, Load
  for (int j = 0; j < 40; ++j) {
    int cs = j % 4 == 2 ? (j / 4) % NCP : -1;
    int c = j % NGC, r = (j / NGC) % 4, tk = (j / 2) % NTS;
    double f, l;
    bool full = false;
    if (cs >= 0) { f = G2D[COSP[cs][0]].f; l = G2D[COSP[cs][0]].l; }
    else { curveRange(c, r, f, l); full = curveFull(c, r); }
    double t1 = f + 0.2 * (l - f), t2 = f + 0.7 * (l - f);
    begin("gtc_copy"); prTS(TSD[tk]); I(cs); I(cs >= 0 ? COSP[cs][0] : -1); I(cs >= 0 ? COSP[cs][1] : -1); I(c); I(full); F(f); F(l); F(t1); F(t2); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedCurve> a;
      if (cs >= 0) { a = new GeomAdaptor_TransformedCurve(); a->SetTrsf(tr); a->LoadCurveOnSurface(makeCos(cs)); }
      else if (full) a = new GeomAdaptor_TransformedCurve(CC[c], tr);
      else a = new GeomAdaptor_TransformedCurve(CC[c], f, l, tr);
      occ::handle<Adaptor3d_Curve> cp = a->ShallowCopy();
      prCurveBasics(*cp);
      ATT(prD0(*cp, 0.5 * (f + l));)
      occ::handle<Adaptor3d_Curve> tc = a->Trim(t1, t2, 1.0e-3);
      prCurveBasics(*tc);
      ATT(prD0(*tc, 0.5 * (t1 + t2));)
      ATT(F(occ::down_cast<GeomAdaptor_TransformedCurve>(tc)->Trsf().ScaleFactor());)
      // Load on a fresh object
      GeomAdaptor_TransformedCurve b;
      b.Load(CC[(c + 2) % NGC]);
      b.SetTrsf(tr);
      prCurveBasics(b);
      ATT(double uu = (Precision::IsInfinite(b.FirstParameter()) || Precision::IsInfinite(b.LastParameter())) ? 0.25 : 0.5 * (b.FirstParameter() + b.LastParameter()); prD0(b, uu);)
      if (!Precision::IsInfinite(CC[(c + 3) % NGC]->FirstParameter()) && !Precision::IsInfinite(CC[(c + 3) % NGC]->LastParameter())) {
        GeomAdaptor_TransformedCurve d;
        d.Load(CC[(c + 3) % NGC], CC[(c + 3) % NGC]->FirstParameter(), CC[(c + 3) % NGC]->LastParameter());
        prCurveBasics(d);
      })
  }
  // -- GeomAdaptor_TransformedSurface: evaluation and the common data
  for (int j = 0; j < 44; ++j) {
    int s = j % NGS, r = (j / NGS + j / 11) % 4, tk = (j / 3 + j / 7) % NTS, tl = (j / 5) % 4;
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double pt[2][2];
    pt[0][0] = u1 + SFR[p1][0] * (u2 - u1); pt[0][1] = v1 + SFR[p1][1] * (v2 - v1);
    pt[1][0] = u1 + SFR[p2][0] * (u2 - u1); pt[1][1] = v1 + SFR[p2][1] * (v2 - v1);
    begin("gts_eval"); prTS(TSD[tk]); I(s); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedSurface> x = new GeomAdaptor_TransformedSurface(SC[s], u1, u2, v1, v2, tr, tu, tv);
      prSurfFull(*x);
      F(x->ToleranceU()); F(x->ToleranceV());
      B(x->HasTrsf());
      for (auto& q : pt) {
        double u = q[0], v = q[1];
        ATT(P3(x->Value(u, v));)
        ATT(gp_Pnt p; gp_Vec a, b; x->D1(u, v, p, a, b); P3(p); V3(a); V3(b);)
        ATT(gp_Pnt p; gp_Vec a, b, c, d, e; x->D2(u, v, p, a, b, c, d, e); P3(p); V3(a); V3(b); V3(c); V3(d); V3(e);)
        ATT(gp_Pnt p; gp_Vec a, b, c, d, e, f, g, h, k; x->D3(u, v, p, a, b, c, d, e, f, g, h, k); P3(p); V3(a); V3(b); V3(c); V3(d);
            V3(e); V3(f); V3(g); V3(h); V3(k);)
        static const int NN[5][2] = {{1, 0}, {0, 1}, {1, 1}, {2, 1}, {0, 2}};
        for (auto& n : NN) ATT(V3(x->DN(u, v, n[0], n[1]));)
      })
  }
  // -- GeomAdaptor_TransformedSurface: the typed accessors
  for (int j = 0; j < 56; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, tk = (j / 4 + j / 13) % NTS;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gts_info"); prTS(TSD[tk]); I(s); F(u1); F(u2); F(v1); F(v2); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedSurface> x = new GeomAdaptor_TransformedSurface(SC[s], u1, u2, v1, v2, tr);
      prSurfBasics(*x);
      ATT(PrPln(x->Plane());)
      ATT(PrCyl(x->Cylinder());)
      ATT(PrCone(x->Cone());)
      ATT(PrSph(x->Sphere());)
      ATT(PrTor(x->Torus());)
      ATT(I(x->UDegree());)
      ATT(I(x->NbUPoles());)
      ATT(I(x->VDegree());)
      ATT(I(x->NbVPoles());)
      ATT(I(x->NbUKnots());)
      ATT(I(x->NbVKnots());)
      ATT(B(x->IsURational());)
      ATT(B(x->IsVRational());)
      ATT(occ::handle<Geom_BezierSurface> b = x->Bezier(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(occ::handle<Geom_BSplineSurface> b = x->BSpline(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(A1(x->AxeOfRevolution());)
      ATT(D3(x->Direction());)
      ATT(prCurveHandle(x->BasisCurve());)
      ATT(prSurfHandle(x->BasisSurface());)
      ATT(F(x->OffsetValue());)
      ATT(I(surfTag(x->GeomSurfaceTransformed())); I((int)x->AdaptorSurfaceTransformed().GetType());)
      I(surfTag(x->GeomSurfaceOriginal()));
      I((int)x->AdaptorSurfaceOriginal().GetType());)
  }
  // -- GeomAdaptor_TransformedSurface: intervals, Trim, copies, Load, SetTrsf
  for (int j = 0; j < 42; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, tk = (j / 3) % NTS, sh = (j / 5) % 7;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double a1 = u1 + 0.2 * (u2 - u1), a2 = u1 + 0.8 * (u2 - u1), b1 = v1 + 0.3 * (v2 - v1), b2 = v1 + 0.7 * (v2 - v1);
    begin("gts_trim"); prTS(TSD[tk]); prTS(TSD[(tk + 2) % NTS]); I(s); F(u1); F(u2); F(v1); F(v2); F(a1); F(a2); F(b1); F(b2); I(sh); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedSurface> x = new GeomAdaptor_TransformedSurface(SC[s], u1, u2, v1, v2, tr);
      ATT(int nb = x->NbUIntervals(SHP[sh]); I(nb); NCollection_Array1<double> T(1, nb + 1); x->UIntervals(T, SHP[sh]);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      ATT(int nb = x->NbVIntervals(SHP[sh]); I(nb); NCollection_Array1<double> T(1, nb + 1); x->VIntervals(T, SHP[sh]);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      occ::handle<Adaptor3d_Surface> t = x->UTrim(a1, a2, 1.0e-3);
      prSurfBasics(*t);
      ATT(P3(t->Value(0.5 * (a1 + a2), 0.5 * (v1 + v2)));)
      occ::handle<Adaptor3d_Surface> w = x->VTrim(b1, b2, 2.0e-3);
      prSurfBasics(*w);
      occ::handle<Adaptor3d_Surface> cp = x->ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(0.5 * (u1 + u2), 0.5 * (v1 + v2)));)
      GeomAdaptor_TransformedSurface y;
      y.Load(SC[(s + 3) % NGS], tr);
      prSurfBasics(y);
      y.SetTrsf(trsfOf(TSD[(tk + 2) % NTS]));
      B(y.HasTrsf());
      ATT(P3(y.Value(0.25, 0.5)););
      GeomAdaptor_TransformedSurface z;
      z.Load(SC[s], u1, u2, v1, v2, tr);
      prSurfBasics(z);
      GeomAdaptor_TransformedSurface z2(SC[s], tr);
      prSurfBasics(z2);
      GeomAdaptor_TransformedSurface z3;
      z3.SetTrsf(tr);
      ATT(PrPln(z3.Plane());)
      B(z3.HasTrsf());
      ATT(F(z.UResolution(1.0e-3)); F(z.VResolution(1.0e-3));))
  }
}

// ---- GeomAdaptor_SurfaceOfRevolution and GeomAdaptor_SurfaceOfLinearExtrusion --------------------------------------------
struct AXS {
  double loc[3], dir[3];
};
static AXS axisOf(int k) {
  AXS a = {{0, 0, 0}, {0, 0, 1}};
  auto set = [&](double lx, double ly, double lz, double dx, double dy, double dz) {
    a.loc[0] = lx; a.loc[1] = ly; a.loc[2] = lz;
    a.dir[0] = dx; a.dir[1] = dy; a.dir[2] = dz;
  };
  switch (k) {
    case 0: set(0.2, 0.1, -0.3, 0.0, 0.0, 1.0); break;
    case 1: set(0.6, 0.4, 0.1, 0.6, 0.0, 0.8); break;
    case 2: set(0.3, 0.2, 0.1, 0.8, 0.0, -0.6); break;
    case 3: set(0.1, -0.2, 0.3, 0.0, 0.0, 1.0); break;
    case 4: {
      gp_Ax1 x = occ::down_cast<Geom_Circle>(CC[1])->XAxis();
      set(x.Location().X(), x.Location().Y(), x.Location().Z(), x.Direction().X(), x.Direction().Y(), x.Direction().Z());
      break;
    }
    case 5: {
      occ::handle<Geom_Circle> c = occ::down_cast<Geom_Circle>(CC[1]);
      gp_Ax1 x = c->XAxis();
      gp_Dir y = c->Position().YDirection();
      set(x.Location().X() + 2.5 * y.X(), x.Location().Y() + 2.5 * y.Y(), x.Location().Z() + 2.5 * y.Z(), x.Direction().X(), x.Direction().Y(),
          x.Direction().Z());
      break;
    }
    case 6: set(1.0, -0.5, 0.3, 0.3, 0.4, 0.2); break;
    case 7: set(-1.0, 0.0, 0.5, 0.1, 1.0, 0.2); break;
    case 9: set(0.6, 0.4, 0.1, -0.6, 0.0, -0.8); break;
    case 10: set(0.6, 0.4, 0.1, 0.6, 3.0e-4, 0.8); break;
    case 11: set(0.3, 0.2, 0.1, 0.8 + 6.0e-9, 0.0, -0.6 + 8.0e-9); break;
    case 12: set(0.4, -0.2, 0.7, 0.0, 0.0, 1.0); break;
    default: set(0.1, -0.2, 0.3, 0.6, 0.0, 0.8); break;  // along the line
  }
  return a;
}
static void prAx(const AXS& a) { for (double v : a.loc) F(v); for (double v : a.dir) F(v); }
static gp_Ax1 ax1Of(const AXS& a) { return gp_Ax1(gp_Pnt(a.loc[0], a.loc[1], a.loc[2]), gp_Dir(a.dir[0], a.dir[1], a.dir[2])); }
// (curve, axis) pairs for the revolution
static const int NRV = 24;
static const int RVC[NRV][2] = {{0, 1}, {0, 2}, {0, 3}, {1, 4}, {1, 5}, {1, 0}, {1, 6}, {7, 0}, {7, 7}, {5, 0},
                                {9, 7}, {2, 0}, {4, 0}, {3, 0}, {0, 0}, {0, 6}, {0, 8}, {14, 6}, {12, 0}, {13, 7},
                                {0, 9}, {0, 10}, {0, 11}, {0, 12}};
static occ::handle<GeomAdaptor_Curve> basisCurve(int c, int variant, double f, double l) {
  if (variant == 1) return new GeomAdaptor_Curve(CC[c]);
  return new GeomAdaptor_Curve(CC[c], f, l);
}
// the directions of the extrusion
static const int NEX = 15;
static const int EXC[NEX][2] = {{0, 0}, {0, 3}, {1, 1}, {1, 2}, {2, 2}, {2, 1}, {4, 2}, {3, 2}, {7, 0}, {5, 0}, {9, 2}, {1, 0}, {13, 0}, {12, 1}, {1, 4}};
static void dirOf(int c, int k, double d[3]) {
  d[0] = 0.3; d[1] = -0.2; d[2] = 0.9;
  occ::handle<Geom_Conic> cn = occ::down_cast<Geom_Conic>(CC[c]);
  if (k == 3) { d[0] = 0.6; d[1] = 0.0; d[2] = 0.8; }
  else if (k == 4 && !cn.IsNull()) { d[0] = -cn->Position().Direction().X(); d[1] = -cn->Position().Direction().Y(); d[2] = -cn->Position().Direction().Z(); }
  else if (k == 1 && !cn.IsNull()) { d[0] = cn->Position().Direction().X(); d[1] = cn->Position().Direction().Y(); d[2] = cn->Position().Direction().Z(); }
  else if (k == 2 && !cn.IsNull()) { d[0] = cn->Position().XDirection().X(); d[1] = cn->Position().XDirection().Y(); d[2] = cn->Position().XDirection().Z(); }
}

static void prRevInfo(const GeomAdaptor_SurfaceOfRevolution& x) {
  prSurfFull(x);
  ATT(A1(x.AxeOfRevolution());)
  ATT(A3(x.Axis());)
  ATT(prCurveHandle(x.BasisCurve());)
}
static void prExtInfo(const GeomAdaptor_SurfaceOfLinearExtrusion& x) {
  prSurfFull(x);
  ATT(D3(x.Direction());)
  ATT(prCurveHandle(x.BasisCurve());)
}
template <class S>
static void prTypedInfo(const S& x) {
  ATT(PrPln(x.Plane());)
  ATT(PrCyl(x.Cylinder());)
  ATT(PrCone(x.Cone());)
  ATT(PrSph(x.Sphere());)
  ATT(PrTor(x.Torus());)
  ATT(I(x.UDegree());)
  ATT(I(x.NbUPoles());)
  ATT(I(x.VDegree());)
  ATT(I(x.NbVPoles());)
  ATT(I(x.NbUKnots());)
  ATT(I(x.NbVKnots());)
  ATT(B(x.IsURational());)
  ATT(B(x.IsVRational());)
  ATT(occ::handle<Geom_BezierSurface> b = x.Bezier(); B(b.IsNull()););
  ATT(occ::handle<Geom_BSplineSurface> b = x.BSpline(); B(b.IsNull()););
  ATT(A1(x.AxeOfRevolution());)
  ATT(D3(x.Direction());)
  ATT(prSurfHandle(x.BasisSurface());)
  ATT(F(x.OffsetValue());)
}
template <class S>
static void prEval(const S& x, double pt[2][2]) {
  for (int k = 0; k < 2; ++k) {
    double u = pt[k][0], v = pt[k][1];
    ATT(P3(x.Value(u, v));)
    ATT(gp_Pnt p; gp_Vec a, b; x.D1(u, v, p, a, b); P3(p); V3(a); V3(b);)
    ATT(gp_Pnt p; gp_Vec a, b, c, d, e; x.D2(u, v, p, a, b, c, d, e); P3(p); V3(a); V3(b); V3(c); V3(d); V3(e);)
    ATT(gp_Pnt p; gp_Vec a, b, c, d, e, f, g, h, kk; x.D3(u, v, p, a, b, c, d, e, f, g, h, kk); P3(p); V3(a); V3(b); V3(c); V3(d);
        V3(e); V3(f); V3(g); V3(h); V3(kk);)
    static const int NN[6][2] = {{1, 0}, {0, 1}, {1, 1}, {2, 1}, {0, 2}, {0, 0}};
    for (auto& n : NN) ATT(V3(x.DN(u, v, n[0], n[1]));)
  }
}

static void sweptFamilies() {
  // -- the revolution adaptor
  for (int j = 0; j < 60; ++j) {
    int pair = j % NRV, r = (j / NRV + j / 3) % 4;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    begin("gsr_new"); I(c); I(full); F(f); F(l); prAx(ax); I(j % 3); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      int v = j % 3;
      occ::handle<GeomAdaptor_SurfaceOfRevolution> x;
      if (v == 0) x = new GeomAdaptor_SurfaceOfRevolution(hc, ax1Of(ax));
      else if (v == 1) { x = new GeomAdaptor_SurfaceOfRevolution(); x->Load(hc); x->Load(ax1Of(ax)); }
      else { x = new GeomAdaptor_SurfaceOfRevolution(hc); x->Load(ax1Of(ax)); }
      prRevInfo(*x);
      ATT(int nb = x->NbVIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->VIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      ATT(int nb = x->NbUIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->UIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));))
  }
  for (int j = 0; j < 48; ++j) {
    int pair = (j + j / NRV) % NRV, r = (j / NRV + j / 5) % 4;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double pt[2][2];
    pt[0][0] = 6.283185307179586 * SFR[p1][0]; pt[0][1] = f + SFR[p1][1] * (l - f);
    pt[1][0] = 6.283185307179586 * SFR[p2][0]; pt[1][1] = f + SFR[p2][1] * (l - f);
    begin("gsr_eval"); I(c); I(full); F(f); F(l); prAx(ax);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfRevolution x(hc, ax1Of(ax));
      prEval(x, pt);)
  }
  for (int j = 0; j < NRV * 3; ++j) {
    int pair = j % NRV, r = j / NRV;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    begin("gsr_info"); I(c); I(full); F(f); F(l); prAx(ax); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfRevolution x(hc, ax1Of(ax));
      prSurfBasics(x);
      prTypedInfo(x);)
  }
  for (int j = 0; j < 40; ++j) {
    int pair = (j * 7 + j / NRV) % NRV, r = (j / NRV + 1) % 4;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    double t1 = f + 0.2 * (l - f), t2 = f + 0.75 * (l - f);
    int c2 = (c + 1) % NGC;
    double f2, l2;
    curveRange(c2, 0, f2, l2);
    AXS ax2 = axisOf((k + 1) % 8);
    begin("gsr_trim"); I(c); I(full); F(f); F(l); prAx(ax); F(t1); F(t2); I(c2); F(f2); F(l2); prAx(ax2); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfRevolution x(hc, ax1Of(ax));
      occ::handle<Adaptor3d_Surface> u = x.UTrim(0.0, 2.0 * M_PI, 1.0e-3);
      prSurfBasics(*u);
      occ::handle<Adaptor3d_Surface> w = x.VTrim(t1, t2, 1.0e-3);
      prSurfBasics(*w);
      ATT(P3(w->Value(1.0, 0.5 * (t1 + t2)));)
      occ::handle<Adaptor3d_Surface> cp = x.ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(2.0, 0.5 * (f + l)));)
      GeomAdaptor_SurfaceOfRevolution y;
      y.Load(hc);
      y.Load(ax1Of(ax2));
      prSurfBasics(y);
      y.Load(basisCurve(c2, 0, f2, l2));
      prSurfBasics(y);)
  }
  // -- the extrusion adaptor
  for (int j = 0; j < 56; ++j) {
    int pair = j % NEX, r = (j / NEX + j / 3) % 4;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double d[3];
    dirOf(c, k, d);
    begin("gse_new"); I(j); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]); I(j % 3); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      int v = j % 3;
      gp_Dir dir(d[0], d[1], d[2]);
      occ::handle<GeomAdaptor_SurfaceOfLinearExtrusion> x;
      if (v == 0) x = new GeomAdaptor_SurfaceOfLinearExtrusion(hc, dir);
      else if (v == 1) { x = new GeomAdaptor_SurfaceOfLinearExtrusion(); x->Load(hc); x->Load(dir); }
      else { x = new GeomAdaptor_SurfaceOfLinearExtrusion(hc); x->Load(dir); }
      prExtInfo(*x);
      ATT(int nb = x->NbUIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->UIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      ATT(int nb = x->NbVIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->VIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));))
  }
  for (int j = 0; j < 44; ++j) {
    int pair = (j + j / NEX) % NEX, r = (j / NEX + j / 5) % 4;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double d[3];
    dirOf(c, k, d);
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double pt[2][2];
    pt[0][0] = f + SFR[p1][0] * (l - f); pt[0][1] = -1.0 + 2.5 * SFR[p1][1];
    pt[1][0] = f + SFR[p2][0] * (l - f); pt[1][1] = -1.0 + 2.5 * SFR[p2][1];
    begin("gse_eval"); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfLinearExtrusion x(hc, gp_Dir(d[0], d[1], d[2]));
      prEval(x, pt);)
  }
  for (int j = 0; j < NEX * 4; ++j) {
    int pair = j % NEX, r = j / NEX;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r) || (r % 2 == 1 && (c == 0 || c == 3 || c == 4));
    double d[3];
    dirOf(c, k, d);
    begin("gse_info"); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfLinearExtrusion x(hc, gp_Dir(d[0], d[1], d[2]));
      prSurfBasics(x);
      prTypedInfo(x);)
  }
  for (int j = 0; j < 40; ++j) {
    int pair = (j * 5 + j / NEX) % NEX, r = (j / NEX + 1) % 4;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double d[3];
    dirOf(c, k, d);
    double t1 = f + 0.2 * (l - f), t2 = f + 0.75 * (l - f);
    begin("gse_trim"); I(j); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]); F(t1); F(t2); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfLinearExtrusion x(hc, gp_Dir(d[0], d[1], d[2]));
      occ::handle<Adaptor3d_Surface> u = x.UTrim(t1, t2, 1.0e-3);
      prSurfBasics(*u);
      occ::handle<Adaptor3d_Surface> w = x.VTrim(t1, t2, 1.0e-3);
      prSurfBasics(*w);
      ATT(P3(w->Value(0.5 * (t1 + t2), 0.5));)
      occ::handle<Adaptor3d_Surface> cp = x.ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(0.5 * (f + l), 0.25));)
      GeomAdaptor_SurfaceOfLinearExtrusion y;
      y.Load(hc);
      y.Load(gp_Dir(0.0, 0.3, 1.0));
      prSurfBasics(y);
      y.Load(hc);
      prSurfBasics(y);)
  }
}

// ---- GeomAdaptor::MakeCurve, GeomAdaptor::MakeSurface --------------------------------------------------------------------
static void makeFamilies() {
  for (int j = 0; j < 63; ++j) {
    int c = j % NGC, r = (j / NGC) % 4, v = (j / 3 + j / NGC) % 3;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("make_curve"); I(c); I(v); I(full); F(f); F(l); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      occ::handle<Geom_Curve> g;
      if (v == 0) g = GeomAdaptor::MakeCurve(*a);
      else if (v == 1) {
        GeomAdaptor_TransformedCurve t(CC[c], f, l, gp_Trsf());
        g = GeomAdaptor::MakeCurve(t);
      } else {
        occ::handle<Adaptor3d_Curve> t = a->Trim(f + 0.1 * (l - f), l - 0.2 * (l - f), 0.0);
        g = GeomAdaptor::MakeCurve(*t);
      }
      I(curveTag(g));
      F(g->FirstParameter());
      F(g->LastParameter());
      occ::handle<Geom_TrimmedCurve> tc = occ::down_cast<Geom_TrimmedCurve>(g);
      if (!tc.IsNull()) I(curveTag(tc->BasisCurve()));
      double m = f + 0.5 * (l - f);
      P3(g->EvalD0(m));)
  }
  for (int j = 0; j < 84; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, v = (j / 3) % 4, trim = (j / 5) % 2;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    int cc = s % NGC;
    double ff, ll, dd[3];
    curveRange(cc, 1, ff, ll);
    dirOf(cc, s % 4, dd);
    AXS axm = axisOf(s % 9);
    begin("make_surface"); I(s); I(v); I(trim); F(u1); F(u2); F(v1); F(v2); I(cc); F(ff); F(ll); F(dd[0]); F(dd[1]); F(dd[2]); prAx(axm); bar();
    CASE(
      occ::handle<Geom_Surface> g;
      if (v == 0) {
        GeomAdaptor_Surface a(SC[s], u1, u2, v1, v2);
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      } else if (v == 1) {
        GeomAdaptor_TransformedSurface a(SC[s], u1, u2, v1, v2, gp_Trsf());
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      } else if (v == 2) {
        GeomAdaptor_SurfaceOfRevolution a(new GeomAdaptor_Curve(CC[cc], ff, ll), ax1Of(axm));
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      } else {
        GeomAdaptor_SurfaceOfLinearExtrusion a(new GeomAdaptor_Curve(CC[cc], ff, ll), gp_Dir(dd[0], dd[1], dd[2]));
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      }
      I(surfTag(g));
      double a1, a2, b1, b2;
      g->Bounds(a1, a2, b1, b2);
      F(a1); F(a2); F(b1); F(b2);
      occ::handle<Geom_RectangularTrimmedSurface> tc = occ::down_cast<Geom_RectangularTrimmedSurface>(g);
      if (!tc.IsNull()) I(surfTag(tc->BasisSurface()));
      ATT(P3(g->EvalD0(u1 + 0.4 * (u2 - u1), v1 + 0.3 * (v2 - v1)));))
  }
}

int main() {
  makeCurveSpecs();
  for (int i = 0; i < NGC; ++i) CC[i] = buildCurve(i);
  makeSurfaceSpecs();
  for (int i = 0; i < NGS; ++i) SC[i] = buildSurface(i);
  makeTrsfSpecs();
  C2[0] = new Geom2d_Line(gp_Pnt2d(G2D[0].p[0], G2D[0].p[1]), gp_Dir2d(G2D[0].d[0], G2D[0].d[1]));
  C2[1] = new Geom2d_Circle(gp_Ax2d(gp_Pnt2d(G2D[1].p[0], G2D[1].p[1]), gp_Dir2d(G2D[1].d[0], G2D[1].d[1])), G2D[1].r);
  C2[2] = new Geom2d_Line(gp_Pnt2d(G2D[2].p[0], G2D[2].p[1]), gp_Dir2d(G2D[2].d[0], G2D[2].d[1]));
  curveFamilies();
  surfaceFamilies();
  transformedFamilies();
  sweptFamilies();
  makeFamilies();
  return 0;
}
