// Deterministic reference records for the header-only OCCT MathRoot package (OCCT 8.0.1), see docs/api-mathroot.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal; a vector is `lower upper values`. Callbacks are a closed set of functions described by printed
// parameters; every callback call is counted and its argument bits are folded into a 64-bit hash.
#include <MathRoot_All.hxx>
#include <MathRoot_Bisection.hxx>
#include <MathRoot_Brent.hxx>
#include <MathRoot_Multiple.hxx>
#include <MathRoot_Newton.hxx>
#include <MathRoot_Secant.hxx>
#include <MathRoot_Trig.hxx>
#include <Standard_Failure.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

using namespace MathRoot;

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
static void V(const math_Vector& v) {
  std::string keep = sig;
  line += ' ' + std::to_string(v.Lower()) + ' ' + std::to_string(v.Upper());
  for (int i = v.Lower(); i <= v.Upper(); ++i) F(v(i));
  if (inputs) sig = keep + 'V';
}
static void begin(const char* name, const char* schema) {
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
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}

// ---- the callback -----------------------------------------------------------------------------------
static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;
static const double INF = std::numeric_limits<double>::infinity();

// kind 0 cubic p0 + p1 x + p2 x^2 + p3 x^3; 1 plateau p0 * max(0, |x - p1| - p2); 2 p0 (x - p1)^2 (x - p2) (x - p3);
// 3 p0 * T5(x) (Chebyshev, five roots in [-1, 1]). Fails (returns false) for flo < x < fhi.
struct TS {
  int kind;
  double p0, p1, p2, p3, flo, fhi;
  int calls = 0;
  uint64_t hash = H0;
  bool Value(double x, double& y) {
    ++calls;
    hash = mixh(hash, bitsOf(x));
    if (x > flo && x < fhi) return false;
    double d;
    return eval(x, y, d);
  }
  bool Values(double x, double& y, double& dy) {
    ++calls;
    hash = mixh(hash, bitsOf(x) ^ 0x5bd1e995ull);
    if (x > flo && x < fhi) return false;
    return eval(x, y, dy);
  }
  bool eval(double x, double& y, double& dy) const {
    switch (kind) {
      case 0: y = ((p3 * x + p2) * x + p1) * x + p0; dy = (3.0 * p3 * x + 2.0 * p2) * x + p1; break;
      case 1: {
        double v = std::abs(x - p1) - p2;
        y = p0 * (0.0 < v ? v : 0.0);
        dy = (v > 0.0) ? p0 * ((x > p1) ? 1.0 : -1.0) : 0.0;
        break;
      }
      case 2: {
        double a = x - p1, b = x - p2, c = x - p3;
        y = p0 * a * a * b * c;
        dy = p0 * (2.0 * a * b * c + a * a * c + a * a * b);
        break;
      }
      default: {
        double x2 = x * x;
        y = p0 * (((16.0 * x2 - 20.0) * x2 + 5.0) * x);
        dy = p0 * ((80.0 * x2 - 60.0) * x2 + 5.0);
        break;
      }
    }
    return true;
  }
};
static void printTS(const TS& t) { I(t.kind); F(t.p0); F(t.p1); F(t.p2); F(t.p3); F(t.flo); F(t.fhi); }
#define TS_SCHEMA "iffffff"
static TS genTS(int j, bool fail = false) {
  int kind = j % 4;
  double p0 = (rv() < 0 ? -1.0 : 1.0) * (0.5 + std::abs(rv())), p1 = rv(), p2 = rv(), p3 = rv();
  if (kind == 1) { p1 = rv(); p2 = 0.25 + std::abs(rv()) * 0.5; }
  if (kind == 2) { p1 = rv() * 0.5; p2 = rv() - 1.0; p3 = rv() + 1.0; }
  if (kind == 3) p0 = 0.5 + std::abs(rv());
  if (kind == 0 && j % 3 != 2) {  // a cubic with its roots in [-1.5, 1.5]: s (x - r1) (x - r2) (x - r3)
    double r1 = std::floor(rnd() * 12.0 - 6.0) / 4.0, r2 = std::floor(rnd() * 12.0 - 6.0) / 4.0 + 0.125, r3 = std::floor(rnd() * 12.0 - 6.0) / 4.0 - 0.0625;
    p0 = -p0 * r1 * r2 * r3; p1 = p0 == 0.0 ? 0.0 : 0.0;
    double s = (rv() < 0 ? -1.0 : 1.0) * (0.5 + std::abs(rv()));
    p0 = -s * r1 * r2 * r3; p1 = s * (r1 * r2 + r1 * r3 + r2 * r3); p2 = -s * (r1 + r2 + r3); p3 = s;
  }
  double flo = 1.0, fhi = 0.0;
  if (fail) { flo = rv() * 0.5; fhi = flo + 0.125; }
  return TS{kind, p0, p1, p2, p3, flo, fhi};
}
static void outCalls(const TS& t) { I(t.calls); U(t.hash); }
static void prS(const MathUtils::ScalarResult& r, const TS& t) {
  I((int)r.Status); I(r.NbIterations);
  B(r.Root.has_value()); if (r.Root) F(*r.Root);
  B(r.Value.has_value()); if (r.Value) F(*r.Value);
  B(r.Derivative.has_value()); if (r.Derivative) F(*r.Derivative);
  outCalls(t);
}
static void prM(const MultipleResult& r, const TS& t) {
  I((int)r.Status); I(r.NbIterations); B(r.IsAllNull); I(r.NbRoots());
  for (int i = 0; i < r.NbRoots(); ++i) F(r.Roots.Value(i));
  I(r.Values.Length());
  for (int i = 0; i < r.Values.Length(); ++i) F(r.Values.Value(i));
  outCalls(t);
}
static void prA(const AllRootsResult& r, const TS& t) {
  I((int)r.Status); I(r.NbRoots());
  for (int i = 0; i < r.NbRoots(); ++i) F(r.Roots.Value(i));
  I(r.RootStates.Length());
  for (int i = 0; i < r.RootStates.Length(); ++i) I(r.RootStates.Value(i));
  I(r.NbIntervals());
  for (int i = 0; i < r.NbIntervals(); ++i) { F(r.NullIntervals.Value(i).A); F(r.NullIntervals.Value(i).B); I(r.NullIntervals.Value(i).State); }
  outCalls(t);
}
// make [a, b] a sign-changing interval of the function (when possible) for the cases that should succeed
static void bracketIt(const TS& t, int j, double& a, double& b) {
  if (j % 4 == 0) return;
  for (int k = 0; k < 60; ++k) {
    TS p = t; p.flo = 1.0; p.fhi = 0.0;
    double fa, fb;
    p.Value(a, fa); p.Value(b, fb);
    if (fa * fb < 0.0) return;
    a = rv() * 1.5; b = rv() * 1.5;
  }
}
static MathUtils::Config genCfg(int j) {
  MathUtils::Config c;
  c.XTolerance = (j % 3 == 0) ? 1e-10 : ((j % 3 == 1) ? 1e-6 : 1e-14);
  c.FTolerance = (j % 4 == 0) ? 1e-10 : ((j % 4 == 1) ? 1e-3 : 1e-14);
  c.MaxIterations = (j == 2) ? 1 : (j == 3 ? 0 : (j % 5 == 4 ? 5 : 100));
  return c;
}
static void printCfg(const MathUtils::Config& c) { F(c.XTolerance); F(c.FTolerance); I(c.MaxIterations); }
static MultipleConfig genMC(int j) {
  MultipleConfig c;
  c.NbSamples = (j % 4 == 0) ? 100 : ((j % 4 == 1) ? 5 : ((j % 4 == 2) ? 12 : 0));
  c.XTolerance = (j % 3 == 0) ? 1e-10 : 1e-6;
  c.FTolerance = (j % 5 == 0) ? 1e-3 : 1e-10;
  c.NullTolerance = (j % 7 == 0) ? 1e-3 : 1e-12;
  c.MaxIterations = (j % 6 == 5) ? 3 : 100;
  c.Offset = (j % 4 == 3) ? 0.25 * rv() : 0.0;
  return c;
}
static void printMC(const MultipleConfig& c) { I(c.NbSamples); F(c.XTolerance); F(c.FTolerance); F(c.NullTolerance); I(c.MaxIterations); F(c.Offset); }

