// Deterministic reference calls for OCCT 8.0.1's locations and mesh data structures:
//   package TopLoc: TopLoc_Location, TopLoc_Datum3D, TopLoc_ItemLocation, TopLoc_SListOfItemLocation
//   package Poly:   Poly_Triangulation, Poly_Triangle, Poly_Polygon3D/2D, Poly_PolygonOnTriangulation, Poly_Connect, Poly::*,
//                   Poly_MergeNodesTool, Poly_MakeLoops (3D, 2D, HeapOfInteger), TColStd_PackedMapOfInteger,
//                   Poly_CoherentTriangulation (+ node, link, triangle).
//
// Output: one record per line, `<family> <inputs...> | <outputs...>`; floats are 16-hex-digit f64 bit patterns, ints and bools
// decimal, texts one token (spaces `_`, newlines `\n`; input texts start with `T`). Every input is printed on the left in the order
// of the family's schema (`begin(name, schema)`); the oracle aborts if the printed words differ from the schema. The grammar of the
// composite words (tri, lp, lprog, tops, pg3, pot, mops, loops, mapops, cops ...) is in docs/api-poly.md. Outputs are values,
// `throws` (OCCT raised an exception) or `unchecked` (a check is compiled out of the libraries or the result is unspecified: the
// call is not executed and the Rust API returns an error).
// Every call is run 6 times with differently filled stack and heap (random words, all 0, all 1; malloc perturbation); a case whose
// results differ, that is not finite, or (read families, coherent and loop programs) that crashes in a child process, is dropped.
// Families that call cos / atan2 only keep cases whose outputs are unchanged when every C library result is moved by an ulp.
// Merge cases must give the same result for 9 different bucket counts of the node map.
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
#include <mutex>
#include <set>
#include <shared_mutex>
#include <sstream>
#include <string>
#include <vector>
#include <malloc.h>
#include <unistd.h>
#include <sys/wait.h>
// TopLoc_ItemLocation keeps its datum, power and cumulative transformation in private members; the test reads them
// as `datum()`, `power()` and `transformation()` of the Rust API, so they are made reachable here.
#define private public
#include <TopLoc_ItemLocation.hxx>
#include <TopLoc_SListOfItemLocation.hxx>
#undef private
#include <Bnd_Box.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_HArray1.hxx>
#include <NCollection_List.hxx>
#include <NCollection_Vec3.hxx>
#include <Poly.hxx>
#define protected public
#include <Poly_CoherentTriangulation.hxx>
#undef protected
#include <Poly_CoherentLink.hxx>
#include <Poly_CoherentNode.hxx>
#include <Poly_CoherentTriangle.hxx>
#include <Poly_Connect.hxx>
#include <Poly_MakeLoops.hxx>
#include <Poly_MergeNodesTool.hxx>
#include <Poly_MeshPurpose.hxx>
#include <Poly_Polygon2D.hxx>
#include <Poly_Polygon3D.hxx>
#include <Poly_PolygonOnTriangulation.hxx>
#include <Poly_Triangle.hxx>
#include <Poly_Triangulation.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <TColStd_PackedMapOfInteger.hxx>
#include <TopLoc_Datum3D.hxx>
#include <TopLoc_Location.hxx>
#include <gp.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Quaternion.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <gp_XY.hxx>
#include <gp_XYZ.hxx>
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
// printing of gp values
static void xyz(const gp_XYZ& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void pnt(const gp_Pnt& v) { xyz(v.XYZ()); }
static void vec(const gp_Vec& v) { xyz(v.XYZ()); }
static void dir(const gp_Dir& v) { xyz(v.XYZ()); }
static void xy(const gp_XY& v) { h(v.X()); h(v.Y()); }
static void pnt2d(const gp_Pnt2d& v) { h(v.X()); h(v.Y()); }
struct Raw { double x, y, z; };
static gp_Dir D(const Raw& r) { return gp_Dir(r.x, r.y, r.z); }
static void rw(const Raw& r) { h(r.x); h(r.y); h(r.z); }
struct Raw2 { double x, y; };
static gp_Dir2d D2(const Raw2& r) { return gp_Dir2d(r.x, r.y); }
static void rw2(const Raw2& r) { h(r.x); h(r.y); }
static void W_xyz(const gp_XYZ& v) { int s = ntok; xyz(v); word("xyz", s, 3); }
static void W_pnt(const gp_Pnt& v) { int s = ntok; pnt(v); word("pnt", s, 3); }
static void W_vec(const gp_Vec& v) { int s = ntok; vec(v); word("vec", s, 3); }
static void W_dir(const Raw& r) { int s = ntok; rw(r); word("dir", s, 3); }
static void W_xy(const gp_XY& v) { int s = ntok; xy(v); word("xy", s, 2); }
static void W_pnt2d(const gp_Pnt2d& v) { int s = ntok; pnt2d(v); word("pnt2d", s, 2); }
static void W_dir2d(const Raw2& r) { int s = ntok; rw2(r); word("dir2d", s, 2); }

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
  if (std::getenv("POLY_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
  return WIFEXITED(st) && WEXITSTATUS(st) == 0;
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
// transformation recipes (see the header comment)
static double nice() { return 0.5 * rint(-8, 8); }
static double crd() { return coin(0.4) ? nice() : rnd(-6, 6); }
static Raw rraw() { for (;;) { Raw r{rnd(-4, 4), rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y + r.z * r.z > 0.25) return r; } }
static Raw nraw() { for (;;) { Raw r{(double)rint(-1, 1), (double)rint(-1, 1), (double)rint(-1, 1)}; if (r.x != 0 || r.y != 0 || r.z != 0) return r; } }
static Raw mraw() { return coin(0.5) ? nraw() : rraw(); }
static Raw2 rraw2() { for (;;) { Raw2 r{rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y > 0.25) return r; } }
static Raw2 nraw2() { for (;;) { Raw2 r{(double)rint(-1, 1), (double)rint(-1, 1)}; if (r.x != 0 || r.y != 0) return r; } }
static Raw2 mraw2() { return coin(0.5) ? nraw2() : rraw2(); }
static gp_Pnt rpnt() { return gp_Pnt(crd(), crd(), crd()); }
static gp_Pnt2d rpnt2d() { return gp_Pnt2d(crd(), crd()); }

struct TR { int kind; gp_Pnt p; Raw d; gp_XYZ v; double sc; double q[4]; gp_Trsf t; };
static TR mkTR(int kind) {
  TR r; r.kind = kind; r.p = rpnt(); r.d = mraw(); r.v = gp_XYZ(crd(), crd(), crd()); r.sc = coin(0.3) ? (coin(0.5) ? 2.0 : 0.5) : (coin(0.5) ? rnd(0.3, 2.5) : -rnd(0.3, 2.5));
  for (;;) {
    for (int k = 0; k < 4; ++k) r.q[k] = rnd(-1, 1);
    double n = std::sqrt(r.q[0] * r.q[0] + r.q[1] * r.q[1] + r.q[2] * r.q[2] + r.q[3] * r.q[3]);
    if (n > 0.3) { for (int k = 0; k < 4; ++k) r.q[k] /= n; break; }
  }
  if (r.kind == 7 || r.kind == 8) { if (coin(0.3)) { r.q[0] = 0; r.q[1] = 0; r.q[2] = std::sqrt(0.5); r.q[3] = std::sqrt(0.5); } }
  gp_Quaternion qq(r.q[0], r.q[1], r.q[2], r.q[3]);
  switch (kind) {
    case 2: r.t.SetScale(r.p, r.sc); break;
    case 3: r.t.SetTranslation(gp_Vec(r.v)); break;
    case 4: r.t.SetMirror(gp_Ax1(r.p, D(r.d))); break;
    case 5: r.t.SetMirror(r.p); break;
    case 6: r.t.SetMirror(gp_Ax2(r.p, D(r.d))); break;
    case 7: r.t.SetRotation(qq); break;
    case 8: r.t.SetRotation(qq); r.t.SetTranslationPart(gp_Vec(r.v)); r.t.SetScaleFactor(r.sc); break;
    default: break;
  }
  return r;
}
static void p_trsf(const TR& r) {
  in(r.kind);
  switch (r.kind) {
    case 2: pnt(r.p); h(r.sc); break;
    case 3: xyz(r.v); break;
    case 4: pnt(r.p); rw(r.d); break;
    case 5: pnt(r.p); break;
    case 6: pnt(r.p); rw(r.d); break;
    case 7: for (int k = 0; k < 4; ++k) h(r.q[k]); break;
    case 8: for (int k = 0; k < 4; ++k) h(r.q[k]); xyz(r.v); h(r.sc); break;
    default: break;
  }
}
static void W_trsf(const TR& r) { int s = ntok; p_trsf(r); word("trsf", s, -1); }
static const int TK3[] = {9, 3, 2, 3, 7, 8, 4, 5, 6, 3, 9, 3};
static TR rtr(int n) { return mkTR(TK3[n % 12]); }
// =================================================================================
// Bnd_Box recipes
struct Bx { int kind; double c[6]; double gap; int flags; };
static Bx BX(int kind, double a, double b, double c, double d, double e, double f, double gap = 0, int flags = 0) {
  Bx s; s.kind = kind; s.c[0] = a; s.c[1] = b; s.c[2] = c; s.c[3] = d; s.c[4] = e; s.c[5] = f; s.gap = gap; s.flags = flags; return s;
}
static Bnd_Box mkB(const Bx& s) {
  Bnd_Box b;
  switch (s.kind) {
    case 1: b = Bnd_Box(gp_Pnt(s.c[0], s.c[1], s.c[2]), gp_Pnt(s.c[3], s.c[4], s.c[5])); break;
    case 2: b.Update(s.c[0], s.c[1], s.c[2], s.c[3], s.c[4], s.c[5]); break;
    case 3: b.SetWhole(); break;
    case 4: b.Update(s.c[0], s.c[1], s.c[2]); break;
    default: break;
  }
  b.SetGap(s.gap);
  if (s.flags & 1) b.OpenXmin();
  if (s.flags & 2) b.OpenXmax();
  if (s.flags & 4) b.OpenYmin();
  if (s.flags & 8) b.OpenYmax();
  if (s.flags & 16) b.OpenZmin();
  if (s.flags & 32) b.OpenZmax();
  return b;
}
static void W_bx(const Bx& s) {
  int st = ntok; in(s.kind); for (int k = 0; k < 6; ++k) h(s.c[k]); h(s.gap); in(s.flags); word("bx", st, 9);
}
static const int NEDGE_BX = 26;
static Bx edge_bx(int i) {
  switch (((i % NEDGE_BX) + NEDGE_BX) % NEDGE_BX) {
    case 0: return BX(0, 0, 0, 0, 0, 0, 0);
    case 1: return BX(3, 0, 0, 0, 0, 0, 0);
    case 2: return BX(0, 0, 0, 0, 0, 0, 0, 0.5);
    case 3: return BX(0, 0, 0, 0, 0, 0, 0, 0, 63);
    case 4: return BX(0, 0, 0, 0, 0, 0, 0, 0, 1);
    case 5: return BX(1, 0, 0, 0, 1, 1, 1);
    case 6: return BX(2, 0, 0, 0, 1, 1, 1, 0.25);
    case 7: return BX(2, 0, 0, 2, 1, 1, 2);
    case 8: return BX(4, 1, 2, 3, 0, 0, 0);
    case 9: return BX(1, 1, 1, 1, 0, 0, 0);
    case 10: return BX(2, 0, 0, 0, 1, 1, 1, 0, 1);
    case 11: return BX(2, 0, 0, 0, 1, 1, 1, 0, 2);
    case 12: return BX(2, 0, 0, 0, 1, 1, 1, 0, 4);
    case 13: return BX(2, 0, 0, 0, 1, 1, 1, 0, 8);
    case 14: return BX(2, 0, 0, 0, 1, 1, 1, 0, 16);
    case 15: return BX(2, 0, 0, 0, 1, 1, 1, 0, 32);
    case 16: return BX(2, -1, -1, -1, 2, 2, 2, 0, 62);
    case 17: return BX(2, 0, 0, 0, 1, 1, 1, 0, 63);
    case 18: return BX(2, 0, 0, 0, 1, 1, 1, -0.5);
    case 19: return BX(2, -1e10, -1e10, -1e10, 1e10, 1e10, 1e10);
    case 20: return BX(2, -1, -1, -1, 1, 1, 1, 1.0);
    case 21: return BX(3, 0, 0, 0, 0, 0, 0, 0.3);
    case 22: return BX(2, -1e99, -1e99, -1e99, 1e99, 1e99, 1e99);
    case 23: return BX(2, 0, 0, 0, 2, 1, 1, 0, 3);
    case 24: return BX(2, 2, 0, 0, 3, 1, 1);
    default: return BX(2, 1, 0, 0, 2, 1, 1);
  }
}
static Bx rbx(int style = 0) {
  // style 0: anything; 1: finite closed boxes only; 2: finite closed boxes, sometimes void
  double r = rnd(0, 1);
  Bx s;
  if (style == 0 && r < 0.05) s = BX(0, 0, 0, 0, 0, 0, 0);
  else if (style == 0 && r < 0.09) s = BX(3, 0, 0, 0, 0, 0, 0);
  else if (style == 2 && r < 0.08) s = BX(0, 0, 0, 0, 0, 0, 0);
  else {
    double lo[3], sz[3];
    for (int k = 0; k < 3; ++k) { lo[k] = crd(); sz[k] = coin(0.1) ? 0.0 : (coin(0.5) ? 0.5 * rint(1, 8) : rnd(0, 6)); }
    int kind = coin(0.12) ? 4 : (coin(0.4) ? 1 : 2);
    if (kind == 1 && coin(0.12)) for (int k = 0; k < 3; ++k) sz[k] = -sz[k];
    s = BX(kind, lo[0], lo[1], lo[2], lo[0] + sz[0], lo[1] + sz[1], lo[2] + sz[2]);
  }
  double g = rnd(0, 1);
  s.gap = g < 0.6 ? 0.0 : (g < 0.8 ? 0.25 * rint(-4, 4) : rnd(-1, 1));
  if (style == 0 && s.kind != 3 && coin(0.35)) { int nbits = rint(1, 3); for (int k = 0; k < nbits; ++k) s.flags |= 1 << rint(0, 5); if (coin(0.06)) s.flags = 63; }
  return s;
}
// ---------------------------------------------------------------------------------
// Bnd_Box state
static void put_box(const Bnd_Box& b) {
  ob(b.IsVoid()); ob(b.IsWhole()); ob(b.IsOpen());
  ob(b.IsOpenXmin()); ob(b.IsOpenXmax()); ob(b.IsOpenYmin()); ob(b.IsOpenYmax()); ob(b.IsOpenZmin()); ob(b.IsOpenZmax());
  h(b.GetGap());
  Bnd_Box::Limits l = b.Get();
  h(l.Xmin); h(l.Xmax); h(l.Ymin); h(l.Ymax); h(l.Zmin); h(l.Zmax);
  hi(b.SquareExtent()); ob(b.HasFinitePart());
}

// =================================================================================
// helpers shared by the families
static const char* g_only_fam = nullptr;
static bool fam_wanted(const char* n) {
  if (!g_only_fam) return true;
  std::string o = std::string(",") + g_only_fam + ",", k = std::string(",") + n + ",";
  return o.find(k) != std::string::npos;
}
#define LOOP(NAME, TARGET) const char* N = NAME; for (int att = 0; fam_wanted(N) && att < 20000 && fam_lines[N] < (TARGET); ++att)

// text of a stream dump: pointers (0x...) are replaced by <ptr>
static std::string mask_ptr(const std::string& s) {
  std::string o;
  for (size_t i = 0; i < s.size();) {
    if (s[i] == '0' && i + 2 < s.size() && s[i + 1] == 'x' && std::isxdigit((unsigned char)s[i + 2])) {
      i += 2;
      while (i < s.size() && std::isxdigit((unsigned char)s[i])) ++i;
      o += "<ptr>";
    } else o += s[i++];
  }
  return o;
}
// stream capture of the library's std::cout writers
template <class F> static void put_cout_text(F f) {
  std::ostringstream os;
  std::streambuf* old = std::cout.rdbuf(os.rdbuf());
  try { f(); } catch (...) { std::cout.rdbuf(old); throw; }
  std::cout.rdbuf(old);
  os_text(mask_ptr(os.str()));
}
// one token: a text (spaces as _, newlines as \n) as an input
static void W_txt(const std::string& t) { int s = ntok; os_text("T" + t); word("txt", s, 1); }   // "T" prefix: the empty text is a token too
static void put_trsf(const gp_Trsf& t) {
  for (int i = 1; i <= 3; ++i) for (int j = 1; j <= 4; ++j) h(t.Value(i, j));
  h(t.ScaleFactor()); in((int)t.Form()); ob(t.IsNegative());
}

// =================================================================================
// TopLoc: datum pools and location programs
struct LPool { std::vector<TR> tr; };
static std::vector<occ::handle<TopLoc_Datum3D>> g_pool, g_reg;
static void W_lp(const LPool& p) { int s = ntok; in((long)p.tr.size()); for (const TR& t : p.tr) p_trsf(t); word("lp", s, -1); }
static void make_pool(const LPool& p) {
  g_pool.clear(); g_reg.clear();
  for (const TR& t : p.tr) { g_pool.push_back(new TopLoc_Datum3D(t.t)); g_reg.push_back(g_pool.back()); }
}
static int reg_index(const occ::handle<TopLoc_Datum3D>& d) {
  for (size_t i = 0; i < g_reg.size(); ++i) if (g_reg[i].get() == d.get()) return (int)i;
  std::fprintf(stderr, "datum not in the registry in %s\n", cur_name.c_str()); std::abort();
}
// op codes: 0 ID  1 D k  2 T k  3 MUL  4 DIV  5 PRE  6 POW n  7 INV  8 DUP
struct LOp { int code; int arg; };
struct LProg { std::vector<LOp> ops; };
static bool op_has_arg(int c) { return c == 1 || c == 2 || c == 6; }
static void W_lprog(const LProg& p) {
  int s = ntok; in((long)p.ops.size());
  for (const LOp& o : p.ops) { in(o.code); if (op_has_arg(o.code)) in(o.arg); }
  word("lprog", s, -1);
}
static TopLoc_Location eval_prog(const LProg& p) {
  std::vector<TopLoc_Location> st;
  for (const LOp& o : p.ops) {
    switch (o.code) {
      case 0: st.push_back(TopLoc_Location()); break;
      case 1: st.push_back(TopLoc_Location(g_pool.at(o.arg))); break;
      case 2: { TopLoc_Location l(g_pool.at(o.arg)->Transformation()); g_reg.push_back(l.FirstDatum()); st.push_back(l); } break;
      case 3: case 4: case 5: {
        TopLoc_Location b = st.back(); st.pop_back(); TopLoc_Location a = st.back(); st.pop_back();
        st.push_back(o.code == 3 ? a.Multiplied(b) : (o.code == 4 ? a.Divided(b) : a.Predivided(b)));
      } break;
      case 6: st.back() = st.back().Powered(o.arg); break;
      case 7: st.back() = st.back().Inverted(); break;
      default: st.push_back(st.back()); break;
    }
  }
  return st.back();
}
static int pw() {
  static const int P[] = {0, 1, -1, 2, -2, 3, -3, 4, 5, -5, 7, -9, 12, 33, -17};
  return coin(0.85) ? pick(P) : rint(-6, 6);
}
static LPool rpool(int att) {
  LPool p; int n = att < 8 ? 1 + att % 3 : rint(1, 4);
  for (int i = 0; i < n; ++i) p.tr.push_back(rtr(rint(0, 11)));
  return p;
}
static LProg rprog(int att, int np) {
  LProg p; auto L = [&](int c, int a = 0) { p.ops.push_back({c, a}); };
  int k1 = np > 1 ? 1 : 0;
  switch (att) {
    case 0: L(0); return p;
    case 1: L(1, 0); return p;
    case 2: L(2, 0); return p;
    case 3: L(1, 0); L(7); return p;
    case 4: L(1, 0); L(1, 0); L(3); return p;
    case 5: L(1, 0); L(1, 0); L(7); L(3); return p;
    case 6: L(1, 0); L(1, k1); L(3); return p;
    case 7: L(1, 0); L(1, k1); L(3); L(1, k1); L(7); L(3); return p;
    case 8: L(1, 0); L(6, 0); return p;
    case 9: L(1, 0); L(6, -3); return p;
    case 10: L(1, 0); L(1, k1); L(3); L(6, 3); return p;
    case 11: L(1, 0); L(8); L(3); L(8); L(3); return p;
    case 12: L(1, 0); L(0); L(3); return p;
    case 13: L(0); L(0); L(3); return p;
    default: break;
  }
  int depth = 0, steps = rint(1, 7);
  for (int s = 0; s < steps || depth > 1; ++s) {
    double r = rnd(0, 1);
    if (s >= steps) { L(coin(0.7) ? 3 : (coin(0.5) ? 4 : 5)); --depth; continue; }
    if (depth < 2 || r < 0.45) {
      double q = rnd(0, 1);
      if (q < 0.6) L(1, rint(0, np - 1)); else if (q < 0.75) L(2, rint(0, np - 1)); else if (q < 0.85) L(0); else if (depth > 0) { L(8); } else L(1, rint(0, np - 1));
      ++depth;
    } else if (r < 0.8) { L(coin(0.5) ? 3 : (coin(0.5) ? 4 : 5)); --depth; }
    else if (r < 0.9) L(6, pw());
    else L(7);
  }
  return p;
}
static void put_loc(const TopLoc_Location& L) {
  ob(L.IsIdentity());
  int n = 0;
  for (TopLoc_Location c = L; !c.IsIdentity(); c = c.NextLocation()) ++n;
  in(n);
  for (TopLoc_Location c = L; !c.IsIdentity(); c = c.NextLocation()) { in(reg_index(c.FirstDatum())); in(c.FirstPower()); }
  put_trsf(L.Transformation());
}

static int NL = 30;
static void fam_toploc() {
  { LOOP("loc_state", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size());
    begin(N, "lp lprog"); W_lp(p); W_lprog(a); bar();
    emit([&] { make_pool(p); put_loc(eval_prog(a)); });
  } }
  { LOOP("loc_mul", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size()), b = rprog(att / 2 + 3 * (att % 2), (int)p.tr.size());
    begin(N, "lp lprog lprog"); W_lp(p); W_lprog(a); W_lprog(b); bar();
    emit([&] { make_pool(p); TopLoc_Location x = eval_prog(a), y = eval_prog(b); put_loc(x.Multiplied(y)); put_loc(x * y); });
  } }
  { LOOP("loc_div", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size()), b = rprog(att / 2 + 3 * (att % 2), (int)p.tr.size());
    begin(N, "lp lprog lprog"); W_lp(p); W_lprog(a); W_lprog(b); bar();
    emit([&] { make_pool(p); TopLoc_Location x = eval_prog(a), y = eval_prog(b); put_loc(x.Divided(y)); put_loc(x / y); });
  } }
  { LOOP("loc_predivided", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size()), b = rprog(att / 2 + 3 * (att % 2), (int)p.tr.size());
    begin(N, "lp lprog lprog"); W_lp(p); W_lprog(a); W_lprog(b); bar();
    emit([&] { make_pool(p); TopLoc_Location x = eval_prog(a), y = eval_prog(b); put_loc(x.Predivided(y)); });
  } }
  { LOOP("loc_inverted", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size());
    begin(N, "lp lprog"); W_lp(p); W_lprog(a); bar();
    emit([&] { make_pool(p); put_loc(eval_prog(a).Inverted()); });
  } }
  { LOOP("loc_powered", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size()); int n = att < 15 ? att - 7 : pw();
    begin(N, "lp lprog i"); W_lp(p); W_lprog(a); W_i(n); bar();
    emit([&] { make_pool(p); put_loc(eval_prog(a).Powered(n)); });
  } }
  { LOOP("loc_equal", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size()), b = (att % 3 == 0) ? a : rprog(att / 3, (int)p.tr.size());
    begin(N, "lp lprog lprog"); W_lp(p); W_lprog(a); W_lprog(b); bar();
    emit([&] {
      make_pool(p); TopLoc_Location x = eval_prog(a), y = eval_prog(b);
      bool e = x.IsEqual(y); ob(e); ob(x.IsDifferent(y)); ob(x == y); ob(x != y);
      if (e) ob(x.HashCode() == y.HashCode());
      ob(x.IsIdentity()); if (x.IsIdentity()) ob(x.HashCode() == 0);
    });
  } }
  { LOOP("loc_first", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size());
    begin(N, "lp lprog"); W_lp(p); W_lprog(a); bar();
    emit([&] {
      make_pool(p); TopLoc_Location x = eval_prog(a);
      if (x.IsIdentity()) { unchecked(); return; }   // FirstDatum / FirstPower of an empty location: a null pointer is read (crash)
      in(reg_index(x.FirstDatum())); in(x.FirstPower()); put_loc(x.NextLocation());
      put_trsf(x.FirstDatum()->Transformation());
    });
  } }
  { LOOP("loc_laws", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size()), b = rprog(att / 2 + 5 * (att % 2), (int)p.tr.size());
    begin(N, "lp lprog lprog"); W_lp(p); W_lprog(a); W_lprog(b); bar();
    emit([&] {
      make_pool(p); TopLoc_Location x = eval_prog(a), y = eval_prog(b);
      ob((x * x.Inverted()).IsIdentity()); ob((x.Inverted() * x).IsIdentity()); ob((x / x).IsIdentity());
      ob(x.Predivided(x).IsIdentity()); ob(x.Powered(0).IsIdentity()); ob((x.Powered(2) * x.Powered(-2)).IsIdentity());
      ob(x.Inverted().Inverted() == x); ob(x.Powered(1) == x); ob(x.Powered(-1) == x.Inverted());
      ob((x * TopLoc_Location()) == x); ob((TopLoc_Location() * x) == x);
      TopLoc_Location c = x; c.Identity(); ob(c.IsIdentity()); TopLoc_Location d = x; d.Clear(); ob(d.IsIdentity());
      ob((x * y).Inverted() == y.Inverted() * x.Inverted()); ob((x * y) / y == x); ob(x.Predivided(y) == y.Inverted() * x);
      ob((x * y) * y.Inverted() == x); ob(x.Divided(y) == x * y.Inverted()); ob((x * y).IsIdentity()); ob(x.Powered(3) == x * x * x);
    });
  } }
  { LOOP("loc_from_trsf", NL) {
    TR t = rtr(att % 12 + (att >= 12 ? rint(0, 11) : 0));
    begin(N, "trsf"); W_trsf(t); bar();
    emit([&] {
      g_reg.clear(); TopLoc_Location l(t.t); g_reg.push_back(l.FirstDatum()); put_loc(l);
      gp_Trsf conv = l; put_trsf(conv); ob(l.IsIdentity());
    });
  } }
  { LOOP("datum3d", NL) {
    TR t = rtr(att % 12 + (att >= 12 ? rint(0, 11) : 0));
    begin(N, "trsf"); W_trsf(t); bar();
    emit([&] {
      occ::handle<TopLoc_Datum3D> d = new TopLoc_Datum3D(t.t);
      put_trsf(d->Transformation()); put_trsf(d->Trsf()); in((int)d->Form());
      put_cout_text([&] { d->ShallowDump(std::cout); });
      occ::handle<TopLoc_Datum3D> e = new TopLoc_Datum3D(); put_trsf(e->Transformation());
    });
  } }
  { LOOP("loc_dump", NL) {
    LPool p = rpool(att); LProg a = rprog(att, (int)p.tr.size());
    begin(N, "lp lprog"); W_lp(p); W_lprog(a); bar();
    emit([&] { make_pool(p); TopLoc_Location x = eval_prog(a); put_cout_text([&] { x.ShallowDump(std::cout); }); });
  } }
  { LOOP("item_loc", NL) {
    LPool p = rpool(att); int idx = rint(0, (int)p.tr.size() - 1), pwr = att < 15 ? att - 6 : pw();
    begin(N, "lp i i"); W_lp(p); W_i(idx); W_i(pwr); bar();
    emit([&] {
      make_pool(p); TopLoc_ItemLocation it(g_pool.at(idx), pwr);
      in(reg_index(it.myDatum)); in(it.myPower); put_trsf(it.myTrsf);
    });
  } }
  { LOOP("slist_build", NL) {
    LPool p = rpool(att); int n = att < 4 ? att : rint(0, 6);
    std::vector<std::pair<int, int>> items;
    for (int k = 0; k < n; ++k) items.push_back({rint(0, (int)p.tr.size() - 1), coin(0.8) ? rint(-3, 4) : pw()});
    int ntail = att % 4 == 0 ? 0 : rint(0, n + 2), flag = att % 7 == 3;
    begin(N, "lp items i i"); W_lp(p);
    { int s = ntok; in(n); for (auto& it : items) { in(it.first); in(it.second); } word("items", s, -1); }
    W_i(ntail); W_i(flag); bar();
    emit([&] {
      make_pool(p);
      TopLoc_SListOfItemLocation built;
      for (auto& it : items) built.Construct(TopLoc_ItemLocation(g_pool.at(it.first), it.second));
      // ToTail() of a list whose head node has no other owner reads freed memory in OCCT 8.0.1 (Assign(Tail()) clears the
      // node that holds the tail first), so the walk is done on a copy while `built` keeps every node alive
      TopLoc_SListOfItemLocation s = built;
      for (int k = 0; k < ntail; ++k) s.ToTail();
      ob(s.IsEmpty()); ob(s.More());
      if (s.IsEmpty() && flag) { unchecked(); return; }   // Value() of an empty list reads a null pointer
      int cnt = 0;
      for (TopLoc_SListOfItemLocation c = s; c.More(); c.Next()) ++cnt;
      in(cnt);
      for (TopLoc_SListOfItemLocation c = s; c.More(); c = c.Tail()) {
        in(reg_index(c.Value().myDatum)); in(c.Value().myPower); put_trsf(c.Value().myTrsf);
      }
      ob(s.Tail().IsEmpty());
    });
  } }
}

