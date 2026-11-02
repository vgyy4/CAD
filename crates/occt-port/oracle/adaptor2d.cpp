// Deterministic reference records for OCCT 8.0.1's package Adaptor2d (Adaptor2d_Curve2d, Adaptor2d_Line2d,
// Adaptor2d_OffsetCurve), see docs/api-adaptor2d.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal. The curves adapted by the offset curve (and the base-class defaults) are a closed set of test adaptors
// ("TC") that fill in every virtual method from printed parameters and fold every call (method id and argument bits)
// into a counter and a 64-bit hash, so the sequence of calls an adaptor makes is part of the behaviour.
#include <Adaptor2d_Curve2d.hxx>
#include <Adaptor2d_Line2d.hxx>
#include <Adaptor2d_OffsetCurve.hxx>
#include <Geom2d_BezierCurve.hxx>
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
static void U(unsigned long long v) {
  line += ' ' + std::to_string(v);
  if (inputs) sig += 'u';
}
static void B(bool v) { I(v ? 1 : 0); }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void begin(const char* name, const std::string& schema) {
  line = name;
  sig.clear();
  want = schema;
  inputs = true;
}
static void bar() {
  if (sig != want) {
    std::cerr << "schema mismatch " << line.substr(0, 24) << ": " << sig << " vs " << want << "\n";
    std::abort();
  }
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
static const int LB[4] = {1, 0, 3, -2};

static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;

// ---- printing of gp values ---------------------------------------------------------------------------------------------
static void P2(const gp_Pnt2d& p) { F(p.X()); F(p.Y()); }
static void V2(const gp_Vec2d& v) { F(v.X()); F(v.Y()); }
static void D2(const gp_Dir2d& d) { F(d.X()); F(d.Y()); }
static void A22(const gp_Ax22d& a) { P2(a.Location()); D2(a.XDirection()); D2(a.YDirection()); }
static void PrLin(const gp_Lin2d& l) { P2(l.Location()); D2(l.Direction()); }
static void PrCirc(const gp_Circ2d& c) { A22(c.Axis()); F(c.Radius()); }
static void PrElips(const gp_Elips2d& e) { A22(e.Axis()); F(e.MajorRadius()); F(e.MinorRadius()); }
static void PrHypr(const gp_Hypr2d& h) { A22(h.Axis()); F(h.MajorRadius()); F(h.MinorRadius()); }
static void PrParab(const gp_Parab2d& p) { A22(p.Axis()); F(p.Focal()); }

// ---- the test adaptor ----------------------------------------------------------------------------------------------------
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
};
#define TC_SCHEMA "ffiii" "fffff" "iif" "ffffffffff" "ffff" "i" "ff" "iiiii"
static void printTC(const TC& t) {
  F(t.u0); F(t.u1); I(t.ctype); I(t.cont); I(t.nint);
  for (int k = 0; k < 5; ++k) F(t.brk[k]);
  B(t.closed); B(t.periodic); F(t.period);
  F(t.p0[0]); F(t.p0[1]); F(t.a[0]); F(t.a[1]); F(t.b[0]); F(t.b[1]); F(t.c[0]); F(t.c[1]); F(t.d[0]); F(t.d[1]);
  F(t.cx); F(t.cy); F(t.dx); F(t.dy); B(t.sense);
  F(t.r1); F(t.r2);
  I(t.degree); B(t.rational); I(t.nbpoles); I(t.nbknots); B(t.hasBezier);
}
static void prTC(const TC& t) { I(t.calls); U(t.hash); }
// the shape enumerators in order: C0 G1 C1 G2 C2 C3 CN; the curve types: Line Circle Ellipse Hyperbola Parabola Bezier BSpline Offset Other
static occ::handle<TC> genTC(int j) {
  occ::handle<TC> t = new TC();
  t->ctype = (j % 9);
  if (j % 9 == 8) t->ctype = 8;  // OtherCurve
  static const int CT[7] = {6, 5, 4, 3, 2, 1, 0};
  t->cont = CT[(j / 3) % 7];
  t->nint = 1 + (j / 2) % 4;
  double v = -1.0;
  for (int k = 0; k < 5; ++k) { v += 0.5 + std::abs(rv()) * 0.25; t->brk[k] = v; }
  t->u0 = (j % 5 == 1) ? t->brk[0] : -1.0;
  t->u1 = (j % 7 == 3) ? t->brk[t->nint] : t->brk[t->nint] + 1.0;
  t->closed = (j % 4 == 1);
  t->periodic = (j % 6 == 2);
  t->period = 2.0 + std::abs(rv());
  t->p0[0] = rv(); t->p0[1] = rv();
  t->a[0] = (j % 11 == 9) ? 0.0 : rv(); t->a[1] = (j % 11 == 9) ? 0.0 : rv();  // a zero tangent at the start for some
  t->b[0] = rv(); t->b[1] = rv(); t->c[0] = rv(); t->c[1] = rv();
  t->d[0] = (j % 3) ? 0.0 : rv(); t->d[1] = (j % 3) ? 0.0 : rv();
  t->cx = rv(); t->cy = rv();
  t->dx = 1.0 + std::abs(rv()); t->dy = rv();
  t->sense = (j % 5 != 3);
  t->r2 = 0.5 + std::abs(rv()) * 0.25;
  t->r1 = t->r2 + 0.25 + std::abs(rv()) * 0.5;
  t->degree = 1 + j % 5;
  t->rational = (j % 2 == 0);
  t->nbpoles = 2 + (j * 5) % 40;
  t->nbknots = 2 + (j * 3) % 60;
  t->hasBezier = (j % 4 != 2);
  return t;
}

