// Deterministic reference calls for OCCT 8.0.1's ElSLib (all of it) and ElCLib (everything the
// first ElCLib port, src/elclib.rs, did not have: 3D Value/D1/D2, the Ax1/Ax2-based overloads, every
// 2D function and the To3d family).
//
// Output format, one record per line:
//   <family> <inputs...> | <outputs...>
// f64 values are 16-hex-digit bit patterns, ints are decimal. Every input of a call is printed
// on the left of " | " in the order named by the family's schema string (`begin(name, schema)`);
// tests/elslib.rs reads them back with the reader method of the same name. Outputs are either
// values, `throws` (OCCT raised an exception) or `unchecked` (a check that OCCT 8.0.1 as built
// has compiled out would have failed; the numbers it returns are unspecified; the Rust API must
// return an error there). The oracle aborts if an output is NaN/inf that was not announced.
//
// Schema words (token counts): f=1 i=1 pnt=3 pnt2d=2 vec2d=2 (raw) dir2d=2 (raw, un-normalized)
// ax1=6 (pnt rawdir) ax2=9 (pnt rawN rawX) ax3=10 (pnt rawN rawX flag; flag 0 direct, 1 X
// reversed, 2 Y reversed) ax2d=4 (pnt2d rawdir2d) ax22d=6 (pnt2d rawX rawY: the frame is built
// with gp_Ax22d(P, Vx, Vy), Vy only selects the handedness)
// Curves: lin=6 (= ax1) circ=10 (ax2 radius) elips=11 (ax2 major minor) hypr=11 parab=10 (ax2 focal)
//   lin2d=4 (= ax2d) circ2d=7 (ax22d radius) elips2d=8 hypr2d=8 parab2d=7 (ax22d focal)
// Surfaces: pln=10 (= ax3) cylinder=11 (ax3 radius) cone=12 (ax3 semi-angle radius) sphere=11
//   torus=12 (ax3 major minor)
//
// The "Ax3 level" families of ElSLib take `ax3` followed by the shape parameters in the order of the
// C++ arguments (cylinder/sphere: radius; cone: radius, semi-angle; torus: major, minor) and then
// u v (or the point, or the derivative orders); they use ElSLib::PlaneValue, CylinderD1 ... .
#include <ElCLib.hxx>
#include <ElSLib.hxx>
#include <gp.hxx>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax22d.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Circ2d.hxx>
#include <gp_Cone.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Dir.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Elips.hxx>
#include <gp_Elips2d.hxx>
#include <gp_Hypr.hxx>
#include <gp_Hypr2d.hxx>
#include <gp_Lin.hxx>
#include <gp_Lin2d.hxx>
#include <gp_Parab.hxx>
#include <gp_Parab2d.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Sphere.hxx>
#include <gp_Torus.hxx>
#include <gp_Vec.hxx>
#include <gp_Vec2d.hxx>
#include <Standard_Failure.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

static const double PI = M_PI;

// ---------------------------------------------------------------------------------
// deterministic random numbers
static uint64_t state = 0x9E3779B97F4A7C15ull;
static double rnd(double lo, double hi) {
  state ^= state << 13; state ^= state >> 7; state ^= state << 17;
  return lo + (hi - lo) * (double)(state >> 11) / 9007199254740992.0;
}
static int rint(int lo, int hi) { int v = lo + (int)std::floor(rnd(0, 1) * (hi - lo + 1)); return v > hi ? hi : v; }

// ---------------------------------------------------------------------------------
// record buffer
static std::string buf;
static size_t mark = 0;
static int ntok = 0, want = -1;
static bool outphase = false;
static void h(double v) {
  if (outphase && !std::isfinite(v)) { std::fprintf(stderr, "non-finite output in %s\n", buf.c_str()); std::abort(); }
  uint64_t bits; std::memcpy(&bits, &v, 8); char t[24]; std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bits); buf += t; ++ntok;
}
static void in(long v) { buf += " " + std::to_string(v); ++ntok; }
static int schema_tokens(const char* sc) {
  int n = 0; std::string s(sc); size_t pos = 0;
  while (pos < s.size()) {
    size_t e = s.find(' ', pos); if (e == std::string::npos) e = s.size();
    std::string w = s.substr(pos, e - pos); pos = e + 1;
    if (w.empty()) continue;
    if (w == "f" || w == "i") n += 1;
    else if (w == "pnt2d" || w == "vec2d" || w == "dir2d") n += 2;
    else if (w == "pnt") n += 3;
    else if (w == "ax2d" || w == "lin2d") n += 4;
    else if (w == "ax1" || w == "lin" || w == "ax22d") n += 6;
    else if (w == "circ2d" || w == "parab2d") n += 7;
    else if (w == "elips2d" || w == "hypr2d") n += 8;
    else if (w == "ax2") n += 9;
    else if (w == "ax3" || w == "pln" || w == "circ" || w == "parab") n += 10;
    else if (w == "elips" || w == "hypr" || w == "cylinder" || w == "sphere") n += 11;
    else if (w == "cone" || w == "torus") n += 12;
    else { std::fprintf(stderr, "bad schema word %s\n", w.c_str()); std::abort(); }
  }
  return n;
}
static void begin(const std::string& name, const std::string& schema) {
  if (std::getenv("ELSLIB_SCHEMAS")) std::fprintf(stderr, "SCHEMA %s | %s\n", name.c_str(), schema.c_str());
  buf = name; ntok = 0; outphase = false; want = schema_tokens(schema.c_str()); }
static void bar() {
  if (want >= 0 && ntok != want) { std::fprintf(stderr, "schema mismatch in %s: printed %d expected %d\n", buf.c_str(), ntok, want); std::abort(); }
  buf += " |"; mark = buf.size(); outphase = true;
}
static void finish() { buf += "\n"; std::fputs(buf.c_str(), stdout); buf.clear(); outphase = false; }
#define GUARD(...) try { __VA_ARGS__ } catch (Standard_Failure&) { buf.resize(mark); buf += " throws"; }
#define UNCHECKED() do { buf.resize(mark); buf += " unchecked"; } while (0)

