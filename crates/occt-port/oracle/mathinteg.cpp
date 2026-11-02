// Deterministic reference records for the header-only OCCT MathInteg package.
#include <MathInteg_DoubleExp.hxx>
#include <MathInteg_Gauss.hxx>
#include <MathInteg_Kronrod.hxx>
#include <MathInteg_Multiple.hxx>
#include <MathInteg_Set.hxx>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <iomanip>
#include <iostream>
#include <string>
#include <vector>

using namespace MathInteg;
using namespace MathUtils;

static bool inphase = false;
static int input_words = 0;
static void f(double x) { uint64_t b; std::memcpy(&b, &x, 8); std::cout << ' ' << std::hex << std::setw(16) << std::setfill('0') << b << std::dec; if (inphase) ++input_words; }
static uint64_t mix(uint64_t h, uint64_t x) { h ^= x; return h * 0x100000001b3ull; }
static double ipow(double x, int n) { double y=1.0; for(int i=0;i<n;++i)y*=x; return y; }

struct Scalar {
  int kind, param, calls = 0; uint64_t hash = 0xcbf29ce484222325ull;
  bool Value(double x, double& y) {
    ++calls; uint64_t b; std::memcpy(&b, &x, 8); hash = mix(hash, b);
    if (kind == 4 && x > -0.125 && x < 0.125) return false;
    switch (kind) {
      case 0: y = ipow(x, param); break;
      case 1: y = std::exp(-x * x); break;
      case 2: y = std::sin(x); break;
      case 3: y = std::abs(x - 0.125); break;
      default: y = x * x + 1.0; break;
    }
    return true;
  }
};

struct Multi {
  int dim, kind, calls = 0; uint64_t hash = 0xcbf29ce484222325ull;
  bool Value(const math_Vector& x, double& y) {
    ++calls; y = 0.0;
    for (int i = 1; i <= dim; ++i) { double v=x(i); uint64_t b; std::memcpy(&b,&v,8); hash=mix(hash,b); y += (kind == 0 ? v*v : std::abs(v)); }
    return true;
  }
};

struct Set {
  int n, calls = 0; uint64_t hash = 0xcbf29ce484222325ull;
  int NbEquations() const { return n; }
  bool Value(const math_Vector& x, math_Vector& y) {
    ++calls; double v=x(x.Lower()); uint64_t b; std::memcpy(&b,&v,8); hash=mix(hash,b);
    for (int j=1;j<=n;++j) y(j)=ipow(v,j);
    return true;
  }
};

static void begin(const char* name, int k, int p, int dim, double a, double b, int order, double tol, int it, double sing, int extra) {
  inphase=true; input_words=0;
  std::cout << name << ' ' << k << ' ' << p << ' ' << dim; input_words+=3; f(a); f(b); std::cout << ' ' << order; ++input_words; f(tol); std::cout << ' ' << it; ++input_words; f(sing); std::cout << ' ' << extra; ++input_words;
  const char* schema="i i i f f i f i f i"; int schema_words=0; for(const char* q=schema;*q;++q)if(*q=='i'||*q=='f')++schema_words;
  if(input_words!=schema_words){std::cerr<<"schema mismatch in "<<name<<"\n";std::abort();}
  std::cout << " | "; inphase=false;
}
static void out(const IntegResult& r, int calls, uint64_t hash) {
  std::cout << static_cast<int>(r.Status) << ' ' << (r.Value ? 1 : 0); if (r.Value) f(*r.Value); std::cout << ' ' << (r.AbsoluteError ? 1 : 0); if (r.AbsoluteError) f(*r.AbsoluteError); std::cout << ' ' << (r.RelativeError ? 1 : 0); if (r.RelativeError) f(*r.RelativeError); std::cout << ' ' << r.NbPoints << ' ' << r.NbIterations << ' ' << calls << ' ' << hash << '\n';
}
static void outset(const SetResult& r, int calls, uint64_t hash) {
  std::cout << static_cast<int>(r.Status) << ' ' << (r.Values ? 1 : 0);
  if (r.Values) for (int i=r.Values->Lower();i<=r.Values->Upper();++i) f((*r.Values)(i));
  std::cout << ' ' << r.NbEquations << ' ' << calls << ' ' << hash << '\n';
}

