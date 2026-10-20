// Deterministic reference calls for OCCT 8.0.1's bounding volumes and surface-normal helpers:
//   package Bnd:    Bnd_Box, Bnd_Box2d, Bnd_Range, Bnd_Sphere, Bnd_OBB, Bnd_BoundSortBox,
//   package CSLib:  CSLib::Normal (4 overloads), CSLib::DNNUV (2), CSLib::DNNormal, CSLib_Class2d, CSLib_NormalPolyDef.
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints (and bools) are decimal. Every input of a call is printed on the
// left of " | " in the order named by the family's schema string (`begin(name, schema)`); tests/bnd.rs reads them
// back with the reader method of the same name as each schema word. The oracle aborts if the words it printed differ
// from the schema (and if a fixed-size word printed the wrong number of tokens). Outputs are values, `throws` (OCCT
// raised an exception that the line's call really raises: an explicit `throw`, or a check that is compiled into the
// code the call runs through, like the range checks of NCollection_Array1/2 and HArray1) or `unchecked` (OCCT 8.0.1
// as built has the check compiled out, so what it does is unspecified; the Rust API must return an error).
//
// Schema words (token counts):
//   f=1 i=1 xyz=3 pnt=3 vec=3 (floats) dir=3 (RAW direction; gp_Dir normalizes it) xy=2 pnt2d=2 dir2d=2 (raw)
//   bx=9    a Bnd_Box recipe:  kind xmin ymin zmin xmax ymax zmax gap flags
//             kind 0  Bnd_Box()                         (void; the six numbers are printed but unused)
//             kind 1  Bnd_Box(gp_Pnt(xmin,ymin,zmin), gp_Pnt(xmax,ymax,zmax))   (may be inverted)
//             kind 2  Bnd_Box() then Update(xmin,ymin,zmin,xmax,ymax,zmax)
//             kind 3  Bnd_Box() then SetWhole()         (the six numbers are unused)
//             kind 4  Bnd_Box() then Update(xmin,ymin,zmin)                       (a point; the last three unused)
//           then SetGap(gap), then OpenXmin OpenXmax OpenYmin OpenYmax OpenZmin OpenZmax for the set bits 0..5 of flags
//           (in this order).
//   bx2=7   a Bnd_Box2d recipe: kind xmin ymin xmax ymax gap flags
//             kind 0  Bnd_Box2d()   kind 1  Update(xmin,ymin,xmax,ymax)   kind 2  SetWhole()
//             kind 3  Update(xmin,ymin)   kind 4  Set(gp_Pnt2d(xmin,ymin)) (resets Flags and Gap first)
//           then SetGap(gap), then OpenXmin OpenXmax OpenYmin OpenYmax for the set bits 0..3 of flags.
//   trsf = variable: a gp_Trsf recipe, kind then the fields the kind uses
//             2 SetScale(pnt, f)   3 SetTranslation(vec)   4 SetMirror(gp_Ax1(pnt, dir))   5 SetMirror(pnt)
//             6 SetMirror(gp_Ax2(pnt, dir))   7 SetRotation(quat x y z w)  8 SetRotation(quat) + SetTranslationPart(vec)
//             + SetScaleFactor(f)   9 gp_Trsf() (identity).     (No kind uses sin/cos.)
//   trsf2d = variable: a gp_Trsf2d recipe (the numbers are those of the gp2d oracle, without the trigonometric ones)
//             2 SetScale(pnt2d, f)   3 SetTranslation(gp_Vec2d xy)   4 SetTranslation(pnt2d, pnt2d)   5 SetMirror(pnt2d)
//             6 SetMirror(gp_Ax2d(pnt2d, dir2d))   7 SetTransformation(gp_Ax2d)   8 SetTransformation(gp_Ax2d from, gp_Ax2d to)
//             9 SetMirror(gp_Ax2d) + SetScaleFactor(f) + SetTranslationPart(xy)   10 SetValues(six floats a11 a12 a13 a21 a22 a23)
//             12 gp_Trsf2d() (identity).
//   rg=3    a Bnd_Range recipe: kind a b   (0: Bnd_Range()   1: Bnd_Range(a, b)   2: Bnd_Range() Add(a) Add(b)   3: Add(a))
//   sph=7   a Bnd_Sphere: cx cy cz radius u v valid  (Bnd_Sphere(gp_XYZ, r, u, v) then SetValid(valid))
//   pts, tols = variable: `pts` is  lo n x1 y1 z1 .. xn yn zn  (an NCollection_Array1<gp_Pnt> with lower index lo);
//           `tols` is  flag [t1 .. tn]  (flag 0: no tolerance array; 1: an NCollection_Array1<double> with the same
//           bounds and n values follows)
//   obb = variable: kind payload aabox  (0: Bnd_OBB()   1: Bnd_OBB(pnt center, dir dir dir (raw), f hx, f hy, f hz)
//           2: Bnd_OBB(Bnd_Box bx)   3: Bnd_OBB() ReBuild(pts tols optimal) with `pts tols i`);  aabox is -1 (nothing) or
//           0/1 for SetAABox(aabox) afterwards.
//   bsb = variable, see put_bsb. a2 = variable (an NCollection_Array2<gp_Vec>): rlo clo nr nc then nr*nc vec, row-major.
//   c2d = variable (a CSLib_Class2d): kind then, for kind > 0, p2s tolu tolv umin vmin umax vmax; see put_c2d.
//   p2s = variable: lo n x1 y1 .. xn yn.
//
// THE GENERATED BOX SETS of the Bnd_BoundSortBox families (`bgen`, 6 tokens: seed n lo hi size mode) are described in
// docs/api-bnd.md ("Box generator"): they use only + * / on xorshift64 bits, identically on both sides.
//
// Cases for which the C++ result depends on uninitialised memory are detected by running every call four times with
// differently, randomly filled stack and heap and are NOT printed; so are cases in which a result is not finite. The
// cases of the families that call the C library's sin, cos, pow (`cslib_normal_order`, `cslib_normal_poly_def`) are only
// printed when moving every result of those functions by an ulp, in 24 pseudo-random patterns, leaves the outputs
// within 2 ulps (the test allows 8): see `emit` and the interposed functions below.
#include <Bnd_Box.hxx>
#include <Bnd_Box2d.hxx>
#include <Bnd_BoundSortBox.hxx>
#include <Bnd_OBB.hxx>
#include <Bnd_Range.hxx>
#include <Bnd_Sphere.hxx>
#include <CSLib.hxx>
#include <CSLib_DerivativeStatus.hxx>
#include <CSLib_NormalPolyDef.hxx>
#include <CSLib_NormalStatus.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <NCollection_DynamicArray.hxx>
#include <NCollection_HArray1.hxx>
#include <NCollection_List.hxx>
#include <NCollection_Sequence.hxx>
#include <Precision.hxx>
#include <Standard_Failure.hxx>
#include <gp.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Lin.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Quaternion.hxx>
#include <gp_Trsf.hxx>
#include <gp_Trsf2d.hxx>
#include <gp_Vec.hxx>
#include <gp_Vec2d.hxx>
#include <gp_XY.hxx>
#include <gp_XYZ.hxx>
#include <malloc.h>
#include <unistd.h>
#include <algorithm>
#include <cfloat>
#include <climits>
#include <cmath>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <iostream>
#include <map>
#include <memory>
#include <set>
#include <sstream>
#include <string>
#include <vector>
// CSLib_Class2d::internalSiDans / internalSiDansOuOn are private; the test exercises them as `internal_si_dans` /
// `internal_si_dans_ou_on` of the Rust API, so they are made reachable here (all its includes are already in).
#define private public
#include <CSLib_Class2d.hxx>
#undef private
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
// text of a Dump(): newlines as \n, spaces as _ (one token)
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
  if (std::getenv("BND_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
    a[i] = r ^ (r >> 31);
  }
  asm volatile("" ::: "memory");
}
static const int POISON_RUNS = 4;

static std::string run_once(const std::function<void()>& body, int poison, bool& discard) {
  std::string left = buf.substr(0, mark);
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
// `libm_robust` the line is only kept when moving every sin / cos / ... result of the C library by an ulp (24
// different pseudo-random patterns) leaves the outputs within 2 ulps (the test allows 8)
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
    for (uint64_t salt = 1; salt <= 24; ++salt) {
      g_perturb = true; g_salt = salt;
      bool dp = false;
      std::string rp = run_once(body, 0, dp);
      g_perturb = false;
      ntok = (int)keep_ntok;
      if (!dp && rp != r1) fam_dropped[cur_name + "~changed"]++;
      bool ok = !dp && lines_close(r1, rp, 2.0);
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
static void W_trsf(const TR& r) {
  int s = ntok; in(r.kind);
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
  word("trsf", s, -1);
}
static const int TK3[] = {9, 3, 2, 3, 7, 8, 4, 5, 6, 3, 9, 3};
static TR rtr(int n) { return mkTR(TK3[n % 12]); }

struct T2 { int kind; gp_Pnt2d p, q; Raw2 d, e; double sc; gp_XY v; double vals[6]; gp_Trsf2d t; };
static T2 mkT2(int kind) {
  T2 r; r.kind = kind; r.p = rpnt2d(); r.q = rpnt2d(); r.d = mraw2(); r.e = mraw2(); r.sc = coin(0.3) ? (coin(0.5) ? 2.0 : 0.5) : rnd(0.3, 2.5); r.v = gp_XY(crd(), crd());
  for (;;) {
    for (int k = 0; k < 6; ++k) r.vals[k] = rnd(-3, 3);
    double det = r.vals[0] * r.vals[4] - r.vals[3] * r.vals[1];
    if (std::abs(det) > 0.3) break;
  }
  switch (kind) {
    case 2: r.t.SetScale(r.p, r.sc); break;
    case 3: r.t.SetTranslation(gp_Vec2d(r.v)); break;
    case 4: r.t.SetTranslation(r.p, r.q); break;
    case 5: r.t.SetMirror(r.p); break;
    case 6: r.t.SetMirror(gp_Ax2d(r.p, D2(r.d))); break;
    case 7: r.t.SetTransformation(gp_Ax2d(r.p, D2(r.d))); break;
    case 8: r.t.SetTransformation(gp_Ax2d(r.p, D2(r.d)), gp_Ax2d(r.q, D2(r.e))); break;
    case 9: r.t.SetMirror(gp_Ax2d(r.p, D2(r.d))); r.t.SetScaleFactor(r.sc); r.t.SetTranslationPart(gp_Vec2d(r.v)); break;
    case 10: r.t.SetValues(r.vals[0], r.vals[1], r.vals[2], r.vals[3], r.vals[4], r.vals[5]); break;
    default: break;
  }
  return r;
}
static void W_trsf2d(const T2& r) {
  int s = ntok; in(r.kind);
  switch (r.kind) {
    case 2: pnt2d(r.p); h(r.sc); break;
    case 3: xy(r.v); break;
    case 4: pnt2d(r.p); pnt2d(r.q); break;
    case 5: pnt2d(r.p); break;
    case 6: pnt2d(r.p); rw2(r.d); break;
    case 7: pnt2d(r.p); rw2(r.d); break;
    case 8: pnt2d(r.p); rw2(r.d); pnt2d(r.q); rw2(r.e); break;
    case 9: pnt2d(r.p); rw2(r.d); h(r.sc); xy(r.v); break;
    case 10: for (int k = 0; k < 6; ++k) h(r.vals[k]); break;
    default: break;
  }
  word("trsf2d", s, -1);
}
static const int TK2[] = {12, 3, 2, 3, 6, 5, 4, 7, 8, 9, 10, 3, 12, 3};
static T2 rt2(int n) { return mkT2(TK2[n % 14]); }

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
static Bx pairbx(int att, int slot) {
  // edge pairs first: every edge box against a second, rotating one
  if (att < NEDGE_BX * 2) return edge_bx(slot == 0 ? att % NEDGE_BX : (att * 7 + 3 + att / NEDGE_BX) % NEDGE_BX);
  if (att < NEDGE_BX * 2 + 10) return slot == 0 ? rbx(0) : edge_bx(att);
  return rbx(0);
}
// coordinate of a point near a box: on a boundary (with and without the gap), just off it, inside, or free
static double pcoord(const Bx& b, int a) {
  double lo = b.c[a], hi = b.c[3 + a], g = std::abs(b.gap);
  switch (rint(0, 9)) {
    case 0: return lo;
    case 1: return hi;
    case 2: return lo - g;
    case 3: return hi + g;
    case 4: return lo - g - 1e-9;
    case 5: return hi + g + 1e-9;
    case 6: return lo + 0.5 * (hi - lo);
    case 7: return nice();
    default: return rnd(std::min(lo, hi) - 1.5, std::max(lo, hi) + 1.5);
  }
}
static gp_Pnt pnear(const Bx& b) { return gp_Pnt(pcoord(b, 0), pcoord(b, 1), pcoord(b, 2)); }
static Raw rawto(const gp_Pnt& a, const gp_Pnt& b) {
  Raw r{b.X() - a.X(), b.Y() - a.Y(), b.Z() - a.Z()};
  if (r.x == 0 && r.y == 0 && r.z == 0) return nraw();
  return r;
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
template <class F> static void put_dump(F f) {
  std::ostringstream os;
  std::streambuf* old = std::cout.rdbuf(os.rdbuf());
  try { f(); } catch (...) { std::cout.rdbuf(old); throw; }
  std::cout.rdbuf(old);
  os_text(os.str());
}
static int NB = 34; // lines per family of the box families (set in main)

#define LOOP(NAME, TARGET) const char* N = NAME; for (int att = 0; att < 20000 && fam_lines[N] < (TARGET); ++att)

static void fam_box_basic() {
  { LOOP("box_state", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    begin(N, "bx"); W_bx(s); bar();
    emit([&] { Bnd_Box b = mkB(s); put_box(b); auto c = b.Center(); ob(c.has_value()); if (c) pnt(*c); });
  } }
  { LOOP("box_get", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    begin(N, "bx"); W_bx(s); bar();
    emit([&] { Bnd_Box b = mkB(s); double a[6]; b.Get(a[0], a[1], a[2], a[3], a[4], a[5]); for (double v : a) h(v); });
  } }
  { LOOP("box_corners", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    begin(N, "bx"); W_bx(s); bar();
    emit([&] { Bnd_Box b = mkB(s); pnt(b.CornerMin()); pnt(b.CornerMax()); });
  } }
  { LOOP("box_thin", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    double tol = att % 5 == 0 ? 0.0 : (att % 5 == 1 ? 1e-9 : (att % 5 == 2 ? 1.0 : rnd(0, 3)));
    if (att % 7 == 0) tol = std::abs(s.c[3] - s.c[0]);
    begin(N, "bx f"); W_bx(s); W_f(tol); bar();
    emit([&] { Bnd_Box b = mkB(s); ob(b.IsXThin(tol)); ob(b.IsYThin(tol)); ob(b.IsZThin(tol)); ob(b.IsThin(tol)); });
  } }
  { LOOP("box_gap", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    double a = att % 4 == 0 ? 0.0 : (coin(0.5) ? 0.25 * rint(-8, 8) : rnd(-2, 2)), c = coin(0.5) ? 0.25 * rint(-8, 8) : rnd(-2, 2);
    begin(N, "bx f f"); W_bx(s); W_f(a); W_f(c); bar();
    emit([&] { Bnd_Box b = mkB(s); b.SetGap(a); put_box(b); b.Enlarge(c); put_box(b); });
  } }
  { LOOP("box_modify", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    int m = att < 64 ? (att * 5 + 1) % 256 : (coin(0.15) ? (64 + rint(0, 1) * 64 + rint(0, 63)) : (1 << rint(0, 5)) | (coin(0.5) ? 1 << rint(0, 5) : 0));
    begin(N, "bx i"); W_bx(s); W_i(m); bar();
    emit([&] {
      Bnd_Box b = mkB(s);
      if (m & 1) b.OpenXmin();
      if (m & 2) b.OpenXmax();
      if (m & 4) b.OpenYmin();
      if (m & 8) b.OpenYmax();
      if (m & 16) b.OpenZmin();
      if (m & 32) b.OpenZmax();
      if (m & 64) b.SetWhole();
      if (m & 128) b.SetVoid();
      put_box(b);
    });
  } }
  { LOOP("box_update6", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    double lo[3], hi[3];
    for (int k = 0; k < 3; ++k) { lo[k] = crd(); hi[k] = lo[k] + (coin(0.15) ? -rnd(0, 2) : rnd(0, 3)); if (att % 6 == 0) { lo[k] = s.c[k]; hi[k] = s.c[3 + k]; } }
    begin(N, "bx f f f f f f"); W_bx(s); W_f(lo[0]); W_f(lo[1]); W_f(lo[2]); W_f(hi[0]); W_f(hi[1]); W_f(hi[2]); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Update(lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]); put_box(b); });
  } }
  { LOOP("box_update3", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s);
    begin(N, "bx f f f"); W_bx(s); W_f(p.X()); W_f(p.Y()); W_f(p.Z()); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Update(p.X(), p.Y(), p.Z()); put_box(b); });
  } }
  { LOOP("box_set_pnt", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s);
    begin(N, "bx pnt"); W_bx(s); W_pnt(p); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Set(p); put_box(b); });
  } }
  { LOOP("box_set_pnt_dir", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s); Raw d = mraw();
    begin(N, "bx pnt dir"); W_bx(s); W_pnt(p); W_dir(d); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Set(p, D(d)); put_box(b); });
  } }
  { LOOP("box_add_pnt", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s);
    begin(N, "bx pnt"); W_bx(s); W_pnt(p); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Add(p); put_box(b); });
  } }
  { LOOP("box_add_dir", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    Raw d = att % 9 == 0 ? Raw{1e-17, 1.0, -1e-17} : (att % 9 == 1 ? Raw{3e-16, 0.0, -3e-16} : mraw());
    begin(N, "bx dir"); W_bx(s); W_dir(d); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Add(D(d)); put_box(b); });
  } }
  { LOOP("box_add_pnt_dir", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s); Raw d = mraw();
    begin(N, "bx pnt dir"); W_bx(s); W_pnt(p); W_dir(d); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Add(p, D(d)); put_box(b); });
  } }
  { LOOP("box_add_box", NB + 14) {
    Bx s = pairbx(att, 0), o = pairbx(att, 1);
    begin(N, "bx bx"); W_bx(s); W_bx(o); bar();
    emit([&] { Bnd_Box b = mkB(s); b.Add(mkB(o)); put_box(b); });
  } }
}

