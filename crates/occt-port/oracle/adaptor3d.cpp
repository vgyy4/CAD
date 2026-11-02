// Deterministic reference records for OCCT 8.0.1's package Adaptor3d (Adaptor3d_Curve, _Surface, _IsoCurve, _CurveOnSurface,
// _TopolTool, _HSurfaceTool, _HVertex, _InterFunc), see docs/api-adaptor3d.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values (call hashes) decimal.
// The adaptors the classes work on are a closed set of test adaptors ("TC3" for Adaptor3d_Curve, "TS" for Adaptor3d_Surface,
// "TC" for Adaptor2d_Curve2d) that fill in every virtual method from printed data and fold every call (method id and argument
// bits) into a counter and a 64-bit hash, so the sequence of calls a class makes is part of the behaviour. Each is printed
// once, by the families `def_curve3d`, `def_surface`, `def_curve2d` (in this order, indexed from 0); every other record names
// the adaptors it uses by their index and works on fresh copies (counters at zero).
#include <Adaptor2d_Curve2d.hxx>
#include <Adaptor2d_Line2d.hxx>
#include <Adaptor3d_Curve.hxx>
#include <Adaptor3d_CurveOnSurface.hxx>
#include <Adaptor3d_HSurfaceTool.hxx>
#include <Adaptor3d_HVertex.hxx>
#include <Adaptor3d_InterFunc.hxx>
#include <Adaptor3d_IsoCurve.hxx>
#include <Adaptor3d_Surface.hxx>
#include <Adaptor3d_TopolTool.hxx>
#include <Geom2d_BSplineCurve.hxx>
#include <Geom2d_BezierCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BezierSurface.hxx>
#include <Geom_Line.hxx>
#include <Geom_OffsetCurve.hxx>
#include <GeomAbs_CurveType.hxx>
#include <GeomAbs_IsoType.hxx>
#include <GeomAbs_Shape.hxx>
#include <GeomAbs_SurfaceType.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <Standard_Failure.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopAbs_State.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Lin.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Parab.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Vec.hxx>
#include <gp_Vec2d.hxx>
#include <cfloat>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <iostream>
#include <string>
#include <vector>

