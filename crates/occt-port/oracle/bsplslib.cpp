// Deterministic reference calls for OCCT 8.0.1's BSplSLib package (B-spline surface evaluation and modification):
// every public function of `BSplSLib`, the class `BSplSLib_Cache`, the evaluator callback of `FunctionMultiply`, and
// the BSplCLib functions that BSplSLib calls and that src/bsplclib.rs does not have (families `bsplclib_*`).
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints (and bools) are decimal. Every input of a call is printed on the
// left of " | " in the order named by the family's schema string (`begin(name, schema)`); tests/bsplslib.rs reads
// them back with the reader method of the same name as each schema word. The oracle aborts if the words it printed
// differ from the schema (and if a word printed the wrong number of tokens). Outputs are values, `throws` (OCCT raised
// an exception: only explicit `throw` statements of the library count) or `unchecked` (the call is invalid, OCCT 8.0.1
// as built has the check compiled out or has none, and the numbers it returns, if it returns at all, are unspecified;
// the Rust API must return an error). Where evaluating an unchecked call would read or write outside an array,
// dereference a null pointer or read uninitialised memory, the oracle does not call it and prints `unchecked` directly.
//
// Every call is executed twice, with the stack filled with 0x00 and with 0xFF before the call; the oracle aborts if the
// two runs print different bits (the call would read uninitialised memory). The oracle was also run under valgrind.
//
// ARRAYS. All OCCT arrays have lower bound 1 (rows and columns of 2D arrays, knots, multiplicities): a Rust slice is
// indexed from 0 and "element k of the OCCT array" is slice[k - 1]; every index argument (UIndex, Index, ...) is the
// OCCT, 1-based, one. 2D arrays are row-major with OCCT's ColLength() = number of rows (the U or first index) and
// RowLength() = number of columns (the V or second index).
//
// Schema words (token counts): f i = 1; fs = int n + n floats; is = int n + n ints; arr2 = rows cols + rows*cols floats;
// p3a2 = rows cols + rows*cols*3 floats (points); ofs / ois / oarr2 = flag (0 or 1) and when 1 an fs / is / arr2 (ofs is unused);
// vec = 3 floats (a gp_Vec, printed raw); evf = kind + 4 floats (the evaluator function of FunctionMultiply);
// surf = udeg vdeg urat vrat uper vper (ints) poles(p3a2) weights(oarr2) uknots(fs) vknots(fs) umults(ois) vmults(ois).
//
// Every output float is finite: the oracle aborts on NaN or infinity.
#include <BSplCLib.hxx>
#include <BSplSLib.hxx>
#include <BSplSLib_Cache.hxx>
#include <BSplSLib_EvaluatorFunction.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <Standard_Failure.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>
#include <math_Matrix.hxx>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <set>
#include <string>
#include <vector>

typedef std::vector<double> Vd;
typedef std::vector<int>    Vi;
typedef NCollection_Array1<double> A1d;
typedef NCollection_Array1<int>    A1i;
typedef NCollection_Array2<gp_Pnt> A2p;
typedef NCollection_Array2<double> A2d;

// ---------------------------------------------------------------------------------
// deterministic random numbers
static uint64_t state = 0x9E3779B97F4A7C15ull;
static double rnd(double lo, double hi) {
  state ^= state << 13; state ^= state >> 7; state ^= state << 17;
  return lo + (hi - lo) * (double)(state >> 11) / 9007199254740992.0;
}
static double rnz(double lo, double hi) { double v = rnd(lo, hi); return rnd(0, 1) < 0.5 ? -v : v; }
static int rint(int lo, int hi) { int v = lo + (int)std::floor(rnd(0, 1) * (hi - lo + 1)); return v > hi ? hi : v; }
static bool coin(double p = 0.5) { return rnd(0, 1) < p; }
static Vd rvec(int n, double lo, double hi) { Vd v; for (int i = 0; i < n; ++i) v.push_back(rnd(lo, hi)); return v; }

// ---------------------------------------------------------------------------------
// record buffer
static std::string buf, cur_name, cur_schema;
static size_t mark = 0;
static int ntok = 0;
static std::vector<std::string> got_words;
static std::set<std::string> seen;
static std::map<std::string, int> fam_lines;
static std::vector<std::string> fam_order;

static void h(double v) {
  if (!std::isfinite(v)) { std::fprintf(stderr, "non-finite float in %s\n", buf.c_str()); std::abort(); }
  uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits);
  buf += t; ++ntok;
}
static void in(long v) { buf += " " + std::to_string(v); ++ntok; }

static void word(const char* w, int start, int expect) {
  got_words.push_back(w);
  if (expect >= 0 && ntok - start != expect) {
    std::fprintf(stderr, "word %s printed %d tokens, expected %d in %s\n", w, ntok - start, expect, cur_name.c_str());
    std::abort();
  }
}
static void W_f(double v) { int s = ntok; h(v); word("f", s, 1); }
static void W_i(long v) { int s = ntok; in(v); word("i", s, 1); }
static void put_fs(const Vd& v) { in((long)v.size()); for (double x : v) h(x); }
static void put_is(const Vi& v) { in((long)v.size()); for (int x : v) in(x); }
static void W_fs(const Vd& v) { int s = ntok; put_fs(v); word("fs", s, 1 + (int)v.size()); }
static void W_is(const Vi& v) { int s = ntok; put_is(v); word("is", s, 1 + (int)v.size()); }
static void W_vec(const gp_Vec& v) { int s = ntok; h(v.X()); h(v.Y()); h(v.Z()); word("vec", s, 3); }
// row-major 2D float array
struct A2 { int rows = 0, cols = 0; Vd v; double& at(int r, int c) { return v[(size_t)r * cols + c]; } double at(int r, int c) const { return v[(size_t)r * cols + c]; } };
static A2 mkA2(int rows, int cols, double lo, double hi) { A2 a; a.rows = rows; a.cols = cols; a.v = rvec(rows * cols, lo, hi); return a; }
static void put_arr2(const A2& a) { in(a.rows); in(a.cols); for (double x : a.v) h(x); }
static void W_arr2(const A2& a) { int s = ntok; put_arr2(a); word("arr2", s, 2 + (int)a.v.size()); }
struct P3A2 { int rows = 0, cols = 0; std::vector<gp_Pnt> v; gp_Pnt& at(int r, int c) { return v[(size_t)r * cols + c]; } const gp_Pnt& at(int r, int c) const { return v[(size_t)r * cols + c]; } };
static P3A2 mkP3A2(int rows, int cols, double lo, double hi) {
  P3A2 a; a.rows = rows; a.cols = cols;
  for (int i = 0; i < rows * cols; ++i) { double x = rnd(lo, hi), y = rnd(lo, hi), z = rnd(lo, hi); a.v.push_back(gp_Pnt(x, y, z)); }
  return a;
}
static void put_p3a2(const P3A2& a) { in(a.rows); in(a.cols); for (const gp_Pnt& p : a.v) { h(p.X()); h(p.Y()); h(p.Z()); } }
static void W_p3a2(const P3A2& a) { int s = ntok; put_p3a2(a); word("p3a2", s, 2 + 3 * (int)a.v.size()); }
// optional arrays: flag, then the array
static void put_ofs(const Vd* v) { if (v) { in(1); put_fs(*v); } else in(0); }
static void put_ois(const Vi* v) { if (v) { in(1); put_is(*v); } else in(0); }
static void put_oarr2(const A2* a) { if (a) { in(1); put_arr2(*a); } else in(0); }
static void W_ofs(const Vd* v) { int s = ntok; put_ofs(v); word("ofs", s, -1); }
static void W_ois(const Vi* v) { int s = ntok; put_ois(v); word("ois", s, -1); }
static void W_oarr2(const A2* a) { int s = ntok; put_oarr2(a); word("oarr2", s, -1); }

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
  if (std::getenv("BSPLSLIB_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
  buf = name; cur_name = name; cur_schema = schema; ntok = 0; got_words.clear();
}
static void bar() {
  if (got_words != split_words(cur_schema)) {
    std::fprintf(stderr, "schema mismatch in %s: printed words differ from '%s'\n", cur_name.c_str(), cur_schema.c_str());
    std::abort();
  }
  buf += " |"; mark = buf.size();
}
static void finish() {
  if (!seen.insert(buf).second) { buf.clear(); return; } // exact duplicate: dropped
  if (!fam_lines.count(cur_name)) fam_order.push_back(cur_name);
  fam_lines[cur_name]++;
  buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear();
}
#define GUARD(...) try { __VA_ARGS__ } catch (Standard_Failure&) { buf.resize(mark); buf += " throws"; }
static void unchecked() { buf.resize(mark); buf += " unchecked"; }

// ---------------------------------------------------------------------------------
// results of one call, and the uninitialised-memory check
struct Tok { bool isf; double f; long i; };
struct Out {
  std::vector<Tok> t;
  void f(double v) { t.push_back({true, v, 0}); }
  void i(long v) { t.push_back({false, 0, v}); }
  void fs(const Vd& v, size_t n) { for (size_t k = 0; k < n; ++k) f(v[k]); }
  void fs(const Vd& v) { fs(v, v.size()); }
  void p(const gp_Pnt& q) { f(q.X()); f(q.Y()); f(q.Z()); }
  void v(const gp_Vec& q) { f(q.X()); f(q.Y()); f(q.Z()); }
  void is(const Vi& v) { for (int x : v) i(x); }
  void a2(const A2d& a) { for (int r = a.LowerRow(); r <= a.UpperRow(); ++r) for (int c = a.LowerCol(); c <= a.UpperCol(); ++c) f(a(r, c)); }
  void p3a2(const A2p& a) { for (int r = a.LowerRow(); r <= a.UpperRow(); ++r) for (int c = a.LowerCol(); c <= a.UpperCol(); ++c) p(a(r, c)); }
  void dims(int rows, int cols) { i(rows); i(cols); }
  void emit() const {
    for (const Tok& k : t) { if (k.isf) h(k.f); else in(k.i); }
  }
};
static bool same(const Out& a, const Out& b) {
  if (a.t.size() != b.t.size()) return false;
  for (size_t k = 0; k < a.t.size(); ++k) {
    if (a.t[k].isf != b.t[k].isf) return false;
    if (a.t[k].isf) { if (std::memcmp(&a.t[k].f, &b.t[k].f, 8) != 0) return false; }
    else if (a.t[k].i != b.t[k].i) return false;
  }
  return true;
}
static __attribute__((noinline)) void dirty(unsigned char c) {
  volatile unsigned char big[1 << 18];
  for (size_t k = 0; k < sizeof big; ++k) big[k] = c;
  __asm__ volatile("" ::: "memory");
}
// Runs `body` twice with differently dirtied stacks; aborts if the results differ.
static void run2(const std::function<void(Out&)>& body) {
  Out a, b;
  if (std::getenv("BSPLSLIB_TRACE")) std::fprintf(stderr, "TRACE %s\n", buf.c_str());
  dirty(0x00); body(a);
  dirty(0xFF); body(b);
  if (!same(a, b)) { std::fprintf(stderr, "uninitialised memory read in %s:%s\n", cur_name.c_str(), buf.c_str()); std::abort(); }
  a.emit();
}

// ---------------------------------------------------------------------------------
// OCCT array builders
static A1d mk1(const Vd& v) { A1d a(1, (int)v.size()); for (size_t k = 0; k < v.size(); ++k) a((int)k + 1) = v[k]; return a; }
static A1i mk1i(const Vi& v) { A1i a(1, (int)v.size()); for (size_t k = 0; k < v.size(); ++k) a((int)k + 1) = v[k]; return a; }
static A2p mk2p(const P3A2& g) {
  A2p a(1, g.rows, 1, g.cols);
  for (int r = 0; r < g.rows; ++r) for (int c = 0; c < g.cols; ++c) a(r + 1, c + 1) = g.at(r, c);
  return a;
}
static A2d mk2d(const A2& g) {
  A2d a(1, g.rows, 1, g.cols);
  for (int r = 0; r < g.rows; ++r) for (int c = 0; c < g.cols; ++c) a(r + 1, c + 1) = g.at(r, c);
  return a;
}
static A2p blank2p(int rows, int cols) { A2p a(1, rows, 1, cols); for (int r = 1; r <= rows; ++r) for (int c = 1; c <= cols; ++c) a(r, c) = gp_Pnt(7.5, 7.5, 7.5); return a; }
static A2d blank2d(int rows, int cols) { A2d a(1, rows, 1, cols); a.Init(7.5); return a; }
static const double SENT = 1234.5678; // sentinel for result buffers

// ---------------------------------------------------------------------------------
// knot vectors
struct KS { Vd k; Vi m; int deg = 1; bool per = false; };
static int ks_np(const KS& s) { A1i m = mk1i(s.m); return BSplCLib::NbPoles(s.deg, s.per, m); }
static void ks_fl(const KS& s, int& fi, int& la) { // first and last usable knot index (1-based)
  if (s.per) { fi = 1; la = (int)s.k.size(); }
  else { A1i m = mk1i(s.m); fi = BSplCLib::FirstUKnotIndex(s.deg, m); la = BSplCLib::LastUKnotIndex(s.deg, m); }
}
static bool ks_valid(const KS& s) {
  if (s.k.size() < 2) return false;
  int np = ks_np(s);
  if (np < 2) return false;
  if (s.per) return np > s.deg;
  int sum = 0; for (int x : s.m) sum += x;
  if (sum < s.deg + 1) return false;
  A1i m = mk1i(s.m);
  int fi = BSplCLib::FirstUKnotIndex(s.deg, m), la = BSplCLib::LastUKnotIndex(s.deg, m);
  return fi >= 1 && la <= (int)s.k.size() && fi < la;
}
// style: 0 simple knots, 1 multiple interior knots, 2 interior knots of multiplicity = degree (C0), 3 mixed,
//        4 tiny spans, 5 large coordinates
static KS mk_ks(int deg, bool per, int nspans, int style, bool unclamped = false) {
  for (int attempt = 0; attempt < 200; ++attempt) {
    KS s; s.deg = deg; s.per = per;
    int nk = nspans + 1 + (per ? attempt / 10 : 0);
    double k = style == 5 ? rnd(-900, 900) : rnd(-3, 3);
    for (int i = 0; i < nk; ++i) {
      s.k.push_back(k);
      double gap = style == 4 ? (coin(0.5) ? rnd(0.001, 0.01) : rnd(0.2, 1.5)) : style == 5 ? rnd(20, 300) : rnd(0.2, 2.0);
      k += gap;
      int m = 1;
      if (style == 1) m = rint(1, deg);
      else if (style == 2) m = deg;
      else if (style == 3) m = coin() ? 1 : deg;
      s.m.push_back(m);
    }
    if (per) {
      int m1 = style == 0 ? 1 : rint(1, deg);
      s.m[0] = m1; s.m[nk - 1] = m1;
    } else if (unclamped) {
      s.m[0] = rint(1, deg + 1); s.m[nk - 1] = rint(1, deg + 1);
    } else {
      s.m[0] = deg + 1; s.m[nk - 1] = deg + 1;
    }
    if (ks_valid(s)) return s;
  }
  KS s; s.deg = deg; s.per = per;
  s.k = {0.0, 1.0}; s.m = {deg + 1, deg + 1};
  if (per) { s.k.clear(); for (int i = 0; i <= deg + 1; ++i) s.k.push_back(i); s.m.assign(deg + 2, 1); }
  return s;
}
static Vd flat_of(const KS& s) {
  A1d k = mk1(s.k); A1i m = mk1i(s.m);
  int n = BSplCLib::KnotSequenceLength(m, s.deg, s.per);
  A1d seq(1, n);
  BSplCLib::KnotSequence(k, m, s.deg, s.per, seq);
  Vd out; for (int i = 1; i <= n; ++i) out.push_back(seq(i));
  return out;
}
// parameter modes: 0 inside, 1 on an inner knot, 2 at an end, 3 outside the range, 4 a few ulps from a knot,
// 5 a tiny distance from a knot, 6 inside but near an end
static double pick_u(const KS& s, int mode) {
  int fi, la; ks_fl(s, fi, la);
  double lo = s.k[fi - 1], hi = s.k[la - 1], span = hi - lo;
  auto knot = [&]() { return s.k[rint(fi - 1, la - 1)]; };
  switch (mode) {
    case 0: return lo + rnd(0, 1) * span;
    case 1: return knot();
    case 2: return coin() ? lo : hi;
    case 3:
      if (s.per) return lo + rnd(-2.5, 3.5) * span;
      return coin() ? lo - rnd(0.05, 0.8) * span : hi + rnd(0.05, 0.8) * span;
    case 4: { double x = knot(); int n = rint(1, 3); bool up = coin(); for (int i = 0; i < n; ++i) x = std::nextafter(x, up ? 1e300 : -1e300); return x; }
    case 5: return knot() + (coin() ? 1 : -1) * rnd(1e-11, 1e-8);
    default: return coin() ? lo + rnd(0, 1e-3) * span : hi - rnd(0, 1e-3) * span;
  }
}
static double pick_any(const KS& s) { return pick_u(s, rint(0, 6)); }

// ---------------------------------------------------------------------------------
// surfaces: the compound schema word `surf`
struct SurfIn {
  int ud = 1, vd = 1;
  bool urat = false, vrat = false, uper = false, vper = false;
  P3A2 poles;
  bool hasW = false; A2 w;
  Vd uk, vk;                  // the knots exactly as given to OCCT (flat knots when the mults are absent)
  bool hasUm = true, hasVm = true;
  Vi um, vm;
  KS u, v;                    // the structure they came from
};
static void put_surf(const SurfIn& s) {
  in(s.ud); in(s.vd); in(s.urat); in(s.vrat); in(s.uper); in(s.vper);
  put_p3a2(s.poles);
  put_oarr2(s.hasW ? &s.w : nullptr);
  put_fs(s.uk); put_fs(s.vk);
  put_ois(s.hasUm ? &s.um : nullptr); put_ois(s.hasVm ? &s.vm : nullptr);
}
static void W_surf(const SurfIn& s) {
  int st = ntok; put_surf(s); word("surf", st, -1);
}
// An OCCT view of a SurfIn
struct OccSurf {
  A2p poles; A2d w; bool hasW; A1d uk, vk; A1i um, vm; bool hasUm, hasVm;
  explicit OccSurf(const SurfIn& s)
      : poles(mk2p(s.poles)), w(s.hasW ? mk2d(s.w) : A2d(1, 1, 1, 1)), hasW(s.hasW), uk(mk1(s.uk)), vk(mk1(s.vk)),
        um(mk1i(s.hasUm ? s.um : Vi{1})), vm(mk1i(s.hasVm ? s.vm : Vi{1})), hasUm(s.hasUm), hasVm(s.hasVm) {}
  const A2d* W() const { return hasW ? &w : nullptr; }
  const A1i* UM() const { return hasUm ? &um : nullptr; }
  const A1i* VM() const { return hasVm ? &vm : nullptr; }
};
// weights: 0 none, 1 random, 2 some tiny, 3 all equal, 4 a locally equal block, 5 random but large range
static void mk_weights(SurfIn& s, int mode) {
  s.hasW = mode != 0;
  if (!s.hasW) return;
  int R = s.poles.rows, C = s.poles.cols;
  s.w = mkA2(R, C, 0.4, 2.5);
  if (mode == 2) { for (int k = 0, n = rint(1, 3); k < n; ++k) s.w.at(rint(0, R - 1), rint(0, C - 1)) = rnd(1e-4, 1e-2); }
  else if (mode == 3) { double c = rnd(0.5, 2.0); for (double& x : s.w.v) x = c; }
  else if (mode == 4) {
    int r0 = rint(0, R - 1), c0 = rint(0, C - 1); double c = rnd(0.5, 2.0);
    for (int r = 0; r < R; ++r) for (int cc = 0; cc < C; ++cc) if (r <= r0 + 1 && cc <= c0 + 1 && r >= r0 && cc >= c0) s.w.at(r, cc) = c;
  } else if (mode == 5) { for (double& x : s.w.v) x = rnd(0.05, 20.0); }
}
// A surface from two knot structures. `uflat` / `vflat` pass flat knots without multiplicities.
static SurfIn mk_surf(const KS& u, const KS& v, int wmode, bool uflat = false, bool vflat = false, double pole_range = 8.0) {
  SurfIn s; s.u = u; s.v = v; s.ud = u.deg; s.vd = v.deg; s.uper = u.per; s.vper = v.per;
  s.poles = mkP3A2(ks_np(u), ks_np(v), -pole_range, pole_range);
  mk_weights(s, wmode);
  s.hasUm = !uflat; s.hasVm = !vflat;
  s.uk = uflat ? flat_of(u) : u.k; s.vk = vflat ? flat_of(v) : v.k;
  s.um = u.m; s.vm = v.m;
  if (wmode == 0) { s.urat = s.vrat = false; }
  else { s.urat = coin(0.7); s.vrat = s.urat ? coin(0.5) : true; }
  return s;
}
// the index OCCT's LocateParameter gives (what a caller passes as UIndex), or 0
static int locate_index(const KS& s, double u, bool flat) {
  int idx = 0; double nu = u;
  A1d k = mk1(flat ? flat_of(s) : s.k); A1i m = mk1i(s.m);
  BSplCLib::LocateParameter(s.deg, k, flat ? BSplCLib::NoMults() : &m, u, s.per, idx, nu);
  return idx;
}
// UIndex argument: 0 (OCCT locates), the located index, or a different usable index of a non-periodic structure
static int pick_index(const KS& s, double u, bool flat, int mode) {
  if (mode == 0) return 0;
  int idx = locate_index(s, u, flat);
  if (mode == 1 || s.per) return idx;
  int fi, la; ks_fl(s, fi, la);
  if (flat) { int n = (int)flat_of(s).size(); return rint(1 + s.deg, n - s.deg - 1); }
  return rint(fi, la - 1);
}

// =================================================================================
// BSplCLib: raw-buffer evaluation
// =================================================================================
static Vd local_knots(int n, bool allow_equal) { // n non-decreasing values
  Vd k; double x = rnd(-2, 2);
  for (int i = 0; i < n; ++i) { k.push_back(x); x += (allow_equal && coin(0.3)) ? 0.0 : rnd(0.1, 1.5); }
  return k;
}
static void emit_eval_buffer(double u, int deg, const Vd& kn, int dim, const Vd& pl) {
  begin("bsplclib_eval_buffer", "f i fs i fs");
  W_f(u); W_i(deg); W_fs(kn); W_i(dim); W_fs(pl); bar();
  if (deg < 0 || dim < 1 || kn.empty() || (long)kn.size() < 2L * deg || (long)pl.size() < (long)(deg + 1) * dim) unchecked();
  else run2([&](Out& o) { Vd k(kn), p(pl); BSplCLib::Eval(u, deg, k[0], dim, p[0]); o.fs(p); });
  finish();
}
static const int DIMS[] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 16, 20};
static void fam_eval_buffer() {
  for (int dim : DIMS)
    for (int k = 0; k < 1; ++k) {
      int deg = rint(1, 9);
      Vd kn = local_knots(2 * deg, false);
      double u = coin(0.75) ? rnd(kn[deg - 1], kn[deg]) : rnd(kn[0] - 1, kn[2 * deg - 1] + 1);
      emit_eval_buffer(u, deg, kn, dim, rvec((deg + 1) * dim, -5, 5));
    }
  for (int deg : {1, 3, 5, 7, 9}) {
    Vd kn = local_knots(2 * deg, false);
    emit_eval_buffer(rnd(kn[deg - 1], kn[deg]), deg, kn, 3, rvec((deg + 1) * 3, -5, 5));
  }
  // exactly on the span ends
  { Vd kn = local_knots(6, false); emit_eval_buffer(kn[2], 3, kn, 3, rvec(12, -5, 5)); emit_eval_buffer(kn[3], 3, kn, 4, rvec(16, -5, 5)); }
  // the maximum degree
  { Vd kn = local_knots(50, false); emit_eval_buffer(rnd(kn[24], kn[25]), 25, kn, 3, rvec(26 * 3, -5, 5)); }
  // degree 0 does nothing
  emit_eval_buffer(0.5, 0, Vd{0.0}, 3, rvec(3, -1, 1));
  // longer arrays: the extra values are not read
  { Vd kn = local_knots(9, false); emit_eval_buffer(rnd(kn[2], kn[3]), 3, kn, 2, rvec(11, -5, 5)); }
  // invalid
  { Vd kn = local_knots(6, false); emit_eval_buffer(0.5, 3, kn, 3, rvec(11, -5, 5)); }   // 11 < 12 poles values
  { Vd kn = local_knots(5, false); emit_eval_buffer(0.5, 3, kn, 3, rvec(12, -5, 5)); }   // 5 < 6 knots
  { Vd kn = local_knots(6, false); emit_eval_buffer(0.5, 3, kn, 0, rvec(12, -5, 5)); }   // dimension 0
  { Vd kn = local_knots(6, false); emit_eval_buffer(0.5, -1, kn, 3, rvec(12, -5, 5)); }  // negative degree
}

