// Deterministic reference calls for OCCT 8.0.1's one-dimensional solvers of the `math` package:
//   math_Function, math_FunctionWithDerivative (the callback interfaces), math_FunctionSample, math_FunctionRoots,
//   math_FunctionRoot, math_FunctionAllRoots, math_BracketedRoot, math_NewtonFunctionRoot, math_BracketMinimum,
//   math_BrentMinimum, math_TrigonometricFunctionRoots and math_TrigonometricEquationFunction.
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints (and bools) are decimal. Every input of a call is printed on the
// left of " | " in the order named by the family's schema string (`begin(name, schema)`); tests/math1d.rs reads them
// back with the reader method of the same name as each schema word. The oracle aborts if the words it printed differ
// from the schema (and if a fixed-size word printed the wrong number of tokens). Outputs are values, `throws` (OCCT
// raised an exception that the line's call really raises: an explicit `throw`, or a check that is compiled into the
// caller, like the StdFail_NotDone checks of the inline accessors) or `unchecked` (OCCT 8.0.1 as built has no
// check, so the numbers it would return are unspecified; the Rust API must return an error).
//
// Schema words (token counts): f=1 i=1 tf=variable. `tf` is a test function (see below), printed as
//   kind nparams p_1 .. p_nparams fail_lo fail_hi fail_mode state_mode
// i.e. 1 + 1 + nparams + 4 tokens (kind, nparams and fail_mode, state_mode are ints, the rest floats).
//
// THE TEST FUNCTIONS. The solvers call user functions; the judge cannot pass closures through text, so every
// function is fully described by a `tf` record and implemented identically here and in tests/math1d.rs, with
// + - * / floor and fabs only (no libm), so that both sides see the same bits. With x the argument and p the
// parameters (all evaluation orders below are the code's):
//   kind 0 POLYNOMIAL  p = c_0 .. c_d (d = 1..8): f = sum c_k x^k (Horner), f' = sum k c_k x^(k-1) (Horner)
//   kind 1 TRIG_SUM    p = a_1 w_1 ph_1 .. (1..3 terms): f = sum a sinp(w x + ph), f' = sum (a w) cosp(w x + ph)
//   kind 2 RATIONAL    p = a b c pole: f = a + b/(x-pole) + c x, f' = c - (b/(x-pole))/(x-pole); every callback
//                      FAILS (returns false) when |x - pole| < 1e-100
//   kind 3 MULTIPLE    p = a r m s (m = 2..4): f = a (x-r)^m (x-s), f' = a (m (x-r)^(m-1) (x-s) + (x-r)^m)
//   kind 4 FLAT        p = lo hi a b pw c (pw = 1,2,3): c on [lo,hi]; c + a (lo-x)^pw for x < lo; c + b (x-hi)^pw
//                      for x > hi
//   kind 5 EXP         p = a b: f = expp(a x) - b, f' = a expp(a x)
//   kind 6 ABS         p = a c d: f = a |x-c| + d, f' = a sign(x-c) (0 at c)
// sinp / cosp / expp are the deterministic stand-ins of sin / cos / exp described at the functions below.
// A function FAILS (the callback returns false and leaves its out-parameters untouched) for x in [fail_lo, fail_hi]
// (inclusive) when the bit of the callback is set in fail_mode: 1 = Value, 2 = Derivative, 4 = Values. fail_lo >
// fail_hi means "never". GetStateNumber returns, by state_mode: 0 -> 0; 1 -> the number of Value / Derivative /
// Values calls so far; 2 -> 4 * (that number) + (1 for the last Value, 2 for Derivative, 3 for Values; 0 if none).
// The lines of the construction families (not the *_access and *_dump ones) also print the callback counts `nvalue nderivative nvalues nstate` and a 64-bit FNV-1a hash of the
// call trace (per call: a tag byte 'V' 'D' 'W' (Values) or 'S' (GetStateNumber), and for the first three the 8
// bytes of x, little endian): the same calls in the same order give the same hash.
//
// Cases for which the C++ result depends on uninitialised memory (OCCT reads a variable that a failing callback
// left unwritten) are detected by running every call four times with differently, randomly filled stack and heap and are NOT
// printed; so are cases in which a callback sees or produces a non-finite number or runs away (> 300000 calls).
// Cases of the trigonometric families (they call the C library's sin, cos, asin, acos, atan, pow) are only printed when
// moving every result of those functions by an ulp, in three pseudo-random patterns, leaves the outputs within 3 ulps
// (the test allows 8): see `emit` and the interposed functions below.
#include <GeomAbs_Shape.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <math_BracketMinimum.hxx>
#include <math_BracketedRoot.hxx>
#include <math_BrentMinimum.hxx>
#include <math_Function.hxx>
#include <math_FunctionAllRoots.hxx>
#include <math_FunctionRoot.hxx>
#include <math_FunctionRoots.hxx>
#include <math_FunctionSample.hxx>
#include <math_FunctionWithDerivative.hxx>
#include <math_NewtonFunctionRoot.hxx>
#include <math_TrigonometricEquationFunction.hxx>
#include <math_TrigonometricFunctionRoots.hxx>
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


