// Deterministic reference records for OCCT 8.0.1's package Geom2dEval (the evaluator curves: Archimedean spiral, circle
// involute, logarithmic spiral, sine wave, AHT-Bezier and T-Bezier curves), see docs/api-geom2deval.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal.
// A curve is printed as `type params`: 0 `px py dx dy r0 rate` (spiral); 1 `px py dx dy radius` (involute);
// 2 `px py dx dy scale exponent`; 3 `px py dx dy amplitude omega phase`; 4 AHT `algDegree alpha beta n x y ... hasW [w ...]`;
// 5 T-Bezier `alpha n x y ... hasW [w ...]`.
#include <Geom2dEval_AHTBezierCurve.hxx>
#include <Geom2dEval_ArchimedeanSpiralCurve.hxx>
#include <Geom2dEval_CircleInvoluteCurve.hxx>
#include <Geom2dEval_LogarithmicSpiralCurve.hxx>
#include <Geom2dEval_SineWaveCurve.hxx>
#include <Geom2dEval_TBezierCurve.hxx>
#include <Geom2d_BoundedCurve.hxx>
#include <Geom2d_Curve.hxx>
#include <GeomAbs_Shape.hxx>
#include <NCollection_Array1.hxx>
#include <Standard_Failure.hxx>
#include <gp_Ax2d.hxx>
#include <gp_Dir2d.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Trsf2d.hxx>
#include <gp_Vec2d.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <iostream>
#include <string>
#include <vector>

// ---- output -------------------------------------------------------------------------------------
static std::string line, sig;
static bool inputs = true;
static uint64_t bitsOf(double x) {
  uint64_t b;
  std::memcpy(&b, &x, 8);
  return b;
}
static void F(double x) {
  char t[24];
  std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bitsOf(x));
  line += t;
  if (inputs) sig += 'f';
}
static void I(long long v) {
  line += ' ' + std::to_string(v);
  if (inputs) sig += 'i';
}
static void B(bool v) { I(v ? 1 : 0); }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void begin(const std::string& name) {
  line = name;
  sig.clear();
  inputs = true;
}
static void bar() {
  inputs = false;
  line += " |";
  smash();
}
static void end() { std::cout << line << std::endl; }
#define CASE(...) \
  { \
    size_t keep_ = line.size(); \
    try { \
      __VA_ARGS__ \
    } catch (Standard_Failure&) { \
      line.resize(keep_); \
      line += " throws"; \
    } catch (std::exception&) { \
      line.resize(keep_); \
      line += " throws"; \
    } \
    end(); \
  }

static uint64_t S = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S ^= S << 13;
  S ^= S >> 7;
  S ^= S << 17;
  return (double)(S >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)

static void P2(const gp_Pnt2d& p) { F(p.X()); F(p.Y()); }
static void V2(const gp_Vec2d& v) { F(v.X()); F(v.Y()); }

