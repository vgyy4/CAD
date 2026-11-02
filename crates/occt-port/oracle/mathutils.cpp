// Deterministic reference records for the OCCT MathUtils package (OCCT 8.0.1), see docs/api-mathutils.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned
// 64-bit values decimal; a matrix is `lowerRow upperRow lowerCol upperCol values(row-major)`, a vector
// `lower upper values`. Callbacks are a closed set of functions described by printed parameters; every callback
// call is counted and its argument bits are folded into a 64-bit hash (the call sequence is part of the behaviour).
#include <MathUtils_Bracket.hxx>
#include <MathUtils_Config.hxx>
#include <MathUtils_Convergence.hxx>
#include <MathUtils_Core.hxx>
#include <MathUtils_Deriv.hxx>
#include <MathUtils_Domain.hxx>
#include <MathUtils_FunctorScalar.hxx>
#include <MathUtils_FunctorVector.hxx>
#include <MathUtils_Gauss.hxx>
#include <MathUtils_GaussKronrodWeights.hxx>
#include <MathUtils_LineSearch.hxx>
#include <MathUtils_Poly.hxx>
#include <MathUtils_Random.hxx>
#include <MathUtils_Types.hxx>
#include <Standard_Failure.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

using namespace MathUtils;

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
// run the computation; an OCCT exception (range checks are on in this build) is recorded as `throws`
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

// ---- deterministic inputs ----------------------------------------------------------------------------
static uint64_t S = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S ^= S << 13;
  S ^= S >> 7;
  S ^= S << 17;
  return (double)(S >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)