// =================================================================================
// meshes and triangulation recipes
typedef std::array<int, 3> T3;
struct Mesh {
  std::vector<gp_XYZ> p;
  std::vector<gp_XY> uv;
  std::vector<T3> t;
  std::vector<std::array<float, 3>> n;
};
static gp_XYZ mp(double x, double y, double z) { return gp_XYZ(x, y, z); }
static Mesh m_grid(int nx, int ny) {
  Mesh m; double dx = coin(0.5) ? 0.5 * rint(1, 4) : rnd(0.2, 2), dy = coin(0.5) ? 0.5 * rint(1, 4) : rnd(0.2, 2);
  bool flat = coin(0.5), alt = coin(0.5);
  for (int j = 0; j <= ny; ++j) for (int i = 0; i <= nx; ++i) m.p.push_back(mp(i * dx, j * dy, flat ? 0.0 : rnd(-0.4, 0.4)));
  for (int j = 0; j < ny; ++j) for (int i = 0; i < nx; ++i) {
    int a = j * (nx + 1) + i + 1, b = a + 1, c = a + nx + 1, d = c + 1;
    if (alt && ((i + j) & 1)) { m.t.push_back({a, b, c}); m.t.push_back({b, d, c}); }
    else { m.t.push_back({a, b, d}); m.t.push_back({a, d, c}); }
  }
  return m;
}
static Mesh m_fan(int n, bool closed) {
  Mesh m; m.p.push_back(mp(0, 0, 0));
  for (int i = 0; i < (closed ? n : n + 1); ++i) { double a = (closed ? 6.283185307179586 : 3.141592653589793) * i / (closed ? n : n); m.p.push_back(mp(std::cos(a), std::sin(a), coin(0.3) ? rnd(-0.2, 0.2) : 0.0)); }
  for (int i = 0; i < n; ++i) m.t.push_back({1, 2 + i, closed && i == n - 1 ? 2 : 3 + i});
  return m;
}
static Mesh m_strip(int n) {
  Mesh m;
  for (int i = 0; i <= n; ++i) { m.p.push_back(mp(i, 0, 0)); m.p.push_back(mp(i + 0.5, 1, coin(0.4) ? 0.25 : 0.0)); }
  for (int i = 0; i < n; ++i) { int a = 2 * i + 1; m.t.push_back({a, a + 2, a + 1}); m.t.push_back({a + 1, a + 2, a + 3}); }
  return m;
}
static Mesh m_tetra() {
  Mesh m; m.p = {mp(0, 0, 0), mp(1, 0, 0), mp(0, 1, 0), mp(0, 0, 1)};
  m.t = {{1, 3, 2}, {1, 2, 4}, {2, 3, 4}, {3, 1, 4}}; return m;
}
static Mesh m_cube() {
  Mesh m; double s = coin(0.5) ? 1.0 : rnd(0.5, 2);
  for (int k = 0; k < 8; ++k) m.p.push_back(mp((k & 1) * s, ((k >> 1) & 1) * s, ((k >> 2) & 1) * s));
  m.t = {{1, 3, 2}, {2, 3, 4}, {5, 6, 7}, {6, 8, 7}, {1, 2, 5}, {2, 6, 5}, {3, 7, 4}, {4, 7, 8}, {1, 5, 3}, {3, 5, 7}, {2, 4, 6}, {4, 8, 6}};
  return m;
}
static Mesh m_octa() {
  Mesh m; m.p = {mp(1, 0, 0), mp(-1, 0, 0), mp(0, 1, 0), mp(0, -1, 0), mp(0, 0, 1), mp(0, 0, -1)};
  m.t = {{1, 3, 5}, {3, 2, 5}, {2, 4, 5}, {4, 1, 5}, {3, 1, 6}, {2, 3, 6}, {4, 2, 6}, {1, 4, 6}}; return m;
}
static Mesh m_single() {
  Mesh m; m.p = {mp(crd(), crd(), crd()), mp(crd(), crd(), crd()), mp(crd(), crd(), crd())}; m.t = {{1, 2, 3}}; return m;
}
static Mesh m_degen() {
  Mesh m; m.p = {mp(0, 0, 0), mp(1, 0, 0), mp(0, 1, 0), mp(0.5, 0.5, 0), mp(2, 0, 0)};
  m.t = {{1, 2, 3}, {1, 1, 2}, {2, 3, 3}, {4, 4, 4}, {1, 2, 3}, {3, 2, 1}, {1, 2, 5}, {2, 4, 3}};
  if (coin(0.5)) std::swap(m.t[rint(0, (int)m.t.size() - 1)], m.t[rint(0, (int)m.t.size() - 1)]);
  return m;
}
static Mesh m_nonmanifold() {
  Mesh m; m.p = {mp(0, 0, 0), mp(1, 0, 0), mp(0.5, 1, 0), mp(0.5, -1, 0), mp(0.5, 0, 1), mp(5, 5, 5)};
  m.t = {{1, 2, 3}, {2, 1, 4}, {1, 2, 5}};
  if (coin(0.5)) m.t.push_back({1, 2, 4});
  return m;
}
static Mesh m_bowtie() {
  Mesh m; m.p = {mp(0, 0, 0), mp(1, 0, 0), mp(0.5, 1, 0), mp(1, -1, 0), mp(0, -1, 0)};
  m.t = {{1, 2, 3}, {3, 4, 5}}; if (coin(0.4)) m.t.push_back({3, 5, 1}); return m;
}
static Mesh m_two() {
  Mesh m; m.p = {mp(0, 0, 0), mp(1, 0, 0), mp(0, 1, 0), mp(3, 0, 0), mp(4, 0, 0), mp(3, 1, 0)};
  m.t = {{1, 2, 3}, {4, 5, 6}}; return m;
}
static Mesh m_soup(int nn, int nt) {
  Mesh m; for (int i = 0; i < nn; ++i) m.p.push_back(mp(crd(), crd(), crd()));
  for (int i = 0; i < nt; ++i) m.t.push_back({rint(1, nn), rint(1, nn), rint(1, nn)});
  return m;
}
static Mesh m_zero_tris(int nn) { Mesh m; for (int i = 0; i < nn; ++i) m.p.push_back(mp(crd(), crd(), crd())); return m; }
static Mesh m_collinear() {
  Mesh m; double k = rnd(0.5, 3); gp_XYZ a(crd(), crd(), crd()), d(rnd(-1, 1), rnd(-1, 1), rnd(-1, 1));
  m.p = {a, mp(a.X() + d.X(), a.Y() + d.Y(), a.Z() + d.Z()), mp(a.X() + k * d.X(), a.Y() + k * d.Y(), a.Z() + k * d.Z()), mp(a.X(), a.Y() + 1, a.Z())};
  m.t = {{1, 2, 3}, {1, 3, 4}}; return m;
}
static Mesh rmesh(int att) {
  Mesh m;
  int g = att < 18 ? att : rint(0, 17);
  switch (g) {
    case 0: m = m_single(); break;
    case 1: m = m_grid(1, 1); break;
    case 2: m = m_grid(2, 2); break;
    case 3: m = m_grid(rint(2, 4), rint(1, 4)); break;
    case 4: m = m_fan(rint(3, 7), true); break;
    case 5: m = m_fan(rint(2, 6), false); break;
    case 6: m = m_strip(rint(1, 5)); break;
    case 7: m = m_tetra(); break;
    case 8: m = m_cube(); break;
    case 9: m = m_octa(); break;
    case 10: m = m_degen(); break;
    case 11: m = m_nonmanifold(); break;
    case 12: m = m_bowtie(); break;
    case 13: m = m_two(); break;
    case 14: m = m_soup(rint(3, 8), rint(1, 9)); break;
    case 15: m = m_collinear(); break;
    case 16: m = m_zero_tris(rint(1, 4)); break;
    default: m = m_grid(rint(1, 3), rint(1, 3)); break;
  }
  for (const T3& t : m.t) for (int k = 0; k < 3; ++k) if (t[k] < 1 || t[k] > (int)m.p.size()) { std::fprintf(stderr, "bad generated mesh %d\n", g); std::abort(); }
  // a rigid-ish placement of the nodes
  if (coin(0.5)) { gp_XYZ o(crd(), crd(), crd()); double sc = coin(0.5) ? 1.0 : rnd(0.3, 3); for (gp_XYZ& q : m.p) q = mp(q.X() * sc + o.X(), q.Y() * sc + o.Y(), q.Z() * sc + o.Z()); }
  return m;
}
struct TriSpec { int kind = -1; int lo = 1; int uv = 0, nrm = 0; double defl = 0; int purpose = 0; Mesh m; };
static void fill_uv_nrm(TriSpec& s) {
  s.m.uv.clear(); s.m.n.clear();
  if (s.uv) for (const gp_XYZ& q : s.m.p) s.m.uv.push_back(gp_XY(coin(0.5) ? q.X() : rnd(0, 1), coin(0.5) ? q.Y() : rnd(0, 1)));
  if (s.nrm) for (size_t i = 0; i < s.m.p.size(); ++i) {
    std::array<float, 3> v = {(float)rnd(-1, 1), (float)rnd(-1, 1), (float)rnd(-1, 1)};
    if (coin(0.2)) v = {0.f, 0.f, 1.f};
    if (coin(0.05)) v = {0.f, 0.f, 0.f};
    s.m.n.push_back(v);
  }
}
static TriSpec rtri(int att, int kind = -2, bool uv = false, bool nrm = false, bool allow_lo = false) {
  TriSpec s; s.m = rmesh(att);
  static const int LOS[] = {1, 0, 3, -2, 7};
  s.kind = kind != -2 ? kind : (att % 5 == 4 ? 2 : (att % 3 == 1 ? 1 : 0));
  if (att % 3 == 0 && kind == -2) s.kind = 0;
  if (s.kind == 2 && (s.m.p.size() < 1 || s.m.t.size() < 1)) s.kind = 0;
  s.lo = (s.kind == 2 && allow_lo) ? pick(LOS) : 1;
  s.uv = uv || (kind == -2 && coin(0.4)); s.nrm = nrm || (kind == -2 && coin(0.35));
  static const double DF[] = {0.0, 0.1, 1e-3, 0.5, 2.0};
  s.defl = coin(0.5) ? pick(DF) : rnd(0, 1);
  static const int PU[] = {0, 1, 2, 3, 4, 8, 16, 32, 33, 63};
  s.purpose = pick(PU);
  fill_uv_nrm(s);
  return s;
}
// the recipe, see the grammar in docs/api-poly.md ("tri")
static void p_tri(const TriSpec& s) {
  in(s.kind);
  if (s.kind < 0) return;
  in((long)s.m.p.size()); in((long)s.m.t.size()); in(s.uv); in(s.nrm); in(s.lo); h(s.defl); in(s.purpose);
  for (const gp_XYZ& q : s.m.p) xyz(q);
  if (s.uv) for (const gp_XY& q : s.m.uv) xy(q);
  for (const T3& t : s.m.t) { in(t[0]); in(t[1]); in(t[2]); }
  if (s.nrm) for (const auto& v : s.m.n) { h(v[0]); h(v[1]); h(v[2]); }
}
static void W_tri(const TriSpec& s) { int st = ntok; p_tri(s); word("tri", st, -1); }
static occ::handle<Poly_Triangulation> mk_tri(const TriSpec& s) {
  if (s.kind < 0) return occ::handle<Poly_Triangulation>();
  const Mesh& m = s.m; int nn = (int)m.p.size(), nt = (int)m.t.size();
  occ::handle<Poly_Triangulation> T;
  if (s.kind == 0) T = new Poly_Triangulation(nn, nt, s.uv != 0, s.nrm != 0);
  else if (s.kind == 1) {
    T = new Poly_Triangulation(); T->SetDoublePrecision(false); T->ResizeNodes(nn, false); T->ResizeTriangles(nt, false);
    if (s.uv) T->AddUVNodes();
    if (s.nrm) T->AddNormals();
  } else {
    NCollection_Array1<gp_Pnt> a(s.lo, s.lo + nn - 1); NCollection_Array1<Poly_Triangle> b(s.lo, s.lo + nt - 1);
    for (int i = 0; i < nn; ++i) a.SetValue(s.lo + i, gp_Pnt(m.p[i]));
    for (int i = 0; i < nt; ++i) b.SetValue(s.lo + i, Poly_Triangle(m.t[i][0], m.t[i][1], m.t[i][2]));
    if (s.uv) {
      NCollection_Array1<gp_Pnt2d> u(s.lo, s.lo + nn - 1);
      for (int i = 0; i < nn; ++i) u.SetValue(s.lo + i, gp_Pnt2d(m.uv[i]));
      T = new Poly_Triangulation(a, u, b);
    } else T = new Poly_Triangulation(a, b);
    if (s.nrm) T->AddNormals();
  }
  for (int i = 1; i <= nn; ++i) {
    T->SetNode(i, gp_Pnt(m.p[i - 1]));
    if (s.uv) T->SetUVNode(i, gp_Pnt2d(m.uv[i - 1]));
    if (s.nrm) T->SetNormal(i, NCollection_Vec3<float>(m.n[i - 1][0], m.n[i - 1][1], m.n[i - 1][2]));
  }
  { int tl = s.kind == 2 ? s.lo : 1; for (int i = 0; i < nt; ++i) T->SetTriangle(tl + i, Poly_Triangle(m.t[i][0], m.t[i][1], m.t[i][2])); }
  T->Deflection(s.defl); T->SetMeshPurpose((Poly_MeshPurpose)s.purpose);
  return T;
}
static void put_f3(const NCollection_Vec3<float>& v) { h((double)v.x()); h((double)v.y()); h((double)v.z()); }
static void put_nodes_of(const occ::handle<Poly_Triangulation>& T) {
  for (int i = 1; i <= T->NbNodes(); ++i) { gp_Pnt q = T->Node(i); h(q.X()); h(q.Y()); h(q.Z()); }
}
static void put_tri_state(const occ::handle<Poly_Triangulation>& T) {
  in(T->NbNodes()); in(T->NbTriangles()); ob(T->HasUVNodes()); ob(T->HasNormals()); ob(T->HasGeometry());
  ob(T->IsDoublePrecision()); h(T->Deflection()); in((long)T->MeshPurpose()); ob(T->HasCachedMinMax());
  put_nodes_of(T);
  if (T->HasUVNodes()) for (int i = 1; i <= T->NbNodes(); ++i) { gp_Pnt2d q = T->UVNode(i); h(q.X()); h(q.Y()); }
  if (T->HasNormals()) for (int i = 1; i <= T->NbNodes(); ++i) { NCollection_Vec3<float> v; T->Normal(i, v); put_f3(v); }
  // OCCT 8: NCollection_Array1::operator= takes over the lower bound of the source, so the triangle array built from
  // an Array1 with Lower() != 1 is indexed from that lower bound
  int tl = T->NbTriangles() > 0 ? T->InternalTriangles().Lower() : 1; in(tl);
  for (int i = tl; i < tl + T->NbTriangles(); ++i) { int a, b, c; T->Triangle(i).Get(a, b, c); in(a); in(b); in(c); }
}
static bool tri_indices_ok(const TriSpec& s) {
  int nn = (int)s.m.p.size();
  for (const T3& t : s.m.t) for (int k = 0; k < 3; ++k) if (t[k] < 1 || t[k] > nn) return false;
  return true;
}

