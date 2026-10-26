// Deterministic reference calls for OCCT 8.0.1's 2D geometry: package Geom2d (cpp/Geom2d/, TKG2d) and the parts of Geom2dEval it
// uses (Geom2dEval_RepCurveDesc, Geom2dEval_RepUtils): Geom2d_Geometry (Mirror, Rotate, Scale, Translate, Transform and the
// Mirrored / Rotated / Scaled / Translated / Transformed copies, Copy), Geom2d_CartesianPoint, Geom2d_Direction,
// Geom2d_VectorWithMagnitude, Geom2d_AxisPlacement, Geom2d_Transformation, Geom2d_Curve (the evaluations, Reverse / Reversed,
// ReversedParameter, TransformedParameter, ParametricTransformation, IsClosed, IsPeriodic, Period, Continuity, IsCN), Geom2d_Line,
// Geom2d_Conic with Geom2d_Circle / Geom2d_Ellipse / Geom2d_Hyperbola / Geom2d_Parabola, Geom2d_BoundedCurve with
// Geom2d_BezierCurve, Geom2d_BSplineCurve and Geom2d_TrimmedCurve, Geom2d_OffsetCurve, and the evaluation representations that
// Bezier, B-spline and offset curves can carry. DumpJson is not covered.
//
// Output: one record per line, `<family> <inputs...> | <outputs...>`. Floats are 16-hex-digit f64 bit patterns, ints and bools are
// decimal. No text token occurs (the escaping `\\ \n \t \r \s \e` of oracle/poly.cpp is implemented in os_text() for completeness
// and is not used). Every input is printed on the left of ` | ` in the order of the family's schema (`begin(name, schema)`): the
// words `f` `i` (scalars), `tr` (a transformation recipe), `crv` (a curve recipe, recursive for trimmed and offset curves) and
// `ops` (a sequence of operations on the object, each with its code and f64 arguments); their grammar is in docs/api-geom2d.md.
// The oracle aborts if the printed tokens do not match the schema. The outputs are the values the operations return, in order
// (the `state` of an object after the operations that change it), `throws` (OCCT raised an exception: the Rust API must return
// Err) or `unchecked` (a Standard_*_Raise_if check of OCCT is compiled out of the libraries and the call has no defined result
// in OCCT: the call is not executed and the Rust API must return Err). A sequence of operations stops at the first one that throws
// or is unchecked, and the whole line then prints that marker.
// Every call is run 6 times with differently filled stack and heap (random words, all 0, all 1; malloc perturbation); a case whose
// results differ, that is not finite, or that crashes in a child process, is dropped. Families that call sin / cos / tan / sinh /
// cosh / atan2 / acos / pow only keep cases whose outputs are unchanged (within 2 ulps) when every C library result is moved by an
// ulp. Build: c++ -std=c++17 -O2 oracle/geom2d.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKG2d -lTKMath -lTKernel -ldl
// unspecified or reads outside an array: the call is not executed and the Rust API returns an error). A GridPolynomialToPoles that
// is not done prints `0 unchecked` (its accessors are unchecked).
// Every call is run 6 times with differently filled stack and heap (random words, all 0, all 1; malloc perturbation); a case whose
// results differ, that is not finite, or that crashes in a child process, is dropped. Families that call sin / cos / tan / sinh /
// cosh / atan2 only keep cases whose outputs are unchanged (within 2 ulps) when every C library result is moved by an ulp.
#include <algorithm>
#include <array>
#include <atomic>
#include <cfloat>
#include <climits>
#include <cmath>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <iomanip>
#include <iostream>
#include <map>
#include <memory>
#include <set>
#include <sstream>
#include <string>
#include <vector>
#include <typeinfo>
#include <malloc.h>
#include <unistd.h>
#include <sys/wait.h>
#include <Geom2d_AxisPlacement.hxx>
#include <Geom2d_BSplineCurve.hxx>
#include <Geom2d_BezierCurve.hxx>
#include <Geom2d_BoundedCurve.hxx>
#include <Geom2d_CartesianPoint.hxx>
#include <Geom2d_Circle.hxx>
#include <Geom2d_Conic.hxx>
#include <Geom2d_Curve.hxx>
#include <Geom2d_Direction.hxx>
#include <Geom2d_Ellipse.hxx>
#include <Geom2d_Geometry.hxx>
#include <Geom2d_Hyperbola.hxx>
#include <Geom2d_Line.hxx>
#include <Geom2d_OffsetCurve.hxx>
#include <Geom2d_Parabola.hxx>
#include <Geom2d_Point.hxx>
#include <Geom2d_Transformation.hxx>
#include <Geom2d_TrimmedCurve.hxx>
#include <Geom2d_Vector.hxx>
#include <Geom2d_VectorWithMagnitude.hxx>
#include <Geom2dEval_RepCurveDesc.hxx>
#include <GeomAbs_BSplKnotDistribution.hxx>
#include <GeomAbs_Shape.hxx>
#include <BSplCLib.hxx>
#include <NCollection_Array1.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <gp.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Trsf2d.hxx>
#include <gp_Vec2d.hxx>
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

static bool g_quiet = false;   // planning pass: nothing is printed
static void h(double v) {
  if (g_quiet) return;
  if (!std::isfinite(v)) {
    if (!outphase) { std::fprintf(stderr, "non-finite input in %s: %s\n", cur_name.c_str(), buf.c_str()); std::abort(); }
    throw Discard();
  }
  uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits);
  buf += t; ++ntok;
}
// like h(), but an infinite output is allowed (an overflow of an exact + - * / expression is deterministic)
static void hi(double v) {
  if (g_quiet) return;
  if (std::isinf(v) && outphase) { uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits); buf += t; ++ntok; return; }
  h(v);
}
static void in(long v) { if (g_quiet) return; buf += " " + std::to_string(v); ++ntok; }
static void ob(bool v) { in(v ? 1 : 0); }
// A text as one token, escaped so that it decodes exactly: \\ for a backslash, \n, \t, \s for a
// space, and \e alone for the empty text (which would otherwise print nothing).
static void os_text(const std::string& s) {
  if (g_quiet) return;
  std::string t;
  for (char c : s) {
    if (c == '\\') t += "\\\\";
    else if (c == '\n') t += "\\n";
    else if (c == '\t') t += "\\t";
    else if (c == ' ') t += "\\s";
    else if (c == '\r') t += "\\r";
    else t += c;
  }
  if (t.empty()) t = "\\e";
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
// schema / record handling and the re-run harness (as in oracle/math1d.cpp and oracle/convert.cpp)
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
  if (std::getenv("GEOM2D_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
  if (std::find(fam_order.begin(), fam_order.end(), cur_name) == fam_order.end()) fam_order.push_back(cur_name);
  fam_lines[cur_name]++;
  buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear(); outphase = false;
}
static void unchecked() { buf.resize(mark); buf += " unchecked"; }

// ---------------------------------------------------------------------------------
// The C library's transcendental functions, interposed so that a case can be re-run with every result moved by
// 0 or 1 ulp (a different libm gives such differences, and tests/geom2d.rs compares the families that use them within a
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
double sinh(double x) noexcept { REAL1(sinh); return pert(real_sinh(x), x, 9); }
double cosh(double x) noexcept { REAL1(cosh); return pert(real_cosh(x), x, 10); }
double tanh(double x) noexcept { REAL1(tanh); return pert(real_tanh(x), x, 11); }
double asin(double x) noexcept { REAL1(asin); return pert(real_asin(x), x, 4); }
double acos(double x) noexcept { REAL1(acos); return pert(real_acos(x), x, 5); }
double atan(double x) noexcept { REAL1(atan); return pert(real_atan(x), x, 6); }
double atan2(double y, double x) noexcept {
  static double (*real_atan2)(double, double) = nullptr;
  if (!real_atan2) real_atan2 = (double (*)(double, double))dlsym(RTLD_NEXT, "atan2");
  return pert(real_atan2(y, x), y + 3.0 * x, 7);
}
double exp(double x) noexcept { REAL1(exp); return pert(real_exp(x), x, 12); }
double log(double x) noexcept { REAL1(log); return pert(real_log(x), x, 13); }
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
    a[i] = k == 4 ? 0ull : (k == 5 ? ~0ull : (r ^ (r >> 31)));
  }
  asm volatile("" ::: "memory");
}
static const int POISON_RUNS = 6;