static void emit_bohm_buffer(double u, int deg, int n, const Vd& kn, int dim, const Vd& pl) {
  begin("bsplclib_bohm_buffer", "f i i fs i fs");
  W_f(u); W_i(deg); W_i(n); W_fs(kn); W_i(dim); W_fs(pl); bar();
  if (deg < 0 || n < 0 || dim < 1 || kn.empty() || (long)kn.size() < 2L * deg || (long)pl.size() < (long)(deg + 1) * dim) unchecked();
  else run2([&](Out& o) { Vd k(kn), p(pl); BSplCLib::Bohm(u, deg, n, k[0], dim, p[0]); o.fs(p); });
  finish();
}
static void fam_bohm_buffer() {
  for (int dim : DIMS)
    for (int k = 0; k < 1; ++k) {
      int deg = rint(1, 9);
      Vd kn = local_knots(2 * deg, dim % 2 == 0);
      int ns[] = {0, 1, 2, 3, deg, deg + 1, deg + 2, rint(0, 6)};
      emit_bohm_buffer(coin(0.8) ? rnd(kn[deg - 1], kn[deg]) : rnd(kn[0] - 1, kn[2 * deg - 1] + 1), deg, ns[rint(0, 7)], kn, dim,
                       rvec((deg + 1) * dim, -5, 5));
    }
  for (int deg : {1, 3, 5, 9}) {
    Vd kn = local_knots(2 * deg, deg % 2 == 0);
    emit_bohm_buffer(rnd(kn[deg - 1], kn[deg] + 0.1), deg, std::min(3, deg + 1), kn, 3, rvec((deg + 1) * 3, -5, 5));
  }
  // clamped-end knots (equal values), as BuildKnots gives them
  emit_bohm_buffer(0.4, 3, 3, Vd{0, 0, 0, 1, 1, 1}, 3, rvec(12, -5, 5));
  emit_bohm_buffer(0.4, 2, 2, Vd{0, 0, 1, 1}, 4, rvec(12, -5, 5));
  emit_bohm_buffer(0.9, 3, 2, Vd{0, 0, 1, 2, 3, 3}, 6, rvec(24, -5, 5));
  emit_bohm_buffer(1.4, 4, 4, Vd{0, 1, 1, 1, 2, 2, 3, 4}, 5, rvec(25, -5, 5));
  // the maximum degree
  { Vd kn = local_knots(50, false); emit_bohm_buffer(rnd(kn[24], kn[25]), 25, 3, kn, 3, rvec(26 * 3, -5, 5)); }
  // invalid
  { Vd kn = local_knots(6, false); emit_bohm_buffer(0.5, 3, 1, kn, 3, rvec(11, -5, 5)); }
  { Vd kn = local_knots(5, false); emit_bohm_buffer(0.5, 3, 1, kn, 3, rvec(12, -5, 5)); }
  { Vd kn = local_knots(6, false); emit_bohm_buffer(0.5, 3, -1, kn, 3, rvec(12, -5, 5)); }
  { Vd kn = local_knots(6, false); emit_bohm_buffer(0.5, 3, 1, kn, 0, rvec(12, -5, 5)); }
}

// =================================================================================
// BSplCLib: BuildKnots, PoleIndex, LocateParameter variants, knot sequences
// =================================================================================
static void emit_build_knots(int deg, int index, bool per, const Vd& kn, const Vi* mults) {
  begin("bsplclib_build_knots", "i i i fs ois");
  W_i(deg); W_i(index); W_i(per); W_fs(kn); W_ois(mults); bar();
  bool bad = deg < 1 || kn.empty() || (mults && mults->size() != kn.size());
  if (!bad && !mults) bad = index < deg || index + deg > (long)kn.size();
  if (!bad && mults) bad = index < 1 || index > (long)kn.size() - (per ? 0 : 1);
  if (bad) { unchecked(); finish(); return; }
  Vd res(2 * deg, SENT);
  Out o;
  run2([&](Out& oo) {
    A1d k = mk1(kn); A1i m = mk1i(mults ? *mults : Vi{1});
    Vd r(res);
    BSplCLib::BuildKnots(deg, index, per, k, mults ? &m : nullptr, r[0]);
    oo.fs(r); o = oo;
  });
  // the call left a value unwritten: the result is unspecified
  for (const Tok& t : o.t) if (t.f == SENT) { unchecked(); break; }
  finish();
}
static void fam_build_knots() {
  // with multiplicities
  for (int k = 0; k < 14; ++k) {
    int deg = k < 2 ? 25 : rint(1, 9);
    bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 4), k % 4, k % 5 == 4 && !per);
    int fi, la; ks_fl(s, fi, la);
    int idx = per ? rint(1, (int)s.k.size()) : rint(fi, la - 1);
    emit_build_knots(deg, idx, per, s.k, &s.m);
  }
  // flat knots
  for (int deg : {1, 2, 3, 4, 5, 6, 7, 9, 25}) {
    KS s = mk_ks(deg, false, rint(1, 3), 1);
    Vd f = flat_of(s);
    emit_build_knots(deg, rint(deg, (int)f.size() - deg), false, f, nullptr);
  }
  { KS s = mk_ks(3, false, 3, 0); Vd f = flat_of(s); emit_build_knots(3, 3, false, f, nullptr); emit_build_knots(3, (int)f.size() - 3, false, f, nullptr); }
  // periodic index at the last knot, and an index that leaves the knots unwritten
  { KS s = mk_ks(3, true, 4, 1); emit_build_knots(3, (int)s.k.size(), true, s.k, &s.m); }
  { KS s = mk_ks(3, false, 3, 0); emit_build_knots(3, 1, false, s.k, &s.m); }
  { KS s = mk_ks(4, false, 3, 0); emit_build_knots(4, (int)s.k.size(), false, s.k, &s.m); }
  // invalid
  { KS s = mk_ks(3, false, 3, 0); Vd f = flat_of(s); emit_build_knots(3, 2, false, f, nullptr); }
  { KS s = mk_ks(3, false, 3, 0); Vd f = flat_of(s); emit_build_knots(3, (int)f.size() - 1, false, f, nullptr); }
  { KS s = mk_ks(3, false, 3, 0); emit_build_knots(3, 0, false, s.k, &s.m); }
  { KS s = mk_ks(3, false, 3, 0); emit_build_knots(0, 1, false, s.k, &s.m); }
}

static void emit_pole_index(int deg, int index, bool per, const Vi& m) {
  begin("bsplclib_pole_index", "i i i is");
  W_i(deg); W_i(index); W_i(per); W_is(m); bar();
  if (m.empty() || index > (long)m.size()) unchecked();
  else run2([&](Out& o) { A1i mm = mk1i(m); o.i(BSplCLib::PoleIndex(deg, index, per, mm)); });
  finish();
}
static void fam_pole_index() {
  for (int k = 0; k < 22; ++k) {
    int deg = rint(1, 9); bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 5), k % 4, k % 7 == 6 && !per);
    emit_pole_index(deg, rint(1, (int)s.m.size()), per, s.m);
  }
  { KS s = mk_ks(3, false, 3, 1); emit_pole_index(3, 0, false, s.m); emit_pole_index(3, -2, false, s.m); emit_pole_index(3, (int)s.m.size(), false, s.m); }
  { KS s = mk_ks(3, false, 3, 1); emit_pole_index(3, (int)s.m.size() + 1, false, s.m); emit_pole_index(3, 1000, false, s.m); }
}

static void emit_locate_flat(int deg, const Vd& fl, double u, bool per, int idx) {
  begin("bsplclib_locate_parameter_flat", "i fs f i i");
  W_i(deg); W_fs(fl); W_f(u); W_i(per); W_i(idx); bar();
  if (deg < 1 || (long)fl.size() < 2L * deg + 2) unchecked();
  else run2([&](Out& o) {
    A1d k = mk1(fl); int index = idx; double nu = u;
    BSplCLib::LocateParameter(deg, k, BSplCLib::NoMults(), u, per, index, nu);
    o.i(index); o.f(nu);
  });
  finish();
}
static void fam_locate_flat() {
  for (int k = 0; k < 26; ++k) {
    int deg = k < 1 ? 25 : rint(1, 9);
    bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 4), k % 6, k % 9 == 8 && !per);
    Vd f = flat_of(s);
    double u = pick_u(s, k % 7);
    int idx = k % 5 == 4 ? rint(1, (int)f.size()) : (k % 11 == 10 ? -3 : 0);
    emit_locate_flat(deg, f, u, per, idx);
  }
  { KS s = mk_ks(3, false, 3, 0); Vd f = flat_of(s); emit_locate_flat(3, f, f[3], false, 0); emit_locate_flat(3, f, f[(int)f.size() - 4], false, 0); }
  // invalid: too few knots
  { Vd f = local_knots(7, false); emit_locate_flat(3, f, 0.5, false, 0); }
  { Vd f = local_knots(3, false); emit_locate_flat(2, f, 0.5, false, 0); }
  { Vd f = local_knots(8, false); emit_locate_flat(0, f, 0.5, false, 0); }
}

static void emit_locate_range(int deg, const Vd& kn, double u, bool per, int from, int to) {
  begin("bsplclib_locate_parameter_range", "i fs f i i i");
  W_i(deg); W_fs(kn); W_f(u); W_i(per); W_i(from); W_i(to); bar();
  int n = (int)kn.size();
  if (deg < 0 || n < deg + 1 || from < 1 || to < 1 || from > n || to > n || from == to) unchecked();
  else run2([&](Out& o) {
    A1d k = mk1(kn); int index = 0; double nu = u;
    BSplCLib::LocateParameter(deg, k, u, per, from, to, index, nu);
    o.i(index); o.f(nu);
  });
  finish();
}
static void fam_locate_range() {
  for (int k = 0; k < 26; ++k) {
    int deg = rint(1, 9);
    bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 4), k % 6, k % 9 == 8 && !per);
    Vd f = flat_of(s);
    int n = (int)f.size();
    double u = pick_u(s, k % 7);
    int from = deg + 1, to = n - deg;   // what EvalBsplineBasis passes
    if (k % 4 == 3) { from = rint(1, n); to = rint(1, n); if (coin()) std::swap(from, to); }
    emit_locate_range(deg, f, u, per, from, to);
  }
  { KS s = mk_ks(2, false, 3, 0); Vd f = flat_of(s); int n = (int)f.size(); emit_locate_range(2, f, f[2], false, 3, n - 2); emit_locate_range(2, f, f[n - 3], false, 3, n - 2); }
  // invalid
  { Vd f = local_knots(8, false); emit_locate_range(3, f, 0.5, false, 4, 4); emit_locate_range(3, f, 0.5, false, 0, 4); emit_locate_range(3, f, 0.5, false, 4, 9); }
  { Vd f = local_knots(2, false); emit_locate_range(3, f, 0.5, false, 1, 2); }
}

