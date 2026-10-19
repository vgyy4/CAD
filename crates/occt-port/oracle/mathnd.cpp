// Deterministic reference calls for OCCT 8.0.1's multi-variable solvers and numerical integration of the `math`
// package: the callback interfaces math_MultipleVarFunction, math_MultipleVarFunctionWithGradient,
// math_MultipleVarFunctionWithHessian, math_FunctionSet and math_FunctionSetWithDerivatives; the solvers
// math_FunctionSetRoot, math_NewtonFunctionSetRoot, math_NewtonMinimum, math_BFGS, math_FRPR, math_Powell,
// math_Uzawa and math_BissecNewton; and math_GaussSingleIntegration, math_GaussMultipleIntegration,
// math_GaussSetIntegration and math_KronrodSingleIntegration.
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints (and bools) are decimal. Every input of a call is printed on the
// left of " | " in the order named by the family's schema string (`begin(name, schema)`); tests/mathnd.rs reads them
// back with the reader method of the same name as each schema word. The oracle aborts if the words it printed differ
// from the schema (and if a word printed the wrong number of tokens). Outputs are values, `throws` (OCCT raised an
// exception that the line's call really raises: an explicit `throw`, or a check that is compiled into the caller,
// like the StdFail_NotDone checks of the inline accessors) or `unchecked` (OCCT 8.0.1 as built has no check, so the
// numbers it would return are unspecified; the Rust API must return an error).
//
// Schema words (token counts):
//   f   a float (1)                       i   an int or bool (1)
//   v   a math_Vector: lower length x_1 .. x_length (2 + length)
//   iv  a math_IntegerVector: lower length k_1 .. k_length (2 + length)
//   m   a math_Matrix: lower_row rows lower_col cols a_11 a_12 .. (row major) (4 + rows * cols)
//   tf  a one-variable test function, the record of oracle/math1d.cpp:
//       kind nparams p_1 .. p_nparams fail_lo fail_hi fail_mode state_mode (6 + nparams)
//   mv  a multiple-variable test function: n mode ng g_1 .. g_ng fail_mode lo_1 .. lo_n hi_1 .. hi_n state_mode
//   fs  a function-set test function:      n neq g_1 .. g_neq fail_mode lo_1 .. lo_n hi_1 .. hi_n state_mode
//       where every g is `gkind nparams p_1 .. p_nparams` (see THE SCALAR TERMS below).
//
// THE TEST FUNCTIONS. The solvers call user functions; the judge cannot pass closures through text, so every
// function is fully described by its record and implemented identically here and in tests/mathnd.rs, with
// + - * / floor and fabs only (no libm), so that both sides see the same bits. The one-variable functions (`tf`)
// are those of oracle/math1d.cpp (polynomials, trigonometric sums, rational, multiple roots, flat, exp, abs).
// The functions of several variables are built from scalar terms g(x), x = (x_0 .. x_{m-1}) (x_j is the vector
// element at position j, i.e. X(X.Lower() + j)):
//   gkind 0 QUAD   p = c, b_0..b_{m-1}, A_00 A_01 .. A_{m-1,m-1} (row major):
//                  g = c + sum_i x_i (b_i + 0.5 sum_j A_ij x_j); dg_i = b_i + 0.5 sum_j (A_ij + A_ji) x_j;
//                  d2g_ij = 0.5 (A_ij + A_ji)
//   gkind 1 ROSEN  p = a, b: m = 1: (a - x_0)^2; else sum_{i<m-1} b (x_{i+1} - x_i^2)^2 + (a - x_i)^2
//   gkind 2 POLY   p = nterms, then per term: coef, e_0 .. e_{m-1} (small integers): g = sum coef prod x_k^e_k
//   gkind 3 TRIG   p = c, then per variable A W P B: g = c + sum_j A sinp(W x_j + P) + B x_j^2
//   gkind 4 KINK   p = c, then per variable a k: g = c + sum_j a |x_j - k|
//   gkind 5 EXP    p = c, a, w_0 .. w_{m-1}: g = c + a expp(sum_j w_j x_j)
// (the exact evaluation orders are the code's: g_value, g_grad, g_hess). A `mv` function is, by mode, the sum of its
// terms (mode 0) or the sum of their squares (mode 1: f = sum g_k^2, grad = sum 2 g_k dg_k,
// hess = sum 2 (dg_k dg_k^T + g_k d2g_k)); a `fs` function has the equations F_k = g_k, derivatives D_kj = dg_k/dx_j.
// A function FAILS (the callback returns false and writes nothing) when lo_j <= x_j <= hi_j for every j and the bit
// of the callback is set in fail_mode: mv 1 = Value, 2 = Gradient, 4 = Values(F, G), 8 = Values(F, G, H);
// fs 1 = Value, 2 = Derivatives, 4 = Values. GetStateNumber returns, by state_mode: 0 -> 0; 1 -> the number of
// calls so far; 2 -> 4 * (that number) + a code of the last call (mv: 1 Value, 2 Gradient, 3 Values, 4 Values with
// Hessian, so 4 * n + 4 may equal 4 * (n + 1); fs: 1 Value, 2 Derivatives, 3 Values; 0 if none).
// The construction families print the callback counts (mv: nvalue ngradient nvalues nhessian nstate; fs: nvalue
// nderivatives nvalues nstate; tf: nvalue nderivative nvalues nstate) and a 64-bit FNV-1a hash of the call trace:
// per call a tag byte (mv: 'V' 'G' 'W' 'H' 'S'; fs: 'V' 'D' 'W' 'S'; tf: 'V' 'D' 'W' 'S'), then for the calls with an
// argument the argument: tf the 8 bytes of x (little endian); mv/fs the 4 bytes of X.Lower(), the 4 bytes of
// X.Length() (little endian, two's complement) and the 8 bytes of every element in order.
// A test function checks the dimensions of the vectors and matrices it is given (X of length n, G of length n,
// H n x n, F of length neq, D neq x n); the oracle drops a case in which OCCT passes others (the judge panics).
//
// Cases for which the C++ result depends on uninitialised memory are detected by running every call four times with
// differently, randomly filled stack and heap and are NOT printed; so are cases in which a callback sees or
// produces a non-finite number or runs away (> 300000 calls). Cases of the Kronrod families (they call the C
// library's pow) are only printed when moving every result of pow (and sin, cos, ...) by an ulp, in three
// pseudo-random patterns, leaves the outputs within 3 ulps (the test allows 8).
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <math.hxx>
#include <math_BFGS.hxx>
#include <math_BissecNewton.hxx>
#include <math_Crout.hxx>
#include <math_FRPR.hxx>
#include <math_Function.hxx>
#include <math_FunctionSet.hxx>
#include <math_FunctionSetRoot.hxx>
#include <math_FunctionSetWithDerivatives.hxx>
#include <math_FunctionWithDerivative.hxx>
#include <math_GaussMultipleIntegration.hxx>
#include <math_GaussSetIntegration.hxx>
#include <math_GaussSingleIntegration.hxx>
#include <math_IntegerVector.hxx>
#include <math_KronrodSingleIntegration.hxx>
#include <math_Matrix.hxx>
#include <math_MultipleVarFunction.hxx>
#include <math_MultipleVarFunctionWithGradient.hxx>
#include <math_MultipleVarFunctionWithHessian.hxx>
#include <math_NewtonFunctionSetRoot.hxx>
#include <math_NewtonMinimum.hxx>
#include <math_Powell.hxx>
#include <math_Uzawa.hxx>
#include <math_Vector.hxx>
#include <malloc.h>
#include <unistd.h>
#include <algorithm>
#include <cfloat>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <set>
#include <sstream>
#include <string>
#include <vector>

typedef std::vector<double> Vd;
typedef std::vector<int> Vi;

// ---------------------------------------------------------------------------------
// deterministic random numbers
static uint64_t state = 0x9E3779B97F4A7C15ull;
static double rnd(double lo, double hi) {
  state ^= state << 13; state ^= state >> 7; state ^= state << 17;
  return lo + (hi - lo) * (double)(state >> 11) / 9007199254740992.0;
}
static double rnz(double lo, double hi) { double v = rnd(lo, hi); return rnd(0, 1) < 0.5 ? -v : v; }
static int rint(int lo, int hi) { int v = lo + (int)std::floor(rnd(0, 1) * (hi - lo + 1)); return v > hi ? hi : v; }
static bool coin(double p) { return rnd(0, 1) < p; }
template <typename T, size_t N> static T pick(const T (&a)[N]) { return a[rint(0, (int)N - 1)]; }

// ---------------------------------------------------------------------------------
// the harness: a case is dropped (Discard) when it is not reproducible or not finite
struct Discard {};
static long g_calls = 0;      // callback calls of the current run
static const long MAX_CALLS = 300000;

// ---------------------------------------------------------------------------------
// record buffer
static std::string buf, cur_name, cur_schema;
static size_t mark = 0;
static int ntok = 0;
static bool outphase = false;
static std::vector<std::string> got_words;
static std::set<std::string> seen;
static std::map<std::string, int> fam_lines;
static std::map<std::string, int> fam_dropped;
static std::vector<std::string> fam_order;

static void h(double v) {
  if (!std::isfinite(v)) {
    if (!outphase) { std::fprintf(stderr, "non-finite input in %s: %s\n", cur_name.c_str(), buf.c_str()); std::abort(); }
    throw Discard();
  }
  uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits);
  buf += t; ++ntok;
}
static void hx(uint64_t bits) { char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits); buf += t; ++ntok; }
static void in(long v) { buf += " " + std::to_string(v); ++ntok; }
static void ob(bool v) { in(v ? 1 : 0); }
// dump text of a Dump(): newlines as \n, spaces as _ (one token)
static void os_text(const std::string& s) {
  std::string t;
  for (char c : s) { if (c == '\n') t += "\\n"; else if (c == ' ') t += "_"; else t += c; }
  buf += " " + t; ++ntok;
}

static void word(const char* w, int start, int expect) {
  got_words.push_back(w);
  if (expect >= 0 && ntok - start != expect) {
    std::fprintf(stderr, "word %s printed %d tokens, expected %d in %s\n", w, ntok - start, expect, cur_name.c_str());
    std::abort();
  }
}
static void W_f(double v) { int s = ntok; h(v); word("f", s, 1); }
static void W_i(long v) { int s = ntok; in(v); word("i", s, 1); }

// ---------------------------------------------------------------------------------
// deterministic stand-ins for sin, cos, exp: + - * / and floor only
static double SC[16], CC[16], EC[13];
static double TWO_PI_C, INV_TWO_PI;
static void init_tables() {
  TWO_PI_C = 2.0 * 3.14159265358979323846;
  INV_TWO_PI = 1.0 / TWO_PI_C;
  double f = 1.0;
  for (int j = 0; j < 16; ++j) { SC[j] = (j % 2 ? -1.0 : 1.0) / f; f *= (double)(2 * j + 2) * (double)(2 * j + 3); }
  f = 1.0;
  for (int j = 0; j < 16; ++j) { CC[j] = (j % 2 ? -1.0 : 1.0) / f; f *= (double)(2 * j + 1) * (double)(2 * j + 2); }
  f = 1.0;
  for (int j = 0; j < 13; ++j) { EC[j] = 1.0 / f; f *= (double)(j + 1); }
}
// sinp(x) = r * P(r^2), r = x - floor(x / 2pi + 1/2) * 2pi, P the Taylor series of sin(r)/r through r^30
static double sinp(double x) {
  double k = std::floor(x * INV_TWO_PI + 0.5);
  double r = x - k * TWO_PI_C;
  double r2 = r * r;
  double s = SC[15];
  for (int j = 14; j >= 0; --j) s = s * r2 + SC[j];
  return r * s;
}
// cosp(x) = Q(r^2), the Taylor series of cos(r) through r^30
static double cosp(double x) {
  double k = std::floor(x * INV_TWO_PI + 0.5);
  double r = x - k * TWO_PI_C;
  double r2 = r * r;
  double s = CC[15];
  for (int j = 14; j >= 0; --j) s = s * r2 + CC[j];
  return s;
}
// expp(x) = (T(x / 1024))^1024: T the Taylor polynomial of exp of degree 12, then ten squarings
static double expp(double x) {
  double z = x / 1024.0;
  double e = EC[12];
  for (int j = 11; j >= 0; --j) e = e * z + EC[j];
  for (int j = 0; j < 10; ++j) e = e * e;
  return e;
}

// ---------------------------------------------------------------------------------
// the test functions
enum { K_POLY = 0, K_TRIG = 1, K_RATIONAL = 2, K_MULTIPLE = 3, K_FLAT = 4, K_EXP = 5, K_ABS = 6, K_COUNT = 7 };
struct FnSpec {
  int kind = 0;
  Vd p;
  double flo = 1.0, fhi = 0.0;
  int fmode = 0, smode = 0;
};
// evaluates value and derivative; returns false when the function itself has no value (the pole)
static bool eval_fn(const FnSpec& s, double x, double& v, double& d) {
  const Vd& p = s.p;
  switch (s.kind) {
    case K_POLY: {
      int n = (int)p.size();
      v = p[n - 1];
      for (int i = n - 2; i >= 0; --i) v = v * x + p[i];
      d = (double)(n - 1) * p[n - 1];
      for (int i = n - 2; i >= 1; --i) d = d * x + (double)i * p[i];
      return true;
    }
    case K_TRIG: {
      v = 0.0; d = 0.0;
      for (size_t t = 0; t + 2 < p.size(); t += 3) {
        double arg = p[t + 1] * x + p[t + 2];
        v += p[t] * sinp(arg);
        d += (p[t] * p[t + 1]) * cosp(arg);
      }
      return true;
    }
    case K_RATIONAL: {
      double e = x - p[3];
      if (std::fabs(e) < 1e-100) return false;
      v = p[0] + p[1] / e + p[2] * x;
      d = p[2] - (p[1] / e) / e;
      return true;
    }
    case K_MULTIPLE: {
      int m = (int)p[2];
      double u = x - p[1];
      double pm1 = 1.0;
      for (int i = 0; i < m - 1; ++i) pm1 *= u;
      double pm = pm1 * u;
      v = p[0] * pm * (x - p[3]);
      d = p[0] * ((double)m * pm1 * (x - p[3]) + pm);
      return true;
    }
    case K_FLAT: {
      int pw = (int)p[4];
      double c = p[5];
      if (x < p[0]) {
        double t = p[0] - x;
        double w = pw == 1 ? t : (pw == 2 ? t * t : t * t * t);
        double wd = pw == 1 ? 1.0 : (pw == 2 ? 2.0 * t : 3.0 * (t * t));
        v = c + p[2] * w;
        d = -(p[2] * wd);
      } else if (x > p[1]) {
        double t = x - p[1];
        double w = pw == 1 ? t : (pw == 2 ? t * t : t * t * t);
        double wd = pw == 1 ? 1.0 : (pw == 2 ? 2.0 * t : 3.0 * (t * t));
        v = c + p[3] * w;
        d = p[3] * wd;
      } else {
        v = c;
        d = 0.0;
      }
      return true;
    }
    case K_EXP: {
      double e = expp(p[0] * x);
      v = e - p[1];
      d = p[0] * e;
      return true;
    }
    case K_ABS: {
      double u = x - p[1];
      v = p[0] * std::fabs(u) + p[2];
      d = u > 0.0 ? p[0] : (u < 0.0 ? -p[0] : 0.0);
      return true;
    }
  }
  return false;
}

// FNV-1a over the call trace
static const uint64_t FNV0 = 0xcbf29ce484222325ull, FNVP = 0x100000001b3ull;
class TF : public math_FunctionWithDerivative {
public:
  FnSpec s;
  long nv = 0, nd = 0, nw = 0, ns = 0;
  uint64_t trace = FNV0;
  long calls = 0;
  int last = 0;
  explicit TF(const FnSpec& spec) : s(spec) {}
  void tag(char c) { trace ^= (uint8_t)c; trace *= FNVP; }
  void tagx(char c, double x) {
    tag(c);
    uint64_t b; std::memcpy(&b, &x, 8);
    for (int i = 0; i < 8; ++i) { trace ^= (b >> (8 * i)) & 0xff; trace *= FNVP; }
  }
  bool fails(double x, int bit) const { return (s.fmode & bit) && x >= s.flo && x <= s.fhi; }
  void begin_call(double x) {
    if (!std::isfinite(x)) throw Discard();
    if (++g_calls > MAX_CALLS) throw Discard();
    ++calls;
  }
  bool Value(const double X, double& F) override {
    begin_call(X); ++nv; tagx('V', X); last = 1;
    if (fails(X, 1)) return false;
    double v, d;
    if (!eval_fn(s, X, v, d)) return false;
    if (!std::isfinite(v)) throw Discard();
    F = v;
    return true;
  }
  bool Derivative(const double X, double& D) override {
    begin_call(X); ++nd; tagx('D', X); last = 2;
    if (fails(X, 2)) return false;
    double v, d;
    if (!eval_fn(s, X, v, d)) return false;
    if (!std::isfinite(d)) throw Discard();
    D = d;
    return true;
  }
  bool Values(const double X, double& F, double& D) override {
    begin_call(X); ++nw; tagx('W', X); last = 3;
    if (fails(X, 4)) return false;
    double v, d;
    if (!eval_fn(s, X, v, d)) return false;
    if (!std::isfinite(v) || !std::isfinite(d)) throw Discard();
    F = v; D = d;
    return true;
  }
  int GetStateNumber() override {
    ++ns; tag('S');
    if (s.smode == 0) return 0;
    if (s.smode == 1) return (int)calls;
    return (int)(4 * calls + last);
  }
};
static void out_counts(const TF& f) { in(f.nv); in(f.nd); in(f.nw); in(f.ns); hx(f.trace); }

// ---------------------------------------------------------------------------------
// printing a test function
static void put_tf(const FnSpec& s) {
  in(s.kind); in((long)s.p.size());
  for (double v : s.p) h(v);
  h(s.flo); h(s.fhi); in(s.fmode); in(s.smode);
}
static void W_tf(const FnSpec& s) { int st = ntok; put_tf(s); word("tf", st, 2 + (int)s.p.size() + 4); }

