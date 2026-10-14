// Deterministic reference calls for the 2D gp value algebra (OCCT 8.0.1).
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints are decimal, bools are 0/1.
// Every input of a call is printed on the left of " | " in the order named by the
// family's schema string (`begin(name, schema)`); tests/gp2d.rs reads them back with the
// reader method of the same name. Outputs are either values, `throws` (OCCT raised an
// exception) or `unchecked` (OCCT 8.0.1 as built has this check compiled out and returns
// unspecified numbers; the Rust API must report an error there).
//
// Schema words (token counts): f=1 i=1 xy=2 pnt=2 vec=2 dir=2 (raw, un-normalized
// direction input) ax2d=4 (pnt dir) ax22d=6 (pnt dirX dirY: the frame is built with
// gp_Ax22d(P, Vx, Vy), so Vy only selects the handedness) mat=4 (row-major) lin=4 (= ax2d)
// circ=7 (ax22d radius) elips=8 (ax22d major minor) hypr=8 parab=7 (ax22d focal)
// trsf3d / trsf2d / gtrsf2d = variable (a recipe, see mkT3 / mkT2 / mkG2).
#include <gp.hxx>
#include <gp_XY.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Vec2d.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Mat2d.hxx>
#include <gp_Trsf2d.hxx>
#include <gp_GTrsf2d.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pnt.hxx>
#include <gp_Dir.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Trsf.hxx>
#include <gp_Quaternion.hxx>
#include <gp_TrsfForm.hxx>
#include <Standard_Failure.hxx>
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
    else if (w == "xy" || w == "pnt" || w == "vec" || w == "dir") n += 2;
    else if (w == "ax2d" || w == "lin" || w == "mat") n += 4;
    else if (w == "ax22d") n += 6;
    else if (w == "circ" || w == "parab") n += 7;
    else if (w == "elips" || w == "hypr") n += 8;
    else if (w == "trsf3d" || w == "trsf2d" || w == "gtrsf2d") return -1;
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
static void xy(const gp_XY& v) { h(v.X()); h(v.Y()); }
static void pnt(const gp_Pnt2d& v) { xy(v.XY()); }
static void vec(const gp_Vec2d& v) { xy(v.XY()); }
static void dir(const gp_Dir2d& v) { xy(v.XY()); }
static void ax2d(const gp_Ax2d& a) { pnt(a.Location()); dir(a.Direction()); }
static void ax22d(const gp_Ax22d& a) { pnt(a.Location()); dir(a.XDirection()); dir(a.YDirection()); }
static void mat(const gp_Mat2d& a) { for (int r = 1; r <= 2; ++r) for (int c = 1; c <= 2; ++c) h(a.Value(r, c)); }
// a Trsf2d is printed as its internal state: form, scale factor, the raw matrix and the translation
static void trsf(const gp_Trsf2d& t) { in((int)t.Form()); h(t.ScaleFactor()); mat(t.HVectorialPart()); xy(t.TranslationPart()); }
// a GTrsf2d: form, raw matrix, translation, then the six Value(row, col) (they expose the hidden scale)
static void gtrsf(const gp_GTrsf2d& t) {
  in((int)t.Form()); mat(t.VectorialPart()); xy(t.TranslationPart());
  for (int r = 1; r <= 2; ++r) for (int c = 1; c <= 3; ++c) h(t.Value(r, c));
}
static void lin(const gp_Lin2d& l) { ax2d(l.Position()); }
static void circ(const gp_Circ2d& c) { ax22d(c.Position()); h(c.Radius()); }
static void elips(const gp_Elips2d& e) { ax22d(e.Axis()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void hypr(const gp_Hypr2d& e) { ax22d(e.Axis()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void parab(const gp_Parab2d& e) { ax22d(e.Axis()); h(e.Focal()); }

// ---------------------------------------------------------------------------------
// random input objects. A "raw" direction is the un-normalized pair that is printed;
// the oracle and the test both build the gp_Dir2d from it.
struct Raw { double x, y; };
static void rw(const Raw& r) { h(r.x); h(r.y); }
static gp_Dir2d D(const Raw& r) { return gp_Dir2d(r.x, r.y); }
static gp_Vec2d V(const Raw& r) { return gp_Vec2d(r.x, r.y); }
static Raw rraw() { for (;;) { Raw r{rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y > 0.25) return r; } }
static Raw perp(const Raw& a) { return Raw{-a.y, a.x}; }
static double cross2(const Raw& a, const Raw& b) { return a.x * b.y - a.y * b.x; }
static Raw lin2(const Raw& a, double k, const Raw& b, double l) { return Raw{a.x * k + b.x * l, a.y * k + b.y * l}; }
static double norm(const Raw& a) { return std::sqrt(a.x * a.x + a.y * a.y); }
static gp_XY rxy() { double x = rnd(-6, 6), y = rnd(-6, 6); return gp_XY(x, y); }
static gp_Pnt2d rp() { return gp_Pnt2d(rxy()); }

struct RA { gp_Pnt2d p; Raw d; gp_Ax2d a; };
static RA mkA(const gp_Pnt2d& p, const Raw& d) { RA r; r.p = p; r.d = d; r.a = gp_Ax2d(p, D(d)); return r; }
static RA ra() { gp_Pnt2d p = rp(); Raw d = rraw(); return mkA(p, d); }
static void pa(const RA& r) { pnt(r.p); rw(r.d); }

// frames: mode 0 random Y, 1 direct (Y = k * perp X), 2 indirect (Y = -k * perp X), 3 Y = X, 4 Y = -X
struct RF { gp_Pnt2d p; Raw x, y; gp_Ax22d a; };
static RF mkF(const gp_Pnt2d& p, const Raw& x, const Raw& y) { RF r; r.p = p; r.x = x; r.y = y; r.a = gp_Ax22d(p, D(x), D(y)); return r; }
static RF rf(int mode) {
  gp_Pnt2d p = rp(); Raw x = rraw(); Raw y = rraw(); double k = rnd(0.3, 2.0);
  switch (mode % 5) {
    case 1: y = Raw{-x.y * k, x.x * k}; break;
    case 2: y = Raw{x.y * k, -x.x * k}; break;
    case 3: y = x; break;
    case 4: y = Raw{-x.x, -x.y}; break;
    default: break;
  }
  return mkF(p, x, y);
}
static void pf(const RF& r) { pnt(r.p); rw(r.x); rw(r.y); }

struct Mt { double v[4]; gp_Mat2d m; };
static Mt mkMt(const double* v) { Mt t; for (int k = 0; k < 4; ++k) t.v[k] = v[k]; t.m = gp_Mat2d(gp_XY(v[0], v[2]), gp_XY(v[1], v[3])); return t; }
static void pm(const Mt& t) { for (int k = 0; k < 4; ++k) h(t.v[k]); }
static Mt rmat() { for (;;) { double v[4]; for (int k = 0; k < 4; ++k) v[k] = rnd(-3, 3); Mt t = mkMt(v); if (std::abs(t.m.Determinant()) > 0.3) return t; } }
static double quarter() { return std::round(rnd(-4, 4) * 4) / 4; }
// rank-1 matrix with exactly representable entries: its determinant is exactly 0
static Mt rmat_singular() {
  double a = quarter(), b = quarter(); double k = (double)rint(-2, 2);
  double v[4] = {a, b, k * a, k * b};
  return mkMt(v);
}

// ---- the 3D transformation recipe used by gp_Trsf2d(gp_Trsf) --------------------
// kind first, then the fields that kind uses:
//  0 rotation(ax1,angle)   1 rotation + translation part   2 scale(pnt,factor)
//  3 translation           4 mirror ax1   5 mirror pnt   6 mirror ax2(pnt,dir)
//  7 rotation(unit quat)   8 rotation(quat) + translation part + scale factor
// Kinds 2..8 do not use sin/cos.
struct Raw3 { double x, y, z; };
static Raw3 rraw3() { for (;;) { Raw3 r{rnd(-4, 4), rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y + r.z * r.z > 0.25) return r; } }
static gp_Dir D3(const Raw3& r) { return gp_Dir(r.x, r.y, r.z); }
static void pnt3(const gp_Pnt& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void xyz3(const gp_XYZ& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void rw3(const Raw3& r) { h(r.x); h(r.y); h(r.z); }
static gp_Pnt rp3() { return gp_Pnt(rnd(-6, 6), rnd(-6, 6), rnd(-6, 6)); }
struct T3 { int kind; gp_Pnt p; Raw3 d; double ang; gp_XYZ v; double sc; double q[4]; gp_Trsf t; };
static T3 mkT3(int kind) {
  T3 r; r.kind = kind; r.p = rp3(); r.d = rraw3(); r.ang = rnd(-3, 3); r.v = gp_XYZ(rnd(-6, 6), rnd(-6, 6), rnd(-6, 6)); r.sc = rnz(0.3, 2.5);
  gp_Quaternion q(rnd(-1, 1), rnd(-1, 1), rnd(-1, 1), rnd(-1, 1)); q.Normalize();
  r.q[0] = q.X(); r.q[1] = q.Y(); r.q[2] = q.Z(); r.q[3] = q.W();
  gp_Quaternion qq(r.q[0], r.q[1], r.q[2], r.q[3]);
  switch (kind) {
    case 0: r.t.SetRotation(gp_Ax1(r.p, D3(r.d)), r.ang); break;
    case 1: r.t.SetRotation(gp_Ax1(r.p, D3(r.d)), r.ang); r.t.SetTranslationPart(gp_Vec(r.v)); break;
    case 2: r.t.SetScale(r.p, r.sc); break;
    case 3: r.t.SetTranslation(gp_Vec(r.v)); break;
    case 4: r.t.SetMirror(gp_Ax1(r.p, D3(r.d))); break;
    case 5: r.t.SetMirror(r.p); break;
    case 6: r.t.SetMirror(gp_Ax2(r.p, D3(r.d))); break;
    case 7: r.t.SetRotation(qq); break;
    default: r.t.SetRotation(qq); r.t.SetTranslationPart(gp_Vec(r.v)); r.t.SetScaleFactor(r.sc); break;
  }
  return r;
}
static void pt3(const T3& r) {
  in(r.kind);
  switch (r.kind) {
    case 0: pnt3(r.p); rw3(r.d); h(r.ang); break;
    case 1: pnt3(r.p); rw3(r.d); h(r.ang); xyz3(r.v); break;
    case 2: pnt3(r.p); h(r.sc); break;
    case 3: xyz3(r.v); break;
    case 4: pnt3(r.p); rw3(r.d); break;
    case 5: pnt3(r.p); break;
    case 6: pnt3(r.p); rw3(r.d); break;
    case 7: for (int k = 0; k < 4; ++k) h(r.q[k]); break;
    default: for (int k = 0; k < 4; ++k) h(r.q[k]); xyz3(r.v); h(r.sc); break;
  }
}

// ---- the 2D transformation recipe ---------------------------------------------------
//  0 rotation(pnt,angle)  1 rotation + translation part   2 scale(pnt,factor)
//  3 translation(vec)     4 translation(pnt,pnt)           5 mirror pnt
//  6 mirror ax2d          7 transformation(ax2d)           8 transformation(ax2d,ax2d)
//  9 mirror ax2d + scale factor + translation part (compound)
// 10 SetValues(a11 a12 a13 a21 a22 a23)   11 gp_Trsf2d(gp_Trsf) with a 3D recipe   12 identity
// Kinds 0 and 1 use sin/cos, every other kind only + - * / sqrt.
struct T2 { int kind; gp_Pnt2d p, q; Raw d, e; double ang, sc; gp_XY v; double vals[6]; T3 t3; gp_Trsf2d t; };
static double rscale() {
  static int cnt = 0; ++cnt;
  if (cnt % 7 == 0) return 1.0;
  if (cnt % 7 == 3) return -1.0;
  return rnz(0.3, 2.5);
}
static T2 mkT2(int kind, int sub = 7) {
  T2 r; r.kind = kind; r.p = rp(); r.q = rp(); r.d = rraw(); r.e = rraw(); r.ang = rnd(-3, 3); r.sc = rscale(); r.v = rxy();
  for (;;) {
    for (int k = 0; k < 6; ++k) r.vals[k] = rnd(-3, 3);
    double det = r.vals[0] * r.vals[4] - r.vals[3] * r.vals[1];
    if (std::abs(det) > 0.3) break;
  }
  r.t3 = mkT3(sub);
  switch (kind) {
    case 0: r.t.SetRotation(r.p, r.ang); break;
    case 1: r.t.SetRotation(r.p, r.ang); r.t.SetTranslationPart(gp_Vec2d(r.v)); break;
    case 2: r.t.SetScale(r.p, r.sc); break;
    case 3: r.t.SetTranslation(gp_Vec2d(r.v)); break;
    case 4: r.t.SetTranslation(r.p, r.q); break;
    case 5: r.t.SetMirror(r.p); break;
    case 6: r.t.SetMirror(gp_Ax2d(r.p, D(r.d))); break;
    case 7: r.t.SetTransformation(gp_Ax2d(r.p, D(r.d))); break;
    case 8: r.t.SetTransformation(gp_Ax2d(r.p, D(r.d)), gp_Ax2d(r.q, D(r.e))); break;
    case 9: r.t.SetMirror(gp_Ax2d(r.p, D(r.d))); r.t.SetScaleFactor(r.sc); r.t.SetTranslationPart(gp_Vec2d(r.v)); break;
    case 10: r.t.SetValues(r.vals[0], r.vals[1], r.vals[2], r.vals[3], r.vals[4], r.vals[5]); break;
    case 11: r.t = gp_Trsf2d(r.t3.t); break;
    default: break;
  }
  return r;
}
static void pt2(const T2& r) {
  in(r.kind);
  switch (r.kind) {
    case 0: pnt(r.p); h(r.ang); break;
    case 1: pnt(r.p); h(r.ang); xy(r.v); break;
    case 2: pnt(r.p); h(r.sc); break;
    case 3: xy(r.v); break;
    case 4: pnt(r.p); pnt(r.q); break;
    case 5: pnt(r.p); break;
    case 6: pnt(r.p); rw(r.d); break;
    case 7: pnt(r.p); rw(r.d); break;
    case 8: pnt(r.p); rw(r.d); pnt(r.q); rw(r.e); break;
    case 9: pnt(r.p); rw(r.d); h(r.sc); xy(r.v); break;
    case 10: for (int k = 0; k < 6; ++k) h(r.vals[k]); break;
    case 11: pt3(r.t3); break;
    default: break;
  }
}
// kinds that do not use sin/cos: 2..12; `k` selects among them
static int knotrig(int k) { return 2 + k % 11; }
// alternate between the two rotation recipes (0, 1) and the other recipes
static int rotkind(int k) { return (k % 2 == 0) ? (k / 2) % 2 : knotrig(k); }
// the recipes without sin/cos used by the all-pairs composition loops; 11 variants of the shape
struct KS { int kind, sub; };
static const KS PAIRKINDS[] = {{12, 7}, {2, 7}, {3, 7}, {5, 7}, {6, 7}, {7, 7}, {9, 7}, {10, 7}, {11, 7}, {11, 4}, {11, 6}};
static const int NPAIR = 11;
static T2 mkPair(int i) { return mkT2(PAIRKINDS[i].kind, PAIRKINDS[i].sub); }

// general 2D transformation recipe: 0 = GTrsf2d(Trsf2d recipe), 1 = GTrsf2d(matrix, translation),
// 2 = default GTrsf2d + SetAffinity(ax2d, ratio)
struct G2 { int gk; T2 t; Mt m; gp_XY v; RA ax; double ratio; gp_GTrsf2d g; };
static G2 mkG2(int gk, int tkind, int sub = 7, bool singular_ok = false) {
  G2 r; r.gk = gk; r.t = mkT2(tkind, sub); r.m = singular_ok ? rmat_singular() : rmat(); r.v = rxy(); r.ax = ra(); r.ratio = rnz(0.2, 2.5);
  if (gk == 0) r.g = gp_GTrsf2d(r.t.t);
  else if (gk == 1) r.g = gp_GTrsf2d(r.m.m, r.v);
  else r.g.SetAffinity(r.ax.a, r.ratio);
  return r;
}
static void pg2(const G2& r) {
  in(r.gk);
  if (r.gk == 0) pt2(r.t); else if (r.gk == 1) { pm(r.m); xy(r.v); } else { pa(r.ax); h(r.ratio); }
}

// The singular checks of gp_Trsf2d::Invert and gp_Mat2d::Invert are compiled out of OCCT 8.0.1 as built:
// these predicates are the conditions of the (absent) checks.
static bool tinv_unc(const gp_Trsf2d& t) {
  gp_TrsfForm f = t.Form();
  if (f == gp_Identity || f == gp_Translation || f == gp_PntMirror) return false;
  return std::abs(t.ScaleFactor()) <= gp::Resolution();
}
static bool minv_unc(const gp_Mat2d& m) { return std::abs(m.Determinant()) <= gp::Resolution(); }

static int badidx(int hi) { static const int pick[] = {0, -1, 4, 5, -7, 100}; int v = pick[rint(0, 5)]; if (v >= 1 && v <= hi) v = hi + 2; return v; }
static void badrc(int rmax, int cmax, int& r, int& c) {
  int w = rint(0, 2);
  r = (w == 1) ? rint(1, rmax) : badidx(rmax);
  c = (w == 0) ? rint(1, cmax) : badidx(cmax);
}

// curve generators; the n-th iteration forces the degenerate parameters
struct RC { RF f; double r; gp_Circ2d c; };
static RC rcirc(int n) { RC k; k.f = rf(n); k.r = (n == 3) ? 0.0 : rnd(0.1, 5); k.c = gp_Circ2d(k.f.a, k.r); return k; }
static void pcirc(const RC& k) { pf(k.f); h(k.r); }
struct RE { RF f; double a, b; gp_Elips2d c; };
static RE relips(int n) {
  RE k; k.f = rf(n); k.a = rnd(0.5, 5); k.b = k.a * rnd(0.05, 1);
  if (n == 3) k.b = k.a;
  if (n == 4) k.b = 0;
  if (n == 5) { k.a = 0; k.b = 0; }
  k.c = gp_Elips2d(k.f.a, k.a, k.b); return k;
}
static void pelips(const RE& k) { pf(k.f); h(k.a); h(k.b); }
struct RH { RF f; double a, b; gp_Hypr2d c; };
static RH rhypr(int n) {
  RH k; k.f = rf(n); k.a = rnd(0.2, 4); k.b = rnd(0, 4);
  if (n == 4) k.a = 0;
  if (n == 5) k.b = 0;
  if (n == 6) { k.a = 0; k.b = 0; }
  k.c = gp_Hypr2d(k.f.a, k.a, k.b); return k;
}
static void phypr(const RH& k) { pf(k.f); h(k.a); h(k.b); }
struct RP { RF f; double fo; gp_Parab2d c; };
static RP rparab(int n) { RP k; k.f = rf(n); k.fo = (n == 3) ? 0.0 : rnd(0, 4); k.c = gp_Parab2d(k.f.a, k.fo); return k; }
static void pparab(const RP& k) { pf(k.f); h(k.fo); }

// motion families on a curve-like object (exact part): mirror point / axis, scale, transform, translate.
#define MOTION_EXACT(NAME, SCHEMA, PRINTIN, OBJ, PR)                                                              \
  { gp_Pnt2d cen = rp(); RA mx = ra(); T2 tr = mkT2(knotrig(n)); gp_Pnt2d sp = rp(); double ss = rnz(0.3, 2.5);   \
    gp_XY dv = rxy(); gp_Pnt2d p1 = rp(), p2 = rp();                                                                \
    begin(NAME "_motion", SCHEMA " pnt ax2d pnt f trsf2d xy pnt pnt"); PRINTIN; pnt(cen); pa(mx); pnt(sp); h(ss);  \
    pt2(tr); xy(dv); pnt(p1); pnt(p2); bar();                                                                       \
    PR(OBJ.Mirrored(cen)); { auto t = OBJ; t.Mirror(cen); PR(t); }                                                  \
    PR(OBJ.Mirrored(mx.a)); { auto t = OBJ; t.Mirror(mx.a); PR(t); }                                                \
    PR(OBJ.Scaled(sp, ss)); { auto t = OBJ; t.Scale(sp, ss); PR(t); }                                               \
    PR(OBJ.Transformed(tr.t)); { auto t = OBJ; t.Transform(tr.t); PR(t); }                                          \
    PR(OBJ.Translated(gp_Vec2d(dv))); { auto t = OBJ; t.Translate(gp_Vec2d(dv)); PR(t); }                           \
    PR(OBJ.Translated(p1, p2)); { auto t = OBJ; t.Translate(p1, p2); PR(t); }                                       \
    finish(); }
#define MOTION_ROTATE(NAME, SCHEMA, PRINTIN, OBJ, PR)                                                             \
  { gp_Pnt2d rc = rp(); double ra_ = rnd(-3.2, 3.2); T2 tr = mkT2(rotkind(n));            \
    begin(NAME "_rotate", SCHEMA " pnt f trsf2d"); PRINTIN; pnt(rc); h(ra_); pt2(tr); bar();                        \
    PR(OBJ.Rotated(rc, ra_)); { auto t = OBJ; t.Rotate(rc, ra_); PR(t); }                                          \
    PR(OBJ.Transformed(tr.t)); { auto t = OBJ; t.Transform(tr.t); PR(t); }                                          \
    finish(); }

int main() {
  const int N = 18;
  for (int n = 0; n < N; ++n) {
    double x = rnd(-8, 8), y = rnd(-8, 8);
    double u = rnd(-8, 8), v = rnd(-8, 8), s = rnd(-3, 3);
    if (n == 0) { x = y = 0; u = v = 0; s = 0; }
    if (n == 1) { x = 1; y = 0; u = 1; v = 1e-14; s = -1; }
    if (n == 2) { x = 1e150; y = -1e150; u = 1e-150; v = 0; s = 1e-150; }
    if (n == 3) { s = 1e-15; }
    if (n == 4) { s = 3.141592653589783; }
    gp_XY a(x, y), c(u, v);
    gp_Pnt2d p(x, y), q(u, v);
    gp_Vec2d vv(x, y), ww(u, v);

    // relation partners: from n>=5 the second vector/direction is related to the first
    // (mode 1..8: parallel, opposite, normal, nearly equal, equal, negated, perpendicular, other perpendicular).
    int mode = n < 5 ? 0 : 1 + (n - 5) % 8;
    double rk = rnd(0.3, 2.0);
    Raw rz{rnd(-1, 1), rnd(-1, 1)};
    auto rel = [&](const Raw& base_a, const Raw& base_b) -> Raw {
      double m = norm(base_a), e = 1e-3 * m;
      switch (mode) {
        case 1: return lin2(base_a, rk, rz, e);
        case 2: return lin2(base_a, -rk, rz, e);
        case 3: { Raw pp = perp(base_a); return lin2(pp, 1.0, rz, e); }
        case 4: return lin2(base_a, 1.0, rz, e);
        case 5: return base_a;
        case 6: return Raw{-base_a.x, -base_a.y};
        case 7: return perp(base_a);
        case 8: return Raw{base_a.y, -base_a.x};
        default: return base_b;
      }
    };
    double atol = (n % 3 == 2) ? 1e-5 : rnd(0.05, 0.5);
    Raw vaR{x + 2, y + 3}, vbaseR{u + 3, v + 2};
    Raw vbR = rel(vaR, vbaseR);
    gp_Vec2d va = V(vaR), vb = V(vbR);

    // ---- gp_XY -------------------------------------------------------------
    begin("xy_constructors", "f f"); h(x); h(y); bar(); xy(gp_XY()); xy(gp_XY(x, y)); finish();
    begin("xy_add", "xy xy"); xy(a); xy(c); bar(); xy(a.Added(c)); { gp_XY t = a; t.Add(c); xy(t); } finish();
    begin("xy_subtract", "xy xy"); xy(a); xy(c); bar(); xy(a.Subtracted(c)); { gp_XY t = a; t.Subtract(c); xy(t); } finish();
    begin("xy_cross", "xy xy"); xy(a); xy(c); bar(); h(a.Crossed(c)); h(a.CrossMagnitude(c)); h(a.CrossSquareMagnitude(c)); finish();
    begin("xy_dot", "xy xy"); xy(a); xy(c); bar(); h(a.Dot(c)); finish();
    begin("xy_modulus", "xy"); xy(a); bar(); h(a.Modulus()); h(a.SquareModulus()); finish();
    begin("xy_multiply", "xy xy f"); xy(a); xy(c); h(s); bar();
    xy(a.Multiplied(s)); xy(a.Multiplied(c)); { gp_XY t = a; t.Multiply(s); xy(t); t = a; t.Multiply(c); xy(t); } finish();
    { Mt xm = (n == 6 || n == 11) ? rmat_singular() : rmat();
      begin("xy_multiply_matrix", "xy mat"); xy(a); pm(xm); bar();
      xy(a.Multiplied(xm.m)); gp_XY t = a; t.Multiply(xm.m); xy(t); finish(); }
    begin("xy_divide", "xy f"); xy(a); h(s); bar();
    // OCCT 8.0.1 as built does not check the divisor: 0 gives NaN/inf (unchecked)
    if (std::abs(s) <= gp::Resolution()) UNCHECKED(); else { xy(a.Divided(s)); gp_XY t = a; t.Divide(s); xy(t); }
    finish();
    begin("xy_reverse", "xy"); xy(a); bar(); xy(a.Reversed()); { gp_XY t = a; t.Reverse(); xy(t); } finish();
    { bool near = n >= 5 && (n % 2 == 1);
      gp_XY bq = near ? gp_XY(x + rnd(-0.02, 0.02), y + rnd(-0.02, 0.02)) : c;
      double tol = near ? rnd(0, 0.03) : std::abs(s);
      begin("xy_equal", "xy xy f"); xy(a); xy(bq); h(tol); bar(); bl(a.IsEqual(bq, tol)); finish(); }
    { int idx = rint(1, 2);
      begin("xy_access", "xy i"); xy(a); in(idx); bar();
      h(a.Coord(1)); h(a.Coord(2)); h(a.Coord(idx)); h(a.X()); h(a.Y()); double cx, cy; a.Coord(cx, cy); h(cx); h(cy); finish(); }
    { int idx = badidx(2);
      begin("xy_bad_index", "xy i"); xy(a); in(idx); bar(); GUARD(h(a.Coord(idx));) finish(); }
    { int idx = badidx(2); double val = rnd(-3, 3);
      begin("xy_bad_set_index", "xy i f"); xy(a); in(idx); h(val); bar(); GUARD(gp_XY t = a; t.SetCoord(idx, val); xy(t);) finish(); }
    { int idx = badidx(2);
      begin("xy_bad_change_index", "xy i"); xy(a); in(idx); bar(); GUARD(gp_XY t = a; h(t.ChangeCoord(idx));) finish(); }
    { int i1 = rint(1, 2), i2 = rint(1, 2);
      begin("xy_setters", "xy xy f i i"); xy(a); xy(c); h(s); in(i1); in(i2); bar();
      gp_XY t = a; t.SetCoord(i1, u); xy(t); t.SetX(v); xy(t); t.SetY(s); xy(t); t.SetCoord(x, u); xy(t); t.ChangeCoord(i2) = y; xy(t); finish(); }
    { double r2 = rsc(); gp_XY e1 = rxy(), e2 = rxy();
      begin("xy_linear_forms", "xy xy f f xy xy"); xy(a); xy(c); h(s); h(r2); xy(e1); xy(e2); bar();
      gp_XY lf;
      lf.SetLinearForm(s, a, r2, c); xy(lf);
      lf.SetLinearForm(s, a, r2, c, e1); xy(lf);
      lf.SetLinearForm(s, a, e2); xy(lf);
      lf.SetLinearForm(a, c); xy(lf);
      finish(); }
    begin("xy_normalize", "xy"); xy(a); bar(); GUARD(xy(a.Normalized()); gp_XY t = a; t.Normalize(); xy(t);) finish();

    // ---- gp_Pnt2d ----------------------------------------------------------
    begin("pnt2d_constructors", "xy f f"); xy(a); h(u); h(v); bar();
    pnt(gp_Pnt2d()); pnt(gp_Pnt2d(a)); pnt(gp_Pnt2d(u, v)); finish();
    begin("pnt2d_access", "pnt"); pnt(p); bar(); h(p.X()); h(p.Y()); h(p.Coord(1)); h(p.Coord(2)); xy(p.XY()); double px, py; p.Coord(px, py); h(px); h(py); finish();
    { int idx = badidx(2);
      begin("pnt2d_bad_coord", "pnt i"); pnt(p); in(idx); bar(); GUARD(h(p.Coord(idx));) finish(); }
    { int idx = badidx(2); double val = rsc();
      begin("pnt2d_bad_set_index", "pnt i f"); pnt(p); in(idx); h(val); bar(); GUARD(gp_Pnt2d t = p; t.SetCoord(idx, val); pnt(t);) finish(); }
    { int i1 = rint(1, 2);
      begin("pnt2d_setters", "pnt xy f i"); pnt(p); xy(c); h(s); in(i1); bar();
      gp_Pnt2d t = p; t.SetCoord(i1, u); pnt(t); t.SetCoord(v, s); pnt(t); t.SetX(x); pnt(t); t.SetY(y); pnt(t); t.SetXY(c); pnt(t); t.ChangeCoord().SetCoord(s, x); pnt(t); finish(); }
    { bool near = n >= 5 && (n % 2 == 1);
      gp_Pnt2d bq = near ? gp_Pnt2d(x + rnd(-0.02, 0.02), y + rnd(-0.02, 0.02)) : q;
      double tol = near ? rnd(0, 0.03) : std::abs(s);
      begin("pnt2d_equal", "pnt pnt f"); pnt(p); pnt(bq); h(tol); bar(); bl(p.IsEqual(bq, tol)); finish(); }
    begin("pnt2d_distance", "pnt pnt"); pnt(p); pnt(q); bar(); h(p.Distance(q)); h(p.SquareDistance(q)); finish();
    begin("pnt2d_mirror_point", "pnt pnt"); pnt(p); pnt(q); bar(); pnt(p.Mirrored(q)); { gp_Pnt2d t = p; t.Mirror(q); pnt(t); } finish();
    { RA ax = ra();
      begin("pnt2d_mirror_axis", "pnt ax2d"); pnt(p); pa(ax); bar(); pnt(p.Mirrored(ax.a)); gp_Pnt2d t = p; t.Mirror(ax.a); pnt(t); finish(); }
    begin("pnt2d_rotate", "pnt pnt f"); pnt(p); pnt(q); h(s); bar(); pnt(p.Rotated(q, s)); { gp_Pnt2d t = p; t.Rotate(q, s); pnt(t); } finish();
    begin("pnt2d_scale", "pnt pnt f"); pnt(p); pnt(q); h(s); bar(); pnt(p.Scaled(q, s)); { gp_Pnt2d t = p; t.Scale(q, s); pnt(t); } finish();
    begin("pnt2d_translate_vec", "pnt vec"); pnt(p); vec(gp_Vec2d(c)); bar(); pnt(p.Translated(gp_Vec2d(c))); { gp_Pnt2d t = p; t.Translate(gp_Vec2d(c)); pnt(t); } finish();
    { gp_Pnt2d p1 = rp(), p2 = rp();
      begin("pnt2d_translate_points", "pnt pnt pnt"); pnt(p); pnt(p1); pnt(p2); bar(); pnt(p.Translated(p1, p2)); gp_Pnt2d t = p; t.Translate(p1, p2); pnt(t); finish(); }
    { T2 tr = mkT2(knotrig(n));
      begin("pnt2d_transform", "pnt trsf2d"); pnt(p); pt2(tr); bar(); pnt(p.Transformed(tr.t)); gp_Pnt2d t = p; t.Transform(tr.t); pnt(t); finish(); }
    { T2 tr = mkT2(rotkind(n));
      begin("pnt2d_transform_rotation", "pnt trsf2d"); pnt(p); pt2(tr); bar(); pnt(p.Transformed(tr.t)); gp_Pnt2d t = p; t.Transform(tr.t); pnt(t); finish(); }

    // ---- gp_Vec2d ----------------------------------------------------------
    { Raw d1 = rraw();
      begin("vec2d_constructors", "xy xy dir"); xy(a); xy(c); rw(d1); bar();
      vec(gp_Vec2d()); vec(gp_Vec2d(a)); vec(gp_Vec2d(x, y)); vec(gp_Vec2d(D(d1))); vec(gp_Vec2d(gp_Pnt2d(a), gp_Pnt2d(c))); finish(); }
    { int i1 = rint(1, 2);
      begin("vec2d_access", "vec vec f i"); vec(vv); vec(ww); h(s); in(i1); bar();
      h(vv.X()); h(vv.Y()); h(vv.Coord(1)); h(vv.Coord(2)); xy(vv.XY()); double cx, cy; vv.Coord(cx, cy); h(cx); h(cy);
      gp_Vec2d t = vv; t.SetCoord(i1, u); vec(t); t.SetCoord(v, s); vec(t); t.SetX(x); vec(t); t.SetY(y); vec(t); t.SetXY(ww.XY()); vec(t); finish(); }
    { int idx = badidx(2);
      begin("vec2d_bad_coord", "vec i"); vec(vv); in(idx); bar(); GUARD(h(vv.Coord(idx));) finish(); }
    { int idx = badidx(2); double val = rsc();
      begin("vec2d_bad_set_index", "vec i f"); vec(vv); in(idx); h(val); bar(); GUARD(gp_Vec2d t = vv; t.SetCoord(idx, val); vec(t);) finish(); }
    gp_Vec2d vzero;
    { int which = rint(0, 1);
      begin("vec2d_zero_angle", "vec i"); vec(vv); in(which); bar(); GUARD(h(which == 0 ? vzero.Angle(vv) : vv.Angle(vzero));) finish(); }
    { double tol = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec2d_zero_is_normal", "vec f i"); vec(vv); h(tol); in(which); bar(); GUARD(bl(which == 0 ? vzero.IsNormal(vv, tol) : vv.IsNormal(vzero, tol));) finish(); }
    { double tol = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec2d_zero_is_opposite", "vec f i"); vec(vv); h(tol); in(which); bar(); GUARD(bl(which == 0 ? vzero.IsOpposite(vv, tol) : vv.IsOpposite(vzero, tol));) finish(); }
    { double tol = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec2d_zero_is_parallel", "vec f i"); vec(vv); h(tol); in(which); bar(); GUARD(bl(which == 0 ? vzero.IsParallel(vv, tol) : vv.IsParallel(vzero, tol));) finish(); }
    { // a zero vector against a non-negative linear tolerance never raises; a negative one reaches Angle
      double lt = (n % 4 == 3) ? -0.05 : rnd(0, 0.5); double at2 = rnd(0.01, 0.5); int which = rint(0, 1);
      begin("vec2d_zero_is_equal", "vec f f i"); vec(vv); h(lt); h(at2); in(which); bar();
      GUARD(bl(which == 0 ? vzero.IsEqual(vv, lt, at2) : vv.IsEqual(vzero, lt, at2));) finish(); }
    begin("vec2d_cross", "vec vec"); vec(vv); vec(ww); bar(); h(vv.Crossed(ww)); h(vv.Dot(ww)); h(vv.CrossMagnitude(ww)); h(vv.CrossSquareMagnitude(ww)); finish();
    begin("vec2d_arithmetic", "vec vec f"); vec(vv); vec(ww); h(s); bar(); vec(vv.Added(ww)); vec(vv.Subtracted(ww)); vec(vv.Multiplied(s)); vec(vv.Reversed()); vec(vv.GetNormal()); finish();
    begin("vec2d_normalize", "vec"); vec(vv); bar(); GUARD(vec(vv.Normalized()); gp_Vec2d t = vv; t.Normalize(); vec(t);) finish();
    { double lt = rnd(0.01, 1.0);
      begin("vec2d_relations", "vec vec f f"); vec(va); vec(vb); h(lt); h(atol); bar();
      bl(va.IsEqual(vb, lt, atol)); bl(va.IsNormal(vb, atol)); bl(va.IsOpposite(vb, atol)); bl(va.IsParallel(vb, atol)); finish(); }
    begin("vec2d_queries", "vec vec"); vec(va); vec(vb); bar();
    h(va.Angle(vb)); h(va.Magnitude()); h(va.SquareMagnitude()); h(va.CrossMagnitude(vb)); h(va.CrossSquareMagnitude(vb)); h(va.Dot(vb)); h(va.Crossed(vb)); finish();
    { double r1 = rsc(); gp_Vec2d vr(rxy());
      begin("vec2d_mutators", "vec vec vec f f"); vec(va); vec(vb); vec(vr); h(s); h(r1); bar();
      double sd = (s == 0 ? 1 : s);
      vec(va.Added(vb)); vec(va.Subtracted(vb)); vec(va.Multiplied(s)); vec(va.Divided(sd)); vec(va.Reversed());
      gp_Vec2d vm = va; vm.Add(vb); vec(vm); vm = va; vm.Subtract(vb); vec(vm); vm = va; vm.Multiply(s); vec(vm); vm = va; vm.Divide(sd); vec(vm);
      vm = va; vm.Reverse(); vec(vm);
      vm.SetLinearForm(s, va, r1, vb); vec(vm); vm.SetLinearForm(s, va, r1, vb, vr); vec(vm);
      vm.SetLinearForm(s, va, vr); vec(vm); vm.SetLinearForm(va, vb); vec(vm);
      finish(); }
    { RA ax = ra(); gp_Vec2d vmz = (n % 5 == 3) ? vzero : vb;
      begin("vec2d_mirrors", "vec vec ax2d"); vec(va); vec(vmz); pa(ax); bar();
      vec(va.Mirrored(vmz)); gp_Vec2d vm = va; vm.Mirror(vmz); vec(vm);
      vec(va.Mirrored(ax.a)); vm = va; vm.Mirror(ax.a); vec(vm); finish(); }
    { T2 tr = mkT2(knotrig(n));
      begin("vec2d_scale_transform", "vec f trsf2d"); vec(va); h(s); pt2(tr); bar();
      vec(va.Scaled(s)); gp_Vec2d vm = va; vm.Scale(s); vec(vm); vec(va.Transformed(tr.t)); vm = va; vm.Transform(tr.t); vec(vm); finish(); }
    { T2 tr = mkT2(rotkind(n));
      begin("vec2d_rotate", "vec f trsf2d"); vec(va); h(s); pt2(tr); bar();
      vec(va.Rotated(s)); gp_Vec2d vm = va; vm.Rotate(s); vec(vm); vec(va.Transformed(tr.t)); vm = va; vm.Transform(tr.t); vec(vm); finish(); }

    // ---- gp_Dir2d ----------------------------------------------------------
    Raw aR{x, y}, cR{u, v};
    Raw b2R = (n < 5) ? cR : rel(aR, cR);
    begin("dir2d_construct", "xy"); xy(a); bar(); GUARD(dir(gp_Dir2d(x, y)); dir(gp_Dir2d(gp_Vec2d(a))); dir(gp_Dir2d(a));) finish();
    begin("dir2d_angle", "xy xy"); xy(a); rw(b2R); bar(); GUARD(gp_Dir2d d(x, y), e(b2R.x, b2R.y); h(d.Angle(e)); h(d.Dot(e)); h(d.Crossed(e));) finish();
    { Raw d1 = rraw(); int i1 = rint(1, 2); double r1 = rsc(), r2 = rsc(), r3 = rsc(); gp_XY e1 = rxy(), e2 = rxy();
      begin("dir2d_setters", "dir i f f f xy xy"); rw(d1); in(i1); h(r1); h(r2); h(r3); xy(e1); xy(e2); bar();
      GUARD(gp_Dir2d dd = D(d1); h(dd.X()); h(dd.Y()); h(dd.Coord(1)); h(dd.Coord(2)); xy(dd.XY()); double cx, cy; dd.Coord(cx, cy); h(cx); h(cy);
            gp_Dir2d ds = dd; ds.SetCoord(i1, r1); dir(ds); ds.SetX(r2); dir(ds); ds.SetY(r3); dir(ds); ds.SetCoord(r1, r3); dir(ds); ds.SetXY(e1); dir(ds);
            dir(gp_Dir2d()); dir(gp_Dir2d(gp_Vec2d(e2))); dir(gp_Dir2d(e2));)
      finish(); }
    { Raw d1 = rraw(); int idx = badidx(2);
      begin("dir2d_bad_coord", "dir i"); rw(d1); in(idx); bar(); GUARD(h(D(d1).Coord(idx));) finish(); }
    { Raw d1 = rraw(); int idx = badidx(2); double val = rsc();
      begin("dir2d_bad_set_index", "dir i f"); rw(d1); in(idx); h(val); bar(); GUARD(gp_Dir2d t = D(d1); t.SetCoord(idx, val); dir(t);) finish(); }
    { // the Direction 2d enumeration constructors (the signed zeros of the enum constructors count)
      begin("axis2d_enum_constructors", "pnt"); pnt(p); bar();
      for (auto d : {gp_Dir2d::D::X, gp_Dir2d::D::Y, gp_Dir2d::D::NX, gp_Dir2d::D::NY}) {
        dir(gp_Dir2d(d)); ax2d(gp_Ax2d(d)); ax2d(gp_Ax2d(p, d)); ax22d(gp_Ax22d(gp_Ax2d(p, d), false));
      }
      ax2d(gp_Ax2d()); ax22d(gp_Ax22d()); finish(); }
    { Raw d1 = rraw(); Raw d2 = rel(d1, rraw()); double tol = atol;
      begin("dir2d_queries", "dir dir f"); rw(d1); rw(d2); h(tol); bar();
      gp_Dir2d dd = D(d1), de = D(d2); bl(dd.IsEqual(de, tol)); bl(dd.IsNormal(de, tol)); bl(dd.IsOpposite(de, tol)); bl(dd.IsParallel(de, tol)); finish(); }
    { Raw d1 = rraw(), d2 = rraw();
      begin("dir2d_ops", "dir dir"); rw(d1); rw(d2); bar();
      gp_Dir2d dd = D(d1), de = D(d2); h(dd.Crossed(de)); h(dd.Dot(de)); dir(dd.Reversed()); gp_Dir2d dm = dd; dm.Reverse(); dir(dm); finish(); }
    { Raw d1 = rraw(), d2 = rraw(); RA ax = ra();
      begin("dir2d_mirrors", "dir dir ax2d"); rw(d1); rw(d2); pa(ax); bar();
      gp_Dir2d dd = D(d1), de = D(d2); dir(dd.Mirrored(de)); gp_Dir2d dm = dd; dm.Mirror(de); dir(dm);
      dir(dd.Mirrored(ax.a)); dm = dd; dm.Mirror(ax.a); dir(dm); finish(); }
    { Raw d1 = rraw(); T2 tr = mkT2(knotrig(n));
      begin("dir2d_transform", "dir trsf2d"); rw(d1); pt2(tr); bar();
      gp_Dir2d dd = D(d1); dir(dd.Transformed(tr.t)); gp_Dir2d dm = dd; dm.Transform(tr.t); dir(dm); finish(); }
    { Raw d1 = rraw(); T2 tr = mkT2(rotkind(n));
      begin("dir2d_rotate", "dir f trsf2d"); rw(d1); h(s); pt2(tr); bar();
      gp_Dir2d dd = D(d1); dir(dd.Rotated(s)); gp_Dir2d dm = dd; dm.Rotate(s); dir(dm); dir(dd.Transformed(tr.t)); dm = dd; dm.Transform(tr.t); dir(dm); finish(); }

    // ---- gp_Ax2d -----------------------------------------------------------
    RA aa = mkA(p, rraw());
    { gp_Pnt2d nl = rp(); Raw nd = rraw();
      begin("ax2d_access", "ax2d pnt dir"); pa(aa); pnt(nl); rw(nd); bar();
      ax2d(aa.a); ax2d(gp_Ax2d()); ax2d(gp_Ax2d(nl, D(nd))); gp_Ax2d t = aa.a; t.SetLocation(nl); ax2d(t); t.SetDirection(D(nd)); ax2d(t); finish(); }
    { Raw d2 = rel(aa.d, rraw()); double tt = rnd(-2, 2); double lt = rnd(0.01, 0.5);
      gp_Pnt2d L = (mode == 1 || mode == 2 || mode == 5 || mode == 6) ? gp_Pnt2d(p.X() + aa.d.x * tt, p.Y() + aa.d.y * tt) : rp();
      RA ab = mkA(L, d2);
      begin("ax2d_relations", "ax2d ax2d f f"); pa(aa); pa(ab); h(atol); h(lt); bar();
      bl(aa.a.IsCoaxial(ab.a, atol, lt)); bl(aa.a.IsNormal(ab.a, atol)); bl(aa.a.IsOpposite(ab.a, atol)); bl(aa.a.IsParallel(ab.a, atol)); h(aa.a.Angle(ab.a));
      bl(aa.a.IsCoaxial(gp_Ax2d(aa.a), 1e-9, 1e-9)); bl(aa.a.IsOpposite(gp_Ax2d(aa.p, D(aa.d).Reversed()), 0.1)); bl(aa.a.IsParallel(gp_Ax2d(aa.a), 0.1)); finish(); }
    begin("ax2d_reverse", "ax2d"); pa(aa); bar(); ax2d(aa.a.Reversed()); { gp_Ax2d t = aa.a; t.Reverse(); ax2d(t); } finish();
    begin("ax2d_mirror_point", "ax2d pnt"); pa(aa); pnt(q); bar(); ax2d(aa.a.Mirrored(q)); { gp_Ax2d t = aa.a; t.Mirror(q); ax2d(t); } finish();
    { RA o = ra();
      begin("ax2d_mirror_axis", "ax2d ax2d"); pa(aa); pa(o); bar(); ax2d(aa.a.Mirrored(o.a)); gp_Ax2d t = aa.a; t.Mirror(o.a); ax2d(t); finish(); }
    begin("ax2d_scale", "ax2d pnt f"); pa(aa); pnt(q); h(s); bar(); ax2d(aa.a.Scaled(q, s)); { gp_Ax2d t = aa.a; t.Scale(q, s); ax2d(t); } finish();
    { T2 tr = mkT2(knotrig(n));
      begin("ax2d_transform", "ax2d trsf2d"); pa(aa); pt2(tr); bar(); ax2d(aa.a.Transformed(tr.t)); gp_Ax2d t = aa.a; t.Transform(tr.t); ax2d(t); finish(); }
    { T2 tr = mkT2(rotkind(n));
      begin("ax2d_rotate", "ax2d pnt f trsf2d"); pa(aa); pnt(q); h(s); pt2(tr); bar();
      ax2d(aa.a.Rotated(q, s)); gp_Ax2d t = aa.a; t.Rotate(q, s); ax2d(t); ax2d(aa.a.Transformed(tr.t)); t = aa.a; t.Transform(tr.t); ax2d(t); finish(); }
    { gp_Pnt2d p1 = rp(), p2 = rp();
      begin("ax2d_translate", "ax2d xy pnt pnt"); pa(aa); xy(c); pnt(p1); pnt(p2); bar();
      ax2d(aa.a.Translated(gp_Vec2d(c))); gp_Ax2d t = aa.a; t.Translate(gp_Vec2d(c)); ax2d(t);
      ax2d(aa.a.Translated(p1, p2)); t = aa.a; t.Translate(p1, p2); ax2d(t); finish(); }

    // ---- gp_Ax22d ----------------------------------------------------------
    { RF fr = rf(n); RA a1 = ra(); int sense = rint(0, 1);
      begin("ax22d_construct", "ax22d ax2d i"); pf(fr); pa(a1); in(sense); bar();
      ax22d(gp_Ax22d()); ax22d(fr.a);
      ax22d(gp_Ax22d(fr.p, D(fr.x), D(fr.y)));
      ax22d(gp_Ax22d(fr.p, D(fr.x), sense == 1)); ax22d(gp_Ax22d(fr.p, D(fr.y), sense == 0));
      ax22d(gp_Ax22d(a1.a, sense == 1)); ax22d(gp_Ax22d(a1.a)); finish(); }
    { RF fr = rf(n + 1); RF g2 = rf(n + 2); RA a1 = ra(); RA a2 = ra(); gp_Pnt2d nl = rp(); Raw d1 = rraw(), d2 = rraw();
      begin("ax22d_access", "ax22d ax22d ax2d ax2d pnt dir dir"); pf(fr); pf(g2); pa(a1); pa(a2); pnt(nl); rw(d1); rw(d2); bar();
      ax22d(fr.a); ax2d(fr.a.XAxis()); ax2d(fr.a.YAxis()); pnt(fr.a.Location()); dir(fr.a.XDirection()); dir(fr.a.YDirection());
      gp_Ax22d t = fr.a; t.SetAxis(g2.a); ax22d(t);
      t = fr.a; t.SetXAxis(a1.a); ax22d(t);
      t = fr.a; t.SetYAxis(a2.a); ax22d(t);
      t = fr.a; t.SetLocation(nl); ax22d(t);
      t = fr.a; t.SetXDirection(D(d1)); ax22d(t);
      t = fr.a; t.SetYDirection(D(d2)); ax22d(t);
      finish(); }
    { RF fr = rf(n + 3); gp_Pnt2d cen = rp(); RA mx = ra();
      begin("ax22d_mirror", "ax22d pnt ax2d"); pf(fr); pnt(cen); pa(mx); bar();
      ax22d(fr.a.Mirrored(cen)); { gp_Ax22d t = fr.a; t.Mirror(cen); ax22d(t); }
      ax22d(fr.a.Mirrored(mx.a)); { gp_Ax22d t = fr.a; t.Mirror(mx.a); ax22d(t); }
      finish(); }
    { RF fr = rf(n + 4); gp_Pnt2d sp = rp(); double ss = rnz(0.3, 2.5); gp_XY dv = rxy(); gp_Pnt2d p1 = rp(), p2 = rp(); T2 tr = mkT2(knotrig(n));
      begin("ax22d_scale_transform_translate", "ax22d pnt f trsf2d xy pnt pnt"); pf(fr); pnt(sp); h(ss); pt2(tr); xy(dv); pnt(p1); pnt(p2); bar();
      ax22d(fr.a.Scaled(sp, ss)); { gp_Ax22d t = fr.a; t.Scale(sp, ss); ax22d(t); }
      ax22d(fr.a.Transformed(tr.t)); { gp_Ax22d t = fr.a; t.Transform(tr.t); ax22d(t); }
      ax22d(fr.a.Translated(gp_Vec2d(dv))); { gp_Ax22d t = fr.a; t.Translate(gp_Vec2d(dv)); ax22d(t); }
      ax22d(fr.a.Translated(p1, p2)); { gp_Ax22d t = fr.a; t.Translate(p1, p2); ax22d(t); }
      finish(); }
    { RF fr = rf(n); gp_Pnt2d rc = rp(); double ra_ = rnd(-3.2, 3.2); T2 tr = mkT2(rotkind(n));
      begin("ax22d_rotate", "ax22d pnt f trsf2d"); pf(fr); pnt(rc); h(ra_); pt2(tr); bar();
      ax22d(fr.a.Rotated(rc, ra_)); { gp_Ax22d t = fr.a; t.Rotate(rc, ra_); ax22d(t); }
      ax22d(fr.a.Transformed(tr.t)); { gp_Ax22d t = fr.a; t.Transform(tr.t); ax22d(t); }
      finish(); }

    // ---- gp_Mat2d (iterations 0, 6 and 11 use a singular matrix; 12 a denormal-determinant one) ---------
    Mt m1 = (n == 0 || n == 6 || n == 11) ? rmat_singular() : rmat();
    if (n == 0) { double z4[4] = {0, 0, 0, 0}; m1 = mkMt(z4); }
    if (n == 12) { double t4[4] = {1e-160, 0, 0, 1e-160}; m1 = mkMt(t4); }
    if (n == 13) { double t4[4] = {1e-154, 1e-154, 0, 1e-154}; m1 = mkMt(t4); }
    { gp_XY e1 = rxy(), e2 = rxy();
      begin("mat2d_constructors", "mat xy xy"); pm(m1); xy(e1); xy(e2); bar();
      mat(gp_Mat2d()); mat(m1.m); mat(gp_Mat2d(e1, e2)); finish(); }
    { int ci = rint(1, 2), ri = rint(1, 2), vr = rint(1, 2), vc = rint(1, 2), cr = rint(1, 2), cc = rint(1, 2); double nv = rsc();
      begin("mat2d_access", "mat i i i i i i f"); pm(m1); in(ci); in(ri); in(vr); in(vc); in(cr); in(cc); h(nv); bar();
      xy(m1.m.Column(ci)); xy(m1.m.Row(ri)); xy(m1.m.Diagonal()); h(m1.m.Value(vr, vc)); h(m1.m.Determinant());
      gp_Mat2d t = m1.m; t.ChangeValue(cr, cc) = nv; mat(t); finish(); }
    { gp_XY e1 = rxy(), e2 = rxy(); int ci = rint(1, 2), ri = rint(1, 2), vr = rint(1, 2), vc = rint(1, 2); double d1 = rsc(), d2 = rsc(), sc = rsc(), nv = rsc();
      begin("mat2d_setters", "mat xy xy i i i i f f f f"); pm(m1); xy(e1); xy(e2); in(ci); in(ri); in(vr); in(vc); h(d1); h(d2); h(sc); h(nv); bar();
      gp_Mat2d t = m1.m; t.SetCol(ci, e1); mat(t);
      t = m1.m; t.SetCols(e1, e2); mat(t);
      t = m1.m; t.SetRow(ri, e1); mat(t);
      t = m1.m; t.SetRows(e1, e2); mat(t);
      t = m1.m; t.SetDiagonal(d1, d2); mat(t);
      t = m1.m; t.SetIdentity(); mat(t);
      t = m1.m; t.SetScale(sc); mat(t);
      t = m1.m; t.SetValue(vr, vc, nv); mat(t);
      finish(); }
    begin("mat2d_rotation", "mat f"); pm(m1); h(s); bar(); { gp_Mat2d t = m1.m; t.SetRotation(s); mat(t); } finish();
    begin("mat2d_singular", "mat"); pm(m1); bar(); bl(m1.m.IsSingular()); h(m1.m.Determinant()); finish();
    { int r, cc; badrc(2, 2, r, cc);
      begin("mat2d_bad_index", "mat i i"); pm(m1); in(r); in(cc); bar(); GUARD(h(m1.m.Value(r, cc));) finish(); }
    { int idx = badidx(2);
      begin("mat2d_bad_row", "mat i"); pm(m1); in(idx); bar(); GUARD((void)m1.m.Row(idx); UNCHECKED();) finish(); }
    { int idx = badidx(2);
      begin("mat2d_bad_column", "mat i"); pm(m1); in(idx); bar(); GUARD((void)m1.m.Column(idx); UNCHECKED();) finish(); }
    { int idx = badidx(2); gp_XY e = rxy();
      begin("mat2d_bad_set_row", "mat i xy"); pm(m1); in(idx); xy(e); bar(); GUARD(gp_Mat2d t = m1.m; t.SetRow(idx, e); UNCHECKED();) finish(); }
    { int idx = badidx(2); gp_XY e = rxy();
      begin("mat2d_bad_set_column", "mat i xy"); pm(m1); in(idx); xy(e); bar(); GUARD(gp_Mat2d t = m1.m; t.SetCol(idx, e); UNCHECKED();) finish(); }
    { int r, cc; badrc(2, 2, r, cc);
      begin("mat2d_bad_change_value", "mat i i"); pm(m1); in(r); in(cc); bar(); GUARD(gp_Mat2d t = m1.m; h(t.ChangeValue(r, cc));) finish(); }
    { int r, cc; badrc(2, 2, r, cc); double val = rsc();
      begin("mat2d_bad_set_value", "mat i i f"); pm(m1); in(r); in(cc); h(val); bar(); GUARD(gp_Mat2d t = m1.m; t.SetValue(r, cc, val); mat(t);) finish(); }
    { Mt m2 = rmat(); double sc = rnz(0.2, 3);
      begin("mat2d_arithmetic", "mat mat f"); pm(m1); pm(m2); h(sc); bar();
      mat(m1.m.Added(m2.m)); mat(m1.m.Subtracted(m2.m)); mat(m1.m.Multiplied(m2.m)); mat(m1.m.Multiplied(sc)); mat(m1.m.Divided(sc));
      gp_Mat2d t = m1.m; t.Add(m2.m); mat(t); t = m1.m; t.Subtract(m2.m); mat(t); t = m1.m; t.Multiply(m2.m); mat(t);
      t = m1.m; t.PreMultiply(m2.m); mat(t); t = m1.m; t.Multiply(sc); mat(t); t = m1.m; t.Divide(sc); mat(t); finish(); }
    { double dv = (n % 7 == 0) ? 0.0 : (n % 7 == 3) ? 1e-310 : rnz(0.2, 3);
      begin("mat2d_divide_zero", "mat f"); pm(m1); h(dv); bar();
      // OCCT does not check the divisor: a (nearly) zero divisor gives NaN/inf (unchecked)
      if (std::abs(dv) <= gp::Resolution()) UNCHECKED(); else { mat(m1.m.Divided(dv)); gp_Mat2d t = m1.m; t.Divide(dv); mat(t); }
      finish(); }
    { int pe = (n == 0) ? 0 : (n == 1) ? 1 : (n == 2) ? -1 : rint(-5, 6);
      begin("mat2d_inverse", "mat"); pm(m1); bar();
      // OCCT 8.0.1 as built has the singular-matrix check of gp_Mat2d::Invert compiled out
      if (std::abs(m1.m.Determinant()) <= gp::Resolution()) UNCHECKED();
      else { mat(m1.m.Inverted()); gp_Mat2d mm = m1.m; mm.Invert(); mat(mm); }
      finish();
      begin("mat2d_power", "mat i"); pm(m1); in(pe); bar();
      if (pe < 0 && std::abs(m1.m.Determinant()) <= gp::Resolution()) UNCHECKED();
      else { mat(m1.m.Powered(pe)); gp_Mat2d mm = m1.m; mm.Power(pe); mat(mm); mat(m1.m.Powered(0)); }
      finish();
      begin("mat2d_transpose", "mat"); pm(m1); bar(); mat(m1.m.Transposed()); { gp_Mat2d mm = m1.m; mm.Transpose(); mat(mm); } finish(); }

    // ---- gp_Trsf2d ---------------------------------------------------------
    begin("trsf2d_rotation", "pnt f"); pnt(p); h(s); bar(); trsf(gp_Trsf2d()); { gp_Trsf2d t; t.SetRotation(p, s); trsf(t); } finish();
    begin("trsf2d_mirror_point", "pnt"); pnt(p); bar(); { gp_Trsf2d t; t.SetMirror(p); trsf(t); } finish();
    { RA ax = ra();
      begin("trsf2d_mirror_axis", "ax2d"); pa(ax); bar(); { gp_Trsf2d t; t.SetMirror(ax.a); trsf(t); } finish(); }
    { double sv = n == 5 ? 0.0 : n == 6 ? 1.0 : n == 7 ? -1.0 : s;
      begin("trsf2d_scale", "pnt f"); pnt(p); h(sv); bar(); gp_Trsf2d t; t.SetScale(p, sv); trsf(t); finish(); }
    { gp_Pnt2d p1 = rp(), p2 = rp();
      begin("trsf2d_translation", "xy pnt pnt"); xy(c); pnt(p1); pnt(p2); bar();
      { gp_Trsf2d t; t.SetTranslation(gp_Vec2d(c)); trsf(t); } { gp_Trsf2d t; t.SetTranslation(p1, p2); trsf(t); } finish(); }
    { RA from = ra(), to = ra();
      begin("trsf2d_transformation", "ax2d ax2d"); pa(from); pa(to); bar();
      { gp_Trsf2d t; t.SetTransformation(to.a); trsf(t); } { gp_Trsf2d t; t.SetTransformation(from.a, to.a); trsf(t); } finish(); }
    { T2 tr = mkT2(knotrig(n)); gp_XY tp = (n % 6 == 0) ? gp_XY(0, 0) : (n % 6 == 3) ? gp_XY(1e-320, -1e-320) : rxy();
      begin("trsf2d_translation_part", "trsf2d xy"); pt2(tr); xy(tp); bar();
      gp_Trsf2d t = tr.t; t.SetTranslationPart(gp_Vec2d(tp)); trsf(t); finish(); }
    { T2 tr = mkT2(knotrig(n)); double sf = (n % 6 == 0) ? 1.0 : (n % 6 == 1) ? -1.0 : (n % 6 == 2) ? 0.0 : rnz(0.2, 2.5);
      begin("trsf2d_scale_factor", "trsf2d f"); pt2(tr); h(sf); bar();
      gp_Trsf2d t = tr.t; t.SetScaleFactor(sf); trsf(t); finish(); }
    { double vals[6]; for (int k = 0; k < 6; ++k) vals[k] = rnd(-3, 3);
      if (n == 0) { for (int k = 0; k < 6; ++k) vals[k] = 0; }
      if (n == 1) { vals[0] = 1; vals[1] = 0; vals[2] = 0.5; vals[3] = 0; vals[4] = 1; vals[5] = -0.5; }
      if (n == 2) { vals[0] = 2; vals[1] = 4; vals[3] = 1; vals[4] = 2; }
      if (n == 3) { vals[0] = 0; vals[1] = -1; vals[3] = 1; vals[4] = 0; }
      if (n == 4) { vals[0] = 1e-160; vals[1] = 0; vals[3] = 0; vals[4] = 1e-160; }
      if (n == 5) { vals[0] = 1; vals[1] = 0; vals[3] = 0; vals[4] = -1; }
      begin("trsf2d_set_values", "f f f f f f"); for (int k = 0; k < 6; ++k) h(vals[k]); bar();
      // OCCT 8.0.1 as built has the null-determinant check of SetValues compiled out (condition |det| < Resolution)
      gp_Mat2d dm(gp_XY(vals[0], vals[3]), gp_XY(vals[1], vals[4]));
      if (std::abs(dm.Determinant()) < gp::Resolution()) UNCHECKED();
      else { gp_Trsf2d t; t.SetValues(vals[0], vals[1], vals[2], vals[3], vals[4], vals[5]); trsf(t); }
      finish(); }
    { T2 tr = mkT2(knotrig(n));
      begin("trsf2d_queries", "trsf2d"); pt2(tr); bar();
      const gp_Trsf2d& t = tr.t; bl(t.IsNegative()); in((int)t.Form()); h(t.ScaleFactor()); xy(t.TranslationPart()); mat(t.VectorialPart()); mat(t.HVectorialPart()); finish(); }
    { T2 tr = mkT2(rotkind(n));
      begin("trsf2d_rotation_part", "trsf2d"); pt2(tr); bar(); h(tr.t.RotationPart()); finish(); }
    { T2 tr = mkT2(knotrig(n));
      begin("trsf2d_values", "trsf2d"); pt2(tr); bar(); for (int r = 1; r <= 2; ++r) for (int c2 = 1; c2 <= 3; ++c2) h(tr.t.Value(r, c2)); finish(); }
    { T2 tr = mkT2(knotrig(n)); int r, cc; badrc(2, 3, r, cc);
      begin("trsf2d_bad_value", "trsf2d i i"); pt2(tr); in(r); in(cc); bar(); GUARD(h(tr.t.Value(r, cc));) finish(); }
    { T2 tr = mkT2(knotrig(n));
      if (n % 9 == 4) { tr = mkT2(2); tr.sc = 0.0; tr.t.SetScale(tr.p, 0.0); }
      begin("trsf2d_inverse", "trsf2d"); pt2(tr); bar();
      // the zero scale check of gp_Trsf2d::Invert is compiled out
      if (tinv_unc(tr.t)) UNCHECKED(); else { trsf(tr.t.Inverted()); gp_Trsf2d t = tr.t; t.Invert(); trsf(t); }
      finish(); }
    { T2 tr = mkT2(knotrig(n)); gp_XY pt = rxy();
      begin("trsf2d_transforms", "trsf2d xy"); pt2(tr); xy(pt); bar();
      gp_XY t1 = pt; tr.t.Transforms(t1); xy(t1); double tx = pt.X(), ty = pt.Y(); tr.t.Transforms(tx, ty); h(tx); h(ty); finish(); }
    { T2 tr = mkT2(rotkind(n)); gp_XY pt = rxy();
      begin("trsf2d_transforms_rotation", "trsf2d xy"); pt2(tr); xy(pt); bar();
      gp_XY t1 = pt; tr.t.Transforms(t1); xy(t1); double tx = pt.X(), ty = pt.Y(); tr.t.Transforms(tx, ty); h(tx); h(ty); finish(); }
    { T3 t3 = mkT3(n % 9);
      begin("trsf2d_from_trsf", "trsf3d"); pt3(t3); bar(); trsf(gp_Trsf2d(t3.t)); finish(); }
    { T2 ta = mkT2(rotkind(n)), tb = mkT2(rotkind(n + 1));
      begin("trsf2d_composition_rotation", "trsf2d trsf2d"); pt2(ta); pt2(tb); bar();
      trsf(ta.t.Multiplied(tb.t)); { gp_Trsf2d t = ta.t; t.Multiply(tb.t); trsf(t); t = ta.t; t.PreMultiply(tb.t); trsf(t); }
      finish(); }
    { T2 tr = mkT2(knotrig(n)); int pe = (n == 0) ? 0 : (n == 1) ? 1 : (n == 2) ? -1 : rint(-5, 6);
      if (n == 5) { tr = mkT2(2); tr.sc = 0.0; tr.t.SetScale(tr.p, 0.0); }
      begin("trsf2d_power", "trsf2d i"); pt2(tr); in(pe); bar();
      if (pe < 0 && tinv_unc(tr.t)) UNCHECKED(); else { trsf(tr.t.Powered(pe)); gp_Trsf2d t = tr.t; t.Power(pe); trsf(t); }
      finish(); }
    { T2 tr = mkT2(rotkind(n)); int pe = rint(-4, 5);
      begin("trsf2d_rotation_ops", "trsf2d i"); pt2(tr); in(pe); bar();
      if (tinv_unc(tr.t)) UNCHECKED(); else { trsf(tr.t.Inverted()); gp_Trsf2d t = tr.t; t.Invert(); trsf(t); trsf(tr.t.Powered(pe)); t = tr.t; t.Power(pe); trsf(t); }
      finish(); }

    // ---- gp_GTrsf2d --------------------------------------------------------
    { T2 tr = mkT2(knotrig(n)); Mt m = rmat(); gp_XY e = rxy();
      begin("gtrsf2d_constructors", "trsf2d mat xy"); pt2(tr); pm(m); xy(e); bar();
      gtrsf(gp_GTrsf2d()); gtrsf(gp_GTrsf2d(tr.t)); gtrsf(gp_GTrsf2d(m.m, e)); finish(); }
    { G2 g = mkG2(n % 3, knotrig(n + 2), 2 + n % 7, false); Mt m = rmat(); gp_XY c1 = rxy(); T2 tr = mkT2(knotrig(n + 6)); RA ax = ra(); double ratio = rnz(0.2, 2.5);
      int ri = rint(1, 2), ci = rint(1, 3); double val = rsc();
      begin("gtrsf2d_setters", "gtrsf2d ax2d f i i f mat xy trsf2d"); pg2(g); pa(ax); h(ratio); in(ri); in(ci); h(val); pm(m); xy(c1); pt2(tr); bar();
      { gp_GTrsf2d t = g.g; t.SetAffinity(ax.a, ratio); gtrsf(t); }
      { gp_GTrsf2d t = g.g; t.SetValue(ri, ci, val); gtrsf(t); }
      { gp_GTrsf2d t = g.g; t.SetVectorialPart(m.m); gtrsf(t); }
      { gp_GTrsf2d t = g.g; t.SetTranslationPart(c1); gtrsf(t); }
      { gp_GTrsf2d t = g.g; t.SetTrsf2d(tr.t); gtrsf(t); }
      { gp_GTrsf2d t = g.g; t.SetTranslationPart(gp_XY(0, 0)); gtrsf(t); }
      finish(); }
    { G2 g = mkG2(n % 3, knotrig(n), 2 + n % 7, false);
      begin("gtrsf2d_queries", "gtrsf2d"); pg2(g); bar(); gtrsf(g.g); bl(g.g.IsNegative()); bl(g.g.IsSingular()); xy(g.g.TranslationPart()); mat(g.g.VectorialPart()); in((int)g.g.Form()); finish(); }
    { G2 g = mkG2(n % 3, knotrig(n + 1), 2 + n % 7, false); gp_XY pt = rxy();
      int r1 = rint(1, 2), c1 = rint(1, 3), r2 = rint(1, 2), c2 = rint(1, 3);
      begin("gtrsf2d_value_transform", "gtrsf2d i i i i xy"); pg2(g); in(r1); in(c1); in(r2); in(c2); xy(pt); bar();
      h(g.g.Value(r1, c1)); h(g.g.Value(r2, c2));
      gp_XY t1 = pt; g.g.Transforms(t1); xy(t1); xy(g.g.Transformed(pt)); double tx = pt.X(), ty = pt.Y(); g.g.Transforms(tx, ty); h(tx); h(ty); finish(); }
    { G2 g = mkG2(n % 3, knotrig(n + 3), 2 + n % 7, false); int r, cc; badrc(2, 3, r, cc);
      begin("gtrsf2d_bad_value", "gtrsf2d i i"); pg2(g); in(r); in(cc); bar(); GUARD(h(g.g.Value(r, cc));) finish(); }
    { G2 g = mkG2(n % 3, knotrig(n + 3), 2 + n % 7, false); int r, cc; badrc(2, 3, r, cc); double val = rsc();
      begin("gtrsf2d_bad_set_value", "gtrsf2d i i f"); pg2(g); in(r); in(cc); h(val); bar(); GUARD(gp_GTrsf2d t = g.g; t.SetValue(r, cc, val); gtrsf(t);) finish(); }
    { G2 g = mkG2(n % 4 == 3 ? 1 : n % 3, knotrig(n + 4), 2 + n % 7, n % 8 == 3);
      begin("gtrsf2d_invert", "gtrsf2d"); pg2(g); bar();
      // OCCT 8.0.1 as built has the singular checks of gp_Mat2d::Invert and gp_Trsf2d::Invert compiled out;
      // gp_GTrsf2d::Trsf2d() still raises for a non-orthogonal transformation
      GUARD(if (g.g.Form() == gp_Other ? minv_unc(g.g.VectorialPart()) : tinv_unc(g.g.Trsf2d())) UNCHECKED();
            else { gtrsf(g.g.Inverted()); gp_GTrsf2d t = g.g; t.Invert(); gtrsf(t); })
      finish(); }
    { G2 g = mkG2(n % 3, knotrig(n + 5), 2 + n % 7, false);
      begin("gtrsf2d_to_trsf2d", "gtrsf2d"); pg2(g); bar(); GUARD(trsf(g.g.Trsf2d());) finish(); }
    { G2 ga = mkG2(n % 3, knotrig(n), 2 + n % 7, false), gb = mkG2((n + 1) % 3, knotrig(n + 3), 2 + (n + 2) % 7, false);
      begin("gtrsf2d_composition", "gtrsf2d gtrsf2d"); pg2(ga); pg2(gb); bar();
      GUARD(gtrsf(ga.g.Multiplied(gb.g)); gp_GTrsf2d m = ga.g; m.Multiply(gb.g); gtrsf(m); m = ga.g; m.PreMultiply(gb.g); gtrsf(m);) finish(); }
    { G2 g = mkG2(n % 4 == 3 ? 1 : n % 3, knotrig(n + 2), 2 + n % 7, n % 8 == 7); int pe = (n == 0) ? 0 : (n == 1) ? 1 : (n == 2) ? -1 : rint(-4, 5);
      begin("gtrsf2d_power", "gtrsf2d i"); pg2(g); in(pe); bar();
      GUARD(if (pe < 0 && (g.g.Form() == gp_Other ? minv_unc(g.g.VectorialPart()) : tinv_unc(g.g.Trsf2d()))) UNCHECKED();
            else { gtrsf(g.g.Powered(pe)); gp_GTrsf2d t = g.g; t.Power(pe); gtrsf(t); })
      finish(); }
    { G2 g = mkG2(0, rotkind(n));
      G2 h2 = mkG2(n % 3, knotrig(n + 1), 2 + n % 7, false);
      begin("gtrsf2d_rotation", "gtrsf2d gtrsf2d"); pg2(g); pg2(h2); bar();
      GUARD(gtrsf(g.g.Multiplied(h2.g)); gp_GTrsf2d m = g.g; m.Multiply(h2.g); gtrsf(m); m = g.g; m.PreMultiply(h2.g); gtrsf(m);
            gp_GTrsf2d i = g.g; i.Invert(); gtrsf(i);) finish(); }

    // ---- gp_Lin2d ----------------------------------------------------------
    { RA a1 = ra(); gp_Pnt2d nl = rp(); Raw nd = rraw();
      begin("lin2d_constructors", "ax2d pnt dir"); pa(a1); pnt(nl); rw(nd); bar();
      lin(gp_Lin2d()); lin(gp_Lin2d(a1.a)); lin(gp_Lin2d(nl, D(nd))); finish(); }
    { double ca = rsc(), cb = rsc(), cc2 = rsc();
      if (n == 0) { ca = 0; cb = 0; } if (n == 1) { ca = 1e-160; cb = 0; } if (n == 2) { ca = 0; cb = 1; } if (n == 3) { ca = 1e-155; cb = 1e-155; }
      begin("lin2d_from_coefficients", "f f f"); h(ca); h(cb); h(cc2); bar();
      // the zero norm check of gp_Lin2d(A, B, C) is compiled out of OCCT 8.0.1 as built (condition A*A + B*B <= Resolution)
      if (ca * ca + cb * cb <= gp::Resolution()) UNCHECKED(); else lin(gp_Lin2d(ca, cb, cc2));
      finish(); }
    { RA a1 = ra(); gp_Pnt2d nl = rp(); RA a2 = ra(); Raw nd = rraw();
      begin("lin2d_access", "lin pnt ax2d dir"); pa(a1); pnt(nl); pa(a2); rw(nd); bar();
      gp_Lin2d l(a1.a); lin(l); pnt(l.Location()); dir(l.Direction()); ax2d(l.Position());
      double ca, cb, cc2; l.Coefficients(ca, cb, cc2); h(ca); h(cb); h(cc2);
      lin(l.Reversed()); { gp_Lin2d t = l; t.Reverse(); lin(t); }
      { gp_Lin2d t = l; t.SetLocation(nl); lin(t); } { gp_Lin2d t = l; t.SetDirection(D(nd)); lin(t); } { gp_Lin2d t = l; t.SetPosition(a2.a); lin(t); }
      finish(); }
    { RA a1 = ra(); gp_Pnt2d pt = rp(); double lt = rnd(0.01, 2);
      gp_Pnt2d onl = (n % 3 == 0) ? a1.p : pt;
      begin("lin2d_point_queries", "lin pnt pnt f"); pa(a1); pnt(pt); pnt(onl); h(lt); bar();
      gp_Lin2d l(a1.a); bl(l.Contains(pt, lt)); bl(l.Contains(onl, lt)); h(l.Distance(pt)); h(l.SquareDistance(pt)); h(l.Distance(onl)); lin(l.Normal(pt)); finish(); }
    { RA a1 = ra(); Raw d2 = rel(a1.d, rraw());
      // parallel lines are exactly parallel here (same or reversed direction) so the parallel test of
      // Distance/SquareDistance does not sit on a rounding boundary
      Raw d3 = (n % 3 == 0) ? a1.d : (n % 3 == 1) ? Raw{-a1.d.x, -a1.d.y} : d2;
      RA a2 = mkA(rp(), d3);
      begin("lin2d_line_queries", "lin lin"); pa(a1); pa(a2); bar();
      gp_Lin2d l1(a1.a), l2(a2.a); h(l1.Angle(l2)); h(l1.Distance(l2)); h(l1.SquareDistance(l2)); finish(); }
    { RA a1 = ra(); gp_Pnt2d cen = rp(); RA mx = ra(); gp_Pnt2d sp = rp(); double ss = rnz(0.3, 2.5); T2 tr = mkT2(knotrig(n)); gp_XY dv = rxy(); gp_Pnt2d p1 = rp(), p2 = rp();
      begin("lin2d_motion", "lin pnt ax2d pnt f trsf2d xy pnt pnt"); pa(a1); pnt(cen); pa(mx); pnt(sp); h(ss); pt2(tr); xy(dv); pnt(p1); pnt(p2); bar();
      gp_Lin2d l(a1.a);
      lin(l.Mirrored(cen)); { gp_Lin2d t = l; t.Mirror(cen); lin(t); } lin(l.Mirrored(mx.a)); { gp_Lin2d t = l; t.Mirror(mx.a); lin(t); }
      lin(l.Scaled(sp, ss)); { gp_Lin2d t = l; t.Scale(sp, ss); lin(t); } lin(l.Transformed(tr.t)); { gp_Lin2d t = l; t.Transform(tr.t); lin(t); }
      lin(l.Translated(gp_Vec2d(dv))); { gp_Lin2d t = l; t.Translate(gp_Vec2d(dv)); lin(t); } lin(l.Translated(p1, p2)); { gp_Lin2d t = l; t.Translate(p1, p2); lin(t); }
      finish(); }
    { RA a1 = ra(); gp_Pnt2d rc = rp(); double ra_ = rnd(-3.2, 3.2); T2 tr = mkT2(rotkind(n));
      begin("lin2d_rotate", "lin pnt f trsf2d"); pa(a1); pnt(rc); h(ra_); pt2(tr); bar();
      gp_Lin2d l(a1.a); lin(l.Rotated(rc, ra_)); { gp_Lin2d t = l; t.Rotate(rc, ra_); lin(t); } lin(l.Transformed(tr.t)); { gp_Lin2d t = l; t.Transform(tr.t); lin(t); } finish(); }

    // ---- gp_Circ2d ---------------------------------------------------------
    { RC k = rcirc(n); RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n + 2);
      begin("circ2d_constructors", "circ ax2d i ax22d"); pcirc(k); pa(a1); in(sense); pf(f2); bar();
      circ(gp_Circ2d()); circ(k.c); circ(gp_Circ2d(a1.a, k.r, sense == 1)); circ(gp_Circ2d(a1.a, k.r)); circ(gp_Circ2d(f2.a, k.r)); finish(); }
    { double rneg = (n == 5) ? -0.0 : (n % 4 == 3) ? rnd(0, 3) : -rnd(1e-3, 3); RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n); int w = rint(0, 3);
      begin("circ2d_radius_checks", "ax2d i ax22d f i"); pa(a1); in(sense); pf(f2); h(rneg); in(w); bar();
      GUARD(if (w == 0) circ(gp_Circ2d(a1.a, rneg, sense == 1)); else if (w == 1) circ(gp_Circ2d(f2.a, rneg));
            else { gp_Circ2d t(a1.a, 1.0); t.SetRadius(rneg); circ(t); })
      finish(); }
    { RC k = rcirc(n); gp_Pnt2d nl = rp(); RA a1 = ra(), a2 = ra(); RF f2 = rf(n + 1); double nr = rnd(0, 5);
      begin("circ2d_access", "circ pnt ax2d ax2d ax22d f"); pcirc(k); pnt(nl); pa(a1); pa(a2); pf(f2); h(nr); bar();
      const gp_Circ2d& c0 = k.c; pnt(c0.Location()); h(c0.Radius()); ax22d(c0.Axis()); ax22d(c0.Position()); ax2d(c0.XAxis()); ax2d(c0.YAxis());
      h(c0.Area()); h(c0.Length()); bl(c0.IsDirect());
      double ca, cb, cc2, cd, ce, cf; c0.Coefficients(ca, cb, cc2, cd, ce, cf); h(ca); h(cb); h(cc2); h(cd); h(ce); h(cf);
      circ(c0.Reversed()); { gp_Circ2d t = c0; t.Reverse(); circ(t); }
      { gp_Circ2d t = c0; t.SetLocation(nl); circ(t); } { gp_Circ2d t = c0; t.SetXAxis(a1.a); circ(t); } { gp_Circ2d t = c0; t.SetYAxis(a2.a); circ(t); }
      { gp_Circ2d t = c0; t.SetAxis(f2.a); circ(t); } { gp_Circ2d t = c0; t.SetRadius(nr); circ(t); }
      finish(); }
    { RC k = rcirc(n); gp_Pnt2d pt = rp(); double lt = rnd(0.01, 3);
      begin("circ2d_queries", "circ pnt f"); pcirc(k); pnt(pt); h(lt); bar();
      bl(k.c.Contains(pt, lt)); h(k.c.Distance(pt)); h(k.c.SquareDistance(pt)); bl(k.c.Contains(gp_Pnt2d(k.f.p.X() + k.r * k.f.a.XDirection().X(), k.f.p.Y() + k.r * k.f.a.XDirection().Y()), 1e-6)); finish(); }
    { RC k = rcirc(n); MOTION_EXACT("circ2d", "circ", pcirc(k), k.c, circ) }
    { RC k = rcirc(n); MOTION_ROTATE("circ2d", "circ", pcirc(k), k.c, circ) }

    // ---- gp_Elips2d --------------------------------------------------------
    { RE k = relips(n); RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n + 2);
      begin("elips2d_constructors", "elips ax2d i ax22d"); pelips(k); pa(a1); in(sense); pf(f2); bar();
      elips(gp_Elips2d()); elips(k.c); elips(gp_Elips2d(a1.a, k.a, k.b, sense == 1)); elips(gp_Elips2d(a1.a, k.a, k.b)); elips(gp_Elips2d(f2.a, k.a, k.b)); finish(); }
    { RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n); int w = rint(0, 3); double mj = rnd(0.5, 3), mn = rnd(0.5, 3);
      if (n % 3 == 0) { mn = -rnd(0.01, 1); } else if (mn < mj) { double t = mn; mn = mj + 0.01; mj = t; }   // minor > major, or negative minor
      begin("elips2d_radius_checks", "ax2d i ax22d f f i"); pa(a1); in(sense); pf(f2); h(mj); h(mn); in(w); bar();
      GUARD(if (w == 0) elips(gp_Elips2d(a1.a, mj, mn, sense == 1)); else if (w == 1) elips(gp_Elips2d(f2.a, mj, mn));
            else if (w == 2) { gp_Elips2d t(a1.a, 3.0, 2.0); t.SetMajorRadius(mj); elips(t); }
            else { gp_Elips2d t(a1.a, 3.0, 2.0); t.SetMinorRadius(mn); elips(t); })
      finish(); }
    { RE k = relips(n); gp_Pnt2d nl = rp(); RA a1 = ra(), a2 = ra(); RF f2 = rf(n + 1); double nmaj = k.a + rnd(0, 2), nmin = k.b * rnd(0, 1);
      begin("elips2d_access", "elips pnt ax2d ax2d ax22d f f"); pelips(k); pnt(nl); pa(a1); pa(a2); pf(f2); h(nmaj); h(nmin); bar();
      const gp_Elips2d& e0 = k.c; pnt(e0.Location()); h(e0.MajorRadius()); h(e0.MinorRadius()); ax22d(e0.Axis()); ax2d(e0.XAxis()); ax2d(e0.YAxis());
      h(e0.Area()); h(e0.Focal()); h(e0.Parameter()); h(e0.Eccentricity()); bl(e0.IsDirect()); pnt(e0.Focus1()); pnt(e0.Focus2());
      elips(e0.Reversed()); { gp_Elips2d t = e0; t.Reverse(); elips(t); }
      { gp_Elips2d t = e0; t.SetLocation(nl); elips(t); } { gp_Elips2d t = e0; t.SetXAxis(a1.a); elips(t); } { gp_Elips2d t = e0; t.SetYAxis(a2.a); elips(t); }
      { gp_Elips2d t = e0; t.SetAxis(f2.a); elips(t); } { gp_Elips2d t = e0; t.SetMajorRadius(nmaj); elips(t); } { gp_Elips2d t = e0; t.SetMinorRadius(nmin); elips(t); }
      finish(); }
    { RE k = relips(n);
      begin("elips2d_coefficients", "elips"); pelips(k); bar();
      double ca, cb, cc2, cd, ce, cf; k.c.Coefficients(ca, cb, cc2, cd, ce, cf); h(ca); h(cb); h(cc2); h(cd); h(ce); h(cf); finish(); }
    { RE k = relips(n);
      begin("elips2d_directrix", "elips"); pelips(k); bar();
      GUARD(ax2d(k.c.Directrix1()); ax2d(k.c.Directrix2());) finish(); }
    { RE k = relips(n); MOTION_EXACT("elips2d", "elips", pelips(k), k.c, elips) }
    { RE k = relips(n); MOTION_ROTATE("elips2d", "elips", pelips(k), k.c, elips) }

    // ---- gp_Hypr2d ---------------------------------------------------------
    { RH k = rhypr(n); RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n + 2);
      begin("hypr2d_constructors", "hypr ax2d i ax22d"); phypr(k); pa(a1); in(sense); pf(f2); bar();
      hypr(gp_Hypr2d()); hypr(k.c); hypr(gp_Hypr2d(a1.a, k.a, k.b, sense == 1)); hypr(gp_Hypr2d(a1.a, k.a, k.b)); hypr(gp_Hypr2d(f2.a, k.a, k.b)); finish(); }
    { RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n); int w = rint(0, 4); double mj = rnd(0.5, 3), mn = rnd(0.5, 3);
      if (n % 5 == 4) {} else if (n % 2 == 0) mj = -rnd(0.01, 1); else mn = -rnd(0.01, 1);
      begin("hypr2d_radius_checks", "ax2d i ax22d f f i"); pa(a1); in(sense); pf(f2); h(mj); h(mn); in(w); bar();
      GUARD(if (w == 0) hypr(gp_Hypr2d(a1.a, mj, mn, sense == 1)); else if (w == 1) hypr(gp_Hypr2d(f2.a, mj, mn));
            else if (w == 2) { gp_Hypr2d t(a1.a, 3.0, 2.0); t.SetMajorRadius(mj); hypr(t); }
            else if (w == 3) { gp_Hypr2d t(a1.a, 3.0, 2.0); t.SetMinorRadius(mn); hypr(t); }
            else { hypr(gp_Hypr2d(a1.a, (mj < 0) ? mj : 1.0, (mj < 0) ? 1.0 : mn)); })
      finish(); }
    { RH k = rhypr(n); gp_Pnt2d nl = rp(); RA a1 = ra(), a2 = ra(); RF f2 = rf(n + 1); double nmaj = rnd(0, 4), nmin = rnd(0, 4);
      begin("hypr2d_access", "hypr pnt ax2d ax2d ax22d f f"); phypr(k); pnt(nl); pa(a1); pa(a2); pf(f2); h(nmaj); h(nmin); bar();
      const gp_Hypr2d& e0 = k.c; pnt(e0.Location()); h(e0.MajorRadius()); h(e0.MinorRadius()); ax22d(e0.Axis()); ax2d(e0.XAxis()); ax2d(e0.YAxis());
      h(e0.Focal()); bl(e0.IsDirect()); pnt(e0.Focus1()); pnt(e0.Focus2());
      hypr(e0.Reversed()); { gp_Hypr2d t = e0; t.Reverse(); hypr(t); }
      hypr(e0.ConjugateBranch1()); hypr(e0.ConjugateBranch2()); hypr(e0.OtherBranch());
      { gp_Hypr2d t = e0; t.SetLocation(nl); hypr(t); } { gp_Hypr2d t = e0; t.SetXAxis(a1.a); hypr(t); } { gp_Hypr2d t = e0; t.SetYAxis(a2.a); hypr(t); }
      { gp_Hypr2d t = e0; t.SetAxis(f2.a); hypr(t); } { gp_Hypr2d t = e0; t.SetMajorRadius(nmaj); hypr(t); } { gp_Hypr2d t = e0; t.SetMinorRadius(nmin); hypr(t); }
      finish(); }
    { RH k = rhypr(n);
      begin("hypr2d_eccentricity", "hypr"); phypr(k); bar();
      GUARD(h(k.c.Eccentricity()); h(k.c.Parameter());) finish(); }
    { RH k = rhypr(n);
      begin("hypr2d_asymptotes", "hypr"); phypr(k); bar();
      GUARD(ax2d(k.c.Asymptote1()); ax2d(k.c.Asymptote2());) finish(); }
    { RH k = rhypr(n);
      begin("hypr2d_directrix", "hypr"); phypr(k); bar();
      GUARD(ax2d(k.c.Directrix1()); ax2d(k.c.Directrix2());) finish(); }
    { RH k = rhypr(n);
      begin("hypr2d_coefficients", "hypr"); phypr(k); bar();
      double ca, cb, cc2, cd, ce, cf; k.c.Coefficients(ca, cb, cc2, cd, ce, cf); h(ca); h(cb); h(cc2); h(cd); h(ce); h(cf); finish(); }
    { RH k = rhypr(n); MOTION_EXACT("hypr2d", "hypr", phypr(k), k.c, hypr) }
    { RH k = rhypr(n); MOTION_ROTATE("hypr2d", "hypr", phypr(k), k.c, hypr) }

    // ---- gp_Parab2d --------------------------------------------------------
    { RP k = rparab(n); RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n + 2); RA dr = ra(); gp_Pnt2d fo = rp();
      begin("parab2d_constructors", "parab ax2d i ax22d ax2d pnt"); pparab(k); pa(a1); in(sense); pf(f2); pa(dr); pnt(fo); bar();
      parab(gp_Parab2d()); parab(k.c); parab(gp_Parab2d(a1.a, k.fo, sense == 1)); parab(gp_Parab2d(a1.a, k.fo)); parab(gp_Parab2d(f2.a, k.fo));
      parab(gp_Parab2d(dr.a, fo, sense == 1)); parab(gp_Parab2d(dr.a, fo)); finish(); }
    { // focus on the location of the directrix: zero focal length, the axis is a quarter turn of the directrix
      RA dr = ra(); int sense = rint(0, 1);
      begin("parab2d_zero_focal", "ax2d i"); pa(dr); in(sense); bar();
      parab(gp_Parab2d(dr.a, dr.p, sense == 1)); finish(); }
    { RA a1 = ra(); int sense = rint(0, 1); RF f2 = rf(n); double neg = (n == 5) ? -0.0 : (n % 4 == 3) ? rnd(0, 3) : -rnd(1e-3, 3); int w = rint(0, 2);
      begin("parab2d_focal_checks", "ax2d i ax22d f i"); pa(a1); in(sense); pf(f2); h(neg); in(w); bar();
      GUARD(if (w == 0) parab(gp_Parab2d(a1.a, neg, sense == 1)); else if (w == 1) parab(gp_Parab2d(f2.a, neg));
            else { gp_Parab2d t(a1.a, 1.0); t.SetFocal(neg); parab(t); })
      finish(); }
    { RP k = rparab(n); gp_Pnt2d nl = rp(); RA a1 = ra(); RF f2 = rf(n + 1); double nf = rnd(0, 4);
      begin("parab2d_access", "parab pnt ax2d ax22d f"); pparab(k); pnt(nl); pa(a1); pf(f2); h(nf); bar();
      const gp_Parab2d& e0 = k.c; pnt(e0.Location()); h(e0.Focal()); h(e0.Parameter()); ax22d(e0.Axis()); ax2d(e0.MirrorAxis()); pnt(e0.Focus()); ax2d(e0.Directrix()); bl(e0.IsDirect());
      parab(e0.Reversed()); { gp_Parab2d t = e0; t.Reverse(); parab(t); }
      { gp_Parab2d t = e0; t.SetLocation(nl); parab(t); } { gp_Parab2d t = e0; t.SetMirrorAxis(a1.a); parab(t); }
      { gp_Parab2d t = e0; t.SetAxis(f2.a); parab(t); } { gp_Parab2d t = e0; t.SetFocal(nf); parab(t); }
      finish(); }
    { RP k = rparab(n);
      begin("parab2d_coefficients", "parab"); pparab(k); bar();
      double ca, cb, cc2, cd, ce, cf; k.c.Coefficients(ca, cb, cc2, cd, ce, cf); h(ca); h(cb); h(cc2); h(cd); h(ce); h(cf); finish(); }
    { RP k = rparab(n); MOTION_EXACT("parab2d", "parab", pparab(k), k.c, parab) }
    { RP k = rparab(n); MOTION_ROTATE("parab2d", "parab", pparab(k), k.c, parab) }
  }

  // ---- all-pairs transformation composition (outside the iteration loop) -------------
  for (int i = 0; i < NPAIR; ++i) {
    for (int j = 0; j < NPAIR; ++j) {
      T2 ta = mkPair(i), tb = mkPair(j);
      begin("trsf2d_composition", "trsf2d trsf2d"); pt2(ta); pt2(tb); bar();
      trsf(ta.t.Multiplied(tb.t)); { gp_Trsf2d t = ta.t; t.Multiply(tb.t); trsf(t); t = ta.t; t.PreMultiply(tb.t); trsf(t); }
      finish();
    }
  }
  // GTrsf2d composition over the same shapes, general (Other) ones included
  static const int GPAIR[8] = {0, 1, 3, 4, 5, 7, 8, 10};   // indices into PAIRKINDS
  for (int i = 0; i < 10; ++i) {
    for (int j = 0; j < 10; ++j) {
      G2 ga = i < 8 ? mkG2(0, PAIRKINDS[GPAIR[i]].kind, PAIRKINDS[GPAIR[i]].sub) : mkG2(i - 7, 12);
      G2 gb = j < 8 ? mkG2(0, PAIRKINDS[GPAIR[j]].kind, PAIRKINDS[GPAIR[j]].sub) : mkG2(j - 7, 12);
      begin("gtrsf2d_composition_all", "gtrsf2d gtrsf2d"); pg2(ga); pg2(gb); bar();
      GUARD(gtrsf(ga.g.Multiplied(gb.g)); gp_GTrsf2d m = ga.g; m.Multiply(gb.g); gtrsf(m); m = ga.g; m.PreMultiply(gb.g); gtrsf(m);) finish();
    }
  }

  // ---- families without any input: printed exactly once ------------------------
  begin("constants2d", ""); bar();
  pnt(gp::Origin2d()); dir(gp::DX2d()); dir(gp::DY2d()); ax2d(gp::OX2d()); ax2d(gp::OY2d()); finish();
}
