
// ---- the transformations -------------------------------------------------------------------------------------------------
struct TS {
  int kind = 0;  // 0 identity, 1 translation, 2 rotation, 3 scale, 4 point mirror, 5 axis mirror, 6 plane mirror
  double p[3] = {0, 0, 0}, a[3] = {0, 0, 1}, v = 0;
};
static const int NTS = 7;
static TS TSD[NTS];
static void makeTrsfSpecs() {
  TSD[0].kind = 0;
  TSD[1].kind = 1; TSD[1].a[0] = 0.3; TSD[1].a[1] = -0.2; TSD[1].a[2] = 0.5;
  TSD[2].kind = 2; TSD[2].p[0] = 0.2; TSD[2].p[1] = -0.1; TSD[2].p[2] = 0.3; TSD[2].a[0] = 0.3; TSD[2].a[1] = 0.2; TSD[2].a[2] = 1.0; TSD[2].v = 0.7;
  TSD[3].kind = 3; TSD[3].p[0] = 0.1; TSD[3].p[1] = 0.4; TSD[3].p[2] = -0.2; TSD[3].v = 1.5;
  TSD[4].kind = 4; TSD[4].p[0] = 0.5; TSD[4].p[1] = 0.1; TSD[4].p[2] = 0.2;
  TSD[5].kind = 5; TSD[5].p[0] = 0.0; TSD[5].p[1] = 0.3; TSD[5].p[2] = 0.1; TSD[5].a[0] = 1.0; TSD[5].a[1] = 0.2; TSD[5].a[2] = 0.0;
  TSD[6].kind = 6; TSD[6].p[0] = 0.2; TSD[6].p[1] = 0.2; TSD[6].p[2] = 0.0; TSD[6].a[0] = 0.1; TSD[6].a[1] = 0.1; TSD[6].a[2] = 1.0;
}
static gp_Trsf trsfOf(const TS& t) {
  gp_Trsf r;
  gp_Pnt p(t.p[0], t.p[1], t.p[2]);
  switch (t.kind) {
    case 1: r.SetTranslation(gp_Vec(t.a[0], t.a[1], t.a[2])); break;
    case 2: r.SetRotation(gp_Ax1(p, gp_Dir(t.a[0], t.a[1], t.a[2])), t.v); break;
    case 3: r.SetScale(p, t.v); break;
    case 4: r.SetMirror(p); break;
    case 5: r.SetMirror(gp_Ax1(p, gp_Dir(t.a[0], t.a[1], t.a[2]))); break;
    case 6: r.SetMirror(gp_Ax2(p, gp_Dir(t.a[0], t.a[1], t.a[2]))); break;
    default: break;
  }
  return r;
}
static void prTS(const TS& t) {
  I(t.kind);
  for (double v : t.p) F(v);
  for (double v : t.a) F(v);
  F(t.v);
}

// ---- the 2D curves of the curves on surfaces -----------------------------------------------------------------------------
struct G2 {
  int kind;  // 0 line, 1 circle
  double p[2], d[2], r;
  double f, l;  // the range of the adaptor
};
static const int NG2 = 3;
static const G2 G2D[NG2] = {{0, {0.2, 0.1}, {1.0, 0.5}, 0.0, 0.0, 1.5},
                            {1, {1.2, 1.0}, {1.0, 0.0}, 0.6, 0.0, 5.5},
                            {0, {0.3, -1.0}, {0.2, 1.0}, 0.0, 0.0, 2.0}};
static occ::handle<Geom2d_Curve> C2[NG2];
static void prG2(const G2& g) {
  I(g.kind); F(g.p[0]); F(g.p[1]); F(g.d[0]); F(g.d[1]); F(g.r); F(g.f); F(g.l);
}
static const int NCP = 5;
static const int COSP[NCP][2] = {{0, 7}, {1, 7}, {0, 0}, {2, 1}, {1, 4}};
static occ::handle<Adaptor3d_CurveOnSurface> makeCos(int k) {
  int g = COSP[k][0], s = COSP[k][1];
  occ::handle<Geom2dAdaptor_Curve> c2 = new Geom2dAdaptor_Curve(C2[g], G2D[g].f, G2D[g].l);
  double u1, u2, v1, v2;
  surfRange(s, 0, u1, u2, v1, v2);
  occ::handle<GeomAdaptor_Surface> sa = new GeomAdaptor_Surface(SC[s], u1, u2, v1, v2);
  return new Adaptor3d_CurveOnSurface(c2, sa);
}