int main() {
  const char* names[] = {"gauss","gauss_adaptive","gauss_composite","kronrod_rule","kronrod","kronrod_auto","kronrod_semi_infinite","kronrod_infinite","tanh_sinh","exp_sinh","sinh_sinh","double_exponential","tanh_sinh_singular","tanh_sinh_with_singularity","gauss_multiple","gauss_multiple_uniform","gauss_set","gauss_set_vectors","gauss_multiple_config"};
  uint64_t s=0x9e3779b97f4a7c15ull;
  auto rnd=[&](){s^=s<<13;s^=s>>7;s^=s<<17;return (double)(s>>11)/9007199254740992.0;};
  for (int family=0;family<19;++family) for (int j=0;j<16;++j) {
    int kind = (family==0 || family==2 || family>=14) ? 0 : ((family==6 || family==7 || family==9 || family==10) ? 1 : (j < 5 ? j : (int)(rnd()*4))); int param = (j%6)+1;
    double a=-1.0+0.25*j, b=1.0+0.125*(j%5); if(j==0){a=0;b=0;} if(j==1){a=1;b=-1;}
    int order = (j==2 ? 0 : (j==3 ? 1 : (j==14 ? 61 : (j==15 ? 62 : 2+(j*3)%9)))); double tol=(j==2?0.0:(j==3?1e-300:(j==4?1e6:1e-8))); int it=(j==4?1:(j==5?0:8)); double sing=a+(b-a)*0.5;
    int dim=(j>=14?1:1+j%3), extra=(family==2?(j%5)+1:(family==4?j%2:(family==5?2+(j%8):(family==18?1+(j%7):dim))));
    begin(names[family],kind,param,dim,a,b,order,tol,it,sing,extra);
    Scalar fn{kind,param}; IntegConfig ic; ic.InitialOrder=std::max(1,order); ic.MaxOrder=std::max(ic.InitialOrder, std::min(61,2*ic.InitialOrder)); ic.MaxIterations=it; ic.Tolerance=tol;
    KronrodConfig kc; kc.NbGaussPoints=std::max(1,std::min(15,order)); kc.InitialOrder=ic.InitialOrder; kc.MaxOrder=ic.MaxOrder; kc.MaxIterations=it; kc.Tolerance=tol; kc.Adaptive=(extra==0);
    DoubleExpConfig dc; dc.Tolerance=tol; dc.MaxIterations=it; dc.NbLevels=1+dim;
    IntegResult r;
    switch(family){
      case 0:r=Gauss(fn,a,b,order);break; case 1:r=GaussAdaptive(fn,a,b,ic);break; case 2:r=GaussComposite(fn,a,b,extra,order);break;
      case 3:r=KronrodRule(fn,a,b,std::max(1,order));break; case 4:r=Kronrod(fn,a,b,kc);break; case 5:r=KronrodAuto(fn,a,b,tol,extra);break;
      case 6:r=KronrodSemiInfinite(fn,std::max(0.0,a),kc);break; case 7:r=KronrodInfinite(fn,kc);break; case 8:r=TanhSinh(fn,a,b,dc);break;
      case 9:r=ExpSinh(fn,std::max(0.0,a),dc);break; case 10:r=SinhSinh(fn,dc);break; case 11:r=DoubleExponential(fn,a,b,dc);break;
      case 12:r=TanhSinhSingular(fn,a,b,tol);break; case 13:r=TanhSinhWithSingularity(fn,a,b,sing,dc);break;
      case 14: case 18: { int n=dim; math_Vector lo(1,n),hi(1,n); math_IntegerVector ord(0,n-1); for(int q=1;q<=n;++q){lo(q)=a;hi(q)=b;ord(q-1)=order;} Multi mf{n,0}; if(family==18){MultipleConfig mc;mc.MaxOrder=extra;r=GaussMultiple(mf,n,lo,hi,ord,mc);}else{r=GaussMultiple(mf,n,lo,hi,ord);} fn.calls=mf.calls; fn.hash=mf.hash; break; }
      case 15: { int n=dim; math_Vector lo(1,n),hi(1,n); for(int q=1;q<=n;++q){lo(q)=a;hi(q)=b;} Multi mf{n,0}; r=GaussMultipleUniform(mf,n,lo,hi,order); fn.calls=mf.calls; fn.hash=mf.hash; break; }
      case 16: { Set sf{dim}; SetResult z=GaussSet(sf,a,b,order); outset(z,sf.calls,sf.hash); continue; }
      default: { Set sf{dim}; math_Vector lo(0,0),hi(0,0);lo(0)=a;hi(0)=b;SetResult z=GaussSet(sf,lo,hi,order);outset(z,sf.calls,sf.hash);continue; }
    }
    out(r,fn.calls,fn.hash);
  }
}