static std::vector<std::string> split_words(const std::string& s) {
  std::vector<std::string> out; size_t pos = 0;
  while (pos < s.size()) {
    size_t e = s.find(' ', pos); if (e == std::string::npos) e = s.size();
    if (e > pos) out.push_back(s.substr(pos, e - pos));
    pos = e + 1;
  }
  return out;
}
static void begin(const std::string& name, const std::string& schema) {
  if (std::getenv("MATHND_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
  buf = name; cur_name = name; cur_schema = schema; ntok = 0; outphase = false; got_words.clear();
}
static void bar() {
  if (got_words != split_words(cur_schema)) {
    std::fprintf(stderr, "schema mismatch in %s: printed words differ from '%s'\n", cur_name.c_str(), cur_schema.c_str());
    std::abort();
  }
  buf += " |"; mark = buf.size(); outphase = true;
}
static void finish() {
  if (!seen.insert(buf).second) { buf.clear(); outphase = false; return; } // exact duplicate: dropped
  if (!fam_lines.count(cur_name)) fam_order.push_back(cur_name);
  fam_lines[cur_name]++;
  buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear(); outphase = false;
}
static void unchecked() { buf.resize(mark); buf += " unchecked"; }

// ---------------------------------------------------------------------------------
// The C library's transcendental functions, interposed so that a case can be re-run with every result moved by
// 0 or 1 ulp (a different libm gives such differences, and tests/mathnd.rs compares the families that use them within a
// tolerance, not bit for bit). With g_perturb off the functions are the C library's.
#include <dlfcn.h>
static bool g_perturb = false;
static uint64_t g_salt = 0;
static double pert(double r, double x, uint64_t k) {
  if (!g_perturb || !std::isfinite(r)) return r;
  uint64_t b; std::memcpy(&b, &x, 8);
  uint64_t z = (b ^ (g_salt * 0x9E3779B97F4A7C15ull) ^ (k * 0xD1B54A32D192ED03ull)) * 0xBF58476D1CE4E5B9ull;
  z ^= z >> 31; z *= 0x94D049BB133111EBull; z ^= z >> 29;
  switch (z & 3) {
    case 0: return std::nextafter(r, HUGE_VAL);
    case 1: return std::nextafter(r, -HUGE_VAL);
    default: return r;
  }
}
#define REAL1(name) static double (*real_##name)(double) = nullptr; if (!real_##name) real_##name = (double (*)(double))dlsym(RTLD_NEXT, #name)
extern "C" {
double sin(double x) noexcept { REAL1(sin); return pert(real_sin(x), x, 1); }
double cos(double x) noexcept { REAL1(cos); return pert(real_cos(x), x, 2); }
double tan(double x) noexcept { REAL1(tan); return pert(real_tan(x), x, 3); }
double asin(double x) noexcept { REAL1(asin); return pert(real_asin(x), x, 4); }
double acos(double x) noexcept { REAL1(acos); return pert(real_acos(x), x, 5); }
double atan(double x) noexcept { REAL1(atan); return pert(real_atan(x), x, 6); }
double atan2(double y, double x) noexcept {
  static double (*real_atan2)(double, double) = nullptr;
  if (!real_atan2) real_atan2 = (double (*)(double, double))dlsym(RTLD_NEXT, "atan2");
  return pert(real_atan2(y, x), y + 3.0 * x, 7);
}
double pow(double x, double y) noexcept {
  static double (*real_pow)(double, double) = nullptr;
  if (!real_pow) real_pow = (double (*)(double, double))dlsym(RTLD_NEXT, "pow");
  return pert(real_pow(x, y), x + 5.0 * y, 8);
}
void sincos(double x, double* s, double* c) noexcept {
  REAL1(sin); static double (*real_cos2)(double) = nullptr; if (!real_cos2) real_cos2 = (double (*)(double))dlsym(RTLD_NEXT, "cos");
  *s = pert(real_sin(x), x, 1); *c = pert(real_cos2(x), x, 2);
}
}

// ---------------------------------------------------------------------------------
// run a call four times with differently filled stack and heap, keep the line when all runs agree
// fills the stack below the caller with pseudo-random words (different for every `k`), so that a variable that is
// read without being written gets a different value in every run (a constant fill byte would give two equal
// "uninitialised" doubles, whose product or difference looks deterministic)
__attribute__((noinline)) static void poison_stack(int k) {
  volatile uint64_t a[8192];
  uint64_t z = 0x9E3779B97F4A7C15ull * (uint64_t)(k + 1);
  for (size_t i = 0; i < 8192; ++i) {
    z += 0x9E3779B97F4A7C15ull;
    uint64_t r = z;
    r = (r ^ (r >> 30)) * 0xBF58476D1CE4E5B9ull;
    r = (r ^ (r >> 27)) * 0x94D049BB133111EBull;
    a[i] = r ^ (r >> 31);
  }
  asm volatile("" ::: "memory");
}
static const int POISON_RUNS = 4;

static std::string run_once(const std::function<void()>& body, int poison, bool& discard) {
  std::string left = buf.substr(0, mark);
  g_calls = 0;
  mallopt(M_PERTURB, (0x37 + 0x4B * poison) & 0xff);
  poison_stack(poison);
  discard = false;
  try {
    body();
  } catch (Standard_Failure&) {
    buf.resize(mark); buf += " throws";
  } catch (Discard&) {
    discard = true;
  }
  std::string res = buf;
  buf = left;
  return res;
}
static bool is_float_token(const std::string& t) {
  if (t.size() != 16) return false;
  for (char c : t) if (!std::isxdigit((unsigned char)c)) return false;
  return true;
}
static double tok_double(const std::string& t) { uint64_t b = std::strtoull(t.c_str(), nullptr, 16); double d; std::memcpy(&d, &b, 8); return d; }
// the outputs of two lines agree within `ulps` units of eps times the largest absolute value (at least 1) of the
// line, like the test's comparison; every other token must be equal
static bool lines_close(const std::string& x, const std::string& y, double ulps) {
  size_t px = x.find(" |"), py = y.find(" |");
  if (px == std::string::npos || py == std::string::npos || x.substr(0, px) != y.substr(0, py)) return false;
  std::vector<std::string> a = split_words(x.substr(px + 2)), b = split_words(y.substr(py + 2));
  if (a.size() != b.size()) return false;
  double scale = 1.0;
  for (size_t i = 0; i < a.size(); ++i) if (is_float_token(a[i])) scale = std::max(scale, std::fabs(tok_double(a[i])));
  for (size_t i = 0; i < a.size(); ++i) {
    if (is_float_token(a[i]) != is_float_token(b[i])) return false;
    if (is_float_token(a[i])) { if (std::fabs(tok_double(a[i]) - tok_double(b[i])) > ulps * DBL_EPSILON * scale) return false; }
    else if (a[i] != b[i]) return false;
  }
  return true;
}
// returns true when a line was produced; `body` prints the outputs (after bar()) or calls unchecked(). With
// `libm_robust` the line is only kept when moving every sin / cos / ... result of the C library by an ulp (two
// different pseudo-random patterns) leaves the outputs within 3 ulps (the test allows 8)
static bool emit(const std::function<void()>& body, bool libm_robust = false) {
  size_t keep_ntok = (size_t)ntok;
  std::string r1;
  for (int k = 0; k < POISON_RUNS; ++k) {
    bool dk = false;
    std::string rk = run_once(body, k, dk);
    ntok = (int)keep_ntok;
    if (dk) { fam_dropped[cur_name]++; buf.clear(); return false; }
    if (k == 0) r1 = rk;
    else if (rk != r1) {
      if (std::getenv("MATHND_DEBUG")) std::fprintf(stderr, "UNINIT\n%s\n%s\n", r1.c_str(), rk.c_str());
      fam_dropped[cur_name]++; fam_dropped[cur_name + "!uninit"]++; buf.clear(); return false;
    }
  }
  if (libm_robust) {
    for (uint64_t salt = 1; salt <= 3; ++salt) {
      g_perturb = true; g_salt = salt;
      bool dp = false;
      std::string rp = run_once(body, 0, dp);
      g_perturb = false;
      ntok = (int)keep_ntok;
      if (!dp && rp != r1) fam_dropped[cur_name + "~changed"]++;
      bool ok = !dp && lines_close(r1, rp, 3.0);
      if (!ok) { fam_dropped[cur_name]++; fam_dropped[cur_name + "!libm"]++; buf.clear(); return false; }
    }
  }
  buf = r1;
  finish();
  return true;
}

// ---------------------------------------------------------------------------------
// generating test functions
static Vd poly_from_roots(const Vd& roots, double lead) {
  Vd c(1, lead);
  for (double r : roots) {
    Vd n(c.size() + 1, 0.0);
    for (size_t k = 0; k < c.size(); ++k) { n[k + 1] += c[k]; n[k] -= r * c[k]; }
    c = n;
  }
  return c;
}
static FnSpec mk(int kind, const Vd& p, int smode) {
  FnSpec s; s.kind = kind; s.p = p; s.smode = smode;
  for (double v : p) if (!std::isfinite(v)) { s.kind = K_POLY; s.p = Vd{-1.0, 1.0}; } // a generator overflow: use a simple function
  return s;
}
static void add_fail(FnSpec& s, double lo, double hi, double prob) {
  if (!coin(prob)) return;
  double w = (hi - lo) * rnd(0.01, 0.25);
  double u = rnd(lo, hi - w);
  s.flo = u; s.fhi = u + w; s.fmode = rint(1, 7);
}
static void add_fail_exact(FnSpec& s, double lo, double hi, int mode) { s.flo = lo; s.fhi = hi; s.fmode = mode; }

// multiply the function by a huge or tiny factor now and then (values up to 1e200, finite)
static FnSpec scaled(FnSpec s) {
  if (!coin(0.06)) return s;
  double f = std::pow(10.0, coin(0.6) ? rnd(40, 150) : rnd(-150, -40));
  switch (s.kind) {
    case K_POLY: for (double& c : s.p) c *= f; break;
    case K_TRIG: for (size_t t = 0; t < s.p.size(); t += 3) s.p[t] *= f; break;
    case K_RATIONAL: s.p[0] *= f; s.p[1] *= f; s.p[2] *= f; break;
    case K_MULTIPLE: s.p[0] *= f; break;
    case K_FLAT: s.p[2] *= f; s.p[3] *= f; s.p[5] *= f; break;
    case K_ABS: s.p[0] *= f; s.p[2] *= f; break;
    default: break;
  }
  return s;
}
// a function that has roots inside [lo, hi]
static FnSpec fn_rootful0(double lo, double hi, int smode) {
  double span = hi - lo;
  double r = rnd(0, 1);
  if (r < 0.34) {
    int nroots = rint(1, 4);
    Vd roots; int deg = 0;
    while ((int)roots.size() < nroots && deg < 8) {
      double rt;
      double q = rnd(0, 1);
      if (q < 0.08) rt = lo; else if (q < 0.16) rt = hi; else rt = rnd(lo, hi);
      int mult = coin(0.7) ? 1 : rint(2, 3);
      if (deg + mult > 8) mult = 1;
      for (int m = 0; m < mult; ++m) roots.push_back(rt);
      deg += mult;
      if (coin(0.15) && deg + 1 <= 8) { roots.push_back(rt + rnz(1e-4, 1e-2) * (span > 0 ? span : 1.0)); ++deg; } // close pair
    }
    if (coin(0.3)) for (int e = 0; e < 2 && (int)roots.size() < 8; ++e) roots.push_back(rnz(hi + 0.5, hi + 6.0)); // outside
    return mk(K_POLY, poly_from_roots(roots, rnz(0.2, 3.0)), smode);
  } else if (r < 0.40) {
    int deg = rint(1, 8); Vd c;
    for (int i = 0; i <= deg; ++i) c.push_back(rnd(-3, 3));
    if (c.back() == 0.0) c.back() = 1.0;
    return mk(K_POLY, c, smode);
  } else if (r < 0.54) {
    int nt = rint(1, 3); Vd p;
    for (int t = 0; t < nt; ++t) {
      p.push_back(rnz(0.2, 3.0));
      p.push_back(coin(0.4) ? (double)rint(1, 5) : rnd(0.3, 6.0));
      p.push_back(rnd(0, 6.2831853));
    }
    return mk(K_TRIG, p, smode);
  } else if (r < 0.62) {
    double pole = coin(0.7) ? rnd(lo, hi) : rnz(hi + 0.2, hi + 3.0);
    if (coin(0.1)) pole = lo;
    return mk(K_RATIONAL, Vd{rnd(-2, 2), rnz(0.1, 2.0), rnd(-1, 1), pole}, smode);
  } else if (r < 0.72) {
    return mk(K_MULTIPLE, Vd{rnz(0.2, 3.0), rnd(lo, hi), (double)rint(2, 4), coin(0.5) ? rnd(lo - 2, hi + 2) : rnz(hi + 1, hi + 5)}, smode);
  } else if (r < 0.82) {
    double a = rnd(lo, hi), b = rnd(lo, hi);
    if (a > b) std::swap(a, b);
    if (coin(0.1)) b = a;
    return mk(K_FLAT, Vd{a, b, rnz(0.2, 3.0), rnz(0.2, 3.0), (double)rint(1, 3), coin(0.75) ? 0.0 : rnd(-1, 1)}, smode);
  } else if (r < 0.92) {
    double a = rnz(0.2, 2.0);
    double x0 = rnd(lo, hi);
    double b = expp(a * x0);
    if (coin(0.2)) b *= rnd(0.5, 1.5);
    return mk(K_EXP, Vd{a, b}, smode);
  } else {
    return mk(K_ABS, Vd{rnz(0.3, 3.0), rnd(lo, hi), rnd(-1, 0.3)}, smode);
  }
}
// a function with a minimum inside [lo, hi] (or none at all: a monotone one)
static FnSpec fn_minful0(double lo, double hi, int smode) {
  double r = rnd(0, 1);
  double m = rnd(lo, hi);
  if (r < 0.28) {
    double a = rnd(0.2, 3.0);
    return mk(K_POLY, Vd{a * m * m + rnd(-1, 1), -2 * a * m, a}, smode);
  } else if (r < 0.40) {
    double m2 = rnd(lo, hi), a = rnd(0.1, 2.0);
    Vd c = poly_from_roots(Vd{m, m, m2, m2}, a);
    c[0] += rnd(-1, 1);
    return mk(K_POLY, c, smode);
  } else if (r < 0.46) {
    int deg = rint(1, 8); Vd c;
    for (int i = 0; i <= deg; ++i) c.push_back(rnd(-3, 3));
    if (c.back() == 0.0) c.back() = 1.0;
    return mk(K_POLY, c, smode);
  } else if (r < 0.62) {
    int nt = rint(1, 3); Vd p;
    for (int t = 0; t < nt; ++t) { p.push_back(rnz(0.2, 3.0)); p.push_back(coin(0.4) ? (double)rint(1, 5) : rnd(0.3, 6.0)); p.push_back(rnd(0, 6.2831853)); }
    return mk(K_TRIG, p, smode);
  } else if (r < 0.74) {
    double a = rnd(lo, hi), b = rnd(lo, hi);
    if (a > b) std::swap(a, b);
    if (coin(0.15)) b = a;
    return mk(K_FLAT, Vd{a, b, rnd(0.2, 3.0), rnd(0.2, 3.0), (double)rint(1, 3), rnd(-1, 1)}, smode);
  } else if (r < 0.84) {
    return mk(K_ABS, Vd{rnd(0.3, 3.0), m, rnd(-1, 1)}, smode);
  } else if (r < 0.92) {
    return mk(K_EXP, Vd{rnz(0.1, 1.2), rnd(0.1, 5.0)}, smode);
  } else {
    return mk(K_MULTIPLE, Vd{rnz(0.2, 3.0), rnd(lo, hi), (double)rint(2, 4), rnz(lo - 3, hi + 3)}, smode);
  }
}

static FnSpec fn_rootful(double lo, double hi, int smode) { return scaled(fn_rootful0(lo, hi, smode)); }
static FnSpec fn_minful(double lo, double hi, int smode) { return scaled(fn_minful0(lo, hi, smode)); }
// a root of the function in [lo, hi]: a bracketing sample pair (x0, x1) of 256 samples, and the bisected root r
static bool find_root(const FnSpec& s, double lo, double hi, double& x0, double& x1, double& r) {
  const int NS = 256;
  std::vector<std::pair<double, double>> pairs;
  double px = lo, pv = 0, dd;
  bool have = false;
  for (int i = 0; i <= NS; ++i) {
    double x = lo + (hi - lo) * i / NS, v;
    if (!eval_fn(s, x, v, dd) || !std::isfinite(v)) { have = false; continue; }
    if (have && ((pv < 0 && v > 0) || (pv > 0 && v < 0))) pairs.push_back({px, x});
    px = x; pv = v; have = true;
  }
  if (pairs.empty()) return false;
  std::pair<double, double> pr = pairs[rint(0, (int)pairs.size() - 1)];
  x0 = pr.first; x1 = pr.second;
  double a = x0, b = x1, fa, fb;
  eval_fn(s, a, fa, dd); eval_fn(s, b, fb, dd);
  for (int i = 0; i < 70; ++i) {
    double m = 0.5 * (a + b), fm;
    eval_fn(s, m, fm, dd);
    if ((fm < 0) == (fa < 0)) { a = m; fa = fm; } else { b = m; fb = fm; }
  }
  r = 0.5 * (a + b);
  return true;
}

// ---------------------------------------------------------------------------------
// common random picks
static const double TOLS[] = {0.0, 1e-300, 1e-15, 1e-14, 1e-12, 1e-10, 1e-8, 1e-6, 1e-4, 1e-2, 0.1, 0.5, 1.0, 10.0};
static double rtol() { return coin(0.5) ? pick(TOLS) : std::pow(10.0, rnd(-14, -1)); }
static void rinterval(double& a, double& b) {
  double q = rnd(0, 1);
  if (q < 0.50) { a = rnd(-4, 0); b = a + rnd(0.5, 8); }
  else if (q < 0.64) { a = rnd(-4, 0); b = a + rnd(0.5, 8); std::swap(a, b); }
  else if (q < 0.69) { a = rnd(-3, 3); b = a; }
  else if (q < 0.75) { a = rnd(-1, 1); b = a + rnd(1e-7, 1e-4); }
  else if (q < 0.82) { a = 0; b = 1; }
  else if (q < 0.88) { a = -1; b = 1; }
  else if (q < 0.94) { a = 0; b = 6.283185307179586; }
  else if (q < 0.97) { a = rnz(50, 1e4); b = a + rnd(1, 1e3); }
  else { a = rnz(1e5, 1e8); b = a + rnz(1e3, 1e8); }
}
static int rstate() { return rint(0, 2); }
// a count used only to pick indices: any failure of the call gives 0
// an index into a collection of n elements: mostly valid, sometimes out of range
static void lohi(double a, double b, double& lo, double& hi) { lo = std::min(a, b); hi = std::max(a, b); if (hi - lo < 1e-3) hi = lo + 1e-3; }

static double pickl(std::initializer_list<double> l) { int k = rint(0, (int)l.size() - 1); return *(l.begin() + k); }
static int pickli(std::initializer_list<int> l) { int k = rint(0, (int)l.size() - 1); return *(l.begin() + k); }

// =================================================================================
// THE SCALAR TERMS and the functions of several variables
// =================================================================================
enum { G_QUAD = 0, G_ROSEN = 1, G_POLY = 2, G_TRIG = 3, G_KINK = 4, G_EXP = 5 };
struct GSpec { int kind = G_QUAD; Vd p; };
struct NSpec {
  int n = 1;                 // variables
  int mode = 0;              // mv only: 0 sum, 1 sum of squares
  std::vector<GSpec> g;      // terms (mv) or equations (fs)
  int fmode = 0;
  Vd lo, hi;                 // the failure box
  int smode = 0;
};

static double pw(double x, int e) { double r = 1.0; for (int i = 0; i < e; ++i) r *= x; return r; }

static double g_value(const GSpec& g, int m, const double* x) {
  const Vd& p = g.p;
  switch (g.kind) {
    case G_QUAD: {
      double v = p[0];
      if ((int)p.size() == 1 + m) {   // no matrix: linear
        for (int i = 0; i < m; ++i) v += x[i] * p[1 + i];
        return v;
      }
      for (int i = 0; i < m; ++i) {
        double r = 0.0;
        for (int j = 0; j < m; ++j) r += p[1 + m + i * m + j] * x[j];
        v += x[i] * (p[1 + i] + 0.5 * r);
      }
      return v;
    }
    case G_ROSEN: {
      double a = p[0], b = p[1];
      if (m == 1) { double u = a - x[0]; return u * u; }
      double v = 0.0;
      for (int i = 0; i + 1 < m; ++i) {
        double u = x[i + 1] - x[i] * x[i];
        double w = a - x[i];
        v += b * (u * u) + w * w;
      }
      return v;
    }
    case G_POLY: {
      int nt = (int)p[0];
      double v = 0.0;
      for (int t = 0; t < nt; ++t) {
        const double* q = &p[1 + t * (1 + m)];
        double term = q[0];
        for (int k = 0; k < m; ++k) term *= pw(x[k], (int)q[1 + k]);
        v += term;
      }
      return v;
    }
    case G_TRIG: {
      double v = p[0];
      for (int j = 0; j < m; ++j) {
        const double* q = &p[1 + 4 * j];
        double arg = q[1] * x[j] + q[2];
        v += q[0] * sinp(arg) + q[3] * (x[j] * x[j]);
      }
      return v;
    }
    case G_KINK: {
      double v = p[0];
      for (int j = 0; j < m; ++j) v += p[1 + 2 * j] * std::fabs(x[j] - p[2 + 2 * j]);
      return v;
    }
    case G_EXP: {
      double s = 0.0;
      for (int j = 0; j < m; ++j) s += p[2 + j] * x[j];
      return p[0] + p[1] * expp(s);
    }
  }
  std::abort();
}
// one term of a polynomial differentiated d_k times in x_k (d_k = 0, 1, 2); false when the term vanishes
static bool poly_term(const double* q, int m, const double* x, const int* d, double& out) {
  double term = q[0];
  for (int k = 0; k < m; ++k) {
    int e = (int)q[1 + k];
    if (e < d[k]) return false;
    if (d[k] == 1) term *= (double)e;
    else if (d[k] == 2) term *= (double)(e * (e - 1));
    term *= pw(x[k], e - d[k]);
  }
  out = term;
  return true;
}
static void g_grad(const GSpec& g, int m, const double* x, double* gr) {
  const Vd& p = g.p;
  switch (g.kind) {
    case G_QUAD:
      if ((int)p.size() == 1 + m) { for (int i = 0; i < m; ++i) gr[i] = p[1 + i]; return; }
      for (int i = 0; i < m; ++i) {
        double r = 0.0;
        for (int j = 0; j < m; ++j) r += (p[1 + m + i * m + j] + p[1 + m + j * m + i]) * x[j];
        gr[i] = p[1 + i] + 0.5 * r;
      }
      return;
    case G_ROSEN: {
      double a = p[0], b = p[1];
      if (m == 1) { gr[0] = -2.0 * (a - x[0]); return; }
      for (int i = 0; i < m; ++i) gr[i] = 0.0;
      for (int i = 0; i + 1 < m; ++i) {
        double u = x[i + 1] - x[i] * x[i];
        double w = a - x[i];
        gr[i] += -4.0 * b * u * x[i] - 2.0 * w;
        gr[i + 1] += 2.0 * b * u;
      }
      return;
    }
    case G_POLY: {
      int nt = (int)p[0];
      int d[8];
      for (int j = 0; j < m; ++j) {
        for (int k = 0; k < m; ++k) d[k] = k == j ? 1 : 0;
        double s = 0.0, t;
        for (int u = 0; u < nt; ++u) if (poly_term(&p[1 + u * (1 + m)], m, x, d, t)) s += t;
        gr[j] = s;
      }
      return;
    }
    case G_TRIG:
      for (int j = 0; j < m; ++j) {
        const double* q = &p[1 + 4 * j];
        double arg = q[1] * x[j] + q[2];
        gr[j] = (q[0] * q[1]) * cosp(arg) + 2.0 * q[3] * x[j];
      }
      return;
    case G_KINK:
      for (int j = 0; j < m; ++j) {
        double u = x[j] - p[2 + 2 * j], a = p[1 + 2 * j];
        gr[j] = u > 0.0 ? a : (u < 0.0 ? -a : 0.0);
      }
      return;
    case G_EXP: {
      double s = 0.0;
      for (int j = 0; j < m; ++j) s += p[2 + j] * x[j];
      double e = expp(s);
      for (int j = 0; j < m; ++j) gr[j] = (p[1] * p[2 + j]) * e;
      return;
    }
  }
  std::abort();
}
// the Hessian, row major m x m
static void g_hess(const GSpec& g, int m, const double* x, double* hs) {
  const Vd& p = g.p;
  for (int i = 0; i < m * m; ++i) hs[i] = 0.0;
  switch (g.kind) {
    case G_QUAD:
      if ((int)p.size() == 1 + m) return;
      for (int i = 0; i < m; ++i)
        for (int j = 0; j < m; ++j) hs[i * m + j] = 0.5 * (p[1 + m + i * m + j] + p[1 + m + j * m + i]);
      return;
    case G_ROSEN: {
      double b = p[1];
      if (m == 1) { hs[0] = 2.0; return; }
      for (int i = 0; i + 1 < m; ++i) {
        double u = x[i + 1] - x[i] * x[i];
        hs[i * m + i] += 8.0 * b * (x[i] * x[i]) - 4.0 * b * u + 2.0;
        hs[i * m + i + 1] += -4.0 * b * x[i];
        hs[(i + 1) * m + i] += -4.0 * b * x[i];
        hs[(i + 1) * m + i + 1] += 2.0 * b;
      }
      return;
    }
    case G_POLY: {
      int nt = (int)p[0];
      int d[8];
      for (int i = 0; i < m; ++i)
        for (int j = 0; j < m; ++j) {
          for (int k = 0; k < m; ++k) d[k] = 0;
          d[i] += 1; d[j] += 1;
          double s = 0.0, t;
          for (int u = 0; u < nt; ++u) if (poly_term(&p[1 + u * (1 + m)], m, x, d, t)) s += t;
          hs[i * m + j] = s;
        }
      return;
    }
    case G_TRIG:
      for (int j = 0; j < m; ++j) {
        const double* q = &p[1 + 4 * j];
        double arg = q[1] * x[j] + q[2];
        hs[j * m + j] = -((q[0] * q[1]) * q[1]) * sinp(arg) + 2.0 * q[3];
      }
      return;
    case G_KINK: return;
    case G_EXP: {
      double s = 0.0;
      for (int j = 0; j < m; ++j) s += p[2 + j] * x[j];
      double e = expp(s);
      for (int i = 0; i < m; ++i)
        for (int j = 0; j < m; ++j) hs[i * m + j] = ((p[1] * p[2 + i]) * p[2 + j]) * e;
      return;
    }
  }
  std::abort();
}

// the mv function
static double mv_value(const NSpec& s, const double* x) {
  double f = 0.0;
  for (const GSpec& g : s.g) { double v = g_value(g, s.n, x); f += s.mode == 1 ? v * v : v; }
  return f;
}
static void mv_grad(const NSpec& s, const double* x, double* G) {
  int n = s.n;
  double gr[8];
  for (int j = 0; j < n; ++j) G[j] = 0.0;
  for (const GSpec& g : s.g) {
    g_grad(g, n, x, gr);
    if (s.mode == 1) { double v = g_value(g, n, x); for (int j = 0; j < n; ++j) G[j] += (2.0 * v) * gr[j]; }
    else for (int j = 0; j < n; ++j) G[j] += gr[j];
  }
}
static void mv_hess(const NSpec& s, const double* x, double* H) {
  int n = s.n;
  double gr[8], hs[64];
  for (int j = 0; j < n * n; ++j) H[j] = 0.0;
  for (const GSpec& g : s.g) {
    g_hess(g, n, x, hs);
    if (s.mode == 1) {
      double v = g_value(g, n, x);
      g_grad(g, n, x, gr);
      for (int i = 0; i < n; ++i)
        for (int j = 0; j < n; ++j) H[i * n + j] += 2.0 * (gr[i] * gr[j] + v * hs[i * n + j]);
    } else {
      for (int j = 0; j < n * n; ++j) H[j] += hs[j];
    }
  }
}

// shared bookkeeping of the vector test functions
struct Tracer {
  uint64_t trace = FNV0;
  long calls = 0;
  int last = 0;
  void tag(char c) { trace ^= (uint8_t)c; trace *= FNVP; }
  void byte(uint8_t b) { trace ^= b; trace *= FNVP; }
  void tagv(char c, const math_Vector& X) {
    tag(c);
    uint32_t lo = (uint32_t)X.Lower(), ln = (uint32_t)X.Length();
    for (int i = 0; i < 4; ++i) byte((lo >> (8 * i)) & 0xff);
    for (int i = 0; i < 4; ++i) byte((ln >> (8 * i)) & 0xff);
    for (int i = X.Lower(); i <= X.Upper(); ++i) {
      double v = X(i); uint64_t b; std::memcpy(&b, &v, 8);
      for (int k = 0; k < 8; ++k) byte((b >> (8 * k)) & 0xff);
    }
  }
};
static bool in_box(const NSpec& s, const double* x) {
  for (int j = 0; j < s.n; ++j) if (!(x[j] >= s.lo[j] && x[j] <= s.hi[j])) return false;
  return true;
}
// X as a C array (Discard when the solver passes a wrong length or a non-finite element)
static void get_x(const NSpec& s, const math_Vector& X, double* x) {
  if (X.Length() != s.n) throw Discard();
  for (int j = 0; j < s.n; ++j) { x[j] = X(X.Lower() + j); if (!std::isfinite(x[j])) throw Discard(); }
}
static void fin(double v) { if (!std::isfinite(v)) throw Discard(); }

class MVF : public math_MultipleVarFunctionWithHessian {
public:
  NSpec s;
  long nv = 0, ng = 0, nw = 0, nh = 0, ns = 0;
  Tracer t;
  explicit MVF(const NSpec& spec) : s(spec) {}
  int NbVariables() const override { return s.n; }
  void begin_call(char tag, const math_Vector& X, double* x, int code) {
    t.tagv(tag, X); t.last = code;
    if (++g_calls > MAX_CALLS) throw Discard();
    ++t.calls;
    get_x(s, X, x);
  }
  bool fails(const double* x, int bit) const { return (s.fmode & bit) && in_box(s, x); }
  bool Value(const math_Vector& X, double& F) override {
    double x[8]; ++nv; begin_call('V', X, x, 1);
    if (fails(x, 1)) return false;
    double v = mv_value(s, x); fin(v);
    F = v;
    return true;
  }
  bool Gradient(const math_Vector& X, math_Vector& G) override {
    double x[8]; ++ng; begin_call('G', X, x, 2);
    if (G.Length() != s.n) throw Discard();
    if (fails(x, 2)) return false;
    double gr[8]; mv_grad(s, x, gr);
    for (int j = 0; j < s.n; ++j) fin(gr[j]);
    for (int j = 0; j < s.n; ++j) G(G.Lower() + j) = gr[j];
    return true;
  }
  bool Values(const math_Vector& X, double& F, math_Vector& G) override {
    double x[8]; ++nw; begin_call('W', X, x, 3);
    if (G.Length() != s.n) throw Discard();
    if (fails(x, 4)) return false;
    double v = mv_value(s, x), gr[8]; mv_grad(s, x, gr);
    fin(v); for (int j = 0; j < s.n; ++j) fin(gr[j]);
    F = v;
    for (int j = 0; j < s.n; ++j) G(G.Lower() + j) = gr[j];
    return true;
  }
  bool Values(const math_Vector& X, double& F, math_Vector& G, math_Matrix& H) override {
    double x[8]; ++nh; begin_call('H', X, x, 4);
    if (G.Length() != s.n || H.RowNumber() != s.n || H.ColNumber() != s.n) throw Discard();
    if (fails(x, 8)) return false;
    double v = mv_value(s, x), gr[8], hs[64]; mv_grad(s, x, gr); mv_hess(s, x, hs);
    fin(v); for (int j = 0; j < s.n; ++j) fin(gr[j]); for (int j = 0; j < s.n * s.n; ++j) fin(hs[j]);
    F = v;
    for (int j = 0; j < s.n; ++j) G(G.Lower() + j) = gr[j];
    for (int i = 0; i < s.n; ++i)
      for (int j = 0; j < s.n; ++j) H(H.LowerRow() + i, H.LowerCol() + j) = hs[i * s.n + j];
    return true;
  }
  int GetStateNumber() override {
    ++ns; t.tag('S');
    if (s.smode == 0) return 0;
    if (s.smode == 1) return (int)t.calls;
    return (int)(4 * t.calls + t.last);
  }
};

class FSF : public math_FunctionSetWithDerivatives {
public:
  NSpec s;
  long nv = 0, nd = 0, nw = 0, ns = 0;
  Tracer t;
  explicit FSF(const NSpec& spec) : s(spec) {}
  int NbVariables() const override { return s.n; }
  int NbEquations() const override { return (int)s.g.size(); }
  void begin_call(char tag, const math_Vector& X, double* x, int code) {
    t.tagv(tag, X); t.last = code;
    if (++g_calls > MAX_CALLS) throw Discard();
    ++t.calls;
    get_x(s, X, x);
  }
  bool fails(const double* x, int bit) const { return (s.fmode & bit) && in_box(s, x); }
  bool Value(const math_Vector& X, math_Vector& F) override {
    double x[8]; ++nv; begin_call('V', X, x, 1);
    int ne = (int)s.g.size();
    if (F.Length() != ne) throw Discard();
    if (fails(x, 1)) return false;
    double fv[8];
    for (int k = 0; k < ne; ++k) { fv[k] = g_value(s.g[k], s.n, x); fin(fv[k]); }
    for (int k = 0; k < ne; ++k) F(F.Lower() + k) = fv[k];
    return true;
  }
  bool Derivatives(const math_Vector& X, math_Matrix& D) override {
    double x[8]; ++nd; begin_call('D', X, x, 2);
    int ne = (int)s.g.size();
    if (D.RowNumber() != ne || D.ColNumber() != s.n) throw Discard();
    if (fails(x, 2)) return false;
    double dv[64];
    for (int k = 0; k < ne; ++k) g_grad(s.g[k], s.n, x, &dv[k * s.n]);
    for (int k = 0; k < ne * s.n; ++k) fin(dv[k]);
    for (int k = 0; k < ne; ++k)
      for (int j = 0; j < s.n; ++j) D(D.LowerRow() + k, D.LowerCol() + j) = dv[k * s.n + j];
    return true;
  }
  bool Values(const math_Vector& X, math_Vector& F, math_Matrix& D) override {
    double x[8]; ++nw; begin_call('W', X, x, 3);
    int ne = (int)s.g.size();
    if (F.Length() != ne || D.RowNumber() != ne || D.ColNumber() != s.n) throw Discard();
    if (fails(x, 4)) return false;
    double fv[8], dv[64];
    for (int k = 0; k < ne; ++k) { fv[k] = g_value(s.g[k], s.n, x); fin(fv[k]); }
    for (int k = 0; k < ne; ++k) g_grad(s.g[k], s.n, x, &dv[k * s.n]);
    for (int k = 0; k < ne * s.n; ++k) fin(dv[k]);
    for (int k = 0; k < ne; ++k) F(F.Lower() + k) = fv[k];
    for (int k = 0; k < ne; ++k)
      for (int j = 0; j < s.n; ++j) D(D.LowerRow() + k, D.LowerCol() + j) = dv[k * s.n + j];
    return true;
  }
  int GetStateNumber() override {
    ++ns; t.tag('S');
    if (s.smode == 0) return 0;
    if (s.smode == 1) return (int)t.calls;
    return (int)(4 * t.calls + t.last);
  }
};
static void out_mv(const MVF& f) { in(f.nv); in(f.ng); in(f.nw); in(f.nh); in(f.ns); hx(f.t.trace); }
static void out_fs(const FSF& f) { in(f.nv); in(f.nd); in(f.nw); in(f.ns); hx(f.t.trace); }

// ---------------------------------------------------------------------------------
// printing inputs and outputs
static int g_np(const GSpec& g) { return (int)g.p.size(); }
static void put_g(const GSpec& g) { in(g.kind); in(g_np(g)); for (double v : g.p) h(v); }
static void put_box(const NSpec& s) { in(s.fmode); for (double v : s.lo) h(v); for (double v : s.hi) h(v); in(s.smode); }
static int ntok_g(const NSpec& s) { int k = 0; for (const GSpec& g : s.g) k += 2 + g_np(g); return k; }
static void W_mv(const NSpec& s) {
  int st = ntok;
  in(s.n); in(s.mode); in((long)s.g.size());
  for (const GSpec& g : s.g) put_g(g);
  put_box(s);
  word("mv", st, 3 + ntok_g(s) + 2 + 2 * s.n);
}
static void W_fs(const NSpec& s) {
  int st = ntok;
  in(s.n); in((long)s.g.size());
  for (const GSpec& g : s.g) put_g(g);
  put_box(s);
  word("fs", st, 2 + ntok_g(s) + 2 + 2 * s.n);
}
struct VecIn { int lower = 1; Vd v; };
static VecIn vin(const Vd& v, int lower = 1) { VecIn r; r.lower = lower; r.v = v; return r; }
static void W_v(const VecIn& v) { int st = ntok; in(v.lower); in((long)v.v.size()); for (double x : v.v) h(x); word("v", st, 2 + (int)v.v.size()); }
static void W_iv(int lower, const Vi& v) { int st = ntok; in(lower); in((long)v.size()); for (int x : v) in(x); word("iv", st, 2 + (int)v.size()); }
struct MatIn { int lr = 1, lc = 1, rows = 1, cols = 1; Vd a; };
static void W_m(const MatIn& m) {
  int st = ntok;
  in(m.lr); in(m.rows); in(m.lc); in(m.cols);
  for (double x : m.a) h(x);
  word("m", st, 4 + m.rows * m.cols);
}
static math_Vector mkv(const VecIn& v) {
  math_Vector r(v.lower, v.lower + (int)v.v.size() - 1);
  for (size_t i = 0; i < v.v.size(); ++i) r(v.lower + (int)i) = v.v[i];
  return r;
}
static math_IntegerVector mkiv(int lower, const Vi& v) {
  math_IntegerVector r(lower, lower + (int)v.size() - 1);
  for (size_t i = 0; i < v.size(); ++i) r(lower + (int)i) = v[i];
  return r;
}
static math_Matrix mkm(const MatIn& m) {
  math_Matrix r(m.lr, m.lr + m.rows - 1, m.lc, m.lc + m.cols - 1);
  for (int i = 0; i < m.rows; ++i)
    for (int j = 0; j < m.cols; ++j) r(m.lr + i, m.lc + j) = m.a[i * m.cols + j];
  return r;
}
static void ov(const math_Vector& v) { in(v.Lower()); in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) h(v(i)); }
static void om(const math_Matrix& m) {
  in(m.LowerRow()); in(m.RowNumber()); in(m.LowerCol()); in(m.ColNumber());
  for (int i = m.LowerRow(); i <= m.UpperRow(); ++i)
    for (int j = m.LowerCol(); j <= m.UpperCol(); ++j) h(m(i, j));
}
static std::string dump_of(const std::function<void(std::ostream&)>& d) { std::ostringstream os; d(os); return os.str(); }


// ---------------------------------------------------------------------------------
// generating functions of several variables
static int rdim() {
  double q = rnd(0, 1);
  return q < 0.25 ? 1 : q < 0.60 ? 2 : q < 0.80 ? 3 : q < 0.88 ? 4 : q < 0.94 ? 5 : 6;
}
static Vd rvec(int n, double lo, double hi) { Vd v(n); for (double& x : v) x = rnd(lo, hi); return v; }
// a symmetric matrix M M^T + shift I (positive definite for shift > 0, semidefinite of rank r for shift = 0)
static Vd rspd(int m, double shift, int rank) {
  Vd M(m * m, 0.0), A(m * m, 0.0);
  for (int i = 0; i < m; ++i) for (int k = 0; k < rank; ++k) M[i * m + k] = rnd(-1.5, 1.5);
  for (int i = 0; i < m; ++i)
    for (int j = 0; j < m; ++j) {
      double s = i == j ? shift : 0.0;
      for (int k = 0; k < m; ++k) s += M[i * m + k] * M[j * m + k];
      A[i * m + j] = s;
    }
  return A;
}
static GSpec mkg(int kind, const Vd& p) { GSpec g; g.kind = kind; g.p = p; return g; }
// a quadratic c + b.x + 0.5 x^T A x with the stationary point at `ctr` (A symmetric)
static GSpec g_quad_at(int m, const Vd& A, const Vd& ctr, double c) {
  Vd p; p.push_back(c);
  for (int i = 0; i < m; ++i) { double s = 0.0; for (int j = 0; j < m; ++j) s += A[i * m + j] * ctr[j]; p.push_back(-s); }
  for (double a : A) p.push_back(a);
  return mkg(G_QUAD, p);
}
static GSpec g_quad_rand(int m) {
  Vd p; p.push_back(rnd(-2, 2));
  for (int i = 0; i < m; ++i) p.push_back(rnd(-2, 2));
  for (int i = 0; i < m * m; ++i) p.push_back(coin(0.3) ? 0.0 : rnd(-2, 2));
  return mkg(G_QUAD, p);
}
static GSpec g_linear(int m) {
  Vd p; p.push_back(rnd(-2, 2));
  for (int i = 0; i < m; ++i) p.push_back(rnz(0.2, 2));
  if (coin(0.15)) for (int i = 0; i < m * m; ++i) p.push_back(0.0);   // also with an explicit zero matrix
  return mkg(G_QUAD, p);
}
static GSpec g_poly_rand(int m, int maxdeg) {
  int nt = rint(1, m >= 4 ? 3 : std::min(5, m + 2));
  Vd p; p.push_back(nt);
  for (int t = 0; t < nt; ++t) {
    p.push_back(rnd(-2, 2));
    int left = rint(0, maxdeg);
    for (int k = 0; k < m; ++k) { int e = k == m - 1 ? left : rint(0, left); left -= e; p.push_back(e); }
  }
  return mkg(G_POLY, p);
}
// sum_j a_j (x_j - c_j)^4 + b_j (x_j - c_j)^2 expanded per variable (a minimum at c)
static GSpec g_poly_min(int m, const Vd& ctr, bool couple = true) {
  Vd p; p.push_back(0);
  int nt = 0;
  for (int j = 0; j < m; ++j) {
    double a = coin(0.5) ? rnd(0.1, 1.0) : 0.0, b = rnd(0.2, 2.0), c = ctr[j];
    // a (x-c)^4 = a x^4 - 4ac x^3 + 6ac^2 x^2 - 4ac^3 x + a c^4 ; b (x-c)^2 = b x^2 - 2bc x + b c^2
    double co[5] = {a * c * c * c * c + b * c * c, -4 * a * c * c * c - 2 * b * c, 6 * a * c * c + b, -4 * a * c, a};
    for (int e = 0; e <= 4; ++e) {
      if (co[e] == 0.0) continue;
      p.push_back(co[e]);
      for (int k = 0; k < m; ++k) p.push_back(k == j ? e : 0);
      ++nt;
    }
  }
  if (couple && m >= 2 && coin(0.5)) { // a small coupling term x_0 x_1
    p.push_back(rnd(-0.3, 0.3));
    for (int k = 0; k < m; ++k) p.push_back(k <= 1 ? 1 : 0);
    ++nt;
  }
  p[0] = nt;
  return mkg(G_POLY, p);
}
static GSpec g_trig(int m, bool convex_part) {
  Vd p; p.push_back(rnd(-1, 1));
  for (int j = 0; j < m; ++j) {
    p.push_back(rnz(0.2, 1.5));
    p.push_back(coin(0.4) ? (double)rint(1, 3) : rnd(0.3, 3.0));
    p.push_back(rnd(0, 6.2831853));
    p.push_back(convex_part ? rnd(0.05, 1.0) : (coin(0.5) ? 0.0 : rnd(-0.5, 0.5)));
  }
  return mkg(G_TRIG, p);
}
static GSpec g_kink(int m, const Vd& ctr, bool convex) {
  Vd p; p.push_back(rnd(-1, 1));
  for (int j = 0; j < m; ++j) { p.push_back(convex ? rnd(0.2, 2.0) : rnz(0.2, 2.0)); p.push_back(ctr[j]); }
  return mkg(G_KINK, p);
}
static GSpec g_exp(int m) {
  Vd p; p.push_back(rnd(-2, 2)); p.push_back(rnz(0.1, 1.5));
  for (int j = 0; j < m; ++j) p.push_back(coin(0.2) ? 0.0 : rnz(0.1, 1.2));
  return mkg(G_EXP, p);
}
static GSpec g_rosen(double a, double b) { return mkg(G_ROSEN, Vd{a, b}); }
// add a constant so that g(r) is (nearly) 0
static void g_shift_root(GSpec& g, int m, const Vd& r) {
  double v = g_value(g, m, r.data());
  if (g.kind == G_POLY) {
    int nt = (int)g.p[0];
    g.p.push_back(-v);
    for (int k = 0; k < m; ++k) g.p.push_back(0);
    g.p[0] = nt + 1;
  } else if (g.kind != G_ROSEN) {
    g.p[0] -= v;
  }
}
static bool g_finite(const GSpec& g) { for (double v : g.p) if (!std::isfinite(v)) return false; return true; }
static NSpec nspec(int n, int mode, const std::vector<GSpec>& g) {
  NSpec s; s.n = n; s.mode = mode; s.g = g; s.lo.assign(n, 1.0); s.hi.assign(n, 0.0);
  for (const GSpec& t : g) if (!g_finite(t)) { s.g.assign(1, g_linear(n)); break; }
  return s;
}
// a failure box around `ctr` (half widths `w`), for the callbacks of `mask`
static void set_fail(NSpec& s, const Vd& ctr, double w, int mode) {
  s.fmode = mode;
  for (int j = 0; j < s.n; ++j) {
    double a = ctr[j] + rnd(-w, w), b = ctr[j] + rnd(-w, w);
    s.lo[j] = std::min(a, b); s.hi[j] = std::max(a, b);
  }
}
static void maybe_fail(NSpec& s, const Vd& ctr, double w, double prob, int maxmode) {
  if (coin(prob)) set_fail(s, ctr, w, rint(1, maxmode));
}

// a function to minimise, with its (approximate) minimum in `xmin` (when it has one)
static NSpec mv_min(int n, Vd& xmin) {
  xmin = rvec(n, -2, 2);
  double q = rnd(0, 1);
  std::vector<GSpec> g;
  int mode = 0;
  if (q < 0.26) {                                       // convex quadratic
    double shift = pickl({1.0, 0.1, 0.01, 1e-4, 2.0});
    Vd A = rspd(n, shift, n);
    if (coin(0.1)) for (double& a : A) a *= 1e3;
    g.push_back(g_quad_at(n, A, xmin, rnd(-2, 2)));
  } else if (q < 0.34) {                                // semidefinite (singular Hessian) or indefinite quadratic
    Vd A = rspd(n, coin(0.5) ? 0.0 : -rnd(0.1, 1.0), std::max(1, n - 1));
    g.push_back(g_quad_at(n, A, xmin, rnd(-2, 2)));
  } else if (q < 0.48) {                                // Rosenbrock
    double a = coin(0.5) ? 1.0 : rnd(-1.5, 1.5), b = pickl({1.0, 10.0, 100.0, 0.5});
    g.push_back(g_rosen(a, b));
    for (int j = 0; j < n; ++j) xmin[j] = j == 0 ? a : xmin[j - 1] * xmin[j - 1];
    if (n == 1) xmin[0] = a;
  } else if (q < 0.62) {                                // sum of squares of residuals with a common root
    mode = 1;
    int k = std::max(1, n + rint(-1, 1));
    for (int e = 0; e < k; ++e) {
      double r = rnd(0, 1);
      GSpec t = r < 0.4 ? (n <= 3 ? g_quad_rand(n) : g_poly_rand(n, 3)) : (r < 0.7 ? g_poly_rand(n, 3) : (r < 0.85 ? g_trig(n, false) : g_exp(n)));
      if (coin(0.9)) g_shift_root(t, n, xmin);
      g.push_back(t);
    }
  } else if (q < 0.72) {                                // trigonometric with a convex part
    g.push_back(g_trig(n, true));
  } else if (q < 0.78) {                                // polynomial of degree 4
    g.push_back(g_poly_min(n, xmin));
  } else if (q < 0.84) {                                // kink
    g.push_back(g_kink(n, xmin, true));
  } else if (q < 0.92) {                                // quadratic + exp
    g.push_back(g_quad_at(n, rspd(n, 0.5, n), xmin, rnd(-2, 2)));
    g.push_back(g_exp(n));
  } else {                                              // linear: no minimum (on a bound only)
    g.push_back(g_linear(n));
  }
  NSpec s = nspec(n, mode, g);
  s.smode = rint(0, 2);
  return s;
}

static int rdim_fs() {
  double q = rnd(0, 1);
  return q < 0.30 ? 1 : q < 0.65 ? 2 : q < 0.85 ? 3 : q < 0.92 ? 4 : q < 0.96 ? 5 : 6;
}
// a system of neq equations in n unknowns with the root r
static NSpec fs_sys(int n, int neq, Vd& r, int variant) {
  r = rvec(n, -2, 2);
  std::vector<GSpec> g;
  for (int k = 0; k < neq; ++k) {
    GSpec t;
    double q = rnd(0, 1);
    if (variant == 1) {                                 // singular Jacobian at the root: every equation has its extremum there
      t = n <= 3 ? g_quad_at(n, rspd(n, rnd(0.0, 0.5), n), r, 0.0) : g_poly_min(n, r, false);
    } else if (variant == 2) {                          // linear system
      t = g_linear(n);
      if (coin(0.4)) for (int j = 0; j < n; ++j) t.p[1 + j] = coin(0.3) ? 0.0 : rnd(-2, 2);
    } else if (q < 0.35) t = n <= 3 ? g_quad_rand(n) : g_poly_rand(n, 3);
    else if (q < 0.60) t = g_poly_rand(n, 3);
    else if (q < 0.80) t = g_trig(n, false);
    else if (q < 0.93) t = g_exp(n);
    else t = g_kink(n, rvec(n, -2, 2), false);
    if (variant != 4 || k > 0) g_shift_root(t, n, r);   // variant 4: the first equation has no root at r
    g.push_back(t);
  }
  if (variant == 3 && neq >= 2) g[1] = g[0];            // rank deficient: two equal equations
  NSpec s = nspec(n, 0, g);
  s.smode = rint(0, 2);
  return s;
}
static NSpec fs_rand(int n, int neq, Vd& r) {
  double q = rnd(0, 1);
  int variant = q < 0.66 ? 0 : q < 0.76 ? 1 : q < 0.86 ? 2 : q < 0.94 ? 3 : 4;
  return fs_sys(n, neq, r, variant);
}
// a starting point near `r`
static Vd start_near(const Vd& r) {
  static const double D[] = {0.0, 1e-12, 1e-8, 1e-4, 1e-2, 0.1, 0.3, 0.6, 1.0, 2.0};
  double d = pick(D);
  Vd x = r;
  for (double& v : x) v += d * rnd(-1, 1);
  return x;
}
static Vd rtolv(int n) {
  static const double T[] = {0.0, 1e-300, 1e-15, 1e-12, 1e-10, 1e-8, 1e-6, 1e-4, 1e-2, 1.0, 1e3};
  Vd t(n);
  if (coin(0.5)) { double v = coin(0.5) ? pick(T) : std::pow(10.0, rnd(-12, -1)); for (double& x : t) x = v; }
  else for (double& x : t) x = coin(0.5) ? pick(T) : std::pow(10.0, rnd(-12, -1));
  return t;
}
static double rtol1() {
  static const double T[] = {0.0, 1e-300, 1e-15, 1e-12, 1e-10, 1e-8, 1e-6, 1e-4, 1e-2, 0.1, 1.0, 1e3};
  return coin(0.5) ? pick(T) : std::pow(10.0, rnd(-12, -1));
}
static int rnbit(int dflt) {
  static const int N[] = {0, 1, 2, 3, 5, 10, 20, 50, 100, 200};
  return coin(0.4) ? dflt : pick(N);
}
// bounds: containing r and x0, excluding r, degenerate in a coordinate, infinite, or not containing x0
static void rbounds(const Vd& r, const Vd& x0, Vd& inf, Vd& sup) {
  int n = (int)r.size();
  inf.resize(n); sup.resize(n);
  double q = rnd(0, 1);
  for (int j = 0; j < n; ++j) {
    double lo = std::min(r[j], x0[j]), hi = std::max(r[j], x0[j]);
    inf[j] = lo - rnd(0.01, 2.0); sup[j] = hi + rnd(0.01, 2.0);
  }
  if (q < 0.40) return;
  int j = rint(0, n - 1);
  if (q < 0.55) {                       // the root is outside: cut it off on one side
    if (x0[j] > r[j]) inf[j] = 0.5 * (r[j] + x0[j]); else sup[j] = 0.5 * (r[j] + x0[j]);
    if (inf[j] >= sup[j]) { inf[j] = std::min(r[j], x0[j]); sup[j] = std::max(r[j], x0[j]) + 1.0; }
  } else if (q < 0.65) {                // degenerate: lower = upper
    inf[j] = sup[j] = coin(0.5) ? x0[j] : r[j];
  } else if (q < 0.75) {                // infinite
    for (int k = 0; k < n; ++k) { inf[k] = -DBL_MAX; sup[k] = DBL_MAX; }
    if (coin(0.4)) { inf[j] = -2e100; sup[j] = 2e100; }
  } else if (q < 0.85) {                // the start is outside (divergent)
    if (coin(0.5)) inf[j] = x0[j] + rnd(0.01, 0.5); else sup[j] = x0[j] - rnd(0.01, 0.5);
    if (inf[j] > sup[j]) std::swap(inf[j], sup[j]);
  } else if (q < 0.92) {                // the start on a bound
    if (coin(0.5)) inf[j] = x0[j]; else sup[j] = x0[j];
  } else {                              // tight box around the root
    for (int k = 0; k < n; ++k) { inf[k] = r[k] - 1e-6; sup[k] = r[k] + 1e-6; }
  }
}
static int rlower() { return coin(0.8) ? 1 : pickli({0, -2, 3, 10}); }


// ---------------------------------------------------------------------------------
// runs an accessor whose documented check fails (`bad`): `throws` when OCCT raises, `unchecked` when it does not
static void checked_call(bool bad, const std::function<void()>& body) {
  if (!bad) { body(); return; }
  try {
    body();
  } catch (Standard_Failure&) {
    buf.resize(mark); buf += " throws";
    return;
  }
  unchecked();
}
static int fam_quota(const char* n, int q) { return fam_lines[n] < q; }

// =================================================================================
// the test functions themselves (checks the judge's implementation)
// =================================================================================
static void fam_test_tf() {
  const char* N = "test_tf";
  for (int att = 0; att < 3000 && fam_lines[N] < 30; ++att) {
    double lo = rnd(-5, 0), hi = lo + rnd(0.5, 10);
    FnSpec s = fn_rootful(lo, hi, rstate());
    if (att < 21) { int kind = att % K_COUNT; for (int tries = 0; tries < 200 && s.kind != kind; ++tries) s = fn_rootful(lo, hi, rstate()); }
    add_fail(s, lo, hi, 0.4);
    double x = coin(0.3) ? rnd(-12, 12) : rnd(lo, hi);
    if (coin(0.1) && s.flo <= s.fhi) x = coin(0.5) ? s.flo : s.fhi;
    begin(N, "tf f"); W_tf(s); W_f(x); bar();
    emit([&] {
      TF f(s);
      double v = 0, d = 0, v2 = 0, d2 = 0;
      bool ok1 = f.Value(x, v); ob(ok1); if (ok1) h(v);
      bool ok2 = f.Derivative(x, d); ob(ok2); if (ok2) h(d);
      bool ok3 = f.Values(x, v2, d2); ob(ok3); if (ok3) { h(v2); h(d2); }
      in(f.GetStateNumber());
      out_counts(f);
    });
  }
}
static NSpec any_mv(int n) {
  Vd xm;
  NSpec s = mv_min(n, xm);
  if (coin(0.3)) {        // any term kind, possibly a sum of several
    std::vector<GSpec> g;
    int k = rint(1, 3);
    for (int e = 0; e < k; ++e) {
      int kind = rint(0, 5);
      g.push_back(kind == 0 ? g_quad_rand(n) : kind == 1 ? g_rosen(rnd(-2, 2), rnd(0.5, 50)) : kind == 2 ? g_poly_rand(n, 4)
                  : kind == 3 ? g_trig(n, coin(0.5)) : kind == 4 ? g_kink(n, rvec(n, -2, 2), false) : g_exp(n));
    }
    s = nspec(n, rint(0, 1), g);
    s.smode = rint(0, 2);
  }
  return s;
}
static void fam_test_mv() {
  const char* N = "test_mv";
  for (int att = 0; att < 3000 && fam_lines[N] < 40; ++att) {
    int n = att < 12 ? 1 + att % 6 : rdim();
    NSpec s = any_mv(n);
    if (att < 36) {         // every term kind, alone
      int kind = att % 6;
      GSpec g = kind == 0 ? g_quad_rand(n) : kind == 1 ? g_rosen(rnd(-2, 2), rnd(0.5, 50)) : kind == 2 ? g_poly_rand(n, 4)
                : kind == 3 ? g_trig(n, coin(0.5)) : kind == 4 ? g_kink(n, rvec(n, -2, 2), false) : g_exp(n);
      s = nspec(n, (att / 6) % 2, std::vector<GSpec>{g}); s.smode = rint(0, 2);
    }
    Vd x = rvec(n, -2.5, 2.5);
    if (coin(0.4)) set_fail(s, x, 0.5, rint(1, 15));
    int lower = rlower();
    begin(N, "mv v"); W_mv(s); W_v(vin(x, lower)); bar();
    emit([&] {
      MVF f(s);
      math_Vector X = mkv(vin(x, lower));
      double v = 0, v2 = 0, v3 = 0;
      math_Vector G(1, n, 0.0), G2(1, n, 0.0), G3(1, n, 0.0);
      math_Matrix H(1, n, 1, n, 0.0);
      bool o1 = f.Value(X, v); ob(o1); if (o1) h(v);
      bool o2 = f.Gradient(X, G); ob(o2); if (o2) ov(G);
      bool o3 = f.Values(X, v2, G2); ob(o3); if (o3) { h(v2); ov(G2); }
      bool o4 = f.Values(X, v3, G3, H); ob(o4); if (o4) { h(v3); ov(G3); om(H); }
      in(f.GetStateNumber()); in(f.NbVariables());
      out_mv(f);
    });
  }
}
static void fam_test_fs() {
  const char* N = "test_fs";
  for (int att = 0; att < 3000 && fam_lines[N] < 30; ++att) {
    int n = att < 12 ? 1 + att % 6 : rdim_fs();
    int neq = std::max(1, std::min(6, n + rint(-1, 1)));
    Vd r;
    NSpec s = fs_rand(n, neq, r);
    Vd x = coin(0.2) ? r : rvec(n, -2.5, 2.5);
    if (coin(0.4)) set_fail(s, x, 0.5, rint(1, 7));
    int lower = rlower();
    begin(N, "fs v"); W_fs(s); W_v(vin(x, lower)); bar();
    emit([&] {
      FSF f(s);
      math_Vector X = mkv(vin(x, lower));
      math_Vector F(1, neq, 0.0), F2(1, neq, 0.0);
      math_Matrix D(1, neq, 1, n, 0.0), D2(1, neq, 1, n, 0.0);
      bool o1 = f.Value(X, F); ob(o1); if (o1) ov(F);
      bool o2 = f.Derivatives(X, D); ob(o2); if (o2) om(D);
      bool o3 = f.Values(X, F2, D2); ob(o3); if (o3) { ov(F2); om(D2); }
      in(f.GetStateNumber()); in(f.NbVariables()); in(f.NbEquations());
      out_fs(f);
    });
  }
}

// =================================================================================
// math_FunctionSetRoot
// =================================================================================
struct FSRCase { NSpec fn; Vd tol; int nbit = 100; VecIn start; VecIn inf, sup; int stop = 0; };
static FSRCase rand_fsr(int att) {
  FSRCase c;
  int n = rdim_fs();
  double q = rnd(0, 1);
  int neq = q < 0.12 ? std::min(6, n + 1) : (q < 0.22 && n > 1 ? n - 1 : n);
  Vd r;
  c.fn = fs_rand(n, neq, r);
  Vd x0 = start_near(r);
  if (coin(0.05)) x0 = r;
  maybe_fail(c.fn, x0, 0.6, 0.12, 7);
  c.tol = rtolv(n);
  c.nbit = rnbit(100);
  c.start = vin(x0, rlower());
  Vd inf, sup;
  rbounds(r, x0, inf, sup);
  c.inf = vin(inf); c.sup = vin(sup);
  c.stop = coin(0.2) ? 1 : 0;
  (void)att;
  return c;
}
// hand-made edge cases for the first iterations
static void edge_fsr(int att, FSRCase& c) {
  auto one = [&](const GSpec& g) { NSpec s = nspec(1, 0, std::vector<GSpec>{g}); return s; };
  switch (att) {
    case 0: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 100; c.start = vin({1.0}); break;         // x^2 - 2
    case 1: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {0.0}; c.nbit = 100; c.start = vin({1.4142135623730951}); break; // at the root
    case 2: c.fn = one(mkg(G_QUAD, Vd{0.0, 0.0, 2.0})); c.tol = {1e-8}; c.nbit = 100; c.start = vin({0.5}); break;          // x^2: singular at the root
    case 3: c.fn = one(mkg(G_QUAD, Vd{1.0, 0.0, 2.0})); c.tol = {1e-8}; c.nbit = 100; c.start = vin({0.5}); break;          // x^2 + 1: no root
    case 4: c.fn = one(mkg(G_QUAD, Vd{1.0, 0.0, 2.0})); c.tol = {1e-8}; c.nbit = 100; c.start = vin({0.0}); break;          // at the stationary point
    case 5: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 0; c.start = vin({1.0}); break;          // no iteration
    case 6: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 1; c.start = vin({1.0}); break;          // one iteration
    case 7: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 100; c.start = vin({1.0, 2.0}); break;   // wrong start length
    case 8: c.fn = nspec(2, 0, {}); c.tol = {1e-10, 1e-10}; c.nbit = 100; c.start = vin({1.0, 2.0}); break;                  // no equation
    case 9: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 100; c.start = vin({1.0});
            c.inf = vin({0.0, 0.0}); c.sup = vin({3.0, 3.0}); break;                                                        // wrong bound length
    case 10: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 100; c.start = vin({1.0});
             c.inf = vin({1.5}); c.sup = vin({3.0}); break;                                                                 // start below the box
    case 11: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e-10}; c.nbit = 100; c.start = vin({1.0});
             c.inf = vin({1.0}); c.sup = vin({1.0}); break;                                                                 // degenerate box
    case 12: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); c.tol = {1e3}; c.nbit = 100; c.start = vin({1.0}); break;        // huge tolerance
    case 13: c.fn = one(mkg(G_QUAD, Vd{-2.0, 0.0, 2.0})); set_fail(c.fn, {1.0}, 0.0, 7); c.fn.lo = {0.9}; c.fn.hi = {1.1};
             c.tol = {1e-10}; c.nbit = 100; c.start = vin({1.0}); break;                                                    // fails at the start
    case 14: { Vd r; c.fn = fs_sys(2, 2, r, 1); c.tol = {1e-8, 1e-8}; c.nbit = 100; c.start = vin(start_near(r)); break; }   // singular
    case 15: { Vd r; c.fn = fs_sys(3, 3, r, 3); c.tol = {1e-8, 1e-8, 1e-8}; c.nbit = 100; c.start = vin(start_near(r)); break; } // rank deficient
    case 16: { Vd r; c.fn = fs_sys(2, 3, r, 0); c.tol = {1e-9, 1e-9}; c.nbit = 100; c.start = vin(start_near(r)); break; }   // more equations
    case 17: { Vd r; c.fn = fs_sys(3, 2, r, 0); c.tol = {1e-9, 1e-9, 1e-9}; c.nbit = 100; c.start = vin(start_near(r)); break; } // fewer equations
    case 18: { Vd r; c.fn = fs_sys(6, 6, r, 2); c.tol = Vd(6, 1e-9); c.nbit = 100; c.start = vin(start_near(r)); break; }   // linear, 6 unknowns
    default: return;
  }
  if (att != 9) { int n = c.fn.n; Vd a(n, -10.0), b(n, 10.0); if (att < 9 || att > 11) { c.inf = vin(a); c.sup = vin(b); } }
  if (att != 13) { c.fn.fmode = 0; c.fn.lo.assign(c.fn.n, 1.0); c.fn.hi.assign(c.fn.n, 0.0); }
  c.stop = 0;
}
static void out_fsr(math_FunctionSetRoot& r, FSF& f) {
  ob(r.IsDone()); ob(r.IsDivergent()); out_fs(f);
  if (r.IsDone()) { ov(r.Root()); in(r.NbIterations()); in(r.StateNumber()); om(r.Derivative()); }
}
static void fam_function_set_root() {
  const char* N = "function_set_root";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    FSRCase c = rand_fsr(att); edge_fsr(att, c);
    if (att == 9) continue;
    begin(N, "fs v i v i"); W_fs(c.fn); W_v(vin(c.tol)); W_i(c.nbit); W_v(c.start); W_i(c.stop); bar();
    emit([&] {
      FSF f(c.fn);
      math_FunctionSetRoot r(f, mkv(vin(c.tol)), c.nbit);
      r.Perform(f, mkv(c.start), c.stop != 0);
      out_fsr(r, f);
    });
  }
}
static void fam_function_set_root_bounded() {
  const char* N = "function_set_root_bounded";
  for (int att = 0; att < 6000 && fam_lines[N] < 80; ++att) {
    FSRCase c = rand_fsr(att); edge_fsr(att, c);
    begin(N, "fs v i v v v i"); W_fs(c.fn); W_v(vin(c.tol)); W_i(c.nbit); W_v(c.start); W_v(c.inf); W_v(c.sup); W_i(c.stop); bar();
    emit([&] {
      FSF f(c.fn);
      math_FunctionSetRoot r(f, mkv(vin(c.tol)), c.nbit);
      r.Perform(f, mkv(c.start), mkv(c.inf), mkv(c.sup), c.stop != 0);
      out_fsr(r, f);
    });
  }
}
// constructor without tolerance, SetTolerance, one or two Perform (with or without bounds)
static void fam_function_set_root_staged() {
  const char* N = "function_set_root_staged";
  for (int att = 0; att < 6000 && fam_lines[N] < 50; ++att) {
    FSRCase c = rand_fsr(att); edge_fsr(att % 19, c);
    if (att % 19 == 7 || att % 19 == 8 || att % 19 == 9) continue;
    int bounded = rint(0, 1), twice = coin(0.5) ? 1 : 0;
    VecIn start2 = vin(start_near(c.start.v), rlower());
    begin(N, "fs i v i v v v i i v"); W_fs(c.fn); W_i(c.nbit); W_v(vin(c.tol)); W_i(bounded); W_v(c.start); W_v(c.inf); W_v(c.sup);
    W_i(c.stop); W_i(twice); W_v(start2); bar();
    emit([&] {
      FSF f(c.fn);
      math_FunctionSetRoot r(f, c.nbit);
      r.SetTolerance(mkv(vin(c.tol)));
      for (int k = 0; k <= twice; ++k) {
        math_Vector s = mkv(k == 0 ? c.start : start2);
        if (bounded) r.Perform(f, s, mkv(c.inf), mkv(c.sup), c.stop != 0); else r.Perform(f, s, c.stop != 0);
      }
      out_fsr(r, f);
    });
  }
}
class FSRCustom : public math_FunctionSetRoot {
public:
  int mode; double thr;
  FSRCustom(FSF& f, const math_Vector& tol, int nbit, int m, double t) : math_FunctionSetRoot(f, tol, nbit), mode(m), thr(t) {}
  bool IsSolutionReached(math_FunctionSetWithDerivatives& F) override {
    bool def = math_FunctionSetRoot::IsSolutionReached(F);
    if (mode == 1) {
      double s = 0.0;
      for (int i = Delta.Lower(); i <= Delta.Upper(); ++i) s += Delta(i) * Delta(i);
      return def || s <= thr;
    }
    if (mode == 2) {   // the function is called whatever the default test says
      math_Vector fv(1, F.NbEquations(), 0.0);
      bool ok = F.Value(Sol, fv);
      double s = 0.0;
      if (ok) for (int i = fv.Lower(); i <= fv.Upper(); ++i) s += fv(i) * fv(i);
      return def || (ok && s <= thr);
    }
    if (mode == 3) return false;
    return def && std::fabs(DF(DF.LowerRow(), DF.LowerCol())) + Tol(Tol.Lower()) >= thr;
  }
};
static void fam_function_set_root_custom() {
  const char* N = "function_set_root_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    FSRCase c = rand_fsr(att);
    int bounded = rint(0, 1), mode = att % 4;
    double thr = mode == 0 ? rnd(0.0, 2.0) : std::pow(10.0, rnd(-14, 0));
    begin(N, "fs v i v i v v i f"); W_fs(c.fn); W_v(vin(c.tol)); W_i(c.nbit); W_v(c.start); W_i(bounded); W_v(c.inf); W_v(c.sup);
    W_i(mode); W_f(thr); bar();
    emit([&] {
      FSF f(c.fn);
      FSRCustom r(f, mkv(vin(c.tol)), c.nbit, mode, thr);
      if (bounded) r.Perform(f, mkv(c.start), mkv(c.inf), mkv(c.sup)); else r.Perform(f, mkv(c.start));
      out_fsr(r, f);
    });
  }
}
static void fam_function_set_root_access() {
  const char* N = "function_set_root_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 54; ++att) {
    FSRCase c = rand_fsr(att);
    int bounded = rint(0, 1), q = att % 9;
    int n = c.fn.n, neq = (int)c.fn.g.size();
    int olow = rlower(), orows = coin(0.8) ? neq : neq + 1, olen = coin(0.8) ? n : std::max(1, n + pickli({-1, 1}));
    if (q == 3 && coin(0.4)) { orows = neq + 1; olen = n; }
    begin(N, "fs v i v i v v i i i i i"); W_fs(c.fn); W_v(vin(c.tol)); W_i(c.nbit); W_v(c.start); W_i(bounded); W_v(c.inf);
    W_v(c.sup); W_i(c.stop); W_i(q); W_i(olow); W_i(orows); W_i(olen); bar();
    emit([&] {
      FSF f(c.fn);
      math_FunctionSetRoot r(f, mkv(vin(c.tol)), c.nbit);
      if (bounded) r.Perform(f, mkv(c.start), mkv(c.inf), mkv(c.sup), c.stop != 0); else r.Perform(f, mkv(c.start), c.stop != 0);
      math_Vector out(olow, olow + olen - 1, 0.0);
      math_Matrix outm(olow, olow + orows - 1, olow, olow + olen - 1, 0.0);
      switch (q) {
        case 0: ov(r.Root()); break;
        case 1: checked_call(r.IsDone() && olen != n, [&] { r.Root(out); ov(out); }); break;
        case 2: om(r.Derivative()); break;
        case 3: checked_call(r.IsDone() && (olen != n || orows != neq), [&] { r.Derivative(outm); om(outm); }); break;
        case 4: ov(r.FunctionSetErrors()); break;
        case 5: checked_call(r.IsDone() && olen != n, [&] { r.FunctionSetErrors(out); ov(out); }); break;
        case 6: in(r.NbIterations()); break;
        case 7: in(r.StateNumber()); break;
        default: ob(r.IsDivergent()); break;
      }
    });
  }
}
static void fam_function_set_root_dump() {
  const char* N = "function_set_root_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 20; ++att) {
    FSRCase c = rand_fsr(att);
    begin(N, "fs v i v i"); W_fs(c.fn); W_v(vin(c.tol)); W_i(c.nbit); W_v(c.start); W_i(c.stop); bar();
    emit([&] {
      FSF f(c.fn);
      math_FunctionSetRoot r(f, mkv(vin(c.tol)), c.nbit);
      r.Perform(f, mkv(c.start), c.stop != 0);
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}

// =================================================================================
// math_NewtonFunctionSetRoot
// =================================================================================
struct NSRCase { NSpec fn; Vd xtol; double ftol = 1e-10; int nbit = 100; VecIn start; VecIn inf, sup; };
static NSRCase rand_nsr(int att) {
  NSRCase c;
  int n = rdim_fs();
  Vd r;
  c.fn = fs_rand(n, n, r);
  Vd x0 = start_near(r);
  if (coin(0.05)) x0 = r;
  maybe_fail(c.fn, x0, 0.6, 0.12, 7);
  c.xtol = rtolv(n);
  c.ftol = rtol1();
  c.nbit = rnbit(100);
  c.start = vin(x0, rlower());
  Vd inf, sup;
  rbounds(r, x0, inf, sup);
  c.inf = vin(inf); c.sup = vin(sup);
  if (att < 19) {
    FSRCase e; e.fn = c.fn; e.tol = c.xtol; e.nbit = c.nbit; e.start = c.start; e.inf = c.inf; e.sup = c.sup;
    edge_fsr(att, e);
    if (att != 7 && att != 8 && att != 9 && att != 16 && att != 17) {
      c.fn = e.fn; c.xtol = e.tol; c.nbit = e.nbit; c.start = e.start; c.inf = e.inf; c.sup = e.sup;
      if (att == 1) c.ftol = 0.0;
    }
  }
  return c;
}
static void out_nsr(math_NewtonFunctionSetRoot& r, FSF& f) {
  ob(r.IsDone()); out_fs(f);
  if (r.IsDone()) { ov(r.Root()); om(r.Derivative()); ov(r.FunctionSetErrors()); in(r.NbIterations()); }
}
static void fam_newton_set_root() {
  const char* N = "newton_set_root";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    NSRCase c = rand_nsr(att);
    begin(N, "fs v f i v"); W_fs(c.fn); W_v(vin(c.xtol)); W_f(c.ftol); W_i(c.nbit); W_v(c.start); bar();
    emit([&] {
      FSF f(c.fn);
      math_NewtonFunctionSetRoot r(f, mkv(vin(c.xtol)), c.ftol, c.nbit);
      r.Perform(f, mkv(c.start));
      out_nsr(r, f);
    });
  }
}
static void fam_newton_set_root_bounded() {
  const char* N = "newton_set_root_bounded";
  for (int att = 0; att < 6000 && fam_lines[N] < 50; ++att) {
    NSRCase c = rand_nsr(att);
    begin(N, "fs v f i v v v"); W_fs(c.fn); W_v(vin(c.xtol)); W_f(c.ftol); W_i(c.nbit); W_v(c.start); W_v(c.inf); W_v(c.sup); bar();
    emit([&] {
      FSF f(c.fn);
      math_NewtonFunctionSetRoot r(f, mkv(vin(c.xtol)), c.ftol, c.nbit);
      r.Perform(f, mkv(c.start), mkv(c.inf), mkv(c.sup));
      out_nsr(r, f);
    });
  }
}
static void fam_newton_set_root_staged() {
  const char* N = "newton_set_root_staged";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    NSRCase c = rand_nsr(att);
    int bounded = rint(0, 1), twice = coin(0.5) ? 1 : 0;
    VecIn start2 = vin(start_near(c.start.v), rlower());
    begin(N, "fs f i v i v v v i v"); W_fs(c.fn); W_f(c.ftol); W_i(c.nbit); W_v(vin(c.xtol)); W_i(bounded); W_v(c.start); W_v(c.inf);
    W_v(c.sup); W_i(twice); W_v(start2); bar();
    emit([&] {
      FSF f(c.fn);
      math_NewtonFunctionSetRoot r(f, c.ftol, c.nbit);
      r.SetTolerance(mkv(vin(c.xtol)));
      for (int k = 0; k <= twice; ++k) {
        math_Vector s = mkv(k == 0 ? c.start : start2);
        if (bounded) r.Perform(f, s, mkv(c.inf), mkv(c.sup)); else r.Perform(f, s);
      }
      out_nsr(r, f);
    });
  }
}
class NSRCustom : public math_NewtonFunctionSetRoot {
public:
  int mode; double thr;
  NSRCustom(FSF& f, const math_Vector& xtol, double ftol, int nbit, int m, double t)
      : math_NewtonFunctionSetRoot(f, xtol, ftol, nbit), mode(m), thr(t) {}
  bool IsSolutionReached(math_FunctionSetWithDerivatives& F) override {
    bool def = math_NewtonFunctionSetRoot::IsSolutionReached(F);
    if (mode == 1) {
      double s = 0.0;
      for (int i = FValues.Lower(); i <= FValues.Upper(); ++i) s += FValues(i) * FValues(i);
      return def || s <= thr;
    }
    if (mode == 2) {
      math_Vector fv(1, F.NbEquations(), 0.0);
      bool ok = F.Value(Sol, fv);
      return def || (ok && std::fabs(fv(1)) <= thr);
    }
    if (mode == 3) return false;
    return def && std::fabs(Jacobian(Jacobian.LowerRow(), Jacobian.LowerCol())) + TolX(TolX.Lower()) + TolF >= thr
           && std::fabs(DeltaX(DeltaX.Lower())) < 1e300;
  }
};
static void fam_newton_set_root_custom() {
  const char* N = "newton_set_root_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    NSRCase c = rand_nsr(att);
    int mode = att % 4;
    double thr = mode == 0 ? rnd(0.0, 2.0) : std::pow(10.0, rnd(-14, 0));
    begin(N, "fs v f i v i f"); W_fs(c.fn); W_v(vin(c.xtol)); W_f(c.ftol); W_i(c.nbit); W_v(c.start); W_i(mode); W_f(thr); bar();
    emit([&] {
      FSF f(c.fn);
      NSRCustom r(f, mkv(vin(c.xtol)), c.ftol, c.nbit, mode, thr);
      r.Perform(f, mkv(c.start));
      out_nsr(r, f);
    });
  }
}
static void fam_newton_set_root_access() {
  const char* N = "newton_set_root_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 42; ++att) {
    NSRCase c = rand_nsr(att);
    int q = att % 7, n = c.fn.n;
    int olow = rlower(), orows = coin(0.8) ? n : n + 1, olen = coin(0.8) ? n : std::max(1, n + pickli({-1, 1}));
    if (q == 3 && coin(0.4)) { orows = n + 1; olen = n; }
    begin(N, "fs v f i v i i i i"); W_fs(c.fn); W_v(vin(c.xtol)); W_f(c.ftol); W_i(c.nbit); W_v(c.start); W_i(q); W_i(olow);
    W_i(orows); W_i(olen); bar();
    emit([&] {
      FSF f(c.fn);
      math_NewtonFunctionSetRoot r(f, mkv(vin(c.xtol)), c.ftol, c.nbit);
      r.Perform(f, mkv(c.start));
      math_Vector out(olow, olow + olen - 1, 0.0);
      math_Matrix outm(olow, olow + orows - 1, olow, olow + olen - 1, 0.0);
      switch (q) {
        case 0: ov(r.Root()); break;
        case 1: checked_call(r.IsDone() && olen != n, [&] { r.Root(out); ov(out); }); break;
        case 2: om(r.Derivative()); break;
        case 3: checked_call(r.IsDone() && (olen != n || orows != n), [&] { r.Derivative(outm); om(outm); }); break;
        case 4: ov(r.FunctionSetErrors()); break;
        case 5: checked_call(r.IsDone() && olen != n, [&] { r.FunctionSetErrors(out); ov(out); }); break;
        default: in(r.NbIterations()); break;
      }
    });
  }
}
static void fam_newton_set_root_dump() {
  const char* N = "newton_set_root_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 20; ++att) {
    NSRCase c = rand_nsr(att);
    begin(N, "fs v f i v"); W_fs(c.fn); W_v(vin(c.xtol)); W_f(c.ftol); W_i(c.nbit); W_v(c.start); bar();
    emit([&] {
      FSF f(c.fn);
      math_NewtonFunctionSetRoot r(f, mkv(vin(c.xtol)), c.ftol, c.nbit);
      r.Perform(f, mkv(c.start));
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}


// =================================================================================
// the minimisers: common cases
// =================================================================================
struct MinCase {
  NSpec fn; double tol = 1e-8; int nbit = 100; double zeps = 1e-12; double conv = 1e-6; int sing = 1;
  VecIn start, start2; int twice = 0; int setb = 0; VecIn left, right; MatIn dirs;
};
static MinCase rand_min(int att, double dtol, int dnbit) {
  MinCase c;
  int n = rdim();
  Vd xm;
  c.fn = mv_min(n, xm);
  Vd x0 = start_near(xm);
  if (coin(0.15)) x0 = rvec(n, -3, 3);
  maybe_fail(c.fn, x0, 0.6, 0.12, 15);
  c.tol = coin(0.4) ? dtol : rtol1();
  c.nbit = rnbit(dnbit);
  c.zeps = coin(0.6) ? 1e-12 : pickl({0.0, 1e-300, 1e-8, 1e-3});
  c.conv = coin(0.6) ? 1e-6 : pickl({0.0, 1e-12, 1e-3, 0.1, 1.0, 10.0});
  c.sing = coin(0.8) ? 1 : 0;
  c.start = vin(x0, rlower());
  c.start2 = vin(start_near(x0), rlower());
  c.twice = coin(0.3) ? 1 : 0;
  c.setb = coin(0.7) ? 1 : 0;
  Vd inf, sup;
  rbounds(xm, x0, inf, sup);
  for (int j = 0; j < n; ++j) if (inf[j] < -1e6 || sup[j] > 1e6) { inf[j] = -1e6; sup[j] = 1e6; }
  c.left = vin(inf, rlower()); c.right = vin(sup, rlower());
  if (coin(0.03)) c.left.v.push_back(0.0);      // wrong length
  c.dirs.rows = c.dirs.cols = n; c.dirs.a.assign(n * n, 0.0);
  double q = rnd(0, 1);
  for (int i = 0; i < n; ++i)
    for (int j = 0; j < n; ++j) {
      if (q < 0.6) c.dirs.a[i * n + j] = i == j ? 1.0 : 0.0;
      else if (q < 0.9) c.dirs.a[i * n + j] = rnd(-1, 1);
      else c.dirs.a[i * n + j] = i == j ? rnd(0.01, 3.0) : 0.0;
    }
  // edge cases
  switch (att) {
    case 0: c.fn = nspec(1, 0, {mkg(G_QUAD, Vd{1.0, -2.0, 2.0})}); c.start = vin({0.0}); break;          // (x-1)^2: one step
    case 1: c.fn = nspec(2, 0, {g_rosen(1.0, 100.0)}); c.start = vin({-1.2, 1.0}); break;                 // Rosenbrock
    case 2: c.fn = nspec(2, 0, {g_rosen(1.0, 100.0)}); c.start = vin({1.0, 1.0}); break;                  // at the minimum
    case 3: c.fn = nspec(2, 0, {mkg(G_QUAD, Vd{0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0})}); c.start = vin({0.0, 0.0}); break; // saddle point
    case 4: c.fn = nspec(2, 0, {g_linear(2)}); c.start = vin({0.5, 0.5}); break;                          // no minimum
    case 5: c.nbit = 0; break;
    case 6: c.nbit = 1; break;
    case 7: c.tol = 0.0; break;
    case 8: c.tol = 1e3; break;
    case 9: { Vd z(n, 0.0); c.fn = nspec(n, 0, {g_quad_at(n, rspd(n, 0.0, 1), z, 0.0)}); c.start = vin(start_near(z)); break; } // singular Hessian
    case 10: c.fn = nspec(6, 0, {g_rosen(1.0, 10.0)}); c.start = vin(Vd(6, 0.5)); break;
    default: break;
  }
  if (att <= 4 || att >= 9) {
    int m = c.fn.n;
    c.fn.fmode = 0; c.fn.lo.assign(m, 1.0); c.fn.hi.assign(m, 0.0);
    if ((int)c.start2.v.size() != m) c.start2 = vin(start_near(c.start.v));
    if ((int)c.left.v.size() != m) { c.left = vin(Vd(m, -2.0)); c.right = vin(Vd(m, 2.0)); }
    c.dirs.rows = c.dirs.cols = m; c.dirs.a.assign(m * m, 0.0); for (int i = 0; i < m; ++i) c.dirs.a[i * m + i] = 1.0;
  }
  return c;
}
static void W_minbase(const MinCase& c) { W_mv(c.fn); W_f(c.tol); W_i(c.nbit); }
static bool bad_bounds(const MinCase& c) { return (int)c.left.v.size() != c.fn.n || (int)c.right.v.size() != c.fn.n; }

// =================================================================================
// math_NewtonMinimum
// =================================================================================
static void out_nm(math_NewtonMinimum& r, MVF& f) {
  ob(r.IsDone()); in((int)r.GetStatus()); out_mv(f);
  if (r.IsDone()) { ov(r.Location()); h(r.Minimum()); ov(r.Gradient()); in(r.NbIterations()); }
}
static const char* NM_SCHEMA = "mv f i f i v";
static void W_nm(const MinCase& c) { W_minbase(c); W_f(c.conv); W_i(c.sing); W_v(c.start); }
static void fam_newton_minimum() {
  const char* N = "newton_minimum";
  for (int att = 0; att < 6000 && fam_lines[N] < 80; ++att) {
    MinCase c = rand_min(att, 1e-7, 40);
    begin(N, NM_SCHEMA); W_nm(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_NewtonMinimum r(f, c.tol, c.nbit, c.conv, c.sing != 0);
      r.Perform(f, mkv(c.start));
      out_nm(r, f);
    });
  }
}
static void fam_newton_minimum_bounded() {
  const char* N = "newton_minimum_bounded";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    MinCase c = rand_min(att, 1e-7, 40);
    begin(N, "mv f i f i v i v v i v"); W_nm(c); W_i(c.setb); W_v(c.left); W_v(c.right); W_i(c.twice); W_v(c.start2); bar();
    emit([&] {
      MVF f(c.fn);
      math_NewtonMinimum r(f, c.tol, c.nbit, c.conv, c.sing != 0);
      if (c.setb) {
        if (bad_bounds(c)) { unchecked(); return; }
        r.SetBoundary(mkv(c.left), mkv(c.right));
      }
      r.Perform(f, mkv(c.start));
      if (c.twice) r.Perform(f, mkv(c.start2));
      out_nm(r, f);
    });
  }
}
class NMCustom : public math_NewtonMinimum {
public:
  int mode; double thr;
  NMCustom(MVF& f, const MinCase& c, int m, double t) : math_NewtonMinimum(f, c.tol, c.nbit, c.conv, c.sing != 0), mode(m), thr(t) {}
  bool IsConverged() const override {
    bool def = math_NewtonMinimum::IsConverged();
    if (mode == 1) return def || std::fabs(TheMinimum - PreviousMinimum) <= thr;
    if (mode == 2) return def && nbiter >= 3;
    if (mode == 3) return false;
    double s = 0.0;
    for (int i = TheGradient.Lower(); i <= TheGradient.Upper(); ++i) s += TheGradient(i) * TheGradient(i);
    return (def && s <= thr) || (MinEigenValue > 1e300 && Convex && TheHessian(1, 1) + XTol + CTol > 0.0);
  }
};
static void fam_newton_minimum_custom() {
  const char* N = "newton_minimum_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    MinCase c = rand_min(att, 1e-7, 40);
    int mode = att % 4;
    double thr = std::pow(10.0, rnd(-14, 0));
    begin(N, "mv f i f i v i f"); W_nm(c); W_i(mode); W_f(thr); bar();
    emit([&] {
      MVF f(c.fn);
      NMCustom r(f, c, mode, thr);
      r.Perform(f, mkv(c.start));
      out_nm(r, f);
    });
  }
}
static void fam_newton_minimum_access() {
  const char* N = "newton_minimum_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 60; ++att) {
    MinCase c = rand_min(att, 1e-7, 40);
    if (bad_bounds(c)) continue;
    int q = att % 7, n = c.fn.n;
    int olow = rlower(), olen = coin(0.8) ? n : std::max(1, n + pickli({-1, 1}));
    begin(N, "mv f i f i v i v v i i i"); W_nm(c); W_i(c.setb); W_v(c.left); W_v(c.right); W_i(q); W_i(olow); W_i(olen); bar();
    emit([&] {
      MVF f(c.fn);
      math_NewtonMinimum r(f, c.tol, c.nbit, c.conv, c.sing != 0);
      if (c.setb) r.SetBoundary(mkv(c.left), mkv(c.right));
      r.Perform(f, mkv(c.start));
      math_Vector out(olow, olow + olen - 1, 0.0);
      switch (q) {
        case 0: ov(r.Location()); break;
        case 1: checked_call(r.IsDone() && olen != n, [&] { r.Location(out); ov(out); }); break;
        case 2: h(r.Minimum()); break;
        case 3: ov(r.Gradient()); break;
        case 4: checked_call(r.IsDone() && olen != n, [&] { r.Gradient(out); ov(out); }); break;
        case 5: in(r.NbIterations()); break;
        default: in((int)r.GetStatus()); break;
      }
    });
  }
}
static void fam_newton_minimum_dump() {
  const char* N = "newton_minimum_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    MinCase c = rand_min(att, 1e-7, 40);
    begin(N, NM_SCHEMA); W_nm(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_NewtonMinimum r(f, c.tol, c.nbit, c.conv, c.sing != 0);
      r.Perform(f, mkv(c.start));
      // Dump calls Location(), Minimum(), NbIterations(): their StdFail_NotDone checks are compiled out of the library
      if (!r.IsDone()) { unchecked(); return; }
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}

// =================================================================================
// math_BFGS
// =================================================================================
static void out_bfgs(math_BFGS& r, MVF& f) {
  ob(r.IsDone()); out_mv(f);
  if (r.IsDone()) { ov(r.Location()); h(r.Minimum()); ov(r.Gradient()); in(r.NbIterations()); }
}
static void W_bfgs(const MinCase& c) { W_minbase(c); W_f(c.zeps); W_v(c.start); }
static void fam_bfgs() {
  const char* N = "bfgs";
  for (int att = 0; att < 6000 && fam_lines[N] < 80; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v"); W_bfgs(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_BFGS r(c.fn.n, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start));
      out_bfgs(r, f);
    });
  }
}
static void fam_bfgs_bounded() {
  const char* N = "bfgs_bounded";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v i v v i v"); W_bfgs(c); W_i(c.setb); W_v(c.left); W_v(c.right); W_i(c.twice); W_v(c.start2); bar();
    emit([&] {
      MVF f(c.fn);
      math_BFGS r(c.fn.n, c.tol, c.nbit, c.zeps);
      if (c.setb) {
        if (bad_bounds(c)) { unchecked(); return; }
        r.SetBoundary(mkv(c.left), mkv(c.right));
      }
      r.Perform(f, mkv(c.start));
      if (c.twice) r.Perform(f, mkv(c.start2));
      out_bfgs(r, f);
    });
  }
}
class BFGSCustom : public math_BFGS {
public:
  int mode; double thr;
  BFGSCustom(const MinCase& c, int m, double t) : math_BFGS(c.fn.n, c.tol, c.nbit, c.zeps), mode(m), thr(t) {}
  bool IsSolutionReached(math_MultipleVarFunctionWithGradient& F) const override {
    bool def = math_BFGS::IsSolutionReached(F);
    if (mode == 1) return def || std::fabs(TheMinimum - PreviousMinimum) <= thr;
    if (mode == 2) { double v = 0.0; bool ok = F.Value(TheLocation, v); return def || (ok && v <= thr); }
    if (mode == 3) return false;
    double s = 0.0;
    for (int i = TheGradient.Lower(); i <= TheGradient.Upper(); ++i) s += TheGradient(i) * TheGradient(i);
    return def && (s <= thr || nbiter >= 3) && XTol + EPSZ >= 0.0;
  }
};
static void fam_bfgs_custom() {
  const char* N = "bfgs_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    int mode = att % 4;
    double thr = mode == 2 ? rnd(-1, 3) : std::pow(10.0, rnd(-14, 0));
    begin(N, "mv f i f v i f"); W_bfgs(c); W_i(mode); W_f(thr); bar();
    emit([&] {
      MVF f(c.fn);
      BFGSCustom r(c, mode, thr);
      r.Perform(f, mkv(c.start));
      out_bfgs(r, f);
    });
  }
}
static void fam_bfgs_access() {
  const char* N = "bfgs_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 50; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    if (bad_bounds(c)) continue;
    int q = att % 6, n = c.fn.n;
    int olow = rlower(), olen = coin(0.8) ? n : std::max(1, n + pickli({-1, 1}));
    begin(N, "mv f i f v i v v i i i"); W_bfgs(c); W_i(c.setb); W_v(c.left); W_v(c.right); W_i(q); W_i(olow); W_i(olen); bar();
    emit([&] {
      MVF f(c.fn);
      math_BFGS r(c.fn.n, c.tol, c.nbit, c.zeps);
      if (c.setb) r.SetBoundary(mkv(c.left), mkv(c.right));
      r.Perform(f, mkv(c.start));
      math_Vector out(olow, olow + olen - 1, 0.0);
      switch (q) {
        case 0: ov(r.Location()); break;
        case 1: checked_call(r.IsDone() && olen != n, [&] { r.Location(out); ov(out); }); break;
        case 2: h(r.Minimum()); break;
        case 3: ov(r.Gradient()); break;
        case 4: checked_call(r.IsDone() && olen != n, [&] { r.Gradient(out); ov(out); }); break;
        default: in(r.NbIterations()); break;
      }
    });
  }
}
static void fam_bfgs_dump() {
  const char* N = "bfgs_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v"); W_bfgs(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_BFGS r(c.fn.n, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start));
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}