// math_FunctionAllRoots documents Standard_NumericError (when its inner math_FunctionRoots fails or is null), but the
// OCCT libraries are built without that check (the library has no reference to Standard_NumericError): then the
// algorithm just goes on, reading empty sequences (crash). To tell the cases where OCCT's source says "throws" from the
// others, the very same source file is compiled into this program under another class name, with the checks ON (this
// translation unit is built without No_Exception). Inclusion order matters: no OCCT header may follow it, because the
// source file defines the No_Standard_* macros.
#undef _math_FunctionAllRoots_HeaderFile
#define math_FunctionAllRoots math_FunctionAllRootsChecked
#include "../cpp/math/math_FunctionAllRoots.cxx"
#undef math_FunctionAllRoots
typedef std::vector<double> Vd;

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
  if (std::getenv("MATH1D_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
// 0 or 1 ulp (a different libm gives such differences, and tests/math1d.rs compares the families that use them within a
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
    else if (rk != r1) { fam_dropped[cur_name]++; fam_dropped[cur_name + "!uninit"]++; buf.clear(); return false; }
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
static double finv(double v) { return std::isfinite(v) ? v : 0.0; }
static int rstate() { return rint(0, 2); }
// a count used only to pick indices: any failure of the call gives 0
static int safe_int(const std::function<int()>& f) { try { return f(); } catch (Discard&) { return 0; } catch (Standard_Failure&) { return 0; } }
// an index into a collection of n elements: mostly valid, sometimes out of range
static int ridx(int n) { if (n >= 1 && coin(0.6)) return rint(1, n); int bad[] = {0, -1, n + 1, n + 5, 1000}; return bad[rint(0, 4)]; }
static void lohi(double a, double b, double& lo, double& hi) { lo = std::min(a, b); hi = std::max(a, b); if (hi - lo < 1e-3) hi = lo + 1e-3; }

// ---------------------------------------------------------------------------------
// the shared output of a solver family's call counts and a dump text
static std::string dump_text(const std::function<void(std::ostream&)>& d) { std::ostringstream os; d(os); return os.str(); }

// =================================================================================
// the test functions themselves (checks the judge's implementation)
// =================================================================================
static void fam_test_function() {
  const char* N = "test_function";
  for (int att = 0; att < 3000 && fam_lines[N] < 220; ++att) {
    FnSpec s;
    double lo = rnd(-5, 0), hi = lo + rnd(0.5, 10);
    switch (att % 7) {
      case 0: s = mk(K_POLY, [&] { Vd c; int d = rint(1, 8); for (int i = 0; i <= d; ++i) c.push_back(rnd(-3, 3)); if (c.back() == 0) c.back() = 1; return c; }(), rstate()); break;
      case 1: s = fn_rootful(lo, hi, rstate()); break;
      default: s = fn_rootful(lo, hi, rstate()); break;
    }
    // force every kind to appear
    if (att < 70) {
      int kind = att % K_COUNT;
      for (int tries = 0; tries < 200 && s.kind != kind; ++tries) s = fn_rootful(lo, hi, rstate());
    }
    add_fail(s, lo, hi, 0.4);
    double x = coin(0.3) ? rnd(-12, 12) : rnd(lo, hi);
    if (coin(0.1) && s.flo <= s.fhi) x = coin(0.5) ? s.flo : s.fhi;
    if (coin(0.05)) x = 0.0;
    if (coin(0.05) && s.kind == K_RATIONAL) x = s.p[3];
    if (coin(0.05) && (s.kind == K_FLAT)) x = coin(0.5) ? s.p[0] : s.p[1];
    if (coin(0.05) && (s.kind == K_ABS || s.kind == K_MULTIPLE)) x = s.p[1];
    begin(N, "tf f");
    W_tf(s); W_f(x); bar();
    emit([&] {
      TF f(s);
      double v = 0, d = 0, v2 = 0, d2 = 0;
      bool ok1 = f.Value(x, v);
      ob(ok1); if (ok1) h(v);
      bool ok2 = f.Derivative(x, d);
      ob(ok2); if (ok2) h(d);
      bool ok3 = f.Values(x, v2, d2);
      ob(ok3); if (ok3) { h(v2); h(d2); }
      in(f.GetStateNumber());
      out_counts(f);
    });
  }
}

// =================================================================================
// math_FunctionSample
// =================================================================================
static void fam_function_sample() {
  const char* N = "function_sample";
  static const double AB[][2] = {{0, 1}, {-1, 1}, {1, 0}, {2, 2}, {0, 6.283185307179586}, {-1e6, 1e6}, {1e-300, 1}, {-5, -3}};
  for (int att = 0; att < 4000 && fam_lines[N] < 160; ++att) {
    double a, b;
    if (att < 40) { a = AB[att % 8][0]; b = AB[att % 8][1]; } else if (coin(0.5)) { a = rnd(-10, 10); b = rnd(-10, 10); } else { a = rnz(0.001, 1e5); b = rnz(0.001, 1e5); }
    int n = att % 9 == 0 ? rint(-3, 1) : (coin(0.3) ? rint(2, 5) : rint(2, 40));
    int index = coin(0.15) ? rint(-2, 0) : (coin(0.15) ? n + rint(1, 3) : rint(1, std::max(1, n)));
    if (att < 40) { n = 2 + att % 5; index = att % 7 == 6 ? 0 : (att % 7 == 5 ? n + 1 : 1 + (att / 7) % n); }
    begin(N, "f f i i");
    W_f(a); W_f(b); W_i(n); W_i(index); bar();
    emit([&] {
      math_FunctionSample s(a, b, n);
      double ra, rb; s.Bounds(ra, rb);
      h(ra); h(rb); in(s.NbPoints());
      // GetParameter: the Standard_OutOfRange check is compiled out of the library, and (n - 1) = 0 divides by zero
      if (index <= 0 || index > n || n < 2) {
        unchecked();
      } else {
        h(s.GetParameter(index));
      }
    });
  }
}

// =================================================================================
// math_FunctionRoots
// =================================================================================
struct FRCase { FnSpec fn; double a, b; int nb; double ex, ef, en, k; };
static int rnbsample() {
  static const int NB[] = {0, 1, 2, 3, 5, 8, 10, 15, 20, 30, 50, 100};
  if (coin(0.05)) return -rint(1, 30);
  return coin(0.6) ? pick(NB) : rint(1, 60);
}
static FRCase rand_fr(int att, bool allow_fail = true) {
  FRCase c;
  rinterval(c.a, c.b);
  double lo, hi; lohi(c.a, c.b, lo, hi);
  c.fn = fn_rootful(lo, hi, rstate());
  if (allow_fail) add_fail(c.fn, lo, hi, 0.18);
  c.nb = rnbsample();
  c.ex = coin(0.3) ? 0.0 : rtol();
  c.ef = coin(0.3) ? 0.0 : rtol();
  static const double ENS[] = {0.0, 0.0, 0.0, 1e-12, 1e-6, 1e-3, 0.1, 1.0, 1e3};
  c.en = pick(ENS);
  double q = rnd(0, 1);
  if (q < 0.6) c.k = 0.0;
  else if (q < 0.85) c.k = rnd(-2, 2);
  else { double v, d; eval_fn(c.fn, coin(0.5) ? c.a : 0.5 * (c.a + c.b), v, d); c.k = std::isfinite(v) ? v : 0.0; }
  (void)att;
  return c;
}
static void W_fr(const FRCase& c) { W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.nb); W_f(c.ex); W_f(c.ef); W_f(c.en); W_f(c.k); }
static const char* FR_SCHEMA = "tf f f i f f f f";
static void edge_fr(int att, FRCase& c) {
  // hand-made edge cases for the first iterations
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 0; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break;       // x^2-2
    case 1: c.fn = mk(K_POLY, Vd{0.0, 1.0}, 1); c.a = -1; c.b = 1; c.nb = 5; c.ex = 0; c.ef = 0; c.en = 0; c.k = 0; break;                  // root at a sample
    case 2: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.a = -1; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-6; c.en = 0; c.k = 0; break;       // double root
    case 3: c.fn = mk(K_MULTIPLE, Vd{1.0, 0.5, 3.0, 10.0}, 2); c.a = -1; c.b = 2; c.nb = 10; c.ex = 1e-9; c.ef = 1e-6; c.en = 0; c.k = 0; break; // triple root
    case 4: c.fn = mk(K_FLAT, Vd{-0.5, 0.5, 1.0, 1.0, 1.0, 0.0}, 0); c.a = -2; c.b = 2; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-12; c.k = 0; break; // zero on a sub-interval
    case 5: c.fn = mk(K_POLY, Vd{1.0, 0.0, 1.0}, 0); c.a = -2; c.b = 2; c.nb = 10; c.ex = 0; c.ef = 1e-9; c.en = 0; c.k = 0; break;          // no root
    case 6: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 3; c.b = 0; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break;       // reversed
    case 7: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 1.5; c.b = 1.5; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break;     // empty interval
    case 8: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 0; c.b = 3; c.nb = 0; c.ex = 0; c.ef = 0; c.en = 0; c.k = 0; break;               // zero everything
    case 9: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 0; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e3; c.k = 0; break;      // everything null
    case 10: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 0; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 2.0; break;     // K
    case 11: c.fn = mk(K_RATIONAL, Vd{0.0, 1.0, 0.0, 0.5}, 0); c.a = 0; c.b = 1; c.nb = 10; c.ex = 1e-9; c.ef = 1e-6; c.en = 0; c.k = 0; break;  // pole
    case 12: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); add_fail_exact(c.fn, 1.0, 1.2, 1); c.a = 0; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break; // fails
    case 13: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); add_fail_exact(c.fn, 1.0, 1.2, 4); c.a = 0; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break; // Values fails
    case 14: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); add_fail_exact(c.fn, 1.3, 1.5, 2); c.a = 0; c.b = 3; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break; // Derivative fails
    case 15: c.fn = mk(K_POLY, Vd{-1e-3, 0.0, 1.0}, 0); c.a = -1; c.b = 1; c.nb = 3; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break;       // two close roots, coarse sample
    case 16: c.fn = mk(K_TRIG, Vd{1.0, 1.0, 0.0}, 0); c.a = 0; c.b = 6.283185307179586; c.nb = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 0; c.k = 0; break; // roots at both ends
    case 17: c.fn = mk(K_POLY, Vd{-1.0, 1.0}, 0); c.a = 0; c.b = 1; c.nb = 10; c.ex = 0; c.ef = 1e-300; c.en = 0; c.k = 0; break;              // root at the end b
    default: break;
  }
}
static void fam_function_roots() {
  const char* N = "function_roots";
  for (int att = 0; att < 6000 && fam_lines[N] < 260; ++att) {
    FRCase c = rand_fr(att);
    edge_fr(att, c);
    begin(N, FR_SCHEMA); W_fr(c); bar();
    emit([&] {
      TF f(c.fn);
      math_FunctionRoots r(f, c.a, c.b, c.nb, c.ex, c.ef, c.en, c.k);
      ob(r.IsDone());
      out_counts(f);
      if (r.IsDone()) {
        ob(r.IsAllNull());
        in(r.NbSolutions());
        for (int j = 1; j <= r.NbSolutions(); ++j) { h(r.Value(j)); in(r.StateNumber(j)); }
      }
    });
  }
}
static void fam_function_roots_access() {
  const char* N = "function_roots_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 120; ++att) {
    FRCase c = rand_fr(att);
    edge_fr(att / 4, c);
    int q = att % 4;
    int nsol = safe_int([&] { TF f(c.fn); math_FunctionRoots r(f, c.a, c.b, c.nb, c.ex, c.ef, c.en, c.k); return r.IsDone() ? r.NbSolutions() : 0; });
    int idx = ridx(nsol);
    begin(N, "tf f f i f f f f i i");
    W_fr(c); W_i(q); W_i(idx); bar();
    emit([&] {
      TF f(c.fn);
      math_FunctionRoots r(f, c.a, c.b, c.nb, c.ex, c.ef, c.en, c.k);
      if (q == 0) ob(r.IsAllNull());
      else if (q == 1) in(r.NbSolutions());
      else if (q == 2) h(r.Value(idx));
      else in(r.StateNumber(idx));
    });
  }
}
static void fam_function_roots_dump() {
  const char* N = "function_roots_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    FRCase c = rand_fr(att);
    edge_fr(att, c);
    begin(N, FR_SCHEMA); W_fr(c); bar();
    emit([&] {
      TF f(c.fn);
      math_FunctionRoots r(f, c.a, c.b, c.nb, c.ex, c.ef, c.en, c.k);
      std::ostringstream os; r.Dump(os);
      os_text(os.str());
    });
  }
}

