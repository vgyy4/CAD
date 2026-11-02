// Deterministic reference records for the header-only OCCT MathLin package (OCCT 8.0.1).
// Two translation units are needed because MathLin_EigenSearch.hxx declares a second `MathLin::EigenResult`
// that clashes with the one of MathLin_Jacobi.hxx: compile once normally and once with -DPART_EIGEN,
// and concatenate both outputs (normal part first).
#ifdef PART_EIGEN
#include <MathLin_EigenSearch.hxx>
#else
#include <MathLin_Crout.hxx>
#include <MathLin_Gauss.hxx>
#include <MathLin_Householder.hxx>
#include <MathLin_Jacobi.hxx>
#include <MathLin_LeastSquares.hxx>
#include <MathLin_SVD.hxx>
#endif
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
static void F(double x) {
  uint64_t b;
  std::memcpy(&b, &x, 8);
  char t[24];
  std::snprintf(t, sizeof t, " %016llx", (unsigned long long)b);
  line += t;
  if (inputs) sig += 'f';
}
static void I(long long v) {
  line += ' ' + std::to_string(v);
  if (inputs) sig += 'i';
}
static void M(const math_Matrix& m) {
  long long lr = m.LowerRow(), ur = m.UpperRow(), lc = m.LowerCol(), uc = m.UpperCol();
  std::string keep = sig;
  line += ' ' + std::to_string(lr) + ' ' + std::to_string(ur) + ' ' + std::to_string(lc) + ' ' + std::to_string(uc);
  for (int i = lr; i <= ur; ++i)
    for (int j = lc; j <= uc; ++j) F(m(i, j));
  if (inputs) sig = keep + 'M';
}
static void V(const math_Vector& v) {
  std::string keep = sig;
  line += ' ' + std::to_string(v.Lower()) + ' ' + std::to_string(v.Upper());
  for (int i = v.Lower(); i <= v.Upper(); ++i) F(v(i));
  if (inputs) sig = keep + 'V';
}
#ifndef PART_EIGEN
static void IV(const math_IntegerVector& v) {
  line += ' ' + std::to_string(v.Lower()) + ' ' + std::to_string(v.Upper());
  for (int i = v.Lower(); i <= v.Upper(); ++i) line += ' ' + std::to_string(v(i));
}
#endif
static void oM(const std::optional<math_Matrix>& m) { I(m ? 1 : 0); if (m) M(*m); }
static void oV(const std::optional<math_Vector>& v) { I(v ? 1 : 0); if (v) V(*v); }
static void oD(const std::optional<double>& d) { I(d ? 1 : 0); if (d) F(*d); }
static void begin(const char* name, const char* schema) {
  line = name;
  sig.clear();
  want = schema;
  inputs = true;
}
static void bar() {
  if (sig != want) {
    std::cerr << "schema mismatch " << line.substr(0, 20) << ": " << sig << " vs " << want << "\n";
    std::abort();
  }
  inputs = false;
  line += " |";
}
static void end() { std::cout << line << '\n'; }
// run the computation; an OCCT exception (range checks are on in this build) is recorded as `throws`
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

// ---- deterministic inputs ----------------------------------------------------------------------------
static uint64_t S = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S ^= S << 13;
  S ^= S >> 7;
  S ^= S << 17;
  return (double)(S >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)
static const int LB[4] = {1, 0, 3, -2};

static void smash() {  // fill a chunk of the stack with a recognisable pattern (uninitialised-read check)
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}