static void emit_knot_seq_len(const Vi& m, int deg, bool per) {
  begin("bsplclib_knot_sequence_length", "is i i");
  W_is(m); W_i(deg); W_i(per); bar();
  if (m.empty()) unchecked();
  else run2([&](Out& o) { A1i mm = mk1i(m); o.i(BSplCLib::KnotSequenceLength(mm, deg, per)); });
  finish();
}
static void fam_knot_seq_len() {
  for (int k = 0; k < 24; ++k) {
    int deg = rint(1, 9); bool per = k % 2 == 1;
    KS s = mk_ks(deg, per, rint(1, 5), k % 4, k % 7 == 6 && !per);
    emit_knot_seq_len(s.m, deg, per);
  }
  emit_knot_seq_len(Vi{3, 1, 3}, 5, true);
  emit_knot_seq_len(Vi{4, 4}, 3, false);
  emit_knot_seq_len(Vi{}, 3, false);
}

static void emit_knot_seq(const Vd& kn, const Vi& m, int deg, bool per) {
  begin("bsplclib_knot_sequence", "fs is i i");
  W_fs(kn); W_is(m); W_i(deg); W_i(per); bar();
  bool bad = kn.size() != m.size() || kn.empty() || deg < 1 || (per && kn.size() < 2) || (per && deg + 1 - m[0] < 0);
  if (!bad) { A1i mm = mk1i(m); bad = BSplCLib::KnotSequenceLength(mm, deg, per) < 1; }
  if (!bad && per) bad = m[0] != m.back();
  if (bad) unchecked();
  else run2([&](Out& o) {
    A1d k = mk1(kn); A1i mm = mk1i(m);
    int n = BSplCLib::KnotSequenceLength(mm, deg, per);
    A1d seq(1, n); seq.Init(SENT);
    BSplCLib::KnotSequence(k, mm, deg, per, seq);
    for (int i = 1; i <= n; ++i) o.f(seq(i));
  });
  finish();
}
static void fam_knot_seq() {
  for (int k = 0; k < 16; ++k) {
    int deg = k < 1 ? 25 : rint(1, 9); bool per = k % 2 == 1;
    KS s = mk_ks(deg, per, rint(1, 5), k % 5, k % 7 == 6 && !per);
    emit_knot_seq(s.k, s.m, deg, per);
  }
  // invalid
  emit_knot_seq(Vd{0, 1, 2}, Vi{1, 1}, 2, false);
  emit_knot_seq(Vd{0, 1, 2}, Vi{4, 1, 4}, 2, true);
  emit_knot_seq(Vd{0, 1, 2}, Vi{3, 1, 4}, 2, true);
}

// =================================================================================
// BSplCLib: B-spline basis, Schoenberg points, interpolation
// =================================================================================
static void emit_basis(int der, int order, const Vd& fl, double u, bool per, int lr, int ur, int lc, int uc) {
  begin("bsplclib_eval_bspline_basis", "i i fs f i i i i i");
  W_i(der); W_i(order); W_fs(fl); W_f(u); W_i(per); W_i(lr); W_i(ur); W_i(lc); W_i(uc); bar();
  if (order < 1 || der < 0 || (long)fl.size() < 2L * order || lr > ur || lc > uc) unchecked();
  else run2([&](Out& o) {
    A1d k = mk1(fl);
    math_Matrix m(lr, ur, lc, uc, 0.25);
    int first = -77;
    int code = BSplCLib::EvalBsplineBasis(der, order, k, u, first, m, per);
    o.i(code); o.i(first);
    for (int r = lr; r <= ur; ++r) for (int c = lc; c <= uc; ++c) o.f(m(r, c));
  });
  finish();
}
static void fam_basis() {
  for (int k = 0; k < 22; ++k) {
    int deg = k < 1 ? 25 : rint(1, 9);
    bool per = k % 5 == 4;
    KS s = mk_ks(deg, per, rint(1, 4), k % 6, k % 9 == 8 && !per);
    Vd f = flat_of(s);
    int order = deg + 1;
    if (k % 7 == 6) order = rint(1, deg + 1);
    if (k % 13 == 12) { // a block of deg+1 equal knots: a zero-length interval
      int p = rint(0, (int)f.size() - deg - 2);
      for (int j = 1; j <= deg; ++j) f[p + j] = f[p];
    }
    int ders[] = {0, 0, 0, 1, 2, 3, deg, deg + 1, order, order + 1};
    int der = deg == 25 ? 0 : ders[rint(0, 9)];
    int local = std::min(der, order - 1);
    int rows = local + 1 + (k % 4 == 3 ? 1 : 0), cols = order + (k % 6 == 5 ? 2 : 0);
    int lr = 1, lc = 1;
    if (k % 10 == 9) { rows -= 1; if (rows < 1) rows = 1; }      // too few rows: code 1
    if (k % 17 == 16) lc = 2;                                    // lower column bound not 1: code 1
    emit_basis(der, order, f, pick_u(s, k % 7), per, lr, lr + rows - 1, lc, lc + cols - 1);
  }
  { KS s = mk_ks(3, false, 3, 0); Vd f = flat_of(s); emit_basis(0, 4, f, f[4], false, 1, 1, 1, 4); emit_basis(3, 4, f, f[4], false, 1, 4, 1, 4); emit_basis(1, 3, f, f[(int)f.size() - 1], false, 1, 2, 1, 3); }
  // invalid
  { Vd f = local_knots(8, false); emit_basis(0, 0, f, 0.5, false, 1, 1, 1, 1); emit_basis(-1, 3, f, 0.5, false, 1, 3, 1, 3); emit_basis(0, 5, f, 0.5, false, 1, 1, 1, 5); }
}

static void emit_schoenberg(int deg, const Vd& fl, int nb) {
  begin("bsplclib_build_schoenberg_points", "i fs i");
  W_i(deg); W_fs(fl); W_i(nb); bar();
  if (deg < 1 || nb < 1 || (long)fl.size() < (long)deg + nb) unchecked();
  else run2([&](Out& o) {
    A1d k = mk1(fl); A1d p(1, nb); p.Init(SENT);
    BSplCLib::BuildSchoenbergPoints(deg, k, p);
    for (int i = 1; i <= nb; ++i) o.f(p(i));
  });
  finish();
}
static void fam_schoenberg() {
  for (int k = 0; k < 18; ++k) {
    int deg = k < 1 ? 25 : rint(1, 9);
    KS s = mk_ks(deg, false, rint(1, 4), k % 5, k % 7 == 6);
    Vd f = flat_of(s);
    int nb = (int)f.size() - deg - 1;
    if (k % 4 == 3) nb = rint(1, (int)f.size() - deg);
    emit_schoenberg(deg, f, nb);
  }
  // invalid
  { Vd f = local_knots(8, false); emit_schoenberg(3, f, 0); emit_schoenberg(3, f, 6); emit_schoenberg(0, f, 3); }
}

// Is every row of the interpolation matrix written inside its band? (otherwise BuildBSpMatrix writes outside the matrix)
static int interp_state(int deg, const Vd& fl, const Vd& par, const Vi& co) { // 0 ok, 1 unsafe (unchecked), 2 error (OCCT throws)
  int order = deg + 1, n = (int)par.size();
  A1d k = mk1(fl);
  for (int i = 1; i <= n; ++i) {
    int c = co[i - 1];
    if (c < 0 || c > deg) return 1;
    math_Matrix m(1, order + 1, 1, order + 1);
    int first = 0;
    int code = BSplCLib::EvalBsplineBasis(c, order, k, par[i - 1], first, m, false);
    if (code != 0) return 2;
    int index = deg + 1 + first - i;
    if (index < 1 || index + order - 2 > 2 * deg) return 1;
  }
  return 0;
}
static void emit_interpolate(int deg, const Vd& fl, const Vd& par, const Vi& co, int dim, const Vd& pl) {
  begin("bsplclib_interpolate", "i fs fs is i fs");
  W_i(deg); W_fs(fl); W_fs(par); W_is(co); W_i(dim); W_fs(pl); bar();
  bool bad = deg < 1 || dim < 1 || par.empty() || co.size() != par.size() || (long)pl.size() < (long)par.size() * dim ||
             (long)fl.size() != (long)par.size() + deg + 1;
  if (!bad) bad = interp_state(deg, fl, par, co) == 1;
  if (bad) { unchecked(); finish(); return; }
  GUARD(
    run2([&](Out& o) {
      A1d k = mk1(fl), p = mk1(par); A1i c = mk1i(co);
      Vd poles(pl); int inv = -9;
      BSplCLib::Interpolate(deg, k, p, c, dim, poles[0], inv);
      o.i(inv); o.fs(poles);
    });
  )
  finish();
}
static void fam_interpolate() {
  for (int k = 0; k < 17; ++k) {
    int deg = k < 1 ? 25 : rint(1, 9);
    KS s = mk_ks(deg, false, rint(1, 3), k % 3 == 0 ? 0 : k % 3);
    Vd f = flat_of(s);
    int n = (int)f.size() - deg - 1;
    A1d fk = mk1(f); A1d ps(1, n); BSplCLib::BuildSchoenbergPoints(deg, fk, ps);
    Vd par; for (int i = 1; i <= n; ++i) par.push_back(ps(i));
    if (k % 4 == 3) for (size_t i = 1; i + 1 < par.size(); ++i) par[i] += rnd(-0.02, 0.02) * (par[i + 1] - par[i - 1]); // keeps the order
    Vi co(n, 0);
    if (k % 6 == 5) for (int i = 0; i < n; ++i) co[i] = coin(0.3) ? rint(1, std::min(deg, 2)) : 0;
    int dims[] = {1, 2, 3, 3, 4, 6, 8};
    int dim = dims[k % 7];
    emit_interpolate(deg, f, par, co, dim, rvec(n * dim, -5, 5));
  }
  // not a spline interpolation problem: unordered / equal parameters give a singular system (OCCT throws)
  { KS s = mk_ks(3, false, 2, 0); Vd f = flat_of(s); int n = (int)f.size() - 4;
    Vd par; for (int i = 0; i < n; ++i) par.push_back(f[3] + (f[f.size() - 4] - f[3]) * i / (n - 1));
    Vd p2(par); p2[1] = p2[0]; emit_interpolate(3, f, p2, Vi(n, 0), 3, rvec(n * 3, -5, 5)); }
  // invalid
  { KS s = mk_ks(3, false, 2, 0); Vd f = flat_of(s); int n = (int)f.size() - 4;
    Vd par; for (int i = 0; i < n; ++i) par.push_back(f[3] + (f[f.size() - 4] - f[3]) * i / (n - 1));
    emit_interpolate(3, f, par, Vi(n, 0), 3, rvec(n * 3 - 1, -5, 5));          // short poles
    emit_interpolate(3, f, par, Vi(n - 1, 0), 3, rvec(n * 3, -5, 5));          // short contact orders
    Vi bad(n, 0); bad[2] = 5; emit_interpolate(3, f, par, bad, 3, rvec(n * 3, -5, 5)); // contact order above the degree
    Vd p3(par); std::reverse(p3.begin(), p3.end()); emit_interpolate(3, f, p3, Vi(n, 0), 3, rvec(n * 3, -5, 5)); // decreasing parameters
  }
}

// =================================================================================
// BSplCLib: knot insertion, removal, degree elevation, unperiodizing
// =================================================================================
static KS ks_from(int deg, bool per, const Vd& k, const Vi& m) { KS s; s.deg = deg; s.per = per; s.k = k; s.m = m; return s; }
static bool structure_ok(int deg, bool per, const Vd& k, const Vi& m) {
  if (deg < 1 || k.size() != m.size() || k.size() < 2) return false;
  for (int x : m) if (x < 1 || x > deg + 1) return false;
  for (size_t i = 1; i < k.size(); ++i) if (!(k[i] > k[i - 1])) return false;
  return ks_valid(ks_from(deg, per, k, m));
}
// knots to add: sorted values inside the usable range, some equal to knots, some doubled, some within the tolerance
static Vd gen_add_knots(const KS& s, int count, int mode, bool allow_dup = true) {
  int fi, la; ks_fl(s, fi, la);
  double lo = s.k[fi - 1], hi = s.k[la - 1];
  Vd a;
  for (int i = 0; i < count; ++i) {
    int kind = mode == 0 ? 0 : rint(0, 3);
    if (kind == 0) a.push_back(rnd(lo, hi));
    else if (kind == 1) a.push_back(s.k[rint(fi - 1, la - 1)]);
    else if (kind == 2) { double x = rnd(lo, hi); a.push_back(x); a.push_back(x); }
    else a.push_back(s.k[rint(fi - 1, la - 1)] + rnd(-1e-9, 1e-9));
  }
  std::sort(a.begin(), a.end());
  if (!allow_dup) { // OCCT's PrepareInsertKnots and InsertKnots disagree on repeated knots when the multiplicities are not added
    Vd b; for (double x : a) if (b.empty() || x - b.back() > 1e-7) b.push_back(x);
    a = b;
  }
  return a;
}
static Vi gen_add_mults(int n, int deg) { Vi m; for (int i = 0; i < n; ++i) m.push_back(rint(0, deg + 1)); return m; }

static void emit_prep_ins(int deg, bool per, const Vd& kn, const Vi& m, const Vd& ak, const Vi* am, double tol, bool add) {
  begin("bsplclib_prepare_insert_knots", "i i fs is fs ois f i");
  W_i(deg); W_i(per); W_fs(kn); W_is(m); W_fs(ak); W_ois(am); W_f(tol); W_i(add); bar();
  if (!structure_ok(deg, per, kn, m) || ak.empty() || (am && am->size() != ak.size())) unchecked();
  else run2([&](Out& o) {
    A1d k = mk1(kn), a = mk1(ak); A1i mm = mk1i(m), aam = mk1i(am ? *am : Vi{1});
    int nbp = -1, nbk = -1;
    bool ok = BSplCLib::PrepareInsertKnots(deg, per, k, mm, a, am ? &aam : nullptr, nbp, nbk, tol, add);
    o.i(ok);
    if (ok) { o.i(nbp); o.i(nbk); }
  });
  finish();
}
static const double TOLS[] = {0.0, 1e-9, 1e-6, 1e-3};
static void fam_prep_ins() {
  for (int k = 0; k < 28; ++k) {
    int deg = rint(1, 6); bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 4), k % 4, k % 7 == 6 && !per);
    bool add = k % 5 != 4;
    Vd ak = gen_add_knots(s, rint(1, 4), k % 2, add);
    Vi am = gen_add_mults(ak.size(), deg);
    emit_prep_ins(deg, per, s.k, s.m, ak, k % 3 == 1 ? nullptr : &am, TOLS[k % 4], add);
  }
  { KS s = mk_ks(3, false, 3, 0); Vd ak = {s.k[1], s.k[1] - 0.1}; emit_prep_ins(3, false, s.k, s.m, ak, nullptr, 0.0, true); }   // decreasing: false
  { KS s = mk_ks(3, false, 3, 0); Vd ak = {s.k[0] - 0.5}; emit_prep_ins(3, false, s.k, s.m, ak, nullptr, 0.0, true); }          // below the range: false
  { KS s = mk_ks(3, false, 3, 0); Vd ak = {s.k.back() + 0.5}; emit_prep_ins(3, false, s.k, s.m, ak, nullptr, 0.0, true); }      // above the range: false
  { KS s = mk_ks(3, true, 3, 1); Vd ak = {s.k.front(), s.k.back()}; emit_prep_ins(3, true, s.k, s.m, ak, nullptr, 0.0, true); } // a whole period
  // invalid
  { KS s = mk_ks(3, false, 3, 0); emit_prep_ins(3, false, s.k, s.m, Vd(), nullptr, 0.0, true); }
  { KS s = mk_ks(3, false, 3, 0); Vi am = {1}; emit_prep_ins(3, false, s.k, s.m, Vd{s.k[1], s.k[2]}, &am, 0.0, true); }
}