// the base class itself (every virtual method not overridden)
struct Bare : public Adaptor2d_Curve2d {};

static void prShape(GeomAbs_Shape s) { I((int)s); }


static const double OFF[6] = {0.0, 0.5, -0.75, 1.0, 0.125, -1.5};
static const GeomAbs_Shape SH[7] = {GeomAbs_C0, GeomAbs_G1, GeomAbs_C1, GeomAbs_G2, GeomAbs_C2, GeomAbs_C3, GeomAbs_CN};

static void prBezier(const occ::handle<Geom2d_BezierCurve>& b) {
  I(b.IsNull());
  if (!b.IsNull()) {
    I(b->NbPoles()); I(b->Degree());
    for (int i = 1; i <= b->NbPoles(); ++i) P2(b->Pole(i));
  }
}
// an offset curve over a fresh test adaptor, with a window of the parameter range
struct OC {
  occ::handle<TC> tc;
  occ::handle<Adaptor2d_OffsetCurve> oc;
  double off, wf, wl;
};
static OC genOC(int j, int forceType = -1) {
  OC r;
  r.tc = genTC(j);
  if (forceType >= 0) r.tc->ctype = forceType;
  r.off = OFF[(j / 2) % 6];
  r.wf = (j % 3 == 0) ? r.tc->brk[0] + 0.125 : r.tc->u0;
  r.wl = (j % 3 == 1) ? r.tc->brk[r.tc->nint] - 0.125 : r.tc->u1;
  return r;
}
static void printOC(const OC& r) { printTC(*r.tc); F(r.off); F(r.wf); F(r.wl); }
#define OC_SCHEMA TC_SCHEMA "fff"
static void prOff(const occ::handle<Adaptor2d_OffsetCurve>& o) { F(o->FirstParameter()); F(o->LastParameter()); F(o->Offset()); }

