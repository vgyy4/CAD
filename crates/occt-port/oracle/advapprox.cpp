// Deterministic reference records for OCCT 8.0.1's package AdvApprox (AdvApprox_Cutting, _DichoCutting, _PrefCutting,
// _PrefAndRec, _SimpleApprox, _ApproxAFunction, _EvaluatorFunction), see docs/api-advapprox.md.
// Line format: `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal, unsigned 64-bit
// values (hashes) decimal.
// The function to approximate is a test evaluator ("TF") made of up to 8 scalar components; each family names it by its
// index and the first records (`def_func_exact`, `def_func_trig`) print all of them with a few evaluations, so the Rust
// test rebuilds the same functions. The evaluator folds every call (dimension, interval, parameter, derivative order) into
// a counter and a 64-bit hash, so the sequence of calls the approximators make is part of the behaviour.
#include <AdvApprox_ApproxAFunction.hxx>
#include <AdvApprox_Cutting.hxx>
#include <AdvApprox_DichoCutting.hxx>
#include <AdvApprox_EvaluatorFunction.hxx>
#include <AdvApprox_PrefAndRec.hxx>
#include <AdvApprox_PrefCutting.hxx>
#include <AdvApprox_SimpleApprox.hxx>
#include <GeomAbs_Shape.hxx>
#include <NCollection_Array1.hxx>
#include <NCollection_Array2.hxx>
#include <NCollection_HArray1.hxx>
#include <NCollection_HArray2.hxx>
#include <PLib.hxx>
#include <PLib_JacobiPolynomial.hxx>
#include <Standard_Failure.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

