//! SolidWorks assemblies (`.sldasm`, ADR 0043 amendment). An assembly is a chunk container
//! whose stream `swXmlContents/COMPINSTANCETREE` is an XML document: `swFile` elements list
//! the files (a full path each), `swModel` elements the models (a file, a configuration) and
//! one root `swModel` holds a `swReference` per component instance with its model, flags
//! and `swTransform`, sixteen numbers in the row-vector layout (`p' = p M`, translation in
//! the last row), lengths in metres. The XML is scanned by a small bounded tag reader (no
//! entity expansion beyond the five predefined and numeric ones, no DTD, no external data).

use crate::{Limits, SldprtError, chunks};

/// Most components kept from one assembly.
pub const MAX_COMPONENTS: usize = 20_000;
/// Most XML elements looked at.
const MAX_TAGS: usize = 2_000_000;
/// Longest attribute value kept.
const MAX_VALUE: usize = 4096;

const STREAM: &str = "swXmlContents/COMPINSTANCETREE";

/// What a component refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    /// An external part file (`.sldprt`).
    Part,
    /// An external sub-assembly file (`.sldasm`); its own file lists its components.
    Assembly,
    /// A component made inside the assembly (`Part1^Assembly`), stored in the assembly file
    /// itself in a stream this reader does not decode.
    Virtual,
}

/// One component instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// Instance name: model name and `-` instance number (`Bracket-2`).
    pub name: String,
    /// File name (no folder) of the referenced part or assembly, as SolidWorks stored it.
    pub file: String,
    /// The configuration of the model the instance uses.
    pub configuration: String,
    pub kind: ComponentKind,
    /// Instance to assembly space, metres, column vectors, rows first: the translation is
    /// the last column.
    pub placement: [[f64; 4]; 4],
    pub suppressed: bool,
    pub hidden: bool,
    /// Bounding box of the referenced model in its own space (min xyz, max xyz; metres), when
    /// the file stores one.
    pub bbox: Option<[f64; 6]>,
    /// For a [`ComponentKind::Virtual`] component: the stream of the assembly file that holds
    /// its part (`VirtualComp/Part1`), itself a complete part file; see [`embedded_part`].
    pub embedded: Option<String>,
}

/// The components of an assembly's top level.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    pub components: Vec<Component>,
    pub warnings: Vec<String>,
}

#[derive(Default, Clone)]
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    open: bool,
    closing: bool,
}

impl Tag {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i..).unwrap_or_default();
        // Entities are short: look for the ';' in the next few bytes only, so a text full of
        // '&' stays linear (security review S6).
        let Some(j) = tail.bytes().take(11).position(|b| b == b';') else {
            out.push('&');
            rest = tail.get(1..).unwrap_or_default();
            continue;
        };
        let ent = tail.get(1..j).unwrap_or_default();
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix('x') {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = tail.get(j + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// The next element tag at or after `at`, skipping text, comments, processing instructions
/// and declarations; the position after it.
fn next_tag(s: &str, mut at: usize) -> Option<(Tag, usize)> {
    loop {
        let lt = at + s.get(at..)?.find('<')?;
        let rest = s.get(lt + 1..)?;
        if rest.starts_with('?') || rest.starts_with('!') {
            let end = if rest.starts_with("!--") { "-->" } else { ">" };
            at = lt + 1 + rest.find(end)? + end.len();
            continue;
        }
        let b = rest.as_bytes();
        let (closing, mut i) = if b.first() == Some(&b'/') { (true, 1) } else { (false, 0) };
        let start = i;
        while b.get(i).is_some_and(|c| !c.is_ascii_whitespace() && !matches!(c, b'>' | b'/')) {
            i += 1;
        }
        let name = rest.get(start..i)?.to_string();
        let mut attrs = Vec::new();
        let mut open = true;
        loop {
            while b.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            match b.get(i)? {
                b'>' => {
                    i += 1;
                    break;
                }
                b'/' => {
                    open = false;
                    i += 1;
                }
                _ => {
                    let k0 = i;
                    while b.get(i).is_some_and(|c| !c.is_ascii_whitespace() && !matches!(c, b'=' | b'>' | b'/')) {
                        i += 1;
                    }
                    let key = rest.get(k0..i)?.to_string();
                    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
                        i += 1;
                    }
                    if b.get(i) != Some(&b'=') {
                        if i == k0 {
                            i += 1;
                        }
                        continue;
                    }
                    i += 1;
                    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
                        i += 1;
                    }
                    let q = *b.get(i)?;
                    if q != b'"' && q != b'\'' {
                        return None;
                    }
                    i += 1;
                    let v0 = i;
                    while *b.get(i)? != q {
                        i += 1;
                    }
                    let raw = rest.get(v0..i)?;
                    i += 1;
                    if attrs.len() < 64 {
                        attrs.push((key, unescape(raw.get(..raw.len().min(MAX_VALUE)).unwrap_or_default())));
                    }
                }
            }
        }
        return Some((Tag { name, attrs, open: open && !closing, closing }, lt + 1 + i));
    }
}