// =================================================================================
// triangulation families
struct TOp { int code; long a = 0, b = 0, c = 0, d = 0; double x = 0, y = 0, z = 0; };
static void p_tops(const std::vector<TOp>& ops) {
  in((long)ops.size());
  for (const TOp& o : ops) {
    in(o.code);
    switch (o.code) {
      case 4: case 5: in(o.a); in(o.b); break;
      case 6: case 10: in(o.a); break;
      case 9: h(o.x); break;
      case 11: in(o.a); h(o.x); h(o.y); h(o.z); break;
      case 12: in(o.a); h(o.x); h(o.y); break;
      case 13: in(o.a); in(o.b); in(o.c); in(o.d); break;
      case 14: in(o.a); h(o.x); h(o.y); h(o.z); break;
      default: break;
    }
  }
}
static void W_tops(const std::vector<TOp>& ops) { int s = ntok; p_tops(ops); word("tops", s, -1); }
static void run_tops(occ::handle<Poly_Triangulation>& T, const std::vector<TOp>& ops) {
  for (const TOp& o : ops) {
    switch (o.code) {
      case 0: T->AddUVNodes(); break;
      case 1: T->RemoveUVNodes(); break;
      case 2: T->AddNormals(); break;
      case 3: T->RemoveNormals(); break;
      case 4: T->ResizeNodes((int)o.a, o.b != 0); break;
      case 5: T->ResizeTriangles((int)o.a, o.b != 0); break;
      case 6: T->SetDoublePrecision(o.a != 0); break;
      case 7: T->Clear(); break;
      case 8: T->ComputeNormals(); break;
      case 9: T->Deflection(o.x); break;
      case 10: T->SetMeshPurpose((Poly_MeshPurpose)o.a); break;
      case 11: T->SetNode((int)o.a, gp_Pnt(o.x, o.y, o.z)); break;
      case 12: T->SetUVNode((int)o.a, gp_Pnt2d(o.x, o.y)); break;
      case 13: T->SetTriangle((int)o.a, Poly_Triangle((int)o.b, (int)o.c, (int)o.d)); break;
      case 14: T->SetNormal((int)o.a, NCollection_Vec3<float>((float)o.x, (float)o.y, (float)o.z)); break;
      default: T = T->Copy(); break;
    }
  }
}
// generator of valid programs (with a small shadow of the sizes; every possibly undefined element is refilled)
static std::vector<TOp> rtops(const TriSpec& s, int att) {
  std::vector<TOp> ops; int nn = (int)s.m.p.size(), nt = (int)s.m.t.size(); bool uv = s.uv, nrm = s.nrm;
  std::vector<T3> tri = s.m.t;
  auto op = [&](int c) { TOp o; o.code = c; ops.push_back(o); return &ops.back(); };
  auto fill_nodes = [&] { for (int i = 1; i <= nn; ++i) { TOp* o = op(11); o->a = i; o->x = crd(); o->y = crd(); o->z = crd(); } };
  auto fill_uv = [&] { for (int i = 1; i <= nn; ++i) { TOp* o = op(12); o->a = i; o->x = rnd(0, 1); o->y = rnd(0, 1); } };
  auto fill_nrm = [&] { for (int i = 1; i <= nn; ++i) { TOp* o = op(14); o->a = i; o->x = (float)rnd(-1, 1); o->y = (float)rnd(-1, 1); o->z = (float)rnd(-1, 1); } };
  auto fill_tri = [&] {
    tri.assign(nt, T3{1, 1, 1});
    for (int i = 1; i <= nt; ++i) { TOp* o = op(13); o->a = i; o->b = rint(1, nn); o->c = rint(1, nn); o->d = rint(1, nn); tri[i - 1] = {(int)o->b, (int)o->c, (int)o->d}; }
  };
  auto tri_ok = [&] { for (const T3& t : tri) for (int k = 0; k < 3; ++k) if (t[k] < 1 || t[k] > nn) return false; return nn >= 1; };
  int steps = att < 12 ? 1 + att % 4 : rint(1, 6);
  for (int st = 0; st < steps; ++st) {
    int c = att < 12 ? (att * 3 + st * 5) % 16 : rint(0, 15);
    switch (c) {
      case 0: op(0); uv = true; fill_uv(); break;
      case 1: op(1); uv = false; break;
      case 2: op(2); nrm = true; fill_nrm(); break;
      case 3: op(3); nrm = false; break;
      case 4: {
        int n = pick((const int[]){nn + 2, std::max(1, nn - 1), nn, 1, nn + 5}); TOp* o = op(4); o->a = n; o->b = coin(0.5); nn = n;
        fill_nodes(); if (uv) fill_uv(); if (nrm) fill_nrm(); fill_tri(); break;
      }
      case 5: { int n = pick((const int[]){0, 1, nt + 2, std::max(0, nt - 1), nt}); TOp* o = op(5); o->a = n; o->b = coin(0.5); nt = n; fill_tri(); break; }
      case 6: { TOp* o = op(6); o->a = coin(0.5); break; }
      case 7: op(7); nn = nt = 0; uv = nrm = false; tri.clear(); break;
      case 8: if (tri_ok()) { op(8); nrm = true; } break;
      case 9: { TOp* o = op(9); o->x = rnd(0, 2); break; }
      case 10: { TOp* o = op(10); o->a = pick((const int[]){0, 1, 2, 4, 8, 16, 32}); break; }
      case 11: if (nn >= 1) { TOp* o = op(11); o->a = rint(1, nn); o->x = crd(); o->y = crd(); o->z = crd(); } break;
      case 12: if (uv && nn >= 1) { TOp* o = op(12); o->a = rint(1, nn); o->x = rnd(0, 1); o->y = rnd(0, 1); } break;
      case 13: if (nt >= 1) { TOp* o = op(13); o->a = rint(1, nt); o->b = rint(1, std::max(1, nn)); o->c = rint(1, std::max(1, nn)); o->d = rint(1, std::max(1, nn)); tri[o->a - 1] = {(int)o->b, (int)o->c, (int)o->d}; } break;
      case 14: if (nrm && nn >= 1) { TOp* o = op(14); o->a = rint(1, nn); o->x = (float)rnd(-1, 1); o->y = (float)rnd(-1, 1); o->z = (float)rnd(-1, 1); } break;
      default: op(15); break;
    }
  }
  if (att == 1) { ops.clear(); TOp* o = op(4); o->a = 0; o->b = 1; }                        // ResizeNodes(0): throws
  if (att == 2) { ops.clear(); TOp* o = op(6); o->a = 0; }                                  // SetDoublePrecision on allocated nodes: throws
  if (att == 3) { ops.clear(); op(7); TOp* o = op(6); o->a = 0; TOp* r = op(4); r->a = 3; r->b = 0; nn = 3; fill_nodes(); }   // after Clear(): works
  return ops;
}