static void fam_box_queries() {
  { LOOP("box_is_out_pnt", NB + 14) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s);
    begin(N, "bx pnt"); W_bx(s); W_pnt(p); bar();
    emit([&] { Bnd_Box b = mkB(s); ob(b.IsOut(p)); ob(b.Contains(p)); });
  } }
  { LOOP("box_is_out_lin", NB + 24) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(coin(0.8) ? 0 : 1);
    gp_Pnt p = pnear(s); Raw d = coin(0.5) ? mraw() : rawto(p, pnear(s));
    begin(N, "bx pnt dir"); W_bx(s); W_pnt(p); W_dir(d); bar();
    emit([&] { Bnd_Box b = mkB(s); ob(b.IsOut(gp_Lin(p, D(d)))); });
  } }
  { LOOP("box_is_out_pln", NB + 14) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    gp_Pnt p = pnear(s); Raw d = mraw();
    begin(N, "bx pnt dir"); W_bx(s); W_pnt(p); W_dir(d); bar();
    emit([&] { Bnd_Box b = mkB(s); ob(b.IsOut(gp_Pln(p, D(d)))); });
  } }
  { LOOP("box_is_out_box", NB + 24) {
    Bx s = pairbx(att, 0), o = pairbx(att, 1);
    begin(N, "bx bx"); W_bx(s); W_bx(o); bar();
    emit([&] { Bnd_Box b = mkB(s), c = mkB(o); ob(b.IsOut(c)); ob(b.Intersects(c)); });
  } }
  { LOOP("box_is_out_box_trsf", NB) {
    Bx s = pairbx(att, 0), o = pairbx(att, 1); TR t = rtr(att);
    begin(N, "bx bx trsf"); W_bx(s); W_bx(o); W_trsf(t); bar();
    emit([&] { Bnd_Box b = mkB(s), c = mkB(o); ob(b.IsOut(c, t.t)); });
  } }
  { LOOP("box_is_out_trsf_box_trsf", NB) {
    Bx s = pairbx(att, 0), o = pairbx(att, 1); TR t1 = rtr(att), t2 = rtr(att / 3 + 5);
    begin(N, "bx trsf bx trsf"); W_bx(s); W_trsf(t1); W_bx(o); W_trsf(t2); bar();
    emit([&] { Bnd_Box b = mkB(s), c = mkB(o); ob(b.IsOut(t1.t, c, t2.t)); });
  } }
  { LOOP("box_is_out_segment", NB + 24) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(coin(0.8) ? 0 : 1);
    gp_Pnt p1 = pnear(s), p2 = pnear(s); Raw d = coin(0.5) ? rawto(p1, p2) : mraw();
    if (att % 3 == 0) { Raw n = nraw(); d = n; p2 = gp_Pnt(p1.X() + n.x * rnd(0, 3), p1.Y() + n.y * rnd(0, 3), p1.Z() + n.z * rnd(0, 3)); }
    begin(N, "bx pnt pnt dir"); W_bx(s); W_pnt(p1); W_pnt(p2); W_dir(d); bar();
    emit([&] { Bnd_Box b = mkB(s); ob(b.IsOut(p1, p2, D(d))); });
  } }
  { LOOP("box_distance", NB) {
    Bx s = pairbx(att, 0), o = pairbx(att, 1);
    begin(N, "bx bx"); W_bx(s); W_bx(o); bar();
    emit([&] { Bnd_Box b = mkB(s), c = mkB(o); h(b.Distance(c)); });
  } }
  { LOOP("box_transformed", NB + 14) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0); TR t = rtr(att);
    begin(N, "bx trsf"); W_bx(s); W_trsf(t); bar();
    emit([&] { put_box(mkB(s).Transformed(t.t)); });
  } }
  { LOOP("box_finite_part", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    begin(N, "bx"); W_bx(s); bar();
    emit([&] { put_box(mkB(s).FinitePart()); });
  } }
  { LOOP("box_dump", NB) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    begin(N, "bx"); W_bx(s); bar();
    emit([&] { Bnd_Box b = mkB(s); put_dump([&] { b.Dump(); }); });
  } }
}

// =================================================================================
// Bnd_Box2d
struct Bx2 { int kind; double c[4]; double gap; int flags; };
static Bx2 BX2(int kind, double a, double b, double c, double d, double gap = 0, int flags = 0) {
  Bx2 s; s.kind = kind; s.c[0] = a; s.c[1] = b; s.c[2] = c; s.c[3] = d; s.gap = gap; s.flags = flags; return s;
}
static Bnd_Box2d mkB2(const Bx2& s) {
  Bnd_Box2d b;
  switch (s.kind) {
    case 1: b.Update(s.c[0], s.c[1], s.c[2], s.c[3]); break;
    case 2: b.SetWhole(); break;
    case 3: b.Update(s.c[0], s.c[1]); break;
    case 4: b.Set(gp_Pnt2d(s.c[0], s.c[1])); break;
    default: break;
  }
  b.SetGap(s.gap);
  if (s.flags & 1) b.OpenXmin();
  if (s.flags & 2) b.OpenXmax();
  if (s.flags & 4) b.OpenYmin();
  if (s.flags & 8) b.OpenYmax();
  return b;
}
static void W_bx2(const Bx2& s) {
  int st = ntok; in(s.kind); for (int k = 0; k < 4; ++k) h(s.c[k]); h(s.gap); in(s.flags); word("bx2", st, 7);
}
static const int NEDGE_B2 = 22;
static Bx2 edge_b2(int i) {
  switch (((i % NEDGE_B2) + NEDGE_B2) % NEDGE_B2) {
    case 0: return BX2(0, 0, 0, 0, 0);
    case 1: return BX2(2, 0, 0, 0, 0);
    case 2: return BX2(0, 0, 0, 0, 0, 0.5);
    case 3: return BX2(0, 0, 0, 0, 0, 0, 15);
    case 4: return BX2(0, 0, 0, 0, 0, 0, 1);
    case 5: return BX2(1, 0, 0, 1, 1);
    case 6: return BX2(1, 0, 0, 1, 1, 0.25);
    case 7: return BX2(1, 0, 2, 1, 2);
    case 8: return BX2(3, 1, 2, 0, 0);
    case 9: return BX2(1, 1, 1, 0, 0);
    case 10: return BX2(1, 0, 0, 1, 1, 0, 1);
    case 11: return BX2(1, 0, 0, 1, 1, 0, 2);
    case 12: return BX2(1, 0, 0, 1, 1, 0, 4);
    case 13: return BX2(1, 0, 0, 1, 1, 0, 8);
    case 14: return BX2(1, -1, -1, 2, 2, 0, 14);
    case 15: return BX2(1, 0, 0, 1, 1, 0, 15);
    case 16: return BX2(1, 0, 0, 1, 1, -0.5);
    case 17: return BX2(1, -1e10, -1e10, 1e10, 1e10);
    case 18: return BX2(4, 1, 2, 0, 0, 0.5, 3);
    case 19: return BX2(1, 0, 0, 2, 1, 0, 3);
    case 20: return BX2(1, 2, 0, 3, 1);
    default: return BX2(1, 1, 0, 2, 1);
  }
}
static Bx2 rbx2(int style = 0) {
  double r = rnd(0, 1);
  Bx2 s;
  if (style == 0 && r < 0.05) s = BX2(0, 0, 0, 0, 0);
  else if (style == 0 && r < 0.09) s = BX2(2, 0, 0, 0, 0);
  else {
    double lo[2], sz[2];
    for (int k = 0; k < 2; ++k) { lo[k] = crd(); sz[k] = coin(0.1) ? 0.0 : (coin(0.5) ? 0.5 * rint(1, 8) : rnd(0, 6)); }
    int kind = coin(0.12) ? (coin(0.5) ? 3 : 4) : 1;
    if (kind == 1 && coin(0.1)) { sz[0] = -sz[0]; sz[1] = -sz[1]; }
    s = BX2(kind, lo[0], lo[1], lo[0] + sz[0], lo[1] + sz[1]);
  }
  double g = rnd(0, 1);
  s.gap = g < 0.6 ? 0.0 : (g < 0.8 ? 0.25 * rint(-4, 4) : rnd(-1, 1));
  if (style == 0 && s.kind != 2 && coin(0.35)) { int nbits = rint(1, 2); for (int k = 0; k < nbits; ++k) s.flags |= 1 << rint(0, 3); if (coin(0.06)) s.flags = 15; }
  return s;
}
static Bx2 pairb2(int att, int slot) {
  if (att < NEDGE_B2 * 2) return edge_b2(slot == 0 ? att % NEDGE_B2 : (att * 7 + 3 + att / NEDGE_B2) % NEDGE_B2);
  if (att < NEDGE_B2 * 2 + 10) return slot == 0 ? rbx2(0) : edge_b2(att);
  return rbx2(0);
}
static double pcoord2(const Bx2& b, int a) {
  double lo = b.c[a], hi = b.c[2 + a], g = std::abs(b.gap);
  switch (rint(0, 9)) {
    case 0: return lo;
    case 1: return hi;
    case 2: return lo - g;
    case 3: return hi + g;
    case 4: return lo - g - 1e-9;
    case 5: return hi + g + 1e-9;
    case 6: return lo + 0.5 * (hi - lo);
    case 7: return nice();
    default: return rnd(std::min(lo, hi) - 1.5, std::max(lo, hi) + 1.5);
  }
}
static gp_Pnt2d pnear2(const Bx2& b) { return gp_Pnt2d(pcoord2(b, 0), pcoord2(b, 1)); }
static void put_box2(const Bnd_Box2d& b) {
  ob(b.IsVoid()); ob(b.IsWhole());
  ob(b.IsOpenXmin()); ob(b.IsOpenXmax()); ob(b.IsOpenYmin()); ob(b.IsOpenYmax());
  h(b.GetGap());
  Bnd_Box2d::Limits l = b.Get();
  h(l.Xmin); h(l.Xmax); h(l.Ymin); h(l.Ymax);
  hi(b.SquareExtent());
}

