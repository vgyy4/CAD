
// ---- GeomAdaptor_SurfaceOfRevolution and GeomAdaptor_SurfaceOfLinearExtrusion --------------------------------------------
struct AXS {
  double loc[3], dir[3];
};
static AXS axisOf(int k) {
  AXS a = {{0, 0, 0}, {0, 0, 1}};
  auto set = [&](double lx, double ly, double lz, double dx, double dy, double dz) {
    a.loc[0] = lx; a.loc[1] = ly; a.loc[2] = lz;
    a.dir[0] = dx; a.dir[1] = dy; a.dir[2] = dz;
  };
  switch (k) {
    case 0: set(0.2, 0.1, -0.3, 0.0, 0.0, 1.0); break;
    case 1: set(0.6, 0.4, 0.1, 0.6, 0.0, 0.8); break;
    case 2: set(0.3, 0.2, 0.1, 0.8, 0.0, -0.6); break;
    case 3: set(0.1, -0.2, 0.3, 0.0, 0.0, 1.0); break;
    case 4: {
      gp_Ax1 x = occ::down_cast<Geom_Circle>(CC[1])->XAxis();
      set(x.Location().X(), x.Location().Y(), x.Location().Z(), x.Direction().X(), x.Direction().Y(), x.Direction().Z());
      break;
    }
    case 5: {
      occ::handle<Geom_Circle> c = occ::down_cast<Geom_Circle>(CC[1]);
      gp_Ax1 x = c->XAxis();
      gp_Dir y = c->Position().YDirection();
      set(x.Location().X() + 2.5 * y.X(), x.Location().Y() + 2.5 * y.Y(), x.Location().Z() + 2.5 * y.Z(), x.Direction().X(), x.Direction().Y(),
          x.Direction().Z());
      break;
    }
    case 6: set(1.0, -0.5, 0.3, 0.3, 0.4, 0.2); break;
    case 7: set(-1.0, 0.0, 0.5, 0.1, 1.0, 0.2); break;
    case 9: set(0.6, 0.4, 0.1, -0.6, 0.0, -0.8); break;
    case 10: set(0.6, 0.4, 0.1, 0.6, 3.0e-4, 0.8); break;
    case 11: set(0.3, 0.2, 0.1, 0.8 + 6.0e-9, 0.0, -0.6 + 8.0e-9); break;
    case 12: set(0.4, -0.2, 0.7, 0.0, 0.0, 1.0); break;
    default: set(0.1, -0.2, 0.3, 0.6, 0.0, 0.8); break;  // along the line
  }
  return a;
}
static void prAx(const AXS& a) { for (double v : a.loc) F(v); for (double v : a.dir) F(v); }
static gp_Ax1 ax1Of(const AXS& a) { return gp_Ax1(gp_Pnt(a.loc[0], a.loc[1], a.loc[2]), gp_Dir(a.dir[0], a.dir[1], a.dir[2])); }
// (curve, axis) pairs for the revolution
static const int NRV = 24;
static const int RVC[NRV][2] = {{0, 1}, {0, 2}, {0, 3}, {1, 4}, {1, 5}, {1, 0}, {1, 6}, {7, 0}, {7, 7}, {5, 0},
                                {9, 7}, {2, 0}, {4, 0}, {3, 0}, {0, 0}, {0, 6}, {0, 8}, {14, 6}, {12, 0}, {13, 7},
                                {0, 9}, {0, 10}, {0, 11}, {0, 12}};
