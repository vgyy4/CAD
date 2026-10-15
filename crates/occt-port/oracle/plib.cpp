// Deterministic reference calls for OCCT 8.0.1's PLib package: the static functions of `PLib`, and the
// classes `PLib_JacobiPolynomial` and `PLib_HermitJacobi`.
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints (and bools) are decimal. Every input of a call is printed on the
// left of " | " in the order named by the family's schema string (`begin(name, schema)`); tests/plib.rs reads
// them back with the reader method of the same name as each schema word. The oracle aborts if the words it
// printed differ from the schema (and if a fixed-size word printed the wrong number of tokens). Outputs are values,
// `throws` (OCCT raised an exception: only the explicit `throw` statements of the library count) or `unchecked`
// (OCCT 8.0.1 as built has the check compiled out, or has none; the numbers it returns, if it returns at all, are
// unspecified; the Rust API must return an error). Where evaluating the unchecked call would read or write outside
// an array (undefined behaviour, a crash is possible) the oracle does not call it and prints `unchecked`
// directly; where a call is memory-safe the oracle calls it, so that a check that does exist would show as `throws`.
//
// Arrays: OCCT arrays become slices. "fs" = int n followed by n floats. 2D arrays are row-major: "arr2" = rows cols
// followed by rows*cols floats; point arrays "p3s" / "p2s" = int n followed by n*3 / n*2 floats and "p3a2" = rows cols
// followed by rows*cols*3 floats. The optional weight arrays of PLib ("NULL pointer = no weights") are "ofs" =
// flag (0 or 1), and when the flag is 1 an "fs"; "oarr2" likewise with an "arr2". See docs/api-plib.md for the
// layouts the functions expect.
//
// Schema words (token counts): f=1 i=1 shape=1 (a GeomAbs_Shape ordinal 0..6: C0 G1 C1 G2 C2 C3 CN) fs p3s p2s arr2
// p3a2 ofs oarr2 = variable (see above).
//
// Every output float is finite: the oracle aborts on NaN or infinity.
#include <GeomAbs_Shape.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <PLib.hxx>
#include <PLib_HermitJacobi.hxx>
#include <PLib_JacobiPolynomial.hxx>
#include <Standard_Failure.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <math_Gauss.hxx>
#include <math_Matrix.hxx>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <set>
#include <string>
#include <vector>

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
static Vd rvec(int n, double lo, double hi) { Vd v; for (int i = 0; i < n; ++i) v.push_back(rnd(lo, hi)); return v; }
// coefficients that shrink with the index, so that degree reduction has something to find
static Vd rdecay(int n, int dim, double base) {
  Vd v;
  for (int i = 0; i < n; ++i) for (int d = 0; d < dim; ++d) v.push_back(rnz(0.2, 1.0) * std::pow(base, i));
  return v;
}
static const double EDGE_U[] = {0.0, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 1.5, -3.0, 1e-3, 0.999999999, 1.0000001,
                                -1.0000001, 7.5, -12.25, 0.1, 100.0, -0.999, 0.25, 3.0};
static double ru() { return rnd(0, 1) < 0.55 ? EDGE_U[rint(0, (int)(sizeof EDGE_U / sizeof EDGE_U[0]) - 1)] : rnd(-2.5, 2.5); }

