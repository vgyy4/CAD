// Deterministic reference records for the header-only OCCT MathSys package (OCCT 8.0.1), see docs/api-mathsys.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values decimal; a matrix is `lowerRow upperRow lowerCol upperCol values(row-major)`, a vector `lower upper values`.
// Callbacks are a closed set of functions described by printed parameters; every call is counted and its argument bits
// are folded into a 64-bit hash (the call sequence is part of the behaviour).
#include <MathSys_LevenbergMarquardt.hxx>
#include <MathSys_Newton.hxx>
#include <MathSys_Newton2D.hxx>
#include <MathSys_Newton3D.hxx>
#include <MathSys_Newton4D.hxx>
#include <MathSys_NewtonTypes.hxx>
#include <Standard_Failure.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

using namespace MathSys;

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
static void smash() {
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

static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;

// ---- a function set: F_k(x) = sum_j C(k, j) x_j + c3 * x_(k mod n)^3 - b_k --------------------------------------------
struct TSys : public math_FunctionSetWithDerivatives {
  int n, m;
  math_Matrix C;
  math_Vector b;
  double c3, failHi, failLo;
  int vcalls = 0, dcalls = 0, vscalls = 0;
  uint64_t hash = H0;
  TSys(int n_, int m_, math_Matrix C_, math_Vector b_, double c3_, double fh, double fl)
      : n(n_), m(m_), C(C_), b(b_), c3(c3_), failHi(fh), failLo(fl) {}
  int NbVariables() const override { return n; }
  int NbEquations() const override { return m; }
  void mixx(const math_Vector& x, uint64_t salt) {
    hash = mixh(hash, salt);
    for (int i = x.Lower(); i <= x.Upper(); ++i) hash = mixh(hash, bitsOf(x(i)));
  }
  void fill(const math_Vector& x, math_Vector* F, math_Matrix* D) const {
    for (int k = 0; k < m; ++k) {
      if (F) {
        double s = 0.0;
        for (int j = 0; j < n; ++j) s += C(C.LowerRow() + k, C.LowerCol() + j) * x(x.Lower() + j);
        double xc = x(x.Lower() + k % n);
        (*F)(F->Lower() + k) = s + c3 * xc * xc * xc - b(b.Lower() + k);
      }
      if (D) {
        for (int j = 0; j < n; ++j)
          (*D)(D->LowerRow() + k, D->LowerCol() + j) = C(C.LowerRow() + k, C.LowerCol() + j) + (j == k % n ? 3.0 * c3 * x(x.Lower() + j) * x(x.Lower() + j) : 0.0);
      }
    }
  }
  bool Value(const math_Vector& x, math_Vector& f) override {
    ++vcalls;
    mixx(x, 1);
    if (x(x.Lower()) > failHi) return false;
    fill(x, &f, nullptr);
    return true;
  }
  bool Derivatives(const math_Vector& x, math_Matrix& d) override {
    ++dcalls;
    mixx(x, 2);
    if (x(x.Lower()) > failHi || x(x.Lower()) < failLo) return false;
    fill(x, nullptr, &d);
    return true;
  }
  bool Values(const math_Vector& x, math_Vector& f, math_Matrix& d) override {
    ++vscalls;
    mixx(x, 3);
    if (x(x.Lower()) > failHi || x(x.Lower()) < failLo) return false;
    fill(x, &f, &d);
    return true;
  }
};
static void printSys(const TSys& t) { I(t.n); I(t.m); F(t.c3); F(t.failHi); F(t.failLo); M(t.C); V(t.b); }
#define SYS_SCHEMA "iifffMV"
static void outCalls(const TSys& t) { I(t.vcalls); I(t.dcalls); I(t.vscalls); U(t.hash); }
static void prV(const MathUtils::VectorResult& r, const TSys& t) {
  I((int)r.Status); I(r.NbIterations);
  B(r.Solution.has_value()); if (r.Solution) V(*r.Solution);
  B(r.Value.has_value()); if (r.Value) F(*r.Value);
  B(r.Gradient.has_value()); if (r.Gradient) V(*r.Gradient);
  B(r.Jacobian.has_value()); if (r.Jacobian) M(*r.Jacobian);
  outCalls(t);
}
// a square, well-conditioned system (diagonally dominant C) for the Newton solvers; n variables, m equations
static TSys genSys(int j, int n, int m, bool square) {
  math_Matrix C = genM(1, m, 1, n);
  for (int i = 1; i <= m && i <= n; ++i) C(i, i) += (i % 2 ? 1 : -1) * (2.0 * n + 1.0);
  if (square && j % 8 == 5) for (int c = 1; c <= n; ++c) C(n, c) = C(1, c);  // singular Jacobian (linear part)
  double c3 = (j % 3 == 0) ? 0.0 : 0.125 * rv();
  double failHi = (j % 7 == 3) ? 0.25 : 1e300, failLo = (j % 9 == 4) ? 0.5 : -1e300;
  return TSys(n, m, C, genV(1, m), c3, failHi, failLo);
}

// ---- fixed-size callbacks ---------------------------------------------------------------------------------
template <int N>
struct TN {
  double A[N][N], c[N], b[N], failx;
  mutable int calls = 0;
  mutable uint64_t hash = H0;
  bool operator()(const double* x, double* f, double (*J)[N]) const {
    ++calls;
    for (int i = 0; i < N; ++i) hash = mixh(hash, bitsOf(x[i]));
    if (x[0] > failx) return false;
    for (int k = 0; k < N; ++k) {
      double s = 0.0;
      for (int j = 0; j < N; ++j) s += A[k][j] * x[j];
      f[k] = s + c[k] * x[k] * x[k] - b[k];
      for (int j = 0; j < N; ++j) J[k][j] = A[k][j] + (j == k ? 2.0 * c[k] * x[k] : 0.0);
    }
    return true;
  }
  bool operator()(double x0, double x1, double F[2], double J[2][2]) const {
    double x[2] = {x0, x1};
    return (*this)(x, F, J);
  }
  bool operator()(double x0, double x1, double x2, double F[3], double J[3][3]) const {
    double x[3] = {x0, x1, x2};
    return (*this)(x, F, J);
  }
  bool operator()(double x0, double x1, double x2, double x3, double F[4], double J[4][4]) const {
    double x[4] = {x0, x1, x2, x3};
    return (*this)(x, F, J);
  }
};
template <int N>
static TN<N> genTN(int j) {
  TN<N> t;
  for (int a = 0; a < N; ++a) {
    for (int c = 0; c < N; ++c) t.A[a][c] = rv() * 0.5 + (a == c ? 2.0 + a : 0.0);
    t.c[a] = (j % 3 == 0) ? 0.0 : 0.25 * rv();
    t.b[a] = rv();
  }
  if (j % 8 == 5) for (int c = 0; c < N; ++c) t.A[N - 1][c] = t.A[0][c];  // singular linear part
  if (j % 8 == 6) for (int a = 0; a < N; ++a) for (int c = 0; c < N; ++c) t.A[a][c] = 0.0;  // zero Jacobian except the quadratic part
  t.failx = (j % 11 == 7) ? 0.75 : 1e300;
  return t;
}
template <int N>
static void printTN(const TN<N>& t) {
  for (int a = 0; a < N; ++a) for (int c = 0; c < N; ++c) F(t.A[a][c]);
  for (int a = 0; a < N; ++a) F(t.c[a]);
  for (int a = 0; a < N; ++a) F(t.b[a]);
  F(t.failx);
}
template <int N>
static std::string tnSchema() { return std::string(N * N + 2 * N + 1, 'f'); }

// symmetric 2D system: the gradient of phi = (H11 u^2 + 2 H12 u v + H22 v^2) / 2 + c (u^4 + v^4) / 4 - b . p
struct TSym {
  double H11, H12, H22, c, b1, b2, failx;
  mutable int calls = 0;
  mutable uint64_t hash = H0;
  bool Value(double u, double v, double& f1, double& f2) const {
    ++calls;
    hash = mixh(hash, bitsOf(u) ^ 0x11);
    hash = mixh(hash, bitsOf(v));
    if (u > failx) return false;
    f1 = H11 * u + H12 * v + c * u * u * u - b1;
    f2 = H12 * u + H22 * v + c * v * v * v - b2;
    return true;
  }
  bool ValueAndJacobian(double u, double v, double& f1, double& f2, double& j11, double& j12, double& j22) const {
    ++calls;
    hash = mixh(hash, bitsOf(u) ^ 0x22);
    hash = mixh(hash, bitsOf(v));
    if (u > failx) return false;
    f1 = H11 * u + H12 * v + c * u * u * u - b1;
    f2 = H12 * u + H22 * v + c * v * v * v - b2;
    j11 = H11 + 3.0 * c * u * u;
    j12 = H12;
    j22 = H22 + 3.0 * c * v * v;
    return true;
  }
};
static TSym genSym(int j) {
  double h11 = 2.0 + std::abs(rv()), h22 = 2.0 + std::abs(rv()), h12 = 0.5 * rv();
  if (j % 8 == 5) { h12 = std::sqrt(h11 * h22); h12 = std::floor(h12 * 1024.0) / 1024.0; h22 = h12 * h12 / h11; }
  if (j % 8 == 6) { h11 = 0.0; h12 = 0.0; h22 = 0.0; }
  return TSym{h11, h12, h22, (j % 3 == 0) ? 0.0 : 0.25 * rv(), rv(), rv(), (j % 11 == 7) ? 0.75 : 1e300};
}

// a polynomial curve P0 + t A + t^2 B + t^3 C and a polynomial surface P0 + u A + v B + u^2 C + v^2 D + u v E (all vectors printed)
struct TCurve {
  double p[3], a[3], b[3], c[3];
  void D2(double t, gp_Pnt& P, gp_Vec& D1, gp_Vec& D2v) const {
    P = gp_Pnt(p[0] + t * a[0] + t * t * b[0] + t * t * t * c[0], p[1] + t * a[1] + t * t * b[1] + t * t * t * c[1], p[2] + t * a[2] + t * t * b[2] + t * t * t * c[2]);
    D1 = gp_Vec(a[0] + 2.0 * t * b[0] + 3.0 * t * t * c[0], a[1] + 2.0 * t * b[1] + 3.0 * t * t * c[1], a[2] + 2.0 * t * b[2] + 3.0 * t * t * c[2]);
    D2v = gp_Vec(2.0 * b[0] + 6.0 * t * c[0], 2.0 * b[1] + 6.0 * t * c[1], 2.0 * b[2] + 6.0 * t * c[2]);
  }
};
struct TSurf {
  double p[3], a[3], b[3], c[3], d[3], e[3];
  void D2(double u, double v, gp_Pnt& P, gp_Vec& Du, gp_Vec& Dv, gp_Vec& Duu, gp_Vec& Dvv, gp_Vec& Duv) const {
    double q[3], du[3], dv[3], duu[3], dvv[3], duv[3];
    for (int i = 0; i < 3; ++i) {
      q[i] = p[i] + u * a[i] + v * b[i] + u * u * c[i] + v * v * d[i] + u * v * e[i];
      du[i] = a[i] + 2.0 * u * c[i] + v * e[i];
      dv[i] = b[i] + 2.0 * v * d[i] + u * e[i];
      duu[i] = 2.0 * c[i];
      dvv[i] = 2.0 * d[i];
      duv[i] = e[i];
    }
    P = gp_Pnt(q[0], q[1], q[2]);
    Du = gp_Vec(du[0], du[1], du[2]);
    Dv = gp_Vec(dv[0], dv[1], dv[2]);
    Duu = gp_Vec(duu[0], duu[1], duu[2]);
    Dvv = gp_Vec(dvv[0], dvv[1], dvv[2]);
    Duv = gp_Vec(duv[0], duv[1], duv[2]);
  }
};
static void gen3(double* v, double s = 1.0) { for (int i = 0; i < 3; ++i) v[i] = rv() * s; }
static void print3(const double* v) { F(v[0]); F(v[1]); F(v[2]); }

static NewtonOptions genOpts(int j) {
  NewtonOptions o;
  o.FTolerance = (j % 4 == 0) ? 1e-10 : ((j % 4 == 1) ? 1e-6 : 1e-12);
  o.XTolerance = (j % 3 == 0) ? 1e-16 : 1e-9;
  o.MaxIterations = (j == 2) ? 1 : (j == 3 ? 0 : (j % 5 == 4 ? 4 : 100));
  o.MaxStepRatio = (j % 6 == 5) ? 0.05 : 0.5;
  o.EnableLineSearch = (j % 2 == 0);
  o.AllowSoftBounds = (j % 5 == 2);
  o.SoftBoundsExtension = (j % 7 == 4) ? 0.1 : 1e-4;
  if (j == 11) o.FTolerance = 0.0;
  if (j == 13) o.MaxStepRatio = -1.0;
  if (j == 15) o.SoftBoundsExtension = -0.5;
  return o;
}
static void printOpts(const NewtonOptions& o) {
  F(o.Tolerance); F(o.XTolerance); F(o.FTolerance); I(o.MaxIterations); F(o.MaxStepRatio); B(o.EnableLineSearch); B(o.AllowSoftBounds);
  F(o.SoftBoundsExtension);
}
#define OPT_SCHEMA "fffifiif"
template <int N>
static NewtonBoundsN<N> genBounds(int j) {
  NewtonBoundsN<N> b;
  for (int i = 0; i < N; ++i) { b.Min[i] = -3.0 - 0.5 * i - (j % 4); b.Max[i] = 3.0 + 0.25 * i + (j % 3); }
  if (j % 9 == 6) b.HasBounds = false;
  if (j % 10 == 7) { b.Min[0] = 1.0; b.Max[0] = 0.5; }  // invalid
  if (j % 6 == 2) { b.Min[N - 1] = -0.5; b.Max[N - 1] = 0.5; }
  return b;
}
template <int N>
static void printBounds(const NewtonBoundsN<N>& b) {
  for (int i = 0; i < N; ++i) { F(b.Min[i]); F(b.Max[i]); }
  B(b.HasBounds);
}
template <int N>
static std::string boundsSchema() { return std::string(2 * N, 'f') + "i"; }
template <int N>
static void prN(const NewtonResultN<N>& r, int calls, uint64_t hash) {
  I((int)r.Status);
  for (int i = 0; i < N; ++i) F(r.X[i]);
  I(r.NbIterations); F(r.ResidualNorm); F(r.StepNorm);
  I(calls); U(hash);
}

int main() {
  // -- generic Newton ------------------------------------------------------------------------------------
  for (int fam = 0; fam < 3; ++fam)
    for (int j = 0; j < 16; ++j) {  // newton, newton_scalar_tol, newton_bounded
      smash();
      int n = 1 + j % 4, l = LB[j % 4];
      int m = (j == 13) ? n + 1 : n;
      TSys t = genSys(j, n, m, true);
      math_Vector x0 = genV(l, (j == 14) ? n + 1 : n);
      math_Vector tolx = genV((j == 12) ? l + 1 : l, n);  // j == 12: another lower bound, indexed out of range
      for (int i = tolx.Lower(); i <= tolx.Upper(); ++i) tolx(i) = std::abs(tolx(i)) * ((j % 2) ? 1e-6 : 1e-10);
      double tolF = (j % 3 == 0) ? 1e-10 : 1e-6, tolXs = (j % 2) ? 1e-9 : 1e-12;
      int mi = (j == 2) ? 1 : (j == 3 ? 0 : (j % 5 == 4 ? 4 : 100));
      math_Vector inf = genV(l, n), sup = genV(l, n);
      for (int i = l; i < l + n; ++i) { inf(i) = -1.0 - std::abs(inf(i)); sup(i) = 1.0 + std::abs(sup(i)); }
      if (j % 8 == 7) for (int i = l; i < l + n; ++i) { inf(i) = -0.25; sup(i) = 0.25; }
      if (fam == 0) {
        begin("newton", SYS_SCHEMA "VVfi"); printSys(t); V(x0); V(tolx); F(tolF); I(mi); bar();
        CASE(prV(Newton(t, x0, tolx, tolF, mi), t);)
      } else if (fam == 1) {
        begin("newton_scalar_tol", SYS_SCHEMA "Vffi"); printSys(t); V(x0); F(tolXs); F(tolF); I(mi); bar();
        CASE(prV(Newton(t, x0, tolXs, tolF, mi), t);)
      } else {
        begin("newton_bounded", SYS_SCHEMA "VVVVfi"); printSys(t); V(x0); V(inf); V(sup); V(tolx); F(tolF); I(mi); bar();
        CASE(prV(NewtonBounded(t, x0, inf, sup, tolx, tolF, mi), t);)
      }
    }
  // -- Levenberg-Marquardt -------------------------------------------------------------------------------
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 16; ++j) {  // levenberg_marquardt, levenberg_marquardt_bounded
      smash();
      int n = 1 + j % 3, l = LB[j % 4];
      int m = n + j % 4;
      TSys t = genSys(j, n, m, false);
      math_Vector x0 = genV(l, (j == 14) ? n + 1 : n, 2.0);
      LMConfig c((j % 3 == 0) ? 1e-10 : ((j % 3 == 1) ? 1e-6 : 1e-12), (j == 2) ? 1 : (j == 3 ? 0 : (j % 5 == 4 ? 4 : 100)));
      if (j % 4 == 1) { c.LambdaInit = 1.0; c.LambdaIncrease = 4.0; c.LambdaDecrease = 0.25; }
      if (j % 6 == 5) c.LambdaMax = 1e-2;
      math_Vector inf = genV(l, (j == 13) ? n + 1 : n), sup = genV(l, n);
      for (int i = l; i < l + n; ++i) { inf(i) = -0.75 - std::abs(inf(i)) * 0.5; sup(i) = 0.75 + std::abs(sup(i)) * 0.5; }
      if (fam == 0) {
        begin("levenberg_marquardt", SYS_SCHEMA "Vffifffff"); printSys(t); V(x0); F(c.Tolerance); F(c.XTolerance); I(c.MaxIterations);
        F(c.LambdaInit); F(c.LambdaIncrease); F(c.LambdaDecrease); F(c.LambdaMax); F(c.LambdaMin); bar();
        CASE(prV(LevenbergMarquardt(t, x0, c), t);)
      } else {
        begin("levenberg_marquardt_bounded", SYS_SCHEMA "VVVffifffff"); printSys(t); V(x0); V(inf); V(sup); F(c.Tolerance); F(c.XTolerance); I(c.MaxIterations);
        F(c.LambdaInit); F(c.LambdaIncrease); F(c.LambdaDecrease); F(c.LambdaMax); F(c.LambdaMin); bar();
        CASE(prV(LevenbergMarquardtBounded(t, x0, inf, sup, c), t);)
      }
    }
  // -- fixed-size Newton ---------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // solve2d
    smash();
    TN<2> t = genTN<2>(j);
    std::array<double, 2> x0 = {rv() * 2.0, rv() * 2.0};
    NewtonBoundsN<2> bd = genBounds<2>(j);
    NewtonOptions o = genOpts(j);
    begin("solve2d", (tnSchema<2>() + "ff" + boundsSchema<2>() + OPT_SCHEMA).c_str());
    printTN<2>(t); F(x0[0]); F(x0[1]); printBounds<2>(bd); printOpts(o); bar();
    CASE(auto r = Solve2D(t, x0, bd, o); prN<2>(r, t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // solve2d_symmetric
    smash();
    TSym t = genSym(j);
    std::array<double, 2> x0 = {rv() * 2.0, rv() * 2.0};
    NewtonBoundsN<2> bd = genBounds<2>(j);
    NewtonOptions o = genOpts(j + 1);
    begin("solve2d_symmetric", (std::string("fffffff") + "ff" + boundsSchema<2>() + OPT_SCHEMA).c_str());
    F(t.H11); F(t.H12); F(t.H22); F(t.c); F(t.b1); F(t.b2); F(t.failx); F(x0[0]); F(x0[1]); printBounds<2>(bd); printOpts(o); bar();
    CASE(auto r = Solve2DSymmetric(t, x0, bd, o); prN<2>(r, t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // solve3d
    smash();
    TN<3> t = genTN<3>(j);
    std::array<double, 3> x0 = {rv() * 2.0, rv() * 2.0, rv() * 2.0};
    NewtonBoundsN<3> bd = genBounds<3>(j);
    NewtonOptions o = genOpts(j);
    begin("solve3d", (tnSchema<3>() + "fff" + boundsSchema<3>() + OPT_SCHEMA).c_str());
    printTN<3>(t); F(x0[0]); F(x0[1]); F(x0[2]); printBounds<3>(bd); printOpts(o); bar();
    CASE(auto r = Solve3D(t, x0, bd, o); prN<3>(r, t.calls, t.hash);)
  }
  for (int j = 0; j < 16; ++j) {  // solve4d
    smash();
    TN<4> t = genTN<4>(j);
    std::array<double, 4> x0 = {rv() * 2.0, rv() * 2.0, rv() * 2.0, rv() * 2.0};
    NewtonBoundsN<4> bd = genBounds<4>(j);
    NewtonOptions o = genOpts(j);
    begin("solve4d", (tnSchema<4>() + "ffff" + boundsSchema<4>() + OPT_SCHEMA).c_str());
    printTN<4>(t); F(x0[0]); F(x0[1]); F(x0[2]); F(x0[3]); printBounds<4>(bd); printOpts(o); bar();
    CASE(auto r = Solve4D(t, x0, bd, o); prN<4>(r, t.calls, t.hash);)
  }
  // -- extrema through the evaluators --------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // solve_curve_surface_extrema_3d
    smash();
    TCurve cu; TSurf su;
    gen3(cu.p, 2.0); gen3(cu.a); gen3(cu.b, 0.5); gen3(cu.c, 0.25);
    gen3(su.p, 2.0); gen3(su.a); gen3(su.b); gen3(su.c, 0.25); gen3(su.d, 0.25); gen3(su.e, 0.25);
    if (j % 4 == 3) { su.c[0] = su.c[1] = su.c[2] = su.d[0] = su.d[1] = su.d[2] = su.e[0] = su.e[1] = su.e[2] = 0.0; }  // a plane
    std::array<double, 3> x0 = {rv(), rv(), rv()};
    NewtonBoundsN<3> bd = genBounds<3>(j);
    NewtonOptions o = genOpts(j);
    begin("solve_curve_surface_extrema_3d", (std::string(30, 'f') + "fff" + boundsSchema<3>() + OPT_SCHEMA).c_str());
    print3(cu.p); print3(cu.a); print3(cu.b); print3(cu.c);
    print3(su.p); print3(su.a); print3(su.b); print3(su.c); print3(su.d); print3(su.e);
    F(x0[0]); F(x0[1]); F(x0[2]); printBounds<3>(bd); printOpts(o); bar();
    CASE(auto r = SolveCurveSurfaceExtrema3D(cu, su, x0, bd, o); prN<3>(r, 0, 0);)
  }
  for (int j = 0; j < 16; ++j) {  // solve_surface_surface_extrema_4d
    smash();
    TSurf s1, s2;
    gen3(s1.p, 2.0); gen3(s1.a); gen3(s1.b); gen3(s1.c, 0.25); gen3(s1.d, 0.25); gen3(s1.e, 0.25);
    gen3(s2.p, 2.0); gen3(s2.a); gen3(s2.b); gen3(s2.c, 0.25); gen3(s2.d, 0.25); gen3(s2.e, 0.25);
    std::array<double, 4> x0 = {rv(), rv(), rv(), rv()};
    NewtonBoundsN<4> bd = genBounds<4>(j);
    NewtonOptions o = genOpts(j);
    begin("solve_surface_surface_extrema_4d", (std::string(36, 'f') + "ffff" + boundsSchema<4>() + OPT_SCHEMA).c_str());
    print3(s1.p); print3(s1.a); print3(s1.b); print3(s1.c); print3(s1.d); print3(s1.e);
    print3(s2.p); print3(s2.a); print3(s2.b); print3(s2.c); print3(s2.d); print3(s2.e);
    F(x0[0]); F(x0[1]); F(x0[2]); F(x0[3]); printBounds<4>(bd); printOpts(o); bar();
    CASE(auto r = SolveSurfaceSurfaceExtrema4D(s1, s2, x0, bd, o); prN<4>(r, 0, 0);)
  }
  // -- helpers (namespace detail) ------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // solve_symmetric_2x2_svd
    smash();
    double j11 = rv(), j12 = rv(), j22 = rv(), f1 = rv(), f2 = rv();
    if (j % 4 == 1) { j12 = std::sqrt(std::abs(j11 * j22)); }
    if (j % 4 == 2) { j11 = 1.0; j22 = 1.0; j12 = 1.0 - 1e-17 * j; }  // singular
    if (j == 3) { j11 = j12 = j22 = 0.0; }
    double tol = (j % 5 == 4) ? 1e-3 : 1e-15;
    begin("solve_symmetric_2x2_svd", "ffffff"); F(j11); F(j12); F(j22); F(f1); F(f2); F(tol); bar();
    CASE(double du = 7.0, dv = 7.0; bool ok = detail::SolveSymmetric2x2SVD(j11, j12, j22, f1, f2, du, dv, tol); B(ok); F(du); F(dv);
         double du2 = 7.0, dv2 = 7.0; bool ok2 = detail::SolveSymmetric2x2SVD(j11, j12, j22, f1, f2, du2, dv2); B(ok2); F(du2); F(dv2);)
  }
  for (int j = 0; j < 16; ++j) {  // solve3x3
    smash();
    double J[3][3], f[3], d[3] = {7.0, 7.0, 7.0};
    for (auto& r : J) for (double& x : r) x = rv();
    for (double& x : f) x = rv();
    if (j % 4 == 1) for (int c = 0; c < 3; ++c) J[2][c] = J[0][c];
    if (j == 3) for (auto& r : J) for (double& x : r) x = 0.0;
    begin("solve3x3", "ffffffffffff"); for (auto& r : J) for (double x : r) F(x); for (double x : f) F(x); bar();
    CASE(bool ok = detail::Solve3x3(J, f, d); B(ok); F(d[0]); F(d[1]); F(d[2]);)
  }
  for (int j = 0; j < 16; ++j) {  // solve4x4
    smash();
    double J[4][4], f[4], d[4] = {7.0, 7.0, 7.0, 7.0};
    for (auto& r : J) for (double& x : r) x = rv();
    for (double& x : f) x = rv();
    if (j % 4 == 1) for (int c = 0; c < 4; ++c) J[3][c] = J[1][c];
    if (j % 4 == 2) for (int r = 0; r < 4; ++r) J[r][0] = 0.0;
    if (j == 3) for (auto& r : J) for (double& x : r) x = 0.0;
    begin("solve4x4", "ffffffffffffffffffff"); for (auto& r : J) for (double x : r) F(x); for (double x : f) F(x); bar();
    CASE(bool ok = detail::Solve4x4(J, f, d); B(ok); F(d[0]); F(d[1]); F(d[2]); F(d[3]);)
  }
  for (int j = 0; j < 16; ++j) {  // clamp_nd
    smash();
    NewtonBoundsN<2> b2 = genBounds<2>(j % 8);
    NewtonBoundsN<3> b3 = genBounds<3>(j % 8);
    NewtonBoundsN<4> b4 = genBounds<4>(j % 8);
    for (int i = 0; i < 2; ++i) { if (b2.Min[i] > b2.Max[i]) std::swap(b2.Min[i], b2.Max[i]); }
    for (int i = 0; i < 3; ++i) { if (b3.Min[i] > b3.Max[i]) std::swap(b3.Min[i], b3.Max[i]); }
    for (int i = 0; i < 4; ++i) { if (b4.Min[i] > b4.Max[i]) std::swap(b4.Min[i], b4.Max[i]); }
    b2.HasBounds = b3.HasBounds = b4.HasBounds = (j % 5 != 4);
    double sc = (j % 3 == 0) ? 8.0 : 3.0;
    std::array<double, 4> x = {rv() * sc, rv() * sc, rv() * sc, rv() * sc};
    bool soft = j % 2;
    double ext = (j % 4 == 1) ? 0.1 : 1e-4;
    begin("clamp_nd", (std::string("ffff") + boundsSchema<2>() + boundsSchema<3>() + boundsSchema<4>() + "if").c_str()); F(x[0]); F(x[1]); F(x[2]); F(x[3]);
    printBounds<2>(b2); printBounds<3>(b3); printBounds<4>(b4); B(soft); F(ext); bar();
    CASE(std::array<double, 2> a2 = {x[0], x[1]}; detail::Clamp2D(a2, b2, soft, ext); F(a2[0]); F(a2[1]);
         std::array<double, 3> a3 = {x[0], x[1], x[2]}; detail::Clamp3D(a3, b3, soft, ext); F(a3[0]); F(a3[1]); F(a3[2]);
         std::array<double, 4> a4 = {x[0], x[1], x[2], x[3]}; detail::Clamp4D(a4, b4, soft, ext); F(a4[0]); F(a4[1]); F(a4[2]); F(a4[3]);
         F(detail::MaxDomainSize2D(b2)); F(detail::MaxDomainSize3D(b3)); F(detail::MaxDomainSize4D(b4));
         B(detail::IsBoundsValid2D(b2)); B(detail::IsBoundsValid3D(b3)); B(detail::IsBoundsValid4D(b4));)
  }
  for (int j = 0; j < 16; ++j) {  // options_validity
    smash();
    NewtonOptions o = genOpts(j);
    NewtonBoundsN<2> b2 = genBounds<2>(j);
    NewtonBoundsN<3> b3 = genBounds<3>(j);
    NewtonBoundsN<4> b4 = genBounds<4>(j);
    begin("options_validity", (std::string(OPT_SCHEMA) + boundsSchema<2>() + boundsSchema<3>() + boundsSchema<4>()).c_str()); printOpts(o); printBounds<2>(b2); printBounds<3>(b3); printBounds<4>(b4); bar();
    CASE(B(detail::IsOptionsValid(o)); B(detail::IsOptionsValid3D(o)); B(detail::IsOptionsValid4D(o)); B(detail::IsBoundsValid2D(b2));
         B(detail::IsBoundsValid3D(b3)); B(detail::IsBoundsValid4D(b4)); F(detail::MaxDomainSize2D(b2)); F(detail::MaxDomainSize3D(b3));
         F(detail::MaxDomainSize4D(b4));
         F(detail::THE_SINGULAR_DET_TOL); F(detail::THE_CRITICAL_GRAD_SQ); I(detail::THE_LINE_SEARCH_MAX); F(detail::THE_ARMIJO_C1);)
  }
  for (int j = 0; j < 4; ++j) {  // types (defaults)
    smash();
    begin("types", "i"); I(j); bar();
    CASE(NewtonOptions o; LMConfig c; LMConfig c2(1e-6, 5 + j); NewtonResultN<2> r2; NewtonResultN<3> r3; NewtonResultN<4> r4; NewtonBoundsN<2> b2;
         NewtonBoundsN<3> b3; NewtonBoundsN<4> b4;
         F(o.Tolerance); F(o.XTolerance); F(o.FTolerance); I(o.MaxIterations); F(o.StepMin); F(o.MaxStepRatio); B(o.EnableLineSearch);
         B(o.AllowSoftBounds); F(o.SoftBoundsExtension);
         F(c.Tolerance); F(c.XTolerance); F(c.FTolerance); I(c.MaxIterations); F(c.StepMin); F(c.LambdaInit); F(c.LambdaIncrease);
         F(c.LambdaDecrease); F(c.LambdaMax); F(c.LambdaMin);
         F(c2.Tolerance); F(c2.XTolerance); F(c2.FTolerance); I(c2.MaxIterations); F(c2.LambdaInit);
         I((int)r2.Status); I(r2.NbIterations); F(r2.ResidualNorm); F(r2.StepNorm); F(r2.X[0]); F(r2.X[1]); B(r2.IsDone());
         I((int)r3.Status); F(r3.X[2]); I((int)r4.Status); F(r4.X[3]);
         F(b2.Min[1]); F(b2.Max[1]); B(b2.HasBounds); F(b3.Min[2]); B(b3.HasBounds); F(b4.Max[3]); B(b4.HasBounds);
         r2.Status = static_cast<MathUtils::Status>(j % 2 ? 0 : 3); B(r2.IsDone());)
  }
  return 0;
}