static occ::handle<GeomAdaptor_Curve> basisCurve(int c, int variant, double f, double l) {
  if (variant == 1) return new GeomAdaptor_Curve(CC[c]);
  return new GeomAdaptor_Curve(CC[c], f, l);
}
// the directions of the extrusion
static const int NEX = 15;
static const int EXC[NEX][2] = {{0, 0}, {0, 3}, {1, 1}, {1, 2}, {2, 2}, {2, 1}, {4, 2}, {3, 2}, {7, 0}, {5, 0}, {9, 2}, {1, 0}, {13, 0}, {12, 1}, {1, 4}};
static void dirOf(int c, int k, double d[3]) {
  d[0] = 0.3; d[1] = -0.2; d[2] = 0.9;
  occ::handle<Geom_Conic> cn = occ::down_cast<Geom_Conic>(CC[c]);
  if (k == 3) { d[0] = 0.6; d[1] = 0.0; d[2] = 0.8; }
  else if (k == 4 && !cn.IsNull()) { d[0] = -cn->Position().Direction().X(); d[1] = -cn->Position().Direction().Y(); d[2] = -cn->Position().Direction().Z(); }
  else if (k == 1 && !cn.IsNull()) { d[0] = cn->Position().Direction().X(); d[1] = cn->Position().Direction().Y(); d[2] = cn->Position().Direction().Z(); }
  else if (k == 2 && !cn.IsNull()) { d[0] = cn->Position().XDirection().X(); d[1] = cn->Position().XDirection().Y(); d[2] = cn->Position().XDirection().Z(); }
}

static void prRevInfo(const GeomAdaptor_SurfaceOfRevolution& x) {
  prSurfFull(x);
  ATT(A1(x.AxeOfRevolution());)
  ATT(A3(x.Axis());)
  ATT(prCurveHandle(x.BasisCurve());)
}
static void prExtInfo(const GeomAdaptor_SurfaceOfLinearExtrusion& x) {
  prSurfFull(x);
  ATT(D3(x.Direction());)
  ATT(prCurveHandle(x.BasisCurve());)
}
template <class S>
static void prTypedInfo(const S& x) {
  ATT(PrPln(x.Plane());)
  ATT(PrCyl(x.Cylinder());)
  ATT(PrCone(x.Cone());)
  ATT(PrSph(x.Sphere());)
  ATT(PrTor(x.Torus());)
  ATT(I(x.UDegree());)
  ATT(I(x.NbUPoles());)
  ATT(I(x.VDegree());)
  ATT(I(x.NbVPoles());)
  ATT(I(x.NbUKnots());)
  ATT(I(x.NbVKnots());)
  ATT(B(x.IsURational());)
  ATT(B(x.IsVRational());)
  ATT(occ::handle<Geom_BezierSurface> b = x.Bezier(); B(b.IsNull()););
  ATT(occ::handle<Geom_BSplineSurface> b = x.BSpline(); B(b.IsNull()););
  ATT(A1(x.AxeOfRevolution());)
  ATT(D3(x.Direction());)
  ATT(prSurfHandle(x.BasisSurface());)
  ATT(F(x.OffsetValue());)
}
template <class S>
static void prEval(const S& x, double pt[2][2]) {
  for (int k = 0; k < 2; ++k) {
    double u = pt[k][0], v = pt[k][1];
    ATT(P3(x.Value(u, v));)
    ATT(gp_Pnt p; gp_Vec a, b; x.D1(u, v, p, a, b); P3(p); V3(a); V3(b);)
    ATT(gp_Pnt p; gp_Vec a, b, c, d, e; x.D2(u, v, p, a, b, c, d, e); P3(p); V3(a); V3(b); V3(c); V3(d); V3(e);)
    ATT(gp_Pnt p; gp_Vec a, b, c, d, e, f, g, h, kk; x.D3(u, v, p, a, b, c, d, e, f, g, h, kk); P3(p); V3(a); V3(b); V3(c); V3(d);
        V3(e); V3(f); V3(g); V3(h); V3(kk);)
    static const int NN[6][2] = {{1, 0}, {0, 1}, {1, 1}, {2, 1}, {0, 2}, {0, 0}};
    for (auto& n : NN) ATT(V3(x.DN(u, v, n[0], n[1]));)
  }
}

