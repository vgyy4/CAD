// Reference vectors for the BSplCLib port (ADR 0025): 3D B-spline curve evaluation
// (D0..D3, DN, LocateParameter) by the C++ OCCT on pseudo-random curves, printed as
// exact f64 bit patterns.
// Build: c++ -std=c++17 -O2 bsplclib.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel
#include <BSplCLib.hxx>
#include <NCollection_Array1.hxx>
#include <Standard_Failure.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>
#include <cstdint>
#include <cstdio>
#include <cstring>

static uint64_t state = 0x2545F4914F6CDD1Dull;
static uint64_t next() { state ^= state << 13; state ^= state >> 7; state ^= state << 17; return state; }
static double rnd(double lo, double hi) { return lo + (hi - lo) * (double)(next() >> 11) / 9007199254740992.0; }
static int irnd(int lo, int hi) { return lo + (int)(next() % (uint64_t)(hi - lo + 1)); }
static void h(double v) { uint64_t b; std::memcpy(&b, &v, 8); std::printf(" %016llx", (unsigned long long)b); }
static void p(const gp_XYZ& v) { h(v.X()); h(v.Y()); h(v.Z()); }

int main() {
  int printed = 0;
  for (int c = 0; printed < 500 && c < 4000; ++c) {
    const int  deg      = irnd(1, 5);
    const bool periodic = (c % 3) == 2;
    const bool rational = (c % 2) == 1;
    // Knots: strictly increasing; inner multiplicities 1..deg; clamped ends (deg + 1)
    // when not periodic, equal end multiplicities when periodic.
    const int nk = irnd(2, 6);
    NCollection_Array1<double> knots(1, nk);
    NCollection_Array1<int>    mults(1, nk);
    double k = rnd(-3, 3);
    for (int i = 1; i <= nk; ++i) {
      knots(i) = k;
      k += rnd(0.1, 2.0);
      mults(i) = irnd(1, deg);
    }
    if (periodic) {
      mults(nk) = mults(1);
    } else {
      mults(1) = deg + 1;
      mults(nk) = deg + 1;
    }
    const int np = BSplCLib::NbPoles(deg, periodic, mults);
    if (np < 2 || (periodic && np <= deg)) continue;
    NCollection_Array1<gp_Pnt> poles(1, np);
    NCollection_Array1<double> weights(1, np);
    for (int i = 1; i <= np; ++i) {
      poles(i) = gp_Pnt(rnd(-10, 10), rnd(-10, 10), rnd(-10, 10));
      weights(i) = rnd(0.3, 3.0);
    }
    // Every fifth rational curve has equal weights: OCCT then evaluates it as polynomial.
    if (rational && c % 5 == 1)
      for (int i = 1; i <= np; ++i) weights(i) = 1.75;
    const NCollection_Array1<double>* w = rational ? &weights : nullptr;
    const double span = knots(nk) - knots(1);
    for (int s = 0; s < 3; ++s) {
      // Inside the range, on a knot, and (periodic) outside the first period.
      double u = s == 0 ? knots(1) + rnd(0, 1) * span
               : s == 1 ? knots(irnd(1, nk))
                        : knots(1) + rnd(periodic ? -2.0 : 0.0, periodic ? 3.0 : 1.0) * span;
      try {
        gp_Pnt P0, P1, P2, P3; gp_Vec V11, V21, V22, V31, V32, V33;
        BSplCLib::D0(u, 0, deg, periodic, poles, w, knots, &mults, P0);
        BSplCLib::D1(u, 0, deg, periodic, poles, w, knots, &mults, P1, V11);
        BSplCLib::D2(u, 0, deg, periodic, poles, w, knots, &mults, P2, V21, V22);
        BSplCLib::D3(u, 0, deg, periodic, poles, w, knots, &mults, P3, V31, V32, V33);
        gp_Vec dn[4];
        for (int n = 1; n <= 4; ++n) BSplCLib::DN(u, n, 0, deg, periodic, poles, w, knots, &mults, dn[n - 1]);
        int index = 0; double newu = u;
        BSplCLib::LocateParameter(deg, knots, &mults, u, periodic, index, newu);
        std::printf("bspl %d %d %d %d %d", deg, periodic ? 1 : 0, rational ? 1 : 0, nk, np);
        for (int i = 1; i <= nk; ++i) h(knots(i));
        for (int i = 1; i <= nk; ++i) std::printf(" %d", mults(i));
        for (int i = 1; i <= np; ++i) p(poles(i).XYZ());
        if (rational) for (int i = 1; i <= np; ++i) h(weights(i));
        h(u);
        std::printf(" | %d", index); h(newu);
        p(P0.XYZ()); p(P1.XYZ()); p(V11.XYZ()); p(P2.XYZ()); p(V21.XYZ()); p(V22.XYZ());
        p(P3.XYZ()); p(V31.XYZ()); p(V32.XYZ()); p(V33.XYZ());
        for (auto& v : dn) p(v.XYZ());
        std::printf("\n");
        ++printed;
      } catch (Standard_Failure& e) {
        std::fprintf(stderr, "skipped: %s\n", e.what());
      }
    }
  }
}