struct FileRec {
    path: String,
    doc_type: String,
}

struct ModelRec {
    file: Option<String>,
    config: String,
    bbox: Option<[f64; 6]>,
}

struct RefRec {
    model: String,
    tag: Tag,
}

fn numbers<const N: usize>(s: &str) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    let mut it = s.split_ascii_whitespace();
    for o in &mut out {
        let v: f64 = it.next()?.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        *o = v;
    }
    it.next().is_none().then_some(out)
}

/// The file name of a stored path (either separator), if it is a plain name.
fn base_name(path: &str) -> Option<String> {
    let n = path.rsplit(['\\', '/']).next()?.trim();
    (!n.is_empty() && n != "." && n != ".." && !n.chars().any(char::is_control)).then(|| n.to_string())
}

fn flag(t: &Tag, k: &str) -> bool {
    t.get(k).is_some_and(|v| v.eq_ignore_ascii_case("yes") || v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Parses the component tree XML.
fn parse(xml: &str) -> Result<Assembly, SldprtError> {
    use std::collections::HashMap;
    let mut files: HashMap<String, FileRec> = HashMap::new();
    let mut models: HashMap<String, ModelRec> = HashMap::new();
    // Models in document order, each with the references written inside it.
    let mut order: Vec<(String, Vec<RefRec>)> = Vec::new();
    let mut current: Option<usize> = None;
    let (mut at, mut tags) = (0usize, 0usize);
    while let Some((t, next)) = next_tag(xml, at) {
        at = next;
        tags += 1;
        if tags > MAX_TAGS {
            return Err(SldprtError::TooLarge("too many XML elements in the component tree".into()));
        }
        match (t.name.as_str(), t.closing) {
            ("swFile", false) => {
                if let (Some(id), Some(path)) = (t.get("id"), t.get("swPath"))
                    && files.len() < MAX_COMPONENTS
                {
                    files.insert(
                        id.to_string(),
                        FileRec { path: path.to_string(), doc_type: t.get("swDocType").unwrap_or_default().into() },
                    );
                }
            }
            ("swModel", false) => {
                if let Some(id) = t.get("id")
                    && models.len() < MAX_COMPONENTS
                {
                    models.insert(
                        id.to_string(),
                        ModelRec {
                            file: t.get("swFileRef").map(str::to_string),
                            config: t.get("swConfigurationName").unwrap_or_default().to_string(),
                            bbox: t.get("swBoundingBox").and_then(numbers::<6>),
                        },
                    );
                    if t.open {
                        order.push((id.to_string(), Vec::new()));
                        current = Some(order.len() - 1);
                    }
                }
            }
            ("swModel", true) => current = None,
            ("swReference", false) => {
                if let (Some(i), Some(model)) = (current, t.get("swModelRef"))
                    && let Some((_, refs)) = order.get_mut(i)
                    && refs.len() <= MAX_COMPONENTS
                {
                    refs.push(RefRec { model: model.to_string(), tag: t.clone() });
                }
            }
            _ => {}
        }
    }
    // The root: the model with components that no other model's components use.
    let used: std::collections::HashSet<&str> =
        order.iter().flat_map(|(_, r)| r.iter().map(|x| x.model.as_str())).collect();
    let Some((_, refs)) = order.iter().find(|(id, r)| !r.is_empty() && !used.contains(id.as_str())) else {
        return Err(SldprtError::Malformed("the component tree lists no components".into()));
    };
    let mut warnings = Vec::new();
    let mut components = Vec::new();
    for r in refs {
        if components.len() >= MAX_COMPONENTS {
            warnings.push(format!("only the first {MAX_COMPONENTS} components are read"));
            break;
        }
        let Some(model) = models.get(&r.model) else { continue };
        let file = model.file.as_ref().and_then(|f| files.get(f));
        let name = match (r.tag.get("swName"), r.tag.get("swReferenceNumber")) {
            (Some(n), Some(k)) => format!("{n}-{k}"),
            (Some(n), None) => n.to_string(),
            _ => r.model.clone(),
        };
        let kind = if flag(&r.tag, "swIsVirtualComponent") {
            ComponentKind::Virtual
        } else if file.is_some_and(|f| f.doc_type.eq_ignore_ascii_case("ASSEMBLY")) {
            ComponentKind::Assembly
        } else {
            ComponentKind::Part
        };
        let Some(file_name) = file.and_then(|f| base_name(&f.path)) else {
            warnings.push(format!("component '{name}' names no usable file"));
            continue;
        };
        // Row-vector layout in the file; column vectors out.
        let placement = match r.tag.get("swTransform").and_then(numbers::<16>) {
            Some(m) => {
                let at = |row: usize, col: usize| m.get(row * 4 + col).copied().unwrap_or(0.0);
                let mut p = [[0.0; 4]; 4];
                for (i, row) in p.iter_mut().enumerate() {
                    for (j, v) in row.iter_mut().enumerate() {
                        *v = at(j, i);
                    }
                }
                p
            }
            None => {
                warnings.push(format!("component '{name}' has no readable placement; it is left at the origin"));
                [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
            }
        };
        // `Part1^<assembly>` lives in the stream `VirtualComp/Part1` (2 of 2 in the sample).
        let embedded = (kind == ComponentKind::Virtual)
            .then(|| r.tag.get("swName").and_then(|n| n.split('^').next()))
            .flatten()
            .filter(|n| !n.is_empty() && n.len() <= 200 && !n.chars().any(char::is_control))
            .map(|n| format!("VirtualComp/{n}"));
        components.push(Component {
            name,
            file: file_name,
            configuration: model.config.clone(),
            kind,
            placement,
            suppressed: flag(&r.tag, "swSuppressed"),
            hidden: flag(&r.tag, "swHidden"),
            bbox: model.bbox,
            embedded,
        });
    }
    Ok(Assembly { components, warnings })
}

/// Reads the component list of a `.sldasm` file.
///
/// # Errors
/// [`SldprtError`] for files that are not chunk containers, have no component tree
/// (a part), or are damaged or over the limits.
pub fn read_assembly(bytes: &[u8], limits: &Limits) -> Result<Assembly, SldprtError> {
    match crate::detect(bytes) {
        Some(crate::Container::Chunks) => {}
        Some(crate::Container::Ole2) => {
            return Err(SldprtError::Malformed("assemblies in the older compound-file container are not read".into()));
        }
        None => return Err(SldprtError::NotSldprt),
    }
    let st = chunks::streams(bytes, limits, |n| n == STREAM)?.pop();
    let Some(st) = st else {
        return Err(SldprtError::Malformed("the file has no component tree (is it a part?)".into()));
    };
    let xml = String::from_utf8_lossy(&st.data);
    parse(&xml)
}

/// The part file a virtual component keeps inside the assembly: the stream `stream` (a
/// [`Component::embedded`] name, which must start with `VirtualComp/`). The bytes are a
/// complete part container, readable with [`crate::extract_xt`] and [`crate::local_bodies`].
/// `None` when the assembly has no such stream.
///
/// # Errors
/// [`SldprtError`] for other containers and over-limit files.
pub fn embedded_part(bytes: &[u8], limits: &Limits, stream: &str) -> Result<Option<Vec<u8>>, SldprtError> {
    if !stream.starts_with("VirtualComp/") {
        return Ok(None);
    }
    match crate::detect(bytes) {
        Some(crate::Container::Chunks) => {}
        Some(crate::Container::Ole2) => return Ok(None),
        None => return Err(SldprtError::NotSldprt),
    }
    Ok(chunks::streams(bytes, limits, |n| n == stream)?.pop().map(|s| s.data))
}
