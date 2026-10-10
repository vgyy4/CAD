//! A small character cursor used by the text and reference parsers.

use std::fmt;

/// A parse error with a 1-based line/column position.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hint: Option<String>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.column, self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

pub(crate) const MAX_IDENT: usize = 64;
pub(crate) const MAX_STRING: usize = 4096;
pub(crate) const MAX_REF_DEPTH: usize = 16;

#[derive(Debug, Clone)]
pub(crate) struct Cursor<'a> {
    pub(crate) s: &'a str,
    pub(crate) pos: usize,
    /// Line number of `s` within the whole document (1-based) and byte offset of the
    /// line start, for error positions.
    pub(crate) line: usize,
    pub(crate) depth: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(s: &'a str, line: usize) -> Self {
        Cursor { s, pos: 0, line, depth: 0 }
    }

    pub(crate) fn rest(&self) -> &'a str {
        self.s.get(self.pos..).unwrap_or("")
    }

    pub(crate) fn err(&self, msg: impl Into<String>) -> ParseError {
        let column = self.s.get(..self.pos).map_or(1, |p| p.chars().count() + 1);
        ParseError { line: self.line, column, message: msg.into(), hint: None }
    }

    pub(crate) fn err_hint(&self, msg: impl Into<String>, hint: impl Into<String>) -> ParseError {
        let mut e = self.err(msg);
        e.hint = Some(hint.into());
        e
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(crate) fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    pub(crate) fn at_end(&mut self) -> bool {
        self.ws();
        self.rest().is_empty()
    }

    /// Consumes `c` (after whitespace) if present.
    pub(crate) fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, c: char) -> Result<(), ParseError> {
        if self.eat(c) {
            Ok(())
        } else {
            let found = self.peek().map_or("end of line".to_string(), |f| format!("'{f}'"));
            Err(self.err(format!("expected '{c}', found {found}")))
        }
    }

    /// Consumes a keyword (identifier equal to `kw`) if present.
    pub(crate) fn eat_kw(&mut self, kw: &str) -> bool {
        let save = self.pos;
        if let Ok(id) = self.ident()
            && id == kw
        {
            return true;
        }
        self.pos = save;
        false
    }

    /// An identifier: ASCII letter or `_`, then letters, digits, `_`.
    pub(crate) fn ident(&mut self) -> Result<&'a str, ParseError> {
        self.ws();
        let rest = self.rest();
        let mut end = 0;
        for (i, c) in rest.char_indices() {
            let ok = if i == 0 { c.is_ascii_alphabetic() || c == '_' } else { c.is_ascii_alphanumeric() || c == '_' };
            if !ok {
                break;
            }
            end = i + 1;
        }
        if end == 0 {
            let found = self.peek().map_or("end of line".to_string(), |f| format!("'{f}'"));
            return Err(self.err(format!("expected a name, found {found}")));
        }
        if end > MAX_IDENT {
            return Err(self.err(format!("name is longer than {MAX_IDENT} characters")));
        }
        let id = rest.get(..end).unwrap_or("");
        self.pos += end;
        Ok(id)
    }

    /// An unsigned integer.
    pub(crate) fn uint(&mut self) -> Result<u32, ParseError> {
        self.ws();
        let rest = self.rest();
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if end == 0 {
            return Err(self.err("expected a whole number"));
        }
        let v = rest.get(..end).unwrap_or("").parse::<u32>().map_err(|_| self.err("number is too large"))?;
        self.pos += end;
        Ok(v)
    }

    /// A finite decimal number, optionally signed (`-12.5`, `1e-3`).
    pub(crate) fn number(&mut self) -> Result<f64, ParseError> {
        self.ws();
        let rest = self.rest();
        let mut end = 0;
        let b = rest.as_bytes();
        let mut i = 0;
        if matches!(b.first(), Some(b'-' | b'+')) {
            i = 1;
        }
        let mut seen_digit = false;
        while let Some(&c) = b.get(i) {
            match c {
                b'0'..=b'9' => {
                    seen_digit = true;
                    i += 1;
                    end = i;
                }
                b'.' => {
                    i += 1;
                    end = i;
                }
                b'e' | b'E' if seen_digit => {
                    let mut j = i + 1;
                    if matches!(b.get(j), Some(b'-' | b'+')) {
                        j += 1;
                    }
                    if b.get(j).is_some_and(u8::is_ascii_digit) {
                        i = j;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if !seen_digit {
            return Err(self.err("expected a number"));
        }
        let text = rest.get(..end).unwrap_or("");
        let v: f64 = text.parse().map_err(|_| self.err(format!("'{text}' is not a valid number")))?;
        if !v.is_finite() {
            return Err(self.err(format!("'{text}' is not a finite number")));
        }
        self.pos += end;
        Ok(v)
    }

    /// A double-quoted string with `\"`, `\\`, `\n`, `\t` and `\u{...}` escapes.
    /// Control characters and invisible formatting characters are rejected.
    pub(crate) fn string(&mut self) -> Result<String, ParseError> {
        self.ws();
        if !self.eat('"') {
            return Err(self.err("expected a quoted string"));
        }
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("string is not closed").hint_closing());
            };
            self.pos += c.len_utf8();
            match c {
                '"' => break,
                '\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("string is not closed"));
                    };
                    self.pos += e.len_utf8();
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'u' => {
                            self.expect('{')?;
                            let start = self.pos;
                            let close =
                                self.rest().find('}').ok_or_else(|| self.err("unterminated \\u{...} escape"))?;
                            let hex = self.s.get(start..start + close).unwrap_or("");
                            self.pos = start + close + 1;
                            let ch = u32::from_str_radix(hex, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.err(format!("invalid character escape \\u{{{hex}}}")))?;
                            out.push(ch);
                        }
                        other => return Err(self.err(format!("unknown escape '\\{other}'"))),
                    }
                }
                c if c.is_control() => return Err(self.err("control characters are not allowed in strings")),
                c => out.push(c),
            }
            if out.len() > MAX_STRING {
                return Err(self.err(format!("string is longer than {MAX_STRING} bytes")));
            }
        }
        Ok(out)
    }

    /// Captures an expression up to the next `,` / `)` / `]` at nesting depth 0 (or end).
    pub(crate) fn expr_text(&mut self) -> Result<(&'a str, usize), ParseError> {
        self.ws();
        let rest = self.rest();
        let mut depth = 0i32;
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' if depth == 0 => {
                    end = i;
                    break;
                }
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    end = i;
                    break;
                }
                '#' if depth == 0 => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        let text = rest.get(..end).unwrap_or("").trim_end();
        if text.is_empty() {
            return Err(self.err("expected a value"));
        }
        let start = self.pos;
        self.pos += end;
        Ok((text, start))
    }

    /// Parses an expression captured by [`Cursor::expr_text`].
    pub(crate) fn expr(&mut self) -> Result<gusset_units::expr::Expr, ParseError> {
        let (text, start) = self.expr_text()?;
        gusset_units::expr::parse(text).map_err(|e| {
            let col_pos = start + e.span.start;
            let column = self.s.get(..col_pos).map_or(1, |p| p.chars().count() + 1);
            ParseError { line: self.line, column, message: e.message, hint: e.hint }
        })
    }

    pub(crate) fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_REF_DEPTH {
            return Err(self.err("reference is nested too deeply"));
        }
        Ok(())
    }

    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

trait HintClosing {
    fn hint_closing(self) -> Self;
}

impl HintClosing for ParseError {
    fn hint_closing(mut self) -> Self {
        self.hint = Some("add a closing '\"'".into());
        self
    }
}

/// Writes `s` as a quoted string literal that [`Cursor::string`] parses back exactly.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