static const int LB[4] = {1, 0, 3, -2};
static const double INF = std::numeric_limits<double>::infinity();
static void smash() {  // fill a chunk of the stack with a recognisable pattern (uninitialised-read check)
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static math_Vector genV(int l, int n, double scale = 1.0) {
  math_Vector v(l, l + n - 1);
  for (int i = l; i < l + n; ++i) v(i) = rv() * scale;
  return v;
}
static math_Matrix genM(int lr, int m, int lc, int n) {
  math_Matrix a(lr, lr + m - 1, lc, lc + n - 1, 0.0);
  for (int i = lr; i < lr + m; ++i)
    for (int j = lc; j < lc + n; ++j) a(i, j) = rv();
  return a;
}

// ---- callbacks --------------------------------------------------------------------------------------
static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;

// scalar: cubic ((d x + c) x + b) x + a that fails (returns false) for flo < x < fhi
struct TS {
  double a, b, c, d, flo, fhi;
  int calls = 0;
  uint64_t hash = H0;
  bool Value(double x, double& y) {
    ++calls;
    hash = mixh(hash, bitsOf(x));
    if (x > flo && x < fhi) return false;
    y = ((d * x + c) * x + b) * x + a;
    return true;
  }
};
static void printTS(const TS& t) { F(t.a); F(t.b); F(t.c); F(t.d); F(t.flo); F(t.fhi); }
#define TS_SCHEMA "ffffff"
static void outCalls(int calls, uint64_t hash) { I(calls); U(hash); }

// vector: kind 0 Sphere, 1 Rosenbrock(pa,pb), 2 Booth, 3 QuadraticForm(A,b,c), 4 Himmelblau; fails when x(lower) > failx
struct TV {
  int kind;
  double pa, pb, failx;
  math_Matrix A;
  math_Vector bv;
  double c;
  int calls = 0, gcalls = 0;
  uint64_t hash = H0;
  void mixx(const math_Vector& x, uint64_t salt) {
    hash = mixh(hash, salt);
    for (int i = x.Lower(); i <= x.Upper(); ++i) hash = mixh(hash, bitsOf(x(i)));
  }
  bool Value(const math_Vector& x, double& y) {
    ++calls;
    mixx(x, 1);
    if (x(x.Lower()) > failx) return false;
    switch (kind) {
      case 0: return Sphere().Value(x, y);
      case 1: return Rosenbrock(pa, pb).Value(x, y);
      case 2: return Booth().Value(x, y);
      case 3: return QuadraticForm(A, bv, c).Value(x, y);
      default: return Himmelblau().Value(x, y);
    }
  }
  bool Gradient(const math_Vector& x, math_Vector& g) {
    ++gcalls;
    mixx(x, 2);
    switch (kind) {
      case 0: return Sphere().Gradient(x, g);
      case 1: return Rosenbrock(pa, pb).Gradient(x, g);
      case 2: return Booth().Gradient(x, g);
      case 3: return QuadraticForm(A, bv, c).Gradient(x, g);
      default: return Himmelblau().Gradient(x, g);
    }
  }
};
// a vector callback for the given dimension n and lower bound l (random parameters)
static TV genTV(int j, int l, int& n) {
  int kind = j % 5;
  if (kind == 1 || kind == 2 || kind == 4) n = std::max(n, 2);
  double failx = (j % 7 == 3) ? 0.5 : 1e300;
  TV t{kind, 1.0 + (j % 3), 10.0 * (1 + j % 4), failx, genM(l, n, l, n), genV(l, n), rv()};
  return t;
}
static void printTV(const TV& t) {
  I(t.kind); F(t.pa); F(t.pb); F(t.failx); M(t.A); V(t.bv); F(t.c);
}
#define TV_SCHEMA "ifffMVf"

// system: m equations f_i = (i + 1) * (sum_k x_k^2) + x_(lower + i mod n); fails when x(lower) > failx
struct TSys {
  int m;
  double failx;
  int calls = 0;
  uint64_t hash = H0;
  bool Value(const math_Vector& x, math_Vector& f) {
    ++calls;
    for (int i = x.Lower(); i <= x.Upper(); ++i) hash = mixh(hash, bitsOf(x(i)));
    if (x(x.Lower()) > failx) return false;
    double s = 0.0;
    for (int i = x.Lower(); i <= x.Upper(); ++i) s += x(i) * x(i);
    int n = x.Length();
    for (int i = 1; i <= m; ++i) f(i) = (double)i * s + x(x.Lower() + (i - 1) % n);
    return true;
  }
};

int main() {
  const double SP[16] = {0.0, 1.0, -1.0, 1e-15, -1e-15, 1e-16, 1e15, -2.5, 0.5, 3.0, 1e-300, 1e300, INF, -INF, 2.0, -0.0};
  const double TOLS[4] = {1e-15, 1e-10, 0.5, 1.0};

  // -- Core ------------------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // core_scalar
    smash();
    double v1 = SP[j], v2 = SP[(j * 5 + 3) % 16], v3 = SP[(j * 7 + 5) % 16], tol = TOLS[j % 4];
    begin("core_scalar", "ffff"); F(v1); F(v2); F(v3); F(tol); bar();
    CASE(F(Clamp(v1, v2, v3)); B(IsZero(v1)); B(IsZero(v1, tol)); B(MathUtils::IsEqual(v1, v2)); B(MathUtils::IsEqual(v1, v2, tol));
         F(SafeDiv(v1, v2)); F(SafeDiv(v1, v2, v3)); I(Sign(v1)); F(SignTransfer(v1, v2)); F(Sqr(v1)); F(Cube(v1));
         B(IsFinite(v1)); F(THE_EPSILON); F(THE_ZERO_TOL); F(THE_PI); F(THE_2PI); F(THE_GOLDEN_RATIO); F(THE_GOLDEN_SECTION);)
  }
  for (int j = 0; j < 16; ++j) {  // cube_root
    smash();
    double v = (j < 8) ? SP[j * 2 % 16] : rv() * 100.0;
    if (j == 5) v = 8.0;
    if (j == 6) v = -27.0;
    begin("cube_root", "f"); F(v); bar();
    CASE(F(CubeRoot(v));)
  }
  for (int j = 0; j < 16; ++j) {  // compute_scale_factor
    smash();
    int n = 1 + j % 5;
    math_Vector c = genV(0, n);
    if (j % 4 == 1) for (int i = 0; i < n; ++i) c(i) *= 1e17;
    if (j % 4 == 2) for (int i = 0; i < n; ++i) c(i) *= 1e-17;
    if (j % 8 == 3) for (int i = 0; i < n; ++i) c(i) = 0.0;
    if (j == 15) c(0) = 1e300;
    begin("compute_scale_factor", "V"); V(c); bar();
    std::vector<double> cc(n);
    for (int i = 0; i < n; ++i) cc[i] = c(i);
    CASE(F(ComputeScaleFactor(cc.data(), n));)
  }
  for (int j = 0; j < 16; ++j) {  // vector_norms
    smash();
    int n = 1 + j % 5, la = LB[j % 4];
    math_Vector a = genV(la, n), b = genV(j == 13 ? la + 1 : (j == 14 ? la : la), j == 14 ? std::max(1, n - 1) : n);
    begin("vector_norms", "VV"); V(a); V(b); bar();
    CASE(F(DotProduct(a, b)); F(VectorNorm(a)); F(VectorInfNorm(a)); F(VectorNorm(b)); F(VectorInfNorm(b));)
  }
  // -- Convergence -----------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // converged_scalar
    smash();
    double xo = rv(), xn = (j % 3 == 0) ? xo + 1e-12 * (1 + j) : rv(), fo = rv(), fn = (j % 4 == 1) ? fo + 1e-12 : rv();
    if (j % 5 == 2) fn = 1e-12;
    double tol = TOLS[j % 4] * (j % 2 ? 1e-8 : 1.0), xt = (j % 2) ? 1e-10 : 1e-3, ft = (j % 3 == 1) ? 1e-3 : 1e-10;
    begin("converged_scalar", "fffffff"); F(xo); F(xn); F(fo); F(fn); F(tol); F(xt); F(ft); bar();
    CASE(Config c; c.XTolerance = xt; c.FTolerance = ft;
         B(IsXConverged(xo, xn, tol)); B(IsFConverged(fn, tol)); B(IsConverged(xo, xn, fn, c)); B(IsMinConverged(xo, xn, fo, fn, c));)
  }
  for (int j = 0; j < 16; ++j) {  // converged_vector
    smash();
    int n = 1 + j % 5, l = LB[j % 4];
    math_Vector o = genV(l, n), nw = genV(l, (j == 15) ? n + 1 : n), g = genV(l, n, (j % 2) ? 1e-9 : 1.0);
    if (j % 3 == 0) for (int i = l; i < l + n; ++i) nw(i) = o(i) + 1e-12;
    double tol = TOLS[j % 4] * (j % 2 ? 1e-8 : 1.0);
    begin("converged_vector", "VVVf"); V(o); V(nw); V(g); F(tol); bar();
    CASE(B(IsVectorConverged(o, nw, tol)); B(IsGradientConverged(g, tol)); F(InfinityNorm(g)); F(EuclideanNorm(g));)
  }
  // -- Config / types ---------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // config
    smash();
    double tol = (j < 4) ? TOLS[j] : std::ldexp(1.0, -(j * 3)), lo = rv(), up = lo + std::abs(rv());
    int it = (j == 0) ? 0 : j * 7 - 3;
    bool ub = j % 2;
    begin("config", "fiffi"); F(tol); I(it); F(lo); F(up); I(ub); bar();
    CASE(Config d; Config c(tol, it); Config c1(tol); BoundedConfig bd; BoundedConfig bc(lo, up, tol, it); BoundedConfig bc1(lo, up);
         NDimConfig nd; NDimConfig nc(tol, it, ub); NDimConfig nc1(tol); IntegConfig id; IntegConfig ic(tol, it); LinConfig lc;
         for (const Config* p : {&d, &c, &c1}) { I(p->MaxIterations); F(p->Tolerance); F(p->XTolerance); F(p->FTolerance); F(p->StepMin); }
         for (const BoundedConfig* p : {&bd, &bc, &bc1}) { I(p->MaxIterations); F(p->Tolerance); F(p->XTolerance); F(p->FTolerance); F(p->StepMin); F(p->LowerBound); F(p->UpperBound); }
         for (const NDimConfig* p : {&nd, &nc, &nc1}) { I(p->MaxIterations); F(p->Tolerance); F(p->XTolerance); F(p->FTolerance); F(p->StepMin); B(p->UseBounds); }
         for (const IntegConfig* p : {&id, &ic}) { I(p->InitialOrder); I(p->MaxOrder); I(p->MaxIterations); F(p->Tolerance); }
         F(lc.SingularityTolerance); B(lc.UsePivoting);
         F(THE_NEWTON_FTOL_SQ); F(THE_NEWTON_STEP_TOL_FACTOR); I(THE_NEWTON_MAX_ITER); F(THE_NEWTON2D_SINGULAR_DET);
         F(THE_NEWTON2D_CRITICAL_GRAD_SQ); F(THE_NEWTON2D_MAX_STEP_RATIO); F(THE_NEWTON2D_DOMAIN_EXT); F(THE_NEWTON2D_STAGNATION_RATIO);
         I(THE_NEWTON2D_STAGNATION_COUNT); I(THE_NEWTON2D_LINE_SEARCH_MAX); F(THE_NEWTON2D_SKIP_LINESEARCH_SQ);
         F(THE_NEWTON2D_STAGNATION_RELAX); F(THE_NEWTON2D_BACKTRACK_TRIGGER); F(THE_NEWTON2D_BACKTRACK_ACCEPT);
         F(THE_NEWTON2D_MAXITER_RELAX); F(THE_HESSIAN_DEGENERACY_REL); F(THE_HESSIAN_DEGENERACY_ABS); F(THE_ARMIJO_C1);)
  }
  for (int j = 0; j < 10; ++j) {  // result_types (status = 0..9)
    smash();
    begin("result_types", "i"); I(j); bar();
    CASE(Status st = static_cast<Status>(j);
         ScalarResult sr; PolyResult pr; VectorResult vr; LinearResult lr; LinearMultipleResult lm; EigenResult er; DecompResult dr;
         IntegResult ir; InverseResult nr;
         I((int)sr.Status); I(sr.NbIterations); B(sr.Root.has_value()); B(sr.Value.has_value()); B(sr.Derivative.has_value()); B(sr.IsDone());
         I((int)pr.Status); I(pr.NbRoots); for (int i = 0; i < 4; ++i) F(pr[i]); B(pr.IsDone());
         I((int)vr.Status); I(vr.NbIterations); B(vr.Solution.has_value()); B(vr.Value.has_value()); B(vr.Gradient.has_value()); B(vr.Jacobian.has_value()); B(vr.IsDone());
         I((int)lr.Status); B(lr.Solution.has_value()); B(lr.Determinant.has_value()); B(lr.IsDone());
         I((int)lm.Status); B(lm.Solutions.has_value()); B(lm.Determinant.has_value()); B(lm.IsDone());
         I((int)er.Status); I(er.NbIterations); B(er.EigenValues.has_value()); B(er.EigenVectors.has_value()); B(er.IsDone());
         I((int)dr.Status); B(dr.L.has_value()); B(dr.U.has_value()); B(dr.D.has_value()); B(dr.Determinant.has_value()); B(dr.IsDone());
         I((int)ir.Status); I(ir.NbIterations); I(ir.NbPoints); B(ir.Value.has_value()); B(ir.AbsoluteError.has_value()); B(ir.RelativeError.has_value()); B(ir.IsDone());
         I((int)nr.Status); B(nr.Inverse.has_value()); B(nr.Determinant.has_value()); B(nr.IsDone());
         sr.Status = st; pr.Status = st; vr.Status = st; lr.Status = st; lm.Status = st; er.Status = st; dr.Status = st; ir.Status = st; nr.Status = st;
         B(sr.IsDone()); B(pr.IsDone()); B(vr.IsDone()); B(lr.IsDone()); B(lm.IsDone()); B(er.IsDone()); B(dr.IsDone()); B(ir.IsDone()); B(nr.IsDone());
         B((bool)sr); B((bool)pr); B((bool)ir);)
  }
  // -- Random ----------------------------------------------------------------------------------------
  {
    const unsigned long long SEEDS[16] = {0ull, 1ull, 2ull, 12345ull, 0xffffffffffffffffull, 0x9e3779b97f4a7c15ull, 42ull, 1ull << 63,
                                          0xdeadbeefull, 7ull, 100ull, 0x0123456789abcdefull, 31337ull, 1ull << 32, 999999999999ull, 5ull};
    for (int j = 0; j < 16; ++j) {  // random_generator
      smash();
      unsigned long long s1 = SEEDS[j], s2 = SEEDS[(j * 3 + 1) % 16];
      begin("random_generator", "uu"); U(s1); U(s2); bar();
      CASE(RandomGenerator g(s1); for (int i = 0; i < 6; ++i) U(g.NextInt()); for (int i = 0; i < 6; ++i) F(g.NextReal());
           g.SetSeed(s2); for (int i = 0; i < 3; ++i) U(g.NextInt()); F(g.NextReal()); RandomGenerator d; U(d.NextInt()); U(d.NextInt());)
    }
  }
  // -- Poly ------------------------------------------------------------------------------------------
  auto polyVec = [&](int j, int maxdeg) {
    int deg = (j * 3 + j / 4) % (maxdeg + 1);
    math_Vector c = genV(0, deg + 1);
    if (j % 5 == 4) c(deg) = 1.0;
    return c;
  };
  auto cvec = [&](const math_Vector& c) { std::vector<double> v(c.Length()); for (int i = 0; i < c.Length(); ++i) v[i] = c(i); return v; };
  for (int j = 0; j < 16; ++j) {  // eval_poly
    smash();
    math_Vector c = polyVec(j, 5); double x = (j % 4 == 0) ? 0.0 : rv();
    begin("eval_poly", "Vf"); V(c); F(x); bar();
    CASE(auto cc = cvec(c); int d = c.Length() - 1; F(EvalPoly(cc.data(), d, x)); F(EvalPolyDesc(cc.data(), d, x)););
  }
  for (int j = 0; j < 16; ++j) {  // eval_poly_deriv
    smash();
    math_Vector c = polyVec(j, 5); double x = rv();
    begin("eval_poly_deriv", "Vf"); V(c); F(x); bar();
    CASE(auto cc = cvec(c); int d = c.Length() - 1; double v = 7.0, dv = 7.0; EvalPolyDeriv(cc.data(), d, x, v, dv); F(v); F(dv);)
  }
  for (int j = 0; j < 16; ++j) {  // refine_poly_root
    smash();
    math_Vector c0 = polyVec(j, 4);
    math_Vector c = (c0.Length() < 2) ? genV(0, 3) : c0;
    int mi = (j == 0) ? 0 : (j == 1 ? 1 : (j % 4 == 2 ? 5 : 1 + j % 8));
    double r = rv() * 2.0;
    begin("refine_poly_root", "Vfi"); V(c); F(r); I(mi); bar();
    CASE(auto cc = cvec(c); int d = c.Length() - 1; F(RefinePolyRoot(cc.data(), d, r, mi)); F(RefinePolyRootDesc(cc.data(), d, r, mi));)
  }
  for (int j = 0; j < 16; ++j) {  // sort_roots
    smash();
    int n = j % 9;
    std::vector<double> r(n + 1);
    for (int i = 0; i < n; ++i) r[i] = (j % 3 == 0 && i > 1) ? r[i - 2] : std::floor(rv() * 4.0) / 4.0;
    math_Vector rr(1, std::max(1, n), rv());
    for (int i = 0; i < n; ++i) rr(1 + i) = r[i];
    begin("sort_roots", "iV"); I(n); V(rr); bar();
    CASE(SortRoots(r.data(), n); for (int i = 0; i < n; ++i) F(r[i]);)
  }
  for (int j = 0; j < 16; ++j) {  // remove_duplicate_roots
    smash();
    int n = j % 9;
    std::vector<double> r(n + 1);
    for (int i = 0; i < n; ++i) r[i] = (i && j % 2) ? r[i - 1] + ((i % 3 == 0) ? 1e-12 : 0.5) : std::floor(rv() * 4.0) / 4.0;
    math_Vector rr(1, std::max(1, n), rv());
    for (int i = 0; i < n; ++i) rr(1 + i) = r[i];
    double tol = (j % 3 == 0) ? 1e-10 : (j % 3 == 1 ? 0.3 : 0.0);
    begin("remove_duplicate_roots", "iVf"); I(n); V(rr); F(tol); bar();
    CASE(size_t k = RemoveDuplicateRoots(r.data(), n, tol); I(k); for (size_t i = 0; i < k; ++i) F(r[i]); size_t k2 = RemoveDuplicateRoots(r.data(), k);
         I(k2);)
  }
  for (int j = 0; j < 16; ++j) {  // depress_cubic_quartic
    smash();
    double b = rv(), c = rv(), d = rv(), e = rv();
    if (j == 0) b = 0.0;
    begin("depress_cubic_quartic", "ffff"); F(b); F(c); F(d); F(e); bar();
    CASE(double p, q, r, s; DepressCubic(b, c, d, p, q, s); F(p); F(q); F(s); double p2, q2, r2, s2; DepressQuartic(b, c, d, e, p2, q2, r2, s2);
         F(p2); F(q2); F(r2); F(s2);)
  }
  // -- Domain ----------------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // domain1d
    smash();
    double mn = (j == 0) ? 0.0 : rv() * 4.0, mx = (j == 1) ? mn : mn + std::abs(rv()) * (j % 3 == 0 ? 1000.0 : 1.0);
    if (j == 2) mx = mn - 1.0;
    if (j == 3) { mn = -1e101; mx = 1e101; }
    double u = rv() * 4.0, tol = TOLS[j % 4], thr = (j % 2) ? 1.0 : 1000.0, per = 6.283185307179586, mn2 = (j % 4 == 0) ? mn : mn + 1e-11, mx2 = mx;
    begin("domain1d", "fffffffff"); F(mn); F(mx); F(u); F(tol); F(thr); F(per); F(mn2); F(mx2); F(1e100); bar();
    CASE(Domain1D dflt; Domain1D d(mn, mx), o(mn2, mx2); F(dflt.Min); F(dflt.Max); F(d.Length()); F(d.Mid()); B(d.Contains(u)); B(d.Contains(u, tol));
         F(d.Clamp(u)); B(d.IsLarge()); B(d.IsLarge(thr)); B(d.IsFullPeriod(per)); B(d.IsFullPeriod(per, tol)); F(d.Lerp(u)); F(d.Normalize(u));
         B(d.IsFinite()); B(d.IsFinite(thr)); B(d.IsEqual(o)); B(d.IsEqual(o, tol));)
  }
  for (int j = 0; j < 16; ++j) {  // domain2d
    smash();
    double u0 = rv() * 4.0, v0 = rv() * 4.0, u1 = u0 + std::abs(rv()) * (j % 3 == 0 ? 1000.0 : 1.0), v1 = v0 + std::abs(rv()) * (j % 4 == 0 ? 2000.0 : 1.0);
    if (j == 0) { u0 = u1 = 0.0; }
    if (j == 1) { u1 = u0 - 2.0; }
    if (j == 2) { v0 = -1e101; v1 = 1e101; }
    double u = rv() * 4.0, v = rv() * 4.0, tol = TOLS[j % 4], thr = (j % 2) ? 1.0 : 1000.0, per = 6.283185307179586;
    begin("domain2d", "fffffffff"); F(u0); F(u1); F(v0); F(v1); F(u); F(v); F(tol); F(thr); F(per); bar();
    CASE(Domain2D dflt; Domain2D d(u0, u1, v0, v1); Domain2D e(Domain1D(u0, u1), Domain1D(v0, v1)); F(dflt.UMin); F(dflt.VMax);
         F(d.ULength()); F(d.VLength()); F(d.UMid()); F(d.VMid()); B(d.Contains(u, v)); B(d.Contains(u, v, tol)); double cu = u, cv = v; d.Clamp(cu, cv);
         F(cu); F(cv); B(d.IsLarge()); B(d.IsLarge(thr)); B(d.IsUFullPeriod(per)); B(d.IsUFullPeriod(per, tol)); B(d.IsVFullPeriod(per));
         B(d.IsVFullPeriod(per, tol)); B(d.IsFinite()); B(d.IsFinite(thr)); F(d.U().Min); F(d.U().Max); F(d.V().Min); F(d.V().Max);
         F(e.UMin); F(e.UMax); F(e.VMin); F(e.VMax);)
  }
  // -- Deriv -----------------------------------------------------------------------------------------
  auto genTS = [&](int j) {
    double flo = (j % 5 == 2) ? 0.0 : 1.0, fhi = (j % 5 == 2) ? 2.0 : 0.0;  // failing interval (0, 2) for some cases
    return TS{rv(), rv(), rv(), rv(), flo, fhi};
  };
  const double STEPS[4] = {1e-8, 1e-5, 1e-3, 0.25};
  for (int j = 0; j < 16; ++j) {  // central_difference
    smash();
    TS t = genTS(j); double x = (j % 5 == 2) ? 1.0 : rv(), step = STEPS[j % 4];
    begin("central_difference", TS_SCHEMA "ff"); printTS(t); F(x); F(step); bar();
    CASE(double d = 0.0; bool ok = CentralDifference(t, x, d, step); B(ok); F(d); outCalls(t.calls, t.hash);
         double d2 = 0.0; TS t2 = t; t2.calls = 0; t2.hash = H0; bool ok2 = CentralDifference(t2, x, d2); B(ok2); F(d2);)
  }
  for (int j = 0; j < 16; ++j) {  // forward_difference
    smash();
    TS t = genTS(j); double x = (j % 5 == 2) ? 1.0 : rv(), fx = rv(), step = STEPS[j % 4];
    begin("forward_difference", TS_SCHEMA "fff"); printTS(t); F(x); F(fx); F(step); bar();
    CASE(double d = 0.0; bool ok = ForwardDifference(t, x, fx, d, step); B(ok); F(d); outCalls(t.calls, t.hash);
         double d2 = 0.0; TS t2 = t; t2.calls = 0; t2.hash = H0; bool ok2 = ForwardDifference(t2, x, fx, d2); B(ok2); F(d2);)
  }
  for (int j = 0; j < 16; ++j) {  // second_derivative
    smash();
    TS t = genTS(j); double x = (j % 5 == 2) ? 1.0 : rv(), fx = rv(), step = (j % 2) ? 1e-5 : 1e-3;
    begin("second_derivative", TS_SCHEMA "fff"); printTS(t); F(x); F(fx); F(step); bar();
    CASE(double d = 0.0; bool ok = SecondDerivative(t, x, fx, d, step); B(ok); F(d); outCalls(t.calls, t.hash);
         double d2 = 0.0; TS t2 = t; t2.calls = 0; t2.hash = H0; bool ok2 = SecondDerivative(t2, x, fx, d2); B(ok2); F(d2);)
  }
  for (int j = 0; j < 32; ++j) {  // numerical_gradient (j < 16) and numerical_gradient_adaptive
    smash();
    int jj = j % 16, n = 1 + jj % 4, l = LB[jj % 4];
    TV t = genTV(jj, l, n);
    math_Vector x = genV(l, n, (jj % 3 == 0) ? 50.0 : 1.0), g(l, l + n - 1, 0.0);
    double step = STEPS[jj % 4];
    begin(j < 16 ? "numerical_gradient" : "numerical_gradient_adaptive", TV_SCHEMA "Vf"); printTV(t); V(x); F(step); bar();
    CASE(bool ok = (j < 16) ? NumericalGradient(t, x, g, step) : NumericalGradientAdaptive(t, x, g, step); B(ok); V(g); V(x); outCalls(t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // numerical_jacobian
    smash();
    int n = 1 + j % 4, l = LB[j % 4], m = 1 + (j * 3) % 5;
    TSys t{m, (j % 7 == 3) ? 0.5 : 1e300};
    math_Vector x = genV(l, n);
    math_Matrix jac(1, m, 1, n, 0.0);
    double step = STEPS[j % 4];
    begin("numerical_jacobian", "ifVf"); I(m); F(t.failx); V(x); F(step); bar();
    CASE(bool ok = NumericalJacobian(t, x, jac, step); B(ok); M(jac); V(x); outCalls(t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // numerical_hessian
    smash();
    int n = 1 + j % 4, l = LB[j % 4];
    TV t = genTV(j, l, n);
    math_Vector x = genV(l, n);
    math_Matrix h(1, n, 1, n, 0.0);
    double step = (j % 2) ? 1e-5 : 1e-3;
    begin("numerical_hessian", TV_SCHEMA "Vf"); printTV(t); V(x); F(step); bar();
    CASE(bool ok = NumericalHessian(t, x, h, step); B(ok); M(h); V(x); outCalls(t.calls, t.hash);)
  }
  // -- Bracket ---------------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // bracket_root
    smash();
    TS t = genTS(j); double a = rv(), b = (j == 0) ? a : rv();
    int mi = (j == 1) ? 0 : (j == 3 ? 1 : (j % 4 == 2 ? 50 : 3 + j % 7));
    begin("bracket_root", TS_SCHEMA "ffi"); printTS(t); F(a); F(b); I(mi); bar();
    CASE(BracketResult r = BracketRoot(t, a, b, mi); B(r.IsValid); F(r.A); F(r.B); F(r.Fa); F(r.Fb); outCalls(t.calls, t.hash);
         BracketResult r2 = BracketRoot(t, a, b); B(r2.IsValid); F(r2.A); F(r2.B); outCalls(t.calls, t.hash);)
  }
  for (int j = 0; j < 32; ++j) {  // bracket_minimum (j < 16) and bracket_minimum_iter
    smash();
    int jj = j % 16;
    TS t = genTS(jj); double a = rv(), b = (jj == 0) ? a : rv();
    t.d = (jj % 3 == 0) ? 0.0 : t.d;
    if (jj % 3 == 0) { t.c = std::abs(t.c) + 0.25; }  // a parabola
    int mi = (jj == 1) ? 0 : (jj % 4 == 2 ? 50 : 3 + jj % 9);
    MinBracketOptions o;
    o.MaxIterations = mi;
    o.UseLimits = (jj % 4 == 1);
    o.LeftLimit = -1.5 - (jj % 2); o.RightLimit = (jj == 5) ? -3.0 : 2.5;
    o.HasFA = (jj % 3 == 1); o.HasFB = (jj % 5 == 2); o.FA = rv(); o.FB = rv();
    if (j < 16) {
      begin("bracket_minimum", TS_SCHEMA "ffiiffiiff"); printTS(t); F(a); F(b); I(o.MaxIterations); I(o.UseLimits); F(o.LeftLimit); F(o.RightLimit);
      I(o.HasFA); I(o.HasFB); F(o.FA); F(o.FB); bar();
    } else {
      begin("bracket_minimum_iter", TS_SCHEMA "ffi"); printTS(t); F(a); F(b); I(mi); bar();
    }
    CASE(MinBracketResult r = (j < 16) ? BracketMinimum(t, a, b, o) : BracketMinimum(t, a, b, mi); B(r.IsValid); F(r.A); F(r.B); F(r.C); F(r.Fa);
         F(r.Fb); F(r.Fc); outCalls(t.calls, t.hash);)
  }
  // -- Line search -----------------------------------------------------------------------------------
  struct LS { TV t; math_Vector x, dir, g; double fx; };
  auto lsSetup = [&](int j) {
    int n = 1 + j % 4, l = LB[j % 4];
    TV t = genTV(j, l, n);
    math_Vector x = genV(l, n), dir = genV(l, n), g = genV(l, n);
    double fx = 0.0;
    TV probe = t; probe.failx = 1e300;
    probe.Value(x, fx);
    probe.Gradient(x, g);
    if (j % 3 != 1) for (int i = l; i < l + n; ++i) dir(i) = -g(i) * 0.5;  // a descent direction
    return LS{t, x, dir, g, fx};
  };
  for (int j = 0; j < 16; ++j) {  // armijo_backtrack
    smash();
    LS ls = lsSetup(j); TV& t = ls.t; math_Vector &x = ls.x, &dir = ls.dir, &g = ls.g; double fx = ls.fx;
    double ai = (j % 4 == 0) ? 1.0 : 0.25 * (1 + j % 5), c1 = (j % 3 == 0) ? 1e-4 : 0.3, rho = (j % 2) ? 0.5 : 0.1;
    int mi = (j == 3) ? 1 : (j == 4 ? 0 : 50);
    begin("armijo_backtrack", TV_SCHEMA "VVVffffi"); printTV(t); V(x); V(dir); V(g); F(fx); F(ai); F(c1); F(rho); I(mi); bar();
    CASE(LineSearchResult r = ArmijoBacktrack(t, x, dir, g, fx, ai, c1, rho, mi); B(r.IsValid); F(r.Alpha); F(r.FNew); I(r.NbEvals);
         outCalls(t.calls, t.hash); LineSearchResult r2 = ArmijoBacktrack(t, x, dir, g, fx); B(r2.IsValid); F(r2.Alpha); F(r2.FNew); I(r2.NbEvals);
         outCalls(t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // wolfe_search
    smash();
    LS ls = lsSetup(j); TV& t = ls.t; math_Vector &x = ls.x, &dir = ls.dir, &g = ls.g; double fx = ls.fx;
    double ai = (j % 4 == 0) ? 1.0 : 0.25 * (1 + j % 5), c1 = (j % 3 == 0) ? 1e-4 : 0.3, c2 = (j % 2) ? 0.9 : 0.5;
    int mi = (j == 3) ? 1 : (j == 4 ? 0 : (j % 5 == 2 ? 20 : 3 + j));
    begin("wolfe_search", TV_SCHEMA "VVVffffi"); printTV(t); V(x); V(dir); V(g); F(fx); F(ai); F(c1); F(c2); I(mi); bar();
    CASE(LineSearchResult r = WolfeSearch(t, x, dir, g, fx, ai, c1, c2, mi); B(r.IsValid); F(r.Alpha); F(r.FNew); I(r.NbEvals); outCalls(t.calls, t.hash);
         I(t.gcalls); LineSearchResult r2 = WolfeSearch(t, x, dir, g, fx); B(r2.IsValid); F(r2.Alpha); F(r2.FNew); I(r2.NbEvals); outCalls(t.calls, t.hash);
         I(t.gcalls);)
  }
  for (int j = 0; j < 16; ++j) {  // exact_line_search
    smash();
    LS ls = lsSetup(j); TV& t = ls.t; math_Vector &x = ls.x, &dir = ls.dir, &g = ls.g; double fx = ls.fx;
    double am = (j % 3 == 0) ? 10.0 : 0.5 * (1 + j % 4), tol = (j % 2) ? 1e-6 : 1e-10;
    int mi = (j == 3) ? 1 : (j == 4 ? 0 : (j % 5 == 2 ? 100 : 5 + j));
    begin("exact_line_search", TV_SCHEMA "VVffi"); printTV(t); V(x); V(dir); F(am); F(tol); I(mi); bar();
    CASE(LineSearchResult r = ExactLineSearch(t, x, dir, am, tol, mi); B(r.IsValid); F(r.Alpha); F(r.FNew); I(r.NbEvals); outCalls(t.calls, t.hash);
         LineSearchResult r2 = ExactLineSearch(t, x, dir); B(r2.IsValid); F(r2.Alpha); F(r2.FNew); I(r2.NbEvals); outCalls(t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // quadratic_interpolation
    smash();
    double p0 = rv(), dp = (j % 4 == 0) ? 0.0 : rv(), a1 = (j % 5 == 1) ? 0.0 : std::abs(rv()), p1 = (j % 6 == 2) ? p0 + dp * a1 : rv();
    begin("quadratic_interpolation", "ffff"); F(p0); F(dp); F(a1); F(p1); bar();
    CASE(F(QuadraticInterpolation(p0, dp, a1, p1));)
  }
  for (int j = 0; j < 16; ++j) {  // brent_along_coordinate
    smash();
    int n = 1 + j % 4, l = LB[j % 4];
    TV t = genTV(j, l, n);
    math_Vector p = genV(l, n);
    int idx = l + (j * 3) % n;
    double lo = p(idx) - 1.0 - (j % 3), up = p(idx) + 1.0 + (j % 2), fx = 0.0;
    { TV probe = t; probe.failx = 1e300; probe.Value(p, fx); }
    if (j % 5 == 2) fx += 1.0;  // a point that can be improved
    double tol = (j % 2) ? 1e-6 : 1e-10;
    int mi = (j == 3) ? 1 : (j == 4 ? 0 : 5 + 4 * j), ec = j;
    begin("brent_along_coordinate", TV_SCHEMA "Viffffii"); printTV(t); V(p); I(idx); F(lo); F(up); F(fx); F(tol); I(mi); I(ec); bar();
    CASE(bool ok = BrentAlongCoordinate(t, p, idx, lo, up, fx, tol, mi, ec); B(ok); V(p); F(fx); I(ec); outCalls(t.calls, t.hash);)
  }
  // -- Functors --------------------------------------------------------------------------------------
  for (int j = 0; j < 32; ++j) {  // scalar_functor (arithmetic kinds)
    smash();
    int kind = j % 14, jj = j / 14;
    math_Vector p = genV(0, 1 + (j * 5) % 4), q = (kind == 1) ? genV(0, 1 + (j * 3) % 3) : genV(0, 2);
    if (j % 6 == 1) q(0) = 0.0;
    double s = rv(), x = (j % 7 == 3) ? -std::abs(rv()) : rv();
    int idx = (j % 6);
    begin("scalar_functor", "iVVffi"); I(kind); V(p); V(q); F(s); F(x); I(idx); bar();
    CASE(
      auto poly = [&]() { return Polynomial(p); };
      auto lin = [&]() { return Linear(q(0), q(1)); };
      double y = 3.0, dy = 3.0; bool ok, ok2 = false; int hasV = 0;
      switch (kind) {
        case 0: { Polynomial f(p); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); hasV = 1; I(f.Degree()); F(f.Coefficient(idx)); break; }
        case 1: { Rational f(p, q); ok = f.Value(x, y); break; }
        case 2: { Constant f(s); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); hasV = 1; break; }
        case 3: { Linear f(q(0), q(1)); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); hasV = 1; break; }
        case 4: { auto f = MakeComposite(poly(), lin()); ok = f.Value(x, y); break; }
        case 5: { auto f = MakeSum(poly(), lin()); ok = f.Value(x, y); break; }
        case 6: { auto f = MakeDifference(poly(), lin()); ok = f.Value(x, y); break; }
        case 7: { auto f = MakeProduct(poly(), lin()); ok = f.Value(x, y); break; }
        case 8: { auto f = MakeQuotient(poly(), lin()); ok = f.Value(x, y); break; }
        case 9: { auto f = MakeScaled(poly(), s); ok = f.Value(x, y); break; }
        case 10: { auto f = MakeShifted(poly(), s); ok = f.Value(x, y); break; }
        case 11: { auto f = MakeNegated(poly()); ok = f.Value(x, y); break; }
        case 12: {
          auto f = MakeScalar([&](double t, double& r) { if (t < 0.0) return false; r = s * t * t + q(0); return true; });
          ok = f.Value(x, y);
          break;
        }
        default: {
          auto f = MakeScalarWithDerivative([&](double t, double& r, double& d) { if (t < 0.0) return false; r = s * t * t + q(0); d = 2.0 * s * t; return true; });
          ok = f.Value(x, y); double y2 = 5.0, d2 = 5.0; ok2 = f.Values(x, y2, d2); dy = d2; hasV = 1; F(y2);
          break;
        }
      }
      B(ok); F(y); B(hasV); if (hasV) { B(ok2); F(dy); })
  }
  for (int j = 0; j < 16; ++j) {  // scalar_functor_libm
    smash();
    int kind = j % 5;
    double p1 = rv(), p2 = rv(), p3 = rv(), p4 = rv(), x = (j % 4 == 3) ? -std::abs(rv()) : rv();
    if (kind == 3) p1 = (j % 2) ? 2.0 : 1.5;  // Power exponent
    if (j == 14) { p3 = 1e-16; kind = 4; }
    if (j == 9) { x = 0.0; kind = 3; p1 = 1.0; }
    begin("scalar_functor_libm", "iffffff"); I(kind); F(p1); F(p2); F(p3); F(p4); F(x); F(x * 0.5); bar();
    CASE(double y = 3.0, dy = 3.0; bool ok = false, ok2 = false;
         switch (kind) {
           case 0: { Sine f(p1, p2, p3, p4); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); break; }
           case 1: { Cosine f(p1, p2, p3, p4); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); break; }
           case 2: { Exponential f(p1, p2, p3); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); break; }
           case 3: { Power f(p1, p2, p3); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); break; }
           default: { Gaussian f(p1, p2, p3); ok = f.Value(x, y); ok2 = f.Values(x, y, dy); break; }
         }
         B(ok); F(y); B(ok2); F(dy); Sine ds; Cosine dc; Exponential de; Gaussian dg; double a = 0, b = 0; ds.Value(x, a); dc.Value(x, b); F(a); F(b);
         de.Value(x, a); dg.Value(x, b); F(a); F(b);)
  }
  for (int j = 0; j < 24; ++j) {  // vector_functor (arithmetic kinds)
    smash();
    int kind = j % 7, n = 2 + j % 3, l = LB[j % 4];
    if (kind == 0 || kind == 2) n = 1 + j % 4;
    int m = 1 + j % 4;
    math_Matrix A = (kind == 6) ? genM(LB[(j + 1) % 4], m, l, n) : genM(l, n, l, n);
    math_Vector b = (kind == 6) ? genV(A.LowerRow(), m) : genV(l, n);
    math_Vector x = genV(l, n);
    double pa = 1.0 + (j % 3), pb = 10.0 * (1 + j % 4), c = rv();
    begin("vector_functor", "iffMVfV"); I(kind); F(pa); F(pb); M(A); V(b); F(c); V(x); bar();
    CASE(double y = 3.0; math_Vector g(l, l + n - 1, 0.0), g2(l, l + n - 1, 0.0); double y2 = 4.0; bool ok = false, ok2 = false, ok3 = false;
         switch (kind) {
           case 0: { QuadraticForm f(A, b, c); ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
           case 1: { Rosenbrock f(pa, pb); ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
           case 2: { Sphere f; ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
           case 3: { Booth f; ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
           case 4: { Beale f; ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
           case 5: { Himmelblau f; ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
           default: { LinearResidual f(A, b); ok = f.Value(x, y); ok2 = f.Gradient(x, g); ok3 = f.Values(x, y2, g2); break; }
         }
         B(ok); F(y); B(ok2); V(g); B(ok3); F(y2); V(g2);)
  }
  for (int j = 0; j < 16; ++j) {  // vector_functor_libm
    smash();
    int kind = j % 2, n = 1 + j % 4, l = LB[j % 4];
    math_Vector x = genV(l, n, (j % 4 == 3) ? 3.0 : 1.0);
    double pa = (j % 3 == 0) ? 10.0 : 5.0 + j, pb = 0.1 + 0.05 * (j % 4), pc = (j % 5 == 0) ? 6.283185307179586 : 3.0;
    begin("vector_functor_libm", "ifffV"); I(kind); F(pa); F(pb); F(pc); V(x); bar();
    CASE(double y = 3.0; math_Vector g(l, l + n - 1, 0.0); bool ok = false, ok2 = false;
         if (kind == 0) { Rastrigin f(pa); ok = f.Value(x, y); ok2 = f.Gradient(x, g); Rastrigin d; double yd; d.Value(x, yd); F(yd); }
         else { Ackley f(pa, pb, pc); ok = f.Value(x, y); Ackley d; double yd; d.Value(x, yd); F(yd); }
         B(ok); F(y); B(ok2); V(g);)
  }
  for (int j = 0; j < 16; ++j) {  // vector_lambda
    smash();
    int n = 1 + j % 4, l = LB[j % 4], m = 1 + j % 3;
    math_Vector k = genV(l, n), x = genV(l, n);
    if (j % 4 == 3) x(l) = -x(l) - 1.0;
    begin("vector_lambda", "iVV"); I(m); V(k); V(x); bar();
    CASE(auto val = [&](const math_Vector& t, double& y) { if (t(t.Lower()) < -1.0) return false; y = 0.0; for (int i = t.Lower(); i <= t.Upper(); ++i) y += k(i) * t(i) * t(i); return true; };
         auto grad = [&](const math_Vector& t, math_Vector& g) { for (int i = t.Lower(); i <= t.Upper(); ++i) g(i) = 2.0 * k(i) * t(i); return true; };
         auto vl = MakeVector(val); auto vg = MakeVectorWithGradient(val, grad);
         double y = 3.0, y2 = 3.0, y3 = 3.0; math_Vector g(l, l + n - 1, 0.0), g2(l, l + n - 1, 0.0);
         bool ok = vl.Value(x, y), ok2 = vg.Value(x, y2), ok3 = vg.Gradient(x, g), ok4 = vg.Values(x, y3, g2);
         B(ok); F(y); B(ok2); F(y2); B(ok3); V(g); B(ok4); F(y3); V(g2);
         auto sys = MakeSystem([&](const math_Vector& t, math_Vector& f) { if (t(t.Lower()) < -1.0) return false; for (int i = 1; i <= m; ++i) f(i) = i * t(t.Lower()) + k(k.Lower()); return true; }, m);
         math_Vector f(1, m, 0.0); bool ok5 = sys.Value(x, f); I(sys.NbEquations()); B(ok5); V(f);)
  }
  // -- Gauss and Kronrod points ----------------------------------------------------------------------
  const int ORD[16] = {1, 2, 3, 4, 5, 7, 8, 15, 16, 31, 61, 62, 64, 70, 100, 0};
  for (int fam = 0; fam < 3; ++fam)
    for (int j = 0; j < 16; ++j) {  // gauss_points_weights, ordered_gauss_points_weights, kronrod_points_weights
      smash();
      int order = ORD[j];
      static const int KRON[16] = {3, 5, 9, 7, 11, 19, 13, 17, 27, 33, 23, 37, 4, 123, 125, 201};
      if (fam == 2) order = KRON[j];
      int np = (j == 11 && fam != 2) ? order + 1 : order, nw = (j == 12 && fam != 2) ? order - 1 : order;
      if (fam == 2 && j == 9) np = order + 2;
      int lp = LB[j % 4], lw = LB[(j + 1) % 4];
      // OCCT's point functions write the weights at the lower bound of the points (out of range, unchecked, when the
      // lower bounds differ): both vectors always get the same lower bound
      lw = lp;
      if (np < 1) np = 1;
      if (nw < 1) nw = 1;
      begin(fam == 0 ? "gauss_points_weights" : (fam == 1 ? "ordered_gauss_points_weights" : "kronrod_points_weights"), "iiiii"); I(order); I(np); I(nw); I(lp); I(lw); bar();
      CASE(math_Vector p(lp, lp + np - 1, 0.0), w(lw, lw + nw - 1, 0.0);
           bool ok = fam == 0 ? GetGaussPointsAndWeights(order, p, w) : (fam == 1 ? GetOrderedGaussPointsAndWeights(order, p, w) : GetKronrodPointsAndWeights(order, p, w));
           B(ok); V(p); V(w);)
    }
  return 0;
}