static void fam_box2d() {
  { LOOP("box2d_state", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    begin(N, "bx2"); W_bx2(s); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); put_box2(b); auto c = b.Center(); ob(c.has_value()); if (c) pnt2d(*c); });
  } }
  { LOOP("box2d_get", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    begin(N, "bx2"); W_bx2(s); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); double a[4]; b.Get(a[0], a[1], a[2], a[3]); for (double v : a) h(v); });
  } }
  { LOOP("box2d_gap", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    double a = att % 4 == 0 ? 0.0 : (coin(0.5) ? 0.25 * rint(-8, 8) : rnd(-2, 2)), c = coin(0.5) ? 0.25 * rint(-8, 8) : rnd(-2, 2);
    begin(N, "bx2 f f"); W_bx2(s); W_f(a); W_f(c); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.SetGap(a); put_box2(b); b.Enlarge(c); put_box2(b); });
  } }
  { LOOP("box2d_modify", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    int m = att < 40 ? (att * 5 + 1) % 64 : (1 << rint(0, 3)) | (coin(0.3) ? 16 << rint(0, 1) : 0) | (coin(0.3) ? 1 << rint(0, 3) : 0);
    begin(N, "bx2 i"); W_bx2(s); W_i(m); bar();
    emit([&] {
      Bnd_Box2d b = mkB2(s);
      if (m & 1) b.OpenXmin();
      if (m & 2) b.OpenXmax();
      if (m & 4) b.OpenYmin();
      if (m & 8) b.OpenYmax();
      if (m & 16) b.SetWhole();
      if (m & 32) b.SetVoid();
      put_box2(b);
    });
  } }
  { LOOP("box2d_update4", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    double lo[2], hi[2];
    for (int k = 0; k < 2; ++k) { lo[k] = crd(); hi[k] = lo[k] + (coin(0.15) ? -rnd(0, 2) : rnd(0, 3)); if (att % 6 == 0) { lo[k] = s.c[k]; hi[k] = s.c[2 + k]; } }
    begin(N, "bx2 f f f f"); W_bx2(s); W_f(lo[0]); W_f(lo[1]); W_f(hi[0]); W_f(hi[1]); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Update(lo[0], lo[1], hi[0], hi[1]); put_box2(b); });
  } }
  { LOOP("box2d_update2", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    gp_Pnt2d p = pnear2(s);
    begin(N, "bx2 f f"); W_bx2(s); W_f(p.X()); W_f(p.Y()); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Update(p.X(), p.Y()); put_box2(b); });
  } }
  { LOOP("box2d_set_pnt", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    gp_Pnt2d p = pnear2(s);
    begin(N, "bx2 pnt2d"); W_bx2(s); W_pnt2d(p); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Set(p); put_box2(b); });
  } }
  { LOOP("box2d_set_pnt_dir", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    gp_Pnt2d p = pnear2(s); Raw2 d = mraw2();
    begin(N, "bx2 pnt2d dir2d"); W_bx2(s); W_pnt2d(p); W_dir2d(d); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Set(p, D2(d)); put_box2(b); });
  } }
  { LOOP("box2d_add_pnt", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    gp_Pnt2d p = pnear2(s);
    begin(N, "bx2 pnt2d"); W_bx2(s); W_pnt2d(p); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Add(p); put_box2(b); });
  } }
  { LOOP("box2d_add_dir", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    Raw2 d = att % 9 == 0 ? Raw2{1e-17, 1.0} : (att % 9 == 1 ? Raw2{3e-16, -3e-16} : mraw2());
    begin(N, "bx2 dir2d"); W_bx2(s); W_dir2d(d); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Add(D2(d)); put_box2(b); });
  } }
  { LOOP("box2d_add_pnt_dir", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    gp_Pnt2d p = pnear2(s); Raw2 d = mraw2();
    begin(N, "bx2 pnt2d dir2d"); W_bx2(s); W_pnt2d(p); W_dir2d(d); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Add(p, D2(d)); put_box2(b); });
  } }
  { LOOP("box2d_add_box", NB + 14) {
    Bx2 s = pairb2(att, 0), o = pairb2(att, 1);
    begin(N, "bx2 bx2"); W_bx2(s); W_bx2(o); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); b.Add(mkB2(o)); put_box2(b); });
  } }
  { LOOP("box2d_is_out_pnt", NB + 14) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    gp_Pnt2d p = pnear2(s);
    begin(N, "bx2 pnt2d"); W_bx2(s); W_pnt2d(p); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); ob(b.IsOut(p)); ob(b.Contains(p)); });
  } }
  { LOOP("box2d_is_out_lin", NB + 24) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(coin(0.8) ? 0 : 1);
    gp_Pnt2d p = pnear2(s); Raw2 d = mraw2();
    begin(N, "bx2 pnt2d dir2d"); W_bx2(s); W_pnt2d(p); W_dir2d(d); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); ob(b.IsOut(gp_Lin2d(p, D2(d)))); });
  } }
  { LOOP("box2d_is_out_segment", NB + 24) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(coin(0.8) ? 0 : 1);
    gp_Pnt2d p1 = pnear2(s), p2 = pnear2(s);
    if (att % 5 == 0) p2 = gp_Pnt2d(p1.X(), p1.Y() + rnd(0, 3)); else if (att % 5 == 1) p2 = gp_Pnt2d(p1.X() + rnd(-3, 3), p1.Y());
    begin(N, "bx2 pnt2d pnt2d"); W_bx2(s); W_pnt2d(p1); W_pnt2d(p2); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); ob(b.IsOut(p1, p2)); });
  } }
  { LOOP("box2d_is_out_box", NB + 24) {
    Bx2 s = pairb2(att, 0), o = pairb2(att, 1);
    begin(N, "bx2 bx2"); W_bx2(s); W_bx2(o); bar();
    emit([&] { Bnd_Box2d b = mkB2(s), c = mkB2(o); ob(b.IsOut(c)); ob(b.Intersects(c)); });
  } }
  { LOOP("box2d_is_out_box_trsf", NB) {
    Bx2 s = pairb2(att, 0), o = pairb2(att, 1); T2 t = rt2(att);
    begin(N, "bx2 bx2 trsf2d"); W_bx2(s); W_bx2(o); W_trsf2d(t); bar();
    emit([&] { Bnd_Box2d b = mkB2(s), c = mkB2(o); ob(b.IsOut(c, t.t)); });
  } }
  { LOOP("box2d_is_out_trsf_box_trsf", NB) {
    Bx2 s = pairb2(att, 0), o = pairb2(att, 1); T2 t1 = rt2(att), t2 = rt2(att / 3 + 5);
    begin(N, "bx2 trsf2d bx2 trsf2d"); W_bx2(s); W_trsf2d(t1); W_bx2(o); W_trsf2d(t2); bar();
    emit([&] { Bnd_Box2d b = mkB2(s), c = mkB2(o); ob(b.IsOut(t1.t, c, t2.t)); });
  } }
  { LOOP("box2d_distance", NB) {
    Bx2 s = pairb2(att, 0), o = pairb2(att, 1);
    begin(N, "bx2 bx2"); W_bx2(s); W_bx2(o); bar();
    emit([&] { Bnd_Box2d b = mkB2(s), c = mkB2(o); h(b.Distance(c)); });
  } }
  { LOOP("box2d_transformed", NB + 14) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0); T2 t = rt2(att);
    begin(N, "bx2 trsf2d"); W_bx2(s); W_trsf2d(t); bar();
    emit([&] { put_box2(mkB2(s).Transformed(t.t)); });
  } }
  { LOOP("box2d_dump", NB) {
    Bx2 s = att < NEDGE_B2 ? edge_b2(att) : rbx2(0);
    begin(N, "bx2"); W_bx2(s); bar();
    emit([&] { Bnd_Box2d b = mkB2(s); put_dump([&] { b.Dump(); }); });
  } }
}

// =================================================================================
// Bnd_Range
struct Rg { int kind; double a, b; };
static Bnd_Range mkR(const Rg& s) {
  switch (s.kind) {
    case 1: return Bnd_Range(s.a, s.b);
    case 2: { Bnd_Range r; r.Add(s.a); r.Add(s.b); return r; }
    case 3: { Bnd_Range r; r.Add(s.a); return r; }
    default: return Bnd_Range();
  }
}
static void W_rg(const Rg& s) { int st = ntok; in(s.kind); h(s.a); h(s.b); word("rg", st, 3); }
static const int NEDGE_RG = 14;
static Rg edge_rg(int i) {
  switch (((i % NEDGE_RG) + NEDGE_RG) % NEDGE_RG) {
    case 0: return Rg{0, 0, 0};
    case 1: return Rg{1, 0, 1};
    case 2: return Rg{1, 1, 1};
    case 3: return Rg{1, 2, 1};      // Bnd_Range(2, 1) throws
    case 4: return Rg{2, 3, -1};
    case 5: return Rg{3, 2, 0};
    case 6: return Rg{1, -1, 1};
    case 7: return Rg{1, 0, 6.283185307179586};
    case 8: return Rg{1, -0.0, 0.0};
    case 9: return Rg{1, 1e10, 2e10};
    case 10: return Rg{1, -1e-12, 1e-12};
    case 11: return Rg{2, 5, 5};
    case 12: return Rg{1, 0.5, 1.5};
    default: return Rg{1, 1, 3};
  }
}
static Rg rrg() {
  double x = crd(), w = coin(0.1) ? 0.0 : (coin(0.5) ? 0.5 * rint(1, 8) : rnd(0, 5));
  int k = rint(0, 9);
  if (k == 0) return Rg{0, x, x + w};
  if (k == 1) return Rg{1, x + w, x};
  if (k == 2) return Rg{2, x + w, x};
  if (k == 3) return Rg{3, x, 0};
  return Rg{1, x, x + w};
}
static Rg prg(int att, int slot) {
  if (att < NEDGE_RG * 2) return edge_rg(slot == 0 ? att % NEDGE_RG : (att * 5 + 2 + att / NEDGE_RG) % NEDGE_RG);
  return rrg();
}
static double rgval(const Rg& s) {
  switch (rint(0, 6)) {
    case 0: return s.a;
    case 1: return s.b;
    case 2: return 0.5 * (s.a + s.b);
    case 3: return nice();
    default: return rnd(std::min(s.a, s.b) - 2, std::max(s.a, s.b) + 2);
  }
}
static void put_range(const Bnd_Range& r) {
  ob(r.IsVoid());
  auto g = r.Get();
  ob(g.has_value());
  if (g) { h(g->Min); h(g->Max); }
}
static int NR = 28;
static void fam_range() {
  { LOOP("range_state", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg();
    begin(N, "rg"); W_rg(s); bar();
    emit([&] {
      Bnd_Range r = mkR(s);
      put_range(r);
      double v = 7.5; bool o1 = r.GetMin(v); ob(o1); if (o1) h(v);
      v = 7.5; bool o2 = r.GetMax(v); ob(o2); if (o2) h(v);
      double a = 7.5, b = 8.5; bool o3 = r.GetBounds(a, b); ob(o3); if (o3) { h(a); h(b); }
      auto c = r.Center(); ob(c.has_value()); if (c) h(*c);
      h(r.Delta());
      auto mn = r.Min(); ob(mn.has_value()); if (mn) h(*mn);
      auto mx = r.Max(); ob(mx.has_value()); if (mx) h(*mx);
    });
  } }
  { LOOP("range_intermediate", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double l = att % 4 == 0 ? 0.5 : (att % 4 == 1 ? 0.0 : (att % 4 == 2 ? 1.0 : rnd(-1, 2)));
    begin(N, "rg f"); W_rg(s); W_f(l); bar();
    emit([&] { Bnd_Range r = mkR(s); double p = 9.5; bool ok = r.GetIntermediatePoint(l, p); ob(ok); if (ok) h(p); });
  } }
  { LOOP("range_add_param", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double v = rgval(s);
    begin(N, "rg f"); W_rg(s); W_f(v); bar();
    emit([&] { Bnd_Range r = mkR(s); r.Add(v); put_range(r); });
  } }
  { LOOP("range_add_range", NR) {
    Rg s = prg(att, 0), o = prg(att, 1);
    begin(N, "rg rg"); W_rg(s); W_rg(o); bar();
    emit([&] { Bnd_Range r = mkR(s); r.Add(mkR(o)); put_range(r); });
  } }
  { LOOP("range_common", NR) {
    Rg s = prg(att, 0), o = prg(att, 1);
    begin(N, "rg rg"); W_rg(s); W_rg(o); bar();
    emit([&] { Bnd_Range r = mkR(s); r.Common(mkR(o)); put_range(r); });
  } }
  { LOOP("range_union", NR) {
    Rg s = prg(att, 0), o = prg(att, 1);
    begin(N, "rg rg"); W_rg(s); W_rg(o); bar();
    emit([&] { Bnd_Range r = mkR(s); bool u = r.Union(mkR(o)); ob(u); put_range(r); });
  } }
  { LOOP("range_enlarge", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double d = coin(0.5) ? 0.25 * rint(-8, 8) : rnd(-2, 2);
    begin(N, "rg f"); W_rg(s); W_f(d); bar();
    emit([&] { Bnd_Range r = mkR(s); r.Enlarge(d); put_range(r); });
  } }
  { LOOP("range_shift", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double d = coin(0.5) ? 0.25 * rint(-8, 8) : rnd(-3, 3);
    begin(N, "rg f"); W_rg(s); W_f(d); bar();
    emit([&] { Bnd_Range r = mkR(s); put_range(r.Shifted(d)); r.Shift(d); put_range(r); });
  } }
  { LOOP("range_trim", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double lo = rgval(s), hi = rgval(s);
    begin(N, "rg f f"); W_rg(s); W_f(lo); W_f(hi); bar();
    emit([&] { Bnd_Range r = mkR(s), q = mkR(s); r.TrimFrom(lo); put_range(r); q.TrimTo(hi); put_range(q); });
  } }
  { LOOP("range_set_void", 18) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg();
    begin(N, "rg"); W_rg(s); bar();
    emit([&] { Bnd_Range r = mkR(s); r.SetVoid(); put_range(r); });
  } }
  { LOOP("range_is_out", NR) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double v = rgval(s);
    begin(N, "rg f"); W_rg(s); W_f(v); bar();
    emit([&] { Bnd_Range r = mkR(s); ob(r.IsOut(v)); ob(r.Contains(v)); });
  } }
  { LOOP("range_is_out_range", NR) {
    Rg s = prg(att, 0), o = prg(att, 1);
    begin(N, "rg rg"); W_rg(s); W_rg(o); bar();
    emit([&] { Bnd_Range r = mkR(s), q = mkR(o); ob(r.IsOut(q)); ob(r.Intersects(q)); ob(r == q); });
  } }
  { LOOP("range_is_intersected", NR + 14) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double v = att % 3 == 0 ? s.a : (att % 3 == 1 ? s.b : rgval(s));
    double per = att % 4 == 0 ? 0.0 : (att % 4 == 1 ? 1.0 : (att % 4 == 2 ? 0.5 * rint(-6, 6) : rnd(0.2, 4)));
    if (att % 7 == 0) per = s.b - s.a;
    begin(N, "rg f f"); W_rg(s); W_f(v); W_f(per); bar();
    emit([&] { Bnd_Range r = mkR(s); in((long)r.IsIntersected(v, per)); });
  } }
  { LOOP("range_split", NR + 14) {
    Rg s = att < NEDGE_RG ? edge_rg(att) : rrg(); double v = att % 3 == 0 ? s.a : rgval(s);
    double per = att % 3 == 0 ? 0.0 : (att % 3 == 1 ? 0.5 * rint(1, 8) : rnd(0.2, 3));
    if (std::abs(s.b - s.a) / std::max(per, 1e-300) > 400.0 && per > 0) per = 0.0;
    begin(N, "rg f f"); W_rg(s); W_f(v); W_f(per); bar();
    emit([&] {
      Bnd_Range r = mkR(s); NCollection_List<Bnd_Range> lst; r.Split(v, lst, per);
      in((long)lst.Size());
      for (const Bnd_Range& x : lst) put_range(x);
    });
  } }
}