// =================================================================================
// math_FunctionAllRoots
// =================================================================================
struct ARCase { FnSpec fn; double a, b; int n; double ex, ef, en; };
static ARCase rand_ar(int att) {
  ARCase c;
  rinterval(c.a, c.b);
  double lo, hi; lohi(c.a, c.b, lo, hi);
  // functions that are null (or nearly) on a sub-interval make the family interesting
  if (coin(0.4)) {
    double u = rnd(lo, hi), v = rnd(lo, hi); if (u > v) std::swap(u, v);
    c.fn = mk(K_FLAT, Vd{u, v, rnz(0.2, 3.0), rnz(0.2, 3.0), (double)rint(1, 3), 0.0}, rstate());
  } else {
    c.fn = fn_rootful(lo, hi, rstate());
  }
  add_fail(c.fn, lo, hi, 0.12);
  static const int NS[] = {2, 3, 4, 5, 8, 10, 20, 40};
  c.n = coin(0.85) ? pick(NS) : rint(2, 30);
  if (att % 40 == 39) c.n = rint(-1, 1);
  c.ex = coin(0.3) ? 0.0 : rtol();
  c.ef = coin(0.3) ? 0.0 : rtol();
  static const double ENS[] = {0.0, 1e-12, 1e-9, 1e-6, 1e-3, 0.05, 0.3, 1.0};
  c.en = pick(ENS);
  return c;
}
static void W_ar(const ARCase& c) { W_tf(c.fn); W_f(c.a); W_f(c.b); W_i(c.n); W_f(c.ex); W_f(c.ef); W_f(c.en); }
static const char* AR_SCHEMA = "tf f f i f f f";
static void edge_ar(int att, ARCase& c) {
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 0; c.b = 3; c.n = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break;
    case 1: c.fn = mk(K_FLAT, Vd{-0.5, 0.5, 1.0, 1.0, 1.0, 0.0}, 1); c.a = -2; c.b = 2; c.n = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break; // null interval inside
    case 2: c.fn = mk(K_FLAT, Vd{-5.0, 5.0, 1.0, 1.0, 1.0, 0.0}, 2); c.a = -2; c.b = 2; c.n = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break; // everywhere null
    case 3: c.fn = mk(K_FLAT, Vd{0.5, 3.0, 1.0, 1.0, 2.0, 0.0}, 0); c.a = -2; c.b = 2; c.n = 8; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-6; break; // null from the left
    case 4: c.fn = mk(K_FLAT, Vd{-3.0, 0.5, 1.0, 1.0, 2.0, 0.0}, 0); c.a = -2; c.b = 2; c.n = 8; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-6; break; // null to the right
    case 5: c.fn = mk(K_POLY, Vd{1.0, 0.0, 1.0}, 0); c.a = -2; c.b = 2; c.n = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break; // no root
    case 6: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 3; c.b = 0; c.n = 10; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break; // reversed sample
    case 7: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.a = 3; c.b = 0; c.n = 1; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break; // one point
    case 8: c.fn = mk(K_FLAT, Vd{-0.7, -0.2, 1.0, 1.0, 1.0, 0.0}, 0); c.a = -2; c.b = 2; c.n = 20; c.ex = 1e-9; c.ef = 1e-9; c.en = 1e-9; break; // narrower than the step
    default: break;
  }
}
// returns false when the library must not be called (the documented Standard_NumericError case)
static bool run_ar(const ARCase& c, TF& f, std::function<void(math_FunctionAllRoots&)> use) {
  math_FunctionSample S(c.a, c.b, c.n);
  TF fc(c.fn);
  try {
    math_FunctionAllRootsChecked rc(fc, S, c.ex, c.ef, c.en);
  } catch (Standard_Failure&) {
    return false;
  }
  math_FunctionAllRoots r(f, S, c.ex, c.ef, c.en);
  if (fc.trace != f.trace || fc.nv != f.nv || fc.nd != f.nd || fc.nw != f.nw) {
    // only a failing callback can leave a variable unwritten (uninitialised memory), and then the two copies differ
    if (c.fn.fmode == 0) {
      std::fprintf(stderr, "checked and library variants of math_FunctionAllRoots disagree\n");
      std::abort();
    }
    throw Discard();
  }
  use(r);
  return true;
}
static void fam_function_all_roots() {
  const char* N = "function_all_roots";
  for (int att = 0; att < 6000 && fam_lines[N] < 220; ++att) {
    ARCase c = rand_ar(att);
    edge_ar(att, c);
    begin(N, AR_SCHEMA); W_ar(c); bar();
    emit([&] {
      TF f(c.fn);
      if (c.n < 2) { unchecked(); return; } // GetParameter divides by (NbPoints - 1)
      bool ok = run_ar(c, f, [&](math_FunctionAllRoots& r) {
        ob(r.IsDone());
        out_counts(f);
        int ni = r.NbIntervals();
        in(ni);
        for (int j = 1; j <= ni; ++j) {
          double a, b; int i1, i2;
          r.GetInterval(j, a, b); r.GetIntervalState(j, i1, i2);
          h(a); h(b); in(i1); in(i2);
        }
        int np = r.NbPoints();
        in(np);
        for (int j = 1; j <= np; ++j) { h(r.GetPoint(j)); in(r.GetPointState(j)); }
      });
      if (!ok) unchecked();
    });
  }
}
static void fam_function_all_roots_access() {
  const char* N = "function_all_roots_access";
  for (int att = 0; att < 6000 && fam_lines[N] < 120; ++att) {
    ARCase c = rand_ar(att);
    edge_ar(att / 6, c);
    if (c.n < 2) c.n = 2 + att % 5;
    int q = att % 6;
    int cnt = safe_int([&] { int k = 0; TF f(c.fn); run_ar(c, f, [&](math_FunctionAllRoots& r) { k = (q <= 2) ? r.NbIntervals() : r.NbPoints(); }); return k; });
    int idx = ridx(cnt);
    begin(N, "tf f f i f f f i i");
    W_ar(c); W_i(q); W_i(idx); bar();
    emit([&] {
      TF f(c.fn);
      bool ok = run_ar(c, f, [&](math_FunctionAllRoots& r) {
        double a, b; int i1, i2;
        switch (q) {
          case 0: in(r.NbIntervals()); break;
          case 1: r.GetInterval(idx, a, b); h(a); h(b); break;
          case 2: r.GetIntervalState(idx, i1, i2); in(i1); in(i2); break;
          case 3: in(r.NbPoints()); break;
          case 4: h(r.GetPoint(idx)); break;
          default: in(r.GetPointState(idx)); break;
        }
      });
      if (!ok) unchecked();
    });
  }
}
static void fam_function_all_roots_dump() {
  const char* N = "function_all_roots_dump";
  for (int att = 0; att < 6000 && fam_lines[N] < 60; ++att) {
    ARCase c = rand_ar(att);
    edge_ar(att, c);
    if (c.n < 2) c.n = 2 + att % 5;
    begin(N, AR_SCHEMA); W_ar(c); bar();
    emit([&] {
      TF f(c.fn);
      bool ok = run_ar(c, f, [&](math_FunctionAllRoots& r) { std::ostringstream os; r.Dump(os); os_text(os.str()); });
      if (!ok) unchecked();
    });
  }
}