static int NTRI = 28;
static void fam_tri() {
  { LOOP("tri_state", NTRI) {
    TriSpec s = rtri(att, -2, false, false, true);
    begin(N, "tri"); W_tri(s); bar();
    emit([&] { put_tri_state(mk_tri(s)); });
  } }
  { LOOP("tri_copy", NTRI) {
    TriSpec s = rtri(att, -2, false, false, true); int mode = att % 3;
    begin(N, "tri i"); W_tri(s); W_i(mode); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      if (mode == 2) T->UpdateCachedMinMax();
      occ::handle<Poly_Triangulation> C = mode == 1 ? new Poly_Triangulation(T) : T->Copy();
      put_tri_state(C);
      C->SetNode(1, gp_Pnt(7, 8, 9)); C->Deflection(99.0);
      gp_Pnt a = T->Node(1), b = C->Node(1); h(a.X()); h(a.Y()); h(a.Z()); h(b.X()); h(b.Y()); h(b.Z()); h(T->Deflection()); h(C->Deflection());
    });
  } }
  { LOOP("tri_ctor_invalid", NTRI) {
    static const int NNS[] = {-3, -1, 0, 1, 2, 5, 9}, NTS[] = {-2, -1, 0, 1, 3, 8};
    int nn = att < 14 ? NNS[att % 7] : pick(NNS), nt = att < 14 ? NTS[(att / 2) % 6] : pick(NTS), uv = att % 2, nrm = (att / 2) % 2;
    begin(N, "i i i i"); W_i(nn); W_i(nt); W_i(uv); W_i(nrm); bar();
    emit([&] {
      if (nn > 0 && nt < 0) { unchecked(); return; }   // the triangle array of negative size is built without any check (crash); a node count <= 0 throws first
      occ::handle<Poly_Triangulation> T = new Poly_Triangulation(nn, nt, uv != 0, nrm != 0);
      in(T->NbNodes()); in(T->NbTriangles()); ob(T->HasUVNodes()); ob(T->HasNormals()); ob(T->IsDoublePrecision()); ob(T->HasGeometry());
      h(T->Deflection()); in((long)T->MeshPurpose());
    });
  } }
  { LOOP("tri_access", NTRI) {
    TriSpec s = rtri(att, att % 4 == 3 ? 1 : -2, false, false, true); int nn = (int)s.m.p.size(), nt = (int)s.m.t.size();
    int opc = att % 11, idx, tl = s.kind == 2 ? s.lo : 1;
    bool tri_op = opc == 2 || opc == 7;
    int base = tri_op ? tl : 1, cnt = tri_op ? nt : nn;
    switch ((att / 11) % 6) { case 0: idx = base - 1; break; case 1: idx = base + cnt; break; case 2: idx = base - 2; break; case 3: idx = base + cnt - 1; break; case 4: idx = base; break; default: idx = base + rint(0, std::max(0, cnt - 1)); }
    if (att < 22) idx = att / 11 == 0 ? base + rint(0, std::max(0, cnt - 1)) : (att % 3 == 0 ? base - 1 : base + cnt);
    double x = coin(0.5) ? crd() : rnd(-3, 3), y = crd(), z = coin(0.2) ? 0.0 : crd();
    if (coin(0.2)) { x = y = z = 0; }
    begin(N, "tri i i xyz"); W_tri(s); W_i(opc); W_i(idx); W_xyz(gp_XYZ(x, y, z)); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      switch (opc) {
        case 0: { gp_Pnt q = T->Node(idx); h(q.X()); h(q.Y()); h(q.Z()); } break;
        case 1: { gp_Pnt2d q = T->UVNode(idx); h(q.X()); h(q.Y()); } break;
        case 2: { int a, b, c; T->Triangle(idx).Get(a, b, c); in(a); in(b); in(c); } break;
        case 3: { gp_Dir d = T->Normal(idx); h(d.X()); h(d.Y()); h(d.Z()); } break;
        case 4: { NCollection_Vec3<float> v; T->Normal(idx, v); put_f3(v); } break;
        case 5: { T->SetNode(idx, gp_Pnt(x, y, z)); gp_Pnt q = T->Node(idx); h(q.X()); h(q.Y()); h(q.Z()); } break;
        case 6: { T->SetUVNode(idx, gp_Pnt2d(x, y)); gp_Pnt2d q = T->UVNode(idx); h(q.X()); h(q.Y()); } break;
        case 7: { T->SetTriangle(idx, Poly_Triangle(1, 2, idx)); int a, b, c; T->Triangle(idx).Get(a, b, c); in(a); in(b); in(c); } break;
        case 8: { T->SetNormal(idx, NCollection_Vec3<float>((float)x, (float)y, (float)z)); NCollection_Vec3<float> v; T->Normal(idx, v); put_f3(v); } break;
        case 9: { T->SetNormal(idx, gp_Dir(x, y, z)); NCollection_Vec3<float> v; T->Normal(idx, v); put_f3(v); } break;
        default: { T->SetNormal(idx, NCollection_Vec3<float>((float)x, (float)y, (float)z)); gp_Dir d = T->Normal(idx); h(d.X()); h(d.Y()); h(d.Z()); } break;
      }
    });
  } }
  { LOOP("tri_minmax", NTRI) {
    TriSpec s = rtri(att, -2, false, false, true); Bx cb = att % 4 == 0 ? BX(0, 0, 0, 0, 0, 0, 0) : (coin(0.3) ? edge_bx(att) : rbx(1)), tb = coin(0.4) ? BX(0, 0, 0, 0, 0, 0, 0) : rbx(2);
    TR t = rtr(att % 5 == 0 ? 0 : (att < 24 ? att : rint(0, 11)));
    int acc = att % 2, mode = (att / 2) % 5;
    begin(N, "tri bx bx trsf i i"); W_tri(s); W_bx(cb); W_bx(tb); W_trsf(t); W_i(acc); W_i(mode); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      Bnd_Box c = mkB(cb), box = mkB(tb);
      switch (mode) {
        case 1: T->UpdateCachedMinMax(); break;
        case 2: T->SetCachedMinMax(c); break;
        case 3: T->SetCachedMinMax(c); T->SetNode(1, gp_Pnt(100, 100, 100)); break;
        case 4: T->UpdateCachedMinMax(); T->Clear(); break;
        default: break;
      }
      bool r = T->MinMax(box, t.t, acc != 0);
      ob(r); put_box(box); ob(T->HasCachedMinMax()); if (T->HasCachedMinMax()) put_box(T->CachedMinMax());
    });
  } }
  { LOOP("tri_normals", NTRI) {
    TriSpec s = rtri(att, -2, false, false, true); int mode = att % 4;
    begin(N, "tri i"); W_tri(s); W_i(mode); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      switch (mode) {
        case 0: T->ComputeNormals(); break;
        case 1: Poly::ComputeNormals(T); break;
        case 2: T->ComputeNormals(); T->RemoveNormals(); break;
        default: T->AddNormals(); in(T->NbNodes()); ob(T->HasNormals()); return;   // the normals are not initialised
      }
      ob(T->HasNormals());
      if (T->HasNormals()) for (int i = 1; i <= T->NbNodes(); ++i) { NCollection_Vec3<float> v; T->Normal(i, v); put_f3(v); gp_Dir d = T->Normal(i); h(d.X()); h(d.Y()); h(d.Z()); }
    });
  } }
  { LOOP("tri_modify", NTRI) {
    TriSpec s = rtri(att); std::vector<TOp> ops = rtops(s, att);
    begin(N, "tri tops"); W_tri(s); W_tops(ops); bar();
    emit([&] { occ::handle<Poly_Triangulation> T = mk_tri(s); run_tops(T, ops); put_tri_state(T); });
  } }
  { LOOP("mesh_bad_index", NTRI) {
    TriSpec s = rtri(att, att % 3 == 0 ? 1 : (att % 3 == 1 ? 0 : -2));
    int nn = (int)s.m.p.size();
    if (!s.m.t.empty()) { int k = rint(0, (int)s.m.t.size() - 1); static const int BAD[] = {0, -1}; s.m.t[k][rint(0, 2)] = coin(0.5) ? nn + rint(1, 4) : pick(BAD); }
    else s.m.t.push_back({1, 2, nn + 1});
    if (s.kind == 2) s.kind = 0;
    int opc = att % 5;
    begin(N, "tri i"); W_tri(s); W_i(opc); bar();
    emit([&] { unchecked(); });   // ComputeNormals / Intersect / Poly_Connect / merge / Poly_CoherentTriangulation read nodes outside the array (no check)
  } }
  { LOOP("poly_catenate", NTRI) {
    int k = att < 3 ? att : rint(0, 4); std::vector<TriSpec> v;
    for (int i = 0; i < k; ++i) { TriSpec t = rtri(att * 5 + i); if (coin(0.2)) { t.kind = -1; } v.push_back(t); }
    begin(N, "trilist");
    { int st = ntok; in(k); for (const TriSpec& t : v) p_tri(t); word("trilist", st, -1); }
    bar();
    emit([&] {
      NCollection_List<occ::handle<Poly_Triangulation>> l; for (const TriSpec& t : v) l.Append(mk_tri(t));
      occ::handle<Poly_Triangulation> r = Poly::Catenate(l);
      ob(!r.IsNull()); if (!r.IsNull()) put_tri_state(r);
    });
  } }
}

// =================================================================================
// Poly:: helpers, text formats and polygons
static gp_XY rxy(double s = 3.0) { return gp_XY(coin(0.4) ? nice() : rnd(-s, s), coin(0.4) ? nice() : rnd(-s, s)); }
static void W_gxy(const gp_XY& v) { int s = ntok; xy(v); word("xy", s, 2); }

static std::string write_text(const std::function<void(std::ostream&)>& f, int prec) {
  std::ostringstream os; os << std::setprecision(prec); f(os); return os.str();
}
// polygons: recipes
struct Pg3 { int kind = 0; int nn = 0, hasp = 0, lo = 1, np = 0, plo = 1; double defl = 0; std::vector<gp_XYZ> p; std::vector<double> par; };
static Pg3 rpg3(int att) {
  Pg3 g; g.kind = att % 3; g.nn = att == 0 ? 0 : (att < 4 ? att : rint(1, 7)); if (g.kind != 0 && g.nn < 1) g.nn = 2;
  static const int LOS[] = {1, 0, 4, -3};
  g.lo = g.kind == 0 ? 1 : pick(LOS); g.hasp = g.kind == 0 ? (att % 2) : (g.kind == 2);
  g.np = g.kind == 2 ? g.nn : (g.hasp ? g.nn : 0); g.plo = g.lo;
  if (g.kind == 2 && att % 7 == 5) g.np = g.nn + 1;              // mismatching parameter array: unchecked
  if (g.kind == 2 && att % 7 == 6) g.plo = g.lo + 1;
  g.defl = coin(0.5) ? 0.0 : rnd(0, 1);
  for (int i = 0; i < g.nn; ++i) g.p.push_back(mp(crd(), crd(), crd()));
  for (int i = 0; i < g.np; ++i) g.par.push_back(coin(0.5) ? 0.25 * i : rnd(-2, 2));
  return g;
}
static void p_pg3(const Pg3& g) {
  in(g.kind); in(g.nn); in(g.hasp); in(g.lo); in(g.np); in(g.plo); h(g.defl);
  for (const gp_XYZ& q : g.p) xyz(q);
  for (double v : g.par) h(v);
}
static void W_pg3(const Pg3& g) { int s = ntok; p_pg3(g); word("pg3", s, -1); }
static bool pg3_unchecked(const Pg3& g) { return g.kind == 2 && (g.np != g.nn || g.plo != g.lo); }
static occ::handle<Poly_Polygon3D> mk_pg3(const Pg3& g) {
  occ::handle<Poly_Polygon3D> P;
  if (g.kind == 0) {
    P = new Poly_Polygon3D(g.nn, g.hasp != 0);
    for (int i = 0; i < g.nn; ++i) P->ChangeNodes().SetValue(i + 1, gp_Pnt(g.p[i]));
    if (g.hasp) for (int i = 0; i < g.nn; ++i) P->ChangeParameters().SetValue(i + 1, g.par[i]);
  } else {
    NCollection_Array1<gp_Pnt> a(g.lo, g.lo + g.nn - 1);
    for (int i = 0; i < g.nn; ++i) a.SetValue(g.lo + i, gp_Pnt(g.p[i]));
    if (g.kind == 1) P = new Poly_Polygon3D(a);
    else {
      NCollection_Array1<double> b(g.plo, g.plo + g.np - 1);
      for (int i = 0; i < g.np; ++i) b.SetValue(g.plo + i, g.par[i]);
      P = new Poly_Polygon3D(a, b);
    }
  }
  P->Deflection(g.defl);
  return P;
}
static void put_pg3(const occ::handle<Poly_Polygon3D>& P) {
  in(P->NbNodes()); ob(P->HasParameters()); h(P->Deflection());
  in(P->Nodes().Lower()); in(P->Nodes().Upper());
  for (int i = 1; i <= P->NbNodes(); ++i) { const gp_Pnt& q = P->Nodes().Value(i); h(q.X()); h(q.Y()); h(q.Z()); }
  if (P->HasParameters()) { in(P->Parameters().Lower()); in(P->Parameters().Upper()); for (int i = P->Parameters().Lower(); i <= P->Parameters().Upper(); ++i) h(P->Parameters().Value(i)); }
}
struct Pg2 { int kind = 0; int nn = 0, lo = 1; double defl = 0; std::vector<gp_XY> p; };
static Pg2 rpg2(int att) {
  Pg2 g; g.kind = att % 2; g.nn = att == 0 ? 0 : (att < 4 ? att : rint(1, 8)); if (g.kind == 1 && g.nn < 1) g.nn = 3;
  static const int LOS[] = {1, 0, 4, -3};
  g.lo = g.kind == 0 ? 1 : pick(LOS); g.defl = coin(0.5) ? 0.0 : rnd(0, 1);
  for (int i = 0; i < g.nn; ++i) g.p.push_back(rxy());
  return g;
}
static void p_pg2(const Pg2& g) { in(g.kind); in(g.nn); in(g.lo); h(g.defl); for (const gp_XY& q : g.p) xy(q); }
static void W_pg2(const Pg2& g) { int s = ntok; p_pg2(g); word("pg2", s, -1); }
static occ::handle<Poly_Polygon2D> mk_pg2(const Pg2& g) {
  occ::handle<Poly_Polygon2D> P;
  if (g.kind == 0) {
    P = new Poly_Polygon2D(g.nn);
    for (int i = 0; i < g.nn; ++i) P->ChangeNodes().SetValue(i + 1, gp_Pnt2d(g.p[i]));
  } else {
    NCollection_Array1<gp_Pnt2d> a(g.lo, g.lo + g.nn - 1);
    for (int i = 0; i < g.nn; ++i) a.SetValue(g.lo + i, gp_Pnt2d(g.p[i]));
    P = new Poly_Polygon2D(a);
  }
  P->Deflection(g.defl);
  return P;
}
static void put_pg2(const occ::handle<Poly_Polygon2D>& P) {
  in(P->NbNodes()); h(P->Deflection()); in(P->Nodes().Lower()); in(P->Nodes().Upper());
  for (int i = 1; i <= P->NbNodes(); ++i) { const gp_Pnt2d& q = P->Nodes().Value(i); h(q.X()); h(q.Y()); }
}
// PolygonOnTriangulation
struct Pot { int kind = 0; int nn = 0, hasp = 0, nlo = 1, np = 0, plo = 1; double defl = 0; std::vector<int> n; std::vector<double> par; };
static Pot rpot(int att) {
  Pot g; g.kind = att % 3; g.nn = att == 0 ? 0 : (att < 4 ? att : rint(1, 7)); if (g.kind != 0 && g.nn < 1) g.nn = 2;
  static const int LOS[] = {1, 0, 4, -3};
  g.nlo = g.kind == 0 ? 1 : pick(LOS); g.hasp = g.kind == 0 ? (att % 2) : (g.kind == 2);
  g.np = g.kind == 2 ? (att % 5 == 4 ? g.nn + 2 : (att % 5 == 3 && g.nn > 1 ? g.nn - 1 : g.nn)) : (g.hasp ? g.nn : 0);
  g.plo = g.kind == 2 ? pick(LOS) : 1;
  g.defl = coin(0.5) ? 0.0 : rnd(0, 1);
  for (int i = 0; i < g.nn; ++i) g.n.push_back(rint(-2, 12));
  for (int i = 0; i < g.np; ++i) g.par.push_back(coin(0.5) ? 0.25 * i : rnd(-2, 2));
  return g;
}
static void p_pot(const Pot& g) {
  in(g.kind); in(g.nn); in(g.hasp); in(g.nlo); in(g.np); in(g.plo); h(g.defl);
  for (int v : g.n) in(v);
  for (double v : g.par) h(v);
}
static void W_pot(const Pot& g) { int s = ntok; p_pot(g); word("pot", s, -1); }
static occ::handle<Poly_PolygonOnTriangulation> mk_pot(const Pot& g) {
  occ::handle<Poly_PolygonOnTriangulation> P;
  if (g.kind == 0) {
    P = new Poly_PolygonOnTriangulation(g.nn, g.hasp != 0);
    for (int i = 0; i < g.nn; ++i) P->SetNode(i + 1, g.n[i]);
    if (g.hasp) for (int i = 0; i < g.nn; ++i) P->SetParameter(i + 1, g.par[i]);
  } else {
    NCollection_Array1<int> a(g.nlo, g.nlo + g.nn - 1);
    for (int i = 0; i < g.nn; ++i) a.SetValue(g.nlo + i, g.n[i]);
    if (g.kind == 1) P = new Poly_PolygonOnTriangulation(a);
    else {
      NCollection_Array1<double> b(g.plo, g.plo + g.np - 1);
      for (int i = 0; i < g.np; ++i) b.SetValue(g.plo + i, g.par[i]);
      P = new Poly_PolygonOnTriangulation(a, b);
    }
  }
  P->Deflection(g.defl);
  return P;
}
static void put_pot(const occ::handle<Poly_PolygonOnTriangulation>& P) {
  in(P->NbNodes()); ob(P->HasParameters()); h(P->Deflection()); in(P->Nodes().Lower()); in(P->Nodes().Upper());
  // OCCT 8: the node array and the parameter array keep the lower bound of the arrays they were assigned from
  for (int i = P->Nodes().Lower(); i <= P->Nodes().Upper(); ++i) in(P->Node(i));
  if (P->HasParameters()) { const auto& a = P->Parameters()->Array1(); in(a.Lower()); in(a.Upper()); for (int i = a.Lower(); i <= a.Upper(); ++i) h(a.Value(i)); }
}