// ---- the class of a Geom object ---------------------------------------------------------------------------------------------
static int curveTag(const occ::handle<Geom_Curve>& c) {
  if (c.IsNull()) return -1;
  if (!occ::down_cast<Geom_Line>(c).IsNull()) return 0;
  if (!occ::down_cast<Geom_Circle>(c).IsNull()) return 1;
  if (!occ::down_cast<Geom_Ellipse>(c).IsNull()) return 2;
  if (!occ::down_cast<Geom_Parabola>(c).IsNull()) return 3;
  if (!occ::down_cast<Geom_Hyperbola>(c).IsNull()) return 4;
  if (!occ::down_cast<Geom_BezierCurve>(c).IsNull()) return 5;
  if (!occ::down_cast<Geom_BSplineCurve>(c).IsNull()) return 6;
  if (!occ::down_cast<Geom_TrimmedCurve>(c).IsNull()) return 7;
  if (!occ::down_cast<Geom_OffsetCurve>(c).IsNull()) return 8;
  return 9;
}
static int surfTag(const occ::handle<Geom_Surface>& c) {
  if (c.IsNull()) return -1;
  if (!occ::down_cast<Geom_Plane>(c).IsNull()) return 0;
  if (!occ::down_cast<Geom_CylindricalSurface>(c).IsNull()) return 1;
  if (!occ::down_cast<Geom_ConicalSurface>(c).IsNull()) return 2;
  if (!occ::down_cast<Geom_SphericalSurface>(c).IsNull()) return 3;
  if (!occ::down_cast<Geom_ToroidalSurface>(c).IsNull()) return 4;
  if (!occ::down_cast<Geom_BezierSurface>(c).IsNull()) return 5;
  if (!occ::down_cast<Geom_BSplineSurface>(c).IsNull()) return 6;
  if (!occ::down_cast<Geom_SurfaceOfRevolution>(c).IsNull()) return 7;
  if (!occ::down_cast<Geom_SurfaceOfLinearExtrusion>(c).IsNull()) return 8;
  if (!occ::down_cast<Geom_OffsetSurface>(c).IsNull()) return 9;
  if (!occ::down_cast<Geom_RectangularTrimmedSurface>(c).IsNull()) return 10;
  return 11;
}

