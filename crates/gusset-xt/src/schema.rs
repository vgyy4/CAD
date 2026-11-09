//! Node layouts: which fields each node type has, in transmit order, with their type codes
//! (ADR 0035). The base table is schema 12006 (Parasolid V12) as the public XT Format
//! Reference describes it, written here in our own notation. Older files differ in a few
//! node types; those differences were worked out from public-domain sample files (NIST
//! engineering-design-models) and are listed as overrides. Files from V14 on may carry
//! their own differences as an embedded edit script per node type (see [`apply_delta`]).

use std::collections::BTreeMap;

/// The type code of one field (the Reference's `$x` codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Code {
    /// Unsigned byte.
    U,
    /// Character.
    C,
    /// Logical (`T`/`F` in text, a byte in binary).
    L,
    /// 16-bit integer.
    N,
    /// 16-bit unicode unit.
    W,
    /// 32-bit integer.
    D,
    /// Node index.
    P,
    /// Double.
    F,
    /// Interval: two doubles.
    I,
    /// Vector: three doubles.
    V,
    /// Box: six doubles.
    B,
    /// Intersection point: three doubles are stored.
    H,
}

impl Code {
    pub(crate) fn from_char(c: char) -> Option<Code> {
        Some(match c {
            'u' => Code::U,
            'c' => Code::C,
            'l' => Code::L,
            'n' => Code::N,
            'w' => Code::W,
            'd' => Code::D,
            'p' => Code::P,
            'f' => Code::F,
            'i' => Code::I,
            'v' => Code::V,
            'b' => Code::B,
            'h' => Code::H,
            _ => return None,
        })
    }
}

/// How many values a field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Count {
    One,
    Fixed(u16),
    /// The node's variable-length field (its length precedes the node index).
    Var,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) code: Code,
    pub(crate) count: Count,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) name: String,
    pub(crate) fields: Vec<Field>,
    /// Visible at the PK interface: such nodes are followed by the user fields.
    pub(crate) visible: bool,
}

impl Layout {
    pub(crate) fn variable(&self) -> bool {
        self.fields.iter().any(|f| f.count == Count::Var)
    }

