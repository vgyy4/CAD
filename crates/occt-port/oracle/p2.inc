
// ---- helpers for the curve families --------------------------------------------------------------------------------------
static void prCurveBasics(const Adaptor3d_Curve& a) {
  I((int)a.GetType());
  F(a.FirstParameter());
  F(a.LastParameter());
}
static void prD0(const Adaptor3d_Curve& a, double u) { P3(a.Value(u)); }
static void prCurveFull(const Adaptor3d_Curve& a) {
  prCurveBasics(a);
  B(a.IsClosed());
  B(a.IsPeriodic());
  ATT(F(a.Period());)
  ATT(I((int)a.Continuity());)
  ATT(F(a.Resolution(1.0e-3));)
}
static const double TFR[9][3] = {{0.0, 0.5, 1.0}, {0.31, 0.32, 0.9}, {1.0, 0.0, 0.5}, {0.5, 0.5, 0.5}, {0.25, 0.75, 0.26},
                                 {0.1, 0.999999, 1.0}, {0.5, 0.0, 1.0}, {0.9, 0.1, 0.9}, {0.0, 0.0, 0.0}};
// an interior knot of a B-spline curve (or the middle of the range)
static double knotOrMid(int c, double f, double l, int which) {
  const CS& s = GC[c];
  if (s.kind == 6 || (s.kind == 8 && GC[s.basis].kind == 6)) {
    const CS& b = s.kind == 6 ? s : GC[s.basis];
    std::vector<double> in;
    for (double k : b.k)
      if (k > f + 1.0e-3 && k < l - 1.0e-3) in.push_back(k);
    if (!in.empty()) return in[(size_t)which % in.size()];
  }
  return f + 0.5 * (l - f);
}

