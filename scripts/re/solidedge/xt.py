"""Parasolid XT neutral-binary node decoder (port of Gusset's own gusset-xt decode; Python, for scripts)."""
import struct,re
CURVE='node_id:d attributes_groups:p owner:p next:p previous:p geometric_owner:p sense:c'
V12='\n10 ASSEMBLY 1 highest_node_id:d attributes_groups:p attribute_chains:p list:p surface:p curve:p point:p key:p res_size:f res_linear:f ref_instance:p next:p previous:p state:u owner:p type:u sub_instance:p\n11 INSTANCE 1 node_id:d attributes_groups:p type:u part:p transform:p assembly:p next_in_part:p prev_in_part:p next_of_part:p prev_of_part:p\n12 BODY 1 highest_node_id:d attributes_groups:p attribute_chains:p surface:p curve:p point:p key:p res_size:f res_linear:f ref_instance:p next:p previous:p state:u owner:p body_type:u shell:p boundary_surface:p boundary_curve:p boundary_point:p region:p edge:p vertex:p\n13 SHELL 1 node_id:d attributes_groups:p body:p next:p face:p edge:p vertex:p region:p front_face:p\n14 FACE 1 node_id:d attributes_groups:p tolerance:f next:p previous:p loop:p shell:p surface:p sense:c next_on_surface:p previous_on_surface:p next_front:p previous_front:p front_shell:p\n15 LOOP 1 node_id:d attributes_groups:p fin:p face:p next:p\n16 EDGE 1 node_id:d attributes_groups:p tolerance:f fin:p previous:p next:p curve:p next_on_curve:p previous_on_curve:p owner:p\n17 FIN 1 attributes_groups:p loop:p forward:p backward:p vertex:p other:p edge:p curve:p next_at_vx:p sense:c\n18 VERTEX 1 node_id:d attributes_groups:p fin:p previous:p next:p point:p tolerance:f owner:p\n19 REGION 1 node_id:d attributes_groups:p body:p next:p previous:p shell:p type:c\n29 POINT 1 node_id:d attributes_groups:p owner:p next:p previous:p pvec:v\n30 LINE 1 $C pvec:v direction:v\n31 CIRCLE 1 $C centre:v normal:v x_axis:v radius:f\n32 ELLIPSE 1 $C centre:v normal:v x_axis:v major_radius:f minor_radius:f\n38 INTERSECTION 1 $C surface:p[2] chart:p start:p end:p\n40 CHART 0 base_parameter:f base_scale:f chart_count:d chordal_error:f angular_error:f parameter_error:f[2] hvec:h[]\n41 LIMIT 0 type:c hvec:h[]\n45 BSPLINE_VERTICES 0 vertices:f[]\n50 PLANE 1 $C pvec:v normal:v x_axis:v\n51 CYLINDER 1 $C pvec:v axis:v radius:f x_axis:v\n52 CONE 1 $C pvec:v axis:v radius:f sin_half_angle:f cos_half_angle:f x_axis:v\n53 SPHERE 1 $C centre:v radius:f axis:v x_axis:v\n54 TORUS 1 $C centre:v axis:v major_radius:f minor_radius:f x_axis:v\n56 BLENDED_EDGE 1 $C blend_type:c surface:p[2] spine:p range:f[2] thumb_weight:f[2] boundary:p[2] start:p end:p\n59 BLEND_BOUND 0 $C boundary:n blend:p\n60 OFFSET_SURF 1 $C check:c true_offset:l surface:p offset:f scale:f\n67 SWEPT_SURF 1 $C section:p sweep:v scale:f\n68 SPUN_SURF 1 $C profile:p base:v axis:v start:v end:v start_param:f end_param:f x_axis:v scale:f\n70 LIST 1 node_id:d owner:p next:p previous:p list_type:d list_length:d block_length:d size_of_entry:d list_block:p finger_block:p finger_index:d notransmit:l\n74 POINTER_LIS_BLOCK 0 n_entries:d next_block:p entries:p[]\n79 ATT_DEF_ID 0 string:c[]\n80 ATTRIB_DEF 1 next:p identifier:p type_id:d actions:u[8] field_names:p legal_owners:l[14] fields:u[]\n81 ATTRIBUTE 1 node_id:d definition:p owner:p next:p previous:p next_of_type:p previous_of_type:p fields:p[]\n82 INT_VALUES 0 values:d[]\n83 REAL_VALUES 0 values:f[]\n84 CHAR_VALUES 0 values:c[]\n85 POINT_VALUES 0 values:v[]\n86 VECTOR_VALUES 0 values:v[]\n87 AXIS_VALUES 0 values:v[]\n88 TAG_VALUES 0 values:d[]\n89 DIRECTION_VALUES 0 values:v[]\n90 GROUP 1 node_id:d attributes_groups:p owner:p next:p previous:p type:u first_member:p\n91 MEMBER_OF_GROUP 0 dummy_node_id:d owning_group:p owner:p next:p previous:p next_member:p previous_member:p\n98 UNICODE_VALUES 0 values:w[]\n99 FIELD_NAMES 0 names:p[]\n100 TRANSFORM 1 node_id:d owner:p next:p previous:p rotation_matrix:f[9] translation_vector:v scale:f flag:d perspective_vector:v\n101 WORLD 0 assembly:p attribute:p body:p transform:p surface:p curve:p point:p alive:l attrib_def:p highest_id:d current_id:d\n102 KEY 0 string:c[]\n124 B_SURFACE 1 $C nurbs:p data:p\n125 SURFACE_DATA 0 original_uint:i original_vint:i extended_uint:i extended_vint:i self_int:u original_u_start:c original_u_end:c original_v_start:c original_v_end:c extended_u_start:c extended_u_end:c extended_v_start:c extended_v_end:c analytic_form_type:c swept_form_type:c spun_form_type:c blend_form_type:c analytic_form:p swept_form:p spun_form:p blend_form:p\n126 NURBS_SURF 0 u_periodic:l v_periodic:l u_degree:n v_degree:n n_u_vertices:d n_v_vertices:d u_knot_type:u v_knot_type:u n_u_knots:d n_v_knots:d rational:l u_closed:l v_closed:l surface_form:u vertex_dim:n bspline_vertices:p u_knot_mult:p v_knot_mult:p u_knots:p v_knots:p\n127 KNOT_MULT 0 mult:n[]\n128 KNOT_SET 0 knots:f[]\n133 TRIMMED_CURVE 1 $C basis_curve:p point_1:v point_2:v parm_1:f parm_2:f\n134 B_CURVE 1 $C nurbs:p data:p\n135 CURVE_DATA 0 self_int:u analytic_form:p\n136 NURBS_CURVE 0 degree:n n_vertices:d vertex_dim:n n_knots:d knot_type:u periodic:l closed:l rational:l curve_form:u bspline_vertices:p knot_mult:p knots:p\n137 SP_CURVE 1 $C surface:p b_curve:p original:p tolerance_to_original:f\n141 GEOMETRIC_OWNER 0 owner:p next:p previous:p shared_geometry:p\n'
V9='\n17 FIN 1 loop:p forward:p backward:p vertex:p other:p edge:p curve:p next_at_vx:p sense:c\n'
def parse_table(t):
    out={}
    for line in t.splitlines():
        w=line.split()
        if len(w)<3: continue
        ty=int(w[0]); fields=[]
        for x in w[3:]:
            for s in (CURVE.split() if x=='$C' else [x]):
                n,r=s.split(':'); code=r[0]; tail=r[1:]
                cnt=1 if not tail else ('v' if tail=='[]' else int(tail[1:-1]))
                fields.append((n,code,cnt))
        out[ty]=(w[1],w[2]=='1',fields)
    return out
