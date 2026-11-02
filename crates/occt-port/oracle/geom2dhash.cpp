// Deterministic reference records for OCCT 8.0.1's package Geom2dHash (hashers and equality functors of the 2D geometry),
// see docs/api-geom2dhash.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values (hashes, seeds) decimal. A hasher is printed as `ti comp hash` (ti 0: the default constructor, whose tolerances are
// printed). A curve is printed as `kind params` (kind -1: a null handle, kind -2: the constructors refused it):
//   0 line `px py dx dy`; 1 circle `px py dx dy sense r`; 2 ellipse `px py dx dy sense major minor`; 3 hyperbola (same);
//   4 parabola `px py dx dy sense focal`; 5 Bezier `n x y ... hasW [w ...]`;
//   6 B-spline `degree periodic nk knots... nk mults... np x y ... hasW [w ...]`;
//   7 offset `offset <curve>`; 8 trimmed `u1 u2 <curve>`; 13 Geom2dEval_ArchimedeanSpiralCurve `px py dx dy r0 b`;
//   14 Geom2dEval_SineWaveCurve `px py dx dy amplitude omega phase` (the inner <curve> is printed in full, recursively).
#include <Geom2dHash_CurveHasher.hxx>
#include <Geom2dHash_AxisPlacement.pxx>
#include <Geom2dHash_BSplineCurveHasher.pxx>
#include <Geom2dHash_BezierCurveHasher.pxx>
#include <Geom2dHash_CircleHasher.pxx>
#include <Geom2dHash_DirectionHasher.pxx>
#include <Geom2dHash_EllipseHasher.pxx>
#include <Geom2dHash_HyperbolaHasher.pxx>
#include <Geom2dHash_LineHasher.pxx>
#include <Geom2dHash_OffsetCurveHasher.pxx>
#include <Geom2dHash_ParabolaHasher.pxx>
#include <Geom2dHash_PointHasher.pxx>
#include <Geom2dHash_TrimmedCurveHasher.pxx>
#include <Geom2dEval_ArchimedeanSpiralCurve.hxx>
#include <Geom2dEval_SineWaveCurve.hxx>
#include <Standard_CStringHasher.hxx>
#include <Standard_HashUtils.hxx>
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
static void U(unsigned long long v) {
  line += ' ' + std::to_string(v);
  if (inputs) sig += 'i';
}
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
static double gDelta = 0.0;  // added to every generated value: a second copy of an object that differs by it
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0 + gDelta; }  // dyadic in [-2, 2) (+ gDelta)

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

// ---- tolerances -------------------------------------------------------------------------------------------------------------------
struct Tol { int ti; double comp, hash; };
static Tol tolOf(int j) {
  static const double C[6] = {1e-12, 1e-9, 1e-3, 1e-2, 1e-6, 0.5};
  static const double H[6] = {1e-7, 1e-7, 1e-3, 0.25, 1e-5, 0.5};
  int ti = j % 6;
  return Tol{ti, C[ti], H[ti]};
}
static Tol prTol(int j) {
  Tol t = tolOf(j);
  I(t.ti); F(t.comp); F(t.hash);
  return t;
}
template <class H> static H mkH(const Tol& t) { return t.ti == 0 ? H() : H(t.comp, t.hash); }
// the shift of the second copy of an object in an equality record: 0 identical, 1 within the tolerance, 2 beyond it
static double deltaOf(int v, const Tol& t) { return v == 0 ? 0.0 : (v == 1 ? 0.3 * t.comp : 3.0 * t.comp); }

