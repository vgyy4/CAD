// Deterministic reference records for OCCT 8.0.1's package Geom2dAdaptor (Geom2dAdaptor_Curve, Geom2dAdaptor::MakeCurve),
// see docs/api-geom2dadaptor.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal. A curve is printed as `kind params`:
//   0 line `px py dx dy`; 1 circle `px py dx dy sense r`; 2 ellipse `px py dx dy sense major minor`; 3 hyperbola (same);
//   4 parabola `px py dx dy sense focal`; 5 Bezier `n x y ... hasW [w ...]`;
//   6 B-spline `degree periodic nk knots... nk mults... np x y ... hasW [w ...]`;
//   7 offset `offset basisKind params`; 8 trimmed `u1 u2 basisKind params` (a basis is any of kinds 0..6).
#include <Adaptor2d_Curve2d.hxx>
#include <Geom2dAdaptor.hxx>
#include <Geom2dAdaptor_Curve.hxx>
#include <Geom2d_BSplineCurve.hxx>
#include <Geom2d_BezierCurve.hxx>
#include <Geom2d_Circle.hxx>
#include <Geom2d_Curve.hxx>
#include <Geom2d_Ellipse.hxx>
#include <Geom2d_Hyperbola.hxx>
#include <Geom2d_Line.hxx>
#include <Geom2d_OffsetCurve.hxx>
#include <Geom2d_Parabola.hxx>
#include <Geom2d_TrimmedCurve.hxx>
#include <GeomAbs_CurveType.hxx>
#include <GeomAbs_Shape.hxx>
#include <NCollection_Array1.hxx>
#include <Standard_Failure.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Vec2d.hxx>
#include <cmath>
#include <exception>
#include <variant>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