class B:
    def __init__(s,d): s.d=d; s.p=0
    def take(s,n):
        if s.p+n>len(s.d): raise EOFError(s.p)
        r=s.d[s.p:s.p+n]; s.p+=n; return r
    def u16(s): return struct.unpack('>H',s.take(2))[0]
    def i32(s): return struct.unpack('>i',s.take(4))[0]
    def ptr(s):
        r=struct.unpack('>h',s.take(2))[0]
        if r>=0: return r-1
        q=struct.unpack('>H',s.take(2))[0]
        return q*32767+(-r)-1
    def val(s,c):
        if c=='u': return s.take(1)[0]
        if c in 'nw': v=struct.unpack('>h',s.take(2))[0]; return None if v==-32764 else v
        if c=='d': v=s.i32(); return None if v==-32764 else v
        if c=='c': return s.take(1)[0]
        if c=='l': return s.take(1)[0]!=0
        if c=='p': return ('p',s.ptr())
        if c=='f': return struct.unpack('>d',s.take(8))[0]
        if c in 'vh': return struct.unpack('>3d',s.take(24))
        if c=='i': return struct.unpack('>2d',s.take(16))
        if c=='b': return struct.unpack('>6d',s.take(48))
V13="12 BODY 1 highest_node_id:d attributes_groups:p attribute_chains:p surface:p curve:p point:p key:p res_size:f res_linear:f ref_instance:p next:p previous:p state:u owner:p body_type:u nom_geom_state:u shell:p boundary_surface:p boundary_curve:p boundary_point:p region:p edge:p vertex:p"
def sstr(b): n=b.take(1)[0]; return b.take(n).decode('latin1')
def sfield(b):
    name=sstr(b); cls=struct.unpack('>h',b.take(2))[0]; ne=b.ptr()
    ts=sstr(b) if cls==0 else ''
    if ne==1: b.take(1)
    code='p' if cls else ts[0]
    return (name,code,1 if ne==0 else ('v' if ne==1 else ne))