static std::string run_once(const std::function<void()>& body, int poison, bool& discard) {
  std::string left = buf.substr(0, mark);
  mallopt(M_PERTURB, poison == 4 ? 0x01 : (poison == 5 ? 0xFE : ((0x37 + 0x4B * poison) & 0xff)));
  poison_stack(poison);
  discard = false;
  try {
    body();
  } catch (Standard_Failure& e) {
    if (poison == 0 && std::getenv("GEOM2D_WHY")) std::fprintf(stderr, "WHY %s: %s %s\n", cur_name.c_str(), typeid(e).name(), e.what());
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
// `libm_robust` the line is only kept when moving every sin / cos / ... result of the C library by an ulp (24
// different pseudo-random patterns) leaves the outputs within 2 ulps (the test allows 8)
// a call that may crash (it reads uninitialised memory): run it once in a child process first
static bool g_guard = false;
static bool survives(const std::function<void()>& body) {
  fflush(stdout); fflush(stderr);
  pid_t p = fork();
  if (p == 0) {
    signal(SIGSEGV, SIG_DFL); signal(SIGABRT, SIG_DFL); signal(SIGFPE, SIG_DFL);
    alarm(10); mallopt(M_PERTURB, 0x6b); poison_stack(7);
    try { body(); } catch (Standard_Failure&) {} catch (Discard&) {} catch (std::exception&) { _exit(3); }
    _exit(0);
  }
  int st = 0; waitpid(p, &st, 0);
  bool ok = WIFEXITED(st) && WEXITSTATUS(st) == 0;
  if (!ok && std::getenv("GEOM2D_DEBUG")) std::fprintf(stderr, "CRASH %s\n", buf.c_str());
  return ok;
}
static bool g_plan_failed;
// runs the body POISON_RUNS times (and, for libm_robust, with the C library results moved by an ulp); returns 0 when the line is
// kept (its text in r1), 1 when it must be dropped (not finite), 2 (not reproducible: an uninitialised read), 3 (not stable
// under a different C library); `changed` is set when a perturbed run differed from the first
static int emit_core(const std::function<void()>& body, bool libm_robust, double robust_ulps, std::string& r1, bool& changed) {
  size_t keep_ntok = (size_t)ntok;
  changed = false;
  for (int k = 0; k < POISON_RUNS; ++k) {
    bool dk = false;
    std::string rk = run_once(body, k, dk);
    ntok = (int)keep_ntok;
    if (dk) return 1;
    if (k == 0) r1 = rk;
    else if (rk != r1) return 2;
  }
  if (libm_robust) {
    for (uint64_t salt = 1; salt <= 24; ++salt) {
      g_perturb = true; g_salt = salt;
      bool dp = false;
      std::string rp = run_once(body, 0, dp);
      g_perturb = false;
      ntok = (int)keep_ntok;
      if (!dp && rp != r1) changed = true;
      bool ok = !dp && lines_close(r1, rp, robust_ulps);
      if (!ok) return 3;
    }
  }
  return 0;
}
static bool emit(const std::function<void()>& body, bool libm_robust = false, double robust_ulps = 2.0) {
  if (g_plan_failed) { g_plan_failed = false; fam_dropped[cur_name]++; fam_dropped[cur_name + "!plancrash"]++; if (std::getenv("GEOM2D_DROPLOG")) std::fprintf(stderr, "DROP %s plancrash %s\n", cur_name.c_str(), buf.substr(0, 160).c_str()); buf.clear(); return false; }
  int st = 0; std::string r1; bool changed = false;
  if (g_guard) {
    // the whole check runs in a child process (a call that OCCT does not check may corrupt the heap or crash): the parent only
    // receives the verdict and the text of the line
    int fd[2];
    if (pipe(fd) != 0) std::abort();
    fflush(stdout); fflush(stderr);
    pid_t p = fork();
    if (p == 0) {
      close(fd[0]);
      signal(SIGSEGV, SIG_DFL); signal(SIGABRT, SIG_DFL); signal(SIGFPE, SIG_DFL);
      alarm(30);
      int s = emit_core(body, libm_robust, robust_ulps, r1, changed);
      std::string out; out.push_back((char)s); out.push_back(changed ? 1 : 0); out += r1;
      size_t off = 0;
      while (off < out.size()) { ssize_t w = write(fd[1], out.data() + off, out.size() - off); if (w <= 0) _exit(4); off += (size_t)w; }
      _exit(0);
    }
    close(fd[1]);
    std::string out; char tmp[8192]; ssize_t n;
    while ((n = read(fd[0], tmp, sizeof tmp)) > 0) out.append(tmp, (size_t)n);
    close(fd[0]);
    int status = 0; waitpid(p, &status, 0);
    if (!(WIFEXITED(status) && WEXITSTATUS(status) == 0) || out.size() < 2) {
      fam_dropped[cur_name]++; fam_dropped[cur_name + "!crash"]++; if (std::getenv("GEOM2D_DROPLOG")) std::fprintf(stderr, "DROP %s crash %s\n", cur_name.c_str(), buf.substr(0, 160).c_str()); buf.clear(); return false;
    }
    st = (int)out[0]; changed = out[1] != 0; r1 = out.substr(2);
  } else {
    st = emit_core(body, libm_robust, robust_ulps, r1, changed);
  }
  if (changed) fam_dropped[cur_name + "~changed"]++;
  if (st != 0) {
    fam_dropped[cur_name]++;
    if (st == 2) fam_dropped[cur_name + "!uninit"]++;
    if (st == 3) fam_dropped[cur_name + "!libm"]++;
    if (std::getenv("GEOM2D_DROPLOG")) std::fprintf(stderr, "DROP %s st%d %s\n", cur_name.c_str(), st, buf.substr(0, 160).c_str());
    buf.clear(); return false;
  }
  buf = r1;
  finish();
  return true;
}

// =================================================================================
static const char* g_only_fam = nullptr;
static bool fam_wanted(const char* n) {
  if (!g_only_fam) return true;
  std::string o = std::string(",") + g_only_fam + ",", k = std::string(",") + n + ",";
  return o.find(k) != std::string::npos;
}
#define LOOP(NAME, TARGET) const char* N = NAME; for (int att = 0; fam_wanted(N) && att < 30000 && fam_lines[N] < std::min((int)(TARGET), 18); ++att)

// =================================================================================
// small helpers
static double nice() { return 0.5 * rint(-8, 8); }
static double crd() { return coin(0.4) ? nice() : rnd(-6, 6); }
static double nz(double lo, double hi) { return rnz(lo, hi); }
// a nonzero length for a direction component pair
static void rdir(double& x, double& y) {
  double r = rnd(0, 1);
  if (r < 0.3) { do { x = rint(-2, 2); y = rint(-2, 2); } while (x == 0 && y == 0); }
  else if (r < 0.4) { x = coin(0.5) ? 3 : -3; y = coin(0.5) ? 4 : -4; if (coin(0.5)) std::swap(x, y); }
  else { do { x = rnd(-4, 4); y = rnd(-4, 4); } while (x * x + y * y < 0.25); }
}
static double rang() {
  double r = rnd(0, 1);
  if (r < 0.3) return (M_PI / 4) * rint(-8, 8);
  if (r < 0.4) return rnd(-1e-6, 1e-6);
  return rnd(-7, 7);
}
static int ai(double v) { return (int)std::lround(v); }

// ---------------------------------------------------------------------------------
// operations (word ops): n, then per operation: code, number of arguments, the arguments as f64 (integers are
// converted with lround on both sides). The codes are listed in docs/api-geom2d.md.
struct Op { int code; Vd a; };
typedef std::vector<Op> Ops;
static void p_ops(const Ops& o) {
  in((long)o.size());
  for (const Op& x : o) { in(x.code); in((long)x.a.size()); for (double v : x.a) h(v); }
}
static void W_ops(const Ops& o) { int s = ntok; p_ops(o); word("ops", s, -1); }
static Op mk(int code, Vd a = Vd()) { Op o; o.code = code; o.a = a; return o; }

// ---------------------------------------------------------------------------------
// output helpers
static void o_p(const gp_Pnt2d& p) { h(p.X()); h(p.Y()); }
static void o_v(const gp_Vec2d& v) { h(v.X()); h(v.Y()); }
static void o_d(const gp_Dir2d& d) { h(d.X()); h(d.Y()); }
static void o_ax(const gp_Ax2d& a) { o_p(a.Location()); o_d(a.Direction()); }
static void o_ax22(const gp_Ax22d& a) { o_p(a.Location()); o_d(a.XDirection()); o_d(a.YDirection()); }
static void o_fv(const Vd& v) { in((long)v.size()); for (double x : v) h(x); }

// ---------------------------------------------------------------------------------
// transformation recipes (word tr = kind + 8 floats)
//   0 identity / default   1 mirror point (x y)   2 mirror axis (ox oy dx dy)   3 rotation (x y angle)
//   4 scale (x y s)   5 SetTransformation(to) (ox oy dx dy)   6 SetTransformation(from, to) (8 values)
//   7 translation vector (x y)   8 translation points (x1 y1 x2 y2)   9 SetValues (a11 a12 a13 a21 a22 a23)
struct Tr { int kind; double p[8]; };
static Tr tr_make(const Vd& a, size_t at) {
  Tr t; t.kind = a.size() > at ? ai(a[at]) : 0;
  for (int i = 0; i < 8; ++i) t.p[i] = a.size() > at + 1 + i ? a[at + 1 + i] : 0.0;
  return t;
}
static void tr_args(const Tr& t, Vd& a) { a.push_back(t.kind); for (int i = 0; i < 8; ++i) a.push_back(t.p[i]); }
static void p_tr(const Tr& t) { in(t.kind); for (int i = 0; i < 8; ++i) h(t.p[i]); }
static void W_tr(const Tr& t) { int s = ntok; p_tr(t); word("tr", s, 9); }
// kind 9 with a degenerate matrix: the check of gp_Trsf2d::SetValues is compiled out
static bool tr_unchecked(const Tr& t) {
  return t.kind == 9 && std::fabs(t.p[0] * t.p[4] - t.p[3] * t.p[1]) < gp::Resolution();
}
static gp_Trsf2d tr_gp(const Tr& t) {
  gp_Trsf2d T; const double* p = t.p;
  switch (t.kind) {
    case 1: T.SetMirror(gp_Pnt2d(p[0], p[1])); break;
    case 2: T.SetMirror(gp_Ax2d(gp_Pnt2d(p[0], p[1]), gp_Dir2d(p[2], p[3]))); break;
    case 3: T.SetRotation(gp_Pnt2d(p[0], p[1]), p[2]); break;
    case 4: T.SetScale(gp_Pnt2d(p[0], p[1]), p[2]); break;
    case 5: T.SetTransformation(gp_Ax2d(gp_Pnt2d(p[0], p[1]), gp_Dir2d(p[2], p[3]))); break;
    case 6: T.SetTransformation(gp_Ax2d(gp_Pnt2d(p[0], p[1]), gp_Dir2d(p[2], p[3])), gp_Ax2d(gp_Pnt2d(p[4], p[5]), gp_Dir2d(p[6], p[7]))); break;
    case 7: T.SetTranslation(gp_Vec2d(p[0], p[1])); break;
    case 8: T.SetTranslation(gp_Pnt2d(p[0], p[1]), gp_Pnt2d(p[2], p[3])); break;
    case 9: T.SetValues(p[0], p[1], p[2], p[3], p[4], p[5]); break;
    default: break;
  }
  return T;
}
// the same recipe applied with the Set* methods of Geom2d_Transformation
static void tr_set(Geom2d_Transformation& g, const Tr& t) {
  const double* p = t.p;
  switch (t.kind) {
    case 1: g.SetMirror(gp_Pnt2d(p[0], p[1])); break;
    case 2: g.SetMirror(gp_Ax2d(gp_Pnt2d(p[0], p[1]), gp_Dir2d(p[2], p[3]))); break;
    case 3: g.SetRotation(gp_Pnt2d(p[0], p[1]), p[2]); break;
    case 4: g.SetScale(gp_Pnt2d(p[0], p[1]), p[2]); break;
    case 5: g.SetTransformation(gp_Ax2d(gp_Pnt2d(p[0], p[1]), gp_Dir2d(p[2], p[3]))); break;
    case 6: g.SetTransformation(gp_Ax2d(gp_Pnt2d(p[0], p[1]), gp_Dir2d(p[2], p[3])), gp_Ax2d(gp_Pnt2d(p[4], p[5]), gp_Dir2d(p[6], p[7]))); break;
    case 7: g.SetTranslation(gp_Vec2d(p[0], p[1])); break;
    case 8: g.SetTranslation(gp_Pnt2d(p[0], p[1]), gp_Pnt2d(p[2], p[3])); break;
    case 9: g.SetTrsf2d(tr_gp(t)); break;
    default: g.SetTrsf2d(gp_Trsf2d()); break;
  }
}
// a random recipe; `rot` allows kinds that use sin / cos; `zero_scale` allows scale 0 (only the transformation families)
static Tr rtr(bool rot, bool zero_scale = false, bool bad_values = false, int force = -1) {
  Tr t; t.kind = 0; for (int i = 0; i < 8; ++i) t.p[i] = 0;
  if (zero_scale && force < 0 && coin(0.3)) force = coin(0.7) ? 4 : 9;
  for (;;) {
    double r = rnd(0, 1);
    int k = r < 0.04 ? 0 : r < 0.14 ? 1 : r < 0.26 ? 2 : r < 0.40 ? 3 : r < 0.54 ? 4 : r < 0.62 ? 5 : r < 0.72 ? 6 : r < 0.84 ? 7 : r < 0.92 ? 8 : 9;
    if (force >= 0) k = force;
    if (k == 3 && !rot) continue;
    t.kind = k;
    break;
  }
  double* p = t.p;
  switch (t.kind) {
    case 1: p[0] = crd(); p[1] = crd(); break;
    case 2: p[0] = crd(); p[1] = crd(); rdir(p[2], p[3]); break;
    case 3: p[0] = crd(); p[1] = crd(); p[2] = rang(); break;
    case 4: p[0] = crd(); p[1] = crd(); p[2] = (zero_scale && coin(0.6)) ? 0.0 : coin(0.1) ? 1.0 : ( (coin(0.35) ? -1 : 1) * (coin(0.5) ? 0.25 * rint(1, 16) : rnd(0.1, 4))); break;
    case 5: p[0] = crd(); p[1] = crd(); rdir(p[2], p[3]); break;
    case 6: p[0] = crd(); p[1] = crd(); rdir(p[2], p[3]); p[4] = crd(); p[5] = crd(); rdir(p[6], p[7]); break;
    case 7: p[0] = crd(); p[1] = crd(); break;
    case 8: p[0] = crd(); p[1] = crd(); p[2] = crd(); p[3] = crd(); break;
    case 9: {
      // s * (rotation or reflection by an exact (c, s) pair) and a translation; sometimes not a similarity
      static const double cs[][2] = {{1, 0}, {0, 1}, {0.6, 0.8}, {0.8, 0.6}, {-0.6, 0.8}, {0, -1}, {-1, 0}};
      const double* c = cs[rint(0, 6)];
      double s = (zero_scale && coin(0.6)) ? 0.0 : (coin(0.5) ? 0.5 * rint(1, 8) : rnd(0.2, 3));
      bool refl = coin(0.4);
      p[0] = s * c[0]; p[1] = refl ? s * c[1] : -s * c[1];
      p[3] = s * c[1]; p[4] = refl ? -s * c[0] : s * c[0];
      p[2] = crd(); p[5] = crd();
      if (bad_values && coin(0.4)) { p[0] *= 1.5; p[1] += 0.25; }
      break;
    }
    default: break;
  }
  return t;
}

// =================================================================================
// curve recipes (word crv); the grammar is in docs/api-geom2d.md
//   kind 0 line  1 circle  2 ellipse  3 hyperbola  4 parabola:  kind ctor ox oy dx dy sense  then  r | a b | a b | f [fx fy when ctor = 3]
//   kind 5 bezier:   n  n*(x y)  rat  [nw  nw*w]
//   kind 6 bspline:  deg periodic  nk  nk*knot  nm  nm*mult  np  np*(x y)  rat  [nw  nw*w]
//   kind 7 trimmed:  u1 u2 sense adjust  crv        kind 8 offset:  offset notcheck  crv
struct CV {
  int kind = 0, ctor = 0;
  double o[2] = {0, 0}, d[2] = {1, 0};
  int sense = 1;
  double a = 0, b = 0, fx = 0, fy = 0;
  int deg = 1, per = 0, rat = 0;
  Vd knots; std::vector<int> mults; Vd px, py; Vd w;
  double u1 = 0, u2 = 1; int adj = 1; double off = 0; int notc = 0;
  std::shared_ptr<CV> basis;
};
static void p_crv(const CV& c) {
  in(c.kind);
  if (c.kind <= 4) {
    in(c.ctor); h(c.o[0]); h(c.o[1]); h(c.d[0]); h(c.d[1]); in(c.sense);
    if (c.kind == 1 || c.kind == 4) h(c.a);
    if (c.kind == 2 || c.kind == 3) { h(c.a); h(c.b); }
    if (c.kind == 4 && c.ctor == 3) { h(c.fx); h(c.fy); }
  } else if (c.kind == 5) {
    in((long)c.px.size()); for (size_t i = 0; i < c.px.size(); ++i) { h(c.px[i]); h(c.py[i]); }
    in(c.rat); if (c.rat) { in((long)c.w.size()); for (double v : c.w) h(v); }
  } else if (c.kind == 6) {
    in(c.deg); in(c.per); in((long)c.knots.size());
    for (double v : c.knots) h(v);
    // the multiplicities are printed with their own count (a wrong size is one of the invalid variants)
    in((long)c.mults.size()); for (int m : c.mults) in(m);
    in((long)c.px.size()); for (size_t i = 0; i < c.px.size(); ++i) { h(c.px[i]); h(c.py[i]); }
    in(c.rat); if (c.rat) { in((long)c.w.size()); for (double v : c.w) h(v); }
  } else if (c.kind == 7) {
    h(c.u1); h(c.u2); in(c.sense); in(c.adj); p_crv(*c.basis);
  } else {
    h(c.off); in(c.notc); p_crv(*c.basis);
  }
}
static void W_crv(const CV& c) { int s = ntok; p_crv(c); word("crv", s, -1); }

typedef occ::handle<Geom2d_Curve> HC;
typedef occ::handle<Geom2d_Geometry> HG;
static gp_Ax2d cv_ax(const CV& c) { return gp_Ax2d(gp_Pnt2d(c.o[0], c.o[1]), gp_Dir2d(c.d[0], c.d[1])); }
static HC build(const CV& c);
static HC build_conic(const CV& c) {
  gp_Ax2d ax = cv_ax(c); bool s = c.sense != 0;
  switch (c.kind) {
    case 0:
      if (c.ctor == 0) return new Geom2d_Line(ax);
      if (c.ctor == 1) return new Geom2d_Line(gp_Lin2d(ax));
      return new Geom2d_Line(ax.Location(), ax.Direction());
    case 1:
      if (c.ctor == 0) return new Geom2d_Circle(gp_Circ2d(ax, c.a, s));
      if (c.ctor == 1) return new Geom2d_Circle(ax, c.a, s);
      return new Geom2d_Circle(gp_Ax22d(ax, s), c.a);
    case 2:
      if (c.ctor == 0) return new Geom2d_Ellipse(gp_Elips2d(ax, c.a, c.b, s));
      if (c.ctor == 1) return new Geom2d_Ellipse(ax, c.a, c.b, s);
      return new Geom2d_Ellipse(gp_Ax22d(ax, s), c.a, c.b);
    case 3:
      if (c.ctor == 0) return new Geom2d_Hyperbola(gp_Hypr2d(ax, c.a, c.b, s));
      if (c.ctor == 1) return new Geom2d_Hyperbola(ax, c.a, c.b, s);
      return new Geom2d_Hyperbola(gp_Ax22d(ax, s), c.a, c.b);
    default:
      if (c.ctor == 0) return new Geom2d_Parabola(gp_Parab2d(ax, c.a, s));
      if (c.ctor == 1) return new Geom2d_Parabola(ax, c.a, s);
      if (c.ctor == 2) return new Geom2d_Parabola(gp_Ax22d(ax, s), c.a);
      return new Geom2d_Parabola(ax, gp_Pnt2d(c.fx, c.fy));
  }
}
static HC build(const CV& c) {
  if (c.kind <= 4) return build_conic(c);
  if (c.kind == 5) {
    NCollection_Array1<gp_Pnt2d> P(1, (int)c.px.size());
    for (size_t i = 0; i < c.px.size(); ++i) P((int)i + 1) = gp_Pnt2d(c.px[i], c.py[i]);
    if (!c.rat) return new Geom2d_BezierCurve(P);
    NCollection_Array1<double> W(1, (int)c.w.size());
    for (size_t i = 0; i < c.w.size(); ++i) W((int)i + 1) = c.w[i];
    return new Geom2d_BezierCurve(P, W);
  }
  if (c.kind == 6) {
    NCollection_Array1<gp_Pnt2d> P(1, (int)c.px.size());
    for (size_t i = 0; i < c.px.size(); ++i) P((int)i + 1) = gp_Pnt2d(c.px[i], c.py[i]);
    NCollection_Array1<double> K(1, (int)c.knots.size());
    for (size_t i = 0; i < c.knots.size(); ++i) K((int)i + 1) = c.knots[i];
    NCollection_Array1<int> M(1, (int)c.mults.size());
    for (size_t i = 0; i < c.mults.size(); ++i) M((int)i + 1) = c.mults[i];
    if (!c.rat) return new Geom2d_BSplineCurve(P, K, M, c.deg, c.per != 0);
    NCollection_Array1<double> W(1, (int)c.w.size());
    for (size_t i = 0; i < c.w.size(); ++i) W((int)i + 1) = c.w[i];
    return new Geom2d_BSplineCurve(P, W, K, M, c.deg, c.per != 0);
  }
  HC b = build(*c.basis);
  if (c.kind == 7) return new Geom2d_TrimmedCurve(b, c.u1, c.u2, c.sense != 0, c.adj != 0);
  return new Geom2d_OffsetCurve(b, c.off, c.notc != 0);
}

// ---------------------------------------------------------------------------------
// the state of a geometric object, printed as the oracle's `state` (type code first); the Rust test prints the same values
//   0 line: loc dir          1 circle: ax22 r          2 ellipse / 3 hyperbola: ax22 a b         4 parabola: ax22 f
//   5 bezier: n poles rational weights(n) closed erep
//   6 bspline: deg periodic rational nk np knots mults poles weights(np) closed distribution continuity erep
//   7 trimmed: first last periodic closed, then the state of the basis curve
//   8 offset: offset continuity basis-continuity erep, then the state of the basis curve
//   10 cartesian point: x y      11 direction: x y      12 vector with magnitude: x y      13 axis placement: loc dir
static void o_state(const HG& g);
static void o_bez(const Geom2d_BezierCurve* b) {
  in(5); in(b->NbPoles());
  for (int i = 1; i <= b->NbPoles(); ++i) o_p(b->Pole(i));
  ob(b->IsRational());
  for (int i = 1; i <= b->NbPoles(); ++i) h(b->Weight(i));
  ob(b->IsClosed()); ob(b->HasEvalRepresentation());
}
static void o_bsp(const Geom2d_BSplineCurve* b) {
  in(6); in(b->Degree()); ob(b->IsPeriodic()); ob(b->IsRational()); in(b->NbKnots()); in(b->NbPoles());
  for (int i = 1; i <= b->NbKnots(); ++i) h(b->Knot(i));
  for (int i = 1; i <= b->NbKnots(); ++i) in(b->Multiplicity(i));
  for (int i = 1; i <= b->NbPoles(); ++i) o_p(b->Pole(i));
  for (int i = 1; i <= b->NbPoles(); ++i) h(b->Weight(i));
  ob(b->IsClosed()); in((int)b->KnotDistribution()); in((int)b->Continuity()); ob(b->HasEvalRepresentation());
}
static void o_state(const HG& g) {
  Geom2d_Geometry* p = g.get();
  if (auto* x = dynamic_cast<Geom2d_Line*>(p)) { in(0); o_p(x->Location()); o_d(x->Direction()); }
  else if (auto* x = dynamic_cast<Geom2d_Circle*>(p)) { in(1); o_ax22(x->Position()); h(x->Radius()); }
  else if (auto* x = dynamic_cast<Geom2d_Ellipse*>(p)) { in(2); o_ax22(x->Position()); h(x->MajorRadius()); h(x->MinorRadius()); }
  else if (auto* x = dynamic_cast<Geom2d_Hyperbola*>(p)) { in(3); o_ax22(x->Position()); h(x->MajorRadius()); h(x->MinorRadius()); }
  else if (auto* x = dynamic_cast<Geom2d_Parabola*>(p)) { in(4); o_ax22(x->Position()); h(x->Focal()); }
  else if (auto* x = dynamic_cast<Geom2d_BezierCurve*>(p)) o_bez(x);
  else if (auto* x = dynamic_cast<Geom2d_BSplineCurve*>(p)) o_bsp(x);
  else if (auto* x = dynamic_cast<Geom2d_TrimmedCurve*>(p)) {
    in(7); h(x->FirstParameter()); h(x->LastParameter()); ob(x->IsPeriodic()); ob(x->IsClosed());
    o_state(x->BasisCurve());
  } else if (auto* x = dynamic_cast<Geom2d_OffsetCurve*>(p)) {
    in(8); h(x->Offset()); in((int)x->Continuity()); in((int)x->GetBasisCurveContinuity()); ob(x->HasEvalRepresentation());
    o_state(x->BasisCurve());
  } else if (auto* x = dynamic_cast<Geom2d_CartesianPoint*>(p)) { in(10); h(x->X()); h(x->Y()); }
  else if (auto* x = dynamic_cast<Geom2d_Direction*>(p)) { in(11); h(x->X()); h(x->Y()); }
  else if (auto* x = dynamic_cast<Geom2d_VectorWithMagnitude*>(p)) { in(12); h(x->X()); h(x->Y()); }
  else if (auto* x = dynamic_cast<Geom2d_AxisPlacement*>(p)) { in(13); o_p(x->Location()); o_d(x->Direction()); }
  else { std::fprintf(stderr, "o_state: unknown geometry\n"); std::abort(); }
}

// ---------------------------------------------------------------------------------
// random curves
static void rframe(CV& c) {
  c.ctor = rint(0, 2);
  c.o[0] = crd(); c.o[1] = crd();
  if (coin(0.3)) { c.d[0] = 1; c.d[1] = 0; } else rdir(c.d[0], c.d[1]);
  c.sense = coin(0.7) ? 1 : 0;
}
static double rradius(bool allow_zero) {
  double r = rnd(0, 1);
  if (r < 0.35) return 0.5 * rint(1, 12);
  if (r < 0.85) return rnd(0.05, 8);
  if (r < 0.92) return rnd(1e-4, 1e-2);
  return allow_zero ? 0.0 : rnd(0.5, 3);
}
static CV rline() { CV c; c.kind = 0; rframe(c); return c; }
static CV rcircle(bool allow_zero = true) { CV c; c.kind = 1; rframe(c); c.a = rradius(allow_zero); return c; }
static CV rellipse() {
  CV c; c.kind = 2; rframe(c); c.b = rradius(true);
  double r = rnd(0, 1);
  c.a = r < 0.1 ? c.b : (r < 0.15 ? c.b + 1e-12 : c.b * rnd(1, 5) + (coin(0.2) ? 0.5 : 0));
  return c;
}
static CV rhyperbola() { CV c; c.kind = 3; rframe(c); c.a = rradius(false); c.b = rradius(true); return c; }
static CV rparabola() {
  CV c; c.kind = 4; rframe(c); c.a = rradius(true);
  if (c.ctor == 3 || coin(0.0)) { c.fx = crd(); c.fy = crd(); }
  return c;
}
// the fourth parabola constructor (directrix and focus): the focus must not lie on the directrix (else gp raises)
static void fix_parabola(CV& c) {
  c.ctor = rint(0, 3);
  if (c.ctor == 3) {
    // focus at a distance of at least 0.2 from the directrix, on its left side (so the sense is the positive one)
    double dd = std::sqrt(c.d[0] * c.d[0] + c.d[1] * c.d[1]);
    double nx = -c.d[1] / dd, ny = c.d[0] / dd;
    double t = rnd(0.2, 4), s = rnd(-3, 3);
    c.fx = c.o[0] + (c.d[0] / dd) * s + nx * t * (coin(0.8) ? 1 : -1);
    c.fy = c.o[1] + (c.d[1] / dd) * s + ny * t * (coin(0.8) ? 1 : -1);
  }
}
static CV rbezier(bool cusp_ok = true, int nmax = 8) {
  CV c; c.kind = 5;
  int n = coin(0.05) ? rint(10, nmax > 10 ? nmax : 10) : rint(2, std::min(nmax, 7));
  for (int i = 0; i < n; ++i) { c.px.push_back(crd()); c.py.push_back(crd()); }
  if (cusp_ok && n >= 3 && coin(0.12)) { int k = rint(0, n - 2); c.px[k + 1] = c.px[k]; c.py[k + 1] = c.py[k]; }
  if (coin(0.12) && n >= 2) { c.px[n - 1] = c.px[0]; c.py[n - 1] = c.py[0]; }   // closed
  c.rat = coin(0.45) ? 1 : 0;
  if (c.rat) {
    double r = rnd(0, 1);
    for (int i = 0; i < n; ++i) c.w.push_back(r < 0.2 ? 1.75 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3)));
  }
  return c;
}
// a valid B-spline; `style`: 0 any, 1 clamped non periodic, 2 periodic
static CV rbspline(int style = 0, bool cusp_ok = true, int degmax = 5) {
  for (;;) {
    CV c; c.kind = 6;
    c.deg = coin(0.04) ? rint(6, 9) : rint(1, degmax);
    c.per = style == 2 ? 1 : (style == 1 ? 0 : (coin(0.3) ? 1 : 0));
    int nk = rint(2, 6);
    double k = nz(0.5, 3);
    bool uniform = coin(0.25);
    double step = coin(0.5) ? 0.5 * rint(1, 4) : rnd(0.1, 2.0);
    int innermode = rint(0, 9);   // 0..5 random multiplicities, 6..7 simple, 8..9 piecewise Bezier (inner multiplicity = degree; OCCT refuses more)
    for (int i = 0; i < nk; ++i) {
      c.knots.push_back(k);
      k += uniform ? step : (coin(0.5) ? 0.5 * rint(1, 4) : rnd(0.1, 2.0));
      int m;
      if (innermode <= 5) m = rint(1, c.deg);
      else if (innermode <= 7) m = 1;
      else if (innermode == 8) m = c.deg;
      else m = rint(1, c.deg);
      c.mults.push_back(m);
    }
    if (c.per) c.mults[nk - 1] = c.mults[0];
    else if (style == 1 || coin(0.85)) { c.mults[0] = c.deg + 1; c.mults[nk - 1] = c.deg + 1; }
    else { c.mults[0] = rint(1, c.deg + 1); c.mults[nk - 1] = rint(1, c.deg + 1); }
    NCollection_Array1<int> M(1, nk);
    for (int i = 0; i < nk; ++i) M(i + 1) = c.mults[(size_t)i];
    int np = BSplCLib::NbPoles(c.deg, c.per != 0, M);
    if (np < 2 || np > 30 || (c.per && np <= c.deg)) continue;   // a periodic curve with fewer poles than its degree + 1 makes several OCCT operations read outside their arrays
    for (int i = 0; i < np; ++i) { c.px.push_back(crd()); c.py.push_back(crd()); }
    if (cusp_ok && np >= 3 && coin(0.12)) { int q = rint(0, np - 2); c.px[(size_t)q + 1] = c.px[(size_t)q]; c.py[(size_t)q + 1] = c.py[(size_t)q]; }
    c.rat = coin(0.45) ? 1 : 0;
    if (c.rat) {
      double r = rnd(0, 1);
      for (int i = 0; i < np; ++i) c.w.push_back(r < 0.2 ? 1.75 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3)));
    }
    return c;
  }
}
// a curve of any basic kind (no trimmed, no offset)
static CV rbasic(bool allow_c0 = true) {
  double r = rnd(0, 1);
  if (r < 0.14) return rline();
  if (r < 0.26) return rcircle();
  if (r < 0.36) return rellipse();
  if (r < 0.44) return rhyperbola();
  if (r < 0.52) { CV c = rparabola(); fix_parabola(c); return c; }
  if (r < 0.76) return rbezier();
  return rbspline(0, true, 4);
}
static CV mk_trim(const CV& basis, double u1, double u2, bool sense, bool adj) {
  CV c; c.kind = 7; c.basis = std::make_shared<CV>(basis); c.u1 = u1; c.u2 = u2; c.sense = sense; c.adj = adj; return c;
}
static CV mk_off(const CV& basis, double off, bool notc) {
  CV c; c.kind = 8; c.basis = std::make_shared<CV>(basis); c.off = off; c.notc = notc; return c;
}
static double roffset() {
  double r = rnd(0, 1);
  if (r < 0.1) return 0.0;
  if (r < 0.45) return 0.25 * rint(-12, 12);
  if (r < 0.9) return rnd(-3, 3);
  return coin(0.5) ? rnd(100, 1000) : -rnd(100, 1000);
}