// =================================================================================
// math_FRPR
// =================================================================================
static void out_frpr(math_FRPR& r, MVF& f) {
  ob(r.IsDone()); out_mv(f);
  if (r.IsDone()) { ov(r.Location()); h(r.Minimum()); ov(r.Gradient()); in(r.NbIterations()); }
}
static void fam_frpr() {
  const char* N = "frpr";
  for (int att = 0; att < 6000 && fam_lines[N] < 80; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v i v"); W_bfgs(c); W_i(c.twice); W_v(c.start2); bar();
    emit([&] {
      MVF f(c.fn);
      math_FRPR r(f, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start));
      if (c.twice) r.Perform(f, mkv(c.start2));
      out_frpr(r, f);
    });
  }
}
class FRPRCustom : public math_FRPR {
public:
  int mode; double thr;
  FRPRCustom(MVF& f, const MinCase& c, int m, double t) : math_FRPR(f, c.tol, c.nbit, c.zeps), mode(m), thr(t) {}
  bool IsSolutionReached(math_MultipleVarFunctionWithGradient& F) override {
    bool def = math_FRPR::IsSolutionReached(F);
    if (mode == 1) return def || std::fabs(TheMinimum - PreviousMinimum) <= thr;
    if (mode == 2) { double v = 0.0; bool ok = F.Value(TheLocation, v); return def || (ok && v <= thr); }
    if (mode == 3) return false;
    double s = 0.0;
    for (int i = TheGradient.Lower(); i <= TheGradient.Upper(); ++i) s += TheGradient(i) * TheGradient(i);
    return def && s <= thr + XTol + EPSZ;
  }
};
static void fam_frpr_custom() {
  const char* N = "frpr_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    int mode = att % 4;
    double thr = mode == 2 ? rnd(-1, 3) : std::pow(10.0, rnd(-14, 0));
    begin(N, "mv f i f v i f"); W_bfgs(c); W_i(mode); W_f(thr); bar();
    emit([&] {
      MVF f(c.fn);
      FRPRCustom r(f, c, mode, thr);
      r.Perform(f, mkv(c.start));
      out_frpr(r, f);
    });
  }
}
static void fam_frpr_access() {
  const char* N = "frpr_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 50; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    int q = att % 6, n = c.fn.n;
    int olow = rlower(), olen = coin(0.8) ? n : std::max(1, n + pickli({-1, 1}));
    begin(N, "mv f i f v i i i"); W_bfgs(c); W_i(q); W_i(olow); W_i(olen); bar();
    emit([&] {
      MVF f(c.fn);
      math_FRPR r(f, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start));
      math_Vector out(olow, olow + olen - 1, 0.0);
      switch (q) {
        case 0: ov(r.Location()); break;
        case 1: checked_call(r.IsDone() && olen != n, [&] { r.Location(out); ov(out); }); break;
        case 2: h(r.Minimum()); break;
        case 3: ov(r.Gradient()); break;
        case 4: checked_call(r.IsDone() && olen != n, [&] { r.Gradient(out); ov(out); }); break;
        default: in(r.NbIterations()); break;
      }
    });
  }
}
static void fam_frpr_dump() {
  const char* N = "frpr_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v"); W_bfgs(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_FRPR r(f, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start));
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}