static void sweptFamilies() {
  // -- the revolution adaptor
  for (int j = 0; j < 60; ++j) {
    int pair = j % NRV, r = (j / NRV + j / 3) % 4;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    begin("gsr_new"); I(c); I(full); F(f); F(l); prAx(ax); I(j % 3); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      int v = j % 3;
      occ::handle<GeomAdaptor_SurfaceOfRevolution> x;
      if (v == 0) x = new GeomAdaptor_SurfaceOfRevolution(hc, ax1Of(ax));
      else if (v == 1) { x = new GeomAdaptor_SurfaceOfRevolution(); x->Load(hc); x->Load(ax1Of(ax)); }
      else { x = new GeomAdaptor_SurfaceOfRevolution(hc); x->Load(ax1Of(ax)); }
      prRevInfo(*x);
      ATT(int nb = x->NbVIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->VIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      ATT(int nb = x->NbUIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->UIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));))
  }
  for (int j = 0; j < 48; ++j) {
    int pair = (j + j / NRV) % NRV, r = (j / NRV + j / 5) % 4;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double pt[2][2];
    pt[0][0] = 6.283185307179586 * SFR[p1][0]; pt[0][1] = f + SFR[p1][1] * (l - f);
    pt[1][0] = 6.283185307179586 * SFR[p2][0]; pt[1][1] = f + SFR[p2][1] * (l - f);
    begin("gsr_eval"); I(c); I(full); F(f); F(l); prAx(ax);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfRevolution x(hc, ax1Of(ax));
      prEval(x, pt);)
  }
  for (int j = 0; j < NRV * 3; ++j) {
    int pair = j % NRV, r = j / NRV;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    begin("gsr_info"); I(c); I(full); F(f); F(l); prAx(ax); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfRevolution x(hc, ax1Of(ax));
      prSurfBasics(x);
      prTypedInfo(x);)
  }
  for (int j = 0; j < 40; ++j) {
    int pair = (j * 7 + j / NRV) % NRV, r = (j / NRV + 1) % 4;
    int c = RVC[pair][0], k = RVC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    AXS ax = axisOf(k);
    double t1 = f + 0.2 * (l - f), t2 = f + 0.75 * (l - f);
    int c2 = (c + 1) % NGC;
    double f2, l2;
    curveRange(c2, 0, f2, l2);
    AXS ax2 = axisOf((k + 1) % 8);
    begin("gsr_trim"); I(c); I(full); F(f); F(l); prAx(ax); F(t1); F(t2); I(c2); F(f2); F(l2); prAx(ax2); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfRevolution x(hc, ax1Of(ax));
      occ::handle<Adaptor3d_Surface> u = x.UTrim(0.0, 2.0 * M_PI, 1.0e-3);
      prSurfBasics(*u);
      occ::handle<Adaptor3d_Surface> w = x.VTrim(t1, t2, 1.0e-3);
      prSurfBasics(*w);
      ATT(P3(w->Value(1.0, 0.5 * (t1 + t2)));)
      occ::handle<Adaptor3d_Surface> cp = x.ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(2.0, 0.5 * (f + l)));)
      GeomAdaptor_SurfaceOfRevolution y;
      y.Load(hc);
      y.Load(ax1Of(ax2));
      prSurfBasics(y);
      y.Load(basisCurve(c2, 0, f2, l2));
      prSurfBasics(y);)
  }
  // -- the extrusion adaptor
  for (int j = 0; j < 56; ++j) {
    int pair = j % NEX, r = (j / NEX + j / 3) % 4;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double d[3];
    dirOf(c, k, d);
    begin("gse_new"); I(j); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]); I(j % 3); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      int v = j % 3;
      gp_Dir dir(d[0], d[1], d[2]);
      occ::handle<GeomAdaptor_SurfaceOfLinearExtrusion> x;
      if (v == 0) x = new GeomAdaptor_SurfaceOfLinearExtrusion(hc, dir);
      else if (v == 1) { x = new GeomAdaptor_SurfaceOfLinearExtrusion(); x->Load(hc); x->Load(dir); }
      else { x = new GeomAdaptor_SurfaceOfLinearExtrusion(hc); x->Load(dir); }
      prExtInfo(*x);
      ATT(int nb = x->NbUIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->UIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));)
      ATT(int nb = x->NbVIntervals(GeomAbs_C2); I(nb); NCollection_Array1<double> T(1, nb + 1); x->VIntervals(T, GeomAbs_C2);
          for (int i = 1; i <= nb + 1; ++i) F(T(i));))
  }
  for (int j = 0; j < 44; ++j) {
    int pair = (j + j / NEX) % NEX, r = (j / NEX + j / 5) % 4;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double d[3];
    dirOf(c, k, d);
    int p1 = (j / 3 + j) % 10, p2 = (j / 7 + 2 * j + 1) % 10;
    double pt[2][2];
    pt[0][0] = f + SFR[p1][0] * (l - f); pt[0][1] = -1.0 + 2.5 * SFR[p1][1];
    pt[1][0] = f + SFR[p2][0] * (l - f); pt[1][1] = -1.0 + 2.5 * SFR[p2][1];
    begin("gse_eval"); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]);
    for (auto& q : pt) { F(q[0]); F(q[1]); }
    bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfLinearExtrusion x(hc, gp_Dir(d[0], d[1], d[2]));
      prEval(x, pt);)
  }
  for (int j = 0; j < NEX * 4; ++j) {
    int pair = j % NEX, r = j / NEX;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r) || (r % 2 == 1 && (c == 0 || c == 3 || c == 4));
    double d[3];
    dirOf(c, k, d);
    begin("gse_info"); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfLinearExtrusion x(hc, gp_Dir(d[0], d[1], d[2]));
      prSurfBasics(x);
      prTypedInfo(x);)
  }
  for (int j = 0; j < 40; ++j) {
    int pair = (j * 5 + j / NEX) % NEX, r = (j / NEX + 1) % 4;
    int c = EXC[pair][0], k = EXC[pair][1];
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    double d[3];
    dirOf(c, k, d);
    double t1 = f + 0.2 * (l - f), t2 = f + 0.75 * (l - f);
    begin("gse_trim"); I(j); I(c); I(full); F(f); F(l); F(d[0]); F(d[1]); F(d[2]); F(t1); F(t2); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> hc = full ? basisCurve(c, 1, f, l) : basisCurve(c, 0, f, l);
      GeomAdaptor_SurfaceOfLinearExtrusion x(hc, gp_Dir(d[0], d[1], d[2]));
      occ::handle<Adaptor3d_Surface> u = x.UTrim(t1, t2, 1.0e-3);
      prSurfBasics(*u);
      occ::handle<Adaptor3d_Surface> w = x.VTrim(t1, t2, 1.0e-3);
      prSurfBasics(*w);
      ATT(P3(w->Value(0.5 * (t1 + t2), 0.5));)
      occ::handle<Adaptor3d_Surface> cp = x.ShallowCopy();
      prSurfBasics(*cp);
      ATT(P3(cp->Value(0.5 * (f + l), 0.25));)
      GeomAdaptor_SurfaceOfLinearExtrusion y;
      y.Load(hc);
      y.Load(gp_Dir(0.0, 0.3, 1.0));
      prSurfBasics(y);
      y.Load(hc);
      prSurfBasics(y);)
  }
}