// Inserts knots with OCCT; returns false when PrepareInsertKnots refuses.
static bool occ_insert(int deg, bool per, int dim, const Vd& poles, const KS& s, const Vd& ak, const Vi* am, double tol, bool add, Vd& np, Vd& nk, Vi& nm) {
  A1d k = mk1(s.k), a = mk1(ak); A1i mm = mk1i(s.m), aam = mk1i(am ? *am : Vi{1});
  int nbp = 0, nbk = 0;
  if (!BSplCLib::PrepareInsertKnots(deg, per, k, mm, a, am ? &aam : nullptr, nbp, nbk, tol, add)) return false;
  A1d P = mk1(poles), NP(1, nbp * dim), NK(1, nbk); A1i NM(1, nbk);
  NP.Init(SENT);
  BSplCLib::InsertKnots(deg, per, dim, P, k, mm, a, am ? &aam : nullptr, NP, NK, NM, tol, add);
  np.clear(); nk.clear(); nm.clear();
  for (int i = 1; i <= nbp * dim; ++i) np.push_back(NP(i));
  for (int i = 1; i <= nbk; ++i) { nk.push_back(NK(i)); nm.push_back(NM(i)); }
  return true;
}
static void emit_insert(int deg, bool per, int dim, const Vd& poles, const KS& s, const Vd& ak, const Vi* am, double tol, bool add) {
  begin("bsplclib_insert_knots", "i i i fs fs is fs ois f i");
  W_i(deg); W_i(per); W_i(dim); W_fs(poles); W_fs(s.k); W_is(s.m); W_fs(ak); W_ois(am); W_f(tol); W_i(add); bar();
  bool bad = !structure_ok(deg, per, s.k, s.m) || ak.empty() || (am && am->size() != ak.size()) || dim < 1;
  if (!bad) bad = (long)poles.size() != (long)ks_np(s) * dim;
  if (!bad) { A1d k = mk1(s.k), a = mk1(ak); A1i mm = mk1i(s.m), aam = mk1i(am ? *am : Vi{1}); int nbp, nbk;
              bad = !BSplCLib::PrepareInsertKnots(deg, per, k, mm, a, am ? &aam : nullptr, nbp, nbk, tol, add); }
  if (bad) { unchecked(); finish(); return; }
  { // PrepareInsertKnots and InsertKnots disagree for some inputs with repeated knots (periodic, multiplicities that overflow the degree):
    // the number of poles is then wrong and OCCT leaves poles unwritten. Such cases are not in the golden file.
    Vd np, nk; Vi nm; occ_insert(deg, per, dim, poles, s, ak, am, tol, add, np, nk, nm);
    for (double x : np) if (x == SENT) { buf.clear(); return; }
  }
  run2([&](Out& o) {
    Vd np, nk; Vi nm;
    occ_insert(deg, per, dim, poles, s, ak, am, tol, add, np, nk, nm);
    for (double x : np) if (x == SENT) { std::fprintf(stderr, "InsertKnots left a pole unwritten\n"); std::abort(); }
    o.fs(np); o.fs(nk); o.is(nm);
  });
  finish();
}
static void fam_insert() {
  for (int k = 0; k < 17; ++k) {
    int deg = k < 1 ? 12 : rint(1, 6); bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 4), k % 4, k % 7 == 6 && !per);
    int dims[] = {1, 2, 3, 4, 3, 6, 12};
    int dim = dims[k % 7];
    bool add = k % 5 != 4;
    Vd ak = gen_add_knots(s, rint(1, 3), k % 2, add);
    Vi am = gen_add_mults(ak.size(), deg);
    emit_insert(deg, per, dim, rvec(ks_np(s) * dim, -5, 5), s, ak, k % 3 == 1 ? nullptr : &am, TOLS[k % 4], add);
  }
  // knots already present, a full period, repeated knots at once
  { KS s = mk_ks(3, false, 3, 0); emit_insert(3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, Vd{s.k[1], s.k[1], s.k[2]}, nullptr, 0.0, true); }
  { KS s = mk_ks(3, true, 3, 1); emit_insert(3, true, 3, rvec(ks_np(s) * 3, -5, 5), s, Vd{s.k.front(), s.k.back()}, nullptr, 0.0, true); }
  { KS s = mk_ks(3, true, 3, 1); emit_insert(3, true, 2, rvec(ks_np(s) * 2, -5, 5), s, Vd{s.k.front(), 0.5 * (s.k[1] + s.k[2])}, nullptr, 0.0, true); }
  // invalid: refused by PrepareInsertKnots, wrong number of poles, no knots
  { KS s = mk_ks(3, false, 3, 0); emit_insert(3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, Vd{s.k[0] - 1}, nullptr, 0.0, true); }
  { KS s = mk_ks(3, false, 3, 0); emit_insert(3, false, 3, rvec(ks_np(s) * 3 - 3, -5, 5), s, Vd{s.k[1] + 0.1}, nullptr, 0.0, true); }
  { KS s = mk_ks(3, false, 3, 0); emit_insert(3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, Vd(), nullptr, 0.0, true); }
}

static void emit_remove(int index, int mult, int deg, bool per, int dim, const Vd& poles, const KS& s, double tol) {
  begin("bsplclib_remove_knot", "i i i i i fs fs is f");
  W_i(index); W_i(mult); W_i(deg); W_i(per); W_i(dim); W_fs(poles); W_fs(s.k); W_is(s.m); W_f(tol); bar();
  bool bad = !structure_ok(deg, per, s.k, s.m) || dim < 1;
  int nk = (int)s.k.size();
  if (!bad) bad = (long)poles.size() != (long)ks_np(s) * dim;
  int fi = 0, la = 0, depth = 0;
  bool inrange = false;
  if (!bad) {
    if (per) { fi = 1; la = nk; }
    else { A1i mm = mk1i(s.m); fi = BSplCLib::FirstUKnotIndex(deg, mm) + 1; la = BSplCLib::LastUKnotIndex(deg, mm) - 1; }
    inrange = index >= fi && index <= la;
    if (inrange) {
      int the = (per && index == fi) ? la : index;
      depth = s.m[the - 1] - mult;
      if (mult < 0 || depth < 1) bad = true;
    }
  }
  if (bad) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    A1d k = mk1(s.k), P = mk1(poles); A1i mm = mk1i(s.m);
    int nbk = mult > 0 ? nk : nk - 1, nbp = (int)(poles.size() / dim) - depth;
    if (!inrange) { nbk = nk; nbp = (int)(poles.size() / dim); }
    A1d NP(1, nbp * dim), NK(1, nbk); A1i NM(1, nbk);
    NP.Init(SENT);
    bool ok = BSplCLib::RemoveKnot(index, mult, deg, per, dim, P, k, mm, NP, NK, NM, tol);
    o.i(ok);
    if (ok) {
      for (int i = 1; i <= nbp * dim; ++i) { if (NP(i) == SENT) { std::fprintf(stderr, "RemoveKnot left a pole unwritten\n"); std::abort(); } o.f(NP(i)); }
      for (int i = 1; i <= nbk; ++i) o.f(NK(i));
      for (int i = 1; i <= nbk; ++i) o.i(NM(i));
    }
  });
  finish();
}
static void fam_remove() {
  int made = 0;
  for (int k = 0; made < 18 && k < 400; ++k) {
    int deg = rint(1, 5); bool per = k % 3 == 2;
    KS base = mk_ks(deg, per, rint(2, 4), k % 4, false);
    int dims[] = {1, 2, 3, 4, 3, 6};
    int dim = dims[k % 6];
    Vd poles = rvec(ks_np(base) * dim, -5, 5);
    int mode = k % 4;
    if (mode == 3) {                 // random poles: removal refused (or accepted with a large tolerance)
      int fi, la; ks_fl(base, fi, la);
      int idx = per ? rint(1, (int)base.k.size()) : rint(fi + 1, std::max(fi + 1, la - 1));
      int mm = base.m[idx - 1] - 1;
      emit_remove(idx, std::max(0, mm), deg, per, dim, poles, base, k % 8 == 3 ? 1e3 : 1e-6); ++made; continue;
    }
    // insert a knot, then remove it again
    int fi, la; ks_fl(base, fi, la);
    double u = mode == 0 ? rnd(base.k[fi - 1] + 1e-3, base.k[la - 1] - 1e-3) : base.k[rint(fi, std::max(fi, la - 2))];
    int depth = rint(1, std::max(1, std::min(deg, 2)));
    Vd np, nk; Vi nm;
    Vi am = {depth};
    if (!occ_insert(deg, per, dim, poles, base, Vd{u}, &am, 0.0, true, np, nk, nm)) continue;
    KS big = ks_from(deg, per, nk, nm);
    if (!structure_ok(deg, per, nk, nm)) continue;
    int idx = 1; double best = 1e300;
    for (size_t i = 0; i < nk.size(); ++i) if (std::fabs(nk[i] - u) < best) { best = std::fabs(nk[i] - u); idx = (int)i + 1; }
    int m_new = std::max(0, nm[idx - 1] - depth);
    if (mode == 2 && m_new > 0 && coin()) m_new = std::max(0, m_new - 1 + (coin() ? 1 : 0));
    emit_remove(idx, m_new, deg, per, dim, np, big, 1e-6); ++made;
  }
  // an index outside the removable range: false
  { KS s = mk_ks(3, false, 3, 0); emit_remove(1, 0, 3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, 1e-6); emit_remove((int)s.k.size(), 0, 3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, 1e-6); emit_remove(40, 0, 3, false, 2, rvec(ks_np(s) * 2, -5, 5), s, 1e-6); }
  // invalid: multiplicity not smaller than the present one, negative, wrong number of poles
  { KS s = mk_ks(3, false, 3, 1); int m = s.m[1]; emit_remove(2, m, 3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, 1e-6); emit_remove(2, -1, 3, false, 3, rvec(ks_np(s) * 3, -5, 5), s, 1e-6); emit_remove(2, 0, 3, false, 3, rvec(ks_np(s) * 3 + 3, -5, 5), s, 1e-6); }
}

static void emit_inc_count(int deg, int newdeg, bool per, const Vi& m) {
  begin("bsplclib_increase_degree_count_knots", "i i i is");
  W_i(deg); W_i(newdeg); W_i(per); W_is(m); bar();
  bool bad = deg < 1 || newdeg < deg || m.empty();
  if (!bad) { int sum = 0; for (int x : m) sum += x; bad = sum < deg + 1; }
  if (!bad && !per) { A1i mm = mk1i(m); int f = BSplCLib::FirstUKnotIndex(deg, mm), l = BSplCLib::LastUKnotIndex(deg, mm); bad = f < 1 || l > (int)m.size() || f > l; }
  if (bad) unchecked();
  else run2([&](Out& o) { A1i mm = mk1i(m); o.i(BSplCLib::IncreaseDegreeCountKnots(deg, newdeg, per, mm)); });
  finish();
}
static void fam_inc_count() {
  for (int k = 0; k < 28; ++k) {
    int deg = rint(1, 6); bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 5), k % 4, k % 5 == 4 && !per);
    emit_inc_count(deg, deg + rint(0, 4), per, s.m);
  }
  emit_inc_count(3, 2, false, Vi{4, 4});
  emit_inc_count(0, 2, false, Vi{4, 4});
  emit_inc_count(3, 4, false, Vi{});
}

static void emit_inc(int deg, int newdeg, bool per, int dim, const Vd& poles, const KS& s) {
  begin("bsplclib_increase_degree", "i i i i fs fs is");
  W_i(deg); W_i(newdeg); W_i(per); W_i(dim); W_fs(poles); W_fs(s.k); W_is(s.m); bar();
  bool bad = !structure_ok(deg, per, s.k, s.m) || newdeg < deg || newdeg > 25 || dim < 1;
  if (!bad) bad = (long)poles.size() != (long)ks_np(s) * dim;
  if (!bad && !per) { A1i mm = mk1i(s.m); bad = BSplCLib::FirstUKnotIndex(deg, mm) >= BSplCLib::LastUKnotIndex(deg, mm); }
  int nbp = 0;
  if (!bad) { // the number of poles of the result follows from its multiplicities; probe them with a one-pole result
    A1i mm = mk1i(s.m); A1d k = mk1(s.k), P = mk1(poles);
    int nk = BSplCLib::IncreaseDegreeCountKnots(deg, newdeg, per, mm);
    A1d NP(1, dim), NK(1, nk); A1i NM(1, nk);
    BSplCLib::IncreaseDegree(deg, newdeg, per, dim, P, k, mm, NP, NK, NM);
    nbp = BSplCLib::NbPoles(newdeg, per, NM);
    if (nbp < 1) bad = true;
  }
  if (bad) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    A1i mm = mk1i(s.m); A1d k = mk1(s.k), P = mk1(poles);
    int nk = BSplCLib::IncreaseDegreeCountKnots(deg, newdeg, per, mm);
    A1d NP(1, nbp * dim), NK(1, nk); A1i NM(1, nk);
    BSplCLib::IncreaseDegree(deg, newdeg, per, dim, P, k, mm, NP, NK, NM);
    for (int i = 1; i <= nbp * dim; ++i) o.f(NP(i));
    for (int i = 1; i <= nk; ++i) o.f(NK(i));
    for (int i = 1; i <= nk; ++i) o.i(NM(i));
  });
  finish();
}
static void fam_inc() {
  for (int k = 0; k < 17; ++k) {
    int deg = rint(1, 5); bool per = k % 3 == 2;
    KS s = mk_ks(deg, per, rint(1, 4), k % 4, k % 6 == 5 && !per);
    int dims[] = {1, 2, 3, 4, 3, 6, 12};
    int dim = dims[k % 7];
    int newdeg = deg + (k % 4 == 3 ? rint(2, 3) : 1);
    emit_inc(deg, newdeg, per, dim, rvec(ks_np(s) * dim, -5, 5), s);
  }
  { KS s = mk_ks(3, false, 3, 0); emit_inc(3, 3, false, 3, rvec(ks_np(s) * 3, -5, 5), s); }  // same degree
  { KS s = mk_ks(1, false, 1, 0); emit_inc(1, 9, false, 3, rvec(ks_np(s) * 3, -5, 5), s); }
  // invalid
  { KS s = mk_ks(3, false, 3, 0); emit_inc(3, 2, false, 3, rvec(ks_np(s) * 3, -5, 5), s); emit_inc(3, 4, false, 3, rvec(ks_np(s) * 3 - 3, -5, 5), s); emit_inc(3, 4, false, 0, rvec(ks_np(s) * 3, -5, 5), s); }
}

static void emit_prep_unper(int deg, const Vi& m) {
  begin("bsplclib_prepare_unperiodize", "i is");
  W_i(deg); W_is(m); bar();
  bool bad = deg < 1 || m.size() < 2 || m[0] != m.back() || m[0] < 1 || m[0] > deg;
  for (int x : m) if (x < 1 || x > deg) bad = true;
  if (bad) unchecked();
  else run2([&](Out& o) { A1i mm = mk1i(m); int nk = 0, np = 0; BSplCLib::PrepareUnperiodize(deg, mm, nk, np); o.i(nk); o.i(np); });
  finish();
}
static void fam_prep_unper() {
  for (int k = 0; k < 24; ++k) {
    int deg = rint(1, 6);
    KS s = mk_ks(deg, true, rint(1, 5), k % 4);
    emit_prep_unper(deg, s.m);
  }
  emit_prep_unper(3, Vi{2, 2});
  emit_prep_unper(3, Vi{4, 1, 4});
  emit_prep_unper(3, Vi{2, 1, 3});
  emit_prep_unper(3, Vi{1});
}

static void emit_unper(int deg, int dim, const KS& s, const Vd& poles) {
  begin("bsplclib_unperiodize", "i i is fs fs");
  W_i(deg); W_i(dim); W_is(s.m); W_fs(s.k); W_fs(poles); bar();
  bool bad = !structure_ok(deg, true, s.k, s.m) || dim < 1;
  if (!bad) bad = (long)poles.size() != (long)ks_np(s) * dim;
  if (bad) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    A1i mm = mk1i(s.m); A1d k = mk1(s.k), P = mk1(poles);
    int nk = 0, np = 0; BSplCLib::PrepareUnperiodize(deg, mm, nk, np);
    A1i NM(1, nk); A1d NK(1, nk), NP(1, np * dim);
    BSplCLib::Unperiodize(deg, dim, mm, k, P, NM, NK, NP);
    for (int i = 1; i <= nk; ++i) o.i(NM(i));
    for (int i = 1; i <= nk; ++i) o.f(NK(i));
    for (int i = 1; i <= np * dim; ++i) o.f(NP(i));
  });
  finish();
}
static void fam_unper() {
  for (int k = 0; k < 17; ++k) {
    int deg = rint(1, 6);
    KS s = mk_ks(deg, true, rint(1, 5), k % 4);
    int dims[] = {1, 2, 3, 4, 3, 6, 12};
    int dim = dims[k % 7];
    emit_unper(deg, dim, s, rvec(ks_np(s) * dim, -5, 5));
  }
  { KS s = mk_ks(3, true, 3, 0); emit_unper(3, 3, s, rvec(ks_np(s) * 3 - 3, -5, 5)); emit_unper(3, 0, s, rvec(ks_np(s) * 3, -5, 5)); }
  { KS s = mk_ks(3, false, 3, 0); emit_unper(3, 3, s, rvec(ks_np(s) * 3, -5, 5)); }
}

static void fam_constants() {
  begin("bsplclib_constants", ""); bar();
  Out o;
  o.i(BSplCLib::MaxDegree()); o.i(BSplCLib::MaxUnitWeightsSize());
  const double* w = BSplCLib::UnitWeightsData();
  o.f(w[0]); o.f(w[BSplCLib::MaxUnitWeightsSize() - 1]);
  o.i(BSplCLib::NoMults() == nullptr ? 1 : 0);
  o.emit();
  finish();
}