// ---- curve generation (printing the inputs) --------------------------------------------------------------------------------------------
// `bad` makes the parameters invalid for the constructor
struct Gen {
  occ::handle<Geom2d_Curve> c;  // null when the constructor refused
  double u0 = 0.0, u1 = 1.0;    // a sensible parameter range for evaluation
};
static Gen gen(int type, int j, bool bad) {
  Gen g;
  I(type);
  try {
    if (type <= 3) {
      double px = rv(), py = rv(), dx = rv(), dy = rv();
      if (dx == 0.0 && dy == 0.0) dx = 1.0;
      P2(gp_Pnt2d(px, py)); F(dx); F(dy);
      gp_Ax2d ax(gp_Pnt2d(px, py), gp_Dir2d(dx, dy));
      double a = 0.25 + std::abs(rv()), b = 0.125 + std::abs(rv()) * 0.5, ph = rv();
      if (type == 0) {
        double r0 = (j % 3 == 0) ? 0.0 : a;
        if (bad) { if (j % 2) r0 = -0.5; else b = 0.0; }
        F(r0); F(b); g.u0 = 0.0; g.u1 = 6.0;
        g.c = new Geom2dEval_ArchimedeanSpiralCurve(ax, r0, b);
      } else if (type == 1) {
        if (bad) a = (j % 2) ? 0.0 : -1.0;
        F(a); g.u0 = 0.0; g.u1 = 5.0;
        g.c = new Geom2dEval_CircleInvoluteCurve(ax, a);
      } else if (type == 2) {
        if (bad) { if (j % 2) a = 0.0; else b = -0.25; }
        F(a); F(b * 0.5); g.u0 = -2.0; g.u1 = 3.0;
        g.c = new Geom2dEval_LogarithmicSpiralCurve(ax, a, b * 0.5);
      } else {
        if (bad) { if (j % 2) a = 0.0; else b = -1.0; }
        F(a); F(b * 2.0); F(ph); g.u0 = -3.0; g.u1 = 3.0;
        g.c = new Geom2dEval_SineWaveCurve(ax, a, b * 2.0, ph);
      }
      return g;
    }
    if (type == 4) {
      int deg = j % 4;
      double alpha = (j % 3 == 0) ? 0.0 : 0.5 + 0.25 * (j % 3);
      double beta = ((j / 3) % 3 == 0) ? 0.0 : 0.75 * (1 + (j / 3) % 3);
      int dim = deg + 1 + (alpha > 0.0 ? 2 : 0) + (beta > 0.0 ? 2 : 0);
      int n = (bad && j % 2 == 0) ? dim + 1 : dim;
      I(deg); F(alpha); F(beta); I(n);
      NCollection_Array1<gp_Pnt2d> poles(1, n);
      for (int i = 1; i <= n; ++i) { poles(i) = gp_Pnt2d(rv(), rv()); P2(poles(i)); }
      bool hasW = (j % 2 == 1);
      B(hasW);
      g.u0 = 0.0; g.u1 = 1.0;
      if (!hasW) { g.c = new Geom2dEval_AHTBezierCurve(poles, deg, alpha, beta); return g; }
      NCollection_Array1<double> w(1, n);
      for (int i = 1; i <= n; ++i) { w(i) = 0.5 + std::abs(rv()); if (bad && i == 2) w(i) = 0.0; F(w(i)); }
      g.c = new Geom2dEval_AHTBezierCurve(poles, w, deg, alpha, beta);
      return g;
    }
    // T-Bezier
    double alpha = 0.5 + 0.25 * (j % 5);
    int n = 3 + 2 * (j % 4);
    if (bad) { if (j % 2) n += 1; else alpha = -0.5; }
    F(alpha); I(n);
    NCollection_Array1<gp_Pnt2d> poles(1, n);
    for (int i = 1; i <= n; ++i) { poles(i) = gp_Pnt2d(rv(), rv()); P2(poles(i)); }
    bool hasW = (j % 3 == 1);
    B(hasW);
    g.u0 = 0.0; g.u1 = M_PI / (alpha > 0.0 ? alpha : 1.0);
    if (!hasW) { g.c = new Geom2dEval_TBezierCurve(poles, alpha); return g; }
    NCollection_Array1<double> w(1, n);
    for (int i = 1; i <= n; ++i) { w(i) = 0.5 + std::abs(rv()); F(w(i)); }
    g.c = new Geom2dEval_TBezierCurve(poles, w, alpha);
    return g;
  } catch (Standard_Failure&) {
    g.c.Nullify();
    return g;
  }
}

static const char* NAMES[6] = {"archimedean", "involute", "logarithmic", "sine_wave", "aht_bezier", "t_bezier"};

