// Deterministic reference records for the parts of OCCT 8.0.1's BSplCLib that the curve adaptors need and that the
// BSplCLib translation lacks: BSplCLib_CacheParams, BSplCLib_Cache (2D and 3D), BSplCLib::BuildCache with a span index,
// BSplCLib::Intervals and BSplCLib::FlatBezierKnots; see docs/api-bsplclib_cache.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal. Arrays are `count values`; all arrays have lower bound 1.
#include <BSplCLib.hxx>
#include <BSplCLib_Cache.hxx>
#include <BSplCLib_CacheParams.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <Standard_Failure.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Vec.hxx>
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
static void B(bool v) { I(v ? 1 : 0); }
// array of doubles ('D'), of integers ('J'), of 2D points ('P'), of 3D points ('Q'), of 2D / 3D vectors are printed with F
static void DA(const NCollection_Array1<double>& a) {
  std::string keep = sig;
  line += ' ' + std::to_string(a.Length());
  for (int i = a.Lower(); i <= a.Upper(); ++i) F(a(i));
  if (inputs) sig = keep + 'D';
}
static void JA(const NCollection_Array1<int>& a) {
  std::string keep = sig;
  line += ' ' + std::to_string(a.Length());
  for (int i = a.Lower(); i <= a.Upper(); ++i) I(a(i));
  if (inputs) sig = keep + 'J';
}
static void PA(const NCollection_Array1<gp_Pnt2d>& a) {
  std::string keep = sig;
  line += ' ' + std::to_string(a.Length());
  for (int i = a.Lower(); i <= a.Upper(); ++i) { F(a(i).X()); F(a(i).Y()); }
  if (inputs) sig = keep + 'P';
}
static void QA(const NCollection_Array1<gp_Pnt>& a) {
  std::string keep = sig;
  line += ' ' + std::to_string(a.Length());
  for (int i = a.Lower(); i <= a.Upper(); ++i) { F(a(i).X()); F(a(i).Y()); F(a(i).Z()); }
  if (inputs) sig = keep + 'Q';
}
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

// ---- a spline ---------------------------------------------------------------------------------------------------------
struct Spl {
  int degree;
  bool periodic;
  NCollection_Array1<double> knots;
  NCollection_Array1<int> mults;
  NCollection_Array1<double> flat;
  int nbPoles;
  Spl() : degree(1), periodic(false), knots(1, 2), mults(1, 2), flat(1, 4), nbPoles(2) {}
};
static Spl genSpl(int j) {
  Spl s;
  int kind = j % 3;  // 0 Bezier, 1 non-periodic B-spline, 2 periodic B-spline
  s.degree = 1 + (j / 3) % (kind == 2 ? 3 : 4);
  s.periodic = (kind == 2);
  int k = (kind == 0) ? 2 : 3 + (j / 5) % 4;
  NCollection_Array1<double> kn(1, k);
  NCollection_Array1<int> mu(1, k);
  double v = rv() - 2.0;
  for (int i = 1; i <= k; ++i) {
    kn(i) = v;
    v += 0.5 + std::abs(rv()) * 0.5;
    if (kind == 0) mu(i) = s.degree + 1;
    else if (kind == 1) mu(i) = (i == 1 || i == k) ? s.degree + 1 : 1 + (j + i) % s.degree;
    else mu(i) = 1;
  }
  s.knots = kn;
  s.mults = mu;
  int sum = 0;
  for (int i = 1; i <= k; ++i) sum += mu(i);
  s.nbPoles = s.periodic ? sum - mu(k) : sum - s.degree - 1;
  int len = BSplCLib::KnotSequenceLength(mu, s.degree, s.periodic);
  NCollection_Array1<double> fl(1, len);
  BSplCLib::KnotSequence(kn, mu, s.degree, s.periodic, fl);
  s.flat = fl;
  return s;
}
static NCollection_Array1<gp_Pnt2d> genP2(int n) {
  NCollection_Array1<gp_Pnt2d> a(1, n);
  for (int i = 1; i <= n; ++i) a(i) = gp_Pnt2d(rv(), rv());
  return a;
}
static NCollection_Array1<gp_Pnt> genP3(int n) {
  NCollection_Array1<gp_Pnt> a(1, n);
  for (int i = 1; i <= n; ++i) a(i) = gp_Pnt(rv(), rv(), rv());
  return a;
}
static NCollection_Array1<double> genW(int n, bool unit) {
  NCollection_Array1<double> w(1, n);
  for (int i = 1; i <= n; ++i) w(i) = unit ? 1.0 : 0.5 + std::abs(rv());
  return w;
}