// =================================================================================
// BSplSLib: evaluation
// =================================================================================
// A random surface for evaluation tests. k selects the combination.
static int small_deg() { return coin(0.8) ? rint(1, 4) : rint(5, 9); }
static SurfIn gen_surf(int k, bool allow_flat = true, bool allow_per = true) {
  int ud = small_deg(), vd = small_deg();
  if (k % 11 == 10) { ud = rint(1, 3); vd = ud; }          // equal degrees
  bool uper = allow_per && k % 9 == 5, vper = allow_per && k % 11 == 7;
  int us = (ud > 4 || coin(0.6)) ? 1 : 2, vs = (vd > 4 || coin(0.6)) ? 1 : 2;
  KS u = mk_ks(ud, uper, us, k % 4, !uper && k % 13 == 12);
  KS v = mk_ks(vd, vper, vs, (k / 2) % 4, !vper && k % 17 == 16);
  static const int WM[] = {0, 1, 0, 2, 3, 4, 5, 1};
  bool uf = allow_flat && k % 6 == 4, vf = allow_flat && k % 8 == 5;
  SurfIn s = mk_surf(u, v, WM[k % 8], uf, vf);
  if (s.hasW && k % 19 == 18) s.urat = s.vrat = false;       // weights present but ignored
  return s;
}
static bool surf_bad(const SurfIn& s) {
  if (s.ud < 1 || s.ud > 25 || s.vd < 1 || s.vd > 25) return true;
  if ((s.urat || s.vrat) && !s.hasW) return true;            // OCCT dereferences the null weights
  return false;
}
// kinds: 0 D0, 1 D1, 2 D2, 3 D3, 4 DN, 5 HomogeneousD0, 6 HomogeneousD1
static const char* const EVAL_NAMES[] = {"bsplslib_d0", "bsplslib_d1", "bsplslib_d2", "bsplslib_d3", "bsplslib_dn", "bsplslib_homogeneous_d0", "bsplslib_homogeneous_d1"};
static void emit_eval(int kind, double u, double v, int nu, int nv, int ui, int vi, const SurfIn& s) {
  begin(EVAL_NAMES[kind], kind == 4 ? "f f i i i i surf" : "f f i i surf");
  W_f(u); W_f(v);
  if (kind == 4) { W_i(nu); W_i(nv); }
  W_i(ui); W_i(vi); W_surf(s); bar();
  if (surf_bad(s) || (kind == 4 && (nu < 0 || nv < 0))) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    OccSurf c(s);
    gp_Pnt P, N; gp_Vec Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv, Vn, Nu, Nv; double W = 0, D = 0, Du = 0, Dv = 0;
#define ARGS u, v, ui, vi, c.poles, c.W(), c.uk, c.vk, c.UM(), c.VM(), s.ud, s.vd, s.urat, s.vrat, s.uper, s.vper
    switch (kind) {
      case 0: BSplSLib::D0(ARGS, P); o.p(P); break;
      case 1: BSplSLib::D1(ARGS, P, Vu, Vv); o.p(P); o.v(Vu); o.v(Vv); break;
      case 2: BSplSLib::D2(ARGS, P, Vu, Vv, Vuu, Vvv, Vuv); o.p(P); o.v(Vu); o.v(Vv); o.v(Vuu); o.v(Vvv); o.v(Vuv); break;
      case 3: BSplSLib::D3(ARGS, P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv);
              o.p(P); o.v(Vu); o.v(Vv); o.v(Vuu); o.v(Vvv); o.v(Vuv); o.v(Vuuu); o.v(Vvvv); o.v(Vuuv); o.v(Vuvv); break;
      case 4: BSplSLib::DN(u, v, nu, nv, ui, vi, c.poles, c.W(), c.uk, c.vk, c.UM(), c.VM(), s.ud, s.vd, s.urat, s.vrat, s.uper, s.vper, Vn); o.v(Vn); break;
      case 5: BSplSLib::HomogeneousD0(ARGS, W, P); o.f(W); o.p(P); break;
      default: BSplSLib::HomogeneousD1(ARGS, N, Nu, Nv, D, Du, Dv); o.p(N); o.v(Nu); o.v(Nv); o.f(D); o.f(Du); o.f(Dv); break;
    }
#undef ARGS
  });
  finish();
}
static void fam_eval(int kind, int count) {
  for (int k = 0; k < count; ++k) {
    SurfIn s = gen_surf(k + 3 * kind);
    // keep high degrees rare and thin
    double u = pick_any(s.u), v = pick_any(s.v);
    if (k % 5 == 3) { u = pick_u(s.u, 4); v = pick_u(s.v, 1); }
    int ui = pick_index(s.u, u, !s.hasUm, k % 3 == 1 ? 1 : (k % 7 == 3 ? 2 : 0));
    int vi = pick_index(s.v, v, !s.hasVm, k % 4 == 1 ? 1 : (k % 9 == 4 ? 2 : 0));
    int nu = rint(0, s.ud + 1), nv = rint(0, s.vd + 1);
    if (k % 6 == 5) { nu = rint(0, 3); nv = rint(0, 3); }
    emit_eval(kind, u, v, nu, nv, ui, vi, s);
  }
  // degrees: 25 in one direction, 25 in both (D0 only), 9
  { KS u = mk_ks(25, false, 1, 0), v = mk_ks(1, false, 2, 0);
    SurfIn s = mk_surf(u, v, kind % 2 ? 1 : 0); emit_eval(kind, pick_any(u), pick_any(v), 2, 1, 0, 0, s);
    if (kind < 2) { s = mk_surf(v, u, 1); emit_eval(kind, pick_any(v), pick_any(u), 1, 3, 0, 0, s); } }
  if (kind == 0 || kind == 3) { KS u = mk_ks(9, false, 1, 0), v = mk_ks(kind == 0 ? 9 : 2, false, 1, 0); SurfIn s = mk_surf(u, v, 0, false, false, 3.0); emit_eval(kind, pick_any(u), pick_any(v), 4, 5, 0, 0, s); }
  // rational surfaces with weights close to zero and u-degree above, equal to, below the v-degree
  for (int d = 0; d < 3; ++d) {
    KS u = mk_ks(d == 0 ? 3 : 2, false, 1, 0), v = mk_ks(d == 1 ? 3 : 2, false, 1, 0);
    SurfIn s = mk_surf(u, v, 2); s.urat = s.vrat = true;
    emit_eval(kind, pick_u(u, 0), pick_u(v, 0), 1 + d, 2, 0, 0, s);
  }
  // invalid: degree above the maximum (the data container of OCCT overflows), rational flag without weights
  { KS u = mk_ks(26, false, 1, 0), v = mk_ks(1, false, 1, 0); SurfIn s = mk_surf(u, v, 0); emit_eval(kind, 0.3, 0.3, 1, 1, 0, 0, s); }
  { KS u = mk_ks(2, false, 1, 0), v = mk_ks(2, false, 1, 0); SurfIn s = mk_surf(u, v, 0); s.urat = true; emit_eval(kind, 0.3, 0.3, 1, 1, 0, 0, s); }
  if (kind == 4) { KS u = mk_ks(2, false, 1, 0), v = mk_ks(2, false, 1, 0); SurfIn s = mk_surf(u, v, 0); emit_eval(kind, 0.3, 0.3, -1, 1, 0, 0, s); }
}

// ---- RationalDerivative
static void emit_rat_der(int ud, int vd, int n, int m, const Vd& ders, bool all) {
  begin("bsplslib_rational_derivative", "i i i i fs i");
  W_i(ud); W_i(vd); W_i(n); W_i(m); W_fs(ders); W_i(all); bar();
  long need = 0;
  bool bad = ud < 0 || vd < 0 || n < 0 || m < 0;
  if (!bad) { int mn = std::min(n, ud), mm = std::min(m, vd); need = (long)mn * (vd + 1) * 4 + (long)mm * 4 + 4; bad = (long)ders.size() < need; }
  if (!bad && ders[3] == 0.0) bad = true;
  if (bad) unchecked();
  else run2([&](Out& o) {
    Vd d(ders); Vd r(all ? 3 * (n + 1) * (m + 1) : 3, SENT);
    BSplSLib::RationalDerivative(ud, vd, n, m, d[0], r[0], all);
    o.fs(r);
  });
  finish();
}
static void fam_rat_der() {
  for (int k = 0; k < 18; ++k) {
    int ud = rint(1, 4), vd = rint(1, 4);
    Vd d = rvec((ud + 1) * (vd + 1) * 4, -3, 3);
    for (size_t i = 3; i < d.size(); i += 4) d[i] = rnd(0.4, 2.5) * (coin(0.2) ? -1 : 1);
    d[3] = rnd(0.5, 2.0);
    int n = rint(0, 3), m = rint(0, 3);
    emit_rat_der(ud, vd, n, m, d, k % 3 != 2);
  }
  { Vd d = rvec(4 * 4 * 4, -3, 3); d[3] = 1.5; emit_rat_der(3, 3, 5, 5, d, true); emit_rat_der(3, 3, 5, 5, d, false); emit_rat_der(3, 3, 2, 6, d, false); }
  { Vd d = rvec(2 * 2 * 4, -3, 3); d[3] = 1.5; emit_rat_der(1, 1, 1, 1, d, true); emit_rat_der(1, 1, 3, 3, d, true); }
  // invalid
  { Vd d = rvec(3 * 3 * 4, -3, 3); d[3] = 1.5; emit_rat_der(2, 2, -1, 1, d, true); emit_rat_der(2, 2, 2, 2, Vd(d.begin(), d.begin() + 20), true); }
  { Vd d = rvec(3 * 3 * 4, -3, 3); d[3] = 0.0; emit_rat_der(2, 2, 2, 2, d, true); }
}

// ---- Iso
static void emit_iso(double param, bool isu, const P3A2& poles, const A2* w, const Vd& knots, const Vi* mults, int deg, bool per, bool wantw) {
  begin("bsplslib_iso", "f i p3a2 oarr2 fs ois i i i");
  W_f(param); W_i(isu); W_p3a2(poles); W_oarr2(w); W_fs(knots); W_ois(mults); W_i(deg); W_i(per); W_i(wantw); bar();
  if (deg < 1 || (w && !wantw)) unchecked();
  else run2([&](Out& o) {
    A2p P = mk2p(poles); A2d W = w ? mk2d(*w) : A2d(1, 1, 1, 1); A1d k = mk1(knots); A1i m = mk1i(mults ? *mults : Vi{1});
    int n = isu ? poles.cols : poles.rows;
    NCollection_Array1<gp_Pnt> cp(1, n); for (int i = 1; i <= n; ++i) cp(i) = gp_Pnt(7.5, 7.5, 7.5);
    A1d cw(1, n); cw.Init(7.5);
    BSplSLib::Iso(param, isu, P, w ? &W : nullptr, k, mults ? &m : nullptr, deg, per, cp, wantw ? &cw : nullptr);
    for (int i = 1; i <= n; ++i) o.p(cp(i));
    if (wantw) for (int i = 1; i <= n; ++i) o.f(cw(i));
  });
  finish();
}
static void fam_iso() {
  for (int k = 0; k < 16; ++k) {
    SurfIn s = gen_surf(k + 5);
    bool isu = k % 2 == 0;
    const KS& ks = isu ? s.u : s.v;
    bool hasM = isu ? s.hasUm : s.hasVm;
    double p = pick_any(ks);
    bool hasW = s.hasW;
    emit_iso(p, isu, s.poles, hasW ? &s.w : nullptr, isu ? s.uk : s.vk, hasM ? &(isu ? s.um : s.vm) : nullptr, ks.deg, ks.per, hasW || k % 3 == 0);
  }
  { SurfIn s = gen_surf(3); s.hasW = true; s.w = mkA2(s.poles.rows, s.poles.cols, 0.5, 2); emit_iso(pick_any(s.u), true, s.poles, &s.w, s.uk, s.hasUm ? &s.um : nullptr, s.ud, s.uper, false); } // rational without CWeights
  { SurfIn s = gen_surf(0); s.hasW = false; emit_iso(0.5, true, s.poles, nullptr, s.uk, s.hasUm ? &s.um : nullptr, 0, s.uper, false); }
}

// ---- Reverse
static void emit_reverse_poles(const P3A2& p, int last, bool udir) {
  begin("bsplslib_reverse_poles", "p3a2 i i");
  W_p3a2(p); W_i(last); W_i(udir); bar();
  int len = udir ? p.rows : p.cols;
  if (1 + (last - 1) % len < 1) unchecked();
  else run2([&](Out& o) { A2p a = mk2p(p); BSplSLib::Reverse(a, last, udir); o.dims(a.ColLength(), a.RowLength()); o.p3a2(a); });
  finish();
}
static void emit_reverse_weights(const A2& p, int last, bool udir) {
  begin("bsplslib_reverse_weights", "arr2 i i");
  W_arr2(p); W_i(last); W_i(udir); bar();
  int len = udir ? p.rows : p.cols;
  if (1 + (last - 1) % len < 1) unchecked();
  else run2([&](Out& o) { A2d a = mk2d(p); BSplSLib::Reverse(a, last, udir); o.dims(a.ColLength(), a.RowLength()); o.a2(a); });
  finish();
}
static void fam_reverse() {
  for (int k = 0; k < 17; ++k) {
    int r = rint(1, 4), c = rint(1, 4); bool udir = k % 2 == 0;
    int len = udir ? r : c;
    int last = k % 3 == 0 ? len : rint(1, 2 * len + 1);
    emit_reverse_poles(mkP3A2(r, c, -5, 5), last, udir);
    emit_reverse_weights(mkA2(r, c, 0.5, 2), last, udir);
  }
  emit_reverse_poles(mkP3A2(3, 3, -5, 5), 0, true); emit_reverse_poles(mkP3A2(3, 3, -5, 5), -3, true);  // 0 and -3 are multiples: fine; -1 is not
  emit_reverse_poles(mkP3A2(3, 4, -5, 5), -1, false); emit_reverse_weights(mkA2(3, 4, 0.5, 2), -2, true);
}

// ---- IsRational
static void emit_is_rational(const A2& w, int i1, int i2, int j1, int j2, double eps) {
  begin("bsplslib_is_rational", "arr2 i i i i f");
  W_arr2(w); W_i(i1); W_i(i2); W_i(j1); W_i(j2); W_f(eps); bar();
  bool bad = false;
  if (!(eps > 0.0)) bad = i1 < 1 || i1 > w.rows || i2 < 1 || i2 > w.cols;
  if (!bad && i1 < i2 && j1 < j2 && (i1 < 1 || j1 < 1)) bad = true;
  if (bad) unchecked();
  else run2([&](Out& o) { A2d a = mk2d(w); o.i(BSplSLib::IsRational(a, i1, i2, j1, j2, eps)); });
  finish();
}
static void fam_is_rational() {
  for (int k = 0; k < 24; ++k) {
    int r = rint(2, 5), c = rint(2, 5);
    A2 w = mkA2(r, c, 0.5, 2);
    if (k % 3 == 0) { double x = rnd(0.5, 2); for (double& y : w.v) y = x; }
    if (k % 3 == 1) { double x = rnd(0.5, 2); for (int j = 0; j < c; ++j) for (int i = 0; i < r; ++i) w.at(i, j) = x + (j % 2) * 0.5; } // varies along columns only
    int i1 = rint(1, r), i2 = rint(1, c), j1 = rint(1, c), j2 = rint(1, c + 2);
    if (k % 4 == 0) { i1 = 1; i2 = std::min(c, r); j1 = 1; j2 = c; }
    double eps = k % 5 == 0 ? 0.0 : (k % 5 == 1 ? 1e-3 : (k % 5 == 2 ? 0.7 : -1.0));
    emit_is_rational(w, i1, i2, j1, j2, eps);
  }
  { A2 w = mkA2(3, 3, 0.5, 2); emit_is_rational(w, 0, 2, 1, 3, 0.1); emit_is_rational(w, 1, 2, 0, 3, 0.1); emit_is_rational(w, 1, 9, 1, 9, 0.1); emit_is_rational(w, 4, 2, 1, 3, 0.0); emit_is_rational(w, 1, 4, 1, 3, 0.0); }
}

// ---- SetPoles / GetPoles
static void emit_set_poles(const P3A2& p, const A2* w, bool udir) {
  begin(w ? "bsplslib_set_poles_weighted" : "bsplslib_set_poles", w ? "p3a2 arr2 i" : "p3a2 i");
  W_p3a2(p); if (w) W_arr2(*w); W_i(udir); bar();
  if (w && (w->rows < p.rows || w->cols < p.cols)) unchecked();
  else run2([&](Out& o) {
    A2p P = mk2p(p); int n = p.rows * p.cols * (w ? 4 : 3);
    A1d fp(1, n); fp.Init(SENT);
    if (w) { A2d W = mk2d(*w); BSplSLib::SetPoles(P, W, fp, udir); } else BSplSLib::SetPoles(P, fp, udir);
    for (int i = 1; i <= n; ++i) o.f(fp(i));
  });
  finish();
}
static void emit_get_poles(bool weighted, int rows, int cols, const Vd& fp, bool udir) {
  begin(weighted ? "bsplslib_get_poles_weighted" : "bsplslib_get_poles", "i i fs i");
  W_i(rows); W_i(cols); W_fs(fp); W_i(udir); bar();
  int per = weighted ? 4 : 3;
  bool bad = rows < 1 || cols < 1 || (long)fp.size() < (long)rows * cols * per;
  if (!bad && weighted) for (int i = 0; i < rows * cols; ++i) if (fp[i * 4 + 3] == 0.0) bad = true;
  if (bad) unchecked();
  else run2([&](Out& o) {
    A1d f = mk1(fp); A2p P = blank2p(rows, cols); A2d W = blank2d(rows, cols);
    if (weighted) BSplSLib::GetPoles(f, P, W, udir); else BSplSLib::GetPoles(f, P, udir);
    o.dims(rows, cols); o.p3a2(P);
    if (weighted) { o.dims(rows, cols); o.a2(W); }
  });
  finish();
}
static void fam_set_get_poles() {
  for (int k = 0; k < 16; ++k) {
    int r = rint(1, 4), c = rint(1, 4); bool udir = k % 2 == 0;
    P3A2 p = mkP3A2(r, c, -5, 5);
    A2 w = mkA2(r + (k % 7 == 6 ? 1 : 0), c, 0.3, 3);
    emit_set_poles(p, nullptr, udir);
    emit_set_poles(p, &w, udir);
    int extra = k % 5 == 4 ? 3 : 0;
    Vd fp = rvec(r * c * 3 + extra, -4, 4);
    emit_get_poles(false, r, c, fp, udir);
    Vd fw = rvec(r * c * 4 + extra, 0.4, 3);
    emit_get_poles(true, r, c, fw, udir);
  }
  { P3A2 p = mkP3A2(3, 3, -5, 5); A2 w = mkA2(2, 3, 0.3, 3); emit_set_poles(p, &w, true); }
  emit_get_poles(false, 3, 3, rvec(26, -4, 4), true); emit_get_poles(true, 3, 3, rvec(35, 0.4, 3), false);
  { Vd fw = rvec(24, 0.4, 3); fw[7] = 0.0; emit_get_poles(true, 2, 3, fw, true); }
}