// =================================================================================
// Bnd_Sphere
struct Sp { double c[3], r; int u, v, valid; };
static Bnd_Sphere mkS(const Sp& s) { Bnd_Sphere b(gp_XYZ(s.c[0], s.c[1], s.c[2]), s.r, s.u, s.v); b.SetValid(s.valid != 0); return b; }
static void W_sph(const Sp& s) { int st = ntok; h(s.c[0]); h(s.c[1]); h(s.c[2]); h(s.r); in(s.u); in(s.v); in(s.valid); word("sph", st, 7); }
static Sp rsp(int att) {
  Sp s; for (int k = 0; k < 3; ++k) s.c[k] = crd();
  s.r = att % 11 == 0 ? -1.0 : (att % 11 == 1 ? 0.0 : (coin(0.4) ? 0.5 * rint(1, 6) : rnd(0, 4)));
  s.u = rint(-2, 9); s.v = rint(-2, 9); s.valid = coin(0.5);
  return s;
}
static Sp psp(int att, int slot) {
  Sp s = rsp(att + slot * 3);
  if (att < 20 && slot == 1) { s = rsp(att + 3); }
  return s;
}
static void put_sphere(const Bnd_Sphere& b) {
  in(b.U()); in(b.V()); ob(b.IsValid()); xyz(b.Center()); h(b.Radius()); hi(b.SquareExtent());
}
static void fam_sphere() {
  { LOOP("sphere_default", 1) {
    begin(N, ""); bar();
    emit([&] { Bnd_Sphere b; put_sphere(b); });
  } }
  { LOOP("sphere_state", NR) {
    Sp s = rsp(att);
    begin(N, "sph"); W_sph(s); bar();
    emit([&] { put_sphere(mkS(s)); });
  } }
  { LOOP("sphere_set_valid", 18) {
    Sp s = rsp(att); int v = att % 2;
    begin(N, "sph i"); W_sph(s); W_i(v); bar();
    emit([&] { Bnd_Sphere b = mkS(s); b.SetValid(v != 0); put_sphere(b); });
  } }
  { LOOP("sphere_distances", NR) {
    Sp s = rsp(att); gp_XYZ p(crd(), crd(), crd());
    if (att % 5 == 0) p = gp_XYZ(s.c[0], s.c[1], s.c[2]);
    if (att % 5 == 1) p = gp_XYZ(s.c[0] + s.r, s.c[1], s.c[2]);
    begin(N, "sph xyz"); W_sph(s); W_xyz(p); bar();
    emit([&] {
      Bnd_Sphere b = mkS(s);
      double mn, mx; b.Distances(p, mn, mx); h(mn); h(mx);
      b.SquareDistances(p, mn, mx); h(mn); h(mx);
      h(b.Distance(p)); h(b.SquareDistance(p));
      gp_XYZ proj; double dist; bool inside; bool ok = b.Project(p, proj, dist, inside); ob(ok); xyz(proj); h(dist); ob(inside);
    });
  } }
  { LOOP("sphere_add", NR + 14) {
    Sp s = psp(att, 0), o = psp(att, 1);
    if (att % 6 == 2) { o = s; o.r = s.r + 1; }
    if (att % 6 == 3) { o = s; o.c[0] += 0.25; }
    begin(N, "sph sph"); W_sph(s); W_sph(o); bar();
    emit([&] { Bnd_Sphere b = mkS(s); b.Add(mkS(o)); put_sphere(b); });
  } }
  { LOOP("sphere_is_out_sphere", NR) {
    Sp s = psp(att, 0), o = psp(att, 1);
    begin(N, "sph sph"); W_sph(s); W_sph(o); bar();
    emit([&] { ob(mkS(s).IsOut(mkS(o))); });
  } }
  { LOOP("sphere_is_out_pnt", NR + 14) {
    Sp s = rsp(att); gp_XYZ p(crd(), crd(), crd()); double md = att % 4 == 0 ? 1e30 : (coin(0.3) ? 0.5 * rint(0, 12) : rnd(0, 8));
    begin(N, "sph xyz f"); W_sph(s); W_xyz(p); W_f(md); bar();
    emit([&] { Bnd_Sphere b = mkS(s); double m = md; bool o = b.IsOut(p, m); ob(o); h(m); });
  } }
}

