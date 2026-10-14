// Deterministic reference calls for the 3D gp curves and elementary surfaces (OCCT 8.0.1):
// gp_Lin, gp_Circ, gp_Elips, gp_Hypr, gp_Parab, gp_Pln, gp_Cylinder, gp_Cone, gp_Sphere, gp_Torus.
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints are decimal, bools are 0/1.
// Every input of a call is printed on the left of " | " in the order named by the
// family's schema string (`begin(name, schema)`); tests/gpcs.rs reads them back with the
// reader method of the same name. Outputs are either values, `throws` (OCCT raised an
// exception) or `unchecked` (the check is compiled out of OCCT 8.0.1 as built, the numbers
// it returns are unspecified; the Rust API must report an error there).
//
// Schema words (token counts): f=1 i=1 xyz=3 pnt=3 vec=3 dir=3 (raw, un-normalized
// direction input) ax1=6 (pnt dir) ax2=9 (pnt dir dir) ax3=10 (pnt dir dir flag, flag 0
// direct / 1 X reversed / 2 Y reversed) trsf=variable (a recipe, see mkTR),
// lin=6 (= ax1) circ=10 (ax2 radius) elips=11 (ax2 major minor) hypr=11 parab=10 (ax2 focal)
// pln=10 (= ax3) cylinder=11 (ax3 radius) cone=12 (ax3 semi-angle radius) sphere=11
// torus=12 (ax3 major minor).
#include <gp.hxx>
#include <gp_XYZ.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>
#include <gp_Dir.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Trsf.hxx>
#include <gp_Quaternion.hxx>
#include <gp_Lin.hxx>
#include <gp_Circ.hxx>
#include <gp_Elips.hxx>
#include <gp_Hypr.hxx>
#include <gp_Parab.hxx>
#include <gp_Pln.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Cone.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <NCollection_Array1.hxx>
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
    else if (w == "ax1" || w == "lin") n += 6;
    else if (w == "ax2") n += 9;
    else if (w == "ax3" || w == "pln" || w == "circ" || w == "parab") n += 10;
    else if (w == "elips" || w == "hypr" || w == "cylinder" || w == "sphere") n += 11;
    else if (w == "cone" || w == "torus") n += 12;
    else if (w == "trsf") return -1;
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
static void dir(const gp_Dir& v) { xyz(v.XYZ()); }
static void ax1(const gp_Ax1& a) { pnt(a.Location()); dir(a.Direction()); }
static void ax2(const gp_Ax2& a) { pnt(a.Location()); dir(a.Direction()); dir(a.XDirection()); dir(a.YDirection()); }
static void ax3(const gp_Ax3& a) { pnt(a.Location()); dir(a.Direction()); dir(a.XDirection()); dir(a.YDirection()); }
// the output form of every curve and surface is its full state
static void lin(const gp_Lin& l) { ax1(l.Position()); }
static void circ(const gp_Circ& c) { ax2(c.Position()); h(c.Radius()); }
static void elips(const gp_Elips& e) { ax2(e.Position()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void hypr(const gp_Hypr& e) { ax2(e.Position()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void parab(const gp_Parab& e) { ax2(e.Position()); h(e.Focal()); }
static void pln(const gp_Pln& e) { ax3(e.Position()); }
static void cylinder(const gp_Cylinder& e) { ax3(e.Position()); h(e.Radius()); }
static void cone(const gp_Cone& e) { ax3(e.Position()); h(e.SemiAngle()); h(e.RefRadius()); }
static void sphere(const gp_Sphere& e) { ax3(e.Position()); h(e.Radius()); }
static void torus(const gp_Torus& e) { ax3(e.Position()); h(e.MajorRadius()); h(e.MinorRadius()); }

// ---------------------------------------------------------------------------------
// random input objects. A "raw" direction is the un-normalized triple that is printed;
// the oracle and the test both build the gp_Dir from it.
struct Raw { double x, y, z; };
static void rw(const Raw& r) { h(r.x); h(r.y); h(r.z); }
static gp_Dir D(const Raw& r) { return gp_Dir(r.x, r.y, r.z); }
static Raw rraw() { for (;;) { Raw r{rnd(-4, 4), rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y + r.z * r.z > 0.25) return r; } }
static Raw toraw(const gp_Dir& d) { return Raw{d.X(), d.Y(), d.Z()}; }
static Raw lin3(const Raw& a, double k, const Raw& b, double l) { return Raw{a.x * k + b.x * l, a.y * k + b.y * l, a.z * k + b.z * l}; }
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
static void pa1(const RA1& r) { pnt(r.p); rw(r.d); }
static void pa2(const RA2& r) { pnt(r.p); rw(r.n); rw(r.x); }
static void pa3(const RA3& r) { pnt(r.p); rw(r.n); rw(r.x); in(r.f); }

// axis, frame and handed-frame generators: the n-th iteration forces special placements
static RA1 ra1m(int n) {
  gp_Pnt p = rp(); Raw d = rraw();
  switch (n % 6) {
    case 1: d = Raw{0, 0, 1}; break;
    case 2: d = Raw{1, 0, 0}; break;
    case 3: d = Raw{1, 1, 1}; break;
    case 4: p = gp_Pnt(0, 0, 0); break;
    default: break;
  }
  return mkA1(p, d);
}
static RA2 ra2m(int n) {
  gp_Pnt p = rp(); Raw nn = rraw(), xx = rraw();
  switch (n % 8) {
    case 1: nn = Raw{0, 0, 1}; xx = Raw{1, 0, 0}; break;
    case 4: nn = Raw{0, 0, -1}; xx = Raw{1, 0, 0}; break;
    case 5: nn = Raw{1, 1e-9, 0}; xx = Raw{1, 0, 0}; break;
    case 6: nn = Raw{1, 1, 1}; xx = Raw{1, -1, 0}; break;
    case 7: p = gp_Pnt(0, 0, 0); break;
    default: break;
  }
  return mkA2(p, nn, xx);
}
static RA3 ra3m(int n) {
  gp_Pnt p = rp(); Raw nn = rraw(), xx = rraw(); int f = rint(0, 2);
  switch (n % 8) {
    case 1: nn = Raw{0, 0, 1}; xx = Raw{1, 0, 0}; f = 0; break;
    case 2: f = 1; break;
    case 3: f = 2; break;
    case 4: nn = Raw{0, 0, -1}; xx = Raw{1, 0, 0}; f = 0; break;
    case 5: nn = Raw{1, 1e-9, 0}; xx = Raw{1, 0, 0}; f = 0; break;
    case 6: nn = Raw{1, 1, 1}; xx = Raw{1, -1, 0}; f = 1; break;
    case 7: p = gp_Pnt(0, 0, 0); f = 2; break;
    default: break;
  }
  return mkA3(p, nn, xx, f);
}

struct Qn { double c[4]; gp_Quaternion q; };
static Qn mkQ(double x, double y, double z, double w) { Qn t; t.c[0] = x; t.c[1] = y; t.c[2] = z; t.c[3] = w; t.q = gp_Quaternion(x, y, z, w); return t; }
static void pq(const Qn& t) { for (int k = 0; k < 4; ++k) h(t.c[k]); }
static Qn rq() {
  double x = rnd(-1, 1), y = rnd(-1, 1), z = rnd(-1, 1), w = rnd(-1, 1);
  gp_Quaternion q(x, y, z, w);
  q.Normalize();
  return mkQ(q.X(), q.Y(), q.Z(), q.W());
}

// transformation recipes (kind first, then the fields that kind uses)
//  0 rotation(ax1,angle)   1 rotation + translation part   2 scale(pnt,factor)
//  3 translation           4 mirror ax1   5 mirror pnt   6 mirror ax2(pnt,dir)
//  7 rotation(unit quat)   8 rotation(quat) + translation part + scale factor
// Kinds 2..8 do not use sin/cos.
struct TR { int kind; gp_Pnt p; Raw d; double ang; gp_XYZ v; double sc; Qn q; gp_Trsf t; };
static TR mkTR(int kind) {
  TR r; r.kind = kind; r.p = rp(); r.d = rraw(); r.ang = rnd(-3, 3); r.v = rxyz(); r.sc = rnz(0.3, 2.5); r.q = rq();
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
// kinds without sin/cos: 2..8; `k` selects among them
static int knotrig(int k) { return 2 + k % 7; }
// alternate between the two rotation recipes (0, 1) and the other recipes
static int rotkind(int k) { return (k % 2 == 0) ? (k / 2) % 2 : knotrig(k); }
static double rscale() {
  static int cnt = 0; ++cnt;
  if (cnt % 7 == 0) return 1.0;
  if (cnt % 7 == 3) return -1.0;
  if (cnt % 11 == 5) return 0.0;
  return rnz(0.3, 2.5);
}

// ---------------------------------------------------------------------------------
// curve and surface generators; the n-th iteration forces the degenerate parameters
struct RL { RA1 f; gp_Lin c; };
static RL rlin(int n) { RL k; k.f = ra1m(n); k.c = gp_Lin(k.f.a); return k; }
static void plin(const RL& k) { pa1(k.f); }

struct RC { RA2 f; double r; gp_Circ c; };
static RC rcirc(int n) { RC k; k.f = ra2m(n); k.r = (n == 3) ? 0.0 : (n == 6) ? 1.0 : rnd(0.1, 5); k.c = gp_Circ(k.f.a, k.r); return k; }
static void pcirc(const RC& k) { pa2(k.f); h(k.r); }

struct RE { RA2 f; double a, b; gp_Elips c; };
static RE relips(int n) {
  RE k; k.f = ra2m(n); k.a = rnd(0.5, 5); k.b = k.a * rnd(0.05, 1);
  if (n == 3) k.b = k.a;
  if (n == 4) k.b = 0;
  if (n == 5) { k.a = 0; k.b = 0; }
  k.c = gp_Elips(k.f.a, k.a, k.b); return k;
}
static void pelips(const RE& k) { pa2(k.f); h(k.a); h(k.b); }

struct RH { RA2 f; double a, b; gp_Hypr c; };
static RH rhypr(int n) {
  RH k; k.f = ra2m(n); k.a = rnd(0.2, 4); k.b = rnd(0, 4);
  if (n == 4) k.a = 0;
  if (n == 5) k.b = 0;
  if (n == 6) { k.a = 0; k.b = 0; }
  k.c = gp_Hypr(k.f.a, k.a, k.b); return k;
}
static void phypr(const RH& k) { pa2(k.f); h(k.a); h(k.b); }

struct RP { RA2 f; double fo; gp_Parab c; };
static RP rparab(int n) { RP k; k.f = ra2m(n); k.fo = (n == 3) ? 0.0 : rnd(0, 4); k.c = gp_Parab(k.f.a, k.fo); return k; }
static void pparab(const RP& k) { pa2(k.f); h(k.fo); }

struct RPl { RA3 f; gp_Pln c; };
static RPl rpln(int n) { RPl k; k.f = ra3m(n); k.c = gp_Pln(k.f.a); return k; }
static void ppln(const RPl& k) { pa3(k.f); }

static double rrad(int n) { return (n == 3) ? 0.0 : (n == 6) ? 1.0 : rnd(0.1, 5); }
struct RCy { RA3 f; double r; gp_Cylinder c; };
static RCy rcyl(int n) { RCy k; k.f = ra3m(n); k.r = rrad(n); k.c = gp_Cylinder(k.f.a, k.r); return k; }
static void pcyl(const RCy& k) { pa3(k.f); h(k.r); }

struct RCo { RA3 f; double ang, r; gp_Cone c; };
static RCo rcone(int n) {
  RCo k; k.f = ra3m(n); k.ang = rnz(0.05, 1.5); k.r = rnd(0, 4);
  if (n == 3) k.ang = 1e-9;
  if (n == 4) k.ang = std::nextafter(M_PI * 0.5, 0.0);
  if (n == 5) k.ang = -1.0;
  if (n == 6) k.ang = M_PI * 0.25;
  if (n == 7) k.r = 0.0;
  if (n == 9) k.ang = -std::nextafter(M_PI * 0.5, 0.0);
  k.c = gp_Cone(k.f.a, k.ang, k.r); return k;
}
static void pcone(const RCo& k) { pa3(k.f); h(k.ang); h(k.r); }

struct RS { RA3 f; double r; gp_Sphere c; };
static RS rsph(int n) { RS k; k.f = ra3m(n); k.r = rrad(n); k.c = gp_Sphere(k.f.a, k.r); return k; }
static void psph(const RS& k) { pa3(k.f); h(k.r); }

struct RT { RA3 f; double a, b; gp_Torus c; };
static RT rtorus(int n) {
  RT k; k.f = ra3m(n); k.a = rnd(0.5, 5); k.b = k.a * rnd(0.05, 1.5);
  if (n == 3) k.b = k.a;
  if (n == 4) k.b = 0;
  if (n == 5) k.b = k.a * 3;
  k.c = gp_Torus(k.f.a, k.a, k.b); return k;
}
static void ptorus(const RT& k) { pa3(k.f); h(k.a); h(k.b); }

// motion families on a curve or surface: mirror point / axis / plane, scale, transform, translate.
// All copy forms are printed; of the in-place forms only translate-by-points (it differs in the last bit from the
// copy form in C++) is always printed, the others in rotation selected by the printed integer `sel` (n % 3):
// 0 the three mirrors, 1 scale and transform, 2 translate by a vector.
#define MOTION_EXACT(NAME, SCHEMA, PRINTIN, OBJ, PR)                                                              \
  { gp_Pnt cen = rp(); RA1 mx1 = ra1(); RA2 mx2 = ra2(); TR tr = mkTR(knotrig(n)); gp_Pnt sp = rp();             \
    double ss = rscale(); gp_XYZ dv = rxyz(); gp_Pnt p1 = rp(), p2 = rp(); int sel = n % 3;                       \
    begin(NAME "_motion", SCHEMA " pnt ax1 ax2 pnt f trsf vec pnt pnt i"); PRINTIN; pnt(cen); pa1(mx1); pa2(mx2); \
    pnt(sp); h(ss); ptr(tr); xyz(dv); pnt(p1); pnt(p2); in(sel); bar();                                           \
    PR(OBJ.Mirrored(cen)); PR(OBJ.Mirrored(mx1.a)); PR(OBJ.Mirrored(mx2.a));                                      \
    PR(OBJ.Scaled(sp, ss)); PR(OBJ.Transformed(tr.t)); PR(OBJ.Translated(gp_Vec(dv))); PR(OBJ.Translated(p1, p2)); \
    { auto t = OBJ; t.Translate(p1, p2); PR(t); }                                                                 \
    if (sel == 0) { { auto t = OBJ; t.Mirror(cen); PR(t); } { auto t = OBJ; t.Mirror(mx1.a); PR(t); } { auto t = OBJ; t.Mirror(mx2.a); PR(t); } } \
    else if (sel == 1) { { auto t = OBJ; t.Scale(sp, ss); PR(t); } { auto t = OBJ; t.Transform(tr.t); PR(t); } } \
    else { auto t = OBJ; t.Translate(gp_Vec(dv)); PR(t); }                                                        \
    finish(); }
#define MOTION_ROTATE(NAME, SCHEMA, PRINTIN, OBJ, PR)                                                             \
  { RA1 rax = ra1(); double ra_ = rnd(-3.2, 3.2); TR tr = mkTR(rotkind(n));                                       \
    begin(NAME "_rotate", SCHEMA " ax1 f trsf"); PRINTIN; pa1(rax); h(ra_); ptr(tr); bar();                       \
    PR(OBJ.Rotated(rax.a, ra_)); { auto t = OBJ; t.Rotate(rax.a, ra_); PR(t); }                                   \
    PR(OBJ.Transformed(tr.t)); { auto t = OBJ; t.Transform(tr.t); PR(t); }                                        \
    finish(); }

// radii (and focal lengths) for the "checks" families: negative ones raise, -0.0 and 0 do not
static const double RADII[16] = {-0.5, 0.0, -0.0, 2.5, -1e-9, 3.0, -2.0, 0.1, -3.0, 5.0, -7e-3, 0.25, -0.0, 1.0, -4.0, 2.0};

// the six axis-like outputs shared by the access families
static void axes(const gp_Ax1& a, const gp_Ax1& x, const gp_Ax1& y) { ax1(a); ax1(x); ax1(y); }

int main() {
  const int N = 16;
  for (int n = 0; n < N; ++n) {
    // ================= gp_Lin =================================================
    { RL k = rlin(n); RA1 a1 = ra1(); gp_Pnt np = rp(); Raw nd = rraw();
      begin("lin_constructors", "lin ax1 pnt dir"); plin(k); pa1(a1); pnt(np); rw(nd); bar();
      lin(gp_Lin()); lin(k.c); lin(gp_Lin(a1.a)); lin(gp_Lin(np, D(nd))); finish(); }
    { RL k = rlin(n); gp_Pnt nl = rp(); RA1 a1 = ra1(); Raw nd = rraw();
      begin("lin_access", "lin pnt ax1 dir"); plin(k); pnt(nl); pa1(a1); rw(nd); bar();
      const gp_Lin& l = k.c; pnt(l.Location()); dir(l.Direction()); ax1(l.Position());
      lin(l.Reversed()); { gp_Lin t = l; t.Reverse(); lin(t); }
      { gp_Lin t = l; t.SetLocation(nl); lin(t); } { gp_Lin t = l; t.SetPosition(a1.a); lin(t); } { gp_Lin t = l; t.SetDirection(D(nd)); lin(t); }
      finish(); }
    { RL k = rlin(n); gp_Pnt pt = rp(); double t0 = rnd(-3, 3); gp_Pnt onl = padd(k.f.p, k.f.d, t0 / std::sqrt(k.f.d.x * k.f.d.x + k.f.d.y * k.f.d.y + k.f.d.z * k.f.d.z)); double lt = rnd(0.01, 3);
      if (n == 2) onl = k.f.p;
      begin("lin_point_queries", "lin pnt pnt f"); plin(k); pnt(pt); pnt(onl); h(lt); bar();
      h(k.c.Distance(pt)); h(k.c.SquareDistance(pt)); bl(k.c.Contains(pt, lt)); h(k.c.Distance(onl)); h(k.c.SquareDistance(onl)); bl(k.c.Contains(onl, 1e-6));
      finish(); }
    { RL k = rlin(n); gp_Pnt pt = rp();
      if (n == 3) pt = k.f.p;                                                  // at the location: zero vector
      if (n == 4) { k.f = mkA1(k.f.p, Raw{0, 0, 1}); k.c = gp_Lin(k.f.a); pt = gp_Pnt(k.f.p.X(), k.f.p.Y(), k.f.p.Z() + 2.5); }   // exactly on the line
      begin("lin_normal", "lin pnt"); plin(k); pnt(pt); bar();
      GUARD(lin(k.c.Normal(pt));) finish(); }
    { RL k = rlin(n); RA1 o = ra1();
      // partner line: 0 random, 1 same raw direction (parallel), 2 opposite raw direction, 3 meets the first line, 4 identical,
      // 5 parallel through a random point, 6 perpendicular through the first location
      int mode = n % 7;
      Raw dd = rraw();
      switch (mode) {
        case 1: o = mkA1(rp(), k.f.d); break;
        case 2: o = mkA1(rp(), Raw{-k.f.d.x, -k.f.d.y, -k.f.d.z}); break;
        case 3: o = mkA1(padd(k.f.p, k.f.d, 0.5), dd); break;
        case 4: o = k.f; break;
        case 5: o = mkA1(rp(), lin3(k.f.d, 2.5, Raw{0, 0, 0}, 0)); break;
        case 6: { Raw nz{k.f.d.y, -k.f.d.x, 0.0}; if (nz.x == 0 && nz.y == 0) nz = Raw{1, 0, 0}; o = mkA1(k.f.p, nz); break; }
        default: break;
      }
      begin("lin_line_distance", "lin lin"); plin(k); pa1(o); bar();
      gp_Lin l2(o.a); h(k.c.Distance(l2)); h(k.c.SquareDistance(l2)); h(l2.Distance(k.c)); finish();
      begin("lin_angle", "lin lin"); plin(k); pa1(o); bar();
      h(k.c.Angle(l2)); h(l2.Angle(k.c)); finish(); }
    { RL k = rlin(n); MOTION_EXACT("lin", "lin", plin(k), k.c, lin) }
    { RL k = rlin(n); MOTION_ROTATE("lin", "lin", plin(k), k.c, lin) }

    // ================= gp_Circ ================================================
    { RC k = rcirc(n); RA2 a1 = ra2();
      begin("circ_constructors", "circ ax2"); pcirc(k); pa2(a1); bar();
      circ(gp_Circ()); circ(k.c); circ(gp_Circ(a1.a, k.r)); finish(); }
    { double rneg = RADII[n]; RA2 a1 = ra2(); int w = n % 2;
      begin("circ_radius_checks", "ax2 f i"); pa2(a1); h(rneg); in(w); bar();
      GUARD(if (w == 0) circ(gp_Circ(a1.a, rneg)); else { gp_Circ t(a1.a, 1.0); t.SetRadius(rneg); circ(t); })
      finish(); }
    { RC k = rcirc(n); gp_Pnt nl = rp(); RA2 a1 = ra2(); RA1 nx = ra1(); double nr = rnd(0, 5);
      begin("circ_access", "circ pnt ax2 ax1 f"); pcirc(k); pnt(nl); pa2(a1); pa1(nx); h(nr); bar();
      const gp_Circ& c0 = k.c; pnt(c0.Location()); h(c0.Radius()); ax2(c0.Position()); axes(c0.Axis(), c0.XAxis(), c0.YAxis());
      h(c0.Area()); h(c0.Length());
      { gp_Circ t = c0; t.SetLocation(nl); circ(t); } { gp_Circ t = c0; t.SetPosition(a1.a); circ(t); } { gp_Circ t = c0; t.SetAxis(nx.a); circ(t); }
      { gp_Circ t = c0; t.SetRadius(nr); circ(t); }
      finish(); }
    { RC k = rcirc(n); gp_Pnt pt = rp(); double lt = rnd(0.01, 3);
      gp_Pnt onc(k.f.p.X() + k.r * k.c.XAxis().Direction().X(), k.f.p.Y() + k.r * k.c.XAxis().Direction().Y(), k.f.p.Z() + k.r * k.c.XAxis().Direction().Z());
      begin("circ_queries", "circ pnt pnt f"); pcirc(k); pnt(pt); pnt(onc); h(lt); bar();
      bl(k.c.Contains(pt, lt)); h(k.c.Distance(pt)); h(k.c.SquareDistance(pt)); bl(k.c.Contains(onc, 1e-6)); h(k.c.Distance(onc)); h(k.c.SquareDistance(k.f.p)); finish(); }
    { RC k = rcirc(n); MOTION_EXACT("circ", "circ", pcirc(k), k.c, circ) }
    { RC k = rcirc(n); MOTION_ROTATE("circ", "circ", pcirc(k), k.c, circ) }

    // ================= gp_Elips ===============================================
    { RE k = relips(n); RA2 a1 = ra2();
      begin("elips_constructors", "elips ax2"); pelips(k); pa2(a1); bar();
      elips(gp_Elips()); elips(k.c); elips(gp_Elips(a1.a, k.a, k.b)); finish(); }
    { // w 0: constructor(major, minor); 1: SetMajorRadius(major) on (3, 2); 2: SetMinorRadius(minor) on (3, 2)
      static const int    ws[16]  = {0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2};
      static const double mjs[16] = {3.0, 2.0, 3.0, 0.0, 1.0, 3.0, -1.0, 0.0, 2.0, 1.9, 10.0, -5.0, 0.5, 0.5, 0.5, 0.5};
      static const double mns[16] = {2.0, 2.0, 0.0, 0.0, 2.0, -0.1, -2.0, -0.0, 0.7, 0.7, 0.7, 0.7, 3.0, 3.1, 0.0, -1e-3};
      RA2 a1 = ra2(); int w = ws[n]; double mj = mjs[n], mn = mns[n];
      begin("elips_radius_checks", "ax2 f f i"); pa2(a1); h(mj); h(mn); in(w); bar();
      GUARD(if (w == 0) elips(gp_Elips(a1.a, mj, mn));
            else if (w == 1) { gp_Elips t(a1.a, 3.0, 2.0); t.SetMajorRadius(mj); elips(t); }
            else { gp_Elips t(a1.a, 3.0, 2.0); t.SetMinorRadius(mn); elips(t); })
      finish(); }
    { RE k = relips(n); gp_Pnt nl = rp(); RA2 a1 = ra2(); RA1 nx = ra1(); double nmaj = k.a + rnd(0, 2), nmin = k.b * rnd(0, 1);
      begin("elips_access", "elips pnt ax2 ax1 f f"); pelips(k); pnt(nl); pa2(a1); pa1(nx); h(nmaj); h(nmin); bar();
      const gp_Elips& e0 = k.c; pnt(e0.Location()); h(e0.MajorRadius()); h(e0.MinorRadius()); ax2(e0.Position()); axes(e0.Axis(), e0.XAxis(), e0.YAxis());
      h(e0.Area()); h(e0.Focal()); h(e0.Parameter()); h(e0.Eccentricity()); pnt(e0.Focus1()); pnt(e0.Focus2());
      { gp_Elips t = e0; t.SetLocation(nl); elips(t); } { gp_Elips t = e0; t.SetPosition(a1.a); elips(t); } { gp_Elips t = e0; t.SetAxis(nx.a); elips(t); }
      { gp_Elips t = e0; t.SetMajorRadius(nmaj); elips(t); } { gp_Elips t = e0; t.SetMinorRadius(nmin); elips(t); }
      finish(); }
    { RE k = relips(n);
      begin("elips_directrix", "elips"); pelips(k); bar();
      GUARD(ax1(k.c.Directrix1()); ax1(k.c.Directrix2());) finish(); }
    { RE k = relips(n); MOTION_EXACT("elips", "elips", pelips(k), k.c, elips) }
    { RE k = relips(n); MOTION_ROTATE("elips", "elips", pelips(k), k.c, elips) }

    // ================= gp_Hypr ================================================
    { RH k = rhypr(n); RA2 a1 = ra2();
      begin("hypr_constructors", "hypr ax2"); phypr(k); pa2(a1); bar();
      hypr(gp_Hypr()); hypr(k.c); hypr(gp_Hypr(a1.a, k.a, k.b)); finish(); }
    { static const int    ws[16]  = {0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 0};
      static const double mjs[16] = {3.0, 0.0, -1.0, 2.0, -0.0, 1.0, 0.0, -1.0, 5.0, -1e-3, 0.4, 0.4, 0.4, 0.4, 0.4, 4.0};
      static const double mns[16] = {2.0, 0.0, 2.0, -1.0, 1.0, 5.0, 0.2, 0.2, 0.2, 0.2, 0.0, -1e-3, 7.0, -0.0, -3.0, 0.5};
      RA2 a1 = ra2(); int w = ws[n]; double mj = mjs[n], mn = mns[n];
      begin("hypr_radius_checks", "ax2 f f i"); pa2(a1); h(mj); h(mn); in(w); bar();
      GUARD(if (w == 0) hypr(gp_Hypr(a1.a, mj, mn));
            else if (w == 1) { gp_Hypr t(a1.a, 3.0, 2.0); t.SetMajorRadius(mj); hypr(t); }
            else { gp_Hypr t(a1.a, 3.0, 2.0); t.SetMinorRadius(mn); hypr(t); })
      finish(); }
    { RH k = rhypr(n); gp_Pnt nl = rp(); RA2 a1 = ra2(); RA1 nx = ra1(); double nmaj = rnd(0, 4), nmin = rnd(0, 4);
      begin("hypr_access", "hypr pnt ax2 ax1 f f"); phypr(k); pnt(nl); pa2(a1); pa1(nx); h(nmaj); h(nmin); bar();
      const gp_Hypr& e0 = k.c; pnt(e0.Location()); h(e0.MajorRadius()); h(e0.MinorRadius()); ax2(e0.Position()); axes(e0.Axis(), e0.XAxis(), e0.YAxis());
      h(e0.Focal()); pnt(e0.Focus1()); pnt(e0.Focus2());
      hypr(e0.ConjugateBranch1()); hypr(e0.ConjugateBranch2()); hypr(e0.OtherBranch());
      { gp_Hypr t = e0; t.SetLocation(nl); hypr(t); } { gp_Hypr t = e0; t.SetPosition(a1.a); hypr(t); } { gp_Hypr t = e0; t.SetAxis(nx.a); hypr(t); }
      { gp_Hypr t = e0; t.SetMajorRadius(nmaj); hypr(t); } { gp_Hypr t = e0; t.SetMinorRadius(nmin); hypr(t); }
      finish(); }
    { // the default hyperbola has a negative minor radius: its derived branches cannot be built
      RA2 a1 = ra2(); int w = n % 3;
      begin("hypr_default_branches", "ax2 i"); pa2(a1); in(w); bar();
      GUARD(gp_Hypr t; t.SetPosition(a1.a); if (w == 0) hypr(t.ConjugateBranch1()); else if (w == 1) hypr(t.ConjugateBranch2()); else hypr(t.OtherBranch());)
      finish(); }
    { RH k = rhypr(n);
      begin("hypr_eccentricity", "hypr"); phypr(k); bar();
      GUARD(h(k.c.Eccentricity()); h(k.c.Parameter());) finish(); }
    { RH k = rhypr(n);
      begin("hypr_asymptotes", "hypr"); phypr(k); bar();
      GUARD(ax1(k.c.Asymptote1()); ax1(k.c.Asymptote2());) finish(); }
    { RH k = rhypr(n);
      begin("hypr_directrix", "hypr"); phypr(k); bar();
      GUARD(ax1(k.c.Directrix1()); ax1(k.c.Directrix2());) finish(); }
    { RH k = rhypr(n); MOTION_EXACT("hypr", "hypr", phypr(k), k.c, hypr) }
    { RH k = rhypr(n); MOTION_ROTATE("hypr", "hypr", phypr(k), k.c, hypr) }

    // ================= gp_Parab ===============================================
    { RP k = rparab(n); RA2 a1 = ra2();
      begin("parab_constructors", "parab ax2"); pparab(k); pa2(a1); bar();
      parab(gp_Parab()); parab(k.c); parab(gp_Parab(a1.a, k.fo)); finish(); }
    { // directrix and focus: the focus on the directrix has no normal (raises), also the focus at the directrix location
      RA1 dr = ra1m(n); gp_Pnt fo = rp();
      if (n == 3) fo = dr.p;
      if (n == 4) { dr = mkA1(dr.p, Raw{0, 0, 1}); fo = gp_Pnt(dr.p.X(), dr.p.Y(), dr.p.Z() + 2.0); }
      begin("parab_directrix_focus", "ax1 pnt"); pa1(dr); pnt(fo); bar();
      GUARD(parab(gp_Parab(dr.a, fo));) finish(); }
    { double neg = RADII[n]; RA2 a1 = ra2(); int w = n % 2;
      begin("parab_focal_checks", "ax2 f i"); pa2(a1); h(neg); in(w); bar();
      GUARD(if (w == 0) parab(gp_Parab(a1.a, neg)); else { gp_Parab t(a1.a, 1.0); t.SetFocal(neg); parab(t); })
      finish(); }
    { RP k = rparab(n); gp_Pnt nl = rp(); RA2 a1 = ra2(); RA1 nx = ra1(); double nf = rnd(0, 4);
      begin("parab_access", "parab pnt ax2 ax1 f"); pparab(k); pnt(nl); pa2(a1); pa1(nx); h(nf); bar();
      const gp_Parab& e0 = k.c; pnt(e0.Location()); h(e0.Focal()); h(e0.Parameter()); ax2(e0.Position()); axes(e0.Axis(), e0.XAxis(), e0.YAxis());
      ax1(e0.Directrix()); pnt(e0.Focus());
      { gp_Parab t = e0; t.SetLocation(nl); parab(t); } { gp_Parab t = e0; t.SetPosition(a1.a); parab(t); } { gp_Parab t = e0; t.SetAxis(nx.a); parab(t); }
      { gp_Parab t = e0; t.SetFocal(nf); parab(t); }
      finish(); }
    { RP k = rparab(n); MOTION_EXACT("parab", "parab", pparab(k), k.c, parab) }
    { RP k = rparab(n); MOTION_ROTATE("parab", "parab", pparab(k), k.c, parab) }

    // ================= gp_Pln =================================================
    { RPl k = rpln(n); RA3 a3 = ra3m(n + 3); Raw nv = rraw(); gp_Pnt np = rp();
      // normals that tie the coordinate comparisons of gp_Pln(P, V)
      static const Raw ties[] = {{0, 0, 1}, {1, 0, 0}, {0, 1, 0}, {1, 1, 1}, {-1, 0, 1}, {0, -1, -1}, {1, 1, 0}, {0, 1, 1}, {1, 0, 1}, {-2, 1, 2}, {1, -1, -1}, {0, 0, -3}};
      if (n < 12) nv = ties[n];
      begin("pln_constructors", "pln ax3 pnt dir"); ppln(k); pa3(a3); pnt(np); rw(nv); bar();
      pln(gp_Pln()); pln(k.c); pln(gp_Pln(a3.a)); pln(gp_Pln(np, D(nv))); finish(); }
    { static const double coef[][4] = {
        {0, 0, 0, 1}, {0, 0, 1, 0}, {0, 2, 0, 1}, {1, 0, 0, -3}, {1, 1, 1, 1}, {1, 1, 0, 2}, {0, 1, 1, 2}, {1, 0, 1, 2},
        {-1, 2, -3, 5}, {0, 0, 1e-200, 1}, {1e-160, 0, 0, 4}, {0, -2, 2, 0}, {-3, 3, 3, 1}};
      double a, b, c, d;
      if (n < 13) { a = coef[n][0]; b = coef[n][1]; c = coef[n][2]; d = coef[n][3]; }
      else { a = rnz(0.2, 3); b = rnz(0.2, 3); c = rnz(0.2, 3); d = rnz(0, 4); }
      begin("pln_from_coefficients", "f f f f"); h(a); h(b); h(c); h(d); bar();
      // the zero-norm check of the internal gp_Dir is compiled out of the library: unspecified numbers
      if (std::sqrt(a * a + b * b + c * c) <= gp::Resolution()) UNCHECKED();
      else { GUARD(pln(gp_Pln(a, b, c, d));) }
      finish(); }
    { RPl k = rpln(n); gp_Pnt nl = rp(); RA3 a3 = ra3m(n + 1); RA1 nx = ra1();
      begin("pln_access", "pln pnt ax3 ax1"); ppln(k); pnt(nl); pa3(a3); pa1(nx); bar();
      const gp_Pln& p0 = k.c; pnt(p0.Location()); ax3(p0.Position()); axes(p0.Axis(), p0.XAxis(), p0.YAxis()); bl(p0.Direct());
      double ca, cb, cc, cd; p0.Coefficients(ca, cb, cc, cd); h(ca); h(cb); h(cc); h(cd);
      { gp_Pln t = p0; t.SetLocation(nl); pln(t); } { gp_Pln t = p0; t.SetPosition(a3.a); pln(t); } { gp_Pln t = p0; t.SetAxis(nx.a); pln(t); }
      { gp_Pln t = p0; t.UReverse(); pln(t); bl(t.Direct()); } { gp_Pln t = p0; t.VReverse(); pln(t); bl(t.Direct()); }
      finish(); }
    { RPl k = rpln(n); gp_Pnt pt = rp(); double lt = rnd(0.01, 3);
      double ua = rnd(-3, 3), ub = rnd(-3, 3);
      const gp_Ax3& ps = k.c.Position();
      gp_Pnt onp(ps.Location().X() + ua * ps.XDirection().X() + ub * ps.YDirection().X(), ps.Location().Y() + ua * ps.XDirection().Y() + ub * ps.YDirection().Y(), ps.Location().Z() + ua * ps.XDirection().Z() + ub * ps.YDirection().Z());
      begin("pln_point_queries", "pln pnt pnt f"); ppln(k); pnt(pt); pnt(onp); h(lt); bar();
      h(k.c.SignedDistance(pt)); h(k.c.Distance(pt)); h(k.c.SquareDistance(pt)); bl(k.c.Contains(pt, lt));
      h(k.c.SignedDistance(onp)); h(k.c.Distance(onp)); h(k.c.SquareDistance(onp)); bl(k.c.Contains(onp, 1e-6)); finish(); }
    { RPl k = rpln(n); RA1 o = ra1(); double lt = rnd(0.01, 2), at = (n % 3 == 2) ? 1e-5 : rnd(0.01, 0.4);
      const gp_Ax3& ps = k.c.Position();
      Raw xr = toraw(ps.XDirection()), yr = toraw(ps.YDirection()), nr = toraw(ps.Direction());
      // partner line: 0 random, 1 in the plane, 2 parallel above the plane, 3 along the normal, 4 in the plane (X direction) at the location,
      // 5 nearly in the plane, 6 crossing the plane obliquely, 7 parallel below the plane
      switch (n % 8) {
        case 1: o = mkA1(padd(padd(ps.Location(), xr, 1.5), yr, 0.5), lin3(xr, 1.0, yr, 0.75)); break;
        case 2: o = mkA1(padd(ps.Location(), nr, 3.0), xr); break;
        case 3: o = mkA1(rp(), nr); break;
        case 4: o = mkA1(ps.Location(), xr); break;
        case 5: o = mkA1(padd(ps.Location(), nr, 1e-9), lin3(xr, 1.0, nr, 1e-9)); break;
        case 6: o = mkA1(padd(ps.Location(), nr, 1.0), lin3(xr, 1.0, nr, 1.0)); break;
        case 7: o = mkA1(padd(ps.Location(), nr, -1.5), yr); break;
        default: break;
      }
      gp_Lin l2(o.a);
      begin("pln_line_queries", "pln lin f f"); ppln(k); pa1(o); h(lt); h(at); bar();
      h(k.c.SignedDistance(l2)); h(k.c.Distance(l2)); h(k.c.SquareDistance(l2)); bl(k.c.Contains(l2, lt, at)); finish(); }
    { RPl k = rpln(n); RA3 o = ra3m(n + 5);
      // partner plane: 0 random, 1 same raw orientation, shifted, 2 opposite normal, 3 identical, 4 nearly parallel, 5 parallel above, 6 perpendicular
      const gp_Ax3& ps = k.c.Position();
      switch (n % 7) {
        case 1: o = mkA3(rp(), k.f.n, k.f.x, k.f.f); break;
        case 2: o = mkA3(rp(), Raw{-k.f.n.x, -k.f.n.y, -k.f.n.z}, k.f.x, k.f.f); break;
        case 3: o = k.f; break;
        case 4: o = mkA3(padd(k.f.p, k.f.n, 0.5), lin3(k.f.n, 1.0, Raw{0.3, -0.2, 0.1}, 1e-9), k.f.x, k.f.f); break;
        case 5: { Raw nr = toraw(ps.Direction()); o = mkA3(padd(ps.Location(), nr, 2.0), k.f.n, k.f.x, k.f.f); break; }
        case 6: { Raw xr = toraw(ps.XDirection()); o = mkA3(rp(), xr, toraw(ps.Direction()), 0); break; }
        default: break;
      }
      gp_Pln p2(o.a);
      begin("pln_plane_queries", "pln pln"); ppln(k); pa3(o); bar();
      h(k.c.SignedDistance(p2)); h(k.c.Distance(p2)); h(k.c.SquareDistance(p2)); h(p2.SignedDistance(k.c)); finish(); }
    { RPl k = rpln(n); MOTION_EXACT("pln", "pln", ppln(k), k.c, pln) }
    { RPl k = rpln(n); MOTION_ROTATE("pln", "pln", ppln(k), k.c, pln) }

    // ================= gp_Cylinder ============================================
    { RCy k = rcyl(n); RA3 a1 = ra3m(n + 1);
      begin("cylinder_constructors", "cylinder ax3"); pcyl(k); pa3(a1); bar();
      cylinder(gp_Cylinder()); cylinder(k.c); cylinder(gp_Cylinder(a1.a, k.r)); finish(); }
    { double rneg = RADII[n]; RA3 a1 = ra3m(n); int w = n % 2;
      begin("cylinder_radius_checks", "ax3 f i"); pa3(a1); h(rneg); in(w); bar();
      GUARD(if (w == 0) cylinder(gp_Cylinder(a1.a, rneg)); else { gp_Cylinder t(a1.a, 1.0); t.SetRadius(rneg); cylinder(t); })
      finish(); }
    { RCy k = rcyl(n); gp_Pnt nl = rp(); RA3 a3 = ra3m(n + 2); RA1 nx = ra1(); double nr = rnd(0, 5);
      begin("cylinder_access", "cylinder pnt ax3 ax1 f"); pcyl(k); pnt(nl); pa3(a3); pa1(nx); h(nr); bar();
      const gp_Cylinder& c0 = k.c; pnt(c0.Location()); h(c0.Radius()); ax3(c0.Position()); axes(c0.Axis(), c0.XAxis(), c0.YAxis()); bl(c0.Direct());
      { gp_Cylinder t = c0; t.SetLocation(nl); cylinder(t); } { gp_Cylinder t = c0; t.SetPosition(a3.a); cylinder(t); } { gp_Cylinder t = c0; t.SetAxis(nx.a); cylinder(t); }
      { gp_Cylinder t = c0; t.SetRadius(nr); cylinder(t); }
      { gp_Cylinder t = c0; t.UReverse(); cylinder(t); bl(t.Direct()); } { gp_Cylinder t = c0; t.VReverse(); cylinder(t); bl(t.Direct()); }
      finish(); }
    { RCy k = rcyl(n);
      begin("cylinder_coefficients", "cylinder"); pcyl(k); bar();
      double c[10]; k.c.Coefficients(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]); for (double v : c) h(v); finish(); }
    { RCy k = rcyl(n); MOTION_EXACT("cylinder", "cylinder", pcyl(k), k.c, cylinder) }
    { RCy k = rcyl(n); MOTION_ROTATE("cylinder", "cylinder", pcyl(k), k.c, cylinder) }

    // ================= gp_Cone ================================================
    { RCo k = rcone(n); RA3 a1 = ra3m(n + 1);
      begin("cone_constructors", "cone ax3"); pcone(k); pa3(a1); bar();
      cone(gp_Cone()); cone(k.c); cone(gp_Cone(a1.a, k.ang, k.r)); finish(); }
    { // invalid radius or semi-angle: w 0 constructor(angle, radius), 1 SetSemiAngle(angle) on (0.5, 1), 2 SetRadius(radius) on (0.5, 1)
      static const int    ws[16]   = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2};
      static const double angs[16] = {0.0, -0.0, M_PI * 0.5, -M_PI * 0.5, 2.0, 1e-310, 1e-300, 1.5707963267948963, 0.5, 0.5, 0.0, M_PI * 0.5, -1.2, 1e-300, 0.3, 0.3};
      static const double rads[16] = {1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, -0.5, -0.0, 0.7, 0.7, 0.7, 0.7, -1e-9, 0.0};
      RA3 a1 = ra3m(n); int w = ws[n]; double ang = angs[n], rad = rads[n];
      begin("cone_checks", "ax3 f f i"); pa3(a1); h(ang); h(rad); in(w); bar();
      GUARD(if (w == 0) cone(gp_Cone(a1.a, ang, rad));
            else if (w == 1) { gp_Cone t(a1.a, 0.5, 1.0); t.SetSemiAngle(ang); cone(t); }
            else { gp_Cone t(a1.a, 0.5, 1.0); t.SetRadius(rad); cone(t); })
      finish(); }
    { RCo k = rcone(n); gp_Pnt nl = rp(); RA3 a3 = ra3m(n + 2); RA1 nx = ra1(); double nr = rnd(0, 5), na = rnz(0.05, 1.5);
      begin("cone_access", "cone pnt ax3 ax1 f f"); pcone(k); pnt(nl); pa3(a3); pa1(nx); h(nr); h(na); bar();
      const gp_Cone& c0 = k.c; pnt(c0.Location()); h(c0.RefRadius()); h(c0.SemiAngle()); ax3(c0.Position()); axes(c0.Axis(), c0.XAxis(), c0.YAxis()); bl(c0.Direct());
      { gp_Cone t = c0; t.SetLocation(nl); cone(t); } { gp_Cone t = c0; t.SetPosition(a3.a); cone(t); } { gp_Cone t = c0; t.SetAxis(nx.a); cone(t); }
      { gp_Cone t = c0; t.SetRadius(nr); cone(t); } { gp_Cone t = c0; t.SetSemiAngle(na); cone(t); }
      { gp_Cone t = c0; t.UReverse(); cone(t); bl(t.Direct()); } { gp_Cone t = c0; t.VReverse(); cone(t); bl(t.Direct()); }
      finish(); }
    { RCo k = rcone(n);
      begin("cone_apex", "cone"); pcone(k); bar();
      pnt(k.c.Apex()); { gp_Cone t = k.c; t.VReverse(); pnt(t.Apex()); } finish(); }
    { RCo k = rcone(n);
      // keep tan() moderate: near pi/2 the coefficients reach 1e32 and would hide the other values of the line
      if (std::abs(k.ang) > 1.4) { k.ang = std::copysign(1.1, k.ang); k.c = gp_Cone(k.f.a, k.ang, k.r); }
      begin("cone_coefficients", "cone"); pcone(k); bar();
      double c[10]; k.c.Coefficients(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]); for (double v : c) h(v); finish(); }
    { RCo k = rcone(n); MOTION_EXACT("cone", "cone", pcone(k), k.c, cone) }
    { RCo k = rcone(n); MOTION_ROTATE("cone", "cone", pcone(k), k.c, cone) }

    // ================= gp_Sphere ==============================================
    { RS k = rsph(n); RA3 a1 = ra3m(n + 1);
      begin("sphere_constructors", "sphere ax3"); psph(k); pa3(a1); bar();
      sphere(gp_Sphere()); sphere(k.c); sphere(gp_Sphere(a1.a, k.r)); finish(); }
    { double rneg = RADII[n]; RA3 a1 = ra3m(n); int w = n % 2;
      begin("sphere_radius_checks", "ax3 f i"); pa3(a1); h(rneg); in(w); bar();
      GUARD(if (w == 0) sphere(gp_Sphere(a1.a, rneg)); else { gp_Sphere t(a1.a, 1.0); t.SetRadius(rneg); sphere(t); })
      finish(); }
    { RS k = rsph(n); gp_Pnt nl = rp(); RA3 a3 = ra3m(n + 2); double nr = rnd(0, 5);
      begin("sphere_access", "sphere pnt ax3 f"); psph(k); pnt(nl); pa3(a3); h(nr); bar();
      const gp_Sphere& c0 = k.c; pnt(c0.Location()); h(c0.Radius()); ax3(c0.Position()); ax1(c0.XAxis()); ax1(c0.YAxis()); bl(c0.Direct());
      h(c0.Area()); h(c0.Volume());
      { gp_Sphere t = c0; t.SetLocation(nl); sphere(t); } { gp_Sphere t = c0; t.SetPosition(a3.a); sphere(t); } { gp_Sphere t = c0; t.SetRadius(nr); sphere(t); }
      { gp_Sphere t = c0; t.UReverse(); sphere(t); bl(t.Direct()); } { gp_Sphere t = c0; t.VReverse(); sphere(t); bl(t.Direct()); }
      finish(); }
    { RS k = rsph(n);
      begin("sphere_coefficients", "sphere"); psph(k); bar();
      double c[10]; k.c.Coefficients(c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9]); for (double v : c) h(v); finish(); }
    { RS k = rsph(n); MOTION_EXACT("sphere", "sphere", psph(k), k.c, sphere) }
    { RS k = rsph(n); MOTION_ROTATE("sphere", "sphere", psph(k), k.c, sphere) }

    // ================= gp_Torus ===============================================
    { RT k = rtorus(n); RA3 a1 = ra3m(n + 1);
      begin("torus_constructors", "torus ax3"); ptorus(k); pa3(a1); bar();
      torus(gp_Torus()); torus(k.c); torus(gp_Torus(a1.a, k.a, k.b)); finish(); }
    { // constructor: any non-negative radii (minor > major allowed, zeros allowed), negative ones raise;
      // SetMajorRadius on (3, 1) needs major - 1 > resolution, SetMinorRadius on (3, 1) needs 0 <= minor and 3 - minor > resolution
      static const int    ws[16]  = {0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2};
      static const double mjs[16] = {0.0, 2.0, -1.0, 3.0, 1.0, 5.0, 0.5, 1.0, 1.0000000000000002, 7.0, -2.0, 2.0, 2.0, 2.0, 2.0, 2.0};
      static const double mns[16] = {0.0, 3.0, 1.0, -0.5, -0.0, 0.0, 0.3, 0.3, 0.3, 0.3, 0.3, 3.0, 2.9999999999999996, -0.5, -0.0, 0.0};
      RA3 a1 = ra3m(n); int w = ws[n]; double mj = mjs[n], mn = mns[n];
      begin("torus_radius_checks", "ax3 f f i"); pa3(a1); h(mj); h(mn); in(w); bar();
      GUARD(if (w == 0) torus(gp_Torus(a1.a, mj, mn));
            else if (w == 1) { gp_Torus t(a1.a, 3.0, 1.0); t.SetMajorRadius(mj); torus(t); }
            else { gp_Torus t(a1.a, 3.0, 1.0); t.SetMinorRadius(mn); torus(t); })
      finish(); }
    { RT k = rtorus(n); gp_Pnt nl = rp(); RA3 a3 = ra3m(n + 2); RA1 nx = ra1(); double nmaj = k.b + rnd(0.1, 3), nmin = k.a * rnd(0, 0.9);
      begin("torus_access", "torus pnt ax3 ax1 f f"); ptorus(k); pnt(nl); pa3(a3); pa1(nx); h(nmaj); h(nmin); bar();
      const gp_Torus& c0 = k.c; pnt(c0.Location()); h(c0.MajorRadius()); h(c0.MinorRadius()); ax3(c0.Position()); axes(c0.Axis(), c0.XAxis(), c0.YAxis()); bl(c0.Direct());
      h(c0.Area()); h(c0.Volume());
      { gp_Torus t = c0; t.SetLocation(nl); torus(t); } { gp_Torus t = c0; t.SetPosition(a3.a); torus(t); } { gp_Torus t = c0; t.SetAxis(nx.a); torus(t); }
      { gp_Torus t = c0; t.SetMajorRadius(nmaj); torus(t); } { gp_Torus t = c0; t.SetMinorRadius(nmin); torus(t); }
      { gp_Torus t = c0; t.UReverse(); torus(t); bl(t.Direct()); } { gp_Torus t = c0; t.VReverse(); torus(t); bl(t.Direct()); }
      finish(); }
    { RT k = rtorus(n);
      begin("torus_coefficients", "torus"); ptorus(k); bar();
      NCollection_Array1<double> c(1, 35); k.c.Coefficients(c); for (int i = 1; i <= 35; ++i) h(c(i)); finish(); }
    { RT k = rtorus(n); MOTION_EXACT("torus", "torus", ptorus(k), k.c, torus) }
    { RT k = rtorus(n); MOTION_ROTATE("torus", "torus", ptorus(k), k.c, torus) }
  }
  return 0;
}
