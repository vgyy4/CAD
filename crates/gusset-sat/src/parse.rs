//! Version 1 files have only the first header line; versions 1 to 3 write edges without
//! parameters (version 1 with senses `0`/`1`). Versions 4 to 6 share one layout (read from version 5 and 6 samples). The record stream: three header lines (`version records bodies flags`, the product
//! line, `scale resabs resnor`), then records `type fields... #` until the end marker.
//! A field is a pointer `$n` (record number from 0, `$-1` for none) or a word (a number,
//! `I` for an infinite interval end, `forward`, a string). From version 7 strings are
//! written `@<length> <text>` and every record carries two extra fields after its
//! attribute pointer (an integer and a pointer, both unused here); they are dropped so
//! both layouts read the same.

use crate::{Limits, SatError};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ptr(i64),
    Word(String),
}

/// One record: its type (`face`, `cone-surface`) and its fields after the type.
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    pub ty: String,
    pub f: Vec<Tok>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sat {
    pub version: u32,
    /// Millimetres per model unit (the third header line).
    pub scale: f64,
    pub recs: Vec<Rec>,
    /// Subtype definitions (`{ exactsur ...`, every `{` not followed by `ref`) in file
    /// order, as (record, field of the `{`): `{ ref n }` names the n-th.
    pub subtypes: Vec<(usize, usize)>,
}

impl Sat {
    pub fn rec(&self, i: i64) -> Option<&Rec> {
        usize::try_from(i).ok().and_then(|i| self.recs.get(i))
    }
}

/// The attribute type that names an entity's Fusion material.
const MATERIAL_ATTRIB: &str = "NEUTRON_Material_attrib_def";

/// A lowercase hyphenated UUID (36 characters).
pub(crate) fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        })
}

impl Sat {
    /// The Fusion material id (a UUID) of every entity (by record number) that carries a
    /// `NEUTRON_Material_attrib_def` `ATTRIB_CUSTOM` attribute. An attribute record reads
    /// `$-1 $link $owner name fields...`; the chain links are not followed (Fusion writes
    /// them in no order that was needed), the owner field names the entity.
    pub fn materials(&self) -> std::collections::HashMap<i64, &str> {
        let mut out = std::collections::HashMap::new();
        for r in &self.recs {
            if r.word(3) == Some(MATERIAL_ATTRIB)
                && r.ty.ends_with("ATTRIB_CUSTOM-attrib")
                && let Some(owner) = r.ptr(2)
                && let Some(id) = r.f.iter().skip(4).find_map(|t| match t {
                    Tok::Word(w) if is_uuid(w) => Some(w.as_str()),
                    _ => None,
                })
            {
                out.entry(owner).or_insert(id);
            }
        }
        out
    }
}

/// The attribute type Fusion gives faces to follow them through its feature history.
const TAG_ATTRIB: &str = "generic_tag_attrib_def";

impl Sat {
    /// The tag of every face that carries a `generic_tag_attrib_def` attribute: `(face record,
    /// tag)`, the tag being the attribute's words after its definition name joined by commas
    /// (kind, tag id, owning feature and the like). The same face has the same tag in the body
    /// blob and in the history blob written before it.
    pub fn face_tags(&self) -> Vec<(i64, String)> {
        let mut out = Vec::new();
        for r in &self.recs {
            if r.word(3) != Some(TAG_ATTRIB) || !r.ty.ends_with("ATTRIB_CUSTOM-attrib") {
                continue;
            }
            let Some(owner) = r.ptr(2) else { continue };
            if self.rec(owner).is_none_or(|o| o.ty != "face") {
                continue;
            }
            let words: Vec<&str> =
                r.f.iter()
                    .skip(8)
                    .filter_map(|t| match t {
                        Tok::Word(w) => Some(w.as_str()),
                        Tok::Ptr(_) => None,
                    })
                    .collect();
            if words.len() >= 3 {
                out.push((owner, words.join(",")));
            }
        }
        out
    }
}

impl Rec {
    /// Pointer field `i`; `None` for `$-1`, a word or a missing field.
    pub fn ptr(&self, i: usize) -> Option<i64> {
        match self.f.get(i)? {
            Tok::Ptr(p) if *p >= 0 => Some(*p),
            _ => None,
        }
    }