// =================================================================================
// Bnd_OBB
static Raw rcross(const Raw& a, const Raw& b) { return Raw{a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x}; }
static double rnorm(const Raw& a) { return std::sqrt(a.x * a.x + a.y * a.y + a.z * a.z); }
static Raw runit(const Raw& a) { double n = rnorm(a); return Raw{a.x / n, a.y / n, a.z / n}; }
// three mutually orthogonal raw directions (right-handed)
static void rframe(Raw ax[3]) {
  int k = rint(0, 5);
  if (k == 0) { ax[0] = Raw{1, 0, 0}; ax[1] = Raw{0, 1, 0}; ax[2] = Raw{0, 0, 1}; return; }
  if (k == 1) { ax[0] = Raw{1, 1, 0}; ax[1] = Raw{-1, 1, 0}; ax[2] = Raw{0, 0, 1}; return; }
  if (k == 2) { ax[0] = Raw{0, 1, 0}; ax[1] = Raw{0, 0, 1}; ax[2] = Raw{1, 0, 0}; return; }
  for (;;) {
    Raw a = rraw(), t = rraw(), b = rcross(a, t);
    if (rnorm(b) < 0.3) continue;
    Raw c = rcross(a, b);
    ax[0] = a; ax[1] = b; ax[2] = c; return;
  }
}
struct Pts { int lo; std::vector<gp_Pnt> p; int has_tol; std::vector<double> t; };
static void put_pts(const Pts& s) { in(s.lo); in((long)s.p.size()); for (const gp_Pnt& p : s.p) pnt(p); }
static void put_tols(const Pts& s) { in(s.has_tol); if (s.has_tol) for (double t : s.t) h(t); }
static void W_pts(const Pts& s) { int st = ntok; put_pts(s); word("pts", st, -1); }
static void W_tols(const Pts& s) { int st = ntok; put_tols(s); word("tols", st, -1); }
static NCollection_Array1<gp_Pnt> mkPts(const Pts& s) {
  NCollection_Array1<gp_Pnt> a(s.lo, s.lo + (int)s.p.size() - 1);
  for (size_t i = 0; i < s.p.size(); ++i) a.SetValue(s.lo + (int)i, s.p[i]);
  return a;
}
static NCollection_Array1<double> mkTols(const Pts& s) {
  NCollection_Array1<double> a(s.lo, s.lo + (int)s.p.size() - 1);
  for (size_t i = 0; i < s.p.size(); ++i) a.SetValue(s.lo + (int)i, s.t[i]);
  return a;
}
static gp_Pnt lincomb(const gp_Pnt& o, const Raw& a, double s, const Raw& b, double t, const Raw& c, double u) {
  return gp_Pnt(o.X() + a.x * s + b.x * t + c.x * u, o.Y() + a.y * s + b.y * t + c.y * u, o.Z() + a.z * s + b.z * t + c.z * u);
}
// point sets: sel 0 generic cloud in a rotated box; 1 axis-aligned cloud; 2 on a sphere; the rest are the special sets
static const int SPECIAL_LO = 3, SPECIAL_HI = 19;
static Pts mkpts(int sel, int n) {
  Pts s; s.lo = coin(0.6) ? 1 : (coin(0.7) ? 0 : 5); s.has_tol = 0;
  Raw fr[3]; rframe(fr);
  Raw u0 = runit(fr[0]), u1 = runit(fr[1]), u2 = runit(fr[2]);
  gp_Pnt o = rpnt();
  double sx = rnd(0.2, 4), sy = rnd(0.2, 4), sz = rnd(0.2, 4);
  auto add = [&](const gp_Pnt& p) { s.p.push_back(p); };
  switch (sel) {
    case 0: for (int i = 0; i < n; ++i) add(lincomb(o, u0, rnd(-1, 1) * sx, u1, rnd(-1, 1) * sy, u2, rnd(-1, 1) * sz)); break;
    case 1: for (int i = 0; i < n; ++i) add(gp_Pnt(o.X() + rnd(-1, 1) * sx, o.Y() + rnd(-1, 1) * sy, o.Z() + rnd(-1, 1) * sz)); break;
    case 2: for (int i = 0; i < n; ++i) { Raw d = runit(rraw()); add(gp_Pnt(o.X() + d.x * sx, o.Y() + d.y * sx, o.Z() + d.z * sx)); } break;
    case 3: { Raw d = mraw(); for (int i = 0; i < n; ++i) { double t = rnd(-3, 3); add(gp_Pnt(o.X() + d.x * t, o.Y() + d.y * t, o.Z() + d.z * t)); } } break; // collinear
    case 4: for (int i = 0; i < n; ++i) add(lincomb(o, u0, rnd(-1, 1) * sx, u1, rnd(-1, 1) * sy, u2, 0.0)); break;                                         // coplanar (rotated)
    case 5: for (int i = 0; i < n; ++i) add(gp_Pnt(o.X() + rnd(-2, 2), o.Y() + rnd(-2, 2), o.Z())); break;                                                  // coplanar z = const
    case 6: for (int i = 0; i < n; ++i) add(o); break;                                                                                                       // all equal
    case 7: for (int i = 0; i < 8; ++i) add(gp_Pnt(o.X() + ((i & 1) ? 1 : -1), o.Y() + ((i & 2) ? 1 : -1), o.Z() + ((i & 4) ? 1 : -1))); break;            // cube corners
    case 8: for (int i = 0; i < 27; ++i) add(gp_Pnt(o.X() + (i % 3) - 1, o.Y() + ((i / 3) % 3) - 1, o.Z() + (i / 9) - 1)); break;                          // 3x3x3 grid
    case 9: { add(gp_Pnt(1, 0, 0)); add(gp_Pnt(-1, 0, 0)); add(gp_Pnt(0, 1, 0)); add(gp_Pnt(0, -1, 0)); add(gp_Pnt(0, 0, 1)); add(gp_Pnt(0, 0, -1)); } break; // octahedron
    case 10: { add(gp_Pnt(0, 0, 0)); add(gp_Pnt(1, 0, 0)); add(gp_Pnt(0, 1, 0)); add(gp_Pnt(0, 0, 1)); } break;                                              // tetrahedron
    case 11: { add(gp_Pnt(0, 0, 0)); add(gp_Pnt(2, 0, 0)); add(gp_Pnt(2, 1, 0)); add(gp_Pnt(0, 1, 0)); } break;                                              // rectangle
    case 12: { add(gp_Pnt(0, 0, 0)); add(gp_Pnt(1, 1, 1)); add(gp_Pnt(2, 2, 2)); add(gp_Pnt(1, 0, 0)); } break;                                              // line + one point
    case 13: { gp_Pnt p = rpnt(); add(p); add(gp_Pnt(p.X() + 3e-8, p.Y(), p.Z())); } break;                                                                  // two points closer than Confusion
    case 14: { add(rpnt()); add(rpnt()); } break;                                                                                                            // two points
    case 15: { add(rpnt()); } break;                                                                                                                         // one point
    case 16: for (int i = 0; i < n / 2 + 1; ++i) { gp_Pnt p = lincomb(gp_Pnt(0, 0, 0), u0, rnd(-1, 1) * sx, u1, rnd(-1, 1) * sy, u2, rnd(-1, 1) * sz); add(p); add(gp_Pnt(-p.X(), -p.Y(), -p.Z())); } break; // centrally symmetric
    case 17: for (int i = 0; i < n; ++i) add(gp_Pnt(rint(-2, 2), rint(-2, 2), rint(-2, 2))); break;                                                          // lattice points (many ties)
    case 18: { for (int i = 0; i < n; ++i) { double t = i; add(gp_Pnt(o.X() + t, o.Y(), o.Z())); } } break;                                                    // equally spaced on an axis
    default: { add(gp_Pnt(0, 0, 0)); add(gp_Pnt(0, 0, 0)); add(gp_Pnt(1, 0, 0)); } break;                                                                     // duplicates + one
  }
  return s;
}
static void settols(Pts& s, double prob) {
  if (!coin(prob)) return;
  s.has_tol = 1; s.t.clear();
  int mode = rint(0, 3);
  for (size_t i = 0; i < s.p.size(); ++i) s.t.push_back(mode == 0 ? 0.0 : (mode == 1 ? 0.1 : rnd(0, 0.3)));
}
struct Ob { int kind; gp_Pnt c; Raw ax[3]; double hs[3]; Bx bx; Pts pts; int optimal; int aabox; };
static Bnd_OBB mkO(const Ob& s) {
  Bnd_OBB o;
  switch (s.kind) {
    case 1: o = Bnd_OBB(s.c, D(s.ax[0]), D(s.ax[1]), D(s.ax[2]), s.hs[0], s.hs[1], s.hs[2]); break;
    case 2: o = Bnd_OBB(mkB(s.bx)); break;
    case 3: {
      NCollection_Array1<gp_Pnt> a = mkPts(s.pts);
      if (s.pts.has_tol) { NCollection_Array1<double> t = mkTols(s.pts); o.ReBuild(a, &t, s.optimal != 0); }
      else o.ReBuild(a, nullptr, s.optimal != 0);
    } break;
    default: break;
  }
  if (s.aabox >= 0) o.SetAABox(s.aabox != 0);
  return o;
}
static void W_obb(const Ob& s) {
  int st = ntok; in(s.kind);
  switch (s.kind) {
    case 1: pnt(s.c); rw(s.ax[0]); rw(s.ax[1]); rw(s.ax[2]); h(s.hs[0]); h(s.hs[1]); h(s.hs[2]); break;
    case 2: in(s.bx.kind); for (int k = 0; k < 6; ++k) h(s.bx.c[k]); h(s.bx.gap); in(s.bx.flags); break;
    case 3: put_pts(s.pts); put_tols(s.pts); in(s.optimal); break;
    default: break;
  }
  in(s.aabox);
  word("obb", st, -1);
}
static Ob mkOb0(int kind) { Ob s; s.kind = kind; s.aabox = -1; s.optimal = 0; s.c = gp_Pnt(0, 0, 0); for (int k = 0; k < 3; ++k) { s.ax[k] = Raw{1, 0, 0}; s.hs[k] = 0; } s.bx = BX(0, 0, 0, 0, 0, 0, 0); s.pts.lo = 1; s.pts.has_tol = 0; return s; }
static Ob rob(int att) {
  Ob s;
  int k = att < 24 ? att % 12 : rint(0, 11);
  switch (k) {
    case 0: s = mkOb0(0); break;
    case 1: case 2: case 3: case 4: {
      s = mkOb0(1); s.c = rpnt(); rframe(s.ax); for (int j = 0; j < 3; ++j) s.hs[j] = coin(0.1) ? 0.0 : (coin(0.5) ? 0.5 * rint(1, 6) : rnd(0.1, 3));
      if (att % 17 == 5) s.hs[1] = -1.0;
    } break;
    case 5: case 6: { s = mkOb0(2); s.bx = rbx(2); } break;
    case 7: case 8: case 9: { s = mkOb0(3); s.pts = mkpts(coin(0.5) ? 0 : rint(1, 5), rint(3, 12)); settols(s.pts, 0.3); s.optimal = coin(0.3); } break;
    case 10: { s = mkOb0(3); s.pts = mkpts(rint(SPECIAL_LO, SPECIAL_HI), 4); settols(s.pts, 0.5); } break;
    default: { s = mkOb0(1); s.c = rpnt(); s.ax[0] = Raw{1, 0, 0}; s.ax[1] = Raw{0, 1, 0}; s.ax[2] = Raw{0, 0, 1}; for (int j = 0; j < 3; ++j) s.hs[j] = 0.5 * rint(1, 4); if (coin(0.5)) s.aabox = 1; } break;
  }
  if (s.aabox < 0 && coin(0.12)) s.aabox = coin(0.5);
  return s;
}
static Ob pob(int att, int slot) {
  Ob s = rob(att + slot * 5);
  if (att % 8 == 3 && slot == 1) { s = mkOb0(1); s.c = gp_Pnt(1, 0, 0); s.ax[0] = Raw{1, 0, 0}; s.ax[1] = Raw{0, 1, 0}; s.ax[2] = Raw{0, 0, 1}; s.hs[0] = s.hs[1] = s.hs[2] = 1.0; s.aabox = 1; }
  if (att % 8 == 3 && slot == 0) { s = mkOb0(1); s.c = gp_Pnt(0, 0, 0); s.ax[0] = Raw{1, 0, 0}; s.ax[1] = Raw{0, 1, 0}; s.ax[2] = Raw{0, 0, 1}; s.hs[0] = s.hs[1] = s.hs[2] = 1.0; s.aabox = 1; }
  return s;
}
static void put_obb(const Bnd_OBB& o) {
  ob(o.IsVoid()); ob(o.IsAABox()); xyz(o.Center()); xyz(o.XDirection()); xyz(o.YDirection()); xyz(o.ZDirection());
  h(o.XHSize()); h(o.YHSize()); h(o.ZHSize());
  Bnd_OBB::HalfSizes hs = o.GetHalfSizes(); h(hs.X); h(hs.Y); h(hs.Z);
  hi(o.SquareExtent());
}
static int NO = 22;
static void fam_obb_rebuild() {
  auto run = [&](const char* N, int target, bool optimal, bool special, int maxn) {
    for (int att = 0; att < 20000 && fam_lines[N] < target; ++att) {
      int sel, n;
      if (special) { sel = SPECIAL_LO + att % (SPECIAL_HI - SPECIAL_LO + 1); n = rint(3, 16); if (att % 3 == 2) n = rint(3, 30); if (optimal && att % 5 == 4 && (sel == 17 || sel == 16 || sel == 18)) n = rint(31, 64); }
      else { sel = coin(0.6) ? 0 : (coin(0.5) ? 1 : 2); n = att < 8 ? 3 + att : (coin(0.2) ? rint(maxn / 2, maxn) : rint(3, std::min(maxn, 24))); }
      Pts p = mkpts(sel, n);
      settols(p, 0.35);
      begin(N, "pts tols i"); W_pts(p); W_tols(p); W_i(optimal); bar();
      emit([&] {
        Bnd_OBB o; NCollection_Array1<gp_Pnt> a = mkPts(p);
        if (p.has_tol) { NCollection_Array1<double> t = mkTols(p); o.ReBuild(a, &t, optimal); } else o.ReBuild(a, nullptr, optimal);
        put_obb(o);
      });
    }
  };
  run("obb_rebuild", NO + 12, false, false, 30);
  run("obb_rebuild_special", NO + 16, false, true, 30);
  run("obb_rebuild_optimal", NO, true, false, 90);
  run("obb_rebuild_optimal_special", NO + 12, true, true, 30);
}
static void fam_obb() {
  { LOOP("obb_state", NO) {
    Ob s = rob(att);
    begin(N, "obb"); W_obb(s); bar();
    emit([&] { put_obb(mkO(s)); });
  } }
  { LOOP("obb_from_box", NO) {
    Bx s = att < NEDGE_BX ? edge_bx(att) : rbx(0);
    begin(N, "bx"); W_bx(s); bar();
    emit([&] { put_obb(Bnd_OBB(mkB(s))); });
  } }
  { LOOP("obb_ctor", NO) {
    Ob s = mkOb0(1); s.c = rpnt(); rframe(s.ax); for (int j = 0; j < 3; ++j) s.hs[j] = coin(0.1) ? 0.0 : (coin(0.1) ? -0.5 : rnd(0.1, 3));
    begin(N, "pnt dir dir dir f f f"); W_pnt(s.c); W_dir(s.ax[0]); W_dir(s.ax[1]); W_dir(s.ax[2]); W_f(s.hs[0]); W_f(s.hs[1]); W_f(s.hs[2]); bar();
    emit([&] { put_obb(Bnd_OBB(s.c, D(s.ax[0]), D(s.ax[1]), D(s.ax[2]), s.hs[0], s.hs[1], s.hs[2])); });
  } }
  { LOOP("obb_setters", NO) {
    Ob s = rob(att); gp_Pnt c = rpnt(); Raw ax[3]; rframe(ax); double hs[3]; for (int j = 0; j < 3; ++j) hs[j] = coin(0.1) ? 0.0 : rnd(0.1, 3);
    begin(N, "obb pnt dir f dir f dir f"); W_obb(s); W_pnt(c); W_dir(ax[0]); W_f(hs[0]); W_dir(ax[1]); W_f(hs[1]); W_dir(ax[2]); W_f(hs[2]); bar();
    emit([&] {
      Bnd_OBB o = mkO(s); o.SetCenter(c); put_obb(o);
      o.SetXComponent(D(ax[0]), hs[0]); put_obb(o);
      o.SetYComponent(D(ax[1]), hs[1]); o.SetZComponent(D(ax[2]), hs[2]); put_obb(o);
    });
  } }
  { LOOP("obb_enlarge", NO) {
    Ob s = rob(att); double g = coin(0.3) ? 0.0 : (coin(0.5) ? -0.5 * rint(1, 4) : rnd(0, 2));
    begin(N, "obb f"); W_obb(s); W_f(g); bar();
    emit([&] { Bnd_OBB o = mkO(s); o.Enlarge(g); put_obb(o); });
  } }
  { LOOP("obb_void_aabox", 18) {
    Ob s = rob(att); int m = att % 3;
    begin(N, "obb i"); W_obb(s); W_i(m); bar();
    emit([&] { Bnd_OBB o = mkO(s); if (m == 0) o.SetVoid(); else o.SetAABox(m == 2); put_obb(o); });
  } }
  { LOOP("obb_vertices", NO) {
    Ob s = rob(att);
    begin(N, "obb"); W_obb(s); bar();
    emit([&] { Bnd_OBB o = mkO(s); gp_Pnt v[8]; bool ok = o.GetVertex(v); ob(ok); if (ok) for (int i = 0; i < 8; ++i) pnt(v[i]); });
  } }
  { LOOP("obb_position", NO) {
    Ob s = rob(att);
    begin(N, "obb"); W_obb(s); bar();
    emit([&] { gp_Ax3 a = mkO(s).Position(); pnt(a.Location()); dir(a.Direction()); dir(a.XDirection()); dir(a.YDirection()); });
  } }
  { LOOP("obb_is_out_pnt", NO + 12) {
    Ob s = rob(att); gp_Pnt p = rpnt();
    if (att % 3 == 0) { Bnd_OBB o = mkO(s); if (!o.IsVoid()) { gp_Pnt v[8]; o.GetVertex(v); p = v[att % 8]; if (att % 6 == 3) p = gp_Pnt(o.Center().X(), o.Center().Y(), o.Center().Z()); } }
    begin(N, "obb pnt"); W_obb(s); W_pnt(p); bar();
    emit([&] { Bnd_OBB o = mkO(s); ob(o.IsOut(p)); ob(o.Contains(p)); });
  } }
  { LOOP("obb_is_out_obb", NO + 16) {
    Ob s = pob(att, 0), q = pob(att, 1);
    begin(N, "obb obb"); W_obb(s); W_obb(q); bar();
    emit([&] { Bnd_OBB o = mkO(s), p = mkO(q); ob(o.IsOut(p)); ob(o.Intersects(p)); });
  } }
  { LOOP("obb_completely_inside", NO) {
    Ob s = pob(att, 0), q = pob(att, 1);
    begin(N, "obb obb"); W_obb(s); W_obb(q); bar();
    emit([&] { ob(mkO(s).IsCompletelyInside(mkO(q))); });
  } }
  { LOOP("obb_add_pnt", NO) {
    Ob s = rob(att); gp_Pnt p = rpnt();
    begin(N, "obb pnt"); W_obb(s); W_pnt(p); bar();
    emit([&] { Bnd_OBB o = mkO(s); o.Add(p); put_obb(o); });
  } }
  { LOOP("obb_add_obb", NO) {
    Ob s = pob(att, 0), q = pob(att, 1);
    begin(N, "obb obb"); W_obb(s); W_obb(q); bar();
    emit([&] { Bnd_OBB o = mkO(s); o.Add(mkO(q)); put_obb(o); });
  } }
  fam_obb_rebuild();
}

