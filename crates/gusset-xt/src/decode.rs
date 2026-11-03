//! The node stream: after the flag and the modeller and schema strings, nodes follow as
//! `type [length] index fields... [user fields]` until the terminator (type 1).

use crate::reader::{Binary, Source, Text, Val, Width};
use crate::schema::{self, Code, Count, Edit, Family, Layout, Schema};
use crate::{Limits, XtError};
use std::collections::{BTreeMap, BTreeSet};

/// One node: its type and its field values (in the order of its layout).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub ty: u16,
    pub values: Vec<Val>,
}

/// Every node of a file by index, with the layouts that describe them.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: BTreeMap<u32, Node>,
    pub(crate) layouts: Schema,
    pub schema_number: u32,
    pub modeller: String,
}

impl Graph {
    pub fn node(&self, idx: u32) -> Option<&Node> {
        self.nodes.get(&idx)
    }

    /// The type name of a node.
    pub fn type_name(&self, idx: u32) -> Option<&str> {
        let n = self.nodes.get(&idx)?;
        self.layouts.get(&n.ty).map(|l| l.name.as_str())
    }

    /// A named field of a node.
    pub fn field(&self, idx: u32, name: &str) -> Option<&Val> {
        let n = self.nodes.get(&idx)?;
        let pos = self.layouts.get(&n.ty)?.position(name)?;
        n.values.get(pos)
    }

    /// A pointer field; 0 (null) when absent, null or pointing at no node.
    pub fn ptr(&self, idx: u32, name: &str) -> u32 {
        match self.field(idx, name) {
            Some(Val::Ptr(p)) if self.nodes.contains_key(p) => *p,
            _ => 0,
        }
    }