// ---------------------------------------------------------------------------------
// value printers (used for inputs and outputs alike)
static void pnt(const gp_Pnt& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void vec(const gp_Vec& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void dir(const gp_Dir& v) { h(v.X()); h(v.Y()); h(v.Z()); }
static void ax1(const gp_Ax1& a) { pnt(a.Location()); dir(a.Direction()); }
static void ax2(const gp_Ax2& a) { pnt(a.Location()); dir(a.Direction()); dir(a.XDirection()); dir(a.YDirection()); }
static void lin(const gp_Lin& l) { ax1(l.Position()); }
static void circ(const gp_Circ& c) { ax2(c.Position()); h(c.Radius()); }
static void elips(const gp_Elips& e) { ax2(e.Position()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void hypr(const gp_Hypr& e) { ax2(e.Position()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void parab(const gp_Parab& e) { ax2(e.Position()); h(e.Focal()); }
static void pnt2(const gp_Pnt2d& v) { h(v.X()); h(v.Y()); }
static void vec2(const gp_Vec2d& v) { h(v.X()); h(v.Y()); }
static void dir2(const gp_Dir2d& v) { h(v.X()); h(v.Y()); }
static void ax2d(const gp_Ax2d& a) { pnt2(a.Location()); dir2(a.Direction()); }
static void ax22d(const gp_Ax22d& a) { pnt2(a.Location()); dir2(a.XDirection()); dir2(a.YDirection()); }
static void lin2(const gp_Lin2d& l) { ax2d(l.Position()); }
static void circ2(const gp_Circ2d& c) { ax22d(c.Position()); h(c.Radius()); }
static void elips2(const gp_Elips2d& e) { ax22d(e.Axis()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void hypr2(const gp_Hypr2d& e) { ax22d(e.Axis()); h(e.MajorRadius()); h(e.MinorRadius()); }
static void parab2(const gp_Parab2d& e) { ax22d(e.Axis()); h(e.Focal()); }

// ---------------------------------------------------------------------------------
// input objects. A "raw" direction is the un-normalized triple (pair) that is printed; the oracle
// and the test both build the gp_Dir (gp_Dir2d) from it.
struct Raw { double x, y, z; };
struct Raw2 { double x, y; };
static void rw(const Raw& r) { h(r.x); h(r.y); h(r.z); }
static void rw2(const Raw2& r) { h(r.x); h(r.y); }
static gp_Dir D(const Raw& r) { return gp_Dir(r.x, r.y, r.z); }
static gp_Dir2d D2(const Raw2& r) { return gp_Dir2d(r.x, r.y); }
static Raw rraw() { for (;;) { Raw r{rnd(-4, 4), rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y + r.z * r.z > 0.25) return r; } }
static Raw2 rraw2() { for (;;) { Raw2 r{rnd(-4, 4), rnd(-4, 4)}; if (r.x * r.x + r.y * r.y > 0.25) return r; } }
static Raw cross3(const Raw& a, const Raw& b) { return Raw{a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x}; }
static double norm3(const Raw& a) { return std::sqrt(a.x * a.x + a.y * a.y + a.z * a.z); }
static gp_Pnt rp() { double x = rnd(-6, 6), y = rnd(-6, 6), z = rnd(-6, 6); return gp_Pnt(x, y, z); }
static gp_Pnt2d rp2() { double x = rnd(-6, 6), y = rnd(-6, 6); return gp_Pnt2d(x, y); }
// a pair of raw directions that are safely not parallel (so that gp_Ax2(P, N, Vx) is legal)
static void rpair(Raw& n, Raw& x) {
  for (;;) { n = rraw(); x = rraw(); if (norm3(cross3(n, x)) > 0.2 * norm3(n) * norm3(x)) return; }
}

struct RA1 { gp_Pnt p; Raw d; gp_Ax1 a; };
struct RA2 { gp_Pnt p; Raw n, x; gp_Ax2 a; };
struct RA3 { gp_Pnt p; Raw n, x; int f; gp_Ax3 a; };
static RA2 mkA2(const gp_Pnt& p, const Raw& n, const Raw& x) { RA2 r; r.p = p; r.n = n; r.x = x; r.a = gp_Ax2(p, D(n), D(x)); return r; }
static RA3 mkA3(const gp_Pnt& p, const Raw& n, const Raw& x, int f) {
  RA3 r; r.p = p; r.n = n; r.x = x; r.f = f; r.a = gp_Ax3(gp_Ax2(p, D(n), D(x)));
  if (f == 1) r.a.XReverse(); else if (f == 2) r.a.YReverse();
  return r;
}
static void pa2(const RA2& r) { pnt(r.p); rw(r.n); rw(r.x); }
static void pa3(const RA3& r) { pnt(r.p); rw(r.n); rw(r.x); in(r.f); }

// 2D frames: ax2d (point, raw direction), ax22d (point, raw X, raw Y)
struct RAx { gp_Pnt2d p; Raw2 d; gp_Ax2d a; };
static RAx mkAx(const gp_Pnt2d& p, const Raw2& d) { RAx r; r.p = p; r.d = d; r.a = gp_Ax2d(p, D2(d)); return r; }
static void pax(const RAx& r) { pnt2(r.p); rw2(r.d); }
struct RF { gp_Pnt2d p; Raw2 x, y; gp_Ax22d a; };
static RF mkF(const gp_Pnt2d& p, const Raw2& x, const Raw2& y) { RF r; r.p = p; r.x = x; r.y = y; r.a = gp_Ax22d(p, D2(x), D2(y)); return r; }
static void pf(const RF& r) { pnt2(r.p); rw2(r.x); rw2(r.y); }
// mode 0 random Y, 1 direct (Y = k * perp X), 2 indirect (Y = -k * perp X), 3 Y = X, 4 Y = -X
static RF rf(int mode) {
  gp_Pnt2d p = rp2(); Raw2 x = rraw2(); Raw2 y = rraw2(); double k = rnd(0.3, 2.0);
  switch (mode % 5) {
    case 1: y = Raw2{-x.y * k, x.x * k}; break;
    case 2: y = Raw2{x.y * k, -x.x * k}; break;
    case 3: y = x; break;
    case 4: y = Raw2{-x.x, -x.y}; break;
    default: break;
  }
  return mkF(p, x, y);
}

// ---------------------------------------------------------------------------------
// case schedules shared by all families: the first 12 iterations are edge cases
static const int N = 33;
static const double UE[12] = {0, PI / 2, PI, 3 * PI / 2, 2 * PI, -PI / 2, 5 * PI, -1e-17, 2 * PI + 1e-9, 100.5, -3 * PI, 1e-12};
static const double VE[12] = {0, PI / 2, -PI / 2, PI, 2 * PI, -PI, -3 * PI, PI / 2 + 1e-8, -PI / 2 - 1e-16, -77.25, 1e-17, 7.5};
static double sched_u(int n) { return n < 12 ? UE[n] : rnd(-8, 8); }
static double sched_v(int n) { return n < 12 ? VE[n] : rnd(-8, 8); }

// ================================== ElSLib ====================================
enum { PLN, CYL, CONE, SPH, TOR };
static const char* KN[5] = {"pln", "cylinder", "cone", "sphere", "torus"};
// shape words of the Ax3-level schemas
static const char* SHAPE[5] = {"", " f", " f f", " f", " f f"};

static RA3 frame3(int n) {
  switch (n) {
    case 0: return mkA3(gp_Pnt(0, 0, 0), Raw{0, 0, 1}, Raw{1, 0, 0}, 0);
    case 1: return mkA3(gp_Pnt(0, 0, 0), Raw{0, 0, 1}, Raw{1, 0, 0}, 1);
    case 2: return mkA3(gp_Pnt(0, 0, 0), Raw{0, 0, 1}, Raw{1, 0, 0}, 2);
    case 3: return mkA3(gp_Pnt(3, -2, 1), Raw{0, 1, 0}, Raw{1, 0, 0}, 0);
    case 4: return mkA3(gp_Pnt(0, 0, 0), Raw{0, 0, -1}, Raw{-1, 0, 0}, 0);
    case 5: return mkA3(gp_Pnt(-1.5, 2.25, 0.5), Raw{1, 1, 1}, Raw{1, -1, 0}, 1);
    default: { gp_Pnt p = rp(); Raw nn, x; rpair(nn, x); return mkA3(p, nn, x, n % 3); }
  }
}

struct Surf { int k; RA3 f; double r1, r2, u, v; int n; };
// r1: radius (cone: reference radius, torus: major), r2: cone semi-angle / torus minor.
// `object`: only values that the gp_* constructor accepts.
static Surf mkSurf(int k, int n, bool object) {
  Surf s; s.k = k; s.n = n;
  bool std0 = (k == CONE && (n == 7 || n == 8)) || (k == TOR && n == 7);
  s.f = frame3(std0 ? 0 : n);
  s.u = sched_u(n); s.v = sched_v(n);
  s.r1 = rnd(0.1, 20); s.r2 = 0;
  switch (k) {
    case CYL: case SPH:
      if (n == 0) s.r1 = 0; else if (n == 3) s.r1 = 1e-9; else if (n == 5) s.r1 = -2.5;
      break;
    case CONE: {
      static const double SAE[6] = {0, 0.5, -PI / 4, PI / 2, -PI / 2, 1e-9};
      s.r2 = n < 6 ? SAE[n] : rnd(-1.45, 1.45);
      if (n == 0) s.r1 = 0; else if (n == 5) s.r1 = -2.5; else if (n == 4) s.r1 = 3;
      if (n == 8) { s.r1 = 1; s.r2 = 0.7; }
      break;
    }
    case TOR: {
      s.r2 = s.r1 * rnd(0.05, 1);
      switch (n) {
        case 0: s.r1 = 2.5; s.r2 = 0; break;
        case 1: s.r1 = 1.5; s.r2 = 1.5; break;
        case 2: s.r1 = 1; s.r2 = 3; break;
        case 3: s.r1 = 0; s.r2 = 2; break;
        case 4: s.r1 = 2; s.r2 = -1; break;
        case 5: s.r1 = -2; s.r2 = 1; break;
        case 7: s.r1 = 0; s.r2 = 1.25; break;
        default: break;
      }
      break;
    }
    default: break;
  }
  if (object) {
    if (k == CYL || k == SPH || k == CONE) s.r1 = std::fabs(s.r1);
    if (k == TOR) { s.r1 = std::fabs(s.r1); s.r2 = std::fabs(s.r2); }
    if (k == CONE) {
      double a = std::fabs(s.r2);
      if (a <= gp::Resolution()) s.r2 = 1e-300;
      else if (PI / 2 - a <= gp::Resolution()) s.r2 = (s.r2 > 0 ? 1 : -1) * std::nextafter(PI / 2, 0.0);
    }
  }
  return s;
}
static void pshape(const Surf& s) {
  switch (s.k) {
    case CYL: case SPH: h(s.r1); break;
    case CONE: h(s.r1); h(s.r2); break;
    case TOR: h(s.r1); h(s.r2); break;
    default: break;
  }
}

static gp_Pnt local(const gp_Ax3& a, double x, double y, double z) {
  const gp_XYZ& X = a.XDirection().XYZ(); const gp_XYZ& Y = a.YDirection().XYZ(); const gp_XYZ& Z = a.Direction().XYZ();
  const gp_XYZ& L = a.Location().XYZ();
  return gp_Pnt(x * X.X() + y * Y.X() + z * Z.X() + L.X(), x * X.Y() + y * Y.Y() + z * Z.Y() + L.Y(), x * X.Z() + y * Y.Z() + z * Z.Z() + L.Z());
}

// ---- the Ax3-level calls, by kind ----
static gp_Pnt s_value(const Surf& s) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case PLN: return ElSLib::PlaneValue(s.u, s.v, a);
    case CYL: return ElSLib::CylinderValue(s.u, s.v, a, s.r1);
    case CONE: return ElSLib::ConeValue(s.u, s.v, a, s.r1, s.r2);
    case SPH: return ElSLib::SphereValue(s.u, s.v, a, s.r1);
    default: return ElSLib::TorusValue(s.u, s.v, a, s.r1, s.r2);
  }
}
static gp_Pnt s_d0(const Surf& s) {
  const gp_Ax3& a = s.f.a; gp_Pnt P;
  switch (s.k) {
    case PLN: ElSLib::PlaneD0(s.u, s.v, a, P); break;
    case CYL: ElSLib::CylinderD0(s.u, s.v, a, s.r1, P); break;
    case CONE: ElSLib::ConeD0(s.u, s.v, a, s.r1, s.r2, P); break;
    case SPH: ElSLib::SphereD0(s.u, s.v, a, s.r1, P); break;
    default: ElSLib::TorusD0(s.u, s.v, a, s.r1, s.r2, P); break;
  }
  return P;
}
static void s_d1(const Surf& s, gp_Pnt& P, gp_Vec& Vu, gp_Vec& Vv) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case PLN: ElSLib::PlaneD1(s.u, s.v, a, P, Vu, Vv); break;
    case CYL: ElSLib::CylinderD1(s.u, s.v, a, s.r1, P, Vu, Vv); break;
    case CONE: ElSLib::ConeD1(s.u, s.v, a, s.r1, s.r2, P, Vu, Vv); break;
    case SPH: ElSLib::SphereD1(s.u, s.v, a, s.r1, P, Vu, Vv); break;
    default: ElSLib::TorusD1(s.u, s.v, a, s.r1, s.r2, P, Vu, Vv); break;
  }
}
static void s_d2(const Surf& s, gp_Pnt& P, gp_Vec* V) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case CYL: ElSLib::CylinderD2(s.u, s.v, a, s.r1, P, V[0], V[1], V[2], V[3], V[4]); break;
    case CONE: ElSLib::ConeD2(s.u, s.v, a, s.r1, s.r2, P, V[0], V[1], V[2], V[3], V[4]); break;
    case SPH: ElSLib::SphereD2(s.u, s.v, a, s.r1, P, V[0], V[1], V[2], V[3], V[4]); break;
    default: ElSLib::TorusD2(s.u, s.v, a, s.r1, s.r2, P, V[0], V[1], V[2], V[3], V[4]); break;
  }
}
static void s_d3(const Surf& s, gp_Pnt& P, gp_Vec* V) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case CYL: ElSLib::CylinderD3(s.u, s.v, a, s.r1, P, V[0], V[1], V[2], V[3], V[4], V[5], V[6], V[7], V[8]); break;
    case CONE: ElSLib::ConeD3(s.u, s.v, a, s.r1, s.r2, P, V[0], V[1], V[2], V[3], V[4], V[5], V[6], V[7], V[8]); break;
    case SPH: ElSLib::SphereD3(s.u, s.v, a, s.r1, P, V[0], V[1], V[2], V[3], V[4], V[5], V[6], V[7], V[8]); break;
    default: ElSLib::TorusD3(s.u, s.v, a, s.r1, s.r2, P, V[0], V[1], V[2], V[3], V[4], V[5], V[6], V[7], V[8]); break;
  }
}
static gp_Vec s_dn(const Surf& s, int nu, int nv) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case PLN: return ElSLib::PlaneDN(s.u, s.v, a, nu, nv);
    case CYL: return ElSLib::CylinderDN(s.u, s.v, a, s.r1, nu, nv);
    case CONE: return ElSLib::ConeDN(s.u, s.v, a, s.r1, s.r2, nu, nv);
    case SPH: return ElSLib::SphereDN(s.u, s.v, a, s.r1, nu, nv);
    default: return ElSLib::TorusDN(s.u, s.v, a, s.r1, s.r2, nu, nv);
  }
}
static void s_params(const Surf& s, const gp_Pnt& P, double& U, double& V) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case PLN: ElSLib::PlaneParameters(a, P, U, V); break;
    case CYL: ElSLib::CylinderParameters(a, s.r1, P, U, V); break;
    case CONE: ElSLib::ConeParameters(a, s.r1, s.r2, P, U, V); break;
    case SPH: ElSLib::SphereParameters(a, s.r1, P, U, V); break;
    default: ElSLib::TorusParameters(a, s.r1, s.r2, P, U, V); break;
  }
}

