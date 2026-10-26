// Deterministic reference calls for OCCT 8.0.1's 3D geometry: package Geom (cpp/Geom/, TKG3d) and the parts of GeomEval it uses
// (GeomEval_RepCurveDesc, GeomEval_RepSurfaceDesc, GeomEval_RepUtils): Geom_Geometry (Mirror about a point, an axis and a plane,
// Rotate, Scale, Translate, Transform and the Mirrored / Rotated / Scaled / Translated / Transformed copies, Copy), Geom_CartesianPoint,
// Geom_Vector with Geom_Direction and Geom_VectorWithMagnitude, Geom_AxisPlacement with Geom_Axis1Placement and Geom_Axis2Placement,
// Geom_Transformation, Geom_Curve (the evaluations, Reverse / Reversed, ReversedParameter, TransformedParameter,
// ParametricTransformation, IsClosed, IsPeriodic, Period, Continuity, IsCN), Geom_Line, Geom_Conic with Geom_Circle / Geom_Ellipse /
// Geom_Hyperbola / Geom_Parabola, Geom_BoundedCurve with Geom_BezierCurve, Geom_BSplineCurve and Geom_TrimmedCurve, Geom_OffsetCurve;
// Geom_Surface (the evaluations, UReverse / VReverse and the parameter variants, TransformParameters, ParametricTransformation,
// bounds, closedness, periodicity, continuity, UIso / VIso, IsCNu / IsCNv), Geom_ElementarySurface with Geom_Plane,
// Geom_CylindricalSurface, Geom_ConicalSurface, Geom_SphericalSurface, Geom_ToroidalSurface, Geom_SweptSurface with
// Geom_SurfaceOfLinearExtrusion and Geom_SurfaceOfRevolution, Geom_BoundedSurface with Geom_BezierSurface, Geom_BSplineSurface and
// Geom_RectangularTrimmedSurface, Geom_OffsetSurface (including Geom_OsculatingSurface through UOsculatingSurface /
// VOsculatingSurface), and the evaluation representations that Bezier, B-spline, offset, extrusion, revolution and offset-surface
// objects can carry. DumpJson is not covered.
//
// Output: one record per line, `<family> <inputs...> | <outputs...>`. Floats are 16-hex-digit f64 bit patterns, ints and bools are
// decimal. No text token occurs. Every input is printed on the left of ` | ` in the order of the family's schema (`begin(name, schema)`):
// the words `f` `i` (scalars), `tr` (a transformation recipe), `crv` (a curve recipe, recursive for trimmed and offset curves),
// `srf` (a surface recipe, recursive for swept, trimmed and offset surfaces) and `ops` (a sequence of operations on the object, each
// with its code and f64 arguments); their grammar is in docs/api-geom.md. The oracle aborts if the printed tokens do not match the
// schema. The outputs are the values the operations return, in order (the `state` of an object after the operations that change it),
// `throws` (OCCT raised an exception: the Rust API must return Err) or `unchecked` (a Standard_*_Raise_if check of OCCT is compiled out
// of the libraries and the call has no defined result in OCCT: the call is not executed and the Rust API must return Err). A sequence
// of operations stops at the first one that throws or is unchecked, and the whole line then prints that marker.
// Every call is run 6 times with differently filled stack and heap (random words, all 0, all 1; malloc perturbation); a case whose
// results differ, that is not finite, or that crashes in a child process, is dropped. Families that call sin / cos / tan / sinh /
// cosh / atan2 / acos / pow only keep cases whose outputs are unchanged (within 2 ulps) when every C library result is moved by an ulp.
// Build: c++ -std=c++17 -O2 oracle/geom.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKG3d -lTKG2d -lTKMath -lTKernel -ldl
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
#include <Geom_Axis1Placement.hxx>
#include <Geom_Axis2Placement.hxx>
#include <Geom_AxisPlacement.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_BSplineSurface.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BezierSurface.hxx>
#include <Geom_BoundedCurve.hxx>
#include <Geom_BoundedSurface.hxx>
#include <Geom_CartesianPoint.hxx>
#include <Geom_Circle.hxx>
#include <Geom_Conic.hxx>
#include <Geom_ConicalSurface.hxx>
#include <Geom_Curve.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <Geom_Direction.hxx>
#include <Geom_ElementarySurface.hxx>
#include <Geom_Ellipse.hxx>
#include <Geom_Geometry.hxx>
#include <Geom_Hyperbola.hxx>
#include <Geom_Line.hxx>
#include <Geom_OffsetCurve.hxx>
#include <Geom_OffsetSurface.hxx>
#include <Geom_Parabola.hxx>
#include <Geom_Plane.hxx>
#include <Geom_Point.hxx>
#include <Geom_RectangularTrimmedSurface.hxx>
#include <Geom_SphericalSurface.hxx>
#include <Geom_Surface.hxx>
#include <Geom_SurfaceOfLinearExtrusion.hxx>
#include <Geom_SurfaceOfRevolution.hxx>
#include <Geom_SweptSurface.hxx>
#include <Geom_ToroidalSurface.hxx>
#include <Geom_Transformation.hxx>
#include <Geom_TrimmedCurve.hxx>
#include <Geom_Vector.hxx>
#include <Geom_VectorWithMagnitude.hxx>
#include <GeomEval_RepCurveDesc.hxx>
#include <GeomEval_RepSurfaceDesc.hxx>
#include <GeomAbs_BSplKnotDistribution.hxx>
#include <GeomAbs_Shape.hxx>
#include <BSplCLib.hxx>
#include <BSplSLib.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <gp.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Elips.hxx>
#include <gp_GTrsf2d.hxx>
#include <gp_Hypr.hxx>
#include <gp_Lin.hxx>
#include <gp_Parab.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

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
  if (std::getenv("GEOM_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
  buf = name; cur_name = name; cur_schema = schema; ntok = 0; outphase = false; got_words.clear();
}
static void bar() {
  if (got_words != split_words(cur_schema)) {
    std::fprintf(stderr, "schema mismatch in %s: printed words differ from '%s'\n", cur_name.c_str(), cur_schema.c_str());
    std::abort();
  }
  buf += " |"; mark = buf.size(); outphase = true;
}
static size_t g_maxlen = 1200;
static void finish() {
  if (buf.size() > g_maxlen) { buf.clear(); outphase = false; fam_dropped[cur_name + "!big"]++; return; }
  if (!seen.insert(buf).second) { buf.clear(); outphase = false; return; } // exact duplicate: dropped
  if (std::find(fam_order.begin(), fam_order.end(), cur_name) == fam_order.end()) fam_order.push_back(cur_name);
  fam_lines[cur_name]++;
  buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear(); outphase = false;
}
static void unchecked() { buf.resize(mark); buf += " unchecked"; }

// ---------------------------------------------------------------------------------
// The C library's transcendental functions, interposed so that a case can be re-run with every result moved by
// 0 or 1 ulp (a different libm gives such differences, and tests/geom.rs compares the families that use them within a
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
    if (poison == 0 && std::getenv("GEOM_WHY")) std::fprintf(stderr, "WHY %s: %s %s\n", cur_name.c_str(), typeid(e).name(), e.what());
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
  if (!ok && std::getenv("GEOM_DEBUG")) std::fprintf(stderr, "CRASH %s\n", buf.c_str());
  return ok;
}
static bool g_plan_failed;
static void on_alarm(int) { if (std::getenv("GEOM_DROPLOG")) { const char* m = "HANG "; (void)!write(2, m, 5); (void)!write(2, buf.c_str(), std::min<size_t>(buf.size(), 1500)); (void)!write(2, "\n", 1); } _exit(5); }
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
  if (std::getenv("GEOM_CHECK_EXACT")) libm_robust = true;
  if (g_plan_failed) { g_plan_failed = false; fam_dropped[cur_name]++; fam_dropped[cur_name + "!plancrash"]++; if (std::getenv("GEOM_DROPLOG")) std::fprintf(stderr, "DROP %s plancrash %s\n", cur_name.c_str(), buf.substr(0, 160).c_str()); buf.clear(); return false; }
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
      signal(SIGALRM, on_alarm); alarm(8);
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
      fam_dropped[cur_name]++; fam_dropped[cur_name + "!crash"]++; if (std::getenv("GEOM_DROPLOG")) std::fprintf(stderr, "DROP %s crash %s\n", cur_name.c_str(), buf.substr(0, 160).c_str()); buf.clear(); return false;
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
    if (std::getenv("GEOM_DROPLOG")) std::fprintf(stderr, "DROP %s st%d %s\n", cur_name.c_str(), st, buf.substr(0, 160).c_str());
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
static int g_cap = 16;
static size_t g_maxlen_default = 1500;
#define LOOP(NAME, TARGET) const char* N = NAME; g_maxlen = g_maxlen_default; for (int att = 0; fam_wanted(N) && att < 6000 && fam_lines[N] < std::min((int)(TARGET), g_cap); ++att)