    pub fn word(&self, i: usize) -> Option<&str> {
        match self.f.get(i)? {
            Tok::Word(w) => Some(w),
            Tok::Ptr(_) => None,
        }
    }

    pub fn num(&self, i: usize) -> Option<f64> {
        self.word(i)?.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    pub fn vec3(&self, i: usize) -> Option<[f64; 3]> {
        Some([self.num(i)?, self.num(i + 1)?, self.num(i + 2)?])
    }
}

struct Lexer<'a> {
    b: &'a [u8],
    pos: usize,
    /// Strings are `@len text` (version 7 on).
    at_strings: bool,
}

impl Lexer<'_> {
    fn skip_space(&mut self) {
        while self.b.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    /// The rest of the current line (header lines).
    fn line(&mut self) -> &[u8] {
        let start = self.pos;
        while self.b.get(self.pos).is_some_and(|c| *c != b'\n') {
            self.pos += 1;
        }
        let out = self.b.get(start..self.pos).unwrap_or_default();
        self.pos += 1;
        out
    }

    fn token(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.pos;
        if self.at_strings && self.b.get(self.pos) == Some(&b'@') {
            // `@<len> <text>`: the text may hold spaces and `#`.
            let mut p = self.pos + 1;
            let mut n = 0usize;
            while let Some(d) = self.b.get(p).filter(|c| c.is_ascii_digit()) {
                n = n.saturating_mul(10).saturating_add(usize::from(d - b'0'));
                p += 1;
            }
            if p > self.pos + 1 && self.b.get(p) == Some(&b' ') {
                let s = p + 1;
                let e = s.saturating_add(n).min(self.b.len());
                self.pos = e;
                return Some(String::from_utf8_lossy(self.b.get(s..e).unwrap_or_default()).into_owned());
            }
        }
        while self.b.get(self.pos).is_some_and(|c| !c.is_ascii_whitespace()) {
            self.pos += 1;
        }
        (self.pos > start)
            .then(|| String::from_utf8_lossy(self.b.get(start..self.pos).unwrap_or_default()).into_owned())
    }
}

/// The words of a header line (at most its first 4 KiB: a hostile file's endless line is
/// not split into millions of words).
fn words(line: &[u8]) -> Vec<String> {
    let line = line.get(..line.len().min(4096)).unwrap_or_default();
    String::from_utf8_lossy(line).split_whitespace().take(64).map(str::to_string).collect()
}

/// Reads a SAT text file or its binary form (SAB, `ACIS BinaryFile` / `ASM BinaryFile`).
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Sat, SatError> {
    for magic in [&b"ACIS BinaryFile"[..], b"ASM BinaryFile"] {
        if let Some(rest) = bytes.strip_prefix(magic) {
            return parse_binary(rest, limits);
        }
    }
    parse_text(bytes, limits)
}