// the point of the `parameters` families
static gp_Pnt ppoint(int n, const Surf& s) {
  const gp_Ax3& a = s.f.a;
  if (s.k == CONE && n == 7) return local(a, 0, 0, -s.r1 / std::tan(s.r2));
  if (s.k == CONE && n == 8) return local(a, 2, 1, -9);
  if (s.k == TOR && n == 7) return local(a, 0, 0, 0);
  switch (n) {
    case 0: return local(a, 0, 0, 0);
    case 1: return local(a, 0, 0, 5);
    case 2: return local(a, 0, 0, -5);
    case 3: return local(a, -3, 0, 0);
    case 4: return local(a, 3, -1e-20, 0);
    case 5: return local(a, 3, -1e-15, 1);
    case 6: return local(a, -3, -0.0, 1);
    default: break;
  }
  if (n < 22) {
    gp_Pnt p = s_value(s);
    if (n % 4 == 0) p = gp_Pnt(p.X() + rnd(-1e-3, 1e-3), p.Y() + rnd(-1e-3, 1e-3), p.Z() + rnd(-1e-3, 1e-3));
    return p;
  }
  return rp();
}

// Iso curves: `unchecked` is never needed (see docs): the gp_Circ radius check is compiled out
// of ElSLib.cxx, a negative radius is stored as it is.
static void s_uiso(const Surf& s) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case PLN: lin(ElSLib::PlaneUIso(a, s.u)); break;
    case CYL: lin(ElSLib::CylinderUIso(a, s.r1, s.u)); break;
    case CONE: lin(ElSLib::ConeUIso(a, s.r1, s.r2, s.u)); break;
    case SPH: circ(ElSLib::SphereUIso(a, s.r1, s.u)); break;
    default: circ(ElSLib::TorusUIso(a, s.r1, s.r2, s.u)); break;
  }
}
static void s_viso(const Surf& s) {
  const gp_Ax3& a = s.f.a;
  switch (s.k) {
    case PLN: lin(ElSLib::PlaneVIso(a, s.v)); break;
    case CYL: circ(ElSLib::CylinderVIso(a, s.r1, s.v)); break;
    case CONE: circ(ElSLib::ConeVIso(a, s.r1, s.r2, s.v)); break;
    case SPH: circ(ElSLib::SphereVIso(a, s.r1, s.v)); break;
    default: circ(ElSLib::TorusVIso(a, s.r1, s.r2, s.v)); break;
  }
}