// ---- output -------------------------------------------------------------------------------------
static std::string line;
static bool inputs = true;
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
static void U(unsigned long long v) { line += ' ' + std::to_string(v); }
static void B(bool v) { I(v ? 1 : 0); }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void begin(const char* name) {
  line = name;
  inputs = true;
}
static void bar() {
  inputs = false;
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

static uint64_t S0 = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S0 ^= S0 << 13;
  S0 ^= S0 >> 7;
  S0 ^= S0 << 17;
  return (double)(S0 >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)
static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;

// ---- printing of gp values ---------------------------------------------------------------------------------------------
static void P2(const gp_Pnt2d& p) { F(p.X()); F(p.Y()); }
static void V2(const gp_Vec2d& v) { F(v.X()); F(v.Y()); }
static void D2(const gp_Dir2d& d) { F(d.X()); F(d.Y()); }
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
static void PrLin2(const gp_Lin2d& l) { P2(l.Location()); D2(l.Direction()); }

// ---- the 2D test adaptor (as in the Adaptor2d judge) -----------------------------------------------------------------------
struct TC : public Adaptor2d_Curve2d {
  double u0 = 0, u1 = 1;
  int ctype = 0, cont = 0, nint = 1;
  double brk[5] = {0, 1, 0, 0, 0};
  bool closed = false, periodic = false;
  double period = 0;
  double p0[2] = {0, 0}, a[2] = {1, 0}, b[2] = {0, 0}, c[2] = {0, 0}, d[2] = {0, 0};
  double cx = 0, cy = 0, dx = 1, dy = 0;
  bool sense = true;
  double r1 = 1, r2 = 1;
  int degree = 3, nbpoles = 4, nbknots = 2;
  bool rational = false, hasBezier = false;
  bool hasBsp2 = false;
  int bsp2Deg = 1;
  bool bsp2Per = false;
  std::vector<double> bsp2K, bsp2P, bsp2W;
  std::vector<int> bsp2M;
  mutable int calls = 0;
  mutable uint64_t hash = H0;
  void touch(int id, double x = 0.0, double y = 0.0) const {
    ++calls;
    hash = mixh(hash, (uint64_t)id);
    hash = mixh(hash, bitsOf(x));
    hash = mixh(hash, bitsOf(y));
  }
  occ::handle<Adaptor2d_Curve2d> ShallowCopy() const override {
    touch(26);
    occ::handle<TC> cp = new TC(*this);
    cp->hash = mixh(cp->hash, 0xC0FFEEull);
    return cp;
  }
  double FirstParameter() const override { touch(1); return u0; }
  double LastParameter() const override { touch(2); return u1; }
  GeomAbs_Shape Continuity() const override { touch(3); return (GeomAbs_Shape)cont; }
  int NbIntervals(const GeomAbs_Shape S) const override { touch(4, (double)(int)S); return nint; }
  void Intervals(NCollection_Array1<double>& T, const GeomAbs_Shape S) const override {
    touch(5, (double)(int)S);
    for (int k = 0; k <= nint; ++k) T(T.Lower() + k) = brk[k];
  }
  occ::handle<Adaptor2d_Curve2d> Trim(const double F0, const double L0, const double Tol) const override {
    touch(32, F0, L0);
    occ::handle<TC> cp = new TC(*this);
    cp->u0 = F0;
    cp->u1 = L0;
    cp->hash = mixh(cp->hash, bitsOf(Tol));
    return cp;
  }
  bool IsClosed() const override { touch(6); return closed; }
  bool IsPeriodic() const override { touch(7); return periodic; }
  double Period() const override { touch(8); return period; }
  void poly(double u, gp_Pnt2d& P, gp_Vec2d& V1, gp_Vec2d& V2, gp_Vec2d& V3, gp_Vec2d& V4) const {
    double uu = u * u, uuu = uu * u;
    P = gp_Pnt2d(p0[0] + u * a[0] + uu * b[0] + uuu * c[0] + uu * uu * d[0], p0[1] + u * a[1] + uu * b[1] + uuu * c[1] + uu * uu * d[1]);
    V1 = gp_Vec2d(a[0] + 2.0 * u * b[0] + 3.0 * uu * c[0] + 4.0 * uuu * d[0], a[1] + 2.0 * u * b[1] + 3.0 * uu * c[1] + 4.0 * uuu * d[1]);
    V2 = gp_Vec2d(2.0 * b[0] + 6.0 * u * c[0] + 12.0 * uu * d[0], 2.0 * b[1] + 6.0 * u * c[1] + 12.0 * uu * d[1]);
    V3 = gp_Vec2d(6.0 * c[0] + 24.0 * u * d[0], 6.0 * c[1] + 24.0 * u * d[1]);
    V4 = gp_Vec2d(24.0 * d[0], 24.0 * d[1]);
  }
  gp_Pnt2d Value(const double u) const override { touch(9, u); gp_Pnt2d P; gp_Vec2d v1, v2, v3, v4; poly(u, P, v1, v2, v3, v4); return P; }
  void D0(const double u, gp_Pnt2d& P) const override { touch(10, u); gp_Vec2d v1, v2, v3, v4; poly(u, P, v1, v2, v3, v4); }
  void D1(const double u, gp_Pnt2d& P, gp_Vec2d& V1) const override { touch(11, u); gp_Vec2d v2, v3, v4; poly(u, P, V1, v2, v3, v4); }
  void D2(const double u, gp_Pnt2d& P, gp_Vec2d& V1, gp_Vec2d& V2) const override { touch(12, u); gp_Vec2d v3, v4; poly(u, P, V1, V2, v3, v4); }
  void D3(const double u, gp_Pnt2d& P, gp_Vec2d& V1, gp_Vec2d& V2, gp_Vec2d& V3) const override { touch(13, u); gp_Vec2d v4; poly(u, P, V1, V2, V3, v4); }
  gp_Vec2d DN(const double u, const int n) const override {
    touch(14, u, (double)n);
    if (n <= 0) throw Standard_OutOfRange();
    gp_Pnt2d P; gp_Vec2d v[4];
    poly(u, P, v[0], v[1], v[2], v[3]);
    return n <= 4 ? v[n - 1] : gp_Vec2d(0.0, 0.0);
  }
  GeomAbs_CurveType GetType() const override { touch(15); return (GeomAbs_CurveType)ctype; }
  gp_Ax22d axes() const { return gp_Ax22d(gp_Pnt2d(cx, cy), gp_Dir2d(dx, dy), sense); }
  gp_Lin2d Line() const override { touch(16); return gp_Lin2d(gp_Pnt2d(cx, cy), gp_Dir2d(dx, dy)); }
  gp_Circ2d Circle() const override { touch(17); return gp_Circ2d(axes(), r1); }
  gp_Elips2d Ellipse() const override { touch(18); return gp_Elips2d(axes(), r1, r2); }
  gp_Hypr2d Hyperbola() const override { touch(19); return gp_Hypr2d(axes(), r1, r2); }
  gp_Parab2d Parabola() const override { touch(20); return gp_Parab2d(axes(), r1); }
  int Degree() const override { touch(21); return degree; }
  bool IsRational() const override { touch(22); return rational; }
  int NbPoles() const override { touch(23); return nbpoles; }
  int NbKnots() const override { touch(24); return nbknots; }
  occ::handle<Geom2d_BezierCurve> Bezier() const override {
    touch(25);
    if (!hasBezier) return Adaptor2d_Curve2d::Bezier();
    NCollection_Array1<gp_Pnt2d> poles(1, 3);
    poles(1) = gp_Pnt2d(p0[0], p0[1]);
    poles(2) = gp_Pnt2d(p0[0] + a[0], p0[1] + a[1]);
    poles(3) = gp_Pnt2d(p0[0] + b[0], p0[1] + b[1]);
    return new Geom2d_BezierCurve(poles);
  }
  occ::handle<Geom2d_BSplineCurve> BSpline() const override {
    touch(31);
    if (!hasBsp2) return Adaptor2d_Curve2d::BSpline();
    int np = (int)bsp2P.size() / 2, nk = (int)bsp2K.size();
    NCollection_Array1<gp_Pnt2d> poles(1, np);
    for (int i = 0; i < np; ++i) poles(i + 1) = gp_Pnt2d(bsp2P[2 * i], bsp2P[2 * i + 1]);
    NCollection_Array1<double> kn(1, nk);
    NCollection_Array1<int> mu(1, nk);
    for (int i = 0; i < nk; ++i) { kn(i + 1) = bsp2K[i]; mu(i + 1) = bsp2M[i]; }
    if (bsp2W.empty()) return new Geom2d_BSplineCurve(poles, kn, mu, bsp2Deg, bsp2Per);
    NCollection_Array1<double> w(1, np);
    for (int i = 0; i < np; ++i) w(i + 1) = bsp2W[i];
    return new Geom2d_BSplineCurve(poles, w, kn, mu, bsp2Deg, bsp2Per);
  }
};

static void printTC(const TC& t) {
  F(t.u0); F(t.u1); I(t.ctype); I(t.cont); I(t.nint);
  for (int k = 0; k < 5; ++k) F(t.brk[k]);
  B(t.closed); B(t.periodic); F(t.period);
  F(t.p0[0]); F(t.p0[1]); F(t.a[0]); F(t.a[1]); F(t.b[0]); F(t.b[1]); F(t.c[0]); F(t.c[1]); F(t.d[0]); F(t.d[1]);
  F(t.cx); F(t.cy); F(t.dx); F(t.dy); B(t.sense);
  F(t.r1); F(t.r2);
  I(t.degree); B(t.rational); I(t.nbpoles); I(t.nbknots); B(t.hasBezier);
  B(t.hasBsp2);
  if (t.hasBsp2) {
    I(t.bsp2Deg); B(t.bsp2Per);
    I((long long)t.bsp2K.size()); for (double x : t.bsp2K) F(x);
    I((long long)t.bsp2M.size()); for (int m : t.bsp2M) I(m);
    I((long long)t.bsp2P.size()); for (double x : t.bsp2P) F(x);
    I((long long)t.bsp2W.size()); for (double x : t.bsp2W) F(x);
  }
}
static void prTC(const TC& t) { I(t.calls); U(t.hash); }

// ---- the 3D curve test adaptor ("TC3") -----------------------------------------------------------------------------------------
struct TC3 : public Adaptor3d_Curve {
  double u0 = 0, u1 = 1;
  int ctype = 0, cont = 0, nint = 1;
  double brk[5] = {0, 1, 0, 0, 0};
  bool closed = false, periodic = false;
  double period = 0, rf = 1;
  double c[5][3] = {{0, 0, 0}, {1, 0, 0}, {0, 0, 0}, {0, 0, 0}, {0, 0, 0}};
  double org[3] = {0, 0, 0}, zd[3] = {0, 0, 1}, xd[3] = {1, 0, 0}, ld[3] = {1, 0, 0};
  double r1 = 1, r2 = 1;
  int degree = 3, nbpoles = 4, nbknots = 2;
  bool rational = false;
  // the Geom curves handed out by Bezier(), BSpline(), OffsetCurve(): printed as a number of poles and the data
  bool hasBez = false, hasBsp = false, hasOff = false;
  std::vector<double> bezP, bezW;  // 3 per pole / one per pole (empty: not rational)
  int bspDeg = 1;
  bool bspPer = false;
  std::vector<double> bspK, bspP, bspW;
  std::vector<int> bspM;
  double offVal = 0, offDir[3] = {0, 0, 1};
  mutable int calls = 0;
  mutable uint64_t hash = H0;
  void touch(int id, double x = 0.0, double y = 0.0) const {
    ++calls;
    hash = mixh(hash, (uint64_t)id);
    hash = mixh(hash, bitsOf(x));
    hash = mixh(hash, bitsOf(y));
  }
  gp_Ax2 ax() const { return gp_Ax2(gp_Pnt(org[0], org[1], org[2]), gp_Dir(zd[0], zd[1], zd[2]), gp_Dir(xd[0], xd[1], xd[2])); }
  void poly(double u, gp_Pnt& P, gp_Vec* V) const {
    double uu = u * u, uuu = uu * u, u4 = uu * uu;
    double x[3], d1[3], d2[3], d3[3], d4[3];
    for (int k = 0; k < 3; ++k) {
      x[k] = c[0][k] + u * c[1][k] + uu * c[2][k] + uuu * c[3][k] + u4 * c[4][k];
      d1[k] = c[1][k] + 2.0 * u * c[2][k] + 3.0 * uu * c[3][k] + 4.0 * uuu * c[4][k];
      d2[k] = 2.0 * c[2][k] + 6.0 * u * c[3][k] + 12.0 * uu * c[4][k];
      d3[k] = 6.0 * c[3][k] + 24.0 * u * c[4][k];
      d4[k] = 24.0 * c[4][k];
    }
    P = gp_Pnt(x[0], x[1], x[2]);
    if (V) {
      V[0] = gp_Vec(d1[0], d1[1], d1[2]);
      V[1] = gp_Vec(d2[0], d2[1], d2[2]);
      V[2] = gp_Vec(d3[0], d3[1], d3[2]);
      V[3] = gp_Vec(d4[0], d4[1], d4[2]);
    }
  }
  occ::handle<Adaptor3d_Curve> ShallowCopy() const override {
    touch(26);
    occ::handle<TC3> cp = new TC3(*this);
    cp->hash = mixh(cp->hash, 0xC0FFEEull);
    return cp;
  }
  double FirstParameter() const override { touch(1); return u0; }
  double LastParameter() const override { touch(2); return u1; }
  GeomAbs_Shape Continuity() const override { touch(3); return (GeomAbs_Shape)cont; }
  int NbIntervals(const GeomAbs_Shape S) const override { touch(4, (double)(int)S); return nint; }
  void Intervals(NCollection_Array1<double>& T, const GeomAbs_Shape S) const override {
    touch(5, (double)(int)S);
    for (int k = 0; k <= nint; ++k) T(T.Lower() + k) = brk[k];
  }
  occ::handle<Adaptor3d_Curve> Trim(const double F0, const double L0, const double Tol) const override {
    touch(27, F0, L0);
    occ::handle<TC3> cp = new TC3(*this);
    cp->u0 = F0;
    cp->u1 = L0;
    cp->hash = mixh(cp->hash, bitsOf(Tol));
    return cp;
  }
  bool IsClosed() const override { touch(6); return closed; }
  bool IsPeriodic() const override { touch(7); return periodic; }
  double Period() const override { touch(8); return period; }
  double Resolution(const double R3d) const override { touch(28, R3d); return R3d * rf; }
  gp_Pnt EvalD0(const double u) const override { touch(10, u); gp_Pnt P; poly(u, P, nullptr); return P; }
  Geom_Curve::ResD1 EvalD1(const double u) const override { touch(11, u); gp_Pnt P; gp_Vec V[4]; poly(u, P, V); return {P, V[0]}; }
  Geom_Curve::ResD2 EvalD2(const double u) const override { touch(12, u); gp_Pnt P; gp_Vec V[4]; poly(u, P, V); return {P, V[0], V[1]}; }
  Geom_Curve::ResD3 EvalD3(const double u) const override { touch(13, u); gp_Pnt P; gp_Vec V[4]; poly(u, P, V); return {P, V[0], V[1], V[2]}; }
  gp_Vec EvalDN(const double u, const int n) const override {
    touch(14, u, (double)n);
    if (n <= 0) throw Standard_OutOfRange();
    gp_Pnt P; gp_Vec V[4];
    poly(u, P, V);
    return n <= 4 ? V[n - 1] : gp_Vec(0.0, 0.0, 0.0);
  }
  GeomAbs_CurveType GetType() const override { touch(15); return (GeomAbs_CurveType)ctype; }
  gp_Lin Line() const override { touch(16); return gp_Lin(gp_Pnt(org[0], org[1], org[2]), gp_Dir(ld[0], ld[1], ld[2])); }
  gp_Circ Circle() const override { touch(17); return gp_Circ(ax(), r1); }
  gp_Elips Ellipse() const override { touch(18); return gp_Elips(ax(), r1, r2); }
  gp_Hypr Hyperbola() const override { touch(19); return gp_Hypr(ax(), r1, r2); }
  gp_Parab Parabola() const override { touch(20); return gp_Parab(ax(), r1); }
  int Degree() const override { touch(21); return degree; }
  bool IsRational() const override { touch(22); return rational; }
  int NbPoles() const override { touch(23); return nbpoles; }
  int NbKnots() const override { touch(24); return nbknots; }
  occ::handle<Geom_BezierCurve> Bezier() const override {
    touch(25);
    if (!hasBez) return Adaptor3d_Curve::Bezier();
    int n = (int)bezP.size() / 3;
    NCollection_Array1<gp_Pnt> poles(1, n);
    for (int i = 0; i < n; ++i) poles(i + 1) = gp_Pnt(bezP[3 * i], bezP[3 * i + 1], bezP[3 * i + 2]);
    if (bezW.empty()) return new Geom_BezierCurve(poles);
    NCollection_Array1<double> w(1, n);
    for (int i = 0; i < n; ++i) w(i + 1) = bezW[i];
    return new Geom_BezierCurve(poles, w);
  }
  occ::handle<Geom_BSplineCurve> BSpline() const override {
    touch(29);
    if (!hasBsp) return Adaptor3d_Curve::BSpline();
    return makeBsp();
  }
  occ::handle<Geom_BSplineCurve> makeBsp() const {
    int np = (int)bspP.size() / 3, nk = (int)bspK.size();
    NCollection_Array1<gp_Pnt> poles(1, np);
    for (int i = 0; i < np; ++i) poles(i + 1) = gp_Pnt(bspP[3 * i], bspP[3 * i + 1], bspP[3 * i + 2]);
    NCollection_Array1<double> kn(1, nk);
    NCollection_Array1<int> mu(1, nk);
    for (int i = 0; i < nk; ++i) { kn(i + 1) = bspK[i]; mu(i + 1) = bspM[i]; }
    if (bspW.empty()) return new Geom_BSplineCurve(poles, kn, mu, bspDeg, bspPer);
    NCollection_Array1<double> w(1, np);
    for (int i = 0; i < np; ++i) w(i + 1) = bspW[i];
    return new Geom_BSplineCurve(poles, w, kn, mu, bspDeg, bspPer);
  }
  occ::handle<Geom_OffsetCurve> OffsetCurve() const override {
    touch(30);
    if (!hasOff) return Adaptor3d_Curve::OffsetCurve();
    occ::handle<Geom_Curve> l = hasBsp ? occ::handle<Geom_Curve>(makeBsp())
                                       : occ::handle<Geom_Curve>(new Geom_Line(gp_Pnt(org[0], org[1], org[2]), gp_Dir(ld[0], ld[1], ld[2])));
    return new Geom_OffsetCurve(l, offVal, gp_Dir(offDir[0], offDir[1], offDir[2]));
  }
};
static void prVec(const std::vector<double>& v) { I((long long)v.size()); for (double x : v) F(x); }
static void printTC3(const TC3& t) {
  F(t.u0); F(t.u1); I(t.ctype); I(t.cont); I(t.nint);
  for (int k = 0; k < 5; ++k) F(t.brk[k]);
  B(t.closed); B(t.periodic); F(t.period); F(t.rf);
  for (int i = 0; i < 5; ++i) for (int k = 0; k < 3; ++k) F(t.c[i][k]);
  for (int k = 0; k < 3; ++k) F(t.org[k]);
  for (int k = 0; k < 3; ++k) F(t.zd[k]);
  for (int k = 0; k < 3; ++k) F(t.xd[k]);
  for (int k = 0; k < 3; ++k) F(t.ld[k]);
  F(t.r1); F(t.r2);
  I(t.degree); B(t.rational); I(t.nbpoles); I(t.nbknots);
  B(t.hasBez);
  if (t.hasBez) { prVec(t.bezP); prVec(t.bezW); }
  B(t.hasBsp);
  if (t.hasBsp) {
    I(t.bspDeg); B(t.bspPer);
    prVec(t.bspK);
    I((long long)t.bspM.size());
    for (int m : t.bspM) I(m);
    prVec(t.bspP); prVec(t.bspW);
  }
  B(t.hasOff);
  if (t.hasOff) { F(t.offVal); for (int k = 0; k < 3; ++k) F(t.offDir[k]); }
}
static void prTC3(const TC3& t) { I(t.calls); U(t.hash); }

// ---- the surface test adaptor ("TS") -----------------------------------------------------------------------------------------
static const int MI[10] = {0, 1, 0, 2, 1, 0, 3, 2, 1, 0};
static const int MJ[10] = {0, 0, 1, 0, 1, 2, 0, 1, 2, 3};
static double falling(int i, int n) {
  double r = 1.0;
  for (int k = 0; k < n; ++k) r *= (double)(i - k);
  return r;
}
static double powi(double x, int e) {
  double r = 1.0;
  for (int k = 0; k < e; ++k) r *= x;
  return r;
}
struct TS : public Adaptor3d_Surface {
  double u0 = 0, u1 = 1, v0 = 0, v1 = 1;
  int ucont = 0, vcont = 0, nu = 1, nv = 1;
  double ubrk[5] = {0, 1, 0, 0, 0}, vbrk[5] = {0, 1, 0, 0, 0};
  bool uclosed = false, vclosed = false, uperiodic = false, vperiodic = false;
  double uperiod = 0, vperiod = 0, ures = 1, vres = 1;
  int stype = 0;
  double cf[10][3];
  double org[3] = {0, 0, 0}, zd[3] = {0, 0, 1}, xd[3] = {1, 0, 0};
  double r1 = 1, r2 = 1, semi = 0.5;
  double ap[3] = {0, 0, 0}, ad[3] = {0, 0, 1}, ed[3] = {0, 0, 1};
  double offv = 0;
  int udeg = 1, nupoles = 2, vdeg = 1, nvpoles = 2, nuknots = 2, nvknots = 2;
  bool urat = false, vrat = false;
  int basisCurve = -1, basisSurf = -1;  // indices of earlier definitions
  occ::handle<TC3> bc;
  occ::handle<TS> bs;
  bool hasBez = false, hasBsp = false;
  int bezNu = 0, bezNv = 0;
  std::vector<double> bezP, bezW;
  int bspUD = 1, bspVD = 1;
  bool bspUP = false, bspVP = false;
  std::vector<double> bspUK, bspVK, bspP, bspW;
  std::vector<int> bspUM, bspVM;
  int bspNu = 0, bspNv = 0;
  mutable int calls = 0;
  mutable uint64_t hash = H0;
  TS() { for (auto& r : cf) r[0] = r[1] = r[2] = 0.0; }
  void touch(int id, double x = 0.0, double y = 0.0) const {
    ++calls;
    hash = mixh(hash, (uint64_t)id);
    hash = mixh(hash, bitsOf(x));
    hash = mixh(hash, bitsOf(y));
  }
  gp_Vec deriv(int nu_, int nv_, double u, double v) const {
    double x[3] = {0, 0, 0};
    for (int k = 0; k < 10; ++k) {
      if (MI[k] < nu_ || MJ[k] < nv_) continue;
      double w = falling(MI[k], nu_) * powi(u, MI[k] - nu_) * (falling(MJ[k], nv_) * powi(v, MJ[k] - nv_));
      for (int m = 0; m < 3; ++m) x[m] += w * cf[k][m];
    }
    return gp_Vec(x[0], x[1], x[2]);
  }
  gp_Ax3 ax() const { return gp_Ax3(gp_Pnt(org[0], org[1], org[2]), gp_Dir(zd[0], zd[1], zd[2]), gp_Dir(xd[0], xd[1], xd[2])); }
  // a deep copy with the counters at zero (the nested adaptors are copied as well)
  occ::handle<TS> fresh() const {
    occ::handle<TS> cp = new TS(*this);
    cp->calls = 0;
    cp->hash = H0;
    if (!bc.IsNull()) {
      cp->bc = new TC3(*bc);
      cp->bc->calls = 0;
      cp->bc->hash = H0;
    }
    if (!bs.IsNull()) cp->bs = bs->fresh();
    return cp;
  }
  occ::handle<Adaptor3d_Surface> ShallowCopy() const override {
    touch(47);
    occ::handle<TS> cp = new TS(*this);
    cp->hash = mixh(cp->hash, 0xC0FFEEull);
    return cp;
  }
  double FirstUParameter() const override { touch(1); return u0; }
  double LastUParameter() const override { touch(2); return u1; }
  double FirstVParameter() const override { touch(3); return v0; }
  double LastVParameter() const override { touch(4); return v1; }
  GeomAbs_Shape UContinuity() const override { touch(5); return (GeomAbs_Shape)ucont; }
  GeomAbs_Shape VContinuity() const override { touch(6); return (GeomAbs_Shape)vcont; }
  int NbUIntervals(const GeomAbs_Shape S) const override { touch(7, (double)(int)S); return nu; }
  int NbVIntervals(const GeomAbs_Shape S) const override { touch(8, (double)(int)S); return nv; }
  void UIntervals(NCollection_Array1<double>& T, const GeomAbs_Shape S) const override {
    touch(9, (double)(int)S);
    for (int k = 0; k <= nu; ++k) T(T.Lower() + k) = ubrk[k];
  }
  void VIntervals(NCollection_Array1<double>& T, const GeomAbs_Shape S) const override {
    touch(10, (double)(int)S);
    for (int k = 0; k <= nv; ++k) T(T.Lower() + k) = vbrk[k];
  }
  occ::handle<Adaptor3d_Surface> UTrim(const double F0, const double L0, const double Tol) const override {
    touch(11, F0, L0);
    occ::handle<TS> cp = new TS(*this);
    cp->u0 = F0;
    cp->u1 = L0;
    cp->hash = mixh(cp->hash, bitsOf(Tol));
    return cp;
  }
  occ::handle<Adaptor3d_Surface> VTrim(const double F0, const double L0, const double Tol) const override {
    touch(12, F0, L0);
    occ::handle<TS> cp = new TS(*this);
    cp->v0 = F0;
    cp->v1 = L0;
    cp->hash = mixh(cp->hash, bitsOf(Tol));
    return cp;
  }
  bool IsUClosed() const override { touch(13); return uclosed; }
  bool IsVClosed() const override { touch(14); return vclosed; }
  bool IsUPeriodic() const override { touch(15); return uperiodic; }
  double UPeriod() const override { touch(16); return uperiod; }
  bool IsVPeriodic() const override { touch(17); return vperiodic; }
  double VPeriod() const override { touch(18); return vperiod; }
  double UResolution(const double R3d) const override { touch(19, R3d); return R3d * ures; }
  double VResolution(const double R3d) const override { touch(20, R3d); return R3d * vres; }
  GeomAbs_SurfaceType GetType() const override { touch(21); return (GeomAbs_SurfaceType)stype; }
  gp_Pln Plane() const override { touch(22); return gp_Pln(ax()); }
  gp_Cylinder Cylinder() const override { touch(23); return gp_Cylinder(ax(), r1); }
  gp_Cone Cone() const override { touch(24); return gp_Cone(ax(), semi, r1); }
  gp_Sphere Sphere() const override { touch(25); return gp_Sphere(ax(), r1); }
  gp_Torus Torus() const override { touch(26); return gp_Torus(ax(), r1, r2); }
  int UDegree() const override { touch(27); return udeg; }
  int NbUPoles() const override { touch(28); return nupoles; }
  int VDegree() const override { touch(29); return vdeg; }
  int NbVPoles() const override { touch(30); return nvpoles; }
  int NbUKnots() const override { touch(31); return nuknots; }
  int NbVKnots() const override { touch(32); return nvknots; }
  bool IsURational() const override { touch(33); return urat; }
  bool IsVRational() const override { touch(34); return vrat; }
  occ::handle<Geom_BezierSurface> Bezier() const override {
    touch(35);
    if (!hasBez) return Adaptor3d_Surface::Bezier();
    NCollection_Array2<gp_Pnt> poles(1, bezNu, 1, bezNv);
    for (int i = 0; i < bezNu; ++i)
      for (int j = 0; j < bezNv; ++j) {
        int k = 3 * (i * bezNv + j);
        poles(i + 1, j + 1) = gp_Pnt(bezP[k], bezP[k + 1], bezP[k + 2]);
      }
    if (bezW.empty()) return new Geom_BezierSurface(poles);
    NCollection_Array2<double> w(1, bezNu, 1, bezNv);
    for (int i = 0; i < bezNu; ++i)
      for (int j = 0; j < bezNv; ++j) w(i + 1, j + 1) = bezW[i * bezNv + j];
    return new Geom_BezierSurface(poles, w);
  }
  occ::handle<Geom_BSplineSurface> BSpline() const override {
    touch(36);
    if (!hasBsp) return Adaptor3d_Surface::BSpline();
    NCollection_Array2<gp_Pnt> poles(1, bspNu, 1, bspNv);
    for (int i = 0; i < bspNu; ++i)
      for (int j = 0; j < bspNv; ++j) {
        int k = 3 * (i * bspNv + j);
        poles(i + 1, j + 1) = gp_Pnt(bspP[k], bspP[k + 1], bspP[k + 2]);
      }
    int nuk = (int)bspUK.size(), nvk = (int)bspVK.size();
    NCollection_Array1<double> uk(1, nuk), vk(1, nvk);
    NCollection_Array1<int> um(1, nuk), vm(1, nvk);
    for (int i = 0; i < nuk; ++i) { uk(i + 1) = bspUK[i]; um(i + 1) = bspUM[i]; }
    for (int i = 0; i < nvk; ++i) { vk(i + 1) = bspVK[i]; vm(i + 1) = bspVM[i]; }
    if (bspW.empty()) return new Geom_BSplineSurface(poles, uk, vk, um, vm, bspUD, bspVD, bspUP, bspVP);
    NCollection_Array2<double> w(1, bspNu, 1, bspNv);
    for (int i = 0; i < bspNu; ++i)
      for (int j = 0; j < bspNv; ++j) w(i + 1, j + 1) = bspW[i * bspNv + j];
    return new Geom_BSplineSurface(poles, w, uk, vk, um, vm, bspUD, bspVD, bspUP, bspVP);
  }
  gp_Ax1 AxeOfRevolution() const override { touch(37); return gp_Ax1(gp_Pnt(ap[0], ap[1], ap[2]), gp_Dir(ad[0], ad[1], ad[2])); }
  gp_Dir Direction() const override { touch(38); return gp_Dir(ed[0], ed[1], ed[2]); }
  occ::handle<Adaptor3d_Curve> BasisCurve() const override { touch(39); return occ::handle<Adaptor3d_Curve>(bc); }
  occ::handle<Adaptor3d_Surface> BasisSurface() const override { touch(40); return occ::handle<Adaptor3d_Surface>(bs); }
  double OffsetValue() const override { touch(41); return offv; }
  gp_Pnt EvalD0(const double u, const double v) const override {
    touch(42, u, v);
    gp_Vec p = deriv(0, 0, u, v);
    return gp_Pnt(p.X(), p.Y(), p.Z());
  }
  Geom_Surface::ResD1 EvalD1(const double u, const double v) const override {
    touch(43, u, v);
    gp_Vec p = deriv(0, 0, u, v);
    Geom_Surface::ResD1 r;
    r.Point = gp_Pnt(p.X(), p.Y(), p.Z());
    r.D1U = deriv(1, 0, u, v);
    r.D1V = deriv(0, 1, u, v);
    return r;
  }
  Geom_Surface::ResD2 EvalD2(const double u, const double v) const override {
    touch(44, u, v);
    gp_Vec p = deriv(0, 0, u, v);
    Geom_Surface::ResD2 r;
    r.Point = gp_Pnt(p.X(), p.Y(), p.Z());
    r.D1U = deriv(1, 0, u, v);
    r.D1V = deriv(0, 1, u, v);
    r.D2U = deriv(2, 0, u, v);
    r.D2V = deriv(0, 2, u, v);
    r.D2UV = deriv(1, 1, u, v);
    return r;
  }
  Geom_Surface::ResD3 EvalD3(const double u, const double v) const override {
    touch(45, u, v);
    gp_Vec p = deriv(0, 0, u, v);
    Geom_Surface::ResD3 r;
    r.Point = gp_Pnt(p.X(), p.Y(), p.Z());
    r.D1U = deriv(1, 0, u, v);
    r.D1V = deriv(0, 1, u, v);
    r.D2U = deriv(2, 0, u, v);
    r.D2V = deriv(0, 2, u, v);
    r.D2UV = deriv(1, 1, u, v);
    r.D3U = deriv(3, 0, u, v);
    r.D3V = deriv(0, 3, u, v);
    r.D3UUV = deriv(2, 1, u, v);
    r.D3UVV = deriv(1, 2, u, v);
    return r;
  }
  gp_Vec EvalDN(const double u, const double v, const int a, const int b) const override {
    touch(46, u, v);
    hash = mixh(hash, (uint64_t)(a * 64 + b));
    if (a < 0 || b < 0 || a + b < 1) throw Standard_OutOfRange();
    return deriv(a, b, u, v);
  }
};
static void printTS(const TS& t) {
  F(t.u0); F(t.u1); F(t.v0); F(t.v1);
  I(t.ucont); I(t.vcont); I(t.nu); I(t.nv);
  for (int k = 0; k < 5; ++k) F(t.ubrk[k]);
  for (int k = 0; k < 5; ++k) F(t.vbrk[k]);
  B(t.uclosed); B(t.vclosed); B(t.uperiodic); B(t.vperiodic);
  F(t.uperiod); F(t.vperiod); F(t.ures); F(t.vres);
  I(t.stype);
  for (int i = 0; i < 10; ++i) for (int k = 0; k < 3; ++k) F(t.cf[i][k]);
  for (int k = 0; k < 3; ++k) F(t.org[k]);
  for (int k = 0; k < 3; ++k) F(t.zd[k]);
  for (int k = 0; k < 3; ++k) F(t.xd[k]);
  F(t.r1); F(t.r2); F(t.semi);
  for (int k = 0; k < 3; ++k) F(t.ap[k]);
  for (int k = 0; k < 3; ++k) F(t.ad[k]);
  for (int k = 0; k < 3; ++k) F(t.ed[k]);
  F(t.offv);
  I(t.udeg); I(t.nupoles); I(t.vdeg); I(t.nvpoles); I(t.nuknots); I(t.nvknots); B(t.urat); B(t.vrat);
  I(t.basisCurve); I(t.basisSurf);
  B(t.hasBez);
  if (t.hasBez) { I(t.bezNu); I(t.bezNv); prVec(t.bezP); prVec(t.bezW); }
  B(t.hasBsp);
  if (t.hasBsp) {
    I(t.bspUD); I(t.bspVD); B(t.bspUP); B(t.bspVP); I(t.bspNu); I(t.bspNv);
    prVec(t.bspUK); prVec(t.bspVK);
    I((long long)t.bspUM.size()); for (int m : t.bspUM) I(m);
    I((long long)t.bspVM.size()); for (int m : t.bspVM) I(m);
    prVec(t.bspP); prVec(t.bspW);
  }
}
static void prTS(const TS& t) {
  I(t.calls); U(t.hash);
  if (!t.bc.IsNull()) prTC3(*t.bc);
  if (!t.bs.IsNull()) prTS(*t.bs);
}

// ---- the definitions -------------------------------------------------------------------------------------------------------------
static std::vector<occ::handle<TC3>> DC3;
static std::vector<occ::handle<TS>> DS;
static std::vector<occ::handle<TC>> DC2;
static const int NC3 = 12, NS = 32, NC2 = 31, NC2_COS = 28;

static occ::handle<TC3> fresh3(int i) {
  occ::handle<TC3> t = new TC3(*DC3[i]);
  t->calls = 0;
  t->hash = H0;
  return t;
}
static occ::handle<TS> freshS(int i) { return DS[i]->fresh(); }
static occ::handle<TC> fresh2(int i) {
  occ::handle<TC> t = new TC(*DC2[i]);
  t->calls = 0;
  t->hash = H0;
  return t;
}

static void genFrame(double* org, double* zd, double* xd) {
  for (int k = 0; k < 3; ++k) org[k] = rv();
  for (;;) {
    for (int k = 0; k < 3; ++k) { zd[k] = rv(); xd[k] = rv(); }
    double cx = zd[1] * xd[2] - zd[2] * xd[1], cy = zd[2] * xd[0] - zd[0] * xd[2], cz = zd[0] * xd[1] - zd[1] * xd[0];
    double lc = std::sqrt(cx * cx + cy * cy + cz * cz);
    double lz = std::sqrt(zd[0] * zd[0] + zd[1] * zd[1] + zd[2] * zd[2]), lx = std::sqrt(xd[0] * xd[0] + xd[1] * xd[1] + xd[2] * xd[2]);
    if (lz > 0.3 && lx > 0.3 && lc > 0.15 * lz * lx) break;
  }
}
static void genBreaks(double* brk, int n) {
  double v = -1.0 + 0.25 * rv();
  for (int k = 0; k <= 4; ++k) { v += 0.5 + std::abs(rv()) * 0.25; brk[k] = (k <= n) ? v : 0.0; }
}
static void bezData(std::vector<double>& P, std::vector<double>& W, int n, bool rat) {
  P.clear(); W.clear();
  for (int i = 0; i < n; ++i) { P.push_back(rv()); P.push_back(rv()); P.push_back(rv()); }
  if (rat) for (int i = 0; i < n; ++i) W.push_back(0.5 + std::abs(rv()));
}
static occ::handle<TC3> genC3(int k) {
  occ::handle<TC3> t = new TC3();
  t->ctype = k == 10 ? 1 : (k == 11 ? 5 : (k == 7 ? 6 : k));
  if (k == 9) t->ctype = 8;
  if (k == 8) t->ctype = 7;
  static const int CT[7] = {6, 5, 4, 3, 2, 1, 0};
  t->cont = CT[(k / 2) % 7];
  t->nint = 1 + k % 4;
  genBreaks(t->brk, t->nint);
  t->u0 = t->brk[0];
  t->u1 = t->brk[t->nint];
  t->closed = (k % 4 == 1);
  t->periodic = (k % 3 == 1);
  t->period = 2.0 + std::abs(rv());
  t->rf = 0.5 + std::abs(rv());
  for (int i = 0; i < 5; ++i) for (int m = 0; m < 3; ++m) t->c[i][m] = (i == 4 && k % 3) ? 0.0 : rv();
  genFrame(t->org, t->zd, t->xd);
  for (int m = 0; m < 3; ++m) t->ld[m] = rv();
  if (std::abs(t->ld[0]) + std::abs(t->ld[1]) + std::abs(t->ld[2]) < 0.5) t->ld[0] = 1.0;
  t->r2 = 0.5 + std::abs(rv()) * 0.25;
  t->r1 = t->r2 + 0.25 + std::abs(rv()) * 0.5;
  t->degree = 1 + k % 5;
  t->rational = (k % 2 == 0);
  t->nbpoles = 2 + (k * 5) % 11;
  t->nbknots = 2 + (k * 3) % 9;
  if (k == 5 || k == 11) {
    t->hasBez = true;
    bezData(t->bezP, t->bezW, 3 + k % 2, k == 11);
  }
  if (k == 6 || k == 7 || k == 8) {
    t->hasBsp = true;
    t->bspDeg = 2;
    t->bspPer = false;
    t->bspK = {0.0, 0.5 + 0.25 * (k % 2), 1.5};
    t->bspM = {3, 1, 3};
    bezData(t->bspP, t->bspW, 4, k == 7);
  }
  if (k == 11) {
    // a periodic B-spline curve in the second Bezier slot is not wanted: keep the Bezier data only
  }
  if (k == 8) {
    t->hasOff = true;
    t->offVal = 0.25 + std::abs(rv());
    for (int m = 0; m < 3; ++m) t->offDir[m] = rv();
    if (std::abs(t->offDir[2]) < 0.5) t->offDir[2] = 1.0;
  }
  return t;
}

static occ::handle<TC> genC2(int k) {
  occ::handle<TC> t = new TC();
  static const int CT[31] = {0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 8, 0, 0, 6, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};
  t->ctype = CT[k];
  static const int CONT[7] = {6, 5, 4, 3, 2, 1, 0};
  t->cont = CONT[(k / 2) % 7];
  t->nint = 1 + (k / 2) % 4;
  genBreaks(t->brk, t->nint);
  if (k <= 4 || k == 12 || k == 13 || k >= 16) {  // the lines start at the parameter 0, where the position is (cx, cy)
    t->brk[0] = 0.0;
    for (int m = 1; m <= t->nint; ++m) t->brk[m] = 0.5 * m;
  }
  t->u0 = t->brk[0];
  t->u1 = t->brk[t->nint];
  t->closed = (k % 4 == 1);
  t->periodic = (k % 6 == 2);
  t->period = 2.0 + std::abs(rv());
  for (int m = 0; m < 2; ++m) { t->p0[m] = rv(); t->a[m] = rv(); t->b[m] = rv(); t->c[m] = rv(); t->d[m] = (k % 3) ? 0.0 : rv(); }
  t->cx = rv(); t->cy = rv();
  t->dx = 1.0 + std::abs(rv());
  t->dy = rv();
  t->sense = (k % 5 != 3);
  t->r2 = 0.5 + std::abs(rv()) * 0.25;
  t->r1 = t->r2 + 0.25 + std::abs(rv()) * 0.5;
  t->degree = 1 + k % 5;
  t->rational = (k % 2 == 0);
  t->nbpoles = 2 + (k * 5) % 11;
  t->nbknots = 2 + (k * 3) % 9;
  t->hasBezier = (k % 4 != 2);
  if (k == 10 || k == 14 || k == 9) {
    t->hasBsp2 = true;
    t->bsp2Deg = 2;
    t->bsp2K = {0.0, 1.0, 2.5};
    t->bsp2M = {3, 1, 3};
    for (int i = 0; i < 4; ++i) { t->bsp2P.push_back(rv()); t->bsp2P.push_back(rv()); }
    if (k == 14) for (int i = 0; i < 4; ++i) t->bsp2W.push_back(0.5 + std::abs(rv()));
  }
  if (k <= 4 || k == 12 || k == 13 || k >= 16) {
    // lines: P(u) = (cx, cy) + u * (dx, dy), the direction one of +X, -X, +Y, -Y, oblique
    static const double LX[5] = {1.0, -1.0, 0.0, 0.0, 1.0}, LY[5] = {0.0, 0.0, 1.0, -1.0, 0.5};
    int d = k == 12 ? 0 : (k == 13 ? 1 : (k >= 19 ? (k - 19) % 5 : (k >= 16 ? k - 16 : k)));
    t->dx = LX[d]; t->dy = LY[d];
    // positions on the knots of the B-spline surfaces (0, 1, 2 / 0, 0.5, 1.5) or inside the spans
    static const double PX[5] = {0.5, 1.0, 2.0, 0.0, 1.5}, PY[5] = {0.5, 1.5, 1.0, 0.5, 0.0};
    t->cx = PX[k % 5]; t->cy = PY[(k + 2) % 5];
    if (k == 12) { t->cx = 0.25; t->cy = 1.5; }  // |v| = pi / 2 is checked for the sphere: 1.5 is near it
    if (k == 13) { t->cx = 0.75; t->cy = 0.25; }
    if (k >= 19) {
      static const double PXS[8] = {-0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.5};
      int idx = k - 19;
      t->cx = PXS[(idx * 3) % 8]; t->cy = PXS[(idx * 5 + 1) % 8];
      t->p0[0] = t->cx; t->p0[1] = t->cy;
      t->dx = (idx % 5 == 0) ? 1.0 : ((idx % 5 == 1) ? -1.0 : ((idx % 5 == 4) ? 1.0 : 0.0));
      t->dy = (idx % 5 == 2) ? 1.0 : ((idx % 5 == 3) ? -1.0 : ((idx % 5 == 4) ? 0.5 : 0.0));
      t->a[0] = t->dx; t->a[1] = t->dy;
    }
    if (k == 16) t->u0 = -2.0e100;            // half infinite and infinite parameter ranges
    if (k == 17) t->u1 = 2.0e100;
    if (k == 18) { t->u0 = -2.0e100; t->u1 = 2.0e100; }
    t->p0[0] = t->cx; t->p0[1] = t->cy;
    t->a[0] = t->dx; t->a[1] = t->dy;
    for (int m = 0; m < 2; ++m) t->b[m] = t->c[m] = t->d[m] = 0.0;
  }
  return t;
}

static void bspSurfData(TS& t, int ud, int vd, bool up, bool vp, bool rat, int nuk, int nvk) {
  t.hasBsp = true;
  t.bspUD = ud; t.bspVD = vd; t.bspUP = up; t.bspVP = vp;
  auto mk = [&](int deg, bool per, int nk, std::vector<double>& K, std::vector<int>& M, double start) {
    K.clear(); M.clear();
    double v = start;
    int sum = 0;
    static const double STEP[3] = {1.0, 0.5, 1.0};
    for (int i = 0; i < nk; ++i) {
      K.push_back(v);
      v += STEP[i % 3];
      int m = per ? 1 : ((i == 0 || i == nk - 1) ? deg + 1 : 1 + (i % deg));
      M.push_back(m);
      sum += m;
    }
    return per ? sum - M.back() : sum - deg - 1;
  };
  t.bspNu = mk(ud, up, nuk, t.bspUK, t.bspUM, 0.0);
  t.bspNv = mk(vd, vp, nvk, t.bspVK, t.bspVM, 0.0);
  t.bspP.clear(); t.bspW.clear();
  for (int i = 0; i < t.bspNu * t.bspNv; ++i) { t.bspP.push_back(rv()); t.bspP.push_back(rv()); t.bspP.push_back(rv()); }
  if (rat) for (int i = 0; i < t.bspNu * t.bspNv; ++i) t.bspW.push_back(0.5 + std::abs(rv()));
  t.udeg = ud; t.vdeg = vd; t.nupoles = t.bspNu; t.nvpoles = t.bspNv; t.nuknots = nuk; t.nvknots = nvk;
  t.urat = t.vrat = rat;
  // the parameter range is that of the knots
  t.u0 = t.bspUK.front(); t.u1 = t.bspUK.back();
  t.v0 = t.bspVK.front(); t.v1 = t.bspVK.back();
  t.nu = nuk - 1; t.nv = nvk - 1;
  for (int i = 0; i <= t.nu && i < 5; ++i) t.ubrk[i] = t.bspUK[i];
  for (int i = 0; i <= t.nv && i < 5; ++i) t.vbrk[i] = t.bspVK[i];
}

static occ::handle<TS> genS(int k) {
  occ::handle<TS> t = new TS();
  static const int ST[NS] = {0, 1, 2, 3, 4, 5, 6, 6, 6, 7, 7, 7, 8, 8, 8, 8, 8, 9, 9, 9, 9, 10, 0, 1, 2, 3, 4, 6, 0, 0, 0, 0};
  t->stype = ST[k];
  static const int CT[7] = {6, 5, 4, 3, 2, 1, 0};
  t->ucont = CT[k % 7];
  t->vcont = CT[(k / 3) % 7];
  t->nu = 1 + k % 3;
  t->nv = 1 + (k / 2) % 3;
  genBreaks(t->ubrk, t->nu);
  genBreaks(t->vbrk, t->nv);
  t->u0 = t->ubrk[0]; t->u1 = t->ubrk[t->nu];
  t->v0 = t->vbrk[0]; t->v1 = t->vbrk[t->nv];
  t->uclosed = (k % 4 == 1); t->vclosed = (k % 5 == 2);
  t->uperiodic = false; t->vperiodic = false;
  t->uperiod = 6.25; t->vperiod = 3.5;
  t->ures = 0.5 + std::abs(rv()); t->vres = 0.5 + std::abs(rv());
  for (int i = 0; i < 10; ++i) for (int m = 0; m < 3; ++m) t->cf[i][m] = (i >= 6 && k % 3) ? 0.0 : rv();
  genFrame(t->org, t->zd, t->xd);
  t->r2 = 0.5 + std::abs(rv()) * 0.25;
  t->r1 = t->r2 + 0.25 + std::abs(rv()) * 0.5;
  t->semi = 0.25 + std::abs(rv()) * 0.25;
  for (int m = 0; m < 3; ++m) { t->ap[m] = rv(); t->ad[m] = rv(); t->ed[m] = rv(); }
  if (std::abs(t->ad[0]) + std::abs(t->ad[1]) + std::abs(t->ad[2]) < 0.5) t->ad[2] = 1.0;
  if (std::abs(t->ed[0]) + std::abs(t->ed[1]) + std::abs(t->ed[2]) < 0.5) t->ed[2] = 1.0;
  t->offv = 0.25 + std::abs(rv());
  t->udeg = 1 + k % 3; t->vdeg = 1 + (k / 2) % 3;
  t->nupoles = 2 + k % 5; t->nvpoles = 2 + (k / 2) % 4;
  t->nuknots = 2 + k % 4; t->nvknots = 2 + (k / 3) % 4;
  t->urat = (k % 2 == 0); t->vrat = (k % 3 == 0);
  switch (k) {
    case 1: case 2: case 3: case 4: case 23: case 24: case 25: case 26:
      t->uperiodic = true;  // cylinder, cone, sphere, torus: periodic in U
      if (k == 4 || k == 26) t->vperiodic = true;
      break;
    default: break;
  }
  switch (k) {
    case 5:
      t->hasBez = true;
      t->bezNu = 3; t->bezNv = 3;
      bezData(t->bezP, t->bezW, 9, true);
      t->udeg = 2; t->vdeg = 2; t->nupoles = 6; t->nvpoles = 7; t->urat = t->vrat = true;  // (the counts of the adaptor are not those of the surface)
      break;
    case 6: bspSurfData(*t, 2, 1, false, false, false, 3, 4); break;
    case 7: bspSurfData(*t, 2, 2, true, false, true, 4, 3); break;
    case 8: bspSurfData(*t, 3, 2, false, true, false, 3, 4); break;
    case 27: bspSurfData(*t, 3, 3, false, false, false, 5, 2); t->u1 = t->u0 + 40.0; break;  // anisotropic
    case 9: t->basisCurve = 6; break;
    case 10: t->basisCurve = 1; break;
    case 11: t->basisCurve = 5; break;
    case 12: t->basisCurve = 6; break;
    case 13: t->basisCurve = 1; break;
    case 14: t->basisCurve = 2; break;
    case 15: t->basisCurve = 5; break;
    case 16: t->basisCurve = 8; break;
    case 17: t->basisSurf = 6; break;
    case 18: t->basisSurf = 9; break;
    case 19: t->basisSurf = 12; break;
    case 20: t->basisSurf = 0; break;
    case 22: t->u0 = -DBL_MAX; t->u1 = DBL_MAX; t->v0 = -2.0e100; t->v1 = 2.0e100; break;
    case 23: t->v0 = 0.5; t->v1 = 2.0e100; break;
    case 28: t->u0 = -DBL_MAX; break;  // planes with a half infinite range: RealFirst / RealLast and the infinite value
    case 29: t->u1 = DBL_MAX; break;
    case 30: t->v0 = -2.0e100; break;
    case 31: t->v1 = 2.0e100; t->u0 = -2.0e100; break;
    case 24: t->v0 = -2.0e100; t->v1 = 2.0e100; t->u0 = 0.25; t->u1 = 4.0; break;
    default: break;
  }
  if (t->basisCurve >= 0) t->bc = fresh3(t->basisCurve);
  if (t->basisSurf >= 0) t->bs = freshS(t->basisSurf);
  return t;
}

static bool h_same(const occ::handle<Adaptor3d_HVertex>& a, const occ::handle<Adaptor3d_HVertex>& b) { return a->IsSame(b); }
// ---- helpers of the records --------------------------------------------------------------------------------------------------------
static const GeomAbs_Shape SH[7] = {GeomAbs_C0, GeomAbs_G1, GeomAbs_C1, GeomAbs_G2, GeomAbs_C2, GeomAbs_C3, GeomAbs_CN};
static void prBez(const occ::handle<Geom_BezierCurve>& b) {
  B(b.IsNull());
  if (b.IsNull()) return;
  I(b->NbPoles()); B(b->IsRational());
  for (int i = 1; i <= b->NbPoles(); ++i) { P3(b->Pole(i)); if (b->IsRational()) F(b->Weight(i)); }
}
static void prBsp(const occ::handle<Geom_BSplineCurve>& b) {
  B(b.IsNull());
  if (b.IsNull()) return;
  I(b->Degree()); B(b->IsPeriodic()); B(b->IsRational()); I(b->NbKnots()); I(b->NbPoles());
  for (int i = 1; i <= b->NbKnots(); ++i) { F(b->Knot(i)); I(b->Multiplicity(i)); }
  for (int i = 1; i <= b->NbPoles(); ++i) { P3(b->Pole(i)); if (b->IsRational()) F(b->Weight(i)); }
}
static void prBezS(const occ::handle<Geom_BezierSurface>& b) {
  B(b.IsNull());
  if (b.IsNull()) return;
  I(b->NbUPoles()); I(b->NbVPoles()); B(b->IsURational()); B(b->IsVRational());
  for (int i = 1; i <= b->NbUPoles(); ++i)
    for (int j = 1; j <= b->NbVPoles(); ++j) { P3(b->Pole(i, j)); if (b->IsURational() || b->IsVRational()) F(b->Weight(i, j)); }
}
static void prBspS(const occ::handle<Geom_BSplineSurface>& b) {
  B(b.IsNull());
  if (b.IsNull()) return;
  I(b->UDegree()); I(b->VDegree()); I(b->NbUKnots()); I(b->NbVKnots()); I(b->NbUPoles()); I(b->NbVPoles());
  for (int i = 1; i <= b->NbUKnots(); ++i) { F(b->UKnot(i)); I(b->UMultiplicity(i)); }
  for (int i = 1; i <= b->NbVKnots(); ++i) { F(b->VKnot(i)); I(b->VMultiplicity(i)); }
  for (int i = 1; i <= b->NbUPoles(); ++i)
    for (int j = 1; j <= b->NbVPoles(); ++j) P3(b->Pole(i, j));
}
// the arguments of an iso curve of the surface S: the kind, the parameter and the range of the other direction
static void isoArgs(int j, const TS& S, GeomAbs_IsoType& iso, double& p, double& wf, double& wl, bool fit = false) {
  iso = (j % 2) ? GeomAbs_IsoU : GeomAbs_IsoV;
  // `fit`: the kind the data of the curve exists for (U iso of a surface of revolution, V iso of an extrusion), except every fifth record
  if (fit && j % 5 != 4) { if (S.stype == 7) iso = GeomAbs_IsoU; else if (S.stype == 8) iso = GeomAbs_IsoV; }
  double a0 = iso == GeomAbs_IsoU ? S.u0 : S.v0, a1 = iso == GeomAbs_IsoU ? S.u1 : S.v1;
  double b0 = iso == GeomAbs_IsoU ? S.v0 : S.u0, b1 = iso == GeomAbs_IsoU ? S.v1 : S.u1;
  if (std::abs(a0) >= 1e50 || std::abs(a1) >= 1e50) { a0 = -1.0; a1 = 1.0; }
  // an infinite range: the range of the breaks of the intervals (the iso curve then stays inside the intervals)
  bool binf = std::abs(b0) >= 1e50 || std::abs(b1) >= 1e50;
  if (binf) {
    const double* bb = iso == GeomAbs_IsoU ? S.vbrk : S.ubrk;
    int nb = iso == GeomAbs_IsoU ? S.nv : S.nu;
    b0 = bb[0];
    b1 = bb[nb];
  }
  p = a0 + (a1 - a0) * ((double)(j % 3 + 1) / 4.0);
  int w = (j / 2) % 3;
  if (binf && w == 2) w = 0;
  if (w == 0) { wf = b0; wl = b1; }
  else if (w == 1) { wf = b0 + (b1 - b0) * 0.25; wl = b1 - (b1 - b0) * 0.125; }
  else { wf = b0 - 1.0; wl = b1 + 1.0; }
}
// a parameter of the iso curve between its bounds
static double inIso(const Adaptor3d_IsoCurve& c, int j) {
  double f = c.FirstParameter(), l = c.LastParameter();
  return f + (l - f) * ((double)(j % 5 + 1) / 6.0);
}
// a surface of the list (every fifth record: any surface)
static int pickS(int j, const int* list, int n) { return (j % 5 == 4) ? (j * 3 + j / NS) % NS : list[(j * 7 / 5) % n]; }
// which curve the family `iso_geom` asks for (0 Bezier, 1 B-spline): the one the surface has data for, except every fifth record
static int geomOp(int j, int s) {
  const TS& t = *DS[s];
  if (j % 5 == 4) return j % 2;
  if (t.stype == 5) return 0;
  if (t.stype == 6) return 1;
  if (t.stype == 7 || t.stype == 8) return t.basisCurve == 5 ? 0 : 1;
  return j % 2;
}
static const int LIST_INFO[13] = {5, 6, 7, 8, 27, 9, 10, 11, 12, 13, 14, 15, 16};
static const int LIST_CIRC[16] = {1, 2, 3, 4, 23, 24, 25, 9, 10, 11, 12, 13, 14, 15, 16, 3};
static const int LIST_CONIC[5] = {12, 13, 14, 15, 16};

int main() {
  // -- the definitions
  for (int k = 0; k < NC3; ++k) DC3.push_back(genC3(k));
  for (int k = 0; k < NS; ++k) DS.push_back(genS(k));
  for (int k = 0; k < NC2; ++k) DC2.push_back(genC2(k));
  for (int k = 0; k < NC3; ++k) {
    begin("def_curve3d"); I(k); printTC3(*DC3[k]); bar();
    CASE(occ::handle<TC3> c = fresh3(k); F(c->FirstParameter()); F(c->LastParameter()); I((int)c->GetType()); prTC3(*c);)
  }
  for (int k = 0; k < NS; ++k) {
    begin("def_surface"); I(k); printTS(*DS[k]); bar();
    CASE(occ::handle<TS> c = freshS(k); F(c->FirstUParameter()); F(c->LastUParameter()); F(c->FirstVParameter()); F(c->LastVParameter());
         I((int)c->GetType()); prTS(*c);)
  }
  for (int k = 0; k < NC2; ++k) {
    begin("def_curve2d"); I(k); printTC(*DC2[k]); bar();
    CASE(occ::handle<TC> c = fresh2(k); F(c->FirstParameter()); F(c->LastParameter()); I((int)c->GetType()); prTC(*c);)
  }

  // -- Adaptor3d_IsoCurve: constructors and Load
  for (int j = 0; j < 36; ++j) {
    int s = j % NS;
    GeomAbs_IsoType iso; double p, wf, wl;
    isoArgs(j, *DS[s], iso, p, wf, wl);
    int v = j % 6;
    if (v == 5) iso = GeomAbs_NoneIso;
    else if (v == 1 || v == 3) iso = ((j / 6) % 2) ? GeomAbs_IsoU : GeomAbs_IsoV;
    begin("iso_new"); I(s); I(v); I(iso); F(p); F(wf); F(wl); bar();
    CASE(
      occ::handle<TS> S = freshS(s);
      occ::handle<Adaptor3d_IsoCurve> c;
      if (v == 0) c = new Adaptor3d_IsoCurve(S);
      else if (v == 1) c = new Adaptor3d_IsoCurve(S, iso, p);
      else if (v == 2) c = new Adaptor3d_IsoCurve(S, iso, p, wf, wl);
      else if (v == 3) { c = new Adaptor3d_IsoCurve(); c->Load(S); c->Load(iso, p); }
      else if (v == 4) { c = new Adaptor3d_IsoCurve(); c->Load(S); c->Load(iso, p, wf, wl); }
      else { c = new Adaptor3d_IsoCurve(S); c->Load(iso, p); }
      I((int)c->Iso()); F(c->Parameter()); F(c->FirstParameter()); F(c->LastParameter()); prTS(*S);)
  }

  // -- continuity, closedness, periodicity, resolution
  for (int j = 0; j < 40; ++j) {
    int s = (j * 3 + j / NS) % NS;
    GeomAbs_IsoType iso; double p, wf, wl;
    isoArgs(j, *DS[s], iso, p, wf, wl);
    int op = j % 5;
    bool none = (j % 9 == 8);
    double r = 0.25 + (j % 4);
    begin("iso_params"); I(s); I(none ? 2 : (int)iso); F(p); F(wf); F(wl); I(op); F(r); bar();
    CASE(
      occ::handle<TS> S = freshS(s);
      Adaptor3d_IsoCurve c(S);
      if (!none) c.Load(iso, p, wf, wl);
      if (op == 0) I((int)c.Continuity());
      else if (op == 1) B(c.IsClosed());
      else if (op == 2) B(c.IsPeriodic());
      else if (op == 3) F(c.Period());
      else F(c.Resolution(r));
      prTS(*S);)
  }

  // -- intervals
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 30; ++j) {
      int s = (j * 3 + j / NS) % NS;
      GeomAbs_IsoType iso; double p, wf, wl;
      isoArgs(j, *DS[s], iso, p, wf, wl);
      int si = j % 7;
      bool none = (j % 10 == 9);
      begin(fam == 0 ? "iso_nb_intervals" : "iso_intervals"); I(s); I(none ? 2 : (int)iso); F(p); F(wf); F(wl); I(si); bar();
      if (fam == 0) CASE(
        occ::handle<TS> S = freshS(s);
        Adaptor3d_IsoCurve c(S);
        if (!none) c.Load(iso, p, wf, wl);
        I(c.NbIntervals(SH[si])); prTS(*S);)
      else CASE(
        occ::handle<TS> S = freshS(s);
        Adaptor3d_IsoCurve c(S);
        if (!none) c.Load(iso, p, wf, wl);
        int n = c.NbIntervals(SH[si]);
        NCollection_Array1<double> T(1, n + 1);
        T.Init(-7.0);
        c.Intervals(T, SH[si]);
        I(n);
        for (int i = 1; i <= n + 1; ++i) F(T(i));
        prTS(*S);)
    }

  // -- evaluation
  for (int j = 0; j < 80; ++j) {
    int s = (j * 3 + j / NS) % NS;
    GeomAbs_IsoType iso; double p, wf, wl;
    isoArgs(j, *DS[s], iso, p, wf, wl);
    int op = j % 5;
    static const int NS5[6] = {1, 2, 3, 4, 0, -1};
    int n = op == 4 ? NS5[(j / 5) % 6] : 0;
    bool none = (j % 13 == 12);
    occ::handle<TS> S0 = freshS(s);
    Adaptor3d_IsoCurve c0(S0);
    if (!none) c0.Load(iso, p, wf, wl);
    double u = none ? 0.5 : inIso(c0, j);
    begin("iso_eval"); I(s); I(none ? 2 : (int)iso); F(p); F(wf); F(wl); I(op); I(n); F(u); bar();
    CASE(
      occ::handle<TS> S = freshS(s);
      Adaptor3d_IsoCurve c(S);
      if (!none) c.Load(iso, p, wf, wl);
      if (op == 0) { gp_Pnt P = c.EvalD0(u); P3(P); }
      else if (op == 1) { auto r = c.EvalD1(u); P3(r.Point); V3(r.D1); }
      else if (op == 2) { auto r = c.EvalD2(u); P3(r.Point); V3(r.D1); V3(r.D2); }
      else if (op == 3) { auto r = c.EvalD3(u); P3(r.Point); V3(r.D1); V3(r.D2); V3(r.D3); }
      else V3(c.EvalDN(u, n));
      prTS(*S);)
  }

  // -- the type, the data of the curve
  for (int j = 0; j < 56; ++j) {
    int s = (j * 3 + j / NS) % NS;
    GeomAbs_IsoType iso; double p, wf, wl;
    isoArgs(j, *DS[s], iso, p, wf, wl);
    bool none = (j % 11 == 10);
    begin("iso_type"); I(s); I(none ? 2 : (int)iso); F(p); F(wf); F(wl); bar();
    CASE(
      occ::handle<TS> S = freshS(s);
      Adaptor3d_IsoCurve c(S);
      if (!none) c.Load(iso, p, wf, wl);
      I((int)c.GetType()); prTS(*S);)
  }
  for (int j = 0; j < 56; ++j) {
    int s = pickS(j, LIST_INFO, 13);
    GeomAbs_IsoType iso; double p, wf, wl;
    isoArgs(j, *DS[s], iso, p, wf, wl, true);
    int op = (j / 2) % 4;
    bool none = (j % 12 == 11);
    begin("iso_info"); I(s); I(none ? 2 : (int)iso); F(p); F(wf); F(wl); I(op); bar();
    CASE(
      occ::handle<TS> S = freshS(s);
      Adaptor3d_IsoCurve c(S);
      if (!none) c.Load(iso, p, wf, wl);
      if (op == 0) I(c.Degree());
      else if (op == 1) B(c.IsRational());
      else if (op == 2) I(c.NbPoles());
      else I(c.NbKnots());
      prTS(*S);)
  }
  for (int fam = 0; fam < 4; ++fam)
    for (int j = 0; j < 64; ++j) {
      int s = fam == 0 ? (j * 3 + j / NS) % NS
              : fam == 1 ? pickS(j, LIST_CIRC, 16)
              : fam == 2 ? pickS(j, LIST_CONIC, 5)
                         : pickS(j, LIST_INFO, 13);
      GeomAbs_IsoType iso; double p, wf, wl;
      isoArgs(j, *DS[s], iso, p, wf, wl, true);
      if (fam == 1 && j % 5 != 4 && DS[s]->stype != 7 && DS[s]->stype != 8) iso = ((j * 7 / 5) / 16) % 2 ? GeomAbs_IsoU : GeomAbs_IsoV;
      static const char* NM[4] = {"iso_line", "iso_circle", "iso_conic", "iso_geom"};
      int op = fam == 3 ? geomOp(j, s) : ((j % 5 == 0) ? 1 : ((j % 5 == 1) ? 2 : 0));
      begin(NM[fam]); I(s); I((j % 17 == 16) ? 2 : (int)iso); F(p); F(wf); F(wl); if (fam >= 2) I(op); I(j); bar();
      bool none = (j % 17 == 16);
      CASE(
        occ::handle<TS> S = freshS(s);
        Adaptor3d_IsoCurve c(S);
        if (!none) c.Load(iso, p, wf, wl);
        if (fam == 0) PrLin(c.Line());
        else if (fam == 1) PrCirc(c.Circle());
        else if (fam == 2) {
          if (op == 0) PrElips(c.Ellipse());
          else if (op == 1) PrHypr(c.Hyperbola());
          else PrParab(c.Parabola());
        } else {
          if (op == 0) prBez(c.Bezier());
          else prBsp(c.BSpline());
        }
        prTS(*S);)
    }

  // -- shallow copy and trim
  for (int j = 0; j < 24; ++j) {
    int s = (j * 3 + j / NS) % NS;
    GeomAbs_IsoType iso; double p, wf, wl;
    isoArgs(j, *DS[s], iso, p, wf, wl);
    bool trim = (j % 2 == 0);
    double a = wf + (wl - wf) * 0.125, b = wl - (wl - wf) * 0.25, tol = 1e-7;
    begin("iso_copy"); I(s); I(iso); F(p); F(wf); F(wl); B(trim); F(a); F(b); bar();
    CASE(
      occ::handle<TS> S = freshS(s);
      Adaptor3d_IsoCurve c(S, iso, p, wf, wl);
      occ::handle<Adaptor3d_Curve> h = trim ? c.Trim(a, b, tol) : c.ShallowCopy();
      occ::handle<Adaptor3d_IsoCurve> i2 = occ::down_cast<Adaptor3d_IsoCurve>(h);
      I((int)i2->Iso()); F(i2->Parameter()); F(i2->FirstParameter()); F(i2->LastParameter());
      B(i2->Surface().get() == S.get());
      prTS(*S); prTS(*occ::down_cast<TS>(i2->Surface()));)
  }

  // -- Adaptor3d_CurveOnSurface
  // pairs (surface, curve): every surface with a rotating choice of the 2D curves (the first five are the lines)
  auto cosPair = [](int j, int& si, int& ci) {
    si = (j * 5 + j / NS) % NS;
    int x = (j % 3 == 0) ? (j / 3) % 5 : (j * 7 + j / 4) % NC2_COS;
    // the indices 0..15 and 19..30 (not the infinite lines 16..18)
    ci = x < 16 ? x : x + 3;
  };
  auto prCos = [](Adaptor3d_CurveOnSurface& c) {
    I((int)c.GetType());
    switch (c.GetType()) {
      case GeomAbs_Line: PrLin(c.Line()); break;
      case GeomAbs_Circle: PrCirc(c.Circle()); break;
      default: break;
    }
  };
  for (int j = 0; j < 72; ++j) {
    int si, ci;
    cosPair(j, si, ci);
    int v = j % 6;
    begin("cos_new"); I(si); I(ci); I(v); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<TC> C = fresh2(ci);
      occ::handle<Adaptor3d_CurveOnSurface> c;
      if (v == 0) c = new Adaptor3d_CurveOnSurface(S);
      else if (v == 1) c = new Adaptor3d_CurveOnSurface(C, S);
      else if (v == 2) { c = new Adaptor3d_CurveOnSurface(); c->Load(S); c->Load(C); }
      else if (v == 3) { c = new Adaptor3d_CurveOnSurface(); c->Load(C); c->Load(S); }
      else if (v == 4) { c = new Adaptor3d_CurveOnSurface(); c->Load(C, S); }
      else { c = new Adaptor3d_CurveOnSurface(S); c->Load(C); }
      prCos(*c); B(c->GetCurve().get() == (v == 0 ? nullptr : C.get())); prTS(*S); prTC(*C);)
  }
  for (int j = 0; j < 56; ++j) {
    int si, ci;
    cosPair(j, si, ci);
    int op = j % 7;
    double r = 0.25 + (j % 4);
    begin("cos_params"); I(si); I(ci); I(op); F(r); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_CurveOnSurface c(C, S);
      if (op == 0) F(c.FirstParameter());
      else if (op == 1) F(c.LastParameter());
      else if (op == 2) I((int)c.Continuity());
      else if (op == 3) B(c.IsClosed());
      else if (op == 4) { B(c.IsPeriodic()); }
      else if (op == 5) F(c.Period());
      else F(c.Resolution(r));
      I((int)c.GetType()); prTS(*S); prTC(*C);)
  }
  for (int j = 0; j < 120; ++j) {
    int si, ci;
    cosPair(j, si, ci);
    int op = j % 5;
    static const int NS5[5] = {1, 2, 3, 4, 0};
    int n = op == 4 ? NS5[(j / 5) % 5] : 0;
    static const double FR[6] = {0.0, 1.0, 0.25, 0.5, 0.75, 0.0};
    double fr = FR[(j / 5) % 6];
    double f0 = DC2[ci]->u0, l0 = DC2[ci]->u1;
    double u = f0 + (l0 - f0) * fr;
    begin("cos_eval"); I(si); I(ci); I(op); I(n); F(u); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_CurveOnSurface c(C, S);
      if (op == 0) { gp_Pnt P = c.EvalD0(u); P3(P); }
      else if (op == 1) { auto r = c.EvalD1(u); P3(r.Point); V3(r.D1); }
      else if (op == 2) { auto r = c.EvalD2(u); P3(r.Point); V3(r.D1); V3(r.D2); }
      else if (op == 3) { auto r = c.EvalD3(u); P3(r.Point); V3(r.D1); V3(r.D2); V3(r.D3); }
      else V3(c.EvalDN(u, n));
      I((int)c.GetType()); prTS(*S); prTC(*C);)
  }
  for (int j = 0; j < 60; ++j) {
    // a plane (0, then the infinite one 22) and an ellipse, hyperbola, parabola, Bezier curve, B-spline curve
    static const int CIS[6] = {5, 6, 7, 8, 9, 10};
    int si = j < 36 ? 0 : 22, ci = CIS[j % 6];
    int op = (j / 6) % 6;
    if (j % 7 == 3) si = (j + 3) % NS;
    begin("cos_info"); I(si); I(ci); I(op); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_CurveOnSurface c(C, S);
      if (op == 0) PrElips(c.Ellipse());
      else if (op == 1) PrHypr(c.Hyperbola());
      else if (op == 2) PrParab(c.Parabola());
      else if (op == 3) { I(c.Degree()); B(c.IsRational()); I(c.NbPoles()); }
      else if (op == 4) { I(c.NbKnots()); prBez(c.Bezier()); }
      else prBsp(c.BSpline());
      prTS(*S); prTC(*C);)
  }
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 44; ++j) {
      int si, ci;
      cosPair(j * 3 + 1, si, ci);
      int sh = j % 7;
      begin(fam == 0 ? "cos_nb_intervals" : "cos_intervals"); I(si); I(ci); I(sh); bar();
      if (fam == 0) CASE(
        occ::handle<TS> S = freshS(si);
        occ::handle<TC> C = fresh2(ci);
        Adaptor3d_CurveOnSurface c(C, S);
        I(c.NbIntervals(SH[sh])); prTS(*S); prTC(*C);)
      else CASE(
        occ::handle<TS> S = freshS(si);
        occ::handle<TC> C = fresh2(ci);
        Adaptor3d_CurveOnSurface c(C, S);
        int n = c.NbIntervals(SH[sh]);
        NCollection_Array1<double> T(1, n + 1);
        T.Init(-7.0);
        c.Intervals(T, SH[sh]);
        I(n);
        for (int i = 1; i <= n + 1; ++i) F(T(i));
        prTS(*S); prTC(*C);)
    }
  for (int j = 0; j < 24; ++j) {
    int si, ci;
    cosPair(j * 5 + 2, si, ci);
    bool trim = (j % 2 == 0);
    double f0 = DC2[ci]->u0, l0 = DC2[ci]->u1;
    double a = f0 + (l0 - f0) * 0.125, b = l0 - (l0 - f0) * 0.25;
    begin("cos_copy"); I(si); I(ci); B(trim); F(a); F(b); I(j); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_CurveOnSurface c(C, S);
      occ::handle<Adaptor3d_Curve> h = trim ? c.Trim(a, b, 1e-7) : c.ShallowCopy();
      occ::handle<Adaptor3d_CurveOnSurface> c2 = occ::down_cast<Adaptor3d_CurveOnSurface>(h);
      F(c2->FirstParameter()); F(c2->LastParameter()); I((int)c2->GetType());
      B(c2->GetSurface().get() == S.get());
      prTS(*S); prTC(*C);
      prTS(*occ::down_cast<TS>(c2->GetSurface())); prTC(*occ::down_cast<TC>(c2->GetCurve()));)
  }

  // -- Adaptor3d_TopolTool
  auto prLine2d = [](const occ::handle<Adaptor2d_Curve2d>& c) {
    PrLin2(c->Line()); F(c->FirstParameter()); F(c->LastParameter());
  };
  for (int j = 0; j < 40; ++j) {
    int si = j % NS;
    bool bad = (j % 9 == 8);
    begin("topol_restr"); I(si); B(bad); I(j); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      Adaptor3d_TopolTool t;
      t.Initialize(S);
      int n = 0;
      std::vector<occ::handle<Adaptor2d_Curve2d>> v;
      for (t.Init(); t.More(); t.Next()) { v.push_back(t.Value()); ++n; }
      I(n);
      for (auto& c : v) prLine2d(c);
      B(t.DomainIsInfinite()); B(t.IsUniformSampling());
      if (bad) { occ::handle<Adaptor2d_Curve2d> c = t.Value(); }
      prTS(*S);)
  }
  for (int j = 0; j < 2; ++j) {
    begin("topol_init0"); I(j); bar();
    CASE(Adaptor3d_TopolTool t; t.Initialize(); I(j);)
  }
  for (int j = 0; j < 38; ++j) {
    int ci = (j % 4 == 3) ? 16 + (j / 4) % 3 : (j * 3 + j / 5) % 16;
    if (ci >= 5 && ci < 16 && j % 2) ci = (j / 2) % 5;  // lines for the parameter of a vertex
    bool bad = (j % 8 == 7);
    begin("topol_vertices"); I(ci); B(bad); I(j); bar();
    CASE(
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_TopolTool t;
      t.Initialize(C);
      int n = 0;
      std::vector<occ::handle<Adaptor3d_HVertex>> v;
      for (t.InitVertexIterator(); t.MoreVertex(); t.NextVertex()) { v.push_back(t.Vertex()); ++n; }
      I(n);
      for (auto& h : v) {
        P2(h->Value()); I((int)h->Orientation()); F(h->Resolution(C));
        if (ci <= 4 || ci >= 12) F(h->Parameter(C));
      }
      if (n == 2) { B(h_same(v[0], v[1])); B(t.Identical(v[0], v[1])); }
      if (bad) { t.NextVertex(); occ::handle<Adaptor3d_HVertex> x = t.Vertex(); }
      prTC(*C);)
  }
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 96; ++j) {
      static const int TS_CL[12] = {22, 23, 24, 28, 29, 30, 31, 0, 1, 5, 6, 3};
      int si = TS_CL[(j / 8) % 12];
      const TS& d = *DS[si];
      double u0 = std::abs(d.u0) >= 1e50 ? -1.0 : d.u0, u1 = std::abs(d.u1) >= 1e50 ? 1.0 : d.u1;
      double v0 = std::abs(d.v0) >= 1e50 ? -1.0 : d.v0, v1 = std::abs(d.v1) >= 1e50 ? 1.0 : d.v1;
      double tol = (j % 3 == 0) ? 1e-7 : ((j % 3 == 1) ? 0.0625 : 0.5);
      if (j >= 48) tol = (j % 2) ? 0.125 : 1e-9;
      double U, V;
      switch (j % 8) {
        case 0: U = 0.5 * (u0 + u1); V = 0.5 * (v0 + v1); break;
        case 1: U = u0 + 0.5 * tol; V = 0.5 * (v0 + v1); break;
        case 2: U = u1 - 0.5 * tol; V = 0.5 * (v0 + v1); break;
        case 3: U = 0.5 * (u0 + u1); V = v0 + 0.25 * tol; break;
        case 4: U = 0.5 * (u0 + u1); V = v1; break;
        case 5: U = u1 + 2.0 * tol + 0.25; V = 0.5 * (v0 + v1); break;
        case 6: U = 0.5 * (u0 + u1); V = v0 - 2.0 * tol - 0.25; break;
        default: U = u0 - 2.0 * tol - 0.25; V = v1 + 1.0; break;
      }
      bool rp = (j % 2 == 0);
      begin(fam ? "topol_classify" : "topol_on"); I(si); F(U); F(V); F(tol); B(rp); bar();
      CASE(
        occ::handle<TS> S = freshS(si);
        Adaptor3d_TopolTool t(S);
        if (fam) I((int)t.Classify(gp_Pnt2d(U, V), tol, rp));
        else B(t.IsThePointOn(gp_Pnt2d(U, V), tol, rp));
        prTS(*S);)
    }
  for (int j = 0; j < 40; ++j) {
    int si = j % NS;
    int op = (j / 2) % 8;
    int ci = (j * 3) % 16;
    begin("topol_misc"); I(si); I(ci); I(op); I(j); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_TopolTool t;
      t.Initialize(S);
      occ::handle<Adaptor3d_HVertex> v1 = new Adaptor3d_HVertex(gp_Pnt2d(0.25, 0.5), TopAbs_FORWARD, 1e-8);
      occ::handle<Adaptor3d_HVertex> v2 = new Adaptor3d_HVertex(gp_Pnt2d(0.25 + (j % 2) * 1e-8, 0.5), TopAbs_REVERSED, 1e-8);
      if (op == 0) I((int)t.Orientation(C));
      else if (op == 1) { I((int)t.Orientation(v1)); I((int)t.Orientation(v2)); }
      else if (op == 2) B(t.Identical(v1, v2));
      else if (op == 3) B(t.Has3d());
      else if (op == 4) F(t.Tol3d(C));
      else if (op == 5) F(t.Tol3d(v1));
      else if (op == 6) P3(t.Pnt(v1));
      else B(t.Edge() == nullptr);
      prTS(*S); prTC(*C);)
  }
  for (int j = 0; j < 56; ++j) {
    int si = j % NS;
    begin("topol_samples"); I(si); I(j / NS); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      Adaptor3d_TopolTool t;
      t.Initialize(S);
      if (j / NS == 1) t.ComputeSamplePoints();
      int nu = t.NbSamplesU(), nv = t.NbSamplesV();
      I(nu); I(nv); I(t.NbSamples());
      static const int IDX[4] = {0, 1, 7, 13};
      for (int k = 0; k < 4; ++k) {
        gp_Pnt2d P2d; gp_Pnt P3d;
        t.SamplePoint(IDX[k] % (nu * nv) + 1, P2d, P3d);
        P2(P2d); P3(P3d);
      }
      prTS(*S);)
  }
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 40; ++j) {
      static const int BS[4] = {6, 7, 8, 27};
      int si = fam == 1 ? BS[j % 4] : j % NS;
      static const double DEFL[4] = {0.01, 0.1, 0.001, 1.0};
      static const int NMIN[4] = {4, 6, 10, 5};
      double defl = DEFL[(j / 4) % 4];
      int nu = NMIN[j % 4], nv = NMIN[(j / 2 + j / 8) % 4];
      begin(fam == 0 ? "topol_samplepnts" : "topol_bspl_samplepnts"); I(si); F(defl); I(nu); I(nv); I(j); bar();
      CASE(
        occ::handle<TS> S = freshS(si);
        Adaptor3d_TopolTool t;
        t.Initialize(S);
        if (fam == 0) t.SamplePnts(defl, nu, nv);
        else t.BSplSamplePnts(defl, nu, nv);
        int nsu = t.NbSamplesU(), nsv = t.NbSamplesV();
        I(nsu); I(nsv);
        NCollection_Array1<double> up(1, nsu), vp(1, nsv);
        t.UParameters(up);
        t.VParameters(vp);
        for (int i = 1; i <= nsu; ++i) F(up(i));
        for (int i = 1; i <= nsv; ++i) F(vp(i));
        gp_Pnt2d P2d; gp_Pnt P3d;
        t.SamplePoint(1 + (j % (nsu * nsv)), P2d, P3d);
        P2(P2d); P3(P3d);
        prTS(*S);)
    }
  for (int j = 0; j < 30; ++j) {
    double org[3], zd[3], xd[3];
    genFrame(org, zd, xd);
    if (j % 5 == 0) org[0] = org[1] = org[2] = 0.0;
    double semi = (j % 2 ? 1.0 : -1.0) * (0.125 + 0.125 * (j % 5)), rad = (j % 4 == 0) ? 0.0 : std::abs(rv()) + 0.25;
    begin("topol_cone_apex");
    for (int k = 0; k < 3; ++k) F(org[k]);
    for (int k = 0; k < 3; ++k) F(zd[k]);
    for (int k = 0; k < 3; ++k) F(xd[k]);
    F(semi); F(rad);
    bar();
    CASE(
      gp_Cone cone(gp_Ax3(gp_Pnt(org[0], org[1], org[2]), gp_Dir(zd[0], zd[1], zd[2]), gp_Dir(xd[0], xd[1], xd[2])), semi, rad);
      double U = -7.0, V = -7.0;
      Adaptor3d_TopolTool::GetConeApexParam(cone, U, V);
      F(U); F(V);)
  }

  // -- Adaptor3d_HSurfaceTool
  for (int j = 0; j < 148; ++j) {
    int si = (j * 5 + j / 37) % NS;
    int op = j % 37;
    int sh = (j / 37) % 7;
    double u = 0.25 + (j % 4) * 0.125, v = 0.5 - (j % 3) * 0.125;
    static const int NS5[5] = {1, 2, 3, 0, -1};
    int na = NS5[j % 5], nb = NS5[(j / 5) % 5];
    begin("hst_forward"); I(si); I(op); I(sh); F(u); F(v); I(na); I(nb); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      occ::handle<Adaptor3d_Surface> A = S;
      switch (op) {
        case 0: F(Adaptor3d_HSurfaceTool::FirstUParameter(A)); break;
        case 1: F(Adaptor3d_HSurfaceTool::FirstVParameter(A)); break;
        case 2: F(Adaptor3d_HSurfaceTool::LastUParameter(A)); break;
        case 3: F(Adaptor3d_HSurfaceTool::LastVParameter(A)); break;
        case 4: I(Adaptor3d_HSurfaceTool::NbUIntervals(A, SH[sh])); break;
        case 5: I(Adaptor3d_HSurfaceTool::NbVIntervals(A, SH[sh])); break;
        case 6: {
          int n = Adaptor3d_HSurfaceTool::NbUIntervals(A, SH[sh]);
          NCollection_Array1<double> T(1, n + 1);
          Adaptor3d_HSurfaceTool::UIntervals(A, T, SH[sh]);
          I(n); for (int i = 1; i <= n + 1; ++i) F(T(i));
          break;
        }
        case 7: {
          int n = Adaptor3d_HSurfaceTool::NbVIntervals(A, SH[sh]);
          NCollection_Array1<double> T(1, n + 1);
          Adaptor3d_HSurfaceTool::VIntervals(A, T, SH[sh]);
          I(n); for (int i = 1; i <= n + 1; ++i) F(T(i));
          break;
        }
        case 8: { occ::handle<Adaptor3d_Surface> r = Adaptor3d_HSurfaceTool::UTrim(A, u, u + 1.0, 1e-7); F(r->FirstUParameter()); F(r->LastUParameter()); break; }
        case 9: { occ::handle<Adaptor3d_Surface> r = Adaptor3d_HSurfaceTool::VTrim(A, v, v + 1.0, 1e-7); F(r->FirstVParameter()); F(r->LastVParameter()); break; }
        case 10: B(Adaptor3d_HSurfaceTool::IsUClosed(A)); break;
        case 11: B(Adaptor3d_HSurfaceTool::IsVClosed(A)); break;
        case 12: B(Adaptor3d_HSurfaceTool::IsUPeriodic(A)); break;
        case 13: F(Adaptor3d_HSurfaceTool::UPeriod(A)); break;
        case 14: B(Adaptor3d_HSurfaceTool::IsVPeriodic(A)); break;
        case 15: F(Adaptor3d_HSurfaceTool::VPeriod(A)); break;
        case 16: P3(Adaptor3d_HSurfaceTool::Value(A, u, v)); break;
        case 17: { gp_Pnt P; Adaptor3d_HSurfaceTool::D0(A, u, v, P); P3(P); break; }
        case 18: { gp_Pnt P; gp_Vec a, b; Adaptor3d_HSurfaceTool::D1(A, u, v, P, a, b); P3(P); V3(a); V3(b); break; }
        case 19: { gp_Pnt P; gp_Vec a, b, c, d, e; Adaptor3d_HSurfaceTool::D2(A, u, v, P, a, b, c, d, e); P3(P); V3(a); V3(b); V3(c); V3(d); V3(e); break; }
        case 20: {
          gp_Pnt P; gp_Vec a, b, c, d, e, f, g, h, k;
          Adaptor3d_HSurfaceTool::D3(A, u, v, P, a, b, c, d, e, f, g, h, k);
          P3(P); V3(a); V3(b); V3(c); V3(d); V3(e); V3(f); V3(g); V3(h); V3(k);
          break;
        }
        case 21: V3(Adaptor3d_HSurfaceTool::DN(A, u, v, na, nb)); break;
        case 22: F(Adaptor3d_HSurfaceTool::UResolution(A, u)); break;
        case 23: F(Adaptor3d_HSurfaceTool::VResolution(A, v)); break;
        case 24: I((int)Adaptor3d_HSurfaceTool::GetType(A)); break;
        case 25: { gp_Pln p = Adaptor3d_HSurfaceTool::Plane(A); A3(p.Position()); break; }
        case 26: { gp_Cylinder c = Adaptor3d_HSurfaceTool::Cylinder(A); A3(c.Position()); F(c.Radius()); break; }
        case 27: { gp_Cone c = Adaptor3d_HSurfaceTool::Cone(A); A3(c.Position()); F(c.RefRadius()); F(c.SemiAngle()); break; }
        case 28: { gp_Torus c = Adaptor3d_HSurfaceTool::Torus(A); A3(c.Position()); F(c.MajorRadius()); F(c.MinorRadius()); break; }
        case 29: { gp_Sphere c = Adaptor3d_HSurfaceTool::Sphere(A); A3(c.Position()); F(c.Radius()); break; }
        case 30: prBezS(Adaptor3d_HSurfaceTool::Bezier(A)); break;
        case 31: prBspS(Adaptor3d_HSurfaceTool::BSpline(A)); break;
        case 32: A1(Adaptor3d_HSurfaceTool::AxeOfRevolution(A)); break;
        case 33: D3(Adaptor3d_HSurfaceTool::Direction(A)); break;
        case 34: { occ::handle<Adaptor3d_Curve> c = Adaptor3d_HSurfaceTool::BasisCurve(A); B(c.IsNull()); if (!c.IsNull()) I((int)c->GetType()); break; }
        case 35: { occ::handle<Adaptor3d_Surface> c = Adaptor3d_HSurfaceTool::BasisSurface(A); B(c.IsNull()); if (!c.IsNull()) I((int)c->GetType()); break; }
        default: F(Adaptor3d_HSurfaceTool::OffsetValue(A)); break;
      }
      prTS(*S);)
  }
  for (int j = 0; j < 56; ++j) {
    static const int G1S[14] = {6, 7, 8, 27, 9, 12, 16, 17, 18, 19, 20, 5, 0, 13};
    int si = G1S[j % 14];
    bool alongU = (j / 14) % 2 == 0;
    static const double TOLS[3] = {0.01, 0.5, 1e-9};
    double tol = TOLS[(j / 28) % 3];
    begin("hst_g1"); I(si); B(alongU); F(tol); bar();
    CASE(
      occ::handle<TS> S = freshS(si);
      B(Adaptor3d_HSurfaceTool::IsSurfG1(S, alongU, tol));
      prTS(*S);)
  }
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 40; ++j) {
      int si = (j * 3 + j / NS) % NS;
      const TS& d = *DS[si];
      double a0 = std::abs(d.u0) >= 1e50 ? -1.0 : d.u0, a1 = std::abs(d.u1) >= 1e50 ? 1.0 : d.u1;
      double b0 = std::abs(d.v0) >= 1e50 ? -1.0 : d.v0, b1 = std::abs(d.v1) >= 1e50 ? 1.0 : d.v1;
      double x1 = fam == 0 ? a0 : b0, x2 = fam == 0 ? a0 + (a1 - a0) * (0.25 + 0.25 * (j % 3)) : b0 + (b1 - b0) * (0.25 + 0.25 * (j % 3));
      bool two = (j % 2 == 0);
      begin(fam == 0 ? "hst_nb_samples_u" : "hst_nb_samples_v"); I(si); B(two); F(x1); F(x2); bar();
      CASE(
        occ::handle<TS> S = freshS(si);
        if (fam == 0) I(two ? Adaptor3d_HSurfaceTool::NbSamplesU(S, x1, x2) : Adaptor3d_HSurfaceTool::NbSamplesU(S));
        else I(two ? Adaptor3d_HSurfaceTool::NbSamplesV(S, x1, x2) : Adaptor3d_HSurfaceTool::NbSamplesV(S));
        prTS(*S);)
    }

  // -- Adaptor3d_HVertex
  for (int j = 0; j < 36; ++j) {
    double x = rv(), y = rv();
    int ori = j % 4;
    double res = (j % 3 == 0) ? 1e-8 : 0.125 * (j % 5);
    int ci = (j % 4 == 3) ? 12 + (j / 4) % 2 : j % 5;
    double dx = (j % 3 == 1) ? 1e-8 : ((j % 3 == 2) ? 1e-3 : 0.0);
    begin("hvertex"); F(x); F(y); I(ori); F(res); I(ci); F(dx); bar();
    CASE(
      occ::handle<TC> C = fresh2(ci);
      occ::handle<Adaptor3d_HVertex> v1 = new Adaptor3d_HVertex(gp_Pnt2d(x, y), (TopAbs_Orientation)ori, res);
      occ::handle<Adaptor3d_HVertex> v2 = new Adaptor3d_HVertex(gp_Pnt2d(x + dx, y), TopAbs_REVERSED, 0.5);
      P2(v1->Value()); I((int)v1->Orientation()); F(v1->Resolution(C)); F(v1->Parameter(C));
      B(v1->IsSame(v2)); B(v2->IsSame(v1));
      prTC(*C);)
  }

  // -- Adaptor3d_InterFunc
  for (int j = 0; j < 40; ++j) {
    int ci = j % 5 == 4 ? 5 + (j / 5) % 6 : j % 4;
    int fix = (j % 9 == 8) ? 3 : 1 + j % 2;
    double fv = 0.25 * (j % 7) - 0.5;
    double x = -0.25 + 0.0625 * (j % 11);
    begin("interfunc"); I(ci); I(fix); F(fv); F(x); bar();
    CASE(
      occ::handle<TC> C = fresh2(ci);
      Adaptor3d_InterFunc f(C, fv, fix);
      double a, b, c;
      B(f.Value(x, a)); F(a);
      B(f.Derivative(x, b)); F(b);
      B(f.Values(x, c, b)); F(c); F(b);
      prTC(*C);)
  }
  return 0;
}