// A parameter near the interesting places of a built curve
static double rparam(const HC& c, double outside = 0.4) {
  double f = c->FirstParameter(), l = c->LastParameter();
  if (f < -1e50 || l > 1e50) {
    bool hyp = !c.IsNull() && dynamic_cast<Geom2d_Hyperbola*>(c.get());
    double lim = hyp ? 2.5 : 4;
    return coin(0.2) ? 0.0 : (coin(0.3) ? 0.5 * rint(-6, 6) : rnd(-lim, lim));
  }
  double sp = l - f, r = rnd(0, 1);
  if (r < 0.10) return f;
  if (r < 0.20) return l;
  if (r < 0.28) {
    if (auto* b = dynamic_cast<Geom2d_BSplineCurve*>(c.get())) return b->Knot(rint(1, b->NbKnots()));
    return f + 0.5 * sp;
  }
  if (r < 0.78) return f + rnd(0, 1) * sp;
  if (r < 0.90) return f + rnd(-outside, 1 + outside) * sp;
  return f + rnd(-2.5, 3.5) * sp;
}

// =================================================================================
// operations on geometric objects
static double A(const Vd& a, size_t i) { return i < a.size() ? a[i] : 0.0; }
static int AI(const Vd& a, size_t i) { return ai(A(a, i)); }
static gp_Pnt2d A_p(const Vd& a, size_t i) { return gp_Pnt2d(A(a, i), A(a, i + 1)); }
static gp_Ax2d A_ax(const Vd& a, size_t i) { return gp_Ax2d(gp_Pnt2d(A(a, i), A(a, i + 1)), gp_Dir2d(A(a, i + 2), A(a, i + 3))); }
#define AS(T, v) T* v = dynamic_cast<T*>(g.get())

// the operations every geometric object has (Geom2d_Geometry): codes 1, 4, 21 .. 35
static bool geom_common(HG& g, const Op& op) {
  const Vd& a = op.a;
  switch (op.code) {
    case 1: o_state(g); return true;
    case 4: g->Transform(tr_gp(tr_make(a, 0))); return true;
    case 21: { gp_Trsf2d T = tr_gp(tr_make(a, 0)); HG cp = g->Copy(); g->Transform(T); o_state(cp); return true; }
    case 22: g->Mirror(A_p(a, 0)); return true;
    case 23: g->Mirror(A_ax(a, 0)); return true;
    case 24: g->Rotate(A_p(a, 0), A(a, 2)); return true;
    case 25: g->Scale(A_p(a, 0), A(a, 2)); return true;
    case 26: g->Translate(gp_Vec2d(A(a, 0), A(a, 1))); return true;
    case 27: g->Translate(A_p(a, 0), A_p(a, 2)); return true;
    case 28: o_state(g->Mirrored(A_p(a, 0))); return true;
    case 29: o_state(g->Mirrored(A_ax(a, 0))); return true;
    case 30: o_state(g->Rotated(A_p(a, 0), A(a, 2))); return true;
    case 31: o_state(g->Scaled(A_p(a, 0), A(a, 2))); return true;
    case 32: o_state(g->Translated(gp_Vec2d(A(a, 0), A(a, 1)))); return true;
    case 33: o_state(g->Translated(A_p(a, 0), A_p(a, 2))); return true;
    case 34: o_state(g->Transformed(tr_gp(tr_make(a, 0)))); return true;
    case 35: g = g->Copy(); return true;
    default: return false;
  }
}
// a recipe in the arguments of a geometry operation is unchecked when its matrix is degenerate
static bool geom_unchecked(const Op& op) {
  if (op.code == 4 || op.code == 21 || op.code == 34) return tr_unchecked(tr_make(op.a, 0));
  return false;
}

// ---------------------------------------------------------------------------------
// points, vectors, directions, axes
enum { P_SET_COORD = 40, P_SET_PNT, P_SET_X, P_SET_Y, P_COORD, P_PNT2D, P_X, P_Y, P_DIST, P_SQDIST };
static HG mk_cpoint(int ctor, double x, double y) {
  if (ctor == 0) return new Geom2d_CartesianPoint(gp_Pnt2d(x, y));
  return new Geom2d_CartesianPoint(x, y);
}
static bool point_op(HG& g, const Op& op) {
  if (geom_common(g, op)) return true;
  AS(Geom2d_CartesianPoint, p);
  const Vd& a = op.a;
  switch (op.code) {
    case P_SET_COORD: p->SetCoord(A(a, 0), A(a, 1)); return true;
    case P_SET_PNT: p->SetPnt2d(gp_Pnt2d(A(a, 0), A(a, 1))); return true;
    case P_SET_X: p->SetX(A(a, 0)); return true;
    case P_SET_Y: p->SetY(A(a, 0)); return true;
    case P_COORD: { double x, y; p->Coord(x, y); h(x); h(y); return true; }
    case P_PNT2D: o_p(p->Pnt2d()); return true;
    case P_X: h(p->X()); return true;
    case P_Y: h(p->Y()); return true;
    case P_DIST: { occ::handle<Geom2d_Point> q = occ::down_cast<Geom2d_Point>(mk_cpoint(AI(a, 2), A(a, 0), A(a, 1))); h(p->Distance(q)); return true; }
    case P_SQDIST: { occ::handle<Geom2d_Point> q = occ::down_cast<Geom2d_Point>(mk_cpoint(AI(a, 2), A(a, 0), A(a, 1))); h(p->SquareDistance(q)); return true; }
    default: std::fprintf(stderr, "point_op: bad code %d\n", op.code); std::abort();
  }
}

// directions and vectors with magnitude: the "other" operand is (kind, x, y) with kind 0 direction / 1 vector
enum { V_SET_COORD = 50, V_SET_X, V_SET_Y, V_SET_VEC, V_REVERSE, V_REVERSED, V_ANGLE, V_COORD, V_MAG, V_SQMAG, V_X, V_Y, V_CROSSED, V_DOT, V_VEC2D, V_DIR2D,
       V_ADD = 70, V_ADDED, V_SUBTRACT, V_SUBTRACTED, V_MULTIPLY, V_MULTIPLIED, V_DIVIDE, V_DIVIDED, V_NORMALIZE, V_NORMALIZED };
static occ::handle<Geom2d_Vector> mk_other(const Vd& a, size_t i) {
  if (AI(a, i) == 0) return new Geom2d_Direction(A(a, i + 1), A(a, i + 2));
  return new Geom2d_VectorWithMagnitude(A(a, i + 1), A(a, i + 2));
}
static bool vec_unchecked(HG& g, const Op& op) {
  if (geom_unchecked(op)) return true;
  const Vd& a = op.a;
  if (dynamic_cast<Geom2d_Direction*>(g.get())) {
    auto zero = [](double x, double y) { return std::sqrt(x * x + y * y) <= gp::Resolution(); };
    auto* d = dynamic_cast<Geom2d_Direction*>(g.get());
    switch (op.code) {
      case V_SET_COORD: case V_SET_VEC: return zero(A(a, 0), A(a, 1));
      case V_SET_X: return zero(A(a, 0), d->Y());
      case V_SET_Y: return zero(d->X(), A(a, 0));
      default: break;
    }
  }
  if ((op.code == V_DIVIDE || op.code == V_DIVIDED) && std::fabs(A(a, 0)) <= gp::Resolution()) return true;
  // gp_Vec2d::Normalize raises through a check that is compiled out of the libraries
  if (op.code == V_NORMALIZE || op.code == V_NORMALIZED) return dynamic_cast<Geom2d_Vector*>(g.get())->Vec2d().Magnitude() <= gp::Resolution();
  return false;
}
static HG mk_vec(int kind, int ctor, double x, double y, double x2, double y2) {
  if (kind == 0) {
    if (ctor == 0) return new Geom2d_Direction(x, y);
    return new Geom2d_Direction(gp_Dir2d(x, y));
  }
  if (ctor == 0) return new Geom2d_VectorWithMagnitude(gp_Vec2d(x, y));
  if (ctor == 1) return new Geom2d_VectorWithMagnitude(x, y);
  return new Geom2d_VectorWithMagnitude(gp_Pnt2d(x, y), gp_Pnt2d(x2, y2));
}
static bool vec_op(HG& g, const Op& op) {
  if (geom_common(g, op)) return true;
  AS(Geom2d_Vector, v);
  Geom2d_Direction* d = dynamic_cast<Geom2d_Direction*>(g.get());
  Geom2d_VectorWithMagnitude* m = dynamic_cast<Geom2d_VectorWithMagnitude*>(g.get());
  const Vd& a = op.a;
  switch (op.code) {
    case V_SET_COORD: if (d) d->SetCoord(A(a, 0), A(a, 1)); else m->SetCoord(A(a, 0), A(a, 1)); return true;
    case V_SET_X: if (d) d->SetX(A(a, 0)); else m->SetX(A(a, 0)); return true;
    case V_SET_Y: if (d) d->SetY(A(a, 0)); else m->SetY(A(a, 0)); return true;
    case V_SET_VEC: if (d) d->SetDir2d(gp_Dir2d(A(a, 0), A(a, 1))); else m->SetVec2d(gp_Vec2d(A(a, 0), A(a, 1))); return true;
    case V_REVERSE: v->Reverse(); return true;
    case V_REVERSED: o_state(v->Reversed()); return true;
    case V_ANGLE: h(v->Angle(mk_other(a, 0))); return true;
    case V_COORD: { double x, y; v->Coord(x, y); h(x); h(y); return true; }
    case V_MAG: h(v->Magnitude()); return true;
    case V_SQMAG: h(v->SquareMagnitude()); return true;
    case V_X: h(v->X()); return true;
    case V_Y: h(v->Y()); return true;
    case V_CROSSED: h(v->Crossed(mk_other(a, 0))); return true;
    case V_DOT: h(v->Dot(mk_other(a, 0))); return true;
    case V_VEC2D: o_v(v->Vec2d()); return true;
    case V_DIR2D: o_d(d->Dir2d()); return true;
    case V_ADD: m->Add(mk_other(a, 0)); return true;
    case V_ADDED: o_state(m->Added(mk_other(a, 0))); return true;
    case V_SUBTRACT: m->Subtract(mk_other(a, 0)); return true;
    case V_SUBTRACTED: o_state(m->Subtracted(mk_other(a, 0))); return true;
    case V_MULTIPLY: m->Multiply(A(a, 0)); return true;
    case V_MULTIPLIED: o_state(m->Multiplied(A(a, 0))); return true;
    case V_DIVIDE: m->Divide(A(a, 0)); return true;
    case V_DIVIDED: o_state(m->Divided(A(a, 0))); return true;
    case V_NORMALIZE: m->Normalize(); return true;
    case V_NORMALIZED: o_state(m->Normalized()); return true;
    default: std::fprintf(stderr, "vec_op: bad code %d\n", op.code); std::abort();
  }
}

enum { X_REVERSE = 80, X_REVERSED, X_SET_AXIS, X_SET_DIR, X_SET_LOC, X_ANGLE, X_AX2D, X_DIRECTION, X_LOCATION };
static HG mk_axis(int ctor, double ox, double oy, double dx, double dy) {
  if (ctor == 0) return new Geom2d_AxisPlacement(gp_Ax2d(gp_Pnt2d(ox, oy), gp_Dir2d(dx, dy)));
  return new Geom2d_AxisPlacement(gp_Pnt2d(ox, oy), gp_Dir2d(dx, dy));
}
static bool axis_op(HG& g, const Op& op) {
  if (geom_common(g, op)) return true;
  AS(Geom2d_AxisPlacement, x);
  const Vd& a = op.a;
  switch (op.code) {
    case X_REVERSE: x->Reverse(); return true;
    case X_REVERSED: o_state(x->Reversed()); return true;
    case X_SET_AXIS: x->SetAxis(A_ax(a, 0)); return true;
    case X_SET_DIR: x->SetDirection(gp_Dir2d(A(a, 0), A(a, 1))); return true;
    case X_SET_LOC: x->SetLocation(A_p(a, 0)); return true;
    case X_ANGLE: { occ::handle<Geom2d_AxisPlacement> o = new Geom2d_AxisPlacement(A_ax(a, 0)); h(x->Angle(o)); return true; }
    case X_AX2D: o_ax(x->Ax2d()); return true;
    case X_DIRECTION: o_d(x->Direction()); return true;
    case X_LOCATION: o_p(x->Location()); return true;
    default: std::fprintf(stderr, "axis_op: bad code %d\n", op.code); std::abort();
  }
}

// ---------------------------------------------------------------------------------
// Geom2d_Transformation: the object is a handle; codes 1.. (own numbering)
enum { T_STATE = 1, T_SET, T_INVERT, T_INVERTED, T_MULTIPLY, T_MULTIPLIED, T_PREMULTIPLY, T_POWER, T_POWERED, T_TRANSFORMS, T_VALUE, T_COPY_SWAP, T_FROM_TRSF, T_SET_TRSF, T_OPERATOR_MUL, T_OPERATOR_MULEQ };
typedef occ::handle<Geom2d_Transformation> HT;
static void o_trsf(const HT& t) {
  in((int)t->Form()); ob(t->IsNegative()); h(t->ScaleFactor());
  gp_Trsf2d T = t->Trsf2d();
  for (int r = 1; r <= 2; ++r) for (int c = 1; c <= 3; ++c) h(T.Value(r, c));
}
static HT mk_tr_obj(const Tr& t) { HT g = new Geom2d_Transformation(); tr_set(*g, t); return g; }
// the check of gp_Trsf2d::Invert is compiled out for these forms
static bool trsf_inv_unchecked(const HT& t) {
  gp_TrsfForm f = t->Form();
  return f != gp_Identity && f != gp_Translation && f != gp_PntMirror && std::fabs(t->ScaleFactor()) <= gp::Resolution();
}
static bool trsf_unchecked(const HT& t, const Op& op) {
  const Vd& a = op.a;
  switch (op.code) {
    case T_SET: case T_FROM_TRSF: case T_SET_TRSF: return tr_unchecked(tr_make(a, 0));
    case T_VALUE: return AI(a, 0) < 1 || AI(a, 0) > 2 || AI(a, 1) < 1 || AI(a, 1) > 3;
    case T_INVERT: case T_INVERTED: return trsf_inv_unchecked(t);
    case T_POWER: case T_POWERED: return AI(a, 0) < 0 && trsf_inv_unchecked(t);
    case T_MULTIPLY: case T_MULTIPLIED: case T_PREMULTIPLY: case T_OPERATOR_MUL: case T_OPERATOR_MULEQ: return tr_unchecked(tr_make(a, 0));
    default: return false;
  }
}
static bool trsf_op(HT& t, const Op& op) {
  const Vd& a = op.a;
  switch (op.code) {
    case T_STATE: o_trsf(t); return true;
    case T_SET: tr_set(*t, tr_make(a, 0)); return true;
    case T_INVERT: t->Invert(); return true;
    case T_INVERTED: o_trsf(t->Inverted()); return true;
    case T_MULTIPLY: t->Multiply(mk_tr_obj(tr_make(a, 0))); return true;
    case T_MULTIPLIED: o_trsf(t->Multiplied(mk_tr_obj(tr_make(a, 0)))); return true;
    case T_PREMULTIPLY: t->PreMultiply(mk_tr_obj(tr_make(a, 0))); return true;
    case T_POWER: t->Power(AI(a, 0)); return true;
    case T_POWERED: o_trsf(t->Powered(AI(a, 0))); return true;
    case T_TRANSFORMS: { double x = A(a, 0), y = A(a, 1); t->Transforms(x, y); h(x); h(y); return true; }
    case T_VALUE: h(t->Value(AI(a, 0), AI(a, 1))); return true;
    case T_COPY_SWAP: t = t->Copy(); return true;
    case T_FROM_TRSF: t = new Geom2d_Transformation(tr_gp(tr_make(a, 0))); return true;
    case T_SET_TRSF: t->SetTrsf2d(tr_gp(tr_make(a, 0))); return true;
    case T_OPERATOR_MUL: o_trsf(t->operator*(mk_tr_obj(tr_make(a, 0)))); return true;
    case T_OPERATOR_MULEQ: t->operator*=(mk_tr_obj(tr_make(a, 0))); return true;
    default: std::fprintf(stderr, "trsf_op: bad code %d\n", op.code); std::abort();
  }
}

// ---------------------------------------------------------------------------------
// curves (the codes are listed in docs/api-geom2d.md)
enum {
  C_STATE = 1, C_REVERSE = 2, C_REVERSED_PARAM, C_TRANSFORM_UNUSED, C_TRANSFORMED_PARAM, C_PARAM_TRANSFORM, C_D0, C_D1, C_D2, C_D3, C_DN, C_VALUE,
  C_FIRST, C_LAST, C_IS_CLOSED, C_IS_PERIODIC, C_PERIOD, C_CONTINUITY, C_IS_CN, C_REVERSED,
  L_SET_LIN2D = 100, L_SET_DIR, L_SET_LOC, L_SET_POS, L_LIN2D, L_DISTANCE, L_DIRECTION, L_LOCATION, L_POSITION,
  K_SET_AXIS = 110, K_SET_AXIS3, K_SET_XAXIS, K_SET_YAXIS, K_SET_LOC, K_XAXIS, K_YAXIS, K_ECC, K_LOCATION, K_POSITION,
  CI_SET_RADIUS = 120, CI_SET_CIRC2D, CI_CIRC2D,
  E_SET_MAJOR = 123, E_SET_MINOR, E_SET_ELIPS2D, E_ELIPS2D,
  EH_DIRECTRIX1 = 127, EH_DIRECTRIX2, EH_FOCAL, EH_FOCUS1, EH_FOCUS2, EH_MAJOR, EH_MINOR, EH_PARAMETER,
  H_ASYMPTOTE1 = 135, H_ASYMPTOTE2, H_CONJ1, H_CONJ2, H_OTHER, H_SET_HYPR2D, H_HYPR2D,
  PA_SET_FOCAL = 142, PA_SET_PARAB2D, PA_PARAB2D, PA_DIRECTRIX, PA_FOCUS,
  B_START = 150, B_END,
  BB_INCREASE = 160, BB_INS_AFTER, BB_INS_BEFORE, BB_REMOVE_POLE, BB_SEGMENT, BB_SET_POLE, BB_SET_POLE_W, BB_SET_WEIGHT, BB_RESOLUTION, BB_POLE, BB_WEIGHT,
  BB_NBPOLES, BB_DEGREE, BB_IS_RATIONAL, BB_POLES, BB_WEIGHTS, BB_KNOTS, BB_MULTS, BB_KNOTSEQ, BB_MAXDEGREE,
  S_INC_MULT = 180, S_INC_MULT_RANGE, S_INCR_MULT, S_INS_KNOT, S_INS_KNOTS, S_REMOVE_KNOT, S_SET_KNOT, S_SET_KNOT_M, S_SET_KNOTS, S_PERIODIC_NORM,
  S_SET_PERIODIC, S_SET_ORIGIN, S_SET_NOT_PERIODIC, S_MOVE_POINT, S_MOVE_TANGENT, S_IS_G1, S_LOCATE, S_LOCAL_VALUE, S_LOCAL_D0, S_LOCAL_D1, S_LOCAL_D2,
  S_LOCAL_D3, S_LOCAL_DN, S_KNOT, S_MULT, S_DISTRIBUTION, S_FIRST_UKNOT, S_LAST_UKNOT, S_NBKNOTS,
  TR_SET_TRIM = 220, TR_BASIS,
  O_SET_BASIS = 230, O_SET_OFFSET, O_OFFSET, O_BASIS, O_BASIS_CONT,
  R_SET_EREP = 240, R_CLEAR_EREP, R_HAS_EREP, R_EREP_KIND
};
static Geom2dEval_RepCurveDesc::Base* erep_of(Geom2d_Geometry* p) {
  if (auto* b = dynamic_cast<Geom2d_BezierCurve*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom2d_BSplineCurve*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom2d_OffsetCurve*>(p)) return b->EvalRepresentation().get();
  return nullptr;
}
static void o_arr_p(const NCollection_Array1<gp_Pnt2d>& v) { in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) o_p(v(i)); }
static void o_arr_f(const NCollection_Array1<double>& v) { in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) h(v(i)); }
static void o_arr_i(const NCollection_Array1<int>& v) { in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) in(v(i)); }