// ---- output -------------------------------------------------------------------------------------
static std::string line, sig, want;
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
  if (inputs) sig += 'f';
}
static void I(long long v) {
  line += ' ' + std::to_string(v);
  if (inputs) sig += 'i';
}
static void B(bool v) { I(v ? 1 : 0); }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void begin(const char* name) {
  line = name;
  sig.clear();
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

static uint64_t S = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S ^= S << 13;
  S ^= S >> 7;
  S ^= S << 17;
  return (double)(S >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)

static void P2(const gp_Pnt2d& p) { F(p.X()); F(p.Y()); }
static void V2(const gp_Vec2d& v) { F(v.X()); F(v.Y()); }
static void D2(const gp_Dir2d& d) { F(d.X()); F(d.Y()); }
static void A22(const gp_Ax22d& a) { P2(a.Location()); D2(a.XDirection()); D2(a.YDirection()); }
static void PrLin(const gp_Lin2d& l) { P2(l.Location()); D2(l.Direction()); }
static void PrCirc(const gp_Circ2d& c) { A22(c.Axis()); F(c.Radius()); }
static void PrElips(const gp_Elips2d& e) { A22(e.Axis()); F(e.MajorRadius()); F(e.MinorRadius()); }
static void PrHypr(const gp_Hypr2d& h) { A22(h.Axis()); F(h.MajorRadius()); F(h.MinorRadius()); }
static void PrParab(const gp_Parab2d& p) { A22(p.Axis()); F(p.Focal()); }

// ---- curve generation (printing the inputs) -----------------------------------------------------------------------------------
static int kindOf(const occ::handle<Geom2d_Curve>& c) {
  if (c.IsNull()) return -1;
  if (c->IsKind(STANDARD_TYPE(Geom2d_Line))) return 0;
  if (c->IsKind(STANDARD_TYPE(Geom2d_Circle))) return 1;
  if (c->IsKind(STANDARD_TYPE(Geom2d_Ellipse))) return 2;
  if (c->IsKind(STANDARD_TYPE(Geom2d_Hyperbola))) return 3;
  if (c->IsKind(STANDARD_TYPE(Geom2d_Parabola))) return 4;
  if (c->IsKind(STANDARD_TYPE(Geom2d_BezierCurve))) return 5;
  if (c->IsKind(STANDARD_TYPE(Geom2d_BSplineCurve))) return 6;
  if (c->IsKind(STANDARD_TYPE(Geom2d_OffsetCurve))) return 7;
  if (c->IsKind(STANDARD_TYPE(Geom2d_TrimmedCurve))) return 8;
  return 9;
}
static occ::handle<Geom2d_Curve> genBase(int j, int kind) {
  I(kind);
  if (kind <= 4) {
    double px = rv(), py = rv(), dx = rv(), dy = rv();
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    P2(gp_Pnt2d(px, py)); F(dx); F(dy);
    gp_Pnt2d p(px, py);
    gp_Dir2d d(dx, dy);
    if (kind == 0) return new Geom2d_Line(p, d);
    bool sense = (j % 4 != 3);
    B(sense);
    double r1 = 0.5 + std::abs(rv()), r2 = 0.25 + std::abs(rv()) * 0.25;
    if (kind == 1) { F(r1); return new Geom2d_Circle(gp_Ax2d(p, d), r1, sense); }
    if (kind == 4) { F(r2); return new Geom2d_Parabola(gp_Ax2d(p, d), r2, sense); }
    double major = r1 + r2;
    F(major); F(r2);
    if (kind == 2) return new Geom2d_Ellipse(gp_Ax2d(p, d), major, r2, sense);
    return new Geom2d_Hyperbola(gp_Ax2d(p, d), major, r2, sense);
  }
  if (kind == 5) {
    int n = 2 + j % 4;
    NCollection_Array1<gp_Pnt2d> poles(1, n);
    I(n);
    for (int i = 1; i <= n; ++i) { poles(i) = gp_Pnt2d(rv(), rv()); P2(poles(i)); }
    bool hasW = (j % 3 == 1);
    B(hasW);
    if (!hasW) return new Geom2d_BezierCurve(poles);
    NCollection_Array1<double> w(1, n);
    for (int i = 1; i <= n; ++i) { w(i) = 0.5 + std::abs(rv()); F(w(i)); }
    return new Geom2d_BezierCurve(poles, w);
  }
  // B-spline
  bool periodic = (j % 5 == 2);
  int degree = 1 + (j / 3) % (periodic ? 3 : 4);
  int k = periodic ? 4 + (j / 5) % 3 : 2 + (j / 5) % 4;
  NCollection_Array1<double> kn(1, k);
  NCollection_Array1<int> mu(1, k);
  double v = rv() - 2.0;
  int sum = 0;
  for (int i = 1; i <= k; ++i) {
    kn(i) = v;
    v += 0.5 + std::abs(rv()) * 0.5;
    mu(i) = periodic ? 1 : ((i == 1 || i == k) ? degree + 1 : 1 + (j + i) % degree);
    sum += mu(i);
  }
  int np = periodic ? sum - mu(k) : sum - degree - 1;
  I(degree); B(periodic); I(k);
  for (int i = 1; i <= k; ++i) F(kn(i));
  I(k);
  for (int i = 1; i <= k; ++i) I(mu(i));
  NCollection_Array1<gp_Pnt2d> poles(1, np);
  I(np);
  for (int i = 1; i <= np; ++i) { poles(i) = gp_Pnt2d(rv(), rv()); P2(poles(i)); }
  bool hasW = (j % 3 == 2);
  B(hasW);
  if (!hasW) return new Geom2d_BSplineCurve(poles, kn, mu, degree, periodic);
  NCollection_Array1<double> w(1, np);
  for (int i = 1; i <= np; ++i) { w(i) = 0.5 + std::abs(rv()); F(w(i)); }
  return new Geom2d_BSplineCurve(poles, w, kn, mu, degree, periodic);
}
// a top-level curve (kind 0..8)
static occ::handle<Geom2d_Curve> genCurve(int j, int kind) {
  if (kind <= 6) return genBase(j, kind);
  I(kind);
  if (kind == 7) {
    static const double OFF[5] = {0.5, -0.75, 1.0, 0.125, -1.5};
    double off = OFF[j % 5];
    F(off);
    occ::handle<Geom2d_Curve> b = genBase(j, (j / 7) % 7);
    return new Geom2d_OffsetCurve(b, off);
  }
  // trimmed: the basis is printed after the bounds, so generate the basis first into a side buffer
  std::string keepLine = line, keepSig = sig;
  line.clear();
  sig.clear();
  occ::handle<Geom2d_Curve> b = genBase(j, (j / 7) % 7);
  std::string baseLine = line, baseSig = sig;
  line = keepLine;
  sig = keepSig;
  double f = b->FirstParameter(), l = b->LastParameter();
  double u1, u2;
  if (std::abs(f) >= 1e50 || std::abs(l) >= 1e50) { u1 = -1.0 + 0.25 * (j % 3); u2 = u1 + 1.5 + 0.25 * (j % 2); }
  else if (b->IsPeriodic()) { u1 = f + 0.125 * (j % 4); u2 = u1 + (l - f) * 0.5 + 0.0625 * (j % 3); }
  else { u1 = f + (l - f) * 0.125; u2 = l - (l - f) * 0.25; }
  F(u1); F(u2);
  line += baseLine;
  sig += baseSig;
  return new Geom2d_TrimmedCurve(b, u1, u2);
}

// begins a record and generates its curve; a curve the constructors refuse is printed as kind -2 and the case throws
static bool mk(const char* name, int j, int kind, occ::handle<Geom2d_Curve>& c) {
  begin(name);
  try {
    c = genCurve(j, kind);
    return true;
  } catch (Standard_Failure&) {
    line = name;
    sig.clear();
    I(-2);
    c.Nullify();
    return false;
  }
}
// A parameter range inside the parameter range of the curve (the library does not check ranges beyond it: its B-spline
// span search then reads outside the knots). Curves with an infinite range keep the given values.
static void inRange(const occ::handle<Geom2d_Curve>& c, double& u1, double& u2) {
  if (c.IsNull()) return;
  double f = c->FirstParameter(), l = c->LastParameter();
  if (std::abs(f) >= 1e50 || std::abs(l) >= 1e50) return;
  double a = std::abs(rv()) * 0.5, b = std::abs(rv()) * 0.5;
  u1 = f + (l - f) * 0.5 * a;
  u2 = l - (l - f) * 0.5 * b;
}
static const GeomAbs_Shape SH[7] = {GeomAbs_C0, GeomAbs_G1, GeomAbs_C1, GeomAbs_G2, GeomAbs_C2, GeomAbs_C3, GeomAbs_CN};

// the top-level kinds in rotation, so that every kind is met in each family
static int pickKind(int j, int nbKinds = 9) { return j % nbKinds; }

int main() {
  // -- loading ------------------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 54; ++j) {
    int kind = pickKind(j);
    occ::handle<Geom2d_Curve> c;
    mk("load", j, kind, c);
    int v = j % 6;  // 0 ctor(C), 1 ctor(C,u1,u2), 2 default + Load(C), 3 Load(C,u1,u2), 4 ctor(null), 5 Load(C,u1,u2) with u1 > u2
    double u1 = rv(), u2 = u1 + 0.5 + std::abs(rv());
    if (v == 5) std::swap(u1, u2);
    I(v); F(u1); F(u2);
    bar();
    CASE(
      Geom2dAdaptor_Curve a;
      if (v == 0) a = Geom2dAdaptor_Curve(c);
      else if (v == 1) a = Geom2dAdaptor_Curve(c, u1, u2);
      else if (v == 2) a.Load(c);
      else if (v == 3 || v == 5) a.Load(c, u1, u2);
      else a.Load(occ::handle<Geom2d_Curve>());
      B(a.IsInitialized()); I((int)a.GetType()); F(a.FirstParameter()); F(a.LastParameter()); I(kindOf(a.Curve()));)
  }
  for (int j = 0; j < 9; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("reset", j, j % 9, c);
    bar();
    CASE(Geom2dAdaptor_Curve a(c); a.Reset(); B(a.IsInitialized()); I((int)a.GetType()); F(a.FirstParameter()); F(a.LastParameter()); I(kindOf(a.Curve()));)
  }

  // -- continuity and intervals -------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 54; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("continuity", j, pickKind(j), c);
    double u1 = rv(), u2 = u1 + 0.5 + std::abs(rv());
    int ranged = (j % 3 == 0);
    I(ranged); F(u1); F(u2);
    bar();
    CASE(Geom2dAdaptor_Curve a = ranged ? Geom2dAdaptor_Curve(c, u1, u2) : Geom2dAdaptor_Curve(c); I((int)a.Continuity());)
  }
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 63; ++j) {
      occ::handle<Geom2d_Curve> c;
      mk(fam == 0 ? "nb_intervals" : "intervals", j, pickKind(j + fam), c);
      int si = j % 7, ranged = (j % 3 == 1);
      double u1 = rv(), u2 = u1 + 0.5 + std::abs(rv());
      I(si); I(ranged); F(u1); F(u2);
      bar();
      if (fam == 0) CASE(Geom2dAdaptor_Curve a = ranged ? Geom2dAdaptor_Curve(c, u1, u2) : Geom2dAdaptor_Curve(c); I(a.NbIntervals(SH[si]));)
      else CASE(
        Geom2dAdaptor_Curve a = ranged ? Geom2dAdaptor_Curve(c, u1, u2) : Geom2dAdaptor_Curve(c);
        int n = a.NbIntervals(SH[si]);
        NCollection_Array1<double> t(1, n + 1);
        t.Init(-7.0);
        a.Intervals(t, SH[si]);
        I(n);
        for (int i = 1; i <= n + 1; ++i) F(t(i));)
    }

  // -- closed / periodic / period / samples / type information -----------------------------------------------------------------------------------
  for (int j = 0; j < 54; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("closed", j, pickKind(j), c);
    bar();
    CASE(Geom2dAdaptor_Curve a(c); B(a.IsClosed()); B(a.IsPeriodic()); F(a.Period());)
  }
  for (int j = 0; j < 54; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("nb_samples", j, pickKind(j), c);
    bar();
    CASE(Geom2dAdaptor_Curve a(c); I(a.NbSamples());)
  }
  for (int j = 0; j < 63; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("resolution", j, pickKind(j), c);
    double r = (j % 2) ? 1e-3 : 0.375;
    if (j % 9 == 4) r = 5.0;  // larger than the radius of some circles
    F(r);
    bar();
    CASE(Geom2dAdaptor_Curve a(c); F(a.Resolution(r));)
  }
  for (int j = 0; j < 72; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("info", j, pickKind(j), c);
    int op = (j / 9) % 8;
    I(op);
    bar();
    CASE(
      Geom2dAdaptor_Curve a(c);
      switch (op) {
        case 0: I(a.Degree()); break;
        case 1: B(a.IsRational()); break;
        case 2: I(a.NbPoles()); break;
        case 3: I(a.NbKnots()); break;
        case 4: { auto b = a.Bezier(); I(b.IsNull()); if (!b.IsNull()) { I(b->NbPoles()); I(b->Degree()); } break; }
        case 5: { auto b = a.BSpline(); I(b.IsNull()); if (!b.IsNull()) { I(b->NbKnots()); I(b->Degree()); } break; }
        case 6: PrLin(a.Line()); break;
        default: PrCirc(a.Circle()); break;
      })
  }
  for (int j = 0; j < 30; ++j) {
    int kind = 1 + j % 4;
    occ::handle<Geom2d_Curve> c;
    mk("conic", j, kind, c);
    int wrong = (j / 4) % 2;  // ask for the right accessor, or the next one
    I(wrong);
    bar();
    CASE(
      Geom2dAdaptor_Curve a(c);
      int which = wrong ? (kind % 4) + 1 : kind;
      switch (which) {
        case 1: PrCirc(a.Circle()); break;
        case 2: PrElips(a.Ellipse()); break;
        case 3: PrHypr(a.Hyperbola()); break;
        default: PrParab(a.Parabola()); break;
      })
  }

  // -- evaluation ---------------------------------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 270; ++j) {
    int kind = pickKind(j);
    occ::handle<Geom2d_Curve> c;
    mk("eval", j, kind, c);
    int op = (j / 9) % 12;  // Value D0 D1 D2 D3 DN EvalD0 EvalD1 EvalD2 EvalD3 EvalDN D1 on the boundary
    int n = (j / 3) % 5;
    int ranged = (j % 4 == 1);
    double u1 = rv() * 0.25, u2 = u1 + 0.5 + std::abs(rv()) * 0.25;
    inRange(c, u1, u2);
    double ua = 0.0;
    int sel = (j / 2) % 5;
    I(op); I(n); I(ranged); F(u1); F(u2); I(sel);
    bar();
    CASE(
      Geom2dAdaptor_Curve a = ranged ? Geom2dAdaptor_Curve(c, u1, u2) : Geom2dAdaptor_Curve(c);
      double f = a.FirstParameter(), l = a.LastParameter();
      double fr = (std::abs(f) > 1e50 || std::abs(l) > 1e50) ? 0.0 : 1.0;
      switch (sel) {
        case 0: ua = f; break;
        case 1: ua = l; break;
        case 2: ua = fr ? f + (l - f) * 0.4375 : 0.4375; break;
        case 3: ua = fr ? f + (l - f) * 0.8125 : -0.8125; break;
        default: ua = fr ? f + (l - f) * 0.0625 : 1.0625; break;
      }
      F(ua);
      gp_Pnt2d p; gp_Vec2d v1, v2, v3;
      switch (op) {
        case 0: P2(a.Value(ua)); break;
        case 1: a.D0(ua, p); P2(p); break;
        case 2: case 11: a.D1(ua, p, v1); P2(p); V2(v1); break;
        case 3: a.D2(ua, p, v1, v2); P2(p); V2(v1); V2(v2); break;
        case 4: a.D3(ua, p, v1, v2, v3); P2(p); V2(v1); V2(v2); V2(v3); break;
        case 5: V2(a.DN(ua, n)); break;
        case 6: P2(a.EvalD0(ua)); break;
        case 7: { auto r = a.EvalD1(ua); P2(r.Point); V2(r.D1); break; }
        case 8: { auto r = a.EvalD2(ua); P2(r.Point); V2(r.D1); V2(r.D2); break; }
        case 9: { auto r = a.EvalD3(ua); P2(r.Point); V2(r.D1); V2(r.D2); V2(r.D3); break; }
        default: V2(a.EvalDN(ua, n)); break;
      })
  }
  // evaluation through many parameters of one adaptor (the cache is reused and rebuilt)
  for (int j = 0; j < 27; ++j) {
    int kind = 5 + j % 2;  // Bezier or B-spline
    occ::handle<Geom2d_Curve> c;
    mk("eval_sweep", j, kind, c);
    int ranged = (j % 3 == 2);
    double u1 = rv() * 0.25, u2 = u1 + 0.75 + std::abs(rv()) * 0.25;
    inRange(c, u1, u2);
    I(ranged); F(u1); F(u2);
    bar();
    CASE(
      Geom2dAdaptor_Curve a = ranged ? Geom2dAdaptor_Curve(c, u1, u2) : Geom2dAdaptor_Curve(c);
      double f = a.FirstParameter(), l = a.LastParameter();
      for (int k = 0; k <= 12; ++k) {
        double u = f + (l - f) * (k / 12.0);
        if (k % 3 == 1) u = f + (l - f) * ((12 - k) / 12.0);
        gp_Pnt2d p; gp_Vec2d v1, v2, v3;
        a.D3(u, p, v1, v2, v3);
        F(u); P2(p); V2(v1); V2(v2); V2(v3);
      })
  }

  // -- trim, shallow copy -----------------------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 45; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("trim_copy", j, pickKind(j), c);
    int op = j % 3;  // 0 Trim, 1 ShallowCopy, 2 ShallowCopy of a ranged adaptor
    double f = rv() * 0.25, l = f + 0.5 + std::abs(rv()) * 0.25;
    inRange(c, f, l);
    I(op); F(f); F(l);
    bar();
    CASE(
      Geom2dAdaptor_Curve a = (op == 2) ? Geom2dAdaptor_Curve(c, f, l) : Geom2dAdaptor_Curve(c);
      occ::handle<Adaptor2d_Curve2d> h = (op == 0) ? a.Trim(f, l, 1e-7) : a.ShallowCopy();
      occ::handle<Geom2dAdaptor_Curve> hh = occ::down_cast<Geom2dAdaptor_Curve>(h);
      I(hh.IsNull()); I((int)h->GetType()); F(h->FirstParameter()); F(h->LastParameter()); I(kindOf(hh->Curve()));
      double u = 0.5 * (h->FirstParameter() + h->LastParameter());
      if (std::abs(h->FirstParameter()) > 1e50 || std::abs(h->LastParameter()) > 1e50) u = 0.25;
      P2(h->Value(u));)
  }

  // -- MakeCurve ----------------------------------------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 54; ++j) {
    occ::handle<Geom2d_Curve> c;
    mk("make_curve", j, pickKind(j), c);
    int ranged = (j % 3 != 0);
    double u1 = rv() * 0.25, u2 = u1 + 0.5 + std::abs(rv()) * 0.25;
    I(ranged); F(u1); F(u2);
    bar();
    CASE(
      Geom2dAdaptor_Curve a = ranged ? Geom2dAdaptor_Curve(c, u1, u2) : Geom2dAdaptor_Curve(c);
      occ::handle<Geom2d_Curve> m = Geom2dAdaptor::MakeCurve(a);
      I(kindOf(m)); F(m->FirstParameter()); F(m->LastParameter());
      double f = m->FirstParameter(), l = m->LastParameter();
      double u = (std::abs(f) > 1e50 || std::abs(l) > 1e50) ? 0.25 : 0.5 * (f + l);
      P2(m->Value(u));
      if (kindOf(m) == 8) I(kindOf(occ::down_cast<Geom2d_TrimmedCurve>(m)->BasisCurve()));)
  }
  return 0;
}