// =================================================================================
// BSplSLib: knot operations on surfaces
// =================================================================================
// A pole grid along direction `udir` (rows) or the other (columns) with the structure `s` and `other` poles across.
struct Grid { P3A2 p; bool hasW = false; A2 w; };
static Grid mk_grid(const KS& s, bool udir, int other, int wmode) {
  Grid g; int np = ks_np(s);
  g.p = udir ? mkP3A2(np, other, -5, 5) : mkP3A2(other, np, -5, 5);
  if (wmode) { g.hasW = true; g.w = mkA2(g.p.rows, g.p.cols, 0.4, 2.5); if (wmode == 2) { double c = rnd(0.5, 2); for (double& x : g.w.v) x = c; } }
  return g;
}
static bool grid_ok(const KS& s, const Grid& g, bool udir) {
  return (udir ? g.p.rows : g.p.cols) == ks_np(s) && (!g.hasW || (g.w.rows == g.p.rows && g.w.cols == g.p.cols));
}
struct SlibOut { A2p p; A2d w; bool hasW; Vd k; Vi m; };
// Runs BSplSLib::InsertKnots with the sizes PrepareInsertKnots gives; false when it refuses.
static bool slib_insert(bool udir, int deg, bool per, const Grid& g, const KS& s, const Vd& ak, const Vi* am, double eps, bool add, SlibOut& out) {
  A1d k = mk1(s.k), a = mk1(ak); A1i m = mk1i(s.m), aam = mk1i(am ? *am : Vi{1});
  int nbp = 0, nbk = 0;
  if (!BSplCLib::PrepareInsertKnots(deg, per, k, m, a, am ? &aam : nullptr, nbp, nbk, eps, add)) return false;
  A2p P = mk2p(g.p); A2d W = g.hasW ? mk2d(g.w) : A2d(1, 1, 1, 1);
  int nr = udir ? nbp : g.p.rows, nc = udir ? g.p.cols : nbp;
  out.p = blank2p(nr, nc); out.w = blank2d(nr, nc); out.hasW = g.hasW;
  A1d NK(1, nbk); A1i NM(1, nbk);
  BSplSLib::InsertKnots(udir, deg, per, P, g.hasW ? &W : nullptr, k, m, a, am ? &aam : nullptr, out.p, g.hasW ? &out.w : nullptr, NK, NM, eps, add);
  out.k.clear(); out.m.clear();
  for (int i = 1; i <= nbk; ++i) { out.k.push_back(NK(i)); out.m.push_back(NM(i)); }
  return true;
}
static void out_slib(Out& o, const SlibOut& r) {
  o.dims(r.p.ColLength(), r.p.RowLength()); o.p3a2(r.p);
  if (r.hasW) { o.dims(r.w.ColLength(), r.w.RowLength()); o.a2(r.w); }
  o.fs(r.k); o.is(r.m);
}
static void emit_slib_insert(bool udir, int deg, bool per, const Grid& g, const KS& s, const Vd& ak, const Vi* am, double eps, bool add) {
  begin("bsplslib_insert_knots", "i i i p3a2 oarr2 fs is fs ois f i");
  W_i(udir); W_i(deg); W_i(per); W_p3a2(g.p); W_oarr2(g.hasW ? &g.w : nullptr); W_fs(s.k); W_is(s.m); W_fs(ak); W_ois(am); W_f(eps); W_i(add); bar();
  bool bad = !structure_ok(deg, per, s.k, s.m) || ak.empty() || (am && am->size() != ak.size()) || !grid_ok(s, g, udir);
  if (!bad) { SlibOut tmp; A1d k = mk1(s.k), a = mk1(ak); A1i m = mk1i(s.m), aam = mk1i(am ? *am : Vi{1}); int nbp, nbk;
              bad = !BSplCLib::PrepareInsertKnots(deg, per, k, m, a, am ? &aam : nullptr, nbp, nbk, eps, add); }
  if (bad) { unchecked(); finish(); return; }
  { // see emit_insert: inconsistent PrepareInsertKnots / InsertKnots cases are not in the golden file
    SlibOut r; slib_insert(udir, deg, per, g, s, ak, am, eps, add, r);
    for (int i = 1; i <= r.p.ColLength(); ++i) for (int j = 1; j <= r.p.RowLength(); ++j) if (r.p(i, j).X() == 7.5 && r.p(i, j).Y() == 7.5 && r.p(i, j).Z() == 7.5) { buf.clear(); return; }
  }
  run2([&](Out& o) { SlibOut r; slib_insert(udir, deg, per, g, s, ak, am, eps, add, r); out_slib(o, r); });
  finish();
}
static void fam_slib_insert() {
  for (int k = 0; k < 18; ++k) {
    int deg = rint(1, 4); bool per = k % 3 == 2, udir = k % 2 == 0;
    KS s = mk_ks(deg, per, rint(1, 3), k % 4, k % 7 == 6 && !per);
    Grid g = mk_grid(s, udir, rint(1, 3), k % 4 == 1 ? 1 : (k % 4 == 3 ? 2 : 0));
    bool add = k % 5 != 4;
    Vd ak = gen_add_knots(s, rint(1, 2), k % 2, add);
    Vi am = gen_add_mults(ak.size(), deg);
    emit_slib_insert(udir, deg, per, g, s, ak, k % 3 == 1 ? nullptr : &am, TOLS[k % 4], add);
  }
  { KS s = mk_ks(3, false, 2, 0); Grid g = mk_grid(s, true, 2, 0); emit_slib_insert(true, 3, false, g, s, Vd{s.k[0] - 1}, nullptr, 0.0, true); }                // refused
  { KS s = mk_ks(3, false, 2, 0); Grid g = mk_grid(s, true, 2, 0); g.p = mkP3A2(g.p.rows + 1, 2, -5, 5); emit_slib_insert(true, 3, false, g, s, Vd{s.k[1] + 0.1}, nullptr, 0.0, true); } // wrong poles
}
static void emit_slib_remove(bool udir, int index, int mult, int deg, bool per, const Grid& g, const KS& s, double tol) {
  begin("bsplslib_remove_knot", "i i i i i p3a2 oarr2 fs is f");
  W_i(udir); W_i(index); W_i(mult); W_i(deg); W_i(per); W_p3a2(g.p); W_oarr2(g.hasW ? &g.w : nullptr); W_fs(s.k); W_is(s.m); W_f(tol); bar();
  bool bad = !structure_ok(deg, per, s.k, s.m) || !grid_ok(s, g, udir);
  int nk = (int)s.k.size(), fi = 0, la = 0, depth = 0; bool inrange = false;
  if (!bad) {
    if (per) { fi = 1; la = nk; } else { A1i mm = mk1i(s.m); fi = BSplCLib::FirstUKnotIndex(deg, mm) + 1; la = BSplCLib::LastUKnotIndex(deg, mm) - 1; }
    inrange = index >= fi && index <= la;
    if (inrange) { int the = (per && index == fi) ? la : index; depth = s.m[the - 1] - mult; if (mult < 0 || depth < 1) bad = true; }
  }
  if (bad) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    A1d k = mk1(s.k); A1i m = mk1i(s.m); A2p P = mk2p(g.p); A2d W = g.hasW ? mk2d(g.w) : A2d(1, 1, 1, 1);
    int nbk = (mult > 0 || !inrange) ? nk : nk - 1, d = inrange ? depth : 0;
    int nr = udir ? g.p.rows - d : g.p.rows, nc = udir ? g.p.cols : g.p.cols - d;
    A2p NP = blank2p(nr, nc); A2d NW = blank2d(nr, nc); A1d NK(1, nbk); A1i NM(1, nbk);
    bool ok = BSplSLib::RemoveKnot(udir, index, mult, deg, per, P, g.hasW ? &W : nullptr, k, m, NP, g.hasW ? &NW : nullptr, NK, NM, tol);
    o.i(ok);
    if (ok) {
      o.dims(nr, nc); o.p3a2(NP);
      if (g.hasW) { o.dims(nr, nc); o.a2(NW); }
      for (int i = 1; i <= nbk; ++i) o.f(NK(i));
      for (int i = 1; i <= nbk; ++i) o.i(NM(i));
    }
  });
  finish();
}
static void fam_slib_remove() {
  int made = 0;
  for (int k = 0; made < 17 && k < 300; ++k) {
    int deg = rint(1, 4); bool per = k % 3 == 2, udir = k % 2 == 0;
    KS base = mk_ks(deg, per, rint(2, 3), k % 4, false);
    Grid g = mk_grid(base, udir, rint(1, 3), k % 4 == 1 ? 1 : (k % 4 == 3 ? 2 : 0));
    if (k % 4 == 3) { // random poles
      int fi, la; ks_fl(base, fi, la);
      int idx = per ? rint(1, (int)base.k.size()) : rint(fi + 1, std::max(fi + 1, la - 1));
      emit_slib_remove(udir, idx, std::max(0, base.m[idx - 1] - 1), deg, per, g, base, k % 8 == 3 ? 1e3 : 1e-6); ++made; continue;
    }
    int fi, la; ks_fl(base, fi, la);
    double u = k % 4 == 0 ? rnd(base.k[fi - 1] + 1e-3, base.k[la - 1] - 1e-3) : base.k[rint(fi, std::max(fi, la - 2))];
    int depth = rint(1, std::max(1, std::min(deg, 2)));
    SlibOut r; Vi am = {depth};
    if (!slib_insert(udir, deg, per, g, base, Vd{u}, &am, 0.0, true, r)) continue;
    if (!structure_ok(deg, per, r.k, r.m)) continue;
    int idx = 1; double best = 1e300;
    for (size_t i = 0; i < r.k.size(); ++i) if (std::fabs(r.k[i] - u) < best) { best = std::fabs(r.k[i] - u); idx = (int)i + 1; }
    Grid g2; g2.p.rows = r.p.ColLength(); g2.p.cols = r.p.RowLength();
    for (int a = 1; a <= g2.p.rows; ++a) for (int b = 1; b <= g2.p.cols; ++b) g2.p.v.push_back(r.p(a, b));
    g2.hasW = r.hasW;
    if (r.hasW) { g2.w.rows = g2.p.rows; g2.w.cols = g2.p.cols; for (int a = 1; a <= g2.p.rows; ++a) for (int b = 1; b <= g2.p.cols; ++b) g2.w.v.push_back(r.w(a, b)); }
    emit_slib_remove(udir, idx, std::max(0, r.m[idx - 1] - depth), deg, per, g2, ks_from(deg, per, r.k, r.m), 1e-6); ++made;
  }
  { KS s = mk_ks(3, false, 3, 0); Grid g = mk_grid(s, true, 2, 0); emit_slib_remove(true, 1, 0, 3, false, g, s, 1e-6); emit_slib_remove(true, 30, 0, 3, false, g, s, 1e-6); }
  { KS s = mk_ks(3, false, 3, 1); Grid g = mk_grid(s, true, 2, 0); emit_slib_remove(true, 2, s.m[1], 3, false, g, s, 1e-6); }  // multiplicity not below the present one
}
static void emit_slib_inc(bool udir, int deg, int newdeg, bool per, const Grid& g, const KS& s) {
  begin("bsplslib_increase_degree", "i i i i p3a2 oarr2 fs is");
  W_i(udir); W_i(deg); W_i(newdeg); W_i(per); W_p3a2(g.p); W_oarr2(g.hasW ? &g.w : nullptr); W_fs(s.k); W_is(s.m); bar();
  bool bad = !structure_ok(deg, per, s.k, s.m) || newdeg < deg || newdeg > 25 || !grid_ok(s, g, udir);
  if (!bad && !per) { A1i mm = mk1i(s.m); bad = BSplCLib::FirstUKnotIndex(deg, mm) >= BSplCLib::LastUKnotIndex(deg, mm); }
  int nbp = 0, nk = 0;
  if (!bad) { // probe the multiplicities of the result with one-pole flat arrays
    A1i mm = mk1i(s.m); A1d k = mk1(s.k), P = mk1(Vd(ks_np(s), 0.0));
    nk = BSplCLib::IncreaseDegreeCountKnots(deg, newdeg, per, mm);
    A1d NP(1, 1), NK(1, nk); A1i NM(1, nk);
    BSplCLib::IncreaseDegree(deg, newdeg, per, 1, P, k, mm, NP, NK, NM);
    nbp = BSplCLib::NbPoles(newdeg, per, NM);
    if (nbp < 1) bad = true;
  }
  if (bad) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    A1d k = mk1(s.k); A1i m = mk1i(s.m); A2p P = mk2p(g.p); A2d W = g.hasW ? mk2d(g.w) : A2d(1, 1, 1, 1);
    int nr = udir ? nbp : g.p.rows, nc = udir ? g.p.cols : nbp;
    A2p NP = blank2p(nr, nc); A2d NW = blank2d(nr, nc); A1d NK(1, nk); A1i NM(1, nk);
    BSplSLib::IncreaseDegree(udir, deg, newdeg, per, P, g.hasW ? &W : nullptr, k, m, NP, g.hasW ? &NW : nullptr, NK, NM);
    o.dims(nr, nc); o.p3a2(NP);
    if (g.hasW) { o.dims(nr, nc); o.a2(NW); }
    for (int i = 1; i <= nk; ++i) o.f(NK(i));
    for (int i = 1; i <= nk; ++i) o.i(NM(i));
  });
  finish();
}
static void fam_slib_inc() {
  for (int k = 0; k < 17; ++k) {
    int deg = rint(1, 4); bool per = k % 3 == 2, udir = k % 2 == 0;
    KS s = mk_ks(deg, per, rint(1, 3), k % 4, k % 6 == 5 && !per);
    Grid g = mk_grid(s, udir, rint(1, 3), per ? 0 : (k % 4 == 1 ? 1 : (k % 4 == 3 ? 2 : 0))); // OCCT's periodic elevation can leave zero poles (and zero weights)
    emit_slib_inc(udir, deg, deg + (k % 5 == 4 ? 2 : 1), per, g, s);
  }
  { KS s = mk_ks(3, false, 2, 0); Grid g = mk_grid(s, true, 2, 0); emit_slib_inc(true, 3, 2, false, g, s); g.p = mkP3A2(g.p.rows - 1, 2, -5, 5); emit_slib_inc(true, 3, 4, false, g, s); }
}
static void emit_slib_unper(bool udir, int deg, const Grid& g, const KS& s) {
  begin("bsplslib_unperiodize", "i i is fs p3a2 oarr2");
  W_i(udir); W_i(deg); W_is(s.m); W_fs(s.k); W_p3a2(g.p); W_oarr2(g.hasW ? &g.w : nullptr); bar();
  bool bad = !structure_ok(deg, true, s.k, s.m) || !grid_ok(s, g, udir);
  if (bad) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    A1i m = mk1i(s.m); A1d k = mk1(s.k); A2p P = mk2p(g.p); A2d W = g.hasW ? mk2d(g.w) : A2d(1, 1, 1, 1);
    int nk = 0, np = 0; BSplCLib::PrepareUnperiodize(deg, m, nk, np);
    int nr = udir ? np : g.p.rows, nc = udir ? g.p.cols : np;
    A1i NM(1, nk); A1d NK(1, nk); A2p NP = blank2p(nr, nc); A2d NW = blank2d(nr, nc);
    BSplSLib::Unperiodize(udir, deg, m, k, P, g.hasW ? &W : nullptr, NM, NK, NP, g.hasW ? &NW : nullptr);
    o.is(Vi()); for (int i = 1; i <= nk; ++i) o.i(NM(i));
    for (int i = 1; i <= nk; ++i) o.f(NK(i));
    o.dims(nr, nc); o.p3a2(NP);
    if (g.hasW) { o.dims(nr, nc); o.a2(NW); }
  });
  finish();
}
static void fam_slib_unper() {
  for (int k = 0; k < 17; ++k) {
    int deg = rint(1, 4); bool udir = k % 2 == 0;
    KS s = mk_ks(deg, true, rint(1, 3), k % 4);
    Grid g = mk_grid(s, udir, rint(1, 3), k % 4 == 1 ? 1 : (k % 4 == 3 ? 2 : 0));
    emit_slib_unper(udir, deg, g, s);
  }
  { KS s = mk_ks(3, false, 2, 0); Grid g = mk_grid(s, true, 2, 0); emit_slib_unper(true, 3, g, s); }
  { KS s = mk_ks(3, true, 2, 1); Grid g = mk_grid(s, true, 2, 0); g.p = mkP3A2(g.p.rows + 1, 2, -5, 5); emit_slib_unper(true, 3, g, s); }
}

