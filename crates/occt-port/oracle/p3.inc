
// ---- helpers for the surface families ------------------------------------------------------------------------------------
static void prSurfBasics(const Adaptor3d_Surface& a) {
  I((int)a.GetType());
  F(a.FirstUParameter());
  F(a.LastUParameter());
  F(a.FirstVParameter());
  F(a.LastVParameter());
}
static void prSurfFull(const Adaptor3d_Surface& a) {
  prSurfBasics(a);
  B(a.IsUClosed());
  B(a.IsVClosed());
  B(a.IsUPeriodic());
  ATT(F(a.UPeriod());)
  B(a.IsVPeriodic());
  ATT(F(a.VPeriod());)
  ATT(I((int)a.UContinuity());)
  ATT(I((int)a.VContinuity());)
  ATT(F(a.UResolution(1.0e-3));)
  ATT(F(a.VResolution(1.0e-3));)
}
static void prCurveHandle(const occ::handle<Adaptor3d_Curve>& c) {
  B(c.IsNull());
  if (!c.IsNull()) {
    prCurveBasics(*c);
  }
}
static void prSurfHandle(const occ::handle<Adaptor3d_Surface>& c) {
  B(c.IsNull());
  if (!c.IsNull()) {
    prSurfBasics(*c);
  }
}
static const double SFR[10][2] = {{0.0, 0.0}, {1.0, 1.0}, {1.0, 0.0}, {0.5, 0.5}, {1.0e-4, 0.5},
                                  {0.5, 1.0 - 1.0e-4}, {0.3, 0.6}, {0.999, 0.001}, {0.25, 0.75}, {0.0, 1.0}};
static const double STOL[4][2] = {{0.0, 0.0}, {0.01, 0.01}, {0.0, 0.02}, {0.005, 0.0}};