// =================================================================================
// Bnd_BoundSortBox
static void put_bx(const Bx& s) { in(s.kind); for (int k = 0; k < 6; ++k) h(s.c[k]); h(s.gap); in(s.flags); }
struct Bgen { long seed; int n; double lo, hi, size; int gmode; };
static void put_gen(const Bgen& g) { in(g.seed); in(g.n); h(g.lo); h(g.hi); h(g.size); in(g.gmode); }
// the box generator described in docs/api-bnd.md
static std::vector<Bnd_Box> gen_boxes(const Bgen& g) {
  uint64_t st = 0x9E3779B97F4A7C15ull ^ ((uint64_t)g.seed * 0xD1B54A32D192ED03ull);
  if (st == 0) st = 1;
  auto r = [&](double lo, double hi) {
    st ^= st << 13; st ^= st >> 7; st ^= st << 17;
    return lo + (hi - lo) * (double)(st >> 11) / 9007199254740992.0;
  };
  std::vector<Bnd_Box> out;
  for (int i = 0; i < g.n; ++i) {
    double x = r(g.lo, g.hi), y = r(g.lo, g.hi), z = r(g.lo, g.hi);
    double sx = r(0.0, g.size), sy = r(0.0, g.size), sz = r(0.0, g.size);
    Bnd_Box b;
    if (!(g.gmode == 1 && i % 5 == 4)) {
      b.Update(x, y, z, x + sx, y + sy, z + sz);
      if (g.gmode == 1 && i % 7 == 3) b.SetGap(0.1 * sx);
    }
    out.push_back(b);
  }
  return out;
}
struct Bs { int mode, lo, n; Bx enc; std::vector<Bx> boxes; std::vector<std::pair<int, Bx>> adds; Bgen g; };
static void W_bsb(const Bs& s) {
  int st = ntok; in(s.mode); in(s.lo); in(s.n);
  switch (s.mode) {
    case 0: for (const Bx& b : s.boxes) put_bx(b); break;
    case 1: put_bx(s.enc); for (const Bx& b : s.boxes) put_bx(b); break;
    case 2: put_bx(s.enc); in((long)s.adds.size()); for (auto& a : s.adds) { in(a.first); put_bx(a.second); } break;
    case 3: put_gen(s.g); break;
    default: put_bx(s.enc); put_gen(s.g); break;
  }
  word("bsb", st, -1);
}
static int voxel_res(int n) { return n > 40000 ? 128 : (n > 10000 ? 64 : (n > 1000 ? 32 : (n > 100 ? 16 : 8))); }
// the boxes stored by the structure (the generated ones and the explicit ones)
static std::vector<Bnd_Box> bsb_boxes(const Bs& s) {
  std::vector<Bnd_Box> v;
  if (s.mode == 0 || s.mode == 1) for (const Bx& b : s.boxes) v.push_back(mkB(b));
  else if (s.mode == 2) for (auto& a : s.adds) v.push_back(mkB(a.second));
  else v = gen_boxes(s.g);
  return v;
}
static int bsb_count(const Bs& s) { return (s.mode == 0 || s.mode == 1 || s.mode >= 3) ? (s.mode >= 3 ? s.g.n : s.n) : s.n; }
// The voxel indices are computed with static_cast<int>(double): undefined behaviour when the value does not fit. Cases in
// which any box or query would do that are not generated.
static bool voxel_safe(const Bs& s, const std::vector<Bnd_Box>& queries) {
  Bnd_Box enc;
  std::vector<Bnd_Box> bx = bsb_boxes(s);
  if (s.mode == 0 || s.mode == 3) { for (const Bnd_Box& b : bx) if (!b.IsVoid()) enc.Add(b); }
  else enc = mkB(s.enc);
  if (enc.IsVoid()) return true;
  int res = voxel_res(bsb_count(s));
  double xmin, ymin, zmin, xmax, ymax, zmax;
  enc.Get(xmin, ymin, zmin, xmax, ymax, zmax);
  double cx = (xmax - xmin == 0. ? 0. : res / (xmax - xmin)), cy = (ymax - ymin == 0. ? 0. : res / (ymax - ymin)), cz = (zmax - zmin == 0. ? 0. : res / (zmax - zmin));
  gp_Pnt start = enc.CornerMin();
  auto ok = [&](const Bnd_Box& b) {
    if (b.IsVoid()) return true;
    double a0, a1, a2, a3, a4, a5; b.Get(a0, a1, a2, a3, a4, a5);
    double v[6] = {(a0 - start.X()) * cx, (a1 - start.Y()) * cy, (a2 - start.Z()) * cz, (a3 - start.X()) * cx, (a4 - start.Y()) * cy, (a5 - start.Z()) * cz};
    for (double d : v) if (!(std::fabs(d) < 1e9)) return false;
    return true;
  };
  for (const Bnd_Box& b : bx) if (!ok(b)) return false;
  for (const Bnd_Box& b : queries) if (!ok(b)) return false;
  return true;
}
// mode 2: what the adds do. true = OCCT 8.0.1 as built has the checks of Add() compiled out (an index outside 1..n
// writes outside the array, a second box at a defined index is stored twice): the result is unspecified.
static bool bsb_adds_unchecked(const Bs& s) {
  if (s.mode != 2) return false;
  if (s.n <= 0) return true;
  std::vector<bool> def(s.n + 1, false);
  for (auto& a : s.adds) {
    if (a.first < 1 || a.first > s.n) return true;
    if (def[a.first]) return true;
    if (!mkB(a.second).IsVoid()) def[a.first] = true;
  }
  return false;
}
static void run_bsb(const Bs& s, Bnd_BoundSortBox& b) {
  auto arr = [&](const std::vector<Bnd_Box>& v, int lo) {
    occ::handle<NCollection_HArray1<Bnd_Box>> a = new NCollection_HArray1<Bnd_Box>(lo, lo + (int)v.size() - 1);
    for (size_t i = 0; i < v.size(); ++i) a->SetValue(lo + (int)i, v[i]);
    return a;
  };
  switch (s.mode) {
    case 0: b.Initialize(arr(bsb_boxes(s), s.lo)); break;
    case 1: b.Initialize(mkB(s.enc), arr(bsb_boxes(s), s.lo)); break;
    case 2: { b.Initialize(mkB(s.enc), s.n); for (auto& a : s.adds) b.Add(mkB(a.second), a.first); } break;
    case 3: b.Initialize(arr(gen_boxes(s.g), s.lo)); break;
    case 4: b.Initialize(mkB(s.enc), arr(gen_boxes(s.g), s.lo)); break;
    default: { std::vector<Bnd_Box> v = gen_boxes(s.g); b.Initialize(mkB(s.enc), (int)v.size()); for (size_t i = 0; i < v.size(); ++i) b.Add(v[i], (int)i + 1); } break;
  }
}
static void put_list(const NCollection_List<int>& l) {
  if (l.Size() > 400) throw Discard();
  in((long)l.Size()); for (int v : l) in(v);
}
// a random explicit set of boxes for the modes 0, 1, 2
static Bs rbs(int att, bool errors) {
  Bs s; s.mode = att % 5 < 2 ? 0 : (att % 5 < 4 ? 1 : 2); if (errors) s.mode = 2;
  s.lo = s.mode == 2 ? 1 : (coin(0.6) ? 1 : (coin(0.6) ? 0 : rint(2, 5)));
  s.n = att % 9 == 0 ? rint(1, 3) : rint(4, 26);
  s.enc = edge_bx(0); s.g = Bgen{0, 0, 0, 0, 0, 0};
  std::vector<Bx> v;
  for (int i = 0; i < s.n; ++i) { Bx b = rbx(coin(0.12) ? 0 : 2); v.push_back(b); }
  if (s.mode == 0 || s.mode == 1) s.boxes = v;
  if (s.mode >= 1) {
    int k = rint(0, 13);
    if (k == 0) s.enc = edge_bx(0);                   // void enclosing box
    else if (k == 1) s.enc = edge_bx(1);              // whole
    else if (k == 2) s.enc = BX(2, -2, -2, -2, 2, 2, 2);  // too small: the boxes are clamped to the border voxels
    else if (k == 3) s.enc = BX(2, 0, 0, 0, 0, 3, 3);      // flat in x
    else s.enc = BX(2, -7, -7, -7, 8, 8, 8, coin(0.2) ? 0.5 : 0.0);
  }
  if (s.mode == 2) {
    std::vector<int> idx; for (int i = 1; i <= s.n; ++i) idx.push_back(i);
    for (int i = (int)idx.size() - 1; i > 0; --i) std::swap(idx[i], idx[rint(0, i)]);
    int m = coin(0.3) ? s.n : rint(1, s.n);
    for (int i = 0; i < m; ++i) s.adds.push_back({idx[i], v[i]});
    if (errors && att % 3 != 0) {
      int k = att % 6;
      if (k == 0) s.adds.push_back({s.n + 1, rbx(2)});          // outside the array
      else if (k == 1) s.adds.push_back({0, rbx(2)});
      else if (k == 2 && !s.adds.empty()) s.adds.push_back({s.adds[0].first, rbx(2)});  // defined twice (if the first is not void)
      else if (k == 3) { s.n = 0; s.adds.clear(); }
      else if (k == 4) { std::set<int> used; for (auto& a : s.adds) used.insert(a.first); for (int i = 1; i <= s.n; ++i) if (!used.count(i)) { s.adds.push_back({i, edge_bx(0)}); break; } }  // a void box: ignored
    }
  }
  return s;
}
static Bs gbs(int att) {
  static const int NS[] = {5, 20, 60, 150, 400, 1200, 3000, 10500, 40100};
  Bs s; s.mode = 3 + att % 3; s.lo = s.mode == 5 ? 1 : (coin(0.6) ? 1 : 0);
  int ni = att < 9 ? att : (coin(0.7) ? rint(0, 5) : rint(0, 8));
  s.n = NS[ni];
  double sizes[] = {0.3, 2.0, 12.0};
  s.g = Bgen{(long)rint(1, 1000000), s.n, -10.0, 10.0, sizes[ni >= 7 ? 0 : (att % 3)], att % 2};
  s.enc = coin(0.7) ? BX(2, -12, -12, -12, 24, 24, 24) : (coin(0.5) ? BX(2, -10, -10, -10, 10, 10, 10) : BX(2, -3, -3, -3, 3, 3, 3));
  return s;
}
static Bx qbx_for(const Bs& s, int att) {
  (void)att;
  std::vector<Bx> pool = s.boxes;
  for (auto& a : s.adds) pool.push_back(a.second);
  if (s.mode < 3 && !pool.empty() && coin(0.7)) {
    const Bx& r = pool[rint(0, (int)pool.size() - 1)];
    if (r.kind == 1 || r.kind == 2 || r.kind == 4) {
      double off = coin(0.4) ? 0.0 : rnz(0.0, 3.0), w = coin(0.4) ? 0.0 : rnd(0, 1.5);
      double lo[3], hi[3];
      for (int k = 0; k < 3; ++k) { lo[k] = std::min(r.c[k], r.c[3 + k]) + off - w; hi[k] = std::max(r.c[k], r.c[3 + k]) + off + w; if (r.kind == 4) hi[k] = lo[k] + w; }
      Bx q = BX(2, lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]);
      if (coin(0.15)) q.gap = 0.5;
      return q;
    }
  }
  double lo = s.mode >= 3 ? s.g.lo : -4, hi = s.mode >= 3 ? s.g.hi : 5;
  double sz = s.mode >= 3 ? (s.n >= 10000 ? 0.3 : (coin(0.5) ? 1.0 : 6.0)) : (coin(0.5) ? 1.0 : 4.0);
  double x = rnd(lo, hi), y = rnd(lo, hi), z = rnd(lo, hi);
  Bx b = BX(2, x, y, z, x + rnd(0, sz), y + rnd(0, sz), z + rnd(0, sz));
  if (coin(0.06)) b = edge_bx(0);
  if (coin(0.04)) b.gap = 0.5;
  return b;
}
static void fam_bsb() {
  static const int NBS = 32;
  auto run_boxq = [&](const char* N, int target, bool errors, bool gen) {
    for (int att = 0; att < 20000 && fam_lines[N] < target; ++att) {
      Bs s = gen ? gbs(att) : rbs(att, errors);
      Bx q[3]; std::vector<Bnd_Box> qb;
      for (int k = 0; k < 3; ++k) { q[k] = qbx_for(s, att); qb.push_back(mkB(q[k])); }
      bool unchecked_case = bsb_adds_unchecked(s);
      if (!voxel_safe(s, qb)) { fam_dropped[N]++; continue; }   // also for the unchecked cases: the Rust side may run the adds that precede the error
      begin(N, "bsb bx bx bx"); W_bsb(s); W_bx(q[0]); W_bx(q[1]); W_bx(q[2]); bar();
      emit([&] {
        if (unchecked_case) { unchecked(); return; }
        Bnd_BoundSortBox b; run_bsb(s, b);
        for (int k = 0; k < 3; ++k) put_list(b.Compare(mkB(q[k])));
      });
    }
  };
  run_boxq("bsb_compare_box", NBS, false, false);
  run_boxq("bsb_compare_box_gen", NBS, false, true);
  run_boxq("bsb_add", NBS - 10, true, false);
  auto run_plnq = [&](const char* N, int target, bool gen) {
    for (int att = 0; att < 20000 && fam_lines[N] < target; ++att) {
      Bs s = gen ? gbs(att) : rbs(att, false);
      if (!voxel_safe(s, std::vector<Bnd_Box>())) { fam_dropped[N]++; continue; }
      gp_Pnt p[3]; Raw d[3];
      for (int k = 0; k < 3; ++k) { p[k] = gp_Pnt(rnd(-5, 6), rnd(-5, 6), rnd(-5, 6)); d[k] = mraw(); }
      begin(N, "bsb pnt dir pnt dir pnt dir"); W_bsb(s); for (int k = 0; k < 3; ++k) { W_pnt(p[k]); W_dir(d[k]); } bar();
      emit([&] {
        Bnd_BoundSortBox b; run_bsb(s, b);
        for (int k = 0; k < 3; ++k) put_list(b.Compare(gp_Pln(p[k], D(d[k]))));
      });
    }
  };
  run_plnq("bsb_compare_plane", 30, false);
  run_plnq("bsb_compare_plane_gen", 16, true);
}