// =================================================================================
// math_Powell
// =================================================================================
static void out_powell(math_Powell& r, MVF& f) {
  ob(r.IsDone()); out_mv(f);
  if (r.IsDone()) { ov(r.Location()); h(r.Minimum()); in(r.NbIterations()); }
}
static void W_powell(const MinCase& c) { W_bfgs(c); W_m(c.dirs); }
static void fam_powell() {
  const char* N = "powell";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v m i v"); W_powell(c); W_i(c.twice); W_v(c.start2); bar();
    emit([&] {
      MVF f(c.fn);
      math_Powell r(f, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start), mkm(c.dirs));
      if (c.twice) r.Perform(f, mkv(c.start2), mkm(c.dirs));
      out_powell(r, f);
    });
  }
}
class PowellCustom : public math_Powell {
public:
  int mode; double thr;
  PowellCustom(MVF& f, const MinCase& c, int m, double t) : math_Powell(f, c.tol, c.nbit, c.zeps), mode(m), thr(t) {}
  bool IsSolutionReached(math_MultipleVarFunction& F) override {
    bool def = math_Powell::IsSolutionReached(F);
    if (mode == 1) return def || std::fabs(TheMinimum - PreviousMinimum) <= thr;
    if (mode == 2) { double v = 0.0; bool ok = F.Value(TheLocation, v); return def || (ok && v <= thr); }
    if (mode == 3) return false;
    return def && std::fabs(TheLocation(TheLocation.Lower())) + XTol + EPSZ >= thr && TheLocationError > 0.0;
  }
};
static void fam_powell_custom() {
  const char* N = "powell_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    int mode = att % 4;
    double thr = mode == 2 ? rnd(-1, 3) : (mode == 0 ? rnd(0, 2) : std::pow(10.0, rnd(-14, 0)));
    begin(N, "mv f i f v m i f"); W_powell(c); W_i(mode); W_f(thr); bar();
    emit([&] {
      MVF f(c.fn);
      PowellCustom r(f, c, mode, thr);
      r.Perform(f, mkv(c.start), mkm(c.dirs));
      out_powell(r, f);
    });
  }
}
static void fam_powell_access() {
  const char* N = "powell_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 40; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    int q = att % 4, n = c.fn.n;
    int olow = rlower(), olen = coin(0.8) ? n : std::max(1, n + pickli({-1, 1}));
    begin(N, "mv f i f v m i i i"); W_powell(c); W_i(q); W_i(olow); W_i(olen); bar();
    emit([&] {
      MVF f(c.fn);
      math_Powell r(f, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start), mkm(c.dirs));
      math_Vector out(olow, olow + olen - 1, 0.0);
      switch (q) {
        case 0: ov(r.Location()); break;
        case 1: checked_call(r.IsDone() && olen != n, [&] { r.Location(out); ov(out); }); break;
        case 2: h(r.Minimum()); break;
        default: in(r.NbIterations()); break;
      }
    });
  }
}
static void fam_powell_dump() {
  const char* N = "powell_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    MinCase c = rand_min(att, 1e-8, 200);
    begin(N, "mv f i f v m"); W_powell(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_Powell r(f, c.tol, c.nbit, c.zeps);
      r.Perform(f, mkv(c.start), mkm(c.dirs));
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}


// =================================================================================
// math_Uzawa
// =================================================================================
struct UZCase { MatIn cont; Vd secont, start; int nce = 0, nci = 0; double elx = 1e-6, elc = 1e-6; int nbit = 500; };
static UZCase rand_uz(int att, bool ineq) {
  UZCase c;
  int ncol = rdim();
  int nlig = coin(0.9) ? rint(1, ncol) : ncol + 1;
  c.cont.rows = nlig; c.cont.cols = ncol;
  c.cont.a = rvec(nlig * ncol, -2, 2);
  double q = rnd(0, 1);
  if (q < 0.1 && nlig >= 2) for (int j = 0; j < ncol; ++j) c.cont.a[ncol + j] = 2.0 * c.cont.a[j];   // dependent rows
  else if (q < 0.15) for (double& a : c.cont.a) a *= 1e-4;
  c.secont = rvec(nlig, -3, 3);
  c.start = coin(0.2) ? Vd(ncol, 0.0) : rvec(ncol, -3, 3);
  if (ineq) { c.nci = rint(0, nlig); c.nce = nlig - c.nci; if (coin(0.05)) c.nce += 1; }
  else { c.nce = nlig; c.nci = 0; }
  if (coin(0.04)) c.secont.push_back(1.0);
  c.elx = coin(0.5) ? 1e-6 : pickl({0.0, 1e-12, 1e-9, 1e-3, 0.1, 10.0});
  c.elc = coin(0.5) ? 1e-6 : pickl({0.0, 1e-12, 1e-9, 1e-3, 0.1, 10.0});
  c.nbit = coin(0.5) ? 500 : pickli({0, 1, 2, 5, 50, 2000});
  if (att == 0) { c.cont = MatIn{1, 1, 1, 2, Vd{1.0, 1.0}}; c.secont = {1.0}; c.start = {0.0, 0.0}; }  // x + y = 1
  if (att == 1) { c.cont = MatIn{1, 1, 2, 2, Vd{1.0, 0.0, 0.0, 1.0}}; c.secont = {1.0, 2.0}; c.start = {5.0, 5.0}; }
  if (att <= 1) { if (ineq) { c.nce = 0; c.nci = c.cont.rows; } else { c.nce = c.cont.rows; c.nci = 0; } }
  return c;
}
// the documented checks of Perform that OCCT has compiled out: dimensions, and a singular C C^T for the direct method
static bool uz_bad(const UZCase& c) {
  int nlig = c.cont.rows, ncol = c.cont.cols;
  if ((int)c.secont.size() != nlig || c.nce + c.nci != nlig) return true;
  if (nlig > ncol) return true;   // Errinit(1, ColNumber) is written for i = 1 .. RowNumber: out of range (unchecked)
  if (c.nci == 0) {
    math_Matrix ctc(1, nlig, 1, nlig, 0.0);
    for (int i = 1; i <= nlig; ++i)
      for (int j = 1; j <= i; ++j) {
        double s = c.cont.a[(i - 1) * ncol] * c.cont.a[(j - 1) * ncol];
        for (int k = 2; k <= ncol; ++k) s += c.cont.a[(i - 1) * ncol + k - 1] * c.cont.a[(j - 1) * ncol + k - 1];
        ctc(i, j) = s;
      }
    math_Crout cr(ctc);
    if (!cr.IsDone()) return true;
  }
  return false;
}
static void out_uz(const math_Uzawa& u, int nci, int nlig) {
  ob(u.IsDone());
  if (u.IsDone()) {
    // InitialError has Cont.ColNumber() elements, of which OCCT writes the first Cont.RowNumber() only
    const math_Vector& e = u.InitialError();
    ov(u.Value()); in(e.Lower()); in(e.Length()); for (int i = 1; i <= nlig && i <= e.Length(); ++i) h(e(i));
    ov(u.Error()); in(u.NbIterations());
    if (nci == 0) om(u.InverseCont());   // the iterative method never writes CTCinv
  }
}
static math_Vector duale_of(const math_Uzawa& u, int nlig) { math_Vector v(1, nlig, 0.0); u.Duale(v); return v; }
static void W_uz(const UZCase& c, bool ineq) {
  W_m(c.cont); W_v(vin(c.secont)); W_v(vin(c.start));
  if (ineq) { W_i(c.nce); W_i(c.nci); }
  W_f(c.elx); W_f(c.elc); W_i(c.nbit);
}
static void fam_uzawa() {
  const char* N = "uzawa";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    UZCase c = rand_uz(att, false);
    begin(N, "m v v f f i"); W_uz(c, false); bar();
    emit([&] {
      if (uz_bad(c)) { unchecked(); return; }
      math_Uzawa u(mkm(c.cont), mkv(vin(c.secont)), mkv(vin(c.start)), c.elx, c.elc, c.nbit);
      out_uz(u, c.nci, c.cont.rows);
      ov(duale_of(u, c.cont.rows));
    });
  }
}
static void fam_uzawa_ineq() {
  const char* N = "uzawa_ineq";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    UZCase c = rand_uz(att, true);
    begin(N, "m v v i i f f i"); W_uz(c, true); bar();
    emit([&] {
      if (uz_bad(c)) { unchecked(); return; }
      math_Uzawa u(mkm(c.cont), mkv(vin(c.secont)), mkv(vin(c.start)), c.nce, c.nci, c.elx, c.elc, c.nbit);
      out_uz(u, c.nci, c.cont.rows);
      ov(duale_of(u, c.cont.rows));
    });
  }
}
static void fam_uzawa_access() {
  const char* N = "uzawa_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 50; ++att) {
    UZCase c = rand_uz(att, true);
    int q = att % 7;
    begin(N, "m v v i i f f i i"); W_uz(c, true); W_i(q); bar();
    emit([&] {
      if (uz_bad(c)) { unchecked(); return; }
      math_Uzawa u(mkm(c.cont), mkv(vin(c.secont)), mkv(vin(c.start)), c.nce, c.nci, c.elx, c.elc, c.nbit);
      switch (q) {
        case 0: ov(u.Value()); break;
        case 1: { const math_Vector& e = u.InitialError(); in(e.Lower()); in(e.Length()); for (int i = 1; i <= c.cont.rows && i <= e.Length(); ++i) h(e(i)); break; }
        case 2: ov(u.Error()); break;
        case 3: in(u.NbIterations()); break;
        case 4: om(u.InverseCont()); break;
        case 5: ov(duale_of(u, c.cont.rows)); break;
        default: ob(u.IsDone()); break;
      }
    });
  }
}
static void fam_uzawa_dump() {
  const char* N = "uzawa_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    UZCase c = rand_uz(att, true);
    begin(N, "m v v i i f f i"); W_uz(c, true); bar();
    emit([&] {
      if (uz_bad(c)) { unchecked(); return; }
      math_Uzawa u(mkm(c.cont), mkv(vin(c.secont)), mkv(vin(c.start)), c.nce, c.nci, c.elx, c.elc, c.nbit);
      os_text(dump_of([&](std::ostream& o) { u.Dump(o); }));
    });
  }
}