static void surfaceFamilies() {
  for (int i = 0; i < NGS; ++i) {
    begin("def_gsurface"); I(i); bar();
    CASE(
      prSS(GS[i]);
      double a, b, c, d;
      SC[i]->Bounds(a, b, c, d);
      F(a); F(b); F(c); F(d);
      double u1, u2, v1, v2;
      surfRange(i, 0, u1, u2, v1, v2);
      P3(SC[i]->EvalD0(u1 + 0.4 * (u2 - u1), v1 + 0.3 * (v2 - v1)));
      F(u1); F(u2); F(v1); F(v2);)
  }
  // -- Load, the constructors
  for (int j = 0; j < 90; ++j) {
    int s = j % NGS, r = (j / NGS + j) % 4, v = (j / 3) % 5, tl = (j / 2 + j / 5) % 4;
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double a, b, c, d;
    SC[s]->Bounds(a, b, c, d);
    bool full = r % 4 == 0 && !Precision::IsInfinite(a) && !Precision::IsInfinite(b) && !Precision::IsInfinite(c) && !Precision::IsInfinite(d);
    full = full && tu == 0.0 && tv == 0.0;
    begin("gas_load"); I(s); I(v); I(full); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x;
      auto load = [&](GeomAdaptor_Surface& y, int si) {
        if (full) y.Load(SC[si]);
        else {
          double p1, p2, q1, q2;
          surfRange(si, r, p1, p2, q1, q2);
          y.Load(SC[si], p1, p2, q1, q2, tu, tv);
        }
      };
      if (v == 0) { x = new GeomAdaptor_Surface(); load(*x, s); }
      else if (v == 1) { x = full ? new GeomAdaptor_Surface(SC[s]) : new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2, tu, tv); }
      else if (v == 2) { x = new GeomAdaptor_Surface(SC[s]); load(*x, s); }
      else if (v == 3) { x = new GeomAdaptor_Surface(SC[(s + 1) % NGS]); load(*x, s); }
      else { x = new GeomAdaptor_Surface(); load(*x, s); occ::handle<Adaptor3d_Surface> cp = x->ShallowCopy(); prSurfBasics(*cp); }
      prSurfFull(*x);
      F(x->ToleranceU()); F(x->ToleranceV());
      B(x->Surface().get() == SC[s].get());)
  }
  // -- invalid loads
  for (int j = 0; j < 12; ++j) {
    int s = (j * 5) % NGS, k = j % 4;
    double u1, u2, v1, v2;
    surfRange(s, j, u1, u2, v1, v2);
    begin("gas_load_bad"); I(s); I(k); F(u1); F(u2); F(v1); F(v2); bar();
    CASE(
      GeomAdaptor_Surface x;
      if (k == 0) x.Load(occ::handle<Geom_Surface>());
      else if (k == 1) x.Load(SC[s], u2 + 0.1, u1, v1, v2);
      else if (k == 2) x.Load(SC[s], u1, u2, v2 + 0.1, v1);
      else x = GeomAdaptor_Surface(SC[s], u1, u1, v1, v1);
      prSurfBasics(x);)
  }
  // -- evaluation
  for (int j = 0; j < 58; ++j) {
    int s = j % NGS, r = (j / NGS + j / 11) % 4, tl = (j / 5 + j / 29) % 4;
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double pt[2][2];
    pt[0][0] = u1 + SFR[p1][0] * (u2 - u1); pt[0][1] = v1 + SFR[p1][1] * (v2 - v1);
    pt[1][0] = u1 + SFR[p2][0] * (u2 - u1); pt[1][1] = v1 + SFR[p2][1] * (v2 - v1);
    int ord = (j / 7 + j) % 4;
    begin("gas_eval"); I(ord); I(s); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2, tu, tv);
      for (auto& q : pt) {
        double u = q[0], v = q[1];
        std::function<void()> ev[4] = {
          [&] { ATT(P3(x->Value(u, v));) },
          [&] { ATT(gp_Pnt p; gp_Vec a, b; x->D1(u, v, p, a, b); P3(p); V3(a); V3(b);) },
          [&] { ATT(gp_Pnt p; gp_Vec a, b, c, d, e; x->D2(u, v, p, a, b, c, d, e); P3(p); V3(a); V3(b); V3(c); V3(d); V3(e);) },
          [&] { ATT(gp_Pnt p; gp_Vec a, b, c, d, e, f, g, h, k; x->D3(u, v, p, a, b, c, d, e, f, g, h, k); P3(p); V3(a); V3(b); V3(c);
                    V3(d); V3(e); V3(f); V3(g); V3(h); V3(k);) }};
        for (int q = 0; q < 4; ++q) ev[(q + ord) % 4]();
        static const int NN[8][2] = {{1, 0}, {0, 1}, {1, 1}, {2, 0}, {0, 2}, {2, 1}, {1, 2}, {3, 1}};
        for (auto& n : NN) ATT(V3(x->DN(u, v, n[0], n[1]));)
      })
  }
  // -- intervals
  for (int j = 0; j < NGS * 7; ++j) {
    int s = j % NGS, sh = j / NGS, r = (s * 3 + sh) % 4;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gas_intervals"); I(s); F(u1); F(u2); F(v1); F(v2); I(sh); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
      ATT(I(x->NbUIntervals(SHP[sh]));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); x->UIntervals(T, SHP[sh]); for (int i = 1; i <= 8; ++i) F(T(i));)
      ATT(I(x->NbVIntervals(SHP[sh]));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); x->VIntervals(T, SHP[sh]); for (int i = 1; i <= 8; ++i) F(T(i));))
  }
  // -- the typed accessors
  for (int j = 0; j < 84; ++j) {
    int s = j % NGS, r = (j / NGS) % 4;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gas_info"); I(s); F(u1); F(u2); F(v1); F(v2); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
      prSurfBasics(*x);
      ATT(PrPln(x->Plane());)
      ATT(PrCyl(x->Cylinder());)
      ATT(PrCone(x->Cone());)
      ATT(PrSph(x->Sphere());)
      ATT(PrTor(x->Torus());)
      ATT(I(x->UDegree());)
      ATT(I(x->NbUPoles());)
      ATT(I(x->VDegree());)
      ATT(I(x->NbVPoles());)
      ATT(I(x->NbUKnots());)
      ATT(I(x->NbVKnots());)
      B(x->IsURational());
      B(x->IsVRational());
      ATT(occ::handle<Geom_BezierSurface> b = x->Bezier(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(occ::handle<Geom_BSplineSurface> b = x->BSpline(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(A1(x->AxeOfRevolution());)
      ATT(D3(x->Direction());)
      ATT(prCurveHandle(x->BasisCurve());)
      ATT(prSurfHandle(x->BasisSurface());)
      ATT(F(x->OffsetValue());))
  }
  // -- resolution
  for (int j = 0; j < 56; ++j) {
    int s = j % NGS, r = (j / NGS) % 4;
    static const double R3[6] = {1.0e-3, 5.0, 1.0e-7, 0.5, 2.0, 100.0};
    double rr = R3[(j / 3) % 6];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gas_resolution"); I(s); F(u1); F(u2); F(v1); F(v2); F(rr); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
      ATT(F(x->UResolution(rr));)
      ATT(F(x->VResolution(rr));))
  }
  // -- UTrim, VTrim, ShallowCopy
  for (int j = 0; j < 42; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, tl = (j / 7) % 4;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double a1 = u1 + 0.2 * (u2 - u1), a2 = u1 + 0.8 * (u2 - u1), b1 = v1 + 0.3 * (v2 - v1), b2 = v1 + 0.7 * (v2 - v1);
    begin("gas_trim"); I(s); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv); F(a1); F(a2); F(b1); F(b2); bar();
    CASE(
      occ::handle<GeomAdaptor_Surface> x = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2, tu, tv);
      occ::handle<Adaptor3d_Surface> t = x->UTrim(a1, a2, 1.0e-3);
      prSurfBasics(*t);
      ATT(P3(t->Value(0.5 * (a1 + a2), 0.5 * (v1 + v2)));)
      occ::handle<Adaptor3d_Surface> w = x->VTrim(b1, b2, 2.0e-3);
      prSurfBasics(*w);
      ATT(P3(w->Value(0.5 * (u1 + u2), 0.5 * (b1 + b2)));)
      occ::handle<Adaptor3d_Surface> cp = x->ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(0.5 * (u1 + u2), 0.5 * (v1 + v2)));)
      ATT(I(cp->NbUIntervals(GeomAbs_C1)); I(cp->NbVIntervals(GeomAbs_C1));))
  }
}