// ---- output -------------------------------------------------------------------------------------
static std::string line;
static uint64_t bitsOf(double x) {
  uint64_t b;
  std::memcpy(&b, &x, 8);
  return b;
}
static void F(double x) {
  char t[24];
  std::snprintf(t, sizeof t, " %016llx", (unsigned long long)bitsOf(x));
  line += t;
}
static void I(long long v) { line += ' ' + std::to_string(v); }
static void U(unsigned long long v) { line += ' ' + std::to_string(v); }
static void B(bool v) { I(v ? 1 : 0); }
static void smash() {
  static int fill = -1;
  if (fill < 0) { const char* e = std::getenv("SMASH"); fill = e ? std::atoi(e) : 0; }
  volatile unsigned char buf[1 << 16];
  for (size_t i = 0; i < sizeof buf; ++i) buf[i] = (unsigned char)fill;
}
static void begin(const char* name) {
  line = name;
  if (std::getenv("TR")) std::fprintf(stderr, "BEGIN %s\n", name);
}
static void bar() {
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

static uint64_t S0 = 0x9e3779b97f4a7c15ull;
static double rnd() {
  S0 ^= S0 << 13;
  S0 ^= S0 >> 7;
  S0 ^= S0 << 17;
  return (double)(S0 >> 11) / 9007199254740992.0;
}
static double rv() { return (std::floor(rnd() * 4096.0) - 2048.0) / 1024.0; }  // dyadic in [-2, 2)
static uint64_t mixh(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static const uint64_t H0 = 0xcbf29ce484222325ull;
static uint64_t fnv(const std::string& s) {
  uint64_t h = H0;
  for (unsigned char c : s) h = mixh(h, c);
  return h;
}

// ---- the test functions -------------------------------------------------------------------------
struct Comp {
  int kind;
  double p[5];
};
struct TFDef {
  int n;
  bool trig;
  Comp c[8];
};
static const int NF = 10;  // 0..5 use only exact arithmetic, 6..9 also sin / cos / exp
static TFDef FD[NF];

// sin, cos and exp as fixed series of the four arithmetic operations, so that the values do not depend on the libm
static double msin(double x) {
  double term = x, sum = x;
  for (int k = 1; k < 30; ++k) {
    term = -term * x * x / (double)((2 * k) * (2 * k + 1));
    sum += term;
  }
  return sum;
}
static double mcos(double x) {
  double term = 1.0, sum = 1.0;
  for (int k = 1; k < 30; ++k) {
    term = -term * x * x / (double)((2 * k - 1) * (2 * k));
    sum += term;
  }
  return sum;
}
static double mexp(double x) {
  double term = 1.0, sum = 1.0;
  for (int k = 1; k < 60; ++k) {
    term = term * x / (double)k;
    sum += term;
  }
  return sum;
}

static double compValue(const Comp& c, double t, int d) {
  const double* p = c.p;
  switch (c.kind) {
    case 0:  // polynomial of degree 4
      if (d == 0) return ((((p[4] * t + p[3]) * t + p[2]) * t + p[1]) * t + p[0]);
      if (d == 1) return (((4.0 * p[4] * t + 3.0 * p[3]) * t + 2.0 * p[2]) * t + p[1]);
      return ((12.0 * p[4] * t + 6.0 * p[3]) * t + 2.0 * p[2]);
    case 1: {  // rational p0 / (1 + p2 (t - p1)^2)
      double s = t - p[1];
      double q = 1.0 + p[2] * s * s;
      double q1 = 2.0 * p[2] * s;
      if (d == 0) return p[0] / q;
      if (d == 1) return -p[0] * q1 / (q * q);
      return p[0] * (2.0 * q1 * q1 / (q * q * q) - 2.0 * p[2] / (q * q));
    }
    case 2: {  // p3 + p0 sin(p1 t + p2)
      double a = p[1] * t + p[2];
      if (d == 0) return p[3] + p[0] * msin(a);
      if (d == 1) return p[0] * p[1] * mcos(a);
      return -p[0] * p[1] * p[1] * msin(a);
    }
    case 3: {  // p0 exp(p1 t)
      double e = mexp(p[1] * t);
      if (d == 0) return p[0] * e;
      if (d == 1) return p[0] * p[1] * e;
      return p[0] * p[1] * p[1] * e;
    }
    case 4: {  // p0 (t - p1)^3 for t > p1, else 0
      double s = t - p[1];
      if (!(s > 0.0)) return 0.0;
      if (d == 0) return p[0] * s * s * s;
      if (d == 1) return 3.0 * p[0] * s * s;
      return 6.0 * p[0] * s;
    }
    case 6:  // a jump of height p0 at p1
      if (d == 0) return t > p[1] ? p[0] : 0.0;
      return 0.0;
    default: {  // p0 sqrt(|t - p1 + p2| + p3)
      double r = std::sqrt(std::fabs(t - p[1] + p[2]) + p[3]);
      if (d == 0) return p[0] * r;
      if (d == 1) return p[0] / (2.0 * r);
      return -p[0] / (4.0 * r * r * r);
    }
  }
}

class TF : public AdvApprox_EvaluatorFunction {
public:
  const TFDef* def;
  double errAbove;
  int errCode;
  double errBelow;
  int calls = 0;
  uint64_t hash = H0;
  TF(const TFDef* d, double ea = 1.0e300, int ec = 5, double eb = 1.0e300) : def(d), errAbove(ea), errCode(ec), errBelow(eb) {}
  void Evaluate(int* Dimension, double StartEnd[2], double* Parameter, int* DerivativeRequest, double* Result,
                int* ErrorCode) override {
    ++calls;
    hash = mixh(hash, (uint64_t)(int64_t)*Dimension);
    hash = mixh(hash, bitsOf(StartEnd[0]));
    hash = mixh(hash, bitsOf(StartEnd[1]));
    hash = mixh(hash, bitsOf(*Parameter));
    hash = mixh(hash, (uint64_t)(int64_t)*DerivativeRequest);
    int dim = *Dimension, dv = *DerivativeRequest;
    double t = *Parameter;
    if (dim < 1 || dim > def->n) { *ErrorCode = 9; return; }
    if (dv < 0 || dv > 2) { *ErrorCode = 3; return; }
    if (t > errAbove && t < errBelow) { *ErrorCode = errCode; return; }
    *ErrorCode = 0;
    for (int c = 0; c < dim; ++c) Result[c] = compValue(def->c[c], t, dv);
  }
};
static void prTF(const TF& t) { I(t.calls); U(t.hash); }

static void makeDefs() {
  static const int exactKinds[5] = {0, 1, 4, 5, 6};
  static const int trigKinds[8] = {2, 3, 0, 1, 2, 2, 3, 0};
  for (int f = 0; f < NF; ++f) {
    TFDef& d = FD[f];
    d.n = 8;
    d.trig = f >= 6;
    for (int c = 0; c < 8; ++c) {
      Comp& k = d.c[c];
      k.kind = d.trig ? trigKinds[(f + c) % 8] : exactKinds[(f + c) % 5];
      for (double& x : k.p) x = 0.0;
      switch (k.kind) {
        case 0:
          for (double& x : k.p) x = rv() * 0.5;
          break;
        case 1:
          k.p[0] = rv(); k.p[1] = rv() * 0.5; k.p[2] = 2.0 + 4.0 * rnd();
          break;
        case 2:
          k.p[0] = 0.5 + rnd(); k.p[1] = 1.0 + 4.0 * rnd(); k.p[2] = rv(); k.p[3] = rv();
          break;
        case 3:
          k.p[0] = 0.5 + rnd(); k.p[1] = -1.5 + 3.0 * rnd();
          break;
        case 4:
          k.p[0] = 1.0 + rnd(); k.p[1] = -0.5 + rnd();
          break;
        case 6:
          k.p[0] = 0.5 + rnd(); k.p[1] = -0.5 + rnd();
          break;
        default:
          k.p[0] = 0.5 + rnd(); k.p[1] = rv(); k.p[2] = 0.25 + rnd(); k.p[3] = 0.05 + 0.1 * rnd();
          break;
      }
    }
  }
}

static const GeomAbs_Shape SHP[7] = {GeomAbs_C0, GeomAbs_G1, GeomAbs_C1, GeomAbs_G2, GeomAbs_C2, GeomAbs_C3, GeomAbs_CN};

// ---- selectors of the inputs ------------------------------------------------------------------------
struct Range { double a, b; };
static const Range RG[6] = {{-1.0, 1.0}, {0.0, 1.0}, {-1.2, 1.7}, {0.2, 0.9}, {-2.0, 2.0}, {0.001, 0.5}};
static const int NLAY = 13;
static const int LAY[NLAY][3] = {{1, 0, 0}, {0, 1, 0}, {0, 0, 1}, {2, 0, 0}, {1, 1, 0}, {1, 0, 1}, {0, 1, 1},
                                 {0, 2, 0}, {1, 1, 1}, {0, 0, 2}, {2, 1, 1}, {0, 3, 0}, {3, 0, 1}};
static double tolOf(int j, int i) {
  static const double base[6] = {1e-2, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7};
  return base[(j + i) % 6] * (1.0 + 0.5 * i);
}
static std::vector<double> cutPoints(int j, double a, double b, int n) {
  static const double fr[9] = {0.5, 0.3, 0.55, 0.8, 0.4, 0.62, 0.15, 0.9, 0.25};
  std::vector<double> v;
  for (int i = 0; i < n; ++i) v.push_back(a + (b - a) * fr[(j + 2 * i) % 9]);
  return v;
}
static void FV(const std::vector<double>& v) {
  I((long long)v.size());
  for (double x : v) F(x);
}
static occ::handle<NCollection_HArray1<double>> harr(const std::vector<double>& v) {
  if (v.empty()) return occ::handle<NCollection_HArray1<double>>();
  occ::handle<NCollection_HArray1<double>> h = new NCollection_HArray1<double>(1, (int)v.size());
  for (size_t i = 0; i < v.size(); ++i) h->SetValue((int)i + 1, v[i]);
  return h;
}
static NCollection_Array1<double> arr(const std::vector<double>& v) {
  NCollection_Array1<double> a(1, (int)v.size());
  for (size_t i = 0; i < v.size(); ++i) a.SetValue((int)i + 1, v[i]);
  return a;
}

// a cutting specification: kind 0 Dicho, 1 Pref (points), 2 PrefAndRec (rec, pref, weight)
struct CutSpec {
  int kind = 0;
  std::vector<double> a, b;
  double weight = 2.0;
};
static CutSpec cutSpec(int j, int kind, double lo, double hi) {
  CutSpec c;
  c.kind = kind;
  if (kind == 1) c.a = cutPoints(j, lo, hi, 1 + j % 3);
  if (kind == 2) {
    c.a = cutPoints(j + 1, lo, hi, 1 + (j + 1) % 3);
    c.b = cutPoints(j + 3, lo, hi, 1 + j % 2);
    static const double w[4] = {2.0, 3.5, 1.5, 6.0};
    c.weight = w[j % 4];
  }
  return c;
}
static void prCutSpec(const CutSpec& c) {
  I(c.kind);
  if (c.kind == 1) FV(c.a);
  if (c.kind == 2) {
    FV(c.a);
    FV(c.b);
    F(c.weight);
  }
}

// ---- printing of results ---------------------------------------------------------------------------------
static void prMatrix(const std::vector<double>& flat, int rows, int cols, bool full) {
  I(rows);
  I(cols);
  uint64_t h = H0;
  for (double x : flat) h = mixh(h, bitsOf(x));
  if (!full) U(h);
  if (full || rows <= 2) {
    for (double x : flat) F(x);
  } else {
    for (int j = 0; j < cols; ++j) F(flat[(size_t)j]);
    for (int j = 0; j < cols; ++j) F(flat[(size_t)(rows - 1) * cols + j]);
  }
}
static void prHArr(const occ::handle<NCollection_HArray1<double>>& h) {
  B(h.IsNull());
  if (h.IsNull()) return;
  I(h->Length());
  for (int i = h->Lower(); i <= h->Upper(); ++i) F(h->Value(i));
}

static void prAF(const AdvApprox_ApproxAFunction& a, int n1, int n2, int n3, bool full) {
  B(a.IsDone());
  B(a.HasResult());
  if (!a.HasResult()) return;
  I(a.NbPoles());
  I(a.Degree());
  I(a.NbKnots());
  occ::handle<NCollection_HArray1<double>> K = a.Knots();
  for (int i = K->Lower(); i <= K->Upper(); ++i) F(K->Value(i));
  occ::handle<NCollection_HArray1<int>> M = a.Multiplicities();
  for (int i = M->Lower(); i <= M->Upper(); ++i) I(M->Value(i));
  I(a.NumSubSpaces(1));
  I(a.NumSubSpaces(2));
  I(a.NumSubSpaces(3));
  if (n1 > 0) {
    occ::handle<NCollection_HArray2<double>> P = a.Poles1d();
    std::vector<double> v;
    for (int i = P->LowerRow(); i <= P->UpperRow(); ++i)
      for (int j = P->LowerCol(); j <= P->UpperCol(); ++j) v.push_back(P->Value(i, j));
    prMatrix(v, P->NbRows(), P->NbColumns(), full);
    prHArr(a.MaxError(1));
    prHArr(a.AverageError(1));
  }
  if (n2 > 0) {
    occ::handle<NCollection_HArray2<gp_Pnt2d>> P = a.Poles2d();
    std::vector<double> v;
    for (int i = P->LowerRow(); i <= P->UpperRow(); ++i)
      for (int j = P->LowerCol(); j <= P->UpperCol(); ++j) {
        v.push_back(P->Value(i, j).X());
        v.push_back(P->Value(i, j).Y());
      }
    prMatrix(v, P->NbRows(), P->NbColumns() * 2, full);
    prHArr(a.MaxError(2));
    prHArr(a.AverageError(2));
  }
  if (n3 > 0) {
    occ::handle<NCollection_HArray2<gp_Pnt>> P = a.Poles();
    std::vector<double> v;
    for (int i = P->LowerRow(); i <= P->UpperRow(); ++i)
      for (int j = P->LowerCol(); j <= P->UpperCol(); ++j) {
        v.push_back(P->Value(i, j).X());
        v.push_back(P->Value(i, j).Y());
        v.push_back(P->Value(i, j).Z());
      }
    prMatrix(v, P->NbRows(), P->NbColumns() * 3, full);
    prHArr(a.MaxError(3));
    prHArr(a.AverageError(3));
  }
  std::ostringstream os;
  a.Dump(os);
  I((long long)os.str().size());
  U(fnv(os.str()));
}

// the builder shared by the ApproxAFunction families
struct AfIn {
  int f, n1, n2, n3;
  std::vector<double> t1, t2, t3;
  double first, last;
  int cont, maxdeg, maxseg;
  double errAbove;
  int errCode;
  CutSpec cut;
  bool custom;
};
static void prAfIn(const AfIn& x) {
  I(x.f); I(x.n1); I(x.n2); I(x.n3);
  for (double v : x.t1) F(v);
  for (double v : x.t2) F(v);
  for (double v : x.t3) F(v);
  F(x.first); F(x.last);
  I(x.cont); I(x.maxdeg); I(x.maxseg);
  F(x.errAbove); I(x.errCode);
  if (x.custom) prCutSpec(x.cut);
}
static AfIn mkAf(int j, int f, int lay, int rg, int cont, int maxdeg, int maxseg, bool custom, int cutKind,
                 double errAbove = 1.0e300) {
  AfIn x;
  x.f = f;
  x.n1 = LAY[lay][0]; x.n2 = LAY[lay][1]; x.n3 = LAY[lay][2];
  int k = 0;
  for (int i = 0; i < x.n1; ++i) x.t1.push_back(tolOf(j, k++));
  for (int i = 0; i < x.n2; ++i) x.t2.push_back(tolOf(j, k++));
  for (int i = 0; i < x.n3; ++i) x.t3.push_back(tolOf(j, k++));
  x.first = RG[rg].a;
  x.last = RG[rg].b;
  x.cont = cont; x.maxdeg = maxdeg; x.maxseg = maxseg;
  x.errAbove = errAbove;
  x.errCode = 5;
  x.custom = custom;
  x.cut = cutSpec(j, cutKind, x.first, x.last);
  return x;
}
static void runAf(const AfIn& x, bool full, bool printLog = true, bool nbPoles = false) {
  TF tf(&FD[x.f], x.errAbove, x.errCode);
  occ::handle<NCollection_HArray1<double>> h1 = harr(x.t1), h2 = harr(x.t2), h3 = harr(x.t3);
  if (!x.custom) {
    AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, h1, h2, h3, x.first, x.last, SHP[x.cont], x.maxdeg, x.maxseg, tf);
    prAF(a, x.n1, x.n2, x.n3, full);
    if (nbPoles) I(a.NbPoles());
  } else if (x.cut.kind == 0) {
    AdvApprox_DichoCutting cut;
    AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, h1, h2, h3, x.first, x.last, SHP[x.cont], x.maxdeg, x.maxseg, tf, cut);
    prAF(a, x.n1, x.n2, x.n3, full);
    if (nbPoles) I(a.NbPoles());
  } else if (x.cut.kind == 1) {
    AdvApprox_PrefCutting cut(arr(x.cut.a));
    AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, h1, h2, h3, x.first, x.last, SHP[x.cont], x.maxdeg, x.maxseg, tf, cut);
    prAF(a, x.n1, x.n2, x.n3, full);
    if (nbPoles) I(a.NbPoles());
  } else {
    AdvApprox_PrefAndRec cut(arr(x.cut.a), arr(x.cut.b), x.cut.weight);
    AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, h1, h2, h3, x.first, x.last, SHP[x.cont], x.maxdeg, x.maxseg, tf, cut);
    prAF(a, x.n1, x.n2, x.n3, full);
    if (nbPoles) I(a.NbPoles());
  }
  if (printLog) prTF(tf);
}