// =================================================================================
// math_BissecNewton
// =================================================================================
struct BNCase { double xtol; FnSpec fn; double b1, b2; int nbit; };
static BNCase rand_bn(int att) {
  BNCase c;
  double a, b; rinterval(a, b);
  double lo, hi; lohi(a, b, lo, hi);
  c.fn = fn_rootful(lo, hi, rstate());
  double x0, x1, r;
  if (coin(0.8) && find_root(c.fn, lo, hi, x0, x1, r)) {
    double q = rnd(0, 1);
    if (q < 0.4) { c.b1 = x0; c.b2 = x1; }
    else if (q < 0.7) { c.b1 = lo; c.b2 = hi; }
    else if (q < 0.8) { c.b1 = r; c.b2 = hi; }
    else { c.b1 = x1; c.b2 = x0; }
  } else { c.b1 = a; c.b2 = b; }
  add_fail(c.fn, lo, hi, 0.12);
  c.xtol = rtol();
  c.nbit = coin(0.5) ? 100 : pickli({0, 1, 2, 3, 5, 10, 20, 50});
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.xtol = 1e-10; c.nbit = 100; break;
    case 1: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 2; c.b2 = 3; c.xtol = 1e-10; c.nbit = 100; break;   // not bracketed
    case 2: c.fn = mk(K_POLY, Vd{-1.0, 1.0}, 1); c.b1 = 1; c.b2 = 3; c.xtol = 1e-10; c.nbit = 100; break;        // root at a bound
    case 3: c.fn = mk(K_POLY, Vd{0.0, 0.0, 1.0}, 0); c.b1 = -1; c.b2 = 1; c.xtol = 1e-10; c.nbit = 100; break;    // double root
    case 4: c.fn = mk(K_ABS, Vd{1.0, 0.3, 0.0}, 0); c.b1 = -1; c.b2 = 1; c.xtol = 1e-12; c.nbit = 100; break;    // kink
    case 5: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.xtol = 0.0; c.nbit = 100; break;
    case 6: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 2); c.b1 = 1.5; c.b2 = 1.5; c.xtol = 1e-10; c.nbit = 100; break;
    case 7: c.fn = mk(K_FLAT, Vd{-0.5, 0.5, 1.0, 1.0, 1.0, 0.0}, 0); c.b1 = -2; c.b2 = 2; c.xtol = 1e-9; c.nbit = 100; break;
    default: break;
  }
  return c;
}
static void W_bn(const BNCase& c) { W_f(c.xtol); W_tf(c.fn); W_f(c.b1); W_f(c.b2); W_i(c.nbit); }
static void out_bn(math_BissecNewton& r, TF& f) {
  ob(r.IsDone()); out_counts(f);
  if (r.IsDone()) { h(r.Root()); h(r.Derivative()); h(r.Value()); }
}
static void fam_bissec_newton() {
  const char* N = "bissec_newton";
  for (int att = 0; att < 6000 && fam_lines[N] < 80; ++att) {
    BNCase c = rand_bn(att);
    begin(N, "f tf f f i"); W_bn(c); bar();
    emit([&] {
      TF f(c.fn);
      math_BissecNewton r(c.xtol);
      r.Perform(f, c.b1, c.b2, c.nbit);
      out_bn(r, f);
    });
  }
}
class BNCustom : public math_BissecNewton {
public:
  int mode; double thr;
  BNCustom(double tol, int m, double t) : math_BissecNewton(tol), mode(m), thr(t) {}
  bool IsSolutionReached(math_FunctionWithDerivative& F) override {
    bool def = math_BissecNewton::IsSolutionReached(F);
    if (mode == 1) return def || std::fabs(f) <= thr;
    if (mode == 2) { double v = 0.0; bool ok = F.Value(x, v); return def || (ok && std::fabs(v) <= thr); }
    if (mode == 3) return false;
    return def && std::fabs(df) + XTol >= thr;
  }
};
static void fam_bissec_newton_custom() {
  const char* N = "bissec_newton_custom";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    BNCase c = rand_bn(att);
    int mode = att % 4;
    double thr = mode == 0 ? rnd(0, 2) : std::pow(10.0, rnd(-14, 0));
    begin(N, "f tf f f i i f"); W_bn(c); W_i(mode); W_f(thr); bar();
    emit([&] {
      TF f(c.fn);
      BNCustom r(c.xtol, mode, thr);
      r.Perform(f, c.b1, c.b2, c.nbit);
      out_bn(r, f);
    });
  }
}
static void fam_bissec_newton_access() {
  const char* N = "bissec_newton_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 30; ++att) {
    BNCase c = rand_bn(att);
    int q = att % 3;
    begin(N, "f tf f f i i"); W_bn(c); W_i(q); bar();
    emit([&] {
      TF f(c.fn);
      math_BissecNewton r(c.xtol);
      r.Perform(f, c.b1, c.b2, c.nbit);
      if (q == 0) h(r.Root()); else if (q == 1) h(r.Derivative()); else h(r.Value());
    });
  }
}
static void fam_bissec_newton_dump() {
  const char* N = "bissec_newton_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    BNCase c = rand_bn(att);
    begin(N, "f tf f f i"); W_bn(c); bar();
    emit([&] {
      TF f(c.fn);
      math_BissecNewton r(c.xtol);
      r.Perform(f, c.b1, c.b2, c.nbit);
      os_text(dump_of([&](std::ostream& o) { r.Dump(o); }));
    });
  }
}

