// OCCT 8.0.1 math package oracle. Floating point fields are exact f64 bit patterns.
#include <math.hxx>
#include <math_Vector.hxx>
#include <math_IntegerVector.hxx>
#include <math_Matrix.hxx>
#include <math_Recipes.hxx>
#include <math_Gauss.hxx>
#include <math_GaussLeastSquare.hxx>
#include <math_Crout.hxx>
#include <math_SVD.hxx>
#include <math_Jacobi.hxx>
#include <math_DirectPolynomialRoots.hxx>
#include <math_ComputeGaussPointsAndWeights.hxx>
#include <math_ComputeKronrodPointsAndWeights.hxx>
#include <math_DoubleTab.hxx>
#include <math_Householder.hxx>
#include <math_EigenValuesSearcher.hxx>
#include <math_Status.hxx>
#include <gp_XY.hxx>
#include <gp_XYZ.hxx>
#include <Standard_Failure.hxx>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <sstream>
#include <map>
#include <string>
#include <vector>
#include <unistd.h>
static uint64_t state=0x9E3779B97F4A7C15ull;
static double rnd(double lo=-5.,double hi=5.){state^=state<<13;state^=state>>7;state^=state<<17;return lo+(hi-lo)*(double)(state>>11)/9007199254740992.;}
static void h(double x){uint64_t b;std::memcpy(&b,&x,8);std::printf(" %016llx",(unsigned long long)b);}
static void i(int x){std::printf(" %d",x);} static void b(bool x){std::printf(" %d",x?1:0);}
static void v(const math_Vector& x){i(x.Lower());i(x.Upper());for(int k=x.Lower();k<=x.Upper();++k)h(x(k));}
static void iv(const math_IntegerVector& x){i(x.Lower());i(x.Upper());for(int k=x.Lower();k<=x.Upper();++k)i(x(k));}
static void m(const math_Matrix& x){i(x.LowerRow());i(x.UpperRow());i(x.LowerCol());i(x.UpperCol());for(int r=x.LowerRow();r<=x.UpperRow();++r)for(int c=x.LowerCol();c<=x.UpperCol();++c)h(x(r,c));}
#define BEGIN(n) std::printf("%s",n)
#define END() std::printf("\n")
static int run_oracle(){
 for(int n=1;n<=20;++n){math_Vector x(1,n),w(1,n);bool ok=math::OrderedGaussPointsAndWeights(n,x,w);BEGIN("limits");i(n);std::printf(" |");i(math::GaussPointsMax());i(math::KronrodPointsMax());b(ok);END();}
 for(int n=1;n<=math::GaussPointsMax()+2;++n){math_Vector x(1,n),w(1,n);bool ok=math::OrderedGaussPointsAndWeights(n,x,w);BEGIN("ordered_gauss");i(n);b(ok);std::printf(" |");v(x);v(w);END();if(n<=math::GaussPointsMax()){BEGIN("gauss_table");i(n);std::printf(" |");math::GaussPoints(n,x);math::GaussWeights(n,w);v(x);v(w);END();}}
 for(int n=3;n<=41;n+=2){math_Vector x(1,n),w(1,n);bool ok=math::KronrodPointsAndWeights(n,x,w);BEGIN("kronrod");i(n);b(ok);std::printf(" |");v(x);v(w);END();math_ComputeKronrodPointsAndWeights q(n);BEGIN("compute_kronrod");i(n);b(q.IsDone());std::printf(" |");if(q.IsDone()){v(q.Points());v(q.Weights());}END();}
 for(int n=1;n<=20;++n){math_ComputeGaussPointsAndWeights q(n);BEGIN("compute_gauss");i(n);b(q.IsDone());std::printf(" |");if(q.IsDone()){v(q.Points());v(q.Weights());}END();}
 for(int k=0;k<20;++k){int n=k%6+3;math_Vector a(-2,-3+n),c(-2,-3+n),d(-2,-3+n);for(int j=-2;j<=-3+n;++j){a(j)=rnd();c(j)=rnd();}if(k==0)for(int j=-2;j<=-3+n;++j)a(j)=0.;BEGIN("vector");i(k);v(a);v(c);std::printf(" |");h(a.Norm());h(a.Norm2());i(a.Max());i(a.Min());v(a.Added(c));v(a.Subtracted(c));v(a.Multiplied(-2.));v(a.Divided(2.));h(a.Multiplied(c));v(a.Inverse());v(a.Opposite());d=a;d.Add(c);v(d);d=a;d.Subtract(c);v(d);d=a;d.Multiply(-2.);v(d);d=a;d.Divide(2.);v(d);d=a;d.Invert();v(d);try{d=a;d.Normalize();v(d);}catch(Standard_Failure&){std::printf(" throws");}END();}
 for(int k=0;k<20;++k){int nr=k%8+1,nc=(k*3)%8+1;math_Matrix a(-1,-2+nr,2,1+nc,0.),c(-1,-2+nr,2,1+nc,0.);for(int r=-1;r<=-2+nr;++r)for(int j=2;j<=1+nc;++j){a(r,j)=rnd();c(r,j)=rnd();}if(k==0){a.Init(0.);for(int r=-1;r<=-2+nr && r+3<=1+nc;++r)a(r,r+3)=1.;}BEGIN("matrix_ops");i(k);m(a);m(c);std::printf(" |");i(a.RowNumber());i(a.ColNumber());m(a.Added(c));m(a.Subtracted(c));m(a.Multiplied(-2.));m(a.Divided(2.));math_Matrix d=a;d.Add(c);m(d);d=a;d.Subtract(c);m(d);d=a;d.Multiply(-2);m(d);d=a;d.Divide(2);m(d);END();}
 for(int k=0;k<20;++k){int n=k%7+2;math_Matrix a(1,n,1,n,0.);for(int r=1;r<=n;++r)for(int c=1;c<=n;++c)a(r,c)=rnd();for(int j=1;j<=n;++j)a(j,j)+=8.;if(k==0){a.Init(0.);for(int j=1;j<=n;++j)a(j,j)=1.;}math_Vector rhs(1,n),x(1,n);for(int j=1;j<=n;++j)rhs(j)=rnd();BEGIN("matrix_linear");i(k);m(a);v(rhs);std::printf(" |");h(a.Determinant());v(a.Multiplied(rhs));m(a.Transposed());m(a.TMultiply(a));m(a.Multiplied(a));math_Matrix inv=a;inv.Invert();m(inv);END();}
 for(int k=0;k<20;++k){int n=k%7+2;math_Matrix a(1,n,1,n,0.);for(int r=1;r<=n;++r)for(int c=1;c<=n;++c)a(r,c)=rnd();for(int j=1;j<=n;++j)a(j,j)+=6.;math_IntegerVector ix(1,n);double sign=0.;math_Matrix a0=a;int status=LU_Decompose(a,ix,sign);BEGIN("lu");i(k);m(a0);std::printf(" |");i(status);h(sign);m(a);iv(ix);END();math_Vector rhs(1,n);for(int j=1;j<=n;++j)rhs(j)=rnd();math_Vector rhs0=rhs;LU_Solve(a,ix,rhs);BEGIN("lu_solve");i(k);m(a);iv(ix);v(rhs0);std::printf(" |");v(rhs);END();}
 for(int k=0;k<20;++k){int n=k%7+2;math_Matrix a(1,n,1,n,0.),vm(1,n,1,n,0.);math_Vector w(1,n),rhs(1,n),x(1,n);for(int r=1;r<=n;++r)for(int c=1;c<=n;++c)a(r,c)=rnd();for(int j=1;j<=n;++j)rhs(j)=rnd();math_Matrix a0=a;int st=SVD_Decompose(a,w,vm);SVD_Solve(a,w,vm,rhs,x);BEGIN("svd");i(k);m(a0);v(rhs);std::printf(" |");i(st);m(a);v(w);m(vm);v(x);END();}
 for(int k=0;k<20;++k){int n=k%8+1;math_Matrix a(1,n,1,n,0.),vm(1,n,1,n,0.);math_Vector d(1,n);for(int r=1;r<=n;++r)for(int c=r;c<=n;++c){double q=rnd();a(r,c)=q;a(c,r)=q;}math_Matrix a0=a;int rot=0,st=Jacobi(a,d,vm,rot);BEGIN("jacobi");i(k);m(a0);std::printf(" |");i(st);i(rot);v(d);m(vm);m(a);END();}
 for(int k=0;k<20;++k){math_Matrix a(1,3,1,3,0.);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c)a(r,c)=rnd();for(int j=1;j<=3;++j)a(j,j)+=5.;double aa=rnd(.1,3),bb=rnd(-5,5),cc=rnd(-5,5),dd=rnd(-5,5);math_DirectPolynomialRoots p(aa,bb,cc,dd);BEGIN("poly3");i(k);h(aa);h(bb);h(cc);h(dd);std::printf(" |");b(p.IsDone());b(p.InfiniteRoots());i(p.NbSolutions());for(int j=1;j<=p.NbSolutions();++j)h(p.Value(j));END();}
 for(int k=0;k<20;++k){double aa=rnd(.2,3),bb=rnd(-4,4),cc=rnd(-2,8);math_DirectPolynomialRoots p(aa,bb,cc);BEGIN("poly2");i(k);h(aa);h(bb);h(cc);std::printf(" |");b(p.IsDone());b(p.InfiniteRoots());i(p.NbSolutions());for(int j=1;j<=p.NbSolutions();++j)h(p.Value(j));END();}
 // DoubleTab storage/accessor operations and non-one bounds.
 for(int k=0;k<20;++k){math_DoubleTab a(-1,1,2,4),c(-1,1,2,4);a.Init(0.);for(int r=-1;r<=1;++r)for(int j=2;j<=4;++j)a(r,j)=rnd(-20,20);a.Copy(c);BEGIN("doubletab");i(a.LowerRow());i(a.UpperRow());i(a.LowerCol());i(a.UpperCol());i(a.NbRows());i(a.NbColumns());b(a.IsDeletable());for(int r=-1;r<=1;++r)for(int j=2;j<=4;++j)h(a.Value(r,j));std::printf(" |");for(int r=-1;r<=1;++r)for(int j=2;j<=4;++j)h(c.Value(r,j));c.SetLowerRow(1);c.SetLowerCol(-2);i(c.LowerRow());i(c.LowerCol());h(c.Value(1,-2));END();}
 // Matrix product overloads, output parameters, row/column access and swaps.
 for(int k=0;k<20;++k){math_Matrix a(1,3,1,3,0.),bb(1,3,1,3,0.),out(1,3,1,3,0.);math_Vector x(1,3),y(1,3);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c){a(r,c)=rnd();bb(r,c)=rnd();}for(int j=1;j<=3;++j){x(j)=rnd();y(j)=rnd();}BEGIN("matrix_extra");i(k);m(a);m(bb);v(x);v(y);std::printf(" |");out.Add(a,bb);m(out);out.Subtract(a,bb);m(out);out.Multiply(a,bb);m(out);out.TMultiply(a,bb);m(out);out.Multiply(x,y);m(out);out.SetRow(2,y);m(out);out.SetCol(1,x);m(out);v(out.Row(2));v(out.Col(1));out.SwapRow(1,3);out.SwapCol(1,3);out.SetDiag(7.);m(out);m(a.Opposite());END();}
 // Solver object constructors and all numerical entry points.
 for(int k=0;k<20;++k){math_Matrix a(1,3,1,3,0.);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c)a(r,c)=rnd();for(int j=1;j<=3;++j)a(j,j)+=9.;math_Vector rhs(1,3),sol(1,3);for(int j=1;j<=3;++j)rhs(j)=rnd();math_Gauss g(a);math_Crout cr(a);math_GaussLeastSquare ls(a);math_SVD sv(a);BEGIN("solver_objects");i(k);m(a);v(rhs);std::printf(" |");b(g.IsDone());g.Solve(rhs,sol);v(sol);g.Solve(sol);v(sol);h(g.Determinant());math_Matrix inv(1,3,1,3,0.);g.Invert(inv);m(inv);b(cr.IsDone());cr.Solve(rhs,sol);v(sol);h(cr.Determinant());m(cr.Inverse());cr.Invert(inv);m(inv);b(ls.IsDone());ls.Solve(rhs,sol);v(sol);b(sv.IsDone());sv.Solve(rhs,sol);v(sol);sv.PseudoInverse(inv);m(inv);END();}
 // Householder vector and multi-right-hand-side constructors.
 for(int k=0;k<20;++k){double s=.25+.125*k;math_Matrix a(1,3,1,2,0.),bb(1,3,1,1,0.);for(int r=1;r<=3;++r){a(r,1)=s;a(r,2)=r*s;bb(r,1)=r*r*s;}math_Vector rhs(1,3);for(int j=1;j<=3;++j)rhs(j)=j*j*s;math_Householder hh(a,bb),hv(a,rhs),hr(a,bb,1,3,1,2,1e-20);math_Vector sol(1,2);hh.Value(sol,1);BEGIN("householder");h(s);std::printf(" |");b(hh.IsDone());b(hv.IsDone());b(hr.IsDone());m(hh.AllValues());v(sol);m(hv.AllValues());m(hr.AllValues());END();}
 // Tridiagonal eigensolver getters.
 for(int k=0;k<20;++k){NCollection_Array1<double>d0(1,3),e0(1,3);for(int j=1;j<=3;++j)d0(j)=2.*j+k*.125;e0(1)=.25+k*.01;e0(2)=-.5+k*.005;e0(3)=0.;math_EigenValuesSearcher es(d0,e0);BEGIN("eigen_searcher");for(int j=1;j<=3;++j)h(d0(j));for(int j=1;j<=3;++j)h(e0(j));std::printf(" |");b(es.IsDone());i(es.Dimension());for(int j=1;j<=es.Dimension();++j){h(es.EigenValue(j));v(es.EigenVector(j));}END();}
 // All direct-polynomial constructor overloads.
 for(int k=0;k<20;++k){int deg=k%4;double s=1.+.125*k,a=1.,bb=-s,cc=s*s,dd=-s*s*s,ee=s*s*s*s;if(deg==1){bb=-3.*s;cc=2.*s*s;}else if(deg==2){bb=-6.*s;cc=11.*s*s;dd=-6.*s*s*s;}else if(deg==3){bb=-10.*s;cc=35.*s*s;dd=-50.*s*s*s;ee=24.*s*s*s*s;}math_DirectPolynomialRoots p=deg==0?math_DirectPolynomialRoots(a,bb):deg==1?math_DirectPolynomialRoots(a,bb,cc):deg==2?math_DirectPolynomialRoots(a,bb,cc,dd):math_DirectPolynomialRoots(a,bb,cc,dd,ee);BEGIN("poly_ctor");i(k);h(a);h(bb);h(cc);h(dd);h(ee);std::printf(" |");b(p.IsDone());b(p.InfiniteRoots());i(p.NbSolutions());for(int j=1;j<=p.NbSolutions();++j)h(p.Value(j));END();}
 // IntegerVector methods and range preservation.
 for(int k=0;k<20;++k){math_IntegerVector x(-2,2),y(-2,2);for(int j=-2;j<=2;++j){x(j)=k+j*j;y(j)=j-k;}math_IntegerVector z=x;z.Add(y);BEGIN("integer_vector");i(x.Length());i(x.Lower());i(x.Upper());for(int j=-2;j<=2;++j)i(x(j));std::printf(" |");z.Init(4);for(int j=-2;j<=2;++j)i(z(j));z=x;z.Add(y);for(int j=-2;j<=2;++j)i(z(j));z=x;z.Subtract(y);for(int j=-2;j<=2;++j)i(z(j));z.Resize(3);i(z.Lower());i(z.Upper());for(int j=z.Lower();j<=z.Upper();++j)i(z(j));END();}
 // Vector slices, range replacement, initialization, and output-form operations.
 for(int k=0;k<20;++k){math_Vector x(-2,2),y(-2,2),z(-2,2);for(int j=-2;j<=2;++j){x(j)=rnd(-20,20);y(j)=rnd(-20,20);}z.Init(0.);z.Set(-1,1,y.Slice(-1,1));z.Initialized(x);z.Add(x,y);BEGIN("vector_extra");v(x);v(y);std::printf(" |");v(x.Slice(-1,1));z.Set(-1,1,y.Slice(-1,1));v(z);z.Initialized(x);v(z);z.Add(x,y);v(z);z.Subtract(x,y);v(z);z.Multiply(2.,x);v(z);z.Resize(3);v(z);END();}
 for(int k=0;k<20;++k){math_IntegerVector x(-2,2),y(-2,2);for(int j=-2;j<=2;++j){x(j)=k+j*j;y(j)=j+3-k;}BEGIN("integer_vector_ops");iv(x);iv(y);std::printf(" |");h(x.Norm());h(x.Norm2());i(x.Max());i(x.Min());math_IntegerVector d=x;d.Invert();iv(d);iv(x.Inverse());iv(x.Opposite());iv(x.Multiplied(-2));iv(x.Divided(2));i(x.Multiplied(y));iv(x.Slice(-1,1));math_IntegerVector z=x;z.Multiply(3);iv(z);z=x;z.Divide(2);iv(z);z.Set(-1,1,y.Slice(-1,1));iv(z);END();}
 // Error behavior for range, dimension, square, singular, and zero-divisor cases.
 for(int k=0;k<20;++k){int e1=0,e2=0,e3=0,e4=0,e5=0;try{math_Vector x(1,2);x.Init(k+1.);x.Divide(0.);}catch(Standard_Failure&){e1=1;}try{math_Vector x(1,2),y(1,3);x.Init(k+1.);x.Add(y);}catch(Standard_Failure&){e2=1;}try{math_Matrix x(1,2,1,3,0.);x.Init(k+1.);x.Determinant();}catch(Standard_Failure&){e3=1;}try{math_Matrix x(1,2,1,2,0.);x.Init(0.);x.Invert();}catch(Standard_Failure&){e4=1;}try{math_Vector x(1,2);x.Init(k+1.);x.Value(3);}catch(Standard_Failure&){e5=1;}BEGIN("error_cases");i(k);i(k+1);i(e1);i(e2);i(e3);i(e4);i(e5);std::printf(" |");END();}
 // Object-oriented Jacobi wrapper and all its result accessors.
 for(int k=0;k<20;++k){math_Matrix a(1,3,1,3,0.);for(int r=1;r<=3;++r)for(int c=r;c<=3;++c){double q=rnd();a(r,c)=q;a(c,r)=q;}math_Jacobi j(a);BEGIN("jacobi_object");m(a);std::printf(" |");b(j.IsDone());if(j.IsDone()){v(j.Values());for(int jx=1;jx<=3;++jx){h(j.Value(jx));math_Vector ev(1,3);j.Vector(jx,ev);v(ev);}m(j.Vectors());}END();}
 // LU invert entry point and scratch overload of SVD decomposition.
 for(int k=0;k<20;++k){math_Matrix a(1,3,1,3,0.);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c)a(r,c)=rnd();for(int j=1;j<=3;++j)a(j,j)+=7.;math_Matrix inv=a;int st=LU_Invert(inv);BEGIN("lu_invert");m(a);std::printf(" |");i(st);m(inv);END();}
 for(int k=0;k<20;++k){math_Matrix a(1,3,1,3,0.),vv(1,3,1,3,0.);math_Vector w(1,3),scratch(1,3);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c)a(r,c)=rnd();math_Matrix a0=a;int st=SVD_Decompose(a,w,vv,scratch);BEGIN("svd_scratch");m(a0);std::printf(" |");i(st);m(a);v(w);m(vv);v(scratch);END();}
 // DACTCL packed symmetric 2x2 factorization and solve.
 for(int k=0;k<20;++k){math_Vector packed(1,3),rhs(1,2);math_IntegerVector ix(1,2);packed(1)=4.+k*.25;packed(2)=rnd(-.5,.5);packed(3)=3.+k*.125;ix(1)=1;ix(2)=3;rhs(1)=1.+k*.125;rhs(2)=2.-k*.05;math_Vector packed0=packed,rhs0=rhs;math_IntegerVector ix0=ix;int st=DACTCL_Decompose(packed,ix);int ss=DACTCL_Solve(packed,rhs,ix);BEGIN("dactcl");v(packed0);iv(ix0);v(rhs0);std::printf(" |");i(st);i(ss);v(packed);v(rhs);END();}
 // Public diagnostic methods return stream content; record whether each is nonempty.
 {math_Matrix a(1,2,1,2,0.);a(1,1)=3.;a(2,2)=4.;math_Vector rhs(1,2);rhs(1)=1.;rhs(2)=2.;std::ostringstream os;math_Gauss g(a);g.Dump(os);BEGIN("dump");std::printf(" gauss |");b(!os.str().empty());END();}
 {math_Matrix a(1,2,1,2,0.);a(1,1)=3.;a(2,2)=4.;std::ostringstream os;math_Crout g(a);g.Dump(os);BEGIN("dump");std::printf(" crout |");b(!os.str().empty());END();}
 {math_Matrix a(1,2,1,2,0.);a(1,1)=3.;a(2,2)=4.;std::ostringstream os;math_GaussLeastSquare g(a);g.Dump(os);BEGIN("dump");std::printf(" gauss_least_square |");b(!os.str().empty());END();}
 {math_Matrix a(1,2,1,2,0.);a(1,1)=3.;a(2,2)=4.;std::ostringstream os;math_SVD g(a);g.Dump(os);BEGIN("dump");std::printf(" svd |");b(!os.str().empty());END();}
 {math_Matrix a(1,2,1,2,0.);a(1,1)=3.;a(2,2)=4.;std::ostringstream os;math_Jacobi g(a);g.Dump(os);BEGIN("dump");std::printf(" jacobi |");b(!os.str().empty());END();}
 {math_Matrix a(1,2,1,1,0.),bb(1,2,1,1,0.);a(1,1)=1.;a(2,1)=2.;bb(1,1)=3.;bb(2,1)=4.;std::ostringstream os;math_Householder g(a,bb);g.Dump(os);BEGIN("dump");std::printf(" householder |");b(!os.str().empty());END();}
 {std::ostringstream os;math_DirectPolynomialRoots g(1.,-3.,2.);g.Dump(os);BEGIN("dump");std::printf(" polynomial |");b(!os.str().empty());END();}
 {BEGIN("status_values");i(math_OK);i(math_TooManyIterations);i(math_FunctionError);i(math_DirectionSearchError);i(math_NotBracketed);std::printf(" |");i(math_Status_UserAborted);i(math_Status_OK);i(math_Status_SingularMatrix);i(math_Status_ArgumentError);i(math_Status_NoConvergence);END();}
 for(int n=1;n<=20;++n){math_ComputeGaussPointsAndWeights q(n);BEGIN("status_values");i(n);i(math_OK);i(math_TooManyIterations);i(math_FunctionError);i(math_DirectionSearchError);i(math_NotBracketed);std::printf(" |");b(q.IsDone());i(math_Status_UserAborted);i(math_Status_OK);i(math_Status_SingularMatrix);i(math_Status_ArgumentError);i(math_Status_NoConvergence);END();}
 for(int k=0;k<20;++k){double a=rnd(-20,20),bb=rnd(-20,20),c=rnd(-20,20);gp_XY xy(a,bb);gp_XYZ xyz(c,a,bb);math_Vector vx(xy),vz(xyz);math_IntegerVector ix(xy),iz(xyz);BEGIN("vector_gp");h(a);h(bb);h(c);std::printf(" |");v(vx);v(vz);iv(ix);iv(iz);END();}
 {math_Vector x(-1,1);math_IntegerVector ix(1,2);math_Matrix a(1,2,1,2,0.);x.Init(1.);ix.Init(2);std::ostringstream os;BEGIN("dump");std::printf(" vector |");x.Dump(os);b(!os.str().empty());END();os.str("");BEGIN("dump");std::printf(" integer_vector |");ix.Dump(os);b(!os.str().empty());END();os.str("");BEGIN("dump");std::printf(" matrix |");a.Dump(os);b(!os.str().empty());END();}
 for(int n=1;n<=20;++n){math_Matrix a(1,n,1,n,0.);for(int j=1;j<=n;++j)a(j,j)=n+.25;std::ostringstream os;a.Dump(os);BEGIN("dump");std::printf(" matrix");i(n);h(a(1,1));std::printf(" |");b(!os.str().empty());END();}
 // Vector/matrix product overloads on math_VectorBase.
 for(int k=0;k<20;++k){math_Vector x(1,3),y(1,3),out(1,3);math_Matrix a(1,3,1,3,0.);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c)a(r,c)=rnd();for(int j=1;j<=3;++j){x(j)=rnd();y(j)=rnd();}BEGIN("vector_products");v(x);m(a);v(y);std::printf(" |");v(x.TMultiplied(-2.));v(x.Multiplied(a));out.Multiply(x,a);v(out);out.Multiply(a,y);v(out);out.TMultiply(x,a);v(out);END();}
 // Matrix transpose, Set, Initialized, and in-place matrix product.
 for(int k=0;k<20;++k){math_Matrix x(1,3,1,3,0.),y(1,3,1,3,0.),part(1,2,1,2,0.);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c){x(r,c)=rnd();y(r,c)=rnd();}for(int r=1;r<=2;++r)for(int c=1;c<=2;++c)part(r,c)=rnd();BEGIN("matrix_mutations");m(x);m(y);m(part);std::printf(" |");math_Matrix z=x;z.Transpose();m(z);z=x;z.Set(2,3,2,3,part);m(z);z.Initialized(x);m(z);z.Multiply(y);m(z);m(x.TMultiplied(-2.));END();}
 // LU scratch-area overload, including its in/out scratch vector.
 for(int k=0;k<20;++k){math_Matrix x(1,3,1,3,0.);math_IntegerVector ix(1,3);math_Vector vv(1,3);for(int r=1;r<=3;++r)for(int c=1;c<=3;++c)x(r,c)=rnd();for(int j=1;j<=3;++j)x(j,j)+=5.;double sign=0.;math_Matrix x0=x;int st=LU_Decompose(x,ix,sign,vv,1e-20);BEGIN("lu_scratch");m(x0);std::printf(" |");i(st);m(x);iv(ix);h(sign);v(vv);END();}
 for(int k=0;k<20;++k){double aa=(k==0)?0.:1.+.1*k,bb=(k==0)?0.:-2.-.2*k;math_DirectPolynomialRoots p(aa,bb);BEGIN("poly_infinite");h(aa);h(bb);std::printf(" |");b(p.IsDone());b(p.InfiniteRoots());try{i(p.NbSolutions());}catch(Standard_Failure&){std::printf(" throws");}END();}
 return 0;
}

int main(){
  FILE* capture=std::tmpfile();
  if(!capture) return 2;
  int saved=dup(STDOUT_FILENO);
  if(saved<0 || dup2(fileno(capture),STDOUT_FILENO)<0) return 2;
  int rc=run_oracle();
  std::fflush(stdout);
  dup2(saved,STDOUT_FILENO);
  close(saved);
  std::rewind(capture);
  std::vector<std::string> lines;
  char* buf=nullptr; size_t cap=0;
  while(getline(&buf,&cap,capture)>0) lines.emplace_back(buf);
  free(buf); std::fclose(capture);
  for(const auto& line:lines) std::fputs(line.c_str(),stdout);
  return rc;
}