// =================================================================================
// CSLib
static gp_Vec rvec(double mx = 3.0) { return gp_Vec(rnd(-mx, mx), rnd(-mx, mx), rnd(-mx, mx)); }
static gp_Vec nvec() { return gp_Vec(rint(-1, 1), rint(-1, 1), rint(-1, 1)); }
static gp_Vec vscale(const gp_Vec& v, double k) { return gp_Vec(v.X() * k, v.Y() * k, v.Z() * k); }
static void W_vec3(const gp_Vec& v) { int s = ntok; vec(v); word("vec", s, 3); }
static bool vfinite(const gp_XYZ& d) { return std::isfinite(d.X()) && std::isfinite(d.Y()) && std::isfinite(d.Z()); }
static bool vzero(const gp_Vec& v) { return v.X() == 0 && v.Y() == 0 && v.Z() == 0; }
// the pair of vectors (D1U, D1V) of a case; k selects the situation
static void rpair(int k, gp_Vec& u, gp_Vec& v) {
  u = rvec(); v = rvec();
  if (k % 2 == 1) return;       // half of the cases are generic
  switch ((k / 2) % 13) {
    case 0: u = gp_Vec(0, 0, 0); break;
    case 1: v = gp_Vec(0, 0, 0); break;
    case 2: u = gp_Vec(0, 0, 0); v = gp_Vec(0, 0, 0); break;
    case 3: v = vscale(u, rnz(0.2, 3)); break;                          // parallel
    case 4: v = vscale(u, -rnd(0.2, 3)); break;                         // antiparallel
    case 5: { gp_Vec w = rvec(); v = gp_Vec(u.X() + 1e-4 * w.X(), u.Y() + 1e-4 * w.Y(), u.Z() + 1e-4 * w.Z()); } break; // nearly parallel
    case 6: u = gp_Vec(1e-155, 0, 0); break;                            // squared magnitude <= Resolution
    case 7: u = gp_Vec(1e-150, 0, 0); break;
    case 8: u = nvec(); v = nvec(); break;
    case 9: u = vscale(rvec(), 1e-8); break;
    case 10: u = vscale(rvec(), 1e8); v = vscale(rvec(), 1e-8); break;
    case 11: u = gp_Vec(1, 0, 0); v = gp_Vec(0, 1e-9, 0); break;
    default: u = gp_Vec(1, 0, 0); v = gp_Vec(1, 1e-9, 0); break;
  }
}
static double rtol(int k) {
  static const double T[] = {0.0, 1e-6, 1e-3, 0.5, 1.0, 1e-9, 1e-12, -0.5, 1e-4, 2.0};
  return k % 3 == 0 ? T[(k / 3) % 10] : (coin(0.5) ? T[rint(0, 9)] : rnd(0, 0.2));
}
// Runs `f` with two different initial values of the out-parameter: the normal is `written` when both agree.
struct Wr { bool written; bool finite; gp_Dir d; };
static Wr run_dir(const std::function<void(gp_Dir&)>& f) {
  gp_Dir a(0, 0, 1), b(1, 0, 0);
  f(a); f(b);
  double ca[3] = {a.XYZ().X(), a.XYZ().Y(), a.XYZ().Z()}, cb[3] = {b.XYZ().X(), b.XYZ().Y(), b.XYZ().Z()};
  Wr w; w.written = std::memcmp(ca, cb, sizeof ca) == 0;   // bitwise: a NaN that was written is written
  w.finite = !w.written || vfinite(a.XYZ());
  w.d = a;
  return w;
}
static void put_wr(const Wr& w) { ob(w.written); if (w.written) dir(w.d); }
static int NC = 30;
static void fam_cslib_enums() {
  LOOP("cslib_enum_values", 1) {
    begin(N, ""); bar();
    emit([&] {
      for (int v : {CSLib_Singular, CSLib_Defined, CSLib_InfinityOfSolutions, CSLib_D1NuIsNull, CSLib_D1NvIsNull, CSLib_D1NIsNull, CSLib_D1NuNvRatioIsNull, CSLib_D1NvNuRatioIsNull, CSLib_D1NuIsParallelD1Nv}) in(v);
      for (int v : {CSLib_Done, CSLib_D1uIsNull, CSLib_D1vIsNull, CSLib_D1IsNull, CSLib_D1uD1vRatioIsNull, CSLib_D1vD1uRatioIsNull, CSLib_D1uIsParallelD1v}) in(v);
      for (int v : {(int)CSLib_Class2d::Result_Inside, (int)CSLib_Class2d::Result_Outside, (int)CSLib_Class2d::Result_Uncertain}) in(v);
    });
  }
}
static void fam_cslib_normal_d1() {
  { LOOP("cslib_normal_sin_tol", NC + 12) {
    gp_Vec u, v; rpair(att, u, v); double t = rtol(att);
    if (att % 11 == 7) { u = nvec(); if (vzero(u)) u = gp_Vec(0, 1, 0); v = vscale(u, rint(1, 2)); t = 0.0; }   // exactly parallel, sin tolerance 0: gp_Dir of a null vector
    begin(N, "vec vec f"); W_vec3(u); W_vec3(v); W_f(t); bar();
    emit([&] {
      CSLib_DerivativeStatus st = CSLib_Done;
      Wr w = run_dir([&](gp_Dir& d) { CSLib::Normal(u, v, t, st, d); });
      if (!w.finite) { unchecked(); return; }
      in((int)st); put_wr(w);
    });
  } }
  { LOOP("cslib_normal_mag_tol", NC + 12) {
    gp_Vec u, v; rpair(att, u, v); double t = rtol(att + 1);
    begin(N, "vec vec f"); W_vec3(u); W_vec3(v); W_f(t); bar();
    emit([&] {
      CSLib_NormalStatus st = CSLib_Singular;
      Wr w = run_dir([&](gp_Dir& d) { CSLib::Normal(u, v, t, st, d); });
      if (!w.finite) { unchecked(); return; }
      in((int)st); put_wr(w);
    });
  } }
  { LOOP("cslib_normal_second_order", NC + 40) {
    gp_Vec d1u = rvec(), d1v = rvec(), d2u = rvec(), d2v = rvec(), d2uv = rvec(); double t = rtol(att);
    switch (att % 16) {
      case 0: d2u = gp_Vec(0, 0, 0); d2uv = gp_Vec(0, 0, 0); break;                    // D1Nu = 0
      case 1: d2uv = gp_Vec(0, 0, 0); d2v = gp_Vec(0, 0, 0); break;                    // D1Nv = 0
      case 2: d2u = gp_Vec(0, 0, 0); d2uv = gp_Vec(0, 0, 0); d2v = gp_Vec(0, 0, 0); break; // both null
      case 3: d1u = gp_Vec(0, 0, 0); break;
      case 4: d1v = gp_Vec(0, 0, 0); break;
      case 5: d2u = vscale(d1u, rnz(0.5, 2)); d2uv = vscale(d1v, rnz(0.5, 2)); break;  // D1Nu = D2U x D1V + D1U x D2UV with parallel pieces
      case 6: d1u = gp_Vec(1, 0, 0); d1v = gp_Vec(0, 1, 0); d2u = gp_Vec(0, 0, 1e6); d2uv = gp_Vec(0, 0, 0); d2v = gp_Vec(0, 0, 1e-6); break; // |Nv| << |Nu|
      case 7: d1u = gp_Vec(1, 0, 0); d1v = gp_Vec(0, 1, 0); d2u = gp_Vec(0, 0, 1e-6); d2uv = gp_Vec(0, 0, 0); d2v = gp_Vec(0, 0, 1e6); break; // |Nu| << |Nv|
      case 8: d1u = gp_Vec(1, 0, 0); d1v = gp_Vec(0, 1, 0); d2u = gp_Vec(0, 0, 1); d2uv = gp_Vec(0, 0, 0); d2v = gp_Vec(0, 0, rnz(0.5, 2)); break; // Nu parallel Nv
      case 9: d1u = nvec(); d1v = nvec(); d2u = nvec(); d2v = nvec(); d2uv = nvec(); break;
      case 10: d2uv = vscale(d2u, 0.0); d1u = gp_Vec(0, 1, 0); d1v = gp_Vec(1, 0, 0); d2u = gp_Vec(0, 0, 1); d2v = gp_Vec(0, 0, -1); break;
      default: break;
    }
    begin(N, "vec vec vec vec vec f"); W_vec3(d1u); W_vec3(d1v); W_vec3(d2u); W_vec3(d2v); W_vec3(d2uv); W_f(t); bar();
    emit([&] {
      CSLib_NormalStatus st = CSLib_Singular; bool done = true;
      Wr w = run_dir([&](gp_Dir& d) { CSLib::Normal(d1u, d1v, d2u, d2v, d2uv, t, done, st, d); });
      if (!w.finite) { unchecked(); return; }
      ob(done); in((int)st); put_wr(w);
    });
  } }
}

struct A2 { int rlo, clo, nr, nc; std::vector<gp_Vec> v; };
static A2 mkA2s(int rlo, int clo, int nr, int nc, const std::function<gp_Vec(int, int)>& f) {
  A2 a; a.rlo = rlo; a.clo = clo; a.nr = nr; a.nc = nc;
  for (int r = 0; r < nr; ++r) for (int c = 0; c < nc; ++c) a.v.push_back(f(rlo + r, clo + c));
  return a;
}
static void put_a2(const A2& a) { in(a.rlo); in(a.clo); in(a.nr); in(a.nc); for (const gp_Vec& v : a.v) vec(v); }
static void W_a2(const A2& a) { int st = ntok; put_a2(a); word("a2", st, -1); }
static NCollection_Array2<gp_Vec> toArr(const A2& a) {
  NCollection_Array2<gp_Vec> r(a.rlo, a.rlo + a.nr - 1, a.clo, a.clo + a.nc - 1);
  for (int i = 0; i < a.nr; ++i) for (int j = 0; j < a.nc; ++j) r.SetValue(a.rlo + i, a.clo + j, a.v[i * a.nc + j]);
  return r;
}
static bool covers(const A2& a, int r0, int r1, int c0, int c1) { return a.rlo <= r0 && r1 <= a.rlo + a.nr - 1 && a.clo <= c0 && c1 <= a.clo + a.nc - 1; }
static gp_Vec rv() {
  if (coin(0.15)) return nvec();
  if (coin(0.1)) return gp_Vec(0, 0, 0);
  return rvec(coin(0.5) ? 2.0 : 5.0);
}
// derivative array of a surface for DNNUV/DNNormal: rows rlo.., columns clo.. with the given extent
static A2 rder(int r0, int r1, int c0, int c1, int att) {
  // bounds: exactly the needed range, widened, shifted or (sometimes) too small
  int kind = att % 11;
  int rlo = r0, rhi = r1, clo = c0, chi = c1;
  if (kind == 1 || kind == 2) { rlo = r0 - rint(1, 2); rhi = r1 + rint(0, 2); clo = c0 - rint(0, 2); chi = c1 + rint(0, 1); }
  if (kind == 3) { rhi = r1 + 2; chi = c1 + 2; }
  if (kind == 9) { rhi = r1 - 1; }          // one row too few
  if (kind == 10) { chi = c1 - 1; }         // one column too few
  if (rhi < rlo) rhi = rlo;
  if (chi < clo) chi = clo;
  return mkA2s(rlo, clo, rhi - rlo + 1, chi - clo + 1, [&](int, int) { return rv(); });
}
static void fam_cslib_derivs() {
  { LOOP("cslib_dnnuv", NC + 12) {
    int nu = rint(0, 3), nv = rint(0, 3); if (att % 17 == 5) nu = -1; if (att % 17 == 6) nv = -2;
    A2 a = rder(0, std::max(nu, 0) + 1, 0, std::max(nv, 0) + 1, att);
    begin(N, "i i a2"); W_i(nu); W_i(nv); W_a2(a); bar();
    emit([&] {
      if (nu >= 0 && nv >= 0 && !covers(a, 0, nu + 1, 0, nv + 1)) { unchecked(); return; }
      vec(CSLib::DNNUV(nu, nv, toArr(a)));
    });
  } }
  { LOOP("cslib_dnnuv_two", NC + 12) {
    int nu = rint(0, 3), nv = rint(0, 3); if (att % 17 == 5) nu = -1;
    A2 a = rder(1, std::max(nu, 0) + 1, 0, std::max(nv, 0), att), b = rder(0, std::max(nu, 0), 1, std::max(nv, 0) + 1, att / 11 + 3);
    begin(N, "i i a2 a2"); W_i(nu); W_i(nv); W_a2(a); W_a2(b); bar();
    emit([&] {
      if (nu >= 0 && nv >= 0 && (!covers(a, 1, nu + 1, 0, nv) || !covers(b, 0, nu, 1, nv + 1))) { unchecked(); return; }
      vec(CSLib::DNNUV(nu, nv, toArr(a), toArr(b)));
    });
  } }
  { LOOP("cslib_dnnormal", NC + 12) {
    int nu = rint(0, 3), nv = rint(0, 3), iu = coin(0.6) ? 0 : rint(0, 2), iv = coin(0.6) ? 0 : rint(0, 2);
    A2 a = rder(iu, nu + iu, iv, nv + iv, att);
    if (att % 13 == 4) { // a vanishing first derivative: the null-magnitude check is compiled out
      a = mkA2s(a.rlo, a.clo, a.nr, a.nc, [&](int r, int c) { return (r == iu && c == iv) ? gp_Vec(0, 0, 0) : rv(); });
    }
    begin(N, "i i a2 i i"); W_i(nu); W_i(nv); W_a2(a); W_i(iu); W_i(iv); bar();
    emit([&] {
      if (!covers(a, iu, nu + iu, iv, nv + iv)) { unchecked(); return; }
      gp_Vec r = CSLib::DNNormal(nu, nv, toArr(a), iu, iv);
      if (!vfinite(r.XYZ())) { unchecked(); return; }
      vec(r);
    });
  } }
}

// CSLib::Normal with the derivatives up to MaxOrder
struct Ord { int maxorder; A2 a; double magtol, u, v, umin, umax, vmin, vmax; };
static Ord rord(int att) {
  Ord o; o.maxorder = att % 29 == 0 ? -1 : rint(0, 4);
  int m = std::max(o.maxorder, 0);
  int rlo = 0, clo = 0, nr = m + 1, nc = m + 1;
  if (att % 7 == 3) { nr = m + 3; nc = m + 2; }
  if (att % 19 == 7 && m >= 1) nr = m;                // too few rows
  if (att % 23 == 9) { rlo = -1; nr += 1; }
  int k0 = o.maxorder <= 0 ? 0 : rint(0, o.maxorder);
  int mode = att % 6;               // 0 generic, 1 leading zeros, 2 parallel at order k0, 3 all zero, 4 parallel with sign changes, 5 nearly zero
  gp_Vec base = coin(0.5) ? nvec() : rvec(); if (vzero(base)) base = gp_Vec(0, 0, 1);
  std::vector<double> coef; for (int i = 0; i < 8; ++i) coef.push_back(coin(0.2) ? 0.0 : (mode == 4 ? rnz(0.3, 2) : rnd(0.3, 2)));
  o.a = mkA2s(rlo, clo, nr, nc, [&](int r, int c) {
    int ord = r + c;
    if (mode == 0) return rv();
    if (mode == 3) return gp_Vec(0, 0, 0);
    if (mode == 5) return ord == 0 ? gp_Vec(1e-9, 0, 0) : (ord == 1 ? vscale(base, 1e-9) : rv());
    if (ord < k0) return gp_Vec(0, 0, 0);
    if (mode == 1) return rv();
    if (ord == k0) return vscale(base, coef[(c < 8 ? c : 7)] * (coin(0.1) ? -1.0 : 1.0));
    return rv();
  });
  static const double MT[] = {1e-6, 1e-6, 0.0, 1e-12, 1e-3, 0.1, 1.0};
  o.magtol = MT[rint(0, 6)];
  o.umin = coin(0.5) ? 0.0 : -1.0; o.umax = o.umin + (coin(0.5) ? 1.0 : 3.0); o.vmin = coin(0.5) ? 0.0 : 2.0; o.vmax = o.vmin + (coin(0.5) ? 1.0 : 2.0);
  auto pickp = [&](double lo, double hi) { int k = rint(0, 5); return k == 0 ? lo : (k == 1 ? hi : (k == 2 ? lo + 1e-12 : (k == 3 ? hi - 1e-12 : 0.5 * (lo + hi)))); };
  o.u = pickp(o.umin, o.umax); o.v = pickp(o.vmin, o.vmax);
  return o;
}
static void fam_cslib_normal_order() {
  LOOP("cslib_normal_order", NC + 80) {
    Ord o = rord(att);
    begin(N, "i a2 f f f f f f f"); W_i(o.maxorder); W_a2(o.a); W_f(o.magtol); W_f(o.u); W_f(o.v); W_f(o.umin); W_f(o.umax); W_f(o.vmin); W_f(o.vmax); bar();
    emit([&] {
      if (o.maxorder >= 0 && !covers(o.a, 0, o.maxorder, 0, o.maxorder)) { unchecked(); return; }
      NCollection_Array2<gp_Vec> arr = toArr(o.a);
      CSLib_NormalStatus st = CSLib_Singular; int ou = -77, ov = -77;
      Wr w = run_dir([&](gp_Dir& d) { CSLib::Normal(o.maxorder, arr, o.magtol, o.u, o.v, o.umin, o.umax, o.vmin, o.vmax, st, d, ou, ov); });
      if (!w.finite) { unchecked(); return; }
      in((int)st); put_wr(w); in(ou); in(ov);
    }, true);
  }
}

