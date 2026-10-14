// Deterministic reference calls for the 3D gp value algebra (OCCT 8.0.1).
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints are decimal, bools are 0/1.
// Every input of a call is printed on the left of " | " in the order named by the
// family's schema string (`begin(name, schema)`); tests/gp3d.rs reads them back with
// the reader method of the same name. Outputs are either values, `throws` (OCCT raised
// an exception) or `unchecked` (OCCT 8.0.1 as built has this check compiled out and
// returns unspecified numbers; the Rust API must report an error there).
//
// Schema words (token counts): f=1 i=1 xyz=3 pnt=3 vec=3 dir=3 (raw, un-normalized
// direction input) ax1=6 (pnt dir) ax2=9 (pnt dir dir) ax3=10 (pnt dir dir flag)
// mat=9 (row-major) quat=4 (x y z w) trsf/gtrsf=variable.
#include <gp.hxx>
#include <gp_XYZ.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>
#include <gp_Dir.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Mat.hxx>
#include <gp_Trsf.hxx>
#include <gp_GTrsf.hxx>
#include <gp_Quaternion.hxx>
#include <gp_QuaternionNLerp.hxx>
#include <gp_QuaternionSLerp.hxx>
#include <gp_EulerSequence.hxx>
#include <gp_TrsfForm.hxx>
#include <Standard_Failure.hxx>
#include <NCollection_Mat4.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

// ---------------------------------------------------------------------------------
// deterministic random numbers
static uint64_t state = 0x9E3779B97F4A7C15ull;
static double rnd(double lo, double hi) {
  state ^= state << 13; state ^= state >> 7; state ^= state << 17;
  return lo + (hi - lo) * (double)(state >> 11) / 9007199254740992.0;
}
static double rnz(double lo, double hi) { double v = rnd(lo, hi); return rnd(0, 1) < 0.5 ? -v : v; }
static double rsc() { return rnd(-3, 3); }
static int rint(int lo, int hi) { int v = lo + (int)std::floor(rnd(0, 1) * (hi - lo + 1)); return v > hi ? hi : v; }

// ---------------------------------------------------------------------------------
// record buffer
static std::string buf;
static size_t mark = 0;
static int ntok = 0, want = -1;
static void h(double v) { uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits); buf += t; ++ntok; }
static void bl(bool v) { buf += v ? " 1" : " 0"; ++ntok; }
static void in(long v) { buf += " " + std::to_string(v); ++ntok; }
static int schema_tokens(const char* sc) {
  int n = 0; std::string s(sc); size_t pos = 0;
  while (pos < s.size()) {
    size_t e = s.find(' ', pos); if (e == std::string::npos) e = s.size();
    std::string w = s.substr(pos, e - pos); pos = e + 1;
    if (w.empty()) continue;
    if (w == "f" || w == "i") n += 1;
    else if (w == "xyz" || w == "pnt" || w == "vec" || w == "dir") n += 3;
    else if (w == "ax1") n += 6;
    else if (w == "ax2") n += 9;
    else if (w == "ax3") n += 10;
    else if (w == "mat") n += 9;
    else if (w == "quat") n += 4;
    else if (w == "trsf" || w == "gtrsf") return -1;
    else { std::fprintf(stderr, "bad schema word %s\n", w.c_str()); std::abort(); }
  }
  return n;
}
static void begin(const char* name, const char* schema) { buf = name; ntok = 0; want = schema_tokens(schema); }
static void bar() {
  if (want >= 0 && ntok != want) { std::fprintf(stderr, "schema mismatch in %s: printed %d expected %d\n", buf.c_str(), ntok, want); std::abort(); }
  buf += " |"; mark = buf.size();
}
static void finish() { buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear(); }
#define GUARD(...) try { __VA_ARGS__ } catch (Standard_Failure&) { buf.resize(mark); buf += " throws"; }
#define UNCHECKED() do { buf.resize(mark); buf += " unchecked"; } while (0)

// ---------------------------------------------------------------------------------
// value printers (used for inputs and outputs alike)
static void xyz(const gp_XYZ& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void pnt(const gp_Pnt& v) { xyz(v.XYZ()); }
static void vec(const gp_Vec& v) { xyz(v.XYZ()); }
static void dir(const gp_Dir& v) { xyz(v.XYZ()); }
static void ax1(const gp_Ax1& a) { pnt(a.Location()); dir(a.Direction()); }
static void ax2(const gp_Ax2& a) { pnt(a.Location()); dir(a.Direction()); dir(a.XDirection()); dir(a.YDirection()); }
static void ax3(const gp_Ax3& a) { pnt(a.Location()); dir(a.Direction()); dir(a.XDirection()); dir(a.YDirection()); }
static void mat(const gp_Mat& a) { for (int r = 1; r <= 3; ++r) for (int c = 1; c <= 3; ++c) h(a.Value(r, c)); }
static void trsf(const gp_Trsf& t) { for (int r = 1; r <= 3; ++r) for (int c = 1; c <= 4; ++c) h(t.Value(r, c)); }
static void gtrsf(const gp_GTrsf& t) { for (int r = 1; r <= 3; ++r) for (int c = 1; c <= 4; ++c) h(t.Value(r, c)); }
static void quat(const gp_Quaternion& q) { h(q.X()); h(q.Y()); h(q.Z()); h(q.W()); }

// ---------------------------------------------------------------------------------
// random input objects. A "raw" direction is the un-normalized triple that is printed;
// the oracle and the test both build the gp_Dir from it.
struct Raw { double x, y, z; };
static void rw(const Raw& r) { h(r.x); h(r.y); h(r.z); }
static gp_Dir D(const Raw& r) { return gp_Dir(r.x, r.y, r.z); }
static gp_Vec V(const Raw& r) { return gp_Vec(r.x, r.y, r.z); }
static Raw rraw() { for (;;) { Raw r{rnd(-4, 4), rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y + r.z * r.z > 0.25) return r; } }
static Raw cross(const Raw& a, const Raw& b) { return Raw{a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x}; }
static Raw lin(const Raw& a, double k, const Raw& b, double l) { return Raw{a.x * k + b.x * l, a.y * k + b.y * l, a.z * k + b.z * l}; }
static double norm(const Raw& a) { return std::sqrt(a.x * a.x + a.y * a.y + a.z * a.z); }
static gp_XYZ rxyz() { double x = rnd(-6, 6), y = rnd(-6, 6), z = rnd(-6, 6); return gp_XYZ(x, y, z); }
static gp_Pnt rp() { return gp_Pnt(rxyz()); }
static gp_Pnt padd(const gp_Pnt& p, const Raw& d, double t) { return gp_Pnt(p.X() + d.x * t, p.Y() + d.y * t, p.Z() + d.z * t); }

struct RA1 { gp_Pnt p; Raw d; gp_Ax1 a; };
struct RA2 { gp_Pnt p; Raw n, x; gp_Ax2 a; };
struct RA3 { gp_Pnt p; Raw n, x; int f; gp_Ax3 a; };
static RA1 mkA1(const gp_Pnt& p, const Raw& d) { RA1 r; r.p = p; r.d = d; r.a = gp_Ax1(p, D(d)); return r; }
static RA2 mkA2(const gp_Pnt& p, const Raw& n, const Raw& x) { RA2 r; r.p = p; r.n = n; r.x = x; r.a = gp_Ax2(p, D(n), D(x)); return r; }
static RA3 mkA3(const gp_Pnt& p, const Raw& n, const Raw& x, int f) {
  RA3 r; r.p = p; r.n = n; r.x = x; r.f = f; r.a = gp_Ax3(gp_Ax2(p, D(n), D(x)));
  if (f == 1) r.a.XReverse(); else if (f == 2) r.a.YReverse();
  return r;
}
static RA1 ra1() { gp_Pnt p = rp(); Raw d = rraw(); return mkA1(p, d); }
static RA2 ra2() { gp_Pnt p = rp(); Raw n = rraw(); Raw x = rraw(); return mkA2(p, n, x); }
static RA3 ra3() { gp_Pnt p = rp(); Raw n = rraw(); Raw x = rraw(); int f = rint(0, 2); return mkA3(p, n, x, f); }
static void pa1(const RA1& r) { pnt(r.p); rw(r.d); }
static void pa2(const RA2& r) { pnt(r.p); rw(r.n); rw(r.x); }
static void pa3(const RA3& r) { pnt(r.p); rw(r.n); rw(r.x); in(r.f); }

struct Mt { double v[9]; gp_Mat m; };
static Mt mkMt(const double* v) { Mt t; for (int k = 0; k < 9; ++k) t.v[k] = v[k]; t.m = gp_Mat(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]); return t; }
static void pm(const Mt& t) { for (int k = 0; k < 9; ++k) h(t.v[k]); }
static Mt rmat() { for (;;) { double v[9]; for (int k = 0; k < 9; ++k) v[k] = rnd(-3, 3); Mt t = mkMt(v); if (std::abs(t.m.Determinant()) > 0.5) return t; } }
static double quarter() { return std::round(rnd(-4, 4) * 4) / 4; }
// rank-2 matrix with exactly representable entries: its determinant is exactly 0
static Mt rmat_singular() {
  double r1[3], r2[3];
  for (int k = 0; k < 3; ++k) r1[k] = quarter();
  for (int k = 0; k < 3; ++k) r2[k] = quarter();
  double v[9] = {r1[0], r1[1], r1[2], r2[0], r2[1], r2[2], r1[0] + 2 * r2[0], r1[1] + 2 * r2[1], r1[2] + 2 * r2[2]};
  return mkMt(v);
}