static int NG = 28;
static void fam_geom() {
  { LOOP("poly_point_on_triangle", NG) {
    gp_XY a = rxy(), b = rxy(), c = rxy(), p;
    int g = att % 12;
    switch (g) {
      case 1: b = a; break;
      case 2: c = a; break;
      case 3: b = a; c = a; break;
      case 4: { double k = coin(0.5) ? 2.0 : rnd(-2, 3); gp_XY d = b - a; c = a + d * k; } break;       // collinear
      case 5: a = gp_XY(0, 0); b = gp_XY(1, 0); c = gp_XY(0, 1); break;
      case 6: { double s = 1e-160; a = gp_XY(0, 0); b = gp_XY(s, 0); c = gp_XY(0, s); } break;      // determinant below Resolution
      case 7: { double s = 1e-150; a = gp_XY(0, 0); b = gp_XY(s, 0); c = gp_XY(0, s); } break;
      case 8: { gp_XY d = b - a; c = a + d * (-1.0); } break;                                        // collinear, opposite
      case 9: { a = gp_XY(1e-170, 0); b = a; c = gp_XY(2e-170, 1e-170); } break;
      default: break;
    }
    double u = rnd(0, 1), v = rnd(0, 1 - u);
    switch (rint(0, 9)) {
      case 0: p = a; break;
      case 1: p = b; break;
      case 2: p = c; break;
      case 3: p = a + (b - a) * 0.5; break;
      case 4: p = a + (b - a) * u + (c - a) * v; break;
      case 5: p = a + (b - a) * (u + 1) + (c - a) * v; break;
      case 6: p = a + (b - a) * (-u) + (c - a) * (-v); break;
      case 7: p = a + (b - a) * u + (c - a) * (1 - u + 0.5); break;
      case 8: p = a + (b - a) * (u - 0.3) + (c - a) * (v + 0.4); break;
      default: p = rxy(4); break;
    }
    begin(N, "xy xy xy xy"); W_gxy(a); W_gxy(b); W_gxy(c); W_gxy(p); bar();
    emit([&] { gp_XY uv(-77.0, -77.0); double d = Poly::PointOnTriangle(a, b, c, p, uv); hi(d); h(uv.X()); h(uv.Y()); });
  } }
  { LOOP("poly_tri_line", NG) {
    gp_XYZ v0 = mp(crd(), crd(), crd()), v1 = mp(crd(), crd(), crd()), v2 = mp(crd(), crd(), crd());
    double u = rnd(0, 1), v = rnd(0, 1 - u);
    gp_XYZ q = mp(v0.X() + (v1.X() - v0.X()) * u + (v2.X() - v0.X()) * v, v0.Y() + (v1.Y() - v0.Y()) * u + (v2.Y() - v0.Y()) * v, v0.Z() + (v1.Z() - v0.Z()) * u + (v2.Z() - v0.Z()) * v);
    Raw d = mraw(); double t = rnd(-3, 3);
    gp_XYZ s = mp(q.X() - d.x * t, q.Y() - d.y * t, q.Z() - d.z * t);
    switch (att % 9) {
      case 1: s = mp(q.X() + 0.01, q.Y() - 0.02, q.Z() + 0.5); d = Raw{0, 0, -1}; break;
      case 2: { Raw e{v1.X() - v0.X(), v1.Y() - v0.Y(), v1.Z() - v0.Z()}; d = e; } break;                  // parallel to an edge
      case 3: v1 = v0; break;                                                                               // degenerate triangle
      case 4: s = q; break;                                                                                 // starts on the triangle
      case 5: d = Raw{-d.x, -d.y, -d.z}; break;
      case 6: { gp_XYZ nrm = mp((v1.Y() - v0.Y()) * (v2.Z() - v0.Z()) - (v1.Z() - v0.Z()) * (v2.Y() - v0.Y()), (v1.Z() - v0.Z()) * (v2.X() - v0.X()) - (v1.X() - v0.X()) * (v2.Z() - v0.Z()), (v1.X() - v0.X()) * (v2.Y() - v0.Y()) - (v1.Y() - v0.Y()) * (v2.X() - v0.X())); if (nrm.Modulus() > 0) d = Raw{nrm.X(), nrm.Y(), nrm.Z()}; } break;
      default: break;
    }
    if (att == 8) d = Raw{0, 0, 0};                                                                         // gp_Dir throws
    begin(N, "xyz dir xyz xyz xyz"); W_xyz(s); W_dir(d); W_xyz(v0); W_xyz(v1); W_xyz(v2); bar();
    emit([&] { double par = -123.5; int r = Poly::IntersectTriLine(s, D(d), v0, v1, v2, par); in(r); h(par); });
  } }
  { LOOP("poly_intersect", NG) {
    TriSpec s = rtri(att, att % 3 == 0 ? 1 : 0); Mesh& m = s.m;
    if (m.t.empty()) continue;
    int k = rint(0, (int)m.t.size() - 1); gp_XYZ a = m.p[m.t[k][0] - 1], b = m.p[m.t[k][1] - 1], c = m.p[m.t[k][2] - 1];
    double u = rnd(0, 1), v = rnd(0, 1 - u);
    gp_XYZ q = mp(a.X() + (b.X() - a.X()) * u + (c.X() - a.X()) * v, a.Y() + (b.Y() - a.Y()) * u + (c.Y() - a.Y()) * v, a.Z() + (b.Z() - a.Z()) * u + (c.Z() - a.Z()) * v);
    Raw d = coin(0.5) ? Raw{0, 0, -1} : mraw(); double t = rnd(-2, 4) + (att % 4 == 0 ? 0 : 0);
    gp_Pnt loc(q.X() - d.x * t, q.Y() - d.y * t, q.Z() - d.z * t);
    if (att % 5 == 1) loc = gp_Pnt(q.X(), q.Y(), q.Z() + 3.0), d = Raw{0, 0, -1};
    int closest = att % 2;
    begin(N, "tri pnt dir i"); W_tri(s); W_pnt(loc); W_dir(d); W_i(closest); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s); Poly_Triangle tr(7, 8, 9); double dist = -5.0;
      bool r = Poly::Intersect(T, gp_Ax1(loc, D(d)), closest != 0, tr, dist);
      ob(r); in(tr.Value(1)); in(tr.Value(2)); in(tr.Value(3)); h(dist);
    });
  } }
  { LOOP("poly_polygon_properties", NG) {
    int n = att < 3 ? att : rint(0, 7), lo = att % 2 ? 1 : pick((const int[]){0, 3, -2});
    std::vector<gp_XY> p; for (int i = 0; i < n; ++i) p.push_back(rxy(4));
    if (att % 5 == 4 && n >= 3) for (int i = 0; i < n; ++i) p[i] = gp_XY(std::cos(6.283185307179586 * i / n), std::sin(6.283185307179586 * i / n));
    begin(N, "i pts2d"); W_i(lo);
    { int s = ntok; in(n); for (const gp_XY& q : p) xy(q); word("pts2d", s, -1); }
    bar();
    emit([&] {
      NCollection_Array1<gp_Pnt2d> a(lo, lo + n - 1); for (int i = 0; i < n; ++i) a.SetValue(lo + i, gp_Pnt2d(p[i]));
      double area = -1, per = -1;
      if (n == 0) { unchecked(); return; }   // Array1(lo, lo - 1) is accepted but Lower() > Upper(): Value(lo) is outside
      bool r = Poly::PolygonProperties(a, area, per); ob(r); h(area); h(per);
    });
  } }
  { LOOP("pg3_state", NG) {
    Pg3 g = rpg3(att);
    begin(N, "pg3"); W_pg3(g); bar();
    emit([&] {
      if (pg3_unchecked(g)) { unchecked(); return; }   // parameters array of another length or lower bound: read/written outside
      occ::handle<Poly_Polygon3D> P = mk_pg3(g); put_pg3(P);
      occ::handle<Poly_Polygon3D> C = P->Copy(); put_pg3(C);
      if (C->NbNodes() >= 1) { C->ChangeNodes().SetValue(1, gp_Pnt(9, 9, 9)); C->Deflection(5.5); if (C->HasParameters()) C->ChangeParameters().SetValue(1, 77.0); }
      put_pg3(P); put_pg3(C);
    });
  } }
  { LOOP("pg3_params_none", NG) {
    Pg3 g = rpg3(att); g.kind = att % 2; if (g.kind == 1 && g.nn < 1) g.nn = 2; g.hasp = 0; g.par.clear(); g.np = 0; g.lo = g.kind == 0 ? 1 : g.lo;
    if (g.p.size() < (size_t)g.nn) while (g.p.size() < (size_t)g.nn) g.p.push_back(mp(crd(), crd(), crd()));
    g.p.resize(g.nn);
    begin(N, "pg3"); W_pg3(g); bar();
    emit([&] { unchecked(); (void)g; });   // Parameters() of a polygon without parameters dereferences a null handle
  } }
  { LOOP("pg2_state", NG) {
    Pg2 g = rpg2(att);
    begin(N, "pg2"); W_pg2(g); bar();
    emit([&] {
      occ::handle<Poly_Polygon2D> P = mk_pg2(g); put_pg2(P);
      occ::handle<Poly_Polygon2D> C = P->Copy(); put_pg2(C);
      if (C->NbNodes() >= 1) { C->ChangeNodes().SetValue(1, gp_Pnt2d(9, 9)); C->Deflection(5.5); }
      put_pg2(P); put_pg2(C);
    });
  } }
  { LOOP("pot_state", NG) {
    Pot g = rpot(att);
    begin(N, "pot"); W_pot(g); bar();
    emit([&] {
      occ::handle<Poly_PolygonOnTriangulation> P = mk_pot(g); put_pot(P);
      // Copy() builds the parameter array with the number of nodes and copies the shorter of the two arrays (the rest is
      // uninitialised), so it is only specified when the parameters are absent or as many as the nodes
      if (P->HasParameters() && P->Parameters()->Length() != P->NbNodes()) { put_pot(P); return; }
      occ::handle<Poly_PolygonOnTriangulation> C = P->Copy(); put_pot(C);
      if (C->NbNodes() >= 1) { C->SetNode(1, 99); C->Deflection(5.5); if (C->HasParameters() && C->Parameters()->Length() >= 1) C->SetParameter(1, 77.0); }
      put_pot(P); put_pot(C);
    });
  } }
  { LOOP("pot_ops", NG) {
    int op = att % 7, idx, iv = rint(0, 4); double fv = rnd(-1, 1);
    Pot g = rpot(att + 1);
    for (int k = 1; k < 40 && att % 4 != 0 && (g.nn < 2 || ((op == 2 || op == 3 || op == 6) && g.np < 1)); ++k) g = rpot(att + 1 + k * 7);
    int nl = g.nlo, pl = g.kind == 2 ? g.plo : 1, plen = g.kind == 2 ? g.np : (g.hasp ? g.nn : 0);
    bool par_op = op == 2 || op == 3 || op == 6;
    int base = par_op ? pl : nl, cnt = par_op ? std::max(plen, 1) : std::max(g.nn, 1);
    switch ((att / 7) % 4) { case 0: idx = base; break; case 1: idx = base - 1; break; case 2: idx = base + cnt; break; default: idx = base + rint(0, cnt - 1); }
    if (op == 4) { idx = nl + pick((const int[]){0, 1, -1, 0}); if (att % 3 == 0) iv = g.nn; }
    begin(N, "pot i i i f"); W_pot(g); W_i(op); W_i(idx); W_i(iv); W_f(fv); bar();
    emit([&] {
      occ::handle<Poly_PolygonOnTriangulation> P = mk_pot(g);
      switch (op) {
        case 0: in(P->Node(idx)); break;
        case 1: P->SetNode(idx, iv); put_pot(P); break;
        case 2: h(P->Parameter(idx)); break;
        case 3: P->SetParameter(idx, fv); put_pot(P); break;
        case 4: {
          occ::handle<NCollection_HArray1<double>> H;
          if (iv > 0) { H = new NCollection_HArray1<double>(idx, idx + iv - 1); for (int k = 0; k < iv; ++k) H->SetValue(idx + k, fv + k); }
          P->SetParameters(H); put_pot(P);
        } break;
        case 5: { P->ChangeNodeArray().SetValue(idx, iv); put_pot(P); } break;
        default: { P->ChangeParameterArray().SetValue(idx, fv); put_pot(P); } break;
      }
    });
  } }
  // ---- text formats
  { LOOP("poly_write_tri", NG) {
    TriSpec s = rtri(att, -2); if (s.m.p.size() > 9) { s.m.p.resize(9); for (T3& t : s.m.t) for (int k = 0; k < 3; ++k) t[k] = (t[k] - 1) % 9 + 1; if (s.uv) s.m.uv.resize(9); if (s.nrm) s.m.n.resize(9); if (s.kind == 2) s.lo = 1; }
    static const int PR[] = {6, 6, 1, 3, 10, 15, 17, 12};
    int compact = att % 2, prec = att < 16 ? PR[att % 8] : pick(PR);
    begin(N, "tri i i"); W_tri(s); W_i(compact); W_i(prec); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      os_text(write_text([&](std::ostream& os) { Poly::Write(T, os, compact != 0); }, prec));
      if (!compact) os_text(write_text([&](std::ostream& os) { Poly::Dump(T, os); }, prec));
    });
  } }
  { LOOP("poly_write_pg3", NG) {
    Pg3 g = rpg3(att); if (pg3_unchecked(g)) { g.np = g.nn; g.plo = g.lo; g.par.resize(g.nn); while ((int)g.par.size() < g.nn) g.par.push_back(0.5); }
    static const int PR[] = {6, 6, 1, 3, 10, 15, 17, 12};
    int compact = att % 2, prec = att < 16 ? PR[att % 8] : pick(PR);
    begin(N, "pg3 i i"); W_pg3(g); W_i(compact); W_i(prec); bar();
    emit([&] {
      occ::handle<Poly_Polygon3D> P = mk_pg3(g);
      os_text(write_text([&](std::ostream& os) { Poly::Write(P, os, compact != 0); }, prec));
      if (!compact) os_text(write_text([&](std::ostream& os) { Poly::Dump(P, os); }, prec));
    });
  } }
  { LOOP("poly_write_pg2", NG) {
    Pg2 g = rpg2(att);
    static const int PR[] = {6, 6, 1, 3, 10, 15, 17, 12};
    int compact = att % 2, prec = att < 16 ? PR[att % 8] : pick(PR);
    begin(N, "pg2 i i"); W_pg2(g); W_i(compact); W_i(prec); bar();
    emit([&] {
      occ::handle<Poly_Polygon2D> P = mk_pg2(g);
      os_text(write_text([&](std::ostream& os) { Poly::Write(P, os, compact != 0); }, prec));
      if (!compact) os_text(write_text([&](std::ostream& os) { Poly::Dump(P, os); }, prec));
    });
  } }
}