int main() {
  makeDefs();
  // ---- the test functions
  for (int f = 0; f < NF; ++f) {
    const char* fam = FD[f].trig ? "def_func_trig" : "def_func_exact";
    begin(fam);
    I(f);
    bar();
    CASE(
      I(FD[f].n);
      for (int c = 0; c < FD[f].n; ++c) {
        I(FD[f].c[c].kind);
        for (double x : FD[f].c[c].p) F(x);
      }
      TF tf(&FD[f]);
      double res[8];
      for (double t : {-0.7, 0.1, 0.9}) {
        for (int d = 0; d <= 2; ++d) {
          int dim = 8, err = -1;
          double se[2] = {-1.0, 1.0};
          tf(&dim, se, &t, &d, res, &err);
          I(err);
          for (double x : res) F(x);
        }
      }
      { int dim = 9, err = -1, d = 0; double se[2] = {0.0, 1.0}, t = 0.5; tf(&dim, se, &t, &d, res, &err); I(err); }
      { int dim = 3, err = -1, d = 3; double se[2] = {0.0, 1.0}, t = 0.5; tf(&dim, se, &t, &d, res, &err); I(err); }
      prTF(tf);)
  }

  // ---- the cutting tools
  {
    const double A[10] = {0.0, -1.0, 0.3, 1.0e-9, 2.0, 5.0, 0.0, 0.0, -3.0, 0.125};
    const double W[10] = {1.0, 0.5, 2.0e-8, 1.0e-9, 1.9e-8, 2.1e-8, 2.0e-8, 0.0, 1.0e-3, 4.0};
    for (int j = 0; j < 40; ++j) {
      double a = A[j % 10] + (j / 10) * 0.03125, b = a + W[(j * 3 + j / 10) % 10];
      if (j % 7 == 6) std::swap(a, b);
      begin("cut_dicho"); F(a); F(b); bar();
      CASE(AdvApprox_DichoCutting c; double v = -77.0; bool ok = c.Value(a, b, v); B(ok); F(v);)
    }
    for (int j = 0; j < 40; ++j) {
      double a = A[(j * 7) % 10] + (j / 10) * 0.0625, b = a + 1.0 + W[(j * 3 + j / 10) % 10] * (j % 3);
      std::vector<double> pts = cutPoints(j, a, b, 1 + j % 4);
      if (j % 5 == 0) pts.push_back(a + 1.0e-9);
      if (j % 6 == 1) pts.push_back(b);
      begin("cut_pref"); FV(pts); F(a); F(b); bar();
      CASE(AdvApprox_PrefCutting c(arr(pts)); double v = -77.0; bool ok = c.Value(a, b, v); B(ok); F(v);)
    }
    for (int j = 0; j < 40; ++j) {
      double a = A[(j * 3) % 10] - (j / 10) * 0.125, b = a + 0.5 + 2.0 * (j % 4);
      std::vector<double> rec = cutPoints(j, a, b, 1 + j % 3), pref = cutPoints(j + 5, a, b, 1 + (j + 1) % 3);
      if (j % 4 == 3) pref.push_back(a + (b - a) * 0.5);
      static const double w[7] = {2.0, 1.0, 0.5, 3.0, 10.0, 1.0000001, 5.0};
      double weight = w[j % 7];
      begin("cut_prefrec"); FV(rec); FV(pref); F(weight); F(a); F(b); bar();
      CASE(AdvApprox_PrefAndRec c(arr(rec), arr(pref), weight); double v = -77.0; bool ok = c.Value(a, b, v); B(ok); F(v);)
    }
  }

  // ---- AdvApprox_SimpleApprox
  static const int SLAY[8][4] = {{1, 1, 0, 0}, {1, 2, 0, 0}, {1, 3, 0, 0}, {2, 1, 2, 0}, {2, 3, 3, 0}, {3, 1, 1, 2}, {3, 2, 2, 2}, {2, 1, 3, 0}};
  static const int CONT[3] = {0, 2, 4};
  static const int CODES[7] = {1, 2, 3, 4, -5, -3, -1};
  for (int pass = 0; pass < 3; ++pass) {
    const char* fam = pass == 0 ? "simple_exact" : pass == 1 ? "simple_trig" : "simple_err";
    int count = pass == 0 ? 56 : pass == 1 ? 28 : 18;
    for (int j = 0; j < count; ++j) {
      int c = CONT[j % 3], niv = c / 2;
      int md = 2 * niv + 1 + (j * 3 / 2) % 9;
      int code = CODES[(j / 3 + j) % 7];
      int ng = 0, wd = 0;
      PLib::JacobiParameters(SHP[c], md, code, ng, wd);
      int f = pass == 1 ? 6 + j % 4 : (pass == 0 ? j % 6 : (j % 6));
      int nss = SLAY[j % 8][0];
      std::vector<int> dims;
      int total = 0;
      for (int i = 0; i < nss; ++i) { dims.push_back(SLAY[j % 8][i + 1]); total += dims.back(); }
      std::vector<double> tols;
      for (int i = 0; i < nss; ++i) tols.push_back(tolOf(j, i));
      double first = RG[j % 6].a, last = RG[j % 6].b;
      double errAbove = 1.0e300, errBelow = 1.0e300;
      if (pass == 2) {
        double al = (last - first) / 2, bl = (last + first) / 2;
        switch (j % 6) {
          case 0: errAbove = first - 0.5; break;
          case 1: errAbove = first + (last - first) * 0.25; break;
          case 2: errAbove = bl + 0.05 * al; errBelow = last - 0.001 * al; break;
          case 3: errAbove = first + 0.001 * al; errBelow = bl - 0.05 * al; break;
          case 4: errAbove = bl - 0.002 * al; errBelow = bl + 0.002 * al; break;
          default: errAbove = first + (last - first) * 0.75; break;
        }
      }
      begin(fam);
      I(f); I(c); I(wd); I(ng); I(nss);
      for (int d : dims) I(d);
      for (double t : tols) F(t);
      F(first); F(last); I(md); F(errAbove); F(errBelow); I(j);
      bar();
      CASE(
        TF tf(&FD[f], errAbove, 5, errBelow);
        PLib_JacobiPolynomial jac(wd, SHP[c]);
        AdvApprox_SimpleApprox sa(total, nss, SHP[c], wd, ng, jac, tf);
        NCollection_Array1<int> ld(1, nss);
        for (int i = 0; i < nss; ++i) ld.SetValue(i + 1, dims[(size_t)i]);
        sa.Perform(ld, arr(tols), first, last, md);
        B(sa.IsDone());
        if (sa.IsDone()) {
          I(sa.Degree());
          occ::handle<NCollection_HArray1<double>> co = sa.Coefficients();
          for (int i = co->Lower(); i <= co->Upper(); ++i) F(co->Value(i));
          occ::handle<NCollection_HArray2<double>> fc = sa.FirstConstr(), lc = sa.LastConstr();
          for (int i = 1; i <= total; ++i)
            for (int k = 0; k <= niv; ++k) F(fc->Value(i, k));
          for (int i = 1; i <= total; ++i)
            for (int k = 0; k <= niv; ++k) F(lc->Value(i, k));
          occ::handle<NCollection_HArray1<double>> so = sa.SomTab(), di = sa.DifTab();
          int from = (ng % 2 == 1) ? 0 : total;
          for (int i = so->Lower() + from; i <= so->Upper(); ++i) F(so->Value(i));
          for (int i = di->Lower() + from; i <= di->Upper(); ++i) F(di->Value(i));
          for (int i = 1; i <= nss; ++i) { F(sa.MaxError(i)); F(sa.AverageError(i)); }
          std::ostringstream os;
          sa.Dump(os);
          I((long long)os.str().size());
          U(fnv(os.str()));
        }
        prTF(tf);)
    }
  }
  // invalid arguments
  {
    struct Bad { int jc, c, wd, ng; };
    // jc: continuity of the Jacobi basis, c: continuity given to SimpleApprox (indices into SHP)
    const Bad bad[16] = {{0, 1, 10, 15}, {0, 3, 10, 15}, {0, 5, 12, 20}, {0, 6, 12, 20}, {0, 0, 10, 12}, {2, 2, 12, 11},
                         {2, 2, 14, 70}, {4, 4, 12, 7}, {0, 0, 10, 0}, {0, 0, 1, 8}, {0, 0, 40, 61}, {2, 2, 3, 8},
                         {4, 4, 3, 8}, {0, 0, 33, 61}, {2, 2, 12, 20}, {4, 4, 14, 30}};
    for (int j = 0; j < 16; ++j) {
      begin("simple_bad"); I(j); I(bad[j].jc); I(bad[j].c); I(bad[j].wd); I(bad[j].ng); bar();
      CASE(
        TF tf(&FD[j % 6]);
        PLib_JacobiPolynomial jac(bad[j].wd, SHP[bad[j].jc]);
        AdvApprox_SimpleApprox sa(2, 1, SHP[bad[j].c], bad[j].wd, bad[j].ng, jac, tf);
        B(true);
        I(sa.IsDone());)
    }
  }

  // ---- AdvApprox_ApproxAFunction
  for (int pass = 0; pass < 4; ++pass) {
    const char* fam = pass == 0 ? "afunc_dicho_exact" : pass == 1 ? "afunc_dicho_trig" : pass == 2 ? "afunc_cut_exact" : "afunc_cut_trig";
    int count = pass == 0 ? 56 : pass == 1 ? 24 : pass == 2 ? 40 : 18;
    bool trig = pass == 1 || pass == 3;
    for (int j = 0; j < count; ++j) {
      int f = trig ? 6 + (j + pass) % 4 : (j + pass) % 6;
      int lay = trig ? (j * 5 + 1) % 7 : (j * 5 + pass) % NLAY;
      if (trig && LAY[lay][0] + LAY[lay][1] + LAY[lay][2] > 3) lay = lay % 3;
      int rg = (j * 5 + pass) % 6;
      int cont = CONT[(j / 2 + pass) % 3];
      int maxdeg = 3 + (j * 7 + pass * 3) % 13;
      int maxseg = trig ? 1 + (j % 4) : 1 + (j * 5 + pass) % 12;
      AfIn x = mkAf(j * 4 + pass, f, lay, rg, cont, maxdeg, maxseg, pass >= 2, 1 + (j / 2) % 2 + (j % 5 == 4 ? -1 : 0));
      if (pass >= 2 && j % 7 == 3) x.cut.kind = 0;
      begin(fam);
      prAfIn(x);
      bar();
      CASE(runAf(x, trig);)
    }
  }
  // functions with a jump: many cuts, continuity checks between the pieces
  for (int j = 0; j < 20; ++j) {
    int lay = (j * 5) % NLAY;
    AfIn x = mkAf(700 + j, 4, lay, (j * 3) % 6, CONT[1 + j % 2], 5 + j % 4, 20 + (j * 7) % 21, j % 3 == 2, 1);
    for (double& t : x.t1) t = 1.0e-3;
    for (double& t : x.t2) t = 2.0e-3;
    for (double& t : x.t3) t = 3.0e-3;
    double jumpAt = FD[4].c[0].p[1];
    if (x.first < jumpAt && jumpAt < x.last) x.cut.a = std::vector<double>(1, jumpAt);
    begin("afunc_jump");
    prAfIn(x);
    bar();
    CASE(runAf(x, false);)
  }
  // evaluation errors and degenerate intervals
  for (int j = 0; j < 20; ++j) {
    int lay = (j * 3) % NLAY, rg = j % 6;
    double lo = RG[rg].a, hi = RG[rg].b;
    AfIn x = mkAf(100 + j, j % 6, lay, rg, CONT[j % 3], 5 + j % 6, 1 + j % 4, j % 2 == 1, 1 + (j / 2) % 2,
                  lo + (hi - lo) * (j % 5 == 0 ? 1.0 : 0.15 * (j % 6)));
    if (j % 5 == 1) { x.errAbove = 1.0e300; x.last = x.first + 1.0e-10; }
    if (j % 5 == 2) { x.errAbove = 1.0e300; x.maxseg = 0; }
    begin("afunc_err");
    prAfIn(x);
    bar();
    CASE(runAf(x, true, true, true);)
  }
  // invalid arguments
  {
    for (int j = 0; j < 16; ++j) {
      AfIn x = mkAf(200 + j, j % 6, j % 6, j % 6, 0, 6, 3, false, 0);
      switch (j % 8) {
        case 0: x.cont = 1; break;
        case 1: x.cont = 3; break;
        case 2: x.cont = 5; break;
        case 3: x.maxdeg = 0; break;
        case 4: x.maxseg = -1; break;
        case 5: std::swap(x.first, x.last); break;
        case 6: x.cont = 6; break;
        default: x.maxdeg = -3; break;
      }
      if (j >= 8) { x.maxdeg = std::max(x.maxdeg, 2); x.custom = false; x.cont = j == 8 ? 1 : x.cont; }
      begin("afunc_bad");
      prAfIn(x);
      bar();
      CASE(runAf(x, true);)
    }
    // no subspace at all, or a negative number of them
    for (int j = 0; j < 6; ++j) {
      AfIn x = mkAf(300 + j, j % 6, 0, 0, 0, 6, 3, false, 0);
      static const int NN[6][3] = {{0, 0, 0}, {-1, 1, 1}, {1, -1, 0}, {0, 0, -2}, {0, 0, 0}, {-1, 2, 0}};
      x.n1 = NN[j][0]; x.n2 = NN[j][1]; x.n3 = NN[j][2];
      x.t1.assign((size_t)std::max(0, x.n1), 1.0e-3);
      x.t2.assign((size_t)std::max(0, x.n2), 1.0e-3);
      x.t3.assign((size_t)std::max(0, x.n3), 1.0e-3);
      x.maxdeg = 4 + j;
      begin("afunc_bad");
      prAfIn(x);
      bar();
      CASE(runAf(x, true);)
    }
  }
  // accessors
  for (int j = 0; j < 40; ++j) {
    int lay = (j * 7 + 2) % NLAY;
    AfIn x = mkAf(400 + j, j % 6, lay, j % 6, CONT[j % 3], 4 + j % 7, 1 + j % 5, false, 0);
    static const int DA[12] = {1, 2, 3, 1, 2, 3, 0, 2, 4, 1, 3, 2};
    static const int DB[12] = {1, 2, 3, 3, 1, 2, 1, 0, 2, 4, -1, 2};
    static const int DC[12] = {1, 2, 3, 3, 1, 2, 1, 2, 2, 3, 3, 0};
    int d = DA[j % 12], db = DB[j % 12], dc = DC[(j + j / 12) % 12];
    int idx = 1 + (j / 6) % 3;
    begin("afunc_access");
    prAfIn(x);
    I(d); I(db); I(dc); I(idx);
    bar();
    CASE(
      TF tf(&FD[x.f]);
      AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, harr(x.t1), harr(x.t2), harr(x.t3), x.first, x.last, SHP[x.cont],
                                  x.maxdeg, x.maxseg, tf);
      B(a.IsDone());
      B(a.HasResult());
      if (a.HasResult()) {
        I(a.NumSubSpaces(d));
        occ::handle<NCollection_HArray1<double>> me = a.MaxError(db), ae = a.AverageError(dc);
        prHArr(me);
        prHArr(ae);
        int nd = db == 1 ? x.n1 : db == 2 ? x.n2 : x.n3;
        int nc = dc == 1 ? x.n1 : dc == 2 ? x.n2 : x.n3;
        if (!me.IsNull() && idx <= nd) F(a.MaxError(db, idx));
        if (!ae.IsNull() && idx <= nc) F(a.AverageError(dc, idx));
        I(a.NbPoles());
      }
      prTF(tf);)
  }
  // poles by index
  for (int j = 0; j < 30; ++j) {
    int lay = (j * 5 + 3) % NLAY;
    AfIn x = mkAf(500 + j, j % 6, lay, (j + 2) % 6, CONT[j % 3], 4 + j % 6, 2 + j % 6, false, 0);
    int d = 1 + j % 3;
    int nd = d == 1 ? x.n1 : d == 2 ? x.n2 : x.n3;
    if (nd == 0) {
      d = x.n1 > 0 ? 1 : x.n2 > 0 ? 2 : 3;
      nd = d == 1 ? x.n1 : d == 2 ? x.n2 : x.n3;
    }
    int idx = 1 + (j / 3) % nd;
    int lower = 1, upper = 1;
    {
      TF tf(&FD[x.f]);
      AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, harr(x.t1), harr(x.t2), harr(x.t3), x.first, x.last, SHP[x.cont],
                                  x.maxdeg, x.maxseg, tf);
      int np = a.NbPoles();
      lower = 1 + j % 2;
      upper = np - j % 3;
      if (upper < lower) upper = lower;
    }
    begin("afunc_poles");
    prAfIn(x);
    I(d); I(idx); I(lower); I(upper);
    bar();
    CASE(
      TF tf(&FD[x.f]);
      AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, harr(x.t1), harr(x.t2), harr(x.t3), x.first, x.last, SHP[x.cont],
                                  x.maxdeg, x.maxseg, tf);
      I(a.NbPoles());
      if (d == 1) {
        NCollection_Array1<double> p(lower, upper);
        a.Poles1d(idx, p);
        for (int i = lower; i <= upper; ++i) F(p(i));
      } else if (d == 2) {
        NCollection_Array1<gp_Pnt2d> p(lower, upper);
        a.Poles2d(idx, p);
        for (int i = lower; i <= upper; ++i) { F(p(i).X()); F(p(i).Y()); }
      } else {
        NCollection_Array1<gp_Pnt> p(lower, upper);
        a.Poles(idx, p);
        for (int i = lower; i <= upper; ++i) { F(p(i).X()); F(p(i).Y()); F(p(i).Z()); }
      }
      prTF(tf);)
  }
  // Dump
  for (int j = 0; j < 14; ++j) {
    AfIn x = mkAf(600 + j, j % 6, (j * 3 + 1) % NLAY, (j + 1) % 6, CONT[j % 3], 3 + j % 8, 1 + j % 6, false, 0);
    begin("afunc_dump");
    prAfIn(x);
    bar();
    CASE(
      TF tf(&FD[x.f]);
      AdvApprox_ApproxAFunction a(x.n1, x.n2, x.n3, harr(x.t1), harr(x.t2), harr(x.t3), x.first, x.last, SHP[x.cont],
                                  x.maxdeg, x.maxseg, tf);
      std::ostringstream os;
      if (a.HasResult()) a.Dump(os);
      I((long long)os.str().size());
      U(fnv(os.str()));)
  }

  // ---- AdvApprox_ApproxAFunction::Approximation
  for (int pass = 0; pass < 4; ++pass) {
    const char* fam = pass == 0 ? "approximation_exact" : pass == 1 ? "approximation_trig" : pass == 2 ? "approximation_bad" : "approximation_err";
    int count = pass == 0 ? 40 : pass == 1 ? 16 : pass == 2 ? 20 : 8;
    bool trig = pass == 1;
    for (int j = 0; j < count; ++j) {
      int f = trig ? 6 + j % 4 : (j + pass) % 6;
      int nss = 1 + (j * 3) % 3;
      std::vector<int> dims;
      int total = 0;
      for (int i = 0; i < nss; ++i) { dims.push_back(1 + (j + 2 * i) % 3); total += dims.back(); }
      if (total > 8 || (trig && total > 4)) { dims.assign(1, 1 + j % 3); nss = 1; total = dims[0]; }
      int co = j % 3;
      int nmax = 2 * co + 2 + (j * 5) % 7;
      int maxseg = trig ? 1 + j % 3 : 1 + (j * 3) % 9;
      int code = 1 + (j / 2) % 4;
      double first = RG[j % 6].a, last = RG[j % 6].b;
      int ck = (j % 4 == 3) ? 1 : 0;
      std::vector<double> tols;
      for (int i = 0; i < nss; ++i) tols.push_back(tolOf(j, i + 1));
      std::vector<double> pts = cutPoints(j, first, last, 2);
      double errAbove = 1.0e300;
      if (pass == 3) errAbove = first + (last - first) * (0.2 + 0.2 * (j % 4));
      if (pass == 2) {
        switch (j % 10) {
          case 0: maxseg = 0; break;
          case 1: last = first + 1.0e-10; break;
          case 2: total += 1; break;  // the dimensions do not add up
          case 3: co = -1; break;
          case 4: co = 3; break;
          case 5: nmax = 2 * co + 1; break;
          case 6: code = 0; break;
          case 7: code = 5; break;
          case 8: nmax = 60; break;
          default: maxseg = -2; break;
        }
      }
      begin(fam);
      I(f); I(total); I(nss);
      for (int d : dims) I(d);
      F(first); F(last); I(ck);
      if (ck == 1) FV(pts);
      I(co); I(nmax); I(maxseg);
      for (double t : tols) F(t);
      I(code); F(errAbove);
      bar();
      CASE(
        TF tf(&FD[f], errAbove);
        int ms = std::max(maxseg, 1);
        NCollection_Array1<int> ld(1, nss);
        for (int i = 0; i < nss; ++i) ld.SetValue(i + 1, dims[(size_t)i]);
        AdvApprox_DichoCutting dicho;
        AdvApprox_PrefCutting pref(arr(pts));
        const AdvApprox_Cutting& cut = ck == 1 ? (const AdvApprox_Cutting&)pref : (const AdvApprox_Cutting&)dicho;
        int nsd = std::max(total, 1);
        int numCurves = 0, errorCode = -77;
        NCollection_Array1<int> ncpc(1, ms);
        ncpc.Init(0);
        NCollection_Array1<double> coef(1, ms * nsd * std::max(nmax, 1)), ints(1, ms + 1), emax(1, ms * nss), eavg(1, ms * nss);
        coef.Init(0.0);
        ints.Init(0.0);
        emax.Init(0.0);
        eavg.Init(0.0);
        AdvApprox_ApproxAFunction::Approximation(total, nss, ld, first, last, tf, cut, co, nmax, maxseg, arr(tols), code,
                                                 numCurves, ncpc, coef, ints, emax, eavg, errorCode);
        I(errorCode);
        I(numCurves);
        for (int i = 1; i <= ms; ++i) I(ncpc(i));
        for (int i = 1; i <= ms + 1; ++i) F(ints(i));
        for (int i = 1; i <= ms * nss; ++i) F(emax(i));
        for (int i = 1; i <= ms * nss; ++i) F(eavg(i));
        std::vector<double> cv;
        for (int i = 1; i <= coef.Length(); ++i) cv.push_back(coef(i));
        if (trig) {
          for (double x : cv) F(x);
        } else {
          uint64_t h = H0;
          for (double x : cv) h = mixh(h, bitsOf(x));
          I((long long)cv.size());
          U(h);
          int used = std::min((int)cv.size(), std::max(numCurves, 0) * nsd * nmax);
          for (int i = 0; i < std::min(used, 24); ++i) F(cv[(size_t)i]);
        }
        prTF(tf);)
    }
  }
  return 0;
}