int main() {
  // -- FlatBezierKnots -----------------------------------------------------------------------------------------------------
  for (int d = 1; d <= 25; ++d) {
    begin("flat_bezier_knots", "i");
    I(d);
    bar();
    CASE(const double* p = &BSplCLib::FlatBezierKnots(d); for (int i = 0; i < 2 * (d + 1); ++i) F(p[i]);)
  }

  // -- Intervals ---------------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 60; ++j) {
    Spl s = genSpl(j);
    int cont = 1 + (j / 2) % s.degree;
    double lo = s.knots(1), hi = s.knots(s.knots.Upper());
    double span = hi - lo;
    double f = lo + span * (0.0625 * (j % 4)), l = hi - span * (0.0625 * ((j / 4) % 4));
    if (s.periodic) {  // ranges that wrap around the period
      f = lo - span * 0.25 * (j % 3) + 0.125;
      l = hi + span * (0.5 + 0.5 * ((j / 3) % 3)) - 0.0625;
    }
    double tol = (j % 3 == 0) ? 1e-9 : ((j % 3 == 1) ? 1e-3 : 1e-12);
    if (j % 7 == 3) f = s.knots(2) - tol / 4.0;  // a boundary on a knot
    if (j % 9 == 4) l = s.knots(s.knots.Upper() - 1) + tol / 4.0;
    if (f >= l) l = f + 0.25;
    begin("intervals", "iiifffDJ");
    I(s.degree); B(s.periodic); I(cont); F(f); F(l); F(tol); DA(s.knots); JA(s.mults);
    bar();
    CASE(
      int n0 = BSplCLib::Intervals(s.knots, s.mults, s.degree, s.periodic, cont, f, l, tol, nullptr);
      NCollection_Array1<double> out(1, 2);
      int n1 = BSplCLib::Intervals(s.knots, s.mults, s.degree, s.periodic, cont, f, l, tol, &out);
      I(n0); I(n1); DA(out);)
  }

  // -- BSplCLib_CacheParams ---------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 36; ++j) {
    Spl s = genSpl(j);
    double lo = s.flat(s.flat.Lower() + s.degree), hi = s.flat(s.flat.Upper() - s.degree);
    double span = hi - lo;
    std::vector<double> us;
    for (int k = 0; k < 6; ++k) us.push_back(lo + span * (-0.6 + 0.4 * k + 0.0625 * (j % 5)));
    us.push_back(lo);
    us.push_back(hi);
    begin("cache_params", "");
    I(s.degree); B(s.periodic); DA(s.flat);
    for (double u : us) F(u);
    want = "iiD" + std::string(us.size(), 'f');
    bar();
    CASE(
      BSplCLib_CacheParams p(s.degree, s.periodic, s.flat);
      F(p.FirstParameter); F(p.LastParameter); I(p.SpanIndexMin); I(p.SpanIndexMax); F(p.SpanStart); F(p.SpanLength); I(p.SpanIndex);
      for (double u : us) {
        F(p.PeriodicNormalization(u));
        B(p.IsCacheValid(u));
        double nu = u;
        p.LocateParameter(nu, s.flat);
        F(nu); I(p.SpanIndex); F(p.SpanStart); F(p.SpanLength);
        B(p.IsCacheValid(u));
      })
  }

  // -- BSplCLib::BuildCache with a span index ---------------------------------------------------------------------------------------------
  for (int j = 0; j < 60; ++j) {
    Spl s = genSpl(j);
    bool dim3 = (j % 2 == 1);
    int wmode = (j / 2) % 3;  // 0 no weights, 1 weights, 2 unit weights
    bool hasW = wmode != 0;
    NCollection_Array1<double> w = genW(s.nbPoles, wmode == 2);
    NCollection_Array1<gp_Pnt2d> p2 = genP2(s.nbPoles);
    NCollection_Array1<gp_Pnt> p3 = genP3(s.nbPoles);
    double lo = s.flat(s.flat.Lower() + s.degree), hi = s.flat(s.flat.Upper() - s.degree);
    double u = lo + (hi - lo) * (0.0625 + 0.125 * (j % 7));
    // the span of u
    int spanIndex = 0;
    double nu = u;
    BSplCLib::LocateParameter(s.degree, s.flat, BSplCLib::NoMults(), nu, s.periodic, spanIndex, nu);
    double spanStart = s.flat(spanIndex), spanLen = s.flat(spanIndex + 1) - spanStart;
    begin("build_cache", "");
    I(dim3); I(wmode); I(s.degree); B(s.periodic); F(nu); F(spanLen); I(spanIndex); DA(s.flat);
    if (dim3) QA(p3); else PA(p2);
    if (hasW) DA(w);
    want = "iiiiffiD" + std::string(dim3 ? "Q" : "P") + (hasW ? "D" : "");
    bar();
    CASE(
      int dim = dim3 ? 3 : 2;
      int cols = dim + (hasW ? 1 : 0);
      NCollection_Array2<double> cache(1, s.degree + 1, 1, cols);
      cache.Init(-9.0);
      if (dim3) BSplCLib::BuildCache(nu, spanLen, s.periodic, s.degree, spanIndex, s.flat, p3, hasW ? &w : nullptr, cache);
      else BSplCLib::BuildCache(nu, spanLen, s.periodic, s.degree, spanIndex, s.flat, p2, hasW ? &w : nullptr, cache);
      for (int r = 1; r <= s.degree + 1; ++r)
        for (int c = 1; c <= cols; ++c) F(cache(r, c));)
  }

  // -- BSplCLib_Cache ---------------------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 80; ++j) {
    Spl s = genSpl(j);
    bool dim3 = (j % 2 == 1);
    bool hasW = ((j / 2) % 3) != 0;
    bool unitW = ((j / 2) % 3) == 2;
    NCollection_Array1<double> w = genW(s.nbPoles, unitW);
    NCollection_Array1<gp_Pnt2d> p2 = genP2(s.nbPoles);
    NCollection_Array1<gp_Pnt> p3 = genP3(s.nbPoles);
    double lo = s.flat(s.flat.Lower() + s.degree), hi = s.flat(s.flat.Upper() - s.degree);
    double u0 = lo + (hi - lo) * (0.03125 + 0.125 * (j % 7));
    std::vector<double> us;
    us.push_back(u0);
    for (int k = 1; k < 6; ++k) us.push_back(u0 + (hi - lo) * (0.0625 * k * ((j % 2) ? 1.0 : -1.0)));
    if (s.periodic) us.push_back(u0 + (hi - lo) * 1.5);
    int nus = (int)us.size();
    begin("cache", "");
    I(dim3); I(hasW); I(s.degree); B(s.periodic); DA(s.flat);
    if (dim3) QA(p3); else PA(p2);
    I(hasW ? 1 : 0);
    if (hasW) DA(w);
    I(nus);
    for (double u : us) F(u);
    want = "iiiiD" + std::string(dim3 ? "Q" : "P") + "i" + (hasW ? "D" : "") + "i" + std::string(nus, 'f');
    bar();
    CASE(
      const NCollection_Array1<double>* wp = hasW ? &w : nullptr;
      if (dim3) {
        BSplCLib_Cache c(s.degree, s.periodic, s.flat, p3, wp);
        c.BuildCache(u0, s.flat, p3, wp);
        for (double u : us) {
          B(c.IsCacheValid(u));
          gp_Pnt p; gp_Vec v1, v2, v3;
          c.D0(u, p); F(p.X()); F(p.Y()); F(p.Z());
          c.D1(u, p, v1); F(p.X()); F(p.Y()); F(p.Z()); F(v1.X()); F(v1.Y()); F(v1.Z());
          c.D2(u, p, v1, v2); F(v2.X()); F(v2.Y()); F(v2.Z());
          c.D3(u, p, v1, v2, v3); F(v3.X()); F(v3.Y()); F(v3.Z());
          double lp = 0.375 * (u - u0) + 0.5;
          c.D0Local(lp, p); F(p.X()); F(p.Y()); F(p.Z());
          c.D1Local(lp, p, v1); F(v1.X()); F(v1.Y()); F(v1.Z());
          c.D2Local(lp, p, v1, v2); F(v2.X()); F(v2.Y()); F(v2.Z());
          c.D3Local(lp, p, v1, v2, v3); F(v3.X()); F(v3.Y()); F(v3.Z());
        }
      } else {
        BSplCLib_Cache c(s.degree, s.periodic, s.flat, p2, wp);
        c.BuildCache(u0, s.flat, p2, wp);
        for (double u : us) {
          B(c.IsCacheValid(u));
          gp_Pnt2d p; gp_Vec2d v1, v2, v3;
          c.D0(u, p); F(p.X()); F(p.Y());
          c.D1(u, p, v1); F(p.X()); F(p.Y()); F(v1.X()); F(v1.Y());
          c.D2(u, p, v1, v2); F(v2.X()); F(v2.Y());
          c.D3(u, p, v1, v2, v3); F(v3.X()); F(v3.Y());
          double lp = 0.375 * (u - u0) + 0.5;
          c.D0Local(lp, p); F(p.X()); F(p.Y());
          c.D1Local(lp, p, v1); F(v1.X()); F(v1.Y());
          c.D2Local(lp, p, v1, v2); F(v2.X()); F(v2.Y());
          c.D3Local(lp, p, v1, v2, v3); F(v3.X()); F(v3.Y());
        }
      })
  }
  return 0;
}
