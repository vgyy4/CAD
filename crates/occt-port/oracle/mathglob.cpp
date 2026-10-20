// Deterministic reference calls for OCCT 8.0.1's global optimizers of the `math` package: math_GlobOptMin (every
// constructor and setter, SetGlobalParams, SetLocalParams, SetTol / GetTol, SetContinuity, SetFunctionalMinimalValue,
// SetLipConstState, Perform with and without isFindSingleSolution, Points, NbExtrema, GetF, isDone and the getters),
// math_PSO (constructor, both Perform overloads), math_PSOParticlesPool and math_BullardGenerator (the random number
// generator of PSO).
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints (and bools) are decimal. Every input of a call is printed on the
// left of " | " in the order named by the family's schema string (`begin(name, schema)`); tests/mathglob.rs reads them
// back with the reader method of the same name as each schema word. The oracle aborts if the words it printed differ
// from the schema (and if a word printed the wrong number of tokens). Outputs are values, `throws` (OCCT raised an
// exception that the line's call really raises) or `unchecked` (OCCT 8.0.1 as built has no check, so the numbers it
// would return are unspecified, or the call would corrupt memory; the Rust API must return an error).
//
// Schema words (token counts):
//   f   a float (1)                       i   an int or bool (1)
//   v   a math_Vector: lower length x_1 .. x_length (2 + length)
//   mv  a multiple-variable test function: n mode ng g_1 .. g_ng fail_mode lo_1 .. lo_n hi_1 .. hi_n state_mode
//       where every g is `gkind nparams p_1 .. p_nparams` (see THE SCALAR TERMS below).
//   pool a particle pool: count dim, then for every particle 1..count: position (dim floats), velocity (dim),
//       best position (dim), distance, best distance (3 * dim + 2 floats)
//
// THE TEST FUNCTIONS. The optimizers call user functions; the judge cannot pass closures through text, so every
// function is fully described by its record and implemented identically here and in tests/mathglob.rs, with
// + - * / floor and fabs only (no libm), so that both sides see the same bits. The functions of several variables are
// built from scalar terms g(x), x = (x_0 .. x_{m-1}) (x_j is the vector element at position j, i.e. X(X.Lower() + j)):
//   gkind 0 QUAD   p = c, b_0..b_{m-1}, A_00 A_01 .. A_{m-1,m-1} (row major):
//                  g = c + sum_i x_i (b_i + 0.5 sum_j A_ij x_j); dg_i = b_i + 0.5 sum_j (A_ij + A_ji) x_j;
//                  d2g_ij = 0.5 (A_ij + A_ji)
//   gkind 1 ROSEN  p = a, b: m = 1: (a - x_0)^2; else sum_{i<m-1} b (x_{i+1} - x_i^2)^2 + (a - x_i)^2
//   gkind 2 POLY   p = nterms, then per term: coef, e_0 .. e_{m-1} (small integers): g = sum coef prod x_k^e_k
//   gkind 3 TRIG   p = c, then per variable A W P B: g = c + sum_j A sinp(W x_j + P) + B x_j^2
//   gkind 4 KINK   p = c, then per variable a k: g = c + sum_j a |x_j - k|
//   gkind 5 EXP    p = c, a, w_0 .. w_{m-1}: g = c + a expp(sum_j w_j x_j)
// (the exact evaluation orders are the code's: g_value, g_grad, g_hess). An `mv` function is, by mode, the sum of its
// terms (mode 0) or the sum of their squares (mode 1: f = sum g_k^2, grad = sum 2 g_k dg_k,
// hess = sum 2 (dg_k dg_k^T + g_k d2g_k)). A function FAILS (the callback returns false and writes nothing) when
// lo_j <= x_j <= hi_j for every j and the bit of the callback is set in fail_mode: 1 = Value, 2 = Gradient,
// 4 = Values(F, G), 8 = Values(F, G, H). GetStateNumber returns, by state_mode: 0 -> 0; 1 -> the number of calls so
// far; 2 -> 4 * (that number) + a code of the last call (1 Value, 2 Gradient, 3 Values, 4 Values with Hessian).
//
// WHICH INTERFACE THE OPTIMIZER SEES. math_GlobOptMin decides with dynamic_cast what the function can do (Hessian ->
// Newton, gradient -> BFGS, always Powell). An `mv` record is therefore always accompanied by a `level`: 0 = the
// object is only a math_MultipleVarFunction (class MVP), 1 = a math_MultipleVarFunctionWithGradient (MVG), 2 = a
// math_MultipleVarFunctionWithHessian (MVH). The three classes forward to the same implementation, so the counters
// and the trace of the function are shared. The construction families print the callback counts (nvalue ngradient nvalues
// nhessian nstate) and a 64-bit FNV-1a hash of the call trace: per call a tag byte ('V' 'G' 'W' 'H' 'S'), then for the
// calls with an argument the 4 bytes of X.Lower(), the 4 bytes of X.Length() (little endian, two's complement) and the
// 8 bytes of every element in order. A test function checks the dimensions of the vectors and matrices it is given (X
// of length n, G of length n, H n x n); the oracle drops a case in which OCCT passes others (the judge panics) or in
// which a callback sees or produces a non-finite number or the run needs more than 300000 callback calls.
//
// Cases for which the C++ result depends on uninitialised memory are detected by running every call four times with
// differently, randomly filled stack and heap and are NOT printed.
//
// The state of a math_GlobOptMin after a call (`out_glob`): isDone, NbExtrema, GetF, GetTol (2), GetContinuity,
// GetFunctionalMinimalValue, GetLipConstState, then Points(i) for the first min(3, NbExtrema) solutions (each as a
// vector, lower length x...), then the FNV-1a hash of the 8 bytes (little endian) of every coordinate of every
// solution 1..NbExtrema in order, then the callback counts and the trace hash of the function that was used.