// =================================================================================
// BSplSLib: caches (Taylor expansion of a span)
// =================================================================================
// a surface with flat knots in both directions, small
static SurfIn gen_flat_surf(int k, int maxdeg = 4) {
  int ud = rint(1, maxdeg), vd = rint(1, maxdeg);
  if (k % 5 == 4) vd = ud;
  bool uper = k % 9 == 5, vper = k % 11 == 7;
  KS u = mk_ks(ud, uper, coin(0.6) ? 1 : 2, k % 4), v = mk_ks(vd, vper, coin(0.6) ? 1 : 2, (k / 3) % 4);
  static const int WM[] = {0, 1, 0, 3, 4, 5, 1, 2};
  SurfIn s = mk_surf(u, v, WM[k % 8], true, true);
  s.urat = s.vrat = s.hasW;
  return s;
}
static void emit_build_cache(bool array_version, double u, double v, double us, double vs, const SurfIn& s, int ui, int vi) {
  begin(array_version ? "bsplslib_build_cache_array" : "bsplslib_build_cache", "f f f f i i i i i i fs fs p3a2 oarr2");
  W_f(u); W_f(v); W_f(us); W_f(vs); W_i(s.uper); W_i(s.vper); W_i(s.ud); W_i(s.vd); W_i(ui); W_i(vi);
  W_fs(s.uk); W_fs(s.vk); W_p3a2(s.poles); W_oarr2(s.hasW ? &s.w : nullptr); bar();
  if (s.ud < 1 || s.ud > 25 || s.vd < 1 || s.vd > 25) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    OccSurf c(s);
    int mn = std::min(s.ud, s.vd), mx = std::max(s.ud, s.vd);
    if (array_version) {
      int sh = s.hasW ? 4 : 3;
      A2d arr(1, mx + 1, 1, (mn + 1) * sh); arr.Init(SENT);
      BSplSLib::BuildCache(u, v, us, vs, s.uper, s.vper, s.ud, s.vd, ui, vi, c.uk, c.vk, c.poles, c.W(), arr);
      o.dims(mx + 1, (mn + 1) * sh); o.a2(arr);
    } else {
      A2p cp = blank2p(mx + 1, mn + 1); A2d cw = blank2d(mx + 1, mn + 1);
      BSplSLib::BuildCache(u, v, us, vs, s.uper, s.vper, s.ud, s.vd, ui, vi, c.uk, c.vk, c.poles, c.W(), cp, s.hasW ? &cw : nullptr);
      o.dims(mx + 1, mn + 1); o.p3a2(cp);
      if (s.hasW) { o.dims(mx + 1, mn + 1); o.a2(cw); }
    }
  });
  finish();
}
static void fam_build_cache(bool array_version) {
  for (int k = 0; k < 15; ++k) {
    SurfIn s = gen_flat_surf(k + (array_version ? 3 : 0), 3);
    double u = pick_u(s.u, k % 4 == 3 ? 3 : (k % 4 == 2 ? 1 : 0)), v = pick_u(s.v, k % 5 == 3 ? 3 : 0);
    int ui = k % 3 == 1 ? pick_index(s.u, u, true, 1) : 0, vi = k % 4 == 1 ? pick_index(s.v, v, true, 1) : 0;
    emit_build_cache(array_version, u, v, rnd(0.2, 2), rnd(0.2, 2), s, ui, vi);
  }
  { KS u = mk_ks(1, false, 1, 0), v = mk_ks(25, false, 1, 0); SurfIn s = mk_surf(u, v, 0, true, true); emit_build_cache(array_version, pick_any(u), pick_any(v), 1.0, 0.5, s, 0, 0); }
  { KS u = mk_ks(26, false, 1, 0), v = mk_ks(2, false, 1, 0); SurfIn s = mk_surf(u, v, 0, true, true); emit_build_cache(array_version, 0.5, 0.5, 1.0, 0.5, s, 0, 0); }
}

// cache coefficient arrays: (max degree + 1) rows by (min degree + 1) columns
static void gen_coefs(int ud, int vd, bool rational, P3A2& p, A2& w) {
  int mn = std::min(ud, vd), mx = std::max(ud, vd);
  p = mkP3A2(mx + 1, mn + 1, -2, 2);
  if (rational) { w = mkA2(mx + 1, mn + 1, -0.15, 0.15); w.v[0] = rnd(1.0, 2.0); }
}
static void emit_cache_d(int kind, double u, double v, int ud, int vd, double ucp, double vcp, double us, double vs, const P3A2& p, const A2* w) {
  static const char* const N[] = {"bsplslib_cache_d0", "bsplslib_cache_d1", "bsplslib_cache_d2"};
  begin(N[kind], "f f i i f f f f p3a2 oarr2");
  W_f(u); W_f(v); W_i(ud); W_i(vd); W_f(ucp); W_f(vcp); W_f(us); W_f(vs); W_p3a2(p); W_oarr2(w); bar();
  long need = (long)(std::max(ud, vd) + 1) * (std::min(ud, vd) + 1);
  if (ud < 1 || vd < 1 || ud > 25 || vd > 25 || (long)p.v.size() != need || (w && (long)w->v.size() != need) || us == 0.0 || vs == 0.0) unchecked();
  else run2([&](Out& o) {
    A2p P = mk2p(p); A2d W = w ? mk2d(*w) : A2d(1, 1, 1, 1);
    gp_Pnt pt; gp_Vec a, b, c, d, e;
    if (kind == 0) { BSplSLib::CacheD0(u, v, ud, vd, ucp, vcp, us, vs, P, w ? &W : nullptr, pt); o.p(pt); }
    else if (kind == 1) { BSplSLib::CacheD1(u, v, ud, vd, ucp, vcp, us, vs, P, w ? &W : nullptr, pt, a, b); o.p(pt); o.v(a); o.v(b); }
    else { BSplSLib::CacheD2(u, v, ud, vd, ucp, vcp, us, vs, P, w ? &W : nullptr, pt, a, b, c, d, e); o.p(pt); o.v(a); o.v(b); o.v(c); o.v(d); o.v(e); }
  });
  finish();
}
static void fam_cache_d(int kind) {
  for (int k = 0; k < 16; ++k) {
    int ud = rint(1, 4), vd = rint(1, 4);
    if (k % 4 == 3) vd = ud;
    if (k % 9 == 8) { ud = rint(5, 9); vd = rint(1, 3); }
    P3A2 p; A2 w; bool rat = k % 3 != 0; gen_coefs(ud, vd, rat, p, w);
    double ucp = rnd(-1, 1), vcp = rnd(-1, 1), us = rnd(0.2, 2), vs = rnd(0.2, 2);
    emit_cache_d(kind, ucp + rnd(-1, 1) * us, vcp + rnd(-1, 1) * vs, ud, vd, ucp, vcp, us, vs, p, rat ? &w : nullptr);
  }
  { P3A2 p; A2 w; gen_coefs(25, 2, true, p, w); emit_cache_d(kind, 0.3, 0.2, 25, 2, 0.0, 0.0, 1.0, 1.0, p, &w); }
  { P3A2 p; A2 w; gen_coefs(2, 2, true, p, w); emit_cache_d(kind, 0.3, 0.2, 2, 2, 0.0, 0.0, 0.0, 1.0, p, &w); emit_cache_d(kind, 0.3, 0.2, 2, 2, 0.0, 0.0, 1.0, 1.0, p, nullptr);
    emit_cache_d(kind, 0.3, 0.2, 2, 3, 0.0, 0.0, 1.0, 1.0, p, &w); emit_cache_d(kind, 0.3, 0.2, 0, 2, 0.0, 0.0, 1.0, 1.0, p, &w); }
}
static void emit_coefs_d(int kind, double u, double v, const P3A2& p, const A2* w) {
  static const char* const N[] = {"bsplslib_coefs_d0", "bsplslib_coefs_d1", "bsplslib_coefs_d2"};
  begin(N[kind], "f f p3a2 oarr2");
  W_f(u); W_f(v); W_p3a2(p); W_oarr2(w); bar();
  if (p.rows < 2 || p.cols < 2 || (w && (w->rows != p.rows || w->cols != p.cols))) unchecked();
  else run2([&](Out& o) {
    A2p P = mk2p(p); A2d W = w ? mk2d(*w) : A2d(1, 1, 1, 1);
    gp_Pnt pt; gp_Vec a, b, c, d, e;
    if (kind == 0) { BSplSLib::CoefsD0(u, v, P, w ? &W : nullptr, pt); o.p(pt); }
    else if (kind == 1) { BSplSLib::CoefsD1(u, v, P, w ? &W : nullptr, pt, a, b); o.p(pt); o.v(a); o.v(b); }
    else { BSplSLib::CoefsD2(u, v, P, w ? &W : nullptr, pt, a, b, c, d, e); o.p(pt); o.v(a); o.v(b); o.v(c); o.v(d); o.v(e); }
  });
  finish();
}
static void fam_coefs_d(int kind) {
  for (int k = 0; k < 16; ++k) {
    int r = rint(2, 4), c = rint(2, 4);
    if (k % 4 != 3 && c > r) std::swap(r, c);    // the usual shape of PolesCoefficients (rows >= columns); the others only now and then
    P3A2 p = mkP3A2(r, c, -2, 2); A2 w;
    bool rat = k % 3 != 0;
    if (rat) { w = mkA2(r, c, -0.15, 0.15); w.v[0] = rnd(1.0, 2.0); }
    emit_coefs_d(kind, rnd(-1, 1), rnd(-1, 1), p, rat ? &w : nullptr);
  }
  { P3A2 p = mkP3A2(3, 3, -2, 2); A2 w = mkA2(2, 3, 0.5, 1); emit_coefs_d(kind, 0.3, 0.4, p, &w); emit_coefs_d(kind, 0.3, 0.4, mkP3A2(1, 3, -2, 2), nullptr); }
}
static void emit_poles_coefs(bool nw, const P3A2& p, const A2* w) {
  begin(nw ? "bsplslib_poles_coefficients_nw" : "bsplslib_poles_coefficients", nw ? "p3a2" : "p3a2 oarr2");
  W_p3a2(p); if (!nw) W_oarr2(w); bar();
  if (p.rows < 2 || p.cols < 2 || (w && (w->rows != p.rows || w->cols != p.cols))) unchecked();
  else run2([&](Out& o) {
    A2p P = mk2p(p); A2d W = w ? mk2d(*w) : A2d(1, 1, 1, 1);
    A2p CP = blank2p(p.rows, p.cols); A2d CW = blank2d(p.rows, p.cols);
    if (nw) BSplSLib::PolesCoefficients(P, CP); else BSplSLib::PolesCoefficients(P, w ? &W : nullptr, CP, w ? &CW : nullptr);
    o.dims(p.rows, p.cols); o.p3a2(CP);
    if (w && !nw) { o.dims(p.rows, p.cols); o.a2(CW); }
  });
  finish();
}
static void fam_poles_coefs(bool nw) {
  for (int k = 0; k < 16; ++k) {
    int r = rint(2, 4), c = rint(2, 4);
    if (k % 5 == 4) c = r;
    P3A2 p = mkP3A2(r, c, -4, 4); A2 w = mkA2(r, c, 0.4, 2.5);
    if (k % 6 == 5) { double x = rnd(0.5, 2); for (double& y : w.v) y = x; }
    emit_poles_coefs(nw, p, (!nw && k % 3 != 0) ? &w : nullptr);
  }
  emit_poles_coefs(nw, mkP3A2(8, 2, -4, 4), nullptr); emit_poles_coefs(nw, mkP3A2(2, 8, -4, 4), nullptr);
  emit_poles_coefs(nw, mkP3A2(1, 3, -4, 4), nullptr);
  if (!nw) { A2 w2 = mkA2(2, 3, 0.5, 1); P3A2 p2 = mkP3A2(3, 3, -4, 4); emit_poles_coefs(nw, p2, &w2); }   // weights of another shape
}

// =================================================================================
// BSplSLib: Resolution, Interpolate, FunctionMultiply, UnitWeights, MovePoint
// =================================================================================
static void emit_resolution(const SurfIn& s, double tol) {
  begin("bsplslib_resolution", "surf f");
  W_surf(s); W_f(tol); bar();
  if (surf_bad(s) || !s.hasUm || !s.hasVm) unchecked();
  else run2([&](Out& o) {
    OccSurf c(s); double ut = -1, vt = -1;
    BSplSLib::Resolution(c.poles, c.W(), c.uk, c.vk, c.um, c.vm, s.ud, s.vd, s.urat, s.vrat, s.uper, s.vper, tol, ut, vt);
    o.f(ut); o.f(vt);
  });
  finish();
}
static void fam_resolution() {
  for (int k = 0; k < 17; ++k) {
    SurfIn s = gen_surf(k + 1, false, true);
    emit_resolution(s, k % 4 == 0 ? 1.0 : rnd(1e-6, 0.5));
  }
  { KS u = mk_ks(2, false, 2, 0), v = mk_ks(2, false, 2, 0); SurfIn s = mk_surf(u, v, 0); for (auto& p : s.poles.v) p = gp_Pnt(1, 2, 3); emit_resolution(s, 0.1); }  // flat: zero tolerances
  { KS u = mk_ks(3, false, 1, 0), v = mk_ks(3, false, 1, 0); SurfIn s = mk_surf(u, v, 1); s.urat = true; s.vrat = false; emit_resolution(s, 0.1); s.urat = false; s.vrat = true; emit_resolution(s, 0.1); }
  { KS u = mk_ks(2, false, 1, 0), v = mk_ks(2, false, 1, 0); SurfIn s = mk_surf(u, v, 0); s.vrat = true; emit_resolution(s, 0.1); }
  { KS u = mk_ks(2, false, 1, 0), v = mk_ks(2, false, 1, 0); SurfIn s = mk_surf(u, v, 0, true, false); emit_resolution(s, 0.1); }
}

static bool interp_dir_ok(int deg, const Vd& fl, const Vd& par) { return interp_state(deg, fl, par, Vi(par.size(), 0)) != 1; }
static void emit_slib_interp(bool weighted, int ud, int vd, const Vd& uf, const Vd& vf, const Vd& up, const Vd& vp, const P3A2& p, const A2* w) {
  begin(weighted ? "bsplslib_interpolate_weighted" : "bsplslib_interpolate", weighted ? "i i fs fs fs fs p3a2 arr2" : "i i fs fs fs fs p3a2");
  W_i(ud); W_i(vd); W_fs(uf); W_fs(vf); W_fs(up); W_fs(vp); W_p3a2(p); if (weighted) W_arr2(*w); bar();
  bool bad = ud < 1 || vd < 1 || up.empty() || vp.empty() || p.rows != (int)up.size() || p.cols != (int)vp.size() ||
             (long)uf.size() != (long)up.size() + ud + 1 || (long)vf.size() != (long)vp.size() + vd + 1 ||
             (weighted && (w->rows != p.rows || w->cols != p.cols));
  if (!bad) bad = !interp_dir_ok(ud, uf, up) || !interp_dir_ok(vd, vf, vp);
  if (bad) { unchecked(); finish(); return; }
  GUARD(
    run2([&](Out& o) {
      A1d a = mk1(uf), b = mk1(vf), c = mk1(up), d = mk1(vp); A2p P = mk2p(p); A2d W = weighted ? mk2d(*w) : A2d(1, 1, 1, 1);
      int inv = -9;
      if (weighted) BSplSLib::Interpolate(ud, vd, a, b, c, d, P, W, inv); else BSplSLib::Interpolate(ud, vd, a, b, c, d, P, inv);
      o.i(inv); o.dims(p.rows, p.cols); o.p3a2(P);
      if (weighted) { o.dims(p.rows, p.cols); o.a2(W); }
    });
  )
  finish();
}
static Vd schoenberg(int deg, const Vd& f, int n) { A1d fk = mk1(f); A1d ps(1, n); BSplCLib::BuildSchoenbergPoints(deg, fk, ps); Vd r; for (int i = 1; i <= n; ++i) r.push_back(ps(i)); return r; }
static void fam_slib_interp(bool weighted) {
  for (int k = 0; k < 16; ++k) {
    int ud = rint(1, 3), vd = rint(1, 3);
    KS u = mk_ks(ud, false, coin(0.7) ? 1 : 2, k % 3), v = mk_ks(vd, false, coin(0.7) ? 1 : 2, (k / 2) % 3);
    Vd uf = flat_of(u), vf = flat_of(v);
    int nu = (int)uf.size() - ud - 1, nv = (int)vf.size() - vd - 1;
    Vd up = schoenberg(ud, uf, nu), vp = schoenberg(vd, vf, nv);
    P3A2 p = mkP3A2(nu, nv, -5, 5); A2 w = mkA2(nu, nv, 0.4, 2.5);
    emit_slib_interp(weighted, ud, vd, uf, vf, up, vp, p, &w);
  }
  { KS u = mk_ks(2, false, 2, 0), v = mk_ks(2, false, 2, 0); Vd uf = flat_of(u), vf = flat_of(v); int nu = (int)uf.size() - 3, nv = (int)vf.size() - 3;
    Vd up = schoenberg(2, uf, nu), vp = schoenberg(2, vf, nv); P3A2 p = mkP3A2(nu, nv, -5, 5); A2 w = mkA2(nu, nv, 0.4, 2.5);
    Vd up2(up); up2[1] = up2[0];                                        // equal parameters: singular system (OCCT throws)
    emit_slib_interp(weighted, 2, 2, uf, vf, up2, vp, p, &w);
    emit_slib_interp(weighted, 2, 2, uf, vf, up, vp, mkP3A2(nu + 1, nv, -5, 5), &w);   // wrong poles
    emit_slib_interp(weighted, 2, 2, uf, vf, up, vp, p, &w); }
}

