// Deterministic reference records for the header-only OCCT MathPoly package (OCCT 8.0.1), see docs/api-mathpoly.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal; a vector is
// `lower upper values`. Complex numbers are printed as two floats (real, imaginary).
#include <MathPoly_Cubic.hxx>
#include <MathPoly_Laguerre.hxx>
#include <MathPoly_Quadratic.hxx>
#include <MathPoly_Quartic.hxx>
#include <Standard_Failure.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

// ---- output -------------------------------------------------------------------------------------
static std::string line, sig, want;
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
static void V(const std::vector<double>& v) {
  std::string keep = sig;
  line += " 0 " + std::to_string((long long)v.size() - 1);
  for (double x : v) F(x);
  if (inputs) sig = keep + 'V';
}
static void begin(const char* name, const char* schema) {
  line = name;
  sig.clear();
  want = schema;
  inputs = true;
}
static void bar() {
  if (sig != want) {
    std::cerr << "schema mismatch " << line.substr(0, 24) << ": " << sig << " vs " << want << "\n";
    std::abort();
  }
  inputs = false;
  line += " |";
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
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}

static void pr(const MathUtils::PolyResult& r) {
  I((int)r.Status);
  I(r.NbRoots);
  for (int i = 0; i < 4; ++i) F(r.Roots[i]);
}
static void gr(const MathPoly::GeneralPolyResult& r) {
  I((int)r.Status);
  I(r.NbRoots);
  I(r.NbComplexRoots);
  for (int i = 0; i < MathPoly::THE_MAX_POLY_DEGREE; ++i) F(r.Roots[i]);
  for (int i = 0; i < MathPoly::THE_MAX_POLY_DEGREE; ++i) { F(r.ComplexRoots[i].real()); F(r.ComplexRoots[i].imag()); }
}
static void cplx(std::complex<double> z) { F(z.real()); F(z.imag()); }

// the polynomial with the given real roots and complex-conjugate pairs (re, im), ascending coefficients, scaled by lead
static std::vector<double> fromRoots(const std::vector<double>& real, const std::vector<std::pair<double, double>>& pairs, double lead) {
  std::vector<double> c{lead};  // descending while building
  auto mulLinear = [&](double r) {  // times (x - r)
    std::vector<double> n(c.size() + 1, 0.0);
    for (size_t i = 0; i < c.size(); ++i) { n[i] += c[i]; n[i + 1] -= r * c[i]; }
    c = n;
  };
  auto mulQuad = [&](double a, double b) {  // times (x^2 - 2 a x + a^2 + b^2)
    std::vector<double> n(c.size() + 2, 0.0);
    for (size_t i = 0; i < c.size(); ++i) { n[i] += c[i]; n[i + 1] -= 2.0 * a * c[i]; n[i + 2] += (a * a + b * b) * c[i]; }
    c = n;
  };
  for (double r : real) mulLinear(r);
  for (auto& p : pairs) mulQuad(p.first, p.second);
  return std::vector<double>(c.rbegin(), c.rend());  // ascending
}
static double q4() { return (std::floor(rnd() * 32.0) - 16.0) / 4.0; }  // multiples of 1/4 in [-4, 4)