#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <math_BullardGenerator.hxx>
#include <math_GlobOptMin.hxx>
#include <math_Matrix.hxx>
#include <math_MultipleVarFunction.hxx>
#include <math_MultipleVarFunctionWithGradient.hxx>
#include <math_MultipleVarFunctionWithHessian.hxx>
#include <math_PSO.hxx>
#include <math_PSOParticlesPool.hxx>
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


static const uint64_t FNV0 = 0xcbf29ce484222325ull, FNVP = 0x100000001b3ull;
static double pickl(std::initializer_list<double> l) { int k = rint(0, (int)l.size() - 1); return *(l.begin() + k); }
static int pickli(std::initializer_list<int> l) { int k = rint(0, (int)l.size() - 1); return *(l.begin() + k); }
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
  if (std::getenv("MATHGLOB_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
struct VecIn { int lower = 1; Vd v; };
static VecIn vin(const Vd& v, int lower = 1) { VecIn r; r.lower = lower; r.v = v; return r; }
static void W_v(const VecIn& v) { int st = ntok; in(v.lower); in((long)v.v.size()); for (double x : v.v) h(x); word("v", st, 2 + (int)v.v.size()); }
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


// ---------------------------------------------------------------------------------
// the test function at the three interface levels (shared counters and trace)
struct MVCore {
  NSpec s;
  long nv = 0, ng = 0, nw = 0, nh = 0, ns = 0;
  Tracer t;
  explicit MVCore(const NSpec& spec) : s(spec) {}
  void begin_call(char tag, const math_Vector& X, double* x, int code) {
    t.tagv(tag, X); t.last = code;
    if (++g_calls > MAX_CALLS) throw Discard();
    ++t.calls;
    get_x(s, X, x);
  }
  bool fails(const double* x, int bit) const { return (s.fmode & bit) && in_box(s, x); }
  bool Value(const math_Vector& X, double& F) {
    double x[8]; ++nv; begin_call('V', X, x, 1);
    if (fails(x, 1)) return false;
    double v = mv_value(s, x); fin(v);
    F = v;
    return true;
  }
  bool Gradient(const math_Vector& X, math_Vector& G) {
    double x[8]; ++ng; begin_call('G', X, x, 2);
    if (G.Length() != s.n) throw Discard();
    if (fails(x, 2)) return false;
    double gr[8]; mv_grad(s, x, gr);
    for (int j = 0; j < s.n; ++j) fin(gr[j]);
    for (int j = 0; j < s.n; ++j) G(G.Lower() + j) = gr[j];
    return true;
  }
  bool Values(const math_Vector& X, double& F, math_Vector& G) {
    double x[8]; ++nw; begin_call('W', X, x, 3);
    if (G.Length() != s.n) throw Discard();
    if (fails(x, 4)) return false;
    double v = mv_value(s, x), gr[8]; mv_grad(s, x, gr);
    fin(v); for (int j = 0; j < s.n; ++j) fin(gr[j]);
    F = v;
    for (int j = 0; j < s.n; ++j) G(G.Lower() + j) = gr[j];
    return true;
  }
  bool ValuesH(const math_Vector& X, double& F, math_Vector& G, math_Matrix& H) {
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
  int State() {
    ++ns; t.tag('S');
    if (s.smode == 0) return 0;
    if (s.smode == 1) return (int)t.calls;
    return (int)(4 * t.calls + t.last);
  }
};
class MVP : public math_MultipleVarFunction {
public:
  MVCore& c;
  explicit MVP(MVCore& core) : c(core) {}
  int NbVariables() const override { return c.s.n; }
  bool Value(const math_Vector& X, double& F) override { return c.Value(X, F); }
  int GetStateNumber() override { return c.State(); }
};
class MVG : public math_MultipleVarFunctionWithGradient {
public:
  MVCore& c;
  explicit MVG(MVCore& core) : c(core) {}
  int NbVariables() const override { return c.s.n; }
  bool Value(const math_Vector& X, double& F) override { return c.Value(X, F); }
  bool Gradient(const math_Vector& X, math_Vector& G) override { return c.Gradient(X, G); }
  bool Values(const math_Vector& X, double& F, math_Vector& G) override { return c.Values(X, F, G); }
  int GetStateNumber() override { return c.State(); }
};
class MVH : public math_MultipleVarFunctionWithHessian {
public:
  MVCore& c;
  explicit MVH(MVCore& core) : c(core) {}
  int NbVariables() const override { return c.s.n; }
  bool Value(const math_Vector& X, double& F) override { return c.Value(X, F); }
  bool Gradient(const math_Vector& X, math_Vector& G) override { return c.Gradient(X, G); }
  bool Values(const math_Vector& X, double& F, math_Vector& G) override { return c.Values(X, F, G); }
  bool Values(const math_Vector& X, double& F, math_Vector& G, math_Matrix& H) override { return c.ValuesH(X, F, G, H); }
  int GetStateNumber() override { return c.State(); }
};
struct MVF {
  MVCore c;
  MVP p;
  MVG g;
  MVH hs;
  explicit MVF(const NSpec& spec) : c(spec), p(c), g(c), hs(c) {}
  math_MultipleVarFunction* at(int level) {
    if (level == 0) return &p;
    if (level == 1) return &g;
    return &hs;
  }
};
static void out_mv(const MVF& f) { in(f.c.nv); in(f.c.ng); in(f.c.nw); in(f.c.nh); in(f.c.ns); hx(f.c.t.trace); }

// ---------------------------------------------------------------------------------
// running a case: the harness of the other oracles, without the libm perturbation (everything here is + - * / sqrt)
static bool emit(const std::function<void()>& body) {
  size_t keep_ntok = (size_t)ntok;
  std::string r1;
  for (int k = 0; k < POISON_RUNS; ++k) {
    bool dk = false;
    std::string rk = run_once(body, k, dk);
    ntok = (int)keep_ntok;
    if (dk) { fam_dropped[cur_name]++; buf.clear(); return false; }
    if (k == 0) r1 = rk;
    else if (rk != r1) {
      if (std::getenv("MATHGLOB_DEBUG")) std::fprintf(stderr, "UNINIT\n%s\n%s\n", r1.c_str(), rk.c_str());
      fam_dropped[cur_name]++; fam_dropped[cur_name + "!uninit"]++; buf.clear(); return false;
    }
  }
  buf = r1;
  finish();
  return true;
}

// ---------------------------------------------------------------------------------
// printing the state of a math_GlobOptMin
static void fold_hash(uint64_t& hsh, double v) {
  uint64_t b; std::memcpy(&b, &v, 8);
  for (int k = 0; k < 8; ++k) { hsh ^= (uint8_t)((b >> (8 * k)) & 0xff); hsh *= FNVP; }
}
static void out_glob(math_GlobOptMin& g, int n, const MVF& f, bool brief = false) {
  ob(g.isDone());
  int nb = g.NbExtrema();
  in(nb); h(g.GetF());
  if (!brief) {
    double dt = 0, st = 0;
    g.GetTol(dt, st); h(dt); h(st);
    in(g.GetContinuity()); h(g.GetFunctionalMinimalValue()); ob(g.GetLipConstState());
  }
  math_Vector p(1, n);
  uint64_t hsh = FNV0;
  for (int i = 1; i <= nb; ++i) {
    g.Points(i, p);
    for (int j = 1; j <= n; ++j) fold_hash(hsh, p(j));
    if (!brief && i <= 3) ov(p);
  }
  hx(hsh);
  out_mv(f);
}

// ---------------------------------------------------------------------------------
// generating the global optimization cases
static int rlevel() { double q = rnd(0, 1); return q < 0.3 ? 0 : q < 0.5 ? 1 : 2; }
static int rdim_glob() { double q = rnd(0, 1); return q < 0.28 ? 1 : q < 0.65 ? 2 : q < 0.85 ? 3 : 4; }

// sum_j (x_j^2 - a_j^2)^2: minima at (+-a_1, ..., +-a_n), all of the same value 0
static GSpec g_double_well(int m) {
  Vd p; p.push_back(0);
  int nt = 0;
  double c0 = 0.0;
  for (int j = 0; j < m; ++j) {
    double a = rnd(0.5, 1.5), a2 = a * a;
    p.push_back(1.0); for (int k = 0; k < m; ++k) p.push_back(k == j ? 4 : 0); ++nt;
    p.push_back(-2.0 * a2); for (int k = 0; k < m; ++k) p.push_back(k == j ? 2 : 0); ++nt;
    c0 += a2 * a2;
  }
  p.push_back(c0); for (int k = 0; k < m; ++k) p.push_back(0); ++nt;
  p[0] = nt;
  return mkg(G_POLY, p);
}
// sum_j A_j sin(W_j x_j + P_j): periodic, equal minima at distances 2 pi / W_j
static GSpec g_periodic(int m) {
  Vd p; p.push_back(rnd(-1, 1));
  for (int j = 0; j < m; ++j) {
    p.push_back(rnd(0.4, 1.5));
    p.push_back(coin(0.6) ? (double)rint(1, 3) : rnd(0.5, 2.5));
    p.push_back(rnd(0, 6.2831853));
    p.push_back(0.0);
  }
  return mkg(G_TRIG, p);
}
// a constant: every point is a minimum
static GSpec g_flat(int m) {
  Vd p; p.push_back(1);
  p.push_back(rnd(-2, 2));
  for (int k = 0; k < m; ++k) p.push_back(0);
  return mkg(G_POLY, p);
}
// a function to minimise: unimodal, multimodal, with equal minima, flat, with a kink, without a minimum
static NSpec glob_fn(int n, Vd& xmin) {
  double q = rnd(0, 1);
  if (q < 0.42) return mv_min(n, xmin);
  xmin = rvec(n, -1.5, 1.5);
  std::vector<GSpec> g;
  int mode = 0;
  if (q < 0.56) g.push_back(g_double_well(n));
  else if (q < 0.68) g.push_back(g_periodic(n));
  else if (q < 0.76) g.push_back(g_flat(n));
  else if (q < 0.86) g.push_back(g_trig(n, false));
  else if (q < 0.92) { g.push_back(g_periodic(n)); g.push_back(g_quad_at(n, rspd(n, 0.3, n), xmin, rnd(-1, 1))); }
  else { g.push_back(g_double_well(n)); mode = 1; }
  NSpec s = nspec(n, mode, g);
  s.smode = rint(0, 2);
  return s;
}
// the search box
static void rbox(int n, const Vd& xmin, Vd& lo, Vd& hi) {
  lo.resize(n); hi.resize(n);
  double q = rnd(0, 1);
  for (int j = 0; j < n; ++j) { lo[j] = xmin[j] - rnd(0.5, 3.0); hi[j] = xmin[j] + rnd(0.5, 3.0); }
  if (q < 0.50) return;
  int j = rint(0, n - 1);
  if (q < 0.60) {                       // centred on the origin
    for (int k = 0; k < n; ++k) { double w = rnd(0.5, 3.0); lo[k] = -w; hi[k] = w; }
  } else if (q < 0.70) {                // the minimum is outside in one coordinate
    lo[j] = xmin[j] + rnd(0.1, 1.0); hi[j] = lo[j] + rnd(0.5, 2.5);
  } else if (q < 0.77) {                // tiny
    for (int k = 0; k < n; ++k) { double w = std::pow(10.0, rnd(-6, -2)); lo[k] = xmin[k] - w; hi[k] = xmin[k] + w; }
  } else if (q < 0.82) {                // huge
    double w = std::pow(10.0, rnd(2, 6));
    for (int k = 0; k < n; ++k) { lo[k] = -w; hi[k] = w; }
  } else if (q < 0.87) {                // degenerate: lower = upper
    hi[j] = lo[j];
  } else if (q < 0.91) {                // reversed
    std::swap(lo[j], hi[j]);
  } else if (q < 0.95) {                // very thin in one coordinate
    hi[j] = lo[j] + pickl({1e-10, 1e-9, 5e-9, 1e-8, 1e-7, 1e-5});
  } else {                              // not centred on anything
    for (int k = 0; k < n; ++k) { lo[k] = rnd(-3, 0); hi[k] = lo[k] + rnd(0.2, 5.0); }
  }
}
struct GCase {
  NSpec fn;
  Vd xmin, lo, hi;
  int n = 1, level = 2;
  double C = 9, dtol = 1e-2, stol = 1e-7;
  int cont = 2;
  double fmin = -2e100;
  int lip = 0, single = 0;
  double dtol2 = 1e-2, stol2 = 1e-7;
};
static double rdtol(int n) {
  if (n == 1) return pickl({1e-2, 1e-2, 2e-2, 5e-3, 1e-3, 2e-3, 0.05, 0.1, 0.3, 1.0, 1e-4, 0.02});
  if (n == 2) return pickl({1e-2, 1e-2, 2e-2, 5e-2, 0.05, 0.1, 0.2, 0.3, 0.5, 5e-3, 1.0});
  return pickl({0.05, 0.1, 0.2, 0.3, 0.5, 1.0, 0.1, 2.0, 0.02, 1e-2});
}
static double rC() { return pickl({9.0, 9.0, 9.0, 1.0, 0.5, 2.0, 20.0, 100.0, 0.01, 1e-3, 1e3, 3.0, 50.0}); }
static double rstol() { return pickl({1e-7, 1e-7, 1e-7, 1e-5, 1e-3, 1e-9, 1e-2, 1e-12, 0.1, 1e-4, 0.0}); }
static int rcont() { return pickli({2, 2, 2, 1, 0, 1, 2, 3, -1, 0}); }
static GCase rand_glob(int att) {
  GCase c;
  c.n = att < 4 ? att + 1 : rdim_glob();
  c.fn = glob_fn(c.n, c.xmin);
  maybe_fail(c.fn, c.xmin, 1.0, 0.12, 15);
  rbox(c.n, c.xmin, c.lo, c.hi);
  c.level = att < 3 ? att : rlevel();
  c.C = rC(); c.dtol = rdtol(c.n); c.stol = rstol();
  c.cont = rcont();
  if (coin(0.35)) {
    double x[8];
    for (int j = 0; j < c.n; ++j) x[j] = c.xmin[j];
    double v = mv_value(c.fn, x);
    c.fmin = std::isfinite(v) ? v + pickl({0.0, 0.0, 1e-10, -1e-3, 1.0}) : -2e100;
  }
  c.lip = coin(0.2) ? 1 : 0;
  c.single = coin(0.4) ? 1 : 0;
  c.dtol2 = coin(0.5) ? c.dtol : rdtol(c.n);
  c.stol2 = coin(0.5) ? c.stol : rstol();
  return c;
}
static void put_glob_head(const GCase& c) {
  W_mv(c.fn); W_i(c.level); W_v(vin(c.lo)); W_v(vin(c.hi)); W_f(c.C); W_f(c.dtol); W_f(c.stol);
}
// the setters in the order of the C++ calls: continuity, functional minimal value, Lipschitz state
static void apply_setters(math_GlobOptMin& g, const GCase& c) {
  g.SetContinuity(c.cont);
  g.SetFunctionalMinimalValue(c.fmin);
  g.SetLipConstState(c.lip != 0);
}

// ---------------------------------------------------------------------------------
// vectors with other bounds than 1..n: the library reads theA(1..n) without a range check
struct VecShape { int lower; int length; };
// the vector `v` re-bounded: `shape.length` elements from `shape.lower`; element i (position) = v[i] when it exists,
// a filler otherwise
static VecIn reshape(const Vd& v, VecShape s, double filler) {
  VecIn r; r.lower = s.lower; r.v.resize(s.length);
  for (int i = 0; i < s.length; ++i) r.v[i] = i < (int)v.size() ? v[i] : filler;
  return r;
}
// the OCCT contract reads indices 1..n: valid iff lower <= 1 and upper >= n
static bool covers(const VecIn& v, int n) { return v.lower <= 1 && v.lower + (int)v.v.size() - 1 >= n; }
static VecShape rshape(int n, bool& bad) {
  double q = rnd(0, 1);
  bad = false;
  if (q < 0.80) return VecShape{1, n};
  if (q < 0.86) return VecShape{1, n + 1};
  if (q < 0.92) return VecShape{0, n + 1};
  bad = true;
  if (q < 0.95) return VecShape{0, n};
  if (q < 0.98) return VecShape{2, n};
  return VecShape{1, std::max(1, n - 1)};
}

// =================================================================================
// the test functions themselves (checks the judge's implementation)
// =================================================================================
static void fam_test_mv() {
  const char* N = "test_mv";
  for (int att = 0; att < 3000 && fam_lines[N] < 40; ++att) {
    int n = att < 12 ? 1 + att % 4 : rdim_glob();
    NSpec s;
    Vd xm;
    if (att < 36) {         // every term kind, alone
      int kind = att % 6;
      GSpec g = kind == 0 ? g_quad_rand(n) : kind == 1 ? g_rosen(rnd(-2, 2), rnd(0.5, 50)) : kind == 2 ? g_poly_rand(n, 4)
                : kind == 3 ? g_trig(n, coin(0.5)) : kind == 4 ? g_kink(n, rvec(n, -2, 2), false) : g_exp(n);
      s = nspec(n, (att / 6) % 2, std::vector<GSpec>{g}); s.smode = rint(0, 2);
    } else {
      s = glob_fn(n, xm);
    }
    Vd x = rvec(n, -2.5, 2.5);
    if (coin(0.4)) set_fail(s, x, 0.5, rint(1, 15));
    int lower = coin(0.8) ? 1 : pickli({0, -2, 3});
    begin(N, "mv v"); W_mv(s); W_v(vin(x, lower)); bar();
    emit([&] {
      MVF f(s);
      math_Vector X = mkv(vin(x, lower));
      double v = 0, v2 = 0, v3 = 0;
      math_Vector G(1, n, 0.0), G2(1, n, 0.0), G3(1, n, 0.0);
      math_Matrix H(1, n, 1, n, 0.0);
      bool o1 = f.c.Value(X, v); ob(o1); if (o1) h(v);
      bool o2 = f.c.Gradient(X, G); ob(o2); if (o2) ov(G);
      bool o3 = f.c.Values(X, v2, G2); ob(o3); if (o3) { h(v2); ov(G2); }
      bool o4 = f.c.ValuesH(X, v3, G3, H); ob(o4); if (o4) { h(v3); ov(G3); om(H); }
      in(f.c.State()); in(f.p.NbVariables());
      out_mv(f);
    });
  }
}

// =================================================================================
// math_GlobOptMin
// =================================================================================
// the constructor alone: ComputeInitSol (midpoint value and local searches from the lower corner, midpoint, upper corner)
static void fam_glob_new() {
  const char* N = "glob_new";
  for (int att = 0; att < 4000 && fam_lines[N] < 60; ++att) {
    GCase c = rand_glob(att);
    bool bad1, bad2;
    VecShape s1 = rshape(c.n, bad1), s2 = rshape(c.n, bad2);
    VecIn a = reshape(c.lo, s1, 7.5), b = reshape(c.hi, s2, 7.5);
    if (att < 40) { a = vin(c.lo); b = vin(c.hi); }
    bool bad = !covers(a, c.n) || !covers(b, c.n);
    begin(N, "mv i v v f f f"); W_mv(c.fn); W_i(c.level); W_v(a); W_v(b); W_f(c.C); W_f(c.dtol); W_f(c.stol); bar();
    emit([&] {
      MVF f(c.fn);
      if (bad) {
        try { math_GlobOptMin g(f.at(c.level), mkv(a), mkv(b), c.C, c.dtol, c.stol); } catch (Discard&) {}
        unchecked();
        return;
      }
      math_GlobOptMin g(f.at(c.level), mkv(a), mkv(b), c.C, c.dtol, c.stol);
      out_glob(g, c.n, f);
    });
  }
}
// constructor, setters, SetTol, Perform
static void fam_glob_perform() {
  const char* N = "glob_perform";
  for (int att = 0; att < 12000 && fam_lines[N] < 150; ++att) {
    GCase c = rand_glob(att);
    begin(N, "mv i v v f f f i f i f f i"); put_glob_head(c);
    W_i(c.cont); W_f(c.fmin); W_i(c.lip); W_f(c.dtol2); W_f(c.stol2); W_i(c.single); bar();
    emit([&] {
      MVF f(c.fn);
      math_GlobOptMin g(f.at(c.level), mkv(vin(c.lo)), mkv(vin(c.hi)), c.C, c.dtol, c.stol);
      apply_setters(g, c);
      g.SetTol(c.dtol2, c.stol2);
      out_glob(g, c.n, f, true);
      g.Perform(c.single != 0);
      out_glob(g, c.n, f);
    });
  }
}
// two Performs with SetLocalParams in between
static void rlocal(const GCase& c, Vd& la, Vd& lb) {
  la.resize(c.n); lb.resize(c.n);
  double q = rnd(0, 1);
  for (int j = 0; j < c.n; ++j) {
    double lo = c.lo[j], hi = c.hi[j], w = hi - lo;
    double a = lo + w * rnd(0, 0.6), b = a + w * rnd(0.2, 0.7);
    la[j] = a; lb[j] = std::min(b, hi);
  }
  int j = rint(0, c.n - 1);
  if (q < 0.15) { la = c.lo; lb = c.hi; }                                  // the whole box
  else if (q < 0.25) { lb[j] = la[j]; }                                    // degenerate
  else if (q < 0.32) { std::swap(la[j], lb[j]); }                          // reversed
  else if (q < 0.42) { la[j] = c.lo[j] - 1.0; lb[j] = c.hi[j] + 1.0; }     // larger than the global box
  else if (q < 0.48) { lb[j] = la[j] + 1e-10; }                            // thinner than PConfusion
}
static void fam_glob_staged() {
  const char* N = "glob_staged";
  for (int att = 0; att < 12000 && fam_lines[N] < 90; ++att) {
    GCase c = rand_glob(att);
    Vd la, lb; rlocal(c, la, lb);
    int single2 = coin(0.4) ? 1 : 0, cont2 = rcont();
    bool bad1, bad2;
    VecShape s1 = rshape(c.n, bad1), s2 = rshape(c.n, bad2);
    VecIn A = reshape(la, s1, 7.5), B = reshape(lb, s2, 7.5);
    if (att < 60) { A = vin(la); B = vin(lb); }
    bool bad = !covers(A, c.n) || !covers(B, c.n);
    begin(N, "mv i v v f f f i f i i v v i i"); put_glob_head(c);
    W_i(c.cont); W_f(c.fmin); W_i(c.lip); W_i(c.single); W_v(A); W_v(B); W_i(single2); W_i(cont2); bar();
    emit([&] {
      MVF f(c.fn);
      math_GlobOptMin g(f.at(c.level), mkv(vin(c.lo)), mkv(vin(c.hi)), c.C, c.dtol, c.stol);
      apply_setters(g, c);
      g.Perform(c.single != 0);
      out_glob(g, c.n, f);
      if (bad) {
        try { g.SetLocalParams(mkv(A), mkv(B)); g.SetContinuity(cont2); g.Perform(single2 != 0); } catch (Discard&) {}
        unchecked();
        return;
      }
      g.SetLocalParams(mkv(A), mkv(B));
      g.SetContinuity(cont2);
      g.Perform(single2 != 0);
      out_glob(g, c.n, f);
    });
  }
}
// SetGlobalParams with another function after a Perform (the solutions of the first run stay in the container)
static void fam_glob_global() {
  const char* N = "glob_global";
  for (int att = 0; att < 12000 && fam_lines[N] < 70; ++att) {
    GCase c = rand_glob(att);
    GCase d;
    for (int tries = 0; tries < 200; ++tries) {
      d = rand_glob(att + 100 + tries);
      if (d.n == c.n) break;
    }
    if (d.n != c.n) continue;
    int single2 = coin(0.5) ? 1 : 0;
    bool b1, b2;
    VecShape s1 = rshape(c.n, b1), s2 = rshape(c.n, b2);
    VecIn A = reshape(d.lo, s1, 7.5), B = reshape(d.hi, s2, 7.5);
    if (att < 40) { A = vin(d.lo); B = vin(d.hi); }
    bool bad = !covers(A, c.n) || !covers(B, c.n);
    begin(N, "mv i v v f f f i f i i mv i v v f f f i"); put_glob_head(c);
    W_i(c.cont); W_f(c.fmin); W_i(c.lip); W_i(c.single);
    W_mv(d.fn); W_i(d.level); W_v(A); W_v(B); W_f(d.C); W_f(d.dtol); W_f(d.stol); W_i(single2); bar();
    emit([&] {
      MVF f(c.fn), f2(d.fn);
      math_GlobOptMin g(f.at(c.level), mkv(vin(c.lo)), mkv(vin(c.hi)), c.C, c.dtol, c.stol);
      apply_setters(g, c);
      g.Perform(c.single != 0);
      out_glob(g, c.n, f);
      if (bad) {
        try { g.SetGlobalParams(f2.at(d.level), mkv(A), mkv(B), d.C, d.dtol, d.stol); g.Perform(single2 != 0); } catch (Discard&) {}
        unchecked();
        return;
      }
      g.SetGlobalParams(f2.at(d.level), mkv(A), mkv(B), d.C, d.dtol, d.stol);
      out_glob(g, c.n, f2, true);
      g.Perform(single2 != 0);
      out_glob(g, c.n, f2);
    });
  }
}
// Points with every index and output vector; SetTol / GetTol, SetContinuity / GetContinuity
static void fam_glob_access() {
  const char* N = "glob_access";
  for (int att = 0; att < 12000 && fam_lines[N] < 100; ++att) {
    GCase c = rand_glob(att);
    int pm = pickli({-1, 0, 1, 1});
    int idx = coin(0.6) ? 1 : coin(0.7) ? rint(1, 3) : pickli({0, -1, 4, 5, 12, 100});
    bool bo;
    VecShape so = coin(0.85) ? VecShape{coin(0.7) ? 1 : 0, c.n + (coin(0.5) ? 1 : 0)} : rshape(c.n, bo);
    Vd fill(so.length);
    for (double& x : fill) x = rnd(-9, 9);
    VecIn out = reshape(fill, so, 3.25);
    bool badout = !covers(out, c.n);
    begin(N, "mv i v v f f f i i v f f i"); put_glob_head(c);
    W_i(pm); W_i(idx); W_v(out); W_f(c.dtol2); W_f(c.stol2); W_i(c.cont); bar();
    emit([&] {
      MVF f(c.fn);
      math_GlobOptMin g(f.at(c.level), mkv(vin(c.lo)), mkv(vin(c.hi)), c.C, c.dtol, c.stol);
      if (pm >= 0) g.Perform(pm != 0);
      g.SetTol(c.dtol2, c.stol2);
      g.SetContinuity(c.cont);
      // Points(index, out): the library reads the sequence without a range check and writes out(1..n) without one
      // (an index outside 1..NbExtrema or an output vector not covering 1..n is `unchecked`; the call is not made:
      // it reads or writes outside the containers)
      if (idx < 1 || idx > g.NbExtrema() || badout) { unchecked(); return; }
      math_Vector o = mkv(out);
      g.Points(idx, o);
      ov(o);
      out_glob(g, c.n, f);
    });
  }
}

// =================================================================================
// math_BullardGenerator
// =================================================================================
static void fam_bullard() {
  const char* N = "bullard";
  static const unsigned SEEDS[] = {1u, 0u, 0xFFFFFFFFu, 0x49616E42u, 2u, 0x80000000u, 0x7FFFFFFFu, 12345u, 0xDEADBEEFu};
  for (int att = 0; att < 3000 && fam_lines[N] < 40; ++att) {
    unsigned seed = att < 9 ? SEEDS[att] : (unsigned)(rnd(0, 1) * 4294967295.0);
    unsigned seed2 = att < 9 ? SEEDS[(att * 5 + 3) % 9] : (unsigned)(rnd(0, 1) * 4294967295.0);
    int n1 = att < 9 ? 1 + att % 6 : rint(0, 6), n2 = att < 9 ? 2 : rint(0, 6);
    begin(N, "i i i i"); W_i((int)seed); W_i(n1); W_i((int)seed2); W_i(n2); bar();
    emit([&] {
      math_BullardGenerator r(seed);
      for (int i = 0; i < n1; ++i) in((long)r.NextInt());
      for (int i = 0; i < n1; ++i) h(r.NextReal());
      r.SetSeed(seed2);
      for (int i = 0; i < n2; ++i) h(r.NextReal());
      for (int i = 0; i < n2; ++i) in((long)r.NextInt());
      uint64_t hsh = FNV0;
      for (int i = 0; i < 2000; ++i) fold_hash(hsh, r.NextReal());
      hx(hsh);
      for (int i = 0; i < 2000; ++i) { unsigned v = r.NextInt(); for (int k = 0; k < 4; ++k) { hsh ^= (uint8_t)((v >> (8 * k)) & 0xff); hsh *= FNVP; } }
      hx(hsh);
      math_BullardGenerator d;   // the default seed is 1
      in((long)d.NextInt());
    });
  }
}

// =================================================================================
// math_PSO and math_PSOParticlesPool
// =================================================================================
struct PoolIn { int count = 1, dim = 1; std::vector<Vd> p; };   // per particle: pos, vel, best (dim each), dist, bestdist
static void W_pool(const PoolIn& pl) {
  int st = ntok;
  in(pl.count); in(pl.dim);
  for (const Vd& q : pl.p) for (double x : q) h(x);
  word("pool", st, 2 + pl.count * (3 * pl.dim + 2));
}
static void fill_pool(math_PSOParticlesPool& pool, const PoolIn& pl) {
  for (int i = 0; i < pl.count; ++i) {
    PSO_Particle* q = pool.GetParticle(i + 1);
    const Vd& d = pl.p[i];
    for (int k = 0; k < pl.dim; ++k) {
      q->Position[k] = d[k]; q->Velocity[k] = d[pl.dim + k]; q->BestPosition[k] = d[2 * pl.dim + k];
    }
    q->Distance = d[3 * pl.dim]; q->BestDistance = d[3 * pl.dim + 1];
  }
}
static void out_particle(const PSO_Particle* q, int dim) {
  for (int k = 0; k < dim; ++k) h(q->Position[k]);
  for (int k = 0; k < dim; ++k) h(q->Velocity[k]);
  for (int k = 0; k < dim; ++k) h(q->BestPosition[k]);
  h(q->Distance); h(q->BestDistance);
}
// all particles, then the 1-based indices of the best and the worst particle
static void out_pool(math_PSOParticlesPool& pool, int count, int dim) {
  for (int i = 1; i <= count; ++i) out_particle(pool.GetParticle(i), dim);
  PSO_Particle* first = pool.GetParticle(1);
  in((long)(pool.GetBestParticle() - first) + 1);
  in((long)(pool.GetWorstParticle() - first) + 1);
}
static PoolIn rand_pool(int count, int dim, const Vd& lo, const Vd& hi, bool defaults) {
  PoolIn pl; pl.count = count; pl.dim = dim;
  for (int i = 0; i < count; ++i) {
    Vd q;
    if (defaults && coin(0.3)) {            // a particle as the constructor leaves it
      for (int k = 0; k < 3 * dim; ++k) q.push_back(0.0);
      q.push_back(DBL_MAX); q.push_back(DBL_MAX);
    } else {
      Vd pos(dim), best(dim);
      for (int k = 0; k < dim; ++k) { pos[k] = lo[k] + (hi[k] - lo[k]) * rnd(-0.1, 1.1); best[k] = coin(0.5) ? pos[k] : lo[k] + (hi[k] - lo[k]) * rnd(0, 1); }
      for (int k = 0; k < dim; ++k) q.push_back(pos[k]);
      for (int k = 0; k < dim; ++k) q.push_back(rnd(-1, 1) * (hi[k] - lo[k]) * 0.1);
      for (int k = 0; k < dim; ++k) q.push_back(best[k]);
      double d = coin(0.35) ? (double)rint(0, 2) : rnd(-3, 20);
      if (coin(0.1)) d = DBL_MAX;
      q.push_back(d);
      q.push_back(coin(0.5) ? d : (coin(0.5) ? d + rnd(0, 3) : DBL_MAX));
    }
    pl.p.push_back(q);
  }
  return pl;
}
struct PCase {
  NSpec fn;
  Vd xmin, lo, hi, steps, steps2;
  int n = 1, nbpart = 8, nbiter = 100, nbiter2 = 100;
};
static int rnbpart() { return pickli({1, 2, 3, 4, 5, 8, 8, 16, 16, 32, 32, 32, 50}); }
static int rnbiter() { return pickli({0, 1, 2, 3, 5, 10, 20, 50, 100, 100, 100, 200}); }
static PCase rand_pso(int att) {
  PCase c;
  c.n = att < 4 ? att + 1 : rdim_glob();
  c.fn = glob_fn(c.n, c.xmin);
  maybe_fail(c.fn, c.xmin, 1.0, 0.08, 15);
  rbox(c.n, c.xmin, c.lo, c.hi);
  // grid steps: about 3 to 200 points per coordinate (fewer in higher dimensions)
  int per = c.n == 1 ? rint(4, 200) : c.n == 2 ? rint(3, 40) : c.n == 3 ? rint(3, 14) : rint(3, 8);
  c.steps.resize(c.n); c.steps2.resize(c.n);
  for (int j = 0; j < c.n; ++j) {
    double w = std::fabs(c.hi[j] - c.lo[j]); if (w == 0.0) w = 1.0;
    c.steps[j] = w / per * rnd(0.8, 1.2);
    c.steps2[j] = coin(0.5) ? c.steps[j] : w / per * rnd(0.7, 1.5);
  }
  if (coin(0.05)) c.steps[rint(0, c.n - 1)] = 0.0;
  if (coin(0.04)) c.steps2[rint(0, c.n - 1)] = -c.steps2[0];
  if (coin(0.04)) c.steps2[rint(0, c.n - 1)] = 1e-20;
  c.nbpart = rnbpart(); c.nbiter = rnbiter(); c.nbiter2 = rnbiter();
  return c;
}
static void fam_pso() {
  const char* N = "pso";
  for (int att = 0; att < 12000 && fam_lines[N] < 120; ++att) {
    PCase c = rand_pso(att);
    bool b1, b2, b3, b4;
    bool goodshape = att < 60 || coin(0.8);
    VecShape sl = goodshape ? VecShape{1, c.n} : rshape(c.n, b1), su = goodshape ? VecShape{1, c.n} : rshape(c.n, b2);
    VecShape ss = goodshape ? VecShape{1, c.n} : rshape(c.n, b3), s2 = goodshape ? VecShape{1, c.n} : rshape(c.n, b4);
    // PSO copies its vectors by position (operator=): another lower bound is fine, another length is `unchecked`
    auto lowerOf = [&](VecShape s) { return att < 60 ? 1 : (coin(0.2) ? pickli({0, -3, 5}) : s.lower); };
    VecShape shl{lowerOf(sl), sl.length}, shu{lowerOf(su), su.length}, shs{lowerOf(ss), ss.length}, sh2{lowerOf(s2), s2.length};
    VecIn lo = reshape(c.lo, shl, 0.5), hi = reshape(c.hi, shu, 0.5), st = reshape(c.steps, shs, 0.1), st2 = reshape(c.steps2, sh2, 0.1);
    bool bo;
    VecShape so = coin(0.9) ? VecShape{coin(0.8) ? 1 : pickli({0, -2, 4}), c.n} : rshape(c.n, bo);
    Vd fill(so.length);
    for (double& x : fill) x = rnd(-9, 9);
    VecIn out = reshape(fill, so, 3.25);
    auto len_ok = [&](const VecIn& v) { return (int)v.v.size() == c.n; };
    bool bad = !(len_ok(lo) && len_ok(hi) && len_ok(st) && len_ok(st2) && len_ok(out));
    bool dangerous = false;   // shorter vectors would be read or written outside their storage: not called
    for (const VecIn* v : {&lo, &hi, &st, &st2, &out}) if ((int)v->v.size() < c.n) dangerous = true;
    begin(N, "mv v v v i i v i v"); W_mv(c.fn); W_v(lo); W_v(hi); W_v(st); W_i(c.nbpart); W_i(c.nbiter);
    W_v(st2); W_i(c.nbiter2); W_v(out); bar();
    emit([&] {
      MVF f(c.fn);
      if (bad) {
        if (!dangerous) {
          try {
            math_PSO pso(&f.p, mkv(lo), mkv(hi), mkv(st), c.nbpart, c.nbiter);
            math_Vector o = mkv(out); double val = 0;
            pso.Perform(mkv(st2), val, o, c.nbiter2);
          } catch (Discard&) {}
        }
        unchecked();
        return;
      }
      math_PSO pso(&f.p, mkv(lo), mkv(hi), mkv(st), c.nbpart, c.nbiter);
      math_Vector o = mkv(out);
      double val = 0;
      pso.Perform(mkv(st2), val, o, c.nbiter2);
      h(val); ov(o);
      out_mv(f);
    });
  }
}
// Perform with a given pool of particles
static void fam_pso_pool() {
  const char* N = "pso_pool";
  for (int att = 0; att < 12000 && fam_lines[N] < 100; ++att) {
    PCase c = rand_pso(att);
    int count = pickli({1, 2, 3, 4, 6, 8, 10});
    PoolIn pl = rand_pool(count, c.n, c.lo, c.hi, true);
    int nbp = coin(0.65) ? count : (coin(0.92) ? rint(0, count) : count + pickli({1, 5}));
    bool bo;
    VecShape so = coin(0.8) ? VecShape{1, c.n} : rshape(c.n, bo);
    Vd fill(so.length);
    for (double& x : fill) x = rnd(-9, 9);
    VecIn out = reshape(fill, so, 3.25);
    bool bad = nbp > count || (int)out.v.size() != c.n;
    bool dangerous = nbp > count || (int)out.v.size() < c.n;
    begin(N, "mv v v v i i pool i i v"); W_mv(c.fn); W_v(vin(c.lo)); W_v(vin(c.hi)); W_v(vin(c.steps)); W_i(c.nbpart); W_i(c.nbiter);
    W_pool(pl); W_i(nbp); W_i(c.nbiter2); W_v(out); bar();
    emit([&] {
      MVF f(c.fn);
      math_PSO pso(&f.p, mkv(vin(c.lo)), mkv(vin(c.hi)), mkv(vin(c.steps)), c.nbpart, c.nbiter);
      math_PSOParticlesPool pool(count, c.n);
      fill_pool(pool, pl);
      if (bad) {
        if (!dangerous) {
          try { math_Vector o = mkv(out); double val = 0; pso.Perform(pool, nbp, val, o, c.nbiter2); } catch (Discard&) {}
        }
        unchecked();
        return;
      }
      math_Vector o = mkv(out);
      double val = 0;
      pso.Perform(pool, nbp, val, o, c.nbiter2);
      h(val); ov(o);
      out_mv(f);
      out_pool(pool, count, c.n);
    });
  }
}
// the pool alone: the particles, GetBestParticle, GetWorstParticle (first minimum / first maximum on ties)
static void fam_pso_pool_access() {
  const char* N = "pso_pool_access";
  for (int att = 0; att < 3000 && fam_lines[N] < 50; ++att) {
    int dim = rint(1, 4), count = pickli({1, 2, 3, 4, 5, 8, 12});
    Vd lo(dim, -1.0), hi(dim, 1.0);
    PoolIn pl = rand_pool(count, dim, lo, hi, true);
    if (att % 3 == 0) for (Vd& q : pl.p) { q[3 * dim] = (double)rint(0, 1); }    // many ties
    int idx = coin(0.9) ? rint(1, count) : pickli({0, -1, count + 1, count + 7});
    if (att % 5 == 4) for (Vd& q : pl.p) { for (int k = 0; k < 3 * dim; ++k) q[k] = 0.0; q[3 * dim] = DBL_MAX; q[3 * dim + 1] = DBL_MAX; }   // a pool as the constructor leaves it
    begin(N, "pool i"); W_pool(pl); W_i(idx); bar();
    emit([&] {
      math_PSOParticlesPool pool(count, dim);
      fill_pool(pool, pl);
      out_pool(pool, count, dim);
      // GetParticle(i) has no range check in the library: an index outside 1..count is `unchecked` (the pointer
      // is not dereferenced)
      if (idx < 1 || idx > count) { unchecked(); return; }
      out_particle(pool.GetParticle(idx), dim);
    });
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
  {"test", "test_mv", fam_test_mv},
  {"bullard", "bullard", fam_bullard},
  {"glob", "glob_new", fam_glob_new}, {"glob", "glob_perform", fam_glob_perform}, {"glob", "glob_staged", fam_glob_staged},
  {"glob", "glob_global", fam_glob_global}, {"glob", "glob_access", fam_glob_access},
  {"pso", "pso", fam_pso}, {"pso", "pso_pool", fam_pso_pool}, {"pso", "pso_pool_access", fam_pso_pool_access},
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
  }
  return 0;
}
