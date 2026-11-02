#include <BVH_Box.hxx>
#include <BVH_Ray.hxx>
#include <BVH_Tools.hxx>
#include <BVH_Constants.hxx>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <iomanip>
#include <string>
#include <vector>

static uint64_t rng=0x9E3779B97F4A7C15ull;
static double rnd(double a,double b){rng^=rng<<13;rng^=rng>>7;rng^=rng<<17;return a+(b-a)*double(rng>>11)/9007199254740992.;}
static std::string bits(double x){uint64_t u;std::memcpy(&u,&x,8);char s[17];std::snprintf(s,sizeof(s),"%016llx",(unsigned long long)u);return s;}
using V=BVH::VectorType<double,3>::Type; using B=BVH_Box<double,3>; using T=BVH_Tools<double,3>;
static const char* fam[]={"box_valid","box_area","box_size","box_center","box_center_axis","box_add","box_combine","box_is_out_box","box_is_out_point","box_contains","box_clear","tools_box_distance","tools_point_distance","tools_projection","tools_triangle_projection","tools_triangle_distance","ray_inverse","box_degenerate","box_coincident","box_tangent","box_parallel","box_perpendicular","box_overload_bounds","tools_box_distance_corners","tools_projection_corners"};
static void begin(const char* name, const char* schema){
  (void)name;
  int expected=0; for(const char* p=schema;*p;){ if(*p=='f'||*p=='i'||*p=='b') ++expected; while(*p&&*p!=' ') ++p; while(*p==' ') ++p; }
  if(expected!=16) std::abort();
}
int main(){
 const uint64_t poisonSeed=std::getenv("BVH_POISON_SEED") ? std::strtoull(std::getenv("BVH_POISON_SEED"),nullptr,10) : 0;
 for(int f=0;f<25;++f) for(int k=0;k<16;++k){
  begin(fam[f],"f f f f f f i f f f f f f f f f");
  std::ostringstream rec;
  volatile uint64_t stackPoison[128];
  for(size_t q=0;q<128;++q) stackPoison[q]=rng^poisonSeed^(uint64_t(k+1)*0x9E3779B97F4A7C15ull)^uint64_t(q);
  void* heapPoison=std::malloc(4096);
  if(heapPoison){ std::memset(heapPoison, int((k*37+poisonSeed)&255), 4096); std::free(heapPoison); }
  (void)stackPoison[0];
  double a[6];
  for(double &v:a)v=rnd(-5.,5.);
  if(k==0){for(double &v:a)v=0.;}
  if(k==1){a[3]=a[0];a[4]=a[1];a[5]=a[2];}
  if(k==2){a[3]=a[0]+1.;a[4]=a[1]+1.;a[5]=a[2]+1.;}
  if(k==3){for(double &v:a)v=1.;}
  V lo(a[0],a[1],a[2]), hi(a[0]+std::abs(a[3]),a[1]+std::abs(a[4]),a[2]+std::abs(a[5]));
  V lo2(a[3],a[4],a[5]),hi2(a[3]+1.,a[4]+1.,a[5]+1.);
  B b(lo,hi),c(lo2,hi2); V p(a[3],a[4],a[5]);
  bool overlap=false; bool contains=b.Contains(c,overlap);
  V proj=T::PointBoxProjection(p,b);
  BVH_Tools<double,3>::BVH_PrjStateInTriangle state; int first=-9,last=-9;
  V ta(0.,0.,0.),tb(1.,0.,0.),tc(0.,1.,0.);
  V tri=T::PointTriangleProjection(p,ta,tb,tc,&state,&first,&last);
  BVH_Ray<double,3> ray(V(a[0],a[1],a[2]),V(a[3],a[4],a[5]));
  B added=b; added.Add(p); B combined=b; combined.Combine(c); B cleared=b; cleared.Clear();
  rec<<fam[f]<<" "<<bits(a[0])<<" "<<bits(a[1])<<" "<<bits(a[2])<<" "<<bits(a[3])<<" "<<bits(a[4])<<" "<<bits(a[5])<<" "<<(f%3)<<" "<<bits(ta.x())<<" "<<bits(ta.y())<<" "<<bits(ta.z())<<" "<<bits(tb.x())<<" "<<bits(tb.y())<<" "<<bits(tb.z())<<" "<<bits(tc.x())<<" "<<bits(tc.y())<<" "<<bits(tc.z())<<" | ";
  rec<<(b.IsValid()?1:0)<<" "<<bits(b.Area())<<" "; V sz=b.Size(),ce=b.Center();
  rec<<bits(sz.x())<<" "<<bits(sz.y())<<" "<<bits(sz.z())<<" "<<bits(ce.x())<<" "<<bits(ce.y())<<" "<<bits(ce.z())<<" ";
  rec<<bits(b.Center(f%3))<<" "<<(b.IsOut(c)?1:0)<<" "<<(b.IsOut(p)?1:0)<<" "<<(contains?1:0)<<" "<<(overlap?1:0)<<" ";
  rec<<bits(T::BoxBoxSquareDistance(b,c))<<" "<<bits(T::PointBoxSquareDistance(p,b))<<" ";
  rec<<bits(proj.x())<<" "<<bits(proj.y())<<" "<<bits(proj.z())<<" ";
  rec<<bits(tri.x())<<" "<<bits(tri.y())<<" "<<bits(tri.z())<<" "<<state<<" "<<first<<" "<<last<<" "<<bits(T::PointTriangleSquareDistance(p,ta,tb,tc))<<" ";
  rec<<bits(ray.InvDirect.x())<<" "<<bits(ray.InvDirect.y())<<" "<<bits(ray.InvDirect.z())<<" ";
  rec<<bits(added.CornerMin().x())<<" "<<bits(added.CornerMin().y())<<" "<<bits(added.CornerMin().z())<<" "<<bits(added.CornerMax().x())<<" "<<bits(added.CornerMax().y())<<" "<<bits(added.CornerMax().z())<<" ";
  rec<<bits(combined.CornerMin().x())<<" "<<bits(combined.CornerMin().y())<<" "<<bits(combined.CornerMin().z())<<" "<<bits(combined.CornerMax().x())<<" "<<bits(combined.CornerMax().y())<<" "<<bits(combined.CornerMax().z())<<" "<<(cleared.IsValid()?1:0)<<"\n";
  const std::string row=rec.str(); const size_t bar=row.find(" | ");
  if(bar==std::string::npos) std::abort();
  auto count=[](const std::string& s){ std::istringstream in(s); int n=0; std::string t; while(in>>t) ++n; return n; };
  if(count(row.substr(0,bar))!=17 || count(row.substr(bar+3))!=41) std::abort();
  std::cout<<row;
 }
}
