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