// ---- GeomAdaptor::MakeCurve, GeomAdaptor::MakeSurface --------------------------------------------------------------------
static void makeFamilies() {
  for (int j = 0; j < 63; ++j) {
    int c = j % NGC, r = (j / NGC) % 4, v = (j / 3 + j / NGC) % 3;
    double f, l;
    curveRange(c, r, f, l);
    bool full = curveFull(c, r);
    begin("make_curve"); I(c); I(v); I(full); F(f); F(l); bar();
    CASE(
      occ::handle<GeomAdaptor_Curve> a = full ? new GeomAdaptor_Curve(CC[c]) : new GeomAdaptor_Curve(CC[c], f, l);
      occ::handle<Geom_Curve> g;
      if (v == 0) g = GeomAdaptor::MakeCurve(*a);
      else if (v == 1) {
        GeomAdaptor_TransformedCurve t(CC[c], f, l, gp_Trsf());
        g = GeomAdaptor::MakeCurve(t);
      } else {
        occ::handle<Adaptor3d_Curve> t = a->Trim(f + 0.1 * (l - f), l - 0.2 * (l - f), 0.0);
        g = GeomAdaptor::MakeCurve(*t);
      }
      I(curveTag(g));
      F(g->FirstParameter());
      F(g->LastParameter());
      occ::handle<Geom_TrimmedCurve> tc = occ::down_cast<Geom_TrimmedCurve>(g);
      if (!tc.IsNull()) I(curveTag(tc->BasisCurve()));
      double m = f + 0.5 * (l - f);
      P3(g->EvalD0(m));)
  }
  for (int j = 0; j < 84; ++j) {
    int s = j % NGS, r = (j / NGS) % 4, v = (j / 3) % 4, trim = (j / 5) % 2;
    double u1, u2, v1, v2;
    surfRange(s, r, u1, u2, v1, v2);
    int cc = s % NGC;
    double ff, ll, dd[3];
    curveRange(cc, 1, ff, ll);
    dirOf(cc, s % 4, dd);
    AXS axm = axisOf(s % 9);
    begin("make_surface"); I(s); I(v); I(trim); F(u1); F(u2); F(v1); F(v2); I(cc); F(ff); F(ll); F(dd[0]); F(dd[1]); F(dd[2]); prAx(axm); bar();
    CASE(
      occ::handle<Geom_Surface> g;
      if (v == 0) {
        GeomAdaptor_Surface a(SC[s], u1, u2, v1, v2);
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      } else if (v == 1) {
        GeomAdaptor_TransformedSurface a(SC[s], u1, u2, v1, v2, gp_Trsf());
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      } else if (v == 2) {
        GeomAdaptor_SurfaceOfRevolution a(new GeomAdaptor_Curve(CC[cc], ff, ll), ax1Of(axm));
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      } else {
        GeomAdaptor_SurfaceOfLinearExtrusion a(new GeomAdaptor_Curve(CC[cc], ff, ll), gp_Dir(dd[0], dd[1], dd[2]));
        g = GeomAdaptor::MakeSurface(a, trim != 0);
      }
      I(surfTag(g));
      double a1, a2, b1, b2;
      g->Bounds(a1, a2, b1, b2);
      F(a1); F(a2); F(b1); F(b2);
      occ::handle<Geom_RectangularTrimmedSurface> tc = occ::down_cast<Geom_RectangularTrimmedSurface>(g);
      if (!tc.IsNull()) I(surfTag(tc->BasisSurface()));
      ATT(P3(g->EvalD0(u1 + 0.4 * (u2 - u1), v1 + 0.3 * (v2 - v1)));))
  }
}

int main() {
  makeCurveSpecs();
  for (int i = 0; i < NGC; ++i) CC[i] = buildCurve(i);
  makeSurfaceSpecs();
  for (int i = 0; i < NGS; ++i) SC[i] = buildSurface(i);
  makeTrsfSpecs();
  C2[0] = new Geom2d_Line(gp_Pnt2d(G2D[0].p[0], G2D[0].p[1]), gp_Dir2d(G2D[0].d[0], G2D[0].d[1]));
  C2[1] = new Geom2d_Circle(gp_Ax2d(gp_Pnt2d(G2D[1].p[0], G2D[1].p[1]), gp_Dir2d(G2D[1].d[0], G2D[1].d[1])), G2D[1].r);
  C2[2] = new Geom2d_Line(gp_Pnt2d(G2D[2].p[0], G2D[2].p[1]), gp_Dir2d(G2D[2].d[0], G2D[2].d[1]));
  curveFamilies();
  surfaceFamilies();
  transformedFamilies();
  sweptFamilies();
  makeFamilies();
  return 0;
}
