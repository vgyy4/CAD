//! The symbol library browser's data (ADR 0057): a light index of a `.kicad_sym` library
//! (name, description, keywords, units per symbol), the filter the browser applies and
//! the preview of one symbol. Pure functions over library text: reading folders and
//! caching belong to the caller. Nothing here is KiCad library content; the user's
//! libraries are read, never bundled.

use crate::board::{Limits, Pt};
use crate::sch::{Error, LibSymbol, Mirror, Property, Shape, Symbol, SymbolLib};

/// Most symbols indexed per library; more are left out (`truncated`).
pub const MAX_SYMBOLS_PER_LIB: usize = 20_000;
/// Longest description or keyword text kept (characters).
const MAX_TEXT: usize = 300;

/// One symbol of a library, as the browser lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    pub name: String,
    pub description: String,
    pub keywords: String,
    /// Units (at least 1); `extends` symbols count their parent's.
    pub units: u32,
    pub power: bool,
}

/// The index of one library file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibraryInfo {
    /// The file's name, `Device.kicad_sym`.
    pub file: String,
    pub symbols: Vec<SymbolInfo>,
    /// More symbols than [`MAX_SYMBOLS_PER_LIB`].
    pub truncated: bool,
}

impl AsRef<LibraryInfo> for LibraryInfo {
    fn as_ref(&self) -> &LibraryInfo {
        self
    }
}

impl LibraryInfo {
    /// The library's name: the file name without `.kicad_sym` (KiCad's library nickname).
    pub fn nickname(&self) -> &str {
        self.file.strip_suffix(".kicad_sym").unwrap_or(&self.file)
    }
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

/// Indexes the text of a `.kicad_sym` file.
///
/// # Errors
/// [`Error`] for malformed files or files past `limits`.
pub fn index(file: &str, src: &str, limits: &Limits) -> Result<LibraryInfo, Error> {
    let lib = SymbolLib::read(src, limits)?;
    let mut out = LibraryInfo { file: clean(file), ..LibraryInfo::default() };
    for s in &lib.symbols {
        if out.symbols.len() >= MAX_SYMBOLS_PER_LIB {
            out.truncated = true;
            break;
        }
        // A derived symbol has no drawings of its own: its units come from the parent.
        let units = match s.extends.as_deref().and_then(|p| lib.symbols.iter().find(|x| x.name == p)) {
            Some(parent) if s.units.is_empty() => parent.unit_count(),
            _ => s.unit_count(),
        };
        out.symbols.push(SymbolInfo {
            name: clean(&s.name),
            description: clean(s.property("Description").unwrap_or("")),
            keywords: clean(s.property("ki_keywords").unwrap_or("")),
            units,
            power: s.power,
        });
    }
    Ok(out)
}

/// The symbols of `libs` that match `query`, as (library index, symbol index), best first
/// and at most `max`. Every whitespace-separated word of the query must occur
/// (case-insensitive) in the symbol's name, description or keywords, or in the library's
/// name. Ranking: an exact name, a name starting with the query, a name containing it,
/// then the rest; ties keep library and file order. An empty query lists everything.
pub fn search<L: AsRef<LibraryInfo>>(libs: &[L], query: &str, max: usize) -> Vec<(usize, usize)> {
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut hits: Vec<(u8, usize, usize)> = Vec::new();
    for (li, lib) in libs.iter().enumerate() {
        let lib = lib.as_ref();
        let nick = lib.nickname().to_lowercase();
        for (si, s) in lib.symbols.iter().enumerate() {
            let name = s.name.to_lowercase();
            let rank = if words.is_empty() {
                3
            } else {
                let hay = format!("{nick} {name} {} {}", s.description.to_lowercase(), s.keywords.to_lowercase());
                if !words.iter().all(|w| hay.contains(w)) {
                    continue;
                }
                if name == q {
                    0
                } else if name.starts_with(&q) {
                    1
                } else if name.contains(&q) {
                    2
                } else {
                    3
                }
            };
            hits.push((rank, li, si));
        }
    }
    hits.sort_by_key(|h| h.0);
    hits.into_iter().take(max).map(|(_, l, s)| (l, s)).collect()
}

/// The symbol `name` of a library text with `extends` resolved, for the preview.
///
/// # Errors
/// [`Error`] for malformed files, or a missing symbol.
pub fn load_symbol(src: &str, name: &str, limits: &Limits) -> Result<LibSymbol, Error> {
    let lib = SymbolLib::read(src, limits)?;
    let node = lib
        .embedded_node(name, name)
        .ok_or_else(|| Error::Edit(format!("the library has no symbol '{}'", clean(name))))?;
    LibSymbol::from_node(&node, limits.max_items)?.ok_or_else(|| Error::Edit("the symbol has no name".into()))
}

impl LibSymbol {
    /// The symbol as it would be placed at the origin, for drawing a preview (fields at
    /// the library's places).
    pub fn preview(&self, unit: u32) -> Symbol {
        let mut s = Symbol {
            lib_id: self.name.clone(),
            lib_name: None,
            at: [0.0, 0.0],
            angle: 0.0,
            mirror: Mirror::None,
            unit: unit.clamp(1, self.unit_count()),
            style: 1,
            uuid: None,
            properties: Vec::new(),
            instances: Vec::new(),
            in_bom: true,
            on_board: true,
            dnp: false,
            node: 0,
        };
        let props: Vec<Property> = self
            .properties
            .iter()
            .map(|p| Property {
                key: p.key.clone(),
                value: if p.key == "Reference" { self.prefix() + "?" } else { p.value.clone() },
                at: s.place(p.at),
                angle: p.angle,
                hidden: p.hidden,
            })
            .collect();
        s.properties = props;
        s
    }

    /// The extent of a unit's drawings and pins (library coordinates, Y up): `None` when
    /// it draws nothing.
    pub fn bounds(&self, unit: u32, style: u32) -> Option<[Pt; 2]> {
        let mut b: Option<[Pt; 2]> = None;
        let mut add = |p: Pt| {
            b = Some(match b {
                None => [p, p],
                Some([lo, hi]) => [[lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]],
            });
        };
        for u in self.units_of(unit, style) {
            for g in &u.graphics {
                match &g.shape {
                    Shape::Rect { a, b } => {
                        add(*a);
                        add(*b);
                    }
                    Shape::Poly { pts } => pts.iter().for_each(|p| add(*p)),
                    Shape::Circle { center, radius } => {
                        add([center[0] - radius, center[1] - radius]);
                        add([center[0] + radius, center[1] + radius]);
                    }
                    Shape::Arc { start, mid, end } => [start, mid, end].into_iter().for_each(|p| add(*p)),
                    Shape::Text { .. } => {}
                }
            }
            for p in &u.pins {
                add(p.at);
            }
        }
        b
    }
}