/// Binary SAB, read from Fusion 360 `.smb` files: `ASM BinaryFile<w>` (`w` the integer
/// width, 4 or 8), four integers (version, records, bodies, flags), then tagged values:
/// 0x04 integer, 0x06 double, 0x07/0x08/0x12 string (u8/u16/u32 length), 0x0a/0x0b the
/// second/first value of a two-valued word (`reversed`/`forward`, finite/`I`), 0x0c pointer,
/// 0x0d identifier ending a type name whose earlier parts are 0x0e identifiers, 0x0f/0x10
/// `{`/`}`, 0x11 end of record, 0x13/0x14 three doubles (position, vector), 0x15 an
/// enumeration value. The header's product strings and unit doubles come first; the
/// records then read like version 7 text ones.
fn parse_binary(b: &[u8], limits: &Limits) -> Result<Sat, SatError> {
    let bad = |m: String| SatError::Malformed(m);
    let width = match b.first() {
        Some(b'8') => 8,
        Some(b'4') => 4,
        _ => return Err(bad("unknown binary integer width".into())),
    };
    let mut rd = Bin { b, pos: 1, width };
    let version = u32::try_from(rd.int()?).map_err(|_| bad("a bad version".into()))?;
    for _ in 0..3 {
        rd.int()?;
    }
    let mut recs: Vec<Rec> = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let mut ty = String::new();
    // The type is complete once its final 0x0d identifier is read.
    let mut typed = false;
    let mut units: Vec<f64> = Vec::new();
    let mut tokens = 0usize;
    while rd.pos < b.len() {
        let tag = rd.take(1)?.first().copied().unwrap_or(0);
        tokens += 1;
        if tokens > limits.max_tokens {
            return Err(SatError::TooLarge(format!("more than {} values", limits.max_tokens)));
        }
        // Before the first record: the product strings and the unit doubles.
        let header = recs.is_empty() && ty.is_empty();
        match tag {
            0x04 | 0x15 => cur.push(Tok::Word(rd.int()?.to_string())),
            0x06 => {
                let v = rd.f64()?;
                if header {
                    units.push(v);
                } else {
                    cur.push(Tok::Word(format!("{v:?}")));
                }
            }
            0x07 | 0x08 | 0x12 => {
                let s = rd.string(tag)?;
                if !header {
                    cur.push(Tok::Word(s));
                }
            }
            0x0e | 0x0d => {
                let s = rd.string(tag)?;
                if typed {
                    cur.push(Tok::Word(s));
                } else {
                    ty.push_str(&s);
                    if tag == 0x0e {
                        ty.push('-');
                    } else {
                        typed = true;
                    }
                }
            }
            0x0a => cur.push(Tok::Word("T".into())),
            0x0b => cur.push(Tok::Word("I".into())),
            0x0c => cur.push(Tok::Ptr(rd.int()?)),
            0x0f => cur.push(Tok::Word("{".into())),
            0x10 => cur.push(Tok::Word("}".into())),
            0x13 | 0x14 => {
                for _ in 0..3 {
                    let v = rd.f64()?;
                    cur.push(Tok::Word(format!("{v:?}")));
                }
            }
            0x11 => {
                let mut f = std::mem::take(&mut cur);
                let t = std::mem::take(&mut ty);
                typed = false;
                if t.starts_with("End-of-") {
                    break;
                }
                // As in version 7 text: an integer, then (but in a transform) a pointer.
                if matches!(f.get(1), Some(Tok::Word(w)) if w.parse::<i64>().is_ok()) {
                    f.remove(1);
                    if t != "transform" && matches!(f.get(1), Some(Tok::Ptr(_))) {
                        f.remove(1);
                    }
                }
                if recs.len() >= limits.max_records {
                    return Err(SatError::TooLarge(format!("more than {} records", limits.max_records)));
                }
                recs.push(Rec { ty: t, f });
            }
            other => return Err(bad(format!("unknown binary tag 0x{other:02x} at byte {}", rd.pos + 14))),
        }
    }
    if recs.is_empty() {
        return Err(bad("no records".into()));
    }
    let scale = units.first().copied().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    let subtypes = subtypes_of(&recs);
    Ok(Sat { version, scale, recs, subtypes })
}

struct Bin<'a> {
    b: &'a [u8],
    pos: usize,
    width: usize,
}