int main() {
  // -- bracketing solvers -------------------------------------------------------------------------------
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 16; ++j) {  // brent, bisection
      smash();
      TS t = genTS(j, j % 7 == 5);
      double a = rv(), b = (j == 1) ? a : rv();
      bracketIt(t, j, a, b);
      MathUtils::Config c = genCfg(j);
      begin(fam == 0 ? "brent" : "bisection", TS_SCHEMA "ffffi"); printTS(t); F(a); F(b); printCfg(c); bar();
      CASE(MathUtils::ScalarResult r = (fam == 0) ? Brent(t, a, b, c) : Bisection(t, a, b, c); prS(r, t);)
    }
  for (int j = 0; j < 16; ++j) {  // bisection_newton
    smash();
    TS t = genTS(j, j % 7 == 5);
    double a = rv(), b = (j == 1) ? a : rv();
    bracketIt(t, j, a, b);
    MathUtils::Config c = genCfg(j);
    begin("bisection_newton", TS_SCHEMA "ffffi"); printTS(t); F(a); F(b); printCfg(c); bar();
    CASE(prS(BisectionNewton(t, a, b, c), t);)
  }
  for (int j = 0; j < 16; ++j) {  // secant
    smash();
    TS t = genTS(j, j % 7 == 5);
    double x0 = rv(), x1 = (j == 1) ? x0 : rv();
    MathUtils::Config c = genCfg(j);
    begin("secant", TS_SCHEMA "ffffi"); printTS(t); F(x0); F(x1); printCfg(c); bar();
    CASE(prS(Secant(t, x0, x1, c), t);)
  }
  for (int j = 0; j < 16; ++j) {  // secant_auto
    smash();
    TS t = genTS(j, j % 7 == 5);
    double x0 = (j % 3 == 0) ? 2.0 * rv() + 3.0 : rv();
    MathUtils::Config c = genCfg(j);
    begin("secant_auto", TS_SCHEMA "fffi"); printTS(t); F(x0); printCfg(c); bar();
    CASE(prS(SecantAuto(t, x0, c), t);)
  }
  for (int j = 0; j < 16; ++j) {  // newton
    smash();
    TS t = genTS(j, j % 7 == 5);
    double x0 = rv();
    if (j == 4) { t.kind = 3; x0 = 0.0; }  // stationary point: derivative 5 * p0 != 0; use the plateau for zero derivative
    if (j == 8) { t.kind = 1; x0 = t.p1; }
    MathUtils::Config c = genCfg(j);
    begin("newton", TS_SCHEMA "fffi"); printTS(t); F(x0); printCfg(c); bar();
    CASE(prS(Newton(t, x0, c), t);)
  }
  for (int j = 0; j < 16; ++j) {  // newton_bounded
    smash();
    TS t = genTS(j, j % 7 == 5);
    double lo = rv() - 1.0, up = lo + 0.25 + std::abs(rv()) * 2.0;
    if (j == 3) up = lo;
    double x0 = (j % 3 == 0) ? lo - 1.0 : rv();
    MathUtils::Config c = genCfg(j);
    begin("newton_bounded", TS_SCHEMA "fffffi"); printTS(t); F(x0); F(lo); F(up); printCfg(c); bar();
    CASE(prS(NewtonBounded(t, x0, lo, up, c), t);)
  }
  // -- trigonometric ---------------------------------------------------------------------------------
  const double LOW = std::numeric_limits<double>::lowest(), HIGH = std::numeric_limits<double>::max();
  auto trigIn = [&](int j, double& a, double& b, double& c, double& d, double& e, double& inf, double& sup, double& eps) {
    a = rv(); b = rv(); c = rv(); d = rv(); e = rv();
    switch (j % 8) {
      case 1: a = 0.0; b = 0.0; break;
      case 2: a = 0.0; b = 0.0; c = 0.0; break;
      case 3: a = 0.0; b = 0.0; d = 0.0; break;
      case 4: a = 0.0; e = 0.0; break;
      case 5: a = 0.0; e = 0.0; c = 0.0; break;
      case 6: a = 0.0; e = 0.0; d = 0.0; break;
      case 7: a = 0.0; b = 0.0; c = 0.0; d = 0.0; if (j > 8) e = 0.0; break;
      default: break;
    }
    if (j % 8 == 1 && j > 8) { c = 0.0; d = 0.0; e = 0.5 * rv(); }
    switch (j % 5) {
      case 0: inf = 0.0; sup = 6.283185307179586; break;
      case 1: inf = -3.141592653589793; sup = 3.141592653589793; break;
      case 2: inf = LOW; sup = HIGH; break;
      case 3: inf = 1.0; sup = HIGH; break;
      default: inf = LOW; sup = 2.0; break;
    }
    if (j == 9) { inf = 0.5; sup = 2.5; }
    eps = (j % 6 == 5) ? 1e-9 : 1.5e-12;
  };
  for (int fam = 0; fam < 3; ++fam)
    for (int j = 0; j < 16; ++j) {  // trigonometric, trigonometric_linear, trigonometric_cde
      smash();
      double a, b, c, d, e, inf, sup, eps;
      trigIn(j, a, b, c, d, e, inf, sup, eps);
      if (fam == 1) { c = 0.0; }
      if (fam == 0) begin("trigonometric", "ffffffff"); else if (fam == 1) begin("trigonometric_linear", "ffff"); else begin("trigonometric_cde", "fffff");
      if (fam == 0) { F(a); F(b); F(c); F(d); F(e); F(inf); F(sup); F(eps); }
      else if (fam == 1) { F(d); F(e); F(inf); F(sup); }
      else { F(c); F(d); F(e); F(inf); F(sup); }
      bar();
      CASE(TrigResult r = (fam == 0) ? Trigonometric(a, b, c, d, e, inf, sup, eps) : (fam == 1 ? TrigonometricLinear(d, e, inf, sup) : TrigonometricCDE(c, d, e, inf, sup));
           I((int)r.Status); I(r.NbRoots); B(r.InfiniteRoots); for (int i = 0; i < 4; ++i) F(r.Roots[i]);)
    }
  // -- multiple roots ---------------------------------------------------------------------------------
  for (int fam = 0; fam < 3; ++fam)
    for (int j = 0; j < 16; ++j) {  // find_all_roots, find_all_roots_with_derivative, find_all_roots_nb_samples
      smash();
      TS t = genTS(j, j % 9 == 7);
      double a = rv() - 1.0, b = a + 0.5 + std::abs(rv()) * 2.0;
      if (j % 4 == 3) { t.kind = 3; a = -1.25; b = 1.25; }
      if (j == 5) std::swap(a, b);
      MultipleConfig c = genMC(j);
      if (fam == 2) {
        begin("find_all_roots_nb_samples", TS_SCHEMA "ffi"); printTS(t); F(a); F(b); I(c.NbSamples); bar();
        CASE(prM(FindAllRoots(t, a, b, c.NbSamples), t);)
      } else {
        begin(fam == 0 ? "find_all_roots" : "find_all_roots_with_derivative", TS_SCHEMA "ff" "ifffif"); printTS(t); F(a); F(b); printMC(c); bar();
        CASE(prM(fam == 0 ? FindAllRoots(t, a, b, c) : FindAllRootsWithDerivative(t, a, b, c), t);)
      }
    }
  for (int j = 0; j < 16; ++j) {  // find_multiple_roots
    smash();
    TS t = genTS(j, j % 9 == 7);
    double a = rv() - 1.0, b = a + 0.5 + std::abs(rv()) * 2.0;
    if (j % 4 == 3) { t.kind = 3; a = -1.25; b = 1.25; }
    int ns = (j % 3 == 0) ? 10 : ((j % 3 == 1) ? 3 : 25);
    double ex = (j % 2) ? 1e-10 : 1e-6, ef = (j % 5 == 0) ? 1e-3 : 1e-10, off = (j % 4 == 3) ? 0.25 * rv() : 0.0;
    begin("find_multiple_roots", TS_SCHEMA "ffifff"); printTS(t); F(a); F(b); I(ns); F(ex); F(ef); F(off); bar();
    CASE(prM(FindMultipleRoots(t, a, b, ns, ex, ef, off), t); TS t2 = t; t2.calls = 0; t2.hash = H0; prM(FindMultipleRoots(t2, a, b, ns, ex, ef), t2);)
  }
  // plateaus: functions that are null on intervals
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 16; ++j) {  // find_all_roots_with_intervals (samples), find_all_roots_with_intervals_range
      smash();
      TS t = genTS(j, false);
      t.kind = (j % 3 == 2) ? 3 : 1;
      if (t.kind == 1) { t.p1 = rv() * 0.5; t.p2 = 0.1 + std::abs(rv()) * 0.3; }
      double a = -1.25 - 0.25 * (j % 3), b = 1.25 + 0.25 * (j % 2);
      int n = 4 + (j * 5) % 24;
      double ex = (j % 2) ? 1e-10 : 1e-6, ef = (j % 4 == 0) ? 1e-3 : 1e-10, en = (j % 5 == 0) ? 1e-3 : 1e-10;
      if (fam == 0) {
        int lb = (j % 3);
        math_Vector s = (j == 15) ? math_Vector(lb, lb, 0.5) : math_Vector(lb, lb + n - 1);  // j == 15: fewer than two samples
        if (j != 15) for (int i = 0; i < n; ++i) s(lb + i) = a + (b - a) * i / (n - 1);
        begin("find_all_roots_with_intervals", TS_SCHEMA "Vfff"); printTS(t); V(s); F(ex); F(ef); F(en); bar();
        CASE(prA(FindAllRootsWithIntervals(t, s, ex, ef, en), t);)
      } else {
        begin("find_all_roots_with_intervals_range", TS_SCHEMA "ffifff"); printTS(t); F(a); F(b); I(n); F(ex); F(ef); F(en); bar();
        CASE(prA(FindAllRootsWithIntervals(t, a, b, n, ex, ef, en), t);)
      }
    }
  // -- helpers -------------------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // sort_roots_add_root
    smash();
    int n = j % 9;
    MultipleResult r;
    double epsx = (j % 3 == 0) ? 1e-10 : 0.3;
    std::vector<double> in;
    for (int i = 0; i < n; ++i) in.push_back(std::floor(rv() * 4.0) / 4.0);
    math_Vector iv(1, std::max(1, n), rv());
    for (int i = 0; i < n; ++i) iv(1 + i) = in[i];
    begin("sort_roots_add_root", "iVf"); I(n); V(iv); F(epsx); bar();
    CASE(for (int i = 0; i < n; ++i) AddRoot(r, epsx, in[i], in[i] * 2.0 + 1.0);
         I(r.NbRoots()); for (int i = 0; i < r.NbRoots(); ++i) { F(r.Roots.Value(i)); F(r.Values.Value(i)); }
         MathRoot::SortRoots(r); for (int i = 0; i < r.NbRoots(); ++i) { F(r.Roots.Value(i)); F(r.Values.Value(i)); })
  }
  for (int j = 0; j < 16; ++j) {  // effective_x_tolerance
    smash();
    double lo = rv() * ((j % 3 == 0) ? 1e6 : 1.0), up = lo + std::abs(rv()) * ((j % 4 == 0) ? 1e6 : 1.0), xt = (j % 2) ? 1e-10 : 1e-6;
    begin("effective_x_tolerance", "fff"); F(lo); F(up); F(xt); bar();
    CASE(F(EffectiveXTolerance(lo, up, xt));)
  }
  for (int j = 0; j < 16; ++j) {  // refine_bracketed_root
    smash();
    TS t = genTS(j, j % 7 == 5);
    t.kind = j % 4 == 3 ? 0 : t.kind;
    double x1 = rv() - 1.0, x2 = x1 + 0.25 + std::abs(rv()) * 1.5;
    double y1 = 0.0, y2 = 0.0;
    { TS p = t; p.flo = 1.0; p.fhi = 0.0; p.Value(x1, y1); p.Value(x2, y2); }
    double off = (j % 4 == 2) ? 0.25 * rv() : 0.0;
    y1 -= off; y2 -= off;
    double tol = (j % 2) ? 1e-10 : 1e-6, epsx = (j % 3 == 0) ? 1e-10 : 1e-3;
    begin("refine_bracketed_root", TS_SCHEMA "fffffff"); printTS(t); F(off); F(x1); F(y1); F(x2); F(y2); F(tol); F(epsx); bar();
    CASE(MultipleResult r; r.NbIterations = 7; bool ok = RefineBracketedRoot(t, off, x1, y1, x2, y2, tol, epsx, r); B(ok); I(r.NbIterations);
         I(r.NbRoots()); for (int i = 0; i < r.NbRoots(); ++i) { F(r.Roots.Value(i)); F(r.Values.Value(i)); } outCalls(t);)
  }
  for (int j = 0; j < 10; ++j) {  // result_types
    smash();
    begin("result_types", "i"); I(j); bar();
    CASE(MultipleResult m; MultipleConfig c; NullInterval n; AllRootsResult a; TrigResult tr; I((int)m.Status); I(m.NbIterations); B(m.IsAllNull);
         I(m.NbRoots()); B(m.IsDone()); I(c.NbSamples); F(c.XTolerance); F(c.FTolerance); F(c.NullTolerance); I(c.MaxIterations); F(c.Offset);
         F(n.A); F(n.B); I(n.State); I((int)a.Status); I(a.NbRoots()); I(a.NbIntervals()); B(a.IsDone()); I((int)tr.Status); I(tr.NbRoots);
         B(tr.InfiniteRoots); B(tr.IsDone()); for (int i = 0; i < 4; ++i) F(tr.Roots[i]);
         m.Status = static_cast<MathUtils::Status>(j); a.Status = m.Status; tr.Status = m.Status; B(m.IsDone()); B(a.IsDone()); B(tr.IsDone());)
  }
  return 0;
}