struct Qn { double c[4]; gp_Quaternion q; };
static Qn mkQ(double x, double y, double z, double w) { Qn t; t.c[0] = x; t.c[1] = y; t.c[2] = z; t.c[3] = w; t.q = gp_Quaternion(x, y, z, w); return t; }
static void pq(const Qn& t) { for (int k = 0; k < 4; ++k) h(t.c[k]); }
static Qn rq(bool unit) {
  double x = rnd(-1, 1), y = rnd(-1, 1), z = rnd(-1, 1), w = rnd(-1, 1);
  gp_Quaternion q(x, y, z, w);
  if (!unit) return mkQ(x, y, z, w);
  q.Normalize();
  return mkQ(q.X(), q.Y(), q.Z(), q.W());
}
// interpolation pairs: random, near-opposite, near-equal (slerp clamp branch), random unit
static void qpair(int n, Qn& A, Qn& B) {
  A = rq((n & 1) == 0);
  double a = rnd(-1, 1), b = rnd(-1, 1), c = rnd(-1, 1), d = rnd(-1, 1);
  switch (n % 4) {
    case 0: B = rq((n & 2) == 0); break;
    case 1: B = mkQ(-A.c[0] + 1e-2 * a, -A.c[1] + 1e-2 * b, -A.c[2] + 1e-2 * c, -A.c[3] + 1e-2 * d); break;
    case 2: B = mkQ(A.c[0] + 1e-6 * a, A.c[1] + 1e-6 * b, A.c[2] + 1e-6 * c, A.c[3] + 1e-6 * d); break;
    default: B = rq(true); break;
  }
}
static double tpar(int n) { return n == 5 ? -0.4 : n == 10 ? 1.6 : n == 0 ? 0.0 : n == 1 ? 1.0 : rnd(0, 1); }

// transformation recipes (kind first, then the fields that kind uses)
//  0 rotation(ax1,angle)   1 rotation + translation part   2 scale(pnt,factor)
//  3 translation           4 mirror ax1   5 mirror pnt   6 mirror ax2(pnt,dir)
//  7 rotation(unit quat)   8 rotation(quat) + translation part + scale factor
// Kinds 2..8 do not use sin/cos.
struct TR { int kind; gp_Pnt p; Raw d; double ang; gp_XYZ v; double sc; Qn q; gp_Trsf t; };
static TR mkTR(int kind) {
  TR r; r.kind = kind; r.p = rp(); r.d = rraw(); r.ang = rnd(-3, 3); r.v = rxyz(); r.sc = rnz(0.3, 2.5); r.q = rq(true);
  switch (kind) {
    case 0: r.t.SetRotation(gp_Ax1(r.p, D(r.d)), r.ang); break;
    case 1: r.t.SetRotation(gp_Ax1(r.p, D(r.d)), r.ang); r.t.SetTranslationPart(gp_Vec(r.v)); break;
    case 2: r.t.SetScale(r.p, r.sc); break;
    case 3: r.t.SetTranslation(gp_Vec(r.v)); break;
    case 4: r.t.SetMirror(gp_Ax1(r.p, D(r.d))); break;
    case 5: r.t.SetMirror(r.p); break;
    case 6: r.t.SetMirror(gp_Ax2(r.p, D(r.d))); break;
    case 7: r.t.SetRotation(r.q.q); break;
    default: r.t.SetRotation(r.q.q); r.t.SetTranslationPart(gp_Vec(r.v)); r.t.SetScaleFactor(r.sc); break;
  }
  return r;
}
static void ptr(const TR& r) {
  in(r.kind);
  switch (r.kind) {
    case 0: pnt(r.p); rw(r.d); h(r.ang); break;
    case 1: pnt(r.p); rw(r.d); h(r.ang); xyz(r.v); break;
    case 2: pnt(r.p); h(r.sc); break;
    case 3: xyz(r.v); break;
    case 4: pnt(r.p); rw(r.d); break;
    case 5: pnt(r.p); break;
    case 6: pnt(r.p); rw(r.d); break;
    case 7: pq(r.q); break;
    default: pq(r.q); xyz(r.v); h(r.sc); break;
  }
}
// general transformation recipe: 0 = GTrsf(Trsf recipe), 1 = GTrsf(matrix, translation)
struct GR { int gk; TR t; Mt m; gp_XYZ v; gp_GTrsf g; };
static GR mkGR(int gk, int tkind, bool singular_ok) {
  GR r; r.gk = gk; r.t = mkTR(tkind); r.m = singular_ok ? rmat_singular() : rmat(); r.v = rxyz();
  if (gk == 0) r.g = gp_GTrsf(r.t.t); else r.g = gp_GTrsf(r.m.m, r.v);
  return r;
}
static void pgr(const GR& r) { in(r.gk); if (r.gk == 0) ptr(r.t); else { pm(r.m); xyz(r.v); } }

static int badidx(int hi) { static const int pick[] = {0, -1, 4, 5, -7, 100}; int v = pick[rint(0, 5)]; if (hi != 3 && v == 4) v = hi + 1; if (v >= 1 && v <= hi) v = hi + 2; return v; }
static void badrc(int rmax, int cmax, int& r, int& c) {
  int w = rint(0, 2);
  r = (w == 1) ? rint(1, rmax) : badidx(rmax);
  c = (w == 0) ? rint(1, cmax) : badidx(cmax);
}