int main() {
  // coefficient sets for the closed-form solvers (descending order a, b, c, ...)
  // quadratic regimes: 0 two roots, 1 double root, 2 no real root, 3 a = 0, 4 a = b = 0, 5 all tiny, 6 huge, 7 near double
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 16; ++j) {  // quadratic, quadratic_edge
      smash();
      double a, b, c;
      int mode = (fam == 0) ? j % 4 : 3 + j % 5;
      double lead = (j % 3 == 0) ? 1.0 : rv() + 0.25;
      switch (mode) {
        case 0: { auto p = fromRoots({q4(), q4()}, {}, lead); c = p[0]; b = p[1]; a = p[2]; break; }
        case 1: { double r = q4(); auto p = fromRoots({r, r}, {}, lead); c = p[0]; b = p[1]; a = p[2]; break; }
        case 2: { auto p = fromRoots({}, {{q4(), 0.25 + std::abs(q4())}}, lead); c = p[0]; b = p[1]; a = p[2]; break; }
        case 3: a = 0.0; b = rv(); c = rv(); break;
        case 4: a = (j % 4 == 1) ? 0.0 : 1e-17 * j; b = (j % 2) ? 0.0 : 1e-16; c = (j % 3) ? 0.0 : rv(); break;
        case 5: a = 1e-8 * rv(); b = 1e-8 * rv(); c = 1e-9 * rv(); break;
        case 6: a = 1e150 * (rv() + 0.5); b = 1e150 * rv(); c = -1e150 * std::abs(rv()); break;
        default: a = 1.0; b = -2.0 - 1e-9 * (j % 3); c = 1.0 + 1e-9 * (j % 4); break;
      }
      if (fam == 1 && j == 12) { a = 1e-300; b = 1e-300; c = 1e-300; }
      begin(fam == 0 ? "quadratic" : "quadratic_edge", "fff"); F(a); F(b); F(c); bar();
      CASE(pr(MathPoly::Quadratic(a, b, c));)
    }
  for (int j = 0; j < 16; ++j) {  // linear
    smash();
    double a = (j % 4 == 0) ? 0.0 : ((j % 4 == 1) ? 1e-16 : rv()), b = (j % 3 == 0 && j < 6) ? 0.0 : rv();
    begin("linear", "ff"); F(a); F(b); bar();
    CASE(pr(MathPoly::Linear(a, b));)
  }
  // cubic: 0 three real, 1 one real + pair, 2 double + single, 3 triple, 4 a = 0 (quadratic), 5 near-degenerate, 6 tiny/huge
  for (int fam = 0; fam < 2; ++fam)
    for (int j = 0; j < 16; ++j) {  // cubic, cubic_edge
      smash();
      int mode = (fam == 0) ? j % 3 : 2 + j % 5;
      double lead = (j % 3 == 0) ? 1.0 : rv() + 0.25;
      double c0, c1, c2, c3;
      std::vector<double> p;
      switch (mode) {
        case 0: p = fromRoots({q4(), q4() + 0.125, q4() - 0.25}, {}, lead); break;
        case 1: p = fromRoots({q4()}, {{q4(), 0.25 + std::abs(q4())}}, lead); break;
        case 2: { double r = q4(); p = fromRoots({r, r, q4() + 0.125}, {}, lead); break; }
        case 3: { double r = q4(); p = fromRoots({r, r, r}, {}, lead); break; }
        case 4: p = {rv(), rv(), rv(), 0.0}; break;
        case 5: p = {1.0 + 1e-9 * j, -3.0, 3.0, -1.0}; break;
        default: p = {1e100 * rv(), 1e100 * rv(), 1e100 * rv(), 1e100 * (rv() + 0.5)}; break;
      }
      if (fam == 1 && j == 5) p = {0.0, 0.0, 0.0, 0.0};
      if (fam == 1 && j == 6) p = {1e-300, 0.0, 0.0, 1e-300};
      c0 = p[0]; c1 = p[1]; c2 = p[2]; c3 = p[3];
      begin(fam == 0 ? "cubic" : "cubic_edge", "ffff"); F(c3); F(c2); F(c1); F(c0); bar();
      CASE(pr(MathPoly::Cubic(c3, c2, c1, c0));)
    }
  // quartic: 0 four real, 1 two real + pair, 2 two pairs, 3 biquadratic, 4 double roots, 5 a = 0, 6 edge
  for (int fam = 0; fam < 3; ++fam)
    for (int j = 0; j < 16; ++j) {  // quartic, quartic_biquadratic, quartic_edge
      smash();
      int mode = (fam == 0) ? j % 3 : (fam == 1 ? 3 : 4 + j % 3);
      double lead = (j % 3 == 0) ? 1.0 : rv() + 0.25;
      std::vector<double> p;
      switch (mode) {
        case 0: p = fromRoots({q4(), q4() + 0.125, q4() - 0.25, q4() + 0.0625}, {}, lead); break;
        case 1: p = fromRoots({q4(), q4() + 0.125}, {{q4(), 0.25 + std::abs(q4())}}, lead); break;
        case 2: p = fromRoots({}, {{q4(), 0.25 + std::abs(q4())}, {q4(), 0.5 + std::abs(q4())}}, lead); break;
        case 3: {  // x^4 + p x^2 + r (q = 0): roots +-s, +-t or complex
          double s = std::abs(q4()) + 0.25, t = (j % 2) ? std::abs(q4()) + 0.5 : 0.0;
          if (j % 4 == 3) p = {rv(), 0.0, rv(), 0.0, 1.0};
          else p = {(j % 2 ? s * s * t * t : s * s), 0.0, -(j % 2 ? s * s + t * t : 2.0 * s * s), 0.0, lead};
          break;
        }
        case 4: { double r = q4(); p = fromRoots({r, r, q4() + 0.125, q4() - 0.125}, {}, lead); break; }
        case 5: p = {rv(), rv(), rv(), rv(), 0.0}; break;
        default: p = {1e-200 * (1 + j), 0.0, 0.0, 0.0, 1e-200}; break;
      }
      if (fam == 2 && j == 3) p = {0.0, 0.0, 0.0, 0.0, 0.0};
      if (fam == 2 && j == 4) { p = fromRoots({1.0, 1.0, 1.0, 1.0}, {}, 1.0); }
      begin(fam == 0 ? "quartic" : (fam == 1 ? "quartic_biquadratic" : "quartic_edge"), "fffff"); F(p[4]); F(p[3]); F(p[2]); F(p[1]); F(p[0]); bar();
      CASE(pr(MathPoly::Quartic(p[4], p[3], p[2], p[1], p[0]));)
    }
  // quintic, sextic: through Laguerre; the four first roots are kept
  for (int j = 0; j < 16; ++j) {  // quintic
    smash();
    std::vector<double> p = (j % 4 == 3) ? std::vector<double>{rv(), rv(), rv(), rv(), rv(), 0.0}
                                          : fromRoots({q4(), q4() + 0.125, q4() - 0.25, q4() + 0.0625, q4() - 0.5}, {}, (j % 2) ? 1.0 : rv() + 0.25);
    if (j % 4 == 2) p = fromRoots({q4(), q4() + 0.125, q4() - 0.25}, {{q4(), 0.25 + std::abs(q4())}}, 1.0);
    begin("quintic", "ffffff"); F(p[5]); F(p[4]); F(p[3]); F(p[2]); F(p[1]); F(p[0]); bar();
    CASE(pr(MathPoly::Quintic(p[5], p[4], p[3], p[2], p[1], p[0]));)
  }
  for (int j = 0; j < 16; ++j) {  // sextic
    smash();
    std::vector<double> p = fromRoots({q4(), q4() + 0.125, q4() - 0.25, q4() + 0.0625, q4() - 0.5, q4() + 0.375}, {}, (j % 2) ? 1.0 : rv() + 0.25);
    if (j % 4 == 2) p = fromRoots({q4(), q4() + 0.125}, {{q4(), 0.25 + std::abs(q4())}, {q4(), 0.5 + std::abs(q4())}}, 1.0);
    if (j % 4 == 3) { p.assign(7, 0.0); for (int i = 0; i < 6; ++i) p[i] = rv(); }  // leading coefficient 0: degree 5
    begin("sextic", "fffffff"); for (int i = 6; i >= 0; --i) F(p[i]); bar();
    CASE(pr(MathPoly::Sextic(p[6], p[5], p[4], p[3], p[2], p[1], p[0]));)
  }
  for (int j = 0; j < 16; ++j) {  // octic
    smash();
    std::vector<double> p = fromRoots({q4(), q4() + 0.125, q4() - 0.25, q4() + 0.0625}, {{q4(), 0.25 + std::abs(q4())}, {q4(), 0.5 + std::abs(q4())}}, 1.0);
    if (j % 3 == 1) p = fromRoots({q4(), q4() + 0.125, q4() - 0.25, q4() + 0.0625, q4() - 0.5, q4() + 0.375, q4() - 0.75, q4() + 0.1875}, {}, 1.0);
    if (j % 5 == 2) { p.assign(9, 0.0); for (int i = 0; i < 8; ++i) p[i] = rv(); p[8] = 1e-16; }
    begin("octic", "V"); V(p); bar();
    CASE(double c[9]; for (int i = 0; i < 9; ++i) c[i] = p[i]; gr(MathPoly::Octic(c));)
  }
  for (int j = 0; j < 32; ++j) {  // laguerre (j < 16) and laguerre_n
    smash();
    int jj = j % 16;
    int deg = (jj < 12) ? 1 + jj : (jj == 12 ? 0 : (jj == 13 ? 21 : (jj == 14 ? 20 : 3)));
    std::vector<double> real, rr;
    std::vector<std::pair<double, double>> pairs;
    int nreal = deg, npairs = 0;
    if (jj % 3 == 2 && deg >= 4) { npairs = 1; nreal = deg - 2; }
    if (jj % 5 == 4 && deg >= 6) { npairs = 2; nreal = deg - 4; }
    for (int i = 0; i < nreal; ++i) real.push_back((-0.875 * (nreal - 1) + 1.75 * i) / 2.0 + q4() * 0.03125);
    for (int i = 0; i < npairs; ++i) pairs.push_back({q4() * 0.25, 0.5 + 0.25 * i + std::abs(q4()) * 0.125});
    std::vector<double> p;
    if (deg >= 1 && deg <= 20) p = fromRoots(real, pairs, 1.0 + 0.25 * (jj % 3));
    else { p.assign(deg + 1, 0.0); for (int i = 0; i <= deg; ++i) p[i] = rv(); }
    if (jj == 13) { p.assign(22, 0.0); for (int i = 0; i < 21; ++i) p[i] = rv(); p[21] = 1.0; }
    if (jj == 12) p = {rv()};
    if (jj == 14) { p = fromRoots({-1.5, -1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, -1.25, -1.75, 0.125, -0.125, 0.375, 1.125}, {}, 1.0); }
    if (jj == 15) p = {2.0, 3.0, 1e-16, 0.0};  // leading coefficient below the tolerance
    double tol = (jj % 4 == 1) ? 1e-9 : ((jj % 4 == 3) ? 1e-14 : 1e-12);
    if (j < 16) {
      begin("laguerre", "Vfi"); V(p); F(tol); I((int)p.size() - 1); bar();
      CASE(gr(MathPoly::Laguerre(p.data(), (int)p.size() - 1, tol)); gr(MathPoly::Laguerre(p.data(), (int)p.size() - 1));)
    } else {
      if (jj == 12) p.push_back(0.0), p.pop_back();
      begin("laguerre_n", "Vf"); V(p); F(tol); bar();
      CASE(gr(MathPoly::LaguerreN(p.data(), p.size(), tol));)
    }
  }
  // ---- the internal helpers (namespace detail) -----------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // eval_polynomial_with_derivatives
    smash();
    int deg = j % 7;
    std::vector<double> p(deg + 1);
    for (auto& x : p) x = rv();
    std::complex<double> x(rv(), (j % 3 == 0) ? 0.0 : rv());
    begin("eval_polynomial_with_derivatives", "Vff"); V(p); F(x.real()); F(x.imag()); bar();
    CASE(std::complex<double> P, DP, D2P; MathPoly::detail::EvaluatePolynomialWithDerivatives(p.data(), deg, x, P, DP, D2P); cplx(P); cplx(DP); cplx(D2P);)
  }
  for (int j = 0; j < 16; ++j) {  // laguerre_iteration
    smash();
    int deg = 1 + j % 6;
    std::vector<double> p = fromRoots({q4() * 0.5, q4() * 0.5 + 0.125, q4() * 0.5 - 0.25}, {{q4() * 0.25, 0.5 + std::abs(q4()) * 0.25}}, 1.0);
    p.resize(deg + 1 < (int)p.size() ? deg + 1 : p.size());
    if (p.back() == 0.0) p.back() = 1.0;
    deg = (int)p.size() - 1;
    std::complex<double> x0((j % 4 == 0) ? 0.0 : rv(), (j % 3 == 0) ? 0.1 : rv());
    double tol = (j % 5 == 0) ? 1e-6 : 1e-12;
    int mi = (j == 3) ? 1 : (j == 4 ? 0 : (j % 6 == 1 ? 2 : 100));
    begin("laguerre_iteration", "Vfffi"); V(p); F(x0.real()); F(x0.imag()); F(tol); I(mi); bar();
    CASE(cplx(MathPoly::detail::LaguerreIteration(p.data(), deg, x0, tol, mi));)
  }
  for (int j = 0; j < 16; ++j) {  // deflate_real
    smash();
    int deg = 1 + j % 7;
    std::vector<double> p(deg + 1);
    for (auto& x : p) x = rv();
    double root = rv();
    begin("deflate_real", "Vf"); V(p); F(root); bar();
    CASE(int d = deg; MathPoly::detail::DeflateReal(p.data(), d, root); I(d); for (int i = 0; i < deg; ++i) F(p[i]);)
  }
  for (int j = 0; j < 16; ++j) {  // deflate_complex
    smash();
    int deg = 2 + j % 7;
    std::vector<double> p(deg + 1);
    for (auto& x : p) x = rv();
    std::complex<double> root(rv(), rv());
    begin("deflate_complex", "Vff"); V(p); F(root.real()); F(root.imag()); bar();
    CASE(int d = deg; MathPoly::detail::DeflateComplex(p.data(), d, root); I(d); for (int i = 0; i < deg - 1; ++i) F(p[i]);)
  }
  for (int j = 0; j < 16; ++j) {  // refine_real_root
    smash();
    int deg = 1 + j % 6;
    double r0 = q4() * 0.5;
    std::vector<double> p = fromRoots({r0, r0 + 0.5, r0 - 0.75}, {}, 1.0);
    p.resize(deg + 1 < (int)p.size() ? deg + 1 : p.size());
    if (p.back() == 0.0) p.back() = 1.0;
    deg = (int)p.size() - 1;
    double guess = r0 + ((j % 3 == 0) ? 0.0 : 1e-3 * rv()) + ((j % 5 == 0) ? 0.5 : 0.0);
    begin("refine_real_root", "Vf"); V(p); F(guess); bar();
    CASE(F(MathPoly::detail::RefineRealRoot(p.data(), deg, guess));)
  }
  return 0;
}