// ---- gp-object overloads ----
static void pobj(const Surf& s) {
  pa3(s.f);
  switch (s.k) {
    case CYL: case SPH: h(s.r1); break;
    case CONE: h(s.r2); h(s.r1); break; // gp_Cone(A3, semi-angle, radius)
    case TOR: h(s.r1); h(s.r2); break;
    default: break;
  }
}
static void object_family(const Surf& s, int nu, int nv, const gp_Pnt& P) {
  const gp_Ax3& a = s.f.a;
  double U, V;
  gp_Pnt p0, q; gp_Vec V1, V2, V3, V4, V5, V6, V7, V8, V9;
  switch (s.k) {
    case PLN: {
      gp_Pln o(a);
      pnt(ElSLib::Value(s.u, s.v, o)); ElSLib::D0(s.u, s.v, o, p0); pnt(p0);
      ElSLib::D1(s.u, s.v, o, q, V1, V2); pnt(q); vec(V1); vec(V2);
      vec(ElSLib::DN(s.u, s.v, o, nu, nv));
      ElSLib::Parameters(o, P, U, V); h(U); h(V);
      break;
    }
    case CYL: {
      gp_Cylinder o(a, s.r1);
      pnt(ElSLib::Value(s.u, s.v, o)); ElSLib::D0(s.u, s.v, o, p0); pnt(p0);
      ElSLib::D1(s.u, s.v, o, q, V1, V2); pnt(q); vec(V1); vec(V2);
      ElSLib::D2(s.u, s.v, o, q, V1, V2, V3, V4, V5); pnt(q); vec(V1); vec(V2); vec(V3); vec(V4); vec(V5);
      ElSLib::D3(s.u, s.v, o, q, V1, V2, V3, V4, V5, V6, V7, V8, V9); pnt(q);
      vec(V1); vec(V2); vec(V3); vec(V4); vec(V5); vec(V6); vec(V7); vec(V8); vec(V9);
      vec(ElSLib::DN(s.u, s.v, o, nu, nv));
      ElSLib::Parameters(o, P, U, V); h(U); h(V);
      break;
    }
    case CONE: {
      gp_Cone o(a, s.r2, s.r1);
      pnt(ElSLib::Value(s.u, s.v, o)); ElSLib::D0(s.u, s.v, o, p0); pnt(p0);
      ElSLib::D1(s.u, s.v, o, q, V1, V2); pnt(q); vec(V1); vec(V2);
      ElSLib::D2(s.u, s.v, o, q, V1, V2, V3, V4, V5); pnt(q); vec(V1); vec(V2); vec(V3); vec(V4); vec(V5);
      ElSLib::D3(s.u, s.v, o, q, V1, V2, V3, V4, V5, V6, V7, V8, V9); pnt(q);
      vec(V1); vec(V2); vec(V3); vec(V4); vec(V5); vec(V6); vec(V7); vec(V8); vec(V9);
      vec(ElSLib::DN(s.u, s.v, o, nu, nv));
      ElSLib::Parameters(o, P, U, V); h(U); h(V);
      break;
    }
    case SPH: {
      gp_Sphere o(a, s.r1);
      pnt(ElSLib::Value(s.u, s.v, o)); ElSLib::D0(s.u, s.v, o, p0); pnt(p0);
      ElSLib::D1(s.u, s.v, o, q, V1, V2); pnt(q); vec(V1); vec(V2);
      ElSLib::D2(s.u, s.v, o, q, V1, V2, V3, V4, V5); pnt(q); vec(V1); vec(V2); vec(V3); vec(V4); vec(V5);
      ElSLib::D3(s.u, s.v, o, q, V1, V2, V3, V4, V5, V6, V7, V8, V9); pnt(q);
      vec(V1); vec(V2); vec(V3); vec(V4); vec(V5); vec(V6); vec(V7); vec(V8); vec(V9);
      vec(ElSLib::DN(s.u, s.v, o, nu, nv));
      ElSLib::Parameters(o, P, U, V); h(U); h(V);
      break;
    }
    default: {
      gp_Torus o(a, s.r1, s.r2);
      pnt(ElSLib::Value(s.u, s.v, o)); ElSLib::D0(s.u, s.v, o, p0); pnt(p0);
      ElSLib::D1(s.u, s.v, o, q, V1, V2); pnt(q); vec(V1); vec(V2);
      ElSLib::D2(s.u, s.v, o, q, V1, V2, V3, V4, V5); pnt(q); vec(V1); vec(V2); vec(V3); vec(V4); vec(V5);
      ElSLib::D3(s.u, s.v, o, q, V1, V2, V3, V4, V5, V6, V7, V8, V9); pnt(q);
      vec(V1); vec(V2); vec(V3); vec(V4); vec(V5); vec(V6); vec(V7); vec(V8); vec(V9);
      vec(ElSLib::DN(s.u, s.v, o, nu, nv));
      ElSLib::Parameters(o, P, U, V); h(U); h(V);
      break;
    }
  }
}