    pub(crate) fn position(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

pub(crate) type Schema = BTreeMap<u16, Layout>;

/// Which family of layouts a schema number belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    /// Schemas 4000..=8999 (Parasolid V4..V8): inferred from sample files.
    V4,
    /// Schemas 9000..=11999.
    V9,
    /// Schema 12000 and later (and the base of embedded deltas).
    V12,
}

impl Family {
    pub(crate) fn of(schema: u32) -> Option<Family> {
        match schema {
            4000..=8999 => Some(Family::V4),
            9000..=11999 => Some(Family::V9),
            12000.. => Some(Family::V12),
            _ => None,
        }
    }
}

const CURVE: &str = "node_id:d attributes_groups:p owner:p next:p previous:p geometric_owner:p sense:c";

/// Schema 12006, one node per line: `type name visible fields...`, a field `name:code`,
/// `name:code[n]` for a fixed array and `name:code[]` for the variable-length field.
/// `$C` stands for the fields every curve and surface starts with.
const V12: &str = "
10 ASSEMBLY 1 highest_node_id:d attributes_groups:p attribute_chains:p list:p surface:p curve:p point:p key:p res_size:f res_linear:f ref_instance:p next:p previous:p state:u owner:p type:u sub_instance:p
11 INSTANCE 1 node_id:d attributes_groups:p type:u part:p transform:p assembly:p next_in_part:p prev_in_part:p next_of_part:p prev_of_part:p
12 BODY 1 highest_node_id:d attributes_groups:p attribute_chains:p surface:p curve:p point:p key:p res_size:f res_linear:f ref_instance:p next:p previous:p state:u owner:p body_type:u shell:p boundary_surface:p boundary_curve:p boundary_point:p region:p edge:p vertex:p
13 SHELL 1 node_id:d attributes_groups:p body:p next:p face:p edge:p vertex:p region:p front_face:p
14 FACE 1 node_id:d attributes_groups:p tolerance:f next:p previous:p loop:p shell:p surface:p sense:c next_on_surface:p previous_on_surface:p next_front:p previous_front:p front_shell:p
15 LOOP 1 node_id:d attributes_groups:p fin:p face:p next:p
16 EDGE 1 node_id:d attributes_groups:p tolerance:f fin:p previous:p next:p curve:p next_on_curve:p previous_on_curve:p owner:p
17 FIN 1 attributes_groups:p loop:p forward:p backward:p vertex:p other:p edge:p curve:p next_at_vx:p sense:c
18 VERTEX 1 node_id:d attributes_groups:p fin:p previous:p next:p point:p tolerance:f owner:p
19 REGION 1 node_id:d attributes_groups:p body:p next:p previous:p shell:p type:c
29 POINT 1 node_id:d attributes_groups:p owner:p next:p previous:p pvec:v
30 LINE 1 $C pvec:v direction:v
31 CIRCLE 1 $C centre:v normal:v x_axis:v radius:f
32 ELLIPSE 1 $C centre:v normal:v x_axis:v major_radius:f minor_radius:f
38 INTERSECTION 1 $C surface:p[2] chart:p start:p end:p
40 CHART 0 base_parameter:f base_scale:f chart_count:d chordal_error:f angular_error:f parameter_error:f[2] hvec:h[]
41 LIMIT 0 type:c hvec:h[]
45 BSPLINE_VERTICES 0 vertices:f[]
50 PLANE 1 $C pvec:v normal:v x_axis:v
51 CYLINDER 1 $C pvec:v axis:v radius:f x_axis:v
52 CONE 1 $C pvec:v axis:v radius:f sin_half_angle:f cos_half_angle:f x_axis:v
53 SPHERE 1 $C centre:v radius:f axis:v x_axis:v
54 TORUS 1 $C centre:v axis:v major_radius:f minor_radius:f x_axis:v
56 BLENDED_EDGE 1 $C blend_type:c surface:p[2] spine:p range:f[2] thumb_weight:f[2] boundary:p[2] start:p end:p
59 BLEND_BOUND 0 $C boundary:n blend:p
60 OFFSET_SURF 1 $C check:c true_offset:l surface:p offset:f scale:f
67 SWEPT_SURF 1 $C section:p sweep:v scale:f
68 SPUN_SURF 1 $C profile:p base:v axis:v start:v end:v start_param:f end_param:f x_axis:v scale:f
70 LIST 1 node_id:d owner:p next:p previous:p list_type:d list_length:d block_length:d size_of_entry:d list_block:p finger_block:p finger_index:d notransmit:l
74 POINTER_LIS_BLOCK 0 n_entries:d next_block:p entries:p[]
79 ATT_DEF_ID 0 string:c[]
80 ATTRIB_DEF 1 next:p identifier:p type_id:d actions:u[8] field_names:p legal_owners:l[14] fields:u[]
81 ATTRIBUTE 1 node_id:d definition:p owner:p next:p previous:p next_of_type:p previous_of_type:p fields:p[]
82 INT_VALUES 0 values:d[]
83 REAL_VALUES 0 values:f[]
84 CHAR_VALUES 0 values:c[]
85 POINT_VALUES 0 values:v[]
86 VECTOR_VALUES 0 values:v[]
87 AXIS_VALUES 0 values:v[]
88 TAG_VALUES 0 values:d[]
89 DIRECTION_VALUES 0 values:v[]
90 GROUP 1 node_id:d attributes_groups:p owner:p next:p previous:p type:u first_member:p
91 MEMBER_OF_GROUP 0 dummy_node_id:d owning_group:p owner:p next:p previous:p next_member:p previous_member:p
98 UNICODE_VALUES 0 values:w[]
99 FIELD_NAMES 0 names:p[]
100 TRANSFORM 1 node_id:d owner:p next:p previous:p rotation_matrix:f[9] translation_vector:v scale:f flag:d perspective_vector:v
101 WORLD 0 assembly:p attribute:p body:p transform:p surface:p curve:p point:p alive:l attrib_def:p highest_id:d current_id:d
102 KEY 0 string:c[]
124 B_SURFACE 1 $C nurbs:p data:p
125 SURFACE_DATA 0 original_uint:i original_vint:i extended_uint:i extended_vint:i self_int:u original_u_start:c original_u_end:c original_v_start:c original_v_end:c extended_u_start:c extended_u_end:c extended_v_start:c extended_v_end:c analytic_form_type:c swept_form_type:c spun_form_type:c blend_form_type:c analytic_form:p swept_form:p spun_form:p blend_form:p
126 NURBS_SURF 0 u_periodic:l v_periodic:l u_degree:n v_degree:n n_u_vertices:d n_v_vertices:d u_knot_type:u v_knot_type:u n_u_knots:d n_v_knots:d rational:l u_closed:l v_closed:l surface_form:u vertex_dim:n bspline_vertices:p u_knot_mult:p v_knot_mult:p u_knots:p v_knots:p
127 KNOT_MULT 0 mult:n[]
128 KNOT_SET 0 knots:f[]
133 TRIMMED_CURVE 1 $C basis_curve:p point_1:v point_2:v parm_1:f parm_2:f
134 B_CURVE 1 $C nurbs:p data:p
135 CURVE_DATA 0 self_int:u analytic_form:p
136 NURBS_CURVE 0 degree:n n_vertices:d vertex_dim:n n_knots:d knot_type:u periodic:l closed:l rational:l curve_form:u bspline_vertices:p knot_mult:p knots:p
137 SP_CURVE 1 $C surface:p b_curve:p original:p tolerance_to_original:f
141 GEOMETRIC_OWNER 0 owner:p next:p previous:p shared_geometry:p
163 HELIX_SURF 0 axis_point:v axis:v sense:c parm_range:i pitch:f taper:f tolerance:f
184 HELIX 0 axis_point:v axis:v start_point:v sense:c parm_range:i pitch:f tolerance:f
";

/// Schemas 9000..=11999: fins have no attribute field (NIST sample `nut.x_t`, schema 9008).
const V9: &str = "
17 FIN 1 loop:p forward:p backward:p vertex:p other:p edge:p curve:p next_at_vx:p sense:c
";

/// Schemas 4000..=8999 (NIST samples, schemas 4035 and 4039): no regions, shells hold the
/// geometry chains, fins have no sense (the edge's own fin is the positive one), curves
/// carry an extra tolerance-like double after their sense, and several fields are absent.
const V4: &str = "
12 BODY 1 highest_node_id:d attributes_groups:p attribute_chains:p surface:p curve:p point:p res_size:f res_linear:f key:p state:u next:p previous:p owner:p body_type:u shell:p tolerance:f spare_1:p spare_2:p
13 SHELL 1 node_id:d attributes_groups:p body:p next:p face:p edge:p vertex:p surface:p curve:p point:p
14 FACE 1 node_id:d attributes_groups:p next:p previous:p loop:p shell:p surface:p sense:c
16 EDGE 1 node_id:d attributes_groups:p fin:p previous:p next:p curve:p
17 FIN 1 loop:p forward:p backward:p vertex:p other:p edge:p spare:p
18 VERTEX 1 node_id:d attributes_groups:p fin:p previous:p next:p point:p
30 LINE 1 $C $T pvec:v direction:v
31 CIRCLE 1 $C $T centre:v normal:v x_axis:v radius:f
32 ELLIPSE 1 $C $T centre:v normal:v x_axis:v major_radius:f minor_radius:f
38 INTERSECTION 1 $C $T surface:p[2] chart:p start:p end:p
40 CHART 0 base_parameter:f chart_count:d chordal_error:f angular_error:f parameter_error:f[2] hvec:h[]
70 LIST 1 node_id:d owner:p next:p previous:p list_type:d list_length:d block_length:d size_of_entry:d list_block:p finger_block:p finger_index:d
80 ATTRIB_DEF 1 next:p type_id:d identifier:p actions:u[8] legal_owners:l[12] fields:u[]
133 TRIMMED_CURVE 1 $C $T basis_curve:p point_1:v point_2:v parm_1:f parm_2:f
134 B_CURVE 1 $C $T nurbs:p data:p
137 SP_CURVE 1 $C $T surface:p b_curve:p original:p tolerance_to_original:f
";

/// Parses one table in the notation above.
fn parse_table(src: &str) -> Schema {
    let mut out = Schema::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut words = line.split_whitespace();
        let (Some(ty), Some(name), Some(vis)) = (words.next(), words.next(), words.next()) else { continue };
        let Ok(ty) = ty.parse::<u16>() else { continue };
        let mut fields = Vec::new();
        for w in words {
            let expanded: Vec<&str> = match w {
                "$C" => CURVE.split_whitespace().collect(),
                "$T" => vec!["tolerance:f"],
                other => vec![other],
            };
            for spec in expanded {
                if let Some(f) = parse_field(spec) {
                    fields.push(f);
                }
            }
        }
        out.insert(ty, Layout { name: name.to_string(), fields, visible: vis == "1" });
    }
    out
}