// ---------------------------------------------------------------------------------
// record buffer
static std::string buf, cur_name, cur_schema;
static size_t mark = 0;
static int ntok = 0;
static bool outphase = false;
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
static void W_shape(int v) { int s = ntok; in(v); word("shape", s, 1); }
static void put_fs(const Vd& v) { in((long)v.size()); for (double x : v) h(x); }
static void W_fs(const Vd& v) { int s = ntok; put_fs(v); word("fs", s, 1 + (int)v.size()); }
static void put_p3s(const std::vector<gp_Pnt>& v) { in((long)v.size()); for (const gp_Pnt& p : v) { h(p.X()); h(p.Y()); h(p.Z()); } }
static void W_p3s(const std::vector<gp_Pnt>& v) { int s = ntok; put_p3s(v); word("p3s", s, 1 + 3 * (int)v.size()); }
static void put_p2s(const std::vector<gp_Pnt2d>& v) { in((long)v.size()); for (const gp_Pnt2d& p : v) { h(p.X()); h(p.Y()); } }
static void W_p2s(const std::vector<gp_Pnt2d>& v) { int s = ntok; put_p2s(v); word("p2s", s, 1 + 2 * (int)v.size()); }
// row-major 2D float array
struct A2 { int rows = 0, cols = 0; Vd v; double& at(int r, int c) { return v[(size_t)r * cols + c]; } double at(int r, int c) const { return v[(size_t)r * cols + c]; } };
static A2 mkA2(int rows, int cols, double lo, double hi) { A2 a; a.rows = rows; a.cols = cols; a.v = rvec(rows * cols, lo, hi); return a; }
static void put_arr2(const A2& a) { in(a.rows); in(a.cols); for (double x : a.v) h(x); }
static void W_arr2(const A2& a) { int s = ntok; put_arr2(a); word("arr2", s, 2 + (int)a.v.size()); }
struct P3A2 { int rows = 0, cols = 0; std::vector<gp_Pnt> v; };
static P3A2 mkP3A2(int rows, int cols, double lo, double hi) {
  P3A2 a; a.rows = rows; a.cols = cols;
  for (int i = 0; i < rows * cols; ++i) { double x = rnd(lo, hi), y = rnd(lo, hi), z = rnd(lo, hi); a.v.push_back(gp_Pnt(x, y, z)); }
  return a;
}
static void put_p3a2(const P3A2& a) { in(a.rows); in(a.cols); for (const gp_Pnt& p : a.v) { h(p.X()); h(p.Y()); h(p.Z()); } }
static void W_p3a2(const P3A2& a) { int s = ntok; put_p3a2(a); word("p3a2", s, 2 + 3 * (int)a.v.size()); }
// optional arrays: flag, then the array
static void W_ofs(const Vd* v) { int s = ntok; if (v) { in(1); put_fs(*v); } else in(0); word("ofs", s, -1); }
static void W_oarr2(const A2* a) { int s = ntok; if (a) { in(1); put_arr2(*a); } else in(0); word("oarr2", s, -1); }

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
  if (std::getenv("PLIB_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
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
#define GUARD(...) try { __VA_ARGS__ } catch (Standard_Failure&) { buf.resize(mark); buf += " throws"; }
static void unchecked() { buf.resize(mark); buf += " unchecked"; }

// ---------------------------------------------------------------------------------
// output helpers
static const double SENT = 1234.5678; // sentinel for result buffers
static void out(const Vd& v, size_t n) { for (size_t i = 0; i < n; ++i) h(v[i]); }
// the function must have written exactly the first `used` entries
static void check_buf(const Vd& v, size_t used, const char* what) {
  for (size_t i = 0; i < v.size(); ++i) {
    bool written = v[i] != SENT;
    if ((i < used) != written) {
      std::fprintf(stderr, "%s: entry %zu of %zu (used %zu) %s\n", what, i, v.size(), used, i < used ? "not written" : "written beyond");
      std::abort();
    }
  }
}
static Vd sbuf(size_t used) { return Vd(used + 8, SENT); }

// =================================================================================
// evaluation of polynomials
// =================================================================================
static void emit_eval_polynomial(double u, int der, int deg, int dim, const Vd& c) {
  begin("eval_polynomial", "f i i i fs");
  W_f(u); W_i(der); W_i(deg); W_i(dim); W_fs(c); bar();
  // negative orders / degrees / dimensions write before the result array; a short coefficient array is read past its end
  if (der < 0 || deg < 0 || dim < 1 || (long)c.size() < (long)(deg + 1) * dim) unchecked();
  else {
    size_t used = (size_t)(der + 1) * dim;
    Vd cc(c), res = sbuf(used); // cc has exactly the size of c: valgrind sees reads beyond it
    PLib::EvalPolynomial(u, der, deg, dim, cc[0], res[0]);
    check_buf(res, used, "EvalPolynomial");
    out(res, used);
  }
  finish();
}
static void fam_eval_polynomial() {
  // small dimensions, every degree, derivative orders around the special cases 0, 1, 2 and the general case
  for (int dim = 1; dim <= 3; ++dim)
    for (int deg = 0; deg <= 7; ++deg) {
      int ders[] = {0, 1, 2, 3, deg, deg + 1, deg + 2, rint(0, 6)};
      for (int k = 0; k < 3; ++k) {
        int der = ders[(k + deg + dim) % 8];
        emit_eval_polynomial(ru(), der, deg, dim, rvec((deg + 1) * dim, -4, 4));
      }
    }
  // larger dimensions (the unrolled path stops at 15, then the run-time loop is used)
  int dims[] = {4, 5, 6, 8, 14, 15, 16, 17, 20};
  for (int dim : dims)
    for (int k = 0; k < 3; ++k) {
      int deg = rint(0, 5), der = rint(0, 4);
      emit_eval_polynomial(ru(), der, deg, dim, rvec((deg + 1) * dim, -4, 4));
    }
  // high degree
  for (int k = 0; k < 4; ++k) emit_eval_polynomial(rnd(-1.3, 1.3), rint(0, 4), 25 + k, 2, rvec((26 + k) * 2, -1, 1));
  // a longer coefficient array: only the first (degree+1)*dimension values are used
  emit_eval_polynomial(0.75, 2, 3, 2, rvec(11, -2, 2));
  emit_eval_polynomial(-1.25, 0, 1, 3, rvec(9, -2, 2));
  // invalid
  emit_eval_polynomial(0.5, -1, 3, 2, rvec(8, -1, 1));
  emit_eval_polynomial(0.5, -4, 0, 1, rvec(1, -1, 1));
  emit_eval_polynomial(0.5, 1, -1, 2, rvec(4, -1, 1));
  emit_eval_polynomial(0.5, 1, 3, 0, rvec(4, -1, 1));
  emit_eval_polynomial(0.5, 1, 3, -2, rvec(4, -1, 1));
  emit_eval_polynomial(0.5, 1, 3, 2, rvec(5, -1, 1)); // 5 < 8 coefficients
  emit_eval_polynomial(0.5, 3, 4, 3, rvec(14, -1, 1)); // 14 < 15
  emit_eval_polynomial(0.5, 0, 2, 4, Vd());           // no coefficients
}

static void emit_nde(double u, int deg, int dim, int degdim, const Vd& c) {
  begin("no_derivative_eval_polynomial", "f i i i fs");
  W_f(u); W_i(deg); W_i(dim); W_i(degdim); W_fs(c); bar();
  // the function reads the values at degdim - k*dim + (0..dim) for k = 0..deg: they must lie inside the array
  long first = (long)degdim - (long)deg * dim;
  if (deg < 0 || dim < 1 || first < 0 || (long)degdim + dim > (long)c.size()) unchecked();
  else {
    Vd cc(c), res = sbuf(dim);
    PLib::NoDerivativeEvalPolynomial(u, deg, dim, degdim, cc[0], res[0]);
    check_buf(res, dim, "NoDerivativeEvalPolynomial");
    out(res, dim);
  }
  finish();
}
static void fam_no_derivative() {
  for (int dim = 1; dim <= 3; ++dim)
    for (int deg = 0; deg <= 8; ++deg)
      if (deg % 2 == dim % 2 || deg < 3) emit_nde(ru(), deg, dim, deg * dim, rvec((deg + 1) * dim, -4, 4));
  int dims[] = {4, 7, 12, 15, 16, 18, 25};
  for (int dim : dims) { int deg = rint(0, 4); emit_nde(ru(), deg, dim, deg * dim, rvec((deg + 1) * dim, -4, 4)); }
  // DegreeDimension different from Degree*Dimension (the top Degree+1 groups of a longer array)
  for (int k = 0; k < 6; ++k) {
    int dim = rint(1, 3), deg = rint(0, 3), extra = rint(1, 3);
    emit_nde(ru(), deg, dim, (deg + extra) * dim, rvec((deg + extra + 1) * dim, -3, 3));
  }
  // a trailing group that is not read
  emit_nde(0.4, 2, 2, 4, rvec(10, -2, 2));
  // invalid
  emit_nde(0.5, -1, 2, 0, rvec(4, -1, 1));
  emit_nde(0.5, 2, 0, 0, rvec(4, -1, 1));
  emit_nde(0.5, 3, 2, 4, rvec(8, -1, 1));  // starts before the array
  emit_nde(0.5, 2, 2, 8, rvec(8, -1, 1));  // ends after the array
  emit_nde(0.5, 2, 3, 6, rvec(8, -1, 1));
}

static void emit_poly2var(double u, double v, int ud, int vd, int udeg, int vdeg, int dim, const Vd& c) {
  begin("eval_poly2var", "f f i i i i i fs");
  W_f(u); W_f(v); W_i(ud); W_i(vd); W_i(udeg); W_i(vdeg); W_i(dim); W_fs(c); bar();
  if (ud < 0 || vd < 0 || udeg < 0 || vdeg < 0 || dim < 1 || (long)c.size() < (long)(udeg + 1) * (vdeg + 1) * dim) unchecked();
  else {
    Vd cc(c), res = sbuf(dim);
    PLib::EvalPoly2Var(u, v, ud, vd, udeg, vdeg, dim, cc[0], res[0]);
    check_buf(res, dim, "EvalPoly2Var");
    out(res, dim);
  }
  finish();
}
static void fam_poly2var() {
  for (int k = 0; k < 70; ++k) {
    int udeg = rint(0, 4), vdeg = rint(0, 4), dim = k % 7 == 6 ? rint(4, 6) : rint(1, 3);
    int ud = k % 3 == 0 ? rint(0, 3) : rint(0, udeg + 1), vd = k % 4 == 0 ? rint(0, 3) : rint(0, vdeg + 1);
    emit_poly2var(ru(), ru(), ud, vd, udeg, vdeg, dim, rvec((udeg + 1) * (vdeg + 1) * dim, -3, 3));
  }
  // dimension large enough for the run-time loops: (vdeg+1)*dim > 15
  emit_poly2var(0.3, -0.7, 1, 1, 2, 4, 4, rvec(3 * 5 * 4, -2, 2));
  emit_poly2var(0.3, -0.7, 2, 2, 3, 3, 5, rvec(4 * 4 * 5, -2, 2));
  // invalid
  emit_poly2var(0.3, 0.4, -1, 0, 2, 2, 2, rvec(18, -1, 1));
  emit_poly2var(0.3, 0.4, 0, -2, 2, 2, 2, rvec(18, -1, 1));
  emit_poly2var(0.3, 0.4, 0, 0, -1, 2, 2, rvec(18, -1, 1));
  emit_poly2var(0.3, 0.4, 0, 0, 2, -1, 2, rvec(18, -1, 1));
  emit_poly2var(0.3, 0.4, 0, 0, 2, 2, 0, rvec(18, -1, 1));
  emit_poly2var(0.3, 0.4, 0, 1, 2, 2, 2, rvec(17, -1, 1));
}

// =================================================================================
// Lagrange and cubic Hermite interpolation
// =================================================================================
static void emit_lagrange(double u, int der, int deg, int dim, const Vd& vals, const Vd& pars) {
  begin("eval_lagrange", "f i i i fs fs");
  W_f(u); W_i(der); W_i(deg); W_i(dim); W_fs(vals); W_fs(pars); bar();
  // a negative request leaves most of the result array alone; short arrays are read past their end
  if (der < 0 || deg < 0 || dim < 1 || (long)vals.size() < (long)(deg + 1) * dim || (long)pars.size() < deg + 1) unchecked();
  else {
    size_t used = (size_t)(std::min(der, deg) + 1) * dim;
    Vd v(vals), p(pars), res = sbuf(used);
    int rc = PLib::EvalLagrange(u, der, deg, dim, v[0], p[0], res[0]);
    in(rc);
    if (rc == 0) { check_buf(res, used, "EvalLagrange"); out(res, used); } else check_buf(res, 0, "EvalLagrange (failed)");
  }
  finish();
}
static Vd rpars(int n, double lo, double hi) { // increasing distinct parameters
  Vd v; double x = lo;
  for (int i = 0; i < n; ++i) { v.push_back(x); x += rnd(0.05, 1.0) * (hi - lo) / n; }
  return v;
}
static void fam_lagrange() {
  for (int dim = 1; dim <= 3; ++dim)
    for (int deg = 0; deg <= 8; ++deg) {
      int ders[] = {0, 1, 2, deg, deg + 1, deg + 2, rint(0, 4)};
      for (int k = 0; k < 3; ++k)
        emit_lagrange(ru(), ders[(k * 2 + deg + dim) % 7], deg, dim, rvec((deg + 1) * dim, -3, 3), rpars(deg + 1, -2, 2));
    }
  int dims[] = {4, 5, 7};
  for (int dim : dims) { int deg = rint(1, 5); emit_lagrange(ru(), rint(0, deg), deg, dim, rvec((deg + 1) * dim, -3, 3), rpars(deg + 1, 0, 3)); }
  // parameters in any order, and decreasing
  for (int k = 0; k < 4; ++k) {
    int deg = rint(2, 5); Vd p = rpars(deg + 1, -1, 1);
    if (k % 2) std::reverse(p.begin(), p.end()); else std::swap(p[0], p[deg]);
    emit_lagrange(ru(), rint(0, deg), deg, 2, rvec((deg + 1) * 2, -3, 3), p);
  }
  // evaluation at a node
  emit_lagrange(0.5, 2, 3, 2, rvec(8, -3, 3), Vd{0.0, 0.5, 1.0, 1.5});
  // two equal parameters (return code 1), a difference below the smallest normal double, a near-equal pair that is fine
  emit_lagrange(0.5, 1, 3, 2, rvec(8, -3, 3), Vd{0.0, 0.5, 0.5, 1.5});
  emit_lagrange(0.5, 1, 2, 1, rvec(3, -3, 3), Vd{1.0, 2.0, 1.0});
  emit_lagrange(0.5, 0, 1, 3, rvec(6, -3, 3), Vd{0.0, 4.9406564584124654e-324});
  emit_lagrange(0.5, 0, 1, 3, rvec(6, -3, 3), Vd{3.0, 3.0});
  emit_lagrange(0.5, 2, 2, 2, rvec(6, -3, 3), Vd{0.0, 1e-9, 1.0});
  // invalid
  emit_lagrange(0.5, -1, 2, 2, rvec(6, -3, 3), Vd{0.0, 0.5, 1.0});
  emit_lagrange(0.5, 1, -1, 2, rvec(6, -3, 3), Vd{0.0, 0.5, 1.0});
  emit_lagrange(0.5, 1, 2, 0, rvec(6, -3, 3), Vd{0.0, 0.5, 1.0});
  emit_lagrange(0.5, 1, 2, 2, rvec(5, -3, 3), Vd{0.0, 0.5, 1.0});
  emit_lagrange(0.5, 1, 2, 2, rvec(6, -3, 3), Vd{0.0, 0.5});
}

static void emit_hermite3(double u, int der, int dim, const Vd& vals, const Vd& ders, const Vd& pars) {
  begin("eval_cubic_hermite", "f i i fs fs fs");
  W_f(u); W_i(der); W_i(dim); W_fs(vals); W_fs(ders); W_fs(pars); bar();
  // equal parameters divide by zero (OCCT does not check); a negative request writes only the value
  bool bad = der < 0 || dim < 1 || (long)vals.size() < 2L * dim || (long)ders.size() < 2L * dim || pars.size() < 2;
  if (!bad && pars[0] == pars[1]) bad = true;
  if (bad) unchecked();
  else {
    size_t used = (size_t)(std::min(der, 3) + 1) * dim;
    Vd v(vals), d(ders), p(pars), res = sbuf(used);
    int rc = PLib::EvalCubicHermite(u, der, dim, v[0], d[0], p[0], res[0]);
    check_buf(res, used, "EvalCubicHermite");
    in(rc); out(res, used);
  }
  finish();
}
static void fam_cubic_hermite() {
  for (int dim = 1; dim <= 4; ++dim)
    for (int k = 0; k < 8; ++k) {
      double a = rnz(0.0, 2.0), b = a + rnd(0.1, 3.0) * (k % 4 == 3 ? -1 : 1);
      int der = k < 6 ? k % 5 : 3 + k;
      emit_hermite3(k % 2 ? ru() : rnd(std::min(a, b), std::max(a, b)), der, dim, rvec(2 * dim, -3, 3), rvec(2 * dim, -3, 3), Vd{a, b});
    }
  int dims[] = {5, 8, 16};
  for (int dim : dims) emit_hermite3(0.3, rint(0, 4), dim, rvec(2 * dim, -3, 3), rvec(2 * dim, -3, 3), Vd{0.0, 1.0});
  // at the end parameters
  emit_hermite3(0.0, 1, 2, rvec(4, -3, 3), rvec(4, -3, 3), Vd{0.0, 2.0});
  emit_hermite3(2.0, 2, 2, rvec(4, -3, 3), rvec(4, -3, 3), Vd{0.0, 2.0});
  emit_hermite3(-1.0, 3, 1, rvec(2, -3, 3), rvec(2, -3, 3), Vd{-1.0, 1.0});
  // invalid: zero-length interval, negative order, bad dimension, short arrays
  emit_hermite3(0.5, 1, 2, rvec(4, -3, 3), rvec(4, -3, 3), Vd{1.0, 1.0});
  emit_hermite3(0.5, 0, 1, rvec(2, -3, 3), rvec(2, -3, 3), Vd{-2.5, -2.5});
  emit_hermite3(0.5, -1, 2, rvec(4, -3, 3), rvec(4, -3, 3), Vd{0.0, 1.0});
  emit_hermite3(0.5, 1, 0, rvec(4, -3, 3), rvec(4, -3, 3), Vd{0.0, 1.0});
  emit_hermite3(0.5, 1, 2, rvec(3, -3, 3), rvec(4, -3, 3), Vd{0.0, 1.0});
  emit_hermite3(0.5, 1, 2, rvec(4, -3, 3), rvec(3, -3, 3), Vd{0.0, 1.0});
  emit_hermite3(0.5, 1, 2, rvec(4, -3, 3), rvec(4, -3, 3), Vd{0.0});
}

// =================================================================================
// Hermite polynomials on [FirstParameter, LastParameter]
// =================================================================================
static void emit_hermite_coefficients(double a, double b, int fo, int lo) {
  begin("hermite_coefficients", "f f i i");
  W_f(a); W_f(b); W_i(fo); W_i(lo); bar();
  long nb = (long)fo + lo + 2;
  if (nb < 0) unchecked(); // a vector with negative length: crashes
  else {
    int n = (int)std::max(nb, 1L);
    math_Matrix m(1, n, 1, n, 0.0);
    bool ok = PLib::HermiteCoefficients(a, b, fo, lo, m);
    in(ok ? 1 : 0);
    for (int r = 1; r <= n; ++r) for (int c = 1; c <= n; ++c) h(m(r, c)); // untouched (zero) when the result is false
  }
  finish();
}
static void fam_hermite_coefficients() {
  for (int fo = 0; fo <= 3; ++fo)
    for (int lo = 0; lo <= 3; ++lo) {
      if ((fo + lo) % 2 == 0) emit_hermite_coefficients(-1, 1, fo, lo);
      emit_hermite_coefficients(rnd(-3, 0), rnd(0.5, 4), fo, lo);
    }
  emit_hermite_coefficients(0, 1, 2, 2);
  emit_hermite_coefficients(1, 0, 1, 2); // reversed
  emit_hermite_coefficients(3.5, -2.25, 2, 1);
  emit_hermite_coefficients(-1, 1, 4, 4);
  emit_hermite_coefficients(-1, 1, 5, 5);
  emit_hermite_coefficients(0.2, 1.7, 5, 3);
  // the validity tests: |param| > 100, |first|+|last| < 1/100, a short interval (relative length < 1/100)
  emit_hermite_coefficients(-100.5, 1, 1, 1);
  emit_hermite_coefficients(1, 100.5, 1, 1);
  emit_hermite_coefficients(-100, 100, 1, 1); // |param| = 100 is allowed
  emit_hermite_coefficients(0.004, -0.004, 1, 1);
  emit_hermite_coefficients(0, 0.0099, 0, 0);
  emit_hermite_coefficients(50, 50.4, 1, 2);
  emit_hermite_coefficients(50, 51, 1, 2);
  emit_hermite_coefficients(3, 3, 0, 0);
  // near the 1/100 relative length; the pivot 1e-10 of the elimination decides for high orders
  emit_hermite_coefficients(0.5, 0.7, 4, 4);   // true
  emit_hermite_coefficients(0.5, 0.6, 4, 4);   // false
  emit_hermite_coefficients(0.5, 0.7, 5, 5);   // false
  emit_hermite_coefficients(0.5, 1.0, 5, 5);   // true
  emit_hermite_coefficients(0.0, 0.1, 3, 3);   // true
  emit_hermite_coefficients(0.5, 0.55, 3, 3);  // false
  emit_hermite_coefficients(-0.1, 0.1, 7, 7);  // false
  emit_hermite_coefficients(-0.5, 0.5, 7, 7);  // true
  emit_hermite_coefficients(0.5, 0.52, 1, 6);
  emit_hermite_coefficients(2.0, 2.5, 7, 0);
  emit_hermite_coefficients(0.5, 0.52, 4, 4);
  emit_hermite_coefficients(1.0, 1.05, 5, 5);
  emit_hermite_coefficients(10, 10.2, 3, 3);
  emit_hermite_coefficients(99, 100, 2, 2);
  // negative orders: false before anything is computed
  emit_hermite_coefficients(-1, 1, -1, 0);
  emit_hermite_coefficients(-1, 1, 0, -1);
  emit_hermite_coefficients(-1, 1, -1, -1);
  emit_hermite_coefficients(-1, 1, -2, 0);
  emit_hermite_coefficients(-1, 1, -2, 3);
  emit_hermite_coefficients(-1, 1, 3, -2);
  // negative size
  emit_hermite_coefficients(-1, 1, -2, -1);
  emit_hermite_coefficients(-1, 1, -3, 0);
}

// the matrix PLib::HermiteInterpolate builds and eliminates: when math_Gauss does not finish, Solve is called on a
// failed elimination (unspecified; may crash)
static bool interpolation_matrix_done(double fp, double lp, int fo, int lo) {
  static const double Pattern[3][6] = {{1, 1, 1, 1, 1, 1}, {0, 1, 2, 3, 4, 5}, {0, 0, 2, 6, 12, 20}};
  int n = fo + lo + 1;
  math_Matrix A(0, n, 0, n);
  for (int irow = 0; irow <= fo; irow++) {
    double v = 1.;
    for (int icol = 0; icol <= n; icol++) { A(irow, icol) = Pattern[irow][icol] * v; if (irow <= icol) v *= fp; }
  }
  for (int irow = 0; irow <= lo; irow++) {
    double v = 1.;
    for (int icol = 0; icol <= n; icol++) { A(irow + fo + 1, icol) = Pattern[irow][icol] * v; if (irow <= icol) v *= lp; }
  }
  math_Gauss g(A);
  return g.IsDone();
}
static void emit_hermite_interpolate(int dim, double fp, double lp, int fo, int lo, const A2& F, const A2& L) {
  begin("hermite_interpolate", "i f f i i arr2 arr2");
  W_i(dim); W_f(fp); W_f(lp); W_i(fo); W_i(lo); W_arr2(F); W_arr2(L); bar();
  // orders outside -1..2 read outside the table of derivative patterns; both orders -1 is an empty system (A(0,-1,0,-1)); a singular
  // system is solved after a failed elimination
  bool bad = dim < 0 || fo < -1 || lo < -1 || fo > 2 || lo > 2 || fo + lo < -1 ||
             F.rows < dim || L.rows < dim || F.cols < fo + 1 || L.cols < lo + 1;
  if (!bad && dim >= 1 && !interpolation_matrix_done(fp, lp, fo, lo)) bad = true;
  if (bad) unchecked();
  else {
    NCollection_Array2<double> FC(1, std::max(F.rows, 1), 0, std::max(F.cols - 1, 0)), LC(1, std::max(L.rows, 1), 0, std::max(L.cols - 1, 0));
    for (int r = 0; r < F.rows; ++r) for (int c = 0; c < F.cols; ++c) FC(1 + r, c) = F.at(r, c);
    for (int r = 0; r < L.rows; ++r) for (int c = 0; c < L.cols; ++c) LC(1 + r, c) = L.at(r, c);
    int n = dim * (fo + lo + 2);
    NCollection_Array1<double> C(0, std::max(n, 1) - 1);
    C.Init(SENT);
    // with the failing eliminations excluded above the result is always true
    if (!PLib::HermiteInterpolate(dim, fp, lp, fo, lo, FC, LC, C)) { std::fprintf(stderr, "HermiteInterpolate returned false\n"); std::abort(); }
    in(n); // the number of coefficients first (zero for Dimension 0: an empty output would not parse)
    for (int i = 0; i < n; ++i) { if (C(i) == SENT) { std::fprintf(stderr, "HermiteInterpolate left a coefficient unwritten\n"); std::abort(); } h(C(i)); }
  }
  finish();
}
static void fam_hermite_interpolate() {
  for (int k = 0; k < 60; ++k) {
    int dim = k % 9 == 8 ? 4 : 1 + k % 3;
    int fo = k % 3, lo = (k / 3) % 3;
    double a = rnz(0.0, 2.5), b = a + rnd(0.2, 3.0) * (k % 5 == 4 ? -1 : 1);
    if (k % 7 == 6) { a = -1; b = 1; }
    emit_hermite_interpolate(dim, a, b, fo, lo, mkA2(dim, fo + 1, -3, 3), mkA2(dim, lo + 1, -3, 3));
  }
  // a constraint at one end only
  emit_hermite_interpolate(2, -1, 2, -1, 2, mkA2(2, 1, -3, 3), mkA2(2, 3, -3, 3));
  emit_hermite_interpolate(1, 0.5, 1.5, 2, -1, mkA2(1, 3, -3, 3), mkA2(1, 1, -3, 3));
  emit_hermite_interpolate(3, 0, 1, -1, 0, mkA2(3, 1, -3, 3), mkA2(3, 1, -3, 3));
  emit_hermite_interpolate(2, 0, 1, 0, -1, mkA2(2, 1, -3, 3), mkA2(2, 1, -3, 3));
  // zero dimension: nothing to solve, true
  emit_hermite_interpolate(0, 0, 1, 1, 1, mkA2(1, 2, -3, 3), mkA2(1, 2, -3, 3));
  emit_hermite_interpolate(0, 2, 2, 1, 1, mkA2(1, 2, -3, 3), mkA2(1, 2, -3, 3));
  // extra rows and columns in the constraint arrays are not read
  emit_hermite_interpolate(2, 0, 1, 1, 1, mkA2(3, 3, -3, 3), mkA2(2, 4, -3, 3));
  // ill-conditioned but finishing
  emit_hermite_interpolate(2, 1.0, 1.001, 2, 2, mkA2(2, 3, -3, 3), mkA2(2, 3, -3, 3));
  emit_hermite_interpolate(1, 40, 41, 2, 2, mkA2(1, 3, -3, 3), mkA2(1, 3, -3, 3));
  // equal / tiny parameters: the elimination fails
  emit_hermite_interpolate(2, 0.3, 0.3, 1, 1, mkA2(2, 2, -3, 3), mkA2(2, 2, -3, 3));
  emit_hermite_interpolate(1, -2, -2, 0, 0, mkA2(1, 1, -3, 3), mkA2(1, 1, -3, 3));
  emit_hermite_interpolate(1, 1e-30, 2e-30, 1, 1, mkA2(1, 2, -3, 3), mkA2(1, 2, -3, 3));
  // orders outside the supported range, short constraint arrays, negative dimension
  emit_hermite_interpolate(2, 0, 1, 3, 1, mkA2(2, 4, -3, 3), mkA2(2, 2, -3, 3));
  emit_hermite_interpolate(2, 0, 1, 1, 3, mkA2(2, 2, -3, 3), mkA2(2, 4, -3, 3));
  emit_hermite_interpolate(2, 0, 1, -2, 1, mkA2(2, 1, -3, 3), mkA2(2, 2, -3, 3));
  emit_hermite_interpolate(2, 0, 1, -1, -1, mkA2(2, 1, -3, 3), mkA2(2, 1, -3, 3));
  emit_hermite_interpolate(-1, 0, 1, 1, 1, mkA2(1, 2, -3, 3), mkA2(1, 2, -3, 3));
  emit_hermite_interpolate(3, 0, 1, 1, 1, mkA2(2, 2, -3, 3), mkA2(3, 2, -3, 3));
  emit_hermite_interpolate(2, 0, 1, 2, 1, mkA2(2, 2, -3, 3), mkA2(2, 2, -3, 3));
}

// =================================================================================
// Poles <-> flat arrays
// =================================================================================
static std::vector<gp_Pnt> rp3(int n, double lo, double hi) { std::vector<gp_Pnt> v; for (int i = 0; i < n; ++i) { double x = rnd(lo, hi), y = rnd(lo, hi), z = rnd(lo, hi); v.push_back(gp_Pnt(x, y, z)); } return v; }
static std::vector<gp_Pnt2d> rp2(int n, double lo, double hi) { std::vector<gp_Pnt2d> v; for (int i = 0; i < n; ++i) { double x = rnd(lo, hi), y = rnd(lo, hi); v.push_back(gp_Pnt2d(x, y)); } return v; }
static Vd rweights(int n) { Vd v; for (int i = 0; i < n; ++i) { double r = rnd(0, 1); v.push_back(r < 0.08 ? rnz(1e-6, 1e-4) : r < 0.25 ? rnz(0.05, 0.4) : rnd(0.3, 3.0)); } return v; }

static void fam_set_poles_3d() {
  for (int k = 0; k < 24; ++k) {
    int n = k < 20 ? 1 + k % 8 : 12 + k;
    std::vector<gp_Pnt> p = rp3(n, -6, 6);
    begin("set_poles_3d", "p3s"); W_p3s(p); bar();
    NCollection_Array1<gp_Pnt> P(1, n); NCollection_Array1<double> FP(1, 3 * n);
    for (int i = 0; i < n; ++i) P(1 + i) = p[i];
    PLib::SetPoles(P, FP);
    for (int i = 1; i <= 3 * n; ++i) h(FP(i));
    finish();
  }
}
static void fam_set_poles_3d_w() {
  for (int k = 0; k < 24; ++k) {
    int n = k < 20 ? 1 + k % 8 : 12 + k;
    std::vector<gp_Pnt> p = rp3(n, -6, 6); Vd w = rweights(n);
    if (k % 6 == 5) w[0] = 0.0; // a zero weight is fine here
    begin("set_poles_3d_weighted", "p3s fs"); W_p3s(p); W_fs(w); bar();
    NCollection_Array1<gp_Pnt> P(1, n); NCollection_Array1<double> W(1, n), FP(1, 4 * n);
    for (int i = 0; i < n; ++i) { P(1 + i) = p[i]; W(1 + i) = w[i]; }
    PLib::SetPoles(P, W, FP);
    for (int i = 1; i <= 4 * n; ++i) h(FP(i));
    finish();
  }
}
static void fam_set_poles_2d() {
  for (int k = 0; k < 24; ++k) {
    int n = k < 20 ? 1 + k % 8 : 12 + k;
    std::vector<gp_Pnt2d> p = rp2(n, -6, 6);
    begin("set_poles_2d", "p2s"); W_p2s(p); bar();
    NCollection_Array1<gp_Pnt2d> P(1, n); NCollection_Array1<double> FP(1, 2 * n);
    for (int i = 0; i < n; ++i) P(1 + i) = p[i];
    PLib::SetPoles(P, FP);
    for (int i = 1; i <= 2 * n; ++i) h(FP(i));
    finish();
  }
}
static void fam_set_poles_2d_w() {
  for (int k = 0; k < 24; ++k) {
    int n = k < 20 ? 1 + k % 8 : 12 + k;
    std::vector<gp_Pnt2d> p = rp2(n, -6, 6); Vd w = rweights(n);
    if (k % 6 == 5) w[n - 1] = 0.0;
    begin("set_poles_2d_weighted", "p2s fs"); W_p2s(p); W_fs(w); bar();
    NCollection_Array1<gp_Pnt2d> P(1, n); NCollection_Array1<double> W(1, n), FP(1, 3 * n);
    for (int i = 0; i < n; ++i) { P(1 + i) = p[i]; W(1 + i) = w[i]; }
    PLib::SetPoles(P, W, FP);
    for (int i = 1; i <= 3 * n; ++i) h(FP(i));
    finish();
  }
}
// GetPoles: the number of poles is an input (the C++ output array has it as its length); FP may be longer
static void emit_get_poles(const char* name, int stride, bool is3d, bool weighted, int n, const Vd& fp) {
  begin(name, "i fs"); W_i(n); W_fs(fp); bar();
  long need = (long)n * (stride + (weighted ? 1 : 0));
  bool bad = n < 1 || (long)fp.size() < need;
  if (!bad && weighted) for (int i = 0; i < n; ++i) if (fp[(size_t)i * (stride + 1) + stride] == 0.0) bad = true; // division by zero
  if (bad) unchecked();
  else {
    NCollection_Array1<double> FP(1, (int)fp.size()), W(1, n);
    for (size_t i = 0; i < fp.size(); ++i) FP(1 + (int)i) = fp[i];
    if (is3d) {
      NCollection_Array1<gp_Pnt> P(1, n);
      if (weighted) PLib::GetPoles(FP, P, W); else PLib::GetPoles(FP, P);
      for (int i = 1; i <= n; ++i) { h(P(i).X()); h(P(i).Y()); h(P(i).Z()); }
    } else {
      NCollection_Array1<gp_Pnt2d> P(1, n);
      if (weighted) PLib::GetPoles(FP, P, W); else PLib::GetPoles(FP, P);
      for (int i = 1; i <= n; ++i) { h(P(i).X()); h(P(i).Y()); }
    }
    if (weighted) for (int i = 1; i <= n; ++i) h(W(i));
  }
  finish();
}
static Vd fpw(int n, int stride, bool zero_weight) { // flat poles with weights (last of each group), nonzero
  Vd v = rvec(n * (stride + 1), -5, 5);
  for (int i = 0; i < n; ++i) { double r = rnd(0, 1); double w = r < 0.08 ? rnz(1e-6, 1e-4) : r < 0.25 ? rnz(0.05, 0.4) : rnd(0.3, 3.0); v[(size_t)i * (stride + 1) + stride] = w; }
  if (zero_weight) v[(size_t)(n / 2) * (stride + 1) + stride] = 0.0;
  return v;
}
static void fam_get_poles() {
  for (int k = 0; k < 20; ++k) {
    int n = k < 16 ? 1 + k % 8 : 10 + k;
    emit_get_poles("get_poles_3d", 3, true, false, n, rvec(3 * n + (k % 5 == 4 ? 3 : 0), -5, 5));
    emit_get_poles("get_poles_2d", 2, false, false, n, rvec(2 * n + (k % 5 == 4 ? 2 : 0), -5, 5));
    emit_get_poles("get_poles_3d_weighted", 3, true, true, n, fpw(n, 3, false));
    emit_get_poles("get_poles_2d_weighted", 2, false, true, n, fpw(n, 2, false));
  }
  // invalid: no poles, short flat array, zero weight
  emit_get_poles("get_poles_3d", 3, true, false, 0, rvec(3, -5, 5));
  emit_get_poles("get_poles_3d", 3, true, false, 3, rvec(8, -5, 5));
  emit_get_poles("get_poles_2d", 2, false, false, 4, rvec(7, -5, 5));
  emit_get_poles("get_poles_3d_weighted", 3, true, true, 3, rvec(11, -5, 5));
  emit_get_poles("get_poles_2d_weighted", 2, false, true, 3, rvec(8, -5, 5));
  emit_get_poles("get_poles_3d_weighted", 3, true, true, 3, fpw(3, 3, true));
  emit_get_poles("get_poles_3d_weighted", 3, true, true, 5, fpw(5, 3, true));
  emit_get_poles("get_poles_2d_weighted", 2, false, true, 3, fpw(3, 2, true));
  emit_get_poles("get_poles_2d_weighted", 2, false, true, 4, fpw(4, 2, true));
}

// =================================================================================
// Power basis <-> Bezier poles
// =================================================================================
static void emit_bin(int n, int p) {
  begin("bin", "i i"); W_i(n); W_i(p); bar();
  if (n >= 0 && n <= 25 && p >= 0 && p <= n) h(PLib::Bin(n, p));
  else if (std::abs(n) <= 40 && std::abs(p) <= 40) { GUARD(PLib::Bin(n, p); unchecked();) } // reads outside the table: no check exists
  else unchecked();
  finish();
}
static void fam_bin() {
  for (int n = 0; n <= 25; ++n) for (int p = 0; p <= n; ++p) emit_bin(n, p);
  emit_bin(-1, 0); emit_bin(-1, -1); emit_bin(-5, 2); emit_bin(0, -1); emit_bin(4, -1); emit_bin(5, 6); emit_bin(3, 5); emit_bin(25, 26);
  emit_bin(26, 0); emit_bin(26, 13); emit_bin(30, 4); emit_bin(26, 26); emit_bin(25, 40); emit_bin(100, 3); emit_bin(2147483647, 1);
}

// CoefficientsPoles / Trimming take the coefficient groups of a polynomial in the power basis
template <typename Arr> static void get_weights(const Arr& a, int n, Vd& out_) { out_.clear(); for (int i = 1; i <= n; ++i) out_.push_back(a(i)); }
static void emit_cp_3d(const std::vector<gp_Pnt>& c, const Vd* w) {
  begin("coefficients_poles_3d", "p3s ofs"); W_p3s(c); W_ofs(w); bar();
  int n = (int)c.size();
  if (n >= 27 || (w && (int)w->size() != n)) unchecked(); // Bin(26 or more, ..) is outside the table of binomials
  else {
    NCollection_Array1<gp_Pnt> C(1, n), P(1, n); NCollection_Array1<double> WC(1, n), WP(1, n);
    for (int i = 0; i < n; ++i) { C(1 + i) = c[i]; P(1 + i) = gp_Pnt(SENT, SENT, SENT); if (w) WC(1 + i) = (*w)[i]; WP(1 + i) = SENT; }
    PLib::CoefficientsPoles(C, w ? &WC : nullptr, P, w ? &WP : nullptr);
    for (int i = 1; i <= n; ++i) { if (P(i).X() == SENT) std::abort(); h(P(i).X()); h(P(i).Y()); h(P(i).Z()); }
    if (w) for (int i = 1; i <= n; ++i) { if (WP(i) == SENT) std::abort(); h(WP(i)); }
  }
  finish();
}
static void emit_cp_2d(const std::vector<gp_Pnt2d>& c, const Vd* w) {
  begin("coefficients_poles_2d", "p2s ofs"); W_p2s(c); W_ofs(w); bar();
  int n = (int)c.size();
  if (n >= 27 || (w && (int)w->size() != n)) unchecked();
  else {
    NCollection_Array1<gp_Pnt2d> C(1, n), P(1, n); NCollection_Array1<double> WC(1, n), WP(1, n);
    for (int i = 0; i < n; ++i) { C(1 + i) = c[i]; P(1 + i) = gp_Pnt2d(SENT, SENT); if (w) WC(1 + i) = (*w)[i]; WP(1 + i) = SENT; }
    PLib::CoefficientsPoles(C, w ? &WC : nullptr, P, w ? &WP : nullptr);
    for (int i = 1; i <= n; ++i) { if (P(i).X() == SENT) std::abort(); h(P(i).X()); h(P(i).Y()); }
    if (w) for (int i = 1; i <= n; ++i) { if (WP(i) == SENT) std::abort(); h(WP(i)); }
  }
  finish();
}
// dim == 1 uses the overload for real arrays without dimension, others the one with `dim`
static void emit_cp_dim(const char* name, int dim, const Vd& c, const Vd* w) {
  bool plain = std::string(name) == "coefficients_poles_1d";
  begin(name, plain ? "fs ofs" : "i fs ofs");
  if (!plain) W_i(dim);
  W_fs(c); W_ofs(w); bar();
  int total = (int)c.size(), n = total / dim;
  if (dim < 1 || total % dim != 0 || n < 1 || n >= 27 || (w && (int)w->size() != n)) unchecked();
  else {
    NCollection_Array1<double> C(1, total), P(1, total), WC(1, n), WP(1, n);
    for (int i = 0; i < total; ++i) { C(1 + i) = c[i]; P(1 + i) = SENT; }
    for (int i = 0; i < n; ++i) { if (w) WC(1 + i) = (*w)[i]; WP(1 + i) = SENT; }
    if (plain) PLib::CoefficientsPoles(C, w ? &WC : nullptr, P, w ? &WP : nullptr);
    else PLib::CoefficientsPoles(dim, C, w ? &WC : nullptr, P, w ? &WP : nullptr);
    for (int i = 1; i <= total; ++i) { if (P(i) == SENT) { std::fprintf(stderr, "CoefficientsPoles left a pole unwritten\n"); std::abort(); } h(P(i)); }
    if (w) for (int i = 1; i <= n; ++i) { if (WP(i) == SENT) { std::fprintf(stderr, "CoefficientsPoles left a weight unwritten\n"); std::abort(); } h(WP(i)); }
  }
  finish();
}
static void fam_coefficients_poles() {
  for (int k = 0; k < 24; ++k) {
    int n = k < 18 ? 1 + k % 10 : 10 + (k - 18) * 2;
    bool rat = k % 2 == 1;
    Vd w = rweights(n);
    for (double& x : w) x = std::fabs(x); // positive weights: the sums of the weights never vanish
    emit_cp_3d(rp3(n, -5, 5), rat ? &w : nullptr);
    emit_cp_2d(rp2(n, -5, 5), rat ? &w : nullptr);
    emit_cp_dim("coefficients_poles_1d", 1, rvec(n, -5, 5), rat ? &w : nullptr);
    int dim = 1 + k % 6 + (k % 11 == 10 ? 8 : 0);
    emit_cp_dim("coefficients_poles_dim", dim, rvec(n * dim, -5, 5), rat ? &w : nullptr);
  }
  // the largest supported number of coefficients (26: Bin(25, ..)) and beyond (unchecked)
  Vd w26(26, 1.5); for (int i = 0; i < 26; ++i) w26[i] = 0.5 + 0.1 * i;
  emit_cp_3d(rp3(26, -1, 1), nullptr); emit_cp_3d(rp3(26, -1, 1), &w26);
  emit_cp_2d(rp2(26, -1, 1), &w26);
  emit_cp_dim("coefficients_poles_1d", 1, rvec(26, -1, 1), nullptr);
  emit_cp_dim("coefficients_poles_dim", 2, rvec(52, -1, 1), &w26);
  emit_cp_3d(rp3(27, -1, 1), nullptr); emit_cp_2d(rp2(30, -1, 1), nullptr);
  emit_cp_dim("coefficients_poles_1d", 1, rvec(27, -1, 1), nullptr);
  emit_cp_dim("coefficients_poles_dim", 3, rvec(81, -1, 1), nullptr);
  // invalid: dimension 0, a length that is not a multiple of the dimension, a weight array of the wrong length
  emit_cp_dim("coefficients_poles_dim", 0, rvec(6, -1, 1), nullptr);
  emit_cp_dim("coefficients_poles_dim", -2, rvec(6, -1, 1), nullptr);
  emit_cp_dim("coefficients_poles_dim", 3, rvec(7, -1, 1), nullptr);
  Vd w3 = rweights(3);
  emit_cp_dim("coefficients_poles_dim", 2, rvec(8, -1, 1), &w3);
  emit_cp_dim("coefficients_poles_1d", 1, rvec(5, -1, 1), &w3);
  emit_cp_3d(rp3(4, -1, 1), &w3);
  emit_cp_2d(rp2(2, -1, 1), &w3);
}

// two-dimensional arrays of coefficients / poles
static void emit_cp_surface(const P3A2& c, const A2* w) {
  begin("coefficients_poles_surface", "p3a2 oarr2"); W_p3a2(c); W_oarr2(w); bar();
  // Bin(rows-1, ..) and Bin(cols-1, ..) must stay in the table
  if (c.rows < 1 || c.cols < 1 || c.rows >= 27 || c.cols >= 27 || (w && (w->rows != c.rows || w->cols != c.cols))) unchecked();
  else {
    NCollection_Array2<gp_Pnt> C(1, c.rows, 1, c.cols), P(1, c.rows, 1, c.cols);
    NCollection_Array2<double> WC(1, c.rows, 1, c.cols), WP(1, c.rows, 1, c.cols);
    for (int r = 0; r < c.rows; ++r) for (int q = 0; q < c.cols; ++q) {
      C(1 + r, 1 + q) = c.v[(size_t)r * c.cols + q]; P(1 + r, 1 + q) = gp_Pnt(SENT, SENT, SENT); WP(1 + r, 1 + q) = SENT;
      if (w) WC(1 + r, 1 + q) = w->at(r, q);
    }
    PLib::CoefficientsPoles(C, w ? &WC : nullptr, P, w ? &WP : nullptr);
    for (int r = 1; r <= c.rows; ++r) for (int q = 1; q <= c.cols; ++q) { if (P(r, q).X() == SENT) std::abort(); h(P(r, q).X()); h(P(r, q).Y()); h(P(r, q).Z()); }
    if (w) for (int r = 1; r <= c.rows; ++r) for (int q = 1; q <= c.cols; ++q) { if (WP(r, q) == SENT) std::abort(); h(WP(r, q)); }
  }
  finish();
}
static A2 rposw(int rows, int cols) { A2 a = mkA2(rows, cols, 0.3, 3.0); return a; }
static void fam_coefficients_poles_surface() {
  for (int k = 0; k < 28; ++k) {
    int rows = k < 6 ? 1 : 1 + k % 6, cols = k < 12 ? 1 + k % 6 : 1 + (k * 7) % 6;
    if (k == 4) cols = 1;
    emit_cp_surface(mkP3A2(rows, cols, -4, 4), k % 2 ? nullptr : (A2*)new A2(rposw(rows, cols)));
  }
  emit_cp_surface(mkP3A2(26, 2, -1, 1), nullptr);
  emit_cp_surface(mkP3A2(2, 26, -1, 1), (A2*)new A2(rposw(2, 26)));
  emit_cp_surface(mkP3A2(26, 1, -1, 1), (A2*)new A2(rposw(26, 1)));
  emit_cp_surface(mkP3A2(1, 26, -1, 1), nullptr);
  emit_cp_surface(mkP3A2(27, 1, -1, 1), nullptr);
  emit_cp_surface(mkP3A2(1, 27, -1, 1), nullptr);
  emit_cp_surface(mkP3A2(27, 2, -1, 1), nullptr);
  emit_cp_surface(mkP3A2(2, 28, -1, 1), nullptr);
  emit_cp_surface(mkP3A2(30, 1, -1, 1), nullptr);
  // weights of another shape
  emit_cp_surface(mkP3A2(3, 4, -1, 1), (A2*)new A2(rposw(3, 3)));
  emit_cp_surface(mkP3A2(3, 4, -1, 1), (A2*)new A2(rposw(4, 4)));
}

// =================================================================================
// Trimming (change of parameter interval of a polynomial in the power basis)
// =================================================================================
static void print_pnts3(NCollection_Array1<gp_Pnt>& P, int n) { for (int i = 1; i <= n; ++i) { h(P(i).X()); h(P(i).Y()); h(P(i).Z()); } }
static void emit_trim_3d(double u1, double u2, const std::vector<gp_Pnt>& c, const Vd* w) {
  begin("trimming_3d", "f f p3s ofs"); W_f(u1); W_f(u2); W_p3s(c); W_ofs(w); bar();
  int n = (int)c.size();
  NCollection_Array1<gp_Pnt> C(1, n); NCollection_Array1<double> W(1, w ? (int)w->size() : 1);
  for (int i = 0; i < n; ++i) C(1 + i) = c[i];
  if (w) for (size_t i = 0; i < w->size(); ++i) W(1 + (int)i) = (*w)[i];
  GUARD(PLib::Trimming(u1, u2, C, w ? &W : nullptr); print_pnts3(C, n); if (w) for (int i = 1; i <= n; ++i) h(W(i));)
  finish();
}
static void emit_trim_2d(double u1, double u2, const std::vector<gp_Pnt2d>& c, const Vd* w) {
  begin("trimming_2d", "f f p2s ofs"); W_f(u1); W_f(u2); W_p2s(c); W_ofs(w); bar();
  int n = (int)c.size();
  NCollection_Array1<gp_Pnt2d> C(1, n); NCollection_Array1<double> W(1, w ? (int)w->size() : 1);
  for (int i = 0; i < n; ++i) C(1 + i) = c[i];
  if (w) for (size_t i = 0; i < w->size(); ++i) W(1 + (int)i) = (*w)[i];
  GUARD(PLib::Trimming(u1, u2, C, w ? &W : nullptr);
        for (int i = 1; i <= n; ++i) { h(C(i).X()); h(C(i).Y()); } if (w) for (int i = 1; i <= n; ++i) h(W(i));)
  finish();
}
static void emit_trim_dim(const char* name, double u1, double u2, int dim, const Vd& c, const Vd* w) {
  bool plain = std::string(name) == "trimming_1d";
  begin(name, plain ? "f f fs ofs" : "f f i fs ofs");
  W_f(u1); W_f(u2); if (!plain) W_i(dim); W_fs(c); W_ofs(w); bar();
  int total = (int)c.size();
  if (dim < 1 || total % dim != 0) unchecked(); // division by zero / groups that do not tile the array
  else {
    NCollection_Array1<double> C(1, total), W(1, w ? (int)w->size() : 1);
    for (int i = 0; i < total; ++i) C(1 + i) = c[i];
    if (w) for (size_t i = 0; i < w->size(); ++i) W(1 + (int)i) = (*w)[i];
    GUARD(if (plain) PLib::Trimming(u1, u2, C, w ? &W : nullptr); else PLib::Trimming(u1, u2, dim, C, w ? &W : nullptr);
          for (int i = 1; i <= total; ++i) h(C(i)); if (w) for (size_t i = 0; i < w->size(); ++i) h(W(1 + (int)i));)
  }
  finish();
}
static void trim_interval(int k, double& u1, double& u2) {
  switch (k % 8) {
    case 0: u1 = 0; u2 = 1; break;                 // identity
    case 1: u1 = rnd(-1, 1); u2 = u1 + rnd(0.1, 2); break;
    case 2: u1 = rnd(0, 1); u2 = u1 - rnd(0.1, 2); break; // reversed
    case 3: u1 = u2 = rnz(0.1, 2); break;          // zero length
    case 4: u1 = 0; u2 = 0; break;
    case 5: u1 = -1; u2 = 1; break;
    case 6: u1 = rnz(0, 5); u2 = rnz(0, 5); break;
    default: u1 = rnd(0.2, 0.4); u2 = rnd(0.6, 0.9); break;
  }
}
static void fam_trimming() {
  for (int k = 0; k < 36; ++k) {
    int n = 1 + k % 9; bool rat = k % 3 == 1; double u1, u2; trim_interval(k, u1, u2);
    Vd w = rweights(n);
    emit_trim_3d(u1, u2, rp3(n, -4, 4), rat ? &w : nullptr);
    trim_interval(k + 3, u1, u2);
    emit_trim_2d(u1, u2, rp2(n, -4, 4), rat ? &w : nullptr);
    trim_interval(k + 5, u1, u2);
    emit_trim_dim("trimming_1d", u1, u2, 1, rvec(n, -4, 4), rat ? &w : nullptr);
    trim_interval(k + 2, u1, u2);
    int dim = 1 + k % 5 + (k % 13 == 12 ? 7 : 0);
    emit_trim_dim("trimming_dim", u1, u2, dim, rvec(n * dim, -4, 4), rat ? &w : nullptr);
  }
  // a long polynomial
  Vd w20 = rweights(20);
  emit_trim_dim("trimming_1d", 0.25, 0.75, 1, rvec(20, -1, 1), nullptr);
  emit_trim_dim("trimming_dim", -0.5, 1.5, 2, rvec(40, -1, 1), &w20);
  emit_trim_3d(0.1, 0.9, rp3(20, -1, 1), &w20);
  // weights of the wrong length: Standard_Failure
  Vd w2 = rweights(2), w5 = rweights(5);
  emit_trim_3d(0.1, 0.9, rp3(4, -1, 1), &w2);
  emit_trim_3d(0.1, 0.9, rp3(3, -1, 1), &w5);
  emit_trim_2d(0.1, 0.9, rp2(4, -1, 1), &w2);
  emit_trim_2d(0.1, 0.9, rp2(1, -1, 1), &w2);
  emit_trim_dim("trimming_1d", 0.1, 0.9, 1, rvec(4, -1, 1), &w5);
  emit_trim_dim("trimming_dim", 0.1, 0.9, 3, rvec(12, -1, 1), &w5);
  emit_trim_dim("trimming_dim", 0.1, 0.9, 2, rvec(8, -1, 1), &w5);
  // invalid dimensions
  emit_trim_dim("trimming_dim", 0.1, 0.9, 0, rvec(4, -1, 1), nullptr);
  emit_trim_dim("trimming_dim", 0.1, 0.9, -1, rvec(4, -1, 1), nullptr);
  emit_trim_dim("trimming_dim", 0.1, 0.9, 3, rvec(7, -1, 1), nullptr);
  emit_trim_dim("trimming_dim", 0.1, 0.9, 4, rvec(6, -1, 1), nullptr);
}

static void emit_trim_surface(const char* name, bool u, double a, double b, const P3A2& c, const A2* w) {
  begin(name, "f f p3a2 oarr2"); W_f(a); W_f(b); W_p3a2(c); W_oarr2(w); bar();
  if (c.rows < 1 || c.cols < 1 || (w && (w->rows != c.rows || w->cols != c.cols))) unchecked();
  else {
    NCollection_Array2<gp_Pnt> C(1, c.rows, 1, c.cols); NCollection_Array2<double> W(1, c.rows, 1, c.cols);
    for (int r = 0; r < c.rows; ++r) for (int q = 0; q < c.cols; ++q) { C(1 + r, 1 + q) = c.v[(size_t)r * c.cols + q]; if (w) W(1 + r, 1 + q) = w->at(r, q); }
    GUARD(if (u) PLib::UTrimming(a, b, C, w ? &W : nullptr); else PLib::VTrimming(a, b, C, w ? &W : nullptr);
          for (int r = 1; r <= c.rows; ++r) for (int q = 1; q <= c.cols; ++q) { h(C(r, q).X()); h(C(r, q).Y()); h(C(r, q).Z()); }
          if (w) for (int r = 1; r <= c.rows; ++r) for (int q = 1; q <= c.cols; ++q) h(W(r, q));)
  }
  finish();
}
static void fam_trimming_surface() {
  for (int k = 0; k < 22; ++k) {
    int rows = 1 + k % 5, cols = 1 + (k * 3) % 5; double a, b; trim_interval(k, a, b);
    A2 w = mkA2(rows, cols, 0.3, 3.0); bool rat = k % 3 == 1;
    emit_trim_surface("u_trimming", true, a, b, mkP3A2(rows, cols, -4, 4), rat ? &w : nullptr);
    trim_interval(k + 1, a, b);
    emit_trim_surface("v_trimming", false, a, b, mkP3A2(rows, cols, -4, 4), rat ? &w : nullptr);
  }
  A2 wbad = mkA2(2, 3, 0.3, 3.0);
  emit_trim_surface("u_trimming", true, 0.1, 0.9, mkP3A2(3, 3, -1, 1), &wbad);
  emit_trim_surface("v_trimming", false, 0.1, 0.9, mkP3A2(3, 3, -1, 1), &wbad);
  emit_trim_surface("u_trimming", true, 0.1, 0.9, mkP3A2(12, 2, -1, 1), nullptr);
  emit_trim_surface("v_trimming", false, 0.1, 0.9, mkP3A2(2, 12, -1, 1), nullptr);
}

// =================================================================================
// Derivatives of rational functions
// =================================================================================
// `ders` holds, for each order 0..min(n, degree), the Dimension components followed by the weight derivative
static Vd rders(int degree, int n, int dim, bool zero_w0 = false) {
  int groups = std::min(n, degree) + 1;
  Vd v;
  for (int g = 0; g < groups; ++g) {
    for (int d = 0; d < dim; ++d) v.push_back(rnd(-5, 5));
    v.push_back(g == 0 ? (rnd(0, 1) < 0.15 ? rnz(1e-5, 0.2) : rnz(0.5, 3.0)) : rnd(-2, 2));
  }
  if (zero_w0) v[dim] = 0.0;
  return v;
}
static void emit_rational_derivative(int degree, int n, int dim, bool all, const Vd& ders) {
  begin("rational_derivative", "i i i i fs"); W_i(degree); W_i(n); W_i(dim); W_i(all ? 1 : 0); W_fs(ders); bar();
  // the layout needs (dim+1)*(min(n,degree)+1) values; a zero weight divides by zero
  bool bad = degree < 0 || n < 0 || dim < 1 || (long)ders.size() < (long)(dim + 1) * (std::min(n, degree) + 1);
  if (!bad && ders[dim] == 0.0) bad = true;
  if (bad) unchecked();
  else {
    size_t used = all ? (size_t)dim * (n + 1) : (size_t)dim;
    Vd d(ders), res = sbuf(used);
    PLib::RationalDerivative(degree, n, dim, d[0], res[0], all);
    check_buf(res, used, "RationalDerivative");
    out(res, used);
  }
  finish();
}
static void fam_rational_derivative() {
  for (int k = 0; k < 90; ++k) {
    int dim = k % 10 == 9 ? rint(4, 7) : 1 + k % 3 + (k % 10 == 4 ? 0 : 0);
    if (k % 10 == 3) dim = 3;
    int degree = rint(0, 6), n = rint(0, 6);
    if (k % 5 == 0) n = degree;      // exactly the degree
    if (k % 5 == 1) n = degree + 1;  // one beyond
    emit_rational_derivative(degree, n, dim, k % 3 != 0, rders(degree, n, dim));
  }
  emit_rational_derivative(25, 4, 3, true, rders(25, 4, 3));
  emit_rational_derivative(2, 9, 3, false, rders(2, 9, 3));
  emit_rational_derivative(2, 9, 2, true, rders(2, 9, 2));
  // extra values at the end are not read
  { Vd d = rders(2, 2, 3); d.push_back(7.0); d.push_back(8.0); emit_rational_derivative(2, 2, 3, true, d); }
  // invalid
  emit_rational_derivative(-1, 2, 3, true, rders(2, 2, 3));
  emit_rational_derivative(3, -1, 3, true, rders(3, 2, 3));
  emit_rational_derivative(3, 2, 0, true, rders(3, 2, 2));
  emit_rational_derivative(3, 2, -1, false, rders(3, 2, 2));
  emit_rational_derivative(3, 2, 3, true, rders(3, 1, 3)); // too short
  emit_rational_derivative(3, 2, 2, false, rders(3, 0, 2));
  emit_rational_derivative(3, 2, 3, true, rders(3, 2, 3, true)); // weight zero
  emit_rational_derivative(1, 4, 2, false, rders(1, 4, 2, true));
  emit_rational_derivative(0, 0, 1, true, rders(0, 0, 1, true));
}

static void emit_rational_derivatives(int d, int dim, const Vd& pd, const Vd& wd) {
  begin("rational_derivatives", "i i fs fs"); W_i(d); W_i(dim); W_fs(pd); W_fs(wd); bar();
  bool bad = d < 0 || dim < 1 || (long)pd.size() < (long)dim * (d + 1) || (long)wd.size() < d + 1;
  if (!bad && wd[0] == 0.0) bad = true;
  if (bad) unchecked();
  else {
    size_t used = (size_t)dim * (d + 1);
    Vd p(pd), w(wd), res = sbuf(used);
    PLib::RationalDerivatives(d, dim, p[0], w[0], res[0]);
    check_buf(res, used, "RationalDerivatives");
    out(res, used);
  }
  finish();
}
static Vd rwd(int n, bool zero0 = false) {
  Vd w = rvec(n, -2, 2);
  w[0] = zero0 ? 0.0 : (rnd(0, 1) < 0.2 ? rnz(1e-5, 0.2) : rnz(0.5, 3.0));
  return w;
}
static void fam_rational_derivatives() {
  for (int k = 0; k < 60; ++k) {
    int dim = k % 7 == 6 ? rint(4, 6) : 1 + k % 3, d = rint(0, 6);
    if (k % 4 == 0) dim = 3;
    emit_rational_derivatives(d, dim, rvec(dim * (d + 1), -5, 5), rwd(d + 1));
  }
  emit_rational_derivatives(12, 3, rvec(39, -5, 5), rwd(13));
  { Vd w = rwd(3); w.push_back(4.0); emit_rational_derivatives(2, 2, rvec(6 + 2, -5, 5), w); }
  // invalid
  emit_rational_derivatives(-1, 3, rvec(3, -5, 5), rwd(2));
  emit_rational_derivatives(2, 0, rvec(6, -5, 5), rwd(3));
  emit_rational_derivatives(2, 3, rvec(8, -5, 5), rwd(3));
  emit_rational_derivatives(2, 3, rvec(9, -5, 5), rwd(2));
  emit_rational_derivatives(2, 3, rvec(9, -5, 5), rwd(3, true));
  emit_rational_derivatives(0, 1, rvec(1, -5, 5), rwd(1, true));
  emit_rational_derivatives(3, 2, rvec(8, -5, 5), rwd(4, true));
}

// =================================================================================
// Constraint orders, Jacobi parameters
// =================================================================================
static void fam_jacobi_parameters() {
  // every shape, MaxDegree from below the minimum up to beyond the supported WorkDegree, every code
  int codes[] = {-5, -4, -3, -2, -1, 1, 2, 3, 4, 0, 5, -6};
  for (int shape = 0; shape <= 6; ++shape)
    for (int md = -1; md <= 56; md += (md < 12 ? 1 : 5)) {
      int code = codes[(shape + md + 20) % 12];
      begin("jacobi_parameters", "shape i i"); W_shape(shape); W_i(md); W_i(code); bar();
      GUARD(int ng = -777, wd = -777; PLib::JacobiParameters((GeomAbs_Shape)shape, md, code, ng, wd); in(ng); in(wd);)
      finish();
    }
  // all codes for the supported shapes at a few degrees
  for (int shape : {0, 2, 4})
    for (int code : codes)
      for (int md : {3, 5, 9, 14, 21, 30, 40, 44}) {
        if ((shape + code + md) % 3) continue;
        begin("jacobi_parameters", "shape i i"); W_shape(shape); W_i(md); W_i(code); bar();
        GUARD(int ng = -777, wd = -777; PLib::JacobiParameters((GeomAbs_Shape)shape, md, code, ng, wd); in(ng); in(wd);)
        finish();
      }
}
static void fam_constraint_conversions() {
  // ConstraintOrder(niv) and NivConstr(shape) in one record; either one raising makes the record `throws`
  for (int shape = 0; shape <= 6; ++shape)
    for (int niv = -3; niv <= 6; ++niv) {
      if ((shape * 3 + niv + 9) % 3 == 0 || shape < 2) {
        begin("constraint_conversions", "shape i"); W_shape(shape); W_i(niv); bar();
        GUARD(int nv = PLib::NivConstr((GeomAbs_Shape)shape); GeomAbs_Shape sh = PLib::ConstraintOrder(niv); in(nv); in((int)sh);)
        finish();
      }
    }
}

// =================================================================================
// Length of a polynomial curve
// =================================================================================
static void emit_eval_length(int deg, int dim, const Vd& c, double u1, double u2, bool tol_form, double tol) {
  begin(tol_form ? "eval_length_tol" : "eval_length", tol_form ? "i i fs f f f" : "i i fs f f");
  W_i(deg); W_i(dim); W_fs(c); W_f(u1); W_f(u2); if (tol_form) W_f(tol); bar();
  if (deg < 0 || dim < 1 || (long)c.size() < (long)(deg + 1) * dim) unchecked();
  else {
    Vd cc(c); double len = -1, err = -1;
    if (tol_form) { PLib::EvalLength(deg, dim, cc[0], u1, u2, tol, len, err); h(len); h(err); }
    else { PLib::EvalLength(deg, dim, cc[0], u1, u2, len); h(len); }
  }
  finish();
}
static void fam_eval_length() {
  for (int k = 0; k < 40; ++k) {
    int dim = 1 + k % 3 + (k % 10 == 9 ? 3 : 0), deg = k < 30 ? rint(0, 9) : 10 + (k - 30) * 5;
    double u1 = k % 6 == 5 ? rnd(0, 1) : rnd(-1, 0.5), u2 = k % 6 == 5 ? u1 : u1 + rnd(0.2, 2);
    if (k % 7 == 6) std::swap(u1, u2);
    emit_eval_length(deg, dim, rvec((deg + 1) * dim, -2, 2), u1, u2, false, 0);
  }
  // a line has the length of its segment
  emit_eval_length(1, 3, Vd{0, 0, 0, 3, 4, 12}, 0, 1, false, 0);
  emit_eval_length(1, 2, Vd{1, 1, -3, 4}, -1, 1, false, 0);
  emit_eval_length(0, 2, Vd{1, 2}, 0, 5, false, 0);
  // invalid
  emit_eval_length(-1, 2, rvec(2, -1, 1), 0, 1, false, 0);
  emit_eval_length(2, 0, rvec(6, -1, 1), 0, 1, false, 0);
  emit_eval_length(2, 2, rvec(5, -1, 1), 0, 1, false, 0);
  emit_eval_length(3, 3, rvec(11, -1, 1), 0, 1, false, 0);
}
static void fam_eval_length_tol() {
  double tols[] = {1e-1, 1e-3, 1e-6, 1e-9, 1e-12, 0.0, 1e-15, 5e-2, -1.0};
  for (int k = 0; k < 40; ++k) {
    int dim = 1 + k % 3 + (k % 10 == 9 ? 3 : 0), deg = k < 32 ? rint(0, 9) : 10 + (k - 32) * 6;
    double u1 = rnd(-1, 0.5), u2 = u1 + rnd(0.2, 2);
    if (k % 8 == 7) std::swap(u1, u2);
    emit_eval_length(deg, dim, rvec((deg + 1) * dim, -2, 2), u1, u2, true, tols[k % 9]);
  }
  emit_eval_length(1, 3, Vd{0, 0, 0, 3, 4, 12}, 0, 1, true, 1e-9);
  emit_eval_length(0, 2, Vd{1, 2}, 0, 5, true, 1e-9);
  emit_eval_length(5, 2, rvec(12, -1, 1), 0.5, 0.5, true, 1e-9);
  // invalid
  emit_eval_length(-1, 2, rvec(2, -1, 1), 0, 1, true, 1e-6);
  emit_eval_length(2, 0, rvec(6, -1, 1), 0, 1, true, 1e-6);
  emit_eval_length(2, 2, rvec(5, -1, 1), 0, 1, true, 1e-6);
  emit_eval_length(3, 3, rvec(11, -1, 1), 0, 1, true, 1e-6);
}

// =================================================================================
// PLib_JacobiPolynomial and PLib_HermitJacobi
// =================================================================================
static const int NGAUSS[] = {8, 10, 15, 20, 25, 30, 40, 50, 61};
static bool is_ngauss(int n) { for (int g : NGAUSS) if (g == n) return true; return false; }
// a valid (WorkDegree, shape ordinal) pair; `kind` 0..: edge degrees first, then random
static void jpair(int k, int& wd, int& shape) {
  shape = 2 * (k % 3);
  int niv = shape / 2, minw = 2 * (niv + 1);
  int degs[] = {0, 1, 2, 3, 30, 4, 7, 12, 20, 29, 5, 9, 16, 25};
  int jdeg = k < 14 ? degs[(k * 5) % 14] : rint(0, 30);
  wd = minw + jdeg;
}

static void fam_jacobi_new(bool hermit) {
  const char* name = hermit ? "hermit_new" : "jacobi_new";
  for (int shape : {0, 2, 4})
    for (int wd = 0; wd <= 39; ++wd) {
      begin(name, "i shape"); W_i(wd); W_shape(shape); bar();
      GUARD(if (hermit) { PLib_HermitJacobi j(wd, (GeomAbs_Shape)shape); in(j.WorkDegree()); in(j.NivConstr()); }
            else { PLib_JacobiPolynomial j(wd, (GeomAbs_Shape)shape); in(j.WorkDegree()); in(j.NivConstr()); })
      finish();
    }
  for (int shape : {1, 3, 5, 6})
    for (int wd : {0, 6, 10, 20}) { begin(name, "i shape"); W_i(wd); W_shape(shape); bar();
      GUARD(if (hermit) { PLib_HermitJacobi j(wd, (GeomAbs_Shape)shape); in(j.WorkDegree()); in(j.NivConstr()); }
            else { PLib_JacobiPolynomial j(wd, (GeomAbs_Shape)shape); in(j.WorkDegree()); in(j.NivConstr()); })
      finish(); }
  for (int wd : {-1, -10, 40, 100}) for (int shape : {0, 2, 4}) { begin(name, "i shape"); W_i(wd); W_shape(shape); bar();
    GUARD(if (hermit) { PLib_HermitJacobi j(wd, (GeomAbs_Shape)shape); in(j.WorkDegree()); in(j.NivConstr()); }
          else { PLib_JacobiPolynomial j(wd, (GeomAbs_Shape)shape); in(j.WorkDegree()); in(j.NivConstr()); })
    finish(); }
}

static int jdegree(int wd, int shape) { return wd - 2 * (shape / 2 + 1); }
static void fam_jacobi_points() {
  int ns[] = {8, 10, 15, 20, 25, 30, 40, 50, 61, 0, 1, 7, 9, 12, 35, 62, 100, -8};
  for (int k = 0; k < 9; ++k) {
    int wd, shape; jpair(k, wd, shape);
    for (int n : ns) {
      begin("jacobi_points", "i shape i"); W_i(wd); W_shape(shape); W_i(n); bar();
      GUARD(PLib_JacobiPolynomial j(wd, (GeomAbs_Shape)shape);
            NCollection_Array1<double> t(0, std::max(n / 2, 0)); t.Init(SENT); j.Points(n, t);
            for (int i = 0; i <= n / 2; ++i) h(t(i));)
      finish();
    }
  }
  // the constructor fails
  for (int n : {8, 30}) { begin("jacobi_points", "i shape i"); W_i(1); W_shape(0); W_i(n); bar();
    GUARD(PLib_JacobiPolynomial j(1, GeomAbs_C0); in(0);) finish(); }
}
static void fam_jacobi_weights() {
  for (int k = 0; k < 14; ++k) {
    int wd, shape; jpair(k, wd, shape);
    int deg = jdegree(wd, shape);
    int count = 0;
    for (int n : NGAUSS) {
      if (n <= wd && k % 2) continue;                       // keep most of the unchecked ones for the even pairs
      if (n > wd && (deg > 22 ? count++ >= 1 : count++ >= 2)) continue; // limit the size of the file
      begin("jacobi_weights", "i shape i"); W_i(wd); W_shape(shape); W_i(n); bar();
      // n <= WorkDegree reads the weights of another n; n not in the list is not tabulated
      GUARD(PLib_JacobiPolynomial j(wd, (GeomAbs_Shape)shape);
            if (!is_ngauss(n) || n <= wd) unchecked();
            else {
              NCollection_Array2<double> t(0, n / 2, 0, deg); t.Init(SENT); j.Weights(n, t);
              for (int r = 0; r <= n / 2; ++r) for (int c = 0; c <= deg; ++c) h(t(r, c));
            })
      finish();
    }
  }
  for (int n : {0, 5, 9, 12, 35, 62}) { begin("jacobi_weights", "i shape i"); W_i(10); W_shape(2); W_i(n); bar();
    GUARD(PLib_JacobiPolynomial j(10, GeomAbs_C1); unchecked();) finish(); }
  for (int n : {8, 30}) { begin("jacobi_weights", "i shape i"); W_i(1); W_shape(0); W_i(n); bar();
    GUARD(PLib_JacobiPolynomial j(1, GeomAbs_C0); in(0);) finish(); }
}
static void fam_jacobi_max_value() {
  static const int TABLE[] = {57, 55, 53}; // MaxValuesDB_C0/C1/C2
  for (int k = 0; k < 15; ++k) {
    int wd, shape; jpair(k, wd, shape);
    int lens[] = {1, 2, jdegree(wd, shape) + 1, jdegree(wd, shape) + 2, TABLE[shape / 2], TABLE[shape / 2] + 1, TABLE[shape / 2] + 5};
    for (int j = 0; j < 7; ++j) {
      int len = lens[j];
      if (j >= 4 && k % 3) continue;
      begin("jacobi_max_value", "i shape i"); W_i(wd); W_shape(shape); W_i(len); bar();
      GUARD(PLib_JacobiPolynomial p(wd, (GeomAbs_Shape)shape);
            if (len < 1 || len > TABLE[shape / 2]) unchecked();   // reads past the table
            else { NCollection_Array1<double> t(0, len - 1); p.MaxValue(t); for (int i = 0; i < len; ++i) h(t(i)); })
      finish();
    }
  }
  begin("jacobi_max_value", "i shape i"); W_i(1); W_shape(0); W_i(4); bar(); GUARD(PLib_JacobiPolynomial p(1, GeomAbs_C0); in(0);) finish();
  begin("jacobi_max_value", "i shape i"); W_i(3); W_shape(2); W_i(4); bar(); GUARD(PLib_JacobiPolynomial p(3, GeomAbs_C1); in(0);) finish();
  begin("jacobi_max_value", "i shape i"); W_i(10); W_shape(2); W_i(0); bar();
  GUARD(PLib_JacobiPolynomial p(10, GeomAbs_C1); unchecked();) finish();
  begin("jacobi_max_value", "i shape i"); W_i(10); W_shape(2); W_i(-3); bar();
  GUARD(PLib_JacobiPolynomial p(10, GeomAbs_C1); unchecked();) finish();
}

// ---- error estimates (same record for both classes) ----
template <class T> static T mk(int wd, int shape) { return T(wd, (GeomAbs_Shape)shape); }
template <class T> static void emit_max_error(const char* name, int wd, int shape, int dim, const Vd& c, int nd) {
  begin(name, "i shape i fs i"); W_i(wd); W_shape(shape); W_i(dim); W_fs(c); W_i(nd); bar();
  GUARD(T o = mk<T>(wd, shape);
        if (dim < 1 || (long)c.size() < (long)(wd + 1) * dim) unchecked();
        else { Vd cc(c); h(o.MaxError(dim, cc[0], nd)); })
  finish();
}
template <class T> static void emit_average_error(const char* name, int wd, int shape, int dim, const Vd& c, int nd) {
  begin(name, "i shape i fs i"); W_i(wd); W_shape(shape); W_i(dim); W_fs(c); W_i(nd); bar();
  GUARD(T o = mk<T>(wd, shape);
        if (dim < 1 || (long)c.size() < (long)(jdegree(wd, shape) + 1) * dim) unchecked();
        else { Vd cc(c); h(o.AverageError(dim, cc[0], nd)); })
  finish();
}
template <class T> static void emit_reduce(const char* name, int wd, int shape, int dim, int maxdeg, double tol, const Vd& c) {
  begin(name, "i shape i i f fs"); W_i(wd); W_shape(shape); W_i(dim); W_i(maxdeg); W_f(tol); W_fs(c); bar();
  GUARD(T o = mk<T>(wd, shape);
        if (dim < 1 || (long)c.size() < (long)(wd + 1) * dim) unchecked();
        else { Vd cc(c); int nd = -777; double me = -777; o.ReduceDegree(dim, maxdeg, tol, cc[0], nd, me); in(nd); h(me); })
  finish();
}
template <class T> static void fam_errors(const char* pfx) {
  std::string mx = std::string(pfx) + "_max_error", av = std::string(pfx) + "_average_error", rd = std::string(pfx) + "_reduce_degree";
  for (int k = 0; k < 30; ++k) {
    int wd, shape; jpair(k, wd, shape);
    int dim = 1 + k % 3 + (k % 11 == 10 ? 2 : 0);
    int nds[] = {-1, 0, 1, 2, 3, wd - 1, wd, wd + 1, rint(0, wd), rint(0, wd), 2 * (shape / 2 + 1), 2 * (shape / 2 + 1) - 1};
    int nd = nds[k % 12];
    double base = k % 3 == 0 ? 0.5 : (k % 3 == 1 ? 0.2 : 1.0);
    Vd c = rdecay(wd + 1, dim, base);
    emit_max_error<T>(mx.c_str(), wd, shape, dim, c, nd);
    emit_average_error<T>(av.c_str(), wd, shape, dim, c, nd);
    double tols[] = {1e-2, 1e-4, 1e-6, 1e-8, 1e-10, 0.0, 1e-3, 1.0, 1e-12, 10.0, 1e-5, -1.0};
    int maxd = (k % 5 == 0) ? wd : (k % 5 == 1 ? 2 * (shape / 2 + 1) + 1 : rint(0, wd + 2));
    emit_reduce<T>(rd.c_str(), wd, shape, dim, maxd, tols[k % 12], c);
  }
  // special coefficients: all zero (the fallback search), only the low part non-zero, a single big one
  for (int shape : {0, 2, 4}) {
    int wd = 2 * (shape / 2 + 1) + 8;
    Vd z((wd + 1) * 2, 0.0), low = z, one = z;
    for (int i = 0; i < 2 * (2 * (shape / 2 + 1)); ++i) low[i] = rnz(0.5, 2.0);
    one[wd * 2 + 1] = 3.0;
    emit_max_error<T>(mx.c_str(), wd, shape, 2, z, 3); emit_max_error<T>(mx.c_str(), wd, shape, 2, low, 3); emit_max_error<T>(mx.c_str(), wd, shape, 2, one, 3);
    emit_average_error<T>(av.c_str(), wd, shape, 2, z, 2); emit_average_error<T>(av.c_str(), wd, shape, 2, low, 2); emit_average_error<T>(av.c_str(), wd, shape, 2, one, 2);
    emit_reduce<T>(rd.c_str(), wd, shape, 2, wd, 1e-6, z); emit_reduce<T>(rd.c_str(), wd, shape, 2, wd, 1e-6, low); emit_reduce<T>(rd.c_str(), wd, shape, 2, wd, 1e-6, one);
    emit_reduce<T>(rd.c_str(), wd, shape, 2, wd - 1, 1e-6, one);
  }
  // invalid: bad dimension, short coefficient array, the constructor fails
  emit_max_error<T>(mx.c_str(), 10, 2, 0, Vd(11, 1.0), 3);
  emit_max_error<T>(mx.c_str(), 10, 2, 2, rvec(21, -1, 1), 3);
  emit_max_error<T>(mx.c_str(), 3, 2, 1, rvec(4, -1, 1), 3);
  emit_average_error<T>(av.c_str(), 10, 2, 0, Vd(11, 1.0), 3);
  emit_average_error<T>(av.c_str(), 10, 2, 2, rvec(11, -1, 1), 3);
  emit_average_error<T>(av.c_str(), 1, 0, 1, rvec(2, -1, 1), 0);
  emit_reduce<T>(rd.c_str(), 10, 2, 0, 8, 1e-3, Vd(11, 1.0));
  emit_reduce<T>(rd.c_str(), 10, 2, 2, 8, 1e-3, rvec(21, -1, 1));
  emit_reduce<T>(rd.c_str(), 2, 4, 1, 1, 1e-3, rvec(3, -1, 1));
}

template <class T> static void emit_basis(const char* name, int order, int wd, int shape, double u) {
  begin(name, "i shape f"); W_i(wd); W_shape(shape); W_f(u); bar();
  GUARD(T o = mk<T>(wd, shape);
        int n = std::is_same<T, PLib_HermitJacobi>::value ? wd : jdegree(wd, shape);
        NCollection_Array1<double> a(0, n), b(0, n), c(0, n), d(0, n);
        a.Init(SENT); b.Init(SENT); c.Init(SENT); d.Init(SENT);
        if (order == 0) o.D0(u, a); else if (order == 1) o.D1(u, a, b); else if (order == 2) o.D2(u, a, b, c); else o.D3(u, a, b, c, d);
        for (int i = 0; i <= n; ++i) { if (a(i) == SENT) std::abort(); h(a(i)); }
        if (order >= 1) for (int i = 0; i <= n; ++i) { if (b(i) == SENT) std::abort(); h(b(i)); }
        if (order >= 2) for (int i = 0; i <= n; ++i) { if (c(i) == SENT) std::abort(); h(c(i)); }
        if (order >= 3) for (int i = 0; i <= n; ++i) { if (d(i) == SENT) std::abort(); h(d(i)); })
  finish();
}
template <class T> static void fam_basis(const char* pfx) {
  for (int order = 0; order <= 3; ++order) {
    std::string name = std::string(pfx) + "_d" + std::to_string(order);
    for (int k = 0; k < 24; ++k) {
      int wd, shape; jpair(k, wd, shape);
      double us[] = {0.0, 1.0, -1.0, 0.5, -0.5, 1.5, -2.0, 0.3141592653589793, 0.999, 1e-9};
      double u = k < 10 ? us[k] : ru();
      emit_basis<T>(name.c_str(), order, wd, shape, u);
    }
    // the constructor fails
    emit_basis<T>(name.c_str(), order, 1, 0, 0.5);
    emit_basis<T>(name.c_str(), order, 5, 4, 0.5);
    emit_basis<T>(name.c_str(), order, 6, 1, 0.5);
  }
}

// ---- conversion to the canonical basis ----
template <class T> static void emit_to_coefficients(const char* name, int wd, int shape, int dim, int deg, const Vd& c) {
  begin(name, "i shape i i fs"); W_i(wd); W_shape(shape); W_i(dim); W_i(deg); W_fs(c); bar();
  const bool hermit = std::is_same<T, PLib_HermitJacobi>::value;
  GUARD(T o = mk<T>(wd, shape);
        // Jacobi: the transformation tables stop at half-degree 30; Hermit-Jacobi: AuxCoeff(0, (Degree+1)*Dim-1) is too short
        // for Degree below 2*NivConstr+1; short input arrays are read past their end
        if (dim < 1 || deg < 0 || deg > 61 || (long)c.size() < (long)(deg + 1) * dim || (hermit && deg < 2 * (shape / 2) + 1)) unchecked();
        else {
          NCollection_Array1<double> jc(0, (int)c.size() - 1), res(0, (deg + 1) * dim - 1);
          for (size_t i = 0; i < c.size(); ++i) jc((int)i) = c[i];
          res.Init(SENT);
          o.ToCoefficients(dim, deg, jc, res);
          for (int i = 0; i < (deg + 1) * dim; ++i) { if (res(i) == SENT) { std::fprintf(stderr, "ToCoefficients left an entry unwritten\n"); std::abort(); } h(res(i)); }
        })
  finish();
}
template <class T> static void fam_to_coefficients(const char* name) {
  const bool hermit = std::is_same<T, PLib_HermitJacobi>::value;
  for (int k = 0; k < 36; ++k) {
    int wd, shape; jpair(k, wd, shape);
    int dim = 1 + k % 3 + (k % 9 == 8 ? 3 : 0);
    int deg = k % 5 == 0 ? wd : (k % 5 == 1 ? std::max(wd - 1, 0) : rint(hermit ? 2 * (shape / 2) + 1 : 0, wd));
    if (k % 13 == 12) deg = wd + rint(1, 8); // beyond WorkDegree: only the tables limit it
    emit_to_coefficients<T>(name, wd, shape, dim, deg, rvec((deg + 1) * dim, -2, 2));
  }
  for (int shape : {0, 2, 4}) {
    int h1 = 2 * (shape / 2) + 1;
    emit_to_coefficients<T>(name, h1 + 7, shape, 1, h1, rvec(h1 + 1, -2, 2));        // degree = 2*niv+1 (direct copy for Hermit-Jacobi)
    emit_to_coefficients<T>(name, h1 + 7, shape, 2, h1 + 1, rvec(2 * (h1 + 2), -2, 2));
    emit_to_coefficients<T>(name, 36, 4, 1, 61, rvec(62, -1, 1));                      // the last degree the tables hold
  }
  // invalid
  emit_to_coefficients<T>(name, 10, 2, 0, 5, rvec(6, -1, 1));
  emit_to_coefficients<T>(name, 10, 2, 2, 5, rvec(11, -1, 1));
  emit_to_coefficients<T>(name, 10, 2, 1, -1, rvec(2, -1, 1));
  emit_to_coefficients<T>(name, 10, 2, 1, 62, rvec(63, -1, 1));
  emit_to_coefficients<T>(name, 10, 2, 1, 100, rvec(101, -1, 1));
  emit_to_coefficients<T>(name, 10, 2, 1, 2, rvec(3, -1, 1));
  emit_to_coefficients<T>(name, 10, 4, 2, 4, rvec(10, -1, 1));
  emit_to_coefficients<T>(name, 1, 0, 1, 3, rvec(4, -1, 1));
}

int main() {
  // PLib
  fam_eval_polynomial();
  fam_no_derivative();
  fam_poly2var();
  fam_lagrange();
  fam_cubic_hermite();
  fam_hermite_coefficients();
  fam_hermite_interpolate();
  fam_set_poles_3d();
  fam_set_poles_3d_w();
  fam_set_poles_2d();
  fam_set_poles_2d_w();
  fam_get_poles();
  fam_bin();
  fam_coefficients_poles();
  fam_coefficients_poles_surface();
  fam_trimming();
  fam_trimming_surface();
  fam_rational_derivative();
  fam_rational_derivatives();
  fam_jacobi_parameters();
  fam_constraint_conversions();
  fam_eval_length();
  fam_eval_length_tol();
  // PLib_JacobiPolynomial
  fam_jacobi_new(false);
  fam_jacobi_points();
  fam_jacobi_weights();
  fam_jacobi_max_value();
  fam_errors<PLib_JacobiPolynomial>("jacobi");
  fam_basis<PLib_JacobiPolynomial>("jacobi");
  fam_to_coefficients<PLib_JacobiPolynomial>("jacobi_to_coefficients");
  // PLib_HermitJacobi
  fam_jacobi_new(true);
  fam_errors<PLib_HermitJacobi>("hermit");
  fam_basis<PLib_HermitJacobi>("hermit");
  fam_to_coefficients<PLib_HermitJacobi>("hermit_to_coefficients");
  // every family must have at least 16 distinct lines
  for (const std::string& f : fam_order)
    if (fam_lines[f] < 16) { std::fprintf(stderr, "family %s has only %d lines\n", f.c_str(), fam_lines[f]); return 1; }
  if (std::getenv("PLIB_SUMMARY"))
    for (const std::string& f : fam_order) std::fprintf(stderr, "%-34s %4d\n", f.c_str(), fam_lines[f]);
  return 0;
}
