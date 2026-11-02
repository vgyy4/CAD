// Deterministic reference records for OCCT 8.0.1's package Geom2dGridEval (the grid evaluators of the 2D curves), see
// docs/api-geom2dgrideval.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal. A curve is printed as `kind params` (kind -1: a null handle, kind -2: the constructors refused it):
//   0 line `px py dx dy`; 1 circle `px py dx dy sense r`; 2 ellipse `px py dx dy sense major minor`; 3 hyperbola (same);
//   4 parabola `px py dx dy sense focal`; 5 Bezier `n x y ... hasW [w ...]`;
//   6 B-spline `degree periodic nk knots... nk mults... np x y ... hasW [w ...]`;
//   7 offset `offset <curve>`; 8 trimmed `u1 u2 <curve>` (the inner <curve> is printed in full, recursively).
// A parameter array is printed as its length and its values; its lower bound (not printed: no result depends on it) is
// varied. A result grid is printed as its length and the items (point, then the derivatives).
#include <Adaptor2d_Curve2d.hxx>
#include <BSplCLib.hxx>
#include <Adaptor2d_Line2d.hxx>
#include <Adaptor2d_OffsetCurve.hxx>
#include <Geom2dAdaptor_Curve.hxx>
#include <Geom2dGridEval.hxx>
#include <Geom2dGridEval_BSplineCurve.hxx>
#include <Geom2dGridEval_BezierCurve.hxx>
#include <Geom2dGridEval_Circle.hxx>
#include <Geom2dGridEval_Curve.hxx>
#include <Geom2dGridEval_Ellipse.hxx>
#include <Geom2dGridEval_Hyperbola.hxx>
#include <Geom2dGridEval_Line.hxx>
#include <Geom2dGridEval_OffsetCurve.hxx>
#include <Geom2dGridEval_OtherCurve.hxx>
#include <Geom2dGridEval_Parabola.hxx>
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
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
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

// ---- nested curves --------------------------------------------------------------------------------------------------------------
static occ::handle<Geom2d_Curve> genT(int j, int T, int depth = 0);
static occ::handle<Geom2d_Curve> genOff(int j, int innerT, int depth) {
  static const double OFF[5] = {0.5, -0.75, 1.0, 0.125, -1.5};
  I(7);
  double off = OFF[(j + depth) % 5];
  F(off);
  occ::handle<Geom2d_Curve> b = genT(j, innerT, depth + 1);
  return new Geom2d_OffsetCurve(b, off);
}
static occ::handle<Geom2d_Curve> genTrim(int j, int innerT, int depth) {
  I(8);
  // the inner curve is printed after the bounds, so generate it first into a side buffer
  std::string keepLine = line, keepSig = sig;
  line.clear();
  sig.clear();
  occ::handle<Geom2d_Curve> b = genT(j, innerT, depth + 1);
  std::string innerLine = line, innerSig = sig;
  line = keepLine;
  sig = keepSig;
  double f = b->FirstParameter(), l = b->LastParameter();
  double u1, u2;
  if (std::abs(f) >= 1e50 || std::abs(l) >= 1e50) { u1 = -1.0 + 0.25 * (j % 3); u2 = u1 + 1.5 + 0.25 * (j % 2); }
  else if (b->IsPeriodic()) { u1 = f + 0.125 * (j % 4); u2 = u1 + (l - f) * 0.5 + 0.0625 * (j % 3); }
  else { u1 = f + (l - f) * 0.125; u2 = l - (l - f) * 0.25; }
  F(u1); F(u2);
  line += innerLine;
  sig += innerSig;
  return new Geom2d_TrimmedCurve(b, u1, u2);
}
// T: 0..6 base, 7 offset(base), 8 trimmed(base), 9 offset(trimmed(base)), 10 offset(offset(base)), 11 trimmed(offset(base)),
// 12 trimmed(trimmed(base))
static occ::handle<Geom2d_Curve> genT(int j, int T, int depth) {
  int b = (j / 7) % 7;
  if (T <= 6) return genBase(j, T);
  if (T == 7) return genOff(j, b, depth);
  if (T == 8) return genTrim(j, b, depth);
  if (T == 9) return genOff(j, 8, depth);
  if (T == 10) return genOff(j, 7, depth);
  if (T == 11) return genTrim(j, 7, depth);
  return genTrim(j, 8, depth);
}
// begins a record and generates its curve; T < 0 is the null handle (kind -1); a curve the constructors refuse is printed as
// kind -2 and the case throws
static bool mk(const char* name, int j, int T, occ::handle<Geom2d_Curve>& c) {
  begin(name);
  c.Nullify();
  if (T < 0) {
    I(-1);
    return true;
  }
  try {
    c = genT(j, T);
    return true;
  } catch (Standard_Failure&) {
    line = name;
    sig.clear();
    I(-2);
    I(j);
    c.Nullify();
    return false;
  }
}
#define MK(name, j, T, c) \
  occ::handle<Geom2d_Curve> c; \
  const bool ok_ = mk(name, j, T, c);

