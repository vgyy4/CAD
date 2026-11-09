//! Reading field values from the two encodings (ADR 0035): text, where numbers are
//! separated by single spaces, characters and logicals are glued to what follows and `?`
//! marks an unset value; and neutral binary, big-endian, with node indices packed into one
//! or two 16-bit words.

use crate::XtError;

/// The integer the Reference uses for "unset".
pub(crate) const UNSET_INT: i64 = -32764;
/// The double the Reference uses for "unset".
pub(crate) const UNSET_REAL: f64 = -3.14158e13;

/// The file's header: the text up to `**END_OF_HEADER`, and where the data starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    /// `SCH=...` from part 2, if present.
    pub(crate) schema_key: Option<String>,
    /// Byte offset of the data (just after the header's last line).
    pub(crate) data_start: usize,
}

const END: &[u8] = b"**END_OF_HEADER";

/// Finds the header. Files without one start directly with the data.
pub(crate) fn header(bytes: &[u8]) -> Header {
    let Some(at) = find(bytes, END) else {
        return Header { schema_key: None, data_start: 0 };
    };
    let line_end =
        bytes.get(at..).and_then(|rest| rest.iter().position(|&b| b == b'\n')).map_or(bytes.len(), |p| at + p + 1);
    let head = String::from_utf8_lossy(bytes.get(..at).unwrap_or_default());
    // Part 2 values are `key=value;` pairs; lines may wrap anywhere, so join them first.
    let joined: String = head.lines().collect::<Vec<_>>().join("");
    let schema_key = joined
        .split(';')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.trim_start_matches('*').trim_start_matches("PART2").trim() == "SCH")
        .map(|(_, v)| v.trim().to_string());
    Header { schema_key, data_start: line_end }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// One decoded value.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Int(i64),
    Real(f64),
    Char(u8),
    Ptr(u32),
    Vec3([f64; 3]),
    Pair([f64; 2]),
    Six([f64; 6]),
    Unset,
    List(Vec<Val>),
}

/// A source of field values.
pub(crate) trait Source {
    /// Whether an unset vector is stored as three unset reals (binary) rather than one
    /// (text, a single `?`).
    const UNSET_VECTOR_IS_TRIPLE: bool;
    fn int(&mut self, bytes: Width) -> Result<Option<i64>, XtError>;
    fn real(&mut self) -> Result<Option<f64>, XtError>;
    fn ch(&mut self) -> Result<u8, XtError>;
    fn logical(&mut self) -> Result<bool, XtError>;
    fn ptr(&mut self) -> Result<u32, XtError>;
    /// `n` characters of a string field.
    fn chars(&mut self, n: usize) -> Result<Vec<u8>, XtError>;
    /// A short string of an embedded schema.
    fn short_string(&mut self) -> Result<String, XtError>;
    /// Bytes left (an upper bound for element counts).
    fn remaining(&self) -> usize;
    fn offset(&self) -> usize;
}

/// Integer widths in binary files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Width {
    Byte,
    Short,
    Int,
}

/// Text data: lines are joined (trailing spaces dropped), then read as tokens.
pub(crate) struct Text {
    data: Vec<u8>,
    pos: usize,
    /// The last token was a number, so one separating space comes before a character.
    after_number: bool,
}

impl Text {
    pub(crate) fn new(raw: &[u8]) -> Text {
        let mut data = Vec::with_capacity(raw.len());
        for line in raw.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let end = line.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1);
            data.extend_from_slice(line.get(..end).unwrap_or_default());
        }
        Text { data, pos: 0, after_number: false }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn err(&self, what: &str) -> XtError {
        let near: String =
            self.data.get(self.pos..).unwrap_or_default().iter().take(24).map(|&b| char::from(b)).collect();
        XtError::Malformed(format!("expected {what} at byte {} (near {near:?})", self.pos))
    }

    /// A number token, or `None` for `?`.
    fn number(&mut self) -> Result<Option<f64>, XtError> {
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
        if self.peek() == Some(b'?') {
            self.pos += 1;
            self.after_number = true;
            return Ok(None);
        }
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = 0;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits += 1;
            } else if c != b'.' {
                break;
            }
            self.pos += 1;
        }
        if digits == 0 {
            self.pos = start;
            return Err(self.err("a number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mark = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                self.pos = mark;
            }
        }
        let s = std::str::from_utf8(self.data.get(start..self.pos).unwrap_or_default())
            .map_err(|_| self.err("a number"))?;
        let v: f64 = s.parse().map_err(|_| self.err("a number"))?;
        self.after_number = true;
        Ok(Some(v))
    }

    fn one_char(&mut self) -> Result<u8, XtError> {
        if self.after_number && self.peek() == Some(b' ') {
            self.pos += 1;
        }
        self.after_number = false;
        let c = self.peek().ok_or_else(|| self.err("a character"))?;
        self.pos += 1;
        Ok(c)
    }
}