fn parse_field(spec: &str) -> Option<Field> {
    let (name, rest) = spec.split_once(':')?;
    let mut chars = rest.chars();
    let code = Code::from_char(chars.next()?)?;
    let tail = chars.as_str();
    let count = if tail.is_empty() {
        Count::One
    } else if tail == "[]" {
        Count::Var
    } else {
        let n = tail.strip_prefix('[')?.strip_suffix(']')?.parse::<u16>().ok()?;
        Count::Fixed(n)
    };
    Some(Field { name: name.to_string(), code, count })
}

/// Where the base schema of embedded-schema files (13006) differs from 12006. Worked out
/// from `.sldprt` partitions: the edit scripts of those files copy one more BODY field than
/// 12006 has, and the byte stream shows it is a byte after `body_type`.
const V13: &str = "
12 BODY 1 highest_node_id:d attributes_groups:p attribute_chains:p surface:p curve:p point:p key:p res_size:f res_linear:f ref_instance:p next:p previous:p state:u owner:p body_type:u nom_geom_state:u shell:p boundary_surface:p boundary_curve:p boundary_point:p region:p edge:p vertex:p
";

/// The layouts edit scripts of embedded schemas start from.
pub(crate) fn embedded_base() -> Schema {
    let mut s = schema(Family::V12);
    s.extend(parse_table(V13));
    s
}