impl Bin<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], SatError> {
        let s = self
            .b
            .get(self.pos..self.pos.saturating_add(n))
            .ok_or_else(|| SatError::Malformed("the file ends early".into()))?;
        self.pos += n;
        Ok(s)
    }

    fn int(&mut self) -> Result<i64, SatError> {
        let w = self.width;
        let s = self.take(w)?;
        let short = || SatError::Malformed("a short integer".into());
        Ok(if w == 8 {
            i64::from_le_bytes(s.try_into().map_err(|_| short())?)
        } else {
            i64::from(i32::from_le_bytes(s.try_into().map_err(|_| short())?))
        })
    }

    fn f64(&mut self) -> Result<f64, SatError> {
        let s = self.take(8)?;
        Ok(f64::from_le_bytes(s.try_into().map_err(|_| SatError::Malformed("a short double".into()))?))
    }

    /// A string after its tag: u8 length (0x07, 0x0d, 0x0e), u16 (0x08) or u32 (0x12).
    fn string(&mut self, tag: u8) -> Result<String, SatError> {
        let n = match tag {
            0x08 => {
                let s = self.take(2)?;
                usize::from(u16::from_le_bytes([s.first().copied().unwrap_or(0), s.get(1).copied().unwrap_or(0)]))
            }
            0x12 => {
                let s = self.take(4)?;
                let mut a = [0u8; 4];
                a.copy_from_slice(s);
                u32::from_le_bytes(a) as usize
            }
            _ => usize::from(self.take(1)?.first().copied().unwrap_or(0)),
        };
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

/// Subtype definitions in file order (see [`Sat::subtypes`]).
fn subtypes_of(recs: &[Rec]) -> Vec<(usize, usize)> {
    let mut subtypes = Vec::new();
    for (ri, r) in recs.iter().enumerate() {
        for (fi, t) in r.f.iter().enumerate() {
            if matches!(t, Tok::Word(w) if w == "{") && !matches!(r.f.get(fi + 1), Some(Tok::Word(w)) if w == "ref") {
                subtypes.push((ri, fi));
            }
        }
    }
    subtypes
}

/// Reads a SAT text file.
fn parse_text(bytes: &[u8], limits: &Limits) -> Result<Sat, SatError> {
    let bad = |m: &str| SatError::Malformed(m.to_string());
    let mut lx = Lexer { b: bytes, pos: 0, at_strings: false };
    // The first line `version records bodies flags`, after any lines a mailer or a tool
    // put in front of it.
    let mut version = None;
    for _ in 0..50 {
        let first = words(lx.line());
        if first.len() >= 3
            && first.iter().take(3).all(|w| w.parse::<i64>().is_ok())
            && let Some(v) = first.first().and_then(|v| v.parse::<u32>().ok())
        {
            version = Some(v);
            break;
        }
        if lx.pos >= bytes.len() {
            break;
        }
    }
    let version = version.ok_or_else(|| bad("no version number"))?;
    if version < 100 {
        return Err(SatError::Unsupported(format!("ACIS SAT version {version} cannot be read")));
    }
    // Version 1 files have no product and units lines.
    let scale = if version < 200 {
        1.0
    } else {
        let _product = lx.line();
        let units = words(lx.line());
        units.first().and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0)
    };
    lx.at_strings = version >= 700;
    let mut recs = Vec::new();
    let mut tokens = 0usize;
    let mut cur: Vec<String> = Vec::new();
    while let Some(t) = lx.token() {
        if t == "End-of-ACIS-data" || t == "End-of-ASM-data" {
            break;
        }
        if t != "#" {
            tokens += 1;
            if tokens > limits.max_tokens {
                return Err(SatError::TooLarge(format!("more than {} values", limits.max_tokens)));
            }
            cur.push(t);
            continue;
        }
        let mut it = std::mem::take(&mut cur).into_iter().peekable();
        // An optional record number `-12` before the type.
        if it.peek().is_some_and(|w| w.starts_with('-') && w.len() > 1 && w[1..].bytes().all(|c| c.is_ascii_digit())) {
            it.next();
        }
        let Some(ty) = it.next() else { continue };
        let mut f: Vec<Tok> = it
            .map(|w| match w.strip_prefix('$').and_then(|n| n.parse::<i64>().ok()) {
                Some(p) => Tok::Ptr(p),
                None => Tok::Word(w),
            })
            .collect();
        // Version 7: an integer after the attribute pointer, then (except in a
        // `transform`, whose data follows directly) a pointer.
        if version >= 700 && matches!(f.get(1), Some(Tok::Word(w)) if w.parse::<i64>().is_ok()) {
            f.remove(1);
            if ty != "transform" && matches!(f.get(1), Some(Tok::Ptr(_))) {
                f.remove(1);
            }
        }
        if recs.len() >= limits.max_records {
            return Err(SatError::TooLarge(format!("more than {} records", limits.max_records)));
        }
        recs.push(Rec { ty, f });
    }
    if recs.is_empty() {
        return Err(bad("no records"));
    }
    let subtypes = subtypes_of(&recs);
    Ok(Sat { version, scale, recs, subtypes })
}