static void curveFamilies() {
  // -- the geometry itself
  for (int i = 0; i < NGC; ++i) {
    begin("def_gcurve"); I(i); bar();
    CASE(
      prCS(GC[i]);
      F(CC[i]->FirstParameter());
      F(CC[i]->LastParameter());
      double f, l;
      curveRange(i, 0, f, l);
      P3(CC[i]->EvalD0(f + 0.4 * (l - f)));)
  }
  // -- Load, the constructors, Reset
  for (int j = 0; j < 80; ++j) {
    int c = j % NGC, r = (j / NGC + j) % 4, v = (j / 3) % 5;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_load"); I(c); I(v); I(full); F(f); F(l); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a;
      auto load = [&](GeomAdaptor_Curve& x, int ci) {
        if (full) x.Load(CC[ci]);
        else {
          double ff, ll;
          curveRange(ci, r, ff, ll);
          x.Load(CC[ci], ff, ll);
        }
      };
      if (v == 0) { a = new GeomAdaptor_Curve(); load(*a, c); }
      else if (v == 1) { a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l); }
      else if (v == 2) { a = new GeomAdaptor_Curve(CC[c]); load(*a, c); }
      else if (v == 3) { a = new GeomAdaptor_Curve(CC[c], f, l); a->Reset(); }
      else { a = new GeomAdaptor_Curve(CC[(c + 1) % NGC]); load(*a, c); }
      if (v == 3) {
        prCurveBasics(*a);
        B(a->Curve().IsNull());
      } else {
        prCurveFull(*a);
        B(a->Curve().get() == CC[c].get());
      })
  }
  // -- invalid loads
  for (int j = 0; j < 12; ++j) {
    int c = (j * 5) % NGC, k = j % 4;
    double f, l;
    curveRange(c, j, f, l);
    begin("gac_load_bad"); I(c); I(k); F(f); F(l); bar();
    CASE(
      GeomAdaptor_Curve a;
      if (k == 0) a.Load(occ::handle<Geom_Curve>());
      else if (k == 1) a.Load(CC[c], l + 0.01, f);
      else if (k == 2) a.Load(CC[c], f, f - 0.5e-7);
      else a = GeomAdaptor_Curve(occ::handle<Geom_Curve>(), f, l);
      prCurveBasics(a);)
  }
  // -- evaluation
  for (int j = 0; j < 63; ++j) {
    int c = j % NGC, r = (j / NGC) % 4, pat = (j / 3 + j / 25) % 9;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double t[3];
    for (int k = 0; k < 3; ++k) t[k] = f + TFR[pat][k] * (l - f);
    if (j % 7 == 3) t[1] = knotOrMid(c, f, l, j);
    if (j % 7 == 5) { t[0] = knotOrMid(c, f, l, j); t[2] = knotOrMid(c, f, l, j + 1); }
    int ord = (j / 21 + j) % 4;
    begin("gac_eval"); I(ord); I(c); I(full); F(f); F(l); F(t[0]); F(t[1]); F(t[2]); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      for (double u : t) {
        std::function<void()> ev[4] = {
          [&] { ATT(P3(a->Value(u));) },
          [&] { ATT(gp_Pnt p; gp_Vec d1; a->D1(u, p, d1); P3(p); V3(d1);) },
          [&] { ATT(gp_Pnt p; gp_Vec d1, d2; a->D2(u, p, d1, d2); P3(p); V3(d1); V3(d2);) },
          [&] { ATT(gp_Pnt p; gp_Vec d1, d2, d3; a->D3(u, p, d1, d2, d3); P3(p); V3(d1); V3(d2); V3(d3);) }};
        for (int q = 0; q < 4; ++q) ev[(q + ord) % 4]();
        for (int n = 1; n <= 5; ++n) ATT(V3(a->DN(u, n));)
      })
  }
  // -- continuity intervals
  for (int j = 0; j < NGC * 7; ++j) {
    int c = j % NGC, s = j / NGC, r = (c * 3 + s) % 4;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_intervals"); I(c); I(full); F(f); F(l); I(s); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      ATT(I((int)a->Continuity());)
      ATT(I(a->NbIntervals(SHP[s]));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); a->Intervals(T, SHP[s]); for (int i = 1; i <= 8; ++i) F(T(i));))
  }
  // -- the typed accessors
  for (int j = 0; j < 63; ++j) {
    int c = j % NGC, r = (j / NGC) % 4;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_info"); I(c); I(full); F(f); F(l); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      prCurveBasics(*a);
      ATT(PrLin(a->Line());)
      ATT(PrCirc(a->Circle());)
      ATT(PrElips(a->Ellipse());)
      ATT(PrHypr(a->Hyperbola());)
      ATT(PrParab(a->Parabola());)
      ATT(I(a->Degree());)
      B(a->IsRational());
      ATT(I(a->NbPoles());)
      ATT(I(a->NbKnots());)
      ATT(occ::handle<Geom_BezierCurve> b = a->Bezier(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)););
      ATT(occ::handle<Geom_BSplineCurve> b = a->BSpline(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)); F(b->Knot(1)););
      ATT(occ::handle<Geom_OffsetCurve> b = a->OffsetCurve(); B(b.IsNull()); F(b->Offset()); D3(b->Direction());))
  }
  // -- resolution
  for (int j = 0; j < 42; ++j) {
    int c = j % NGC, r = (j / NGC) % 4;
    static const double R3[6] = {1.0e-3, 5.0, 1.0e-7, 0.5, 2.0, 100.0};
    double rr = R3[(j / 3) % 6];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("gac_resolution"); I(c); I(full); F(f); F(l); F(rr); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      ATT(F(a->Resolution(rr));))
  }
  // -- Trim and ShallowCopy
  for (int j = 0; j < 42; ++j) {
    int c = j % NGC, r = (j / NGC) % 4;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double t1 = f + 0.2 * (l - f), t2 = f + 0.7 * (l - f);
    double tol = (j % 2) ? 1.0e-3 : 0.0;
    begin("gac_trim"); I(c); I(full); F(f); F(l); F(t1); F(t2); F(tol); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      occ::handle<Adaptor3d_Curve> t = a->Trim(t1, t2, tol);
      prCurveBasics(*t);
      ATT(prD0(*t, 0.5 * (t1 + t2));)
      occ::handle<Adaptor3d_Curve> cp = a->ShallowCopy();
      prCurveBasics(*cp);
      ATT(prD0(*cp, 0.5 * (f + l));)
      ATT(I(cp->NbIntervals(GeomAbs_C1));))
  }
}
