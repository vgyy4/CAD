// Reference vectors for the ElCLib port (ADR 0025): calls the C++ OCCT functions on
// pseudo-random inputs and prints inputs and results as exact f64 bit patterns.
// Build: c++ -std=c++17 -O2 elclib.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel
#include <ElCLib.hxx>
#include <gp_Ax2.hxx>
#include <gp_Circ.hxx>
#include <gp_Elips.hxx>
#include <gp_Hypr.hxx>
#include <gp_Lin.hxx>
#include <gp_Parab.hxx>
#include <Standard_Failure.hxx>
#include <cstdint>
#include <cstdio>
#include <cstring>

static uint64_t state = 0x9E3779B97F4A7C15ull;
static double rnd(double lo, double hi) {
  state ^= state << 13; state ^= state >> 7; state ^= state << 17;
  return lo + (hi - lo) * (double)(state >> 11) / 9007199254740992.0;
}
static void h(double v) { uint64_t b; std::memcpy(&b, &v, 8); std::printf(" %016llx", (unsigned long long)b); }
static void p(const gp_XYZ& v) { h(v.X()); h(v.Y()); h(v.Z()); }

int main() {
  for (int i = 0; i < 150; ++i) {
    double in[9];
    for (double& v : in) v = rnd(-10, 10);
    double r1 = rnd(0.1, 20), r2 = rnd(0.05, 1) * r1, u = rnd(-8, 8);
    double q[3] = {rnd(-30, 30), rnd(-30, 30), rnd(-30, 30)};
    try {
      gp_Ax2 ax(gp_Pnt(in[0], in[1], in[2]), gp_Dir(in[3], in[4], in[5]), gp_Dir(in[6], in[7], in[8]));
      gp_Lin lin(ax.Axis());
      gp_Circ circ(ax, r1);
      gp_Elips el(ax, r1, r2);
      gp_Hypr hy(ax, r1, r2);
      gp_Parab pa(ax, r2);
      gp_Pnt qp(q[0], q[1], q[2]);
      for (int k = 0; k < 5; ++k) {
        std::printf("case %d", k);
        for (double v : in) h(v);
        h(r1); h(r2); h(u); h(q[0]); h(q[1]); h(q[2]);
        std::printf(" |");
        gp_Pnt P; gp_Vec V1, V2, V3;
        switch (k) {
          case 0: ElCLib::D1(u, lin, P, V1); p(P.XYZ()); p(V1.XYZ()); h(ElCLib::Parameter(lin, qp)); break;
          case 1: ElCLib::D3(u, circ, P, V1, V2, V3); p(P.XYZ()); p(V1.XYZ()); p(V2.XYZ()); p(V3.XYZ()); h(ElCLib::Parameter(circ, qp)); break;
          case 2: ElCLib::D3(u, el, P, V1, V2, V3); p(P.XYZ()); p(V1.XYZ()); p(V2.XYZ()); p(V3.XYZ()); h(ElCLib::Parameter(el, qp)); break;
          case 3: ElCLib::D3(u, hy, P, V1, V2, V3); p(P.XYZ()); p(V1.XYZ()); p(V2.XYZ()); p(V3.XYZ()); h(ElCLib::Parameter(hy, qp)); break;
          case 4: ElCLib::D2(u, pa, P, V1, V2); p(P.XYZ()); p(V1.XYZ()); p(V2.XYZ()); h(ElCLib::Parameter(pa, qp)); break;
        }
        for (int n = 1; n <= 4; ++n) {
          gp_Vec d = k == 0 ? ElCLib::DN(u, lin, n) : k == 1 ? ElCLib::DN(u, circ, n) : k == 2 ? ElCLib::DN(u, el, n)
                   : k == 3 ? ElCLib::DN(u, hy, n) : ElCLib::DN(u, pa, n);
          p(d.XYZ());
        }
        std::printf("\n");
      }
    } catch (Standard_Failure&) {
      // degenerate axes (parallel N and Vx): skipped
    }
    double a = rnd(-20, 20), w = rnd(0.5, 10), x = rnd(-50, 50);
    std::printf("period"); h(x); h(a); h(a + w); std::printf(" |"); h(ElCLib::InPeriod(x, a, a + w));
    double u1 = rnd(-20, 20), u2 = u1 + rnd(-1, 15);
    double o1 = u1, o2 = u2;
    ElCLib::AdjustPeriodic(a, a + w, 1e-9, o1, o2);
    h(u1); h(u2); std::printf(" |"); h(o1); h(o2); std::printf("\n");
  }
}