static void transformedFamilies() {
  // -- the 2D curves
  for (int i = 0; i < NG2; ++i) {
    begin("def_gcurve2d"); I(i); bar();
    CASE(prG2(G2D[i]); F(C2[i]->FirstParameter()); F(C2[i]->LastParameter()); P2(C2[i]->Value(0.5 * (G2D[i].f + G2D[i].l)));)
  }
  // -- GeomAdaptor_TransformedCurve: evaluation and the common data
  for (int j = 0; j < 50; ++j) {
    int cs = j % 4 == 3 ? (j / 4) % NCP : -1;
    int c = j % NGC, r = (j / NGC + j / 4) % 4, tk = (j / 2 + j / 9) % NTS;
    double f, l;
    bool full = false;
    if (cs >= 0) { f = G2D[COSP[cs][0]].f; l = G2D[COSP[cs][0]].l; }
    else { curveRange(c, r, f, l); full = curveFull(c, r); }
    int pat = (j / 3 + j) % 9;
    double t[2] = {f + TFR[pat][0] * (l - f), f + TFR[pat][1] * (l - f)};
    begin("gtc_eval"); prTS(TSD[tk]); I(cs); I(cs >= 0 ? COSP[cs][0] : -1); I(cs >= 0 ? COSP[cs][1] : -1); I(cs >= 0 ? -1 : c); I(full); F(f); F(l); F(t[0]); F(t[1]); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedCurve> a;
      if (cs >= 0) { a = new GeomAdaptor_TransformedCurve(); a->SetTrsf(tr); a->LoadCurveOnSurface(makeCos(cs)); }
      else if (full) a = new GeomAdaptor_TransformedCurve(CC[c], tr);
      else a = new GeomAdaptor_TransformedCurve(CC[c], f, l, tr);
      prCurveBasics(*a);
      B(a->Is3DCurve());
      B(a->IsCurveOnSurface());
      B(a->IsClosed());
      B(a->IsPeriodic());
      ATT(F(a->Period());)
      ATT(I((int)a->Continuity());)
      ATT(I(a->NbIntervals(GeomAbs_C1));)
      ATT(NCollection_Array1<double> T(1, 8); T.Init(0.0); a->Intervals(T, GeomAbs_C1); for (int i = 1; i <= 8; ++i) F(T(i));)
      ATT(F(a->Resolution(1.0e-3));)
      for (double u : t) {
        ATT(P3(a->Value(u));)
        ATT(gp_Pnt p; gp_Vec d1; a->D1(u, p, d1); P3(p); V3(d1);)
        ATT(gp_Pnt p; gp_Vec d1, d2; a->D2(u, p, d1, d2); P3(p); V3(d1); V3(d2);)
        ATT(gp_Pnt p; gp_Vec d1, d2, d3; a->D3(u, p, d1, d2, d3); P3(p); V3(d1); V3(d2); V3(d3);)
        for (int n = 1; n <= 3; ++n) ATT(V3(a->DN(u, n));)
      })
  }
  // -- GeomAdaptor_TransformedCurve: the typed accessors
  for (int j = 0; j < 63; ++j) {
    int cs = j % 5 == 4 ? (j / 5) % NCP : -1;
    int c = j % NGC, r = (j / NGC) % 4, tk = (j / 3 + j / 11) % NTS;
    double f, l;
    bool full = false;
    if (cs >= 0) { f = G2D[COSP[cs][0]].f; l = G2D[COSP[cs][0]].l; }
    else { curveRange(c, r, f, l); full = curveFull(c, r); }
    begin("gtc_info"); I(j); prTS(TSD[tk]); I(cs); I(cs >= 0 ? COSP[cs][0] : -1); I(cs >= 0 ? COSP[cs][1] : -1); I(cs >= 0 ? -1 : c); I(full); F(f); F(l); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedCurve> a;
      if (cs >= 0) { a = new GeomAdaptor_TransformedCurve(); a->SetTrsf(tr); a->LoadCurveOnSurface(makeCos(cs)); }
      else if (full) a = new GeomAdaptor_TransformedCurve(CC[c], tr);
      else a = new GeomAdaptor_TransformedCurve(CC[c], f, l, tr);
      prCurveBasics(*a);
      ATT(PrLin(a->Line());)
      ATT(PrCirc(a->Circle());)
      ATT(PrElips(a->Ellipse());)
      ATT(PrHypr(a->Hyperbola());)
      ATT(PrParab(a->Parabola());)
      ATT(I(a->Degree());)
      ATT(B(a->IsRational());)
      ATT(I(a->NbPoles());)
      ATT(I(a->NbKnots());)
      if (cs < 0) {
        ATT(occ::handle<Geom_BezierCurve> b = a->Bezier(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)););
        ATT(occ::handle<Geom_BSplineCurve> b = a->BSpline(); B(b.IsNull()); I(b->NbPoles()); P3(b->Pole(1)); F(b->Knot(1)););
      }
      ATT(occ::handle<Geom_OffsetCurve> b = a->OffsetCurve(); B(b.IsNull()); F(b->Offset()); D3(b->Direction());)
      B(a->Curve().Curve().IsNull());
      B(a->GeomCurve().IsNull());)
  }
  // -- GeomAdaptor_TransformedCurve: copies, Trim, Load
  for (int j = 0; j < 40; ++j) {
    int cs = j % 4 == 2 ? (j / 4) % NCP : -1;
    int c = j % NGC, r = (j / NGC) % 4, tk = (j / 2) % NTS;
    double f, l;
    bool full = false;
    if (cs >= 0) { f = G2D[COSP[cs][0]].f; l = G2D[COSP[cs][0]].l; }
    else { curveRange(c, r, f, l); full = curveFull(c, r); }
    double t1 = f + 0.2 * (l - f), t2 = f + 0.7 * (l - f);
    begin("gtc_copy"); prTS(TSD[tk]); I(cs); I(cs >= 0 ? COSP[cs][0] : -1); I(cs >= 0 ? COSP[cs][1] : -1); I(c); I(full); F(f); F(l); F(t1); F(t2); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedCurve> a;
      if (cs >= 0) { a = new GeomAdaptor_TransformedCurve(); a->SetTrsf(tr); a->LoadCurveOnSurface(makeCos(cs)); }
      else if (full) a = new GeomAdaptor_TransformedCurve(CC[c], tr);
      else a = new GeomAdaptor_TransformedCurve(CC[c], f, l, tr);
      occ::handle<Adaptor3d_Curve> cp = a->ShallowCopy();
      prCurveBasics(*cp);
      ATT(prD0(*cp, 0.5 * (f + l));)
      occ::handle<Adaptor3d_Curve> tc = a->Trim(t1, t2, 1.0e-3);
      prCurveBasics(*tc);
      ATT(prD0(*tc, 0.5 * (t1 + t2));)
      ATT(F(occ::down_cast<GeomAdaptor_TransformedCurve>(tc)->Trsf().ScaleFactor());)
      // Load on a fresh object
      GeomAdaptor_TransformedCurve b;
      b.Load(CC[(c + 2) % NGC]);
      b.SetTrsf(tr);
      prCurveBasics(b);
      ATT(double uu = (Precision::IsInfinite(b.FirstParameter()) || Precision::IsInfinite(b.LastParameter())) ? 0.25 : 0.5 * (b.FirstParameter() + b.LastParameter()); prD0(b, uu);)
      if (!Precision::IsInfinite(CC[(c + 3) % NGC]->FirstParameter()) && !Precision::IsInfinite(CC[(c + 3) % NGC]->LastParameter())) {
        GeomAdaptor_TransformedCurve d;
        d.Load(CC[(c + 3) % NGC], CC[(c + 3) % NGC]->FirstParameter(), CC[(c + 3) % NGC]->LastParameter());
        prCurveBasics(d);
      })
  }
  // -- GeomAdaptor_TransformedSurface: evaluation and the common data
  for (int j = 0; j < 44; ++j) {
    int s = j % NGS, r = (j / NGS + j / 11) % 4, tk = (j / 3 + j / 7) % NTS, tl = (j / 5) % 4;
    double tu = STOL[tl][0], tv = STOL[tl][1];
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double pt[2][2];
    pt[0][0] = u1 + SFR[p1][0] * (u2 - u1); pt[0][1] = v1 + SFR[p1][1] * (v2 - v1);
    pt[1][0] = u1 + SFR[p2][0] * (u2 - u1); pt[1][1] = v1 + SFR[p2][1] * (v2 - v1);
    begin("gts_eval"); prTS(TSD[tk]); I(s); F(u1); F(u2); F(v1); F(v2); F(tu); F(tv);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedSurface> x = new GeomAdaptor_TransformedSurface(SC[s], u1, u2, v1, v2, tr, tu, tv);
      prSurfFull(*x);
      F(x->ToleranceU()); F(x->ToleranceV());
      B(x->HasTrsf());
      for (auto& q : pt) {
        double u = q[0], v = q[1];
        ATT(P3(x->Value(u, v));)
        ATT(gp_Pnt p; gp_Vec a, b; x->D1(u, v, p, a, b); P3(p); V3(a); V3(b);)
        ATT(gp_Pnt p; gp_Vec a, b, c, d, e; x->D2(u, v, p, a, b, c, d, e); P3(p); V3(a); V3(b); V3(c); V3(d); V3(e);)
        ATT(gp_Pnt p; gp_Vec a, b, c, d, e, f, g, h, k; x->D3(u, v, p, a, b, c, d, e, f, g, h, k); P3(p); V3(a); V3(b); V3(c); V3(d);
            V3(e); V3(f); V3(g); V3(h); V3(k);)
        static const int NN[5][2] = {{1, 0}, {0, 1}, {1, 1}, {2, 1}, {0, 2}};
        for (auto& n : NN) ATT(V3(x->DN(u, v, n[0], n[1]));)
      })
  }
  // -- GeomAdaptor_TransformedSurface: the typed accessors
  for (int j = 0; j < 56; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, tk = (j / 4 + j / 13) % NTS;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    begin("gts_info"); prTS(TSD[tk]); I(s); F(u1); F(u2); F(v1); F(v2); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedSurface> x = new GeomAdaptor_TransformedSurface(SC[s], u1, u2, v1, v2, tr);
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
      ATT(B(x->IsURational());)
      ATT(B(x->IsVRational());)
      ATT(occ::handle<Geom_BezierSurface> b = x->Bezier(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(occ::handle<Geom_BSplineSurface> b = x->BSpline(); B(b.IsNull()); I(b->NbUPoles()); I(b->NbVPoles()); P3(b->Pole(1, 1)););
      ATT(A1(x->AxeOfRevolution());)
      ATT(D3(x->Direction());)
      ATT(prCurveHandle(x->BasisCurve());)
      ATT(prSurfHandle(x->BasisSurface());)
      ATT(F(x->OffsetValue());)
      ATT(I(surfTag(x->GeomSurfaceTransformed())); I((int)x->AdaptorSurfaceTransformed().GetType());)
      I(surfTag(x->GeomSurfaceOriginal()));
      I((int)x->AdaptorSurfaceOriginal().GetType());)
  }
  // -- GeomAdaptor_TransformedSurface: intervals, Trim, copies, Load, SetTrsf
  for (int j = 0; j < 42; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, tk = (j / 3) % NTS, sh = (j / 5) % 7;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    double a1 = u1 + 0.2 * (u2 - u1), a2 = u1 + 0.8 * (u2 - u1), b1 = v1 + 0.3 * (v2 - v1), b2 = v1 + 0.7 * (v2 - v1);
    begin("gts_trim"); prTS(TSD[tk]); prTS(TSD[(tk + 2) % NTS]); I(s); F(u1); F(u2); F(v1); F(v2); F(a1); F(a2); F(b1); F(b2); I(sh); bar();
    CASE(
      gp_Trsf tr = trsfOf(TSD[tk]);
      occ::handle<GeomAdaptor_TransformedSurface> x = new GeomAdaptor_TransformedSurface(SC[s], u1, u2, v1, v2, tr);
      ATT(int nb = x->NbUIntervals(SHP[sh]); I(nb); NCollection_Array1<double> T(1, nb + 1); x->UIntervals(T, SHP[sh]);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      ATT(int nb = x->NbVIntervals(SHP[sh]); I(nb); NCollection_Array1<double> T(1, nb + 1); x->VIntervals(T, SHP[sh]);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      occ::handle<Adaptor3d_Surface> t = x->UTrim(a1, a2, 1.0e-3);
      prSurfBasics(*t);
      ATT(P3(t->Value(0.5 * (a1 + a2), 0.5 * (v1 + v2)));)
      occ::handle<Adaptor3d_Surface> w = x->VTrim(b1, b2, 2.0e-3);
      prSurfBasics(*w);
      occ::handle<Adaptor3d_Surface> cp = x->ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(0.5 * (u1 + u2), 0.5 * (v1 + v2)));)
      GeomAdaptor_TransformedSurface y;
      y.Load(SC[(s + 3) % NGS], tr);
      prSurfBasics(y);
      y.SetTrsf(trsfOf(TSD[(tk + 2) % NTS]));
      B(y.HasTrsf());
      ATT(P3(y.Value(0.25, 0.5)););
      GeomAdaptor_TransformedSurface z;
      z.Load(SC[s], u1, u2, v1, v2, tr);
      prSurfBasics(z);
      GeomAdaptor_TransformedSurface z2(SC[s], tr);
      prSurfBasics(z2);
      GeomAdaptor_TransformedSurface z3;
      z3.SetTrsf(tr);
      ATT(PrPln(z3.Plane());)
      B(z3.HasTrsf());
      ATT(F(z.UResolution(1.0e-3)); F(z.VResolution(1.0e-3));))
  }
}