// =================================================================================
// math_BracketedRoot
// =================================================================================
struct BRCase { FnSpec fn; double b1, b2, tol; int nbit; double zeps; };
static BRCase rand_br(int att) {
  BRCase c;
  double a, b; rinterval(a, b);
  double lo, hi; lohi(a, b, lo, hi);
  c.fn = fn_rootful(lo, hi, rstate());
  // keep the first two evaluations alive most of the time; later failures keep the previous value
  if (coin(0.15)) add_fail(c.fn, lo, hi, 1.0);
  c.b1 = a; c.b2 = b;
  if (coin(0.15)) { c.b1 = rnd(lo, hi); c.b2 = c.b1 + rnd(-1, 1); }
  double x0, x1, r;
  if (coin(0.8) && find_root(c.fn, lo, hi, x0, x1, r)) {
    double h = x1 - x0;
    c.b1 = x0 - (coin(0.5) ? 0.0 : rnd(0, 2) * h); c.b2 = x1 + (coin(0.5) ? 0.0 : rnd(0, 2) * h);
    if (coin(0.4)) std::swap(c.b1, c.b2);
  }
  static const int NB[] = {0, 1, 2, 3, 4, 6, 10, 20, 100};
  c.nbit = coin(0.7) ? 100 : pick(NB);
  c.tol = coin(0.2) ? 0.0 : rtol();
  static const double ZE[] = {0.0, 1e-15, 1e-12, 1e-12, 1e-9, 1e-6, 1e-3};
  c.zeps = pick(ZE);
  (void)att;
  return c;
}
static void W_br(const BRCase& c) { W_tf(c.fn); W_f(c.b1); W_f(c.b2); W_f(c.tol); W_i(c.nbit); W_f(c.zeps); }
static const char* BR_SCHEMA = "tf f f f i f";
static void edge_br(int att, BRCase& c) {
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 1: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 3; c.b2 = 0; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 2: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 2; c.b2 = 3; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break; // not bracketed
    case 3: c.fn = mk(K_POLY, Vd{0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;     // root at b1 (Fa = 0)
    case 4: c.fn = mk(K_POLY, Vd{-3.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;    // root at b2
    case 5: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.tol = 1e-9; c.nbit = 0; c.zeps = 1e-12; break;
    case 6: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.tol = 1e-9; c.nbit = 1; c.zeps = 1e-12; break;
    case 7: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 0; c.b2 = 3; c.tol = 0.0; c.nbit = 100; c.zeps = 0.0; break;
    case 8: c.fn = mk(K_MULTIPLE, Vd{1.0, 0.5, 3.0, 10.0}, 1); c.b1 = 0; c.b2 = 2; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 9: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.b1 = 1; c.b2 = 1; c.tol = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    default: break;
  }
}
static void fam_bracketed_root() {
  const char* N = "bracketed_root";
  for (int att = 0; att < 8000 && fam_lines[N] < 240; ++att) {
    BRCase c = rand_br(att);
    edge_br(att, c);
    begin(N, BR_SCHEMA); W_br(c); bar();
    emit([&] {
      TF f(c.fn);
      math_BracketedRoot r(f, c.b1, c.b2, c.tol, c.nbit, c.zeps);
      ob(r.IsDone());
      out_counts(f);
      if (r.IsDone()) { h(r.Root()); h(r.Value()); in(r.NbIterations()); }
    });
  }
}
static void fam_bracketed_root_access() {
  const char* N = "bracketed_root_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 90; ++att) {
    BRCase c = rand_br(att);
    edge_br(att / 3, c);
    int q = att % 3;
    begin(N, "tf f f f i f i");
    W_br(c); W_i(q); bar();
    emit([&] {
      TF f(c.fn);
      math_BracketedRoot r(f, c.b1, c.b2, c.tol, c.nbit, c.zeps);
      if (q == 0) h(r.Root()); else if (q == 1) h(r.Value()); else in(r.NbIterations());
    });
  }
}
static void fam_bracketed_root_dump() {
  const char* N = "bracketed_root_dump";
  for (int att = 0; att < 8000 && fam_lines[N] < 40; ++att) {
    BRCase c = rand_br(att);
    edge_br(att, c);
    begin(N, BR_SCHEMA); W_br(c); bar();
    emit([&] {
      TF f(c.fn);
      math_BracketedRoot r(f, c.b1, c.b2, c.tol, c.nbit, c.zeps);
      std::ostringstream os; r.Dump(os); os_text(os.str());
    });
  }
}

// =================================================================================
// math_FunctionRoot (two constructors) -- runs math_FunctionSetRoot with one variable
// =================================================================================
struct FDCase { FnSpec fn; double guess, tol; double a, b; int nbit; };
static FDCase rand_fd(int att, bool bounded) {
  FDCase c;
  double s0, s1; rinterval(s0, s1);
  double lo, hi; lohi(s0, s1, lo, hi);
  c.fn = fn_rootful(lo, hi, rstate());
  if (coin(0.12)) add_fail(c.fn, lo, hi, 1.0);
  c.guess = coin(0.9) ? rnd(lo, hi) : rnz(lo - 3, hi + 3);
  { double x0, x1, r; if (coin(0.7) && find_root(c.fn, lo, hi, x0, x1, r)) c.guess = r + rnz(1e-6, 1.0) * (coin(0.5) ? 0.1 : 1.0); }
  c.tol = coin(0.15) ? 0.0 : rtol();
  c.a = lo; c.b = hi;
  if (bounded) {
    double q = rnd(0, 1);
    if (q < 0.6) { c.a = lo; c.b = hi; }
    else if (q < 0.7) { c.a = hi; c.b = lo; }
    else if (q < 0.8) { c.a = c.guess; c.b = c.guess + rnd(0, 2); }
    else if (q < 0.9) { c.a = rnd(lo, hi); c.b = rnd(lo, hi); }
    else { c.a = c.b = c.guess; }
  }
  static const int NB[] = {0, 1, 2, 3, 5, 10, 20, 50, 100};
  c.nbit = coin(0.5) ? 100 : pick(NB);
  (void)att;
  return c;
}
static void edge_fd(int att, FDCase& c, bool bounded) {
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.tol = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 3; } break;
    case 1: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 1); c.guess = 3.0; c.tol = 1e-12; c.nbit = 100; if (bounded) { c.a = 0; c.b = 5; } break;
    case 2: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.guess = 3.0; c.tol = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 5; } break; // double root
    case 3: c.fn = mk(K_POLY, Vd{1.0, 0.0, 1.0}, 0); c.guess = 3.0; c.tol = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 5; } break; // no root
    case 4: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.tol = 1e-9; c.nbit = 1; if (bounded) { c.a = 0; c.b = 3; } break;
    case 5: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.tol = 1e-9; c.nbit = 0; if (bounded) { c.a = 0; c.b = 3; } break;
    case 6: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 0.0; c.tol = 1e-9; c.nbit = 100; if (bounded) { c.a = -1; c.b = 3; } break; // stationary start
    case 7: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.tol = 0.0; c.nbit = 100; if (bounded) { c.a = 0; c.b = 3; } break;
    case 8: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.5; c.tol = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 1.2; } break; // root outside the bounds
    default: break;
  }
}
static void W_fd(const FDCase& c, bool bounded) { W_tf(c.fn); W_f(c.guess); W_f(c.tol); if (bounded) { W_f(c.a); W_f(c.b); } W_i(c.nbit); }
static void fam_function_root() {
  const char* N = "function_root";
  for (int att = 0; att < 8000 && fam_lines[N] < 240; ++att) {
    FDCase c = rand_fd(att, false); edge_fd(att, c, false);
    begin(N, "tf f f i"); W_fd(c, false); bar();
    emit([&] {
      TF f(c.fn);
      math_FunctionRoot r(f, c.guess, c.tol, c.nbit);
      ob(r.IsDone());
      out_counts(f);
      if (r.IsDone()) { h(r.Root()); h(r.Derivative()); h(r.Value()); in(r.NbIterations()); }
    });
  }
}
static void fam_function_root_bounded() {
  const char* N = "function_root_bounded";
  for (int att = 0; att < 8000 && fam_lines[N] < 240; ++att) {
    FDCase c = rand_fd(att, true); edge_fd(att, c, true);
    begin(N, "tf f f f f i"); W_fd(c, true); bar();
    emit([&] {
      TF f(c.fn);
      math_FunctionRoot r(f, c.guess, c.tol, c.a, c.b, c.nbit);
      ob(r.IsDone());
      out_counts(f);
      if (r.IsDone()) { h(r.Root()); h(r.Derivative()); h(r.Value()); in(r.NbIterations()); }
    });
  }
}
static void fam_function_root_access() {
  const char* N = "function_root_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 100; ++att) {
    bool bounded = att % 2;
    FDCase c = rand_fd(att, bounded); edge_fd(att / 8, c, bounded);
    int q = (att / 2) % 4;
    begin(N, "tf i f f f f i i"); W_tf(c.fn); W_i(bounded); W_f(c.guess); W_f(c.tol); W_f(c.a); W_f(c.b); W_i(c.nbit); W_i(q); bar();
    emit([&] {
      TF f(c.fn);
      math_FunctionRoot* rp = bounded ? new math_FunctionRoot(f, c.guess, c.tol, c.a, c.b, c.nbit) : new math_FunctionRoot(f, c.guess, c.tol, c.nbit);
      std::unique_ptr<math_FunctionRoot> r(rp);
      if (q == 0) h(r->Root()); else if (q == 1) h(r->Derivative()); else if (q == 2) h(r->Value()); else in(r->NbIterations());
    });
  }
}
static void fam_function_root_dump() {
  const char* N = "function_root_dump";
  for (int att = 0; att < 8000 && fam_lines[N] < 40; ++att) {
    bool bounded = att % 2;
    FDCase c = rand_fd(att, bounded); edge_fd(att / 2, c, bounded);
    begin(N, "tf i f f f f i"); W_tf(c.fn); W_i(bounded); W_f(c.guess); W_f(c.tol); W_f(c.a); W_f(c.b); W_i(c.nbit); bar();
    emit([&] {
      TF f(c.fn);
      std::unique_ptr<math_FunctionRoot> r(bounded ? new math_FunctionRoot(f, c.guess, c.tol, c.a, c.b, c.nbit) : new math_FunctionRoot(f, c.guess, c.tol, c.nbit));
      std::ostringstream os; r->Dump(os); os_text(os.str());
    });
  }
}