// =================================================================================
// integration: one-variable integrands
// =================================================================================
// polynomials of known degree, exp, trigonometric sums, kinks (abs, flat) and the others of math1d
static FnSpec integrand(double lo, double hi) {
  double q = rnd(0, 1);
  int sm = rstate();
  if (q < 0.30) { int d = rint(0, 8); Vd c; for (int i = 0; i <= d; ++i) c.push_back(rnd(-3, 3)); if (c.size() == 1 && coin(0.5)) c.push_back(0.0); return mk(K_POLY, c, sm); }
  if (q < 0.45) return mk(K_EXP, Vd{rnz(0.1, 2.0), rnd(-1, 1)}, sm);
  if (q < 0.62) { int nt = rint(1, 3); Vd p; for (int t = 0; t < nt; ++t) { p.push_back(rnz(0.2, 3.0)); p.push_back(coin(0.4) ? (double)rint(1, 8) : rnd(0.3, 12.0)); p.push_back(rnd(0, 6.2831853)); } return mk(K_TRIG, p, sm); }
  if (q < 0.74) return mk(K_ABS, Vd{rnz(0.3, 3.0), coin(0.8) ? rnd(lo, hi) : 0.5 * (lo + hi), rnd(-1, 1)}, sm);
  if (q < 0.82) { double a = rnd(lo, hi), b = rnd(lo, hi); if (a > b) std::swap(a, b); return mk(K_FLAT, Vd{a, b, rnz(0.2, 3.0), rnz(0.2, 3.0), (double)rint(1, 3), rnd(-1, 1)}, sm); }
  return fn_rootful(lo, hi, sm);
}
static void rint_interval(double& a, double& b) {
  double q = rnd(0, 1);
  if (q < 0.55) { a = rnd(-3, 1); b = a + rnd(0.1, 4); }
  else if (q < 0.68) { b = rnd(-3, 1); a = b + rnd(0.1, 4); }     // reversed
  else if (q < 0.74) { a = b = rnd(-2, 2); }                       // empty
  else if (q < 0.80) { a = rnd(-1, 1); b = a + rnd(1e-10, 1e-6); } // tiny
  else if (q < 0.88) { a = 0; b = 1; }
  else if (q < 0.94) { a = -1; b = 1; }
  else { a = rnz(10, 1e3); b = a + rnd(1, 100); }
}
static int rorder() {
  double q = rnd(0, 1);
  if (q < 0.55) return rint(1, 12);
  if (q < 0.75) return rint(13, 61);
  if (q < 0.85) return pickli({61, 62, 70, 100});
  return pickli({1, 2, 0, -1});
}

