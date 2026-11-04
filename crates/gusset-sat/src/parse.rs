//! Version 1 files have only the first header line, edges without parameters and senses
//! written `0`/`1`. Versions 4 to 6 share one layout (read from version 5 and 6 samples). The record stream: three header lines (`version records bodies flags`, the product
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
}

impl Sat {
    pub fn rec(&self, i: i64) -> Option<&Rec> {
        usize::try_from(i).ok().and_then(|i| self.recs.get(i))
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

fn words(line: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(line).split_whitespace().map(str::to_string).collect()
}

/// Reads a SAT text file.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Sat, SatError> {
    let bad = |m: &str| SatError::Malformed(m.to_string());
    let mut lx = Lexer { b: bytes, pos: 0, at_strings: false };
    let first = words(lx.line());
    let version: u32 = first.first().and_then(|v| v.parse().ok()).ok_or_else(|| bad("no version number"))?;
    if (200..400).contains(&version) || version < 100 {
        return Err(SatError::Unsupported(format!(
            "ACIS SAT version {version} cannot be read (versions 1, and 4 or later are); save it from a newer ACIS"
        )));
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
    Ok(Sat { version, scale, recs })
}