// a Period() on a curve that is not periodic: the check is compiled out
static bool period_unchecked(const HC& c) {
  if (auto* t = dynamic_cast<Geom2d_TrimmedCurve*>(c.get())) return period_unchecked(t->BasisCurve());
  if (auto* o = dynamic_cast<Geom2d_OffsetCurve*>(c.get())) return period_unchecked(o->BasisCurve());
  return !c->IsPeriodic();
}
static bool bad_idx(int i, int n) { return i < 1 || i > n; }
static bool curve_unchecked(HG& g, const Op& op) {
  if (geom_unchecked(op)) return true;
  const Vd& a = op.a;
  Geom2d_Curve* c = dynamic_cast<Geom2d_Curve*>(g.get());
  Geom2d_BezierCurve* bz = dynamic_cast<Geom2d_BezierCurve*>(g.get());
  Geom2d_BSplineCurve* bs = dynamic_cast<Geom2d_BSplineCurve*>(g.get());
  switch (op.code) {
    case C_TRANSFORMED_PARAM: case C_PARAM_TRANSFORM: return tr_unchecked(tr_make(a, op.code == C_TRANSFORMED_PARAM ? 1 : 0));
    case C_PERIOD: return period_unchecked(HC(c));
    case C_IS_CN:
      return AI(a, 0) < 0 && (bs || dynamic_cast<Geom2d_TrimmedCurve*>(g.get()) || dynamic_cast<Geom2d_OffsetCurve*>(g.get()));
    case K_ECC: case EH_PARAMETER:
      if (auto* hy = dynamic_cast<Geom2d_Hyperbola*>(g.get())) return hy->MajorRadius() <= gp::Resolution();
      return false;
    case EH_DIRECTRIX1: case EH_DIRECTRIX2:
      if (auto* el = dynamic_cast<Geom2d_Ellipse*>(g.get())) return gp_Elips2d(el->Position(), el->MajorRadius(), el->MinorRadius()).Eccentricity() <= gp::Resolution();
      if (auto* hy = dynamic_cast<Geom2d_Hyperbola*>(g.get())) return hy->MajorRadius() <= gp::Resolution();
      return false;
    case H_ASYMPTOTE1: case H_ASYMPTOTE2: return dynamic_cast<Geom2d_Hyperbola*>(g.get())->MajorRadius() <= gp::Resolution();
    case BB_INCREASE:
      if (bz) { int d = AI(a, 0); return d != bz->Degree() && (d < bz->Degree() || d > Geom2d_BezierCurve::MaxDegree()); }
      return false;
    case BB_INS_AFTER: case BB_INS_BEFORE:
      if (bz) {
        int idx = AI(a, 0) - (op.code == BB_INS_BEFORE ? 1 : 0);
        return bz->NbPoles() >= Geom2d_BezierCurve::MaxDegree() || A(a, 3) <= gp::Resolution() || idx < 0 || idx > bz->NbPoles();
      }
      return false;
    case BB_REMOVE_POLE: if (bz) return bz->NbPoles() <= 2 || bad_idx(AI(a, 0), bz->NbPoles()); return false;
    case BB_SET_POLE: if (bz) return bad_idx(AI(a, 0), bz->NbPoles()); return false;
    case BB_SET_POLE_W: if (bz) return bad_idx(AI(a, 0), bz->NbPoles()) || A(a, 3) <= gp::Resolution(); return false;
    case BB_SET_WEIGHT: if (bz) return bad_idx(AI(a, 0), bz->NbPoles()) || A(a, 1) <= gp::Resolution(); return false;
    case BB_POLE: case BB_WEIGHT: {
      int n = bz ? bz->NbPoles() : bs->NbPoles();
      return bad_idx(AI(a, 0), n);
    }
    case S_INC_MULT: case S_SET_KNOT_M: return bad_idx(AI(a, 0), bs->NbKnots());
    case S_INC_MULT_RANGE: case S_INCR_MULT: return AI(a, 0) < 1 || AI(a, 1) > bs->NbKnots() || AI(a, 0) > AI(a, 1);
    case S_KNOT: case S_MULT: return bad_idx(AI(a, 0), bs->NbKnots());
    case S_LOCAL_VALUE: case S_LOCAL_D0: case S_LOCAL_D1: case S_LOCAL_D2: case S_LOCAL_D3: case S_LOCAL_DN: {
      int k1 = AI(a, 1), k2 = AI(a, 2);
      return k1 < 1 || k2 > bs->NbKnots() || k1 >= k2;
    }
    default: return false;
  }
}

static void conic_set_ax(Geom2d_Conic* c, const Op& op) {
  const Vd& a = op.a;
  switch (op.code) {
    case K_SET_AXIS: c->SetAxis(gp_Ax22d(A_ax(a, 0), AI(a, 4) != 0)); break;
    case K_SET_AXIS3: c->SetAxis(gp_Ax22d(A_p(a, 0), gp_Dir2d(A(a, 2), A(a, 3)), gp_Dir2d(A(a, 4), A(a, 5)))); break;
    case K_SET_XAXIS: c->SetXAxis(A_ax(a, 0)); break;
    case K_SET_YAXIS: c->SetYAxis(A_ax(a, 0)); break;
    default: c->SetLocation(A_p(a, 0)); break;
  }
}
static void o_hypr(const gp_Hypr2d& H) { h(H.MajorRadius()); h(H.MinorRadius()); o_ax22(H.Axis()); }

static bool curve_op(HG& g, const Op& op, const CV* cv2) {
  if (geom_common(g, op)) return true;
  const Vd& a = op.a;
  Geom2d_Curve* c = dynamic_cast<Geom2d_Curve*>(g.get());
  Geom2d_Line* ln = dynamic_cast<Geom2d_Line*>(g.get());
  Geom2d_Conic* cn = dynamic_cast<Geom2d_Conic*>(g.get());
  Geom2d_Circle* ci = dynamic_cast<Geom2d_Circle*>(g.get());
  Geom2d_Ellipse* el = dynamic_cast<Geom2d_Ellipse*>(g.get());
  Geom2d_Hyperbola* hy = dynamic_cast<Geom2d_Hyperbola*>(g.get());
  Geom2d_Parabola* pa = dynamic_cast<Geom2d_Parabola*>(g.get());
  Geom2d_BezierCurve* bz = dynamic_cast<Geom2d_BezierCurve*>(g.get());
  Geom2d_BSplineCurve* bs = dynamic_cast<Geom2d_BSplineCurve*>(g.get());
  Geom2d_TrimmedCurve* tc = dynamic_cast<Geom2d_TrimmedCurve*>(g.get());
  Geom2d_OffsetCurve* oc = dynamic_cast<Geom2d_OffsetCurve*>(g.get());
  switch (op.code) {
    case C_REVERSE: c->Reverse(); return true;
    case C_REVERSED_PARAM: h(c->ReversedParameter(A(a, 0))); return true;
    case C_TRANSFORMED_PARAM: h(c->TransformedParameter(A(a, 0), tr_gp(tr_make(a, 1)))); return true;
    case C_PARAM_TRANSFORM: h(c->ParametricTransformation(tr_gp(tr_make(a, 0)))); return true;
    case C_D0: o_p(c->EvalD0(A(a, 0))); return true;
    case C_D1: { Geom2d_Curve::ResD1 r = c->EvalD1(A(a, 0)); o_p(r.Point); o_v(r.D1); return true; }
    case C_D2: { Geom2d_Curve::ResD2 r = c->EvalD2(A(a, 0)); o_p(r.Point); o_v(r.D1); o_v(r.D2); return true; }
    case C_D3: { Geom2d_Curve::ResD3 r = c->EvalD3(A(a, 0)); o_p(r.Point); o_v(r.D1); o_v(r.D2); o_v(r.D3); return true; }
    case C_DN: o_v(c->EvalDN(A(a, 0), AI(a, 1))); return true;
    case C_VALUE: o_p(c->Value(A(a, 0))); return true;
    case C_FIRST: h(c->FirstParameter()); return true;
    case C_LAST: h(c->LastParameter()); return true;
    case C_IS_CLOSED: ob(c->IsClosed()); return true;
    case C_IS_PERIODIC: ob(c->IsPeriodic()); return true;
    case C_PERIOD: h(c->Period()); return true;
    case C_CONTINUITY: in((int)c->Continuity()); return true;
    case C_IS_CN: ob(c->IsCN(AI(a, 0))); return true;
    case C_REVERSED: o_state(c->Reversed()); return true;
    // line
    case L_SET_LIN2D: ln->SetLin2d(gp_Lin2d(A_ax(a, 0))); return true;
    case L_SET_DIR: ln->SetDirection(gp_Dir2d(A(a, 0), A(a, 1))); return true;
    case L_SET_LOC: ln->SetLocation(A_p(a, 0)); return true;
    case L_SET_POS: ln->SetPosition(A_ax(a, 0)); return true;
    case L_LIN2D: { gp_Lin2d L = ln->Lin2d(); o_p(L.Location()); o_d(L.Direction()); return true; }
    case L_DISTANCE: h(ln->Distance(A_p(a, 0))); return true;
    case L_DIRECTION: o_d(ln->Direction()); return true;
    case L_LOCATION: o_p(ln->Location()); return true;
    case L_POSITION: o_ax(ln->Position()); return true;
    // conics
    case K_SET_AXIS: case K_SET_AXIS3: case K_SET_XAXIS: case K_SET_YAXIS: case K_SET_LOC: conic_set_ax(cn, op); return true;
    case K_XAXIS: o_ax(cn->XAxis()); return true;
    case K_YAXIS: o_ax(cn->YAxis()); return true;
    case K_ECC: h(cn->Eccentricity()); return true;
    case K_LOCATION: o_p(cn->Location()); return true;
    case K_POSITION: o_ax22(cn->Position()); return true;
    case CI_SET_RADIUS: ci->SetRadius(A(a, 0)); return true;
    case CI_SET_CIRC2D: ci->SetCirc2d(gp_Circ2d(A_ax(a, 0), A(a, 5), AI(a, 4) != 0)); return true;
    case CI_CIRC2D: { gp_Circ2d C = ci->Circ2d(); h(C.Radius()); o_ax22(C.Position()); return true; }
    case E_SET_MAJOR: if (el) el->SetMajorRadius(A(a, 0)); else hy->SetMajorRadius(A(a, 0)); return true;
    case E_SET_MINOR: if (el) el->SetMinorRadius(A(a, 0)); else hy->SetMinorRadius(A(a, 0)); return true;
    case E_SET_ELIPS2D: el->SetElips2d(gp_Elips2d(A_ax(a, 0), A(a, 5), A(a, 6), AI(a, 4) != 0)); return true;
    case E_ELIPS2D: { gp_Elips2d E = el->Elips2d(); h(E.MajorRadius()); h(E.MinorRadius()); o_ax22(E.Axis()); return true; }
    case EH_DIRECTRIX1: o_ax(el ? el->Directrix1() : hy->Directrix1()); return true;
    case EH_DIRECTRIX2: o_ax(el ? el->Directrix2() : hy->Directrix2()); return true;
    case EH_FOCAL: h(el ? el->Focal() : (hy ? hy->Focal() : pa->Focal())); return true;
    case EH_FOCUS1: o_p(el ? el->Focus1() : hy->Focus1()); return true;
    case EH_FOCUS2: o_p(el ? el->Focus2() : hy->Focus2()); return true;
    case EH_MAJOR: h(el ? el->MajorRadius() : hy->MajorRadius()); return true;
    case EH_MINOR: h(el ? el->MinorRadius() : hy->MinorRadius()); return true;
    case EH_PARAMETER: h(el ? el->Parameter() : (hy ? hy->Parameter() : pa->Parameter())); return true;
    case H_ASYMPTOTE1: o_ax(hy->Asymptote1()); return true;
    case H_ASYMPTOTE2: o_ax(hy->Asymptote2()); return true;
    case H_CONJ1: o_hypr(hy->ConjugateBranch1()); return true;
    case H_CONJ2: o_hypr(hy->ConjugateBranch2()); return true;
    case H_OTHER: o_hypr(hy->OtherBranch()); return true;
    case H_SET_HYPR2D: hy->SetHypr2d(gp_Hypr2d(A_ax(a, 0), A(a, 5), A(a, 6), AI(a, 4) != 0)); return true;
    case H_HYPR2D: o_hypr(hy->Hypr2d()); return true;
    case PA_SET_FOCAL: pa->SetFocal(A(a, 0)); return true;
    case PA_SET_PARAB2D: pa->SetParab2d(gp_Parab2d(A_ax(a, 0), A(a, 5), AI(a, 4) != 0)); return true;
    case PA_PARAB2D: { gp_Parab2d P = pa->Parab2d(); h(P.Focal()); o_ax22(P.Axis()); return true; }
    case PA_DIRECTRIX: o_ax(pa->Directrix()); return true;
    case PA_FOCUS: o_p(pa->Focus()); return true;
    // bounded curves
    case B_START: { Geom2d_BoundedCurve* b = dynamic_cast<Geom2d_BoundedCurve*>(g.get()); o_p(b->StartPoint()); return true; }
    case B_END: { Geom2d_BoundedCurve* b = dynamic_cast<Geom2d_BoundedCurve*>(g.get()); o_p(b->EndPoint()); return true; }
    // Bezier and B-spline curves
    case BB_INCREASE: if (bz) bz->Increase(AI(a, 0)); else bs->IncreaseDegree(AI(a, 0)); return true;
    case BB_INS_AFTER: if (bz) bz->InsertPoleAfter(AI(a, 0), A_p(a, 1), A(a, 3)); else bs->InsertPoleAfter(AI(a, 0), A_p(a, 1), A(a, 3)); return true;
    case BB_INS_BEFORE: if (bz) bz->InsertPoleBefore(AI(a, 0), A_p(a, 1), A(a, 3)); else bs->InsertPoleBefore(AI(a, 0), A_p(a, 1), A(a, 3)); return true;
    case BB_REMOVE_POLE: if (bz) bz->RemovePole(AI(a, 0)); else bs->RemovePole(AI(a, 0)); return true;
    case BB_SEGMENT: if (bz) bz->Segment(A(a, 0), A(a, 1)); else bs->Segment(A(a, 0), A(a, 1), A(a, 2)); return true;
    case BB_SET_POLE: if (bz) bz->SetPole(AI(a, 0), A_p(a, 1)); else bs->SetPole(AI(a, 0), A_p(a, 1)); return true;
    case BB_SET_POLE_W: if (bz) bz->SetPole(AI(a, 0), A_p(a, 1), A(a, 3)); else bs->SetPole(AI(a, 0), A_p(a, 1), A(a, 3)); return true;
    case BB_SET_WEIGHT: if (bz) bz->SetWeight(AI(a, 0), A(a, 1)); else bs->SetWeight(AI(a, 0), A(a, 1)); return true;
    case BB_RESOLUTION: { double u = 0; if (bz) bz->Resolution(A(a, 0), u); else bs->Resolution(A(a, 0), u); h(u); return true; }
    case BB_POLE: o_p(bz ? bz->Pole(AI(a, 0)) : bs->Pole(AI(a, 0))); return true;
    case BB_WEIGHT: h(bz ? bz->Weight(AI(a, 0)) : bs->Weight(AI(a, 0))); return true;
    case BB_NBPOLES: in(bz ? bz->NbPoles() : bs->NbPoles()); return true;
    case BB_DEGREE: in(bz ? bz->Degree() : bs->Degree()); return true;
    case BB_IS_RATIONAL: ob(bz ? bz->IsRational() : bs->IsRational()); return true;
    case BB_POLES: o_arr_p(bz ? bz->Poles() : bs->Poles()); return true;
    case BB_WEIGHTS: { const NCollection_Array1<double>* w = bz ? bz->Weights() : bs->Weights(); ob(w != nullptr); if (w) o_arr_f(*w); return true; }
    case BB_KNOTS: o_arr_f(bz ? bz->Knots() : bs->Knots()); return true;
    case BB_MULTS: o_arr_i(bz ? bz->Multiplicities() : bs->Multiplicities()); return true;
    case BB_KNOTSEQ: o_arr_f(bz ? bz->KnotSequence() : bs->KnotSequence()); return true;
    case BB_MAXDEGREE: in(bz ? Geom2d_BezierCurve::MaxDegree() : Geom2d_BSplineCurve::MaxDegree()); return true;
    // B-spline curves
    case S_INC_MULT: bs->IncreaseMultiplicity(AI(a, 0), AI(a, 1)); return true;
    case S_INC_MULT_RANGE: bs->IncreaseMultiplicity(AI(a, 0), AI(a, 1), AI(a, 2)); return true;
    case S_INCR_MULT: bs->IncrementMultiplicity(AI(a, 0), AI(a, 1), AI(a, 2)); return true;
    case S_INS_KNOT: bs->InsertKnot(A(a, 0), AI(a, 1), A(a, 2)); return true;
    case S_INS_KNOTS: {
      int n = AI(a, 2);
      NCollection_Array1<double> K(1, std::max(n, 1)); NCollection_Array1<int> M(1, std::max(n, 1));
      for (int i = 1; i <= n; ++i) { K(i) = A(a, (size_t)(2 + i)); M(i) = AI(a, (size_t)(2 + n + i)); }
      if (n == 0) { NCollection_Array1<double> K0; NCollection_Array1<int> M0; bs->InsertKnots(K0, M0, A(a, 0), AI(a, 1) != 0); }
      else bs->InsertKnots(K, M, A(a, 0), AI(a, 1) != 0);
      return true;
    }
    case S_REMOVE_KNOT: ob(bs->RemoveKnot(AI(a, 0), AI(a, 1), A(a, 2))); return true;
    case S_SET_KNOT: bs->SetKnot(AI(a, 0), A(a, 1)); return true;
    case S_SET_KNOT_M: bs->SetKnot(AI(a, 0), A(a, 1), AI(a, 2)); return true;
    case S_SET_KNOTS: {
      int n = AI(a, 0);
      NCollection_Array1<double> K(1, std::max(n, 1));
      for (int i = 1; i <= n; ++i) K(i) = A(a, (size_t)i);
      if (n == 0) { NCollection_Array1<double> K0; bs->SetKnots(K0); } else bs->SetKnots(K);
      return true;
    }
    case S_PERIODIC_NORM: { double u = A(a, 0); bs->PeriodicNormalization(u); h(u); return true; }
    case S_SET_PERIODIC: bs->SetPeriodic(); return true;
    case S_SET_ORIGIN: bs->SetOrigin(AI(a, 0)); return true;
    case S_SET_NOT_PERIODIC: bs->SetNotPeriodic(); return true;
    case S_MOVE_POINT: { int f = 0, l = 0; bs->MovePoint(A(a, 0), A_p(a, 1), AI(a, 3), AI(a, 4), f, l); in(f); in(l); return true; }
    case S_MOVE_TANGENT: { int e = 0; bs->MovePointAndTangent(A(a, 0), A_p(a, 1), gp_Vec2d(A(a, 3), A(a, 4)), A(a, 5), AI(a, 6), AI(a, 7), e); in(e); return true; }
    case S_IS_G1: ob(bs->IsG1(A(a, 0), A(a, 1), A(a, 2))); return true;
    case S_LOCATE: { int i1 = 0, i2 = 0; bs->LocateU(A(a, 0), A(a, 1), i1, i2, AI(a, 2) != 0); in(i1); in(i2); return true; }
    case S_LOCAL_VALUE: o_p(bs->LocalValue(A(a, 0), AI(a, 1), AI(a, 2))); return true;
    case S_LOCAL_D0: { gp_Pnt2d P; bs->LocalD0(A(a, 0), AI(a, 1), AI(a, 2), P); o_p(P); return true; }
    case S_LOCAL_D1: { gp_Pnt2d P; gp_Vec2d V1; bs->LocalD1(A(a, 0), AI(a, 1), AI(a, 2), P, V1); o_p(P); o_v(V1); return true; }
    case S_LOCAL_D2: { gp_Pnt2d P; gp_Vec2d V1, V2; bs->LocalD2(A(a, 0), AI(a, 1), AI(a, 2), P, V1, V2); o_p(P); o_v(V1); o_v(V2); return true; }
    case S_LOCAL_D3: { gp_Pnt2d P; gp_Vec2d V1, V2, V3; bs->LocalD3(A(a, 0), AI(a, 1), AI(a, 2), P, V1, V2, V3); o_p(P); o_v(V1); o_v(V2); o_v(V3); return true; }
    case S_LOCAL_DN: o_v(bs->LocalDN(A(a, 0), AI(a, 1), AI(a, 2), AI(a, 3))); return true;
    case S_KNOT: h(bs->Knot(AI(a, 0))); return true;
    case S_MULT: in(bs->Multiplicity(AI(a, 0))); return true;
    case S_DISTRIBUTION: in((int)bs->KnotDistribution()); return true;
    case S_FIRST_UKNOT: in(bs->FirstUKnotIndex()); return true;
    case S_LAST_UKNOT: in(bs->LastUKnotIndex()); return true;
    case S_NBKNOTS: in(bs->NbKnots()); return true;
    // trimmed and offset curves
    case TR_SET_TRIM: tc->SetTrim(A(a, 0), A(a, 1), AI(a, 2) != 0, AI(a, 3) != 0); return true;
    case TR_BASIS: o_state(tc ? HG(tc->BasisCurve()) : HG(oc->BasisCurve())); return true;
    case O_SET_BASIS: oc->SetBasisCurve(build(*cv2), AI(a, 0) != 0); return true;
    case O_SET_OFFSET: oc->SetOffsetValue(A(a, 0)); return true;
    case O_OFFSET: h(oc->Offset()); return true;
    case O_BASIS_CONT: in((int)oc->GetBasisCurveContinuity()); return true;
    // evaluation representations (a = kind max_deriv has_domain first last scale offset null_rep)
    case R_SET_EREP: {
      occ::handle<Geom2dEval_RepCurveDesc::Base> d;
      int kind = AI(a, 0);
      if (kind == 0) d = new Geom2dEval_RepCurveDesc::Full();
      else if (kind == 1) { occ::handle<Geom2dEval_RepCurveDesc::DerivBounded> x = new Geom2dEval_RepCurveDesc::DerivBounded(); x->MaxDerivOrder = (size_t)std::max(AI(a, 1), 0); d = x; }
      else {
        occ::handle<Geom2dEval_RepCurveDesc::Mapped> x = new Geom2dEval_RepCurveDesc::Mapped();
        x->MaxDerivOrder = (size_t)std::max(AI(a, 1), 0);
        if (AI(a, 2)) { Geom2dEval_RepCurveDesc::Domain1d dom; dom.First = A(a, 3); dom.Last = A(a, 4); x->Domain = dom; }
        x->ParamMap.Scale = A(a, 5); x->ParamMap.Offset = A(a, 6);
        d = x;
      }
      if (!AI(a, 7)) d->Representation = build(*cv2);
      if (bz) bz->SetEvalRepresentation(d); else if (bs) bs->SetEvalRepresentation(d); else oc->SetEvalRepresentation(d);
      return true;
    }
    case R_CLEAR_EREP: if (bz) bz->ClearEvalRepresentation(); else if (bs) bs->ClearEvalRepresentation(); else oc->ClearEvalRepresentation(); return true;
    case R_HAS_EREP: ob(erep_of(g.get()) != nullptr); return true;
    case R_EREP_KIND: { auto* d = erep_of(g.get()); in(d ? (int)d->GetKind() : -1); return true; }
    default: std::fprintf(stderr, "curve_op: bad code %d\n", op.code); std::abort();
  }
}