static void dn_orders(int n, int& nu, int& nv) {
  static const int X[8][2] = {{-1, 2}, {2, -1}, {0, 0}, {6, 0}, {0, 6}, {7, 3}, {3, 7}, {-3, -3}};
  if (n < 25) { nu = n % 5; nv = (n / 5) % 5; }
  else { nu = X[(n - 25) % 8][0]; nv = X[(n - 25) % 8][1]; }
}

static void elslib_families() {
  for (int k = 0; k < 5; ++k) {
    std::string kn = KN[k];
    for (int n = 0; n < N; ++n) {
      Surf s = mkSurf(k, n, false);
      std::string head = std::string("ax3") + SHAPE[k];
      begin("elslib_" + kn + "_value", head + " f f"); pa3(s.f); pshape(s); h(s.u); h(s.v); bar();
      pnt(s_value(s)); pnt(s_d0(s)); finish();
      begin("elslib_" + kn + "_d1", head + " f f"); pa3(s.f); pshape(s); h(s.u); h(s.v); bar();
      { gp_Pnt P; gp_Vec a, b; s_d1(s, P, a, b); pnt(P); vec(a); vec(b); } finish();
      if (k != PLN) {
        begin("elslib_" + kn + "_d2", head + " f f"); pa3(s.f); pshape(s); h(s.u); h(s.v); bar();
        { gp_Pnt P; gp_Vec V[9]; s_d2(s, P, V); pnt(P); for (int i = 0; i < 5; ++i) vec(V[i]); } finish();
        begin("elslib_" + kn + "_d3", head + " f f"); pa3(s.f); pshape(s); h(s.u); h(s.v); bar();
        { gp_Pnt P; gp_Vec V[9]; s_d3(s, P, V); pnt(P); for (int i = 0; i < 9; ++i) vec(V[i]); } finish();
      }
      { int nu, nv; dn_orders(n, nu, nv);
        begin("elslib_" + kn + "_dn", head + " f f i i"); pa3(s.f); pshape(s); h(s.u); h(s.v); in(nu); in(nv); bar();
        vec(s_dn(s, nu, nv)); finish(); }
      { gp_Pnt P = ppoint(n, s);
        begin("elslib_" + kn + "_parameters", head + " pnt"); pa3(s.f); pshape(s); pnt(P); bar();
        double U, V; s_params(s, P, U, V); h(U); h(V); finish(); }
      begin("elslib_" + kn + "_uiso", head + " f"); pa3(s.f); pshape(s); h(s.u); bar(); s_uiso(s); finish();
      begin("elslib_" + kn + "_viso", head + " f"); pa3(s.f); pshape(s); h(s.v); bar(); s_viso(s); finish();
      // the gp-object overloads
      { Surf o = mkSurf(k, n, true);
        gp_Pnt P = ppoint(n, o);
        int nu, nv; dn_orders(n, nu, nv);
        begin("elslib_" + kn + "_object", kn + " f f i i pnt"); pobj(o); h(o.u); h(o.v); in(nu); in(nv); pnt(P); bar();
        object_family(o, nu, nv, P); finish(); }
    }
  }
}

// ================================== ElCLib ====================================
enum { LIN, CIRC, ELIPS, HYPR, PARAB };
static const char* CN[5] = {"lin", "circ", "elips", "hypr", "parab"};
static double sched_cu(int n) { return n < 12 ? UE[n] : rnd(-8, 8); }

static RA2 frame2(int n) {
  switch (n) {
    case 0: return mkA2(gp_Pnt(0, 0, 0), Raw{0, 0, 1}, Raw{1, 0, 0});
    case 1: return mkA2(gp_Pnt(0, 0, 0), Raw{0, 0, -1}, Raw{1, 0, 0});
    case 2: return mkA2(gp_Pnt(1, 2, 3), Raw{1, 0, 0}, Raw{0, 1, 0});
    case 3: return mkA2(gp_Pnt(-1.5, 2.25, 0.5), Raw{1, 1, 1}, Raw{1, -1, 0});
    default: { gp_Pnt p = rp(); Raw nn, x; rpair(nn, x); return mkA2(p, nn, x); }
  }
}