// CSLib_Class2d
struct C2 { int kind, lo; std::vector<gp_Pnt2d> p; double tolu, tolv, umin, vmin, umax, vmax; };
static CSLib_Class2d mkC2(const C2& s) {
  switch (s.kind) {
    case 1: {
      NCollection_Array1<gp_Pnt2d> a(s.lo, s.lo + (int)s.p.size() - 1);
      for (size_t i = 0; i < s.p.size(); ++i) a.SetValue(s.lo + (int)i, s.p[i]);
      return CSLib_Class2d(a, s.tolu, s.tolv, s.umin, s.vmin, s.umax, s.vmax);
    }
    case 2: {
      NCollection_Sequence<gp_Pnt2d> q;
      for (const gp_Pnt2d& p : s.p) q.Append(p);
      return CSLib_Class2d(q, s.tolu, s.tolv, s.umin, s.vmin, s.umax, s.vmax);
    }
    case 3: {
      NCollection_DynamicArray<gp_Pnt2d> q;
      for (const gp_Pnt2d& p : s.p) q.Append(p);
      return CSLib_Class2d(q, s.tolu, s.tolv, s.umin, s.vmin, s.umax, s.vmax);
    }
    default: return CSLib_Class2d();
  }
}
static void W_c2d(const C2& s) {
  int st = ntok; in(s.kind);
  if (s.kind > 0) {
    if (s.kind == 1) in(s.lo);
    in((long)s.p.size()); for (const gp_Pnt2d& p : s.p) pnt2d(p);
    h(s.tolu); h(s.tolv); h(s.umin); h(s.vmin); h(s.umax); h(s.vmax);
  }
  word("c2d", st, -1);
}
static const int NC2POLY = 12;
static C2 rc2(int att) {
  C2 s; s.kind = att % 17 == 0 ? 0 : 1 + att % 3; s.lo = coin(0.6) ? 1 : (coin(0.5) ? 0 : 3);
  int bsel = rint(0, 5);
  s.umin = bsel < 3 ? 0.0 : -2.0; s.umax = s.umin + (bsel == 0 ? 1.0 : (bsel == 1 ? 4.0 : (bsel == 2 ? 0.5 : 5.0)));
  s.vmin = bsel < 2 ? 0.0 : 1.0; s.vmax = s.vmin + (bsel == 0 ? 2.0 : (bsel == 1 ? 1.0 : 3.0));
  if (att % 13 == 6) s.umax = s.umin;                         // empty U range: count 0
  if (att % 13 == 7) s.vmax = s.vmin - 1.0;                   // reversed V range
  if (att % 29 == 8) { s.umax = s.umin + 1e-11; }             // range below the normalization threshold
  static const double TL[] = {0.0, 1e-7, 1e-3, 0.05, 0.2};
  s.tolu = TL[rint(0, 4)]; s.tolv = coin(0.5) ? s.tolu : TL[rint(0, 4)];
  double du = s.umax - s.umin, dv = s.vmax - s.vmin;
  auto P = [&](double fu, double fv) { return gp_Pnt2d(s.umin + fu * du, s.vmin + fv * dv); };
  int poly = att % NC2POLY;
  switch (poly) {
    case 0: s.p = {P(0, 0), P(1, 0), P(1, 1), P(0, 1)}; break;                                      // the whole rectangle
    case 1: s.p = {P(0.1, 0.1), P(0.9, 0.2), P(0.5, 0.9)}; break;                                   // triangle
    case 2: { int n = rint(5, 12); double cu = 0.5, cv = 0.5; for (int i = 0; i < n; ++i) { double a = 6.283185307179586 * i / n, r = rnd(0.2, 0.48); s.p.push_back(P(cu + r * std::cos(a), cv + r * std::sin(a))); } } break; // star
    case 3: s.p = {P(0.2, 0.2), P(0.8, 0.2), P(0.8, 0.8), P(0.2, 0.8), P(0.2, 0.2)}; break;         // closed
    case 4: { int n = rint(4, 8); for (int i = 0; i < n; ++i) s.p.push_back(P(rnd(0, 1), rnd(0, 1))); } break; // random (self-intersecting)
    case 5: s.p = {P(0.1, 0.1), P(0.5, 0.5), P(0.9, 0.9), P(0.3, 0.3)}; break;                      // collinear
    case 6: s.p = {P(-0.5, -0.5), P(1.5, -0.2), P(0.5, 1.7)}; break;                                 // larger than the bounds
    case 7: s.p = {P(0.5, 0.5), P(0.5, 0.5), P(0.5, 0.5)}; break;                                    // all equal
    case 8: s.p = {P(0.5, 0.5), P(0.5 + 1e-9, 0.5), P(0.5, 0.5 + 1e-9)}; break;                       // tiny
    case 9: s.p = {P(0.2, 0.2), P(0.8, 0.8)}; break;                                                 // two points: count 0
    case 10: s.p = {P(0, 0), P(1, 0), P(1, 1), P(0.5, 0.5), P(0, 1)}; break;                         // concave, touching the diagonal
    default: s.p = {P(0, 0.5), P(0.5, 0), P(1, 0.5), P(0.5, 1)}; break;                              // diamond with vertices on the bounds
  }
  return s;
}
static gp_Pnt2d qpt(const C2& s) {
  double du = s.umax - s.umin, dv = s.vmax - s.vmin;
  int n = (int)s.p.size();
  int k = rint(0, 11);
  if (n >= 3 && k < 4) {
    const gp_Pnt2d& a = s.p[rint(0, n - 1)]; const gp_Pnt2d& b = s.p[rint(0, n - 1)];
    double off = k == 0 ? 0.0 : (k == 1 ? 1e-9 : (k == 2 ? -1e-6 : 1e-3));
    double t = coin(0.4) ? 0.0 : (coin(0.5) ? 0.5 : rnd(0, 1));
    return gp_Pnt2d(a.X() + (b.X() - a.X()) * t + off * du, a.Y() + (b.Y() - a.Y()) * t + off * dv * (coin(0.5) ? 1 : 0));
  }
  if (k == 4) return gp_Pnt2d(s.umin, s.vmin + rnd(0, 1) * dv);
  if (k == 5) return gp_Pnt2d(s.umax + (coin(0.5) ? 0.0 : 1e-8), s.vmax);
  if (k == 6) return gp_Pnt2d(s.umin - 1e-4, s.vmin + 0.5 * dv);
  if (k == 7) return gp_Pnt2d(s.umin + du * 0.5, s.vmax + 0.3);
  return gp_Pnt2d(s.umin + rnd(0, 1) * du, s.vmin + rnd(0, 1) * dv);
}
static int NK = 36;
static void fam_class2d() {
  { LOOP("class2d_default", 18) {
    gp_Pnt2d p = rpnt2d(); double t = att % 3 == 0 ? 0.0 : rnd(0, 1);
    begin(N, "pnt2d f"); W_pnt2d(p); W_f(t); bar();
    emit([&] { CSLib_Class2d c; in((int)c.SiDans(p)); in((int)c.SiDans_OnMode(p, t)); });
  } }
  { LOOP("class2d_si_dans", NK) {
    C2 s = rc2(att); gp_Pnt2d q[4]; for (int k = 0; k < 4; ++k) q[k] = qpt(s);
    begin(N, "c2d pnt2d pnt2d pnt2d pnt2d"); W_c2d(s); for (int k = 0; k < 4; ++k) W_pnt2d(q[k]); bar();
    emit([&] { CSLib_Class2d c = mkC2(s); for (int k = 0; k < 4; ++k) in((int)c.SiDans(q[k])); });
  } }
  { LOOP("class2d_si_dans_on_mode", NK) {
    C2 s = rc2(att + 5); gp_Pnt2d q[4]; for (int k = 0; k < 4; ++k) q[k] = qpt(s);
    double t = att % 5 == 0 ? 0.0 : (att % 5 == 1 ? 1e-7 : (att % 5 == 2 ? 0.01 : (att % 5 == 3 ? 0.3 : rnd(0, 0.1))));
    begin(N, "c2d f pnt2d pnt2d pnt2d pnt2d"); W_c2d(s); W_f(t); for (int k = 0; k < 4; ++k) W_pnt2d(q[k]); bar();
    emit([&] { CSLib_Class2d c = mkC2(s); for (int k = 0; k < 4; ++k) in((int)c.SiDans_OnMode(q[k], t)); });
  } }
  { LOOP("class2d_internal", NK) {
    C2 s = rc2(att + 3); s.kind = s.kind == 0 ? 1 : s.kind;
    gp_Pnt2d q[4]; for (int k = 0; k < 4; ++k) { gp_Pnt2d p = qpt(s); q[k] = gp_Pnt2d((p.X() - s.umin) / std::max(s.umax - s.umin, 1e-3), (p.Y() - s.vmin) / std::max(s.vmax - s.vmin, 1e-3)); }
    begin(N, "c2d pnt2d pnt2d pnt2d pnt2d"); W_c2d(s); for (int k = 0; k < 4; ++k) W_pnt2d(q[k]); bar();
    emit([&] {
      bool empty = s.umax <= s.umin || s.vmax <= s.vmin || s.p.size() < 3;   // no polygon stored: the arrays are empty
      if (empty) { unchecked(); return; }
      CSLib_Class2d c = mkC2(s);
      for (int k = 0; k < 4; ++k) { ob(c.internalSiDans(q[k].X(), q[k].Y())); in((int)c.internalSiDansOuOn(q[k].X(), q[k].Y())); }
    });
  } }
}

// CSLib_NormalPolyDef
static void fam_poly_def() {
  LOOP("cslib_normal_poly_def", NC + 12) {
    int k0 = att % 9 == 0 ? 0 : rint(1, 5);
    int lo = att % 7 == 3 ? -1 : 0, n = k0 + 1 + (att % 5 == 2 ? 2 : 0) + (lo < 0 ? 1 : 0); if (att % 11 == 6 && lo == 0) n = k0;   // n = k0: one value too few
    std::vector<double> li; for (int i = 0; i < n; ++i) li.push_back(coin(0.2) ? 0.0 : (coin(0.4) ? nice() : rnd(-3, 3)));
    double x = att % 8 == 0 ? 0.0 : (att % 8 == 1 ? 0.7853981633974483 : (att % 8 == 2 ? 3.141592653589793 : (att % 8 == 3 ? -1.2 : rnd(-6.3, 6.3))));
    begin(N, "i la f");
    W_i(k0);
    { int st = ntok; in(lo); in(n); for (double v : li) h(v); word("la", st, -1); }
    W_f(x); bar();
    emit([&] {
      if (lo > 0 || lo + n - 1 < k0) { unchecked(); return; }
      NCollection_Array1<double> a(lo, lo + n - 1); for (int i = 0; i < n; ++i) a.SetValue(lo + i, li[i]);
      CSLib_NormalPolyDef f(k0, a);
      double v = 11.5, d = 12.5, v2 = 13.5, d2 = 14.5;
      bool o1 = f.Value(x, v), o2 = f.Derivative(x, d), o3 = f.Values(x, v2, d2);
      ob(o1); h(v); ob(o2); h(d); ob(o3); h(v2); h(d2);
    }, true);
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
  auto want = [&](const char* n) {
    if (!only) return true;
    std::string o = std::string(",") + only + ",", k = std::string(",") + n + ",";
    return o.find(k) != std::string::npos;
  };
  if (want("box")) { fam_box_basic(); fam_box_queries(); }
  if (want("box2d")) fam_box2d();
  if (want("range")) fam_range();
  if (want("sphere")) fam_sphere();
  if (want("obb")) fam_obb();
  if (want("bsb")) fam_bsb();
  if (want("cslib")) { fam_cslib_enums(); fam_cslib_normal_d1(); fam_cslib_derivs(); fam_cslib_normal_order(); fam_class2d(); fam_poly_def(); }
  for (const std::string& n : fam_order)
    std::fprintf(stderr, "%-32s %5d lines, %5d dropped (%d uninit)\n", n.c_str(), fam_lines[n], fam_dropped.count(n) ? fam_dropped[n] : 0,
                 fam_dropped.count(n + "!uninit") ? fam_dropped[n + "!uninit"] : 0);
  for (auto& kv : fam_dropped) if (kv.first.find("!libm") != std::string::npos) std::fprintf(stderr, "   libm-perturbed: %s %d dropped\n", kv.first.c_str(), kv.second);
  return 0;
}