// =================================================================================
// running and planning operation sequences
typedef std::function<bool(HG&, const Op&)> RunFn;     // executes one operation
typedef std::function<bool(HG&, const Op&)> CheckFn;   // true when the operation is unchecked in OCCT (not executed)
// Plans a sequence: `choose` appends the next operation(s) for the live object; they are executed on a scratch object
// (nothing is printed); the sequence ends after an operation that throws or is unchecked.
static Ops plan_inner(HG live, int rounds, const std::function<void(const HG&, Ops&)>& choose, const RunFn& run, const CheckFn& check) {
  Ops ops; g_quiet = true;
  bool done = false;
  for (int r = 0; r < rounds && !done; ++r) {
    Ops add; choose(live, add);
    for (const Op& op : add) {
      ops.push_back(op);
      if (std::getenv("GEOM2D_PLANLOG")) { std::fprintf(stderr, "PLAN op %d:", op.code); for (double v : op.a) std::fprintf(stderr, " %.17g", v); std::fprintf(stderr, "\n"); }
      if (check(live, op)) { done = true; break; }
      try { run(live, op); } catch (Standard_Failure&) { done = true; break; }
    }
  }
  g_quiet = false;
  return ops;
}
// The planning runs in a child process: the operations it executes may corrupt the heap or crash (OCCT does not check
// everything); the parent gets the planned operations through a pipe, or notes the failure (the case is then dropped).
static Ops plan(HG live, int rounds, const std::function<void(const HG&, Ops&)>& choose, const RunFn& run, const CheckFn& check) {
  int fd[2];
  if (pipe(fd) != 0) { g_plan_failed = true; return Ops(); }
  fflush(stdout); fflush(stderr);
  pid_t p = fork();
  if (p == 0) {
    close(fd[0]);
    signal(SIGSEGV, SIG_DFL); signal(SIGABRT, SIG_DFL); signal(SIGFPE, SIG_DFL);
    alarm(20);
    Ops ops;
    try { ops = plan_inner(live, rounds, choose, run, check); } catch (...) { _exit(3); }
    std::string s;
    auto put = [&](const void* d, size_t n) { s.append((const char*)d, n); };
    uint64_t rng = state; put(&rng, 8);
    uint32_t n = (uint32_t)ops.size(); put(&n, 4);
    for (const Op& o : ops) { int32_t c = o.code; uint32_t k = (uint32_t)o.a.size(); put(&c, 4); put(&k, 4); for (double v : o.a) put(&v, 8); }
    size_t off = 0;
    while (off < s.size()) { ssize_t w = write(fd[1], s.data() + off, s.size() - off); if (w <= 0) _exit(4); off += (size_t)w; }
    _exit(0);
  }
  close(fd[1]);
  std::string s; char tmp[4096]; ssize_t r;
  while ((r = read(fd[0], tmp, sizeof tmp)) > 0) s.append(tmp, (size_t)r);
  close(fd[0]);
  int st = 0; waitpid(p, &st, 0);
  Ops ops;
  bool ok = WIFEXITED(st) && WEXITSTATUS(st) == 0 && s.size() >= 12;
  size_t at = 0;
  auto get = [&](void* d, size_t n) { if (at + n > s.size()) { ok = false; return; } std::memcpy(d, s.data() + at, n); at += n; };
  if (ok) {
    uint64_t rng = 0; get(&rng, 8); state = rng;   // the random numbers the child used are consumed
    uint32_t n = 0; get(&n, 4);
    for (uint32_t i = 0; ok && i < n; ++i) { int32_t c = 0; uint32_t k = 0; get(&c, 4); get(&k, 4); Op o; o.code = c; for (uint32_t j = 0; ok && j < k; ++j) { double v = 0; get(&v, 8); o.a.push_back(v); } ops.push_back(o); }
  }
  if (!ok) { g_plan_failed = true; return Ops(); }
  return ops;
}

// curve families: `crv [crv] ops`
static void run_curve_ops(const CV& cv, const CV* cv2, const Ops& ops) {
  HG g = build(cv);
  for (const Op& op : ops) {
    if (curve_unchecked(g, op)) { unchecked(); return; }
    curve_op(g, op, cv2);
  }
}
static void trace_cv(const CV& c) {
  if (!std::getenv("GEOM2D_TRACE")) return;
  std::fprintf(stderr, "TRACE kind %d deg %d per %d rat %d nk %zu np %zu mults:", c.kind, c.deg, c.per, c.rat, c.knots.size(), c.px.size());
  for (int m : c.mults) std::fprintf(stderr, " %d", m);
  std::fprintf(stderr, " knots:"); for (double k : c.knots) std::fprintf(stderr, " %g", k);
  try { HC b = build(c); if (auto* s = dynamic_cast<Geom2d_BSplineCurve*>(b.get())) std::fprintf(stderr, " dist %d", (int)s->KnotDistribution()); } catch (...) {}
  std::fprintf(stderr, "\n");
}
static Ops plan_curve(const CV& cv, const CV* cv2, int rounds, const std::function<void(const HC&, Ops&)>& choose) {
  trace_cv(cv);
  HG g;
  try { g = build(cv); } catch (Standard_Failure&) { return Ops{mk(C_STATE)}; }
  return plan(g, rounds, [&](const HG& live, Ops& add) { choose(HC(dynamic_cast<Geom2d_Curve*>(live.get())), add); },
              [&](HG& x, const Op& op) { return curve_op(x, op, cv2); },
              [&](HG& x, const Op& op) { return curve_unchecked(x, op); });
}
static bool emit_curve(const char* N, const CV& cv, const CV* cv2, const Ops& ops, bool libm, bool guard = false) {
  begin(N, cv2 ? "crv crv ops" : "crv ops");
  W_crv(cv); if (cv2) W_crv(*cv2); W_ops(ops);
  bar();
  g_guard = guard;
  return emit([&] { run_curve_ops(cv, cv2, ops); }, libm);
}

// ---------------------------------------------------------------------------------
// operation constructors
static Op op_state() { return mk(C_STATE); }
static Op op_d(int code, double u, int n = 0) { Vd a{u}; if (code == C_DN) a.push_back(n); return mk(code, a); }
static Op op_tr(int code, const Tr& t, double u = 0, bool lead_u = false) { Vd a; if (lead_u) a.push_back(u); tr_args(t, a); return mk(code, a); }
// random evaluation operations (codes C_D0 .. C_VALUE) at parameters of c
static void add_evals(Ops& ops, const HC& c, int n, int maxdn = 5) {
  for (int i = 0; i < n; ++i) {
    int r = rint(0, 5);
    double u = rparam(c);
    if (r == 0) ops.push_back(op_d(C_D0, u));
    else if (r == 1) ops.push_back(op_d(C_D1, u));
    else if (r == 2) ops.push_back(op_d(C_D2, u));
    else if (r == 3) ops.push_back(op_d(C_D3, u));
    else if (r == 4) ops.push_back(op_d(C_DN, u, rint(1, maxdn)));
    else ops.push_back(op_d(C_VALUE, u));
  }
}
// the transformation operations of a curve family: Transform, TransformedParameter, ParametricTransformation, COPY_XFORM
static Op rtr_op(int code, const HC& c, bool rot, bool nonzero = true) {
  Tr t = rtr(rot, false);
  if (code == C_TRANSFORMED_PARAM) return op_tr(code, t, rparam(c), true);
  return op_tr(code, t);
}

// =================================================================================
// families: points, vectors, directions, axes, transformations
// geometry operations (codes 22..35 and 4, 21) with random arguments
static Op rgeom_op(bool rot, bool vector_like = false) {
  int c = rint(0, 13);
  switch (c) {
    case 0: return mk(22, Vd{crd(), crd()});
    case 1: { double dx, dy; rdir(dx, dy); return mk(23, Vd{crd(), crd(), dx, dy}); }
    case 2: return mk(rot ? 24 : 26, rot ? Vd{crd(), crd(), rang()} : Vd{crd(), crd()});
    case 3: return mk(25, Vd{crd(), crd(), nz(0.25, 3)});
    case 4: return mk(26, Vd{crd(), crd()});
    case 5: return mk(27, Vd{crd(), crd(), crd(), crd()});
    case 6: return mk(28, Vd{crd(), crd()});
    case 7: { double dx, dy; rdir(dx, dy); return mk(29, Vd{crd(), crd(), dx, dy}); }
    case 8: return mk(rot ? 30 : 32, rot ? Vd{crd(), crd(), rang()} : Vd{crd(), crd()});
    case 9: return mk(31, Vd{crd(), crd(), nz(0.25, 3)});
    case 10: return mk(32, Vd{crd(), crd()});
    case 11: return mk(33, Vd{crd(), crd(), crd(), crd()});
    case 12: { Vd a; tr_args(rtr(rot), a); return mk(coin(0.5) ? 34 : 4, a); }
    default: { Vd a; tr_args(rtr(rot), a); return mk(21, a); }
  }
}
static void fam_point() {
  { LOOP("point_ops", 24) {
      int ctor = rint(0, 1); double x = crd(), y = crd();
      HG g0 = mk_cpoint(ctor, x, y);
      Ops ops = plan(g0, 4, [&](const HG&, Ops& add) {
        int r = rint(0, 9);
        add.push_back(mk(r == 0 ? P_SET_COORD : r == 1 ? P_SET_PNT : r == 2 ? P_SET_X : r == 3 ? P_SET_Y : r == 4 ? P_COORD : r == 5 ? P_PNT2D : r == 6 ? P_X : r == 7 ? P_Y : r == 8 ? P_DIST : P_SQDIST,
                        r <= 1 ? Vd{crd(), crd()} : r <= 3 ? Vd{crd()} : r >= 8 ? Vd{crd(), crd(), (double)rint(0, 1)} : Vd{}));
        if (r <= 3) add.push_back(mk(1));
      }, [&](HG& g, const Op& o) { return point_op(g, o); }, [&](HG&, const Op&) { return false; });
      begin(N, "i f f ops"); W_i(ctor); W_f(x); W_f(y); W_ops(ops); bar();
      emit([&] { HG g = mk_cpoint(ctor, x, y); for (const Op& o : ops) point_op(g, o); }, false);
    } }
  { LOOP("point_xform", 24) {
      int ctor = rint(0, 1); double x = crd(), y = crd();
      Ops ops;
      for (int k = rint(2, 5); k > 0; --k) { ops.push_back(rgeom_op(true)); }
      ops.push_back(mk(1));
      begin(N, "i f f ops"); W_i(ctor); W_f(x); W_f(y); W_ops(ops); bar();
      emit([&] { HG g = mk_cpoint(ctor, x, y); for (const Op& o : ops) { if (geom_unchecked(o)) { unchecked(); return; } point_op(g, o); } }, true);
    } }
}
static void rvec_other(Vd& a, bool dir_only = false) {
  int k = dir_only ? 0 : rint(0, 1);
  double x, y; rdir(x, y);
  if (k == 1 && coin(0.5)) { x = crd(); y = crd(); }
  if (k == 1 && coin(0.08)) { x = 0; y = 0; }
  a.push_back(k); a.push_back(x); a.push_back(y);
}
static void fam_vec() {
  struct Spec { int kind; int ctor; double v[4]; };
  auto mkspec = [&](int kind) {
    Spec s; s.kind = kind; s.ctor = kind == 0 ? rint(0, 1) : rint(0, 2);
    if (kind == 0) { rdir(s.v[0], s.v[1]); s.v[2] = s.v[3] = 0; }
    else { s.v[0] = coin(0.3) ? nice() : crd(); s.v[1] = coin(0.3) ? nice() : crd(); s.v[2] = crd(); s.v[3] = crd(); }
    return s;
  };
  auto runv = [&](const Spec& s, const Ops& ops) {
    HG g = mk_vec(s.kind, s.ctor, s.v[0], s.v[1], s.v[2], s.v[3]);
    for (const Op& o : ops) { if (vec_unchecked(g, o)) { unchecked(); return; } vec_op(g, o); }
  };
  auto line = [&](const char* N, const Spec& s, const Ops& ops, bool libm) {
    begin(N, "i i f f f f ops"); W_i(s.kind); W_i(s.ctor); W_f(s.v[0]); W_f(s.v[1]); W_f(s.v[2]); W_f(s.v[3]); W_ops(ops); bar();
    emit([&] { runv(s, ops); }, libm);
  };
  auto pl = [&](const Spec& s, int rounds, const std::function<void(const HG&, Ops&)>& ch) {
    HG g0 = mk_vec(s.kind, s.ctor, s.v[0], s.v[1], s.v[2], s.v[3]);
    return plan(g0, rounds, ch, [&](HG& g, const Op& o) { return vec_op(g, o); }, [&](HG& g, const Op& o) { return vec_unchecked(g, o); });
  };
  // direction: accessors, setters, relations with another vector
  { LOOP("direction_ops", 28) {
      Spec s = mkspec(0);
      Ops ops = pl(s, 4, [&](const HG&, Ops& add) {
        int r = rint(0, 12); Vd a;
        switch (r) {
          case 0: a = {nz(0.1, 3), nz(0.1, 3)}; if (coin(0.3)) a = {0, 0}; add.push_back(mk(V_SET_COORD, a)); add.push_back(mk(1)); break;
          case 1: a = {nz(0.1, 3)}; if (coin(0.25)) a = {0.0}; add.push_back(mk(V_SET_X, a)); add.push_back(mk(1)); break;
          case 2: a = {nz(0.1, 3)}; if (coin(0.25)) a = {0.0}; add.push_back(mk(V_SET_Y, a)); add.push_back(mk(1)); break;
          case 3: { double x, y; rdir(x, y); add.push_back(mk(V_SET_VEC, Vd{x, y})); add.push_back(mk(1)); break; }
          case 4: add.push_back(mk(V_REVERSE)); add.push_back(mk(1)); break;
          case 5: add.push_back(mk(V_REVERSED)); break;
          case 6: rvec_other(a); add.push_back(mk(V_ANGLE, a)); break;
          case 7: add.push_back(mk(V_COORD)); add.push_back(mk(V_X)); add.push_back(mk(V_Y)); break;
          case 8: add.push_back(mk(V_MAG)); add.push_back(mk(V_SQMAG)); break;
          case 9: rvec_other(a); add.push_back(mk(V_CROSSED, a)); break;
          case 10: rvec_other(a); add.push_back(mk(V_DOT, a)); break;
          case 11: add.push_back(mk(V_VEC2D)); add.push_back(mk(V_DIR2D)); break;
          default: rvec_other(a); add.push_back(mk(V_ANGLE, a)); add.push_back(mk(V_DOT, a)); break;
        }
      });
      line(N, s, ops, true);
    } }
  { LOOP("direction_xform", 24) {
      Spec s = mkspec(0); Ops ops;
      for (int k = rint(2, 5); k > 0; --k) ops.push_back(rgeom_op(true));
      ops.push_back(mk(1));
      line(N, s, ops, true);
    } }
  // vector with magnitude
  { LOOP("vecmag_ops", 28) {
      Spec s = mkspec(1);
      Ops ops = pl(s, 4, [&](const HG&, Ops& add) {
        int r = rint(0, 11); Vd a;
        switch (r) {
          case 0: add.push_back(mk(V_SET_COORD, Vd{crd(), crd()})); add.push_back(mk(1)); break;
          case 1: add.push_back(mk(V_SET_X, Vd{crd()})); add.push_back(mk(1)); break;
          case 2: add.push_back(mk(V_SET_Y, Vd{crd()})); add.push_back(mk(1)); break;
          case 3: add.push_back(mk(V_SET_VEC, Vd{crd(), crd()})); add.push_back(mk(1)); break;
          case 4: add.push_back(mk(V_REVERSE)); add.push_back(mk(1)); break;
          case 5: add.push_back(mk(V_REVERSED)); break;
          case 6: rvec_other(a); add.push_back(mk(V_ANGLE, a)); break;
          case 7: add.push_back(mk(V_COORD)); add.push_back(mk(V_X)); add.push_back(mk(V_Y)); break;
          case 8: add.push_back(mk(V_MAG)); add.push_back(mk(V_SQMAG)); break;
          case 9: rvec_other(a); add.push_back(mk(V_CROSSED, a)); break;
          case 10: rvec_other(a); add.push_back(mk(V_DOT, a)); break;
          default: add.push_back(mk(V_VEC2D)); break;
        }
      });
      line(N, s, ops, true);
    } }
  { LOOP("vecmag_arith", 28) {
      Spec s = mkspec(1);
      if (coin(0.3)) { s.v[0] = s.v[1] = s.v[2] = s.v[3] = 0.0; }
      Ops ops = pl(s, 4, [&](const HG&, Ops& add) {
        int r = rint(0, 9); Vd a;
        switch (r) {
          case 0: rvec_other(a); add.push_back(mk(V_ADD, a)); add.push_back(mk(1)); break;
          case 1: rvec_other(a); add.push_back(mk(V_ADDED, a)); break;
          case 2: rvec_other(a); add.push_back(mk(V_SUBTRACT, a)); add.push_back(mk(1)); break;
          case 3: rvec_other(a); add.push_back(mk(V_SUBTRACTED, a)); break;
          case 4: add.push_back(mk(V_MULTIPLY, Vd{nz(0.1, 3)})); add.push_back(mk(1)); break;
          case 5: add.push_back(mk(V_MULTIPLIED, Vd{coin(0.1) ? 0.0 : nz(0.1, 3)})); break;
          case 6: add.push_back(mk(V_DIVIDE, Vd{coin(0.08) ? 0.0 : nz(0.1, 3)})); add.push_back(mk(1)); break;
          case 7: add.push_back(mk(V_DIVIDED, Vd{coin(0.08) ? 0.0 : nz(0.1, 3)})); break;
          case 8: add.push_back(mk(V_NORMALIZE)); add.push_back(mk(1)); break;
          default: add.push_back(mk(V_NORMALIZED)); break;
        }
      });
      line(N, s, ops, false);
    } }
  { LOOP("vecmag_xform", 24) {
      Spec s = mkspec(1); Ops ops;
      for (int k = rint(2, 5); k > 0; --k) ops.push_back(rgeom_op(true));
      ops.push_back(mk(1));
      line(N, s, ops, true);
    } }
}
static void fam_axis() {
  struct Spec { int ctor; double o[2]; double d[2]; };
  auto mkspec = [&]() { Spec s; s.ctor = rint(0, 1); s.o[0] = crd(); s.o[1] = crd(); rdir(s.d[0], s.d[1]); return s; };
  auto line = [&](const char* N, const Spec& s, const Ops& ops, bool libm) {
    begin(N, "i f f f f ops"); W_i(s.ctor); W_f(s.o[0]); W_f(s.o[1]); W_f(s.d[0]); W_f(s.d[1]); W_ops(ops); bar();
    emit([&] { HG g = mk_axis(s.ctor, s.o[0], s.o[1], s.d[0], s.d[1]); for (const Op& o : ops) { if (geom_unchecked(o)) { unchecked(); return; } axis_op(g, o); } }, libm);
  };
  { LOOP("axis_ops", 24) {
      Spec s = mkspec(); Ops ops;
      for (int k = rint(2, 5); k > 0; --k) {
        int r = rint(0, 8); double dx, dy; rdir(dx, dy);
        switch (r) {
          case 0: ops.push_back(mk(X_REVERSE)); ops.push_back(mk(1)); break;
          case 1: ops.push_back(mk(X_REVERSED)); break;
          case 2: ops.push_back(mk(X_SET_AXIS, Vd{crd(), crd(), dx, dy})); ops.push_back(mk(1)); break;
          case 3: ops.push_back(mk(X_SET_DIR, Vd{dx, dy})); ops.push_back(mk(1)); break;
          case 4: ops.push_back(mk(X_SET_LOC, Vd{crd(), crd()})); ops.push_back(mk(1)); break;
          case 5: ops.push_back(mk(X_ANGLE, Vd{crd(), crd(), dx, dy})); break;
          case 6: ops.push_back(mk(X_AX2D)); break;
          case 7: ops.push_back(mk(X_DIRECTION)); break;
          default: ops.push_back(mk(X_LOCATION)); break;
        }
      }
      line(N, s, ops, true);
    } }
  { LOOP("axis_xform", 24) {
      Spec s = mkspec(); Ops ops;
      for (int k = rint(2, 5); k > 0; --k) ops.push_back(rgeom_op(true));
      ops.push_back(mk(1));
      line(N, s, ops, true);
    } }
}
// Geom2d_Transformation
static void fam_trsf() {
  auto line = [&](const char* N, const Tr& t0, const Ops& ops, bool libm) {
    begin(N, "tr ops"); W_tr(t0); W_ops(ops); bar();
    emit([&] {
      if (tr_unchecked(t0)) { unchecked(); return; }
      HT t = mk_tr_obj(t0);
      for (const Op& o : ops) { if (trsf_unchecked(t, o)) { unchecked(); return; } trsf_op(t, o); }
    }, libm);
  };
  auto pl = [&](const Tr& t0, int rounds, const std::function<void(const HT&, Ops&)>& ch) {
    if (tr_unchecked(t0)) return Ops{mk(T_STATE)};
    HT t = mk_tr_obj(t0);
    HG dummy;
    Ops ops; g_quiet = true; bool done = false;
    for (int r = 0; r < rounds && !done; ++r) {
      Ops add; ch(t, add);
      for (const Op& op : add) {
        ops.push_back(op);
        if (trsf_unchecked(t, op)) { done = true; break; }
        try { trsf_op(t, op); } catch (Standard_Failure&) { done = true; break; }
      }
    }
    g_quiet = false; return ops;
  };
  auto pt_ops = [&](Ops& add, int n) { for (int i = 0; i < n; ++i) add.push_back(mk(T_TRANSFORMS, Vd{crd(), crd()})); };
  auto rt = [&](bool rot, bool zero = false, bool bad = false, int force = -1) { return rtr(rot, zero, bad, force); };
  { LOOP("trsf_set", 32) {
      Tr t0 = rt(true, false, true);
      Ops ops = pl(t0, 3, [&](const HT&, Ops& add) {
        Vd a; tr_args(rt(true, false, true, coin(0.3) ? 9 : -1), a);
        add.push_back(mk(coin(0.8) ? T_SET : (coin(0.5) ? T_SET_TRSF : T_FROM_TRSF), a)); add.push_back(mk(T_STATE)); pt_ops(add, 1);
        if (coin(0.3)) add.push_back(mk(T_VALUE, Vd{(double)rint(1, 2), (double)rint(1, 3)}));
      });
      line(N, t0, ops, true);
    } }
  { LOOP("trsf_invert", 28) {
      Tr t0 = rt(false, true);
      Ops ops = pl(t0, 3, [&](const HT&, Ops& add) {
        int r = rint(0, 3);
        if (r == 0) { add.push_back(mk(T_INVERTED)); }
        else if (r == 1) { add.push_back(mk(T_INVERT)); add.push_back(mk(T_STATE)); }
        else if (r == 2) { add.push_back(mk(T_INVERT)); pt_ops(add, 1); add.push_back(mk(T_INVERT)); add.push_back(mk(T_STATE)); }
        else { Vd a; tr_args(rt(false), a); add.push_back(mk(T_SET, a)); add.push_back(mk(T_INVERTED)); }
      });
      line(N, t0, ops, true);
    } }
  { LOOP("trsf_multiply", 36) {
      Tr t0 = rt(false, false, true);
      Ops ops = pl(t0, 3, [&](const HT&, Ops& add) {
        Vd a; tr_args(rt(false, false, true), a);
        int r = rint(0, 4);
        add.push_back(mk(r == 0 ? T_MULTIPLY : r == 1 ? T_MULTIPLIED : r == 2 ? T_PREMULTIPLY : r == 3 ? T_OPERATOR_MUL : T_OPERATOR_MULEQ, a));
        add.push_back(mk(T_STATE)); pt_ops(add, 1);
      });
      line(N, t0, ops, true);
    } }
  { LOOP("trsf_power", 32) {
      Tr t0 = rt(false, true);
      Ops ops = pl(t0, 3, [&](const HT&, Ops& add) {
        int n = coin(0.15) ? rint(-2, 2) : rint(-5, 7);
        add.push_back(mk(coin(0.5) ? T_POWER : T_POWERED, Vd{(double)n}));
        add.push_back(mk(T_STATE));
      });
      line(N, t0, ops, true);
    } }
  { LOOP("trsf_misc", 30) {
      Tr t0 = rt(true, false, true);
      Ops ops = pl(t0, 4, [&](const HT&, Ops& add) {
        int r = rint(0, 4);
        if (r == 0) { add.push_back(mk(T_COPY_SWAP)); add.push_back(mk(T_STATE)); }
        else if (r == 1) { bool bad = coin(0.15); add.push_back(mk(T_VALUE, Vd{(double)(bad ? rint(0, 3) : rint(1, 2)), (double)(bad ? rint(0, 4) : rint(1, 3))})); }
        else if (r == 2) { pt_ops(add, 2); }
        else if (r == 3) { Vd a; tr_args(rt(true), a); add.push_back(mk(T_FROM_TRSF, a)); add.push_back(mk(T_STATE)); }
        else { Vd a; tr_args(rt(true), a); add.push_back(mk(T_SET_TRSF, a)); add.push_back(mk(T_VALUE, Vd{(double)rint(1, 2), (double)rint(1, 3)})); }
      });
      line(N, t0, ops, true);
    } }
}