impl Source for Text {
    const UNSET_VECTOR_IS_TRIPLE: bool = false;

    fn int(&mut self, _: Width) -> Result<Option<i64>, XtError> {
        match self.number()? {
            None => Ok(None),
            Some(v) if v.fract() == 0.0 && v.abs() < 9.0e15 => Ok(Some(v as i64)),
            Some(_) => Err(self.err("an integer")),
        }
    }

    fn real(&mut self) -> Result<Option<f64>, XtError> {
        self.number()
    }

    fn ch(&mut self) -> Result<u8, XtError> {
        self.one_char()
    }

    fn logical(&mut self) -> Result<bool, XtError> {
        match self.one_char()? {
            b'T' | b'1' => Ok(true),
            b'F' | b'0' => Ok(false),
            _ => Err(self.err("a logical (T or F)")),
        }
    }

    fn ptr(&mut self) -> Result<u32, XtError> {
        match self.number()? {
            Some(v) if v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX) => Ok(v as u32),
            None => Ok(0),
            Some(_) => Err(self.err("a node index")),
        }
    }

    fn chars(&mut self, n: usize) -> Result<Vec<u8>, XtError> {
        if self.after_number && self.peek() == Some(b' ') {
            self.pos += 1;
        }
        self.after_number = false;
        let mut out = Vec::with_capacity(n.min(4096));
        while out.len() < n {
            let c = self.peek().ok_or_else(|| self.err("string characters"))?;
            self.pos += 1;
            if c == b'\\' {
                // Escapes (V12.1 on): \n, \r, \0 and \\; \9 stands for nine spaces (V14 on).
                let e = self.peek().ok_or_else(|| self.err("an escape"))?;
                self.pos += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b'0' => out.push(0),
                    b'9' => out.extend_from_slice(b"         "),
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        out.truncate(n);
        Ok(out)
    }

    fn short_string(&mut self) -> Result<String, XtError> {
        let n = self.int(Width::Byte)?.unwrap_or(0);
        let n = usize::try_from(n).map_err(|_| self.err("a string length"))?;
        if n > self.remaining() {
            return Err(self.err("a shorter string"));
        }
        let b = self.chars(n)?;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn offset(&self) -> usize {
        self.pos
    }
}

/// Neutral binary data: big-endian.
pub(crate) struct Binary<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Binary<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Binary<'a> {
        Binary { data, pos: 0 }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], XtError> {
        let s = self
            .data
            .get(self.pos..self.pos + N)
            .ok_or_else(|| XtError::Malformed(format!("the binary data ends early (at byte {})", self.pos)))?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        self.pos += N;
        Ok(a)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, XtError> {
        Ok(u16::from_be_bytes(self.take::<2>()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, XtError> {
        Ok(i32::from_be_bytes(self.take::<4>()?))
    }
}

impl Source for Binary<'_> {
    const UNSET_VECTOR_IS_TRIPLE: bool = true;

    fn int(&mut self, w: Width) -> Result<Option<i64>, XtError> {
        let v = match w {
            Width::Byte => i64::from(self.take::<1>()?.first().copied().unwrap_or(0)),
            Width::Short => i64::from(i16::from_be_bytes(self.take::<2>()?)),
            Width::Int => i64::from(i32::from_be_bytes(self.take::<4>()?)),
        };
        Ok(if v == UNSET_INT { None } else { Some(v) })
    }

    fn real(&mut self) -> Result<Option<f64>, XtError> {
        let v = f64::from_be_bytes(self.take::<8>()?);
        Ok(if (v - UNSET_REAL).abs() < 1.0 { None } else { Some(v) })
    }

    fn ch(&mut self) -> Result<u8, XtError> {
        Ok(self.take::<1>()?.first().copied().unwrap_or(0))
    }

    fn logical(&mut self) -> Result<bool, XtError> {
        Ok(self.ch()? != 0)
    }

    fn ptr(&mut self) -> Result<u32, XtError> {
        // Indices below 32767 are one word holding index + 1; larger ones are a negative
        // word -(index mod 32767 + 1) followed by the quotient.
        let r = i32::from(i16::from_be_bytes(self.take::<2>()?));
        if r >= 0 {
            return Ok(u32::try_from(r - 1).unwrap_or(0));
        }
        let q = i64::from(u16::from_be_bytes(self.take::<2>()?));
        let v = q * 32767 + i64::from(-r) - 1;
        u32::try_from(v).map_err(|_| XtError::Malformed("a node index is out of range".into()))
    }

    fn chars(&mut self, n: usize) -> Result<Vec<u8>, XtError> {
        let s = self
            .data
            .get(self.pos..self.pos + n)
            .ok_or_else(|| XtError::Malformed("the binary data ends inside a string".into()))?;
        self.pos += n;
        Ok(s.to_vec())
    }

    fn short_string(&mut self) -> Result<String, XtError> {
        let n = usize::from(self.ch()?);
        let b = self.chars(n)?;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn offset(&self) -> usize {
        self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_gives_the_schema_and_where_data_starts() {
        let f = b"**ABC\n**PART2;SCH=SCH_120\n0000_12006;USFLD_SIZE=0;\n**END_OF_HEADER*****\nT51 : x";
        let h = header(f);
        assert_eq!(h.schema_key.as_deref(), Some("SCH_1200000_12006"));
        assert_eq!(f.get(h.data_start..), Some(&b"T51 : x"[..]));
    }

    #[test]
    fn text_numbers_chars_and_unset_values() {
        // Wrapped lines join; a sense glued to a number; `?` is unset; a string after its
        // count's space.
        let mut t = Text::new(b"12 -.5 1e-8 ?+3\n4 0 +1.5e3 TF 2 ab");
        assert_eq!(t.int(Width::Int).ok(), Some(Some(12)));
        assert_eq!(t.real().ok(), Some(Some(-0.5)));
        assert_eq!(t.real().ok(), Some(Some(1e-8)));
        assert_eq!(t.real().ok(), Some(None));
        assert_eq!(t.ch().ok(), Some(b'+'));
        assert_eq!(t.int(Width::Int).ok(), Some(Some(34)));
        assert_eq!(t.ptr().ok(), Some(0));
        assert_eq!(t.ch().ok(), Some(b'+'));
        assert_eq!(t.real().ok(), Some(Some(1500.0)));
        assert_eq!(t.logical().ok(), Some(true));
        assert_eq!(t.logical().ok(), Some(false));
        assert_eq!(t.int(Width::Int).ok(), Some(Some(2)));
        assert_eq!(t.chars(2).ok(), Some(b"ab".to_vec()));
        assert!(t.int(Width::Int).is_err());
    }

    #[test]
    fn binary_pointers_use_one_or_two_words() {
        // 5 -> 6; 40000 -> -(40000 % 32767 + 1), 40000 / 32767.
        let big = 40000u32;
        let r = -i32::try_from(big % 32767 + 1).unwrap_or(0);
        let mut bytes = vec![0, 6];
        bytes.extend_from_slice(&i16::try_from(r).unwrap_or(0).to_be_bytes());
        bytes.extend_from_slice(&u16::try_from(big / 32767).unwrap_or(0).to_be_bytes());
        bytes.extend_from_slice(&1.25f64.to_be_bytes());
        bytes.extend_from_slice(&UNSET_REAL.to_be_bytes());
        let mut b = Binary::new(&bytes);
        assert_eq!(b.ptr().ok(), Some(5));
        assert_eq!(b.ptr().ok(), Some(big));
        assert_eq!(b.real().ok(), Some(Some(1.25)));
        assert_eq!(b.real().ok(), Some(None));
        assert!(b.ch().is_err());
    }
}