// =================================================================================
// math_NewtonFunctionRoot
// =================================================================================
struct NCase { FnSpec fn; double guess, ex, ef, a, b; int nbit; };
static NCase rand_nw(int att, bool bounded) {
  NCase c;
  double s0, s1; rinterval(s0, s1);
  double lo, hi; lohi(s0, s1, lo, hi);
  c.fn = fn_rootful(lo, hi, rstate());
  if (coin(0.12)) add_fail(c.fn, lo, hi, 1.0);
  c.guess = coin(0.9) ? rnd(lo, hi) : rnz(lo - 3, hi + 3);
  { double x0, x1, r; if (coin(0.7) && find_root(c.fn, lo, hi, x0, x1, r)) c.guess = r + rnz(1e-6, 1.0) * (coin(0.5) ? 0.1 : 1.0); }
  c.ex = coin(0.15) ? 0.0 : rtol();
  c.ef = coin(0.15) ? 0.0 : rtol();
  c.a = lo; c.b = hi;
  if (bounded) {
    double q = rnd(0, 1);
    if (q < 0.55) { c.a = lo; c.b = hi; }
    else if (q < 0.65) { c.a = hi; c.b = lo; }
    else if (q < 0.8) { c.a = c.guess - rnd(0, 1); c.b = c.guess + rnd(0, 1); }
    else if (q < 0.9) { c.a = rnd(lo, hi); c.b = rnd(lo, hi); }
    else { c.a = c.b = c.guess; }
  }
  static const int NB[] = {0, 1, 2, 3, 5, 10, 20, 50, 100};
  c.nbit = coin(0.5) ? 100 : pick(NB);
  (void)att;
  return c;
}
static void edge_nw(int att, NCase& c, bool bounded) {
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 3; } break;
    case 1: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 1); c.guess = 0.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = -1; c.b = 3; } break; // stationary point
    case 2: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.guess = 3.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 5; } break;
    case 3: c.fn = mk(K_MULTIPLE, Vd{1.0, 0.5, 3.0, 10.0}, 2); c.guess = 2.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 5; } break;
    case 4: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 1; if (bounded) { c.a = 0; c.b = 3; } break;
    case 5: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 0; if (bounded) { c.a = 0; c.b = 3; } break;
    case 6: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.0; c.ex = 0.0; c.ef = 0.0; c.nbit = 100; if (bounded) { c.a = 0; c.b = 3; } break;
    case 7: c.fn = mk(K_POLY, Vd{1.0, 0.0, 1.0}, 0); c.guess = 3.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 5; } break;
    case 8: c.fn = mk(K_POLY, Vd{-2.0, 0.0, 1.0}, 0); c.guess = 1.5; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = 0; c.b = 1.2; } break;
    case 9: c.fn = mk(K_FLAT, Vd{-1.0, 1.0, 1.0, 1.0, 1.0, 0.0}, 0); c.guess = 0.0; c.ex = 1e-9; c.ef = 1e-9; c.nbit = 100; if (bounded) { c.a = -3; c.b = 3; } break; // flat: derivative 0
    default: break;
  }
}
static void W_nw(const NCase& c, bool bounded) { W_tf(c.fn); W_f(c.guess); W_f(c.ex); W_f(c.ef); if (bounded) { W_f(c.a); W_f(c.b); } W_i(c.nbit); }
static void out_newton(math_NewtonFunctionRoot& r) {
  ob(r.IsDone());
  if (r.IsDone()) { h(r.Root()); h(r.Derivative()); h(r.Value()); in(r.NbIterations()); }
}
static void fam_newton_root() {
  const char* N = "newton_root";
  for (int att = 0; att < 8000 && fam_lines[N] < 240; ++att) {
    NCase c = rand_nw(att, false); edge_nw(att, c, false);
    begin(N, "tf f f f i"); W_nw(c, false); bar();
    emit([&] {
      TF f(c.fn);
      math_NewtonFunctionRoot r(f, c.guess, c.ex, c.ef, c.nbit);
      out_newton(r); out_counts(f);
    });
  }
}
static void fam_newton_root_bounded() {
  const char* N = "newton_root_bounded";
  for (int att = 0; att < 8000 && fam_lines[N] < 240; ++att) {
    NCase c = rand_nw(att, true); edge_nw(att, c, true);
    begin(N, "tf f f f f f i"); W_nw(c, true); bar();
    emit([&] {
      TF f(c.fn);
      math_NewtonFunctionRoot r(f, c.guess, c.ex, c.ef, c.a, c.b, c.nbit);
      out_newton(r); out_counts(f);
    });
  }
}
// the constructor without a function, then Perform once or twice (with two functions)
static void fam_newton_root_perform() {
  const char* N = "newton_root_perform";
  for (int att = 0; att < 8000 && fam_lines[N] < 160; ++att) {
    NCase c1 = rand_nw(att, true), c2 = rand_nw(att + 1, true);
    if (att < 10) edge_nw(att, c1, true);
    int twice = coin(0.7) ? 1 : 0;
    begin(N, "f f f f i tf f tf f i");
    W_f(c1.a); W_f(c1.b); W_f(c1.ex); W_f(c1.ef); W_i(c1.nbit);
    W_tf(c1.fn); W_f(c1.guess); W_tf(c2.fn); W_f(c2.guess); W_i(twice); bar();
    emit([&] {
      TF f1(c1.fn), f2(c2.fn);
      math_NewtonFunctionRoot r(c1.a, c1.b, c1.ex, c1.ef, c1.nbit);
      ob(r.IsDone());
      r.Perform(f1, c1.guess);
      out_newton(r); out_counts(f1);
      if (twice) { r.Perform(f2, c2.guess); out_newton(r); out_counts(f2); }
    });
  }
}
static void fam_newton_root_access() {
  const char* N = "newton_root_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 90; ++att) {
    bool bounded = att % 2;
    NCase c = rand_nw(att, bounded); edge_nw(att / 8, c, bounded);
    int q = (att / 2) % 4;
    begin(N, "tf i f f f f f i i"); W_tf(c.fn); W_i(bounded); W_f(c.guess); W_f(c.ex); W_f(c.ef); W_f(c.a); W_f(c.b); W_i(c.nbit); W_i(q); bar();
    emit([&] {
      TF f(c.fn);
      std::unique_ptr<math_NewtonFunctionRoot> r(bounded ? new math_NewtonFunctionRoot(f, c.guess, c.ex, c.ef, c.a, c.b, c.nbit) : new math_NewtonFunctionRoot(f, c.guess, c.ex, c.ef, c.nbit));
      if (q == 0) h(r->Root()); else if (q == 1) h(r->Derivative()); else if (q == 2) h(r->Value()); else in(r->NbIterations());
    });
  }
}
static void fam_newton_root_dump() {
  const char* N = "newton_root_dump";
  for (int att = 0; att < 8000 && fam_lines[N] < 40; ++att) {
    bool bounded = att % 2;
    NCase c = rand_nw(att, bounded); edge_nw(att / 2, c, bounded);
    begin(N, "tf i f f f f f i"); W_tf(c.fn); W_i(bounded); W_f(c.guess); W_f(c.ex); W_f(c.ef); W_f(c.a); W_f(c.b); W_i(c.nbit); bar();
    emit([&] {
      TF f(c.fn);
      std::unique_ptr<math_NewtonFunctionRoot> r(bounded ? new math_NewtonFunctionRoot(f, c.guess, c.ex, c.ef, c.a, c.b, c.nbit) : new math_NewtonFunctionRoot(f, c.guess, c.ex, c.ef, c.nbit));
      std::ostringstream os; r->Dump(os); os_text(os.str());
    });
  }
}

