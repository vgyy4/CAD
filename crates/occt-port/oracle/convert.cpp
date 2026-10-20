// Deterministic reference calls for OCCT 8.0.1's package Convert (cpp/Convert/):
//   conics to B-splines: Convert_ConicToBSplineCurve (+ BuildCosAndSin, both overloads) with Convert_CircleToBSplineCurve,
//     Convert_EllipseToBSplineCurve (full and trimmed), Convert_HyperbolaToBSplineCurve, Convert_ParabolaToBSplineCurve,
//     BuildPolynomialCosAndSin (Convert_PolynomialCosAndSin);
//   elementary surfaces: Convert_ElementarySurfaceToBSplineSurface with Convert_CylinderToBSplineSurface,
//     Convert_ConeToBSplineSurface, Convert_SphereToBSplineSurface, Convert_TorusToBSplineSurface (every constructor);
//   Convert_CompBezierCurvesToBSplineCurve (3D) and Convert_CompBezierCurves2dToBSplineCurve2d,
//   Convert_CompPolynomialToPoles (three constructors), Convert_GridPolynomialToPoles (two constructors).
// The package has no Convert_BSplineCurveToBezierCurve-style helper (the headers were checked).
//
// Output: one record per line, `<family> <inputs...> | <outputs...>`; floats are 16-hex-digit f64 bit patterns, ints and bools
// decimal. Every input is printed on the left of ` | ` in the order of the family's schema (`begin(name, schema)`); the oracle
// aborts if the printed words differ from the schema. The grammar of the composite words (fr2, fr3, conic, surf, bzc2, bzc3, cpoly,
// gpoly ...) is in docs/api-convert.md. No text token occurs (the empty text would need the `\e` token; none is printed). Outputs are
// values, `throws` (OCCT raised an exception) or `unchecked` (a check of OCCT is compiled out of the libraries, or the result is
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
#include <malloc.h>
#include <unistd.h>
#include <sys/wait.h>
#define protected public
#include <Convert_CircleToBSplineCurve.hxx>
#include <Convert_CompBezierCurves2dToBSplineCurve2d.hxx>
#include <Convert_CompBezierCurvesToBSplineCurve.hxx>
#include <Convert_CompPolynomialToPoles.hxx>
#include <Convert_ConeToBSplineSurface.hxx>
#include <Convert_ConicToBSplineCurve.hxx>
#include <Convert_CylinderToBSplineSurface.hxx>
#include <Convert_EllipseToBSplineCurve.hxx>
#include <Convert_GridPolynomialToPoles.hxx>
#include <Convert_HyperbolaToBSplineCurve.hxx>
#include <Convert_ParabolaToBSplineCurve.hxx>
#include <Convert_PolynomialCosAndSin.hxx>
#include <Convert_SphereToBSplineSurface.hxx>
#include <Convert_TorusToBSplineSurface.hxx>
#undef protected
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <NCollection_HArray1.hxx>
#include <NCollection_HArray2.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Real.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Vec.hxx>
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