int main() {
  for (int n = 0; n < 16; ++n) {
    double x = rnd(-8, 8), y = rnd(-8, 8), z = rnd(-8, 8);
    double u = rnd(-8, 8), v = rnd(-8, 8), w = rnd(-8, 8), s = rnd(-3, 3);
    if (n == 0) { x = y = z = 0; u = v = w = 0; s = 0; }
    if (n == 1) { x = 1; y = 0; z = 0; u = 1; v = 1e-14; w = 0; s = -1; }
    if (n == 2) { x = 1e150; y = -1e150; z = 1e-150; u = 1e-150; v = 0; w = -1e-150; s = 1e-150; }
    if (n == 3) { s = 1e-15; }
    if (n == 4) { s = 3.141592653589783; }
    gp_XYZ a(x, y, z), c(u, v, w);
    gp_Pnt p(x, y, z), q(u, v, w);
    gp_Vec vv(x, y, z), ww(u, v, w);

    // relation partners: from n>=5 the second vector/direction is made parallel,
    // opposite, normal or nearly equal to the first (mode n%5); n==7 is exactly opposite.
    int mode = n < 5 ? 0 : n % 5;
    double rk = rnd(0.3, 2.0);
    Raw rz{rnd(-1, 1), rnd(-1, 1), rnd(-1, 1)};
    auto rel = [&](const Raw& base_a, const Raw& base_b) -> Raw {
      double m = norm(base_a), e = (n == 7 ? 0.0 : 1e-3 * m);
      switch (mode) {
        case 1: return lin(base_a, rk, rz, e);
        case 2: return lin(base_a, -rk, rz, e);
        case 3: { Raw cr = cross(base_a, base_b); return lin(cr, 1.0, rz, 1e-3 * norm(cr)); }
        case 4: return lin(base_a, 1.0, rz, e);
        default: return base_b;
      }
    };
    double atol = (n % 3 == 2) ? 1e-5 : rnd(0.05, 0.5);
    Raw vaR{x + 2, y + 3, z + 4}, vbaseR{u + 3, v + 2, w + 1};
    Raw vbR = rel(vaR, vbaseR);
    gp_Vec va = V(vaR), vb = V(vbR);

    // ---- gp_XYZ ------------------------------------------------------------
    begin("xyz_add", "xyz xyz"); xyz(a); xyz(c); bar(); xyz(a.Added(c)); finish();
    begin("xyz_subtract", "xyz xyz"); xyz(a); xyz(c); bar(); xyz(a.Subtracted(c)); finish();
    begin("xyz_cross", "xyz xyz"); xyz(a); xyz(c); bar(); xyz(a.Crossed(c)); finish();
    begin("xyz_dot", "xyz xyz"); xyz(a); xyz(c); bar(); h(a.Dot(c)); finish();
    { gp_XYZ e = rxyz();
      begin("xyz_dot_cross", "xyz xyz xyz"); xyz(a); xyz(c); xyz(e); bar(); h(a.DotCross(c, e)); finish(); }
    begin("xyz_modulus", "xyz"); xyz(a); bar(); h(a.Modulus()); h(a.SquareModulus()); finish();
    begin("xyz_cross_magnitude", "xyz xyz"); xyz(a); xyz(c); bar(); h(a.CrossMagnitude(c)); h(a.CrossSquareMagnitude(c)); finish();
    begin("xyz_multiply", "xyz xyz f"); xyz(a); xyz(c); h(s); bar(); xyz(a.Multiplied(s)); xyz(a.Multiplied(c)); finish();
    { Mt xm = (n == 6 || n == 11) ? rmat_singular() : rmat();
      begin("xyz_multiply_matrix", "xyz mat"); xyz(a); pm(xm); bar();
      xyz(a.Multiplied(xm.m)); gp_XYZ xmi = a; xmi.Multiply(xm.m); xyz(xmi); finish(); }
    begin("xyz_divide", "xyz f"); xyz(a); h(s); bar();
    // OCCT 8.0.1 as built does not check the divisor: 0 gives NaN/inf (unchecked)
    if (std::abs(s) <= gp::Resolution()) UNCHECKED(); else xyz(a.Divided(s));
    finish();
    begin("xyz_reverse", "xyz"); xyz(a); bar(); xyz(a.Reversed()); finish();
    { // near-equal pairs in odd iterations so both outcomes occur
      bool near = n >= 5 && (n % 2 == 1);
      gp_XYZ bq = near ? gp_XYZ(x + rnd(-0.02, 0.02), y + rnd(-0.02, 0.02), z + rnd(-0.02, 0.02)) : c;
      double tol = near ? rnd(0, 0.03) : std::abs(s);
      begin("xyz_equal", "xyz xyz f"); xyz(a); xyz(bq); h(tol); bar(); bl(a.IsEqual(bq, tol)); finish(); }
    { int idx = rint(1, 3);
      begin("xyz_access", "xyz i"); xyz(a); in(idx); bar();
      h(a.Coord(1)); h(a.Coord(2)); h(a.Coord(3)); h(a.GetData()[0]); h(a.GetData()[1]); h(a.GetData()[2]); h(a.Coord(idx)); finish(); }
    { int idx = badidx(3);
      begin("xyz_bad_index", "xyz i"); xyz(a); in(idx); bar(); GUARD(h(a.Coord(idx));) finish(); }
    { int idx = badidx(3); double val = rnd(-3, 3);
      begin("xyz_bad_set_index", "xyz i f"); xyz(a); in(idx); h(val); bar(); GUARD(gp_XYZ t = a; t.SetCoord(idx, val); xyz(t);) finish(); }
    { int idx = badidx(3);
      begin("xyz_bad_change_index", "xyz i"); xyz(a); in(idx); bar(); GUARD(gp_XYZ t = a; h(t.ChangeCoord(idx));) finish(); }
    { int i1 = rint(1, 3), i2 = rint(1, 3), i3 = rint(0, 2);
      begin("xyz_setters", "xyz xyz f i i i"); xyz(a); xyz(c); h(s); in(i1); in(i2); in(i3); bar();
      gp_XYZ t = a; t.SetCoord(i1, u); t.SetX(v); t.SetY(w); t.SetZ(s); t.ChangeCoord(i2) = x; t.ChangeData()[i3] = y; xyz(t); finish(); }
    begin("xyz_sub_in_place", "xyz xyz"); xyz(a); xyz(c); bar(); { gp_XYZ t = a; t.Subtract(c); xyz(t); } finish();
    { gp_XYZ e = rxyz();
      begin("xyz_cross_cross", "xyz xyz xyz"); xyz(a); xyz(c); xyz(e); bar(); xyz(a.CrossCrossed(c, e)); finish(); }
    { double r2 = rsc(), r3 = rsc(); gp_XYZ e1 = rxyz(), e2 = rxyz(), e3 = rxyz();
      begin("xyz_linear_forms", "xyz xyz f f f xyz xyz xyz"); xyz(a); xyz(c); h(s); h(r2); h(r3); xyz(e1); xyz(e2); xyz(e3); bar();
      gp_XYZ lf;
      lf.SetLinearForm(s, a, r2, c); xyz(lf);
      lf.SetLinearForm(s, a, r2, c, e1); xyz(lf);
      lf.SetLinearForm(s, a, r2, c, r3, e2); xyz(lf);
      lf.SetLinearForm(s, a, r2, c, r3, e2, e3); xyz(lf);
      lf.SetLinearForm(s, a, e3); xyz(lf);
      lf.SetLinearForm(a, c); xyz(lf);
      finish(); }
    begin("xyz_add_in_place", "xyz xyz"); xyz(a); xyz(c); bar(); { gp_XYZ t = a; t.Add(c); xyz(t); } finish();
    begin("xyz_cross_in_place", "xyz xyz"); xyz(a); xyz(c); bar(); { gp_XYZ t = a; t.Cross(c); xyz(t); } finish();
    begin("xyz_scale_in_place", "xyz f"); xyz(a); h(s); bar(); { gp_XYZ t = a; t.Multiply(s); xyz(t); } finish();
    begin("xyz_normalize", "xyz"); xyz(a); bar(); GUARD(gp_XYZ t = a; t.Normalize(); xyz(t);) finish();

    // ---- gp_Pnt ------------------------------------------------------------
    begin("pnt_access", "pnt"); pnt(p); bar(); h(p.Coord(1)); h(p.Coord(2)); h(p.Coord(3)); xyz(p.Coord()); finish();
    { int idx = badidx(3);
      begin("pnt_bad_coord", "pnt i"); pnt(p); in(idx); bar(); GUARD(h(p.Coord(idx));) finish(); }
    { int idx = badidx(3); double val = rsc();
      begin("pnt_bad_set_index", "pnt i f"); pnt(p); in(idx); h(val); bar(); GUARD(gp_Pnt t = p; t.SetCoord(idx, val); pnt(t);) finish(); }
    { int i1 = rint(1, 3);
      begin("pnt_setters", "pnt xyz f i"); pnt(p); xyz(c); h(s); in(i1); bar();
      gp_Pnt t = p; t.SetCoord(i1, u); t.SetCoord(v, w, s); t.SetX(x); t.SetY(y); t.SetZ(z); t.SetXYZ(c); t.ChangeCoord().SetCoord(x, u, v); pnt(t); finish(); }
    { double al = rnd(-2, 2), be = rnd(-2, 2);
      begin("pnt_barycenter", "pnt pnt f f"); pnt(p); pnt(q); h(al); h(be); bar(); gp_Pnt t = p; t.BaryCenter(al, q, be); pnt(t); finish(); }
    { bool near = n >= 5 && (n % 2 == 1);
      gp_Pnt bq = near ? gp_Pnt(x + rnd(-0.02, 0.02), y + rnd(-0.02, 0.02), z + rnd(-0.02, 0.02)) : q;
      double tol = near ? rnd(0, 0.03) : std::abs(s);
      begin("pnt_equal", "pnt pnt f"); pnt(p); pnt(bq); h(tol); bar(); bl(p.IsEqual(bq, tol)); finish(); }
    begin("pnt_mirror_point", "pnt pnt"); pnt(p); pnt(q); bar(); pnt(p.Mirrored(q)); { gp_Pnt t = p; t.Mirror(q); pnt(t); } finish();
    { RA1 ax = ra1();
      begin("pnt_mirror_axis", "pnt ax1"); pnt(p); pa1(ax); bar(); pnt(p.Mirrored(ax.a)); gp_Pnt t = p; t.Mirror(ax.a); pnt(t); finish(); }
    { RA2 pl = ra2();
      begin("pnt_mirror_plane", "pnt ax2"); pnt(p); pa2(pl); bar(); pnt(p.Mirrored(pl.a)); gp_Pnt t = p; t.Mirror(pl.a); pnt(t); finish(); }
    { RA1 ax = ra1();
      begin("pnt_rotate", "pnt ax1 f"); pnt(p); pa1(ax); h(s); bar(); pnt(p.Rotated(ax.a, s)); gp_Pnt t = p; t.Rotate(ax.a, s); pnt(t); finish(); }
    begin("pnt_scale", "pnt pnt f"); pnt(p); pnt(q); h(s); bar(); pnt(p.Scaled(q, s)); { gp_Pnt t = p; t.Scale(q, s); pnt(t); } finish();
    begin("pnt_translate_vec", "pnt vec"); pnt(p); vec(gp_Vec(c)); bar(); pnt(p.Translated(gp_Vec(c))); { gp_Pnt t = p; t.Translate(gp_Vec(c)); pnt(t); } finish();
    { gp_Pnt p1 = rp(), p2 = rp();
      begin("pnt_translate_points", "pnt pnt pnt"); pnt(p); pnt(p1); pnt(p2); bar(); pnt(p.Translated(p1, p2)); gp_Pnt t = p; t.Translate(p1, p2); pnt(t); finish(); }
    begin("pnt_distance", "pnt pnt"); pnt(p); pnt(q); bar(); h(p.Distance(q)); h(p.SquareDistance(q)); finish();
    begin("pnt_translate", "pnt pnt"); pnt(p); pnt(q); bar(); pnt(p.Translated(p, q)); finish();
    { TR tr = mkTR(2 + (n + 1) % 7);
      begin("pnt_transform", "pnt trsf"); pnt(p); ptr(tr); bar(); pnt(p.Transformed(tr.t)); gp_Pnt t = p; t.Transform(tr.t); pnt(t); finish(); }

    // ---- gp_Vec ------------------------------------------------------------
    { Raw d1 = rraw(), d2 = rraw();
      begin("vec_constructors", "xyz xyz dir dir"); xyz(a); xyz(c); rw(d1); rw(d2); bar();
      vec(gp_Vec()); vec(gp_Vec(a)); vec(gp_Vec(gp_Dir(gp_Dir::D::Y))); vec(gp_Vec(gp_Dir(0, 0, 1))); vec(gp_Vec(D(d1))); vec(gp_Vec(p, q)); vec(gp_Vec(D(d2))); finish(); }
    { int i1 = rint(1, 3);
      begin("vec_access", "vec vec f i"); vec(vv); vec(ww); h(s); in(i1); bar();
      h(vv.Coord(1)); h(vv.Coord(2)); h(vv.Coord(3));
      gp_Vec t = vv; t.SetCoord(i1, u); t.SetCoord(v, w, s); t.SetX(x); t.SetY(y); t.SetZ(z); t.SetXYZ(c); vec(t); finish(); }
    { int idx = badidx(3);
      begin("vec_bad_coord", "vec i"); vec(vv); in(idx); bar(); GUARD(h(vv.Coord(idx));) finish(); }
    { int idx = badidx(3); double val = rsc();
      begin("vec_bad_set_index", "vec i f"); vec(vv); in(idx); h(val); bar(); GUARD(gp_Vec t = vv; t.SetCoord(idx, val); vec(t);) finish(); }
    gp_Vec vzero;
    { int which = rint(0, 1);
      begin("vec_zero_angle", "vec i"); vec(vv); in(which); bar(); GUARD(h(which == 0 ? vzero.Angle(vv) : vv.Angle(vzero));) finish(); }
    { int which = rint(0, 2);
      begin("vec_zero_angle_ref", "vec i"); vec(vv); in(which); bar();
      GUARD(h(which == 0 ? vv.AngleWithRef(vv, vzero) : which == 1 ? vzero.AngleWithRef(vv, vv) : vv.AngleWithRef(vzero, vv));) finish(); }
    { double tol = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec_zero_is_normal", "vec f i"); vec(vv); h(tol); in(which); bar(); GUARD(bl(which == 0 ? vzero.IsNormal(vv, tol) : vv.IsNormal(vzero, tol));) finish(); }
    { double tol = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec_zero_is_opposite", "vec f i"); vec(vv); h(tol); in(which); bar(); GUARD(bl(which == 0 ? vzero.IsOpposite(vv, tol) : vv.IsOpposite(vzero, tol));) finish(); }
    { double tol = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec_zero_is_parallel", "vec f i"); vec(vv); h(tol); in(which); bar(); GUARD(bl(which == 0 ? vzero.IsParallel(vv, tol) : vv.IsParallel(vzero, tol));) finish(); }
    begin("vec_cross", "vec vec"); vec(vv); vec(ww); bar(); vec(vv.Crossed(ww)); h(vv.Dot(ww)); finish();
    begin("vec_arithmetic", "vec vec f"); vec(vv); vec(ww); h(s); bar(); vec(vv.Added(ww)); vec(vv.Subtracted(ww)); vec(vv.Multiplied(s)); finish();
    begin("vec_normalize", "vec"); vec(vv); bar(); GUARD(vec(vv.Normalized());) finish();
    { double lt = rnd(0.01, 1.0);
      begin("vec_relations", "vec vec f f"); vec(va); vec(vb); h(lt); h(atol); bar();
      bl(va.IsEqual(vb, lt, atol)); bl(va.IsNormal(vb, atol)); bl(va.IsOpposite(vb, atol)); bl(va.IsParallel(vb, atol)); finish(); }
    { gp_Vec vr(rxyz());
      begin("vec_queries", "vec vec vec"); vec(va); vec(vb); vec(vr); bar();
      h(va.Angle(vb)); h(va.AngleWithRef(vb, vr)); h(va.Magnitude()); h(va.SquareMagnitude()); h(va.CrossMagnitude(vb)); h(va.CrossSquareMagnitude(vb)); h(va.Dot(vb)); h(va.DotCross(vb, vr)); finish(); }
    { gp_Vec vr(rxyz()); double r1 = rsc(), r2 = rsc();
      begin("vec_mutators", "vec vec vec f f f"); vec(va); vec(vb); vec(vr); h(s); h(r1); h(r2); bar();
      double sd = (s == 0 ? 1 : s);
      vec(va.Added(vb)); vec(va.Subtracted(vb)); vec(va.Multiplied(s)); vec(va.Divided(sd)); vec(va.CrossCrossed(vb, vr)); vec(va.Reversed());
      gp_Vec vm = va; vm.Add(vb); vec(vm); vm = va; vm.Subtract(vb); vec(vm); vm = va; vm.Multiply(s); vec(vm); vm = va; vm.Divide(sd); vec(vm);
      vm = va; vm.Cross(vb); vec(vm); vm = va; vm.CrossCross(vb, vr); vec(vm); vm = va; vm.Reverse(); vec(vm);
      vm.SetLinearForm(s, va, r1, vb); vec(vm); vm.SetLinearForm(s, va, r1, vb, vr); vec(vm); vm.SetLinearForm(s, va, r1, vb, r2, vr); vec(vm);
      vm.SetLinearForm(s, va, r1, vb, r2, vr, va); vec(vm); vm.SetLinearForm(s, va, vr); vec(vm); vm.SetLinearForm(va, vb); vec(vm);
      finish(); }
    { RA1 ax = ra1(); RA2 pl = ra2();
      begin("vec_mirrors", "vec vec ax1 ax2"); vec(va); vec(vb); pa1(ax); pa2(pl); bar();
      vec(va.Mirrored(vb)); gp_Vec vm = va; vm.Mirror(vb); vec(vm);
      vec(va.Mirrored(ax.a)); vm = va; vm.Mirror(ax.a); vec(vm); vec(va.Mirrored(pl.a)); vm = va; vm.Mirror(pl.a); vec(vm); finish(); }
    { RA1 ax = ra1();
      begin("vec_motion", "vec ax1 f"); vec(va); pa1(ax); h(s); bar();
      vec(va.Rotated(ax.a, s)); gp_Vec vm = va; vm.Rotate(ax.a, s); vec(vm); vec(va.Scaled(s)); vm = va; vm.Scale(s); vec(vm);
      gp_Trsf vt; vt.SetRotation(ax.a, s); vec(va.Transformed(vt)); vm = va; vm.Transform(vt); vec(vm); finish(); }

    // ---- gp_Dir ------------------------------------------------------------
    Raw aR{x, y, z}, cR{u, v, w};
    Raw b2R = (n < 5) ? cR : rel(aR, cR);
    begin("dir_construct", "xyz"); xyz(a); bar(); GUARD(dir(gp_Dir(x, y, z)); dir(gp_Dir(gp_Vec(a))); dir(gp_Dir(a));) finish();
    begin("dir_angle", "xyz xyz"); xyz(a); rw(b2R); bar(); GUARD(gp_Dir d(x, y, z), e(b2R.x, b2R.y, b2R.z); h(d.Angle(e)); h(d.Dot(e));) finish();
    begin("dir_cross", "xyz xyz"); xyz(a); rw(b2R); bar(); GUARD(gp_Dir d(x, y, z), e(b2R.x, b2R.y, b2R.z); dir(d.Crossed(e));) finish();
    { Raw d1 = rraw(); int i1 = rint(1, 3); double r1 = rsc(), r2 = rsc(), r3 = rsc(), r4 = rsc(); gp_XYZ e1 = rxyz(), e2 = rxyz(), e3 = rxyz();
      begin("dir_setters", "dir i f f f f xyz xyz xyz"); rw(d1); in(i1); h(r1); h(r2); h(r3); h(r4); xyz(e1); xyz(e2); xyz(e3); bar();
      GUARD(gp_Dir dd = D(d1); h(dd.Coord(1)); h(dd.Coord(2)); h(dd.Coord(3));
            gp_Dir ds = dd; ds.SetCoord(i1, r1); ds.SetX(r2); ds.SetY(r3); ds.SetZ(r4); ds.SetXYZ(e1); dir(ds);
            dir(gp_Dir()); dir(gp_Dir(gp_Dir::D::NX)); dir(gp_Dir(gp_Vec(e2))); dir(gp_Dir(e3)); dir(gp_Dir());)
      finish(); }
    { Raw d1 = rraw(); int idx = badidx(3);
      begin("dir_bad_coord", "dir i"); rw(d1); in(idx); bar(); GUARD(h(D(d1).Coord(idx));) finish(); }
    { Raw d1 = rraw(); int idx = badidx(3); double val = rsc();
      begin("dir_bad_set_index", "dir i f"); rw(d1); in(idx); h(val); bar(); GUARD(gp_Dir t = D(d1); t.SetCoord(idx, val); dir(t);) finish(); }
    begin("axis_enum_constructors", "pnt"); pnt(p); bar();
    ax1(gp_Ax1());
    for (auto d : {gp_Dir::D::X, gp_Dir::D::Y, gp_Dir::D::Z, gp_Dir::D::NX, gp_Dir::D::NY, gp_Dir::D::NZ}) {
      gp_Dir sd(d); dir(sd); ax1(gp_Ax1(d)); ax1(gp_Ax1(p, d)); ax2(gp_Ax2(d)); ax2(gp_Ax2(p, d)); ax3(gp_Ax3(d)); ax3(gp_Ax3(p, d));
    }
    ax2(gp_Ax2(p, gp_Dir::D::Z, gp_Dir::D::X)); ax3(gp_Ax3(p, gp_Dir::D::Z, gp_Dir::D::X)); finish();
    { Raw d1 = rraw(); Raw d2 = rel(d1, rraw()); double tol = atol;
      begin("dir_queries", "dir dir f"); rw(d1); rw(d2); h(tol); bar();
      gp_Dir dd = D(d1), de = D(d2); bl(dd.IsEqual(de, tol)); bl(dd.IsNormal(de, tol)); bl(dd.IsOpposite(de, tol)); bl(dd.IsParallel(de, tol)); finish(); }
    { Raw d1 = rraw(); Raw d2 = rel(d1, rraw()); Raw d3 = rraw();
      begin("dir_angles", "dir dir dir"); rw(d1); rw(d2); rw(d3); bar();
      gp_Dir dd = D(d1), de = D(d2), dr = D(d3); h(dd.Angle(de)); h(dd.AngleWithRef(de, dr)); h(dd.Dot(de)); h(dd.DotCross(de, dr)); finish(); }
    { Raw d1 = rraw(), d2 = rraw(), d3 = rraw();
      begin("dir_ops", "dir dir dir"); rw(d1); rw(d2); rw(d3); bar();
      GUARD(gp_Dir dd = D(d1), de = D(d2), dr = D(d3);
            dir(dd.Crossed(de)); dir(dd.CrossCrossed(de, dr)); dir(dd.Reversed());
            gp_Dir dm = dd; dm.Cross(de); dir(dm); dm = dd; dm.CrossCross(de, dr); dir(dm); dm = dd; dm.Reverse(); dir(dm);)
      finish(); }
    { Raw d1 = rraw(), d2 = rraw(); RA1 ax = ra1(); RA2 pl = ra2();
      begin("dir_mirrors", "dir dir ax1 ax2"); rw(d1); rw(d2); pa1(ax); pa2(pl); bar();
      gp_Dir dd = D(d1), de = D(d2); dir(dd.Mirrored(de)); gp_Dir dm = dd; dm.Mirror(de); dir(dm);
      dir(dd.Mirrored(ax.a)); dm = dd; dm.Mirror(ax.a); dir(dm); dir(dd.Mirrored(pl.a)); dm = dd; dm.Mirror(pl.a); dir(dm); finish(); }
    { Raw d1 = rraw(); RA1 ax = ra1();
      begin("dir_motion", "dir ax1 f"); rw(d1); pa1(ax); h(s); bar();
      gp_Dir dd = D(d1); dir(dd.Rotated(ax.a, s)); gp_Dir dm = dd; dm.Rotate(ax.a, s); dir(dm);
      gp_Trsf dt; dt.SetRotation(ax.a, s); dir(dd.Transformed(dt)); dm = dd; dm.Transform(dt); dir(dm); finish(); }

    // ---- gp_Ax1 ------------------------------------------------------------
    RA1 aa = mkA1(p, rraw());
    { gp_Pnt nl = rp(); Raw nd = rraw();
      begin("ax1_access", "ax1 pnt dir"); pa1(aa); pnt(nl); rw(nd); bar();
      ax1(aa.a); gp_Ax1 t = aa.a; t.SetLocation(nl); t.SetDirection(D(nd)); ax1(t); finish(); }
    { Raw d2 = rel(aa.d, rraw()); double tt = rnd(-2, 2); double lt = rnd(0.01, 0.5);
      gp_Pnt L = (mode == 1 || mode == 2) ? padd(p, aa.d, tt) : rp();
      RA1 ab = mkA1(L, d2);
      begin("ax1_relations", "ax1 ax1 f f"); pa1(aa); pa1(ab); h(atol); h(lt); bar();
      bl(aa.a.IsCoaxial(ab.a, atol, lt)); bl(aa.a.IsNormal(ab.a, atol)); bl(aa.a.IsOpposite(ab.a, atol)); bl(aa.a.IsParallel(ab.a, atol)); h(aa.a.Angle(ab.a));
      bl(aa.a.IsCoaxial(gp_Ax1(aa.a), 1e-9, 1e-9)); bl(aa.a.IsOpposite(gp_Ax1(aa.p, D(aa.d).Reversed()), 0.1)); bl(aa.a.IsParallel(gp_Ax1(aa.a), 0.1)); finish(); }
    begin("ax1_reverse", "ax1"); pa1(aa); bar(); ax1(aa.a.Reversed()); { gp_Ax1 t = aa.a; t.Reverse(); ax1(t); } finish();
    begin("ax1_mirror_point", "ax1 pnt"); pa1(aa); pnt(q); bar(); ax1(aa.a.Mirrored(q)); { gp_Ax1 t = aa.a; t.Mirror(q); ax1(t); } finish();
    { RA1 o = ra1();
      begin("ax1_mirror_axis", "ax1 ax1"); pa1(aa); pa1(o); bar(); ax1(aa.a.Mirrored(o.a)); gp_Ax1 t = aa.a; t.Mirror(o.a); ax1(t); finish(); }
    { RA2 pl = ra2();
      begin("ax1_mirror_plane", "ax1 ax2"); pa1(aa); pa2(pl); bar(); ax1(aa.a.Mirrored(pl.a)); gp_Ax1 t = aa.a; t.Mirror(pl.a); ax1(t); finish(); }
    { RA1 vx = ra1();
      begin("ax1_motion", "ax1 ax1 pnt f"); pa1(aa); pa1(vx); pnt(q); h(s); bar();
      ax1(aa.a.Rotated(vx.a, s)); gp_Ax1 t = aa.a; t.Rotate(vx.a, s); ax1(t); ax1(aa.a.Scaled(q, s)); t = aa.a; t.Scale(q, s); ax1(t);
      gp_Trsf tr; tr.SetRotation(vx.a, s); ax1(aa.a.Transformed(tr)); t = aa.a; t.Transform(tr); ax1(t); finish(); }
    begin("ax1_translate_vec", "ax1 vec"); pa1(aa); vec(gp_Vec(c)); bar(); ax1(aa.a.Translated(gp_Vec(c))); { gp_Ax1 t = aa.a; t.Translate(gp_Vec(c)); ax1(t); } finish();
    { gp_Pnt p1 = rp(), p2 = rp();
      begin("ax1_translate_points", "ax1 pnt pnt"); pa1(aa); pnt(p1); pnt(p2); bar(); ax1(aa.a.Translated(p1, p2)); gp_Ax1 t = aa.a; t.Translate(p1, p2); ax1(t); finish(); }

    // ---- gp_Ax2 ------------------------------------------------------------
    RA2 a2 = mkA2(p, rraw(), rraw());
    { gp_Pnt nl = rp(); RA1 nx = ra1(); Raw d4 = rraw(), d5 = rraw(), d6 = rraw();
      begin("ax2_access", "ax2 pnt ax1 dir dir dir"); pa2(a2); pnt(nl); pa1(nx); rw(d4); rw(d5); rw(d6); bar();
      ax2(a2.a); gp_Ax2 t = a2.a; t.SetLocation(nl); t.SetAxis(nx.a); t.SetDirection(D(d4)); t.SetXDirection(D(d5)); t.SetYDirection(D(d6)); ax2(t);
      ax2(gp_Ax2()); ax2(gp_Ax2(p, gp_Dir::D::Z)); ax2(gp_Ax2(gp_Dir::D::Y)); ax2(gp_Ax2(nl, D(d4))); finish(); }
    { RA2 b = ra2();
      begin("ax2_relations", "ax2 ax2"); pa2(a2); pa2(b); bar(); h(a2.a.Angle(b.a)); finish(); }
    { int m4 = n % 4; Raw w1 = cross(a2.n, rraw()); double t1 = rnd(-2, 2), k = rnd(0.3, 2.0), tn = rnz(0.5, 2.0);
      gp_Pnt L = m4 == 0 ? rp() : (m4 == 2 ? padd(p, a2.n, tn) : padd(p, w1, t1));
      Raw dO = m4 == 0 ? rraw() : (m4 == 3 ? lin(a2.n, -k, a2.n, 0) : lin(a2.n, k, a2.n, 0));
      RA2 o2 = mkA2(L, dO, rraw());
      Raw d1 = m4 == 0 ? rraw() : cross(a2.n, rraw());
      RA1 o1 = mkA1(L, d1);
      double lt = m4 == 0 ? rnd(0.01, 0.2) : 1e-6, at = m4 == 0 ? rnd(0.01, 0.2) : 1e-6;
      begin("ax2_coplanar", "ax2 ax2 ax1 f f"); pa2(a2); pa2(o2); pa1(o1); h(lt); h(at); bar();
      bl(a2.a.IsCoplanar(o2.a, lt, at)); bl(a2.a.IsCoplanar(o1.a, lt, at)); finish(); }
    { RA1 o = ra1(); gp_Pnt cen = rp(); RA2 pl = ra2(); RA1 vx = ra1();
      begin("ax2_mirror_point", "ax2 pnt"); pa2(a2); pnt(cen); bar(); ax2(a2.a.Mirrored(cen)); { gp_Ax2 t = a2.a; t.Mirror(cen); ax2(t); } finish();
      begin("ax2_mirror_axis", "ax2 ax1"); pa2(a2); pa1(o); bar(); ax2(a2.a.Mirrored(o.a)); { gp_Ax2 t = a2.a; t.Mirror(o.a); ax2(t); } finish();
      begin("ax2_mirror_plane", "ax2 ax2"); pa2(a2); pa2(pl); bar(); ax2(a2.a.Mirrored(pl.a)); { gp_Ax2 t = a2.a; t.Mirror(pl.a); ax2(t); } finish();
      begin("ax2_motion", "ax2 ax1 pnt f"); pa2(a2); pa1(vx); pnt(q); h(s); bar();
      ax2(a2.a.Rotated(vx.a, s)); gp_Ax2 t = a2.a; t.Rotate(vx.a, s); ax2(t); ax2(a2.a.Scaled(q, s)); t = a2.a; t.Scale(q, s); ax2(t);
      gp_Trsf tr; tr.SetRotation(vx.a, s); ax2(a2.a.Transformed(tr)); t = a2.a; t.Transform(tr); ax2(t); finish(); }
    begin("ax2_translate_vec", "ax2 vec"); pa2(a2); vec(gp_Vec(c)); bar(); ax2(a2.a.Translated(gp_Vec(c))); { gp_Ax2 t = a2.a; t.Translate(gp_Vec(c)); ax2(t); } finish();
    { gp_Pnt p1 = rp(), p2 = rp();
      begin("ax2_translate_points", "ax2 pnt pnt"); pa2(a2); pnt(p1); pnt(p2); bar(); ax2(a2.a.Translated(p1, p2)); gp_Ax2 t = a2.a; t.Translate(p1, p2); ax2(t); finish(); }

    // ---- gp_Ax3 (every third iteration is left-handed) -----------------------
    RA3 a3 = mkA3(p, rraw(), rraw(), n % 3);
    { gp_Pnt nl = rp(); RA1 nx = ra1(); Raw d4 = rraw(), d5 = rraw(), d6 = rraw();
      begin("ax3_access", "ax3 pnt ax1 dir dir dir"); pa3(a3); pnt(nl); pa1(nx); rw(d4); rw(d5); rw(d6); bar();
      ax3(a3.a); gp_Ax3 t = a3.a; t.SetLocation(nl); t.SetAxis(nx.a); t.SetDirection(D(d4)); t.SetXDirection(D(d5)); t.SetYDirection(D(d6)); ax3(t);
      ax3(a3.a.Ax2()); ax3(gp_Ax3()); ax3(gp_Ax3(p, gp_Dir::D::Z)); ax3(gp_Ax3(nl, D(d4), D(d5))); ax3(gp_Ax3(gp_Dir::D::Y)); finish(); }
    { RA3 b = ra3();
      begin("ax3_relations", "ax3 ax3"); pa3(a3); pa3(b); bar(); h(a3.a.Angle(b.a)); ax3(a3.a.Ax2()); finish(); }
    begin("ax3_direct", "ax3"); pa3(a3); bar(); bl(a3.a.Direct()); { gp_Ax3 left = a3.a; left.XReverse(); bl(!left.Direct()); } finish();
    begin("ax3_reverse", "ax3"); pa3(a3); bar(); { gp_Ax3 rv = a3.a; rv.XReverse(); ax3(rv); rv = a3.a; rv.YReverse(); ax3(rv); rv = a3.a; rv.ZReverse(); ax3(rv); } finish();
    { int m4 = n % 4; Raw w1 = cross(a3.n, rraw()); double t1 = rnd(-2, 2), k = rnd(0.3, 2.0), tn = rnz(0.5, 2.0);
      gp_Pnt L = m4 == 0 ? rp() : (m4 == 2 ? padd(p, a3.n, tn) : padd(p, w1, t1));
      Raw dO = m4 == 0 ? rraw() : (m4 == 3 ? lin(a3.n, -k, a3.n, 0) : lin(a3.n, k, a3.n, 0));
      RA3 o3 = mkA3(L, dO, rraw(), rint(0, 2));
      Raw d1 = m4 == 0 ? rraw() : cross(a3.n, rraw());
      // gp_Ax3::IsCoplanar(Ax1) requires the line to pass through the origin of the Ax3
      gp_Pnt L1 = m4 == 1 ? a3.p : (m4 == 3 ? padd(a3.p, d1, t1) : L);
      RA1 o1 = mkA1(L1, d1);
      double lt = m4 == 0 ? rnd(0.01, 0.2) : 1e-6, at = m4 == 0 ? rnd(0.01, 0.2) : 1e-6;
      begin("ax3_coplanar", "ax3 ax3 ax1 f f"); pa3(a3); pa3(o3); pa1(o1); h(lt); h(at); bar();
      bl(a3.a.IsCoplanar(o3.a, lt, at)); bl(a3.a.IsCoplanar(o1.a, lt, at)); finish(); }
    { RA1 o = ra1(); gp_Pnt cen = rp(); RA2 pl = ra2(); RA1 vx = ra1();
      begin("ax3_mirror_point", "ax3 pnt"); pa3(a3); pnt(cen); bar(); ax3(a3.a.Mirrored(cen)); { gp_Ax3 t = a3.a; t.Mirror(cen); ax3(t); } finish();
      begin("ax3_mirror_axis", "ax3 ax1"); pa3(a3); pa1(o); bar(); ax3(a3.a.Mirrored(o.a)); { gp_Ax3 t = a3.a; t.Mirror(o.a); ax3(t); } finish();
      begin("ax3_mirror_plane", "ax3 ax2"); pa3(a3); pa2(pl); bar(); ax3(a3.a.Mirrored(pl.a)); { gp_Ax3 t = a3.a; t.Mirror(pl.a); ax3(t); } finish();
      begin("ax3_motion", "ax3 ax1 pnt f"); pa3(a3); pa1(vx); pnt(q); h(s); bar();
      ax3(a3.a.Rotated(vx.a, s)); gp_Ax3 t = a3.a; t.Rotate(vx.a, s); ax3(t); ax3(a3.a.Scaled(q, s)); t = a3.a; t.Scale(q, s); ax3(t);
      gp_Trsf tr; tr.SetRotation(vx.a, s); ax3(a3.a.Transformed(tr)); t = a3.a; t.Transform(tr); ax3(t); finish(); }
    begin("ax3_translate_vec", "ax3 vec"); pa3(a3); vec(gp_Vec(c)); bar(); ax3(a3.a.Translated(gp_Vec(c))); { gp_Ax3 t = a3.a; t.Translate(gp_Vec(c)); ax3(t); } finish();
    { gp_Pnt p1 = rp(), p2 = rp();
      begin("ax3_translate_points", "ax3 pnt pnt"); pa3(a3); pnt(p1); pnt(p2); bar(); ax3(a3.a.Translated(p1, p2)); gp_Ax3 t = a3.a; t.Translate(p1, p2); ax3(t); finish(); }

    // ---- gp_Mat (iterations 0, 6 and 11 use a singular matrix) ----------------
    Mt m1 = (n == 0 || n == 6 || n == 11) ? rmat_singular() : rmat();
    if (n == 0) { double z9[9] = {0, 0, 0, 0, 0, 0, 0, 0, 0}; m1 = mkMt(z9); }
    { gp_XYZ e1 = rxyz(), e2 = rxyz(), e3 = rxyz();
      begin("mat_constructors", "mat xyz xyz xyz"); pm(m1); xyz(e1); xyz(e2); xyz(e3); bar();
      mat(gp_Mat()); mat(m1.m); mat(gp_Mat(e1, e2, e3)); finish(); }
    { int ci = rint(1, 3), ri = rint(1, 3), vr = rint(1, 3), vc = rint(1, 3), cr = rint(1, 3), cc = rint(1, 3); double nv = rsc();
      begin("mat_access", "mat i i i i i i f"); pm(m1); in(ci); in(ri); in(vr); in(vc); in(cr); in(cc); h(nv); bar();
      xyz(m1.m.Column(ci)); xyz(m1.m.Row(ri)); xyz(m1.m.Diagonal()); h(m1.m.Value(vr, vc)); h(m1.m.Determinant());
      gp_Mat t = m1.m; t.ChangeValue(cr, cc) = nv; mat(t); finish(); }
    begin("mat_singular", "mat"); pm(m1); bar(); bl(m1.m.IsSingular()); finish();
    { int r, cc; badrc(3, 3, r, cc);
      begin("mat_bad_index", "mat i i"); pm(m1); in(r); in(cc); bar(); GUARD(h(m1.m.Value(r, cc));) finish(); }
    { int idx = badidx(3);
      begin("mat_bad_row", "mat i"); pm(m1); in(idx); bar(); GUARD((void)m1.m.Row(idx); UNCHECKED();) finish(); }
    { int idx = badidx(3);
      begin("mat_bad_column", "mat i"); pm(m1); in(idx); bar(); GUARD((void)m1.m.Column(idx); UNCHECKED();) finish(); }
    { int idx = badidx(3); gp_XYZ e = rxyz();
      begin("mat_bad_set_row", "mat i xyz"); pm(m1); in(idx); xyz(e); bar(); GUARD(gp_Mat t = m1.m; t.SetRow(idx, e); UNCHECKED();) finish(); }
    { int idx = badidx(3); gp_XYZ e = rxyz();
      begin("mat_bad_set_column", "mat i xyz"); pm(m1); in(idx); xyz(e); bar(); GUARD(gp_Mat t = m1.m; t.SetCol(idx, e); UNCHECKED();) finish(); }
    { int r, cc; badrc(3, 3, r, cc);
      begin("mat_bad_change_value", "mat i i"); pm(m1); in(r); in(cc); bar(); GUARD(gp_Mat t = m1.m; h(t.ChangeValue(r, cc));) finish(); }
    { int r, cc; badrc(3, 3, r, cc); double val = rsc();
      begin("mat_bad_set_value", "mat i i f"); pm(m1); in(r); in(cc); h(val); bar(); GUARD(gp_Mat t = m1.m; t.SetValue(r, cc, val); mat(t);) finish(); }
    { double ang = rsc();
      begin("mat_zero_rotation", "f"); h(ang); bar(); GUARD(gp_Mat t; t.SetRotation(gp_XYZ(), ang); UNCHECKED();) finish(); }
    { int ci = rint(1, 3), ri = rint(1, 3), vr = rint(1, 3), vc = rint(1, 3);
      gp_XYZ e1 = rxyz(), e2 = rxyz(), e3 = rxyz(), e4 = rxyz(), e5 = rxyz();
      double f1 = rsc(), f2 = rsc(), f3 = rsc(), f4 = rsc(), f5 = rsc(), ang = rsc(); Raw ax = rraw();
      begin("mat_setters", "mat i xyz xyz xyz xyz xyz i f f f f xyz f i i f");
      pm(m1); in(ci); xyz(e1); xyz(e2); xyz(e3); xyz(e4); xyz(e5); in(ri); h(f1); h(f2); h(f3); h(f4); rw(ax); h(ang); in(vr); in(vc); h(f5); bar();
      gp_Mat ms = m1.m;
      ms.SetCol(ci, e1); mat(ms); ms.SetCols(e1, e2, e3); mat(ms); ms.SetRow(ri, e5); mat(ms); ms.SetRows(e3, e4, e5); mat(ms);
      ms.SetCross(e4); mat(ms); ms.SetDot(e5); mat(ms); ms.SetDiagonal(f1, f2, f3); mat(ms); ms.SetScale(f4); mat(ms); ms.SetIdentity(); mat(ms);
      ms.SetRotation(gp_XYZ(ax.x, ax.y, ax.z), ang); mat(ms); ms.SetValue(vr, vc, f5); mat(ms); finish(); }
    { Mt m2 = rmat(); double d = rnz(0.1, 3), sm = rsc();
      begin("mat_arithmetic", "mat mat f f"); pm(m1); pm(m2); h(d); h(sm); bar();
      const gp_Mat& ma = m1.m; const gp_Mat& mb = m2.m;
      mat(ma.Added(mb)); mat(ma.Subtracted(mb)); mat(ma.Divided(d)); mat(ma.Multiplied(mb)); mat(ma.Multiplied(sm));
      gp_Mat mm = ma; mm.Add(mb); mat(mm); mm = ma; mm.Subtract(mb); mat(mm); mm = ma; mm.Divide(d); mat(mm); mm = ma; mm.Multiply(mb); mat(mm);
      mm = ma; mm.PreMultiply(mb); mat(mm); mm = ma; mm.Multiply(sm); mat(mm); finish(); }
    { int pe = rint(-3, 3);
      begin("mat_inverse_power_transpose", "mat i"); pm(m1); in(pe); bar();
      if (std::abs(m1.m.Determinant()) <= gp::Resolution()) UNCHECKED();
      else {
        mat(m1.m.Inverted()); gp_Mat mm = m1.m; mm.Invert(); mat(mm); mat(m1.m.Powered(pe)); mm = m1.m; mm.Power(pe); mat(mm);
        mat(m1.m.Powered(0)); mat(m1.m.Transposed()); mm = m1.m; mm.Transpose(); mat(mm);
      }
      finish(); }

    // ---- gp_Trsf -------------------------------------------------------------
    {
      RA1 mx = ra1(); RA2 plane = ra2(); RA1 rot = ra1(); Raw tqd = rraw(); double tqa = rsc(), sc = rnz(0.3, 2.5); RA3 fr = ra3(), to = ra3();
      begin("trsf_setters", "pnt pnt ax1 ax2 ax1 f vec f f ax3 ax3 vec xyz");
      pnt(p); pnt(q); pa1(mx); pa2(plane); pa1(rot); h(s); rw(tqd); h(tqa); h(sc); pa3(fr); pa3(to); vec(gp_Vec(c)); xyz(a); bar();
      gp_Quaternion tq(V(tqd), tqa);
      gp_Trsf ts;
      ts.SetMirror(q); trsf(ts); ts.SetMirror(mx.a); trsf(ts); ts.SetMirror(plane.a); trsf(ts);
      ts.SetRotation(rot.a, s); trsf(ts); ts.SetRotation(tq); trsf(ts); ts.SetRotationPart(tq); trsf(ts);
      ts.SetScale(q, sc); trsf(ts); ts.SetDisplacement(fr.a, to.a); trsf(ts); ts.SetTransformation(fr.a, to.a); trsf(ts); ts.SetTransformation(fr.a); trsf(ts);
      ts.SetTransformation(tq, gp_Vec(c)); trsf(ts); ts.SetTranslation(gp_Vec(c)); trsf(ts); ts.SetTranslation(p, q); trsf(ts);
      ts.SetTranslationPart(gp_Vec(c)); trsf(ts); ts.SetScaleFactor(sc); trsf(ts); ts.SetForm(gp_Identity); trsf(ts);
      ts.SetValues(1, 0, 0, x, 0, 1, 0, y, 0, 0, 1, z); trsf(ts);
      finish();
    }
    { TR tr = mkTR((n + 2) % 9); gp_Trsf& tt = tr.t;
      begin("trsf_queries", "trsf"); ptr(tr); bar();
      h(tt.ScaleFactor()); xyz(tt.TranslationPart()); mat(tt.VectorialPart()); mat(tt.HVectorialPart()); trsf(tt); finish(); }
    { TR tr = mkTR((n + 4) % 9);
      begin("trsf_mat4", "trsf"); ptr(tr); bar();
      NCollection_Mat4<double> tout; tr.t.GetMat4(tout); for (int r = 0; r < 4; ++r) for (int k = 0; k < 4; ++k) h(tout.GetValue(r, k)); finish(); }
    { TR tr = mkTR((n + 5) % 9);
      begin("trsf_negative", "trsf"); ptr(tr); bar(); bl(tr.t.IsNegative()); finish(); }
    { TR tr = mkTR((n + 6) % 9);
      begin("trsf_form", "trsf"); ptr(tr); bar(); in((int)tr.t.Form()); finish(); }
    { TR tr = mkTR((n + 7) % 9);
      begin("trsf_get_rotation", "trsf"); ptr(tr); bar();
      gp_XYZ raxis; double rang = 0; bool hasrot = tr.t.GetRotation(raxis, rang); bl(hasrot); xyz(raxis); h(rang); finish(); }
    { TR tr = mkTR((n + 8) % 9);
      begin("trsf_quaternion", "trsf"); ptr(tr); bar(); quat(tr.t.GetRotation()); finish(); }
    { TR tr = mkTR((n + 3) % 9);
      begin("trsf_transform_coords", "trsf xyz"); ptr(tr); xyz(a); bar();
      gp_XYZ tc = a; tr.t.Transforms(tc); xyz(tc); double tx = x, ty = y, tz = z; tr.t.Transforms(tx, ty, tz); h(tx); h(ty); h(tz); finish(); }
    { TR t1 = mkTR((n + 1) % 9), t2 = mkTR((n * 5 + 3) % 9); int pe = rint(-3, 3);
      begin("trsf_composition", "trsf trsf i"); ptr(t1); ptr(t2); in(pe); bar();
      trsf(t1.t.Multiplied(t2.t)); gp_Trsf tc = t1.t; tc.Multiply(t2.t); trsf(tc); tc = t1.t; tc.PreMultiply(t2.t); trsf(tc);
      trsf(t1.t.Powered(pe)); tc = t1.t; tc.Power(pe); trsf(tc); finish(); }
    { TR tr = mkTR((n + 6) % 9);
      begin("trsf_inverse", "trsf"); ptr(tr); bar(); gp_Trsf ti = tr.t; ti.Invert(); trsf(ti); finish(); }
    { Raw d1 = rraw();
      begin("trsf_rotate", "pnt dir f xyz"); pnt(p); rw(d1); h(s); xyz(a); bar();
      gp_Trsf t; t.SetRotation(gp_Ax1(p, D(d1)), s); pnt(p.Transformed(t)); vec(vv.Transformed(t));
      for (int k = 1; k <= 4; ++k) h(t.Value(1, k));
      in((int)t.Form()); finish(); }
    { TR tr = mkTR((n + 2) % 9); int r, cc; badrc(3, 4, r, cc);
      begin("trsf_bad_value", "trsf i i"); ptr(tr); in(r); in(cc); bar(); GUARD(h(tr.t.Value(r, cc));) finish(); }
    begin("trsf_zero_scale", "pnt"); pnt(p); bar(); GUARD(gp_Trsf zero; zero.SetScale(p, 0); UNCHECKED();) finish();

    // ---- gp_GTrsf (families compared bit-exactly use the sin/cos-free recipes) ----
    { TR tr = mkTR(2 + (n + 3) % 7); Mt mm = rmat(); gp_XYZ e = rxyz();
      begin("gtrsf_constructors", "trsf mat xyz"); ptr(tr); pm(mm); xyz(e); bar();
      gtrsf(gp_GTrsf()); gtrsf(gp_GTrsf(tr.t)); gtrsf(gp_GTrsf(mm.m, e)); finish(); }
    { RA1 af = ra1(); RA2 pl = ra2(); int ri = rint(1, 3), ci = rint(1, 4); double val = rsc(); Mt mm = rmat(); TR tr = mkTR(n % 9); double r1 = rnz(0.3, 3), r2 = rnz(0.3, 3), r3 = rnz(0.3, 3);
      begin("gtrsf_setters", "ax1 ax2 f i i f mat xyz trsf f f f xyz");
      pa1(af); pa2(pl); h(s); in(ri); in(ci); h(val); pm(mm); xyz(c); ptr(tr); h(r1); h(r2); h(r3); xyz(a); bar();
      gp_GTrsf gs;
      gs.SetAffinity(af.a, s); gtrsf(gs); gs.SetAffinity(pl.a, s); gtrsf(gs); gs.SetValue(ri, ci, val); gtrsf(gs); gs.SetVectorialPart(mm.m); gtrsf(gs);
      gs.SetTranslationPart(c); gtrsf(gs); gs.SetTrsf(tr.t); gtrsf(gs); gs.SetForm(); gtrsf(gs);
      NCollection_Mat4<double> m4; m4.InitIdentity(); m4.SetValue(0, 0, r1); m4.SetValue(1, 1, r2); m4.SetValue(2, 2, r3);
      m4.SetValue(0, 3, x); m4.SetValue(1, 3, y); m4.SetValue(2, 3, z); gs.SetMat4(m4); gtrsf(gs); finish(); }
    { GR g = mkGR(n % 4 == 3 ? 1 : 0, 2 + (n + 1) % 7, n % 8 == 7);
      begin("gtrsf_queries", "gtrsf"); pgr(g); bar(); gtrsf(g.g); xyz(g.g.TranslationPart()); mat(g.g.VectorialPart()); finish(); }
    { GR g = mkGR(n % 4 == 1 ? 1 : 0, (n + 4) % 9, n % 8 == 5);
      begin("gtrsf_flags", "gtrsf"); pgr(g); bar(); bl(g.g.IsNegative()); bl(g.g.IsSingular()); finish(); }
    { GR g = mkGR(n % 4 == 2 ? 1 : 0, (n + 7) % 9, n % 8 == 6);
      begin("gtrsf_form", "gtrsf"); pgr(g); bar(); in((int)g.g.Form()); finish(); }
    { GR g = mkGR(n % 4 == 3 ? 1 : 0, 2 + (n + 5) % 7, false); int r1 = rint(1, 3), c1 = rint(1, 4), r2 = rint(1, 3), c2 = rint(1, 4);
      begin("gtrsf_value_transform", "gtrsf i i i i xyz"); pgr(g); in(r1); in(c1); in(r2); in(c2); xyz(a); bar();
      h(g.g.Value(r1, c1)); h(g.g.Value(r2, c2)); gp_XYZ gcoord = a; g.g.Transforms(gcoord); xyz(gcoord);
      double gx = x, gy = y, gz = z; g.g.Transforms(gx, gy, gz); h(gx); h(gy); h(gz); finish(); }
    { GR g1 = mkGR(n % 4 == 3 ? 1 : 0, 2 + (n + 2) % 7, false), g2 = mkGR(n % 4 == 1 ? 1 : 0, 2 + (n * 3 + 1) % 7, false); int pe = rint(-3, 3);
      begin("gtrsf_composition", "gtrsf gtrsf i"); pgr(g1); pgr(g2); in(pe); bar();
      gtrsf(g1.g.Multiplied(g2.g)); gp_GTrsf gc = g1.g; gc.Multiply(g2.g); gtrsf(gc); gc = g1.g; gc.PreMultiply(g2.g); gtrsf(gc);
      gtrsf(g1.g.Powered(pe)); gc = g1.g; gc.Power(pe); gtrsf(gc); finish(); }
    { GR g = mkGR(n % 4 == 0 && n > 0 ? 1 : 0, 2 + (n + 6) % 7, false);
      begin("gtrsf_to_trsf", "gtrsf"); pgr(g); bar(); GUARD(trsf(g.g.Trsf());) finish(); }
    { GR g = mkGR(n % 4 == 2 ? 1 : 0, 2 + (n + 4) % 7, false);
      begin("gtrsf_mat4", "gtrsf"); pgr(g); bar();
      NCollection_Mat4<double> gout; g.g.GetMat4(gout); for (int r = 0; r < 4; ++r) for (int k = 0; k < 4; ++k) h(gout.GetValue(r, k)); finish(); }
    { GR g = mkGR(n % 4 == 0 ? 1 : 0, 2 + n % 7, false); int r, cc; badrc(3, 4, r, cc);
      begin("gtrsf_bad_value", "gtrsf i i"); pgr(g); in(r); in(cc); bar(); GUARD(h(g.g.Value(r, cc));) finish(); }
    { Raw d1 = rraw();
      begin("gtrsf_point", "pnt dir f xyz"); pnt(p); rw(d1); h(s); xyz(a); bar();
      gp_Trsf t; t.SetRotation(gp_Ax1(p, D(d1)), s); gp_GTrsf gt(t); gp_XYZ gx = a; gt.Transforms(gx); xyz(gx); finish(); }

    // ---- gp_Quaternion ---------------------------------------------------------
    { Qn qa = rq((n & 1) == 0); gp_Vec help(rxyz()); Raw axr = rraw(); Raw mar = rraw(); double mang = rsc();
      begin("quat_constructors", "quat vec vec vec vec f xyz f"); pq(qa); vec(va); vec(vb); vec(help); rw(axr); h(s); rw(mar); h(mang); bar();
      quat(gp_Quaternion()); quat(qa.q); quat(gp_Quaternion(va, vb)); quat(gp_Quaternion(va, vb, help)); quat(gp_Quaternion(V(axr), s));
      gp_Mat qmat; qmat.SetRotation(gp_XYZ(mar.x, mar.y, mar.z), mang); quat(gp_Quaternion(qmat)); finish(); }
    { Qn A, B; qpair(n, A, B);
      if (n % 4 == 3) B = A;
      if (n % 4 == 2) { A = mkQ(0, A.c[1], A.c[2], A.c[3]); B = mkQ(1e-310, A.c[1], A.c[2], A.c[3]); }
      begin("quat_equal", "quat quat"); pq(A); pq(B); bar(); bl(A.q.IsEqual(B.q)); finish(); }
    { gp_Vec help(rxyz()); Raw axr = rraw(); Raw mar = rraw(); double mang = rsc(); gp_Vec f2(c);
      double e1 = rnd(-3, 3), e2 = rnz(0.3, 1.2), e3 = rnd(-3, 3);
      begin("quat_rotation_setters", "vec vec vec vec vec f xyz f f f f"); vec(va); vec(vb); vec(f2); vec(help); rw(axr); h(s); rw(mar); h(mang); h(e1); h(e2); h(e3); bar();
      gp_Quaternion qrot; qrot.SetRotation(va, vb); quat(qrot);
      qrot.SetRotation(f2, f2.Reversed(), help); quat(qrot);
      qrot.SetVectorAndAngle(V(axr), s); quat(qrot);
      gp_Vec qaxis; double qangle = 0; qrot.GetVectorAndAngle(qaxis, qangle); vec(qaxis); h(qangle);
      gp_Mat qmat; qmat.SetRotation(gp_XYZ(mar.x, mar.y, mar.z), mang); qrot.SetMatrix(qmat); quat(qrot); mat(qrot.GetMatrix());
      qrot.SetEulerAngles(gp_Extrinsic_XYZ, e1, e2, e3); quat(qrot);
      double qa1, qb1, qc1; qrot.GetEulerAngles(gp_Extrinsic_XYZ, qa1, qb1, qc1); h(qa1); h(qb1); h(qc1); finish(); }
    { double e1 = rnd(-3, 3), e2 = rnz(0.3, 1.2), e3 = rnd(-3, 3); Qn qn = rq(true);
      begin("quat_euler_sequences", "f f f quat"); h(e1); h(e2); h(e3); pq(qn); bar();
      for (gp_EulerSequence e : {gp_EulerAngles, gp_YawPitchRoll, gp_Extrinsic_XYZ, gp_Extrinsic_XZY, gp_Extrinsic_YZX, gp_Extrinsic_YXZ, gp_Extrinsic_ZXY, gp_Extrinsic_ZYX,
                                 gp_Intrinsic_XYZ, gp_Intrinsic_XZY, gp_Intrinsic_YZX, gp_Intrinsic_YXZ, gp_Intrinsic_ZXY, gp_Intrinsic_ZYX,
                                 gp_Extrinsic_XYX, gp_Extrinsic_XZX, gp_Extrinsic_YZY, gp_Extrinsic_YXY, gp_Extrinsic_ZYZ, gp_Extrinsic_ZXZ,
                                 gp_Intrinsic_XYX, gp_Intrinsic_XZX, gp_Intrinsic_YZY, gp_Intrinsic_YXY, gp_Intrinsic_ZXZ, gp_Intrinsic_ZYZ}) {
        gp_Quaternion qe; qe.SetEulerAngles(e, e1, e2, e3); quat(qe);
        double qx, qy, qz; qe.GetEulerAngles(e, qx, qy, qz); h(qx); h(qy); h(qz);
        qn.q.GetEulerAngles(e, qx, qy, qz); h(qx); h(qy); h(qz);
      }
      finish(); }
    { Qn qa = rq((n & 1) == 0), qb = rq((n & 2) == 0); double c4[4] = {rsc(), rsc(), rsc(), rsc()};
      begin("quat_components", "quat quat f f f f"); pq(qa); pq(qb); h(c4[0]); h(c4[1]); h(c4[2]); h(c4[3]); bar();
      quat(qa.q); h(qa.q.X()); h(qa.q.Y()); h(qa.q.Z()); h(qa.q.W());
      gp_Quaternion qset = qa.q; qset.Set(c4[0], c4[1], c4[2], c4[3]); quat(qset); qset.Set(qb.q); quat(qset); qset.SetIdent(); quat(qset); finish(); }
    { Qn qa = rq((n & 1) == 0); double sc = rnz(0.2, 3);
      begin("quat_norm_ops", "quat f"); pq(qa); h(sc); bar();
      const gp_Quaternion& q0 = qa.q;
      h(q0.SquareNorm()); h(q0.Norm()); quat(q0.Reversed()); gp_Quaternion qset = q0; qset.Reverse(); quat(qset); quat(q0.Inverted()); qset = q0; qset.Invert(); quat(qset);
      quat(q0.Scaled(sc)); qset = q0; qset.Scale(sc); quat(qset); qset = q0; qset.StabilizeLength(); quat(qset); quat(q0.Normalized()); qset = q0; qset.Normalize(); quat(qset);
      quat(q0.Negated()); h(q0.GetRotationAngle()); finish(); }
    { Qn qa = rq((n & 1) == 0), qb = rq((n & 2) == 0); double sc = rnz(0.2, 3); gp_Vec vq(rxyz());
      begin("quat_arithmetic", "quat quat f vec"); pq(qa); pq(qb); h(sc); vec(vq); bar();
      quat(qa.q.Added(qb.q)); quat(qa.q.Subtracted(qb.q)); quat(qa.q.Multiplied(qb.q)); h(qa.q.Dot(qb.q)); quat(qa.q.Scaled(sc)); quat(qa.q.Negated());
      gp_Quaternion qset = qa.q; qset.Add(qb.q); quat(qset); qset = qa.q; qset.Subtract(qb.q); quat(qset); qset = qa.q; qset.Multiply(qb.q); quat(qset);
      vec(qa.q.Multiply(vq)); finish(); }
    { double ang = rsc();
      begin("quat_zero_axis_angle", "f"); h(ang); bar(); GUARD(gp_Quaternion bad(gp_Vec(), ang); quat(bad); UNCHECKED();) finish(); }
    { double ang = rsc();
      begin("quat_zero_set_axis", "f"); h(ang); bar(); GUARD(gp_Quaternion bad; bad.SetVectorAndAngle(gp_Vec(), ang); quat(bad); UNCHECKED();) finish(); }
    { Qn A, B; qpair(n, A, B); double t = tpar(n);
      begin("quat_nlerp_all", "quat quat f"); pq(A); pq(B); h(t); bar();
      quat(gp_QuaternionNLerp::Interpolate(A.q, B.q, t)); gp_QuaternionNLerp nl(A.q, B.q); gp_Quaternion nr; nl.Interpolate(t, nr); quat(nr);
      gp_QuaternionNLerp nli; nli.Init(A.q, B.q); nli.Interpolate(t, nr); quat(nr);
      gp_QuaternionNLerp nlu; nlu.InitFromUnit(A.q.Normalized(), B.q.Normalized()); nlu.Interpolate(t, nr); quat(nr); finish(); }
    { Qn A, B; qpair(n + 1, A, B); double t = tpar(n + 3);
      begin("quat_slerp_all", "quat quat f"); pq(A); pq(B); h(t); bar();
      quat(gp_QuaternionSLerp::Interpolate(A.q, B.q, t)); gp_QuaternionSLerp sl(A.q, B.q); gp_Quaternion sr; sl.Interpolate(t, sr); quat(sr);
      gp_QuaternionSLerp sli; sli.Init(A.q, B.q); sli.Interpolate(t, sr); quat(sr);
      gp_QuaternionSLerp slu; slu.InitFromUnit(A.q.Normalized(), B.q.Normalized()); slu.Interpolate(t, sr); quat(sr); finish(); }
    { Raw axr = rraw();
      begin("quaternion_axis_angle", "xyz vec f"); xyz(a); rw(axr); h(s); bar();
      gp_Quaternion qq(V(axr), s); quat(qq); vec(qq.Multiply(gp_Vec(x, y, z))); finish(); }
    { Raw axr = rraw(); gp_Quaternion q1(V(axr), s), q2(va, vb); Qn A = mkQ(q1.X(), q1.Y(), q1.Z(), q1.W()), B = mkQ(q2.X(), q2.Y(), q2.Z(), q2.W());
      begin("quaternion_product", "quat quat"); pq(A); pq(B); bar(); gp_Quaternion qp = A.q.Multiplied(B.q); quat(qp); h(A.q.Dot(B.q)); finish(); }
    { Qn A, B; qpair(n + 2, A, B); double t = tpar(n + 1);
      begin("quaternion_nlerp", "quat quat f"); pq(A); pq(B); h(t); bar(); quat(gp_QuaternionNLerp::Interpolate(A.q, B.q, t)); finish(); }
    { Qn A, B; qpair(n + 3, A, B); double t = tpar(n + 2);
      begin("quaternion_slerp", "quat quat f"); pq(A); pq(B); h(t); bar(); quat(gp_QuaternionSLerp::Interpolate(A.q, B.q, t)); finish(); }
  }

  // ---- families without any input: printed exactly once ------------------------
  begin("xyz_constructors", ""); bar(); xyz(gp_XYZ()); xyz(gp_XYZ()); xyz(gp_XYZ(1, 2, 3)); finish();
  begin("pnt_constructors", ""); bar(); pnt(gp_Pnt()); pnt(gp_Pnt(gp_XYZ(1, 2, 3))); pnt(gp_Pnt(1, 2, 3)); pnt(gp::Origin()); finish();
  begin("quat_zero_normalize", ""); bar(); { gp_Quaternion zero(0, 0, 0, 0); zero.Normalize(); quat(zero); } finish();
  begin("constants", ""); bar();
  h(gp::Resolution()); pnt(gp::Origin()); dir(gp::DX()); dir(gp::DY()); dir(gp::DZ()); pnt(gp::OX().Location()); dir(gp::OX().Direction());
  pnt(gp::XOY().Location()); dir(gp::XOY().Direction()); dir(gp::XOY().XDirection()); dir(gp::XOY().YDirection()); finish();
  begin("constants_oy", ""); bar(); pnt(gp::OY().Location()); dir(gp::OY().Direction()); finish();
  begin("constants_oz", ""); bar(); pnt(gp::OZ().Location()); dir(gp::OZ().Direction()); finish();
  begin("constants_zox", ""); bar(); pnt(gp::ZOX().Location()); dir(gp::ZOX().Direction()); dir(gp::ZOX().XDirection()); dir(gp::ZOX().YDirection()); finish();
  begin("constants_yoz", ""); bar(); pnt(gp::YOZ().Location()); dir(gp::YOZ().Direction()); dir(gp::YOZ().XDirection()); dir(gp::YOZ().YDirection()); finish();
  begin("enum_values", ""); bar();
  in(gp_EulerAngles); in(gp_YawPitchRoll); in(gp_Extrinsic_XYZ); in(gp_Extrinsic_XZY); in(gp_Extrinsic_YZX); in(gp_Extrinsic_YXZ); in(gp_Extrinsic_ZXY); in(gp_Extrinsic_ZYX);
  in(gp_Intrinsic_XYZ); in(gp_Intrinsic_XZY); in(gp_Intrinsic_YZX); in(gp_Intrinsic_YXZ); in(gp_Intrinsic_ZXY); in(gp_Intrinsic_ZYX);
  in(gp_Extrinsic_XYX); in(gp_Extrinsic_XZX); in(gp_Extrinsic_YZY); in(gp_Extrinsic_YXY); in(gp_Extrinsic_ZYZ); in(gp_Extrinsic_ZXZ);
  in(gp_Intrinsic_XYX); in(gp_Intrinsic_XZX); in(gp_Intrinsic_YZY); in(gp_Intrinsic_YXY); in(gp_Intrinsic_ZXZ); in(gp_Intrinsic_ZYZ);
  in(gp_Identity); in(gp_Rotation); in(gp_Translation); in(gp_PntMirror); in(gp_Ax1Mirror); in(gp_Ax2Mirror); in(gp_Scale); in(gp_CompoundTrsf); in(gp_Other);
  finish();
}