def read_delta(b,ty,lay):
    n=b.take(1)[0]
    if n==255: return
    if ty not in lay:
        name=sstr(b); sstr(b); lay[ty]=(name,False,[sfield(b) for _ in range(n)]); return
    nm,vis,base=lay[ty]; it=iter(base); out=[]
    while True:
        c=chr(b.take(1)[0])
        if c=='C':
            f=next(it,None)
            if f: out.append(f)
        elif c=='D': next(it,None)
        elif c in 'IA': out.append(sfield(b))
        elif c=='Z': break
        else: raise ValueError(c)
    lay[ty]=(nm,vis,out)
def decode(xt):
    i=xt.find(b'**END_OF_HEADER'); i=xt.index(b'\n',i)+1
    d=xt[i:].lstrip(); assert d[:2]==b'PS'
    b=B(d[4:]); n=b.u16(); modeller=b.take(n); m=b.i32(); key=b.take(m).decode()
    parts=key.split('_'); number=int(parts[2]); base=len(parts)>3
    lay=parse_table(V12)
    if base: lay.update(parse_table(V13)); b.u16()
    elif number<12000: lay.update(parse_table(V9))
    uf=b.i32(); nodes={}; seen=set()
    while True:
        ty=struct.unpack('>h',b.take(2))[0]
        if ty==1: break
        if base and ty not in seen:
            seen.add(ty); read_delta(b,ty,lay)
        name,vis,fields=lay[ty]
        var=any(f[2]=='v' for f in fields); ln=b.i32() if var else 0
        idx=b.ptr(); vals=[]
        for n_,c,cnt in fields:
            if cnt==1: vals.append(b.val(c))
            elif cnt=='v':
                vals.append(b.take(ln) if c=='c' else [b.val(c) for _ in range(ln)])
            else: vals.append([b.val(c) for _ in range(cnt)])
        if vis:
            for _ in range(uf): b.i32()
        nodes[idx]=(name,{f[0]:v for f,v in zip(fields,vals)})
    return nodes,key,modeller