    pub fn real(&self, idx: u32, name: &str) -> Option<f64> {
        match self.field(idx, name)? {
            Val::Real(v) => Some(*v),
            Val::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn int(&self, idx: u32, name: &str) -> Option<i64> {
        match self.field(idx, name)? {
            Val::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn vec3(&self, idx: u32, name: &str) -> Option<[f64; 3]> {
        match self.field(idx, name)? {
            Val::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn ch(&self, idx: u32, name: &str) -> Option<u8> {
        match self.field(idx, name)? {
            Val::Char(c) => Some(*c),
            _ => None,
        }
    }

    pub fn list(&self, idx: u32, name: &str) -> &[Val] {
        match self.field(idx, name) {
            Some(Val::List(v)) => v,
            _ => &[],
        }
    }

    /// A string field (a list of characters).
    pub fn string(&self, idx: u32, name: &str) -> String {
        self.list(idx, name)
            .iter()
            .filter_map(|v| if let Val::Char(c) = v { Some(char::from(*c)) } else { None })
            .collect()
    }
}

/// Decodes a whole file.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Graph, XtError> {
    let header = crate::reader::header(bytes);
    let data = bytes.get(header.data_start..).unwrap_or_default();
    // Skip blank lines before the flag.
    let lead = data.iter().take_while(|b| b.is_ascii_whitespace()).count();
    let data = data.get(lead..).unwrap_or_default();
    match data.first() {
        Some(b'T') => {
            let mut src = Text::new(data.get(1..).unwrap_or_default());
            let (modeller, key) = text_prefix(&mut src)?;
            run(&mut src, modeller, &key, limits)
        }
        Some(b'P') if data.get(..2) == Some(b"PS") => {
            let mut src = Binary::new(data.get(4..).unwrap_or_default());
            let (modeller, key) = binary_prefix(&mut src)?;
            run(&mut src, modeller, &key, limits)
        }
        Some(b'B') => Err(XtError::Unsupported(
            "bare binary XT files (machine-dependent) cannot be read; save as text or neutral binary".into(),
        )),
        _ => Err(XtError::Malformed("no Parasolid data after the header (expected the T or PS flag)".into())),
    }
}

/// Text: `<n> <modeller string><m> <schema key>` (the lengths are numbers).
fn text_prefix(src: &mut Text) -> Result<(String, String), XtError> {
    let n = usize::try_from(src.int(Width::Int)?.unwrap_or(0)).unwrap_or(0).min(4096);
    let modeller = String::from_utf8_lossy(&src.chars(n)?).into_owned();
    let m = usize::try_from(src.int(Width::Int)?.unwrap_or(0)).unwrap_or(0).min(4096);
    let key = String::from_utf8_lossy(&src.chars(m)?).into_owned();
    Ok((modeller, key))
}

/// Neutral binary: 16-bit length and modeller string, 32-bit length and schema key.
fn binary_prefix(src: &mut Binary) -> Result<(String, String), XtError> {
    let n = usize::from(src.u16()?).min(4096);
    let modeller = String::from_utf8_lossy(&src.chars(n)?).into_owned();
    let m = usize::try_from(src.i32()?).unwrap_or(0).min(4096);
    let key = String::from_utf8_lossy(&src.chars(m)?).into_owned();
    Ok((modeller, key))
}

/// `SCH_<modeller>_<schema>[_<base>]`.
fn parse_key(key: &str) -> Result<(u32, Option<u32>), XtError> {
    let bad = || XtError::Malformed(format!("unrecognised schema key '{key}'"));
    let mut parts = key.trim().strip_prefix("SCH_").ok_or_else(bad)?.split('_');
    let _modeller = parts.next().ok_or_else(bad)?;
    let schema: u32 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    let base = parts.next().and_then(|s| s.parse().ok());
    Ok((schema, base))
}

fn run<S: Source>(src: &mut S, modeller: String, key: &str, limits: &Limits) -> Result<Graph, XtError> {
    let (number, base) = parse_key(key)?;
    let family = Family::of(number)
        .ok_or_else(|| XtError::Unsupported(format!("XT schema {number} (older than Parasolid V4) cannot be read")))?;
    // With an embedded schema the layouts are edits of the base (V13's 13006, for which
    // the V12 table stands in) and the header carries the number of node types.
    let mut layouts = schema::schema(if base.is_some() { Family::V12 } else { family });
    if base.is_some() {
        src.int(Width::Short)?;
    }
    let user_fields = usize::try_from(src.int(Width::Int)?.unwrap_or(0)).unwrap_or(0);
    if user_fields > 16 {
        return Err(XtError::Malformed(format!("user field size {user_fields} is above 16")));
    }
    let mut seen: BTreeSet<u16> = BTreeSet::new();
    let mut nodes: BTreeMap<u32, Node> = BTreeMap::new();
    let mut elements = 0usize;
    loop {
        let ty = src.int(Width::Short)?.ok_or_else(|| XtError::Malformed("a node type is unset".into()))?;
        let ty = u16::try_from(ty).map_err(|_| XtError::Malformed(format!("node type {ty} is out of range")))?;
        if ty == 1 {
            break;
        }
        if base.is_some() && seen.insert(ty) {
            read_delta(src, ty, &mut layouts)?;
        }
        let layout = layouts.get(&ty).ok_or_else(|| {
            XtError::Unsupported(format!(
                "node type {ty} at byte {} is not known for schema {number}; the file cannot be read further",
                src.offset()
            ))
        })?;
        let len = if layout.variable() {
            let n = src.int(Width::Int)?.unwrap_or(0);
            let n = usize::try_from(n).map_err(|_| XtError::Malformed("a negative length".into()))?;
            if n > src.remaining() {
                return Err(XtError::Malformed(format!("a node claims {n} elements, more than the file holds")));
            }
            n
        } else {
            0
        };
        let idx = src.ptr()?;
        let mut values = Vec::with_capacity(layout.fields.len());
        for f in &layout.fields {
            let v = match f.count {
                Count::One => value(src, f.code)?,
                Count::Fixed(k) => Val::List((0..k).map(|_| value(src, f.code)).collect::<Result<_, _>>()?),
                Count::Var => {
                    elements += len;
                    if elements > limits.max_elements {
                        return Err(XtError::TooLarge(format!("more than {} values", limits.max_elements)));
                    }
                    if f.code == Code::C {
                        Val::List(src.chars(len)?.into_iter().map(Val::Char).collect())
                    } else {
                        Val::List((0..len).map(|_| value(src, f.code)).collect::<Result<_, _>>()?)
                    }
                }
            };
            values.push(v);
        }
        if layout.visible {
            for _ in 0..user_fields {
                src.int(Width::Int)?;
            }
        }
        if nodes.len() >= limits.max_nodes {
            return Err(XtError::TooLarge(format!("more than {} nodes", limits.max_nodes)));
        }
        nodes.insert(idx, Node { ty, values });
    }
    Ok(Graph { nodes, layouts, schema_number: number, modeller })
}

fn value<S: Source>(src: &mut S, code: Code) -> Result<Val, XtError> {
    let int = |v: Option<i64>| v.map_or(Val::Unset, Val::Int);
    Ok(match code {
        Code::U => int(src.int(Width::Byte)?),
        Code::N | Code::W => int(src.int(Width::Short)?),
        Code::D => int(src.int(Width::Int)?),
        Code::C => Val::Char(src.ch()?),
        Code::L => Val::Int(i64::from(src.logical()?)),
        Code::P => Val::Ptr(src.ptr()?),
        Code::F => src.real()?.map_or(Val::Unset, Val::Real),
        Code::V | Code::H => match src.real()? {
            // A wholly unset vector is a single unset value.
            None => Val::Unset,
            Some(x) => Val::Vec3([x, src.real()?.unwrap_or(0.0), src.real()?.unwrap_or(0.0)]),
        },
        Code::I => Val::Pair([src.real()?.unwrap_or(0.0), src.real()?.unwrap_or(0.0)]),
        Code::B => {
            let mut a = [0.0; 6];
            for v in &mut a {
                *v = src.real()?.unwrap_or(0.0);
            }
            Val::Six(a)
        }
    })
}

/// The embedded schema of one node type (V14 on): 255 for "as the base", else the new
/// field list as an edit script, or the full description of a type the base lacks.
fn read_delta<S: Source>(src: &mut S, ty: u16, layouts: &mut Schema) -> Result<(), XtError> {
    let n = src.int(Width::Byte)?.unwrap_or(0);
    if n == 255 {
        return Ok(());
    }
    let Some(base) = layouts.get(&ty).cloned() else {
        // A type the base schema does not have: name, description, then every field.
        let name = src.short_string()?;
        let _description = src.short_string()?;
        let mut fields = Vec::new();
        for _ in 0..n {
            fields.push(script_field(src)?);
        }
        layouts.insert(ty, Layout { name, fields, visible: false });
        return Ok(());
    };
    let mut edits = Vec::new();
    for _ in 0..4096 {
        match src.ch()? {
            b'C' => edits.push(Edit::Copy),
            b'D' => edits.push(Edit::Delete),
            b'I' => edits.push(Edit::Insert(script_field(src)?)),
            b'A' => edits.push(Edit::Append(script_field(src)?)),
            b'Z' => {
                layouts.insert(ty, schema::apply_delta(&base, &edits));
                return Ok(());
            }
            other => {
                return Err(XtError::Malformed(format!(
                    "unknown instruction '{}' in the embedded schema of node type {ty}",
                    char::from(other)
                )));
            }
        }
    }
    Err(XtError::Malformed(format!("the embedded schema of node type {ty} does not end")))
}

/// `name ptr_class n_elts [type] [xmt_code]`.
fn script_field<S: Source>(src: &mut S) -> Result<schema::Field, XtError> {
    let name = src.short_string()?;
    let class = src.int(Width::Short)?.unwrap_or(0);
    let n_elts = src.ptr()?;
    let type_str = if class == 0 { src.short_string()? } else { String::new() };
    if n_elts == 1 {
        let _transmitted = src.logical()?;
    }
    schema::field_from_script(name.clone(), &type_str, class != 0, n_elts)
        .ok_or_else(|| XtError::Malformed(format!("embedded field '{name}' has an unknown type '{type_str}'")))
}