// ---- parameter arrays -------------------------------------------------------------------------------------------------------------
static void baseOf(const occ::handle<Geom2d_Curve>& c, occ::handle<Geom2d_Curve>& b) {
  b = c;
  while (!b.IsNull()) {
    occ::handle<Geom2d_TrimmedCurve> t = occ::down_cast<Geom2d_TrimmedCurve>(b);
    occ::handle<Geom2d_OffsetCurve> o = occ::down_cast<Geom2d_OffsetCurve>(b);
    if (!t.IsNull()) b = t->BasisCurve();
    else if (!o.IsNull()) b = o->BasisCurve();
    else break;
  }
}
// The parameter range of the innermost basis curve (the grid evaluators ignore trimming); `ext` widens the range of the curves
// that allow it (a Bezier curve, a periodic B-spline curve, the trigonometric conics).
static void domainOf(const occ::handle<Geom2d_Curve>& c, int ext, double& f, double& l) {
  f = -2.0;
  l = 2.0;
  occ::handle<Geom2d_Curve> b;
  baseOf(c, b);
  if (b.IsNull()) return;
  double bf = b->FirstParameter(), bl = b->LastParameter();
  if (std::abs(bf) >= 1e50 || std::abs(bl) >= 1e50) return;
  f = bf;
  l = bl;
  bool trig = b->IsKind(STANDARD_TYPE(Geom2d_Circle)) || b->IsKind(STANDARD_TYPE(Geom2d_Ellipse));
  if (trig) { f -= 3.0; l += 3.0; }
  else if (ext && b->IsKind(STANDARD_TYPE(Geom2d_BezierCurve))) { f -= 0.25; l += 0.25; }
  else if (ext && b->IsKind(STANDARD_TYPE(Geom2d_BSplineCurve)) && b->IsPeriodic()) {
    double w = (bl - bf) * 0.5;
    f -= w;
    l += w;
  }
}
// The B-spline grid evaluator passes the span of the flat knots to BSplCLib::DN together with the knots and the multiplicities, which
// reads past the multiplicities when the span index equals their number: such parameters are left out of derivative grids.
static bool dnUnsafe(const occ::handle<Geom2d_Curve>& c, double u) {
  occ::handle<Geom2d_Curve> b;
  baseOf(c, b);
  occ::handle<Geom2d_BSplineCurve> bs = occ::down_cast<Geom2d_BSplineCurve>(b);
  if (bs.IsNull() || bs->IsPeriodic()) return false;
  int idx = 0;
  double adj = u;
  BSplCLib::LocateParameter(bs->Degree(), bs->KnotSequence(), BSplCLib::NoMults(), u, false, idx, adj);
  return idx >= bs->NbKnots();
}
// prints the parameters and returns them as an array (an empty one for zero); with `fc` (a curve whose derivative grid is
// asked for) the parameters that `dnUnsafe` rejects are dropped
static NCollection_Array1<double> mkParams(double f, double l, int j, int forceMode = -1, bool allowEmpty = true,
                                           const occ::handle<Geom2d_Curve>* fc = nullptr) {
  int mode = forceMode >= 0 ? forceMode : j % 4;
  std::vector<double> v;
  if (allowEmpty && j % 17 == 5) {
  } else if (mode == 0 || mode == 1) {
    int n = mode == 0 ? 3 + j % 7 : 9 + j % 6;
    for (int i = 0; i < n; ++i) v.push_back(f + (l - f) * ((double)i / (double)(n - 1)));
  } else if (mode == 2) {
    int n = 2 + j % 8;
    for (int i = 0; i < n; ++i) v.push_back(f + (l - f) * rnd());
  } else {
    double c0 = f + (l - f) * 0.9 * rnd(), w = (l - f) * 0.03;
    for (int i = 0; i < 8; ++i) v.push_back(c0 + w * ((double)i / 7.0));
    for (int i = 0; i < 3; ++i) v.push_back(f + (l - f) * rnd());
  }
  if (j % 5 == 0 && v.size() >= 2) { v.front() = f; v.back() = l; }
  if (fc) {
    std::vector<double> w;
    for (double x : v) if (!dnUnsafe(*fc, x)) w.push_back(x);
    v = w;
  }
  I((long long)v.size());
  for (double x : v) F(x);
  if (v.empty()) return NCollection_Array1<double>();
  int lo = (j % 5) - 2;
  NCollection_Array1<double> a(lo, lo + (int)v.size() - 1);
  for (size_t i = 0; i < v.size(); ++i) a(lo + (int)i) = v[i];
  return a;
}
static const int NS[10] = {1, 2, 3, 4, 5, 6, 0, -1, 9, 7};
// prints the operation (0 D0 .. 3 D3, 4 DN) and its order
static void mkOp(int op, int& n, int& dnCount) {
  n = 0;
  if (op == 4) n = NS[dnCount++ % 10];
  I(op);
  I(n);
}
template <class E>
static void runOp(const E& e, int op, int n, const NCollection_Array1<double>& ps) {
  if (op == 0) {
    NCollection_Array1<gp_Pnt2d> r = e.EvaluateGrid(ps);
    I(r.Length());
    for (int i = r.Lower(); i <= r.Upper(); ++i) P2(r(i));
  } else if (op == 1) {
    NCollection_Array1<Geom2dGridEval::CurveD1> r = e.EvaluateGridD1(ps);
    I(r.Length());
    for (int i = r.Lower(); i <= r.Upper(); ++i) { P2(r(i).Point); V2(r(i).D1); }
  } else if (op == 2) {
    NCollection_Array1<Geom2dGridEval::CurveD2> r = e.EvaluateGridD2(ps);
    I(r.Length());
    for (int i = r.Lower(); i <= r.Upper(); ++i) { P2(r(i).Point); V2(r(i).D1); V2(r(i).D2); }
  } else if (op == 3) {
    NCollection_Array1<Geom2dGridEval::CurveD3> r = e.EvaluateGridD3(ps);
    I(r.Length());
    for (int i = r.Lower(); i <= r.Upper(); ++i) { P2(r(i).Point); V2(r(i).D1); V2(r(i).D2); V2(r(i).D3); }
  } else {
    NCollection_Array1<gp_Vec2d> r = e.EvaluateGridDN(ps, n);
    I(r.Length());
    for (int i = r.Lower(); i <= r.Upper(); ++i) V2(r(i));
  }
}
// a refused curve: the case throws
#define REFUSED() \
  if (!ok_) { bar(); CASE(throw Standard_ConstructionError();) continue; }