// =================================================================================
// math_BracketMinimum
// =================================================================================
// variant 0: (F, A, B); 1: (F, A, B, FA); 2: (F, A, B, FA, FB)
struct BMCase { FnSpec fn; double a, b, fa, fb; };
static BMCase rand_bm(int att) {
  BMCase c;
  double s0, s1; rinterval(s0, s1);
  double lo, hi; lohi(s0, s1, lo, hi);
  c.fn = fn_minful(lo, hi, rstate());
  if (coin(0.12)) add_fail(c.fn, lo - 1, hi + 1, 1.0);
  c.a = coin(0.7) ? s0 : rnd(lo, hi); c.b = coin(0.7) ? s1 : rnd(lo, hi);
  if (coin(0.08)) c.b = c.a;
  double v, d;
  eval_fn(c.fn, c.a, v, d); c.fa = finv(coin(0.85) ? v : rnd(-2, 2));
  eval_fn(c.fn, c.b, v, d); c.fb = finv(coin(0.85) ? v : rnd(-2, 2));
  (void)att;
  return c;
}
static void edge_bm(int att, BMCase& c) {
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.a = 0; c.b = 0.5; break;
    case 1: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 1); c.a = 3; c.b = 2; break;
    case 2: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 2); c.a = 1; c.b = 1; break;          // A == B
    case 3: c.fn = mk(K_ABS, Vd{1.0, 0.0, 0.0}, 0); c.a = 1; c.b = 2; break;
    case 4: c.fn = mk(K_FLAT, Vd{-1.0, 1.0, 1.0, 1.0, 2.0, 0.0}, 0); c.a = 3; c.b = 2; break; // flat bottom
    case 5: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.a = 1; c.b = 2; break;          // already bracketing the other way
    case 6: c.fn = mk(K_POLY, Vd{0.0, 0.0, 1.0}, 0); c.a = -1; c.b = 1; break;
    case 7: c.fn = mk(K_EXP, Vd{1.0, 1.0}, 0); c.a = 0; c.b = -1; break;                 // minimum at infinity
    default: break;
  }
  double v, d;
  eval_fn(c.fn, c.a, v, d); c.fa = finv(v);
  eval_fn(c.fn, c.b, v, d); c.fb = finv(v);
}
static void out_bm(math_BracketMinimum& m) {
  ob(m.IsDone());
  if (m.IsDone()) {
    double a, b, cc, fa, fb, fc;
    m.Values(a, b, cc); m.FunctionValues(fa, fb, fc);
    h(a); h(b); h(cc); h(fa); h(fb); h(fc);
  }
}
static void fam_bracket_minimum() {
  const char* N = "bracket_minimum";
  for (int att = 0; att < 8000 && fam_lines[N] < 240; ++att) {
    BMCase c = rand_bm(att); edge_bm(att, c);
    int variant = att % 3;
    begin(N, "i tf f f f f"); W_i(variant); W_tf(c.fn); W_f(c.a); W_f(c.b); W_f(c.fa); W_f(c.fb); bar();
    emit([&] {
      TF f(c.fn);
      std::unique_ptr<math_BracketMinimum> m(variant == 0 ? new math_BracketMinimum(f, c.a, c.b)
                                             : variant == 1 ? new math_BracketMinimum(f, c.a, c.b, c.fa)
                                                            : new math_BracketMinimum(f, c.a, c.b, c.fa, c.fb));
      out_bm(*m); out_counts(f);
    });
  }
}
// the two-step form: constructor (A, B), optional SetLimits / SetFA / SetFB, then Perform once or twice
struct BSCase { BMCase c; int limited; double left, right; int setfa, setfb; int performs; };
static BSCase rand_bs(int att) {
  BSCase s; s.c = rand_bm(att);
  double lo = std::min(s.c.a, s.c.b), hi = std::max(s.c.a, s.c.b);
  s.limited = coin(0.5);
  double w = std::max(hi - lo, 1.0);
  double q = rnd(0, 1);
  if (q < 0.5) { s.left = lo - rnd(0, 2) * w; s.right = hi + rnd(0, 3) * w; }
  else if (q < 0.7) { s.left = lo; s.right = hi; }
  else if (q < 0.8) { s.left = hi; s.right = lo; }
  else if (q < 0.9) { s.left = lo + 0.3 * w; s.right = lo + 0.6 * w; }
  else { s.left = s.right = rnd(lo, hi); }
  s.setfa = coin(0.4); s.setfb = coin(0.4);
  s.performs = coin(0.7) ? 1 : (coin(0.5) ? 2 : 0);
  return s;
}
static void edge_bs(int att, BSCase& s) {
  edge_bm(att, s.c);
  if (att < 8) {
    double lo = std::min(s.c.a, s.c.b), hi = std::max(s.c.a, s.c.b);
    s.limited = att % 2; s.left = lo - 1.0; s.right = hi + 1.0; s.setfa = (att / 2) % 2; s.setfb = (att / 4) % 2; s.performs = 1 + att % 2;
  }
}
static void W_bs(const BSCase& s) {
  W_tf(s.c.fn); W_f(s.c.a); W_f(s.c.b); W_i(s.limited); W_f(s.left); W_f(s.right); W_i(s.setfa); W_f(s.c.fa); W_i(s.setfb); W_f(s.c.fb); W_i(s.performs);
}
static const char* BS_SCHEMA = "tf f f i f f i f i f i";
static void run_bs(const BSCase& s, TF& f, math_BracketMinimum& m, bool print) {
  if (s.limited) m.SetLimits(s.left, s.right);
  if (s.setfa) m.SetFA(s.c.fa);
  if (s.setfb) m.SetFB(s.c.fb);
  for (int p = 0; p < s.performs; ++p) { m.Perform(f); if (print) out_bm(m); }
}
static void fam_bracket_minimum_staged() {
  const char* N = "bracket_minimum_staged";
  for (int att = 0; att < 8000 && fam_lines[N] < 260; ++att) {
    BSCase s = rand_bs(att); edge_bs(att, s);
    begin(N, BS_SCHEMA); W_bs(s); bar();
    emit([&] {
      TF f(s.c.fn);
      math_BracketMinimum m(s.c.a, s.c.b);
      ob(m.IsDone());
      run_bs(s, f, m, true);
      out_counts(f);
    });
  }
}
static void fam_bracket_minimum_access() {
  const char* N = "bracket_minimum_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 80; ++att) {
    BSCase s = rand_bs(att); edge_bs(att / 2, s);
    int q = att % 2;
    begin(N, "tf f f i f f i f i f i i"); W_bs(s); W_i(q); bar();
    emit([&] {
      TF f(s.c.fn);
      math_BracketMinimum m(s.c.a, s.c.b);
      run_bs(s, f, m, false);
      // the StdFail_NotDone checks of Values / FunctionValues are compiled out of the library
      if (!m.IsDone()) { unchecked(); return; }
      double a, b, c;
      if (q == 0) m.Values(a, b, c); else m.FunctionValues(a, b, c);
      h(a); h(b); h(c);
    });
  }
}
static void fam_bracket_minimum_dump() {
  const char* N = "bracket_minimum_dump";
  for (int att = 0; att < 8000 && fam_lines[N] < 40; ++att) {
    BSCase s = rand_bs(att); edge_bs(att, s);
    begin(N, BS_SCHEMA); W_bs(s); bar();
    emit([&] {
      TF f(s.c.fn);
      math_BracketMinimum m(s.c.a, s.c.b);
      run_bs(s, f, m, false);
      std::ostringstream os; m.Dump(os); os_text(os.str());
    });
  }
}