struct Cv3 { int k; RA2 f; double u, a, b; gp_Lin l; gp_Circ c; gp_Elips e; gp_Hypr hy; gp_Parab pa; };
static Cv3 mkCv3(int k, int n, bool forparam) {
  Cv3 c; c.k = k; c.f = frame2(n); c.u = sched_cu(n); c.a = 0; c.b = 0;
  switch (k) {
    case CIRC: c.a = rnd(0.1, 20); if (n == 0) c.a = 0; else if (n == 1) c.a = 1e-12; break;
    case ELIPS:
      c.a = rnd(0.5, 20); c.b = c.a * rnd(0.05, 1);
      if (n == 0) c.b = c.a; else if (n == 1) c.b = 0; else if (n == 2) c.a = c.b = 0;
      if (forparam && c.b <= 0) { c.a = 2; c.b = 1; }
      break;
    case HYPR:
      c.a = rnd(0.1, 10); c.b = rnd(0.1, 10);
      if (n == 0) c.b = 0; else if (n == 1) c.a = 0; else if (n == 2) c.b = c.a;
      if (forparam && c.b <= 0) c.b = 0.7;
      break;
    case PARAB:
      c.a = rnd(0.05, 10);
      if (n == 0) c.a = 0; else if (n == 1) c.a = gp::Resolution(); else if (n == 2) c.a = std::nextafter(gp::Resolution(), 1.0);
      break;
    default: break;
  }
  const gp_Ax2& ax = c.f.a;
  switch (k) {
    case LIN: c.l = gp_Lin(gp_Ax1(c.f.p, D(c.f.n))); break;
    case CIRC: c.c = gp_Circ(ax, c.a); break;
    case ELIPS: c.e = gp_Elips(ax, c.a, c.b); break;
    case HYPR: c.hy = gp_Hypr(ax, c.a, c.b); break;
    default: c.pa = gp_Parab(ax, c.a); break;
  }
  return c;
}
static void pcv3(const Cv3& c) {
  if (c.k == LIN) { pnt(c.f.p); rw(c.f.n); return; }
  pa2(c.f);
  if (c.k == ELIPS || c.k == HYPR) { h(c.a); h(c.b); } else h(c.a);
}
static void eval3(const Cv3& c) {
  const double u = c.u; gp_Pnt P; gp_Vec V1, V2, V3;
  switch (c.k) {
    case LIN:
      pnt(ElCLib::Value(u, c.l)); pnt(ElCLib::LineValue(u, c.l.Position()));
      ElCLib::D1(u, c.l, P, V1); pnt(P); vec(V1);
      ElCLib::LineD1(u, c.l.Position(), P, V1); pnt(P); vec(V1);
      break;
    case CIRC:
      pnt(ElCLib::Value(u, c.c)); pnt(ElCLib::CircleValue(u, c.c.Position(), c.c.Radius()));
      ElCLib::D1(u, c.c, P, V1); pnt(P); vec(V1);
      ElCLib::CircleD1(u, c.c.Position(), c.c.Radius(), P, V1); pnt(P); vec(V1);
      ElCLib::D2(u, c.c, P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::CircleD2(u, c.c.Position(), c.c.Radius(), P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::D3(u, c.c, P, V1, V2, V3); pnt(P); vec(V1); vec(V2); vec(V3);
      ElCLib::CircleD3(u, c.c.Position(), c.c.Radius(), P, V1, V2, V3); pnt(P); vec(V1); vec(V2); vec(V3);
      break;
    case ELIPS:
      pnt(ElCLib::Value(u, c.e)); pnt(ElCLib::EllipseValue(u, c.e.Position(), c.e.MajorRadius(), c.e.MinorRadius()));
      ElCLib::D1(u, c.e, P, V1); pnt(P); vec(V1);
      ElCLib::EllipseD1(u, c.e.Position(), c.e.MajorRadius(), c.e.MinorRadius(), P, V1); pnt(P); vec(V1);
      ElCLib::D2(u, c.e, P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::EllipseD2(u, c.e.Position(), c.e.MajorRadius(), c.e.MinorRadius(), P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::D3(u, c.e, P, V1, V2, V3); pnt(P); vec(V1); vec(V2); vec(V3);
      ElCLib::EllipseD3(u, c.e.Position(), c.e.MajorRadius(), c.e.MinorRadius(), P, V1, V2, V3); pnt(P); vec(V1); vec(V2); vec(V3);
      break;
    case HYPR:
      pnt(ElCLib::Value(u, c.hy)); pnt(ElCLib::HyperbolaValue(u, c.hy.Position(), c.hy.MajorRadius(), c.hy.MinorRadius()));
      ElCLib::D1(u, c.hy, P, V1); pnt(P); vec(V1);
      ElCLib::HyperbolaD1(u, c.hy.Position(), c.hy.MajorRadius(), c.hy.MinorRadius(), P, V1); pnt(P); vec(V1);
      ElCLib::D2(u, c.hy, P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::HyperbolaD2(u, c.hy.Position(), c.hy.MajorRadius(), c.hy.MinorRadius(), P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::D3(u, c.hy, P, V1, V2, V3); pnt(P); vec(V1); vec(V2); vec(V3);
      ElCLib::HyperbolaD3(u, c.hy.Position(), c.hy.MajorRadius(), c.hy.MinorRadius(), P, V1, V2, V3); pnt(P); vec(V1); vec(V2); vec(V3);
      break;
    default:
      pnt(ElCLib::Value(u, c.pa)); pnt(ElCLib::ParabolaValue(u, c.pa.Position(), c.pa.Focal()));
      ElCLib::D1(u, c.pa, P, V1); pnt(P); vec(V1);
      ElCLib::ParabolaD1(u, c.pa.Position(), c.pa.Focal(), P, V1); pnt(P); vec(V1);
      ElCLib::D2(u, c.pa, P, V1, V2); pnt(P); vec(V1); vec(V2);
      ElCLib::ParabolaD2(u, c.pa.Position(), c.pa.Focal(), P, V1, V2); pnt(P); vec(V1); vec(V2);
      break;
  }
}
static gp_Pnt value3(const Cv3& c, double u) {
  switch (c.k) {
    case LIN: return ElCLib::Value(u, c.l);
    case CIRC: return ElCLib::Value(u, c.c);
    case ELIPS: return ElCLib::Value(u, c.e);
    case HYPR: return ElCLib::Value(u, c.hy);
    default: return ElCLib::Value(u, c.pa);
  }
}
static void dn3(const Cv3& c, int nn) {
  const double u = c.u;
  switch (c.k) {
    case LIN: vec(ElCLib::DN(u, c.l, nn)); vec(ElCLib::LineDN(u, c.l.Position(), nn)); break;
    case CIRC: vec(ElCLib::DN(u, c.c, nn)); vec(ElCLib::CircleDN(u, c.c.Position(), c.c.Radius(), nn)); break;
    case ELIPS: vec(ElCLib::DN(u, c.e, nn)); vec(ElCLib::EllipseDN(u, c.e.Position(), c.e.MajorRadius(), c.e.MinorRadius(), nn)); break;
    case HYPR: vec(ElCLib::DN(u, c.hy, nn)); vec(ElCLib::HyperbolaDN(u, c.hy.Position(), c.hy.MajorRadius(), c.hy.MinorRadius(), nn)); break;
    default: vec(ElCLib::DN(u, c.pa, nn)); vec(ElCLib::ParabolaDN(u, c.pa.Position(), c.pa.Focal(), nn)); break;
  }
}
static void param3(const Cv3& c, const gp_Pnt& P) {
  switch (c.k) {
    case LIN: h(ElCLib::Parameter(c.l, P)); h(ElCLib::LineParameter(c.l.Position(), P)); break;
    case CIRC: h(ElCLib::Parameter(c.c, P)); h(ElCLib::CircleParameter(c.c.Position(), P)); break;
    case ELIPS: h(ElCLib::Parameter(c.e, P)); h(ElCLib::EllipseParameter(c.e.Position(), c.e.MajorRadius(), c.e.MinorRadius(), P)); break;
    case HYPR: h(ElCLib::Parameter(c.hy, P)); h(ElCLib::HyperbolaParameter(c.hy.Position(), c.hy.MajorRadius(), c.hy.MinorRadius(), P)); break;
    default: h(ElCLib::Parameter(c.pa, P)); h(ElCLib::ParabolaParameter(c.pa.Position(), P)); break;
  }
}
static gp_Pnt cpoint3(int n, const Cv3& c) {
  const gp_Ax2& a = c.f.a;
  const gp_XYZ& X = a.XDirection().XYZ(); const gp_XYZ& Y = a.YDirection().XYZ(); const gp_XYZ& Z = a.Direction().XYZ();
  const gp_XYZ& L = a.Location().XYZ();
  auto at = [&](double x, double y, double z) { return gp_Pnt(x * X.X() + y * Y.X() + z * Z.X() + L.X(), x * X.Y() + y * Y.Y() + z * Z.Y() + L.Y(), x * X.Z() + y * Y.Z() + z * Z.Z() + L.Z()); };
  switch (n) {
    case 0: return gp_Pnt(L);
    case 1: return at(1e-100, 0, 0); // tiny offset: only visible when the location is the origin
    case 2: return at(3, 0, 0);
    case 3: return at(-3, 0, 0);
    case 4: return at(0, 3, 0);
    case 5: return at(0, -3, 0);
    case 6: return at(3, -1e-15, 0);
    case 7: return at(0, 0, 2); // on the axis
    case 8: return at(1e-9, -1e-9, 0);
    default: break;
  }
  if (n % 2 == 1 && n < 24) return value3(c, c.u);
  return rp();
}

// ---- 2D ----
struct Cv2 { int k; RF f; RAx ln; double u, a, b; gp_Lin2d l; gp_Circ2d c; gp_Elips2d e; gp_Hypr2d hy; gp_Parab2d pa; };
static RF frame2d(int n) {
  switch (n) {
    case 0: return mkF(gp_Pnt2d(0, 0), Raw2{1, 0}, Raw2{0, 1});
    case 1: return mkF(gp_Pnt2d(0, 0), Raw2{1, 0}, Raw2{0, -1});
    case 2: return mkF(gp_Pnt2d(3, -2), Raw2{0, 1}, Raw2{-1, 0});
    case 3: return mkF(gp_Pnt2d(1, 2), Raw2{-1, -1}, Raw2{1, 1});
    default: return rf(n);
  }
}
static Cv2 mkCv2(int k, int n, bool forparam) {
  Cv2 c; c.k = k; c.f = frame2d(n); c.u = sched_cu(n); c.a = 0; c.b = 0;
  if (k == LIN) { gp_Pnt2d p = rp2(); Raw2 d = rraw2(); if (n == 0) { p = gp_Pnt2d(0, 0); d = Raw2{1, 0}; } c.ln = mkAx(p, d); c.l = gp_Lin2d(c.ln.a); }
  switch (k) {
    case CIRC: c.a = rnd(0.1, 20); if (n == 0) c.a = 0; else if (n == 1) c.a = 1e-12; break;
    case ELIPS:
      c.a = rnd(0.5, 20); c.b = c.a * rnd(0.05, 1);
      if (n == 0) c.b = c.a; else if (n == 1) c.b = 0; else if (n == 2) c.a = c.b = 0;
      if (forparam && c.b <= 0) { c.a = 2; c.b = 1; }
      break;
    case HYPR:
      c.a = rnd(0.1, 10); c.b = rnd(0.1, 10);
      if (n == 0) c.b = 0; else if (n == 1) c.a = 0; else if (n == 2) c.b = c.a;
      if (forparam && c.b <= 0) c.b = 0.7;
      break;
    case PARAB:
      c.a = rnd(0.05, 10);
      if (n == 0) c.a = 0; else if (n == 1) c.a = gp::Resolution(); else if (n == 2) c.a = std::nextafter(gp::Resolution(), 1.0);
      break;
    default: break;
  }
  switch (k) {
    case CIRC: c.c = gp_Circ2d(c.f.a, c.a); break;
    case ELIPS: c.e = gp_Elips2d(c.f.a, c.a, c.b); break;
    case HYPR: c.hy = gp_Hypr2d(c.f.a, c.a, c.b); break;
    case PARAB: c.pa = gp_Parab2d(c.f.a, c.a); break;
    default: break;
  }
  return c;
}
static void pcv2(const Cv2& c) {
  if (c.k == LIN) { pax(c.ln); return; }
  pf(c.f);
  if (c.k == ELIPS || c.k == HYPR) { h(c.a); h(c.b); } else h(c.a);
}
static gp_Ax22d ax22_of(const Cv2& c) { return c.f.a; }
static void eval2(const Cv2& c) {
  const double u = c.u; gp_Pnt2d P; gp_Vec2d V1, V2, V3;
  switch (c.k) {
    case LIN:
      pnt2(ElCLib::Value(u, c.l)); pnt2(ElCLib::LineValue(u, c.l.Position()));
      ElCLib::D1(u, c.l, P, V1); pnt2(P); vec2(V1);
      ElCLib::LineD1(u, c.l.Position(), P, V1); pnt2(P); vec2(V1);
      break;
    case CIRC:
      pnt2(ElCLib::Value(u, c.c)); pnt2(ElCLib::CircleValue(u, c.c.Position(), c.c.Radius()));
      ElCLib::D1(u, c.c, P, V1); pnt2(P); vec2(V1);
      ElCLib::CircleD1(u, c.c.Position(), c.c.Radius(), P, V1); pnt2(P); vec2(V1);
      ElCLib::D2(u, c.c, P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::CircleD2(u, c.c.Position(), c.c.Radius(), P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::D3(u, c.c, P, V1, V2, V3); pnt2(P); vec2(V1); vec2(V2); vec2(V3);
      ElCLib::CircleD3(u, c.c.Position(), c.c.Radius(), P, V1, V2, V3); pnt2(P); vec2(V1); vec2(V2); vec2(V3);
      break;
    case ELIPS:
      pnt2(ElCLib::Value(u, c.e)); pnt2(ElCLib::EllipseValue(u, c.e.Axis(), c.e.MajorRadius(), c.e.MinorRadius()));
      ElCLib::D1(u, c.e, P, V1); pnt2(P); vec2(V1);
      ElCLib::EllipseD1(u, c.e.Axis(), c.e.MajorRadius(), c.e.MinorRadius(), P, V1); pnt2(P); vec2(V1);
      ElCLib::D2(u, c.e, P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::EllipseD2(u, c.e.Axis(), c.e.MajorRadius(), c.e.MinorRadius(), P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::D3(u, c.e, P, V1, V2, V3); pnt2(P); vec2(V1); vec2(V2); vec2(V3);
      ElCLib::EllipseD3(u, c.e.Axis(), c.e.MajorRadius(), c.e.MinorRadius(), P, V1, V2, V3); pnt2(P); vec2(V1); vec2(V2); vec2(V3);
      break;
    case HYPR:
      pnt2(ElCLib::Value(u, c.hy)); pnt2(ElCLib::HyperbolaValue(u, c.hy.Axis(), c.hy.MajorRadius(), c.hy.MinorRadius()));
      ElCLib::D1(u, c.hy, P, V1); pnt2(P); vec2(V1);
      ElCLib::HyperbolaD1(u, c.hy.Axis(), c.hy.MajorRadius(), c.hy.MinorRadius(), P, V1); pnt2(P); vec2(V1);
      ElCLib::D2(u, c.hy, P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::HyperbolaD2(u, c.hy.Axis(), c.hy.MajorRadius(), c.hy.MinorRadius(), P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::D3(u, c.hy, P, V1, V2, V3); pnt2(P); vec2(V1); vec2(V2); vec2(V3);
      ElCLib::HyperbolaD3(u, c.hy.Axis(), c.hy.MajorRadius(), c.hy.MinorRadius(), P, V1, V2, V3); pnt2(P); vec2(V1); vec2(V2); vec2(V3);
      break;
    default:
      pnt2(ElCLib::Value(u, c.pa)); pnt2(ElCLib::ParabolaValue(u, c.pa.Axis(), c.pa.Focal()));
      ElCLib::D1(u, c.pa, P, V1); pnt2(P); vec2(V1);
      ElCLib::ParabolaD1(u, c.pa.Axis(), c.pa.Focal(), P, V1); pnt2(P); vec2(V1);
      ElCLib::D2(u, c.pa, P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      ElCLib::ParabolaD2(u, c.pa.Axis(), c.pa.Focal(), P, V1, V2); pnt2(P); vec2(V1); vec2(V2);
      break;
  }
}
static gp_Pnt2d value2(const Cv2& c, double u) {
  switch (c.k) {
    case LIN: return ElCLib::Value(u, c.l);
    case CIRC: return ElCLib::Value(u, c.c);
    case ELIPS: return ElCLib::Value(u, c.e);
    case HYPR: return ElCLib::Value(u, c.hy);
    default: return ElCLib::Value(u, c.pa);
  }
}
static void dn2(const Cv2& c, int nn) {
  const double u = c.u;
  switch (c.k) {
    case LIN: vec2(ElCLib::DN(u, c.l, nn)); vec2(ElCLib::LineDN(u, c.l.Position(), nn)); break;
    case CIRC: vec2(ElCLib::DN(u, c.c, nn)); vec2(ElCLib::CircleDN(u, c.c.Position(), c.c.Radius(), nn)); break;
    case ELIPS: vec2(ElCLib::DN(u, c.e, nn)); vec2(ElCLib::EllipseDN(u, c.e.Axis(), c.e.MajorRadius(), c.e.MinorRadius(), nn)); break;
    case HYPR: vec2(ElCLib::DN(u, c.hy, nn)); vec2(ElCLib::HyperbolaDN(u, c.hy.Axis(), c.hy.MajorRadius(), c.hy.MinorRadius(), nn)); break;
    default: vec2(ElCLib::DN(u, c.pa, nn)); vec2(ElCLib::ParabolaDN(u, c.pa.Axis(), c.pa.Focal(), nn)); break;
  }
}
// The 2D circle parameter builds a gp_Dir2d from the vector to the point; the zero-norm check of that
// constructor is compiled out of the library (the result is NaN), see UNCHECKED below.
static void param2(const Cv2& c, const gp_Pnt2d& P) {
  switch (c.k) {
    case LIN: h(ElCLib::Parameter(c.l, P)); h(ElCLib::LineParameter(c.l.Position(), P)); break;
    case CIRC: {
      double vx = P.X() - c.f.a.Location().X(), vy = P.Y() - c.f.a.Location().Y();
      if (std::sqrt(vx * vx + vy * vy) <= gp::Resolution()) { UNCHECKED(); break; }
      h(ElCLib::Parameter(c.c, P)); h(ElCLib::CircleParameter(c.c.Position(), P)); break;
    }
    case ELIPS: h(ElCLib::Parameter(c.e, P)); h(ElCLib::EllipseParameter(c.e.Axis(), c.e.MajorRadius(), c.e.MinorRadius(), P)); break;
    case HYPR: h(ElCLib::Parameter(c.hy, P)); h(ElCLib::HyperbolaParameter(c.hy.Axis(), c.hy.MajorRadius(), c.hy.MinorRadius(), P)); break;
    default: h(ElCLib::Parameter(c.pa, P)); h(ElCLib::ParabolaParameter(c.pa.Axis(), P)); break;
  }
}
static gp_Pnt2d cpoint2(int n, const Cv2& c) {
  const gp_Ax22d a = c.f.a;
  const gp_XY& X = a.XDirection().XY(); const gp_XY& Y = a.YDirection().XY(); const gp_XY& L = a.Location().XY();
  auto at = [&](double x, double y) { return gp_Pnt2d(x * X.X() + y * Y.X() + L.X(), x * X.Y() + y * Y.Y() + L.Y()); };
  switch (n) {
    case 0: return gp_Pnt2d(L);
    case 1: return at(1e-200, 0);
    case 2: return at(3, 0);
    case 3: return at(-3, 0);
    case 4: return at(0, 3);
    case 5: return at(0, -3);
    case 6: return at(3, -1e-15);
    case 7: return at(1e-8, 0);
    case 8: return at(1e-9, -1e-9);
    default: break;
  }
  if (n % 2 == 1 && n < 24) return value2(c, c.u);
  return rp2();
}

static int dnn(int n) { return n < 11 ? n - 2 : rint(-3, 9); }

static void elclib_families() {
  for (int k = 0; k < 5; ++k) {
    std::string cn = CN[k];
    for (int n = 0; n < N; ++n) {
      { Cv3 c = mkCv3(k, n, false);
        begin("elclib_" + cn + "_eval", cn + " f"); pcv3(c); h(c.u); bar(); eval3(c); finish(); }
      { Cv3 c = mkCv3(k, n, false); int nn = dnn(n);
        begin("elclib_" + cn + "_dn", cn + " f i"); pcv3(c); h(c.u); in(nn); bar(); dn3(c, nn); finish(); }
      { Cv3 c = mkCv3(k, n, true); gp_Pnt P = cpoint3(n, c);
        begin("elclib_" + cn + "_parameter", cn + " pnt"); pcv3(c); pnt(P); bar(); GUARD(param3(c, P);) finish(); }
    }
    std::string cn2 = cn + "2d";
    for (int n = 0; n < N; ++n) {
      { Cv2 c = mkCv2(k, n, false);
        begin("elclib_" + cn2 + "_eval", cn2 + " f"); pcv2(c); h(c.u); bar(); eval2(c); finish(); }
      { Cv2 c = mkCv2(k, n, false); int nn = dnn(n);
        begin("elclib_" + cn2 + "_dn", cn2 + " f i"); pcv2(c); h(c.u); in(nn); bar(); dn2(c, nn); finish(); }
      { Cv2 c = mkCv2(k, n, true); gp_Pnt2d P = cpoint2(n, c);
        begin("elclib_" + cn2 + "_parameter", cn2 + " pnt2d"); pcv2(c); pnt2(P); bar(); GUARD(param2(c, P);) finish(); }
    }
  }
}

// ---- To3d ----
static void to3d_families() {
  for (int n = 0; n < N; ++n) {
    RA2 f = frame2(n);
    const gp_Ax2& a = f.a;
    gp_Pnt2d p = rp2(); Raw2 r = rraw2();
    if (n == 0) { p = gp_Pnt2d(0, 0); r = Raw2{1, 0}; }
    if (n == 1) { p = gp_Pnt2d(1, 0); r = Raw2{0, -1}; }
    { begin("elclib_to3d_pnt", "ax2 pnt2d"); pa2(f); pnt2(p); bar(); pnt(ElCLib::To3d(a, p)); finish(); }
    { begin("elclib_to3d_vec", "ax2 vec2d"); pa2(f); rw2(r); bar(); vec(ElCLib::To3d(a, gp_Vec2d(r.x, r.y))); finish(); }
    { begin("elclib_to3d_dir", "ax2 dir2d"); pa2(f); rw2(r); bar(); dir(ElCLib::To3d(a, D2(r))); finish(); }
    { RAx l = mkAx(p, r);
      begin("elclib_to3d_ax2d", "ax2 ax2d"); pa2(f); pax(l); bar(); ax1(ElCLib::To3d(a, l.a)); finish();
      begin("elclib_to3d_lin", "ax2 lin2d"); pa2(f); pax(l); bar(); lin(ElCLib::To3d(a, gp_Lin2d(l.a))); finish(); }
    { RF fr = frame2d(n);
      begin("elclib_to3d_ax22d", "ax2 ax22d"); pa2(f); pf(fr); bar(); ax2(ElCLib::To3d(a, fr.a)); finish();
      double rad = rnd(0.1, 9); if (n == 0) rad = 0;
      begin("elclib_to3d_circ", "ax2 circ2d"); pa2(f); pf(fr); h(rad); bar(); circ(ElCLib::To3d(a, gp_Circ2d(fr.a, rad))); finish();
      double mj = rnd(0.5, 9), mn = mj * rnd(0.05, 1); if (n == 0) mn = mj; if (n == 1) mn = 0;
      begin("elclib_to3d_elips", "ax2 elips2d"); pa2(f); pf(fr); h(mj); h(mn); bar(); elips(ElCLib::To3d(a, gp_Elips2d(fr.a, mj, mn))); finish();
      double ha = rnd(0.1, 9), hb = rnd(0.1, 9); if (n == 0) hb = 0; if (n == 1) ha = 0;
      begin("elclib_to3d_hypr", "ax2 hypr2d"); pa2(f); pf(fr); h(ha); h(hb); bar(); hypr(ElCLib::To3d(a, gp_Hypr2d(fr.a, ha, hb))); finish();
      double fo = rnd(0.05, 9); if (n == 0) fo = 0;
      begin("elclib_to3d_parab", "ax2 parab2d"); pa2(f); pf(fr); h(fo); bar(); parab(ElCLib::To3d(a, gp_Parab2d(fr.a, fo))); finish(); }
  }
}

int main() {
  elslib_families();
  elclib_families();
  to3d_families();
  return 0;
}