// A direct evaluator class of one curve kind: `FAM` is the family, `KIND` the kind of the curve, `TY` the Geom2d class.
#define DIRECT(FAM, KIND, TY, EV, NJ) \
  { \
    int dn_ = 0; \
    for (int j = 0; j < NJ; ++j) { \
      MK(FAM, j, (j % 23 == 11) ? -1 : KIND, c); \
      REFUSED() \
      double f, l; \
      domainOf(c, j % 3 == 0, f, l); \
      int op = j % 5, n; \
      NCollection_Array1<double> ps = mkParams(f, l, j, -1, true, op == 4 ? &c : nullptr); \
      mkOp(op, n, dn_); \
      bar(); \
      CASE(EV e(occ::down_cast<TY>(c)); runOp(e, op, n, ps);) \
    } \
  }

int main() {
  DIRECT("line_grid", 0, Geom2d_Line, Geom2dGridEval_Line, 50)
  DIRECT("circle_grid", 1, Geom2d_Circle, Geom2dGridEval_Circle, 50)
  DIRECT("ellipse_grid", 2, Geom2d_Ellipse, Geom2dGridEval_Ellipse, 50)
  DIRECT("hyperbola_grid", 3, Geom2d_Hyperbola, Geom2dGridEval_Hyperbola, 50)
  DIRECT("parabola_grid", 4, Geom2d_Parabola, Geom2dGridEval_Parabola, 50)
  DIRECT("bezier_grid", 5, Geom2d_BezierCurve, Geom2dGridEval_BezierCurve, 50)
  DIRECT("bspline_grid", 6, Geom2d_BSplineCurve, Geom2dGridEval_BSplineCurve, 60)

  // -- offset curves: the basis of every kind (7), nested (9, 10, 11)
  {
    int dn = 0;
    for (int j = 0; j < 70; ++j) {
      int T = (j % 23 == 11) ? -1 : (j % 7 == 6 ? 9 + (j / 7) % 3 : 7);
      MK("offset_grid", j, T, c);
      REFUSED()
      double f, l;
      domainOf(c, j % 3 == 0, f, l);
      int op = j % 5, n;
      NCollection_Array1<double> ps = mkParams(f, l, j, -1, true, op == 4 ? &c : nullptr);
      mkOp(op, n, dn);
      bar();
      CASE(Geom2dGridEval_OffsetCurve e(occ::down_cast<Geom2d_OffsetCurve>(c)); runOp(e, op, n, ps);)
    }
  }

  // -- the fallback evaluator over adaptors: 0 Geom2dAdaptor_Curve, 1 Adaptor2d_Line2d, 2 Adaptor2d_OffsetCurve
  {
    int dn = 0;
    for (int j = 0; j < 60; ++j) {
      int v = j % 3;
      occ::handle<Geom2d_Curve> c;
      begin("other_grid");
      I(v);
      bool ok = true;
      double off = 0.0, dx = 0.0, dy = 0.0, u0 = 0.0, u1 = 0.0;
      gp_Pnt2d lp;
      try {
        if (v == 1) {
          lp = gp_Pnt2d(rv(), rv());
          dx = rv(); dy = rv();
          if (dx == 0.0 && dy == 0.0) dx = 1.0;
          u0 = -1.0 - std::abs(rv());
          u1 = 1.0 + std::abs(rv());
          P2(lp); F(dx); F(dy); F(u0); F(u1);
        } else {
          if (v == 2) {
            static const double OFF[5] = {0.5, -0.75, 1.0, 0.125, -1.5};
            off = OFF[j % 5];
            F(off);
          }
          c = genT(j, (j / 3) % 9);
        }
      } catch (Standard_Failure&) {
        ok = false;
      }
      if (!ok) {
        line = "other_grid";
        sig.clear();
        I(v);
        I(-2);
        I(j);
        bar();
        CASE(throw Standard_ConstructionError();)
        continue;
      }
      occ::handle<Adaptor2d_Curve2d> ad;
      double f = -2.0, l = 2.0;
      if (v == 1) {
        ad = new Adaptor2d_Line2d(lp, gp_Dir2d(dx, dy), u0, u1);
        f = u0; l = u1;
      } else {
        occ::handle<Geom2dAdaptor_Curve> ga = new Geom2dAdaptor_Curve(c);
        if (v == 0) {
          ad = ga;
        } else {
          ad = new Adaptor2d_OffsetCurve(ga, off);
        }
        f = ad->FirstParameter();
        l = ad->LastParameter();
        if (std::abs(f) >= 1e50 || std::abs(l) >= 1e50) { f = -2.0; l = 2.0; }
      }
      NCollection_Array1<double> ps = mkParams(f, l, j);
      int op = j % 5, n;
      mkOp(op, n, dn);
      bar();
      CASE(Geom2dGridEval_OtherCurve e(*ad); runOp(e, op, n, ps);)
    }
  }

  // -- the dispatcher over a handle: 13 kinds of curve and the null handle
  {
    int dn = 0;
    for (int j = 0; j < 91; ++j) {
      int T = (j % 29 == 28) ? -1 : j % 13;
      MK("curve_geom", j, T, c);
      REFUSED()
      double f, l;
      domainOf(c, j % 3 == 0, f, l);
      int op = j % 5, n;
      NCollection_Array1<double> ps = mkParams(f, l, j, -1, true, op == 4 ? &c : nullptr);
      mkOp(op, n, dn);
      bar();
      CASE(Geom2dGridEval_Curve e(c); I((int)e.GetType()); runOp(e, op, n, ps);)
    }
  }

  // -- the dispatcher over a Geom2dAdaptor_Curve: loaded (0), ranged (1), never loaded (2)
  {
    int dn = 0;
    for (int j = 0; j < 60; ++j) {
      int v = j % 6 == 5 ? 2 : j % 2;
      int T = j % 13;
      begin("curve_adaptor");
      I(v);
      occ::handle<Geom2d_Curve> c;
      bool ok = true;
      if (v != 2) {
        try {
          c = genT(j, T);
        } catch (Standard_Failure&) {
          ok = false;
        }
      }
      if (!ok) {
        line = "curve_adaptor";
        sig.clear();
        I(v);
        I(-2);
        I(j);
        bar();
        CASE(throw Standard_ConstructionError();)
        continue;
      }
      double f, l;
      domainOf(c, 0, f, l);
      double r1 = f + (l - f) * 0.25, r2 = f + (l - f) * 0.75;
      if (v == 1) { F(r1); F(r2); }
      int op = j % 5, n;
      NCollection_Array1<double> ps = mkParams(f, l, j, -1, true, op == 4 ? &c : nullptr);
      mkOp(op, n, dn);
      bar();
      CASE(
        Geom2dAdaptor_Curve a;
        if (v == 0) a.Load(c);
        else if (v == 1) a.Load(c, r1, r2);
        Geom2dGridEval_Curve e(a);
        I((int)e.GetType());
        runOp(e, op, n, ps);)
    }
  }

  // -- the dispatcher over other adaptors: 0 Adaptor2d_Line2d, 1 Adaptor2d_OffsetCurve (of a Geom2dAdaptor_Curve)
  {
    int dn = 0;
    for (int j = 0; j < 40; ++j) {
      int v = j % 2;
      begin("curve_other");
      I(v);
      occ::handle<Geom2d_Curve> c;
      double off = 0.0, dx = 0.0, dy = 0.0, u0 = 0.0, u1 = 0.0;
      gp_Pnt2d lp;
      bool ok = true;
      try {
        if (v == 0) {
          lp = gp_Pnt2d(rv(), rv());
          dx = rv(); dy = rv();
          if (dx == 0.0 && dy == 0.0) dx = 1.0;
          u0 = -1.0 - std::abs(rv());
          u1 = 1.0 + std::abs(rv());
          P2(lp); F(dx); F(dy); F(u0); F(u1);
        } else {
          static const double OFF[5] = {0.5, -0.75, 1.0, 0.125, -1.5};
          off = OFF[j % 5];
          F(off);
          c = genT(j, (j / 2) % 9);
        }
      } catch (Standard_Failure&) {
        ok = false;
      }
      if (!ok) {
        line = "curve_other";
        sig.clear();
        I(v);
        I(-2);
        I(j);
        bar();
        CASE(throw Standard_ConstructionError();)
        continue;
      }
      occ::handle<Adaptor2d_Curve2d> ad;
      double f = -2.0, l = 2.0;
      if (v == 0) {
        ad = new Adaptor2d_Line2d(lp, gp_Dir2d(dx, dy), u0, u1);
        f = u0; l = u1;
      } else {
        occ::handle<Geom2dAdaptor_Curve> ga = new Geom2dAdaptor_Curve(c);
        ad = new Adaptor2d_OffsetCurve(ga, off);
        f = ad->FirstParameter();
        l = ad->LastParameter();
        if (std::abs(f) >= 1e50 || std::abs(l) >= 1e50) { f = -2.0; l = 2.0; }
      }
      NCollection_Array1<double> ps = mkParams(f, l, j);
      int op = j % 5, n;
      mkOp(op, n, dn);
      bar();
      CASE(Geom2dGridEval_Curve e(*ad); I((int)e.GetType()); runOp(e, op, n, ps);)
    }
  }

  // -- the curve type of the dispatcher (and one evaluation of two points)
  for (int j = 0; j < 26; ++j) {
    int T = (j % 13 == 12 && j >= 13) ? -1 : j % 13;
    MK("curve_type", j, T, c);
    REFUSED()
    double f, l;
    domainOf(c, 0, f, l);
    NCollection_Array1<double> ps = mkParams(f, l, 1, 0, false);
    bar();
    CASE(Geom2dGridEval_Curve e(c); I((int)e.GetType()); runOp(e, 0, 0, ps);)
  }

  // -- derivatives of every order 1..8 of every kind
  for (int j = 0; j < 104; ++j) {
    int T = j / 8, n = 1 + j % 8;
    MK("dn_sweep", j, T, c);
    REFUSED()
    double f, l;
    domainOf(c, 0, f, l);
    NCollection_Array1<double> ps = mkParams(f, l, j, -1, false, &c);
    I(n);
    bar();
    CASE(Geom2dGridEval_Curve e(c); runOp(e, 4, n, ps);)
  }

  // -- B-spline curves with clustered parameters (the cache of a span is used when 4 consecutive parameters share a span)
  {
    int dn = 0;
    for (int j = 0; j < 48; ++j) {
      MK("bspline_cache", j, 6, c);
      REFUSED()
      double f, l;
      domainOf(c, 0, f, l);
      NCollection_Array1<double> ps = mkParams(f, l, j + 1, 3, false);
      int op = j % 4, n;
      mkOp(op, n, dn);
      bar();
      CASE(Geom2dGridEval_BSplineCurve e(occ::down_cast<Geom2d_BSplineCurve>(c)); runOp(e, op, n, ps);)
    }
  }

  // -- Bezier curves beyond their parameter range
  {
    int dn = 0;
    for (int j = 0; j < 40; ++j) {
      MK("bezier_outside", j, 5, c);
      REFUSED()
      double f = -0.75, l = 1.75;
      NCollection_Array1<double> ps = mkParams(f, l, j + 2, -1, false);
      int op = j % 5, n;
      mkOp(op, n, dn);
      bar();
      CASE(Geom2dGridEval_BezierCurve e(occ::down_cast<Geom2d_BezierCurve>(c)); runOp(e, op, n, ps);)
    }
  }

  // -- offsets of Bezier curves with a vanishing first derivative at the start (the derivatives are adjusted)
  {
    int dn = 0;
    for (int j = 0; j < 45; ++j) {
      static const double OFF[5] = {0.5, -0.75, 1.0, 0.125, -1.5};
      begin("offset_singular");
      int n = 3 + j % 3;
      NCollection_Array1<gp_Pnt2d> poles(1, n);
      for (int i = 1; i <= n; ++i) poles(i) = gp_Pnt2d(rv(), rv());
      poles(2) = poles(1);
      if (j % 2) poles(n - 1) = poles(n);
      I(7);
      double off = OFF[j % 5];
      F(off);
      I(5);
      I(n);
      for (int i = 1; i <= n; ++i) P2(poles(i));
      B(false);
      occ::handle<Geom2d_BezierCurve> bz = new Geom2d_BezierCurve(poles);
      occ::handle<Geom2d_OffsetCurve> oc;
      bool ok = true;
      try {
        oc = new Geom2d_OffsetCurve(bz, off);
      } catch (Standard_Failure&) {
        ok = false;
      }
      std::vector<double> v = {0.0, 0.25, 0.5, 1.0};
      if (j % 3 == 1) v = {0.0};
      if (j % 3 == 2) v = {1.0, 0.75, 0.0, 0.5, 0.125};
      I((long long)v.size());
      for (double x : v) F(x);
      NCollection_Array1<double> ps(1, (int)v.size());
      for (size_t i = 0; i < v.size(); ++i) ps((int)i + 1) = v[i];
      int op = 1 + j % 4;
      int nn;
      mkOp(op, nn, dn);
      bar();
      if (!ok) { CASE(throw Standard_ConstructionError();) continue; }
      CASE(Geom2dGridEval_OffsetCurve e(oc); runOp(e, op, nn, ps);)
    }
  }
  return 0;
}