// kinds: 0 random, 1 singular (zero row), 2 SPD, 3 diagonally dominant, 4 identity, 5 duplicate rows,
// 6 badly scaled rows, 7 zero, 8 symmetric indefinite, 9 diagonal with a negative entry
static math_Matrix genM(int lr, int m, int lc, int n, int kind) {
  if (m != n && (kind == 2 || kind == 8)) kind = 0;
  math_Matrix a(lr, lr + m - 1, lc, lc + n - 1, 0.0);
  std::vector<double> b(m * n);
  for (auto& x : b) x = rv();
  auto B = [&](int i, int j) { return b[i * n + j]; };
  switch (kind) {
    case 2: {
      for (int i = 0; i < m; ++i)
        for (int j = 0; j < n; ++j) {
          double s = 0.0;
          for (int k = 0; k < m; ++k) s += B(k, i) * B(k, j);
          a(lr + i, lc + j) = s + (i == j ? 0.5 : 0.0);
        }
      return a;
    }
    case 8:
      for (int i = 0; i < m; ++i)
        for (int j = 0; j < n; ++j) a(lr + i, lc + j) = (B(i, j) + B(j % m, i % n)) * 0.5;
      for (int i = 0; i < m && i < n; ++i)
        for (int j = 0; j < i; ++j) a(lr + i, lc + j) = a(lr + j, lc + i);
      return a;
    case 9:
      for (int i = 0; i < m && i < n; ++i) a(lr + i, lc + i) = (i == 1 ? -1.0 : 1.0) + i * 0.5 + 0.125 * std::floor(rnd() * 8.0);
      return a;
    case 4:
      for (int i = 0; i < m && i < n; ++i) a(lr + i, lc + i) = 1.0;
      return a;
    case 7: return a;
    default: break;
  }
  for (int i = 0; i < m; ++i)
    for (int j = 0; j < n; ++j) a(lr + i, lc + j) = B(i, j);
  if (kind == 3)
    for (int i = 0; i < m && i < n; ++i) a(lr + i, lc + i) += (i % 2 ? -1 : 1) * (2.0 * n + 1.0);
  if (kind == 1 && m > 1)
    for (int j = 0; j < n; ++j) a(lr + m / 2, lc + j) = 0.0;
  if (kind == 5 && m > 1)
    for (int j = 0; j < n; ++j) a(lr + m - 1, lc + j) = a(lr, lc + j);
  if (kind == 6)
    for (int i = 0; i < m; ++i)
      for (int j = 0; j < n; ++j) a(lr + i, lc + j) *= std::ldexp(1.0, -9 * i);
  return a;
}
static math_Vector genV(int l, int n, int kind = 0) {
  math_Vector v(l, l + n - 1);
  for (int i = l; i < l + n; ++i) v(i) = kind == 1 ? 0.0 : rv();
  return v;
}
static const int KINDS[8] = {0, 3, 2, 1, 6, 5, 7, 0};

