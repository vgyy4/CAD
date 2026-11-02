// Deterministic reference records for the header-only OCCT MathOpt package (OCCT 8.0.1), see docs/api-mathopt.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal; a matrix is `lowerRow upperRow lowerCol upperCol values(row-major)`, a vector `lower upper values`.
// Callbacks are a closed set of functions described by printed parameters; every call is counted and its argument bits
// are folded into a 64-bit hash (the call sequence is part of the behaviour).
#include <MathOpt_BFGS.hxx>
#include <MathOpt_Brent.hxx>
#include <MathOpt_FRPR.hxx>
#include <MathOpt_GlobOpt.hxx>
#include <MathOpt_Newton.hxx>
#include <MathOpt_PSO.hxx>
#include <MathOpt_Powell.hxx>
#include <MathOpt_Uzawa.hxx>
#include <Standard_Failure.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

using namespace MathOpt;

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
static void M(const math_Matrix& m) {
  std::string keep = sig;
  line += ' ' + std::to_string(m.LowerRow()) + ' ' + std::to_string(m.UpperRow()) + ' ' + std::to_string(m.LowerCol()) + ' ' +
          std::to_string(m.UpperCol());
  for (int i = m.LowerRow(); i <= m.UpperRow(); ++i)
    for (int j = m.LowerCol(); j <= m.UpperCol(); ++j) F(m(i, j));
  if (inputs) sig = keep + 'M';
}
static void V(const math_Vector& v) {
  std::string keep = sig;
  line += ' ' + std::to_string(v.Lower()) + ' ' + std::to_string(v.Upper());
  for (int i = v.Lower(); i <= v.Upper(); ++i) F(v(i));
  if (inputs) sig = keep + 'V';
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
static const int LB[4] = {1, 0, 3, -2};
static math_Vector genV(int l, int n, double scale = 1.0) {
  math_Vector v(l, l + n - 1);
  for (int i = l; i < l + n; ++i) v(i) = rv() * scale;
  return v;
}
static math_Vector genAround(int l, int n, double base) {
  math_Vector v(l, l + n - 1);
  for (int i = l; i < l + n; ++i) v(i) = base + 0.5 * rv();
  return v;
}
static math_Matrix genM(int lr, int m, int lc, int n) {
  math_Matrix a(lr, lr + m - 1, lc, lc + n - 1, 0.0);
  for (int i = lr; i < lr + m; ++i)
    for (int j = lc; j < lc + n; ++j) a(i, j) = rv();
  return a;
}
static math_Vector mv(const std::vector<double>& a) {
  math_Vector v(1, (int)a.size());
  for (size_t i = 0; i < a.size(); ++i) v(1 + (int)i) = a[i];
  return v;
}

static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;

// ---- scalar callback: f(x) = (x - a)^2 (1 + c (x - b)^2) + d, failing on [fl, fh] ---------------------------------------
struct TS {
  double a, b, c, d, fl, fh;
  int calls = 0;
  uint64_t hash = H0;
  bool Value(double x, double& f) {
    ++calls;
    hash = mixh(hash, bitsOf(x));
    if (x >= fl && x <= fh) return false;
    double t = x - a, u = x - b;
    f = t * t * (1.0 + c * u * u) + d;
    return true;
  }
};
#define TS_SCHEMA "ffffff"
static TS genTS(int j) {
  TS t;
  t.a = rv();
  t.b = rv();
  t.c = (j % 3 == 0) ? 0.0 : 0.5 * std::abs(rv());
  t.d = rv();
  if (j % 5 == 2) { t.fl = rv(); t.fh = t.fl + 0.25; } else { t.fl = 1.0; t.fh = 0.0; }
  return t;
}
static void printTS(const TS& t) { F(t.a); F(t.b); F(t.c); F(t.d); F(t.fl); F(t.fh); }

// ---- vector callback: f = sum w_i (x_i - c_i)^2 + q sum (x_(i+1) - x_i^2)^2 + k sum x_i^4 -----------------------------
struct TV {
  std::vector<double> w, c;
  double q, k, failV, failG, failH, wLo, wHi;
  int hmode;  // 0: exact Hessian, 1: zero Hessian, 2: negated Hessian
  int vcalls = 0, gcalls = 0, hcalls = 0;
  uint64_t hash = H0;
  void mixx(const math_Vector& x, uint64_t salt) {
    hash = mixh(hash, salt);
    for (int i = x.Lower(); i <= x.Upper(); ++i) hash = mixh(hash, bitsOf(x(i)));
  }
  double W(int i) const { return w[(size_t)i % w.size()]; }
  double C(int i) const { return c[(size_t)i % c.size()]; }
  bool Value(const math_Vector& x, double& f) {
    ++vcalls;
    mixx(x, 1);
    double x0 = x(x.Lower());
    if (x0 > failV || (x0 >= wLo && x0 <= wHi)) return false;
    int n = x.Length();
    double s = 0.0;
    for (int i = 0; i < n; ++i) {
      double d = x(x.Lower() + i) - C(i);
      s += W(i) * d * d;
    }
    for (int i = 0; i + 1 < n; ++i) {
      double r = x(x.Lower() + i + 1) - x(x.Lower() + i) * x(x.Lower() + i);
      s += q * r * r;
    }
    for (int i = 0; i < n; ++i) {
      double xi2 = x(x.Lower() + i) * x(x.Lower() + i);
      s += k * xi2 * xi2;
    }
    f = s;
    return true;
  }
  bool Gradient(const math_Vector& x, math_Vector& g) {
    ++gcalls;
    mixx(x, 2);
    if (x(x.Lower()) > failG) return false;
    int n = x.Length();
    for (int i = 0; i < n; ++i) g(g.Lower() + i) = 2.0 * W(i) * (x(x.Lower() + i) - C(i));
    for (int i = 0; i + 1 < n; ++i) {
      double xi = x(x.Lower() + i);
      double r = x(x.Lower() + i + 1) - xi * xi;
      g(g.Lower() + i) += 2.0 * q * r * (-2.0 * xi);
      g(g.Lower() + i + 1) += 2.0 * q * r;
    }
    for (int i = 0; i < n; ++i) {
      double xi = x(x.Lower() + i);
      g(g.Lower() + i) += 4.0 * k * xi * xi * xi;
    }
    return true;
  }
  bool Hessian(const math_Vector& x, math_Matrix& h) {
    ++hcalls;
    mixx(x, 3);
    if (x(x.Lower()) > failH) return false;
    int n = x.Length();
    int r0 = h.LowerRow(), c0 = h.LowerCol();
    for (int i = 0; i < n; ++i)
      for (int j = 0; j < n; ++j) h(r0 + i, c0 + j) = 0.0;
    if (hmode == 1) return true;
    for (int i = 0; i < n; ++i) h(r0 + i, c0 + i) += 2.0 * W(i);
    for (int i = 0; i + 1 < n; ++i) {
      double xi = x(x.Lower() + i);
      double r = x(x.Lower() + i + 1) - xi * xi;
      h(r0 + i, c0 + i) += 2.0 * q * (4.0 * xi * xi - 2.0 * r);
      h(r0 + i, c0 + i + 1) += 2.0 * q * (-2.0 * xi);
      h(r0 + i + 1, c0 + i) += 2.0 * q * (-2.0 * xi);
      h(r0 + i + 1, c0 + i + 1) += 2.0 * q;
    }
    for (int i = 0; i < n; ++i) {
      double xi = x(x.Lower() + i);
      h(r0 + i, c0 + i) += 12.0 * k * xi * xi;
    }
    if (hmode == 2)
      for (int i = 0; i < n; ++i)
        for (int j = 0; j < n; ++j) h(r0 + i, c0 + j) = -h(r0 + i, c0 + j);
    return true;
  }
};
#define TV_SCHEMA "VVfffffffi"
static TV genTV(int j, int n) {
  TV t;
  for (int i = 0; i < n; ++i) {
    t.w.push_back(0.5 + std::abs(rv()) * 1.5);
    t.c.push_back(rv());
  }
  t.q = (j % 3 == 0) ? 0.0 : ((j % 3 == 1) ? 0.125 * std::abs(rv()) : 2.0 * std::abs(rv()));
  t.k = (j % 4 == 0) ? 0.0 : 0.0625 * std::abs(rv());
  t.failV = (j % 13 == 6) ? 1.5 : 1e300;
  t.failG = (j % 17 == 9) ? 0.75 : 1e300;
  t.failH = (j % 19 == 11) ? 0.5 : 1e300;
  t.wLo = 1.0;
  t.wHi = 0.0;
  t.hmode = 0;
  return t;
}
static void printTV(const TV& t) {
  V(mv(t.w));
  V(mv(t.c));
  F(t.q); F(t.k); F(t.failV); F(t.failG); F(t.failH); F(t.wLo); F(t.wHi);
  I(t.hmode);
}

// ---- results -------------------------------------------------------------------------------------------------------
static void prSR(const MathUtils::ScalarResult& r, const TS& t) {
  I((int)r.Status); I((long long)r.NbIterations);
  B(r.Root.has_value()); if (r.Root) F(*r.Root);
  B(r.Value.has_value()); if (r.Value) F(*r.Value);
  B(r.Derivative.has_value()); if (r.Derivative) F(*r.Derivative);
  I(t.calls); U(t.hash);
}
static void prCalls(const TV& t) { I(t.vcalls); I(t.gcalls); I(t.hcalls); U(t.hash); }
static void prVR(const MathUtils::VectorResult& r, const TV& t) {
  I((int)r.Status); I((long long)r.NbIterations);
  B(r.Solution.has_value()); if (r.Solution) V(*r.Solution);
  B(r.Value.has_value()); if (r.Value) F(*r.Value);
  B(r.Gradient.has_value()); if (r.Gradient) V(*r.Gradient);
  B(r.Jacobian.has_value()); if (r.Jacobian) M(*r.Jacobian);
  prCalls(t);
}
static void prStats(const PSOStats& s) {
  I(s.NbFunctionEvals); I(s.NbIterations); I(s.NbBoundaryCorrections); I(s.NbStagnationEvents); I(s.NbRestarts);
  F(s.InitialBest); F(s.FinalBest);
}

// ---- configurations ------------------------------------------------------------------------------------------------
static MathUtils::Config genCfg(int j, int iters = 100) {
  MathUtils::Config c;
  c.XTolerance = (j % 3 == 0) ? 1e-12 : ((j % 3 == 1) ? 1e-8 : 1e-6);
  c.FTolerance = (j % 4 == 0) ? 1e-10 : ((j % 4 == 1) ? 1e-6 : 1e-12);
  c.Tolerance = (j % 2) ? 1e-8 : 1e-10;
  c.MaxIterations = (j % 7 == 3) ? 3 : ((j % 11 == 5) ? 1 : iters);
  c.StepMin = (j % 5 == 2) ? 1e-9 : std::numeric_limits<double>::epsilon();
  return c;
}
static void printCfg(const MathUtils::Config& c) { F(c.Tolerance); F(c.XTolerance); F(c.FTolerance); I(c.MaxIterations); F(c.StepMin); }
#define CFG_SCHEMA "fffif"
static NewtonConfig genNCfg(int j) {
  NewtonConfig nc;
  static_cast<MathUtils::Config&>(nc) = genCfg(j, 60);
  nc.Regularization = (j % 3 == 0) ? 1e-8 : ((j % 3 == 1) ? 1e-4 : 0.0);
  nc.UseLineSearch = (j % 2 == 0);
  return nc;
}
static void printNCfg(const NewtonConfig& c) { printCfg(c); F(c.Regularization); B(c.UseLineSearch); }
#define NCFG_SCHEMA CFG_SCHEMA "fi"
static void printFCfg(const FRPRConfig& c) { printCfg(c); I((int)c.Formula); I(c.RestartInterval); }
#define FCFG_SCHEMA CFG_SCHEMA "ii"
static void printPCfg(const PSOConfig& c) {
  printCfg(c); B(c.UseBounds); I(c.NbParticles); F(c.Omega); F(c.PhiPersonal); F(c.PhiGlobal); F(c.VelocityClamp);
  U(c.Seed); I((int)c.InitMode); I((int)c.BoundaryMode); I((int)c.InertiaSchedule); F(c.OmegaMin); I(c.MinIterations);
  B(c.TargetValue.has_value()); F(c.TargetValue.value_or(0.0)); F(c.NoImproveTol); I(c.NoImproveIters);
  F(c.RestartFraction); I(c.MaxRestarts); I(c.PolishBudgetPerDim);
}
#define PCFG_SCHEMA CFG_SCHEMA "iiffffuiiifiiffifii"
static void printGCfg(const GlobalConfig& c) {
  printCfg(c); B(c.UseBounds); I((int)c.Strategy); I(c.NbPopulation); I(c.NbStarts); F(c.MutationScale); F(c.CrossoverProb);
  U(c.Seed); I(c.PolishBudgetPerDim);
}
#define GCFG_SCHEMA CFG_SCHEMA "iiiiffui"

static PSOConfig genPCfg(int j) {
  static const int NP[6] = {8, 12, 20, 5, 30, 10};
  static const int MI[6] = {40, 80, 15, 100, 6, 60};
  static const double OM[3] = {0.7, 0.9, 0.5}, PP[3] = {1.5, 2.0, 1.0}, PG[3] = {1.5, 1.0, 2.0}, VC[3] = {0.5, 0.2, 1.0};
  static const unsigned SD[4] = {6u, 1u, 12345u, 0xdeadbeefu};
  PSOConfig c;
  c.NbParticles = NP[j % 6];
  c.MaxIterations = MI[(j / 2) % 6];
  c.Tolerance = (j % 5 == 1) ? 1e-4 : 1e-8;
  c.Omega = OM[j % 3];
  c.PhiPersonal = PP[(j / 3) % 3];
  c.PhiGlobal = PG[(j / 2) % 3];
  c.VelocityClamp = VC[j % 3];
  c.Seed = SD[j % 4];
  c.InitMode = (PSOInitMode)(j % 3);
  c.BoundaryMode = (PSOBoundaryMode)((j / 3) % 3);
  c.InertiaSchedule = (PSOInertiaSchedule)(j % 2);
  c.OmegaMin = (j % 4 == 1) ? 0.2 : 0.4;
  c.MinIterations = (j % 4 == 2) ? 5 : ((j % 4 == 3) ? 20 : 0);
  if (j % 7 == 3) c.TargetValue = 1.0;
  if (j % 7 == 6) c.TargetValue = 1e9;
  c.NoImproveTol = (j % 5 == 1) ? 1e-3 : 0.0;
  c.NoImproveIters = (j % 3 == 0) ? 10 : ((j % 3 == 1) ? 3 : 5);
  c.RestartFraction = (j % 4 == 0) ? 0.0 : ((j % 4 == 1) ? 0.3 : ((j % 4 == 2) ? 0.5 : 1.0));
  c.MaxRestarts = (j % 2) ? 2 : 0;
  c.PolishBudgetPerDim = (j % 4 == 0) ? 50 : ((j % 4 == 1) ? 0 : ((j % 4 == 2) ? 10 : 200));
  return c;
}

// seeds: a count followed, for each seed, by its position, a value flag and value, a velocity flag and velocity
typedef NCollection_DynamicArray<PSOSeedParticle> Seeds;
static Seeds genSeeds(int j, int l, int n, int count, std::string& schema) {
  Seeds s;
  for (int k = 0; k < count; ++k) {
    int ll = (j % 17 == 5 && k == 0) ? l + 1 : l;  // a seed with another lower bound is invalid
    math_Vector pos = genV(ll, n, 1.5);
    PSOSeedParticle sp(pos);
    if ((j + k) % 2 == 0) sp.Value = rv();
    if ((j + k) % 3 == 0) sp.Velocity = genV((j % 19 == 7 && k == 0) ? l - 1 : l, n, 3.0);
    s.Append(sp);
  }
  I(count);
  schema += "i";
  for (int k = 0; k < count; ++k) {
    const PSOSeedParticle& sp = s.Value(k);
    V(sp.Position);
    B(sp.Value.has_value());
    F(sp.Value.value_or(0.0));
    B(sp.Velocity.has_value());
    V(sp.Velocity ? *sp.Velocity : math_Vector(sp.Position.Lower(), sp.Position.Upper(), 0.0));
    schema += "VifiV";
  }
  return s;
}

int main() {
  // -- scalar minimisers -------------------------------------------------------------------------------------------
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 28; ++j) {
      TS t = genTS(j);
      MathUtils::Config c = genCfg(j);
      double lo = -3.0 + rv(), hi = 3.0 + rv();
      if (j % 9 == 8) std::swap(lo, hi);
      if (j % 10 == 9) hi = lo;
      begin(fam == 0 ? "brent" : "golden", TS_SCHEMA "ff" CFG_SCHEMA);
      printTS(t); F(lo); F(hi); printCfg(c);
      bar();
      if (fam == 0) CASE(auto r = Brent(t, lo, hi, c); prSR(r, t);)
      else CASE(auto r = Golden(t, lo, hi, c); prSR(r, t);)
    }
  for (int j = 0; j < 24; ++j) {
    TS t = genTS(j);
    MathUtils::Config c = genCfg(j);
    double guess = 2.0 * rv();
    double step = (j % 4 == 0) ? 1.0 : ((j % 4 == 1) ? 0.5 : ((j % 4 == 2) ? -0.75 : 4.0));
    if (j == 7) step = 0.0;
    begin("brent_with_bracket", TS_SCHEMA "ff" CFG_SCHEMA);
    printTS(t); F(guess); F(step); printCfg(c);
    bar();
    CASE(auto r = BrentWithBracket(t, guess, step, c); prSR(r, t);)
  }

  // -- Powell -------------------------------------------------------------------------------------------------------
  for (int j = 0; j < 28; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    MathUtils::Config c = genCfg(j, 60);
    math_Vector x0 = genV(l, n, 1.5);
    begin("powell", TV_SCHEMA "V" CFG_SCHEMA);
    printTV(t); V(x0); printCfg(c);
    bar();
    CASE(auto r = Powell(t, x0, c); prVR(r, t);)
  }
  for (int j = 0; j < 24; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    MathUtils::Config c = genCfg(j, 60);
    math_Vector x0 = genV(l, n, 1.5);
    int dr = (j % 9 == 4) ? n + 1 : n;                       // wrong row count: invalid input
    int lr = (j % 6 == 5) ? 3 : 1, lc = (j % 6 == 5) ? -2 : 1;  // another lower bound
    math_Matrix dirs = genM(lr, dr, lc, n);
    for (int i = 0; i < n && i < dr; ++i) dirs(lr + i, lc + i) += 2.0;
    begin("powell_with_directions", TV_SCHEMA "V" "M" CFG_SCHEMA);
    printTV(t); V(x0); M(dirs); printCfg(c);
    bar();
    CASE(auto r = PowellWithDirections(t, x0, dirs, c); prVR(r, t);)
  }

  // -- conjugate gradients -------------------------------------------------------------------------------------------
  for (int f = 0; f < 4; ++f)
    for (int j = 0; j < 9; ++j) {
      int jj = j + 9 * f;
      int n = 1 + jj % 4, l = LB[(jj / 3) % 4];
      TV t = genTV(jj, n);
      FRPRConfig c;
      static_cast<MathUtils::Config&>(c) = genCfg(jj, 80);
      c.Formula = (ConjugateGradientFormula)f;
      c.RestartInterval = (j % 3 == 0) ? 0 : ((j % 3 == 1) ? 1 : 2);
      math_Vector x0 = genV(l, n, 1.5);
      begin("frpr", TV_SCHEMA "V" FCFG_SCHEMA);
      printTV(t); V(x0); printFCfg(c);
      bar();
      CASE(auto r = FRPR(t, x0, c); prVR(r, t);)
    }
  for (int j = 0; j < 16; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    FRPRConfig c;
    static_cast<MathUtils::Config&>(c) = genCfg(j, 80);
    c.Formula = (ConjugateGradientFormula)(j % 4);
    c.RestartInterval = (j % 5 == 0) ? 3 : 0;
    double step = (j % 2) ? 1e-8 : 1e-6;
    math_Vector x0 = genV(l, n, 1.5);
    begin("frpr_numerical", TV_SCHEMA "V" "f" FCFG_SCHEMA);
    printTV(t); V(x0); F(step); printFCfg(c);
    bar();
    CASE(auto r = FRPRNumerical(t, x0, step, c); prVR(r, t);)
  }

  // -- quasi-Newton --------------------------------------------------------------------------------------------------
  for (int j = 0; j < 28; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    MathUtils::Config c = genCfg(j, 80);
    math_Vector x0 = genV(l, n, 1.5);
    begin("bfgs", TV_SCHEMA "V" CFG_SCHEMA);
    printTV(t); V(x0); printCfg(c);
    bar();
    CASE(auto r = BFGS(t, x0, c); prVR(r, t);)
  }
  for (int j = 0; j < 16; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    MathUtils::Config c = genCfg(j, 80);
    double step = (j % 2) ? 1e-8 : 1e-6;
    math_Vector x0 = genV(l, n, 1.5);
    begin("bfgs_numerical", TV_SCHEMA "V" "f" CFG_SCHEMA);
    printTV(t); V(x0); F(step); printCfg(c);
    bar();
    CASE(auto r = BFGSNumerical(t, x0, step, c); prVR(r, t);)
  }
  for (int j = 0; j < 24; ++j) {
    static const int MEM[8] = {1, 2, 3, 4, 5, 10, 7, 15};
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    int mem = MEM[(j / 3) % 8];
    TV t = genTV(j, n);
    MathUtils::Config c = genCfg(j, 80);
    math_Vector x0 = genV(l, n, 1.5);
    begin("lbfgs", TV_SCHEMA "V" "i" CFG_SCHEMA);
    printTV(t); V(x0); I(mem); printCfg(c);
    bar();
    CASE(auto r = LBFGS(t, x0, mem, c); prVR(r, t);)
  }
  for (int j = 0; j < 28; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4], lb = LB[(j + 1) % 4];
    TV t = genTV(j, n);
    MathUtils::Config c = genCfg(j, 80);
    math_Vector x0 = genV(l, n, 1.5 + (j % 5));
    math_Vector lo = genAround(lb, n, -1.0), hi = genAround((j % 7 == 3) ? LB[(j + 2) % 4] : lb, (j % 11 == 10) ? n + 1 : n, 1.0);
    if (j % 13 == 12) std::swap(lo, hi);
    begin("bfgs_bounded", TV_SCHEMA "VVV" CFG_SCHEMA);
    printTV(t); V(x0); V(lo); V(hi); printCfg(c);
    bar();
    CASE(auto r = BFGSBounded(t, x0, lo, hi, c); prVR(r, t);)
  }

  // -- Newton --------------------------------------------------------------------------------------------------------
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < (fam == 0 ? 28 : 8); ++j) {
      int n = 1 + j % 4, l = LB[(j / 4) % 4];
      TV t = genTV(j, n);
      t.hmode = (j % 7 == 3) ? 1 : ((j % 7 == 5) ? 2 : 0);
      NewtonConfig c = genNCfg(j);
      math_Vector x0 = genV(l, n, 1.5);
      begin(fam == 0 ? "newton" : "newton_modified", TV_SCHEMA "V" NCFG_SCHEMA);
      printTV(t); V(x0); printNCfg(c);
      bar();
      if (fam == 0) CASE(auto r = Newton(t, x0, c); prVR(r, t);)
      else CASE(auto r = NewtonModified(t, x0, c); prVR(r, t);)
    }
  for (int j = 0; j < 16; ++j) {
    int n = 1 + j % 4, l = (j % 5 == 4) ? LB[(j / 4) % 4] : 1;  // a lower bound other than 1 makes OCCT's numerical Hessian throw
    TV t = genTV(j, n);
    NewtonConfig c = genNCfg(j);
    double hstep = (j % 2) ? 1e-6 : 1e-4;
    math_Vector x0 = genV(l, n, 1.5);
    begin("newton_numerical_hessian", TV_SCHEMA "V" "f" NCFG_SCHEMA);
    printTV(t); V(x0); F(hstep); printNCfg(c);
    bar();
    CASE(auto r = NewtonNumericalHessian(t, x0, hstep, c); prVR(r, t);)
  }
  for (int j = 0; j < 16; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    NewtonConfig c = genNCfg(j);
    double gstep = (j % 2) ? 1e-8 : 1e-7, hstep = (j % 3) ? 1e-6 : 1e-4;
    math_Vector x0 = genV(l, n, 1.5);
    begin("newton_numerical", TV_SCHEMA "V" "ff" NCFG_SCHEMA);
    printTV(t); V(x0); F(gstep); F(hstep); printNCfg(c);
    bar();
    CASE(auto r = NewtonNumerical(t, x0, gstep, hstep, c); prVR(r, t);)
  }
  for (int j = 0; j < 28; ++j) {
    int n = 1 + j % 4, l = LB[(j / 4) % 4], lb = LB[(j + 1) % 4];
    TV t = genTV(j, n);
    t.hmode = (j % 7 == 3) ? 1 : ((j % 7 == 5) ? 2 : 0);
    NewtonConfig c = genNCfg(j);
    math_Vector x0 = genV(l, n, 1.5 + (j % 5));
    math_Vector lo = genAround(lb, n, -1.0), hi = genAround((j % 7 == 3) ? LB[(j + 2) % 4] : lb, (j % 11 == 10) ? n + 1 : n, 1.0);
    if (j % 13 == 12) std::swap(lo, hi);
    begin("newton_bounded", TV_SCHEMA "VVV" NCFG_SCHEMA);
    printTV(t); V(x0); V(lo); V(hi); printNCfg(c);
    bar();
    CASE(auto r = NewtonBounded(t, x0, lo, hi, c); prVR(r, t);)
  }

  // -- Uzawa -----------------------------------------------------------------------------------------------------------
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < (fam == 0 ? 40 : 16); ++j) {
      int ncol = 1 + j % 4, lig = 1 + (j / 2) % 3;
      int nce = (fam == 1) ? lig : (j * 7) % (lig + 1), nci = lig - nce;
      if (fam == 0 && j % 5 == 0) { nce = 0; nci = lig; }
      if (fam == 0 && j % 4 == 3) { nce = lig; nci = 0; }
      math_Matrix cont = genM(LB[j % 4], lig, LB[(j + 1) % 4], ncol);
      math_Vector sec = genV(LB[(j + 2) % 4], (j % 9 == 5) ? lig + 1 : lig);
      math_Vector x0 = genV(LB[(j + 3) % 4], (j % 9 == 7) ? ncol + 1 : ncol);
      if (j % 9 == 6 && lig >= 2)
        for (int c = 0; c < ncol; ++c) cont(cont.LowerRow() + 1, cont.LowerCol() + c) = cont(cont.LowerRow(), cont.LowerCol() + c);
      if (j % 9 == 8)
        for (int r = 0; r < lig; ++r)
          for (int c = 0; c < ncol; ++c) cont(cont.LowerRow() + r, cont.LowerCol() + c) = 0.0;
      if (fam == 0 && j % 9 == 4) nce += 1;  // the counts no longer add up
      UzawaConfig uc;
      uc.EpsLix = (j % 3 == 0) ? 1e-6 : 1e-9;
      uc.EpsLic = (j % 4 == 0) ? 1e-6 : 1e-8;
      uc.MaxIterations = (j % 6 == 1) ? 20 : ((j % 6 == 2) ? 1 : 500);
      begin(fam == 0 ? "uzawa" : "uzawa_equality", fam == 0 ? "MVViiffi" : "MVVffi");
      M(cont); V(sec); V(x0);
      if (fam == 0) { I(nce); I(nci); }
      F(uc.EpsLix); F(uc.EpsLic); I(uc.MaxIterations);
      bar();
      CASE(UzawaResult r = (fam == 0) ? Uzawa(cont, sec, x0, nce, nci, uc) : UzawaEquality(cont, sec, x0, uc);
           I((int)r.Status); I(r.NbIterations);
           B(r.Solution.has_value()); if (r.Solution) V(*r.Solution);
           B(r.Dual.has_value()); if (r.Dual) V(*r.Dual);
           B(r.Error.has_value()); if (r.Error) V(*r.Error);
           B(r.InitialError.has_value()); if (r.InitialError) V(*r.InitialError);
           B(r.InverseCTC.has_value()); if (r.InverseCTC) M(*r.InverseCTC);
           B(r.IsDone());)
    }

  // -- particle swarm ----------------------------------------------------------------------------------------------------
  for (int j = 0; j < 36; ++j) {
    int n = 1 + j % 3, l = LB[(j / 3) % 4];
    TV t = genTV(j, n);
    if (j % 10 == 4) { t.wLo = 0.5; t.wHi = 0.9; }
    if (j % 15 == 7) t.failV = 1.0;
    PSOConfig c = genPCfg(j);
    if (j % 29 == 11) c.NbParticles = 0;
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, (j % 23 == 7) ? n + 1 : n, 2.0);
    if (j % 19 == 3) lo(l) = hi(l);
    int nseeds = (j % 5 == 2) ? 0 : (j % 4);
    if (c.InitMode == PSOInitMode::SeededOnly && j % 6 == 0) nseeds = 0;  // invalid: no seeds
    std::string schema = TV_SCHEMA "VV" PCFG_SCHEMA;
    begin("pso", "");
    printTV(t); V(lo); V(hi); printPCfg(c);
    Seeds seeds = genSeeds(j, l, n, nseeds, schema);
    bool useSeeds = (nseeds > 0) || (j % 3 == 0);
    PSOStats st;
    bool withStats = (j % 2 == 0);
    B(withStats);
    want = schema + "i";
    bar();
    CASE(auto r = PSO(t, lo, hi, c, useSeeds ? &seeds : nullptr, withStats ? &st : nullptr);
         prVR(r, t); prStats(st);)
  }
  // overload without seeds and stats
  for (int j = 0; j < 8; ++j) {
    int n = 1 + j % 3, l = LB[j % 4];
    TV t = genTV(j + 1, n);
    PSOConfig c = (j % 2) ? PSOConfig(10 + j, 30 + j, 1e-6) : PSOConfig(7 + j);
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, n, 2.0);
    begin("pso_simple", TV_SCHEMA "VV" PCFG_SCHEMA);
    printTV(t); V(lo); V(hi); printPCfg(c);
    bar();
    CASE(auto r = PSO(t, lo, hi, c); prVR(r, t);)
  }
  for (int j = 0; j < 20; ++j) {
    int n = 1 + j % 3, l = LB[(j / 2) % 4];
    TV t = genTV(j, n);
    if (j % 6 == 5) { t.wLo = -1.0; t.wHi = 0.5; }
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, n, 2.0);
    math_Vector pos = genV(l, n, 1.5);
    TV t0 = t;
    t0.wLo = 1.0; t0.wHi = 0.0; t0.failV = 1e300;
    double val = 0.0;
    if (!t0.Value(pos, val)) val = 1e300;
    static const int BUD[7] = {1, 3, 8, 20, 50, 100, 0};
    int budget = BUD[j % 7];
    double tol = (j % 3 == 0) ? 1e-3 : 1e-8;
    begin("polish_coordinate_wise", TV_SCHEMA "VVV" "ffi");
    printTV(t); V(pos); V(lo); V(hi); F(val); F(tol); I(budget);
    bar();
    CASE(int evals = -1; PolishCoordinateWise(t, pos, val, lo, hi, tol, budget, evals); V(pos); F(val); I(evals); prCalls(t);)
  }

  // -- differential evolution, multi-start, global minimum ------------------------------------------------------------------
  for (int j = 0; j < 20; ++j) {
    int n = 1 + j % 3, l = LB[(j / 2) % 4];
    TV t = genTV(j, n);
    if (j % 10 == 4) { t.wLo = 0.5; t.wHi = 0.9; }
    GlobalConfig c;
    static_cast<MathUtils::Config&>(c) = genCfg(j, 60);
    c.Strategy = GlobalStrategy::DifferentialEvolution;
    static const int NPOP[5] = {4, 8, 12, 20, 3};
    c.NbPopulation = NPOP[j % 5];
    c.MutationScale = (j % 3 == 0) ? 0.8 : 0.5;
    c.CrossoverProb = (j % 4 == 0) ? 0.9 : 0.3;
    c.Seed = (j % 2) ? 6u : 99u;
    c.PolishBudgetPerDim = (j % 3 == 1) ? 0 : 50;
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, (j % 17 == 9) ? n + 1 : n, 2.0);
    begin("differential_evolution", TV_SCHEMA "VV" GCFG_SCHEMA);
    printTV(t); V(lo); V(hi); printGCfg(c);
    bar();
    CASE(auto r = DifferentialEvolution(t, lo, hi, c); prVR(r, t);)
  }
  for (int j = 0; j < 16; ++j) {
    int n = 1 + j % 3, l = LB[(j / 2) % 4];
    TV t = genTV(j, n);
    if (j % 8 == 5) t.failV = -10.0;  // every evaluation fails
    if (j % 10 == 4) { t.wLo = 0.5; t.wHi = 0.9; }
    GlobalConfig c;
    static_cast<MathUtils::Config&>(c) = genCfg(j, 60);
    c.Strategy = GlobalStrategy::MultiStart;
    static const int NST[4] = {1, 2, 3, 5};
    c.NbStarts = NST[j % 4];
    c.Seed = (j % 2) ? 6u : 99u;
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, (j % 13 == 9) ? n + 1 : n, 2.0);
    begin("multi_start", TV_SCHEMA "VV" GCFG_SCHEMA);
    printTV(t); V(lo); V(hi); printGCfg(c);
    bar();
    CASE(auto r = MultiStart(t, lo, hi, c); prVR(r, t);)
  }
  for (int j = 0; j < 36; ++j) {
    int n = 1 + j % 3, l = LB[(j / 4) % 4];
    TV t = genTV(j, n);
    if (j % 10 == 4) { t.wLo = 0.5; t.wHi = 0.9; }
    GlobalConfig c;
    static_cast<MathUtils::Config&>(c) = genCfg(j, 60);
    c.Strategy = (GlobalStrategy)(j % 4);
    static const int NPOP[4] = {8, 12, 20, 6};
    static const int NST[4] = {1, 2, 3, 4};
    static const int MIT[4] = {20, 60, 120, 40};
    c.NbPopulation = NPOP[(j / 4) % 4];
    c.NbStarts = NST[(j / 2) % 4];
    c.MaxIterations = (j % 7 == 3) ? 3 : MIT[(j / 3) % 4];
    c.MutationScale = (j % 3 == 0) ? 0.8 : 0.5;
    c.CrossoverProb = (j % 4 == 0) ? 0.9 : 0.3;
    c.Seed = (j % 2) ? 6u : 99u;
    c.PolishBudgetPerDim = (j % 3 == 1) ? 0 : 50;
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, n, 2.0);
    bool withPso = (j % 3 == 1) && (j % 4 == 0 || j % 4 == 2);
    PSOConfig pc = genPCfg(j);
    pc.MaxIterations = 25;
    std::string schema = TV_SCHEMA "VV" GCFG_SCHEMA "i" PCFG_SCHEMA;
    begin("global_minimum", "");
    printTV(t); V(lo); V(hi); printGCfg(c); B(withPso); printPCfg(pc);
    int nseeds = (j % 5 == 2) ? 2 : 0;
    Seeds seeds = genSeeds(j, l, n, nseeds, schema);
    bool withStats = (j % 2 == 0);
    B(withStats);
    want = schema + "i";
    PSOStats st;
    bar();
    CASE(auto r = GlobalMinimum(t, lo, hi, c, withPso ? &pc : nullptr, nseeds ? &seeds : nullptr, withStats ? &st : nullptr);
         prVR(r, t); prStats(st);)
  }
  for (int j = 0; j < 8; ++j) {
    int n = 1 + j % 3, l = LB[j % 4];
    TV t = genTV(j, n);
    GlobalConfig c(GlobalStrategy::PSOHybrid, 30 + j);
    static_cast<MathUtils::Config&>(c) = genCfg(j, 30 + j);
    c.Strategy = (GlobalStrategy)(j % 4);
    c.NbPopulation = 10;
    math_Vector lo = genAround(l, n, -2.0), hi = genAround(l, n, 2.0);
    begin("global_minimum_simple", TV_SCHEMA "VV" GCFG_SCHEMA);
    printTV(t); V(lo); V(hi); printGCfg(c);
    bar();
    CASE(auto r = GlobalMinimum(t, lo, hi, c); prVR(r, t);)
  }

  // -- defaults, constructors, enums -----------------------------------------------------------------------------------------
  for (int kind = 0; kind < 14; ++kind) {
    begin("types", "i");
    I(kind);
    bar();
    CASE(
      switch (kind) {
        case 0: { FRPRConfig c; printFCfg(c); break; }
        case 1: { FRPRConfig c(1e-5); printFCfg(c); break; }
        case 2: { FRPRConfig c(1e-5, 7); printFCfg(c); break; }
        case 3: { NewtonConfig c; printNCfg(c); break; }
        case 4: { NewtonConfig c(1e-3, 9); printNCfg(c); break; }
        case 5: { UzawaConfig c; F(c.EpsLix); F(c.EpsLic); I(c.MaxIterations); break; }
        case 6: { PSOConfig c; printPCfg(c); break; }
        case 7: { PSOConfig c(25); printPCfg(c); break; }
        case 8: { PSOConfig c(25, 77, 1e-3); printPCfg(c); break; }
        case 9: { GlobalConfig c; printGCfg(c); break; }
        case 10: { GlobalConfig c(GlobalStrategy::DifferentialEvolution); printGCfg(c); break; }
        case 11: { GlobalConfig c(GlobalStrategy::MultiStart, 300); printGCfg(c); break; }
        case 12: {
          PSOStats s; prStats(s);
          PSOSeedParticle a(math_Vector(2, 3, 0.5)), b(math_Vector(2, 3, 1.5), 0.25);
          V(a.Position); B(a.Value.has_value()); B(a.Velocity.has_value());
          V(b.Position); B(b.Value.has_value()); F(*b.Value); B(b.Velocity.has_value());
          break;
        }
        default: {
          I((int)ConjugateGradientFormula::FletcherReeves); I((int)ConjugateGradientFormula::PolakRibiere);
          I((int)ConjugateGradientFormula::HestenesStiefel); I((int)ConjugateGradientFormula::DaiYuan);
          I((int)PSOInitMode::RandomOnly); I((int)PSOInitMode::SeededOnly); I((int)PSOInitMode::SeededPlusRandom);
          I((int)PSOBoundaryMode::Clamp); I((int)PSOBoundaryMode::Reflect); I((int)PSOBoundaryMode::Wrap);
          I((int)PSOInertiaSchedule::Constant); I((int)PSOInertiaSchedule::LinearDecay);
          I((int)GlobalStrategy::PSO); I((int)GlobalStrategy::MultiStart); I((int)GlobalStrategy::PSOHybrid);
          I((int)GlobalStrategy::DifferentialEvolution);
          UzawaResult u; I((int)u.Status); I(u.NbIterations); B(u.IsDone());
          break;
        }
      })
  }
  return 0;
}