// =================================================================================
// math_BrentMinimum
// =================================================================================
// variant 0: (TolX, NbIterations, ZEPS); 1: (TolX, Fbx, NbIterations, ZEPS)
struct BTCase { FnSpec fn; int variant; double tolx, fbx; int nbit; double zeps; double ax, bx, cx; };
static BTCase rand_bt(int att) {
  BTCase c;
  double s0, s1; rinterval(s0, s1);
  double lo, hi; lohi(s0, s1, lo, hi);
  c.fn = fn_minful(lo, hi, rstate());
  if (coin(0.12)) add_fail(c.fn, lo, hi, 1.0);
  c.variant = coin(0.5);
  c.ax = lo; c.cx = hi; c.bx = rnd(lo, hi);
  double q = rnd(0, 1);
  if (q < 0.15) std::swap(c.ax, c.cx);
  else if (q < 0.25) c.bx = coin(0.5) ? lo : hi;
  else if (q < 0.30) c.bx = rnz(hi, hi + 3);
  else if (q < 0.34) c.ax = c.bx = c.cx;
  c.tolx = coin(0.2) ? 0.0 : rtol();
  static const double ZE[] = {0.0, 1e-15, 1e-12, 1e-12, 1e-9, 1e-6, 1e-3};
  c.zeps = pick(ZE);
  static const int NB[] = {0, 1, 2, 3, 5, 10, 20, 50, 100};
  c.nbit = coin(0.5) ? 100 : pick(NB);
  double v, d;
  eval_fn(c.fn, c.bx, v, d);
  c.fbx = finv(coin(0.85) ? v : rnd(-2, 2));
  (void)att;
  return c;
}
static void edge_bt(int att, BTCase& c) {
  switch (att) {
    case 0: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.ax = 0; c.bx = 0.5; c.cx = 3; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 1: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 1); c.ax = 3; c.bx = 0.5; c.cx = 0; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 2: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 2); c.ax = 1; c.bx = 1; c.cx = 1; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 3: c.fn = mk(K_ABS, Vd{1.0, 0.3, 0.0}, 0); c.ax = 0; c.bx = 1; c.cx = 2; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 4: c.fn = mk(K_POLY, Vd{0.0, 1.0}, 0); c.ax = 0; c.bx = 1; c.cx = 2; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;   // minimum at the end
    case 5: c.fn = mk(K_POLY, Vd{0.0, -1.0}, 0); c.ax = 0; c.bx = 1; c.cx = 2; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 6: c.fn = mk(K_FLAT, Vd{-1.0, 1.0, 1.0, 1.0, 2.0, 0.0}, 0); c.ax = -3; c.bx = 0.2; c.cx = 3; c.tolx = 1e-9; c.nbit = 100; c.zeps = 1e-12; break;
    case 7: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.ax = 0; c.bx = 0.5; c.cx = 3; c.tolx = 1e-9; c.nbit = 1; c.zeps = 1e-12; break;
    case 8: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.ax = 0; c.bx = 0.5; c.cx = 3; c.tolx = 0.0; c.nbit = 100; c.zeps = 0.0; break;
    case 9: c.fn = mk(K_POLY, Vd{1.0, -2.0, 1.0}, 0); c.ax = 0; c.bx = 0.5; c.cx = 3; c.tolx = 1e-9; c.nbit = 0; c.zeps = 1e-12; break;
    default: break;
  }
  double v, d;
  eval_fn(c.fn, c.bx, v, d); c.fbx = finv(v);
}
static void W_bt(const BTCase& c) { W_i(c.variant); W_f(c.tolx); W_f(c.fbx); W_i(c.nbit); W_f(c.zeps); W_tf(c.fn); W_f(c.ax); W_f(c.bx); W_f(c.cx); }
static const char* BT_SCHEMA = "i f f i f tf f f f";
static void out_brent(math_BrentMinimum& m) {
  ob(m.IsDone());
  if (m.IsDone()) { h(m.Location()); h(m.Minimum()); in(m.NbIterations()); }
}
static std::unique_ptr<math_BrentMinimum> mk_brent(const BTCase& c) {
  return std::unique_ptr<math_BrentMinimum>(c.variant == 0 ? new math_BrentMinimum(c.tolx, c.nbit, c.zeps) : new math_BrentMinimum(c.tolx, c.fbx, c.nbit, c.zeps));
}
static void fam_brent_minimum() {
  const char* N = "brent_minimum";
  for (int att = 0; att < 8000 && fam_lines[N] < 260; ++att) {
    BTCase c = rand_bt(att); edge_bt(att, c);
    begin(N, BT_SCHEMA); W_bt(c); bar();
    emit([&] {
      TF f(c.fn);
      auto m = mk_brent(c);
      m->Perform(f, c.ax, c.bx, c.cx);
      out_brent(*m); out_counts(f);
    });
  }
}
// Perform twice on the same object (the second with another function and bracket)
static void fam_brent_minimum_twice() {
  const char* N = "brent_minimum_twice";
  for (int att = 0; att < 8000 && fam_lines[N] < 120; ++att) {
    BTCase c = rand_bt(att), d = rand_bt(att + 7);
    if (att < 10) edge_bt(att, c);
    begin(N, "i f f i f tf f f f tf f f f"); W_i(c.variant); W_f(c.tolx); W_f(c.fbx); W_i(c.nbit); W_f(c.zeps);
    W_tf(c.fn); W_f(c.ax); W_f(c.bx); W_f(c.cx); W_tf(d.fn); W_f(d.ax); W_f(d.bx); W_f(d.cx); bar();
    emit([&] {
      TF f1(c.fn), f2(d.fn);
      auto m = mk_brent(c);
      m->Perform(f1, c.ax, c.bx, c.cx);
      out_brent(*m); out_counts(f1);
      m->Perform(f2, d.ax, d.bx, d.cx);
      out_brent(*m); out_counts(f2);
    });
  }
}
// a sub-class that redefines IsSolutionReached (it is virtual in OCCT)
//   mode 1: default test, or fx <= thr
//   mode 2: default test, or (F(0.5 (a + b)) is computable and F(0.5 (a + b)) < fx - thr): calls the function
//   mode 3: default test and |fx| <= thr
class BrentCustom : public math_BrentMinimum {
public:
  int mode; double thr;
  BrentCustom(const BTCase& c, int m, double t)
      : math_BrentMinimum(c.tolx, c.nbit, c.zeps), mode(m), thr(t) {}
  BrentCustom(const BTCase& c, int m, double t, int)
      : math_BrentMinimum(c.tolx, c.fbx, c.nbit, c.zeps), mode(m), thr(t) {}
  bool IsSolutionReached(math_Function& F) override {
    bool def = math_BrentMinimum::IsSolutionReached(F);
    if (mode == 1) return def || fx <= thr;
    if (mode == 2) { double t; bool ok = F.Value(0.5 * (a + b), t); return def || (ok && t < fx - thr); }
    return def && std::fabs(fx) <= thr;
  }
};
static void fam_brent_minimum_custom() {
  const char* N = "brent_minimum_custom";
  for (int att = 0; att < 8000 && fam_lines[N] < 120; ++att) {
    BTCase c = rand_bt(att);
    int mode = 1 + att % 3;
    double thr = mode == 2 ? rnd(-0.5, 0.5) : (coin(0.5) ? rnd(0.0, 3.0) : rnd(-1.0, 1.0));
    begin(N, "i f f i f tf f f f i f"); W_i(c.variant); W_f(c.tolx); W_f(c.fbx); W_i(c.nbit); W_f(c.zeps);
    W_tf(c.fn); W_f(c.ax); W_f(c.bx); W_f(c.cx); W_i(mode); W_f(thr); bar();
    emit([&] {
      TF f(c.fn);
      std::unique_ptr<BrentCustom> m(c.variant == 0 ? new BrentCustom(c, mode, thr) : new BrentCustom(c, mode, thr, 0));
      m->Perform(f, c.ax, c.bx, c.cx);
      out_brent(*m); out_counts(f);
    });
  }
}
static void fam_brent_minimum_access() {
  const char* N = "brent_minimum_access";
  for (int att = 0; att < 8000 && fam_lines[N] < 80; ++att) {
    BTCase c = rand_bt(att); edge_bt(att / 3, c);
    int q = att % 3;
    begin(N, "i f f i f tf f f f i"); W_i(c.variant); W_f(c.tolx); W_f(c.fbx); W_i(c.nbit); W_f(c.zeps); W_tf(c.fn); W_f(c.ax); W_f(c.bx); W_f(c.cx); W_i(q); bar();
    emit([&] {
      TF f(c.fn);
      auto m = mk_brent(c);
      m->Perform(f, c.ax, c.bx, c.cx);
      if (q == 0) h(m->Location()); else if (q == 1) h(m->Minimum()); else in(m->NbIterations());
    });
  }
}
static void fam_brent_minimum_dump() {
  const char* N = "brent_minimum_dump";
  for (int att = 0; att < 8000 && fam_lines[N] < 40; ++att) {
    BTCase c = rand_bt(att); edge_bt(att, c);
    begin(N, BT_SCHEMA); W_bt(c); bar();
    emit([&] {
      TF f(c.fn);
      auto m = mk_brent(c);
      m->Perform(f, c.ax, c.bx, c.cx);
      std::ostringstream os; m->Dump(os); os_text(os.str());
    });
  }
}