// =================================================================================
// math_GaussSingleIntegration
// =================================================================================
struct GICase { FnSpec fn; double a, b; int order; double tol; int variant; };
static GICase rand_gi(int att, bool with_tol) {
  GICase c;
  rint_interval(c.a, c.b);
  double lo, hi; lohi(c.a, c.b, lo, hi);
  c.fn = integrand(lo, hi);
  add_fail(c.fn, lo, hi, 0.1);
  c.order = rorder();
  c.tol = pickl({0.0, 1e-15, 1e-12, 1e-9, 1e-6, 1e-3, 0.1, 1.0, 100.0});
  if (with_tol && c.order > 8) c.order = rint(1, 8);
  if (with_tol && c.tol < 1e-9 && coin(0.7)) c.tol = 1e-6;
  c.variant = att % 3;
  if (att < 4) { c.fn = mk(K_POLY, Vd{1.0, 2.0, 3.0}, 0); c.a = 0; c.b = 1; c.order = att == 0 ? 2 : att == 1 ? 1 : att == 2 ? 61 : 100; }
  return c;
}
static void fam_gauss_single() {
  const char* N = "gauss_single";
  for (int att = 0; att < 6000 && fam_lines[N] < 80; ++att) {
    GICase c = rand_gi(att, false);
    if (c.order < 1 && coin(0.5)) c.order = 1;
    begin(N, "tf f f i"); W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.order); bar();
    emit([&] {
      TF f(c.fn);
      math_GaussSingleIntegration g(f, c.a, c.b, c.order);
      ob(g.IsDone()); out_counts(f);
      if (g.IsDone()) h(g.Value());
    });
  }
}
static void fam_gauss_single_tol() {
  const char* N = "gauss_single_tol";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    GICase c = rand_gi(att, true);
    begin(N, "tf f f i f"); W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.order); W_f(c.tol); bar();
    emit([&] {
      TF f(c.fn);
      math_GaussSingleIntegration g(f, c.a, c.b, c.order, c.tol);
      ob(g.IsDone()); out_counts(f);
      if (g.IsDone()) h(g.Value());
    });
  }
}
static void run_gi(const GICase& c, TF& f, const std::function<void(math_GaussSingleIntegration&)>& use) {
  if (c.variant == 0) { math_GaussSingleIntegration g; use(g); }
  else if (c.variant == 1) { math_GaussSingleIntegration g(f, c.a, c.b, c.order); use(g); }
  else { math_GaussSingleIntegration g(f, c.a, c.b, c.order, c.tol); use(g); }
}
static void fam_gauss_single_access() {
  const char* N = "gauss_single_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 30; ++att) {
    GICase c = rand_gi(att, true);
    begin(N, "i tf f f i f"); W_i(c.variant); W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.order); W_f(c.tol); bar();
    emit([&] { TF f(c.fn); run_gi(c, f, [&](math_GaussSingleIntegration& g) { h(g.Value()); }); });
  }
}
static void fam_gauss_single_dump() {
  const char* N = "gauss_single_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    GICase c = rand_gi(att, true);
    begin(N, "i tf f f i f"); W_i(c.variant); W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.order); W_f(c.tol); bar();
    emit([&] { TF f(c.fn); run_gi(c, f, [&](math_GaussSingleIntegration& g) { os_text(dump_of([&](std::ostream& o) { g.Dump(o); })); }); });
  }
}