// ---- nested curves --------------------------------------------------------------------------------------------------------------
static occ::handle<Geom2d_Curve> genT(int j, int T, int depth = 0);
static occ::handle<Geom2d_Curve> genOff(int j, int innerT, int depth) {
  static const double OFF[5] = {0.5, -0.75, 1.0, 0.125, -1.5};
  I(7);
  double off = OFF[(j + depth) % 5] + gDelta;
  F(off);
  occ::handle<Geom2d_Curve> b = genT(j, innerT, depth + 1);
  return new Geom2d_OffsetCurve(b, off);
}
static occ::handle<Geom2d_Curve> genTrim(int j, int innerT, int depth) {
  I(8);
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
static occ::handle<Geom2d_Curve> genEval(int j, int T) {
  I(T);
  double px = rv(), py = rv(), dx = rv(), dy = rv();
  if (dx == 0.0 && dy == 0.0) dx = 1.0;
  P2(gp_Pnt2d(px, py)); F(dx); F(dy);
  gp_Ax2d ax(gp_Pnt2d(px, py), gp_Dir2d(dx, dy));
  double a = 0.25 + std::abs(rv()), b = 0.125 + std::abs(rv()) * 0.5;
  if (T == 13) {
    F(a); F(b);
    return new Geom2dEval_ArchimedeanSpiralCurve(ax, a, b);
  }
  double ph = rv();
  F(a); F(b * 2.0); F(ph);
  return new Geom2dEval_SineWaveCurve(ax, a, b * 2.0, ph);
}
// T: 0..6 base, 7 offset(base), 8 trimmed(base), 9 offset(trimmed(base)), 10 offset(offset(base)), 11 trimmed(offset(base)),
// 12 trimmed(trimmed(base)), 13 Archimedean spiral, 14 sine wave
static occ::handle<Geom2d_Curve> genT(int j, int T, int depth) {
  int b = j % 7;
  if (T <= 6) return genBase(j, T);
  if (T == 7) return genOff(j, b, depth);
  if (T == 8) return genTrim(j, b, depth);
  if (T == 9) return genOff(j, 8, depth);
  if (T == 10) return genOff(j, 7, depth);
  if (T == 11) return genTrim(j, 7, depth);
  if (T == 12) return genTrim(j, 8, depth);
  return genEval(j, T);
}
// begins a record, prints the tolerances (when `tol`) and generates the curve; a refused curve is printed as kind -2 and the
// case throws
static bool mk(const char* name, int j, int T, occ::handle<Geom2d_Curve>& c, bool tol = true) {
  begin(name);
  c.Nullify();
  if (tol) prTol(j);
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
// the same curve again with every value shifted by `delta` (the random sequence restarts where the first copy started)
static bool mk2(int j, int T, double delta, uint64_t seed0, occ::handle<Geom2d_Curve>& c) {
  uint64_t keep = S;
  S = seed0;
  gDelta = delta;
  bool ok = true;
  try {
    c = genT(j, T);
  } catch (Standard_Failure&) {
    ok = false;
  }
  gDelta = 0.0;
  S = keep;
  return ok;
}
#define REFUSED() \
  if (!ok) { bar(); CASE(throw Standard_ConstructionError();) continue; }

// The curves compared by the B-spline hasher (directly or as the basis of a trimmed or an offset curve) must not have the same
// knots, multiplicities and degree but a different number of poles (the functor then reads past the poles of the second curve).
static void baseOfCurve(occ::handle<Geom2d_Curve> c, occ::handle<Geom2d_Curve>& b) {
  b = c;
  while (!b.IsNull()) {
    occ::handle<Geom2d_TrimmedCurve> t = occ::down_cast<Geom2d_TrimmedCurve>(b);
    occ::handle<Geom2d_OffsetCurve> o = occ::down_cast<Geom2d_OffsetCurve>(b);
    if (!t.IsNull()) b = t->BasisCurve();
    else if (!o.IsNull()) b = o->BasisCurve();
    else break;
  }
}
static bool safePair(const occ::handle<Geom2d_Curve>& a, const occ::handle<Geom2d_Curve>& b) {
  occ::handle<Geom2d_Curve> x, y;
  baseOfCurve(a, x);
  baseOfCurve(b, y);
  occ::handle<Geom2d_BSplineCurve> p = occ::down_cast<Geom2d_BSplineCurve>(x), q = occ::down_cast<Geom2d_BSplineCurve>(y);
  if (p.IsNull() || q.IsNull()) return true;
  return p->Degree() != q->Degree() || p->NbKnots() != q->NbKnots() || p->NbPoles() == q->NbPoles();
}
// prints the second curve of an equality record: v 0..2 the same curve shifted by deltaOf(v), 3 a curve of the same kind built
// from another sequence; false if the constructors refuse it
static bool genSecond(int j, int T, int v, double delta, uint64_t s0, const occ::handle<Geom2d_Curve>& a, occ::handle<Geom2d_Curve>& b) {
  size_t mark = line.size(), smark = sig.size();
  for (int attempt = 0; attempt < 8; ++attempt) {
    line.resize(mark);
    sig.resize(smark);
    try {
      if (v == 3) {
        b = genT(j + 1 + attempt + (j / 4) % 3, T);
      } else {
        S = s0;
        gDelta = delta;
        b = genT(j, T);
        gDelta = 0.0;
      }
    } catch (Standard_Failure&) {
      gDelta = 0.0;
      return false;
    }
    if (safePair(a, b)) return true;
  }
  return false;
}

// A hash and an equality family of one hasher type: `TY` the Geom2d class, `HT` the hasher, `T` the kind of curve.
#define TYPED(NAME, T, TY, HT, N) \
  for (int j = 0; j < N; ++j) { \
    occ::handle<Geom2d_Curve> a; \
    bool ok = mk(NAME "_hash", j, T, a); \
    REFUSED() \
    bar(); \
    CASE(Tol t = tolOf(j); HT h = mkH<HT>(t); U(h(occ::down_cast<TY>(a)));) \
  } \
  for (int j = 0; j < N; ++j) { \
    Tol t = tolOf(j); \
    int v = j % 4; \
    occ::handle<Geom2d_Curve> a, b; \
    begin(NAME "_eq"); \
    prTol(j); \
    uint64_t s0 = S; \
    bool ok = true; \
    try { a = genT(j, T); } catch (Standard_Failure&) { ok = false; } \
    if (ok) ok = genSecond(j, T, v, deltaOf(v, t), s0, a, b); \
    if (!ok) { line = NAME "_eq"; sig.clear(); I(-2); I(j); bar(); CASE(throw Standard_ConstructionError();) continue; } \
    bar(); \
    CASE(HT h = mkH<HT>(t); B(h(occ::down_cast<TY>(a), occ::down_cast<TY>(b)));) \
  }

static uint64_t rnd64() {
  uint64_t hi = (uint64_t)(rnd() * 4294967296.0), lo = (uint64_t)(rnd() * 4294967296.0);
  return (hi << 32) | lo;
}

int main() {
  // -- the hash functions
  for (int j = 0; j < 36; ++j) {
    begin("murmur");
    int len = (j * 7) % 41;
    std::vector<unsigned char> buf(len + 1);
    I(len);
    for (int i = 0; i < len; ++i) { buf[i] = (unsigned char)(rnd() * 256.0); I(buf[i]); }
    uint64_t seed = j % 5 == 0 ? 0xA329F1D3A586ULL : rnd64();
    U(seed);
    bar();
    CASE(U(opencascade::MurmurHash::MurmurHash64A(buf.data(), len, seed));)
  }
  for (int j = 0; j < 24; ++j) {
    begin("fnv");
    int len = (j * 5) % 23;
    std::vector<unsigned char> buf(len + 1);
    I(len);
    for (int i = 0; i < len; ++i) { buf[i] = (unsigned char)(rnd() * 256.0); I(buf[i]); }
    uint64_t seed = j % 4 == 0 ? 14695981039346656037ULL : rnd64();
    U(seed);
    bar();
    CASE(U(opencascade::FNVHash::FNVHash64A(buf.data(), len, seed));)
  }
  for (int j = 0; j < 30; ++j) {
    begin("cstring");
    int len = j == 0 ? 0 : 1 + j % 12;
    std::string s;
    I(len);
    for (int i = 0; i < len; ++i) { char ch = (char)('A' + (i == 0 ? j % 26 : (int)(rnd() * 26.0))); s += ch; I(ch); }
    bar();
    CASE(U(Standard_CStringHasher{}(s.c_str()));)
  }
  for (int j = 0; j < 24; ++j) {
    begin("hash_int");
    int v = j < 4 ? j - 2 : (int)(rnd() * 4294967296.0 - 2147483648.0);
    I(v);
    bar();
    CASE(U(opencascade::hash(v));)
  }
  for (int j = 0; j < 24; ++j) {
    begin("hash_int64");
    int64_t v = j < 4 ? j - 2 : (int64_t)rnd64();
    I(v);
    bar();
    CASE(U(opencascade::hash(v));)
  }

  // -- points, directions, axis placements
  for (int j = 0; j < 18; ++j) {
    static const double M[4] = {1.0, 10.0, 1000.0, 100000.0};
    begin("point_hash");
    Tol t = prTol(j);
    gp_Pnt2d p(rv() * M[j % 4], rv() * M[(j / 4) % 4]);
    P2(p);
    bar();
    CASE(Geom2dHash_PointHasher h = mkH<Geom2dHash_PointHasher>(t); U(h(p));)
  }
  for (int j = 0; j < 18; ++j) {
    begin("point_eq");
    Tol t = prTol(j);
    int v = j % 3;
    double d = deltaOf(v, t);
    gp_Pnt2d p(rv(), rv()), q(p.X() + d, p.Y() - d);
    P2(p); P2(q);
    bar();
    CASE(Geom2dHash_PointHasher h = mkH<Geom2dHash_PointHasher>(t); B(h(p, q));)
  }
  for (int j = 0; j < 18; ++j) {
    begin("dir_hash");
    Tol t = prTol(j);
    double dx = rv(), dy = rv();
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    F(dx); F(dy);
    bar();
    CASE(Geom2dHash_DirectionHasher h = mkH<Geom2dHash_DirectionHasher>(t); U(h(gp_Dir2d(dx, dy)));)
  }
  for (int j = 0; j < 18; ++j) {
    begin("dir_eq");
    Tol t = prTol(j);
    int v = j % 3;
    double d = deltaOf(v, t);
    double dx = rv(), dy = rv();
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    F(dx); F(dy); F(dx + d); F(dy);
    bar();
    CASE(Geom2dHash_DirectionHasher h = mkH<Geom2dHash_DirectionHasher>(t); B(h(gp_Dir2d(dx, dy), gp_Dir2d(dx + d, dy)));)
  }
  for (int j = 0; j < 18; ++j) {
    begin("axis_hash");
    Tol t = prTol(j);
    double px = rv(), py = rv(), dx = rv(), dy = rv();
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    bool sense = j % 3 != 2;
    F(px); F(py); F(dx); F(dy); B(sense);
    bar();
    CASE(Geom2dHash_AxisPlacement h = mkH<Geom2dHash_AxisPlacement>(t); U(h(gp_Ax22d(gp_Pnt2d(px, py), gp_Dir2d(dx, dy), sense)));)
  }
  for (int j = 0; j < 18; ++j) {
    begin("axis_eq");
    Tol t = prTol(j);
    int v = j % 3;
    double d = deltaOf(v, t);
    double px = rv(), py = rv(), dx = rv(), dy = rv();
    if (dx == 0.0 && dy == 0.0) dx = 1.0;
    bool sense = j % 2 == 0, sense2 = (j % 5 == 4) ? !sense : sense;
    F(px); F(py); F(dx); F(dy); B(sense);
    F(px + d); F(py); F(dx); F(dy + d); B(sense2);
    bar();
    CASE(Geom2dHash_AxisPlacement h = mkH<Geom2dHash_AxisPlacement>(t);
         B(h(gp_Ax22d(gp_Pnt2d(px, py), gp_Dir2d(dx, dy), sense), gp_Ax22d(gp_Pnt2d(px + d, py), gp_Dir2d(dx, dy + d), sense2)));)
  }

  // -- the curve hashers
  TYPED("line", 0, Geom2d_Line, Geom2dHash_LineHasher, 16)
  TYPED("circle", 1, Geom2d_Circle, Geom2dHash_CircleHasher, 16)
  TYPED("ellipse", 2, Geom2d_Ellipse, Geom2dHash_EllipseHasher, 16)
  TYPED("hyperbola", 3, Geom2d_Hyperbola, Geom2dHash_HyperbolaHasher, 16)
  TYPED("parabola", 4, Geom2d_Parabola, Geom2dHash_ParabolaHasher, 16)
  TYPED("bezier", 5, Geom2d_BezierCurve, Geom2dHash_BezierCurveHasher, 24)
  TYPED("bspline", 6, Geom2d_BSplineCurve, Geom2dHash_BSplineCurveHasher, 36)
  TYPED("trimmed", 8, Geom2d_TrimmedCurve, Geom2dHash_TrimmedCurveHasher, 24)
  TYPED("offset", 7, Geom2d_OffsetCurve, Geom2dHash_OffsetCurveHasher, 24)

  // -- the dispatcher: the 13 kinds of curve, the two curves of Geom2dEval (hashed by the name of their type) and the null handle
  for (int j = 0; j < 48; ++j) {
    int T = (j % 16 == 15) ? -1 : j % 15;
    occ::handle<Geom2d_Curve> c;
    bool ok = mk("curve_hash", j, T, c);
    REFUSED()
    bar();
    CASE(Tol t = tolOf(j); Geom2dHash_CurveHasher h = t.ti == 0 ? Geom2dHash_CurveHasher() : Geom2dHash_CurveHasher(t.comp, t.hash);
         U(h(c));)
  }
  for (int j = 0; j < 80; ++j) {
    Tol t = tolOf(j);
    int T = j % 15;
    int v = (j / 3) % 8;
    // 0 the same handle twice, 1..3 a second curve like genSecond (identical, shifted within / beyond the tolerance), 4 a curve of
    // another kind, 5 null and a curve, 6 two null handles, 7 a curve and null
    begin("curve_eq");
    prTol(j);
    I(v);
    occ::handle<Geom2d_Curve> a, b;
    bool ok = true;
    uint64_t s0 = S;
    try {
      if (v == 6) {
        I(-1);
        I(j);  // (keeps the records of two null handles apart)
      } else if (v == 5) {
        I(-1);
        a = genT(j, T);
        b = a;
        a.Nullify();
      } else {
        a = genT(j, T);
        b = a;
        if (v == 7) { b.Nullify(); }
        else if (v == 4) {
          int T2 = (T + 1 + j % 6) % 15;
          size_t mark = line.size(), smark = sig.size();
          ok = false;
          for (int attempt = 0; attempt < 8 && !ok; ++attempt) {
            line.resize(mark);
            sig.resize(smark);
            b = genT(j + attempt, T2 == T ? (T + 1) % 15 : T2);
            ok = safePair(a, b);
          }
        } else if (v >= 1 && v <= 3) {
          ok = genSecond(j, T, v == 3 ? 3 : v - 1, deltaOf(v == 1 ? 0 : v - 1, t), s0, a, b);
        }
      }
    } catch (Standard_Failure&) {
      ok = false;
    }
    if (!ok) { line = "curve_eq"; sig.clear(); I(-2); I(j); bar(); CASE(throw Standard_ConstructionError();) continue; }
    bar();
    CASE(Geom2dHash_CurveHasher h = t.ti == 0 ? Geom2dHash_CurveHasher() : Geom2dHash_CurveHasher(t.comp, t.hash); B(h(a, b));)
  }
  return 0;
}