int main() {
#ifndef PART_EIGEN
  const double PIV[4] = {1e-20, 0.0, 1e-3, 10.0};
  const double TOL[5] = {1e-15, 1e-6, 0.0, 1e-3, 1e-20};

  // -- Gauss -----------------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // lu(A, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4], kind = KINDS[j % 8], CL = (j == 9) ? LB[(j + 1) % 4] : lb;
    math_Matrix a = (j == 13) ? genM(lb, 3, lb, 2, 0) : genM(lb, n, CL, n, kind);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : (j == 12 ? 2.5 : 1e-20);
    begin("lu", "Mf"); M(a); F(mp); bar();
    CASE(auto r = MathLin::LU(a, mp);
    I((int)r.Status); I(r.Sign); oM(r.LU);
    I(r.Pivot ? 1 : 0);
    if (r.Pivot && r.Status == MathUtils::Status::OK) IV(*r.Pivot);
    oD(r.Determinant);)
  }
  for (int j = 0; j < 16; ++j) {  // solve(A, B, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4], kind = KINDS[j % 8], CL = (j == 9) ? LB[(j + 1) % 4] : lb;
    math_Matrix a = genM(lb, n, CL, n, kind);
    math_Vector b = (j == 13) ? genV(lb + 1, n) : genV(lb, n);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("solve", "MVf"); M(a); V(b); F(mp); bar();
    CASE(auto r = MathLin::Solve(a, b, mp);
    I((int)r.Status); oV(r.Solution); oD(r.Determinant);)
  }
  for (int j = 0; j < 16; ++j) {  // solve_multiple(A, B, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4], kind = KINDS[j % 8], CL = (j == 9) ? LB[(j + 1) % 4] : lb, nc = 1 + j % 3;
    math_Matrix a = genM(lb, n, CL, n, kind);
    math_Matrix b = (j == 13) ? genM(lb + 1, n, 1, nc, 0) : genM(lb, n, LB[(j + 1) % 4], nc, 0);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("solve_multiple", "MMf"); M(a); M(b); F(mp); bar();
    CASE(auto r = MathLin::SolveMultiple(a, b, mp);
    I((int)r.Status); oM(r.Solutions); oD(r.Determinant);)
  }
  for (int j = 0; j < 16; ++j) {  // determinant(A, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4], kind = KINDS[j % 8], CL = (j == 9) ? LB[(j + 1) % 4] : lb;
    math_Matrix a = (j == 14) ? genM(lb, 2, lb, 3, 0) : genM(lb, n, CL, n, kind);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("determinant", "Mf"); M(a); F(mp); bar();
    CASE(auto r = MathLin::Determinant(a, mp);
    I((int)r.Status); oV(r.Solution); oD(r.Determinant);)
  }
  for (int j = 0; j < 16; ++j) {  // invert(A, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4], kind = KINDS[j % 8], CL = (j == 9) ? LB[(j + 1) % 4] : lb;
    math_Matrix a = (j == 15) ? genM(lb, 3, lb, 4, 0) : genM(lb, n, CL, n, kind);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("invert", "Mf"); M(a); F(mp); bar();
    CASE(auto r = MathLin::Invert(a, mp);
    I((int)r.Status); oM(r.Inverse); oD(r.Determinant);)
  }
  // -- Crout -----------------------------------------------------------------------------------------
  const int CK[8] = {2, 2, 8, 1, 2, 9, 7, 4};
  for (int j = 0; j < 16; ++j) {  // crout(A, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4];
    math_Matrix a = (j == 14) ? genM(lb, 3, lb, 2, 0) : genM(lb, n, (j % 8 == 5) ? LB[(j + 1) % 4] : lb, n, CK[j % 8]);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("crout", "Mf"); M(a); F(mp); bar();
    CASE(auto r = MathLin::Crout(a, mp);
    I((int)r.Status); oM(r.L); oV(r.D); oM(r.Inverse); oD(r.Determinant);)
  }
  for (int j = 0; j < 16; ++j) {  // solve_crout(A, B, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4];
    math_Matrix a = genM(lb, n, (j % 8 == 5) ? LB[(j + 1) % 4] : lb, n, CK[j % 8]);
    int bn = (j == 13) ? n + 1 : n;
    math_Vector b = genV(LB[(j + 2) % 4], bn);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("solve_crout", "MVf"); M(a); V(b); F(mp); bar();
    CASE(auto r = MathLin::SolveCrout(a, b, mp);
    I((int)r.Status); oV(r.Solution); oD(r.Determinant);)
  }
  for (int j = 0; j < 16; ++j) {  // invert_crout(A, minPivot)
    smash();
    int n = 1 + j % 5, lb = LB[j % 4];
    math_Matrix a = (j == 15) ? genM(lb, 2, lb, 3, 0) : genM(lb, n, (j % 8 == 5) ? LB[(j + 1) % 4] : lb, n, CK[j % 8]);
    double mp = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("invert_crout", "Mf"); M(a); F(mp); bar();
    CASE(auto r = MathLin::InvertCrout(a, mp);
    I((int)r.Status); oM(r.Inverse); oD(r.Determinant);)
  }
  // -- SVD -------------------------------------------------------------------------------------------
  auto shape = [&](int j, int& m, int& n) {  // tall, wide, square, tiny
    static const int MS[8] = {3, 4, 2, 1, 5, 3, 6, 2}, NS[8] = {3, 2, 4, 1, 3, 5, 2, 2};
    m = MS[j % 8]; n = NS[j % 8];
  };
  for (int j = 0; j < 16; ++j) {  // svd(A, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    double tol = TOL[j % 5];
    begin("svd", "Mf"); M(a); F(tol); bar();
    CASE(auto r = MathLin::SVD(a, tol);
    I((int)r.Status); I(r.Rank); oM(r.U); oV(r.SingularValues); oM(r.V);)
  }
  for (int j = 0; j < 16; ++j) {  // solve_svd(A, B, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    math_Vector b = genV(LB[(j + 2) % 4], (j == 13) ? m + 1 : m);
    double tol = (j % 3 == 0) ? 1e-6 : TOL[j % 5];
    begin("solve_svd", "MVf"); M(a); V(b); F(tol); bar();
    CASE(auto r = MathLin::SolveSVD(a, b, tol);
    I((int)r.Status); oV(r.Solution);)
  }
  for (int j = 0; j < 16; ++j) {  // pseudo_inverse(A, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    double tol = (j % 3 == 0) ? 1e-6 : TOL[j % 5];
    begin("pseudo_inverse", "Mf"); M(a); F(tol); bar();
    CASE(auto r = MathLin::PseudoInverse(a, tol);
    I((int)r.Status); oM(r.Inverse);)
  }
  for (int j = 0; j < 16; ++j) {  // condition_number(A)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    begin("condition_number", "M"); M(a); bar();
    CASE(F(MathLin::ConditionNumber(a));)
  }
  for (int j = 0; j < 16; ++j) {  // numerical_rank(A, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    double tol = TOL[j % 5];
    begin("numerical_rank", "Mf"); M(a); F(tol); bar();
    CASE(I(MathLin::NumericalRank(a, tol));)
  }
  // -- Householder -----------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // qr(A, tol)
    smash();
    int m, n; shape(j, m, n);
    if (j % 8 == 1) std::swap(m, n);  // keep some wide inputs (InvalidInput)
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    double tol = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("qr", "Mf"); M(a); F(tol); bar();
    CASE(auto r = MathLin::QR(a, tol);
    I((int)r.Status); I(r.Rank); oM(r.Q); oM(r.R);)
  }
  for (int j = 0; j < 16; ++j) {  // solve_qr(A, B, tol)
    smash();
    int m, n; shape(j, m, n);
    if (j % 8 == 1) std::swap(m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    math_Vector b = genV(LB[(j + 2) % 4], (j == 13) ? m + 1 : m);
    double tol = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("solve_qr", "MVf"); M(a); V(b); F(tol); bar();
    CASE(auto r = MathLin::SolveQR(a, b, tol);
    I((int)r.Status); oV(r.Solution);)
  }
  for (int j = 0; j < 16; ++j) {  // solve_qr_multiple(A, B, tol)
    smash();
    int m, n; shape(j, m, n);
    if (j % 8 == 1) std::swap(m, n);
    int lb = LB[j % 4], nc = 1 + j % 3;
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 2) % 8]);
    math_Matrix b = genM(LB[(j + 2) % 4], (j == 13) ? m + 1 : m, LB[(j + 3) % 4], nc, 0);
    double tol = (j >= 8 && j < 12) ? PIV[j - 8] : 1e-20;
    begin("solve_qr_multiple", "MMf"); M(a); M(b); F(tol); bar();
    CASE(auto r = MathLin::SolveQRMultiple(a, b, tol);
    I((int)r.Status); oM(r.Solutions);)
  }
  // -- Jacobi ----------------------------------------------------------------------------------------
  const int JK[8] = {2, 8, 9, 4, 7, 2, 8, 0};
  auto jac = [&](const char* name, int mode) {  // 0 jacobi, 1 eigen_values, 2 spectral_decomposition
    for (int j = 0; j < 16; ++j) {
      smash();
      int n = 1 + j % 5, lb = LB[j % 4];
      math_Matrix a = (j == 14) ? genM(lb, 3, lb, 2, 0) : genM(lb, n, LB[(j + 1) % 4], n, JK[j % 8]);
      bool sort = (j % 2 == 0);
      if (mode == 2) { begin(name, "M"); M(a); } else { begin(name, "Mi"); M(a); I(sort); }
      bar();
    CASE(auto r = mode == 0 ? MathLin::Jacobi(a, sort) : mode == 1 ? MathLin::EigenValues(a, sort) : MathLin::SpectralDecomposition(a);
      I((int)r.Status); I((long long)r.NbIterations); oV(r.EigenValues); oM(r.EigenVectors);)
    }
  };
  jac("jacobi", 0);
  jac("eigen_values", 1);
  jac("spectral_decomposition", 2);
  // -- matrix functions (pow: libm family) -----------------------------------------------------------
  {
    const double PW[8] = {0.5, 2.0, 3.0, -1.0, 0.0, 1.0, 1.5, -0.5};
    for (int j = 0; j < 16; ++j) {  // matrix_power(A, p)
      smash();
      int n = 1 + j % 4, lb = LB[j % 4];
      int kind = (j % 8 == 7) ? 8 : (j % 8 == 6 ? 9 : 2);
      math_Matrix a = (j == 15) ? genM(lb, 2, lb, 3, 0) : genM(lb, n, (j % 8 == 3) ? LB[(j + 1) % 4] : lb, n, kind);
      double p = PW[j % 8];
      begin("matrix_power", "Mf"); M(a); F(p); bar();
    CASE(oM(MathLin::MatrixPower(a, p));)
    }
    for (int j = 0; j < 16; ++j) {  // matrix_sqrt(A)
      smash();
      int n = 1 + j % 4, lb = LB[j % 4];
      int kind = (j % 8 == 7) ? 8 : (j % 8 == 6 ? 9 : 2);
      math_Matrix a = (j == 15) ? genM(lb, 2, lb, 3, 0) : genM(lb, n, (j % 8 == 3) ? LB[(j + 1) % 4] : lb, n, kind);
      begin("matrix_sqrt", "M"); M(a); bar();
    CASE(oM(MathLin::MatrixSqrt(a));)
    }
    for (int j = 0; j < 16; ++j) {  // matrix_inv_sqrt(A)
      smash();
      int n = 1 + j % 4, lb = LB[j % 4];
      int kind = (j % 8 == 7) ? 8 : (j % 8 == 6 ? 9 : 2);
      math_Matrix a = (j == 15) ? genM(lb, 2, lb, 3, 0) : genM(lb, n, (j % 8 == 3) ? LB[(j + 1) % 4] : lb, n, kind);
      begin("matrix_inv_sqrt", "M"); M(a); bar();
    CASE(oM(MathLin::MatrixInvSqrt(a));)
    }
  }
  // -- least squares ---------------------------------------------------------------------------------
  auto lsout = [&](const MathLin::LeastSquaresResult& r) {
    I((int)r.Status); I(r.Rank); oV(r.Solution); oD(r.Residual); oD(r.ResidualSq);
  };
  for (int j = 0; j < 16; ++j) {  // least_squares(A, B, method, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4], meth = j % 3;
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 3) % 8]);
    math_Vector b = genV(LB[(j + 2) % 4], (j == 13) ? m + 1 : m);
    double tol = (j % 4 == 3) ? 1e-6 : 1e-15;
    begin("least_squares", "MVif"); M(a); V(b); I(meth); F(tol); bar();
    CASE(lsout(MathLin::LeastSquares(a, b, (MathLin::LeastSquaresMethod)meth, tol));)
  }
  for (int j = 0; j < 16; ++j) {  // weighted_least_squares(A, B, W, method, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4], meth = j % 3;
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 3) % 8]);
    math_Vector b = genV(LB[(j + 2) % 4], m);
    math_Vector w = genV(LB[(j + 3) % 4], (j == 13) ? m + 1 : m);
    for (int i = w.Lower(); i <= w.Upper(); ++i) w(i) = 0.25 + std::abs(w(i));
    if (j == 14) w(w.Lower()) = 0.0;
    if (j == 15) w(w.Upper()) = -1.0;
    double tol = (j % 4 == 3) ? 1e-6 : 1e-15;
    begin("weighted_least_squares", "MVVif"); M(a); V(b); V(w); I(meth); F(tol); bar();
    CASE(lsout(MathLin::WeightedLeastSquares(a, b, w, (MathLin::LeastSquaresMethod)meth, tol));)
  }
  for (int j = 0; j < 16; ++j) {  // regularized_least_squares(A, B, lambda, tol)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 3) % 8]);
    math_Vector b = genV(LB[(j + 2) % 4], (j == 13) ? m + 1 : m);
    double lam = (j == 14) ? -0.5 : (j == 15 ? 0.0 : std::ldexp(1.0, -(j % 8) * 2));
    double tol = (j % 4 == 3) ? 1e-6 : 1e-15;
    begin("regularized_least_squares", "MVff"); M(a); V(b); F(lam); F(tol); bar();
    CASE(lsout(MathLin::RegularizedLeastSquares(a, b, lam, tol));)
  }
  for (int j = 0; j < 16; ++j) {  // optimal_regularization(A, B, lmin, lmax, nbPoints)
    smash();
    int m, n; shape(j, m, n);
    int lb = LB[j % 4];
    math_Matrix a = genM(lb, m, LB[(j + 1) % 4], n, KINDS[(j / 3) % 8]);
    math_Vector b = genV(LB[(j + 2) % 4], m);
    double lmin = (j % 3 == 0) ? 1e-10 : (j % 3 == 1 ? 1e-4 : 0.5), lmax = (j % 2) ? 1e2 : 8.0;
    int np = (j == 14) ? 2 : (j == 15 ? 1 : 3 + j % 9);
    begin("optimal_regularization", "MVffi"); M(a); V(b); F(lmin); F(lmax); I(np); bar();
    CASE(F(MathLin::OptimalRegularization(a, b, lmin, lmax, np));)
  }