// =================================================================================
// Poly::Read*, Poly_Connect
static std::string mutate_text(const std::string& base, int mode, const char* header) {
  std::string t = base;
  switch (mode) {
    case 0: return t;
    case 1: { std::string o; for (char c : t) { if (c == ' ' && coin(0.5)) o += coin(0.5) ? "   " : "\n"; else if (c == '\n' && coin(0.5)) o += "\n\n  "; else o += c; } return o; }
    case 2: { size_t p = t.find(header); if (p != std::string::npos) t.replace(p, std::strlen(header), std::string(header) + "2"); return t; }
    case 3: { size_t p = t.find(header); if (p != std::string::npos) { t[p] = (char)std::tolower((unsigned char)t[p]); } return t; }
    case 4: return "";
    case 5: return std::string(header);
    case 6: return t + "\n  trailing junk 1 2 3\n";   // text after the data is ignored (a truncated text would read uninitialised values)
    case 7: return "Poly_Polygon3D\n2 0\n0\n0 0 0 1 1 1\n";
    case 8: { size_t p = t.find('\n'); return p == std::string::npos ? t : std::string(header) + " " + t.substr(p + 1); }   // header without newline
    case 9: { std::string o = t; for (char& c : o) if (c == '\n') c = ' '; return o; }
    default: { size_t p = t.find('\n'); return p == std::string::npos ? t : t.substr(0, p + 1) + "0 0 0\n0\n"; }   // zero counts
  }
}
static void put_adjacency(Poly_Connect& c, const occ::handle<Poly_Triangulation>& T) {
  for (int t = 1; t <= T->NbTriangles(); ++t) { int a, b, d, n1, n2, n3; c.Triangles(t, a, b, d); c.Nodes(t, n1, n2, n3); in(a); in(b); in(d); in(n1); in(n2); in(n3); }
  for (int n = 1; n <= T->NbNodes(); ++n) in(c.Triangle(n));
}
static int NR = 28;
static void fam_reading() {
  g_guard = true;
  { LOOP("poly_read_tri", NR) {
    TriSpec s = rtri(att, -2); int mode = att % 11;
    if (mode == 7) s.uv = s.uv;
    std::string base = write_text([&](std::ostream& os) { Poly::Write(mk_tri(s), os, true); }, att % 3 == 1 ? 6 : 17);
    if (mode == 9 && att > 20) base = write_text([&](std::ostream& os) { Poly::Write(mk_tri(s), os, false); }, 17);
    std::string txt = mutate_text(base, mode, "Poly_Triangulation");
    begin(N, "txt"); W_txt(txt); bar();
    emit([&] {
      std::istringstream is(txt); occ::handle<Poly_Triangulation> r = Poly::ReadTriangulation(is);
      ob(!r.IsNull()); if (!r.IsNull()) put_tri_state(r);
    });
  } }
  { LOOP("poly_read_pg3", NR) {
    Pg3 g = rpg3(att); if (pg3_unchecked(g)) { g.np = g.nn; g.plo = g.lo; g.par.resize(g.nn); while ((int)g.par.size() < g.nn) g.par.push_back(0.5); }
    int mode = att % 11;
    std::string base = write_text([&](std::ostream& os) { Poly::Write(mk_pg3(g), os, true); }, att % 3 == 1 ? 6 : 17);
    std::string txt = mutate_text(base, mode, "Poly_Polygon3D");
    if (mode == 7) txt = "Poly_Triangulation\n1 0 0\n0\n0 0 0\n";
    begin(N, "txt"); W_txt(txt); bar();
    emit([&] {
      std::istringstream is(txt); occ::handle<Poly_Polygon3D> r = Poly::ReadPolygon3D(is);
      ob(!r.IsNull()); if (!r.IsNull()) put_pg3(r);
    });
  } }
  { LOOP("poly_read_pg2", NR) {
    Pg2 g = rpg2(att); int mode = att % 11;
    std::string base = write_text([&](std::ostream& os) { Poly::Write(mk_pg2(g), os, true); }, att % 3 == 1 ? 6 : 17);
    std::string txt = mutate_text(base, mode, "Poly_Polygon2D");
    if (mode == 7) txt = "Poly_Polygon3D\n1 0\n0\n0 0 0\n";
    begin(N, "txt"); W_txt(txt); bar();
    emit([&] {
      std::istringstream is(txt); occ::handle<Poly_Polygon2D> r = Poly::ReadPolygon2D(is);
      ob(!r.IsNull()); if (!r.IsNull()) put_pg2(r);
    });
  } }
  { LOOP("connect_adjacency", NR) {
    TriSpec s = rtri(att, -2);
    begin(N, "tri"); W_tri(s); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s); Poly_Connect c(T);
      ob(c.Triangulation().get() == T.get()); ob(c.More()); in(c.Value()); put_adjacency(c, T);
    });
  } }
  { LOOP("connect_around", NR) {
    TriSpec s = rtri(att, -2);
    begin(N, "tri"); W_tri(s); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s); Poly_Connect c(T);
      for (int n = 1; n <= T->NbNodes(); ++n) {
        c.Initialize(n); ob(c.More()); std::vector<int> seq;
        for (int guard = 0; c.More() && guard < 4 * T->NbTriangles() + 8; ++guard) { seq.push_back(c.Value()); c.Next(); }
        in((long)seq.size()); for (int v : seq) in(v); ob(c.More());
      }
    });
  } }
  { LOOP("connect_reload", NR) {
    TriSpec s1 = rtri(att, -2), s2 = rtri(att * 3 + 1, -2); int first = att % 3;
    begin(N, "tri tri i"); W_tri(s1); W_tri(s2); W_i(first); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T1 = mk_tri(s1), T2 = mk_tri(s2);
      Poly_Connect c; ob(c.Triangulation().IsNull()); ob(c.More()); in(c.Value());
      if (first == 0) { c.Load(T1); put_adjacency(c, T1); }
      else if (first == 1) { Poly_Connect d(T1); put_adjacency(d, T1); c.Load(T1); }
      c.Load(T2); ob(c.Triangulation().get() == T2.get()); ob(c.More()); in(c.Value()); put_adjacency(c, T2);
      c.Initialize(1); ob(c.More()); in(c.Value());
    });
  } }
  { LOOP("connect_checks", NR) {
    TriSpec s = rtri(att, -2); int nn = (int)s.m.p.size(), nt = (int)s.m.t.size();
    int op = att % 4, dflt = att % 7 == 3, idx;
    int cnt = (op == 0 || op == 3) ? nn : nt;
    switch ((att / 4) % 5) { case 0: idx = 0; break; case 1: idx = cnt + 1; break; case 2: idx = 1; break; case 3: idx = cnt; break; default: idx = rint(1, std::max(1, cnt)); }
    begin(N, "tri i i i"); W_tri(s); W_i(op); W_i(idx); W_i(dflt); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      Poly_Connect c; if (!dflt) c.Load(T);
      if (dflt && op == 3) { unchecked(); return; }   // Initialize on a default object reads an empty array without a check (crash)
      if (op == 3 && (idx < 1 || idx > nn)) { unchecked(); return; }   // Triangle(N) inside the library is not range-checked
      switch (op) {
        case 0: in(c.Triangle(idx)); break;
        case 1: { int a, b, d; c.Triangles(idx, a, b, d); in(a); in(b); in(d); } break;
        case 2: { int a, b, d; c.Nodes(idx, a, b, d); in(a); in(b); in(d); } break;
        default: c.Initialize(idx); ob(c.More()); in(c.Value()); break;
      }
    });
  } }
  g_guard = false;
}

// =================================================================================
// Poly_MergeNodesTool
struct MOp { int code = 0; gp_XYZ q[4]; TriSpec t; TR tr; int rev = 0; };
static void p_mops(const std::vector<MOp>& ops) {
  in((long)ops.size());
  for (const MOp& o : ops) {
    in(o.code);
    if (o.code == 0) for (int k = 0; k < 3; ++k) xyz(o.q[k]);
    else if (o.code == 1) for (int k = 0; k < 4; ++k) xyz(o.q[k]);
    else { p_tri(o.t); p_trsf(o.tr); in(o.rev); }
  }
}
static void W_mops(const std::vector<MOp>& ops) { int s = ntok; p_mops(ops); word("mops", s, -1); }
static double jit(double tol) {
  double r = rnd(0, 1);
  if (r < 0.55) return 0.0;
  double t = tol > 0 ? tol : 1e-7;
  static const double F[] = {0.4, 0.9, 1.05, 2.5, 0.01, 1.0, 0.999};
  double v = t * pick(F); return coin(0.5) ? v : -v;
}
static std::vector<MOp> rmops(int att, double tol) {
  std::vector<MOp> ops; Mesh m = rmesh(att);
  if (att % 5 == 4) {
    TriSpec s = rtri(att, att % 2); MOp o; o.code = 2; o.t = s; o.tr = rtr(att % 3 == 0 ? 0 : rint(0, 11)); o.rev = coin(0.4); ops.push_back(o);
    if (coin(0.4)) { MOp o2 = o; o2.rev = !o.rev; ops.push_back(o2); }
    return ops;
  }
  // a triangle soup of the mesh with jittered duplicates of the corners
  for (const T3& t : m.t) {
    bool quad = coin(0.2);
    MOp o; o.code = quad ? 1 : 0;
    for (int k = 0; k < 3; ++k) { const gp_XYZ& b = m.p[t[k] - 1]; o.q[k] = gp_XYZ(b.X() + jit(tol), b.Y() + jit(tol), b.Z() + jit(tol)); }
    if (quad) { const gp_XYZ& b = m.p[t[(att + 1) % 3] - 1]; o.q[3] = gp_XYZ(b.X() + 0.5 * rint(0, 2), b.Y() + jit(tol), b.Z() + jit(tol)); }
    ops.push_back(o);
    if (coin(0.1)) ops.push_back(o);   // an exact duplicate element
  }
  if (att % 7 == 3) { MOp d; d.code = 0; d.q[0] = d.q[1] = d.q[2] = m.p[0]; ops.push_back(d); }   // degenerate element
  return ops;
}
struct MCfg { double angle, tol, unit; int nbf, flags; };
static MCfg rmcfg(int att) {
  static const double ANG[] = {0.0, 0.1, 0.5, 1.0, 1.5707963267948966, 2.5, 3.141592653589793, 0.9, 0.3};
  static const double TOL[] = {0.0, 1e-3, 0.01, 0.1, 0.5, 1e-7, 0.0, 0.05};
  static const double UN[] = {1.0, 1.0, 0.001, 2.5, 25.4};
  MCfg c; c.angle = att < 9 ? ANG[att] : pick(ANG); c.tol = att < 16 ? TOL[(att / 2) % 8] : pick(TOL); c.unit = pick(UN);
  c.nbf = pick((const int[]){-1, 1, 10, 100}); c.flags = rint(0, 7);
  if (att % 6 == 0) c.flags &= ~2;
  else c.flags |= 2;
  return c;
}
static void W_mcfg(const MCfg& c) { int s = ntok; h(c.angle); h(c.tol); h(c.unit); in(c.nbf); in(c.flags); word("mcfg", s, 5); }
static void run_merge(const MCfg& c, const std::vector<MOp>& ops, int nbf, bool per_elem) {
  Poly_MergeNodesTool t(c.angle, c.tol, nbf);
  t.SetMergeOpposite((c.flags & 1) != 0); t.SetDropDegenerative((c.flags & 2) != 0); t.SetMergeElems((c.flags & 4) != 0); t.SetUnitFactor(c.unit);
  h(t.MergeTolerance()); h(t.MergeAngle()); ob(t.ToMergeOpposite()); ob(t.ToDropDegenerative()); ob(t.ToMergeElems());
  if (per_elem) for (int k = 0; k < 4; ++k) in(t.ElementNodeIndex(k));
  for (const MOp& o : ops) {
    if (o.code == 0) { gp_XYZ q[3] = {o.q[0], o.q[1], o.q[2]}; t.AddTriangle(q); }
    else if (o.code == 1) { gp_XYZ q[4] = {o.q[0], o.q[1], o.q[2], o.q[3]}; t.AddQuad(q); }
    else { occ::handle<Poly_Triangulation> T = mk_tri(o.t); t.AddTriangulation(T, o.tr.t, o.rev != 0); }
    if (per_elem) { for (int k = 0; k < 4; ++k) in(t.ElementNodeIndex(k)); }
  }
  in(t.NbNodes()); in(t.NbElements()); in(t.NbDegenerativeElems()); in(t.NbMergedElems());
  occ::handle<Poly_Triangulation> r = t.Result();
  ob(!r.IsNull()); if (!r.IsNull()) put_tri_state(r);
}
// the result may not depend on the number of buckets of the node map
static bool same_for_all(const std::function<void(int)>& f, int given) {
  static const int V[] = {-1, 1, 2, 3, 7, 40, 997, 5000};
  size_t keep = (size_t)ntok; bool d0 = false;
  std::string r0 = run_once([&] { f(given); }, 0, d0); ntok = (int)keep;
  if (d0) return false;
  for (int v : V) { bool d = false; std::string r = run_once([&] { f(v); }, 0, d); ntok = (int)keep; if (d || r != r0) return false; }
  return true;
}
static int NM = 28;
static void fam_merge() {
  { LOOP("merge_tool", NM) {
    MCfg c = rmcfg(att); std::vector<MOp> ops = rmops(att, c.tol);
    begin(N, "mcfg mops"); W_mcfg(c); W_mops(ops); bar();
    if (!same_for_all([&](int nbf) { run_merge(c, ops, nbf, true); }, c.nbf)) { fam_dropped[N]++; fam_dropped[std::string(N) + "!buckets"]++; buf.clear(); continue; }
    emit([&] { run_merge(c, ops, c.nbf, true); }, true, 0.0);
  } }
  { LOOP("merge_static", NM) {
    TriSpec s = rtri(att, att % 3 == 0 ? 1 : 0); MCfg c = rmcfg(att + 3); TR t = rtr(att % 4 == 0 ? 0 : rint(0, 11)); int rev = att % 2, force = (att / 2) % 2;
    begin(N, "tri trsf i f f i"); W_tri(s); W_trsf(t); W_i(rev); W_f(c.angle); W_f(c.tol); W_i(force); bar();
    auto stat = [&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      occ::handle<Poly_Triangulation> r = Poly_MergeNodesTool::MergeNodes(T, t.t, rev != 0, c.angle, c.tol, force != 0);
      ob(!r.IsNull()); if (!r.IsNull()) put_tri_state(r);
    };
    size_t keep = (size_t)ntok; bool d0 = false; std::string r0 = run_once(stat, 0, d0); ntok = (int)keep;
    bool ok = !d0;
    static const int V[] = {-1, 1, 2, 7, 40, 997};
    for (int v : V) {
      if (!ok) break;
      if (!force) break;   // the variants below cannot decide the "no change" return
      bool d = false;
      std::string r = run_once([&] {
        occ::handle<Poly_Triangulation> T = mk_tri(s);
        if (T->NbNodes() < 3 || T->NbTriangles() < 1) { ob(false); return; }
        Poly_MergeNodesTool tool(c.angle, c.tol, v); tool.AddTriangulation(T, t.t, rev != 0);
        occ::handle<Poly_Triangulation> r2 = tool.Result(); ob(!r2.IsNull()); if (!r2.IsNull()) put_tri_state(r2);
      }, 0, d);
      ntok = (int)keep; if (d || r != r0) ok = false;
    }
    if (!ok) { fam_dropped[N]++; fam_dropped[std::string(N) + "!buckets"]++; buf.clear(); continue; }
    emit(stat, true, 0.0);
  } }
  { LOOP("merge_params", NM) {
    static const double ANG[] = {0.0, 0.1, 0.5, 1.0, 1.5707963267948966, 3.141592653589793, -1.0, 1e-10, 7.0}, TOL[] = {0.0, 1e-3, 0.1, 0.5, 1e-7, -1.0, 1e-10, 3.4e38, 1e300};
    double a1 = ANG[att % 9], t1 = TOL[(att / 9) % 9], a2 = coin(0.5) ? pick(ANG) : rnd(0, 3), t2 = coin(0.5) ? pick(TOL) : rnd(0, 1);
    begin(N, "f f f f"); W_f(a1); W_f(t1); W_f(a2); W_f(t2); bar();
    emit([&] {
      Poly_MergeNodesTool t(a1, t1);
      h(t.MergeAngle()); h(t.MergeTolerance()); ob(t.ToMergeOpposite()); ob(t.ToDropDegenerative()); ob(t.ToMergeElems());
      t.SetMergeAngle(a2); t.SetMergeTolerance(t2); t.SetMergeOpposite(true); t.SetDropDegenerative(false); t.SetMergeElems(true);
      h(t.MergeAngle()); h(t.MergeTolerance()); ob(t.ToMergeOpposite()); ob(t.ToDropDegenerative()); ob(t.ToMergeElems());
    }, true, 0.0);
  } }
  { LOOP("merge_element_errors", NM) {
    int nbn = att < 8 ? att - 1 : rint(-1, 6), via = att % 2; gp_XYZ q[4]; for (int k = 0; k < 4; ++k) q[k] = mp(crd(), crd(), crd());
    double ang = att % 3 == 0 ? 0.0 : 0.5;
    begin(N, "i i f xyz xyz xyz xyz"); W_i(nbn); W_i(via); W_f(ang); for (int k = 0; k < 4; ++k) W_xyz(q[k]); bar();
    emit([&] {
      Poly_MergeNodesTool t(ang, 0.0, 10);
      if (via == 0) t.AddElement(q, nbn);
      else { for (int k = 0; k < 4; ++k) t.ChangeElementNode(k) = q[k]; t.PushLastElement(nbn); }
      in(t.NbNodes()); in(t.NbElements()); in(t.NbDegenerativeElems()); for (int k = 0; k < 4; ++k) in(t.ElementNodeIndex(k));
    }, true, 0.0);
  } }
}