// =================================================================================
// small helpers
static double nice() { return 0.5 * rint(-8, 8); }
static double crd() { return coin(0.4) ? nice() : rnd(-6, 6); }
static double nz(double lo, double hi) { return rnz(lo, hi); }
// a nonzero vector: small integers, Pythagorean triples or random
static void rdir3(double& x, double& y, double& z) {
  double r = rnd(0, 1);
  if (r < 0.3) { do { x = rint(-2, 2); y = rint(-2, 2); z = rint(-2, 2); } while (x == 0 && y == 0 && z == 0); }
  else if (r < 0.4) {
    static const double t[][3] = {{1, 2, 2}, {2, 3, 6}, {3, 4, 0}, {0, 5, 12}, {2, 6, 9}};
    const double* q = t[rint(0, 4)];
    x = q[0]; y = q[1]; z = q[2];
    if (coin(0.5)) std::swap(x, y);
    if (coin(0.5)) std::swap(y, z);
    if (coin(0.5)) x = -x;
    if (coin(0.5)) z = -z;
  } else { do { x = rnd(-4, 4); y = rnd(-4, 4); z = rnd(-4, 4); } while (x * x + y * y + z * z < 0.25); }
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
// converted with lround on both sides). The codes are listed in docs/api-geom.md.
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
static void o_p(const gp_Pnt& p) { h(p.X()); h(p.Y()); h(p.Z()); }
static void o_v(const gp_Vec& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void o_d(const gp_Dir& d) { h(d.X()); h(d.Y()); h(d.Z()); }
static void o_ax1(const gp_Ax1& a) { o_p(a.Location()); o_d(a.Direction()); }
static void o_ax2(const gp_Ax2& a) { o_p(a.Location()); o_d(a.Direction()); o_d(a.XDirection()); o_d(a.YDirection()); }
static void o_ax3(const gp_Ax3& a) { o_p(a.Location()); o_d(a.Direction()); o_d(a.XDirection()); o_d(a.YDirection()); ob(a.Direct()); }
static void o_fv(const Vd& v) { in((long)v.size()); for (double x : v) h(x); }

// ---------------------------------------------------------------------------------
// argument access for operations
static double A(const Vd& a, size_t i) { return i < a.size() ? a[i] : 0.0; }
static int AI(const Vd& a, size_t i) { return ai(A(a, i)); }
static gp_Pnt A_p(const Vd& a, size_t i) { return gp_Pnt(A(a, i), A(a, i + 1), A(a, i + 2)); }
static gp_Vec A_v(const Vd& a, size_t i) { return gp_Vec(A(a, i), A(a, i + 1), A(a, i + 2)); }
static gp_Dir A_d(const Vd& a, size_t i) { return gp_Dir(A(a, i), A(a, i + 1), A(a, i + 2)); }
// an axis (x y z dx dy dz) and a plane axis (x y z nx ny nz xx xy xz)
static gp_Ax1 A_ax1(const Vd& a, size_t i) { return gp_Ax1(A_p(a, i), A_d(a, i + 3)); }
static gp_Ax2 A_ax2(const Vd& a, size_t i) { return gp_Ax2(A_p(a, i), A_d(a, i + 3), A_d(a, i + 6)); }
// a frame (x y z nx ny nz xx xy xz flip)
static gp_Ax3 A_ax3(const Vd& a, size_t i) {
  gp_Ax3 r(A_p(a, i), A_d(a, i + 3), A_d(a, i + 6));
  if (AI(a, i + 9) != 0) r.YReverse();
  return r;
}

// ---------------------------------------------------------------------------------
// transformation recipes (word tr = kind + the parameters of the kind)
//   0 identity / default   1 mirror point (x y z)   2 mirror axis (o d)   3 mirror plane (o n)   4 rotation (o d angle)
//   5 scale (x y z s)   6 SetTransformation(to) (frame: o n x flip)   7 SetTransformation(from, to) (two frames)
//   8 translation vector (x y z)   9 translation points (x1 y1 z1 x2 y2 z2)   10 SetValues (12 values, row-major a11 a12 a13 a14 a21 ...)
//   11 SetDisplacement(from, to) (two frames)
static const int TRN[] = {0, 3, 6, 6, 7, 4, 10, 20, 3, 6, 12, 20};
struct Tr { int kind; double p[20]; };
static Tr tr_make(const Vd& a, size_t at) {
  Tr t; t.kind = a.size() > at ? ai(a[at]) : 0;
  if (t.kind < 0 || t.kind > 11) t.kind = 0;
  for (int i = 0; i < 20; ++i) t.p[i] = a.size() > at + 1 + (size_t)i && i < TRN[t.kind] ? a[at + 1 + (size_t)i] : 0.0;
  return t;
}
static size_t tr_len(const Tr& t) { return 1 + (size_t)TRN[t.kind]; }
static void tr_args(const Tr& t, Vd& a) { a.push_back(t.kind); for (int i = 0; i < TRN[t.kind]; ++i) a.push_back(t.p[i]); }
static void p_tr(const Tr& t) { in(t.kind); for (int i = 0; i < TRN[t.kind]; ++i) h(t.p[i]); }
static void W_tr(const Tr& t) { int s = ntok; p_tr(t); word("tr", s, 1 + TRN[t.kind]); }
static gp_Ax3 tr_ax3(const Tr& t, int at) {
  Vd a(t.p + at, t.p + at + 10);
  return A_ax3(a, 0);
}
static double det3(const double* p) {
  return p[0] * (p[5] * p[10] - p[6] * p[9]) - p[1] * (p[4] * p[10] - p[6] * p[8]) + p[2] * (p[4] * p[9] - p[5] * p[8]);
}
// kind 10 with a degenerate matrix and kind 5 with a null scale: the checks of gp_Trsf are compiled out of the libraries
static bool tr_unchecked(const Tr& t) {
  if (t.kind == 10) {
    double m[12] = {t.p[0], t.p[1], t.p[2], t.p[3], t.p[4], t.p[5], t.p[6], t.p[7], t.p[8], t.p[9], t.p[10], t.p[11]};
    return std::fabs(det3(m)) < gp::Resolution();
  }
  if (t.kind == 5) return std::fabs(t.p[3]) <= gp::Resolution();
  return false;
}
static gp_Trsf tr_gp(const Tr& t) {
  gp_Trsf T; const double* p = t.p; Vd a(p, p + 20);
  switch (t.kind) {
    case 1: T.SetMirror(A_p(a, 0)); break;
    case 2: T.SetMirror(A_ax1(a, 0)); break;
    case 3: T.SetMirror(gp_Ax2(A_p(a, 0), A_d(a, 3))); break;
    case 4: T.SetRotation(A_ax1(a, 0), p[6]); break;
    case 5: T.SetScale(A_p(a, 0), p[3]); break;
    case 6: T.SetTransformation(tr_ax3(t, 0)); break;
    case 7: T.SetTransformation(tr_ax3(t, 0), tr_ax3(t, 10)); break;
    case 8: T.SetTranslation(A_v(a, 0)); break;
    case 9: T.SetTranslation(A_p(a, 0), A_p(a, 3)); break;
    case 10: T.SetValues(p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], p[10], p[11]); break;
    case 11: T.SetDisplacement(tr_ax3(t, 0), tr_ax3(t, 10)); break;
    default: break;
  }
  return T;
}
// the same recipe applied with the Set* methods of Geom_Transformation
static void tr_set(Geom_Transformation& g, const Tr& t) {
  const double* p = t.p; Vd a(p, p + 20);
  switch (t.kind) {
    case 1: g.SetMirror(A_p(a, 0)); break;
    case 2: g.SetMirror(A_ax1(a, 0)); break;
    case 3: g.SetMirror(gp_Ax2(A_p(a, 0), A_d(a, 3))); break;
    case 4: g.SetRotation(A_ax1(a, 0), p[6]); break;
    case 5: g.SetScale(A_p(a, 0), p[3]); break;
    case 6: g.SetTransformation(tr_ax3(t, 0)); break;
    case 7: g.SetTransformation(tr_ax3(t, 0), tr_ax3(t, 10)); break;
    case 8: g.SetTranslation(A_v(a, 0)); break;
    case 9: g.SetTranslation(A_p(a, 0), A_p(a, 3)); break;
    case 10: case 11: g.SetTrsf(tr_gp(t)); break;
    default: g.SetTrsf(gp_Trsf()); break;
  }
}
// a frame with a random (not always orthogonal) x direction: o n x flip
static void rframe_vals(double* f, bool allow_flip = true) {
  f[0] = crd(); f[1] = crd(); f[2] = crd();
  if (coin(0.35)) {
    static const double ax[][6] = {{0, 0, 1, 1, 0, 0}, {0, 0, 1, 0, 1, 0}, {1, 0, 0, 0, 1, 0}, {0, 1, 0, 0, 0, 1}, {0, 0, -1, 1, 0, 0}, {0, -1, 0, 1, 0, 0}, {1, 0, 0, 0, 0, 1}};
    const double* q = ax[rint(0, 6)];
    for (int i = 0; i < 6; ++i) f[3 + i] = q[i];
  } else {
    double nx, ny, nz_, xx, xy, xz;
    rdir3(nx, ny, nz_);
    for (;;) {
      rdir3(xx, xy, xz);
      double cx = ny * xz - nz_ * xy, cy = nz_ * xx - nx * xz, cz = nx * xy - ny * xx;
      if (cx * cx + cy * cy + cz * cz > 0.1 * (nx * nx + ny * ny + nz_ * nz_) * (xx * xx + xy * xy + xz * xz)) break;
    }
    f[3] = nx; f[4] = ny; f[5] = nz_; f[6] = xx; f[7] = xy; f[8] = xz;
  }
  f[9] = allow_flip && coin(0.2) ? 1.0 : 0.0;
}
// a random recipe; `rot` allows the kind that uses sin / cos; `zero_scale` allows scale 0 (only the transformation families)
static Tr rtr(bool rot, bool zero_scale = false, bool bad_values = false, int force = -1) {
  Tr t; t.kind = 0; for (int i = 0; i < 20; ++i) t.p[i] = 0;
  if (zero_scale && force < 0 && coin(0.3)) force = coin(0.7) ? 5 : 10;
  for (;;) {
    double r = rnd(0, 1);
    int k = r < 0.03 ? 0 : r < 0.10 ? 1 : r < 0.19 ? 2 : r < 0.29 ? 3 : r < 0.42 ? 4 : r < 0.52 ? 5 : r < 0.58 ? 6 : r < 0.64 ? 7 : r < 0.74 ? 8 : r < 0.82 ? 9 : r < 0.92 ? 10 : 11;
    if (force >= 0) k = force;
    if (k == 4 && !rot) continue;
    t.kind = k;
    break;
  }
  double* p = t.p;
  switch (t.kind) {
    case 1: p[0] = crd(); p[1] = crd(); p[2] = crd(); break;
    case 2: p[0] = crd(); p[1] = crd(); p[2] = crd(); rdir3(p[3], p[4], p[5]); break;
    case 3: p[0] = crd(); p[1] = crd(); p[2] = crd(); rdir3(p[3], p[4], p[5]); break;
    case 4: p[0] = crd(); p[1] = crd(); p[2] = crd(); rdir3(p[3], p[4], p[5]); p[6] = rang(); break;
    case 5: p[0] = crd(); p[1] = crd(); p[2] = crd();
      p[3] = (zero_scale && coin(0.6)) ? 0.0 : coin(0.1) ? 1.0 : ((coin(0.35) ? -1 : 1) * (coin(0.5) ? 0.25 * rint(1, 16) : rnd(0.1, 4))); break;
    case 6: rframe_vals(p); break;
    case 7: case 11: rframe_vals(p); rframe_vals(p + 10); break;
    case 8: p[0] = crd(); p[1] = crd(); p[2] = crd(); break;
    case 9: for (int i = 0; i < 6; ++i) p[i] = crd(); break;
    case 10: {
      // s * (a signed permutation, sometimes composed with an exact Pythagorean rotation) and a translation; sometimes not a similarity
      int perm[3] = {0, 1, 2};
      for (int i = 2; i > 0; --i) std::swap(perm[i], perm[rint(0, i)]);
      double m[3][3] = {{0, 0, 0}, {0, 0, 0}, {0, 0, 0}};
      for (int c = 0; c < 3; ++c) m[perm[c]][c] = coin(0.5) ? 1 : -1;
      if (coin(0.35)) {
        static const double cs[][2] = {{0.6, 0.8}, {0.8, 0.6}, {-0.6, 0.8}, {0.28, 0.96}};
        const double* q = cs[rint(0, 3)];
        int ax_ = rint(0, 2), i1 = (ax_ + 1) % 3, i2 = (ax_ + 2) % 3;
        for (int c = 0; c < 3; ++c) { double u = m[i1][c], w = m[i2][c]; m[i1][c] = q[0] * u - q[1] * w; m[i2][c] = q[1] * u + q[0] * w; }
      }
      double s = (zero_scale && coin(0.6)) ? 0.0 : (coin(0.5) ? 0.5 * rint(1, 8) : rnd(0.2, 3));
      for (int r = 0; r < 3; ++r) for (int c = 0; c < 3; ++c) p[r * 4 + c] = s * m[r][c];
      p[3] = crd(); p[7] = crd(); p[11] = crd();
      if (bad_values && coin(0.4)) { p[0] *= 1.5; p[1] += 0.25; }
      break;
    }
    default: break;
  }
  return t;
}

// geometry operation codes (Geom_Geometry): 1 state, 4 Transform, 21 copy then Transform the original, 22 .. 35
// 22 Mirror(P) 23 Mirror(Ax1) 24 Mirror(Ax2: o n x) 25 Rotate(Ax1, a) 26 Scale(P, s) 27 Translate(V) 28 Translate(P1, P2)
// 29 Mirrored(P) 30 Mirrored(Ax1) 31 Mirrored(Ax2) 32 Rotated 33 Scaled 34 Translated(V) 35 Translated(P1, P2)
// 36 Transformed(T) 37 replace the object by its Copy()
typedef occ::handle<Geom_Geometry> HG;

// =================================================================================
// curve recipes (word crv); the grammar is in docs/api-geom.md
//   kind 0 line  1 circle  2 ellipse  3 hyperbola  4 parabola:  kind ctor v0..v8 [r | a b | f]
//       line: ctor (0 Ax1, 1 gp_Lin, 2 point + direction) then o d (6 floats);  circle, ellipse, hyperbola: ctor (0 gp_ object, 1 Ax2 form)
//       then the frame o n x (9 floats: Ax2(P, N, Vx)) then r | a b;  parabola: ctor (0 gp_Parab, 1 (Ax2, f), 2 (directrix Ax1, focus))
//       then o n x f (ctor 0, 1) or o d focus (ctor 2, 9 floats)
//   kind 5 bezier:   n  n*(x y z)  rat  [nw  nw*w]
//   kind 6 bspline:  deg periodic  nk  nk*knot  nm  nm*mult  np  np*(x y z)  rat  [nw  nw*w]
//   kind 7 trimmed:  u1 u2 sense adjust  crv        kind 8 offset:  offset notcheck dx dy dz  crv
struct CV {
  int kind = 0, ctor = 0;
  double v[9] = {0, 0, 0, 0, 0, 1, 1, 0, 0};
  double a = 0, b = 0;
  int deg = 1, per = 0, rat = 0;
  Vd knots; std::vector<int> mults; Vd px, py, pz; Vd w;
  double u1 = 0, u2 = 1; int sense = 1, adj = 1; double off = 0; int notc = 0; double od[3] = {0, 0, 1};
  std::shared_ptr<CV> basis;
};
static void p_pts3(const CV& c) { in((long)c.px.size()); for (size_t i = 0; i < c.px.size(); ++i) { h(c.px[i]); h(c.py[i]); h(c.pz[i]); } }
static void p_crv(const CV& c) {
  in(c.kind);
  if (c.kind <= 4) {
    in(c.ctor);
    if (c.kind == 0) { for (int i = 0; i < 6; ++i) h(c.v[i]); }
    else {
      for (int i = 0; i < 9; ++i) h(c.v[i]);
      if (c.kind == 1 || (c.kind == 4 && c.ctor != 2)) h(c.a);
      if (c.kind == 2 || c.kind == 3) { h(c.a); h(c.b); }
    }
  } else if (c.kind == 5) {
    p_pts3(c);
    in(c.rat); if (c.rat) { in((long)c.w.size()); for (double x : c.w) h(x); }
  } else if (c.kind == 6) {
    in(c.deg); in(c.per); in((long)c.knots.size());
    for (double x : c.knots) h(x);
    in((long)c.mults.size()); for (int m : c.mults) in(m);
    p_pts3(c);
    in(c.rat); if (c.rat) { in((long)c.w.size()); for (double x : c.w) h(x); }
  } else if (c.kind == 7) {
    h(c.u1); h(c.u2); in(c.sense); in(c.adj); p_crv(*c.basis);
  } else {
    h(c.off); in(c.notc); h(c.od[0]); h(c.od[1]); h(c.od[2]); p_crv(*c.basis);
  }
}
static void W_crv(const CV& c) { int s = ntok; p_crv(c); word("crv", s, -1); }

typedef occ::handle<Geom_Curve> HC;
typedef occ::handle<Geom_Surface> HS;
static gp_Ax2 cv_ax2(const CV& c) { Vd a(c.v, c.v + 9); return A_ax2(a, 0); }
static HC build(const CV& c);
static HC build_conic(const CV& c) {
  Vd a(c.v, c.v + 9);
  switch (c.kind) {
    case 0:
      if (c.ctor == 0) return new Geom_Line(A_ax1(a, 0));
      if (c.ctor == 1) return new Geom_Line(gp_Lin(A_ax1(a, 0)));
      return new Geom_Line(A_p(a, 0), A_d(a, 3));
    case 1:
      if (c.ctor == 0) return new Geom_Circle(gp_Circ(cv_ax2(c), c.a));
      return new Geom_Circle(cv_ax2(c), c.a);
    case 2:
      if (c.ctor == 0) return new Geom_Ellipse(gp_Elips(cv_ax2(c), c.a, c.b));
      return new Geom_Ellipse(cv_ax2(c), c.a, c.b);
    case 3:
      if (c.ctor == 0) return new Geom_Hyperbola(gp_Hypr(cv_ax2(c), c.a, c.b));
      return new Geom_Hyperbola(cv_ax2(c), c.a, c.b);
    default:
      if (c.ctor == 0) return new Geom_Parabola(gp_Parab(cv_ax2(c), c.a));
      if (c.ctor == 1) return new Geom_Parabola(cv_ax2(c), c.a);
      return new Geom_Parabola(A_ax1(a, 0), A_p(a, 6));
  }
}
static HC build(const CV& c) {
  if (c.kind <= 4) return build_conic(c);
  if (c.kind == 5) {
    NCollection_Array1<gp_Pnt> P(1, (int)c.px.size());
    for (size_t i = 0; i < c.px.size(); ++i) P((int)i + 1) = gp_Pnt(c.px[i], c.py[i], c.pz[i]);
    if (!c.rat) return new Geom_BezierCurve(P);
    NCollection_Array1<double> W(1, (int)c.w.size());
    for (size_t i = 0; i < c.w.size(); ++i) W((int)i + 1) = c.w[i];
    return new Geom_BezierCurve(P, W);
  }
  if (c.kind == 6) {
    NCollection_Array1<gp_Pnt> P(1, (int)c.px.size());
    for (size_t i = 0; i < c.px.size(); ++i) P((int)i + 1) = gp_Pnt(c.px[i], c.py[i], c.pz[i]);
    NCollection_Array1<double> K(1, (int)c.knots.size());
    for (size_t i = 0; i < c.knots.size(); ++i) K((int)i + 1) = c.knots[i];
    NCollection_Array1<int> M(1, (int)c.mults.size());
    for (size_t i = 0; i < c.mults.size(); ++i) M((int)i + 1) = c.mults[i];
    if (!c.rat) return new Geom_BSplineCurve(P, K, M, c.deg, c.per != 0);
    NCollection_Array1<double> W(1, (int)c.w.size());
    for (size_t i = 0; i < c.w.size(); ++i) W((int)i + 1) = c.w[i];
    return new Geom_BSplineCurve(P, W, K, M, c.deg, c.per != 0);
  }
  HC b = build(*c.basis);
  if (c.kind == 7) return new Geom_TrimmedCurve(b, c.u1, c.u2, c.sense != 0, c.adj != 0);
  return new Geom_OffsetCurve(b, c.off, gp_Dir(c.od[0], c.od[1], c.od[2]), c.notc != 0);
}

// =================================================================================
// surface recipes (word srf)
//   kind 0 plane: ctor (0 Ax3, 1 gp_Pln, 2 point + normal, 3 coefficients) then frame (10) | frame (10) | o n (6) | A B C D
//   kind 1 cylinder: ctor (0 (Ax3, r), 1 gp_Cylinder) frame r        kind 2 cone: ctor (0 (Ax3, angle, r), 1 gp_Cone) frame angle r
//   kind 3 sphere: ctor (0, 1) frame r                               kind 4 torus: ctor (0, 1) frame R r
//   kind 5 bezier:  nu nv  nu*nv*(x y z)  rat  [nw  nw*w]            (poles row by row: u index major)
//   kind 6 bspline: udeg vdeg uper vper  nku  knots  nmu  mults  nkv  knots  nmv  mults  nu nv  nu*nv*(x y z)  rat  [nw  nw*w]
//   kind 7 linear extrusion: dx dy dz  crv           kind 8 revolution: o d (6)  crv
//   kind 9 rectangular trimmed: ctor (0: u1 u2 v1 v2 usense vsense, 1: p1 p2 utrim sense)  srf
//   kind 10 offset: offset notcheck  srf
struct SV {
  int kind = 0, ctor = 0;
  double fr[10] = {0, 0, 0, 0, 0, 1, 1, 0, 0, 0};
  double a = 0, b = 0, c = 0, d = 0;
  int nu = 0, nv = 0, udeg = 1, vdeg = 1, uper = 0, vper = 0, rat = 0;
  Vd uk, vk; std::vector<int> um, vm; Vd px, py, pz; Vd w;
  double dir[6] = {0, 0, 1, 0, 0, 1};
  double t[4] = {0, 1, 0, 1}; int s1 = 1, s2 = 1, utrim = 1;
  double off = 0; int notc = 0;
  std::shared_ptr<CV> curve; std::shared_ptr<SV> basis;
};
static void p_srf(const SV& s);
static void W_srf(const SV& s) { int st = ntok; p_srf(s); word("srf", st, -1); }
static void p_srf(const SV& s) {
  in(s.kind);
  switch (s.kind) {
    case 0:
      in(s.ctor);
      if (s.ctor <= 1) { for (int i = 0; i < 10; ++i) h(s.fr[i]); }
      else if (s.ctor == 2) { for (int i = 0; i < 6; ++i) h(s.fr[i]); }
      else { h(s.a); h(s.b); h(s.c); h(s.d); }
      break;
    case 1: in(s.ctor); for (int i = 0; i < 10; ++i) h(s.fr[i]); h(s.a); break;
    case 2: in(s.ctor); for (int i = 0; i < 10; ++i) h(s.fr[i]); h(s.a); h(s.b); break;
    case 3: in(s.ctor); for (int i = 0; i < 10; ++i) h(s.fr[i]); h(s.a); break;
    case 4: in(s.ctor); for (int i = 0; i < 10; ++i) h(s.fr[i]); h(s.a); h(s.b); break;
    case 5:
      in(s.nu); in(s.nv);
      for (size_t i = 0; i < s.px.size(); ++i) { h(s.px[i]); h(s.py[i]); h(s.pz[i]); }
      in(s.rat); if (s.rat) { in((long)s.w.size()); for (double x : s.w) h(x); }
      break;
    case 6:
      in(s.udeg); in(s.vdeg); in(s.uper); in(s.vper);
      in((long)s.uk.size()); for (double x : s.uk) h(x);
      in((long)s.um.size()); for (int m : s.um) in(m);
      in((long)s.vk.size()); for (double x : s.vk) h(x);
      in((long)s.vm.size()); for (int m : s.vm) in(m);
      in(s.nu); in(s.nv);
      for (size_t i = 0; i < s.px.size(); ++i) { h(s.px[i]); h(s.py[i]); h(s.pz[i]); }
      in(s.rat); if (s.rat) { in((long)s.w.size()); for (double x : s.w) h(x); }
      break;
    case 7: h(s.dir[0]); h(s.dir[1]); h(s.dir[2]); p_crv(*s.curve); break;
    case 8: for (int i = 0; i < 6; ++i) h(s.dir[i]); p_crv(*s.curve); break;
    case 9:
      in(s.ctor);
      if (s.ctor == 0) { h(s.t[0]); h(s.t[1]); h(s.t[2]); h(s.t[3]); in(s.s1); in(s.s2); }
      else { h(s.t[0]); h(s.t[1]); in(s.utrim); in(s.s1); }
      p_srf(*s.basis);
      break;
    default: h(s.off); in(s.notc); p_srf(*s.basis); break;
  }
}
static gp_Ax3 sv_ax3(const SV& s) { Vd a(s.fr, s.fr + 10); return A_ax3(a, 0); }
static HS build_s(const SV& s) {
  switch (s.kind) {
    case 0:
      if (s.ctor == 0) return new Geom_Plane(sv_ax3(s));
      if (s.ctor == 1) return new Geom_Plane(gp_Pln(sv_ax3(s)));
      if (s.ctor == 2) return new Geom_Plane(gp_Pnt(s.fr[0], s.fr[1], s.fr[2]), gp_Dir(s.fr[3], s.fr[4], s.fr[5]));
      return new Geom_Plane(s.a, s.b, s.c, s.d);
    case 1: if (s.ctor == 0) return new Geom_CylindricalSurface(sv_ax3(s), s.a); return new Geom_CylindricalSurface(gp_Cylinder(sv_ax3(s), s.a));
    case 2: if (s.ctor == 0) return new Geom_ConicalSurface(sv_ax3(s), s.a, s.b); return new Geom_ConicalSurface(gp_Cone(sv_ax3(s), s.a, s.b));
    case 3: if (s.ctor == 0) return new Geom_SphericalSurface(sv_ax3(s), s.a); return new Geom_SphericalSurface(gp_Sphere(sv_ax3(s), s.a));
    case 4: if (s.ctor == 0) return new Geom_ToroidalSurface(sv_ax3(s), s.a, s.b); return new Geom_ToroidalSurface(gp_Torus(sv_ax3(s), s.a, s.b));
    case 5: case 6: {
      NCollection_Array2<gp_Pnt> P(1, std::max(s.nu, 0), 1, std::max(s.nv, 0));
      for (int i = 0; i < s.nu; ++i) for (int j = 0; j < s.nv; ++j) {
        size_t k = (size_t)(i * s.nv + j);
        P(i + 1, j + 1) = gp_Pnt(s.px[k], s.py[k], s.pz[k]);
      }
      NCollection_Array2<double> W(1, std::max(s.nu, 0), 1, std::max(s.nv, 0));
      bool wfit = s.rat && (int)s.w.size() == s.nu * s.nv;
      if (s.rat) {
        // a weight count that does not fit the poles is an invalid variant: the array keeps its size and the missing weights are 0
        for (int i = 0; i < s.nu; ++i) for (int j = 0; j < s.nv; ++j) { size_t k = (size_t)(i * s.nv + j); W(i + 1, j + 1) = k < s.w.size() ? s.w[k] : 0.0; }
      }
      (void)wfit;
      if (s.kind == 5) {
        if (!s.rat) return new Geom_BezierSurface(P);
        return new Geom_BezierSurface(P, W);
      }
      NCollection_Array1<double> UK(1, (int)s.uk.size()), VK(1, (int)s.vk.size());
      for (size_t i = 0; i < s.uk.size(); ++i) UK((int)i + 1) = s.uk[i];
      for (size_t i = 0; i < s.vk.size(); ++i) VK((int)i + 1) = s.vk[i];
      NCollection_Array1<int> UM(1, (int)s.um.size()), VM(1, (int)s.vm.size());
      for (size_t i = 0; i < s.um.size(); ++i) UM((int)i + 1) = s.um[i];
      for (size_t i = 0; i < s.vm.size(); ++i) VM((int)i + 1) = s.vm[i];
      if (!s.rat) return new Geom_BSplineSurface(P, UK, VK, UM, VM, s.udeg, s.vdeg, s.uper != 0, s.vper != 0);
      return new Geom_BSplineSurface(P, W, UK, VK, UM, VM, s.udeg, s.vdeg, s.uper != 0, s.vper != 0);
    }
    case 7: return new Geom_SurfaceOfLinearExtrusion(build(*s.curve), gp_Dir(s.dir[0], s.dir[1], s.dir[2]));
    case 8: return new Geom_SurfaceOfRevolution(build(*s.curve), gp_Ax1(gp_Pnt(s.dir[0], s.dir[1], s.dir[2]), gp_Dir(s.dir[3], s.dir[4], s.dir[5])));
    case 9: {
      HS b = build_s(*s.basis);
      if (s.ctor == 0) return new Geom_RectangularTrimmedSurface(b, s.t[0], s.t[1], s.t[2], s.t[3], s.s1 != 0, s.s2 != 0);
      return new Geom_RectangularTrimmedSurface(b, s.t[0], s.t[1], s.utrim != 0, s.s1 != 0);
    }
    default: return new Geom_OffsetSurface(build_s(*s.basis), s.off, s.notc != 0);
  }
}

// ---------------------------------------------------------------------------------
// evaluation representations of curves and surfaces
static GeomEval_RepCurveDesc::Base* erep_of(Geom_Geometry* p) {
  if (auto* b = dynamic_cast<Geom_BezierCurve*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom_BSplineCurve*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom_OffsetCurve*>(p)) return b->EvalRepresentation().get();
  return nullptr;
}
static GeomEval_RepSurfaceDesc::Base* erep_of_s(Geom_Geometry* p) {
  if (auto* b = dynamic_cast<Geom_BezierSurface*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom_BSplineSurface*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom_SurfaceOfLinearExtrusion*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom_SurfaceOfRevolution*>(p)) return b->EvalRepresentation().get();
  if (auto* b = dynamic_cast<Geom_OffsetSurface*>(p)) return b->EvalRepresentation().get();
  return nullptr;
}

// ---------------------------------------------------------------------------------
// the state of a geometric object, printed as the oracle's `state` (type code first); the Rust test prints the same values
//   0 line: loc dir          1 circle: ax2 r          2 ellipse / 3 hyperbola: ax2 a b         4 parabola: ax2 f
//   5 bezier curve: n poles rational [weights(n) when rational] closed erep
//   6 bspline curve: deg periodic rational nk np knots mults poles [weights(np) when rational] closed distribution continuity erep
//   7 trimmed curve: first last periodic closed, then the state of the basis curve
//   8 offset curve: offset dir continuity basis-continuity erep, then the state of the basis curve
//   10 cartesian point: x y z      11 direction: x y z      12 vector with magnitude: x y z      13 axis 1 placement: loc dir
//   14 axis 2 placement: loc dir xdir ydir
//   20 plane: ax3        21 cylinder: ax3 r        22 cone: ax3 semi-angle ref-radius        23 sphere: ax3 r        24 torus: ax3 R r
//   25 extrusion: dir erep, then the basis curve        26 revolution: loc axis-dir erep, then the basis curve
//   27 bezier surface: nu nv poles(row by row) urat vrat weights uclosed vclosed erep
//   28 bspline surface: udeg vdeg uper vper urat vrat nku nkv nu nv uknots vknots umults vmults poles weights uclosed vclosed udist vdist cont erep
//   29 rectangular trimmed surface: u1 u2 v1 v2, then the basis surface        30 offset surface: offset basis-continuity erep, then the basis surface
static void o_state(const HG& g);
static void o_pol(const gp_Pnt& p) { o_p(p); }
static void o_bez(const Geom_BezierCurve* b) {
  in(5); in(b->NbPoles());
  for (int i = 1; i <= b->NbPoles(); ++i) o_pol(b->Pole(i));
  ob(b->IsRational());
  if (b->IsRational()) for (int i = 1; i <= b->NbPoles(); ++i) h(b->Weight(i));
  ob(b->IsClosed()); ob(b->HasEvalRepresentation());
}
static void o_bsp(const Geom_BSplineCurve* b) {
  in(6); in(b->Degree()); ob(b->IsPeriodic()); ob(b->IsRational()); in(b->NbKnots()); in(b->NbPoles());
  for (int i = 1; i <= b->NbKnots(); ++i) h(b->Knot(i));
  for (int i = 1; i <= b->NbKnots(); ++i) in(b->Multiplicity(i));
  for (int i = 1; i <= b->NbPoles(); ++i) o_pol(b->Pole(i));
  if (b->IsRational()) for (int i = 1; i <= b->NbPoles(); ++i) h(b->Weight(i));
  ob(b->IsClosed()); in((int)b->KnotDistribution()); in((int)b->Continuity()); ob(b->HasEvalRepresentation());
}
static void o_bezs(const Geom_BezierSurface* b) {
  in(27); in(b->NbUPoles()); in(b->NbVPoles());
  for (int i = 1; i <= b->NbUPoles(); ++i) for (int j = 1; j <= b->NbVPoles(); ++j) o_pol(b->Pole(i, j));
  ob(b->IsURational()); ob(b->IsVRational());
  if (b->IsURational() || b->IsVRational()) for (int i = 1; i <= b->NbUPoles(); ++i) for (int j = 1; j <= b->NbVPoles(); ++j) h(b->Weight(i, j));
  ob(b->IsUClosed()); ob(b->IsVClosed()); ob(b->HasEvalRepresentation());
}
static void o_bsps(const Geom_BSplineSurface* b) {
  in(28); in(b->UDegree()); in(b->VDegree()); ob(b->IsUPeriodic()); ob(b->IsVPeriodic()); ob(b->IsURational()); ob(b->IsVRational());
  in(b->NbUKnots()); in(b->NbVKnots()); in(b->NbUPoles()); in(b->NbVPoles());
  for (int i = 1; i <= b->NbUKnots(); ++i) h(b->UKnot(i));
  for (int i = 1; i <= b->NbVKnots(); ++i) h(b->VKnot(i));
  for (int i = 1; i <= b->NbUKnots(); ++i) in(b->UMultiplicity(i));
  for (int i = 1; i <= b->NbVKnots(); ++i) in(b->VMultiplicity(i));
  for (int i = 1; i <= b->NbUPoles(); ++i) for (int j = 1; j <= b->NbVPoles(); ++j) o_pol(b->Pole(i, j));
  if (b->IsURational() || b->IsVRational()) for (int i = 1; i <= b->NbUPoles(); ++i) for (int j = 1; j <= b->NbVPoles(); ++j) h(b->Weight(i, j));
  ob(b->IsUClosed()); ob(b->IsVClosed()); in((int)b->UKnotDistribution()); in((int)b->VKnotDistribution()); in((int)b->Continuity());
  ob(b->HasEvalRepresentation());
}
static void o_state(const HG& g) {
  Geom_Geometry* p = g.get();
  if (auto* x = dynamic_cast<Geom_Line*>(p)) { in(0); o_ax1(x->Position()); }
  else if (auto* x = dynamic_cast<Geom_Circle*>(p)) { in(1); o_ax2(x->Position()); h(x->Radius()); }
  else if (auto* x = dynamic_cast<Geom_Ellipse*>(p)) { in(2); o_ax2(x->Position()); h(x->MajorRadius()); h(x->MinorRadius()); }
  else if (auto* x = dynamic_cast<Geom_Hyperbola*>(p)) { in(3); o_ax2(x->Position()); h(x->MajorRadius()); h(x->MinorRadius()); }
  else if (auto* x = dynamic_cast<Geom_Parabola*>(p)) { in(4); o_ax2(x->Position()); h(x->Focal()); }
  else if (auto* x = dynamic_cast<Geom_BezierCurve*>(p)) o_bez(x);
  else if (auto* x = dynamic_cast<Geom_BSplineCurve*>(p)) o_bsp(x);
  else if (auto* x = dynamic_cast<Geom_TrimmedCurve*>(p)) {
    in(7); h(x->FirstParameter()); h(x->LastParameter()); ob(x->IsPeriodic()); ob(x->IsClosed());
    o_state(x->BasisCurve());
  } else if (auto* x = dynamic_cast<Geom_OffsetCurve*>(p)) {
    in(8); h(x->Offset()); o_d(x->Direction()); in((int)x->Continuity()); in((int)x->GetBasisCurveContinuity()); ob(x->HasEvalRepresentation());
    o_state(x->BasisCurve());
  } else if (auto* x = dynamic_cast<Geom_CartesianPoint*>(p)) { in(10); h(x->X()); h(x->Y()); h(x->Z()); }
  else if (auto* x = dynamic_cast<Geom_Direction*>(p)) { in(11); h(x->X()); h(x->Y()); h(x->Z()); }
  else if (auto* x = dynamic_cast<Geom_VectorWithMagnitude*>(p)) { in(12); h(x->X()); h(x->Y()); h(x->Z()); }
  else if (auto* x = dynamic_cast<Geom_Axis1Placement*>(p)) { in(13); o_p(x->Location()); o_d(x->Direction()); }
  else if (auto* x = dynamic_cast<Geom_Axis2Placement*>(p)) { in(14); o_ax2(x->Ax2()); }
  else if (auto* x = dynamic_cast<Geom_Plane*>(p)) { in(20); o_ax3(x->Position()); }
  else if (auto* x = dynamic_cast<Geom_CylindricalSurface*>(p)) { in(21); o_ax3(x->Position()); h(x->Radius()); }
  else if (auto* x = dynamic_cast<Geom_ConicalSurface*>(p)) { in(22); o_ax3(x->Position()); h(x->SemiAngle()); h(x->RefRadius()); }
  else if (auto* x = dynamic_cast<Geom_SphericalSurface*>(p)) { in(23); o_ax3(x->Position()); h(x->Radius()); }
  else if (auto* x = dynamic_cast<Geom_ToroidalSurface*>(p)) { in(24); o_ax3(x->Position()); h(x->MajorRadius()); h(x->MinorRadius()); }
  else if (auto* x = dynamic_cast<Geom_SurfaceOfLinearExtrusion*>(p)) {
    in(25); o_d(x->Direction()); ob(x->HasEvalRepresentation()); o_state(x->BasisCurve());
  } else if (auto* x = dynamic_cast<Geom_SurfaceOfRevolution*>(p)) {
    in(26); o_p(x->Location()); o_d(x->Direction()); ob(x->HasEvalRepresentation()); o_state(x->BasisCurve());
  } else if (auto* x = dynamic_cast<Geom_BezierSurface*>(p)) o_bezs(x);
  else if (auto* x = dynamic_cast<Geom_BSplineSurface*>(p)) o_bsps(x);
  else if (auto* x = dynamic_cast<Geom_RectangularTrimmedSurface*>(p)) {
    double u1, u2, v1, v2; x->Bounds(u1, u2, v1, v2);
    in(29); h(u1); h(u2); h(v1); h(v2); o_state(x->BasisSurface());
  } else if (auto* x = dynamic_cast<Geom_OffsetSurface*>(p)) {
    in(30); h(x->Offset()); in((int)x->GetBasisSurfContinuity()); ob(x->HasEvalRepresentation()); o_state(x->BasisSurface());
  } else { if (std::getenv("GEOM_DROPLOG")) std::fprintf(stderr, "o_state: unknown geometry %s\n", typeid(*p).name()); throw Discard(); }
}

// =================================================================================
// operations on geometric objects
#define AS(T, v) T* v = dynamic_cast<T*>(g.get())

// the operations every geometric object has (Geom_Geometry): codes 1, 4, 21 .. 37
static bool geom_common(HG& g, const Op& op) {
  const Vd& a = op.a;
  switch (op.code) {
    case 1: o_state(g); return true;
    case 4: g->Transform(tr_gp(tr_make(a, 0))); return true;
    case 21: { gp_Trsf T = tr_gp(tr_make(a, 0)); HG cp = g->Copy(); g->Transform(T); o_state(cp); return true; }
    case 22: g->Mirror(A_p(a, 0)); return true;
    case 23: g->Mirror(A_ax1(a, 0)); return true;
    case 24: g->Mirror(gp_Ax2(A_p(a, 0), A_d(a, 3))); return true;
    case 25: g->Rotate(A_ax1(a, 0), A(a, 6)); return true;
    case 26: g->Scale(A_p(a, 0), A(a, 3)); return true;
    case 27: g->Translate(A_v(a, 0)); return true;
    case 28: g->Translate(A_p(a, 0), A_p(a, 3)); return true;
    case 29: o_state(g->Mirrored(A_p(a, 0))); return true;
    case 30: o_state(g->Mirrored(A_ax1(a, 0))); return true;
    case 31: o_state(g->Mirrored(gp_Ax2(A_p(a, 0), A_d(a, 3)))); return true;
    case 32: o_state(g->Rotated(A_ax1(a, 0), A(a, 6))); return true;
    case 33: o_state(g->Scaled(A_p(a, 0), A(a, 3))); return true;
    case 34: o_state(g->Translated(A_v(a, 0))); return true;
    case 35: o_state(g->Translated(A_p(a, 0), A_p(a, 3))); return true;
    case 36: o_state(g->Transformed(tr_gp(tr_make(a, 0)))); return true;
    case 37: g = g->Copy(); return true;
    default: return false;
  }
}
// a recipe in the arguments of a geometry operation is unchecked when its matrix is degenerate or its scale null
static bool geom_unchecked(const Op& op) {
  if (op.code == 4 || op.code == 21 || op.code == 36) return tr_unchecked(tr_make(op.a, 0));
  return false;
}
// random geometry operations: 22 .. 36 with random arguments
static Op rgeom_op(bool rot) {
  int c = rint(0, 15);
  double dx, dy, dz;
  Vd a;
  switch (c) {
    case 0: return mk(22, Vd{crd(), crd(), crd()});
    case 1: rdir3(dx, dy, dz); return mk(23, Vd{crd(), crd(), crd(), dx, dy, dz});
    case 2: rdir3(dx, dy, dz); return mk(24, Vd{crd(), crd(), crd(), dx, dy, dz});
    case 3: if (!rot) return mk(27, Vd{crd(), crd(), crd()}); rdir3(dx, dy, dz); return mk(25, Vd{crd(), crd(), crd(), dx, dy, dz, rang()});
    case 4: return mk(26, Vd{crd(), crd(), crd(), nz(0.25, 3)});
    case 5: return mk(27, Vd{crd(), crd(), crd()});
    case 6: return mk(28, Vd{crd(), crd(), crd(), crd(), crd(), crd()});
    case 7: return mk(29, Vd{crd(), crd(), crd()});
    case 8: rdir3(dx, dy, dz); return mk(30, Vd{crd(), crd(), crd(), dx, dy, dz});
    case 9: rdir3(dx, dy, dz); return mk(31, Vd{crd(), crd(), crd(), dx, dy, dz});
    case 10: if (!rot) return mk(34, Vd{crd(), crd(), crd()}); rdir3(dx, dy, dz); return mk(32, Vd{crd(), crd(), crd(), dx, dy, dz, rang()});
    case 11: return mk(33, Vd{crd(), crd(), crd(), nz(0.25, 3)});
    case 12: return mk(34, Vd{crd(), crd(), crd()});
    case 13: return mk(35, Vd{crd(), crd(), crd(), crd(), crd(), crd()});
    case 14: tr_args(rtr(rot), a); return mk(coin(0.5) ? 36 : 4, a);
    default: tr_args(rtr(rot), a); return mk(21, a);
  }
}

// ---------------------------------------------------------------------------------
// points
enum { P_SET_COORD = 40, P_SET_PNT, P_SET_X, P_SET_Y, P_SET_Z, P_COORD, P_PNT, P_X, P_Y, P_Z, P_DIST, P_SQDIST };
static HG mk_cpoint(int ctor, double x, double y, double z) {
  if (ctor == 0) return new Geom_CartesianPoint(gp_Pnt(x, y, z));
  return new Geom_CartesianPoint(x, y, z);
}
static bool point_op(HG& g, const Op& op) {
  if (geom_common(g, op)) return true;
  AS(Geom_CartesianPoint, p);
  const Vd& a = op.a;
  switch (op.code) {
    case P_SET_COORD: p->SetCoord(A(a, 0), A(a, 1), A(a, 2)); return true;
    case P_SET_PNT: p->SetPnt(gp_Pnt(A(a, 0), A(a, 1), A(a, 2))); return true;
    case P_SET_X: p->SetX(A(a, 0)); return true;
    case P_SET_Y: p->SetY(A(a, 0)); return true;
    case P_SET_Z: p->SetZ(A(a, 0)); return true;
    case P_COORD: { double x, y, z; p->Coord(x, y, z); h(x); h(y); h(z); return true; }
    case P_PNT: o_p(p->Pnt()); return true;
    case P_X: h(p->X()); return true;
    case P_Y: h(p->Y()); return true;
    case P_Z: h(p->Z()); return true;
    case P_DIST: { occ::handle<Geom_Point> q = occ::down_cast<Geom_Point>(mk_cpoint(AI(a, 3), A(a, 0), A(a, 1), A(a, 2))); h(p->Distance(q)); return true; }
    case P_SQDIST: { occ::handle<Geom_Point> q = occ::down_cast<Geom_Point>(mk_cpoint(AI(a, 3), A(a, 0), A(a, 1), A(a, 2))); h(p->SquareDistance(q)); return true; }
    default: std::fprintf(stderr, "point_op: bad code %d\n", op.code); std::abort();
  }
}

// ---------------------------------------------------------------------------------
// vectors: the "other" operand is (kind, x, y, z) with kind 0 direction / 1 vector with magnitude
enum { V_SET_COORD = 50, V_SET_X, V_SET_Y, V_SET_Z, V_SET_VEC, V_REVERSE, V_REVERSED, V_ANGLE, V_ANGLE_REF, V_COORD, V_MAG, V_SQMAG, V_X, V_Y, V_Z,
       V_CROSS, V_CROSSED, V_CROSSCROSS, V_CROSSCROSSED, V_DOT, V_DOTCROSS, V_VEC, V_DIR,
       V_ADD = 80, V_ADDED, V_SUBTRACT, V_SUBTRACTED, V_MULTIPLY, V_MULTIPLIED, V_DIVIDE, V_DIVIDED, V_NORMALIZE, V_NORMALIZED };
static occ::handle<Geom_Vector> mk_other(const Vd& a, size_t i) {
  if (AI(a, i) == 0) return new Geom_Direction(A(a, i + 1), A(a, i + 2), A(a, i + 3));
  return new Geom_VectorWithMagnitude(A(a, i + 1), A(a, i + 2), A(a, i + 3));
}
static gp_Vec other_vec(const Vd& a, size_t i) {
  if (AI(a, i) == 0) { gp_Dir d(A(a, i + 1), A(a, i + 2), A(a, i + 3)); return gp_Vec(d); }
  return gp_Vec(A(a, i + 1), A(a, i + 2), A(a, i + 3));
}
static bool vec_unchecked(HG& g, const Op& op) {
  if (geom_unchecked(op)) return true;
  const Vd& a = op.a;
  Geom_Vector* v = dynamic_cast<Geom_Vector*>(g.get());
  bool isdir = dynamic_cast<Geom_Direction*>(g.get()) != nullptr;
  auto zero3 = [](double x, double y, double z) { return std::sqrt(x * x + y * y + z * z) <= gp::Resolution(); };
  auto small = [](const gp_Vec& u) { return u.Magnitude() <= gp::Resolution(); };
  // a Geom_Direction built from a gp_Vec (the constructor of gp_Dir checks that the vector is not null)
  switch (op.code) {
    case V_SET_COORD: if (isdir) return zero3(A(a, 0), A(a, 1), A(a, 2)); break;
    case V_SET_X: if (isdir) return zero3(A(a, 0), v->Y(), v->Z()); break;
    case V_SET_Y: if (isdir) return zero3(v->X(), A(a, 0), v->Z()); break;
    case V_SET_Z: if (isdir) return zero3(v->X(), v->Y(), A(a, 0)); break;
    case V_ANGLE: return small(v->Vec()) || small(other_vec(a, 0));
    case V_ANGLE_REF: return small(v->Vec()) || small(other_vec(a, 0)) || small(other_vec(a, 4));
    case V_CROSS: case V_CROSSED: if (isdir) return small(v->Vec().Crossed(other_vec(a, 0))); break;
    case V_CROSSCROSS: case V_CROSSCROSSED: if (isdir) return small(v->Vec().CrossCrossed(other_vec(a, 0), other_vec(a, 4))); break;
    case V_DIVIDE: case V_DIVIDED: return std::fabs(A(a, 0)) <= gp::Resolution();
    case V_NORMALIZE: case V_NORMALIZED: return small(v->Vec());
    default: break;
  }
  return false;
}
static HG mk_vec(int kind, int ctor, double x, double y, double z, double x2, double y2, double z2) {
  if (kind == 0) {
    if (ctor == 0) return new Geom_Direction(x, y, z);
    return new Geom_Direction(gp_Dir(x, y, z));
  }
  if (ctor == 0) return new Geom_VectorWithMagnitude(gp_Vec(x, y, z));
  if (ctor == 1) return new Geom_VectorWithMagnitude(x, y, z);
  return new Geom_VectorWithMagnitude(gp_Pnt(x, y, z), gp_Pnt(x2, y2, z2));
}
static bool vec_op(HG& g, const Op& op) {
  if (geom_common(g, op)) return true;
  AS(Geom_Vector, v);
  Geom_Direction* d = dynamic_cast<Geom_Direction*>(g.get());
  Geom_VectorWithMagnitude* m = dynamic_cast<Geom_VectorWithMagnitude*>(g.get());
  const Vd& a = op.a;
  switch (op.code) {
    case V_SET_COORD: if (d) d->SetCoord(A(a, 0), A(a, 1), A(a, 2)); else m->SetCoord(A(a, 0), A(a, 1), A(a, 2)); return true;
    case V_SET_X: if (d) d->SetX(A(a, 0)); else m->SetX(A(a, 0)); return true;
    case V_SET_Y: if (d) d->SetY(A(a, 0)); else m->SetY(A(a, 0)); return true;
    case V_SET_Z: if (d) d->SetZ(A(a, 0)); else m->SetZ(A(a, 0)); return true;
    case V_SET_VEC: if (d) d->SetDir(gp_Dir(A(a, 0), A(a, 1), A(a, 2))); else m->SetVec(gp_Vec(A(a, 0), A(a, 1), A(a, 2))); return true;
    case V_REVERSE: v->Reverse(); return true;
    case V_REVERSED: o_state(v->Reversed()); return true;
    case V_ANGLE: h(v->Angle(mk_other(a, 0))); return true;
    case V_ANGLE_REF: h(v->AngleWithRef(mk_other(a, 0), mk_other(a, 4))); return true;
    case V_COORD: { double x, y, z; v->Coord(x, y, z); h(x); h(y); h(z); return true; }
    case V_MAG: h(v->Magnitude()); return true;
    case V_SQMAG: h(v->SquareMagnitude()); return true;
    case V_X: h(v->X()); return true;
    case V_Y: h(v->Y()); return true;
    case V_Z: h(v->Z()); return true;
    case V_CROSS: v->Cross(mk_other(a, 0)); return true;
    case V_CROSSED: o_state(v->Crossed(mk_other(a, 0))); return true;
    case V_CROSSCROSS: v->CrossCross(mk_other(a, 0), mk_other(a, 4)); return true;
    case V_CROSSCROSSED: o_state(v->CrossCrossed(mk_other(a, 0), mk_other(a, 4))); return true;
    case V_DOT: h(v->Dot(mk_other(a, 0))); return true;
    case V_DOTCROSS: h(v->DotCross(mk_other(a, 0), mk_other(a, 4))); return true;
    case V_VEC: o_v(v->Vec()); return true;
    case V_DIR: o_d(d->Dir()); return true;
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

// ---------------------------------------------------------------------------------
// axis placements: Geom_Axis1Placement (spec: ctor ox oy oz dx dy dz) and Geom_Axis2Placement (ctor ox oy oz nx ny nz xx xy xz)
enum { X_REVERSE = 110, X_REVERSED, X_SET_AXIS, X_SET_DIR, X_SET_LOC, X_ANGLE, X_AXIS, X_DIRECTION, X_LOCATION, X_AX1,
       X2_SET_AX2 = 125, X2_SET_XDIR, X2_SET_YDIR, X2_AX2, X2_XDIR, X2_YDIR };
static HG mk_axis1(int ctor, const double* v) {
  if (ctor == 0) return new Geom_Axis1Placement(gp_Ax1(gp_Pnt(v[0], v[1], v[2]), gp_Dir(v[3], v[4], v[5])));
  return new Geom_Axis1Placement(gp_Pnt(v[0], v[1], v[2]), gp_Dir(v[3], v[4], v[5]));
}
static HG mk_axis2(int ctor, const double* v) {
  if (ctor == 0) return new Geom_Axis2Placement(gp_Ax2(gp_Pnt(v[0], v[1], v[2]), gp_Dir(v[3], v[4], v[5]), gp_Dir(v[6], v[7], v[8])));
  return new Geom_Axis2Placement(gp_Pnt(v[0], v[1], v[2]), gp_Dir(v[3], v[4], v[5]), gp_Dir(v[6], v[7], v[8]));
}
static bool axis_op(HG& g, const Op& op) {
  if (geom_common(g, op)) return true;
  AS(Geom_AxisPlacement, x);
  Geom_Axis1Placement* x1 = dynamic_cast<Geom_Axis1Placement*>(g.get());
  Geom_Axis2Placement* x2 = dynamic_cast<Geom_Axis2Placement*>(g.get());
  const Vd& a = op.a;
  switch (op.code) {
    case X_REVERSE: x1->Reverse(); return true;
    case X_REVERSED: o_state(x1->Reversed()); return true;
    case X_SET_AXIS: x->SetAxis(A_ax1(a, 0)); return true;
    case X_SET_DIR: x->SetDirection(A_d(a, 0)); return true;
    case X_SET_LOC: x->SetLocation(A_p(a, 0)); return true;
    case X_ANGLE: { occ::handle<Geom_AxisPlacement> o = new Geom_Axis1Placement(A_ax1(a, 0)); h(x->Angle(o)); return true; }
    case X_AXIS: o_ax1(x->Axis()); return true;
    case X_DIRECTION: o_d(x->Direction()); return true;
    case X_LOCATION: o_p(x->Location()); return true;
    case X_AX1: o_ax1(x1->Ax1()); return true;
    case X2_SET_AX2: x2->SetAx2(A_ax2(a, 0)); return true;
    case X2_SET_XDIR: x2->SetXDirection(A_d(a, 0)); return true;
    case X2_SET_YDIR: x2->SetYDirection(A_d(a, 0)); return true;
    case X2_AX2: o_ax2(x2->Ax2()); return true;
    case X2_XDIR: o_d(x2->XDirection()); return true;
    case X2_YDIR: o_d(x2->YDirection()); return true;
    default: std::fprintf(stderr, "axis_op: bad code %d\n", op.code); std::abort();
  }
}

// ---------------------------------------------------------------------------------
// Geom_Transformation: the object is a handle; codes 1.. (own numbering)
enum { T_STATE = 1, T_SET, T_INVERT, T_INVERTED, T_MULTIPLY, T_MULTIPLIED, T_PREMULTIPLY, T_POWER, T_POWERED, T_TRANSFORMS, T_VALUE, T_COPY_SWAP, T_FROM_TRSF, T_SET_TRSF };
typedef occ::handle<Geom_Transformation> HT;
static void o_trsf(const HT& t) {
  in((int)t->Form()); ob(t->IsNegative()); h(t->ScaleFactor());
  for (int r = 1; r <= 3; ++r) for (int c = 1; c <= 4; ++c) h(t->Value(r, c));
}
static HT mk_tr_obj(const Tr& t) { HT g = new Geom_Transformation(); tr_set(*g, t); return g; }
// the check of gp_Trsf::Invert is compiled out for these forms
static bool trsf_inv_unchecked(const HT& t) {
  gp_TrsfForm f = t->Form();
  return f != gp_Identity && f != gp_Translation && f != gp_PntMirror && std::fabs(t->ScaleFactor()) <= gp::Resolution();
}
static bool trsf_unchecked(const HT& t, const Op& op) {
  const Vd& a = op.a;
  switch (op.code) {
    case T_SET: case T_FROM_TRSF: case T_SET_TRSF: return tr_unchecked(tr_make(a, 0));
    case T_VALUE: return AI(a, 0) < 1 || AI(a, 0) > 3 || AI(a, 1) < 1 || AI(a, 1) > 4;
    case T_INVERT: case T_INVERTED: return trsf_inv_unchecked(t);
    case T_POWER: case T_POWERED: return AI(a, 0) < 0 && trsf_inv_unchecked(t);
    case T_MULTIPLY: case T_MULTIPLIED: case T_PREMULTIPLY: return tr_unchecked(tr_make(a, 0));
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
    case T_TRANSFORMS: { double x = A(a, 0), y = A(a, 1), z = A(a, 2); t->Transforms(x, y, z); h(x); h(y); h(z); return true; }
    case T_VALUE: h(t->Value(AI(a, 0), AI(a, 1))); return true;
    case T_COPY_SWAP: t = t->Copy(); return true;
    case T_FROM_TRSF: t = new Geom_Transformation(tr_gp(tr_make(a, 0))); return true;
    case T_SET_TRSF: t->SetTrsf(tr_gp(tr_make(a, 0))); return true;
    default: std::fprintf(stderr, "trsf_op: bad code %d\n", op.code); std::abort();
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
      if (std::getenv("GEOM_PLANLOG")) { std::fprintf(stderr, "PLAN op %d:", op.code); for (double v : op.a) std::fprintf(stderr, " %.17g", v); std::fprintf(stderr, "\n"); }
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

// =================================================================================
// families: points, vectors, directions, axes, transformations
static void fam_point() {
  { LOOP("cpoint_ops", 24) {
      int ctor = rint(0, 1); double x = crd(), y = crd(), z = crd();
      HG g0 = mk_cpoint(ctor, x, y, z);
      Ops ops = plan(g0, 4, [&](const HG&, Ops& add) {
        int r = rint(0, 11);
        int codes[] = {P_SET_COORD, P_SET_PNT, P_SET_X, P_SET_Y, P_SET_Z, P_COORD, P_PNT, P_X, P_Y, P_Z, P_DIST, P_SQDIST};
        add.push_back(mk(codes[r], r <= 1 ? Vd{crd(), crd(), crd()} : r <= 4 ? Vd{crd()} : r >= 10 ? Vd{crd(), crd(), crd(), (double)rint(0, 1)} : Vd{}));
        if (r <= 4) add.push_back(mk(1));
      }, [&](HG& g, const Op& o) { return point_op(g, o); }, [&](HG&, const Op&) { return false; });
      begin(N, "i f f f ops"); W_i(ctor); W_f(x); W_f(y); W_f(z); W_ops(ops); bar();
      emit([&] { HG g = mk_cpoint(ctor, x, y, z); for (const Op& o : ops) point_op(g, o); }, false);
    } }
  { LOOP("cpoint_xform", 24) {
      int ctor = rint(0, 1); double x = crd(), y = crd(), z = crd();
      Ops ops;
      for (int k = rint(2, 5); k > 0; --k) ops.push_back(rgeom_op(true));
      ops.push_back(mk(1));
      begin(N, "i f f f ops"); W_i(ctor); W_f(x); W_f(y); W_f(z); W_ops(ops); bar();
      emit([&] { HG g = mk_cpoint(ctor, x, y, z); for (const Op& o : ops) { if (geom_unchecked(o)) { unchecked(); return; } point_op(g, o); } }, true);
    } }
}
// an "other" operand: (kind, x, y, z); `par` makes it (anti)parallel to `cur` sometimes
static void rvec_other(Vd& a, const gp_Vec* cur = nullptr, bool dir_only = false) {
  int k = dir_only ? 0 : rint(0, 1);
  double x, y, z; rdir3(x, y, z);
  if (k == 1 && coin(0.5)) { x = crd(); y = crd(); z = crd(); }
  if (k == 1 && coin(0.07)) { x = 0; y = 0; z = 0; }
  if (cur && coin(0.12)) { double s = k == 0 ? (coin(0.5) ? 1 : -1) : nz(0.5, 2); x = s * cur->X(); y = s * cur->Y(); z = s * cur->Z(); if (k == 0 && x == 0 && y == 0 && z == 0) x = 1; }
  a.push_back(k); a.push_back(x); a.push_back(y); a.push_back(z);
}
static void fam_vec() {
  struct Spec { int kind; int ctor; double v[6]; };
  auto mkspec = [&](int kind) {
    Spec s; s.kind = kind; s.ctor = kind == 0 ? rint(0, 1) : rint(0, 2);
    for (int i = 0; i < 6; ++i) s.v[i] = 0;
    if (kind == 0) rdir3(s.v[0], s.v[1], s.v[2]);
    else { for (int i = 0; i < 6; ++i) s.v[i] = coin(0.3) ? nice() : crd(); }
    return s;
  };
  auto runv = [&](const Spec& s, const Ops& ops) {
    HG g = mk_vec(s.kind, s.ctor, s.v[0], s.v[1], s.v[2], s.v[3], s.v[4], s.v[5]);
    for (const Op& o : ops) { if (vec_unchecked(g, o)) { unchecked(); return; } vec_op(g, o); }
  };
  auto line = [&](const char* N, const Spec& s, const Ops& ops, bool libm) {
    begin(N, "i i f f f f f f ops"); W_i(s.kind); W_i(s.ctor); for (int i = 0; i < 6; ++i) W_f(s.v[i]); W_ops(ops); bar();
    emit([&] { runv(s, ops); }, libm);
  };
  auto pl = [&](const Spec& s, int rounds, const std::function<void(const HG&, Ops&)>& ch) {
    HG g0 = mk_vec(s.kind, s.ctor, s.v[0], s.v[1], s.v[2], s.v[3], s.v[4], s.v[5]);
    return plan(g0, rounds, ch, [&](HG& g, const Op& o) { return vec_op(g, o); }, [&](HG& g, const Op& o) { return vec_unchecked(g, o); });
  };
  auto curvec = [](const HG& g) { return dynamic_cast<Geom_Vector*>(g.get())->Vec(); };
  // direction: accessors, setters, relations with another vector
  { LOOP("direction_ops", 28) {
      Spec s = mkspec(0);
      Ops ops = pl(s, 4, [&](const HG& g, Ops& add) {
        int r = rint(0, 12); Vd a;
        switch (r) {
          case 0: a = {nz(0.1, 3), nz(0.1, 3), nz(0.1, 3)}; if (coin(0.3)) a = {0, 0, 0}; add.push_back(mk(V_SET_COORD, a)); add.push_back(mk(1)); break;
          case 1: a = {nz(0.1, 3)}; if (coin(0.2)) a = {0.0}; add.push_back(mk(V_SET_X, a)); add.push_back(mk(1)); break;
          case 2: a = {nz(0.1, 3)}; if (coin(0.2)) a = {0.0}; add.push_back(mk(V_SET_Y, a)); add.push_back(mk(1)); break;
          case 3: a = {nz(0.1, 3)}; if (coin(0.2)) a = {0.0}; add.push_back(mk(V_SET_Z, a)); add.push_back(mk(1)); break;
          case 4: { double x, y, z; rdir3(x, y, z); add.push_back(mk(V_SET_VEC, Vd{x, y, z})); add.push_back(mk(1)); break; }
          case 5: add.push_back(mk(V_REVERSE)); add.push_back(mk(1)); break;
          case 6: add.push_back(mk(V_REVERSED)); break;
          case 7: { gp_Vec c = curvec(g); rvec_other(a, &c); add.push_back(mk(V_ANGLE, a)); break; }
          case 8: add.push_back(mk(V_COORD)); add.push_back(mk(V_X)); add.push_back(mk(V_Y)); add.push_back(mk(V_Z)); break;
          case 9: add.push_back(mk(V_MAG)); add.push_back(mk(V_SQMAG)); add.push_back(mk(V_DIR)); add.push_back(mk(V_VEC)); break;
          case 10: { gp_Vec c = curvec(g); rvec_other(a, &c); add.push_back(mk(V_DOT, a)); break; }
          default: { gp_Vec c = curvec(g); rvec_other(a, &c); add.push_back(mk(V_ANGLE, a)); add.push_back(mk(V_DOT, a)); break; }
        }
      });
      line(N, s, ops, true);
    } }
  { LOOP("direction_cross", 28) {
      Spec s = mkspec(0);
      Ops ops = pl(s, 3, [&](const HG& g, Ops& add) {
        int r = rint(0, 7); Vd a, b;
        gp_Vec c = curvec(g);
        rvec_other(a, &c); rvec_other(b, &c);
        a.insert(a.end(), b.begin(), b.end());
        switch (r) {
          case 0: add.push_back(mk(V_CROSS, Vd(a.begin(), a.begin() + 4))); add.push_back(mk(1)); break;
          case 1: add.push_back(mk(V_CROSSED, Vd(a.begin(), a.begin() + 4))); break;
          case 2: add.push_back(mk(V_CROSSCROSS, a)); add.push_back(mk(1)); break;
          case 3: add.push_back(mk(V_CROSSCROSSED, a)); break;
          case 4: add.push_back(mk(V_DOTCROSS, a)); break;
          case 5: add.push_back(mk(V_ANGLE_REF, a)); break;
          case 6: add.push_back(mk(V_CROSS, Vd(a.begin(), a.begin() + 4))); add.push_back(mk(V_CROSSED, Vd(a.begin() + 4, a.begin() + 8))); break;
          default: add.push_back(mk(V_DOTCROSS, a)); add.push_back(mk(V_CROSSCROSSED, a)); break;
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
      Ops ops = pl(s, 4, [&](const HG& g, Ops& add) {
        int r = rint(0, 11); Vd a;
        switch (r) {
          case 0: add.push_back(mk(V_SET_COORD, Vd{crd(), crd(), crd()})); add.push_back(mk(1)); break;
          case 1: add.push_back(mk(V_SET_X, Vd{crd()})); add.push_back(mk(1)); break;
          case 2: add.push_back(mk(V_SET_Y, Vd{crd()})); add.push_back(mk(V_SET_Z, Vd{crd()})); add.push_back(mk(1)); break;
          case 3: add.push_back(mk(V_SET_VEC, Vd{crd(), crd(), crd()})); add.push_back(mk(1)); break;
          case 4: add.push_back(mk(V_REVERSE)); add.push_back(mk(1)); break;
          case 5: add.push_back(mk(V_REVERSED)); break;
          case 6: { gp_Vec c = curvec(g); rvec_other(a, &c); add.push_back(mk(V_ANGLE, a)); break; }
          case 7: add.push_back(mk(V_COORD)); add.push_back(mk(V_X)); add.push_back(mk(V_Y)); add.push_back(mk(V_Z)); break;
          case 8: add.push_back(mk(V_MAG)); add.push_back(mk(V_SQMAG)); break;
          case 9: { gp_Vec c = curvec(g); rvec_other(a, &c); add.push_back(mk(V_DOT, a)); break; }
          case 10: add.push_back(mk(V_VEC)); break;
          default: { gp_Vec c = curvec(g); rvec_other(a, &c); add.push_back(mk(V_ANGLE, a)); break; }
        }
      });
      line(N, s, ops, true);
    } }
  { LOOP("vecmag_arith", 28) {
      Spec s = mkspec(1);
      if (coin(0.25)) { for (int i = 0; i < 6; ++i) s.v[i] = 0.0; }
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
  { LOOP("vecmag_cross", 28) {
      Spec s = mkspec(1);
      if (coin(0.15)) { for (int i = 0; i < 6; ++i) s.v[i] = 0.0; }
      Ops ops = pl(s, 3, [&](const HG& g, Ops& add) {
        int r = rint(0, 7); Vd a, b;
        gp_Vec c = curvec(g);
        rvec_other(a, &c); rvec_other(b, &c);
        a.insert(a.end(), b.begin(), b.end());
        switch (r) {
          case 0: add.push_back(mk(V_CROSS, Vd(a.begin(), a.begin() + 4))); add.push_back(mk(1)); break;
          case 1: add.push_back(mk(V_CROSSED, Vd(a.begin(), a.begin() + 4))); break;
          case 2: add.push_back(mk(V_CROSSCROSS, a)); add.push_back(mk(1)); break;
          case 3: add.push_back(mk(V_CROSSCROSSED, a)); break;
          case 4: add.push_back(mk(V_DOTCROSS, a)); break;
          case 5: add.push_back(mk(V_ANGLE_REF, a)); break;
          case 6: add.push_back(mk(V_CROSS, Vd(a.begin(), a.begin() + 4))); add.push_back(mk(V_DOT, Vd(a.begin() + 4, a.begin() + 8))); break;
          default: add.push_back(mk(V_DOTCROSS, a)); add.push_back(mk(V_CROSSCROSSED, a)); break;
        }
      });
      line(N, s, ops, true);
    } }
  { LOOP("vecmag_xform", 24) {
      Spec s = mkspec(1); Ops ops;
      for (int k = rint(2, 5); k > 0; --k) ops.push_back(rgeom_op(true));
      ops.push_back(mk(1));
      line(N, s, ops, true);
    } }
}
static void fam_axis() {
  struct Spec { int ctor; double v[9]; };
  auto mk1 = [&]() { Spec s; s.ctor = rint(0, 1); double fr[10]; rframe_vals(fr, false); for (int i = 0; i < 6; ++i) s.v[i] = fr[i]; s.v[6] = s.v[7] = s.v[8] = 0; return s; };
  auto mk2 = [&]() { Spec s; s.ctor = rint(0, 1); double fr[10]; rframe_vals(fr, false); for (int i = 0; i < 9; ++i) s.v[i] = fr[i]; return s; };
  auto line1 = [&](const char* N, const Spec& s, const Ops& ops, bool libm) {
    begin(N, "i f f f f f f ops"); W_i(s.ctor); for (int i = 0; i < 6; ++i) W_f(s.v[i]); W_ops(ops); bar();
    emit([&] { HG g = mk_axis1(s.ctor, s.v); for (const Op& o : ops) { if (geom_unchecked(o)) { unchecked(); return; } axis_op(g, o); } }, libm);
  };
  auto line2 = [&](const char* N, const Spec& s, const Ops& ops, bool libm) {
    begin(N, "i f f f f f f f f f ops"); W_i(s.ctor); for (int i = 0; i < 9; ++i) W_f(s.v[i]); W_ops(ops); bar();
    emit([&] { HG g = mk_axis2(s.ctor, s.v); for (const Op& o : ops) { if (geom_unchecked(o)) { unchecked(); return; } axis_op(g, o); } }, libm);
  };
  { LOOP("axis1_ops", 24) {
      Spec s = mk1(); Ops ops;
      for (int k = rint(2, 4); k > 0; --k) {
        int r = rint(0, 12); double dx, dy, dz; rdir3(dx, dy, dz);
        if (r >= 10) { ops.push_back(rgeom_op(true)); ops.push_back(mk(1)); continue; }
        switch (r) {
          case 0: ops.push_back(mk(X_REVERSE)); ops.push_back(mk(1)); break;
          case 1: ops.push_back(mk(X_REVERSED)); break;
          case 2: ops.push_back(mk(X_SET_AXIS, Vd{crd(), crd(), crd(), dx, dy, dz})); ops.push_back(mk(1)); break;
          case 3: ops.push_back(mk(X_SET_DIR, Vd{dx, dy, dz})); ops.push_back(mk(1)); break;
          case 4: ops.push_back(mk(X_SET_LOC, Vd{crd(), crd(), crd()})); ops.push_back(mk(1)); break;
          case 5: ops.push_back(mk(X_ANGLE, Vd{crd(), crd(), crd(), dx, dy, dz})); break;
          case 6: ops.push_back(mk(X_AXIS)); break;
          case 7: ops.push_back(mk(X_DIRECTION)); break;
          case 8: ops.push_back(mk(X_AX1)); break;
          default: ops.push_back(mk(X_LOCATION)); break;
        }
      }
      line1(N, s, ops, true);
    } }
    { LOOP("axis2_ops", 28) {
      Spec s = mk2(); Ops ops;
      for (int k = rint(2, 4); k > 0; --k) {
        int r = rint(0, 15); double fr[10]; rframe_vals(fr, false); double dx, dy, dz; rdir3(dx, dy, dz);
        if (r >= 13) { ops.push_back(rgeom_op(true)); ops.push_back(mk(1)); continue; }
        switch (r) {
          case 0: ops.push_back(mk(X2_SET_AX2, Vd{fr[0], fr[1], fr[2], fr[3], fr[4], fr[5], fr[6], fr[7], fr[8]})); ops.push_back(mk(1)); break;
          case 1: ops.push_back(mk(X_SET_AXIS, Vd{crd(), crd(), crd(), dx, dy, dz})); ops.push_back(mk(1)); break;
          case 2: ops.push_back(mk(X_SET_DIR, Vd{dx, dy, dz})); ops.push_back(mk(1)); break;
          case 3: ops.push_back(mk(X_SET_LOC, Vd{crd(), crd(), crd()})); ops.push_back(mk(1)); break;
          case 4: ops.push_back(mk(X2_SET_XDIR, Vd{dx, dy, dz})); ops.push_back(mk(1)); break;
          case 5: ops.push_back(mk(X2_SET_YDIR, Vd{dx, dy, dz})); ops.push_back(mk(1)); break;
          case 6: ops.push_back(mk(X_ANGLE, Vd{crd(), crd(), crd(), dx, dy, dz})); break;
          case 7: ops.push_back(mk(X2_AX2)); break;
          case 8: ops.push_back(mk(X2_XDIR)); ops.push_back(mk(X2_YDIR)); break;
          case 9: ops.push_back(mk(X_AXIS)); ops.push_back(mk(X_DIRECTION)); break;
          case 10: ops.push_back(mk(X_LOCATION)); break;
          default: ops.push_back(mk(X2_AX2)); break;
        }
      }
      line2(N, s, ops, true);
    } }
  }
// Geom_Transformation
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
  auto pt_ops = [&](Ops& add, int n) { for (int i = 0; i < n; ++i) add.push_back(mk(T_TRANSFORMS, Vd{crd(), crd(), crd()})); };
  auto rt = [&](bool rot, bool zero = false, bool bad = false, int force = -1) { return rtr(rot, zero, bad, force); };
  { LOOP("trsf_set", 32) { g_maxlen = 2200;
      Tr t0 = rt(true, false, true);
      Ops ops = pl(t0, 3, [&](const HT&, Ops& add) {
        Vd a; tr_args(rt(true, false, true, coin(0.3) ? 10 : -1), a);
        int q = rint(0, 9);
        if (q == 0) { add.push_back(mk(T_COPY_SWAP)); add.push_back(mk(T_STATE)); return; }
        if (q == 1) { bool bad = coin(0.3); add.push_back(mk(T_VALUE, Vd{(double)(bad ? rint(0, 4) : rint(1, 3)), (double)(bad ? rint(0, 5) : rint(1, 4))})); return; }
        add.push_back(mk(coin(0.8) ? T_SET : (coin(0.5) ? T_SET_TRSF : T_FROM_TRSF), a)); add.push_back(mk(T_STATE)); pt_ops(add, 1);
        if (coin(0.3)) add.push_back(mk(T_VALUE, Vd{(double)rint(1, 3), (double)rint(1, 4)}));
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
  { LOOP("trsf_multiply", 36) { g_maxlen = 2600;
      Tr t0 = rt(false, false, true);
      Ops ops = pl(t0, 3, [&](const HT&, Ops& add) {
        Vd a; tr_args(rt(false, false, true), a);
        int r = rint(0, 2);
        add.push_back(mk(r == 0 ? T_MULTIPLY : r == 1 ? T_MULTIPLIED : T_PREMULTIPLY, a));
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
  }

// =================================================================================
// curves (the codes are listed in docs/api-geom.md)
enum {
  C_STATE = 1, C_REVERSE = 2, C_REVERSED_PARAM, C_TRANSFORM_UNUSED, C_TRANSFORMED_PARAM, C_PARAM_TRANSFORM, C_D0, C_D1, C_D2, C_D3, C_DN, C_VALUE,
  C_FIRST, C_LAST, C_IS_CLOSED, C_IS_PERIODIC, C_PERIOD, C_CONTINUITY, C_IS_CN, C_REVERSED,
  L_SET_LIN = 100, L_SET_DIR, L_SET_LOC, L_SET_POS, L_LIN, L_POSITION,
  K_SET_AXIS = 110, K_SET_LOC, K_SET_POS, K_AXIS, K_LOCATION, K_POSITION, K_XAXIS, K_YAXIS, K_ECC,
  CI_SET_RADIUS = 120, CI_SET_CIRC, CI_CIRC,
  E_SET_MAJOR = 123, E_SET_MINOR, E_SET_ELIPS, E_ELIPS,
  EH_DIRECTRIX1 = 127, EH_DIRECTRIX2, EH_FOCAL, EH_FOCUS1, EH_FOCUS2, EH_MAJOR, EH_MINOR, EH_PARAMETER,
  H_ASYMPTOTE1 = 135, H_ASYMPTOTE2, H_CONJ1, H_CONJ2, H_OTHER, H_SET_HYPR, H_HYPR,
  PA_SET_FOCAL = 142, PA_SET_PARAB, PA_PARAB, PA_DIRECTRIX, PA_FOCUS,
  B_START = 150, B_END,
  BB_INCREASE = 160, BB_INS_AFTER, BB_INS_BEFORE, BB_REMOVE_POLE, BB_SEGMENT, BB_SET_POLE, BB_SET_POLE_W, BB_SET_WEIGHT, BB_RESOLUTION, BB_POLE, BB_WEIGHT,
  BB_NBPOLES, BB_DEGREE, BB_IS_RATIONAL, BB_POLES, BB_WEIGHTS, BB_KNOTS, BB_MULTS, BB_KNOTSEQ, BB_MAXDEGREE,
  S_INC_MULT = 180, S_INC_MULT_RANGE, S_INCR_MULT, S_INS_KNOT, S_INS_KNOTS, S_REMOVE_KNOT, S_SET_KNOT, S_SET_KNOT_M, S_SET_KNOTS, S_PERIODIC_NORM,
  S_SET_PERIODIC, S_SET_ORIGIN, S_SET_NOT_PERIODIC, S_MOVE_POINT, S_MOVE_TANGENT, S_IS_G1, S_LOCATE, S_LOCAL_VALUE, S_LOCAL_D0, S_LOCAL_D1, S_LOCAL_D2,
  S_LOCAL_D3, S_LOCAL_DN, S_KNOT, S_MULT, S_DISTRIBUTION, S_FIRST_UKNOT, S_LAST_UKNOT, S_NBKNOTS, S_SET_ORIGIN_U, S_IS_EQUAL, BB_INS_AFTER_NW, BB_INS_BEFORE_NW,
  TR_SET_TRIM = 220, TR_BASIS,
  O_SET_BASIS = 230, O_SET_OFFSET, O_OFFSET, O_SET_DIR, O_BASIS_CONT, O_DIRECTION,
  R_SET_EREP = 240, R_CLEAR_EREP, R_HAS_EREP, R_EREP_KIND
};
static void o_arr_p(const NCollection_Array1<gp_Pnt>& v) { in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) o_p(v(i)); }
static void o_arr_f(const NCollection_Array1<double>& v) { in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) h(v(i)); }
static void o_arr_i(const NCollection_Array1<int>& v) { in(v.Length()); for (int i = v.Lower(); i <= v.Upper(); ++i) in(v(i)); }

// a Period() on a curve that is not periodic: the check is compiled out
static bool period_unchecked(const HC& c) {
  if (auto* t = dynamic_cast<Geom_TrimmedCurve*>(c.get())) return period_unchecked(t->BasisCurve());
  if (auto* o = dynamic_cast<Geom_OffsetCurve*>(c.get())) return period_unchecked(o->BasisCurve());
  return !c->IsPeriodic();
}
static bool bad_idx(int i, int n) { return i < 1 || i > n; }
static bool curve_unchecked(HG& g, const Op& op) {
  if (geom_unchecked(op)) return true;
  const Vd& a = op.a;
  Geom_Curve* c = dynamic_cast<Geom_Curve*>(g.get());
  Geom_BSplineCurve* bs = dynamic_cast<Geom_BSplineCurve*>(g.get());
  switch (op.code) {
    case C_TRANSFORMED_PARAM: case C_PARAM_TRANSFORM: return tr_unchecked(tr_make(a, op.code == C_TRANSFORMED_PARAM ? 1 : 0));
    case C_PERIOD: return period_unchecked(HC(c));
    case C_IS_CN:
      return AI(a, 0) < 0 && (bs || dynamic_cast<Geom_TrimmedCurve*>(g.get()) || dynamic_cast<Geom_OffsetCurve*>(g.get()));
    case K_ECC: case EH_PARAMETER:
      if (auto* hy = dynamic_cast<Geom_Hyperbola*>(g.get())) return hy->MajorRadius() <= gp::Resolution();
      return false;
    case EH_DIRECTRIX1: case EH_DIRECTRIX2:
      if (auto* el = dynamic_cast<Geom_Ellipse*>(g.get())) return gp_Elips(el->Position(), el->MajorRadius(), el->MinorRadius()).Eccentricity() <= gp::Resolution();
      if (auto* hy = dynamic_cast<Geom_Hyperbola*>(g.get())) return hy->MajorRadius() <= gp::Resolution();
      return false;
    case H_ASYMPTOTE1: case H_ASYMPTOTE2: return dynamic_cast<Geom_Hyperbola*>(g.get())->MajorRadius() <= gp::Resolution();
    case S_INC_MULT: case S_SET_KNOT_M: return op.code == S_INC_MULT && bad_idx(AI(a, 0), bs->NbKnots());
    case S_INC_MULT_RANGE: case S_INCR_MULT: return AI(a, 0) < 1 || AI(a, 1) > bs->NbKnots() || AI(a, 0) > AI(a, 1);
    case S_KNOT: case S_MULT: return bad_idx(AI(a, 0), bs->NbKnots());
    case BB_POLE: case BB_WEIGHT: return bs && bad_idx(AI(a, 0), bs->NbPoles());
    case S_LOCAL_VALUE: case S_LOCAL_D0: case S_LOCAL_D1: case S_LOCAL_D2: case S_LOCAL_D3: case S_LOCAL_DN: {
      int k1 = AI(a, 1), k2 = AI(a, 2);
      return k1 < 1 || k2 > bs->NbKnots() || k1 >= k2;
    }
    default: return false;
  }
}

static void o_hypr(const gp_Hypr& H) { h(H.MajorRadius()); h(H.MinorRadius()); o_ax2(H.Position()); }

static bool curve_op(HG& g, const Op& op, const CV* cv2) {
  if (geom_common(g, op)) return true;
  const Vd& a = op.a;
  Geom_Curve* c = dynamic_cast<Geom_Curve*>(g.get());
  Geom_Line* ln = dynamic_cast<Geom_Line*>(g.get());
  Geom_Conic* cn = dynamic_cast<Geom_Conic*>(g.get());
  Geom_Circle* ci = dynamic_cast<Geom_Circle*>(g.get());
  Geom_Ellipse* el = dynamic_cast<Geom_Ellipse*>(g.get());
  Geom_Hyperbola* hy = dynamic_cast<Geom_Hyperbola*>(g.get());
  Geom_Parabola* pa = dynamic_cast<Geom_Parabola*>(g.get());
  Geom_BezierCurve* bz = dynamic_cast<Geom_BezierCurve*>(g.get());
  Geom_BSplineCurve* bs = dynamic_cast<Geom_BSplineCurve*>(g.get());
  Geom_TrimmedCurve* tc = dynamic_cast<Geom_TrimmedCurve*>(g.get());
  Geom_OffsetCurve* oc = dynamic_cast<Geom_OffsetCurve*>(g.get());
  switch (op.code) {
    case C_REVERSE: c->Reverse(); return true;
    case C_REVERSED_PARAM: h(c->ReversedParameter(A(a, 0))); return true;
    case C_TRANSFORMED_PARAM: h(c->TransformedParameter(A(a, 0), tr_gp(tr_make(a, 1)))); return true;
    case C_PARAM_TRANSFORM: h(c->ParametricTransformation(tr_gp(tr_make(a, 0)))); return true;
    case C_D0: o_p(c->EvalD0(A(a, 0))); return true;
    case C_D1: { Geom_Curve::ResD1 r = c->EvalD1(A(a, 0)); o_p(r.Point); o_v(r.D1); return true; }
    case C_D2: { Geom_Curve::ResD2 r = c->EvalD2(A(a, 0)); o_p(r.Point); o_v(r.D1); o_v(r.D2); return true; }
    case C_D3: { Geom_Curve::ResD3 r = c->EvalD3(A(a, 0)); o_p(r.Point); o_v(r.D1); o_v(r.D2); o_v(r.D3); return true; }
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
    case L_SET_LIN: ln->SetLin(gp_Lin(A_ax1(a, 0))); return true;
    case L_SET_DIR: ln->SetDirection(A_d(a, 0)); return true;
    case L_SET_LOC: ln->SetLocation(A_p(a, 0)); return true;
    case L_SET_POS: ln->SetPosition(A_ax1(a, 0)); return true;
    case L_LIN: o_ax1(ln->Lin().Position()); return true;
    case L_POSITION: o_ax1(ln->Position()); return true;
    // conics
    case K_SET_AXIS: cn->SetAxis(A_ax1(a, 0)); return true;
    case K_SET_LOC: cn->SetLocation(A_p(a, 0)); return true;
    case K_SET_POS: cn->SetPosition(A_ax2(a, 0)); return true;
    case K_AXIS: o_ax1(cn->Axis()); return true;
    case K_LOCATION: o_p(cn->Location()); return true;
    case K_POSITION: o_ax2(cn->Position()); return true;
    case K_XAXIS: o_ax1(cn->XAxis()); return true;
    case K_YAXIS: o_ax1(cn->YAxis()); return true;
    case K_ECC: h(cn->Eccentricity()); return true;
    case CI_SET_RADIUS: ci->SetRadius(A(a, 0)); return true;
    case CI_SET_CIRC: ci->SetCirc(gp_Circ(A_ax2(a, 0), A(a, 9))); return true;
    case CI_CIRC: { gp_Circ C = ci->Circ(); h(C.Radius()); o_ax2(C.Position()); return true; }
    case E_SET_MAJOR: if (el) el->SetMajorRadius(A(a, 0)); else hy->SetMajorRadius(A(a, 0)); return true;
    case E_SET_MINOR: if (el) el->SetMinorRadius(A(a, 0)); else hy->SetMinorRadius(A(a, 0)); return true;
    case E_SET_ELIPS: el->SetElips(gp_Elips(A_ax2(a, 0), A(a, 9), A(a, 10))); return true;
    case E_ELIPS: { gp_Elips E = el->Elips(); h(E.MajorRadius()); h(E.MinorRadius()); o_ax2(E.Position()); return true; }
    case EH_DIRECTRIX1: o_ax1(el ? el->Directrix1() : hy->Directrix1()); return true;
    case EH_DIRECTRIX2: o_ax1(el ? el->Directrix2() : hy->Directrix2()); return true;
    case EH_FOCAL: h(el ? el->Focal() : (hy ? hy->Focal() : pa->Focal())); return true;
    case EH_FOCUS1: o_p(el ? el->Focus1() : hy->Focus1()); return true;
    case EH_FOCUS2: o_p(el ? el->Focus2() : hy->Focus2()); return true;
    case EH_MAJOR: h(el ? el->MajorRadius() : hy->MajorRadius()); return true;
    case EH_MINOR: h(el ? el->MinorRadius() : hy->MinorRadius()); return true;
    case EH_PARAMETER: h(el ? el->Parameter() : (hy ? hy->Parameter() : pa->Parameter())); return true;
    case H_ASYMPTOTE1: o_ax1(hy->Asymptote1()); return true;
    case H_ASYMPTOTE2: o_ax1(hy->Asymptote2()); return true;
    case H_CONJ1: o_hypr(hy->ConjugateBranch1()); return true;
    case H_CONJ2: o_hypr(hy->ConjugateBranch2()); return true;
    case H_OTHER: o_hypr(hy->OtherBranch()); return true;
    case H_SET_HYPR: hy->SetHypr(gp_Hypr(A_ax2(a, 0), A(a, 9), A(a, 10))); return true;
    case H_HYPR: o_hypr(hy->Hypr()); return true;
    case PA_SET_FOCAL: pa->SetFocal(A(a, 0)); return true;
    case PA_SET_PARAB: pa->SetParab(gp_Parab(A_ax2(a, 0), A(a, 9))); return true;
    case PA_PARAB: { gp_Parab P = pa->Parab(); h(P.Focal()); o_ax2(P.Position()); return true; }
    case PA_DIRECTRIX: o_ax1(pa->Directrix()); return true;
    case PA_FOCUS: o_p(pa->Focus()); return true;
    // bounded curves
    case B_START: { Geom_BoundedCurve* b = dynamic_cast<Geom_BoundedCurve*>(g.get()); o_p(b->StartPoint()); return true; }
    case B_END: { Geom_BoundedCurve* b = dynamic_cast<Geom_BoundedCurve*>(g.get()); o_p(b->EndPoint()); return true; }
    // Bezier and B-spline curves
    case BB_INCREASE: if (bz) bz->Increase(AI(a, 0)); else bs->IncreaseDegree(AI(a, 0)); return true;
    case BB_INS_AFTER: bz->InsertPoleAfter(AI(a, 0), A_p(a, 1), A(a, 4)); return true;
    case BB_INS_BEFORE: bz->InsertPoleBefore(AI(a, 0), A_p(a, 1), A(a, 4)); return true;
    case BB_INS_AFTER_NW: bz->InsertPoleAfter(AI(a, 0), A_p(a, 1)); return true;
    case BB_INS_BEFORE_NW: bz->InsertPoleBefore(AI(a, 0), A_p(a, 1)); return true;
    case BB_REMOVE_POLE: bz->RemovePole(AI(a, 0)); return true;
    case BB_SEGMENT: if (bz) bz->Segment(A(a, 0), A(a, 1)); else bs->Segment(A(a, 0), A(a, 1), A(a, 2)); return true;
    case BB_SET_POLE: if (bz) bz->SetPole(AI(a, 0), A_p(a, 1)); else bs->SetPole(AI(a, 0), A_p(a, 1)); return true;
    case BB_SET_POLE_W: if (bz) bz->SetPole(AI(a, 0), A_p(a, 1), A(a, 4)); else bs->SetPole(AI(a, 0), A_p(a, 1), A(a, 4)); return true;
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
    case BB_MAXDEGREE: in(bz ? Geom_BezierCurve::MaxDegree() : Geom_BSplineCurve::MaxDegree()); return true;
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
    case S_SET_ORIGIN_U: bs->SetOrigin(A(a, 0), A(a, 1)); return true;
    case S_SET_NOT_PERIODIC: bs->SetNotPeriodic(); return true;
    case S_MOVE_POINT: { int f = 0, l = 0; bs->MovePoint(A(a, 0), A_p(a, 1), AI(a, 4), AI(a, 5), f, l); in(f); in(l); return true; }
    case S_MOVE_TANGENT: { int e = 0; bs->MovePointAndTangent(A(a, 0), A_p(a, 1), A_v(a, 4), A(a, 7), AI(a, 8), AI(a, 9), e); in(e); return true; }
    case S_IS_G1: ob(bs->IsG1(A(a, 0), A(a, 1), A(a, 2))); return true;
    case S_IS_EQUAL: {
      HC o = build(*cv2);
      ob(bs->IsEqual(occ::down_cast<Geom_BSplineCurve>(o), A(a, 0))); return true;
    }
    case S_LOCATE: { int i1 = 0, i2 = 0; bs->LocateU(A(a, 0), A(a, 1), i1, i2, AI(a, 2) != 0); in(i1); in(i2); return true; }
    case S_LOCAL_VALUE: o_p(bs->LocalValue(A(a, 0), AI(a, 1), AI(a, 2))); return true;
    case S_LOCAL_D0: { gp_Pnt P; bs->LocalD0(A(a, 0), AI(a, 1), AI(a, 2), P); o_p(P); return true; }
    case S_LOCAL_D1: { gp_Pnt P; gp_Vec V1; bs->LocalD1(A(a, 0), AI(a, 1), AI(a, 2), P, V1); o_p(P); o_v(V1); return true; }
    case S_LOCAL_D2: { gp_Pnt P; gp_Vec V1, V2; bs->LocalD2(A(a, 0), AI(a, 1), AI(a, 2), P, V1, V2); o_p(P); o_v(V1); o_v(V2); return true; }
    case S_LOCAL_D3: { gp_Pnt P; gp_Vec V1, V2, V3; bs->LocalD3(A(a, 0), AI(a, 1), AI(a, 2), P, V1, V2, V3); o_p(P); o_v(V1); o_v(V2); o_v(V3); return true; }
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
    case O_SET_DIR: oc->SetDirection(A_d(a, 0)); return true;
    case O_BASIS_CONT: in((int)oc->GetBasisCurveContinuity()); return true;
    case O_DIRECTION: o_d(oc->Direction()); return true;
    // evaluation representations (a = kind max_deriv has_domain first last scale offset null_rep)
    case R_SET_EREP: {
      occ::handle<GeomEval_RepCurveDesc::Base> d;
      int kind = AI(a, 0);
      if (kind == 0) d = new GeomEval_RepCurveDesc::Full();
      else if (kind == 1) { occ::handle<GeomEval_RepCurveDesc::DerivBounded> x = new GeomEval_RepCurveDesc::DerivBounded(); x->MaxDerivOrder = (size_t)std::max(AI(a, 1), 0); d = x; }
      else {
        occ::handle<GeomEval_RepCurveDesc::Mapped> x = new GeomEval_RepCurveDesc::Mapped();
        x->MaxDerivOrder = (size_t)std::max(AI(a, 1), 0);
        if (AI(a, 2)) { GeomEval_RepCurveDesc::Domain1d dom; dom.First = A(a, 3); dom.Last = A(a, 4); x->Domain = dom; }
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

static Ops plan_curve_raw(const CV& cv, const CV* cv2, int rounds, const std::function<void(const HC&, Ops&)>& choose);
// curve families: `crv [crv] ops`
static void run_curve_ops(const CV& cv, const CV* cv2, const Ops& ops) {
  HG g = build(cv);
  for (const Op& op : ops) {
    if (curve_unchecked(g, op)) { unchecked(); return; }
    curve_op(g, op, cv2);
  }
}
static void trace_cv(const CV& c) {
  if (!std::getenv("GEOM_TRACE")) return;
  std::fprintf(stderr, "TRACE kind %d deg %d per %d rat %d nk %zu np %zu mults:", c.kind, c.deg, c.per, c.rat, c.knots.size(), c.px.size());
  for (int m : c.mults) std::fprintf(stderr, " %d", m);
  std::fprintf(stderr, " knots:"); for (double k : c.knots) std::fprintf(stderr, " %g", k);
  try { HC b = build(c); if (auto* s = dynamic_cast<Geom_BSplineCurve*>(b.get())) std::fprintf(stderr, " dist %d", (int)s->KnotDistribution()); } catch (...) {}
  std::fprintf(stderr, "\n");
}
// the state operations of a plan are collapsed into the last one (a line prints the state of the object once, at the end)
static void collapse_states(Ops& ops) {
  int last = -1;
  for (size_t i = 0; i < ops.size(); ++i) if (ops[i].code == 1) last = (int)i;
  if (last < 0) return;
  Ops out;
  for (size_t i = 0; i < ops.size(); ++i) if (ops[i].code != 1 || (int)i == last) out.push_back(ops[i]);
  ops = out;
}
static Ops plan_curve(const CV& cv, const CV* cv2, int rounds, const std::function<void(const HC&, Ops&)>& choose) {
  Ops r = plan_curve_raw(cv, cv2, rounds, choose);
  if (!std::getenv("GEOM_KEEP_STATES")) collapse_states(r);
  return r;
}
static Ops plan_curve_raw(const CV& cv, const CV* cv2, int rounds, const std::function<void(const HC&, Ops&)>& choose) {
  trace_cv(cv);
  HG g;
  try { g = build(cv); } catch (Standard_Failure&) { return Ops{mk(C_STATE)}; }
  return plan(g, rounds, [&](const HG& live, Ops& add) { choose(HC(dynamic_cast<Geom_Curve*>(live.get())), add); },
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

// =================================================================================
// random curves
static void rframe9(CV& c) {
  double f[10]; rframe_vals(f, false);
  for (int i = 0; i < 9; ++i) c.v[i] = f[i];
  c.ctor = rint(0, 1);
}
static double rradius(bool allow_zero) {
  double r = rnd(0, 1);
  if (r < 0.35) return 0.5 * rint(1, 12);
  if (r < 0.85) return rnd(0.05, 8);
  if (r < 0.92) return rnd(1e-4, 1e-2);
  return allow_zero ? 0.0 : rnd(0.5, 3);
}
static CV rline() { CV c; c.kind = 0; c.ctor = rint(0, 2); c.v[0] = crd(); c.v[1] = crd(); c.v[2] = crd(); if (coin(0.3)) { c.v[3] = 1; c.v[4] = 0; c.v[5] = 0; } else rdir3(c.v[3], c.v[4], c.v[5]); return c; }
static CV rcircle(bool allow_zero = true) { CV c; c.kind = 1; rframe9(c); c.a = rradius(allow_zero); return c; }
static CV rellipse() {
  CV c; c.kind = 2; rframe9(c); c.b = rradius(true);
  double r = rnd(0, 1);
  c.a = r < 0.1 ? c.b : (r < 0.15 ? c.b + 1e-12 : c.b * rnd(1, 5) + (coin(0.2) ? 0.5 : 0));
  return c;
}
static CV rhyperbola() { CV c; c.kind = 3; rframe9(c); c.a = rradius(false); c.b = rradius(true); return c; }
static CV rparabola() {
  CV c; c.kind = 4; rframe9(c); c.a = rradius(true);
  c.ctor = rint(0, 2);
  if (c.ctor == 2) {
    // directrix (o, d) and a focus at a distance of at least 0.2 from it
    rdir3(c.v[3], c.v[4], c.v[5]);
    double dx = c.v[3], dy = c.v[4], dz = c.v[5];
    double dd = std::sqrt(dx * dx + dy * dy + dz * dz); dx /= dd; dy /= dd; dz /= dd;
    // a vector perpendicular to d: d x (the axis with the smallest component)
    double ax = std::fabs(dx) < 0.6 ? 1 : 0, ay = (ax == 0 && std::fabs(dy) < 0.6) ? 1 : 0, az = (ax == 0 && ay == 0) ? 1 : 0;
    double px = dy * az - dz * ay, py = dz * ax - dx * az, pz = dx * ay - dy * ax;
    double pn = std::sqrt(px * px + py * py + pz * pz); px /= pn; py /= pn; pz /= pn;
    double qx = dy * pz - dz * py, qy = dz * px - dx * pz, qz = dx * py - dy * px;
    double s = rnd(-3, 3), t = rnd(0.2, 4), w = rnd(-2, 2);
    c.v[6] = c.v[0] + dx * s + px * t + qx * w;
    c.v[7] = c.v[1] + dy * s + py * t + qy * w;
    c.v[8] = c.v[2] + dz * s + pz * t + qz * w;
  }
  return c;
}
static void add_pt(CV& c, double x, double y, double z) { c.px.push_back(x); c.py.push_back(y); c.pz.push_back(z); }
static CV rbezier(bool cusp_ok = true, int nmax = 6) {
  CV c; c.kind = 5;
  int n = coin(0.04) ? rint(8, std::max(nmax, 8)) : rint(2, std::min(nmax, 5));
  for (int i = 0; i < n; ++i) add_pt(c, crd(), crd(), crd());
  if (cusp_ok && n >= 3 && coin(0.12)) { int k = rint(0, n - 2); c.px[k + 1] = c.px[k]; c.py[k + 1] = c.py[k]; c.pz[k + 1] = c.pz[k]; }
  if (coin(0.12) && n >= 2) { c.px[n - 1] = c.px[0]; c.py[n - 1] = c.py[0]; c.pz[n - 1] = c.pz[0]; }   // closed
  c.rat = coin(0.3) ? 1 : 0;
  if (c.rat) {
    double r = rnd(0, 1);
    for (int i = 0; i < n; ++i) c.w.push_back(r < 0.2 ? 1.75 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3)));
  }
  return c;
}
// a valid B-spline; `style`: 0 any, 1 clamped non periodic, 2 periodic
static CV rbspline(int style = 0, bool cusp_ok = true, int degmax = 4, int npmax = 6) {
  for (;;) {
    CV c; c.kind = 6;
    c.deg = coin(0.02) ? rint(5, 6) : rint(1, degmax);
    c.per = style == 2 ? 1 : (style == 1 ? 0 : (coin(0.3) ? 1 : 0));
    int nk = rint(2, 4);
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
    if (np < 2 || np > npmax || (c.per && np <= c.deg)) continue;   // a periodic curve with fewer poles than its degree + 1 makes several OCCT operations read outside their arrays
    for (int i = 0; i < np; ++i) add_pt(c, crd(), crd(), crd());
    if (cusp_ok && np >= 3 && coin(0.12)) { int q = rint(0, np - 2); c.px[(size_t)q + 1] = c.px[(size_t)q]; c.py[(size_t)q + 1] = c.py[(size_t)q]; c.pz[(size_t)q + 1] = c.pz[(size_t)q]; }
    c.rat = coin(0.3) ? 1 : 0;
    if (c.rat) {
      double r = rnd(0, 1);
      for (int i = 0; i < np; ++i) c.w.push_back(r < 0.2 ? 1.75 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3)));
    }
    return c;
  }
}
// a curve of any basic kind (no trimmed, no offset)
static CV rbasic() {
  double r = rnd(0, 1);
  if (r < 0.14) return rline();
  if (r < 0.26) return rcircle();
  if (r < 0.36) return rellipse();
  if (r < 0.44) return rhyperbola();
  if (r < 0.52) return rparabola();
  if (r < 0.76) return rbezier();
  return rbspline(0, true, 3, 7);
}
static CV mk_trim(const CV& basis, double u1, double u2, bool sense, bool adj) {
  CV c; c.kind = 7; c.basis = std::make_shared<CV>(basis); c.u1 = u1; c.u2 = u2; c.sense = sense; c.adj = adj; return c;
}
static CV mk_off(const CV& basis, double off, bool notc) {
  CV c; c.kind = 8; c.basis = std::make_shared<CV>(basis); c.off = off; c.notc = notc;
  rdir3(c.od[0], c.od[1], c.od[2]);
  if (coin(0.3)) { c.od[0] = 0; c.od[1] = 0; c.od[2] = coin(0.5) ? 1 : -1; }
  return c;
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
    bool hyp = !c.IsNull() && dynamic_cast<Geom_Hyperbola*>(c.get());
    double lim = hyp ? 2.5 : 4;
    return coin(0.2) ? 0.0 : (coin(0.3) ? 0.5 * rint(-6, 6) : rnd(-lim, lim));
  }
  double sp = l - f, r = rnd(0, 1);
  if (r < 0.10) return f;
  if (r < 0.20) return l;
  if (r < 0.28) {
    if (auto* b = dynamic_cast<Geom_BSplineCurve*>(c.get())) return b->Knot(rint(1, b->NbKnots()));
    return f + 0.5 * sp;
  }
  if (r < 0.78) return f + rnd(0, 1) * sp;
  if (r < 0.90) return f + rnd(-outside, 1 + outside) * sp;
  return f + rnd(-2.5, 3.5) * sp;
}

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
static CV rany(int depth = 0) {
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
    case 4: return rparabola();
    case 5: return rbezier();
    default: return rbspline(0, true, 3, 7);
  }
}
static Geom_BSplineCurve* BS(const HC& c) { return dynamic_cast<Geom_BSplineCurve*>(c.get()); }
static Geom_BezierCurve* BZ(const HC& c) { return dynamic_cast<Geom_BezierCurve*>(c.get()); }
static double rweight() { return coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3); }
static double rtol() { double r = rnd(0, 1); return r < 0.2 ? 0.0 : (r < 0.5 ? 1e-9 : (r < 0.8 ? 1e-4 : 1e-2)); }

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

// a B-spline that OCCT classifies as uniform or quasi-uniform (equal knot spacing, simple inner knots)
static CV rbs_uniform(bool periodic) {
  for (;;) {
    CV c; c.kind = 6; c.deg = rint(2, 3); c.per = periodic ? 1 : 0;
    int nk = rint(c.per ? 4 : 3, 5);
    double k = coin(0.85) ? 0.25 * rint(1, 12) : 0.25 * rint(-8, 8), step = 0.25 * rint(1, 8);
    for (int i = 0; i < nk; ++i) { c.knots.push_back(k); k += step; c.mults.push_back(1); }
    if (!c.per) { c.mults[0] = c.deg + 1; c.mults[(size_t)nk - 1] = c.deg + 1; }
    NCollection_Array1<int> M(1, nk);
    for (int i = 0; i < nk; ++i) M(i + 1) = c.mults[(size_t)i];
    int np = BSplCLib::NbPoles(c.deg, c.per != 0, M);
    if (np < 3 || np > 6 || (c.per && np <= c.deg)) continue;
    for (int i = 0; i < np; ++i) add_pt(c, crd(), crd(), crd());
    c.rat = coin(0.3) ? 1 : 0;
    if (c.rat) for (int i = 0; i < np; ++i) c.w.push_back(rweight());
    return c;
  }
}
static CV rbs_mixed(double bias = 0.45) {
  if (coin(bias)) return rbs_uniform(coin(0.35));
  return rbspline(0, true, 3, 6);
}
// invalid variants for the constructors
static void corrupt_bezier(CV& c) {
  switch (rint(0, 4)) {
    case 0: c.px.resize(1); c.py.resize(1); c.pz.resize(1); c.rat = 0; c.w.clear(); break;
    case 1: while (c.px.size() < 27) add_pt(c, crd(), crd(), crd()); c.rat = 0; c.w.clear(); break;
    case 2: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w[(size_t)rint(0, (int)c.w.size() - 1)] = coin(0.5) ? 0.0 : -1.0; break;
    case 3: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w.pop_back(); break;
    default: while (c.px.size() < 26) add_pt(c, crd(), crd(), crd()); c.rat = 0; c.w.clear(); break;   // 26 poles: the maximum, valid
  }
}
static void corrupt_bspline(CV& c) {
  switch (rint(0, 9)) {
    case 9: if (c.mults.size() >= 3) c.mults[1] = c.deg + 1; else c.mults[0] += 1; break;   // an inner multiplicity above the degree: NbPoles is 0
    case 0: c.deg = 0; break;
    case 1: c.deg = 26; break;
    case 2: c.px.pop_back(); c.py.pop_back(); c.pz.pop_back(); if (c.rat) c.w.pop_back(); break;
    case 3: c.mults.pop_back(); break;
    case 4: if (c.knots.size() >= 2) c.knots[1] = c.knots[0]; break;
    case 5: if (c.knots.size() >= 2) std::swap(c.knots[0], c.knots[1]); break;
    case 6: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w[(size_t)rint(0, (int)c.w.size() - 1)] = coin(0.5) ? 0.0 : -0.5; break;
    case 7: if (!c.rat) { c.rat = 1; for (size_t i = 0; i < c.px.size(); ++i) c.w.push_back(rweight()); } c.w.push_back(1.0); break;
    default: c.mults[0] += 1; break;
  }
}
static int NPo(const HC& c) { return BZ(c) ? BZ(c)->NbPoles() : BS(c)->NbPoles(); }
static double rknotu(const HC& c, double outside = 0.0) {
  Geom_BSplineCurve* b = BS(c);
  double f = b->FirstParameter(), l = b->LastParameter();
  double r = rnd(0, 1);
  if (r < 0.2) return b->Knot(rint(1, b->NbKnots()));
  if (r < 0.28) return f;
  if (r < 0.36) return l;
  return f + rnd(-outside, 1 + outside) * (l - f);
}

// =================================================================================
// families: curves
static bool emit_fam(const char* N, const CV& cv, const CV* cv2, const Ops& ops, bool libm, bool guard = false) { return emit_curve(N, cv, cv2, ops, libm, guard); }
#define CF(NAME, TARGET) LOOP(NAME, TARGET)
static Op op_i(int code, Vd a) { return mk(code, a); }

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
  const struct { const char* n; int mode; int cnt; } ev[] = {{"curve_d01", 0, 5}, {"curve_d23", 1, 4}, {"curve_dn", 2, 4}};
  for (auto& e : ev) {
    LOOP(e.n, 28) {
      CV cv = rkind(rint(0, 6));
      if (coin(0.2)) cv = rtrim(cv, false);
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        for (int k = 0; k < e.cnt; ++k) {
          double u = rparam(c);
          if (e.mode == 2) add.push_back(op_d(C_DN, u, coin(0.07) ? rint(-1, 0) : rint(1, 6)));
          else if (e.mode == 0) { int r = rint(0, 2); add.push_back(op_d(r == 0 ? C_D0 : (r == 1 ? C_D1 : C_VALUE), u)); }
          else add.push_back(op_d(coin(0.5) ? C_D2 : C_D3, u));
        }
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
  }

static void fam_line_conics() {
  { CF("line_ops", 24) {
      CV cv = rline();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        double dx, dy, dz; rdir3(dx, dy, dz);
        int r = rint(0, 8);
        switch (r) {
          case 0: add.push_back(mk(L_SET_LIN, Vd{crd(), crd(), crd(), dx, dy, dz})); add.push_back(mk(C_STATE)); break;
          case 1: add.push_back(mk(L_SET_DIR, Vd{dx, dy, dz})); add.push_back(mk(C_STATE)); break;
          case 2: add.push_back(mk(L_SET_LOC, Vd{crd(), crd(), crd()})); add.push_back(mk(C_STATE)); break;
          case 3: add.push_back(mk(L_SET_POS, Vd{crd(), crd(), crd(), dx, dy, dz})); add.push_back(mk(C_STATE)); break;
          case 4: add.push_back(mk(L_LIN)); add.push_back(mk(L_POSITION)); break;
          case 5: add_evals(add, c, 3); break;
          case 6: add.push_back(mk(C_FIRST)); add.push_back(mk(C_LAST)); add.push_back(mk(C_IS_CLOSED)); add.push_back(mk(C_IS_PERIODIC)); add.push_back(mk(C_CONTINUITY)); add.push_back(mk(C_IS_CN, Vd{3.0})); if (coin(0.25)) add.push_back(mk(C_PERIOD)); break;
          default: add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE)); add.push_back(mk(C_REVERSED_PARAM, Vd{crd()})); break;
        }
      });
      emit_fam(N, cv, nullptr, ops, true);
    } }
  auto conic_ops = [&](int kind, const char* name) {
    LOOP(name, 28) {
      CV cv = rkind(kind);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        double fr[10]; rframe_vals(fr, false);
        Vd fv(fr, fr + 9); double dx, dy, dz; rdir3(dx, dy, dz);
        int r = rint(0, 9);
        switch (kind) {
          case 1:
            if (r == 0) add.push_back(mk(CI_SET_RADIUS, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) { Vd a = fv; a.push_back(rradius(true)); add.push_back(mk(CI_SET_CIRC, a)); }
            else if (r == 2) add.push_back(mk(CI_CIRC));
            break;
          case 2:
            if (r == 0) add.push_back(mk(E_SET_MAJOR, Vd{coin(0.15) ? rnd(0, 1) : rnd(1, 9)}));
            else if (r == 1) add.push_back(mk(E_SET_MINOR, Vd{coin(0.15) ? rnd(-1, 0) : rnd(0, 3)}));
            else if (r == 2) { double b = rradius(true); Vd a = fv; a.push_back(b + rnd(0, 4)); a.push_back(b); add.push_back(mk(E_SET_ELIPS, a)); }
            else if (r == 3) add.push_back(mk(E_ELIPS));
            else if (r == 4) { add.push_back(mk(EH_DIRECTRIX1)); add.push_back(mk(EH_DIRECTRIX2)); }
            else if (r == 5) { add.push_back(mk(EH_FOCAL)); add.push_back(mk(EH_FOCUS1)); add.push_back(mk(EH_FOCUS2)); add.push_back(mk(EH_PARAMETER)); add.push_back(mk(EH_MAJOR)); add.push_back(mk(EH_MINOR)); }
            break;
          case 3:
            if (r == 0) add.push_back(mk(E_SET_MAJOR, Vd{coin(0.12) ? -rnd(0.1, 1) : rradius(true)}));
            else if (r == 1) add.push_back(mk(E_SET_MINOR, Vd{coin(0.12) ? -rnd(0.1, 1) : rradius(true)}));
            else if (r == 2) { Vd a = fv; a.push_back(rradius(true)); a.push_back(rradius(true)); add.push_back(mk(H_SET_HYPR, a)); }
            else if (r == 3) add.push_back(mk(H_HYPR));
            else if (r == 4) { add.push_back(mk(EH_DIRECTRIX1)); add.push_back(mk(EH_DIRECTRIX2)); }
            else if (r == 5) { add.push_back(mk(EH_FOCAL)); add.push_back(mk(EH_FOCUS1)); add.push_back(mk(EH_FOCUS2)); add.push_back(mk(EH_PARAMETER)); add.push_back(mk(EH_MAJOR)); add.push_back(mk(EH_MINOR)); }
            else if (r == 6) { add.push_back(mk(H_ASYMPTOTE1)); add.push_back(mk(H_ASYMPTOTE2)); }
            else if (r == 7) { add.push_back(mk(H_CONJ1)); add.push_back(mk(H_CONJ2)); add.push_back(mk(H_OTHER)); }
            break;
          default:
            if (r == 0) add.push_back(mk(PA_SET_FOCAL, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) { Vd a = fv; a.push_back(rradius(true)); add.push_back(mk(PA_SET_PARAB, a)); }
            else if (r == 2) add.push_back(mk(PA_PARAB));
            else if (r == 3) { add.push_back(mk(PA_DIRECTRIX)); add.push_back(mk(PA_FOCUS)); }
            else if (r == 4) { add.push_back(mk(EH_FOCAL)); add.push_back(mk(EH_PARAMETER)); }
            break;
        }
        add.push_back(mk(C_STATE));
        if (r == 8) {
          int q = rint(0, 2);
          if (q == 0) add.push_back(mk(K_SET_AXIS, Vd{crd(), crd(), crd(), dx, dy, dz}));
          else if (q == 1) add.push_back(mk(K_SET_POS, Vd(fr, fr + 9)));
          else add.push_back(mk(K_SET_LOC, Vd{crd(), crd(), crd()}));
          add.push_back(mk(C_STATE)); add.push_back(mk(K_AXIS)); add.push_back(mk(K_XAXIS)); add.push_back(mk(K_YAXIS));
        }
        if (r == 9) add_evals(add, c, 2);
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
          default: if (cv.ctor != 2) cv.a = -rnd(0.1, 2); break;
        }
      }
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(K_ECC)); ops.push_back(mk(K_AXIS)); ops.push_back(mk(K_XAXIS)); ops.push_back(mk(K_YAXIS)); ops.push_back(mk(K_LOCATION)); ops.push_back(mk(K_POSITION));
      ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST)); ops.push_back(mk(C_IS_CLOSED)); ops.push_back(mk(C_IS_PERIODIC)); ops.push_back(mk(C_CONTINUITY));
      if (kind == 1) ops.push_back(mk(CI_CIRC));
      if (kind == 2) { ops.push_back(mk(E_ELIPS)); ops.push_back(mk(EH_FOCUS1)); ops.push_back(mk(EH_PARAMETER)); }
      if (kind == 3) { ops.push_back(mk(H_HYPR)); ops.push_back(mk(EH_FOCUS2)); ops.push_back(mk(EH_FOCAL)); }
      if (kind == 4) { ops.push_back(mk(PA_PARAB)); ops.push_back(mk(PA_FOCUS)); ops.push_back(mk(PA_DIRECTRIX)); }
      emit_fam(N, cv, nullptr, ops, true);
    } }
  }

// =================================================================================
// Bezier and B-spline curves
static void fam_bezier() {
  { CF("bezier_build", 40) {
      CV cv = rbezier(true, 10);
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
  { CF("bezier_poles", 40) {
      CV cv = rbezier(true, 5);
      if (coin(0.12)) { size_t want = (size_t)(24 + rint(0, 2)); while (cv.px.size() < want) add_pt(cv, crd(), crd(), crd()); cv.rat = 0; cv.w.clear(); }
      Ops ops = plan_curve(cv, nullptr, coin(0.25) ? 2 : 1, [&](const HC& c, Ops& add) {
        int n = NPo(c);
        if (coin(0.4)) {
          int idx = coin(0.08) ? (coin(0.5) ? 0 : n + 1) : rint(1, n);
          add.push_back(op_i(BB_REMOVE_POLE, Vd{(double)idx})); add.push_back(mk(C_STATE));
          return;
        }
        bool after = coin(0.5);
        int idx = after ? rint(0, n) : rint(1, n + 1);
        if (coin(0.08)) idx = after ? n + 1 + rint(0, 2) : -rint(0, 2);
        double w = coin(0.5) ? 1.0 : (coin(0.1) ? (coin(0.5) ? 0.0 : -1.0) : rweight());
        if (coin(0.3)) add.push_back(op_i(after ? BB_INS_AFTER_NW : BB_INS_BEFORE_NW, Vd{(double)idx, crd(), crd(), crd()}));
        else add.push_back(op_i(after ? BB_INS_AFTER : BB_INS_BEFORE, Vd{(double)idx, crd(), crd(), crd(), w}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
    { CF("bezier_increase_segment", 36) {
      CV cv = rbezier(true, 5);
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        if (coin(0.5)) {
          int d = BZ(c)->Degree();
          int nd = coin(0.1) ? d - rint(1, 2) : (coin(0.06) ? 26 : d + rint(0, 4));
          add.push_back(op_i(BB_INCREASE, Vd{(double)nd}));
        } else {
          double u1 = coin(0.3) ? 0.25 * rint(0, 4) : rnd(-0.3, 1.2), u2 = coin(0.3) ? 0.25 * rint(0, 4) : rnd(-0.3, 1.3);
          if (coin(0.7) && u1 > u2) std::swap(u1, u2);
          add.push_back(op_i(BB_SEGMENT, Vd{u1, u2, 0.0}));
        }
        add.push_back(mk(C_STATE));
        add_evals(add, c, 1);
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
    { CF("bezier_set_pole", 36) {
      CV cv = rbezier(true, 5);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        int n = NPo(c); int idx = coin(0.08) ? (coin(0.5) ? 0 : n + 1) : rint(1, n);
        double w = coin(0.1) ? (coin(0.5) ? 0.0 : -2.0) : (coin(0.25) ? 1.0 : rweight());
        int r = rint(0, 3);
        if (r == 0) add.push_back(op_i(BB_SET_POLE, Vd{(double)idx, crd(), crd(), crd()}));
        else if (r == 1) add.push_back(op_i(BB_SET_POLE_W, Vd{(double)idx, crd(), crd(), crd(), w}));
        else if (r == 2) add.push_back(op_i(BB_SET_WEIGHT, Vd{(double)idx, w}));
        else add.push_back(op_i(BB_RESOLUTION, Vd{coin(0.3) ? 1e-7 : (coin(0.5) ? 1e-3 : rnd(0.01, 2))}));
        add.push_back(mk(C_STATE));
        if (coin(0.4)) add.push_back(op_i(BB_RESOLUTION, Vd{1e-3}));
      });
      emit_fam(N, cv, nullptr, ops, false);
    } }
    { CF("bezier_eval", 36) {
      CV cv = rbezier(true, coin(0.1) ? 12 : 6);
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
  { CF("bspline_increase", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c); int nk = b->NbKnots(), d = b->Degree();
        if (coin(0.4)) {
          int nd = coin(0.1) ? d - rint(1, 2) : (coin(0.05) ? 26 : d + rint(0, 2));
          add.push_back(op_i(BB_INCREASE, Vd{(double)nd})); add.push_back(mk(C_STATE));
          add_evals(add, c, 1, 3);
          return;
        }
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
      { CF("bspline_insert_knots", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c); int d = b->Degree();
        if (coin(0.4)) {
          double u = rknotu(c, coin(0.08) ? 0.4 : 0.0);
          if (coin(0.25)) u = 0.5 * (b->Knot(1) + b->Knot(b->NbKnots()));
          add.push_back(op_i(S_INS_KNOT, Vd{u, (double)(coin(0.15) ? rint(d, d + 1) : rint(1, std::max(d - 1, 1))), rtol()}));
        } else {
          int n = rint(1, 3); Vd us;
          for (int i = 0; i < n; ++i) us.push_back(rknotu(c, coin(0.06) ? 0.3 : 0.0));
          std::sort(us.begin(), us.end());
          Vd a{rtol(), (double)rint(0, 1), (double)n};
          for (double u : us) a.push_back(u);
          for (int i = 0; i < n; ++i) a.push_back(rint(1, std::max(1, d - 1)));
          add.push_back(op_i(S_INS_KNOTS, a));
        }
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_remove_knot", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c);
        if (coin(0.55)) {   // insert a knot, then remove it again
          double u = 0.5 * (b->FirstParameter() + b->LastParameter()) + rnd(-0.3, 0.3);
          add.push_back(op_i(S_INS_KNOT, Vd{u, 1.0, 0.0}));
          return;
        }
        int i1 = b->FirstUKnotIndex(), i2 = b->LastUKnotIndex();
        int idx = coin(0.06) ? i2 + 1 : rint(i1, i2);
        int mu = (idx >= 1 && idx <= b->NbKnots()) ? b->Multiplicity(idx) : 1;
        int m = coin(0.1) ? -1 : rint(0, std::max(mu, 1));
        add.push_back(op_i(S_REMOVE_KNOT, Vd{(double)idx, (double)m, coin(0.5) ? 1e-6 : (coin(0.5) ? 1e-2 : 1e3)}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_segment", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c);
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
        Geom_BSplineCurve* b = BS(c); int nk = b->NbKnots();
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
      CV cv = coin(0.5) ? rbspline(2, true, 3, 6) : rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c);
        int r = rint(0, 5);
        if (r == 0) { add.push_back(mk(S_SET_PERIODIC)); add.push_back(mk(C_STATE)); }
        else if (r == 1) { add.push_back(mk(S_SET_NOT_PERIODIC)); add.push_back(mk(C_STATE)); }
        else if (r == 2) {
          int i1 = b->FirstUKnotIndex(), i2 = b->LastUKnotIndex();
          add.push_back(op_i(S_SET_ORIGIN, Vd{(double)(coin(0.1) ? i2 + 1 : rint(i1, i2))})); add.push_back(mk(C_STATE));
        } else if (r == 3) {
          double f = b->FirstParameter(), l = b->LastParameter(), sp = l - f;
          double u = coin(0.25) ? b->Knot(rint(1, b->NbKnots())) : f + rnd(-1.2, 2.2) * sp;
          add.push_back(op_i(S_SET_ORIGIN_U, Vd{u, coin(0.5) ? 1e-7 : (coin(0.5) ? 1e-3 : 0.0)})); add.push_back(mk(C_STATE));
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
        int r = rint(0, 3);
        if (r == 0) add.push_back(op_i(BB_SET_POLE, Vd{(double)idx, crd(), crd(), crd()}));
        else if (r == 1) add.push_back(op_i(BB_SET_POLE_W, Vd{(double)idx, crd(), crd(), crd(), w}));
        else if (r == 2) add.push_back(op_i(BB_SET_WEIGHT, Vd{(double)idx, w}));
        else add.push_back(op_i(BB_RESOLUTION, Vd{coin(0.3) ? 1e-7 : (coin(0.5) ? 1e-3 : rnd(0.01, 2))}));
        add.push_back(mk(C_STATE));
        if (coin(0.4)) add.push_back(op_i(BB_RESOLUTION, Vd{1e-3}));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_move_point", 36) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        int n = NPo(c); int i1 = rint(1, n), i2 = rint(i1, n);
        if (coin(0.08)) { i1 = n; i2 = 1; }
        add.push_back(op_i(S_MOVE_POINT, Vd{rknotu(c, 0.0), crd(), crd(), crd(), (double)i1, (double)i2}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, false, true);
    } }
  { CF("bspline_move_tangent", 32) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 2, [&](const HC& c, Ops& add) {
        add.push_back(op_i(S_MOVE_TANGENT, Vd{rknotu(c, 0.0), crd(), crd(), crd(), crd(), crd(), crd(), coin(0.5) ? 1e-6 : 1e-2, (double)rint(-1, 2), (double)rint(-1, 2)}));
        add.push_back(mk(C_STATE));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
    { CF("bspline_locate_local", 40) {
      CV cv = rbs_mixed();
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c); int nk = b->NbKnots();
        for (int k = 0; k < 2; ++k) add.push_back(op_i(S_LOCATE, Vd{rknotu(c, 1.0), coin(0.3) ? 0.0 : (coin(0.5) ? 1e-9 : 1e-2), (double)rint(0, 1)}));
        for (int k = 0; k < 3; ++k) {
          int k1 = rint(1, nk - 1), k2 = rint(k1 + 1, nk);
          if (coin(0.05)) k2 = k1;
          double u = rknotu(c, 0.0);
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
    { CF("bspline_is_g1", 36) {
      CV cv = rbs_mixed(0.0);
      if (coin(0.3)) {   // a degree 1 curve with collinear or sharp corners
        cv = rbspline(1, true, 1, 8);
        if (coin(0.5)) for (size_t i = 0; i < cv.px.size(); ++i) { cv.px[i] = 0.5 * (double)i; cv.py[i] = 0.25 * (double)i; cv.pz[i] = 0.125 * (double)i; }
      }
      Ops ops = plan_curve(cv, nullptr, 1, [&](const HC& c, Ops& add) {
        Geom_BSplineCurve* b = BS(c);
        double f = b->FirstParameter(), l = b->LastParameter();
        add.push_back(op_i(S_IS_G1, Vd{f, l, coin(0.5) ? 1e-3 : 0.5}));
        add.push_back(op_i(S_IS_G1, Vd{f + 0.3 * (l - f), l - 0.2 * (l - f), 1e-6}));
        for (int k = 0; k < 3; ++k) add.push_back(mk(C_IS_CN, Vd{(double)rint(0, 5)}));
        add.push_back(mk(C_CONTINUITY));
      });
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("bspline_is_equal", 36) {
      CV cv = rbs_mixed();
      CV cv2 = cv;
      int r = rint(0, 7);
      switch (r) {
        case 0: break;                                                                                  // identical
        case 1: cv2.px[(size_t)rint(0, (int)cv2.px.size() - 1)] += 1e-9; break;                          // a pole moved by less than a tolerance
        case 2: cv2.py[(size_t)rint(0, (int)cv2.px.size() - 1)] += 0.5; break;                            // a pole moved by much
        case 3: cv2.knots[(size_t)rint(0, (int)cv2.knots.size() - 1)] += 1e-9; break;                     // a knot moved
        case 4: cv2.rat = 1; cv2.w.assign(cv2.px.size(), 1.0); break;                                    // explicit unit weights
        case 5: if (cv2.rat) cv2.w[(size_t)rint(0, (int)cv2.w.size() - 1)] *= 1.0 + 1e-9; else cv2 = rbs_mixed(); break;
        case 6: cv2 = rbs_mixed(); break;                                                                // another curve
        default: cv2.deg = cv.deg; cv2.pz[0] += 1e-6; break;
      }
      Ops ops = plan_curve(cv, &cv2, 1, [&](const HC&, Ops& add) {
        add.push_back(op_i(S_IS_EQUAL, Vd{coin(0.3) ? 0.0 : (coin(0.5) ? 1e-7 : 1e-3)}));
        add.push_back(op_i(S_IS_EQUAL, Vd{1.0}));
      });
      emit_fam(N, cv, &cv2, ops, false, true);
    } }
}

// =================================================================================
// trimmed curves, offset curves, evaluation representations
static CV rperiodic_basis() {
  double r = rnd(0, 1);
  if (r < 0.4) return rcircle(false);
  if (r < 0.7) return rellipse();
  return rbspline(2, true, 3, 8);
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
      { HC hb; try { hb = build(cv); } catch (Standard_Failure&) {} if (!hb.IsNull()) { add_evals(ops, hb, 2); ops.push_back(op_d(C_REVERSED_PARAM, rparam(hb))); } }
      emit_fam(N, cv, nullptr, ops, true, true);
    } }
  { CF("trimmed_set_trim", 36) {
      CV cv = rtrim(rbasic(), false);
      Ops ops = plan_curve(cv, nullptr, 3, [&](const HC& c, Ops& add) {
        Geom_TrimmedCurve* t = dynamic_cast<Geom_TrimmedCurve*>(c.get());
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
    if (c.px.size() < 3) { add_pt(c, crd(), crd(), crd()); if (c.rat) c.w.push_back(1.0); }
    c.px[1] = c.px[0]; c.py[1] = c.py[0]; c.pz[1] = c.pz[0];
    if (coin(0.4) && c.px.size() >= 4) { size_t n = c.px.size(); c.px[n - 2] = c.px[n - 1]; c.py[n - 2] = c.py[n - 1]; c.pz[n - 2] = c.pz[n - 1]; }
    return c;
  }
  CV c = rbspline(1, false, 3, 7);
  if (c.px.size() >= 3) { size_t k = (size_t)rint(0, (int)c.px.size() - 2); c.px[k + 1] = c.px[k]; c.py[k + 1] = c.py[k]; c.pz[k + 1] = c.pz[k]; }
  return c;
}
static void fam_offset() {
  { CF("offset_build", 44) {
      CV b = roffset_basis();
      if (coin(0.25)) { b = rbspline(0, true, 3, 7); }
      CV cv = roff(b);
      Ops ops;
      ops.push_back(mk(C_STATE)); ops.push_back(mk(O_OFFSET)); ops.push_back(mk(O_DIRECTION)); ops.push_back(mk(O_BASIS_CONT)); ops.push_back(mk(C_CONTINUITY)); ops.push_back(mk(C_FIRST)); ops.push_back(mk(C_LAST));
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
        int r = rint(0, 5);
        if (r == 0) { add.push_back(op_i(O_SET_BASIS, Vd{(double)rint(0, 1)})); add.push_back(mk(C_STATE)); }
        else if (r == 1) { add.push_back(op_i(O_SET_OFFSET, Vd{roffset()})); add.push_back(mk(O_OFFSET)); }
        else if (r == 2) { add.push_back(mk(C_REVERSE)); add.push_back(mk(C_STATE)); }
        else if (r == 3) { add.push_back(op_tr(4, rtr(true))); add.push_back(mk(C_STATE)); }
        else if (r == 4) { double dx, dy, dz; rdir3(dx, dy, dz); add.push_back(op_i(O_SET_DIR, Vd{dx, dy, dz})); add.push_back(mk(O_DIRECTION)); add_evals(add, c, 1); }
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
  return rbspline(1, false, 3, 6);
}
static CV rowner() {
  double r = rnd(0, 1);
  if (r < 0.4) return rbezier(false, 5);
  if (r < 0.75) return rbspline(0, false, 3, 7);
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
          if (mode == 0) add.push_back(coin(0.5) ? erep_op(0, 3, false, 0, 0, 1, 0, false) : erep_op(1, rint(0, 3), false, 0, 0, 1, 0, false));
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
          int r = rint(0, 9);
          if (r == 0) add.push_back(mk(37));
          else if (r == 1) add.push_back(op_tr(21, rtr(false)));
          else if (r == 2) add.push_back(op_tr(4, rtr(false)));
          else if (r == 3) add.push_back(mk(C_REVERSE));
          else if (r == 4) add.push_back(mk(R_CLEAR_EREP));
          else if (r == 5) { if (BZ(c) || BS(c)) add.push_back(op_i(BB_SET_POLE, Vd{1.0, crd(), crd(), crd()})); else add.push_back(op_i(O_SET_OFFSET, Vd{roffset()})); }
          else if (r == 6) { if (BZ(c)) add.push_back(op_i(BB_SEGMENT, Vd{0.1, 0.8, 0.0})); else if (BS(c)) add.push_back(op_i(BB_SEGMENT, Vd{f + 0.1 * (l - f), f + 0.8 * (l - f), Precision::PConfusion()})); else add.push_back(mk(C_FIRST)); }
          else if (r == 7) { if (BS(c)) add.push_back(op_i(BB_INCREASE, Vd{(double)(BS(c)->Degree() + 1)})); else if (BZ(c)) add.push_back(op_i(BB_INCREASE, Vd{(double)(BZ(c)->Degree() + 1)})); else add.push_back(mk(C_FIRST)); }
          else if (dynamic_cast<Geom_OffsetCurve*>(c.get())) add.push_back(op_i(O_SET_BASIS, Vd{0.0}));
          else if (BS(c)) add.push_back(op_i(S_SET_NOT_PERIODIC, Vd{}));
          else add.push_back(mk(C_FIRST));
          add.push_back(mk(R_HAS_EREP));
        }
        erep_evals(add, c, 4, f, l);
      });
      emit_fam(name, cv, &rep, ops, true, true);
    }
  };
  go("evalrep_curve_basic", 24, 0); go("evalrep_curve_mapped", 32, 2); go("evalrep_curve_invalid", 20, 3); go("evalrep_curve_copy", 36, 4);
}

// =================================================================================
// surfaces (the codes are listed in docs/api-geom.md)
enum {
  SU_UREVERSE = 300, SU_UREVERSED, SU_UREV_PARAM, SU_VREVERSE, SU_VREVERSED, SU_VREV_PARAM, SU_TRANSFORM_PARAMS, SU_PARAM_TRANSFORM, SU_BOUNDS,
  SU_IS_UCLOSED, SU_IS_VCLOSED, SU_IS_UPER, SU_IS_VPER, SU_UPERIOD, SU_VPERIOD, SU_UISO, SU_VISO, SU_CONT, SU_ISCNU, SU_ISCNV,
  SU_D0, SU_D1, SU_D2, SU_D3, SU_DN, SU_VALUE,
  EL_SET_AXIS = 330, EL_SET_LOC, EL_SET_POS, EL_AXIS, EL_LOCATION, EL_POSITION,
  PL_SET_PLN = 340, PL_PLN, PL_COEFFS,
  CY_SET_CYL = 345, CY_SET_RADIUS, CY_CYL, CY_COEFFS, CY_RADIUS,
  CO_SET_CONE = 350, CO_SET_RADIUS, CO_SET_SEMI, CO_CONE, CO_APEX, CO_COEFFS, CO_REFRADIUS, CO_SEMIANGLE,
  SP_SET_SPHERE = 360, SP_SET_RADIUS, SP_SPHERE, SP_AREA, SP_VOLUME, SP_COEFFS, SP_RADIUS,
  TO_SET_TORUS = 370, TO_SET_MAJOR, TO_SET_MINOR, TO_TORUS, TO_AREA, TO_VOLUME, TO_COEFFS, TO_MAJOR, TO_MINOR,
  SW_DIRECTION = 380, SW_BASIS, SW_SET_DIR, SW_SET_BASIS, RV_SET_AXIS, RV_SET_LOC, RV_AXIS, RV_LOCATION, RV_REFPLANE,
  RT_SET_TRIM = 390, RT_SET_TRIM_P, RT_BASIS,
  OS_SET_BASIS = 395, OS_SET_OFFSET, OS_OFFSET, OS_BASIS, OS_BASIS_CONT, OS_SURFACE, OS_UOSC, OS_VOSC,
  RS_SET_EREP = 410, RS_CLEAR_EREP, RS_HAS_EREP, RS_EREP_KIND,
  BZ_EXCHANGE = 420, BZ_INCREASE, BZ_INS_COL_AFTER, BZ_INS_COL_BEFORE, BZ_INS_ROW_AFTER, BZ_INS_ROW_BEFORE, BZ_REMOVE_COL, BZ_REMOVE_ROW, BZ_SEGMENT,
  BZ_SET_POLE, BZ_SET_POLE_W, BZ_SET_POLE_COL, BZ_SET_POLE_ROW, BZ_SET_WEIGHT, BZ_SET_WEIGHT_COL, BZ_SET_WEIGHT_ROW, BZ_RESOLUTION, BZ_MAXDEGREE,
  SB_POLE = 440, SB_WEIGHT, SB_POLES, SB_WEIGHTS, BB_UKNOTS, BB_VKNOTS, BB_UMULTS, BB_VMULTS, BB_UKNOTSEQ, BB_VKNOTSEQ, BB_NBU, BB_NBV,
  BS_EXCHANGE = 460, BS_SET_UPER, BS_SET_VPER, BS_PER_NORM, BS_SET_UORIGIN, BS_SET_VORIGIN, BS_SET_UNOTPER, BS_SET_VNOTPER, BS_INCREASE,
  BS_INS_UKNOTS, BS_INS_VKNOTS, BS_REMOVE_UKNOT, BS_REMOVE_VKNOT, BS_INC_UMULT, BS_INC_UMULT_R, BS_INCR_UMULT, BS_INC_VMULT, BS_INC_VMULT_R, BS_INCR_VMULT,
  BS_INS_UKNOT, BS_INS_VKNOT, BS_SEGMENT, BS_CHECK_SEGMENT, BS_SET_UKNOT, BS_SET_UKNOTS, BS_SET_UKNOT_M, BS_SET_VKNOT, BS_SET_VKNOTS, BS_SET_VKNOT_M,
  BS_LOCATE_U, BS_LOCATE_V, BS_SET_POLE, BS_SET_POLE_W, BS_SET_POLE_COL, BS_SET_POLE_COL_W, BS_SET_POLE_ROW, BS_SET_POLE_ROW_W, BS_SET_WEIGHT,
  BS_SET_WEIGHT_COL, BS_SET_WEIGHT_ROW, BS_MOVE_POINT, BS_FIRST_UKNOT, BS_FIRST_VKNOT, BS_LAST_UKNOT, BS_LAST_VKNOT, BS_UKNOT, BS_VKNOT, BS_UMULT, BS_VMULT,
  BS_LOCAL_VALUE, BS_LOCAL_D0, BS_LOCAL_D1, BS_LOCAL_D2, BS_LOCAL_D3, BS_LOCAL_DN, BS_UISO_C, BS_VISO_C, BS_RESOLUTION, BS_MAXDEGREE
};
static HS as_s(const HG& g) { return HS(dynamic_cast<Geom_Surface*>(g.get())); }
static void o_trsf2d_g(const gp_GTrsf2d& G) {
  in((int)G.Form()); ob(G.IsNegative());
  for (int r = 1; r <= 2; ++r) for (int c = 1; c <= 3; ++c) h(G.Value(r, c));
}
static void o_arr2_p(const NCollection_Array2<gp_Pnt>& v) {
  in(v.ColLength()); in(v.RowLength());
  for (int i = v.LowerRow(); i <= v.UpperRow(); ++i) for (int j = v.LowerCol(); j <= v.UpperCol(); ++j) o_p(v(i, j));
}
static void o_arr2_f(const NCollection_Array2<double>& v) {
  in(v.ColLength()); in(v.RowLength());
  for (int i = v.LowerRow(); i <= v.UpperRow(); ++i) for (int j = v.LowerCol(); j <= v.UpperCol(); ++j) h(v(i, j));
}
// arrays in the arguments: from index i: n then n points (x y z); weights: n then n values
static NCollection_Array1<gp_Pnt> A_pts(const Vd& a, size_t& i) {
  int n = AI(a, i); ++i;
  NCollection_Array1<gp_Pnt> P(1, std::max(n, 1));
  for (int k = 1; k <= n; ++k) { P(k) = gp_Pnt(A(a, i), A(a, i + 1), A(a, i + 2)); i += 3; }
  return P;
}
static NCollection_Array1<double> A_wts(const Vd& a, size_t& i) {
  int n = AI(a, i); ++i;
  NCollection_Array1<double> W(1, std::max(n, 1));
  for (int k = 1; k <= n; ++k) { W(k) = A(a, i); ++i; }
  return W;
}
static NCollection_Array1<double> A_kn(const Vd& a, size_t& i) { return A_wts(a, i); }
static NCollection_Array1<int> A_mu(const Vd& a, size_t& i) {
  int n = AI(a, i); ++i;
  NCollection_Array1<int> M(1, std::max(n, 1));
  for (int k = 1; k <= n; ++k) { M(k) = AI(a, i); ++i; }
  return M;
}
// the number of values the array arguments used (count + values) for the unchecked checks
static bool surf_cn_bad(Geom_Surface* s, int n) {
  return n < 0 && (dynamic_cast<Geom_BSplineSurface*>(s) || dynamic_cast<Geom_RectangularTrimmedSurface*>(s) || dynamic_cast<Geom_OffsetSurface*>(s) ||
                   dynamic_cast<Geom_SurfaceOfLinearExtrusion*>(s) || dynamic_cast<Geom_SurfaceOfRevolution*>(s));
}
static bool surf_unchecked(HG& g, const Op& op) {
  if (geom_unchecked(op)) return true;
  const Vd& a = op.a;
  Geom_Surface* s = dynamic_cast<Geom_Surface*>(g.get());
  Geom_BSplineSurface* bs = dynamic_cast<Geom_BSplineSurface*>(g.get());
  Geom_BezierSurface* bz = dynamic_cast<Geom_BezierSurface*>(g.get());
  switch (op.code) {
    case SU_TRANSFORM_PARAMS: return tr_unchecked(tr_make(a, 2));
    case SU_PARAM_TRANSFORM: return tr_unchecked(tr_make(a, 0));
    case SU_UPERIOD: return !s->IsUPeriodic();
    case SU_VPERIOD: return !s->IsVPeriodic();
    case SU_ISCNU: case SU_ISCNV: return surf_cn_bad(s, AI(a, 0));
    case CY_RADIUS: return false;
    case SB_POLE: case SB_WEIGHT:
      if (bs) return bad_idx(AI(a, 0), bs->NbUPoles()) || bad_idx(AI(a, 1), bs->NbVPoles());
      if (bz) return bad_idx(AI(a, 0), bz->NbUPoles()) || bad_idx(AI(a, 1), bz->NbVPoles());
      return false;
    case BS_UKNOT: case BS_UMULT: return bad_idx(AI(a, 0), bs->NbUKnots());
    case BS_VKNOT: case BS_VMULT: return bad_idx(AI(a, 0), bs->NbVKnots());
    case BS_INC_UMULT: return bad_idx(AI(a, 0), bs->NbUKnots());
    case BS_INC_VMULT: return bad_idx(AI(a, 0), bs->NbVKnots());
    case BS_INC_UMULT_R: case BS_INCR_UMULT: return AI(a, 0) < 1 || AI(a, 1) > bs->NbUKnots() || AI(a, 0) > AI(a, 1);
    case BS_INC_VMULT_R: case BS_INCR_VMULT: return AI(a, 0) < 1 || AI(a, 1) > bs->NbVKnots() || AI(a, 0) > AI(a, 1);
    case BS_LOCAL_VALUE: case BS_LOCAL_D0: case BS_LOCAL_D1: case BS_LOCAL_D2: case BS_LOCAL_D3: case BS_LOCAL_DN: {
      int u1 = AI(a, 2), u2 = AI(a, 3), v1 = AI(a, 4), v2 = AI(a, 5);
      return u1 < 1 || u2 > bs->NbUKnots() || u1 >= u2 || v1 < 1 || v2 > bs->NbVKnots() || v1 >= v2;
    }
    default: return false;
  }
}
static void put_eval(const Geom_Surface* s, int code, double u, double v, int nu, int nv) {
  switch (code) {
    case SU_D0: o_p(s->EvalD0(u, v)); break;
    case SU_D1: { auto r = s->EvalD1(u, v); o_p(r.Point); o_v(r.D1U); o_v(r.D1V); break; }
    case SU_D2: { auto r = s->EvalD2(u, v); o_p(r.Point); o_v(r.D1U); o_v(r.D1V); o_v(r.D2U); o_v(r.D2V); o_v(r.D2UV); break; }
    case SU_D3: { auto r = s->EvalD3(u, v); o_p(r.Point); o_v(r.D1U); o_v(r.D1V); o_v(r.D2U); o_v(r.D2V); o_v(r.D2UV); o_v(r.D3U); o_v(r.D3V); o_v(r.D3UUV); o_v(r.D3UVV); break; }
    case SU_DN: o_v(s->EvalDN(u, v, nu, nv)); break;
    default: o_p(s->Value(u, v)); break;
  }
}
static bool surf_op(HG& g, const Op& op, const CV* cv2, const SV* sv2) {
  if (geom_common(g, op)) return true;
  const Vd& a = op.a;
  Geom_Surface* s = dynamic_cast<Geom_Surface*>(g.get());
  Geom_ElementarySurface* es = dynamic_cast<Geom_ElementarySurface*>(g.get());
  Geom_Plane* pl = dynamic_cast<Geom_Plane*>(g.get());
  Geom_CylindricalSurface* cy = dynamic_cast<Geom_CylindricalSurface*>(g.get());
  Geom_ConicalSurface* co = dynamic_cast<Geom_ConicalSurface*>(g.get());
  Geom_SphericalSurface* sp = dynamic_cast<Geom_SphericalSurface*>(g.get());
  Geom_ToroidalSurface* to = dynamic_cast<Geom_ToroidalSurface*>(g.get());
  Geom_SurfaceOfLinearExtrusion* ex = dynamic_cast<Geom_SurfaceOfLinearExtrusion*>(g.get());
  Geom_SurfaceOfRevolution* rv = dynamic_cast<Geom_SurfaceOfRevolution*>(g.get());
  Geom_SweptSurface* sw = dynamic_cast<Geom_SweptSurface*>(g.get());
  Geom_BezierSurface* bz = dynamic_cast<Geom_BezierSurface*>(g.get());
  Geom_BSplineSurface* bs = dynamic_cast<Geom_BSplineSurface*>(g.get());
  Geom_RectangularTrimmedSurface* rt = dynamic_cast<Geom_RectangularTrimmedSurface*>(g.get());
  Geom_OffsetSurface* os = dynamic_cast<Geom_OffsetSurface*>(g.get());
  switch (op.code) {
    case SU_UREVERSE: s->UReverse(); return true;
    case SU_UREVERSED: o_state(s->UReversed()); return true;
    case SU_UREV_PARAM: h(s->UReversedParameter(A(a, 0))); return true;
    case SU_VREVERSE: s->VReverse(); return true;
    case SU_VREVERSED: o_state(s->VReversed()); return true;
    case SU_VREV_PARAM: h(s->VReversedParameter(A(a, 0))); return true;
    case SU_TRANSFORM_PARAMS: { double u = A(a, 0), v = A(a, 1); s->TransformParameters(u, v, tr_gp(tr_make(a, 2))); h(u); h(v); return true; }
    case SU_PARAM_TRANSFORM: o_trsf2d_g(s->ParametricTransformation(tr_gp(tr_make(a, 0)))); return true;
    case SU_BOUNDS: { double u1, u2, v1, v2; s->Bounds(u1, u2, v1, v2); h(u1); h(u2); h(v1); h(v2); return true; }
    case SU_IS_UCLOSED: ob(s->IsUClosed()); return true;
    case SU_IS_VCLOSED: ob(s->IsVClosed()); return true;
    case SU_IS_UPER: ob(s->IsUPeriodic()); return true;
    case SU_IS_VPER: ob(s->IsVPeriodic()); return true;
    case SU_UPERIOD: h(s->UPeriod()); return true;
    case SU_VPERIOD: h(s->VPeriod()); return true;
    case SU_UISO: o_state(s->UIso(A(a, 0))); return true;
    case SU_VISO: o_state(s->VIso(A(a, 0))); return true;
    case SU_CONT: in((int)s->Continuity()); return true;
    case SU_ISCNU: ob(s->IsCNu(AI(a, 0))); return true;
    case SU_ISCNV: ob(s->IsCNv(AI(a, 0))); return true;
    case SU_D0: case SU_D1: case SU_D2: case SU_D3: case SU_VALUE: put_eval(s, op.code, A(a, 0), A(a, 1), 0, 0); return true;
    case SU_DN: put_eval(s, op.code, A(a, 0), A(a, 1), AI(a, 2), AI(a, 3)); return true;
    // elementary surfaces
    case EL_SET_AXIS: es->SetAxis(A_ax1(a, 0)); return true;
    case EL_SET_LOC: es->SetLocation(A_p(a, 0)); return true;
    case EL_SET_POS: es->SetPosition(A_ax3(a, 0)); return true;
    case EL_AXIS: o_ax1(es->Axis()); return true;
    case EL_LOCATION: o_p(es->Location()); return true;
    case EL_POSITION: o_ax3(es->Position()); return true;
    case PL_SET_PLN: pl->SetPln(gp_Pln(A_ax3(a, 0))); return true;
    case PL_PLN: o_ax3(pl->Pln().Position()); return true;
    case PL_COEFFS: { double A_, B, C, D; pl->Coefficients(A_, B, C, D); h(A_); h(B); h(C); h(D); return true; }
    case CY_SET_CYL: cy->SetCylinder(gp_Cylinder(A_ax3(a, 0), A(a, 10))); return true;
    case CY_SET_RADIUS: cy->SetRadius(A(a, 0)); return true;
    case CY_CYL: { gp_Cylinder C = cy->Cylinder(); h(C.Radius()); o_ax3(C.Position()); return true; }
    case CY_COEFFS: { double c[10]; cy->Coefficients(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]); for (double x : c) h(x); return true; }
    case CY_RADIUS: h(cy->Radius()); return true;
    case CO_SET_CONE: co->SetCone(gp_Cone(A_ax3(a, 0), A(a, 10), A(a, 11))); return true;
    case CO_SET_RADIUS: co->SetRadius(A(a, 0)); return true;
    case CO_SET_SEMI: co->SetSemiAngle(A(a, 0)); return true;
    case CO_CONE: { gp_Cone C = co->Cone(); h(C.SemiAngle()); h(C.RefRadius()); o_ax3(C.Position()); return true; }
    case CO_APEX: o_p(co->Apex()); return true;
    case CO_COEFFS: { double c[10]; co->Coefficients(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]); for (double x : c) h(x); return true; }
    case CO_REFRADIUS: h(co->RefRadius()); return true;
    case CO_SEMIANGLE: h(co->SemiAngle()); return true;
    case SP_SET_SPHERE: sp->SetSphere(gp_Sphere(A_ax3(a, 0), A(a, 10))); return true;
    case SP_SET_RADIUS: sp->SetRadius(A(a, 0)); return true;
    case SP_SPHERE: { gp_Sphere S = sp->Sphere(); h(S.Radius()); o_ax3(S.Position()); return true; }
    case SP_AREA: h(sp->Area()); return true;
    case SP_VOLUME: h(sp->Volume()); return true;
    case SP_COEFFS: { double c[10]; sp->Coefficients(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]); for (double x : c) h(x); return true; }
    case SP_RADIUS: h(sp->Radius()); return true;
    case TO_SET_TORUS: to->SetTorus(gp_Torus(A_ax3(a, 0), A(a, 10), A(a, 11))); return true;
    case TO_SET_MAJOR: to->SetMajorRadius(A(a, 0)); return true;
    case TO_SET_MINOR: to->SetMinorRadius(A(a, 0)); return true;
    case TO_TORUS: { gp_Torus T = to->Torus(); h(T.MajorRadius()); h(T.MinorRadius()); o_ax3(T.Position()); return true; }
    case TO_AREA: h(to->Area()); return true;
    case TO_VOLUME: h(to->Volume()); return true;
    case TO_COEFFS: { NCollection_Array1<double> c(1, 35); to->Coefficients(c); o_arr_f(c); return true; }
    case TO_MAJOR: h(to->MajorRadius()); return true;
    case TO_MINOR: h(to->MinorRadius()); return true;
    // swept surfaces
    case SW_DIRECTION: o_d(sw->Direction()); return true;
    case SW_BASIS: o_state(sw->BasisCurve()); return true;
    case SW_SET_DIR: if (ex) ex->SetDirection(A_d(a, 0)); else rv->SetDirection(A_d(a, 0)); return true;
    case SW_SET_BASIS: if (ex) ex->SetBasisCurve(build(*cv2)); else rv->SetBasisCurve(build(*cv2)); return true;
    case RV_SET_AXIS: rv->SetAxis(A_ax1(a, 0)); return true;
    case RV_SET_LOC: rv->SetLocation(A_p(a, 0)); return true;
    case RV_AXIS: o_ax1(rv->Axis()); return true;
    case RV_LOCATION: o_p(rv->Location()); return true;
    case RV_REFPLANE: o_ax2(rv->ReferencePlane()); return true;
    // rectangular trimmed surfaces
    case RT_SET_TRIM: rt->SetTrim(A(a, 0), A(a, 1), A(a, 2), A(a, 3), AI(a, 4) != 0, AI(a, 5) != 0); return true;
    case RT_SET_TRIM_P: rt->SetTrim(A(a, 0), A(a, 1), AI(a, 2) != 0, AI(a, 3) != 0); return true;
    case RT_BASIS: o_state(rt->BasisSurface()); return true;
    // offset surfaces
    case OS_SET_BASIS: os->SetBasisSurface(build_s(*sv2), AI(a, 0) != 0); return true;
    case OS_SET_OFFSET: os->SetOffsetValue(A(a, 0)); return true;
    case OS_OFFSET: h(os->Offset()); return true;
    case OS_BASIS: o_state(os->BasisSurface()); return true;
    case OS_BASIS_CONT: in((int)os->GetBasisSurfContinuity()); return true;
    case OS_SURFACE: { HS r = os->Surface(); ob(!r.IsNull()); if (!r.IsNull()) o_state(r); return true; }
    case OS_UOSC: case OS_VOSC: {
      bool opp = false; occ::handle<Geom_BSplineSurface> o;
      bool ok = op.code == OS_UOSC ? os->UOsculatingSurface(A(a, 0), A(a, 1), opp, o) : os->VOsculatingSurface(A(a, 0), A(a, 1), opp, o);
      ob(ok);
      if (ok) { ob(opp); o_state(o); }
      return true;
    }
    // evaluation representations (a = kind max_deriv has_domain u1 u2 v1 v2 su ou sv ov swap null_rep)
    case RS_SET_EREP: {
      occ::handle<GeomEval_RepSurfaceDesc::Base> d;
      int kind = AI(a, 0);
      if (kind == 0) d = new GeomEval_RepSurfaceDesc::Full();
      else if (kind == 1) { occ::handle<GeomEval_RepSurfaceDesc::DerivBounded> x = new GeomEval_RepSurfaceDesc::DerivBounded(); x->MaxDerivOrder = (size_t)std::max(AI(a, 1), 0); d = x; }
      else {
        occ::handle<GeomEval_RepSurfaceDesc::Mapped> x = new GeomEval_RepSurfaceDesc::Mapped();
        x->MaxDerivOrder = (size_t)std::max(AI(a, 1), 0);
        if (AI(a, 2)) { GeomEval_RepSurfaceDesc::Domain2d dom; dom.UFirst = A(a, 3); dom.ULast = A(a, 4); dom.VFirst = A(a, 5); dom.VLast = A(a, 6); x->Domain = dom; }
        x->ParamMap.ScaleU = A(a, 7); x->ParamMap.OffsetU = A(a, 8); x->ParamMap.ScaleV = A(a, 9); x->ParamMap.OffsetV = A(a, 10); x->ParamMap.SwapUV = AI(a, 11) != 0;
        d = x;
      }
      if (!AI(a, 12)) d->Representation = build_s(*sv2);
      if (bz) bz->SetEvalRepresentation(d); else if (bs) bs->SetEvalRepresentation(d); else if (ex) ex->SetEvalRepresentation(d);
      else if (rv) rv->SetEvalRepresentation(d); else os->SetEvalRepresentation(d);
      return true;
    }
    case RS_CLEAR_EREP:
      if (bz) bz->ClearEvalRepresentation(); else if (bs) bs->ClearEvalRepresentation(); else if (ex) ex->ClearEvalRepresentation();
      else if (rv) rv->ClearEvalRepresentation(); else os->ClearEvalRepresentation();
      return true;
    case RS_HAS_EREP: ob(erep_of_s(g.get()) != nullptr); return true;
    case RS_EREP_KIND: { auto* d = erep_of_s(g.get()); in(d ? (int)d->GetKind() : -1); return true; }
    // Bezier surfaces
    case BZ_EXCHANGE: bz->ExchangeUV(); return true;
    case BZ_INCREASE: bz->Increase(AI(a, 0), AI(a, 1)); return true;
    case BZ_INS_COL_AFTER: case BZ_INS_COL_BEFORE: case BZ_INS_ROW_AFTER: case BZ_INS_ROW_BEFORE: {
      size_t i = 1; int idx = AI(a, 0);
      NCollection_Array1<gp_Pnt> P = A_pts(a, i);
      bool hw = AI(a, i) != 0; ++i;
      NCollection_Array1<double> W;
      if (hw) W = A_wts(a, i);
      if (op.code == BZ_INS_COL_AFTER) { if (hw) bz->InsertPoleColAfter(idx, P, W); else bz->InsertPoleColAfter(idx, P); }
      else if (op.code == BZ_INS_COL_BEFORE) { if (hw) bz->InsertPoleColBefore(idx, P, W); else bz->InsertPoleColBefore(idx, P); }
      else if (op.code == BZ_INS_ROW_AFTER) { if (hw) bz->InsertPoleRowAfter(idx, P, W); else bz->InsertPoleRowAfter(idx, P); }
      else { if (hw) bz->InsertPoleRowBefore(idx, P, W); else bz->InsertPoleRowBefore(idx, P); }
      return true;
    }
    case BZ_REMOVE_COL: bz->RemovePoleCol(AI(a, 0)); return true;
    case BZ_REMOVE_ROW: bz->RemovePoleRow(AI(a, 0)); return true;
    case BZ_SEGMENT: bz->Segment(A(a, 0), A(a, 1), A(a, 2), A(a, 3)); return true;
    case BZ_SET_POLE: bz->SetPole(AI(a, 0), AI(a, 1), A_p(a, 2)); return true;
    case BZ_SET_POLE_W: bz->SetPole(AI(a, 0), AI(a, 1), A_p(a, 2), A(a, 5)); return true;
    case BZ_SET_POLE_COL: case BZ_SET_POLE_ROW: {
      size_t i = 1; int idx = AI(a, 0);
      NCollection_Array1<gp_Pnt> P = A_pts(a, i);
      bool hw = AI(a, i) != 0; ++i;
      NCollection_Array1<double> W;
      if (hw) W = A_wts(a, i);
      if (op.code == BZ_SET_POLE_COL) { if (hw) bz->SetPoleCol(idx, P, W); else bz->SetPoleCol(idx, P); }
      else { if (hw) bz->SetPoleRow(idx, P, W); else bz->SetPoleRow(idx, P); }
      return true;
    }
    case BZ_SET_WEIGHT: bz->SetWeight(AI(a, 0), AI(a, 1), A(a, 2)); return true;
    case BZ_SET_WEIGHT_COL: case BZ_SET_WEIGHT_ROW: {
      size_t i = 1; NCollection_Array1<double> W = A_wts(a, i);
      if (op.code == BZ_SET_WEIGHT_COL) bz->SetWeightCol(AI(a, 0), W); else bz->SetWeightRow(AI(a, 0), W);
      return true;
    }
    case BZ_RESOLUTION: { double u = 0, v = 0; if (bz) bz->Resolution(A(a, 0), u, v); else bs->Resolution(A(a, 0), u, v); h(u); h(v); return true; }
    case BZ_MAXDEGREE: in(bz ? Geom_BezierSurface::MaxDegree() : Geom_BSplineSurface::MaxDegree()); return true;
    case SB_POLE: o_p(bz ? bz->Pole(AI(a, 0), AI(a, 1)) : bs->Pole(AI(a, 0), AI(a, 1))); return true;
    case SB_WEIGHT: h(bz ? bz->Weight(AI(a, 0), AI(a, 1)) : bs->Weight(AI(a, 0), AI(a, 1))); return true;
    case SB_POLES: o_arr2_p(bz ? bz->Poles() : bs->Poles()); return true;
    case SB_WEIGHTS: { const NCollection_Array2<double>* w = bz ? bz->Weights() : bs->Weights(); ob(w != nullptr); if (w) o_arr2_f(*w); return true; }
    case BB_UKNOTS: o_arr_f(bz ? bz->UKnots() : bs->UKnots()); return true;
    case BB_VKNOTS: o_arr_f(bz ? bz->VKnots() : bs->VKnots()); return true;
    case BB_UMULTS: o_arr_i(bz ? bz->UMultiplicities() : bs->UMultiplicities()); return true;
    case BB_VMULTS: o_arr_i(bz ? bz->VMultiplicities() : bs->VMultiplicities()); return true;
    case BB_UKNOTSEQ: o_arr_f(bz ? bz->UKnotSequence() : bs->UKnotSequence()); return true;
    case BB_VKNOTSEQ: o_arr_f(bz ? bz->VKnotSequence() : bs->VKnotSequence()); return true;
    case BB_NBU: in(bz ? bz->NbUPoles() : bs->NbUPoles()); in(bz ? bz->UDegree() : bs->UDegree()); return true;
    case BB_NBV: in(bz ? bz->NbVPoles() : bs->NbVPoles()); in(bz ? bz->VDegree() : bs->VDegree()); return true;
    // B-spline surfaces
    case BS_EXCHANGE: bs->ExchangeUV(); return true;
    case BS_SET_UPER: bs->SetUPeriodic(); return true;
    case BS_SET_VPER: bs->SetVPeriodic(); return true;
    case BS_PER_NORM: { double u = A(a, 0), v = A(a, 1); bs->PeriodicNormalization(u, v); h(u); h(v); return true; }
    case BS_SET_UORIGIN: bs->SetUOrigin(AI(a, 0)); return true;
    case BS_SET_VORIGIN: bs->SetVOrigin(AI(a, 0)); return true;
    case BS_SET_UNOTPER: bs->SetUNotPeriodic(); return true;
    case BS_SET_VNOTPER: bs->SetVNotPeriodic(); return true;
    case BS_INCREASE: bs->IncreaseDegree(AI(a, 0), AI(a, 1)); return true;
    case BS_INS_UKNOTS: case BS_INS_VKNOTS: {
      size_t i = 2; NCollection_Array1<double> K = A_kn(a, i); NCollection_Array1<int> M = A_mu(a, i);
      if (AI(a, 2) == 0) { NCollection_Array1<double> K0; NCollection_Array1<int> M0; if (op.code == BS_INS_UKNOTS) bs->InsertUKnots(K0, M0, A(a, 0), AI(a, 1) != 0); else bs->InsertVKnots(K0, M0, A(a, 0), AI(a, 1) != 0); return true; }
      if (op.code == BS_INS_UKNOTS) bs->InsertUKnots(K, M, A(a, 0), AI(a, 1) != 0); else bs->InsertVKnots(K, M, A(a, 0), AI(a, 1) != 0);
      return true;
    }
    case BS_REMOVE_UKNOT: ob(bs->RemoveUKnot(AI(a, 0), AI(a, 1), A(a, 2))); return true;
    case BS_REMOVE_VKNOT: ob(bs->RemoveVKnot(AI(a, 0), AI(a, 1), A(a, 2))); return true;
    case BS_INC_UMULT: bs->IncreaseUMultiplicity(AI(a, 0), AI(a, 1)); return true;
    case BS_INC_UMULT_R: bs->IncreaseUMultiplicity(AI(a, 0), AI(a, 1), AI(a, 2)); return true;
    case BS_INCR_UMULT: bs->IncrementUMultiplicity(AI(a, 0), AI(a, 1), AI(a, 2)); return true;
    case BS_INC_VMULT: bs->IncreaseVMultiplicity(AI(a, 0), AI(a, 1)); return true;
    case BS_INC_VMULT_R: bs->IncreaseVMultiplicity(AI(a, 0), AI(a, 1), AI(a, 2)); return true;
    case BS_INCR_VMULT: bs->IncrementVMultiplicity(AI(a, 0), AI(a, 1), AI(a, 2)); return true;
    case BS_INS_UKNOT: bs->InsertUKnot(A(a, 0), AI(a, 1), A(a, 2), AI(a, 3) != 0); return true;
    case BS_INS_VKNOT: bs->InsertVKnot(A(a, 0), AI(a, 1), A(a, 2), AI(a, 3) != 0); return true;
    case BS_SEGMENT: bs->Segment(A(a, 0), A(a, 1), A(a, 2), A(a, 3), A(a, 4), A(a, 5)); return true;
    case BS_CHECK_SEGMENT: bs->CheckAndSegment(A(a, 0), A(a, 1), A(a, 2), A(a, 3), A(a, 4), A(a, 5)); return true;
    case BS_SET_UKNOT: bs->SetUKnot(AI(a, 0), A(a, 1)); return true;
    case BS_SET_UKNOT_M: bs->SetUKnot(AI(a, 0), A(a, 1), AI(a, 2)); return true;
    case BS_SET_VKNOT: bs->SetVKnot(AI(a, 0), A(a, 1)); return true;
    case BS_SET_VKNOT_M: bs->SetVKnot(AI(a, 0), A(a, 1), AI(a, 2)); return true;
    case BS_SET_UKNOTS: case BS_SET_VKNOTS: {
      size_t i = 0; NCollection_Array1<double> K = A_kn(a, i);
      if (AI(a, 0) == 0) { NCollection_Array1<double> K0; if (op.code == BS_SET_UKNOTS) bs->SetUKnots(K0); else bs->SetVKnots(K0); return true; }
      if (op.code == BS_SET_UKNOTS) bs->SetUKnots(K); else bs->SetVKnots(K);
      return true;
    }
    case BS_LOCATE_U: { int i1 = 0, i2 = 0; bs->LocateU(A(a, 0), A(a, 1), i1, i2, AI(a, 2) != 0); in(i1); in(i2); return true; }
    case BS_LOCATE_V: { int i1 = 0, i2 = 0; bs->LocateV(A(a, 0), A(a, 1), i1, i2, AI(a, 2) != 0); in(i1); in(i2); return true; }
    case BS_SET_POLE: bs->SetPole(AI(a, 0), AI(a, 1), A_p(a, 2)); return true;
    case BS_SET_POLE_W: bs->SetPole(AI(a, 0), AI(a, 1), A_p(a, 2), A(a, 5)); return true;
    case BS_SET_POLE_COL: case BS_SET_POLE_COL_W: case BS_SET_POLE_ROW: case BS_SET_POLE_ROW_W: {
      size_t i = 1; int idx = AI(a, 0);
      NCollection_Array1<gp_Pnt> P = A_pts(a, i);
      bool hw = op.code == BS_SET_POLE_COL_W || op.code == BS_SET_POLE_ROW_W;
      NCollection_Array1<double> W;
      if (hw) W = A_wts(a, i);
      if (op.code == BS_SET_POLE_COL) bs->SetPoleCol(idx, P); else if (op.code == BS_SET_POLE_COL_W) bs->SetPoleCol(idx, P, W);
      else if (op.code == BS_SET_POLE_ROW) bs->SetPoleRow(idx, P); else bs->SetPoleRow(idx, P, W);
      return true;
    }
    case BS_SET_WEIGHT: bs->SetWeight(AI(a, 0), AI(a, 1), A(a, 2)); return true;
    case BS_SET_WEIGHT_COL: case BS_SET_WEIGHT_ROW: {
      size_t i = 1; NCollection_Array1<double> W = A_wts(a, i);
      if (op.code == BS_SET_WEIGHT_COL) bs->SetWeightCol(AI(a, 0), W); else bs->SetWeightRow(AI(a, 0), W);
      return true;
    }
    case BS_MOVE_POINT: { int uf = 0, ul = 0, vf = 0, vl = 0; bs->MovePoint(A(a, 0), A(a, 1), A_p(a, 2), AI(a, 5), AI(a, 6), AI(a, 7), AI(a, 8), uf, ul, vf, vl); in(uf); in(ul); in(vf); in(vl); return true; }
    case BS_FIRST_UKNOT: in(bs->FirstUKnotIndex()); return true;
    case BS_FIRST_VKNOT: in(bs->FirstVKnotIndex()); return true;
    case BS_LAST_UKNOT: in(bs->LastUKnotIndex()); return true;
    case BS_LAST_VKNOT: in(bs->LastVKnotIndex()); return true;
    case BS_UKNOT: h(bs->UKnot(AI(a, 0))); return true;
    case BS_VKNOT: h(bs->VKnot(AI(a, 0))); return true;
    case BS_UMULT: in(bs->UMultiplicity(AI(a, 0))); return true;
    case BS_VMULT: in(bs->VMultiplicity(AI(a, 0))); return true;
    case BS_LOCAL_VALUE: o_p(bs->LocalValue(A(a, 0), A(a, 1), AI(a, 2), AI(a, 3), AI(a, 4), AI(a, 5))); return true;
    case BS_LOCAL_D0: { gp_Pnt P; bs->LocalD0(A(a, 0), A(a, 1), AI(a, 2), AI(a, 3), AI(a, 4), AI(a, 5), P); o_p(P); return true; }
    case BS_LOCAL_D1: { gp_Pnt P; gp_Vec U, V; bs->LocalD1(A(a, 0), A(a, 1), AI(a, 2), AI(a, 3), AI(a, 4), AI(a, 5), P, U, V); o_p(P); o_v(U); o_v(V); return true; }
    case BS_LOCAL_D2: { gp_Pnt P; gp_Vec a1, a2, a3, a4, a5; bs->LocalD2(A(a, 0), A(a, 1), AI(a, 2), AI(a, 3), AI(a, 4), AI(a, 5), P, a1, a2, a3, a4, a5); o_p(P); o_v(a1); o_v(a2); o_v(a3); o_v(a4); o_v(a5); return true; }
    case BS_LOCAL_D3: { gp_Pnt P; gp_Vec w[9]; bs->LocalD3(A(a, 0), A(a, 1), AI(a, 2), AI(a, 3), AI(a, 4), AI(a, 5), P, w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7], w[8]); o_p(P); for (auto& x : w) o_v(x); return true; }
    case BS_LOCAL_DN: o_v(bs->LocalDN(A(a, 0), A(a, 1), AI(a, 2), AI(a, 3), AI(a, 4), AI(a, 5), AI(a, 6), AI(a, 7))); return true;
    case BS_UISO_C: o_state(bs->UIso(A(a, 0), AI(a, 1) != 0)); return true;
    case BS_VISO_C: o_state(bs->VIso(A(a, 0), AI(a, 1) != 0)); return true;
    case BS_RESOLUTION: { double u = 0, v = 0; bs->Resolution(A(a, 0), u, v); h(u); h(v); return true; }
    case BS_MAXDEGREE: in(Geom_BSplineSurface::MaxDegree()); return true;
    default: std::fprintf(stderr, "surf_op: bad code %d\n", op.code); std::abort();
  }
}
static void run_surf_ops(const SV& sv, const CV* cv2, const SV* sv2, const Ops& ops) {
  HG g = build_s(sv);
  for (const Op& op : ops) {
    if (surf_unchecked(g, op)) { unchecked(); return; }
    surf_op(g, op, cv2, sv2);
  }
}
// the second recipe of a surface line: none, a curve (swept surfaces), a surface
static bool emit_surf(const char* N, const SV& sv, const CV* cv2, const SV* sv2, const Ops& ops, bool libm, bool guard = true) {
  begin(N, cv2 ? "srf crv ops" : (sv2 ? "srf srf ops" : "srf ops"));
  W_srf(sv); if (cv2) W_crv(*cv2); if (sv2) W_srf(*sv2); W_ops(ops);
  bar();
  g_guard = guard;
  return emit([&] { run_surf_ops(sv, cv2, sv2, ops); }, libm);
}
// a surface that takes long to build (the osculating-surface analysis of an offset surface can loop for a long time) is refused
static bool build_survives(const SV& sv) {
  if (sv.kind != 10 && !sv.basis) return true;
  fflush(stdout); fflush(stderr);
  pid_t p = fork();
  if (p == 0) {
    signal(SIGSEGV, SIG_DFL); signal(SIGABRT, SIG_DFL); signal(SIGFPE, SIG_DFL); signal(SIGALRM, SIG_DFL);
    alarm(3);
    try { HS s = build_s(sv); } catch (Standard_Failure&) {}
    _exit(0);
  }
  int st = 0; waitpid(p, &st, 0);
  return WIFEXITED(st) && WEXITSTATUS(st) == 0;
}
static Ops plan_surf_raw(const SV& sv, const CV* cv2, const SV* sv2, int rounds, const std::function<void(const HS&, Ops&)>& choose);
static Ops plan_surf(const SV& sv, const CV* cv2, const SV* sv2, int rounds, const std::function<void(const HS&, Ops&)>& choose) {
  Ops r = plan_surf_raw(sv, cv2, sv2, rounds, choose);
  if (!std::getenv("GEOM_KEEP_STATES")) collapse_states(r);
  return r;
}
static Ops plan_surf_raw(const SV& sv, const CV* cv2, const SV* sv2, int rounds, const std::function<void(const HS&, Ops&)>& choose) {
  if (!build_survives(sv)) { g_plan_failed = true; return Ops(); }
  HG g;
  try { g = build_s(sv); } catch (Standard_Failure&) { return Ops{mk(1)}; }
  return plan(g, rounds, [&](const HG& live, Ops& add) { choose(as_s(live), add); },
              [&](HG& x, const Op& op) { return surf_op(x, op, cv2, sv2); },
              [&](HG& x, const Op& op) { return surf_unchecked(x, op); });
}

// =================================================================================
// random surfaces
static void sv_frame(SV& s) { rframe_vals(s.fr, true); }
static double rang_cone() {
  double r = rnd(0, 1);
  if (r < 0.4) return (M_PI / 12) * rint(1, 5) * (coin(0.3) ? -1 : 1);
  return nz(0.05, 1.45);
}
static SV rplane() {
  SV s; s.kind = 0; s.ctor = rint(0, 3); sv_frame(s);
  if (s.ctor == 3) { double x, y, z; rdir3(x, y, z); s.a = x; s.b = y; s.c = z; s.d = crd(); }
  return s;
}
static SV rcylinder() { SV s; s.kind = 1; s.ctor = rint(0, 1); sv_frame(s); s.a = rradius(true); return s; }
static SV rcone() { SV s; s.kind = 2; s.ctor = rint(0, 1); sv_frame(s); s.a = rang_cone(); s.b = rradius(true); return s; }
static SV rsphere() { SV s; s.kind = 3; s.ctor = rint(0, 1); sv_frame(s); s.a = rradius(true); return s; }
static SV rtorus() { SV s; s.kind = 4; s.ctor = rint(0, 1); sv_frame(s); s.b = rradius(true); s.a = coin(0.2) ? s.b * rnd(0, 1) : s.b + rnd(0.1, 5); return s; }
static void add_spt(SV& s, double x, double y, double z) { s.px.push_back(x); s.py.push_back(y); s.pz.push_back(z); }
static SV rbezs(int nmax = 3) {
  SV s; s.kind = 5;
  s.nu = rint(2, nmax); s.nv = rint(2, nmax);
  for (int i = 0; i < s.nu; ++i) for (int j = 0; j < s.nv; ++j) add_spt(s, crd(), crd(), crd());
  if (coin(0.15)) { for (int j = 1; j < s.nv; ++j) { s.px[(size_t)j] = s.px[0]; s.py[(size_t)j] = s.py[0]; s.pz[(size_t)j] = s.pz[0]; } }   // a collapsed row (singular point)
  s.rat = coin(0.3) ? 1 : 0;
  if (s.rat) { double r = rnd(0, 1); for (int i = 0; i < s.nu * s.nv; ++i) s.w.push_back(r < 0.2 ? 1.5 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3))); }
  return s;
}
// the knots, multiplicities and degree of one direction: returns false when the combination does not give 2 .. npmax poles
static bool rdirection(int& deg, int& per, Vd& knots, std::vector<int>& mults, int& np, int npmax, bool force_per, bool allow_per) {
  deg = rint(1, 3);
  per = force_per ? 1 : (allow_per && coin(0.25) ? 1 : 0);
  int nk = rint(2, 3);
  double k = nz(0.5, 2), step = coin(0.5) ? 0.5 * rint(1, 3) : rnd(0.2, 1.5);
  bool uniform = coin(0.3);
  int mode = rint(0, 5);
  knots.clear(); mults.clear();
  for (int i = 0; i < nk; ++i) {
    knots.push_back(k);
    k += uniform ? step : (coin(0.5) ? 0.5 * rint(1, 3) : rnd(0.2, 1.5));
    mults.push_back(mode <= 2 ? rint(1, deg) : (mode <= 4 ? 1 : deg));
  }
  if (per) mults[(size_t)nk - 1] = mults[0];
  else if (coin(0.88)) { mults[0] = deg + 1; mults[(size_t)nk - 1] = deg + 1; }
  else { mults[0] = rint(1, deg + 1); mults[(size_t)nk - 1] = rint(1, deg + 1); }
  NCollection_Array1<int> M(1, nk);
  for (int i = 0; i < nk; ++i) M(i + 1) = mults[(size_t)i];
  np = BSplCLib::NbPoles(deg, per != 0, M);
  return np >= 2 && np <= npmax && !(per && np <= deg);
}
static SV rbsps(int style = 0, int npmax = 5) {
  for (;;) {
    SV s; s.kind = 6;
    int np1, np2;
    if (!rdirection(s.udeg, s.uper, s.uk, s.um, np1, npmax, style == 1 || style == 3, style != 4)) continue;
    if (!rdirection(s.vdeg, s.vper, s.vk, s.vm, np2, npmax, style == 2 || style == 3, style != 4)) continue;
    if (np1 * np2 > 9) continue;
    s.nu = np1; s.nv = np2;
    for (int i = 0; i < np1 * np2; ++i) add_spt(s, crd(), crd(), crd());
    s.rat = coin(0.22) ? 1 : 0;
    if (s.rat) { double r = rnd(0, 1); for (int i = 0; i < np1 * np2; ++i) s.w.push_back(r < 0.2 ? 1.5 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.3, 3))); }
    return s;
  }
}
static CV rsmall_curve() {
  double r = rnd(0, 1);
  if (r < 0.2) return rline();
  if (r < 0.4) return rcircle(false);
  if (r < 0.5) return rellipse();
  if (r < 0.55) return rparabola();
  if (r < 0.75) return rbezier(true, 4);
  return rbspline(1, true, 3, 5);
}
static SV rext() {
  SV s; s.kind = 7; s.curve = std::make_shared<CV>(rsmall_curve());
  rdir3(s.dir[0], s.dir[1], s.dir[2]);
  if (coin(0.3)) { s.dir[0] = 0; s.dir[1] = 0; s.dir[2] = 1; }
  return s;
}
static SV rrev() {
  SV s; s.kind = 8; s.curve = std::make_shared<CV>(rsmall_curve());
  if (coin(0.4)) { s.dir[0] = 0; s.dir[1] = 0; s.dir[2] = 0; s.dir[3] = 0; s.dir[4] = 0; s.dir[5] = 1; }
  else { s.dir[0] = crd(); s.dir[1] = crd(); s.dir[2] = crd(); rdir3(s.dir[3], s.dir[4], s.dir[5]); }
  return s;
}
// a plane curve revolved about an axis that touches it: the singular points of a revolution
static SV rrev_singular() {
  SV s; s.kind = 8;
  CV c;
  double r = rnd(0, 1);
  if (r < 0.35) { c = rcircle(false); c.ctor = 1; c.v[0] = 0; c.v[1] = 0; c.v[2] = 0; c.v[3] = 0; c.v[4] = 1; c.v[5] = 0; c.v[6] = 1; c.v[7] = 0; c.v[8] = 0; c.a = rradius(false); }   // a circle in the xz plane through the z axis
  else if (r < 0.6) { c = rline(); c.v[0] = 0; c.v[1] = 0; c.v[2] = crd(); rdir3(c.v[3], c.v[4], c.v[5]); c.v[4] = 0; }                                       // a line meeting the z axis
  else { c = rbezier(false, 4); c.py.assign(c.py.size(), 0.0); c.px[0] = 0; c.py[0] = 0; if (coin(0.5)) c.px.back() = 0; }
  s.curve = std::make_shared<CV>(c);
  s.dir[0] = 0; s.dir[1] = 0; s.dir[2] = 0; s.dir[3] = 0; s.dir[4] = 0; s.dir[5] = 1;
  return s;
}
static SV rbasic_surf() {
  double r = rnd(0, 1);
  if (r < 0.10) return rplane();
  if (r < 0.20) return rcylinder();
  if (r < 0.30) return rcone();
  if (r < 0.40) return rsphere();
  if (r < 0.50) return rtorus();
  if (r < 0.64) return rbezs(3);
  if (r < 0.80) return rbsps(0, 4);
  if (r < 0.90) return rext();
  return rrev();
}
static SV mk_rts(const SV& basis) {
  SV s; s.kind = 9; s.basis = std::make_shared<SV>(basis);
  HS b; try { b = build_s(basis); } catch (Standard_Failure&) {}
  double u1 = 0, u2 = 1, v1 = 0, v2 = 1;
  if (!b.IsNull()) b->Bounds(u1, u2, v1, v2);
  if (u1 < -1e50) { u1 = -3; u2 = 3; }
  if (v1 < -1e50) { v1 = -3; v2 = 3; }
  if (b.IsNull() ? false : b->IsUPeriodic()) { u2 = u1 + b->UPeriod(); }
  s.ctor = coin(0.7) ? 0 : 1;
  double su = u2 - u1, sv = v2 - v1;
  if (s.ctor == 0) {
    double a1 = u1 + rnd(0, 0.5) * su, a2 = a1 + rnd(0.1, 0.5) * su, b1 = v1 + rnd(0, 0.5) * sv, b2 = b1 + rnd(0.1, 0.5) * sv;
    if (!b.IsNull() && b->IsUPeriodic() && coin(0.4)) { a1 = u1 + rnd(-1, 2) * su; a2 = a1 + rnd(0.1, 1.5) * su; }
    if (!b.IsNull() && b->IsVPeriodic() && coin(0.4)) { b1 = v1 + rnd(-1, 2) * sv; b2 = b1 + rnd(0.1, 1.5) * sv; }
    if (coin(0.1)) a2 = a1;
    if (coin(0.1)) std::swap(b1, b2);
    s.t[0] = a1; s.t[1] = a2; s.t[2] = b1; s.t[3] = b2; s.s1 = coin(0.8); s.s2 = coin(0.8);
  } else {
    s.utrim = coin(0.5);
    double lo = s.utrim ? u1 : v1, sp = s.utrim ? su : sv;
    double p1 = lo + rnd(0, 0.5) * sp, p2 = p1 + rnd(0.1, 0.5) * sp;
    if (coin(0.15)) { p1 = lo + sp * 0.2; p2 = lo + sp * 0.2; }
    s.t[0] = p1; s.t[1] = p2; s.s1 = coin(0.8);
  }
  return s;
}
static SV mk_offs(const SV& basis, double off, bool notc) {
  SV s; s.kind = 10; s.basis = std::make_shared<SV>(basis); s.off = off; s.notc = notc; return s;
}
static double roffs() {
  double r = rnd(0, 1);
  if (r < 0.1) return 0.0;
  if (r < 0.5) return 0.25 * rint(-8, 8);
  return rnd(-2, 2);
}
static SV roffbasis() {
  double r = rnd(0, 1);
  if (r < 0.10) return rplane();
  if (r < 0.22) return rcylinder();
  if (r < 0.34) return rcone();
  if (r < 0.46) return rsphere();
  if (r < 0.54) return rtorus();
  if (r < 0.64) return rbezs(3);
  if (r < 0.76) return rbsps(0, 4);
  if (r < 0.82) return rext();
  if (r < 0.90) return rrev();
  if (r < 0.95) return rrev_singular();
  return mk_rts(rbasic_surf());
}
static SV rany_surf() {
  double r = rnd(0, 1);
  SV b = rbasic_surf();
  if (r < 0.72) return b;
  if (r < 0.86) return mk_rts(b);
  return mk_offs(roffbasis(), roffs(), coin(0.3));
}
static SV rsurf_kind(int which) {
  switch (which) {
    case 0: return rplane();
    case 1: return rcylinder();
    case 2: return rcone();
    case 3: return rsphere();
    case 4: return rtorus();
    case 5: return rbezs(3);
    case 6: return rbsps(0, 4);
    case 7: return rext();
    default: return rrev();
  }
}
// a parameter pair near the interesting places of a built surface
static void rsparam(const HS& s, double& u, double& v, double outside = 0.3) {
  double u1, u2, v1, v2; s->Bounds(u1, u2, v1, v2);
  auto one = [&](double f, double l, bool inf) {
    if (inf) return coin(0.2) ? 0.0 : (coin(0.3) ? 0.5 * rint(-6, 6) : rnd(-3, 3));
    double sp = l - f, r = rnd(0, 1);
    if (r < 0.1) return f;
    if (r < 0.2) return l;
    if (r < 0.78) return f + rnd(0, 1) * sp;
    if (r < 0.92) return f + rnd(-outside, 1 + outside) * sp;
    return f + rnd(-1.5, 2.5) * sp;
  };
  u = one(u1, u2, u1 < -1e50 || u2 > 1e50);
  v = one(v1, v2, v1 < -1e50 || v2 > 1e50);
  if (dynamic_cast<Geom_SphericalSurface*>(s.get()) && coin(0.25)) v = coin(0.5) ? M_PI / 2 : -M_PI / 2;
  if (auto* c = dynamic_cast<Geom_ConicalSurface*>(s.get())) if (coin(0.2)) v = -c->RefRadius() / std::sin(c->SemiAngle());
  if (auto* b = dynamic_cast<Geom_BSplineSurface*>(s.get())) {
    if (coin(0.2)) u = b->UKnot(rint(1, b->NbUKnots()));
    if (coin(0.2)) v = b->VKnot(rint(1, b->NbVKnots()));
  }
}
static double rspar_u(const HS& s) { double u, v; rsparam(s, u, v); return u; }
static double rspar_v(const HS& s) { double u, v; rsparam(s, u, v); return v; }

// =================================================================================
// families: surfaces
#define SF(NAME, TARGET, MAXLEN) LOOP(NAME, TARGET) if (false) break; else
static Op sop_ev(int code, const HS& s, int nu = 0, int nv = 0) {
  double u, v; rsparam(s, u, v);
  Vd a{u, v};
  if (code == SU_DN) { if (nu == 0 && nv == 0 && !coin(0.08)) nu = 1 + (int)(rnd(0, 1) * 2); a.push_back(nu); a.push_back(nv); }
  return mk(code, a);
}
static void add_sevals(Ops& ops, const HS& s, int n, int maxdn = 3) {
  for (int i = 0; i < n; ++i) {
    int r = rint(0, 5);
    if (r == 0) ops.push_back(sop_ev(SU_D0, s));
    else if (r == 1) ops.push_back(sop_ev(SU_D1, s));
    else if (r == 2) ops.push_back(sop_ev(SU_D2, s));
    else if (r == 3) ops.push_back(sop_ev(SU_D3, s));
    else if (r == 4) ops.push_back(sop_ev(SU_DN, s, rint(0, maxdn), rint(0, maxdn)));
    else ops.push_back(sop_ev(SU_VALUE, s));
  }
}
static Geom_BSplineSurface* SBS(const HS& s) { return dynamic_cast<Geom_BSplineSurface*>(s.get()); }
static Geom_BezierSurface* SBZ(const HS& s) { return dynamic_cast<Geom_BezierSurface*>(s.get()); }
static void add_row(Vd& a, int n) { a.push_back(n); for (int i = 0; i < n; ++i) { a.push_back(crd()); a.push_back(crd()); a.push_back(crd()); } }
static void add_wts(Vd& a, int n) { a.push_back(n); for (int i = 0; i < n; ++i) a.push_back(coin(0.5) ? 1.0 : rweight()); }

static void fam_surface_common() {
  { SF("surface_params", 28, 1100) {
      SV sv = rany_surf();
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        add.push_back(mk(SU_BOUNDS)); add.push_back(mk(SU_IS_UCLOSED)); add.push_back(mk(SU_IS_VCLOSED)); add.push_back(mk(SU_IS_UPER)); add.push_back(mk(SU_IS_VPER));
        add.push_back(mk(SU_CONT));
        for (int k = rint(1, 2); k > 0; --k) { add.push_back(mk(SU_ISCNU, Vd{(double)(coin(0.06) ? -1 : rint(0, 4))})); add.push_back(mk(SU_ISCNV, Vd{(double)(coin(0.06) ? -1 : rint(0, 4))})); }
        if (s->IsUPeriodic() || coin(0.15)) add.push_back(mk(SU_UPERIOD));
        if (s->IsVPeriodic() || coin(0.15)) add.push_back(mk(SU_VPERIOD));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  const struct { const char* n; int mode; int cnt; } ev[] = {{"surface_d01", 0, 3}, {"surface_d2", 1, 2}, {"surface_d3dn", 2, 2}};
  for (auto& e : ev) {
    SF(e.n, 28, 1500) {
      SV sv = coin(0.12) ? mk_rts(rbasic_surf()) : rsurf_kind(rint(0, 8));
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        for (int k = 0; k < e.cnt; ++k) {
          if (e.mode == 0) { int r = rint(0, 2); add.push_back(sop_ev(r == 0 ? SU_D0 : (r == 1 ? SU_D1 : SU_VALUE), s)); }
          else if (e.mode == 1) add.push_back(sop_ev(SU_D2, s));
          else add.push_back(coin(0.5) ? sop_ev(SU_D3, s) : sop_ev(SU_DN, s, coin(0.05) ? -1 : rint(0, 3), rint(0, 3)));
        }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    }
  }
  { SF("surface_reverse", 28, 1500) {
      SV sv = rany_surf();
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        add.push_back(mk(SU_UREV_PARAM, Vd{rspar_u(s)})); add.push_back(mk(SU_VREV_PARAM, Vd{rspar_v(s)}));
        add.push_back(mk(SU_UREVERSED)); add.push_back(mk(SU_VREVERSE)); add.push_back(mk(1));
        add_sevals(add, s, 1);
        if (coin(0.4)) { add.push_back(mk(SU_UREVERSE)); add.push_back(mk(SU_IS_UCLOSED)); }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("surface_xform", 32, 1500) {
      SV sv = rany_surf();
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        Tr t = rtr(true); double u, v; rsparam(s, u, v);
        Vd a{u, v}; tr_args(t, a);
        add.push_back(mk(SU_TRANSFORM_PARAMS, a));
        Vd b; tr_args(t, b); add.push_back(mk(SU_PARAM_TRANSFORM, b));
        add.push_back(mk(4, b)); add.push_back(mk(1));
        add_sevals(add, s, 1); add.push_back(mk(SU_BOUNDS));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("surface_geom_ops", 32, 1500) {
      SV sv = rany_surf();
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        int r = rint(0, 3);
        if (r == 0) { add.push_back(mk(37)); add.push_back(mk(1)); }
        else if (r == 1) { add.push_back(op_tr(21, rtr(true))); add.push_back(mk(1)); }
        else { add.push_back(rgeom_op(true)); add.push_back(mk(1)); }
        if (coin(0.4)) add_sevals(add, s, 1);
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("surface_iso", 28, 1500) {
      SV sv = coin(0.25) ? mk_rts(rbasic_surf()) : rsurf_kind(rint(0, 8));
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        for (int k = 0; k < 2; ++k) { add.push_back(mk(coin(0.5) ? SU_UISO : SU_VISO, Vd{coin(0.5) ? rspar_u(s) : rspar_v(s)})); }
        if (SBS(s)) add.push_back(mk(coin(0.5) ? BS_UISO_C : BS_VISO_C, Vd{rspar_u(s), (double)rint(0, 1)}));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
}

static void fam_elementary() {
  auto fvals = [&](Vd& a) { double fr[10]; rframe_vals(fr, true); for (int i = 0; i < 10; ++i) a.push_back(fr[i]); };
  { SF("plane_ops", 28, 1500) {
      SV sv = rplane();
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Vd a; int r = rint(0, 6);
        if (r >= 5) {
          double dx, dy, dz; rdir3(dx, dy, dz);
          if (r == 5) add.push_back(mk(EL_SET_AXIS, Vd{crd(), crd(), crd(), dx, dy, dz})); else { fvals(a); add.push_back(mk(EL_SET_POS, a)); }
          add.push_back(mk(1)); add.push_back(mk(EL_POSITION));
          return;
        }
        if (r == 0) { fvals(a); add.push_back(mk(PL_SET_PLN, a)); add.push_back(mk(1)); }
        else if (r == 1) { add.push_back(mk(PL_PLN)); add.push_back(mk(PL_COEFFS)); }
        else if (r == 2) { add.push_back(mk(SU_UREVERSE)); add.push_back(mk(SU_VREVERSED)); add.push_back(mk(1)); }
        else if (r == 3) { add_sevals(add, s, 2, 2); }
        else { add.push_back(mk(SU_BOUNDS)); add.push_back(mk(SU_UISO, Vd{crd()})); add.push_back(mk(SU_VISO, Vd{crd()})); }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  auto cyl = [&](int kind, const char* name) {
    SF(name, 28, 1500) {
      SV sv = rsurf_kind(kind);
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Vd a; int r = rint(0, 11);
        if (r >= 10) {
          double dx, dy, dz; rdir3(dx, dy, dz); int q = rint(0, 3);
          if (q == 0) add.push_back(mk(EL_SET_AXIS, Vd{crd(), crd(), crd(), dx, dy, dz}));
          else if (q == 1) { fvals(a); add.push_back(mk(EL_SET_POS, a)); }
          else if (q == 2) add.push_back(mk(EL_SET_LOC, Vd{crd(), crd(), crd()}));
          else { add.push_back(mk(SU_UREVERSE)); add.push_back(mk(SU_VREVERSE)); }
          add.push_back(mk(1)); add.push_back(mk(EL_AXIS));
          return;
        }
        switch (kind) {
          case 1:
            if (r == 0) add.push_back(mk(CY_SET_RADIUS, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) { fvals(a); a.push_back(rradius(true)); add.push_back(mk(CY_SET_CYL, a)); }
            else if (r == 2) { add.push_back(mk(CY_CYL)); add.push_back(mk(CY_RADIUS)); }
            else if (r == 3) add.push_back(mk(CY_COEFFS));
            break;
          case 2:
            if (r == 0) add.push_back(mk(CO_SET_RADIUS, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) add.push_back(mk(CO_SET_SEMI, Vd{coin(0.15) ? (coin(0.5) ? 0.0 : 1.6) : rang_cone()}));
            else if (r == 2) { fvals(a); a.push_back(rang_cone()); a.push_back(rradius(true)); add.push_back(mk(CO_SET_CONE, a)); }
            else if (r == 3) { add.push_back(mk(CO_CONE)); add.push_back(mk(CO_APEX)); }
            else if (r == 4) { add.push_back(mk(CO_COEFFS)); add.push_back(mk(CO_REFRADIUS)); add.push_back(mk(CO_SEMIANGLE)); }
            break;
          case 3:
            if (r == 0) add.push_back(mk(SP_SET_RADIUS, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) { fvals(a); a.push_back(rradius(true)); add.push_back(mk(SP_SET_SPHERE, a)); }
            else if (r == 2) { add.push_back(mk(SP_SPHERE)); add.push_back(mk(SP_RADIUS)); }
            else if (r == 3) { add.push_back(mk(SP_AREA)); add.push_back(mk(SP_VOLUME)); }
            else if (r == 4) add.push_back(mk(SP_COEFFS));
            break;
          default:
            if (r == 0) add.push_back(mk(TO_SET_MAJOR, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 1) add.push_back(mk(TO_SET_MINOR, Vd{coin(0.12) ? -rnd(0.1, 2) : rradius(true)}));
            else if (r == 2) { fvals(a); a.push_back(rradius(true)); a.push_back(rradius(true)); add.push_back(mk(TO_SET_TORUS, a)); }
            else if (r == 3) { add.push_back(mk(TO_TORUS)); add.push_back(mk(TO_MAJOR)); add.push_back(mk(TO_MINOR)); }
            else if (r == 4) { add.push_back(mk(TO_AREA)); add.push_back(mk(TO_VOLUME)); }
            else if (r == 5) add.push_back(mk(TO_COEFFS));
            break;
        }
        add.push_back(mk(1));
        if (r >= 8) add_sevals(add, s, 2, 2);
      });
      emit_surf(name, sv, nullptr, nullptr, ops, true);
    }
  };
  cyl(1, "cylinder_ops"); cyl(2, "cone_ops"); cyl(3, "sphere_ops"); cyl(4, "torus_ops");
  { SF("elementary_build", 40, 1500) {
      int kind = rint(0, 4);
      SV sv = rsurf_kind(kind);
      // invalid variants: negative radii, a cone angle outside (0, pi / 2), a null normal
      if (coin(0.3)) {
        switch (kind) {
          case 1: sv.a = -rnd(0.1, 2); break;
          case 2: if (coin(0.5)) sv.b = -rnd(0.1, 2); else sv.a = coin(0.5) ? 0.0 : (coin(0.5) ? 1.6 : -1.7); break;
          case 3: sv.a = -rnd(0.1, 2); break;
          case 4: if (coin(0.5)) sv.a = -rnd(0.1, 2); else sv.b = -rnd(0.1, 2); break;
          default: if (sv.ctor == 2) { sv.fr[3] = 0; sv.fr[4] = 0; sv.fr[5] = 0; } break;
        }
      }
      Ops ops;
      ops.push_back(mk(1)); ops.push_back(mk(EL_POSITION)); ops.push_back(mk(EL_AXIS)); ops.push_back(mk(EL_LOCATION)); ops.push_back(mk(SU_BOUNDS)); ops.push_back(mk(SU_CONT));
      ops.push_back(mk(SU_IS_UCLOSED)); ops.push_back(mk(SU_IS_VCLOSED)); ops.push_back(mk(SU_IS_UPER)); ops.push_back(mk(SU_IS_VPER));
      ops.push_back(mk(SU_ISCNU, Vd{5.0})); ops.push_back(mk(SU_ISCNV, Vd{2.0}));
      if (kind == 0) { ops.push_back(mk(PL_PLN)); ops.push_back(mk(PL_COEFFS)); }
      if (kind == 1) { ops.push_back(mk(CY_CYL)); ops.push_back(mk(CY_COEFFS)); }
      if (kind == 2) { ops.push_back(mk(CO_CONE)); ops.push_back(mk(CO_APEX)); }
      if (kind == 3) { ops.push_back(mk(SP_SPHERE)); ops.push_back(mk(SP_AREA)); ops.push_back(mk(SP_VOLUME)); }
      if (kind == 4) { ops.push_back(mk(TO_TORUS)); ops.push_back(mk(TO_AREA)); ops.push_back(mk(TO_VOLUME)); }
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  }

static void fam_swept() {
  { SF("swept_build", 40, 1500) {
      SV sv = coin(0.5) ? rext() : rrev();
      Ops ops;
      ops.push_back(mk(1)); ops.push_back(mk(SW_DIRECTION)); ops.push_back(mk(SW_BASIS)); ops.push_back(mk(SU_BOUNDS)); ops.push_back(mk(SU_CONT));
      ops.push_back(mk(SU_IS_UCLOSED)); ops.push_back(mk(SU_IS_VCLOSED)); ops.push_back(mk(SU_IS_UPER)); ops.push_back(mk(SU_IS_VPER));
      ops.push_back(mk(SU_ISCNU, Vd{(double)(coin(0.1) ? -1 : rint(0, 3))})); ops.push_back(mk(SU_ISCNV, Vd{(double)rint(0, 3)}));
      if (sv.kind == 8) { ops.push_back(mk(RV_AXIS)); ops.push_back(mk(RV_LOCATION)); ops.push_back(mk(RV_REFPLANE)); }
      ops.push_back(mk(RS_HAS_EREP));
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("extrusion_ops", 32, 1600) {
      SV sv = rext(); CV c2 = rsmall_curve();
      Ops ops = plan_surf(sv, &c2, nullptr, 3, [&](const HS& s, Ops& add) {
        double dx, dy, dz; rdir3(dx, dy, dz); int r = rint(0, 4);
        if (r == 0) { add.push_back(mk(SW_SET_DIR, Vd{dx, dy, dz})); add.push_back(mk(1)); }
        else if (r == 1) { add.push_back(mk(SW_SET_BASIS)); add.push_back(mk(1)); }
        else if (r == 2) { add.push_back(mk(SU_UREVERSE)); add.push_back(mk(SU_VREVERSE)); add.push_back(mk(1)); }
        else add_sevals(add, s, 3, 3);
      });
      emit_surf(N, sv, &c2, nullptr, ops, true);
    } }
  { SF("revolution_ops", 32, 1600) {
      SV sv = rrev(); CV c2 = rsmall_curve();
      Ops ops = plan_surf(sv, &c2, nullptr, 3, [&](const HS& s, Ops& add) {
        double dx, dy, dz; rdir3(dx, dy, dz); int r = rint(0, 7);
        if (r == 0) { add.push_back(mk(RV_SET_AXIS, Vd{crd(), crd(), crd(), dx, dy, dz})); add.push_back(mk(1)); }
        else if (r == 1) { add.push_back(mk(SW_SET_DIR, Vd{dx, dy, dz})); add.push_back(mk(1)); }
        else if (r == 2) { add.push_back(mk(RV_SET_LOC, Vd{crd(), crd(), crd()})); add.push_back(mk(1)); }
        else if (r == 3) { add.push_back(mk(SW_SET_BASIS)); add.push_back(mk(1)); }
        else if (r == 4) { add.push_back(mk(RV_AXIS)); add.push_back(mk(RV_LOCATION)); add.push_back(mk(RV_REFPLANE)); }
        else add_sevals(add, s, 3, 3);
      });
      emit_surf(N, sv, &c2, nullptr, ops, true);
    } }
  { SF("revolution_singular", 36, 1500) {
      SV sv = rrev_singular();
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        // the parameters where the curve touches the axis
        for (int k = 0; k < 4; ++k) {
          double u = rspar_u(s), v;
          double f, l, v1, v2; s->Bounds(f, l, v1, v2);
          v = coin(0.5) ? v1 : (coin(0.5) ? v2 : rspar_v(s));
          int r = rint(0, 3);
          Vd a{u, v};
          if (r == 0) add.push_back(mk(SU_D0, a)); else if (r == 1) add.push_back(mk(SU_D1, a)); else if (r == 2) add.push_back(mk(SU_D2, a));
          else { a.push_back(rint(0, 2)); a.push_back(rint(0, 2)); add.push_back(mk(SU_DN, a)); }
        }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
}

// Bezier surfaces
static void fam_bezsurf() {
  { SF("bezsurf_build", 40, 1600) {
      SV sv = rbezs(5);
      if (coin(0.3)) {
        switch (rint(0, 3)) {
          case 0: sv.nu = 1; sv.px.resize((size_t)sv.nv); sv.py.resize((size_t)sv.nv); sv.pz.resize((size_t)sv.nv); sv.rat = 0; sv.w.clear(); break;
          case 1: if (!sv.rat) { sv.rat = 1; for (int i = 0; i < sv.nu * sv.nv; ++i) sv.w.push_back(rweight()); } sv.w[(size_t)rint(0, (int)sv.w.size() - 1)] = coin(0.5) ? 0.0 : -1.0; break;
          case 2: if (!sv.rat) { sv.rat = 1; for (int i = 0; i < sv.nu * sv.nv; ++i) sv.w.push_back(rweight()); } sv.w.pop_back(); break;
          default: sv.nu = 27; sv.nv = 2; sv.px.clear(); sv.py.clear(); sv.pz.clear(); sv.rat = 0; sv.w.clear(); for (int i = 0; i < 54; ++i) add_spt(sv, 0.1 * i, 0, 0); break;
        }
      }
      Ops ops;
      ops.push_back(mk(1)); ops.push_back(mk(BB_NBU)); ops.push_back(mk(BB_NBV)); ops.push_back(mk(SB_POLES)); ops.push_back(mk(SB_WEIGHTS));
      ops.push_back(mk(BB_UKNOTS)); ops.push_back(mk(BB_VMULTS)); ops.push_back(mk(BB_UKNOTSEQ)); ops.push_back(mk(SU_BOUNDS)); ops.push_back(mk(SU_CONT));
      ops.push_back(mk(SU_IS_UCLOSED)); ops.push_back(mk(SU_IS_VCLOSED)); ops.push_back(mk(SU_IS_UPER)); ops.push_back(mk(SU_ISCNU, Vd{4.0})); ops.push_back(mk(BZ_MAXDEGREE));
      int nu = std::max(sv.nu, 1), nv = std::max(sv.nv, 1);
      ops.push_back(mk(SB_POLE, Vd{(double)(coin(0.1) ? 0 : rint(1, nu)), (double)(coin(0.1) ? nv + 1 : rint(1, nv))}));
      ops.push_back(mk(SB_WEIGHT, Vd{(double)rint(1, nu), (double)rint(1, nv)}));
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bezsurf_eval", 36, 1500) {
      SV sv = rbezs(4);
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        add_sevals(add, s, 5, 4);
        add.push_back(mk(SU_UISO, Vd{rspar_u(s)})); add.push_back(mk(SU_VISO, Vd{rspar_v(s)}));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bezsurf_increase_segment", 36, 1600) {
      SV sv = rbezs(3);
      Ops ops = plan_surf(sv, nullptr, nullptr, 2, [&](const HS& s, Ops& add) {
        Geom_BezierSurface* b = SBZ(s);
        if (coin(0.5)) {
          int du = b->UDegree(), dv = b->VDegree();
          add.push_back(mk(BZ_INCREASE, Vd{(double)(coin(0.1) ? du - 1 : du + rint(0, 2)), (double)(coin(0.1) ? 26 : dv + rint(0, 2))}));
        } else {
          double u1 = coin(0.3) ? 0.25 * rint(0, 4) : rnd(-0.2, 1.1), u2 = coin(0.3) ? 0.25 * rint(0, 4) : rnd(-0.2, 1.2), v1 = rnd(0, 0.8), v2 = v1 + rnd(0.05, 0.4);
          if (coin(0.8) && u1 > u2) std::swap(u1, u2);
          add.push_back(mk(BZ_SEGMENT, Vd{u1, u2, v1, v2}));
        }
        add.push_back(mk(1)); add_sevals(add, s, 1, 2);
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bezsurf_insert_remove", 40, 1600) {
      SV sv = rbezs(3);
      Ops ops = plan_surf(sv, nullptr, nullptr, 2, [&](const HS& s, Ops& add) {
        Geom_BezierSurface* b = SBZ(s); int nu = b->NbUPoles(), nv = b->NbVPoles();
        int r = rint(0, 5);
        if (r >= 4) { add.push_back(mk(r == 4 ? BZ_REMOVE_COL : BZ_REMOVE_ROW, Vd{(double)(coin(0.1) ? 0 : rint(1, r == 4 ? nv : nu))})); add.push_back(mk(1)); return; }
        bool col = r < 2, after = (r & 1) == 0;
        int n = col ? nu : nv; if (coin(0.1)) n += 1;
        int idx = after ? rint(0, col ? nv : nu) : rint(1, (col ? nv : nu) + 1);
        Vd a{(double)idx}; add_row(a, n);
        bool hw = b->IsURational() || b->IsVRational() || coin(0.3);
        a.push_back(hw ? 1 : 0); if (hw) add_wts(a, n);
        add.push_back(mk(col ? (after ? BZ_INS_COL_AFTER : BZ_INS_COL_BEFORE) : (after ? BZ_INS_ROW_AFTER : BZ_INS_ROW_BEFORE), a));
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bezsurf_set_pole", 40, 1600) {
      SV sv = rbezs(3);
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Geom_BezierSurface* b = SBZ(s); int nu = b->NbUPoles(), nv = b->NbVPoles();
        int i = coin(0.06) ? 0 : rint(1, nu), j = coin(0.06) ? nv + 1 : rint(1, nv);
        double w = coin(0.1) ? (coin(0.5) ? 0.0 : -2.0) : (coin(0.25) ? 1.0 : rweight());
        int r = rint(0, 8);
        if (r == 7) { add.push_back(mk(BZ_EXCHANGE)); add.push_back(mk(1)); return; }
        if (r == 8) { add.push_back(mk(BZ_RESOLUTION, Vd{coin(0.3) ? 1e-7 : rnd(0.01, 2)})); add.push_back(mk(BZ_SET_POLE, Vd{1.0, 1.0, crd(), crd(), crd()})); add.push_back(mk(BZ_RESOLUTION, Vd{1e-3})); return; }
        if (r == 0) add.push_back(mk(BZ_SET_POLE, Vd{(double)i, (double)j, crd(), crd(), crd()}));
        else if (r == 1) add.push_back(mk(BZ_SET_POLE_W, Vd{(double)i, (double)j, crd(), crd(), crd(), w}));
        else if (r == 2) add.push_back(mk(BZ_SET_WEIGHT, Vd{(double)i, (double)j, w}));
        else if (r == 3 || r == 4) {
          bool col = r == 3; int n = col ? nu : nv; if (coin(0.08)) n += 1;
          Vd a{(double)(col ? j : i)}; add_row(a, n);
          bool hw = coin(0.5); a.push_back(hw ? 1 : 0); if (hw) add_wts(a, n);
          add.push_back(mk(col ? BZ_SET_POLE_COL : BZ_SET_POLE_ROW, a));
        } else {
          bool col = r == 5; int n = col ? nu : nv; if (coin(0.08)) n -= 1;
          Vd a{(double)(col ? j : i)}; add_wts(a, std::max(n, 0));
          if (coin(0.1)) a[1] = 0.0;
          add.push_back(mk(col ? BZ_SET_WEIGHT_COL : BZ_SET_WEIGHT_ROW, a));
        }
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  }

// B-spline surfaces
static double rsk(const HS& s, bool u) {
  Geom_BSplineSurface* b = SBS(s);
  double u1, u2, v1, v2; b->Bounds(u1, u2, v1, v2);
  double f = u ? u1 : v1, l = u ? u2 : v2, r = rnd(0, 1);
  if (r < 0.2) return u ? b->UKnot(rint(1, b->NbUKnots())) : b->VKnot(rint(1, b->NbVKnots()));
  if (r < 0.28) return f;
  if (r < 0.36) return l;
  return f + rnd(-0.1, 1.1) * (l - f);
}
static Vd knots_args(const HS& s, bool u, int n, double outside_prob) {
  Geom_BSplineSurface* b = SBS(s);
  Vd us;
  for (int i = 0; i < n; ++i) { double k = rsk(s, u); if (coin(outside_prob)) k += 0.3; us.push_back(k); }
  std::sort(us.begin(), us.end());
  int d = u ? b->UDegree() : b->VDegree();
  Vd a{(double)n}; for (double k : us) a.push_back(k);
  a.push_back((double)n); for (int i = 0; i < n; ++i) a.push_back(rint(1, std::max(1, d - 1)));
  return a;
}
static SV rbss() { return coin(0.15) ? rbsps(coin(0.5) ? 1 : 2, 4) : rbsps(0, 4); }
static void fam_bsplsurf() {
  { SF("bsplsurf_build", 44, 1600) {
      SV sv = rbss();
      if (coin(0.25)) {
        switch (rint(0, 7)) {
          case 0: sv.udeg = 0; break;
          case 1: sv.vdeg = 26; break;
          case 2: sv.nu += 1; for (int j = 0; j < sv.nv; ++j) { add_spt(sv, crd(), crd(), crd()); if (sv.rat) sv.w.push_back(1.0); } break;
          case 3: sv.um.pop_back(); break;
          case 4: if (sv.uk.size() >= 2) sv.uk[1] = sv.uk[0]; break;
          case 5: if (sv.vk.size() >= 2) std::swap(sv.vk[0], sv.vk[1]); break;
          case 6: if (!sv.rat) { sv.rat = 1; for (size_t i = 0; i < sv.px.size(); ++i) sv.w.push_back(rweight()); } sv.w[(size_t)rint(0, (int)sv.w.size() - 1)] = coin(0.5) ? 0.0 : -0.5; break;
          default: sv.vm[0] += 1; break;
        }
      }
      Ops ops;
      ops.push_back(mk(1)); ops.push_back(mk(BB_NBU)); ops.push_back(mk(BB_NBV)); ops.push_back(mk(BB_UKNOTSEQ)); ops.push_back(mk(BB_VKNOTSEQ));
      ops.push_back(mk(SB_WEIGHTS)); ops.push_back(mk(BB_UMULTS)); ops.push_back(mk(BB_VKNOTS)); ops.push_back(mk(BB_UKNOTS)); ops.push_back(mk(BB_VMULTS));
      ops.push_back(mk(BS_FIRST_UKNOT)); ops.push_back(mk(BS_LAST_UKNOT)); ops.push_back(mk(BS_FIRST_VKNOT)); ops.push_back(mk(BS_LAST_VKNOT));
      ops.push_back(mk(SU_BOUNDS)); ops.push_back(mk(SU_CONT)); ops.push_back(mk(SU_IS_UCLOSED)); ops.push_back(mk(SU_IS_VCLOSED));
      ops.push_back(mk(SU_IS_UPER)); ops.push_back(mk(SU_IS_VPER)); ops.push_back(mk(BS_MAXDEGREE));
      int nu = std::max(sv.nu, 1), nv = std::max(sv.nv, 1);
      ops.push_back(mk(BS_UKNOT, Vd{(double)(coin(0.1) ? (int)sv.uk.size() + 1 : rint(1, std::max((int)sv.uk.size(), 1)))}));
      ops.push_back(mk(BS_VMULT, Vd{(double)(coin(0.1) ? 0 : rint(1, std::max((int)sv.vk.size(), 1)))}));
      ops.push_back(mk(SB_POLE, Vd{(double)rint(1, nu), (double)(coin(0.1) ? nv + 1 : rint(1, nv))}));
      ops.push_back(mk(SB_WEIGHT, Vd{(double)(coin(0.1) ? 0 : rint(1, nu)), (double)rint(1, nv)}));
      for (int k = 0; k < 2; ++k) { ops.push_back(mk(SU_ISCNU, Vd{(double)k})); ops.push_back(mk(SU_ISCNV, Vd{(double)(k + 1)})); }
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_eval", 40, 1600) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        for (int k = 0; k < 5; ++k) {
          double u = rsk(s, true), v = rsk(s, false); int r = rint(0, 4);
          if (r == 0) add.push_back(mk(SU_D0, Vd{u, v})); else if (r == 1) add.push_back(mk(SU_D1, Vd{u, v})); else if (r == 2) add.push_back(mk(SU_D2, Vd{u, v}));
          else if (r == 3) add.push_back(mk(SU_D3, Vd{u, v})); else add.push_back(mk(SU_DN, Vd{u, v, (double)rint(0, SBS(s)->UDegree() + 1), (double)rint(0, SBS(s)->VDegree() + 1)}));
        }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_increase", 40, 1700) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 2, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s);
        if (coin(0.4)) {
          add.push_back(mk(BS_INCREASE, Vd{(double)(coin(0.1) ? b->UDegree() - 1 : b->UDegree() + rint(0, 2)), (double)(coin(0.05) ? 26 : b->VDegree() + rint(0, 2))}));
          add.push_back(mk(1)); add_sevals(add, s, 1, 1);
          return;
        }
        bool u = coin(0.5);
        int nk = u ? b->NbUKnots() : b->NbVKnots(), d = u ? b->UDegree() : b->VDegree();
        int i1 = rint(1, nk), i2 = rint(i1, nk), r = rint(0, 2);
        if (coin(0.04)) { i1 = nk + 1; i2 = nk + 2; }
        int mm = u ? b->UMultiplicity(std::min(i1, nk)) : b->VMultiplicity(std::min(i1, nk));
        int m = r == 2 ? rint(1, 2) : std::min(d + 1, mm + rint(0, 2));
        if (r == 0) add.push_back(mk(u ? BS_INC_UMULT : BS_INC_VMULT, Vd{(double)i1, (double)m}));
        else if (r == 1) add.push_back(mk(u ? BS_INC_UMULT_R : BS_INC_VMULT_R, Vd{(double)i1, (double)i2, (double)m}));
        else add.push_back(mk(u ? BS_INCR_UMULT : BS_INCR_VMULT, Vd{(double)i1, (double)i2, (double)m}));
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_knots", 44, 1700) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 2, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s);
        bool u = coin(0.5); Vd a{rtol(), (double)rint(0, 1)};
        int r = rint(0, 3);
        if (r == 0) {
          int d = u ? b->UDegree() : b->VDegree();
          add.push_back(mk(u ? BS_INS_UKNOT : BS_INS_VKNOT, Vd{rsk(s, u), (double)(coin(0.15) ? rint(d, d + 1) : rint(1, std::max(d - 1, 1))), rtol(), (double)rint(0, 1)}));
        } else if (r == 1) {
          Vd k = knots_args(s, u, rint(0, 2), 0.06); a.insert(a.end(), k.begin(), k.end());
          add.push_back(mk(u ? BS_INS_UKNOTS : BS_INS_VKNOTS, a));
        } else {
          if (coin(0.5)) { add.push_back(mk(u ? BS_INS_UKNOT : BS_INS_VKNOT, Vd{rsk(s, u), 1.0, 0.0, 1.0})); add.push_back(mk(1)); }
          int i1 = u ? b->FirstUKnotIndex() : b->FirstVKnotIndex(), i2 = u ? b->LastUKnotIndex() : b->LastVKnotIndex();
          int idx = coin(0.06) ? i2 + 1 : rint(i1, i2);
          int nk = u ? b->NbUKnots() : b->NbVKnots();
          int mu = (idx >= 1 && idx <= nk) ? (u ? b->UMultiplicity(idx) : b->VMultiplicity(idx)) : 1;
          add.push_back(mk(u ? BS_REMOVE_UKNOT : BS_REMOVE_VKNOT, Vd{(double)idx, (double)(coin(0.1) ? -1 : rint(0, std::max(mu, 1))), coin(0.5) ? 1e-6 : (coin(0.5) ? 1e-2 : 1e3)}));
        }
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
      { SF("bsplsurf_segment", 44, 1600) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 2, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s);
        double u1, u2, v1, v2; b->Bounds(u1, u2, v1, v2);
        double su = u2 - u1, sv_ = v2 - v1;
        double a1 = u1 + rnd(0, 0.6) * su, a2 = std::min(u2, a1 + rnd(0.1, 0.8) * su), b1 = v1 + rnd(0, 0.6) * sv_, b2 = std::min(v2, b1 + rnd(0.1, 0.8) * sv_);
        if (a2 - a1 < 0.02 * su) a1 = std::max(u1, a2 - 0.05 * su);
        if (b2 - b1 < 0.02 * sv_) b1 = std::max(v1, b2 - 0.05 * sv_);
        double r = rnd(0, 1);
        if (r < 0.08) std::swap(a1, a2); else if (r < 0.14) b2 = v2 + 0.3 * sv_;
        if (b->IsUPeriodic() && coin(0.4)) a2 = a1 + rnd(0.05, 1.2) * su;
        if (b->IsVPeriodic() && coin(0.4)) b2 = b1 + rnd(0.05, 1.2) * sv_;
        add.push_back(mk(coin(0.8) ? BS_SEGMENT : BS_CHECK_SEGMENT, Vd{a1, a2, b1, b2, coin(0.6) ? Precision::PConfusion() : 1e-4, coin(0.6) ? Precision::PConfusion() : 0.0}));
        add.push_back(mk(1)); add_sevals(add, s, 1, 1);
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_set_knot", 40, 1600) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s); bool u = coin(0.5);
        int nk = u ? b->NbUKnots() : b->NbVKnots(); int r = rint(0, 2);
        if (r == 2) {
          int n = coin(0.1) ? nk + rint(-1, 1) : nk; Vd a{(double)n};
          double k = (u ? b->UKnot(1) : b->VKnot(1)) + rnd(-0.2, 0.2), step = rnd(0.1, 1.0);
          if (coin(0.4)) { for (int i = 1; i <= nk; ++i) a.push_back((u ? b->UKnot(i) : b->VKnot(i)) + rnd(-0.04, 0.04)); a.resize((size_t)n + 1, 1.0); }
          else for (int i = 0; i < n; ++i) { a.push_back(k); k += step * (coin(0.05) ? -1 : 1); }
          add.push_back(mk(u ? BS_SET_UKNOTS : BS_SET_VKNOTS, a));
        } else {
          int idx = coin(0.06) ? nk + 1 : rint(1, nk);
          double base = (idx >= 1 && idx <= nk) ? (u ? b->UKnot(idx) : b->VKnot(idx)) : 0.0;
          double k = coin(0.7) ? base + rnd(-0.1, 0.1) : base + rnd(-3, 3);
          int d = u ? b->UDegree() : b->VDegree();
          if (r == 0) add.push_back(mk(u ? BS_SET_UKNOT : BS_SET_VKNOT, Vd{(double)idx, k}));
          else add.push_back(mk(u ? BS_SET_UKNOT_M : BS_SET_VKNOT_M, Vd{(double)idx, k, (double)rint(1, d + 1)}));
        }
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_periodic", 44, 1600) {
      SV sv = rbsps(coin(0.5) ? 1 : (coin(0.5) ? 2 : 3), 4);
      if (coin(0.25)) sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s);
        int r = rint(0, 5);
        if (r == 0) add.push_back(mk(BS_SET_UPER));
        else if (r == 1) add.push_back(mk(BS_SET_VPER));
        else if (r == 2) add.push_back(mk(coin(0.5) ? BS_SET_UNOTPER : BS_SET_VNOTPER));
        else if (r == 3) {
          bool u = coin(0.5);
          int i1 = u ? b->FirstUKnotIndex() : b->FirstVKnotIndex(), i2 = u ? b->LastUKnotIndex() : b->LastVKnotIndex();
          add.push_back(mk(u ? BS_SET_UORIGIN : BS_SET_VORIGIN, Vd{(double)(coin(0.1) ? i2 + 1 : rint(i1, i2))}));
        } else if (r == 4) { double u1, u2, v1, v2; b->Bounds(u1, u2, v1, v2); add.push_back(mk(BS_PER_NORM, Vd{u1 + rnd(-1.5, 2.5) * (u2 - u1), v1 + rnd(-1.5, 2.5) * (v2 - v1)})); }
        else add.push_back(mk(BS_EXCHANGE));
        add.push_back(mk(1)); add_sevals(add, s, 1, 1);
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_set_pole", 44, 1600) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s); int nu = b->NbUPoles(), nv = b->NbVPoles();
        int i = coin(0.06) ? 0 : rint(1, nu), j = coin(0.06) ? nv + 1 : rint(1, nv);
        double w = coin(0.1) ? (coin(0.5) ? 0.0 : -2.0) : (coin(0.25) ? 1.0 : rweight());
        int r = rint(0, 6);
        if (r == 0) add.push_back(mk(BS_SET_POLE, Vd{(double)i, (double)j, crd(), crd(), crd()}));
        else if (r == 1) add.push_back(mk(BS_SET_POLE_W, Vd{(double)i, (double)j, crd(), crd(), crd(), w}));
        else if (r == 2) add.push_back(mk(BS_SET_WEIGHT, Vd{(double)i, (double)j, w}));
        else if (r == 3 || r == 4) {
          bool col = r == 3, hw = coin(0.5); int n = col ? nu : nv; if (coin(0.06)) n += 1;
          Vd a{(double)(col ? j : i)}; add_row(a, n); if (hw) add_wts(a, n);
          add.push_back(mk(col ? (hw ? BS_SET_POLE_COL_W : BS_SET_POLE_COL) : (hw ? BS_SET_POLE_ROW_W : BS_SET_POLE_ROW), a));
        } else {
          bool col = r == 5; int n = col ? nu : nv; if (coin(0.06)) n -= 1;
          Vd a{(double)(col ? j : i)}; add_wts(a, std::max(n, 0));
          add.push_back(mk(col ? BS_SET_WEIGHT_COL : BS_SET_WEIGHT_ROW, a));
        }
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  { SF("bsplsurf_move_point", 36, 1600) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 2, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s); int nu = b->NbUPoles(), nv = b->NbVPoles();
        int a1 = rint(1, nu), a2 = rint(a1, nu), c1 = rint(1, nv), c2 = rint(c1, nv);
        if (coin(0.08)) { a1 = nu; a2 = 1; }
        add.push_back(mk(BS_MOVE_POINT, Vd{rsk(s, true), rsk(s, false), crd(), crd(), crd(), (double)a1, (double)a2, (double)c1, (double)c2}));
        add.push_back(mk(1));
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("bsplsurf_iso_local", 40, 1700) {
      SV sv = rbss();
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        Geom_BSplineSurface* b = SBS(s);
        add.push_back(mk(BS_LOCATE_U, Vd{rsk(s, true), coin(0.3) ? 0.0 : 1e-9, (double)rint(0, 1)}));
        add.push_back(mk(BS_LOCATE_V, Vd{rsk(s, false), coin(0.3) ? 0.0 : 1e-2, (double)rint(0, 1)}));
        add.push_back(mk(coin(0.5) ? BS_UISO_C : BS_VISO_C, Vd{rsk(s, true), (double)rint(0, 1)}));
        add.push_back(mk(BZ_RESOLUTION, Vd{coin(0.3) ? 1e-7 : rnd(0.01, 2)}));
        for (int k = 0; k < 2; ++k) {
          int nku = b->NbUKnots(), nkv = b->NbVKnots();
          int a1 = rint(1, nku - 1), a2 = rint(a1 + 1, nku), c1 = rint(1, nkv - 1), c2 = rint(c1 + 1, nkv);
          if (coin(0.05)) a2 = a1;
          double u = b->UKnot(a1) + rnd(0, 1) * (b->UKnot(a2) - b->UKnot(a1)), v = b->VKnot(c1) + rnd(0, 1) * (b->VKnot(c2) - b->VKnot(c1));
          Vd a{u, v, (double)a1, (double)a2, (double)c1, (double)c2};
          int r = rint(0, 5);
          if (r == 0) add.push_back(mk(BS_LOCAL_VALUE, a)); else if (r == 1) add.push_back(mk(BS_LOCAL_D0, a)); else if (r == 2) add.push_back(mk(BS_LOCAL_D1, a));
          else if (r == 3) add.push_back(mk(BS_LOCAL_D2, a)); else if (r == 4) add.push_back(mk(BS_LOCAL_D3, a));
          else { a.push_back(rint(0, b->UDegree())); a.push_back(rint(0, b->VDegree())); add.push_back(mk(BS_LOCAL_DN, a)); }
        }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, false);
    } }
  }

// rectangular trimmed surfaces
static void fam_rts() {
  { SF("rts_build", 40, 1600) {
      SV sv = mk_rts(rbasic_surf());
      Ops ops;
      ops.push_back(mk(1)); ops.push_back(mk(SU_BOUNDS)); ops.push_back(mk(RT_BASIS)); ops.push_back(mk(SU_CONT)); ops.push_back(mk(SU_IS_UCLOSED)); ops.push_back(mk(SU_IS_VCLOSED));
      ops.push_back(mk(SU_IS_UPER)); ops.push_back(mk(SU_IS_VPER)); ops.push_back(mk(SU_ISCNU, Vd{(double)(coin(0.08) ? -1 : rint(0, 3))})); ops.push_back(mk(SU_ISCNV, Vd{(double)rint(0, 3)}));
      { bool pu = true; try { pu = !build_s(sv)->IsUPeriodic(); } catch (Standard_Failure&) {} if (!pu || coin(0.15)) ops.push_back(mk(SU_UPERIOD)); }
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("rts_set_trim", 36, 1600) {
      SV sv = mk_rts(rbasic_surf());
      Ops ops = plan_surf(sv, nullptr, nullptr, 3, [&](const HS& s, Ops& add) {
        Geom_RectangularTrimmedSurface* t = dynamic_cast<Geom_RectangularTrimmedSurface*>(s.get());
        HS b = t->BasisSurface(); double u1, u2, v1, v2; b->Bounds(u1, u2, v1, v2);
        if (u1 < -1e50) { u1 = -3; u2 = 3; } if (v1 < -1e50) { v1 = -3; v2 = 3; }
        double su = u2 - u1, sv_ = v2 - v1;
        if (coin(0.6)) {
          double a1 = u1 + rnd(0, 0.9) * su, a2 = a1 + (coin(0.2) ? -1 : 1) * rnd(0.05, 0.4) * su, c1 = v1 + rnd(0, 0.9) * sv_, c2 = c1 + (coin(0.2) ? -1 : 1) * rnd(0.05, 0.4) * sv_;
          if (!b->IsUPeriodic() && !coin(0.1)) a2 = std::min(std::max(a2, u1), u2);
          if (!b->IsVPeriodic() && !coin(0.1)) c2 = std::min(std::max(c2, v1), v2);
          add.push_back(mk(RT_SET_TRIM, Vd{a1, a2, c1, c2, (double)rint(0, 1), (double)rint(0, 1)}));
        } else {
          bool ut = coin(0.5); double lo = ut ? u1 : v1, sp = ut ? su : sv_;
          double p1 = lo + rnd(0, 0.9) * sp, p2 = p1 + rnd(0.05, 0.4) * sp; if (coin(0.1)) p2 = p1;
          add.push_back(mk(RT_SET_TRIM_P, Vd{p1, p2, ut ? 1.0 : 0.0, (double)rint(0, 1)}));
        }
        add.push_back(mk(1)); if (coin(0.4)) add_sevals(add, s, 1, 1);
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  }

// offset surfaces
static SV rsing_basis() {
  double r = rnd(0, 1);
  if (r < 0.3) { SV s = rsphere(); s.a = rradius(false); return s; }
  if (r < 0.55) { SV s = rcone(); s.b = coin(0.5) ? 0.0 : rradius(true); return s; }
  if (r < 0.75) return rrev_singular();
  if (r < 0.88) return rbezs(3);
  return rtorus();
}
static void fam_offsurf() {
  { SF("offsurf_build", 40, 1600) {
      SV sv = mk_offs(roffbasis(), roffs(), coin(0.2));
      Ops ops;
      ops.push_back(mk(1)); ops.push_back(mk(OS_OFFSET)); ops.push_back(mk(OS_BASIS_CONT)); ops.push_back(mk(SU_CONT)); ops.push_back(mk(SU_BOUNDS)); ops.push_back(mk(OS_BASIS));
      ops.push_back(mk(SU_IS_UCLOSED)); ops.push_back(mk(SU_IS_VCLOSED)); ops.push_back(mk(SU_IS_UPER)); ops.push_back(mk(SU_IS_VPER));
      ops.push_back(mk(SU_ISCNU, Vd{(double)(coin(0.08) ? -1 : rint(0, 3))})); ops.push_back(mk(SU_ISCNV, Vd{(double)rint(0, 3)}));
      if (coin(0.4)) ops.push_back(mk(OS_SURFACE));
      { HS hb; try { hb = build_s(sv); } catch (Standard_Failure&) {} if (!hb.IsNull()) add_sevals(ops, hb, 3, 2); }
      { bool pu = true; try { pu = !build_s(sv)->IsVPeriodic(); } catch (Standard_Failure&) {} if (!pu || coin(0.1)) ops.push_back(mk(SU_VPERIOD)); }
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
    { SF("offsurf_singular", 40, 1600) {
      SV sv = mk_offs(rsing_basis(), coin(0.2) ? 0.0 : roffs(), coin(0.7));
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        double u1, u2, v1, v2; s->Bounds(u1, u2, v1, v2);
        for (int k = 0; k < 5; ++k) {
          double u = rspar_u(s), v = coin(0.5) ? (coin(0.5) ? v1 : v2) : rspar_v(s);
          if (coin(0.3)) u = coin(0.5) ? u1 : u2;
          int r = rint(0, 4); Vd a{u, v};
          if (r == 0) add.push_back(mk(SU_D0, a)); else if (r == 1) add.push_back(mk(SU_D1, a)); else if (r == 2) add.push_back(mk(SU_D2, a));
          else if (r == 3) add.push_back(mk(SU_D3, a)); else { a.push_back(rint(0, 2)); a.push_back(rint(0, 2)); add.push_back(mk(SU_DN, a)); }
        }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("offsurf_osculating", 36, 1600) {
      SV sv = mk_offs(coin(0.5) ? rbsps(0, 4) : (coin(0.5) ? rbezs(3) : roffbasis()), roffs(), coin(0.3));
      Ops ops = plan_surf(sv, nullptr, nullptr, 1, [&](const HS& s, Ops& add) {
        for (int k = 0; k < 2; ++k) { add.push_back(mk(coin(0.5) ? OS_UOSC : OS_VOSC, Vd{rspar_u(s), rspar_v(s)})); }
      });
      emit_surf(N, sv, nullptr, nullptr, ops, true);
    } }
  { SF("offsurf_set", 36, 1600) {
      SV sv = mk_offs(roffbasis(), roffs(), coin(0.3)); SV b2 = roffbasis();
      Ops ops = plan_surf(sv, nullptr, &b2, 3, [&](const HS& s, Ops& add) {
        int r = rint(0, 4);
        if (r == 0) { add.push_back(mk(OS_SET_BASIS, Vd{(double)rint(0, 1)})); add.push_back(mk(1)); }
        else if (r == 1) { add.push_back(mk(OS_SET_OFFSET, Vd{roffs()})); add.push_back(mk(OS_OFFSET)); add.push_back(mk(RS_HAS_EREP)); }
        else if (r == 2) { add.push_back(mk(SU_UREVERSE)); add.push_back(mk(1)); }
        else if (r == 3) { add.push_back(op_tr(4, rtr(true))); add.push_back(mk(1)); }
        else add_sevals(add, s, 2, 2);
      });
      emit_surf(N, sv, nullptr, &b2, ops, true);
    } }
}

// evaluation representations of surfaces
static SV rsrep() {
  double r = rnd(0, 1);
  if (r < 0.2) return rplane();
  if (r < 0.4) return rcylinder();
  if (r < 0.55) return rsphere();
  if (r < 0.8) return rbezs(3);
  return rbsps(1, 4);
}
static SV rsowner() {
  double r = rnd(0, 1);
  if (r < 0.25) return rbezs(3);
  if (r < 0.5) return rbsps(0, 4);
  if (r < 0.65) return rext();
  if (r < 0.8) return rrev();
  return mk_offs(roffbasis(), roffs(), true);
}
static Op serep_op(int kind, int maxd, bool dom, const double* d, double su, double ou, double sv_, double ov, bool swap, bool null_rep) {
  return mk(RS_SET_EREP, Vd{(double)kind, (double)maxd, dom ? 1.0 : 0.0, d[0], d[1], d[2], d[3], su, ou, sv_, ov, swap ? 1.0 : 0.0, null_rep ? 1.0 : 0.0});
}
static void fam_serep() {
  auto go = [&](const char* name, int target, int mode) {
    SF(name, target, 1600) {
      SV cv = rsowner(), rep = rsrep();
      Ops ops = plan_surf(cv, nullptr, &rep, 3, [&](const HS& s, Ops& add) {
        double u1, u2, v1, v2; s->Bounds(u1, u2, v1, v2);
        if (u1 < -1e50 || u2 > 1e50) { u1 = -2; u2 = 2; }
        if (v1 < -1e50 || v2 > 1e50) { v1 = -2; v2 = 2; }
        double d[4] = {u1 + rnd(0, 0.5) * (u2 - u1), u1 + rnd(0.5, 1.2) * (u2 - u1), v1 + rnd(0, 0.5) * (v2 - v1), v1 + rnd(0.5, 1.2) * (v2 - v1)};
        double zero[4] = {0, 0, 0, 0};
        if (add.empty() && !erep_of_s(s.get())) {
          if (mode == 0) add.push_back(coin(0.5) ? serep_op(0, 3, false, zero, 1, 0, 1, 0, false, false) : serep_op(1, rint(0, 3), false, zero, 1, 0, 1, 0, false, false));
          else if (mode == 2) {
            bool sw = coin(0.3);
            double su = coin(0.4) ? 1.0 : (coin(0.5) ? rnd(0.5, 2) : -rnd(0.5, 2)), sv_ = coin(0.4) ? 1.0 : rnd(0.5, 2);
            add.push_back(serep_op(2, rint(0, 3), coin(0.6), d, su, coin(0.5) ? 0.0 : rnd(-1, 1), sv_, coin(0.5) ? 0.0 : rnd(-1, 1), sw, false));
          } else if (mode == 3) {
            int r = rint(0, 3);
            if (r == 0) add.push_back(serep_op(rint(0, 2), 3, false, zero, 1, 0, 1, 0, false, true));
            else if (r == 1) add.push_back(serep_op(2, 3, false, zero, 0.0, 0.5, 1, 0, false, false));
            else if (r == 2) add.push_back(serep_op(2, 3, false, zero, 1, 0, 5e-8, 0.5, false, false));
            else add.push_back(serep_op(2, 3, coin(0.5), d, 1e-4, 0.25, 1e-4 * rint(1, 3), 0, coin(0.5), false));
          } else add.push_back(serep_op(rint(0, 2), rint(0, 3), false, zero, 1, 0, 1, 0, false, false));
          add.push_back(mk(RS_HAS_EREP)); add.push_back(mk(RS_EREP_KIND));
          return;
        }
        if (mode == 4) {
          int r = rint(0, 8);
          if (r == 0) add.push_back(mk(37));
          else if (r == 1) add.push_back(op_tr(21, rtr(false)));
          else if (r == 2) add.push_back(op_tr(4, rtr(false)));
          else if (r == 3) add.push_back(mk(SU_UREVERSE));
          else if (r == 4) add.push_back(mk(RS_CLEAR_EREP));
          else if (r == 5) { if (SBZ(s)) add.push_back(mk(BZ_SET_POLE, Vd{1.0, 1.0, crd(), crd(), crd()})); else if (SBS(s)) add.push_back(mk(BS_SET_POLE, Vd{1.0, 1.0, crd(), crd(), crd()})); else add.push_back(mk(SU_BOUNDS)); }
          else if (r == 6) { if (dynamic_cast<Geom_OffsetSurface*>(s.get())) add.push_back(mk(OS_SET_OFFSET, Vd{roffs()})); else add.push_back(mk(SU_BOUNDS)); }
          else if (r == 7) { if (SBS(s)) add.push_back(mk(BS_INCREASE, Vd{(double)(SBS(s)->UDegree() + 1), (double)SBS(s)->VDegree()})); else if (SBZ(s)) add.push_back(mk(BZ_EXCHANGE)); else add.push_back(mk(SU_BOUNDS)); }
          else if (dynamic_cast<Geom_SurfaceOfLinearExtrusion*>(s.get())) { double dx, dy, dz; rdir3(dx, dy, dz); add.push_back(mk(SW_SET_DIR, Vd{dx, dy, dz})); }
          else add.push_back(mk(SU_BOUNDS));
          add.push_back(mk(RS_HAS_EREP));
        }
        for (int k = 0; k < 3; ++k) {
          double u = coin(0.3) ? rspar_u(s) : u1 + rnd(-0.3, 1.3) * (u2 - u1), v = coin(0.3) ? rspar_v(s) : v1 + rnd(-0.3, 1.3) * (v2 - v1);
          int r = rint(0, 3);
          if (r == 0) add.push_back(mk(SU_D0, Vd{u, v})); else if (r == 1) add.push_back(mk(SU_D1, Vd{u, v})); else if (r == 2) add.push_back(mk(SU_D2, Vd{u, v}));
          else { int q1 = rint(0, 2), q2 = rint(0, 2); if (q1 == 0 && q2 == 0) q1 = 1; add.push_back(mk(SU_DN, Vd{u, v, (double)q1, (double)q2})); }
        }
      });
      emit_surf(name, cv, nullptr, &rep, ops, true);
    }
  };
  go("evalrep_surf_basic", 24, 0); go("evalrep_surf_mapped", 32, 2); go("evalrep_surf_invalid", 20, 3); go("evalrep_surf_copy", 36, 4);
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
  if (const char* c = std::getenv("GEOM_CAP")) g_cap = std::atoi(c);
  if (const char* c = std::getenv("GEOM_MAXLEN")) g_maxlen_default = (size_t)std::atoi(c);
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
  if (want("surface")) fam_surface_common();
  if (want("elementary")) fam_elementary();
  if (want("swept")) fam_swept();
  if (want("bezsurf")) fam_bezsurf();
  if (want("bsplsurf")) fam_bsplsurf();
  if (want("rts")) fam_rts();
  if (want("offsurf")) fam_offsurf();
  if (want("serep")) fam_serep();
  for (const std::string& n : fam_order)
    std::fprintf(stderr, "%-24s %5d lines, %5d dropped (%d uninit)\n", n.c_str(), fam_lines[n], fam_dropped.count(n) ? fam_dropped[n] : 0,
                 fam_dropped.count(n + "!uninit") ? fam_dropped[n + "!uninit"] : 0);
  for (auto& kv : fam_dropped) if (kv.first.find("!libm") != std::string::npos || kv.first.find("!crash") != std::string::npos || kv.first.find("~changed") != std::string::npos)
    std::fprintf(stderr, "   dropped: %s %d\n", kv.first.c_str(), kv.second);
  return 0;
}