#else
  // -- EigenSearch -----------------------------------------------------------------------------------
  for (int j = 0; j < 16; ++j) {  // eigen_tridiagonal(diag, sub, maxIter)
    smash();
    int n = 1 + j % 6, ld = LB[j % 4], ls = LB[(j + 1) % 4];
    math_Vector d = genV(ld, n), s = genV(ls, (j == 13) ? n + 1 : n);
    if (j == 12) for (int i = s.Lower(); i <= s.Upper(); ++i) s(i) = 0.0;  // already diagonal
    if (j == 11) for (int i = d.Lower(); i <= d.Upper(); ++i) d(i) = 1.0;  // equal diagonal
    int mi = (j == 10) ? 1 : (j == 9 ? 0 : (j == 8 ? 100 : 30));
    begin("eigen_tridiagonal", "VVi"); V(d); V(s); I(mi); bar();
    CASE(auto r = MathLin::EigenTridiagonal(d, s, mi);
    I((int)r.Status); I(r.Dimension); oV(r.EigenValues); oM(r.EigenVectors);)
  }
  for (int j = 0; j < 16; ++j) {  // get_eigen_vector(EigenTridiagonal(..), index)
    smash();
    int n = 1 + j % 6, ld = LB[j % 4], ls = LB[(j + 1) % 4];
    math_Vector d = genV(ld, n), s = genV(ls, n);
    int mi = (j == 14) ? 0 : 30;
    int idx = 1 + (j * 7) % n;  // 1..n (the index is relative to the lower column)
    begin("get_eigen_vector", "VVii"); V(d); V(s); I(mi); I(idx); bar();
    CASE(auto r = MathLin::EigenTridiagonal(d, s, mi);
    I((int)r.Status);
    math_Vector v = MathLin::GetEigenVector(r, idx);
    I(v.Lower()); I(v.Upper());
    if (r.EigenVectors) for (int i = v.Lower(); i <= v.Upper(); ++i) F(v(i));)
  }
#endif
  return 0;
}