// =================================================================================
// Poly_MakeLoops (3D and 2D), the packed map and the heap of integers it uses
struct LOpSpec { int code; int a[6]; };
struct LoopSpec {
  int dim = 3, left = 0, nn = 0;
  std::vector<gp_XYZ> pos;                          // nodes 1..nn (z unused in 2D)
  std::vector<std::array<int, 3>> hl;               // helper links: node1 node2 tangent mask (bit0: no first tangent, bit1: no last)
  std::vector<std::pair<int, Raw>> nrm;             // 3D: per node 1..nn: has, raw normal
  std::vector<LOpSpec> ops;                         // 0 AddLink n1 n2 flags  1 ReplaceLink n1 n2 f m1 m2 f2  2 SetLinkOrientation n1 n2 o  3 FindLink n1 n2  4 Perform
};
static void p_loops(const LoopSpec& s) {
  in(s.dim); in(s.left); in(s.nn);
  for (int i = 0; i < s.nn; ++i) { h(s.pos[i].X()); h(s.pos[i].Y()); if (s.dim == 3) h(s.pos[i].Z()); }
  in((long)s.hl.size()); for (auto& l : s.hl) { in(l[0]); in(l[1]); in(l[2]); }
  if (s.dim == 3) for (auto& n : s.nrm) { in(n.first); rw(n.second); }
  in((long)s.ops.size());
  for (const LOpSpec& o : s.ops) {
    in(o.code);
    int n = o.code == 0 ? 3 : (o.code == 1 ? 6 : (o.code == 2 ? 3 : (o.code == 3 ? 2 : 0)));
    for (int k = 0; k < n; ++k) in(o.a[k]);
  }
}
static void W_loops(const LoopSpec& s) { int st = ntok; p_loops(s); word("loops", st, -1); }
typedef Poly_MakeLoops::Link LLink;
struct TableCore {
  const LoopSpec* s;
  std::vector<Poly_MakeLoops::ListOfLink> adj;
  Poly_MakeLoops::ListOfLink none;
  explicit TableCore(const LoopSpec* sp) : s(sp), adj((size_t)sp->nn + 1) {
    for (auto& l : sp->hl) {
      LLink k(l[0], l[1]);
      if (l[0] >= 1 && l[0] <= sp->nn) adj[(size_t)l[0]].Append(k);
      if (l[1] >= 1 && l[1] <= sp->nn && l[1] != l[0]) adj[(size_t)l[1]].Append(k);
    }
  }
  const Poly_MakeLoops::ListOfLink& links(int node) const { return node >= 1 && node <= s->nn ? adj[(size_t)node] : none; }
  // direction of a link: effective start -> end; false when the helper has no tangent
  bool tangent(const LLink& l, bool first, double& dx, double& dy, double& dz) const {
    int mask = 0;
    for (auto& h2 : s->hl) if ((h2[0] == l.node1 && h2[1] == l.node2) || (h2[0] == l.node2 && h2[1] == l.node1)) { mask = h2[2]; break; }
    if (mask & (first ? 1 : 2)) return false;
    int a = l.IsReversed() ? l.node2 : l.node1, b = l.IsReversed() ? l.node1 : l.node2;
    if (a < 1 || a > s->nn || b < 1 || b > s->nn) return false;
    dx = s->pos[(size_t)b - 1].X() - s->pos[(size_t)a - 1].X(); dy = s->pos[(size_t)b - 1].Y() - s->pos[(size_t)a - 1].Y(); dz = s->pos[(size_t)b - 1].Z() - s->pos[(size_t)a - 1].Z();
    return true;
  }
};
struct Helper3 : Poly_MakeLoops3D::Helper {
  TableCore c; explicit Helper3(const LoopSpec* s) : c(s) {}
  const Poly_MakeLoops::ListOfLink& GetAdjacentLinks(int node) const override { return c.links(node); }
  bool GetFirstTangent(const LLink& l, gp_Dir& d) const override { double x, y, z; if (!c.tangent(l, true, x, y, z) || (x == 0 && y == 0 && z == 0)) return false; d = gp_Dir(x, y, z); return true; }
  bool GetLastTangent(const LLink& l, gp_Dir& d) const override { double x, y, z; if (!c.tangent(l, false, x, y, z) || (x == 0 && y == 0 && z == 0)) return false; d = gp_Dir(x, y, z); return true; }
  bool GetNormal(int node, gp_Dir& d) const override {
    if (node < 1 || node > c.s->nn || !c.s->nrm[(size_t)node - 1].first) return false;
    const Raw& r = c.s->nrm[(size_t)node - 1].second; d = gp_Dir(r.x, r.y, r.z); return true;
  }
};
struct Helper2 : Poly_MakeLoops2D::Helper {
  TableCore c; explicit Helper2(const LoopSpec* s) : c(s) {}
  const Poly_MakeLoops::ListOfLink& GetAdjacentLinks(int node) const override { return c.links(node); }
  bool GetFirstTangent(const LLink& l, gp_Dir2d& d) const override { double x, y, z; if (!c.tangent(l, true, x, y, z) || (x == 0 && y == 0)) return false; d = gp_Dir2d(x, y); return true; }
  bool GetLastTangent(const LLink& l, gp_Dir2d& d) const override { double x, y, z; if (!c.tangent(l, false, x, y, z) || (x == 0 && y == 0)) return false; d = gp_Dir2d(x, y); return true; }
};
static void put_link(const LLink& l) { in(l.node1); in(l.node2); in(l.flags); }
static void run_loops(const LoopSpec& s) {
  Helper3 h3(&s); Helper2 h2(&s);
  std::unique_ptr<Poly_MakeLoops> ml;
  if (s.dim == 3) ml.reset(new Poly_MakeLoops3D(&h3, nullptr)); else ml.reset(new Poly_MakeLoops2D(s.left != 0, &h2, nullptr));
  for (const LOpSpec& o : s.ops) {
    switch (o.code) {
      case 0: { LLink l(o.a[0], o.a[1]); l.flags = o.a[2]; ml->AddLink(l); } break;
      case 1: { LLink l(o.a[0], o.a[1]), m(o.a[3], o.a[4]); m.flags = o.a[5]; ml->ReplaceLink(l, m); } break;
      case 2: in((int)ml->SetLinkOrientation(LLink(o.a[0], o.a[1]), (Poly_MakeLoops::LinkFlag)o.a[2])); break;
      case 3: put_link(ml->FindLink(LLink(o.a[0], o.a[1]))); break;
      default: {
        in(ml->Perform()); in(ml->GetNbLoops());
        for (int i = 1; i <= ml->GetNbLoops(); ++i) { const Poly_MakeLoops::Loop& L = ml->GetLoop(i); in(L.Size()); for (auto it = L.cbegin(); it != L.cend(); ++it) put_link(*it); }
        in(ml->GetNbHanging()); Poly_MakeLoops::ListOfLink hang; ml->GetHangingLinks(hang); in(hang.Size()); for (auto it = hang.cbegin(); it != hang.cend(); ++it) put_link(*it);
      } break;
    }
  }
}
// a replacement that gives a second entry of the map the key of an existing one is not checked (unchecked)
static bool loops_unchecked(const LoopSpec& s) {
  std::vector<std::pair<int, int>> keys;
  auto find = [&](int a, int b) { for (size_t i = 0; i < keys.size(); ++i) if ((keys[i].first == a && keys[i].second == b) || (keys[i].first == b && keys[i].second == a)) return (int)i; return -1; };
  for (const LOpSpec& o : s.ops) {
    if (o.code == 0) { if (o.a[0] != o.a[1] && find(o.a[0], o.a[1]) < 0) keys.push_back({o.a[0], o.a[1]}); }
    else if (o.code == 1) {
      if (o.a[3] == o.a[4]) continue;
      int i = find(o.a[0], o.a[1]); if (i < 0) { keys.push_back({o.a[0], o.a[1]}); i = (int)keys.size() - 1; }
      int j = find(o.a[3], o.a[4]); if (j >= 0 && j != i) return true;
      keys[(size_t)i] = {o.a[3], o.a[4]};
    }
  }
  return false;
}
static LoopSpec rloops(int att) {
  LoopSpec s; s.dim = att % 2 ? 2 : 3; s.left = (att / 2) % 2; s.nn = att < 6 ? 3 + att % 4 : rint(3, 9);
  bool grid = coin(0.6);
  for (int i = 0; i < s.nn; ++i) s.pos.push_back(grid ? mp(rint(0, 4), rint(0, 4), s.dim == 3 && coin(0.2) ? rint(0, 2) : 0) : mp(rnd(-3, 3), rnd(-3, 3), s.dim == 3 ? rnd(-1, 1) : 0.0));
  if (coin(0.15) && s.nn > 3) s.pos[1] = s.pos[0];   // coincident nodes: no tangent
  for (int i = 0; i < s.nn; ++i) { Raw r = coin(0.75) ? Raw{0, 0, 1} : mraw(); s.nrm.push_back({coin(0.85), r}); }
  // links: cycles through random node subsets, chords, tails and duplicates
  std::vector<std::pair<int, int>> L;
  int ncyc = rint(1, 3);
  for (int c = 0; c < ncyc; ++c) {
    std::vector<int> p; for (int i = 1; i <= s.nn; ++i) p.push_back(i);
    for (int i = (int)p.size() - 1; i > 0; --i) std::swap(p[(size_t)i], p[(size_t)rint(0, i)]);
    int len = rint(3, s.nn);
    for (int i = 0; i < len; ++i) L.push_back({p[(size_t)i], p[(size_t)((i + 1) % len)]});
  }
  int extra = rint(0, 3); for (int i = 0; i < extra; ++i) L.push_back({rint(1, s.nn), rint(1, s.nn)});
  if (coin(0.15)) L.push_back(L[(size_t)rint(0, (int)L.size() - 1)]);
  if (coin(0.1)) L.push_back({rint(1, s.nn), s.nn + 1});    // a node the helper does not know
  for (auto& l : L) {
    int f = coin(0.7) ? 1 : (coin(0.4) ? 2 : (coin(0.5) ? 3 : 5));
    if (coin(0.25)) std::swap(l.first, l.second);
    LOpSpec o{0, {l.first, l.second, f, 0, 0, 0}}; s.ops.push_back(o);
    int mask = coin(0.8) ? 0 : rint(1, 3);
    bool dup = false; for (auto& h2 : s.hl) if ((h2[0] == l.first && h2[1] == l.second) || (h2[0] == l.second && h2[1] == l.first)) dup = true;
    if (!dup) s.hl.push_back({l.first, l.second, mask});
  }
  if (coin(0.3)) s.hl.push_back({rint(1, s.nn), rint(1, s.nn), 0});   // a helper link that is not in the map
  if (coin(0.3)) { std::swap(s.hl[(size_t)rint(0, (int)s.hl.size() - 1)], s.hl[(size_t)rint(0, (int)s.hl.size() - 1)]); }
  if (coin(0.3)) { LOpSpec o{2, {L[0].first, L[0].second, rint(0, 3), 0, 0, 0}}; s.ops.push_back(o); }
  if (coin(0.25)) { LOpSpec o{3, {L[(size_t)rint(0, (int)L.size() - 1)].second, L[0].first, 0, 0, 0}}; s.ops.push_back(o); }
  if (coin(0.2)) { LOpSpec o{1, {L[0].first, L[0].second, 1, s.nn, rint(1, s.nn), 1}}; s.ops.push_back(o); }
  if (coin(0.15)) { LOpSpec o{3, {s.nn, 1, 0, 0, 0, 0}}; s.ops.push_back(o); }
  s.ops.push_back(LOpSpec{4, {0, 0, 0, 0, 0, 0}});
  if (coin(0.1)) s.ops.push_back(LOpSpec{4, {0, 0, 0, 0, 0, 0}});     // a second Perform
  return s;
}
// ---- packed map and heap
struct MapOp { int code; int key; };
static void W_mapops(int nb, const std::vector<MapOp>& ops) { int s = ntok; in(nb); in((long)ops.size()); for (auto& o : ops) { in(o.code); in(o.key); } word("mapops", s, -1); }
static std::vector<MapOp> rmapops(int att, bool heap) {
  std::vector<MapOp> ops; int n = att < 4 ? att + 1 : rint(2, 30); std::set<int> shadow;
  int span = att % 3 == 0 ? 40 : (att % 3 == 1 ? 400 : 70000);
  for (int i = 0; i < n; ++i) {
    int key = coin(0.3) ? -rint(0, span) : rint(0, span); if (coin(0.05)) key = coin(0.5) ? 2147483647 : -2147483647 - 1;
    double r = rnd(0, 1);
    if (!heap) {
      int code = r < 0.55 ? 0 : (r < 0.7 ? 1 : (r < 0.8 ? 2 : (r < 0.9 ? 3 : (r < 0.95 ? 4 : 5))));      // 0 Add 1 Remove 2 Contains 3 snapshot (extent + iteration) 4 Clear 5 Remove an element
      if (code == 5 && !shadow.empty()) { auto it = shadow.begin(); std::advance(it, rint(0, (int)shadow.size() - 1)); key = *it; code = 1; }
      if (code == 0) shadow.insert(key); else if (code == 1) shadow.erase(key); else if (code == 4) shadow.clear();
      ops.push_back({code == 5 ? 3 : code, key});
    } else {
      int code = r < 0.5 ? 0 : (r < 0.65 ? 1 : (r < 0.75 ? 2 : (r < 0.9 ? 3 : 4)));       // 0 Add 1 Remove 2 Contains 3 Top (when not empty) 4 IsEmpty 5 Clear
      if (code == 1 && !shadow.empty() && coin(0.7)) { auto it = shadow.begin(); std::advance(it, rint(0, (int)shadow.size() - 1)); key = *it; }
      if (code == 3 && shadow.empty()) code = 4;
      if (coin(0.03)) code = 5;
      if (code == 0) shadow.insert(key); else if (code == 1) shadow.erase(key); else if (code == 5) shadow.clear();
      ops.push_back({code, key});
    }
  }
  return ops;
}
static void put_packed(const TColStd_PackedMapOfInteger& m) {
  in(m.Extent()); ob(m.IsEmpty()); std::vector<int> v;
  for (TColStd_PackedMapOfInteger::Iterator it(m); it.More(); it.Next()) v.push_back(it.Key());
  in((long)v.size()); for (int k : v) in(k);
}
static int NLP = 28;
static void fam_loops() {
  g_guard = true;
  { LOOP("make_loops", NLP) {
    LoopSpec s = rloops(att);
    begin(N, "loops"); W_loops(s); bar();
    if (loops_unchecked(s)) { emit([&] { unchecked(); }); continue; }
    emit([&] { run_loops(s); }, true, 0.0);
  } }
  g_guard = false;
  { LOOP("packed_map", NLP) {
    int nb = pick((const int[]){1, 1, 10, 4000, 3}); std::vector<MapOp> ops = rmapops(att, false);
    begin(N, "mapops"); W_mapops(nb, ops); bar();
    emit([&] {
      TColStd_PackedMapOfInteger m(nb);
      for (const MapOp& o : ops) {
        switch (o.code) {
          case 0: ob(m.Add(o.key)); break;
          case 1: ob(m.Remove(o.key)); break;
          case 2: ob(m.Contains(o.key)); break;
          case 3: put_packed(m); break;
          default: m.Clear(); ob(m.IsEmpty()); break;
        }
      }
      put_packed(m);
    });
  } }
  { LOOP("heap_of_integer", NLP) {
    int nb = pick((const int[]){1, 1, 10, 4000, 3}); std::vector<MapOp> ops = rmapops(att, true);
    begin(N, "mapops"); W_mapops(nb, ops); bar();
    emit([&] {
      Poly_MakeLoops::HeapOfInteger hp(nb);
      for (const MapOp& o : ops) {
        switch (o.code) {
          case 0: hp.Add(o.key); break;
          case 1: hp.Remove(o.key); break;
          case 2: ob(hp.Contains(o.key)); break;
          case 3: in(hp.Top()); break;
          case 4: ob(hp.IsEmpty()); break;
          default: hp.Clear(); ob(hp.IsEmpty()); break;
        }
      }
      ob(hp.IsEmpty()); if (!hp.IsEmpty()) in(hp.Top());
    });
  } }
}