// =================================================================================
// math_GaussMultipleIntegration
// =================================================================================
struct GMCase { NSpec fn; VecIn lo, up; Vi ord; };
static GMCase rand_gm(int att) {
  GMCase c;
  int n = coin(0.4) ? 1 : (coin(0.6) ? 2 : 3);
  if (att < 6) n = 1 + att % 3;
  c.fn = any_mv(n);
  Vd a(n), b(n);
  for (int j = 0; j < n; ++j) rint_interval(a[j], b[j]);
  c.lo = vin(a, rlower()); c.up = vin(b, rlower());
  int budget = 3000;
  for (int j = 0; j < n; ++j) {
    int o = rorder();
    if (o > 0) { int cap = std::max(1, budget / std::max(1, (int)std::pow(8.0, n - 1 - j))); if (std::min(o, 61) > cap) o = rint(1, cap); }
    if (o < 0) o = 0;
    c.ord.push_back(o);
    budget = std::max(1, budget / std::max(1, std::min(std::max(o, 1), 61)));
  }
  Vd mid(n); for (int j = 0; j < n; ++j) mid[j] = 0.5 * (a[j] + b[j]);
  maybe_fail(c.fn, mid, 0.4, 0.1, 1);
  return c;
}
static void W_gm(const GMCase& c) { W_mv(c.fn); W_v(c.lo); W_v(c.up); W_iv(1, c.ord); }
static void fam_gauss_multiple() {
  const char* N = "gauss_multiple";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    GMCase c = rand_gm(att);
    begin(N, "mv v v iv"); W_gm(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_GaussMultipleIntegration g(f, mkv(c.lo), mkv(c.up), mkiv(1, c.ord));
      ob(g.IsDone()); out_mv(f);
      if (g.IsDone()) h(g.Value());
    });
  }
}
static void fam_gauss_multiple_access() {
  const char* N = "gauss_multiple_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    GMCase c = rand_gm(att);
    begin(N, "mv v v iv"); W_gm(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_GaussMultipleIntegration g(f, mkv(c.lo), mkv(c.up), mkiv(1, c.ord));
      h(g.Value());
    });
  }
}
static void fam_gauss_multiple_dump() {
  const char* N = "gauss_multiple_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    GMCase c = rand_gm(att);
    begin(N, "mv v v iv"); W_gm(c); bar();
    emit([&] {
      MVF f(c.fn);
      math_GaussMultipleIntegration g(f, mkv(c.lo), mkv(c.up), mkiv(1, c.ord));
      os_text(dump_of([&](std::ostream& o) { g.Dump(o); }));
    });
  }
}

// =================================================================================
// math_GaussSetIntegration
// =================================================================================
struct GSCase { NSpec fn; VecIn lo, up; int olow; Vi ord; };
static GSCase rand_gs(int att) {
  GSCase c;
  int n = coin(0.93) ? 1 : 2;
  int neq = rint(1, 4);
  Vd r;
  c.fn = fs_rand(n, neq, r);
  Vd a(n), b(n);
  for (int j = 0; j < n; ++j) rint_interval(a[j], b[j]);
  c.lo = vin(a, rlower()); c.up = vin(b, rlower());
  int o = rorder();
  c.ord.push_back(o);
  if (coin(0.15)) c.ord.push_back(rint(1, 5));
  c.olow = rlower();
  Vd mid(n); for (int j = 0; j < n; ++j) mid[j] = 0.5 * (a[j] + b[j]);
  maybe_fail(c.fn, mid, 0.4, 0.1, 1);
  (void)att;
  return c;
}
static void W_gs(const GSCase& c) { W_fs(c.fn); W_v(c.lo); W_v(c.up); W_iv(c.olow, c.ord); }
// the Standard_NotImplemented check (one variable, an order up to GaussPointsMax) is compiled out of the library
static bool gs_bad(const GSCase& c) { return c.fn.n != 1 || c.ord[0] > math::GaussPointsMax(); }
static void fam_gauss_set() {
  const char* N = "gauss_set";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    GSCase c = rand_gs(att);
    begin(N, "fs v v iv"); W_gs(c); bar();
    emit([&] {
      if (gs_bad(c)) { unchecked(); return; }
      FSF f(c.fn);
      math_GaussSetIntegration g(f, mkv(c.lo), mkv(c.up), mkiv(c.olow, c.ord));
      ob(g.IsDone()); out_fs(f);
      if (g.IsDone()) ov(g.Value());
    });
  }
}
static void fam_gauss_set_access() {
  const char* N = "gauss_set_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    GSCase c = rand_gs(att);
    begin(N, "fs v v iv"); W_gs(c); bar();
    emit([&] {
      if (gs_bad(c)) { unchecked(); return; }
      FSF f(c.fn);
      math_GaussSetIntegration g(f, mkv(c.lo), mkv(c.up), mkiv(c.olow, c.ord));
      ov(g.Value());
    });
  }
}
static void fam_gauss_set_dump() {
  const char* N = "gauss_set_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 24; ++att) {
    GSCase c = rand_gs(att);
    begin(N, "fs v v iv"); W_gs(c); bar();
    emit([&] {
      if (gs_bad(c)) { unchecked(); return; }
      FSF f(c.fn);
      math_GaussSetIntegration g(f, mkv(c.lo), mkv(c.up), mkiv(c.olow, c.ord));
      os_text(dump_of([&](std::ostream& o) { g.Dump(o); }));
    });
  }
}

// =================================================================================
// math_KronrodSingleIntegration
// =================================================================================
struct KRCase { FnSpec fn; double a, b; int n; double tol; int maxit; };
static int rkpoints() {
  double q = rnd(0, 1);
  if (q < 0.5) return pickli({3, 4, 5, 7, 9, 10, 11, 15, 21});
  if (q < 0.8) return rint(3, 61);
  if (q < 0.9) return pickli({121, 122, 123, 124, 125, 127});
  return pickli({0, 1, 2, -3});
}
static KRCase rand_kr(int att) {
  KRCase c;
  rint_interval(c.a, c.b);
  double lo, hi; lohi(c.a, c.b, lo, hi);
  c.fn = integrand(lo, hi);
  add_fail(c.fn, lo, hi, 0.08);
  c.n = rkpoints();
  c.tol = pickl({0.0, -1.0, 1e-15, 1e-12, 1e-9, 1e-6, 1e-3, 0.1, 10.0});
  c.maxit = pickli({0, 1, 2, 3, 5, 10, 20, 50});
  if (att < 3) { c.fn = mk(K_ABS, Vd{1.0, 0.3, 0.0}, 0); c.a = 0; c.b = 1; c.n = 7; c.tol = 1e-10; c.maxit = 50; }
  if (att == 1) c.n = 4;
  if (att == 2) { c.fn = mk(K_POLY, Vd{1.0, 0.0, 0.0, 4.0}, 1); c.tol = 1e-12; }
  return c;
}
static void out_kr(math_KronrodSingleIntegration& k, TF& f) {
  ob(k.IsDone()); out_counts(f);
  if (k.IsDone()) { h(k.Value()); h(k.ErrorReached()); h(k.AbsolutError()); in(k.OrderReached()); in(k.NbIterReached()); }
}
static void fam_kronrod() {
  const char* N = "kronrod";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    KRCase c = rand_kr(att);
    begin(N, "tf f f i"); W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.n); bar();
    emit([&] { TF f(c.fn); math_KronrodSingleIntegration k(f, c.a, c.b, c.n); out_kr(k, f); }, true);
  }
}
static void fam_kronrod_tol() {
  const char* N = "kronrod_tol";
  for (int att = 0; att < 6000 && fam_lines[N] < 70; ++att) {
    KRCase c = rand_kr(att);
    begin(N, "tf f f i f i"); W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.n); W_f(c.tol); W_i(c.maxit); bar();
    emit([&] { TF f(c.fn); math_KronrodSingleIntegration k(f, c.a, c.b, c.n, c.tol, c.maxit); out_kr(k, f); }, true);
  }
}
static void fam_kronrod_perform() {
  const char* N = "kronrod_perform";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    KRCase c = rand_kr(att), d = rand_kr(att + 1000);
    int v1 = rint(0, 1), v2 = rint(-1, 1);
    begin(N, "tf i f f i f i i f f i f i"); W_tf(c.fn); W_i(v1); W_f(c.a); W_f(c.b); W_i(c.n); W_f(c.tol); W_i(c.maxit);
    W_i(v2); W_f(d.a); W_f(d.b); W_i(d.n); W_f(d.tol); W_i(d.maxit); bar();
    emit([&] {
      TF f(c.fn);
      math_KronrodSingleIntegration k;
      if (v1 == 0) k.Perform(f, c.a, c.b, c.n); else k.Perform(f, c.a, c.b, c.n, c.tol, c.maxit);
      if (v2 == 0) k.Perform(f, d.a, d.b, d.n); else if (v2 == 1) k.Perform(f, d.a, d.b, d.n, d.tol, d.maxit);
      out_kr(k, f);
    }, true);
  }
}
static void fam_kronrod_access() {
  const char* N = "kronrod_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 40; ++att) {
    KRCase c = rand_kr(att);
    int v = att % 3, q = (att / 3) % 6;
    begin(N, "tf i f f i f i i"); W_tf(c.fn); W_i(v); W_f(c.a); W_f(c.b); W_i(c.n); W_f(c.tol); W_i(c.maxit); W_i(q); bar();
    emit([&] {
      TF f(c.fn);
      std::unique_ptr<math_KronrodSingleIntegration> k(v == 0 ? new math_KronrodSingleIntegration()
                                                       : v == 1 ? new math_KronrodSingleIntegration(f, c.a, c.b, c.n)
                                                                : new math_KronrodSingleIntegration(f, c.a, c.b, c.n, c.tol, c.maxit));
      switch (q) {
        case 0: h(k->Value()); break;
        case 1: h(k->ErrorReached()); break;
        case 2: h(k->AbsolutError()); break;
        case 3: in(k->OrderReached()); break;
        case 4: in(k->NbIterReached()); break;
        default: ob(k->IsDone()); break;
      }
    }, true);
  }
}
static void fam_kronrod_gk_rule() {
  const char* N = "kronrod_gk_rule";
  for (int att = 0; att < 6000 && fam_lines[N] < 20; ++att) {
    KRCase c = rand_kr(att);
    int nk = coin(0.8) ? pickli({3, 5, 7, 9, 11}) : pickli({15, 21});
    math_Vector kp(1, nk), kw(1, nk), gp(1, nk / 2), gw(1, nk / 2);
    if (!math::KronrodPointsAndWeights(nk, kp, kw) || !math::OrderedGaussPointsAndWeights(nk / 2, gp, gw)) std::abort();
    Vd KP, KW, GP, GW;
    for (int i = 1; i <= nk; ++i) { KP.push_back(kp(i)); KW.push_back(kw(i)); }
    for (int i = 1; i <= nk / 2; ++i) { GP.push_back(gp(i)); GW.push_back(gw(i)); }
    begin(N, "tf f f v v v v"); W_tf(c.fn); W_f(c.a); W_f(c.b); W_v(vin(GP)); W_v(vin(GW)); W_v(vin(KP)); W_v(vin(KW)); bar();
    emit([&] {
      TF f(c.fn);
      double val = 0.0, err = 0.0;
      bool ok = math_KronrodSingleIntegration::GKRule(f, c.a, c.b, mkv(vin(GP)), mkv(vin(GW)), mkv(vin(KP)), mkv(vin(KW)), val, err);
      ob(ok); out_counts(f);
      if (ok) { h(val); h(err); }
    }, true);
  }
}

// =================================================================================
#include <csignal>
static void on_crash(int sig) {
  const char* m = "oracle crashed in family ";
  (void)!write(2, m, std::strlen(m));
  (void)!write(2, cur_name.c_str(), cur_name.size());
  (void)!write(2, "\n", 1);
  (void)!write(2, buf.c_str(), buf.size());
  (void)!write(2, "\n", 1);
  _exit(100 + sig);
}
struct Fam { const char* group; const char* name; void (*fn)(); };
static const Fam FAMS[] = {
  {"test", "test_tf", fam_test_tf}, {"test", "test_mv", fam_test_mv}, {"test", "test_fs", fam_test_fs},
  {"fsr", "function_set_root", fam_function_set_root}, {"fsr", "function_set_root_bounded", fam_function_set_root_bounded},
  {"fsr", "function_set_root_staged", fam_function_set_root_staged}, {"fsr", "function_set_root_custom", fam_function_set_root_custom},
  {"fsr", "function_set_root_access", fam_function_set_root_access}, {"fsr", "function_set_root_dump", fam_function_set_root_dump},
  {"nsr", "newton_set_root", fam_newton_set_root}, {"nsr", "newton_set_root_bounded", fam_newton_set_root_bounded},
  {"nsr", "newton_set_root_staged", fam_newton_set_root_staged}, {"nsr", "newton_set_root_custom", fam_newton_set_root_custom},
  {"nsr", "newton_set_root_access", fam_newton_set_root_access}, {"nsr", "newton_set_root_dump", fam_newton_set_root_dump},
  {"nm", "newton_minimum", fam_newton_minimum}, {"nm", "newton_minimum_bounded", fam_newton_minimum_bounded},
  {"nm", "newton_minimum_custom", fam_newton_minimum_custom}, {"nm", "newton_minimum_access", fam_newton_minimum_access},
  {"nm", "newton_minimum_dump", fam_newton_minimum_dump},
  {"bfgs", "bfgs", fam_bfgs}, {"bfgs", "bfgs_bounded", fam_bfgs_bounded}, {"bfgs", "bfgs_custom", fam_bfgs_custom},
  {"bfgs", "bfgs_access", fam_bfgs_access}, {"bfgs", "bfgs_dump", fam_bfgs_dump},
  {"frpr", "frpr", fam_frpr}, {"frpr", "frpr_custom", fam_frpr_custom}, {"frpr", "frpr_access", fam_frpr_access},
  {"frpr", "frpr_dump", fam_frpr_dump},
  {"powell", "powell", fam_powell}, {"powell", "powell_custom", fam_powell_custom}, {"powell", "powell_access", fam_powell_access},
  {"powell", "powell_dump", fam_powell_dump},
  {"uzawa", "uzawa", fam_uzawa}, {"uzawa", "uzawa_ineq", fam_uzawa_ineq}, {"uzawa", "uzawa_access", fam_uzawa_access},
  {"uzawa", "uzawa_dump", fam_uzawa_dump},
  {"bissec", "bissec_newton", fam_bissec_newton}, {"bissec", "bissec_newton_custom", fam_bissec_newton_custom},
  {"bissec", "bissec_newton_access", fam_bissec_newton_access}, {"bissec", "bissec_newton_dump", fam_bissec_newton_dump},
  {"gsi", "gauss_single", fam_gauss_single}, {"gsi", "gauss_single_tol", fam_gauss_single_tol},
  {"gsi", "gauss_single_access", fam_gauss_single_access}, {"gsi", "gauss_single_dump", fam_gauss_single_dump},
  {"gmi", "gauss_multiple", fam_gauss_multiple}, {"gmi", "gauss_multiple_access", fam_gauss_multiple_access},
  {"gmi", "gauss_multiple_dump", fam_gauss_multiple_dump},
  {"gset", "gauss_set", fam_gauss_set}, {"gset", "gauss_set_access", fam_gauss_set_access}, {"gset", "gauss_set_dump", fam_gauss_set_dump},
  {"kronrod", "kronrod", fam_kronrod}, {"kronrod", "kronrod_tol", fam_kronrod_tol}, {"kronrod", "kronrod_perform", fam_kronrod_perform},
  {"kronrod", "kronrod_access", fam_kronrod_access}, {"kronrod", "kronrod_gk_rule", fam_kronrod_gk_rule},
};
int main(int argc, char** argv) {
  signal(SIGSEGV, on_crash); signal(SIGABRT, on_crash); signal(SIGFPE, on_crash);
  alarm(3000);
  init_tables();
  const char* only = argc > 1 ? argv[1] : nullptr;
  auto want = [&](const char* g) {
    if (!only) return true;
    std::string o = std::string(",") + only + ",", k = std::string(",") + g + ",";
    return o.find(k) != std::string::npos;
  };
  // every family draws from its own generator state, so that a family's lines do not change when another changes
  uint64_t seed = 0x9E3779B97F4A7C15ull;
  for (const Fam& f : FAMS) {
    seed = seed * 6364136223846793005ull + 1442695040888963407ull;
    state = seed | 1;
    if (want(f.group)) f.fn();
  }
  for (const Fam& f : FAMS) {
    std::string n = f.name;
    if (only && !fam_lines.count(n) && !fam_dropped.count(n)) continue;
    std::fprintf(stderr, "%-28s %5d lines, %5d dropped (%d uninit)\n", f.name, fam_lines.count(n) ? fam_lines[n] : 0,
                 fam_dropped.count(n) ? fam_dropped[n] : 0, fam_dropped.count(n + "!uninit") ? fam_dropped[n + "!uninit"] : 0);
    if (fam_dropped.count(n + "!libm") || fam_dropped.count(n + "~changed"))
      std::fprintf(stderr, "   libm-perturbed: %d dropped, %d (case, salt) runs changed bits\n", fam_dropped[n + "!libm"], fam_dropped[n + "~changed"]);
  }
  return 0;
}