/// The layouts of a family.
pub(crate) fn schema(family: Family) -> Schema {
    let mut s = parse_table(V12);
    let overrides = match family {
        Family::V12 => "",
        Family::V9 => V9,
        Family::V4 => V4,
    };
    s.extend(parse_table(overrides));
    if family == Family::V4 {
        // No regions in these schemas.
        s.remove(&19);
    }
    s
}

/// One instruction of an embedded edit script (see [`apply_delta`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Edit {
    Copy,
    Delete,
    Insert(Field),
    Append(Field),
}

/// The layout of a node type after its embedded edit script: copy, delete or insert fields
/// relative to the base layout, then append the rest. Fields the base has after the
/// script's end are dropped (the script covers every current field).
pub(crate) fn apply_delta(base: &Layout, edits: &[Edit]) -> Layout {
    let mut out = Vec::new();
    let mut from = base.fields.iter();
    for e in edits {
        match e {
            Edit::Copy => {
                if let Some(f) = from.next() {
                    out.push(f.clone());
                }
            }
            Edit::Delete => {
                from.next();
            }
            Edit::Insert(f) | Edit::Append(f) => out.push(f.clone()),
        }
    }
    Layout { name: base.name.clone(), fields: out, visible: base.visible }
}

/// A field type from an embedded schema's type string and element count.
pub(crate) fn field_from_script(name: String, type_str: &str, pointer: bool, n_elts: u32) -> Option<Field> {
    let code = if pointer { Code::P } else { Code::from_char(type_str.chars().next()?)? };
    let count = match n_elts {
        0 => Count::One,
        1 => Count::Var,
        n => Count::Fixed(u16::try_from(n).ok()?),
    };
    Some(Field { name, code, count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_parse_and_families_differ_where_the_samples_show() {
        let v12 = schema(Family::V12);
        let fin = v12.get(&17).map(|l| l.fields.len());
        assert_eq!(fin, Some(10));
        assert_eq!(v12.get(&30).map(|l| l.fields.len()), Some(9));
        assert!(v12.get(&74).is_some_and(Layout::variable));
        let v9 = schema(Family::V9);
        assert_eq!(v9.get(&17).and_then(|l| l.position("attributes_groups")), None);
        let v4 = schema(Family::V4);
        assert!(!v4.contains_key(&19));
        assert_eq!(v4.get(&31).and_then(|l| l.position("tolerance")), Some(7));
        assert_eq!(Family::of(4035), Some(Family::V4));
        assert_eq!(Family::of(3000), None);
    }

    #[test]
    fn the_embedded_base_has_the_extra_body_byte() {
        let base = embedded_base();
        let body = base.get(&12).unwrap();
        assert_eq!(body.fields.len(), 23);
        assert_eq!(body.position("nom_geom_state"), body.position("body_type").map(|i| i + 1));
        // Everything else is as in 12006.
        assert_eq!(base.get(&14), schema(Family::V12).get(&14));
    }

    #[test]
    fn a_delta_copies_deletes_inserts_and_appends() {
        let base = schema(Family::V12).remove(&15).unwrap_or_else(|| Layout {
            name: String::new(),
            fields: Vec::new(),
            visible: false,
        });
        let extra = Field { name: "extra".into(), code: Code::D, count: Count::One };
        let tail = Field { name: "tail".into(), code: Code::F, count: Count::One };
        let edits =
            [Edit::Copy, Edit::Delete, Edit::Insert(extra), Edit::Copy, Edit::Copy, Edit::Copy, Edit::Append(tail)];
        let out = apply_delta(&base, &edits);
        let names: Vec<&str> = out.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["node_id", "extra", "fin", "face", "next", "tail"]);
    }

    #[test]
    fn helical_forms_have_the_sizes_seen_in_solidworks_files() {
        // Binary sizes: 105 bytes for type 184 (3 vectors, a sense, 4 reals) and 89 for 163
        // (2 vectors, a sense, 5 reals), after the node index, as in `setScrew_M2x2_5`.
        let size = |code: Code, count: &Count| {
            let one = match code {
                Code::C | Code::U | Code::L => 1,
                Code::F => 8,
                Code::V | Code::H => 24,
                Code::I => 16,
                _ => 0,
            };
            one * match count {
                Count::One => 1,
                Count::Fixed(k) => usize::from(*k),
                Count::Var => 0,
            }
        };
        let base = embedded_base();
        let bytes = |ty: u16| base.get(&ty).map(|l| l.fields.iter().map(|f| size(f.code, &f.count)).sum::<usize>());
        assert_eq!(bytes(184), Some(105));
        assert_eq!(bytes(163), Some(89));
        assert!(schema(Family::V12).contains_key(&184));
    }
}