// =================================================================================
// math_TrigonometricFunctionRoots, math_TrigonometricEquationFunction
// =================================================================================
// A cos^2 + 2 B cos sin + C cos + D sin + E = 0 on [inf, sup]
struct TRCase { double a, b, c, d, e, inf, sup; };
static double rcoef() {
  double q = rnd(0, 1);
  if (q < 0.30) return 0.0;
  if (q < 0.34) return rnz(1e-14, 1e-12);       // below OCCT's own epsilon 1.5e-12
  if (q < 0.38) return rnz(1e-11, 1e-9);
  if (q < 0.55) { static const double V[] = {1.0, -1.0, 2.0, -2.0, 0.5, -0.5, 3.0}; return pick(V); }
  return rnz(0.05, 4.0);
}
static void rtrange(double& inf, double& sup) {
  static const double T = 6.283185307179586;
  double q = rnd(0, 1);
  if (q < 0.25) { inf = 0; sup = T; }
  else if (q < 0.35) { inf = -3.141592653589793; sup = 3.141592653589793; }
  else if (q < 0.43) { inf = 0; sup = 1; }
  else if (q < 0.50) { inf = -10; sup = 10; }
  else if (q < 0.54) { inf = 0.3; sup = 0.3; }
  else if (q < 0.58) { inf = 2; sup = 1; }
  else if (q < 0.65) { inf = -DBL_MAX; sup = DBL_MAX; }
  else if (q < 0.69) { inf = -DBL_MAX; sup = rnd(-4, 8); }
  else if (q < 0.73) { inf = rnd(-4, 8); sup = DBL_MAX; }
  else if (q < 0.77) { inf = 0; sup = 100; }
  else if (q < 0.80) { inf = -1e3; sup = 1e3; }
  else if (q < 0.85) { inf = 3.141592653589793; sup = 3.141592653589793 + 1e-3; }
  else { inf = rnd(-8, 8); sup = inf + rnd(0.01, 9.0); }
}
static TRCase rand_tr(int att) {
  TRCase c;
  double q = rnd(0, 1);
  c.a = c.b = c.c = c.d = c.e = 0;
  if (q < 0.30) { c.a = rcoef(); c.b = rcoef(); c.c = rcoef(); c.d = rcoef(); c.e = rcoef(); }
  else if (q < 0.42) { c.b = rcoef(); c.c = rcoef(); c.d = rcoef(); }                       // A = E = 0: the two analytic cases
  else if (q < 0.50) { c.b = rcoef(); c.d = rcoef(); }
  else if (q < 0.58) { c.b = rcoef(); c.c = rcoef(); }
  else if (q < 0.66) { c.c = rcoef(); c.d = rcoef(); c.e = rcoef(); }                         // second degree in tan(x/2)
  else if (q < 0.74) { c.c = rnz(0.5, 3.0); c.e = rnz(0.1, 3.0); }                            // c cos + e
  else if (q < 0.82) { c.d = rnz(0.5, 3.0); c.e = rnz(0.1, 3.0); }                            // d sin + e
  else if (q < 0.86) { c.e = rcoef(); }
  else {
    // build a root: a given x0 solves it
    c.a = rcoef(); c.b = rcoef(); c.c = rcoef(); c.d = rcoef();
    double x0 = rnd(-4, 7);
    double cn = std::cos(x0), sn = std::sin(x0);
    c.e = -(c.a * cn * cn + 2 * c.b * cn * sn + c.c * cn + c.d * sn);
  }
  rtrange(c.inf, c.sup);
  (void)att;
  return c;
}
static void edge_tr(int att, TRCase& c) {
  switch (att) {
    case 0: c = {0, 0, 0, 0, 0, 0, 6.283185307179586}; break;                            // everything zero: infinitely many roots
    case 1: c = {0, 0, 0, 0, 1.0, 0, 6.283185307179586}; break;                          // 1 = 0: no root
    case 2: c = {0, 0, 0, 1.0, 0.0, 0, 6.283185307179586}; break;                        // sin = 0
    case 3: c = {0, 0, 1.0, 0.0, 0.0, 0, 6.283185307179586}; break;                      // cos = 0
    case 4: c = {0, 0, 0, 1.0, 0.5, -DBL_MAX, DBL_MAX}; break;                           // unbounded
    case 5: c = {0, 0, 0, 1.0, 1.0, 0, 6.283185307179586}; break;                        // |e/d| = 1: tangent
    case 6: c = {1.0, 0, 0, 0, -0.25, 0, 6.283185307179586}; break;                      // cos^2 = 1/4
    case 7: c = {0, 1.0, 0, 0, 0, 0, 6.283185307179586}; break;                          // sin cos = 0
    case 8: c = {0, 1.0, 0, 1.0, 0, 0, 6.283185307179586}; break;                         // A = E = C = 0
    case 9: c = {0, 1.0, 1.0, 0, 0, 0, 6.283185307179586}; break;                         // A = E = D = 0
    case 10: c = {1.0, 0, 0, 0, 0, 0, 6.283185307179586}; break;                          // cos^2 = 0 (double roots)
    case 11: c = {1.0, 0, -2.0, 0, 1.0, 0, 6.283185307179586}; break;                     // (cos - 1)^2
    case 12: c = {1.0, 1.0, 1.0, 1.0, 1.0, -3.141592653589793, 3.141592653589793}; break;
    case 13: c = {1.0, 0, 1.0, 0, 0, 0, 6.283185307179586}; break;                        // A - C + E = 0: the root at pi
    default: break;
  }
}
static void W_tr(const TRCase& c) { W_f(c.a); W_f(c.b); W_f(c.c); W_f(c.d); W_f(c.e); W_f(c.inf); W_f(c.sup); }
static void out_trig(math_TrigonometricFunctionRoots& r) {
  ob(r.IsDone());
  ob(r.InfiniteRoots());
  if (r.IsDone() && !r.InfiniteRoots()) {
    in(r.NbSolutions());
    for (int j = 1; j <= r.NbSolutions(); ++j) h(r.Value(j));
  }
}
static void fam_trig_roots() {
  const char* N = "trig_roots";
  for (int att = 0; att < 20000 && fam_lines[N] < 400; ++att) {
    TRCase c = rand_tr(att); edge_tr(att, c);
    begin(N, "f f f f f f f"); W_tr(c); bar();
    emit([&] { math_TrigonometricFunctionRoots r(c.a, c.b, c.c, c.d, c.e, c.inf, c.sup); out_trig(r); }, true);
  }
}
static void fam_trig_roots_cde() {
  const char* N = "trig_roots_cde";
  for (int att = 0; att < 20000 && fam_lines[N] < 160; ++att) {
    TRCase c = rand_tr(att); c.a = c.b = 0.0;
    if (att % 5 == 0) c.c = rcoef();
    begin(N, "f f f f f"); W_f(c.c); W_f(c.d); W_f(c.e); W_f(c.inf); W_f(c.sup); bar();
    emit([&] { math_TrigonometricFunctionRoots r(c.c, c.d, c.e, c.inf, c.sup); out_trig(r); }, true);
  }
}
static void fam_trig_roots_de() {
  const char* N = "trig_roots_de";
  for (int att = 0; att < 20000 && fam_lines[N] < 120; ++att) {
    TRCase c = rand_tr(att); c.a = c.b = c.c = 0.0;
    begin(N, "f f f f"); W_f(c.d); W_f(c.e); W_f(c.inf); W_f(c.sup); bar();
    emit([&] { math_TrigonometricFunctionRoots r(c.d, c.e, c.inf, c.sup); out_trig(r); }, true);
  }
}
static void fam_trig_roots_access() {
  const char* N = "trig_roots_access";
  for (int att = 0; att < 20000 && fam_lines[N] < 120; ++att) {
    TRCase c = rand_tr(att); edge_tr(att / 3, c);
    int nsol = safe_int([&] { math_TrigonometricFunctionRoots r(c.a, c.b, c.c, c.d, c.e, c.inf, c.sup); return (r.IsDone() && !r.InfiniteRoots()) ? r.NbSolutions() : 0; });
    int q = att % 2;
    int idx = ridx(nsol);
    begin(N, "f f f f f f f i i"); W_tr(c); W_i(q); W_i(idx); bar();
    emit([&] {
      math_TrigonometricFunctionRoots r(c.a, c.b, c.c, c.d, c.e, c.inf, c.sup);
      if (q == 0) in(r.NbSolutions()); else h(r.Value(idx));
    }, true);
  }
}
static void fam_trig_roots_dump() {
  const char* N = "trig_roots_dump";
  for (int att = 0; att < 20000 && fam_lines[N] < 40; ++att) {
    TRCase c = rand_tr(att); edge_tr(att, c);
    begin(N, "f f f f f f f"); W_tr(c); bar();
    emit([&] { math_TrigonometricFunctionRoots r(c.a, c.b, c.c, c.d, c.e, c.inf, c.sup); std::ostringstream os; r.Dump(os); os_text(os.str()); }, true);
  }
}
static void fam_trig_equation_function() {
  const char* N = "trig_equation_function";
  for (int att = 0; att < 3000 && fam_lines[N] < 160; ++att) {
    TRCase c = rand_tr(att);
    double x = coin(0.2) ? rnd(-20, 20) : rnd(-4, 7);
    if (att % 11 == 0) x = 0.0;
    begin(N, "f f f f f f"); W_f(c.a); W_f(c.b); W_f(c.c); W_f(c.d); W_f(c.e); W_f(x); bar();
    emit([&] {
      math_TrigonometricEquationFunction t(c.a, c.b, c.c, c.d, c.e);
      double v, d, v2, d2;
      bool o1 = t.Value(x, v), o2 = t.Derivative(x, d), o3 = t.Values(x, v2, d2);
      ob(o1); h(v); ob(o2); h(d); ob(o3); h(v2); h(d2);
      in(t.GetStateNumber());
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
static const char* ALL_FAMILIES[] = {
  "test_function", "function_sample", "function_roots", "function_roots_access", "function_roots_dump",
  "function_all_roots", "function_all_roots_access", "function_all_roots_dump",
  "bracketed_root", "bracketed_root_access", "bracketed_root_dump",
  "function_root", "function_root_bounded", "function_root_access", "function_root_dump",
  "newton_root", "newton_root_bounded", "newton_root_perform", "newton_root_access", "newton_root_dump",
  "bracket_minimum", "bracket_minimum_staged", "bracket_minimum_access", "bracket_minimum_dump",
  "brent_minimum", "brent_minimum_twice", "brent_minimum_custom", "brent_minimum_access", "brent_minimum_dump",
  "trig_roots", "trig_roots_cde", "trig_roots_de", "trig_roots_access", "trig_roots_dump", "trig_equation_function"};
int main(int argc, char** argv) {
  signal(SIGSEGV, on_crash); signal(SIGABRT, on_crash); signal(SIGFPE, on_crash);
  alarm(3000);
  init_tables();
  const char* only = argc > 1 ? argv[1] : nullptr;
  auto want = [&](const char* n) {
    if (!only) return true;
    std::string o = std::string(",") + only + ",", k = std::string(",") + n + ",";
    return o.find(k) != std::string::npos;
  };
  if (want("test_function")) fam_test_function();
  if (want("function_sample")) fam_function_sample();
  if (want("function_roots")) { fam_function_roots(); fam_function_roots_access(); fam_function_roots_dump(); }
  if (want("function_all_roots")) { fam_function_all_roots(); fam_function_all_roots_access(); fam_function_all_roots_dump(); }
  if (want("bracketed_root")) { fam_bracketed_root(); fam_bracketed_root_access(); fam_bracketed_root_dump(); }
  if (want("function_root")) { fam_function_root(); fam_function_root_bounded(); fam_function_root_access(); fam_function_root_dump(); }
  if (want("newton_root")) { fam_newton_root(); fam_newton_root_bounded(); fam_newton_root_perform(); fam_newton_root_access(); fam_newton_root_dump(); }
  if (want("bracket_minimum")) { fam_bracket_minimum(); fam_bracket_minimum_staged(); fam_bracket_minimum_access(); fam_bracket_minimum_dump(); }
  if (want("brent_minimum")) { fam_brent_minimum(); fam_brent_minimum_twice(); fam_brent_minimum_custom(); fam_brent_minimum_access(); fam_brent_minimum_dump(); }
  if (want("trig")) { fam_trig_roots(); fam_trig_roots_cde(); fam_trig_roots_de(); fam_trig_roots_access(); fam_trig_roots_dump(); fam_trig_equation_function(); }
  for (const char* n : ALL_FAMILIES) {
    if (only && !fam_lines.count(n) && !fam_dropped.count(n)) continue;
    std::fprintf(stderr, "%-28s %5d lines, %5d dropped (%d uninit)\n", n, fam_lines.count(n) ? fam_lines[n] : 0,
                 fam_dropped.count(n) ? fam_dropped[n] : 0, fam_dropped.count(std::string(n) + "!uninit") ? fam_dropped[std::string(n) + "!uninit"] : 0);
    if (fam_dropped.count(std::string(n) + "!libm") || fam_dropped.count(std::string(n) + "~changed"))
      std::fprintf(stderr, "   libm-perturbed: %d dropped, %d (case, salt) runs changed bits\n", fam_dropped[std::string(n) + "!libm"], fam_dropped[std::string(n) + "~changed"]);
  }
  return 0;
}