// =================================================================================
// Poly_CoherentTriangulation with its node, link and triangle classes
struct COp { int code; double x = 0, y = 0, z = 0; long a = 0, b = 0, c = 0, d = 0; };
// 0 SetNode x y z mode | 1 AddTriangle a b c | 2 RemoveTriangle a | 3 ReplaceNodes a b c d | 4 ComputeLinks | 5 ClearLinks
// 6 RemoveDegenerated x a | 7 AddLink a b | 8 RemoveLink a | 9 node UV a x y | 10 node normal a x y z | 11 node index a b | 12 SetDeflection x
static void p_cops(const std::vector<COp>& ops) {
  in((long)ops.size());
  for (const COp& o : ops) {
    in(o.code);
    switch (o.code) {
      case 0: h(o.x); h(o.y); h(o.z); in(o.a); break;
      case 1: in(o.a); in(o.b); in(o.c); break;
      case 2: case 8: in(o.a); break;
      case 3: in(o.a); in(o.b); in(o.c); in(o.d); break;
      case 6: h(o.x); in(o.a); break;
      case 7: in(o.a); in(o.b); break;
      case 9: in(o.a); h(o.x); h(o.y); break;
      case 10: in(o.a); h(o.x); h(o.y); h(o.z); break;
      case 11: in(o.a); in(o.b); break;
      case 12: h(o.x); break;
      default: break;
    }
  }
}
static void W_cops(const std::vector<COp>& ops) { int s = ntok; p_cops(ops); word("cops", s, -1); }
static int tri_slot(Poly_CoherentTriangulation& C, const Poly_CoherentTriangle* p) {
  if (!p) return -1;
  for (int i = 0; i < C.myTriangles.Length(); ++i) if (&C.myTriangles(i) == p) return i;
  std::fprintf(stderr, "triangle pointer not found in %s\n", cur_name.c_str()); std::abort();
}
static int link_slot(Poly_CoherentTriangulation& C, const Poly_CoherentLink* p) {
  if (!p) return -1;
  for (int i = 0; i < C.myLinks.Length(); ++i) if (&C.myLinks(i) == p) return i;
  std::fprintf(stderr, "link pointer not found in %s\n", cur_name.c_str()); std::abort();
}
static void put_coh(Poly_CoherentTriangulation& C) {
  in(C.MaxNode()); in(C.MaxTriangle()); in(C.NNodes()); in(C.NTriangles()); in(C.NLinks()); in((long)C.myLinks.Length()); h(C.Deflection());
  for (int i = 0; i <= C.MaxNode(); ++i) {
    const Poly_CoherentNode& n = C.Node(i);
    h(n.X()); h(n.Y()); h(n.Z()); ob(n.IsFreeNode()); in(n.GetIndex()); h(n.GetU()); h(n.GetV()); ob(n.HasNormal());
    gp_XYZ nn = n.GetNormal(); h(nn.X()); h(nn.Y()); h(nn.Z());
    if (!n.IsFreeNode()) {
      std::vector<int> ring; for (Poly_CoherentTriPtr::Iterator it = n.TriangleIterator(); it.More(); it.Next()) ring.push_back(tri_slot(C, &it.Value()));
      in((long)ring.size()); for (int r : ring) in(r);
    }
  }
  for (int i = 0; i <= C.MaxTriangle(); ++i) {
    const Poly_CoherentTriangle& t = C.Triangle(i);
    ob(t.IsEmpty()); in(t.Node(0)); in(t.Node(1)); in(t.Node(2)); in(t.NConnections());
    for (int k = 0; k < 3; ++k) in(t.GetConnectedNode(k));
    for (int k = 0; k < 3; ++k) in(tri_slot(C, t.GetConnectedTri(k)));
    for (int k = 0; k < 3; ++k) in(link_slot(C, t.GetLink(k)));
  }
  for (int i = 0; i < C.myLinks.Length(); ++i) {
    const Poly_CoherentLink& l = C.myLinks(i);
    ob(l.IsEmpty()); in(l.Node(0)); in(l.Node(1)); in(l.OppositeNode(0)); in(l.OppositeNode(1)); ob(l.GetAttribute() == nullptr);
    if (!l.IsEmpty()) {
      const Poly_CoherentTriangle* pt[2]; bool r = C.FindTriangle(l, pt); ob(r); in(tri_slot(C, pt[0])); in(tri_slot(C, pt[1]));
    }
  }
  NCollection_List<int> fr; ob(C.GetFreeNodes(fr)); in(fr.Size()); for (auto it = fr.cbegin(); it != fr.cend(); ++it) in(*it);
  put_cout_text([&] { C.Dump(std::cout); });
  occ::handle<Poly_Triangulation> T = C.GetTriangulation(); ob(!T.IsNull()); if (!T.IsNull()) put_tri_state(T);
}
static void run_cops(Poly_CoherentTriangulation& C, const std::vector<COp>& ops) {
  for (const COp& o : ops) {
    int n = C.MaxNode() + 1, nt = C.MaxTriangle() + 1;
    switch (o.code) {
      case 0: {
        int r;
        if (o.a == 1) { int fr = -1; for (int i = 0; i < n; ++i) if (C.Node(i).IsFreeNode()) { fr = i; break; } r = fr >= 0 ? C.SetNode(gp_XYZ(o.x, o.y, o.z), fr) : C.SetNode(gp_XYZ(o.x, o.y, o.z)); }
        else if (o.a == 2) r = C.SetNode(gp_XYZ(o.x, o.y, o.z), n);
        else r = C.SetNode(gp_XYZ(o.x, o.y, o.z));
        in(r);
      } break;
      case 1: if (n == 0) { in(-1); break; } in(tri_slot(C, C.AddTriangle((int)(o.a % n), (int)(o.b % n), (int)(o.c % n)))); break;
      case 2: if (nt == 0) { in(-1); break; } ob(C.RemoveTriangle(const_cast<Poly_CoherentTriangle&>(C.Triangle((int)(o.a % nt))))); break;
      case 3: {
        if (nt == 0 || n == 0) { in(-1); break; }
        int a = o.a % nt; int v[3] = {(int)o.b, (int)o.c, (int)o.d}; for (int& x : v) x = x < 0 ? -1 : x % n;
        ob(C.ReplaceNodes(const_cast<Poly_CoherentTriangle&>(C.Triangle(a)), v[0], v[1], v[2]));
      } break;
      case 4: if (C.myLinks.Length() != 0) { in(-1); break; } in(C.ComputeLinks()); break;
      case 5: C.ClearLinks(); break;
      case 6: {
        NCollection_List<Poly_CoherentTriangulation::TwoIntegers> lst;
        bool r = o.a ? C.RemoveDegenerated(o.x, &lst) : C.RemoveDegenerated(o.x);
        ob(r); in(lst.Size()); for (auto it = lst.cbegin(); it != lst.cend(); ++it) { in(it->myValue[0]); in(it->myValue[1]); }
      } break;
      case 7: { if (nt == 0) { in(-1); break; } int a = o.a % nt; in(link_slot(C, C.AddLink(C.Triangle(a), (int)(o.b % 3)))); } break;
      case 8: {
        std::vector<int> ne; for (int i = 0; i < C.myLinks.Length(); ++i) if (!C.myLinks(i).IsEmpty()) ne.push_back(i);
        if (ne.empty()) { in(-1); break; }
        int s = ne[(size_t)(o.a % (long)ne.size())]; C.RemoveLink(C.myLinks(s)); in(s);
      } break;
      case 9: if (n) C.ChangeNode((int)(o.a % n)).SetUV(o.x, o.y); break;
      case 10: if (n) C.ChangeNode((int)(o.a % n)).SetNormal(gp_XYZ(o.x, o.y, o.z)); break;
      case 11: if (n) C.ChangeNode((int)(o.a % n)).SetIndex((int)o.b); break;
      default: C.SetDeflection(o.x); break;
    }
  }
}
static std::vector<COp> rcops(int att, bool with_tri) {
  std::vector<COp> ops; auto op = [&](int c) { COp o; o.code = c; ops.push_back(o); return &ops.back(); };
  int nn = with_tri ? 0 : rint(3, 8);
  for (int i = 0; i < nn; ++i) { COp* o = op(0); o->x = crd(); o->y = crd(); o->z = crd(); o->a = coin(0.1) ? 2 : 0; }
  int nsteps = att < 10 ? 1 + att % 5 : rint(2, 9);
  for (int s = 0; s < nsteps; ++s) {
    double r = rnd(0, 1);
    if (r < 0.28) { COp* o = op(1); o->a = rint(0, 20); o->b = rint(0, 20); o->c = rint(0, 20); if (coin(0.1)) o->b = o->a; }
    else if (r < 0.36) { COp* o = op(2); o->a = rint(0, 30); }
    else if (r < 0.46) { COp* o = op(3); o->a = rint(0, 30); o->b = coin(0.1) ? -1 : rint(0, 20); o->c = rint(0, 20); o->d = rint(0, 20); }
    else if (r < 0.58) op(4);
    else if (r < 0.62) op(5);
    else if (r < 0.70) { COp* o = op(6); o->x = coin(0.5) ? 0.5 : rnd(0, 2); o->a = coin(0.5); }
    else if (r < 0.76) { COp* o = op(7); o->a = rint(0, 30); o->b = rint(0, 2); }
    else if (r < 0.82) { COp* o = op(8); o->a = rint(0, 30); }
    else if (r < 0.88) { COp* o = op(9); o->a = rint(0, 20); o->x = rnd(-1, 2); o->y = rnd(-1, 2); }
    else if (r < 0.93) { COp* o = op(10); o->a = rint(0, 20); o->x = rnd(-1, 1); o->y = rnd(-1, 1); o->z = rnd(-1, 1); if (coin(0.2)) o->x = o->y = o->z = 0; }
    else if (r < 0.97) { COp* o = op(11); o->a = rint(0, 20); o->b = rint(-3, 30); }
    else { COp* o = op(12); o->x = rnd(0, 1); }
  }
  return ops;
}
static int NC2 = 28;
static void fam_coherent() {
  g_guard = true;
  { LOOP("coherent_program", NC2) {
    bool with_tri = att % 2 == 0; TriSpec s = rtri(att, with_tri ? (att % 4 == 0 ? 1 : 0) : -1);
    if (with_tri && s.m.p.size() > 12) { att += 0; }
    std::vector<COp> ops = rcops(att, with_tri);
    begin(N, "tri cops"); W_tri(s); W_cops(ops); bar();
    emit([&] {
      occ::handle<Poly_Triangulation> T = mk_tri(s);
      occ::handle<Poly_CoherentTriangulation> C = with_tri ? new Poly_CoherentTriangulation(T) : new Poly_CoherentTriangulation();
      run_cops(*C, ops); put_coh(*C);
    });
  } }
  { LOOP("coherent_clone", NC2) {
    TriSpec s = rtri(att, att % 2); std::vector<COp> ops = att % 3 == 0 ? std::vector<COp>() : rcops(att, true);
    begin(N, "tri cops"); W_tri(s); W_cops(ops); bar();
    emit([&] {
      occ::handle<Poly_CoherentTriangulation> C = new Poly_CoherentTriangulation(mk_tri(s)); run_cops(*C, ops);
      occ::handle<Poly_CoherentTriangulation> K = C->Clone(nullptr);
      ob(!K.IsNull()); if (!K.IsNull()) put_coh(*K);
    });
  } }
  { LOOP("coherent_empty", NC2) {
    int mode = att % 4, n = att % 5; double x = crd();
    begin(N, "i i f"); W_i(mode); W_i(n); W_f(x); bar();
    emit([&] {
      occ::handle<Poly_CoherentTriangulation> C;
      if (mode == 0) C = new Poly_CoherentTriangulation();
      else if (mode == 1) C = new Poly_CoherentTriangulation(occ::handle<Poly_Triangulation>());
      else C = new Poly_CoherentTriangulation(occ::handle<NCollection_BaseAllocator>());
      for (int i = 0; i < n; ++i) C->SetNode(gp_XYZ(x + i, 0, 0));
      if (mode == 3) C->SetDeflection(x);
      put_coh(*C); occ::handle<Poly_CoherentTriangulation> K = C->Clone(nullptr); ob(K.IsNull());
    });
  } }
  g_guard = false;
  { LOOP("coherent_basic", NC2) {
    int a = rint(-2, 6), b = rint(-2, 6), c = rint(-2, 6), side = att % 3, k = att % 4; double u = rnd(-2, 2), v = rnd(-2, 2), nx = rnd(-1, 1), ny = rnd(-1, 1), nz = rnd(-1, 1);
    if (att % 5 == 0) nx = ny = nz = 0;
    if (att % 7 == 3) nx = ny = nz = 0.0004;
    begin(N, "i i i i i f f f f f"); W_i(a); W_i(b); W_i(c); W_i(side); W_i(k); W_f(u); W_f(v); W_f(nx); W_f(ny); W_f(nz); bar();
    emit([&] {
      Poly_CoherentLink l0; ob(l0.IsEmpty()); in(l0.Node(0)); in(l0.Node(1)); in(l0.OppositeNode(0)); in(l0.OppositeNode(1));
      Poly_CoherentLink l1(a, b); ob(l1.IsEmpty()); in(l1.Node(0)); in(l1.Node(1)); in(l1.Node(2)); in(l1.Node(3)); in(l1.OppositeNode(0)); in(l1.OppositeNode(1)); ob(l1.GetAttribute() == nullptr);
      int tag = 5; l1.SetAttribute(&tag); ob(l1.GetAttribute() == &tag); l1.Nullify(); ob(l1.IsEmpty()); in(l1.Node(0));
      Poly_CoherentTriangle t0; ob(t0.IsEmpty()); in(t0.NConnections()); for (int i = 0; i < 3; ++i) { in(t0.Node(i)); in(t0.GetConnectedNode(i)); ob(t0.GetConnectedTri(i) == nullptr); ob(t0.GetLink(i) == nullptr); }
      Poly_CoherentTriangle t1(a, b, c); ob(t1.IsEmpty()); in(t1.NConnections()); for (int i = 0; i < 3; ++i) { in(t1.Node(i)); in(t1.GetConnectedNode(i)); }
      Poly_CoherentLink l2(t1, side);    // the opposite nodes of a link of a triangle without neighbours
      in(l2.Node(0)); in(l2.Node(1)); in(l2.OppositeNode(0)); in(l2.OppositeNode(1));
      Poly_CoherentNode n0; ob(n0.IsFreeNode()); in(n0.GetIndex()); h(n0.GetU()); h(n0.GetV()); h(n0.X()); h(n0.Y()); h(n0.Z());
      Poly_CoherentNode n1(gp_XYZ(u, v, nx)); ob(n1.IsFreeNode()); in(n1.GetIndex()); h(n1.GetU()); h(n1.GetV()); ob(n1.HasNormal()); gp_XYZ g = n1.GetNormal(); h(g.X()); h(g.Y()); h(g.Z());
      n1.SetUV(u, v); n1.SetIndex(k); n1.SetNormal(gp_XYZ(nx, ny, nz)); h(n1.GetU()); h(n1.GetV()); in(n1.GetIndex()); ob(n1.HasNormal()); g = n1.GetNormal(); h(g.X()); h(g.Y()); h(g.Z());
    });
  } }
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
  if (want("toploc")) fam_toploc();
  if (want("tri")) fam_tri();
  if (want("geom")) fam_geom();
  if (want("reading")) fam_reading();
  if (want("merge")) fam_merge();
  if (want("loops")) fam_loops();
  if (want("coherent")) fam_coherent();
  for (const std::string& n : fam_order)
    std::fprintf(stderr, "%-32s %5d lines, %5d dropped (%d uninit)\n", n.c_str(), fam_lines[n], fam_dropped.count(n) ? fam_dropped[n] : 0,
                 fam_dropped.count(n + "!uninit") ? fam_dropped[n + "!uninit"] : 0);
  for (auto& kv : fam_dropped) if (kv.first.find("!libm") != std::string::npos) std::fprintf(stderr, "   libm-perturbed: %s %d dropped\n", kv.first.c_str(), kv.second);
  return 0;
}