// =================================================================================
// families: curves
static Geom2d_BSplineCurve* BS(const HC& c) { return dynamic_cast<Geom2d_BSplineCurve*>(c.get()); }
static Geom2d_BezierCurve* BZ(const HC& c) { return dynamic_cast<Geom2d_BezierCurve*>(c.get()); }
static double rweight() { return coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3); }

// random trimmed and offset curves over a basis recipe
static CV rtrim(const CV& b, bool allow_bad = true) {
  double f = 0, l = 1; bool per = false;
  try { HC hb = build(b); f = hb->FirstParameter(); l = hb->LastParameter(); per = hb->IsPeriodic(); } catch (Standard_Failure&) {}
  bool inf = f < -1e50 || l > 1e50;
  if (inf) { f = -4; l = 4; }
  double sp = l - f, u1, u2;
  double r = rnd(0, 1);
  bool sense = coin(0.75), adj = coin(0.8);
  if (per) {
    u1 = f + rnd(-2, 3) * sp; if (coin(0.25)) u1 = f + 0.25 * rint(-4, 8) * sp;
    double d = r < 0.15 ? sp : (r < 0.25 ? -rnd(0.1, 1.2) * sp : (r < 0.3 ? 0.0 : rnd(0.05, 1.7) * sp));
    u2 = u1 + d;
  } else if (r < 0.52) { u1 = f + rnd(0, 0.6) * sp; u2 = u1 + rnd(0.05, 0.4) * sp; if (u2 > l) u2 = l; }
  else if (r < 0.62) { u2 = f + rnd(0, 0.6) * sp; u1 = u2 + rnd(0.05, 0.4) * sp; if (u1 > l) u1 = l; }      // reversed
  else if (r < 0.70) { u1 = f; u2 = l; }
  else if (r < 0.74 && allow_bad) { u1 = f + 0.3 * sp; u2 = u1; }                                         // equal: throws
  else if (r < 0.80) { u1 = f - 5e-10; u2 = l + 5e-10; }                                                   // inside PConfusion
  else if (r < 0.86 && allow_bad) { u1 = f - 1e-7; u2 = l; }                                               // outside: throws
  else if (r < 0.92 && allow_bad) { u1 = f + 0.5 * sp; u2 = l + 0.5 * sp; }
  else { u1 = f + rnd(0, 0.5) * sp; u2 = u1 + rnd(0.1, 0.5) * sp; }
  if (inf && !allow_bad && u1 == u2) u2 = u1 + 1;
  return mk_trim(b, u1, u2, sense, adj);
}
static CV roff(const CV& b) {
  bool notc = coin(0.12);
  return mk_off(b, roffset(), notc);
}
static CV rany(int depth = 0, bool allow_c0 = true) {
  double r = rnd(0, 1);
  CV b = rbasic();
  if (depth >= 2 || r < 0.5) return b;
  if (r < 0.75) return rtrim(b);
  if (r < 0.92) return roff(b);
  return depth == 0 ? (coin(0.5) ? roff(rtrim(b)) : rtrim(roff(b))) : b;
}
// a curve of a given family of kinds for the shared evaluation families
static CV rkind(int which) {
  switch (which) {
    case 0: return rline();
    case 1: return rcircle();
    case 2: return rellipse();
    case 3: return rhyperbola();
    case 4: { CV c = rparabola(); fix_parabola(c); return c; }
    case 5: return rbezier();
    default: return rbspline(0, true, 4);
  }
}
static bool emit_fam(const char* N, const CV& cv, const CV* cv2, const Ops& ops, bool libm, bool guard = false) { return emit_curve(N, cv, cv2, ops, libm, guard); }
#define CF(NAME, TARGET) LOOP(NAME, TARGET)

static void fam_curve_common() {
  { CF("curve_params", 28) {
      CV cv = rany();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        add.push_back(mk(C_FIRST)); add.push_back(mk(C_LAST)); add.push_back(mk(C_IS_CLOSED)); add.push_back(mk(C_IS_PERIODIC));
        add.push_back(mk(C_CONTINUITY));
        for (int k = rint(1, 3); k > 0; --k) add.push_back(mk(C_IS_CN, Vd{(double)(coin(0.06) ? -1 : rint(0, 5))}));
        if (c->IsPeriodic() || coin(0.15)) add.push_back(mk(C_PERIOD));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  const struct { const char* n; int code; int cnt; } ev[] = {{"curve_d0", C_D0, 6}, {"curve_d1", C_D1, 5}, {"curve_d2", C_D2, 5}, {"curve_d3", C_D3, 4}, {"curve_dn", C_DN, 5}};
  for (auto& e : ev) {
    LOOP(e.n, 28) {
      CV cv = rkind(rint(0, 6));
      if (coin(0.2)) cv = rtrim(cv, false);
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        for (int k = 0; k < e.cnt; ++k) {
          double u = rparam(c);
          if (e.code == C_DN) add.push_back(op_d(C_DN, u, coin(0.07) ? rint(-1, 0) : rint(1, 6)));
          else add.push_back(op_d(e.code, u));
        }
        if (e.code == C_D0 && coin(0.4)) add.push_back(op_d(C_VALUE, rparam(c)));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    }
  }
  { CF("curve_reverse", 28) {
      CV cv = rany();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        add.push_back(mk(C_REVERSED_PARAM, Vd{rparam(c)}));
        add.push_back(mk(C_REVERSED)); add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE));
        add_evals(add, c, 2); add.push_back(mk(C_FIRST)); add.push_back(mk(C_LAST));
        if (coin(0.4)) { add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE)); }
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("curve_xform", 32) {
      CV cv = rany();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        Tr t = rtr(true);
        add.push_back(op_tr(C_TRANSFORMED_PARAM, t, rparam(c), true));
        add.push_back(op_tr(C_PARAM_TRANSFORM, t));
        add.push_back(op_tr(4, t)); add.push_back(mk(C_STATE));
        add_evals(add, c, 2); add.push_back(mk(C_FIRST)); add.push_back(mk(C_LAST));
        if (coin(0.4)) { add.push_back(op_tr(4, rtr(true))); add.push_back(mk(C_STATE)); }
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("curve_geom_ops", 32) {
      CV cv = rany();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        add.push_back(rgeom_op(true)); add.push_back(mk(C_STATE));
        if (coin(0.4)) add_evals(add, c, 1);
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("curve_copy", 28) {
      CV cv = rany();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        int r = rint(0, 3);
        if (r == 0) { add.push_back(mk(35)); add.push_back(mk(C_STATE)); }
        else if (r == 1) { add.push_back(op_tr(21, rtr(true))); add.push_back(mk(C_STATE)); }
        else if (r == 2) { add.push_back(mk(C_REVERSED)); add.push_back(mk(C_STATE)); }
        else { add_evals(add, c, 2); add.push_back(mk(35)); }
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
}

static void fam_line_conics() {
  { CF("line_ops", 24) {
      CV cv = rline();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        double dx, dy; rdir(dx, dy);
        int r = rint(0, 8);
        switch (r) {
          case 0: add.push_back(mk(L_SET_LIN2D, Vd{crd(), crd(), dx, dy})); add.push_back(mk(C_STATE)); break;
          case 1: add.push_back(mk(L_SET_DIR, Vd{dx, dy})); add.push_back(mk(C_STATE)); break;
          case 2: add.push_back(mk(L_SET_LOC, Vd{crd(), crd()})); add.push_back(mk(C_STATE)); break;
          case 3: add.push_back(mk(L_SET_POS, Vd{crd(), crd(), dx, dy})); add.push_back(mk(C_STATE)); break;
          case 4: add.push_back(mk(L_LIN2D)); add.push_back(mk(L_DIRECTION)); add.push_back(mk(L_LOCATION)); add.push_back(mk(L_POSITION)); break;
          case 5: add.push_back(mk(L_DISTANCE, Vd{crd(), crd()})); break;
          case 6: add_evals(add, c, 3); break;
          case 7: add.push_back(mk(C_FIRST)); add.push_back(mk(C_LAST)); add.push_back(mk(C_IS_CLOSED)); add.push_back(mk(C_IS_PERIODIC)); add.push_back(mk(C_CONTINUITY)); add.push_back(mk(C_IS_CN, Vd{3.0})); if (coin(0.25)) add.push_back(mk(C_PERIOD)); break;
          default: add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE)); add.push_back(mk(C_REVERSED_PARAM, Vd{crd()})); break;
        }
      });
      emit_fam(N, cv, nullptr, ops, true);
    } }
  auto conic_ops = [&](int kind, const char* name) {
    LOOP(name, 28) {
      CV cv = rkind(kind);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        double dx, dy; rdir(dx, dy);
        int r = rint(0, 9);
        switch (kind) {
          case 1:
            if (r == 0) add.push_back(mk(CI_SET_RADIUS, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) add.push_back(mk(CI_SET_CIRC2D, Vd{crd(), crd(), dx, dy, (double)rint(0, 1), rradius(true)}));
            else if (r == 2) add.push_back(mk(CI_CIRC2D));
            break;
          case 2:
            if (r == 0) add.push_back(mk(E_SET_MAJOR, Vd{coin(0.15) ? rnd(0, 1) : rnd(1, 9)}));
            else if (r == 1) add.push_back(mk(E_SET_MINOR, Vd{coin(0.15) ? rnd(-1, 0) : rnd(0, 3)}));
            else if (r == 2) { double b = rradius(true); add.push_back(mk(E_SET_ELIPS2D, Vd{crd(), crd(), dx, dy, (double)rint(0, 1), b + rnd(0, 4), b})); }
            else if (r == 3) add.push_back(mk(E_ELIPS2D));
            else if (r == 4) { add.push_back(mk(EH_DIRECTRIX1)); add.push_back(mk(EH_DIRECTRIX2)); }
            else if (r == 5) { add.push_back(mk(EH_FOCAL)); add.push_back(mk(EH_FOCUS1)); add.push_back(mk(EH_FOCUS2)); add.push_back(mk(EH_PARAMETER)); add.push_back(mk(EH_MAJOR)); add.push_back(mk(EH_MINOR)); }
            break;
          case 3:
            if (r == 0) add.push_back(mk(E_SET_MAJOR, Vd{coin(0.12) ? -rnd(0.1, 1) : rradius(true)}));
            else if (r == 1) add.push_back(mk(E_SET_MINOR, Vd{coin(0.12) ? -rnd(0.1, 1) : rradius(true)}));
            else if (r == 2) add.push_back(mk(H_SET_HYPR2D, Vd{crd(), crd(), dx, dy, (double)rint(0, 1), rradius(true), rradius(true)}));
            else if (r == 3) add.push_back(mk(H_HYPR2D));
            else if (r == 4) { add.push_back(mk(EH_DIRECTRIX1)); add.push_back(mk(EH_DIRECTRIX2)); }
            else if (r == 5) { add.push_back(mk(EH_FOCAL)); add.push_back(mk(EH_FOCUS1)); add.push_back(mk(EH_FOCUS2)); add.push_back(mk(EH_PARAMETER)); add.push_back(mk(EH_MAJOR)); add.push_back(mk(EH_MINOR)); }
            else if (r == 6) { add.push_back(mk(H_ASYMPTOTE1)); add.push_back(mk(H_ASYMPTOTE2)); }
            else if (r == 7) { add.push_back(mk(H_CONJ1)); add.push_back(mk(H_CONJ2)); add.push_back(mk(H_OTHER)); }
            break;
          default:
            if (r == 0) add.push_back(mk(PA_SET_FOCAL, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) add.push_back(mk(PA_SET_PARAB2D, Vd{crd(), crd(), dx, dy, (double)rint(0, 1), rradius(true)}));
            else if (r == 2) add.push_back(mk(PA_PARAB2D));
            else if (r == 3) { add.push_back(mk(PA_DIRECTRIX)); add.push_back(mk(PA_FOCUS)); }
            else if (r == 4) { add.push_back(mk(EH_FOCAL)); add.push_back(mk(EH_PARAMETER)); }
            break;
        }
        add.push_back(mk(C_STATE));
        if (r >= 8) add_evals(add, c, 2);
        if (r == 7 && kind != 3) { add.push_back(mk(K_ECC)); add.push_back(mk(K_XAXIS)); add.push_back(mk(K_YAXIS)); }
      });
      emit_fam(name, cv, nullptr, ops, true);
    }
  };
  conic_ops(1, "circle_ops"); conic_ops(2, "ellipse_ops"); conic_ops(3, "hyperbola_ops"); conic_ops(4, "parabola_ops");
  { CF("conic_build", 36) {
      int kind = rint(1, 4);
      CV cv = rkind(kind);
      // invalid variants: OCCT raises for a negative radius, a major radius below the minor one, a negative focal length
      if (coin(0.25)) {
        switch (kind) {
          case 1: cv.a = -rnd(0.1, 2); break;
          case 2: if (coin(0.5)) { cv.b = rnd(1, 3); cv.a = cv.b - rnd(0.1, 0.9); } else cv.b = -rnd(0.1, 1); break;
          case 3: if (coin(0.5)) cv.a = -rnd(0.1, 2); else cv.b = -rnd(0.1, 2); break;
          default: if (cv.ctor != 3) cv.a = -rnd(0.1, 2); break;
        }
      }
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(K_ECC)); ops.push_back(mk(K_XAXIS)); ops.push_back(mk(K_YAXIS)); ops.push_back(mk(K_LOCATION)); ops.push_back(mk(K_POSITION));
      ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST)); ops.push_back(mk(C_IS_CLOSED)); ops.push_back(mk(C_IS_PERIODIC)); ops.push_back(mk(C_CONTINUITY));
      if (kind == 1) ops.push_back(mk(CI_CIRC2D));
      if (kind == 2) { ops.push_back(mk(E_ELIPS2D)); ops.push_back(mk(EH_FOCUS1)); ops.push_back(mk(EH_PARAMETER)); }
      if (kind == 3) { ops.push_back(mk(H_HYPR2D)); ops.push_back(mk(EH_FOCUS2)); ops.push_back(mk(EH_FOCAL)); }
      if (kind == 4) { ops.push_back(mk(PA_PARAB2D)); ops.push_back(mk(PA_FOCUS)); ops.push_back(mk(PA_DIRECTRIX)); }
      emit_fam(N, cv, nullptr, ops, true);
    } }
  { CF("conic_axes", 36) {
      int kind = rint(1, 4);
      CV cv = rkind(kind);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        double dx, dy, ex, ey; rdir(dx, dy); rdir(ex, ey);
        int r = rint(0, 4);
        if (r == 0) add.push_back(mk(K_SET_AXIS, Vd{crd(), crd(), dx, dy, (double)rint(0, 1)}));
        else if (r == 1) add.push_back(mk(K_SET_AXIS3, Vd{crd(), crd(), dx, dy, ex, ey}));
        else if (r == 2) add.push_back(mk(K_SET_XAXIS, Vd{crd(), crd(), dx, dy}));
        else if (r == 3) add.push_back(mk(K_SET_YAXIS, Vd{crd(), crd(), dx, dy}));
        else add.push_back(mk(K_SET_LOC, Vd{crd(), crd()}));
        add.push_back(mk(C_STATE)); add.push_back(mk(K_XAXIS)); add.push_back(mk(K_YAXIS));
        if (coin(0.5)) add_evals(add, c, 1);
        if (coin(0.2)) { add.push_back(mk(C_REVERSE)); add.push_back(mk(K_POSITION)); }
      });
      emit_fam(N, cv, nullptr, ops, true);
    } }
}