int main() {
  // -- the base class: every method not overridden throws NotImplemented, except NbSamples and the Eval* wrappers ------------
  for (int m = 0; m < 35; ++m)
    for (int sidx = 0; sidx < 2; ++sidx) {
      GeomAbs_Shape sh = sidx ? GeomAbs_CN : GeomAbs_C0;
      double u = 0.25 * (m + 1);
      begin("base_default", "iif");
      I(m); I(sidx); F(u);
      bar();
      occ::handle<Bare> h = new Bare();
      CASE(
        NCollection_Array1<double> arr(1, 3);
        gp_Pnt2d p; gp_Vec2d v1, v2, v3;
        switch (m) {
          case 0: { auto r = h->ShallowCopy(); I(r.IsNull()); break; }
          case 1: F(h->FirstParameter()); break;
          case 2: F(h->LastParameter()); break;
          case 3: prShape(h->Continuity()); break;
          case 4: I(h->NbIntervals(sh)); break;
          case 5: h->Intervals(arr, sh); F(arr(1)); break;
          case 6: { auto r = h->Trim(0.0, 1.0, 1e-7); I(r.IsNull()); break; }
          case 7: B(h->IsClosed()); break;
          case 8: B(h->IsPeriodic()); break;
          case 9: F(h->Period()); break;
          case 10: P2(h->Value(u)); break;
          case 11: h->D0(u, p); P2(p); break;
          case 12: h->D1(u, p, v1); P2(p); break;
          case 13: h->D2(u, p, v1, v2); P2(p); break;
          case 14: h->D3(u, p, v1, v2, v3); P2(p); break;
          case 15: V2(h->DN(u, 2)); break;
          case 16: F(h->Resolution(u)); break;
          case 17: I((int)h->GetType()); break;
          case 18: PrLin(h->Line()); break;
          case 19: PrCirc(h->Circle()); break;
          case 20: PrElips(h->Ellipse()); break;
          case 21: PrHypr(h->Hyperbola()); break;
          case 22: PrParab(h->Parabola()); break;
          case 23: I(h->Degree()); break;
          case 24: B(h->IsRational()); break;
          case 25: I(h->NbPoles()); break;
          case 26: I(h->NbKnots()); break;
          case 27: I(h->NbSamples()); break;
          case 28: { auto r = h->Bezier(); I(r.IsNull()); break; }
          case 29: { auto r = h->BSpline(); I(r.IsNull()); break; }
          case 30: P2(h->EvalD0(u)); break;
          case 31: P2(h->EvalD1(u).Point); break;
          case 32: P2(h->EvalD2(u).Point); break;
          case 33: P2(h->EvalD3(u).Point); break;
          default: V2(h->EvalDN(u, 1)); break;
        })
    }
  // the Eval* wrappers over an adaptor that overrides D0..DN, and the default NbSamples
  for (int j = 0; j < 30; ++j) {
    occ::handle<TC> t = genTC(j);
    int kind = j % 6, n = (j / 6) % 6;
    double u = rv();
    begin("base_eval", TC_SCHEMA "iif");
    printTC(*t); I(kind); I(n); F(u);
    bar();
    CASE(
      switch (kind) {
        case 0: P2(t->EvalD0(u)); break;
        case 1: { auto r = t->EvalD1(u); P2(r.Point); V2(r.D1); break; }
        case 2: { auto r = t->EvalD2(u); P2(r.Point); V2(r.D1); V2(r.D2); break; }
        case 3: { auto r = t->EvalD3(u); P2(r.Point); V2(r.D1); V2(r.D2); V2(r.D3); break; }
        case 4: V2(t->EvalDN(u, n)); break;
        default: I(t->NbSamples()); break;
      }
      prTC(*t);)
  }

  // -- Adaptor2d_Line2d ---------------------------------------------------------------------------------------------------
  for (int j = 0; j < 32; ++j) {
    int v = j % 4;
    double px = rv(), py = rv(), dx = rv(), dy = rv();
    if (j % 9 == 4) { dx = 0.0; dy = -2.0; }
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    double u0 = rv(), u1 = u0 + 0.5 + std::abs(rv()), r3 = std::abs(rv()) + 0.125;
    int si = (j / 4) % 7, tl = LB[j % 4];
    begin("line_params", "iffffffifi");
    I(v); F(px); F(py); F(dx); F(dy); F(u0); F(u1); I(si); F(r3); I(tl);
    bar();
    CASE(
      Adaptor2d_Line2d l;
      if (v == 1) l = Adaptor2d_Line2d(gp_Pnt2d(px, py), gp_Dir2d(dx, dy), u0, u1);
      else if (v == 2) l.Load(gp_Lin2d(gp_Pnt2d(px, py), gp_Dir2d(dx, dy)));
      else if (v == 3) l.Load(gp_Lin2d(gp_Pnt2d(px, py), gp_Dir2d(dx, dy)), u0, u1);
      F(l.FirstParameter()); F(l.LastParameter()); prShape(l.Continuity()); I(l.NbIntervals(SH[si]));
      NCollection_Array1<double> arr(tl, tl + 1);
      l.Intervals(arr, SH[si]); F(arr(tl)); F(arr(tl + 1));
      B(l.IsClosed()); B(l.IsPeriodic()); F(l.Resolution(r3)); I((int)l.GetType()); I(l.NbSamples());
      PrLin(l.Line());)
  }
  for (int j = 0; j < 6; ++j) {
    double px = rv(), py = rv();
    begin("line_period", "ffii");
    F(px); F(py); I(j); I(j * 3);
    bar();
    CASE(Adaptor2d_Line2d l(gp_Pnt2d(px, py), gp_Dir2d(1.0, 2.0), 0.0, 1.0); F(l.Period());)
  }
  for (int m = 0; m < 10; ++m)
    for (int j = 0; j < 2; ++j) {
      double px = rv(), py = rv();
      begin("line_nosuch", "iff");
      I(m); F(px); F(py);
      bar();
      CASE(
        Adaptor2d_Line2d l(gp_Pnt2d(px, py), gp_Dir2d(0.5, 1.0), -1.0, 2.0);
        switch (m) {
          case 0: PrCirc(l.Circle()); break;
          case 1: PrElips(l.Ellipse()); break;
          case 2: PrHypr(l.Hyperbola()); break;
          case 3: PrParab(l.Parabola()); break;
          case 4: I(l.Degree()); break;
          case 5: B(l.IsRational()); break;
          case 6: I(l.NbPoles()); break;
          case 7: I(l.NbKnots()); break;
          case 8: I(l.Bezier().IsNull()); break;
          default: I(l.BSpline().IsNull()); break;
        })
    }
  for (int j = 0; j < 36; ++j) {
    double px = rv(), py = rv(), dx = rv(), dy = rv(), u = rv() * 2.0;
    if (dx == 0.0 && dy == 0.0) dy = 1.0;
    int kind = j % 6, n = (j / 6) % 5 - 1;  // n = -1 .. 3
    begin("line_eval", "ffffiif");
    F(px); F(py); F(dx); F(dy); I(kind); I(n); F(u);
    bar();
    CASE(
      Adaptor2d_Line2d l(gp_Pnt2d(px, py), gp_Dir2d(dx, dy), -2.0, 2.0);
      gp_Pnt2d p; gp_Vec2d v1, v2, v3;
      switch (kind) {
        case 0: P2(l.Value(u)); break;
        case 1: l.D0(u, p); P2(p); break;
        case 2: l.D1(u, p, v1); P2(p); V2(v1); break;
        case 3: l.D2(u, p, v1, v2); P2(p); V2(v1); V2(v2); break;
        case 4: l.D3(u, p, v1, v2, v3); P2(p); V2(v1); V2(v2); V2(v3); break;
        default: V2(l.DN(u, n)); break;
      })
  }
  for (int j = 0; j < 16; ++j) {
    double px = rv(), py = rv(), dx = rv(), dy = rv(), f = rv(), l2 = f + 0.5 + std::abs(rv());
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    int op = j % 2;
    begin("line_copy_trim", "ffffffi");
    F(px); F(py); F(dx); F(dy); F(f); F(l2); I(op);
    bar();
    CASE(
      Adaptor2d_Line2d l(gp_Pnt2d(px, py), gp_Dir2d(dx, dy), -1.0, 1.0);
      occ::handle<Adaptor2d_Curve2d> h = op == 0 ? l.ShallowCopy() : l.Trim(f, l2, 1e-7);
      occ::handle<Adaptor2d_Line2d> hl = occ::down_cast<Adaptor2d_Line2d>(h);
      I(hl.IsNull()); F(h->FirstParameter()); F(h->LastParameter()); PrLin(h->Line()); I((int)h->GetType());)
  }

  // -- Adaptor2d_OffsetCurve ------------------------------------------------------------------------------------------------
  for (int j = 0; j < 24; ++j) {
    OC r = genOC(j);
    int v = j % 6;
    double off2 = OFF[(j + 3) % 6], wf2 = r.wf + 0.25, wl2 = r.wl - 0.25;
    begin("offset_ctor", OC_SCHEMA "ifff");
    printOC(r); I(v); F(off2); F(wf2); F(wl2);
    bar();
    CASE(
      occ::handle<Adaptor2d_OffsetCurve> o;
      if (v == 0) o = new Adaptor2d_OffsetCurve(r.tc);
      else if (v == 1) o = new Adaptor2d_OffsetCurve(r.tc, r.off);
      else if (v == 2) o = new Adaptor2d_OffsetCurve(r.tc, r.off, r.wf, r.wl);
      else if (v == 3) { o = new Adaptor2d_OffsetCurve(); o->Load(occ::handle<Adaptor2d_Curve2d>(r.tc)); }
      else if (v == 4) { o = new Adaptor2d_OffsetCurve(r.tc); o->Load(off2); }
      else { o = new Adaptor2d_OffsetCurve(r.tc, r.off, r.wf, r.wl); o->Load(off2, wf2, wl2); }
      prOff(o); I(o->Curve().IsNull()); I(o->Curve() == occ::handle<Adaptor2d_Curve2d>(r.tc)); prTC(*r.tc);)
  }
  for (int j = 0; j < 28; ++j) {
    OC r = genOC(j);
    begin("offset_continuity", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); prShape(o.Continuity()); prTC(*r.tc);)
  }
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 36; ++j) {
      OC r = genOC(j + fam);
      if (j % 5 == 2) r.tc->nint = 1;
      // the window must lie inside the breaks (T(first) and T(last) are read until they pass it) and TI must be long enough
      r.wf = r.tc->brk[0] + 0.125 * (j % 3);
      r.wl = r.tc->brk[r.tc->nint] - 0.125 * ((j / 3) % 3);
      int si = (j / 2) % 7, tl = LB[j % 4], tn = (r.tc->nint == 1) ? 2 + j % 3 : 8;
      begin(fam == 0 ? "offset_nb_intervals" : "offset_intervals", OC_SCHEMA "iii");
      printOC(r); I(si); I(tl); I(tn);
      bar();
      if (fam == 0) CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); I(o.NbIntervals(SH[si])); prTC(*r.tc);)
      else CASE(
        Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl);
        NCollection_Array1<double> ti(tl, tl + tn - 1);
        ti.Init(-7.0);
        o.Intervals(ti, SH[si]);
        for (int k = tl; k < tl + tn; ++k) F(ti(k));
        prTC(*r.tc);)
    }
  for (int j = 0; j < 20; ++j) {
    OC r = genOC(j);
    double f = rv(), l2 = f + 0.5 + std::abs(rv()), tol = 1e-7;
    begin("offset_trim", OC_SCHEMA "fff");
    printOC(r); F(f); F(l2); F(tol);
    bar();
    CASE(
      Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl);
      occ::handle<Adaptor2d_Curve2d> h = o.Trim(f, l2, tol);
      occ::handle<Adaptor2d_OffsetCurve> ho = occ::down_cast<Adaptor2d_OffsetCurve>(h);
      I(ho.IsNull()); prOff(ho); I(ho->Curve() == occ::handle<Adaptor2d_Curve2d>(r.tc)); prTC(*r.tc);)
  }
  for (int j = 0; j < 36; ++j) {
    OC r = genOC(j);
    if (j % 3 == 0) r.tc->closed = true;
    if (j % 4 == 1) { r.tc->d[0] = 0.0; r.tc->d[1] = 0.0; r.tc->c[0] = 0.0; r.tc->c[1] = 0.0; r.tc->b[0] = 0.0; r.tc->b[1] = 0.0; r.tc->a[0] = 1.0; r.tc->a[1] = 0.5; }  // a straight, parallel tangent
    if (j % 4 == 2) { r.tc->d[0] = 0.0; r.tc->d[1] = 0.0; r.tc->c[0] = 0.0; r.tc->c[1] = 0.0; r.tc->b[0] = 0.0; r.tc->b[1] = 0.0; r.tc->a[0] = 1.0; r.tc->a[1] = 0.5; r.tc->u0 = -1.0; r.tc->u1 = 1.0; }
    begin("offset_closed", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); B(o.IsClosed()); B(o.IsPeriodic()); prTC(*r.tc);)
  }
  for (int j = 0; j < 6; ++j) {
    OC r = genOC(j);
    r.tc->periodic = (j % 2 == 0);
    begin("offset_period", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); F(o.Period()); prTC(*r.tc);)
  }
  for (int j = 0; j < 48; ++j) {
    OC r = genOC(j);
    if (j % 7 == 5) r.off = 0.0;
    double u = rv();
    int kind = j % 6;
    begin("offset_eval", OC_SCHEMA "if");
    printOC(r); I(kind); F(u);
    bar();
    CASE(
      Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl);
      gp_Pnt2d p; gp_Vec2d v1, v2, v3;
      switch (kind) {
        case 0: P2(o.Value(u)); break;
        case 1: o.D0(u, p); P2(p); break;
        case 2: o.D1(u, p, v1); P2(p); V2(v1); break;
        case 3: o.D2(u, p, v1, v2); P2(p); V2(v1); V2(v2); break;
        case 4: o.D3(u, p, v1, v2, v3); P2(p); V2(v1); V2(v2); V2(v3); break;
        default: V2(o.DN(u, 2)); break;
      }
      prTC(*r.tc);)
  }
  for (int j = 0; j < 27; ++j) {
    OC r = genOC(j);
    begin("offset_type", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); I((int)o.GetType()); prTC(*r.tc);)
  }
  for (int j = 0; j < 16; ++j) {
    OC r = genOC(j, 0);  // a line adapted through the offset
    // The library is built without Raise_if checks: a zero tangent at 0 would make gp_Dir2d(gp_Vec2d) return NaN
    // instead of throwing, so the tangent is kept away from zero.
    if (r.tc->a[0] == 0.0 && r.tc->a[1] == 0.0) { r.tc->a[0] = 0.5; r.tc->a[1] = 0.25; }
    begin("offset_line", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); PrLin(o.Line()); prTC(*r.tc);)
  }
  for (int j = 0; j < 16; ++j) {
    OC r = genOC(j, 1);
    if (j % 8 == 3) r.off = r.tc->sense ? -r.tc->r1 : r.tc->r1;               // a circle of radius zero
    if (j % 8 == 5) r.off = r.tc->sense ? -2.0 * r.tc->r1 : 2.0 * r.tc->r1;   // a negative radius
    begin("offset_circle", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); PrCirc(o.Circle()); prTC(*r.tc);)
  }
  for (int j = 0; j < 20; ++j) {
    int which = j % 3;
    int ct = (j % 5 == 4) ? 0 : 2 + which;  // the matching conic type (a line for some)
    OC r = genOC(j, ct);
    if (j % 2 == 0) r.off = 0.0;
    begin("offset_conic", OC_SCHEMA "i");
    printOC(r); I(which);
    bar();
    CASE(
      Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl);
      switch (which) {
        case 0: PrElips(o.Ellipse()); break;
        case 1: PrHypr(o.Hyperbola()); break;
        default: PrParab(o.Parabola()); break;
      }
      prTC(*r.tc);)
  }
  for (int j = 0; j < 30; ++j) {
    int ct = 5 + j % 2;  // Bezier or BSpline
    OC r = genOC(j, (j % 5 == 4) ? 8 : ct);
    if (j % 3 == 0) r.off = 0.0;
    int kind = (j / 2) % 6;
    begin("offset_bezier_info", OC_SCHEMA "i");
    printOC(r); I(kind);
    bar();
    CASE(
      Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl);
      switch (kind) {
        case 0: I(o.Degree()); break;
        case 1: B(o.IsRational()); break;
        case 2: I(o.NbPoles()); break;
        case 3: I(o.NbKnots()); break;
        case 4: prBezier(o.Bezier()); break;
        default: I(o.BSpline().IsNull()); break;
      }
      prTC(*r.tc);)
  }
  for (int j = 0; j < 30; ++j) {
    OC r = genOC(j, 5 + j % 3);
    r.tc->nbpoles = (j % 2) ? 40 : 2 + j % 7;
    r.tc->nbknots = 1 + (j * 7) % 50;
    r.tc->degree = 1 + j % 9;
    begin("offset_nb_samples", OC_SCHEMA);
    printOC(r);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); I(o.NbSamples()); prTC(*r.tc);)
  }
  for (int j = 0; j < 8; ++j) {
    OC r = genOC(j);
    double rr = (j % 2) ? 0.5 * (j + 1) : 2e-3 * j + 1e-9;
    begin("offset_resolution", OC_SCHEMA "f");
    printOC(r); F(rr);
    bar();
    CASE(Adaptor2d_OffsetCurve o(r.tc, r.off, r.wf, r.wl); F(o.Resolution(rr)); prTC(*r.tc);)
  }
  for (int j = 0; j < 12; ++j) {
    OC r = genOC(j);
    begin("offset_copy", OC_SCHEMA "i");
    printOC(r); I(j % 2);
    bar();
    CASE(
      occ::handle<Adaptor2d_OffsetCurve> o = (j % 2) ? new Adaptor2d_OffsetCurve() : new Adaptor2d_OffsetCurve(r.tc, r.off, r.wf, r.wl);
      occ::handle<Adaptor2d_Curve2d> h = o->ShallowCopy();
      occ::handle<Adaptor2d_OffsetCurve> ho = occ::down_cast<Adaptor2d_OffsetCurve>(h);
      I(ho.IsNull()); prOff(ho); I(ho->Curve().IsNull());
      if (!ho->Curve().IsNull()) {
        occ::handle<TC> cp = occ::down_cast<TC>(ho->Curve());
        I(cp == r.tc); prTC(*cp);
      }
      prTC(*r.tc);)
  }
  for (int j = 0; j < 6; ++j) {
    begin("offset_null", "i");
    I(j);
    bar();
    CASE(
      Adaptor2d_OffsetCurve o;
      F(o.FirstParameter()); F(o.LastParameter()); F(o.Offset()); I(o.Curve().IsNull());)
  }
  return 0;
}