int main() {
  for (int type = 0; type < 6; ++type) {
    std::string nm = NAMES[type];
    // -- constructor ---------------------------------------------------------------------------------------------------------------------
    for (int j = 0; j < 14; ++j) {
      begin(nm + "_ctor");
      bool bad = (j % 3 == 2);
      Gen g = gen(type, j, bad);
      bar();
      CASE(
        if (g.c.IsNull()) throw Standard_ConstructionError();
        F(g.c->FirstParameter()); F(g.c->LastParameter());
        const Geom2dEval_AHTBezierCurve* ah = dynamic_cast<const Geom2dEval_AHTBezierCurve*>(g.c.get());
        const Geom2dEval_TBezierCurve* tb = dynamic_cast<const Geom2dEval_TBezierCurve*>(g.c.get());
        if (ah) { I(ah->NbPoles()); I(ah->AlgDegree()); F(ah->Alpha()); F(ah->Beta()); B(ah->IsRational()); }
        if (tb) { I(tb->NbPoles()); I(tb->Order()); F(tb->Alpha()); B(tb->IsRational()); }
        if (!ah && !tb) { P2(g.c->EvalD0(g.u0 + 0.5)); })
    }
    // -- evaluation -----------------------------------------------------------------------------------------------------------------------------
    for (int j = 0; j < 42; ++j) {
      begin(nm + "_eval");
      Gen g = gen(type, j, false);
      int op = j % 9;  // EvalD0 EvalD1 EvalD2 EvalD3 EvalDN Value D1 D2 D3
      int n = (j / 9) % 7 + (j % 8 == 7 ? -1 : 0);  // -1 .. 6
      int sel = (j / 2) % 5;
      I(op); I(n); I(sel);
      bar();
      CASE(
        double f = g.u0, l = g.u1, u = f;
        switch (sel) {
          case 0: u = f; break;
          case 1: u = l; break;
          case 2: u = f + (l - f) * 0.3125; break;
          case 3: u = f + (l - f) * 0.75; break;
          default: u = f + (l - f) * 0.5625; break;
        }
        F(u);
        gp_Pnt2d p; gp_Vec2d v1, v2, v3;
        switch (op) {
          case 0: P2(g.c->EvalD0(u)); break;
          case 1: { auto r = g.c->EvalD1(u); P2(r.Point); V2(r.D1); break; }
          case 2: { auto r = g.c->EvalD2(u); P2(r.Point); V2(r.D1); V2(r.D2); break; }
          case 3: { auto r = g.c->EvalD3(u); P2(r.Point); V2(r.D1); V2(r.D2); V2(r.D3); break; }
          case 4: V2(g.c->EvalDN(u, n)); break;
          case 5: P2(g.c->Value(u)); break;
          case 6: g.c->D1(u, p, v1); P2(p); V2(v1); break;
          case 7: g.c->D2(u, p, v1, v2); P2(p); V2(v1); V2(v2); break;
          default: g.c->D3(u, p, v1, v2, v3); P2(p); V2(v1); V2(v2); V2(v3); break;
        })
    }
    // -- properties -----------------------------------------------------------------------------------------------------------------------------
    for (int j = 0; j < 20; ++j) {
      begin(nm + "_props");
      Gen g = gen(type, j, false);
      int op = j % 10;
      int n = j / 10 + (j % 3) * 4;
      I(op); I(n);
      bar();
      CASE(
        const Geom2dEval_AHTBezierCurve* ah = dynamic_cast<const Geom2dEval_AHTBezierCurve*>(g.c.get());
        const Geom2dEval_TBezierCurve* tb = dynamic_cast<const Geom2dEval_TBezierCurve*>(g.c.get());
        const Geom2d_BoundedCurve* bc = dynamic_cast<const Geom2d_BoundedCurve*>(g.c.get());
        switch (op) {
          case 0: F(g.c->FirstParameter()); F(g.c->LastParameter()); break;
          case 1: B(g.c->IsClosed()); B(g.c->IsPeriodic()); break;
          case 2: I((int)g.c->Continuity()); break;
          case 3: B(g.c->IsCN(n)); break;
          case 4: {
            occ::handle<Geom2d_Geometry> cp = g.c->Copy();
            occ::handle<Geom2d_Curve> cc = occ::down_cast<Geom2d_Curve>(cp);
            I(cc.IsNull()); I(cc->DynamicType() == g.c->DynamicType());
            P2(cc->EvalD0(g.u0 + (g.u1 - g.u0) * 0.4375));
            break;
          }
          case 5: if (!bc) throw Standard_DomainError(); P2(bc->StartPoint()); P2(bc->EndPoint()); break;
          case 6: if (ah) { I(ah->NbPoles()); I(ah->AlgDegree()); F(ah->Alpha()); F(ah->Beta()); B(ah->IsRational()); }
                  else if (tb) { I(tb->NbPoles()); I(tb->Order()); F(tb->Alpha()); B(tb->IsRational()); }
                  else throw Standard_DomainError();
                  break;
          case 7: if (ah) { for (int i = ah->Poles().Lower(); i <= ah->Poles().Upper(); ++i) P2(ah->Poles()(i)); }
                  else if (tb) { for (int i = tb->Poles().Lower(); i <= tb->Poles().Upper(); ++i) P2(tb->Poles()(i)); }
                  else throw Standard_DomainError();
                  break;
          case 8: if (ah && ah->IsRational()) { for (int i = ah->Weights().Lower(); i <= ah->Weights().Upper(); ++i) F(ah->Weights()(i)); }
                  else if (tb && tb->IsRational()) { for (int i = tb->Weights().Lower(); i <= tb->Weights().Upper(); ++i) F(tb->Weights()(i)); }
                  else throw Standard_DomainError();
                  break;
          default: P2(g.c->EvalD0(g.u0)); break;
        })
    }
    // -- the operations these curves do not support ----------------------------------------------------------------------------------------------
    for (int j = 0; j < 3; ++j) {
      begin(nm + "_unsupported");
      Gen g = gen(type, j, false);
      I(j);
      bar();
      CASE(
        if (j == 0) { occ::handle<Geom2d_Curve> c = occ::down_cast<Geom2d_Curve>(g.c->Copy()); c->Reverse(); }
        else if (j == 1) F(g.c->ReversedParameter(0.5));
        else { occ::handle<Geom2d_Curve> c = occ::down_cast<Geom2d_Curve>(g.c->Copy()); gp_Trsf2d t; t.SetTranslation(gp_Vec2d(1.0, 2.0)); c->Transform(t); })
    }
  }
  return 0;
}