// =================================================================================
// Bezier and B-spline curves
static Op op_i(int code, Vd a) { return mk(code, a); }
static double rtol() { double r = rnd(0, 1); return r < 0.2 ? 0.0 : (r < 0.5 ? 1e-9 : (r < 0.8 ? 1e-4 : 1e-2)); }
// a B-spline that OCCT classifies as uniform or quasi-uniform (equal knot spacing, simple inner knots)
static CV rbs_uniform(bool periodic) {
  for (;;) {
    CV c; c.kind = 6; c.deg = rint(2, 4); c.per = periodic ? 1 : 0;
    int nk = rint(c.per ? 4 : 3, 7);
    double k = coin(0.85) ? 0.25 * rint(1, 12) : 0.25 * rint(-8, 8), step = 0.25 * rint(1, 8);
    for (int i = 0; i < nk; ++i) { c.knots.push_back(k); k += step; c.mults.push_back(1); }
    if (!c.per) { c.mults[0] = c.deg + 1; c.mults[(size_t)nk - 1] = c.deg + 1; }
    NCollection_Array1<int> M(1, nk);
    for (int i = 0; i < nk; ++i) M(i + 1) = c.mults[(size_t)i];
    int np = BSplCLib::NbPoles(c.deg, c.per != 0, M);
    if (np < 3 || (c.per && np <= c.deg)) continue;
    for (int i = 0; i < np; ++i) { c.px.push_back(crd()); c.py.push_back(crd()); }
    c.rat = coin(0.4) ? 1 : 0;
    if (c.rat) for (int i = 0; i < np; ++i) c.w.push_back(rweight());
    return c;
  }
}
static CV rbs_mixed(double bias = 0.45) {
  if (coin(bias)) return rbs_uniform(coin(0.35));
  return rbspline(0, true, 4);
}
// invalid variants for the constructors
static void corrupt_bezier(CV& c) {
  switch (rint(0, 4)) {
    case 0: c.px.resize(1); c.py.resize(1); c.rat = 0; c.w.clear(); break;
    case 1: while (c.px.size() < 27) { c.px.push_back(crd()); c.py.push_back(crd()); } c.rat = 0; c.w.clear(); break;
    case 2: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w[(size_t)rint(0, (int)c.w.size() - 1)] = coin(0.5) ? 0.0 : -1.0; break;
    case 3: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w.pop_back(); break;
    default: while (c.px.size() < 26) { c.px.push_back(crd()); c.py.push_back(crd()); } c.rat = 0; c.w.clear(); break;   // 26 poles: the maximum, valid
  }
}
static void corrupt_bspline(CV& c) {
  switch (rint(0, 9)) {
    case 9: if (c.mults.size() >= 3) c.mults[1] = c.deg + 1; else c.mults[0] += 1; break;   // an inner multiplicity above the degree: NbPoles is 0
    case 0: c.deg = 0; break;
    case 1: c.deg = 26; break;
    case 2: c.px.pop_back(); c.py.pop_back(); if (c.rat) c.w.pop_back(); break;
    case 3: c.mults.pop_back(); break;
    case 4: if (c.knots.size() >= 2) c.knots[1] = c.knots[0]; break;
    case 5: if (c.knots.size() >= 2) std::swap(c.knots[0], c.knots[1]); break;
    case 6: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w[(size_t)rint(0, (int)c.w.size() - 1)] = coin(0.5) ? 0.0 : -0.5; break;
    case 7: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w.push_back(1.0); break;
    default: c.mults[0] += 1; break;
  }
}
static int NPo(const HC& c) { return BZ(c) ? BZ(c)->NbPoles() : BS(c)->NbPoles(); }
static int NKn(const HC& c) { return BS(c)->NbKnots(); }
static double rknotu(const HC& c, double outside = 0.0) {
  Geom2d_BSplineCurve* b = BS(c);
  double f = b->FirstParameter(), l = b->LastParameter();
  double r = rnd(0, 1);
  if (r < 0.2) return b->Knot(rint(1, b->NbKnots()));
  if (r < 0.28) return f;
  if (r < 0.36) return l;
  return f + rnd(-outside, 1 + outside) * (l - f);
}