struct Evf { int kind; double c[4]; };
static void W_evf(const Evf& e) { int s = ntok; in(e.kind); for (double x : e.c) h(x); word("evf", s, 5); }
struct EvfFn : BSplSLib_EvaluatorFunction {
  Evf e;
  explicit EvfFn(const Evf& x) : e(x) {}
  void Evaluate(const int, const double u, const double v, double& r, int& err) const override {
    err = 0; r = 0.0;
    switch (e.kind) {
      case 0: r = e.c[0]; break;
      case 1: r = e.c[0] + e.c[1] * u + e.c[2] * v; break;
      case 2: r = e.c[0] + e.c[1] * u * v + e.c[2] * u * u + e.c[3] * v * v; break;
      case 3: err = 1; break;
      default: r = e.c[0] + e.c[1] * u; err = u > e.c[2] ? 1 : 0; break;
    }
  }
};
static void emit_fm(const Evf& e, const SurfIn& s, int und, int vnd, const Vd& uf, const Vd& vf) {
  begin("bsplslib_function_multiply", "evf i i fs fs ois ois p3a2 oarr2 fs fs i i");
  W_evf(e); W_i(s.ud); W_i(s.vd); W_fs(s.uk); W_fs(s.vk); W_ois(s.hasUm ? &s.um : nullptr); W_ois(s.hasVm ? &s.vm : nullptr);
  W_p3a2(s.poles); W_oarr2(s.hasW ? &s.w : nullptr); W_fs(uf); W_fs(vf); W_i(und); W_i(vnd); bar();
  int nu = (int)uf.size() - und - 1, nv = (int)vf.size() - vnd - 1;
  if (!s.hasW || s.ud < 1 || s.vd < 1 || s.ud > 25 || s.vd > 25 || und < 1 || vnd < 1 || nu < 1 || nv < 1) { unchecked(); finish(); return; }
  bool bad = !interp_dir_ok(und, uf, schoenberg(und, uf, nu)) || !interp_dir_ok(vnd, vf, schoenberg(vnd, vf, nv));
  if (bad) { unchecked(); finish(); return; }
  GUARD(
    run2([&](Out& o) {
      OccSurf c(s); EvfFn fn(e);
      A1d a = mk1(uf), b = mk1(vf);
      A2p num = blank2p(nu, nv); A2d den = blank2d(nu, nv); int st = -9;
      BSplSLib::FunctionMultiply(fn, s.ud, s.vd, c.uk, c.vk, c.UM(), c.VM(), c.poles, c.W(), a, b, und, vnd, num, den, st);
      o.i(st); o.dims(nu, nv); o.p3a2(num); o.dims(nu, nv); o.a2(den);
    });
  )
  finish();
}
static void fam_fm() {
  for (int k = 0; k < 16; ++k) {
    int ud = rint(1, 3), vd = rint(1, 3);
    KS u = mk_ks(ud, false, coin(0.7) ? 1 : 2, k % 3), v = mk_ks(vd, false, coin(0.7) ? 1 : 2, (k / 2) % 3);
    SurfIn s = mk_surf(u, v, k % 4 == 3 ? 3 : 1, k % 7 == 6, k % 5 == 4);
    int du = rint(0, 1), dv = rint(0, 1);
    KS nu = u, nv = v; nu.deg = ud + du; nv.deg = vd + dv;
    for (size_t i = 0; i < nu.m.size(); ++i) nu.m[i] = (i == 0 || i + 1 == nu.m.size()) ? nu.deg + 1 : std::min(nu.deg, u.m[i] + du);
    for (size_t i = 0; i < nv.m.size(); ++i) nv.m[i] = (i == 0 || i + 1 == nv.m.size()) ? nv.deg + 1 : std::min(nv.deg, v.m[i] + dv);
    Evf e; e.kind = k % 3; for (double& x : e.c) x = rnd(0.2, 2.0);
    if (k == 7) { e.kind = 4; e.c[2] = 1e9; }
    emit_fm(e, s, nu.deg, nv.deg, flat_of(nu), flat_of(nv));
  }
  { KS u = mk_ks(2, false, 2, 0), v = mk_ks(2, false, 2, 0); SurfIn s = mk_surf(u, v, 1);
    Vd uf = flat_of(u), vf = flat_of(v);
    Evf e{3, {1, 1, 1, 1}}; emit_fm(e, s, 2, 2, uf, vf);                                    // the function reports an error: OCCT throws
    Evf e2{4, {1.0, 0.5, u.k[1], 0}}; emit_fm(e2, s, 2, 2, uf, vf);                         // error only for large u
    SurfIn s2 = s; s2.hasW = false; s2.urat = s2.vrat = false; emit_fm(Evf{0, {1, 0, 0, 0}}, s2, 2, 2, uf, vf);   // no weights: OCCT dereferences null
  }
}

static void emit_unit_weights(int nu, int nv) {
  begin("bsplslib_unit_weights", "i i");
  W_i(nu); W_i(nv); bar();
  if (nu < 1 || nv < 1) unchecked();
  else run2([&](Out& o) {
    A2d w = BSplSLib::UnitWeights(nu, nv);
    double sum = 0, mn = 1e300, mx = -1e300;
    for (int i = w.LowerRow(); i <= w.UpperRow(); ++i) for (int j = w.LowerCol(); j <= w.UpperCol(); ++j) { sum += w(i, j); mn = std::min(mn, w(i, j)); mx = std::max(mx, w(i, j)); }
    o.dims(w.ColLength(), w.RowLength()); o.f(w(w.LowerRow(), w.LowerCol())); o.f(w(w.UpperRow(), w.UpperCol())); o.f(sum); o.f(mn); o.f(mx);
  });
  finish();
}
static void fam_unit_weights() {
  int sz[][2] = {{1, 1}, {2, 3}, {3, 2}, {4, 4}, {5, 7}, {10, 10}, {1, 20}, {20, 1}, {3, 683}, {683, 3}, {2, 1024}, {2, 1025}, {1025, 2}, {1, 2049}, {1, 2050}, {45, 45}, {46, 46}, {7, 300}, {17, 17}};
  for (auto& s : sz) emit_unit_weights(s[0], s[1]);
  emit_unit_weights(0, 5); emit_unit_weights(5, 0); emit_unit_weights(-1, 3);
}

static void emit_move(double u, double v, const gp_Vec& d, int ui1, int ui2, int vi1, int vi2, int ud, int vd, bool rat, const P3A2& p, const A2& w, const Vd& uf, const Vd& vf) {
  begin("bsplslib_move_point", "f f vec i i i i i i i p3a2 arr2 fs fs");
  W_f(u); W_f(v); W_vec(d); W_i(ui1); W_i(ui2); W_i(vi1); W_i(vi2); W_i(ud); W_i(vd); W_i(rat); W_p3a2(p); W_arr2(w); W_fs(uf); W_fs(vf); bar();
  bool bad = ud < 1 || vd < 1 || ud > 25 || vd > 25 || (long)uf.size() != p.rows + ud + 1 || (long)vf.size() != p.cols + vd + 1 ||
             (rat && (w.rows != p.rows || w.cols != p.cols));
  if (bad) { unchecked(); finish(); return; }
  { // a displacement spread over poles whose basis functions are all zero divides by zero: not part of the golden file
    A2p P = mk2p(p); A2d W = mk2d(w); A1d a = mk1(uf), b = mk1(vf);
    A2p NP = blank2p(p.rows, p.cols); int f1 = -5, l1 = -5, f2 = -5, l2 = -5;
    BSplSLib::MovePoint(u, v, d, ui1, ui2, vi1, vi2, ud, vd, rat, P, W, a, b, f1, l1, f2, l2, NP);
    bool fin = true; for (int i = 1; i <= NP.ColLength(); ++i) for (int j = 1; j <= NP.RowLength(); ++j) fin = fin && std::isfinite(NP(i, j).X()) && std::isfinite(NP(i, j).Y()) && std::isfinite(NP(i, j).Z());
    if (!fin) { buf.clear(); return; }
  }
  run2([&](Out& o) {
    A2p P = mk2p(p); A2d W = mk2d(w); A1d a = mk1(uf), b = mk1(vf);
    A2p NP = blank2p(p.rows, p.cols); int f1 = -5, l1 = -5, f2 = -5, l2 = -5;
    BSplSLib::MovePoint(u, v, d, ui1, ui2, vi1, vi2, ud, vd, rat, P, W, a, b, f1, l1, f2, l2, NP);
    bool ok = !(f1 == 0 && l1 == 0 && f2 == 0 && l2 == 0);
    o.i(ok); o.i(f1); o.i(l1); o.i(f2); o.i(l2);
    if (ok) o.p3a2(NP);
  });
  finish();
}
static void fam_move() {
  for (int k = 0; k < 18; ++k) {
    int ud = rint(1, 3), vd = rint(1, 3);
    KS u = mk_ks(ud, false, coin(0.7) ? 1 : 2, k % 3), v = mk_ks(vd, false, coin(0.7) ? 1 : 2, (k / 2) % 3);
    Vd uf = flat_of(u), vf = flat_of(v);
    int nu = ks_np(u), nv = ks_np(v);
    P3A2 p = mkP3A2(nu, nv, -5, 5); A2 w = mkA2(nu, nv, 0.4, 2.5);
    int ranges[][2] = {{1, nu - 1}, {2, nu}, {2, nu - 1}, {1, nu}};
    int vr[][2] = {{1, nv - 1}, {2, nv}, {2, nv - 1}, {1, nv}};
    int a = k % 4, b = (k / 4) % 4;
    emit_move(pick_u(u, k % 3 == 2 ? 1 : 0), pick_u(v, k % 5 == 4 ? 2 : 0), gp_Vec(rnz(0.1, 2), rnz(0.1, 2), rnz(0.1, 2)), ranges[a][0], ranges[a][1], vr[b][0], vr[b][1], ud, vd, k % 2 == 1, p, w, uf, vf);
  }
  { KS u = mk_ks(2, false, 2, 0), v = mk_ks(2, false, 2, 0); Vd uf = flat_of(u), vf = flat_of(v); int nu = ks_np(u), nv = ks_np(v);
    P3A2 p = mkP3A2(nu, nv, -5, 5); A2 w = mkA2(nu, nv, 0.4, 2.5); gp_Vec d(1, 2, 3);
    emit_move(u.k[0] - 1.0, 0.5 * (v.k[0] + v.k[1]), d, 1, nu, 1, nv, 2, 2, false, p, w, uf, vf);   // outside the knots: the basis cannot be evaluated or is extrapolated
    emit_move(u.k[1], v.k[1], d, 1, nu, 1, nv, 2, 2, true, p, w, uf, vf);
    emit_move(u.k[1], v.k[1], d, 3, 2, 1, nv, 2, 2, true, p, w, uf, vf);                           // empty move range
    emit_move(u.k[1], v.k[1], d, 1, nu, 1, nv, 2, 2, true, p, mkA2(nu, nv - 1, 1, 2), uf, vf);     // weights of the wrong size
  }
}

// =================================================================================
// BSplSLib_Cache
// =================================================================================
// kinds: 0 IsCacheValid, 1 D0, 2 D1, 3 D2, 4 D0Local, 5 D1Local, 6 D2Local
static const char* const CLS_NAMES[] = {"bsplslib_cache_class_valid", "bsplslib_cache_class_d0", "bsplslib_cache_class_d1", "bsplslib_cache_class_d2",
                                        "bsplslib_cache_class_d0_local", "bsplslib_cache_class_d1_local", "bsplslib_cache_class_d2_local"};
static void emit_cls(int kind, const SurfIn& s, bool build, double u0, double v0, double u, double v) {
  begin(CLS_NAMES[kind], "surf i f f f f");
  W_surf(s); W_i(build); W_f(u0); W_f(v0); W_f(u); W_f(v); bar();
  if (s.ud < 1 || s.vd < 1 || s.ud > 25 || s.vd > 25 || (!build && kind != 0)) { unchecked(); finish(); return; }
  run2([&](Out& o) {
    OccSurf c(s);
    BSplSLib_Cache cache(s.ud, s.uper, c.uk, s.vd, s.vper, c.vk, c.W());
    if (build) cache.BuildCache(u0, v0, c.uk, c.vk, c.poles, c.W());
    gp_Pnt P; gp_Vec a, b, cc, d, e;
    switch (kind) {
      case 0: o.i(cache.IsCacheValid(u, v)); break;
      case 1: cache.D0(u, v, P); o.p(P); break;
      case 2: cache.D1(u, v, P, a, b); o.p(P); o.v(a); o.v(b); break;
      case 3: cache.D2(u, v, P, a, b, cc, d, e); o.p(P); o.v(a); o.v(b); o.v(cc); o.v(d); o.v(e); break;
      case 4: cache.D0Local(u, v, P); o.p(P); break;
      case 5: cache.D1Local(u, v, P, a, b); o.p(P); o.v(a); o.v(b); break;
      default: cache.D2Local(u, v, P, a, b, cc, d, e); o.p(P); o.v(a); o.v(b); o.v(cc); o.v(d); o.v(e); break;
    }
  });
  finish();
}
static void fam_cls(int kind) {
  for (int k = 0; k < (kind == 0 ? 20 : 16); ++k) {
    SurfIn s = gen_flat_surf(k + kind, 3);
    double u0 = pick_u(s.u, k % 3 == 2 ? 3 : 0), v0 = pick_u(s.v, k % 4 == 3 ? 3 : 0);
    double u, v;
    if (kind == 0) {
      int mode = k % 6;     // inside the built span, on its ends, in a neighbouring span, a few ulps from a knot, outside the range
      u = mode == 0 ? u0 : (mode == 1 ? pick_u(s.u, 1) : (mode == 2 ? pick_u(s.u, 4) : (mode == 3 ? pick_u(s.u, 3) : pick_u(s.u, 0))));
      v = mode == 0 ? v0 : (mode == 1 ? pick_u(s.v, 1) : (mode == 2 ? pick_u(s.v, 5) : pick_u(s.v, 0)));
      if (mode == 1 && coin()) { u = u0; }
    } else if (kind >= 4) { u = rnd(-1.3, 1.3); v = rnd(-1.3, 1.3); }
    else { u = coin(0.7) ? u0 + rnd(-0.1, 0.1) : pick_any(s.u); v = coin(0.7) ? v0 + rnd(-0.1, 0.1) : pick_any(s.v); }
    emit_cls(kind, s, !(kind == 0 && k % 9 == 8), u0, v0, u, v);
  }
  { KS u = mk_ks(25, false, 1, 0), v = mk_ks(1, false, 1, 0); SurfIn s = mk_surf(u, v, 1, true, true); s.urat = s.vrat = true; emit_cls(kind, s, true, 0.5, 0.5, 0.4, 0.6); }
  { KS u = mk_ks(26, false, 1, 0), v = mk_ks(1, false, 1, 0); SurfIn s = mk_surf(u, v, 0, true, true); emit_cls(kind, s, true, 0.5, 0.5, 0.4, 0.6); }
  if (kind != 0) { SurfIn s = gen_flat_surf(3, 3); emit_cls(kind, s, false, 0.5, 0.5, 0.5, 0.5); }       // evaluating a cache that was never built
}

// =================================================================================
int main() {
  // BSplCLib
  fam_eval_buffer(); fam_bohm_buffer(); fam_build_knots(); fam_pole_index(); fam_locate_flat(); fam_locate_range();
  fam_knot_seq_len(); fam_knot_seq(); fam_basis(); fam_schoenberg(); fam_interpolate();
  fam_prep_ins(); fam_insert(); fam_remove(); fam_inc_count(); fam_inc(); fam_prep_unper(); fam_unper(); fam_constants();
  // BSplSLib
  for (int kind = 0; kind < 7; ++kind) fam_eval(kind, 11);
  fam_rat_der(); fam_iso(); fam_reverse(); fam_is_rational(); fam_set_get_poles();
  fam_slib_insert(); fam_slib_remove(); fam_slib_inc(); fam_slib_unper();
  fam_build_cache(false); fam_build_cache(true);
  for (int kind = 0; kind < 3; ++kind) fam_cache_d(kind);
  for (int kind = 0; kind < 3; ++kind) fam_coefs_d(kind);
  fam_poles_coefs(false); fam_poles_coefs(true);
  fam_resolution(); fam_slib_interp(false); fam_slib_interp(true); fam_fm(); fam_unit_weights(); fam_move();
  for (int kind = 0; kind < 7; ++kind) fam_cls(kind);
  // every family must have at least 16 distinct lines (the input-less one is printed once)
  for (const std::string& f : fam_order)
    if (f != "bsplclib_constants" && fam_lines[f] < 16) { std::fprintf(stderr, "family %s has only %d lines\n", f.c_str(), fam_lines[f]); return 1; }
  if (std::getenv("BSPLSLIB_SUMMARY"))
    for (const std::string& f : fam_order) std::fprintf(stderr, "%-44s %4d\n", f.c_str(), fam_lines[f]);
  return 0;
}