static void h(double v) {
  if (!std::isfinite(v)) {
    if (!outphase) { std::fprintf(stderr, "non-finite input in %s: %s\n", cur_name.c_str(), buf.c_str()); std::abort(); }
    throw Discard();
  }
  uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits);
  buf += t; ++ntok;
}
// like h(), but an infinite output is allowed (an overflow of an exact + - * / expression is deterministic)
static void hi(double v) {
  if (std::isinf(v) && outphase) { uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits); buf += t; ++ntok; return; }
  h(v);
}
static void in(long v) { buf += " " + std::to_string(v); ++ntok; }
static void ob(bool v) { in(v ? 1 : 0); }
// A text as one token, escaped so that it decodes exactly: \\ for a backslash, \n, \t, \s for a
// space, and \e alone for the empty text (which would otherwise print nothing).
static void os_text(const std::string& s) {
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
// schema / record handling and the re-run harness (as in oracle/math1d.cpp)
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
  if (std::getenv("CONVERT_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
// 0 or 1 ulp (a different libm gives such differences, and tests/convert.rs compares the families that use them within a
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
  if (!ok && std::getenv("CONVERT_DEBUG")) std::fprintf(stderr, "CRASH %s\n", buf.c_str());
  return ok;
}
static bool emit(const std::function<void()>& body, bool libm_robust = false, double robust_ulps = 2.0) {
  if (g_guard && !survives(body)) { fam_dropped[cur_name]++; fam_dropped[cur_name + "!crash"]++; buf.clear(); return false; }
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
    for (uint64_t salt = 1; salt <= 24; ++salt) {
      g_perturb = true; g_salt = salt;
      bool dp = false;
      std::string rp = run_once(body, 0, dp);
      g_perturb = false;
      ntok = (int)keep_ntok;
      if (!dp && rp != r1) fam_dropped[cur_name + "~changed"]++;
      bool ok = !dp && lines_close(r1, rp, robust_ulps);
      if (!ok) { fam_dropped[cur_name]++; fam_dropped[cur_name + "!libm"]++; buf.clear(); return false; }
    }
  }
  buf = r1;
  finish();
  return true;
}

// =================================================================================
// helpers shared by the families
static const char* g_only_fam = nullptr;
static bool fam_wanted(const char* n) {
  if (!g_only_fam) return true;
  std::string o = std::string(",") + g_only_fam + ",", k = std::string(",") + n + ",";
  return o.find(k) != std::string::npos;
}
#define LOOP(NAME, TARGET) const char* N = NAME; for (int att = 0; fam_wanted(N) && att < 40000 && fam_lines[N] < (TARGET); ++att)

static double nice() { return 0.5 * rint(-8, 8); }
static double crd() { return coin(0.4) ? nice() : rnd(-6, 6); }
// a radius: nice, random, tiny, huge or zero
static double radius(bool allow_zero) {
  double r = rnd(0, 1);
  if (r < 0.30) return 0.5 * rint(1, 12);
  if (r < 0.80) return rnd(0.05, 8);
  if (r < 0.86) return rnd(1e-6, 1e-3);
  if (r < 0.92) return rnd(1e3, 1e5);
  return allow_zero ? 0.0 : rnd(0.5, 3);
}
// an angle (start of an arc)
static double ang() {
  double r = rnd(0, 1);
  if (r < 0.25) return (M_PI / 4) * rint(-16, 16);
  if (r < 0.32) return rnd(-1e-6, 1e-6);
  if (r < 0.42) return 2 * M_PI + rnd(-1e-3, 1e-3);
  if (r < 0.47) return -M_PI + rnd(-1e-3, 1e-3);
  return rnd(-4 * M_PI, 4 * M_PI);
}
// the length of an arc, in (0, 2 pi], with the interesting sizes of the parametrisations
static double span() {
  double r = rnd(0, 1);
  if (r < 0.10) return rnd(1e-9, 1e-3);
  if (r < 0.20) return 2 * M_PI * rnd(0.97, 1.0);
  if (r < 0.25) return 2 * M_PI;
  if (r < 0.30) return 0.9999 * M_PI + rnd(-1e-6, 1e-6);
  if (r < 0.35) return 1.9999 * M_PI + rnd(-1e-6, 1e-6);
  if (r < 0.45) return (M_PI / 4) * rint(1, 8);
  if (r < 0.55) return M_PI * rnd(0.9, 1.1);
  return rnd(0.05, 2 * M_PI);
}
// a parameter range [u1, u2] of a trimmed circle or ellipse; sometimes with a bad span
static void arc(double& u1, double& u2) {
  u1 = ang();
  double r = rnd(0, 1), d;
  if (r < 0.03) d = 0.0;
  else if (r < 0.06) d = -rnd(0.01, 3);
  else if (r < 0.08) d = 2 * M_PI + 5e-10;
  else if (r < 0.10) d = 2 * M_PI + rnd(1e-8, 0.5);
  else d = span();
  u2 = u1 + d;
}
static const int NPT = 8;

// ---------------------------------------------------------------------------------
// 2D and 3D frames (words fr2, fr3)
struct Fr2 { double o[2]; double d[2]; int sense; };
static Fr2 rfr2() {
  Fr2 f; f.o[0] = crd(); f.o[1] = crd();
  double r = rnd(0, 1);
  if (r < 0.3) { f.d[0] = 1; f.d[1] = 0; }
  else if (r < 0.6) { do { f.d[0] = rint(-2, 2); f.d[1] = rint(-2, 2); } while (f.d[0] == 0 && f.d[1] == 0); }
  else { do { f.d[0] = rnd(-4, 4); f.d[1] = rnd(-4, 4); } while (f.d[0] * f.d[0] + f.d[1] * f.d[1] < 0.25); }
  f.sense = coin(0.7) ? 1 : 0;
  return f;
}
static gp_Ax2d mkAx2d(const Fr2& f) { return gp_Ax2d(gp_Pnt2d(f.o[0], f.o[1]), gp_Dir2d(f.d[0], f.d[1])); }
static void p_fr2(const Fr2& f) { h(f.o[0]); h(f.o[1]); h(f.d[0]); h(f.d[1]); in(f.sense); }
struct Fr3 { double o[3]; double n[3]; double v[3]; int ind; };
static Fr3 rfr3() {
  Fr3 f;
  for (int k = 0; k < 3; ++k) f.o[k] = crd();
  for (;;) {
    bool nn = coin(0.5);
    for (int k = 0; k < 3; ++k) { f.n[k] = nn ? (double)rint(-1, 1) : rnd(-3, 3); f.v[k] = nn ? (double)rint(-1, 1) : rnd(-3, 3); }
    if (coin(0.25)) { f.n[0] = 0; f.n[1] = 0; f.n[2] = 1; f.v[0] = 1; f.v[1] = 0; f.v[2] = 0; }
    double nn2 = 0, vv = 0, nv = 0;
    for (int k = 0; k < 3; ++k) { nn2 += f.n[k] * f.n[k]; vv += f.v[k] * f.v[k]; nv += f.n[k] * f.v[k]; }
    if (nn2 > 0.25 && vv > 0.25 && nn2 * vv - nv * nv > 0.04 * nn2 * vv) break;
  }
  f.ind = coin(0.3) ? 1 : 0;
  return f;
}
static gp_Ax3 mkAx3(const Fr3& f) {
  gp_Ax3 a(gp_Pnt(f.o[0], f.o[1], f.o[2]), gp_Dir(f.n[0], f.n[1], f.n[2]), gp_Dir(f.v[0], f.v[1], f.v[2]));
  if (f.ind) a.YReverse();
  return a;
}
static void p_fr3(const Fr3& f) { for (int k = 0; k < 3; ++k) h(f.o[k]); for (int k = 0; k < 3; ++k) h(f.n[k]); for (int k = 0; k < 3; ++k) h(f.v[k]); in(f.ind); }

// ---------------------------------------------------------------------------------
// vectors (words iv, fv, fpairs)
static void p_iv(const std::vector<int>& v) { in((long)v.size()); for (int x : v) in(x); }
static void p_fv(const Vd& v) { in((long)v.size()); for (double x : v) h(x); }
static void W_iv(const std::vector<int>& v) { int s = ntok; p_iv(v); word("iv", s, -1); }
static void W_fv(const Vd& v) { int s = ntok; p_fv(v); word("fv", s, -1); }
static void W_fr2(const Fr2& f) { int s = ntok; p_fr2(f); word("fr2", s, 5); }
static void W_fr3(const Fr3& f) { int s = ntok; p_fr3(f); word("fr3", s, 10); }

// ---------------------------------------------------------------------------------
// output of arrays
static void o_a1(const NCollection_Array1<double>& a) { in(a.Lower()); in(a.Upper()); for (int i = a.Lower(); i <= a.Upper(); ++i) h(a(i)); }
static void o_a1i(const NCollection_Array1<int>& a) { in(a.Lower()); in(a.Upper()); for (int i = a.Lower(); i <= a.Upper(); ++i) in(a(i)); }
static void o_a1p2(const NCollection_Array1<gp_Pnt2d>& a) { in(a.Lower()); in(a.Upper()); for (int i = a.Lower(); i <= a.Upper(); ++i) { h(a(i).X()); h(a(i).Y()); } }
static void o_a2(const NCollection_Array2<double>& a) {
  in(a.LowerRow()); in(a.UpperRow()); in(a.LowerCol()); in(a.UpperCol());
  for (int i = a.LowerRow(); i <= a.UpperRow(); ++i) for (int j = a.LowerCol(); j <= a.UpperCol(); ++j) h(a(i, j));
}
static void o_a2p(const NCollection_Array2<gp_Pnt>& a) {
  in(a.LowerRow()); in(a.UpperRow()); in(a.LowerCol()); in(a.UpperCol());
  for (int i = a.LowerRow(); i <= a.UpperRow(); ++i) for (int j = a.LowerCol(); j <= a.UpperCol(); ++j) { h(a(i, j).X()); h(a(i, j).Y()); h(a(i, j).Z()); }
}

// =================================================================================
// conics (word conic): kind 0 circle, 1 ellipse, 2 hyperbola, 3 parabola
//   circle     fr2 R trim pt [u1 u2]          (trim = 1: the trimmed constructor)
//   ellipse    fr2 major minor trim pt [u1 u2]
//   hyperbola  fr2 major minor u1 u2
//   parabola   fr2 focal u1 u2
struct Cn { int kind; Fr2 f; double a, b; int trim, pt; double u1, u2; };
static void p_conic(const Cn& c) {
  in(c.kind); p_fr2(c.f);
  switch (c.kind) {
    case 0: h(c.a); in(c.trim); in(c.pt); if (c.trim) { h(c.u1); h(c.u2); } break;
    case 1: h(c.a); h(c.b); in(c.trim); in(c.pt); if (c.trim) { h(c.u1); h(c.u2); } break;
    case 2: h(c.a); h(c.b); h(c.u1); h(c.u2); break;
    default: h(c.a); h(c.u1); h(c.u2); break;
  }
}
static void W_conic(const Cn& c) { int s = ntok; p_conic(c); word("conic", s, -1); }
// the parametrisation type: all eight, the ones that refuse included
static int rpt() { return coin(0.2) ? (coin(0.5) ? 0 : 6) : rint(0, NPT - 1); }
static void short_arc(int pt, double& u1, double& u2) {   // the parametrisations with 1 and 2 spans refuse long arcs: mostly give them short ones
  if ((pt == 1 || pt == 2) && coin(0.8)) u2 = u1 + (pt == 1 ? rnd(0.001, 0.9999 * M_PI) : rnd(0.001, 1.9999 * M_PI));
}
static Cn rconic(int kind, int trim) {
  Cn c; c.kind = kind; c.f = rfr2(); c.a = c.b = 0; c.trim = trim; c.pt = rpt(); c.u1 = c.u2 = 0;
  switch (kind) {
    case 0: c.a = radius(true); if (trim) { arc(c.u1, c.u2); short_arc(c.pt, c.u1, c.u2); } break;
    case 1: {
      c.b = radius(true); double r = rnd(0, 1);
      c.a = r < 0.1 ? c.b : (r < 0.15 ? c.b + 1e-12 : c.b * rnd(1, 6) + (coin(0.2) ? 0.5 : 0));
      if (trim) { arc(c.u1, c.u2); short_arc(c.pt, c.u1, c.u2); }
      break;
    }
    case 2: case 3: {
      if (kind == 2) { c.a = radius(false); c.b = radius(false); } else c.a = radius(false);
      double r = rnd(0, 1);
      c.u1 = coin(0.5) ? nice() * 0.5 : rnd(-3, 3);
      if (r < 0.04) c.u2 = c.u1;
      else if (r < 0.10) c.u2 = c.u1 + rnd(-1e-3, 1e-3);
      else if (r < 0.20) c.u2 = c.u1 - rnd(0.1, 4);
      else if (r < 0.25) c.u2 = c.u1 + rnd(5, 15);
      else c.u2 = c.u1 + rnd(0.05, 4);
      break;
    }
  }
  return c;
}
// a call that OCCT does not check (the check is compiled out): the Rust API returns an error
static bool conic_unchecked(const Cn& c) {
  switch (c.kind) {
    case 1: if (c.trim) { double d = c.u2 - c.u1; return (d > (2 * M_PI + Precision::PConfusion())) || d <= 0.0; } return false;
    case 2: case 3: return std::abs(c.u2 - c.u1) < Epsilon(0.);
    default: return false;
  }
}
// builds the conic (the gp object first: it may throw) and hands it to `use`; false when the call is unchecked
static bool with_conic(const Cn& c, const std::function<void(const Convert_ConicToBSplineCurve&)>& use) {
  gp_Ax2d ax = mkAx2d(c.f);
  const Convert_ParameterisationType pt = (Convert_ParameterisationType)c.pt;
  switch (c.kind) {
    case 0: {
      gp_Circ2d g(ax, c.a, c.f.sense != 0);
      if (c.trim) { Convert_CircleToBSplineCurve k(g, c.u1, c.u2, pt); use(k); } else { Convert_CircleToBSplineCurve k(g, pt); use(k); }
      return true;
    }
    case 1: {
      gp_Elips2d g(ax, c.a, c.b, c.f.sense != 0);
      if (conic_unchecked(c)) return false;
      if (c.trim) { Convert_EllipseToBSplineCurve k(g, c.u1, c.u2, pt); use(k); } else { Convert_EllipseToBSplineCurve k(g, pt); use(k); }
      return true;
    }
    case 2: {
      gp_Hypr2d g(ax, c.a, c.b, c.f.sense != 0);
      if (conic_unchecked(c)) return false;
      Convert_HyperbolaToBSplineCurve k(g, c.u1, c.u2); use(k);
      return true;
    }
    default: {
      gp_Parab2d g(ax, c.a, c.f.sense != 0);
      if (conic_unchecked(c)) return false;
      Convert_ParabolaToBSplineCurve k(g, c.u1, c.u2); use(k);
      return true;
    }
  }
}
static void put_curve(const Convert_ConicToBSplineCurve& k) {
  in(k.Degree()); in(k.NbPoles()); in(k.NbKnots()); ob(k.IsPeriodic());
  o_a1p2(k.Poles()); o_a1(k.Weights()); o_a1(k.Knots()); o_a1i(k.Multiplicities());
}
static void put_cs(const NCollection_Array1<double>& c, const NCollection_Array1<double>& s, const NCollection_Array1<double>& d, int degree,
                   const NCollection_Array1<double>& knots, const NCollection_Array1<int>& mults) {
  o_a1(c); o_a1(s); o_a1(d); in(degree); o_a1(knots); o_a1i(mults);
}
static int nspans(double d) { return (int)std::trunc(1.2 * d / M_PI) + 1; }
// BuildCosAndSin(type, u1, u2) cases that read outside an array or divide by zero (not executed)
static bool cs_unchecked(int pt, double u1, double u2) {
  double d = u2 - u1;
  if (pt == Convert_TgtThetaOver2) return nspans(d) < 1;
  if (pt == Convert_QuasiAngular || pt == Convert_RationalC1 || pt == Convert_Polynomial) return d <= 0.0;
  return false;
}
static void fam_conics() {
  g_guard = true;
  const int NC = 44;
  { LOOP("circle_full", NC) {
    Cn c = rconic(0, 0);
    begin(N, "conic"); W_conic(c); bar();
    emit([&] { if (!with_conic(c, put_curve)) unchecked(); }, true);
  } }
  { LOOP("circle_trim", NC + 10) {
    Cn c = rconic(0, 1);
    begin(N, "conic"); W_conic(c); bar();
    emit([&] { if (!with_conic(c, put_curve)) unchecked(); }, true);
  } }
  { LOOP("ellipse_full", NC) {
    Cn c = rconic(1, 0);
    begin(N, "conic"); W_conic(c); bar();
    emit([&] { if (!with_conic(c, put_curve)) unchecked(); }, true);
  } }
  { LOOP("ellipse_trim", NC + 10) {
    Cn c = rconic(1, 1);
    begin(N, "conic"); W_conic(c); bar();
    emit([&] { if (!with_conic(c, put_curve)) unchecked(); }, true);
  } }
  { LOOP("hyperbola", NC) {
    Cn c = rconic(2, 0);
    if (att % 11 == 5) c.u2 = c.u1;      // U1 == U2: unchecked
    begin(N, "conic"); W_conic(c); bar();
    emit([&] { if (!with_conic(c, put_curve)) unchecked(); }, true);
  } }
  { LOOP("parabola", NC) {
    Cn c = rconic(3, 0);
    begin(N, "conic"); W_conic(c); bar();
    emit([&] { if (!with_conic(c, put_curve)) unchecked(); });   // + - * / only
  } }
  { LOOP("conic_index", 44) {
    Cn c = rconic(att % 4, att % 8 < 4 ? 0 : 1);
    { static const int OKPT[] = {0, 3, 4, 5, 6, 7}; if (coin(0.85)) c.pt = pick(OKPT); }
    int acc = rint(0, 3), idx = rint(-1, 12);
    if (coin(0.7)) idx = rint(1, 6);
    begin(N, "conic i i"); W_conic(c); W_i(acc); W_i(idx); bar();
    emit([&] {
      bool ran = with_conic(c, [&](const Convert_ConicToBSplineCurve& k) {
        switch (acc) {
          case 0: { gp_Pnt2d p = k.Pole(idx); h(p.X()); h(p.Y()); break; }
          case 1: h(k.Weight(idx)); break;
          case 2: h(k.Knot(idx)); break;
          default: in(k.Multiplicity(idx)); break;
        }
      });
      if (!ran) unchecked();
    }, true);
  } }
  { LOOP("cos_sin_periodic", 16) {
    int pt = att % NPT;
    begin(N, "i"); W_i(pt); bar();
    emit([&] {
      Convert_ConicToBSplineCurve k(0, 0, 0);
      NCollection_Array1<double> c, s, d, kn; NCollection_Array1<int> m; int degree = 0;
      k.BuildCosAndSin((Convert_ParameterisationType)pt, c, s, d, degree, kn, m);
      put_cs(c, s, d, degree, kn, m);
    }, true);
  } }
  { LOOP("cos_sin_trim", 56) {
    int pt = rpt(); double u1 = ang(), u2;
    double r = rnd(0, 1);
    if (r < 0.1) u2 = u1 - rnd(0.1, 8); else if (r < 0.15) u2 = u1; else if (r < 0.25) u2 = u1 + rnd(7, 14); else u2 = u1 + span();
    short_arc(pt, u1, u2);
    begin(N, "i f f"); W_i(pt); W_f(u1); W_f(u2); bar();
    emit([&] {
      if (cs_unchecked(pt, u1, u2)) { unchecked(); return; }
      Convert_ConicToBSplineCurve k(0, 0, 0);
      NCollection_Array1<double> c, s, d, kn; NCollection_Array1<int> m; int degree = 0;
      k.BuildCosAndSin((Convert_ParameterisationType)pt, u1, u2, c, s, d, degree, kn, m);
      put_cs(c, s, d, degree, kn, m);
    }, true);
  } }
  { LOOP("poly_cos_sin", 40) {
    double u1 = coin(0.5) ? ang() : rnd(-50, 50), u2;
    double r = rnd(0, 1);
    if (r < 0.05) u2 = u1; else if (r < 0.10) u2 = u1 - rnd(0.1, 3); else if (r < 0.15) u2 = u1 + 2 * M_PI + rnd(0.01, 1); else u2 = u1 + span();
    int np = coin(0.9) ? 8 : rint(2, 12);
    begin(N, "f f i"); W_f(u1); W_f(u2); W_i(np); bar();
    emit([&] {
      double d = u2 - u1;
      if (np != 8 || d <= 0.0 || d > 2 * M_PI + Precision::PConfusion()) { unchecked(); return; }
      NCollection_Array1<double> c(1, np), s(1, np), dd(1, np);
      BuildPolynomialCosAndSin(u1, u2, np, c, s, dd);
      o_a1(c); o_a1(s); o_a1(dd);
    }, true);
  } }
}

// =================================================================================
// elementary surfaces (word surf): fr3 frame, then by kind
//   0 cylinder (U1 U2 V1 V2)  R u1 u2 v1 v2      1 cylinder (periodic)  R v1 v2
//   2 cone (U1 U2 V1 V2)      angle R u1 u2 v1 v2   3 cone (periodic)   angle R v1 v2
//   4 sphere (U1 U2 V1 V2)    R u1 u2 v1 v2      5 sphere (Param1 Param2 UTrim)  R p1 p2 utrim   6 sphere  R
//   7 torus (U1 U2 V1 V2)     major minor u1 u2 v1 v2   8 torus (Param1 Param2 UTrim)  major minor p1 p2 utrim   9 torus  major minor
struct Sf { int kind; Fr3 f; double a, b, u1, u2, v1, v2; int ut; };
static void p_surf(const Sf& s) {
  in(s.kind); p_fr3(s.f);
  switch (s.kind) {
    case 0: h(s.a); h(s.u1); h(s.u2); h(s.v1); h(s.v2); break;
    case 1: h(s.a); h(s.v1); h(s.v2); break;
    case 2: h(s.a); h(s.b); h(s.u1); h(s.u2); h(s.v1); h(s.v2); break;
    case 3: h(s.a); h(s.b); h(s.v1); h(s.v2); break;
    case 4: h(s.a); h(s.u1); h(s.u2); h(s.v1); h(s.v2); break;
    case 5: h(s.a); h(s.u1); h(s.u2); in(s.ut); break;
    case 6: h(s.a); break;
    case 7: h(s.a); h(s.b); h(s.u1); h(s.u2); h(s.v1); h(s.v2); break;
    case 8: h(s.a); h(s.b); h(s.u1); h(s.u2); in(s.ut); break;
    default: h(s.a); h(s.b); break;
  }
}
static void W_surf(const Sf& s) { int st = ntok; p_surf(s); word("surf", st, -1); }
// u range of a trimmed surface: mostly valid (0 <= delta <= 2 pi), sometimes not
static void urange(double& u1, double& u2) {
  u1 = ang();
  double r = rnd(0, 1), d;
  if (r < 0.04) d = 0.0;
  else if (r < 0.07) d = -rnd(0.01, 3);
  else if (r < 0.10) d = 2 * M_PI + rnd(1e-9, 0.5);
  else d = span();
  u2 = u1 + d;
}
// v range along a line: V1 != V2 mostly, sometimes reversed or equal
static void vline(double& v1, double& v2) {
  v1 = coin(0.4) ? nice() : rnd(-5, 5);
  double r = rnd(0, 1);
  if (r < 0.05) v2 = v1;
  else if (r < 0.10) v2 = v1 - rnd(0.2, 4);
  else if (r < 0.13) v2 = v1 + rnd(1e-9, 1e-6);
  else v2 = v1 + rnd(0.2, 5);
}
// v range of a sphere (latitudes): within [-pi/2, pi/2] mostly
static void vlat(double& v1, double& v2) {
  double r = rnd(0, 1);
  if (r < 0.15) { v1 = -M_PI / 2; v2 = M_PI / 2; return; }
  if (r < 0.25) { v1 = -M_PI / 2; v2 = rnd(-1.4, 1.5); return; }
  if (r < 0.35) { v1 = rnd(-1.5, 1.4); v2 = M_PI / 2; return; }
  if (r < 0.40) { v1 = rnd(-1.5, 1.5); v2 = v1 - rnd(0.01, 2); return; }
  if (r < 0.43) { v1 = rnd(-1.6, -1.5); v2 = v1 + 1; return; }       // v1 below -pi/2
  if (r < 0.46) { v1 = 0.0; v2 = M_PI / 2 + rnd(1e-6, 0.3); return; } // v2 above pi/2
  v1 = rnd(-1.5, 1.4); v2 = v1 + rnd(0.01, 1.5);
  if (v2 > M_PI / 2) v2 = M_PI / 2;
}
static Sf rsurf(int kind) {
  Sf s; s.kind = kind; s.f = rfr3(); s.a = s.b = s.u1 = s.u2 = s.v1 = s.v2 = 0; s.ut = coin(0.5) ? 1 : 0;
  switch (kind) {
    case 0: s.a = radius(true); urange(s.u1, s.u2); vline(s.v1, s.v2); break;
    case 1: s.a = radius(true); vline(s.v1, s.v2); break;
    case 2: case 3: {
      double r = rnd(0, 1);
      double ang = r < 0.3 ? (M_PI / 4) * (coin(0.5) ? 1 : -1) * rnd(0.2, 1.0) : (r < 0.4 ? (coin(0.5) ? 1.0 : -1.0) * rnd(1e-3, 0.02) : (r < 0.5 ? (coin(0.5) ? 1.0 : -1.0) * rnd(1.5, 1.5707) : (coin(0.5) ? 1.0 : -1.0) * rnd(0.05, 1.5)));
      s.a = ang; s.b = coin(0.2) ? 0.0 : radius(true);
      if (kind == 2) urange(s.u1, s.u2);
      vline(s.v1, s.v2);
      if (coin(0.25) && std::sin(ang) != 0.0 && s.b > 0) {            // a range through the apex (R + V sin(A) = 0)
        double va = -s.b / std::sin(ang);
        s.v1 = va - rnd(0, 2); s.v2 = va + rnd(0, 2);
      }
      break;
    }
    case 4: s.a = radius(true); urange(s.u1, s.u2); vlat(s.v1, s.v2); break;
    case 5: {
      s.a = radius(true); s.u1 = ang();
      double r = rnd(0, 1);
      s.u2 = s.u1 + (r < 0.06 ? -rnd(0.01, 3) : (r < 0.1 ? 2 * M_PI + rnd(1e-6, 0.4) : (r < 0.15 ? 0.0 : (r < 0.25 ? rnd(5.0, 2 * M_PI) : span()))));
      break;
    }
    case 6: s.a = radius(true); break;
    case 7: s.a = radius(true); s.b = radius(true); urange(s.u1, s.u2); { double d; s.v1 = ang(); double r = rnd(0, 1); d = r < 0.05 ? 0.0 : (r < 0.08 ? -rnd(0.1, 2) : (r < 0.11 ? 2 * M_PI + rnd(1e-6, 0.3) : span())); s.v2 = s.v1 + d; } break;
    case 8: {
      s.a = radius(true); s.b = radius(true); s.u1 = ang();
      double r = rnd(0, 1);
      s.u2 = s.u1 + (r < 0.06 ? -rnd(0.01, 3) : (r < 0.1 ? 2 * M_PI + rnd(1e-6, 0.4) : span()));
      break;
    }
    default: s.a = radius(true); s.b = radius(true); break;
  }
  return s;
}
static bool surf_unchecked(const Sf& s) {
  const double du = s.u2 - s.u1;
  switch (s.kind) {
    case 0: return (std::abs(s.v2 - s.v1) <= std::abs(Epsilon(s.v1))) || (du > 2 * M_PI) || (du < 0.);
    case 1: return std::abs(s.v2 - s.v1) <= std::abs(Epsilon(s.v1));
    case 2: return (std::abs(s.v2 - s.v1) <= std::abs(Epsilon(s.v1))) || (du > 2 * M_PI) || (du < 0.);
    case 3: return std::abs(s.v2 - s.v1) <= std::abs(Epsilon(s.v1));
    case 4: return (du > 2 * M_PI) || (du < 0.) || (s.v1 < -M_PI / 2.0) || (s.v2 > M_PI / 2) || nspans(s.v2 - s.v1) < 1;
    case 5: return (du > 2 * M_PI) || (du < 0.) || (!s.ut && nspans(du) > 2);
    case 7: { double dv = s.v2 - s.v1; return (du > 2 * M_PI) || (du < 0.) || (dv > 2 * M_PI) || (dv < 0.); }
    case 8: return (du > 2 * M_PI) || (du < 0.);
    default: return false;
  }
}
static bool with_surf(const Sf& s, const std::function<void(const Convert_ElementarySurfaceToBSplineSurface&)>& use) {
  gp_Ax3 ax = mkAx3(s.f);
  switch (s.kind) {
    case 0: { gp_Cylinder g(ax, s.a); if (surf_unchecked(s)) return false; Convert_CylinderToBSplineSurface k(g, s.u1, s.u2, s.v1, s.v2); use(k); return true; }
    case 1: { gp_Cylinder g(ax, s.a); if (surf_unchecked(s)) return false; Convert_CylinderToBSplineSurface k(g, s.v1, s.v2); use(k); return true; }
    case 2: { gp_Cone g(ax, s.a, s.b); if (surf_unchecked(s)) return false; Convert_ConeToBSplineSurface k(g, s.u1, s.u2, s.v1, s.v2); use(k); return true; }
    case 3: { gp_Cone g(ax, s.a, s.b); if (surf_unchecked(s)) return false; Convert_ConeToBSplineSurface k(g, s.v1, s.v2); use(k); return true; }
    case 4: { gp_Sphere g(ax, s.a); if (surf_unchecked(s)) return false; Convert_SphereToBSplineSurface k(g, s.u1, s.u2, s.v1, s.v2); use(k); return true; }
    case 5: { gp_Sphere g(ax, s.a); if (surf_unchecked(s)) return false; Convert_SphereToBSplineSurface k(g, s.u1, s.u2, s.ut != 0); use(k); return true; }
    case 6: { gp_Sphere g(ax, s.a); Convert_SphereToBSplineSurface k(g); use(k); return true; }
    case 7: { gp_Torus g(ax, s.a, s.b); if (surf_unchecked(s)) return false; Convert_TorusToBSplineSurface k(g, s.u1, s.u2, s.v1, s.v2); use(k); return true; }
    case 8: { gp_Torus g(ax, s.a, s.b); if (surf_unchecked(s)) return false; Convert_TorusToBSplineSurface k(g, s.u1, s.u2, s.ut != 0); use(k); return true; }
    default: { gp_Torus g(ax, s.a, s.b); Convert_TorusToBSplineSurface k(g); use(k); return true; }
  }
}
static void put_surface(const Convert_ElementarySurfaceToBSplineSurface& k) {
  in(k.UDegree()); in(k.VDegree()); in(k.NbUPoles()); in(k.NbVPoles()); in(k.NbUKnots()); in(k.NbVKnots()); ob(k.IsUPeriodic()); ob(k.IsVPeriodic());
  o_a2p(k.Poles()); o_a2(k.Weights()); o_a1(k.UKnots()); o_a1(k.VKnots()); o_a1i(k.UMultiplicities()); o_a1i(k.VMultiplicities());
}
static void fam_surfaces() {
  g_guard = true;
  static const char* const NAMES[10] = {"cylinder_trim", "cylinder_full", "cone_trim", "cone_full", "sphere_trim4", "sphere_trim2", "sphere_full",
                                         "torus_trim4", "torus_trim2", "torus_full"};
  for (int kind = 0; kind < 10; ++kind) {
    LOOP(NAMES[kind], (kind == 6 || kind == 9) ? 24 : (kind == 1 ? 26 : 34)) {
      Sf s = rsurf(kind);
      begin(N, "surf"); W_surf(s); bar();
      emit([&] { if (!with_surf(s, put_surface)) unchecked(); }, true);
    }
  }
  { LOOP("surface_index", 50) {
    Sf s = rsurf(att % 10);
    int acc = rint(0, 5), i1 = rint(0, 9), i2 = rint(0, 8);
    if (coin(0.8)) { i1 = rint(1, 3); i2 = rint(1, 2); }
    begin(N, "surf i i i"); W_surf(s); W_i(acc); W_i(i1); W_i(i2); bar();
    emit([&] {
      bool ran = with_surf(s, [&](const Convert_ElementarySurfaceToBSplineSurface& k) {
        switch (acc) {
          case 0: case 1: {
            if (i1 < 1 || i1 > k.NbUPoles() || i2 < 1 || i2 > k.NbVPoles()) { unchecked(); return; }
            if (acc == 0) { gp_Pnt p = k.Pole(i1, i2); h(p.X()); h(p.Y()); h(p.Z()); } else h(k.Weight(i1, i2));
            break;
          }
          case 2: case 4: {
            if (i1 < 1 || i1 > k.NbUKnots()) { unchecked(); return; }
            if (acc == 2) h(k.UKnot(i1)); else in(k.UMultiplicity(i1));
            break;
          }
          default: {
            if (i1 < 1 || i1 > k.NbVKnots()) { unchecked(); return; }
            if (acc == 3) h(k.VKnot(i1)); else in(k.VMultiplicity(i1));
            break;
          }
        }
      });
      if (!ran) unchecked();
    }, true);
  } }
}

// =================================================================================
// composite Bezier curves (words bzc2 / bzc3: count of curves, then per curve its number of poles and coordinates)
typedef std::vector<Vd> Curves;   // each curve: flat coordinates, `dim` per pole
static void p_curves(const Curves& c, int dim) {
  in((long)c.size());
  for (const Vd& q : c) { in((long)(q.size() / dim)); for (double x : q) h(x); }
}
static void W_bzc(const Curves& c, int dim) { int s = ntok; p_curves(c, dim); word(dim == 2 ? "bzc2" : "bzc3", s, -1); }
// a list of Bezier curves, partly joined with tangent continuity
static Curves rcurves(int dim) {
  Curves out; int n = coin(0.08) ? 0 : rint(1, 5);
  int base = rint(1, 4);
  Vd prev;
  for (int k = 0; k < n; ++k) {
    int deg = coin(0.3) ? rint(1, 6) : base;       // no curve of one pole: BSplCLib::IncreaseDegree needs degree >= 1
    int np = deg + 1;
    Vd q((size_t)np * dim);
    for (double& x : q) x = coin(0.5) ? nice() : rnd(-5, 5);
    if (k > 0 && coin(0.7)) {
      size_t pn = prev.size() / dim;
      for (int d = 0; d < dim; ++d) q[(size_t)d] = prev[(pn - 1) * dim + d];         // G0
      if (pn >= 2 && np >= 2 && coin(0.8)) {                                         // G1 (with a small error sometimes)
        double lam = coin(0.5) ? 1.0 : (coin(0.5) ? rnd(0.2, 3) : (coin(0.5) ? 1e-3 : (coin(0.5) ? 1e-12 : 0.0)));
        double err = coin(0.5) ? 0.0 : (coin(0.3) ? 1e-6 : (coin(0.5) ? 1e-3 : 1e-5));
        for (int d = 0; d < dim; ++d) {
          double dirv = prev[(pn - 1) * dim + d] - prev[(pn - 2) * dim + d];
          q[(size_t)dim + d] = q[(size_t)d] + lam * dirv + err * rnd(-1, 1) * (1 + std::abs(dirv));
        }
        if (coin(0.1)) for (int d = 0; d < dim; ++d) q[(size_t)dim + d] = q[(size_t)d];                 // a zero first handle
      }
    }
    out.push_back(q); prev = q;
  }
  return out;
}
static const double TOLS[] = {1e-4, 1e-4, 1e-4, 0.0, 1e-10, 1e-2, 0.5, 3.2, 1e-6};
template <class C, class P, int DIM> static void run_bezier(double tol, const Curves& cs, int split) {
  C conv(tol);
  auto add = [&](size_t k) {
    const Vd& q = cs[k]; int np = (int)(q.size() / DIM);
    NCollection_Array1<P> a(1, np);
    for (int i = 0; i < np; ++i) {
      if (DIM == 2) a(i + 1) = P(); 
      for (int d = 0; d < DIM; ++d) a(i + 1).SetCoord(d + 1, q[(size_t)i * DIM + d]);
    }
    conv.AddCurve(a);
  };
  auto block = [&]() {
    conv.Perform();
    int np = conv.NbPoles(), nk = conv.NbKnots();
    in(conv.Degree()); in(np); in(nk);
    if (np > 0) { NCollection_Array1<P> pa(1, np); conv.Poles(pa); for (int i = 1; i <= np; ++i) for (int d = 0; d < DIM; ++d) h(pa(i).Coord(d + 1)); }
    if (nk > 0) {
      NCollection_Array1<double> ka(1, nk); NCollection_Array1<int> ma(1, nk); conv.KnotsAndMults(ka, ma);
      for (int i = 1; i <= nk; ++i) h(ka(i));
      for (int i = 1; i <= nk; ++i) in(ma(i));
    }
  };
  for (int k = 0; k < split; ++k) add((size_t)k);
  block();
  for (size_t k = (size_t)split; k < cs.size(); ++k) add(k);
  block();
}
static void fam_bezier() {
  g_guard = true;
  { LOOP("bezier3d", 44) {
    Curves cs = rcurves(3); double tol = pick(TOLS); int split = rint(0, (int)cs.size());
    begin(N, "f bzc3 i"); W_f(tol); W_bzc(cs, 3); W_i(split); bar();
    emit([&] { run_bezier<Convert_CompBezierCurvesToBSplineCurve, gp_Pnt, 3>(tol, cs, split); }, true);
  } }
  { LOOP("bezier2d", 44) {
    Curves cs = rcurves(2); double tol = pick(TOLS); int split = rint(0, (int)cs.size());
    begin(N, "f bzc2 i"); W_f(tol); W_bzc(cs, 2); W_i(split); bar();
    emit([&] { run_bezier<Convert_CompBezierCurves2dToBSplineCurve2d, gp_Pnt2d, 2>(tol, cs, split); }, true);
  } }
}

// =================================================================================
// piecewise polynomials: Convert_CompPolynomialToPoles
//   cpoly_uniform: n continuity dim maxdeg nc coef pint tint        (words i i i i iv fv fv fv)
//   cpoly_array:   n dim maxdeg cont nc coef pint tint
//   cpoly_single:  dim maxdeg degree coef pint tint
// pint of the first two is a flat list of 2n floats (row by row), of the single one its own list.
struct CP { int n, cont, dim, maxdeg; std::vector<int> contv, nc; Vd coef, pint, tint; int degree; };
static int rdegree() {
  double r = rnd(0, 1);
  if (r < 0.55) return rint(1, 5);
  if (r < 0.80) return rint(3, 8);
  if (r < 0.92) return rint(9, 14);
  static const int BIG[] = {16, 20, 25};
  return pick(BIG);
}
static CP rcp(int att, int mode) {   // mode 0 uniform, 1 array, 2 single
  CP c; c.n = mode == 2 ? 1 : rint(1, 5); c.degree = 0;
  int D = att % 2 == 0 ? 1 + (att / 2) % 25 : rdegree();   // every degree 1..25 occurs
  bool big = D > 9;
  if (big) c.n = rint(1, 2);
  c.dim = big ? rint(1, 2) : rint(1, 4);
  double r = rnd(0, 1);
  c.maxdeg = r < 0.55 ? D : (r < 0.75 ? D + rint(1, 3) : (r < 0.9 ? std::max(1, D - rint(1, 3)) : 25));
  c.cont = coin(0.35) ? 0 : (coin(0.5) ? 1 : (coin(0.6) ? 2 : rint(0, D)));
  if (att % 2 == 0 && D >= 3) c.cont = (att / 2) % 3;        // continuity 0, 1, 2 with every degree
  if (mode == 2) c.cont = 0;
  // numbers of coefficients per curve
  c.nc.assign((size_t)c.n, 0);
  int top = 0;
  for (int k = 0; k < c.n; ++k) { c.nc[(size_t)k] = coin(0.6) ? D + 1 : std::max(1, D + 1 - rint(0, 3)); top = std::max(top, c.nc[(size_t)k] - 1); }
  if (att % 2 == 0) c.nc[0] = D + 1;       // the scheduled degree is really reached
  if (mode != 2 && c.nc.size() > 0 && *std::max_element(c.nc.begin(), c.nc.end()) < 2) c.nc[0] = 2;   // no curve set of degree 0 (BSplCLib::Interpolate needs degree >= 1)
  top = 0; for (int x : c.nc) top = std::max(top, x - 1);
  if (mode == 2) { c.degree = D; c.nc.assign(1, D + 1); top = D; }
  int S = std::max(c.maxdeg, mode == 2 ? c.degree : top);
  if (mode == 2) S = c.maxdeg;     // the single-curve constructor does not use the stride of the others: see the layout below
  size_t stride = (size_t)c.dim * (size_t)(std::max(c.maxdeg, mode == 2 ? c.degree : top) + 1);
  (void)S;
  c.coef.assign(stride * (size_t)c.n, 0.0);
  for (int k = 0; k < c.n; ++k) {
    for (size_t j = 0; j < stride; ++j) {
      bool used = j < (size_t)c.nc[(size_t)k] * c.dim;
      double scale = used ? 1.0 / (1 + (j / (size_t)c.dim)) : 1.0;
      double v = coin(0.2) ? nice() : rnd(-2, 2);
      if (used && coin(0.1)) v = 0.0;
      c.coef[(size_t)k * stride + j] = v * scale;
    }
  }
  // intervals
  double t0 = coin(0.5) ? nice() : rnd(-3, 3);
  c.tint.assign(1, t0);
  for (int k = 0; k < c.n; ++k) c.tint.push_back(c.tint.back() + (coin(0.3) ? 1.0 : rnd(0.1, 3)));
  for (int k = 0; k < c.n; ++k) {
    double a = coin(0.4) ? (coin(0.5) ? -1.0 : 0.0) : rnd(-2, 1), b = coin(0.4) ? 1.0 : a + rnd(0.2, 3);
    if (coin(0.05)) std::swap(a, b);
    c.pint.push_back(a); c.pint.push_back(b);
  }
  if (mode == 1) { c.contv.assign((size_t)c.n, 0); for (int& x : c.contv) x = coin(0.4) ? 0 : (coin(0.5) ? 1 : (coin(0.6) ? 2 : rint(0, D))); }
  // invalid variants
  double q = rnd(0, 1);
  if (att % 9 == 8) {
    if (q < 0.2) c.n = coin(0.5) ? 0 : -1;
    else if (q < 0.4) c.dim = coin(0.5) ? 0 : -2;
    else if (q < 0.6) c.maxdeg = coin(0.5) ? 0 : -1;
    else if (q < 0.7 && mode == 0) c.cont = -1;
    else if (q < 0.85 && mode == 0) c.cont = D + 1 + rint(0, 2);
    else if (mode == 1 && c.n > 1) c.contv[(size_t)rint(1, c.n - 1)] = D + 1 + rint(0, 2);
  }
  return c;
}
// whether a constructor raises before it reads anything (an explicit `throw` of OCCT)
static bool cp_pre_throw(const CP& c, int mode) {
  if (mode == 2) return c.maxdeg <= 0 || c.dim <= 0 || c.pint.size() != 2;
  if (mode == 0) return c.n <= 0 || c.cont < 0 || c.maxdeg <= 0 || c.dim <= 0;
  return c.n <= 0 || c.maxdeg <= 0 || c.dim <= 0;
}
// whether the call reads outside one of the arrays (not executed)
static bool cp_oob(const CP& c, int mode) {
  if (mode == 2) {
    size_t need = (size_t)c.dim * (size_t)(c.degree + 1);
    return c.coef.size() < need || c.tint.size() < 2;
  }
  if (mode == 1 && c.n > 1 && (int)c.contv.size() < c.n) return true;
  if ((int)c.nc.size() < c.n || (int)c.tint.size() < c.n + 1 || (int)c.pint.size() < 2 * c.n) return true;
  int top = 0;
  for (int k = 0; k < c.n; ++k) { if (c.nc[(size_t)k] < 1) return true; top = std::max(top, c.nc[(size_t)k] - 1); }
  return c.coef.size() < (size_t)c.n * (size_t)c.dim * (size_t)(std::max(c.maxdeg, top) + 1);
}
static void put_cp(const Convert_CompPolynomialToPoles& p) {
  ob(p.IsDone()); in(p.Degree()); in(p.NbPoles()); o_a2(p.Poles()); in(p.NbKnots()); o_a1(p.Knots()); o_a1i(p.Multiplicities());
}
template <class T> static NCollection_Array1<T> mk_a1(const std::vector<T>& v) {
  NCollection_Array1<T> a(1, std::max((int)v.size(), 1));
  for (size_t i = 0; i < v.size(); ++i) a((int)i + 1) = v[i];
  return a;
}
static void fam_cpoly() {
  g_guard = true;
  for (int mode = 0; mode < 3; ++mode) {
    const char* name = mode == 0 ? "cpoly_uniform" : (mode == 1 ? "cpoly_array" : "cpoly_single");
    LOOP(name, 54) {
      CP c = rcp(att, mode);
      bool shorten = att % 17 == 16 && att % 9 != 8;
      if (shorten) {
        double q = rnd(0, 1);
        if (q < 0.3 && !c.coef.empty()) c.coef.resize(c.coef.size() / 3);
        else if (q < 0.5) c.tint.resize(c.tint.size() > 1 ? c.tint.size() - 1 : 0);
        else if (q < 0.7 && mode != 2) c.nc.resize(c.nc.size() > 0 ? c.nc.size() - 1 : 0);
        else if (q < 0.85 && mode != 2) c.pint.resize(0);
        else if (mode == 1 && c.n > 1) c.contv.resize(c.contv.size() - 1);
      }
      if (mode == 2) {
        // the single-curve constructor reads the coefficients of one curve: dim * (degree + 1) values (more are allowed)
        size_t need = (size_t)std::max(c.dim, 0) * (size_t)(c.degree + 1);
        size_t have = need + (coin(0.2) ? (size_t)rint(0, 3) : 0);
        if (shorten && coin(0.5)) have /= 2;
        c.coef.assign(have, 0.0);
        for (size_t j = 0; j < have; ++j) c.coef[j] = coin(0.2) ? nice() : rnd(-2, 2) / (1.0 + (double)(j / (size_t)std::max(c.dim, 1)));
        c.pint.resize(2);
        if (att % 13 == 12) c.pint.push_back(rnd(0, 1));       // PolynomialIntervals.Length() != 2 throws
      }
      if (mode == 0) { begin(N, "i i i i iv fv fv fv"); W_i(c.n); W_i(c.cont); W_i(c.dim); W_i(c.maxdeg); }
      else if (mode == 1) { begin(N, "i i i iv iv fv fv fv"); W_i(c.n); W_i(c.dim); W_i(c.maxdeg); W_iv(c.contv); }
      else { begin(N, "i i i fv fv fv"); W_i(c.dim); W_i(c.maxdeg); W_i(c.degree); }
      if (mode == 2) { W_fv(c.coef); W_fv(c.pint); W_fv(c.tint); }
      else { W_iv(c.nc); W_fv(c.coef); W_fv(c.pint); W_fv(c.tint); }
      bar();
      emit([&] {
        if (!cp_pre_throw(c, mode) && cp_oob(c, mode)) { unchecked(); return; }
        if (mode == 0) {
          occ::handle<NCollection_HArray1<int>> nc = new NCollection_HArray1<int>(1, std::max((int)c.nc.size(), 1));
          for (size_t i = 0; i < c.nc.size(); ++i) nc->SetValue((int)i + 1, c.nc[i]);
          occ::handle<NCollection_HArray1<double>> cf = new NCollection_HArray1<double>(1, std::max((int)c.coef.size(), 1));
          for (size_t i = 0; i < c.coef.size(); ++i) cf->SetValue((int)i + 1, c.coef[i]);
          occ::handle<NCollection_HArray2<double>> pi = new NCollection_HArray2<double>(1, std::max((int)(c.pint.size() / 2), 1), 1, 2);
          for (size_t i = 0; i < c.pint.size() / 2; ++i) { pi->SetValue((int)i + 1, 1, c.pint[2 * i]); pi->SetValue((int)i + 1, 2, c.pint[2 * i + 1]); }
          occ::handle<NCollection_HArray1<double>> ti = new NCollection_HArray1<double>(1, std::max((int)c.tint.size(), 1));
          for (size_t i = 0; i < c.tint.size(); ++i) ti->SetValue((int)i + 1, c.tint[i]);
          Convert_CompPolynomialToPoles p(c.n, c.cont, c.dim, c.maxdeg, nc, cf, pi, ti);
          put_cp(p);
        } else if (mode == 1) {
          NCollection_Array1<int> cont = mk_a1(c.contv), nc = mk_a1(c.nc);
          NCollection_Array1<double> cf = mk_a1(c.coef), ti = mk_a1(c.tint);
          NCollection_Array2<double> pi(1, std::max((int)(c.pint.size() / 2), 1), 1, 2);
          for (size_t i = 0; i < c.pint.size() / 2; ++i) { pi((int)i + 1, 1) = c.pint[2 * i]; pi((int)i + 1, 2) = c.pint[2 * i + 1]; }
          Convert_CompPolynomialToPoles p(c.n, c.dim, c.maxdeg, cont, nc, cf, pi, ti);
          put_cp(p);
        } else {
          NCollection_Array1<double> cf = mk_a1(c.coef), pi = mk_a1(c.pint), ti = mk_a1(c.tint);
          Convert_CompPolynomialToPoles p(c.dim, c.maxdeg, c.degree, cf, pi, ti);
          put_cp(p);
        }
      });
    }
  }
}

// =================================================================================
// Convert_GridPolynomialToPoles
//   gpoly_single: maxu maxv nc(iv, 2 entries) coef pu pv                         (i i iv fv fv fv)
//   gpoly_multi:  nu nv ucont vcont maxu maxv nc(ipairs) coef pu pv tu tv       (i i i i i i ipairs fv fv fv fv fv)
static void p_ipairs(const std::vector<int>& v) { in((long)(v.size() / 2)); for (int x : v) in(x); }
static void W_ipairs(const std::vector<int>& v) { int s = ntok; p_ipairs(v); word("ipairs", s, -1); }
struct GP { int nu, nv, uc, vc, mu, mv; std::vector<int> nc; Vd coef, pu, pv, tu, tv; };
static Vd incr(int n) {   // n strictly increasing values
  Vd v(1, coin(0.5) ? nice() : rnd(-3, 3));
  for (int k = 1; k < n; ++k) v.push_back(v.back() + (coin(0.3) ? 1.0 : rnd(0.2, 2.5)));
  return v;
}
static GP rgp(int att, int mode) {
  GP g; g.nu = g.nv = 1; g.uc = g.vc = 0;
  int Du = coin(0.7) ? rint(1, 4) : rint(1, 7), Dv = coin(0.7) ? rint(1, 4) : rint(1, 7);
  if (mode == 1) { g.nu = coin(0.8) ? rint(1, 2) : 3; g.nv = coin(0.8) ? rint(1, 2) : 3; Du = std::min(Du, 4); Dv = std::min(Dv, 4);
    g.uc = coin(0.3) ? 0 : (coin(0.5) ? 1 : (coin(0.7) ? 2 : rint(0, Du))); g.vc = coin(0.3) ? 0 : (coin(0.5) ? 1 : (coin(0.7) ? 2 : rint(0, Dv))); }
  g.mu = Du + (coin(0.5) ? 0 : rint(-1, 2)); g.mv = Dv + (coin(0.5) ? 0 : rint(-1, 2));
  if (g.mu < 1) g.mu = 1;
  if (g.mv < 1) g.mv = 1;
  int rU = mode == 1 ? std::max(g.mu, 2 * g.uc + 1) : g.mu, rV = mode == 1 ? std::max(g.mv, 2 * g.vc + 1) : g.mv;
  int patches = g.nu * g.nv;
  for (int k = 0; k < patches; ++k) {
    int a = mode == 0 || k == 0 || coin(0.5) ? Du + 1 : rint(1, Du + 1), b = mode == 0 || k == 0 || coin(0.5) ? Dv + 1 : rint(1, Dv + 1);
    g.nc.push_back(a); g.nc.push_back(b);
  }
  size_t blk = (size_t)3 * (rU + 1) * (rV + 1);
  g.coef.assign(blk * (size_t)patches, 0.0);
  for (int p = 0; p < patches; ++p)
    for (size_t j = 0; j < blk; ++j) g.coef[(size_t)p * blk + j] = (coin(0.2) ? nice() : rnd(-2, 2)) * (coin(0.1) ? 0.0 : 1.0);
  Vd t = incr(2); g.pu = t; t = incr(2); g.pv = t;
  if (mode == 0) { g.pu = incr(2); g.pv = incr(2); }
  g.tu = incr(g.nu + 1); g.tv = incr(g.nv + 1);
  // invalid variants
  double q = rnd(0, 1);
  if (att % 7 == 6) {
    if (q < 0.25) { if (mode == 0) { g.nc.push_back(1); } else g.nc.resize(g.nc.size() > 2 ? g.nc.size() - 2 : 0); }
    else if (q < 0.5) g.coef.resize(g.coef.size() + (coin(0.5) ? 1 : 0) - (g.coef.size() > 3 ? 3 : 0));
    else if (q < 0.75) g.nc[0] = rU + 2 + rint(0, 2);
    else if (q < 0.85 && mode == 0) { g.nc.resize(1); }
    else g.nc[1] = rV + 2;
  } else if (att % 11 == 10) {
    if (coin(0.5)) g.pu.resize(1); else g.pv.resize(rint(0, 1));
  }
  return g;
}
static bool gp_pre_throw(const GP& g, int mode) {
  if (mode == 0) {
    if (g.nc.size() != 2) return true;
    if ((long)g.coef.size() != 3L * (g.mu + 1) * (g.mv + 1)) return true;
    return g.nc[0] - 1 > g.mu || g.nc[1] - 1 > g.mv;
  }
  int rU = std::max(g.mu, 2 * g.uc + 1), rV = std::max(g.mv, 2 * g.vc + 1);
  if ((long)(g.nc.size() / 2) != (long)g.nu * g.nv || g.nc.size() % 2) return true;
  if ((long)g.coef.size() != 3L * g.nu * g.nv * (rU + 1) * (rV + 1)) return true;
  int du = 0, dv = 0;
  for (size_t k = 0; k + 1 < g.nc.size(); k += 2) { du = std::max(du, g.nc[k] - 1); dv = std::max(dv, g.nc[k + 1] - 1); }
  return du > rU || dv > rV;
}
// a continuity larger than the degree gives negative multiplicities of the inner knots (memory corruption in OCCT)
static bool gp_negative_mult(const GP& g) {
  int du = 0, dv = 0;
  for (size_t k = 0; k + 1 < g.nc.size(); k += 2) { du = std::max(du, g.nc[k] - 1); dv = std::max(dv, g.nc[k + 1] - 1); }
  return (g.nu > 1 && g.uc > du) || (g.nv > 1 && g.vc > dv);
}
static void fam_gpoly() {
  g_guard = true;
  for (int mode = 0; mode < 2; ++mode) {
    LOOP(mode == 0 ? "gpoly_single" : "gpoly_multi", mode == 0 ? 32 : 30) {
      GP g = rgp(att, mode);
      if (mode == 0) { begin(N, "i i iv fv fv fv"); W_i(g.mu); W_i(g.mv); W_iv(g.nc); }
      else { begin(N, "i i i i i i ipairs fv fv fv fv fv"); W_i(g.nu); W_i(g.nv); W_i(g.uc); W_i(g.vc); W_i(g.mu); W_i(g.mv); W_ipairs(g.nc); }
      W_fv(g.coef); W_fv(g.pu); W_fv(g.pv);
      if (mode == 1) { W_fv(g.tu); W_fv(g.tv); }
      bar();
      emit([&] {
        if (!gp_pre_throw(g, mode) && (g.pu.size() < 2 || g.pv.size() < 2 || (mode == 1 && gp_negative_mult(g)))) { unchecked(); return; }
        NCollection_Array1<double> cf = mk_a1(g.coef), pu = mk_a1(g.pu), pv = mk_a1(g.pv);
        std::unique_ptr<Convert_GridPolynomialToPoles> p;
        if (mode == 0) {
          NCollection_Array1<int> nc = mk_a1(g.nc);
          p.reset(new Convert_GridPolynomialToPoles(g.mu, g.mv, nc, cf, pu, pv));
        } else {
          NCollection_Array2<int> nc(1, std::max((int)(g.nc.size() / 2), 1), 1, 2);
          for (size_t k = 0; k + 1 < g.nc.size(); k += 2) { nc((int)(k / 2) + 1, 1) = g.nc[k]; nc((int)(k / 2) + 1, 2) = g.nc[k + 1]; }
          if (g.nc.size() / 2 != (size_t)(g.nu * g.nv)) nc.Resize(1, g.nu * g.nv, 1, 2, false);
          NCollection_Array1<double> tu = mk_a1(g.tu), tv = mk_a1(g.tv);
          p.reset(new Convert_GridPolynomialToPoles(g.nu, g.nv, g.uc, g.vc, g.mu, g.mv, nc, cf, pu, pv, tu, tv));
        }
        ob(p->IsDone());
        if (!p->IsDone()) { buf += " unchecked"; ++ntok; return; }
        in(p->UDegree()); in(p->VDegree()); in(p->NbUPoles()); in(p->NbVPoles()); in(p->NbUKnots()); in(p->NbVKnots());
        o_a2p(p->Poles()); o_a1(p->UKnots()); o_a1(p->VKnots()); o_a1i(p->UMultiplicities()); o_a1i(p->VMultiplicities());
      });
    }
  }
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
  if (want("conics")) fam_conics();
  if (want("surfaces")) fam_surfaces();
  if (want("bezier")) fam_bezier();
  if (want("cpoly")) fam_cpoly();
  if (want("gpoly")) fam_gpoly();
  for (const std::string& n : fam_order)
    std::fprintf(stderr, "%-24s %5d lines, %5d dropped (%d uninit)\n", n.c_str(), fam_lines[n], fam_dropped.count(n) ? fam_dropped[n] : 0,
                 fam_dropped.count(n + "!uninit") ? fam_dropped[n + "!uninit"] : 0);
  for (auto& kv : fam_dropped) if (kv.first.find("!libm") != std::string::npos || kv.first.find("!crash") != std::string::npos)
    std::fprintf(stderr, "   dropped: %s %d\n", kv.first.c_str(), kv.second);
  return 0;
}