static void fam_bezier() {
  { CF("bezier_build", 40) {
      CV cv = rbezier(true, 12);
      if (coin(0.28)) corrupt_bezier(cv);
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(BB_DEGREE)); ops.push_back(mk(BB_NBPOLES)); ops.push_back(mk(BB_IS_RATIONAL)); ops.push_back(mk(BB_POLES)); ops.push_back(mk(BB_WEIGHTS));
      ops.push_back(mk(BB_KNOTS)); ops.push_back(mk(BB_MULTS)); ops.push_back(mk(BB_KNOTSEQ)); ops.push_back(mk(B_START)); ops.push_back(mk(B_END)); ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST));
      ops.push_back(mk(C_IS_CLOSED)); ops.push_back(mk(C_IS_PERIODIC)); ops.push_back(mk(C_CONTINUITY)); ops.push_back(mk(C_IS_CN, Vd{7.0})); ops.push_back(mk(BB_MAXDEGREE));
      int np = (int)cv.px.size();
      ops.push_back(mk(BB_POLE, Vd{(double)(coin(0.1) ? rint(-1, 0) : rint(1, std::max(np, 1)))}));
      ops.push_back(mk(BB_WEIGHT, Vd{(double)(coin(0.1) ? np + 1 : rint(1, std::max(np, 1)))}));
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bezier_insert_pole", 40) {
      CV cv = rbezier(true, 7);
      if (coin(0.15)) { size_t want = (size_t)(24 + rint(0, 2)); while (cv.px.size() < want) { cv.px.push_back(crd()); cv.py.push_back(crd()); } cv.rat = 0; cv.w.clear(); }
      Ops ops = plan_curve(cv, nullptr, coin(0.25) ? 2 : 1, [&](const HC& c, Ops& add) {
        int n = NPo(c); bool after = coin(0.5);
        int idx = after ? rint(0, n) : rint(1, n + 1);
        if (coin(0.08)) idx = after ? n + 1 + rint(0, 2) : -rint(0, 2);
        double w = coin(0.5) ? 1.0 : (coin(0.1) ? (coin(0.5) ? 0.0 : -1.0) : rweight());
        add.push_back(op_i(after ? BB_INS_AFTER : BB_INS_BEFORE, Vd{(double)idx, crd(), crd(), w}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
  { CF("bezier_remove_pole", 30) {
      CV cv = rbezier(true, 6);
      Ops ops = plan_curve(cv, nullptr, coin(0.25) ? 2 : 1, [&](const HC& c, Ops& add) {
        int n = NPo(c);
        int idx = coin(0.08) ? (coin(0.5) ? 0 : n + 1) : rint(1, n);
        add.push_back(op_i(BB_REMOVE_POLE, Vd{(double)idx})); add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
  { CF("bezier_increase", 30) {
      CV cv = rbezier(true, 7);
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        int d = BZ(c)->Degree();
        int nd = coin(0.1) ? d - rint(1, 2) : (coin(0.06) ? 26 : d + rint(0, 4));
        add.push_back(op_i(BB_INCREASE, Vd{(double)nd})); add.push_back(mk(C_STATE));
        add_evals(add, c, 1);
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
  { CF("bezier_segment", 32) {
      CV cv = rbezier(true, 7);
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        double u1 = coin(0.3) ? 0.25 * rint(0, 4) : rnd(-0.3, 1.2), u2 = coin(0.3) ? 0.25 * rint(0, 4) : rnd(-0.3, 1.3);
        if (coin(0.7) && u1 > u2) std::swap(u1, u2);
        add.push_back(op_i(BB_SEGMENT, Vd{u1, u2, 0.0})); add.push_back(mk(C_STATE));
        add_evals(add, c, 1);
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
  { CF("bezier_set_pole", 36) {
      CV cv = rbezier(true, 7);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        int n = NPo(c); int idx = coin(0.08) ? (coin(0.5) ? 0 : n + 1) : rint(1, n);
        double w = coin(0.1) ? (coin(0.5) ? 0.0 : -2.0) : (coin(0.25) ? 1.0 : rweight());
        int r = rint(0, 2);
        if (r == 0) add.push_back(op_i(BB_SET_POLE, Vd{(double)idx, crd(), crd()}));
        else if (r == 1) add.push_back(op_i(BB_SET_POLE_W, Vd{(double)idx, crd(), crd(), w}));
        else add.push_back(op_i(BB_SET_WEIGHT, Vd{(double)idx, w}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
  { CF("bezier_resolution", 28) {
      CV cv = rbezier(true, 7);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        add.push_back(op_i(BB_RESOLUTION, Vd{coin(0.3) ? 1e-7 : (coin(0.5) ? 1e-3 : rnd(0.01, 2))}));
        if (coin(0.6)) { int n = NPo(c); add.push_back(op_i(BB_SET_POLE, Vd{(double)rint(1, n), crd(), crd()})); }
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
  { CF("bezier_eval", 36) {
      CV cv = rbezier(true, coin(0.1) ? 20 : 8);
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        add_evals(add, c, 6, 6);
        add.push_back(mk(B_START)); add.push_back(mk(B_END));
        if (coin(0.3)) add.push_back(op_d(C_DN, rparam(c), BZ(c)->Degree() + rint(0, 2)));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
}

static void fam_bspline() {
  { CF("bspline_build", 44) {
      CV cv = rbs_mixed();
      if (coin(0.25)) corrupt_bspline(cv);
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(BB_DEGREE)); ops.push_back(mk(BB_NBPOLES)); ops.push_back(mk(S_NBKNOTS)); ops.push_back(mk(BB_IS_RATIONAL));
      ops.push_back(mk(BB_POLES)); ops.push_back(mk(BB_WEIGHTS)); ops.push_back(mk(BB_KNOTS)); ops.push_back(mk(BB_MULTS)); ops.push_back(mk(BB_KNOTSEQ));
      ops.push_back(mk(S_DISTRIBUTION)); ops.push_back(mk(S_FIRST_UKNOT)); ops.push_back(mk(S_LAST_UKNOT));
      ops.push_back(mk(B_START)); ops.push_back(mk(B_END)); ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST));
      ops.push_back(mk(C_IS_CLOSED)); ops.push_back(mk(C_IS_PERIODIC)); ops.push_back(mk(C_CONTINUITY)); ops.push_back(mk(BB_MAXDEGREE));
      int np = (int)cv.px.size(), nk = (int)cv.knots.size();
      ops.push_back(mk(S_KNOT, Vd{(double)(coin(0.1) ? nk + 1 : rint(1, std::max(nk, 1)))}));
      ops.push_back(mk(S_MULT, Vd{(double)(coin(0.1) ? 0 : rint(1, std::max(nk, 1)))}));
      ops.push_back(mk(BB_POLE, Vd{(double)(coin(0.1) ? np + 1 : rint(1, std::max(np, 1)))}));
      ops.push_back(mk(BB_WEIGHT, Vd{(double)(coin(0.1) ? 0 : rint(1, std::max(np, 1)))}));
      for (int k = 0; k < 4; ++k) ops.push_back(mk(C_IS_CN, Vd{(double)k}));
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_eval", 40) {
      CV cv = rbs_mixed(0.0);
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        for (int k = 0; k < 6; ++k) {
          double u = rknotu(c, 0.5); int r = rint(0, 4);
          if (r == 0) add.push_back(op_d(C_D0, u)); else if (r == 1) add.push_back(op_d(C_D1, u)); else if (r == 2) add.push_back(op_d(C_D2, u));
          else if (r == 3) add.push_back(op_d(C_D3, u)); else add.push_back(op_d(C_DN, u, rint(1, BS(c)->Degree() + 2)));
        }
        add.push_back(mk(B_START)); add.push_back(mk(B_END));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_increase_degree", 30) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        int d = BS(c)->Degree();
        int nd = coin(0.1) ? d - rint(1, 2) : (coin(0.05) ? 26 : d + rint(0, 3));
        add.push_back(op_i(BB_INCREASE, Vd{(double)nd})); add.push_back(mk(C_STATE));
        add_evals(add, c, 1, 3);
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_increase_mult", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c); int nk = b->NbKnots(), d = b->Degree();
        int r = rint(0, 2);
        int i1 = rint(1, nk), i2 = rint(i1, nk);
        if (coin(0.04)) { i1 = nk + 1; i2 = nk + 2; }
        int m = r == 2 ? rint(1, 2) : std::min(d + 1, b->Multiplicity(std::min(i1, nk)) + rint(0, 2));
        if (r == 0) add.push_back(op_i(S_INC_MULT, Vd{(double)i1, (double)m}));
        else if (r == 1) add.push_back(op_i(S_INC_MULT_RANGE, Vd{(double)i1, (double)i2, (double)m}));
        else add.push_back(op_i(S_INCR_MULT, Vd{(double)i1, (double)i2, (double)m}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_insert_knot", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c); int d = b->Degree();
        double u = rknotu(c, coin(0.08) ? 0.4 : 0.0);
        if (coin(0.25)) u = 0.5 * (b->Knot(1) + b->Knot(b->NbKnots()));
        add.push_back(op_i(S_INS_KNOT, Vd{u, (double)(coin(0.15) ? rint(d, d + 1) : rint(1, std::max(d - 1, 1))), rtol()}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_insert_knots", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c); int d = b->Degree();
        int n = rint(1, 3); Vd us;
        for (int i = 0; i < n; ++i) us.push_back(rknotu(c, coin(0.06) ? 0.3 : 0.0));
        std::sort(us.begin(), us.end());
        Vd a{rtol(), (double)rint(0, 1), (double)n};
        for (double u : us) a.push_back(u);
        for (int i = 0; i < n; ++i) a.push_back(rint(1, std::max(1, d - 1)));
        add.push_back(op_i(S_INS_KNOTS, a)); add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_remove_knot", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c); int d = b->Degree();
        if (coin(0.55)) {   // insert a knot, then remove it again
          double u = 0.5 * (b->FirstParameter() + b->LastParameter()) + rnd(-0.3, 0.3);
          add.push_back(op_i(S_INS_KNOT, Vd{u, 1.0, 0.0}));
          return;
        }
        int i1 = b->FirstUKnotIndex(), i2 = b->LastUKnotIndex();
        int idx = coin(0.06) ? i2 + 1 : rint(i1, i2);
        int mu = (idx >= 1 && idx <= b->NbKnots()) ? b->Multiplicity(idx) : 1;
        int m = coin(0.1) ? -1 : rint(0, std::max(mu, 1));
        (void)d;
        add.push_back(op_i(S_REMOVE_KNOT, Vd{(double)idx, (double)m, coin(0.5) ? 1e-6 : (coin(0.5) ? 1e-2 : 1e3)}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_insert_pole", 36) {
      CV cv = rbs_mixed(0.85);
      Ops ops = plan_curve(cv, nullptr, coin(0.25) ? 2 : 1, [&](const HC& c, Ops& add) {
        int n = NPo(c); bool after = coin(0.5);
        int idx = after ? rint(0, n) : rint(1, n + 1);
        if (coin(0.06)) idx = after ? n + 2 : -1;
        double w = coin(0.5) ? 1.0 : (coin(0.1) ? 0.0 : rweight());
        add.push_back(op_i(after ? BB_INS_AFTER : BB_INS_BEFORE, Vd{(double)idx, crd(), crd(), w})); add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_remove_pole", 30) {
      CV cv = rbs_mixed(0.85);
      Ops ops = plan_curve(cv, nullptr, coin(0.25) ? 2 : 1, [&](const HC& c, Ops& add) {
        int n = NPo(c); int idx = coin(0.06) ? n + 1 : rint(1, n);
        add.push_back(op_i(BB_REMOVE_POLE, Vd{(double)idx})); add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_segment", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c);
        double f = b->FirstParameter(), l = b->LastParameter(), sp = l - f;
        double u1 = coin(0.3) ? rknotu(c) : f + rnd(0, 0.7) * sp, u2;
        double r = rnd(0, 1);
        if (r < 0.1) u2 = u1 - rnd(0.01, 0.3) * sp;          // reversed: throws (an empty segment would give coinciding knots, which OCCT does not guard against)
        else if (r < 0.25) u2 = l + rnd(0, 0.3) * sp;
        else { u2 = std::min(l, u1 + rnd(0.05, 0.8) * sp); if (u2 - u1 < 0.02 * sp) u1 = std::max(f, u2 - 0.05 * sp); }
        if (b->IsPeriodic() && coin(0.4)) u2 = u1 + rnd(0.05, 1.2) * sp;
        add.push_back(op_i(BB_SEGMENT, Vd{u1, u2, coin(0.6) ? Precision::PConfusion() : (coin(0.5) ? 0.0 : 1e-4)})); add.push_back(mk(C_STATE));
        add_evals(add, c, 1, 3);
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_set_knot", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c); int nk = b->NbKnots();
        int r = rint(0, 2);
        if (r == 2) {
          int n = coin(0.1) ? nk + rint(-1, 1) : nk; Vd a{(double)n};
          double k = b->Knot(1) + rnd(-0.2, 0.2), step = rnd(0.1, 1.0);
          if (coin(0.4)) { for (int i = 1; i <= nk; ++i) a.push_back(b->Knot(i) + rnd(-0.04, 0.04)); a.resize((size_t)n + 1, 1.0); }
          else for (int i = 0; i < n; ++i) { a.push_back(k); k += step * (coin(0.05) ? -1 : 1); }
          add.push_back(op_i(S_SET_KNOTS, a));
        } else {
          int idx = coin(0.06) ? nk + 1 : rint(1, nk);
          double base = (idx >= 1 && idx <= nk) ? b->Knot(idx) : 0.0;
          double k = coin(0.7) ? base + rnd(-0.1, 0.1) : base + rnd(-3, 3);
          if (r == 0) add.push_back(op_i(S_SET_KNOT, Vd{(double)idx, k}));
          else add.push_back(op_i(S_SET_KNOT_M, Vd{(double)idx, k, (double)rint(1, b->Degree() + 1)}));
        }
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_periodic", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c);
        int r = rint(0, 4);
        if (r == 0) { add.push_back(mk(S_SET_PERIODIC)); add.push_back(mk(C_STATE)); }
        else if (r == 1) { add.push_back(mk(S_SET_NOT_PERIODIC)); add.push_back(mk(C_STATE)); }
        else if (r == 2) {
          int i1 = b->FirstUKnotIndex(), i2 = b->LastUKnotIndex();
          add.push_back(op_i(S_SET_ORIGIN, Vd{(double)(coin(0.1) ? i2 + 1 : rint(i1, i2))})); add.push_back(mk(C_STATE));
        } else add.push_back(op_i(S_PERIODIC_NORM, Vd{rknotu(c, 2.0)}));
        add_evals(add, c, 1, 2);
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_set_pole", 36) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        int n = NPo(c); int idx = coin(0.07) ? (coin(0.5) ? 0 : n + 1) : rint(1, n);
        double w = coin(0.1) ? (coin(0.5) ? 0.0 : -2.0) : (coin(0.25) ? 1.0 : rweight());
        int r = rint(0, 2);
        if (r == 0) add.push_back(op_i(BB_SET_POLE, Vd{(double)idx, crd(), crd()}));
        else if (r == 1) add.push_back(op_i(BB_SET_POLE_W, Vd{(double)idx, crd(), crd(), w}));
        else add.push_back(op_i(BB_SET_WEIGHT, Vd{(double)idx, w}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_move_point", 36) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        int n = NPo(c); int i1 = rint(1, n), i2 = rint(i1, n);
        if (coin(0.08)) { i1 = n; i2 = 1; }
        add.push_back(op_i(S_MOVE_POINT, Vd{rknotu(c, 0.0), crd(), crd(), (double)i1, (double)i2}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_move_tangent", 32) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        add.push_back(op_i(S_MOVE_TANGENT, Vd{rknotu(c, 0.0), crd(), crd(), crd(), crd(), coin(0.5) ? 1e-6 : 1e-2, (double)rint(-1, 2), (double)rint(-1, 2)}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("bspline_locate", 36) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        for (int k = 0; k < 4; ++k) add.push_back(op_i(S_LOCATE, Vd{rknotu(c, 1.0), coin(0.3) ? 0.0 : (coin(0.5) ? 1e-9 : 1e-2), (double)rint(0, 1)}));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_local", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c); int nk = b->NbKnots();
        for (int k = 0; k < 5; ++k) {
          int k1 = rint(1, nk - 1), k2 = rint(k1 + 1, nk);
          if (coin(0.05)) k2 = k1;
          double u = rknotu(c, 0.0);
          // inside the knot span
          if (coin(0.6)) u = b->Knot(k1) + rnd(0, 1) * (b->Knot(k2) - b->Knot(k1));
          int r = rint(0, 5);
          if (r == 0) add.push_back(op_i(S_LOCAL_VALUE, Vd{u, (double)k1, (double)k2}));
          else if (r == 1) add.push_back(op_i(S_LOCAL_D0, Vd{u, (double)k1, (double)k2}));
          else if (r == 2) add.push_back(op_i(S_LOCAL_D1, Vd{u, (double)k1, (double)k2}));
          else if (r == 3) add.push_back(op_i(S_LOCAL_D2, Vd{u, (double)k1, (double)k2}));
          else if (r == 4) add.push_back(op_i(S_LOCAL_D3, Vd{u, (double)k1, (double)k2}));
          else add.push_back(op_i(S_LOCAL_DN, Vd{u, (double)k1, (double)k2, (double)rint(1, b->Degree() + 1)}));
        }
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_resolution", 28) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        add.push_back(op_i(BB_RESOLUTION, Vd{coin(0.3) ? 1e-7 : (coin(0.5) ? 1e-3 : rnd(0.01, 2))}));
        if (coin(0.6)) { int n = NPo(c); add.push_back(op_i(BB_SET_POLE, Vd{(double)rint(1, n), crd(), crd()})); }
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_is_g1", 36) {
      CV cv = rbs_mixed(0.0);
      if (coin(0.3)) {   // a degree 1 curve with collinear or sharp corners
        cv = rbspline(1, true, 1);
        if (coin(0.5)) for (size_t i = 0; i < cv.px.size(); ++i) { cv.px[i] = 0.5 * (double)i; cv.py[i] = 0.25 * (double)i; }
      }
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        Geom2d_BSplineCurve* b = BS(c);
        double f = b->FirstParameter(), l = b->LastParameter();
        add.push_back(op_i(S_IS_G1, Vd{f, l, coin(0.5) ? 1e-3 : 0.5}));
        add.push_back(op_i(S_IS_G1, Vd{f + 0.3 * (l - f), l - 0.2 * (l - f), 1e-6}));
        for (int k = 0; k < 3; ++k) add.push_back(mk(C_IS_CN, Vd{(double)rint(0, 5)}));
        add.push_back(mk(C_CONTINUITY));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
}

// =================================================================================
// trimmed curves, offset curves, evaluation representations
static CV rperiodic_basis() {
  double r = rnd(0, 1);
  if (r < 0.4) return rcircle(false);
  if (r < 0.7) return rellipse();
  return rbspline(2, true, 3);
}
static void fam_trimmed() {
  { CF("trimmed_build", 40) {
      CV b = rbasic();
      CV cv = coin(0.1) ? rtrim(rtrim(b, false), true) : rtrim(b);
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST)); ops.push_back(mk(B_START)); ops.push_back(mk(B_END));
      ops.push_back(mk(C_IS_CLOSED)); ops.push_back(mk(C_IS_PERIODIC)); ops.push_back(mk(C_CONTINUITY)); ops.push_back(mk(TR_BASIS));
      ops.push_back(mk(C_IS_CN, Vd{(double)(coin(0.06) ? -1 : rint(0, 3))}));
      { bool pu = true; try { pu = period_unchecked(build(cv)); } catch (Standard_Failure&) {} if (coin(0.15) || !pu) ops.push_back(mk(C_PERIOD)); }
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("trimmed_set_trim", 36) {
      CV cv = rtrim(rbasic(), false);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom2d_TrimmedCurve* t = dynamic_cast<Geom2d_TrimmedCurve*>(c.get());
        HC b = t->BasisCurve();
        double f = b->FirstParameter(), l = b->LastParameter();
        if (f < -1e50 || l > 1e50) { f = -4; l = 4; }
        double sp = l - f, u1, u2;
        if (b->IsPeriodic()) { u1 = f + rnd(-2, 3) * sp; u2 = u1 + (coin(0.2) ? -rnd(0.1, 1) : rnd(0.05, 1.5)) * sp; }
        else { u1 = f + rnd(0.0, 0.9) * sp; u2 = u1 + (coin(0.3) ? -1 : 1) * rnd(0.05, 0.5) * sp; if (!coin(0.12)) { u2 = std::min(std::max(u2, f), l); if (u2 == u1) u2 = u1 + 0.01 * sp; } }
        if (coin(0.05)) u2 = u1;
        add.push_back(op_i(TR_SET_TRIM, Vd{u1, u2, (double)rint(0, 1), (double)(coin(0.8) ? 1 : 0)}));
        add.push_back(mk(C_STATE));
        if (coin(0.5)) add_evals(add, c, 1);
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("trimmed_periodic", 36) {
      CV cv = rtrim(rperiodic_basis(), false);
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        add.push_back(mk(C_STATE)); add.push_back(mk(C_IS_CLOSED)); add.push_back(mk(C_IS_PERIODIC)); add.push_back(mk(C_PERIOD)); add.push_back(mk(C_FIRST)); add.push_back(mk(C_LAST));
        add_evals(add, c, 2);
        add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE)); add.push_back(mk(C_IS_CLOSED)); add.push_back(mk(C_IS_PERIODIC));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("trimmed_eval", 32) {
      CV cv = rtrim(rbasic(), false);
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        add_evals(add, c, 6);
        add.push_back(mk(B_START)); add.push_back(mk(B_END));
        add.push_back(op_d(C_REVERSED_PARAM, rparam(c)));
        add.push_back(op_tr(C_TRANSFORMED_PARAM, rtr(true), rparam(c), true));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
}

static CV roffset_basis() {
  double r = rnd(0, 1);
  if (r < 0.55) return rbasic();
  if (r < 0.75) return rtrim(rbasic(), false);
  if (r < 0.9) return roff(rbasic());
  return rtrim(roff(rbasic()), false);
}
// curves whose derivative vanishes somewhere (cusps): Bezier / B-spline with repeated poles, circle of radius 0
static CV rsingular() {
  double r = rnd(0, 1);
  if (r < 0.15) { CV c = rcircle(); c.a = 0.0; return c; }
  if (r < 0.55) {
    CV c = rbezier(false, 5);
    if (c.px.size() < 3) { c.px.push_back(crd()); c.py.push_back(crd()); if (c.rat) c.w.push_back(1.0); }
    c.px[1] = c.px[0]; c.py[1] = c.py[0];
    if (coin(0.4) && c.px.size() >= 4) { size_t n = c.px.size(); c.px[n - 2] = c.px[n - 1]; c.py[n - 2] = c.py[n - 1]; }
    return c;
  }
  CV c = rbspline(1, false, 3);
  if (c.px.size() >= 3) { size_t k = (size_t)rint(0, (int)c.px.size() - 2); c.px[k + 1] = c.px[k]; c.py[k + 1] = c.py[k]; }
  return c;
}
static void fam_offset() {
  { CF("offset_build", 44) {
      CV b = roffset_basis();
      if (coin(0.25)) { b = rbspline(0, true, 3); }
      CV cv = roff(b);
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(O_OFFSET)); ops.push_back(mk(O_BASIS_CONT)); ops.push_back(mk(C_CONTINUITY)); ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST));
      ops.push_back(mk(C_IS_CLOSED)); ops.push_back(mk(C_IS_PERIODIC)); ops.push_back(mk(TR_BASIS));
      ops.push_back(mk(C_IS_CN, Vd{(double)(coin(0.06) ? -1 : rint(0, 3))}));
      { bool pu = true; try { pu = period_unchecked(build(cv)); } catch (Standard_Failure&) {} if (coin(0.15) || !pu) ops.push_back(mk(C_PERIOD)); }
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("offset_eval", 44) {
      CV cv = roff(roffset_basis());
      if (coin(0.4)) cv.notc = 0;
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        add_evals(add, c, 6, 5);
        if (coin(0.3)) add.push_back(op_d(C_DN, rparam(c), rint(1, 6)));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("offset_singular", 40) {
      CV b = rsingular();
      CV cv = roff(b); cv.notc = coin(0.7) ? 1 : 0;
      cv.off = coin(0.2) ? 0.0 : roffset();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        // the parameters where the derivative vanishes: the ends and the knots
        for (int k = 0; k < 5; ++k) {
          double f = c->FirstParameter(), l = c->LastParameter();
          double u = k < 2 ? rparam(c) : (coin(0.5) ? f : (coin(0.5) ? l : rparam(c)));
          int r = rint(0, 4);
          if (r == 0) add.push_back(op_d(C_D0, u)); else if (r == 1) add.push_back(op_d(C_D1, u)); else if (r == 2) add.push_back(op_d(C_D2, u));
          else if (r == 3) add.push_back(op_d(C_D3, u)); else add.push_back(op_d(C_DN, u, rint(1, 5)));
        }
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("offset_set", 40) {
      CV cv = roff(roffset_basis()); cv.notc = coin(0.5) ? 1 : 0;
      CV b2 = roffset_basis();
      Ops ops = plan_curve(cv, &b2, 3, [&](const HC& c, Ops& add) {
        int r = rint(0, 4);
        if (r == 0) { add.push_back(op_i(O_SET_BASIS, Vd{(double)rint(0, 1)})); add.push_back(mk(C_STATE)); }
        else if (r == 1) { add.push_back(op_i(O_SET_OFFSET, Vd{roffset()})); add.push_back(mk(O_OFFSET)); }
        else if (r == 2) { add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE)); }
        else if (r == 3) { add.push_back(op_tr(4, rtr(true))); add.push_back(mk(C_STATE)); }
        else add_evals(add, c, 2);
      });
      emit_fam(N, cv, &b2, ops, true, true);
    } }
}

// a representation curve for the evaluation-representation families
static CV rrep() {
  double r = rnd(0, 1);
  if (r < 0.3) return rline();
  if (r < 0.6) return rcircle(false);
  if (r < 0.85) return rbezier(false, 5);
  return rbspline(1, false, 3);
}
static CV rowner() {
  double r = rnd(0, 1);
  if (r < 0.4) return rbezier(false, 6);
  if (r < 0.75) return rbspline(0, false, 4);
  return roff(rbasic());
}
// SET_EREP arguments: kind, max derivative order, has domain, first, last, scale, offset, null representation
static Op erep_op(int kind, int maxd, bool dom, double f, double l, double sc, double off, bool null_rep) {
  return mk(R_SET_EREP, Vd{(double)kind, (double)maxd, dom ? 1.0 : 0.0, f, l, sc, off, null_rep ? 1.0 : 0.0});
}
static void erep_evals(Ops& add, const HC& c, int n, double lo, double hi) {
  for (int i = 0; i < n; ++i) {
    double u = coin(0.3) ? c->FirstParameter() + rnd(-0.3, 1.3) * (c->LastParameter() - c->FirstParameter()) : lo + rnd(-0.3, 1.3) * (hi - lo);
    if (u < -1e50 || u > 1e50) u = rnd(-2, 2);
    int r = rint(0, 4);
    if (r == 0) add.push_back(op_d(C_D0, u)); else if (r == 1) add.push_back(op_d(C_D1, u)); else if (r == 2) add.push_back(op_d(C_D2, u));
    else if (r == 3) add.push_back(op_d(C_D3, u)); else add.push_back(op_d(C_DN, u, rint(1, 5)));
  }
}
static void fam_erep() {
  auto go = [&](const char* name, int target, int mode) {
    LOOP(name, target) {
      CV cv = rowner(), rep = rrep();
      Ops ops = plan_curve(cv, &rep, 3, [&](const HC& c, Ops& add) {
        double f = c->FirstParameter(), l = c->LastParameter();
        if (f < -1e50 || l > 1e50) { f = -2; l = 2; }
        if (add.empty() && !erep_of(c.get())) {
          if (mode == 0) add.push_back(erep_op(0, 3, false, 0, 0, 1, 0, false));
          else if (mode == 1) add.push_back(erep_op(1, rint(0, 3), false, 0, 0, 1, 0, false));
          else if (mode == 2) {
            double sc = coin(0.3) ? 1.0 : (coin(0.5) ? rnd(0.5, 2) : -rnd(0.5, 2)), off = coin(0.4) ? 0.0 : rnd(-1, 1);
            bool dom = coin(0.6);
            add.push_back(erep_op(2, rint(0, 3), dom, f + rnd(0, 0.5) * (l - f), f + rnd(0.5, 1.2) * (l - f), sc, off, false));
          } else if (mode == 3) {   // invalid descriptors
            int r = rint(0, 3);
            if (r == 0) add.push_back(erep_op(rint(0, 2), 3, false, 0, 0, 1, 0, true));
            else if (r == 1) add.push_back(erep_op(2, 3, false, 0, 0, 0.0, 0.5, false));
            else if (r == 2) add.push_back(erep_op(2, 3, false, 0, 0, 5e-8, 0.5, false));
            else add.push_back(erep_op(2, 3, coin(0.5), 0, 1, 1e-4 * rint(1, 3), 0.25, false));
          } else {
            add.push_back(erep_op(rint(0, 2), rint(0, 3), false, 0, 0, 1, 0, false));
          }
          add.push_back(mk(R_HAS_EREP)); add.push_back(mk(R_EREP_KIND));
          return;
        }
        if (mode == 4) {
          int r = rint(0, 7);
          if (r == 0) add.push_back(mk(35));
          else if (r == 1) add.push_back(op_tr(21, rtr(false)));
          else if (r == 2) add.push_back(op_tr(4, rtr(false)));
          else if (r == 3) add.push_back(mk(C_REVERSE));
          else if (r == 4) add.push_back(mk(R_CLEAR_EREP));
          else if (r == 5) { if (BZ(c)) add.push_back(op_i(BB_SET_POLE, Vd{1.0, crd(), crd()})); else if (BS(c)) add.push_back(op_i(BB_SET_POLE, Vd{1.0, crd(), crd()})); else add.push_back(op_i(O_SET_OFFSET, Vd{roffset()})); }
          else if (r == 6) { if (BZ(c)) add.push_back(op_i(BB_SEGMENT, Vd{0.1, 0.8, 0.0})); else if (BS(c)) add.push_back(op_i(BB_SEGMENT, Vd{f + 0.1 * (l - f), f + 0.8 * (l - f), Precision::PConfusion()})); else add.push_back(mk(C_FIRST)); }
          else if (dynamic_cast<Geom2d_OffsetCurve*>(c.get())) add.push_back(op_i(O_SET_BASIS, Vd{0.0}));
          else add.push_back(mk(C_FIRST));
          add.push_back(mk(R_HAS_EREP));
        }
        erep_evals(add, c, 4, f, l);
      });
      emit_fam(name, cv, &rep, ops, true, true);
    }
  };
  go("evalrep_full", 24, 0); go("evalrep_bounded", 28, 1); go("evalrep_mapped", 32, 2); go("evalrep_invalid", 20, 3); go("evalrep_copy", 36, 4);
}

// =================================================================================
static void on_crash(int sig) {
  const char* m = "oracle crashed in family ";
  (void)!write(2, m, std::strlen(m));
  (void)!write(2, cur_name.c_str(), cur_name.size());
  (void)!write(2, "\n", 1);
  (void)!write(2, buf.c_str(), std::min<size_t>(buf.size(), 4000));
  (void)!write(2, "\n", 1);
  _exit(100 + sig);
}
int main(int argc, char** argv) {
  signal(SIGSEGV, on_crash); signal(SIGABRT, on_crash); signal(SIGFPE, on_crash);
  alarm(3000);
  const char* only = argc > 1 ? argv[1] : nullptr;
  if (argc > 2) g_only_fam = argv[2];
  auto want = [&](const char* n) {
    if (!only) return true;
    std::string o = std::string(",") + only + ",", k = std::string(",") + n + ",";
    return o.find(k) != std::string::npos;
  };
  if (want("point")) fam_point();
  if (want("vec")) fam_vec();
  if (want("axis")) fam_axis();
  if (want("trsf")) fam_trsf();
  if (want("curve")) fam_curve_common();
  if (want("conic")) fam_line_conics();
  if (want("bezier")) fam_bezier();
  if (want("bspline")) fam_bspline();
  if (want("trim")) fam_trimmed();
  if (want("offset")) fam_offset();
  if (want("erep")) fam_erep();
  for (const std::string& n : fam_order)
    std::fprintf(stderr, "%-24s %5d lines, %5d dropped (%d uninit)\n", n.c_str(), fam_lines[n], fam_dropped.count(n) ? fam_dropped[n] : 0,
                 fam_dropped.count(n + "!uninit") ? fam_dropped[n + "!uninit"] : 0);
  for (auto& kv : fam_dropped) if (kv.first.find("!libm") != std::string::npos || kv.first.find("!crash") != std::string::npos || kv.first.find("~changed") != std::string::npos)
    std::fprintf(stderr, "   dropped: %s %d\n", kv.first.c_str(), kv.second);
  return 0;
}
