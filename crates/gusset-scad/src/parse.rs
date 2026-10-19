//! Lexer and recursive-descent parser for the OpenSCAD language, written from the
//! OpenSCAD User Manual (ADR 0033; never from OpenSCAD's GPL grammar).

use crate::ScadError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Sym(&'static str),
    Eof,
}

/// A token and the line it starts on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) line: u32,
}

const SYMS: [&str; 28] = [
    "<=", ">=", "==", "!=", "&&", "||", "(", ")", "[", "]", "{", "}", ",", ";", "=", "+", "-", "*", "/", "%", "^", "<",
    ">", "!", "?", ":", ".", "#",
];

/// Most tokens in one file.
pub(crate) const MAX_TOKENS: usize = 2_000_000;

pub(crate) fn lex(src: &str) -> Result<Vec<Token>, ScadError> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut line) = (0usize, 1u32);
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    while i < b.len() {
        let c = at(i);
        if c == b'\n' {
            line += 1;
            i += 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && at(i + 1) == b'/' {
            while i < b.len() && at(i) != b'\n' {
                i += 1;
            }
        } else if c == b'/' && at(i + 1) == b'*' {
            i += 2;
            while i < b.len() && !(at(i) == b'*' && at(i + 1) == b'/') {
                if at(i) == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            let s = i;
            while at(i).is_ascii_digit() || at(i) == b'.' {
                i += 1;
            }
            if at(i) == b'e' || at(i) == b'E' {
                let save = i;
                i += 1;
                if at(i) == b'+' || at(i) == b'-' {
                    i += 1;
                }
                if at(i).is_ascii_digit() {
                    while at(i).is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            let t = src.get(s..i).unwrap_or("0");
            let v: f64 = t.parse().map_err(|_| ScadError::at(line, format!("bad number '{t}'")))?;
            out.push(Token { tok: Tok::Num(v), line });
        } else if c == b'"' {
            i += 1;
            let mut s = String::new();
            loop {
                match at(i) {
                    0 if i >= b.len() => return Err(ScadError::at(line, "unterminated string")),
                    b'"' => {
                        i += 1;
                        break;
                    }
                    b'\\' => {
                        let e = at(i + 1);
                        s.push(match e {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            other => char::from(other),
                        });
                        i += 2;
                    }
                    _ => {
                        // Copy one UTF-8 character.
                        let ch = src.get(i..).and_then(|r| r.chars().next()).unwrap_or('?');
                        if ch == '\n' {
                            line += 1;
                        }
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
            }
            out.push(Token { tok: Tok::Str(s), line });
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let s = i;
            while at(i).is_ascii_alphanumeric() || at(i) == b'_' || at(i) == b'$' {
                i += 1;
            }
            out.push(Token { tok: Tok::Ident(src.get(s..i).unwrap_or("").to_string()), line });
        } else {
            let two = src.get(i..i + 2).unwrap_or("");
            let sym = SYMS.iter().find(|s| s.len() == 2 && **s == two).or_else(|| {
                let one = src.get(i..i + 1).unwrap_or("");
                SYMS.iter().find(|s| s.len() == 1 && **s == one)
            });
            match sym {
                Some(s) => {
                    i += s.len();
                    out.push(Token { tok: Tok::Sym(s), line });
                }
                None => {
                    let ch = src.get(i..).and_then(|r| r.chars().next()).unwrap_or('?');
                    return Err(ScadError::at(line, format!("unexpected character '{ch}'")));
                }
            }
        }
        if out.len() > MAX_TOKENS {
            return Err(ScadError::at(line, "the file is too long"));
        }
    }
    out.push(Token { tok: Tok::Eof, line });
    Ok(out)
}

// ------------------------------------------------------------------------------- AST

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Undef,
    Var(String),
    Vec(Vec<VecItem>),
    Range(Box<Expr>, Option<Box<Expr>>, Box<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Arg>),
    Index(Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),
    Let(Vec<(String, Expr)>, Box<Expr>),
}

/// An element of a vector literal: a value, or a list comprehension part.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VecItem {
    Expr(Expr),
    Each(Expr),
    For(Vec<(String, Expr)>, Box<VecItem>),
    If(Expr, Box<VecItem>, Option<Box<VecItem>>),
    Let(Vec<(String, Expr)>, Box<VecItem>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arg {
    pub(crate) name: Option<String>,
    pub(crate) value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Param {
    pub(crate) name: String,
    pub(crate) default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stmt {
    Assign(String, Expr, u32),
    /// A module call with its children; `line` for messages. Modifiers `*` (disable) drop
    /// the statement while parsing; `!`, `#` and `%` are ignored.
    Inst {
        name: String,
        args: Vec<Arg>,
        children: Vec<Stmt>,
        line: u32,
    },
    Block(Vec<Stmt>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    For(Vec<(String, Expr)>, Vec<Stmt>),
    Let(Vec<(String, Expr)>, Vec<Stmt>),
    Module(String, Vec<Param>, Vec<Stmt>),
    Function(String, Vec<Param>, Expr),
    /// `include <...>` / `use <...>`: not followed.
    Uses(String, u32),
}

/// Deepest nesting of expressions and statements.
const MAX_DEPTH: u32 = 256;

pub(crate) struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: u32,
}

impl Parser {
    pub(crate) fn new(toks: Vec<Token>) -> Parser {
        Parser { toks, pos: 0, depth: 0 }
    }

    fn peek(&self) -> &Tok {
        self.toks.get(self.pos).map_or(&Tok::Eof, |t| &t.tok)
    }

    fn peek_at(&self, k: usize) -> &Tok {
        self.toks.get(self.pos + k).map_or(&Tok::Eof, |t| &t.tok)
    }

    fn line(&self) -> u32 {
        self.toks.get(self.pos).or(self.toks.last()).map_or(0, |t| t.line)
    }

    fn next(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.is(s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), ScadError> {
        if self.eat(s) { Ok(()) } else { Err(self.err(format!("expected '{s}'"))) }
    }

    fn err(&self, m: impl Into<String>) -> ScadError {
        let found = match self.peek() {
            Tok::Num(n) => format!("{n}"),
            Tok::Str(s) => format!("\"{}\"", s.chars().take(20).collect::<String>()),
            Tok::Ident(s) => s.clone(),
            Tok::Sym(s) => (*s).to_string(),
            Tok::Eof => "the end of the file".into(),
        };
        ScadError::at(self.line(), format!("{}, found {found}", m.into()))
    }

    fn ident(&mut self) -> Result<String, ScadError> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            _ => {
                self.pos = self.pos.saturating_sub(1);
                Err(self.err("expected a name"))
            }
        }
    }

    fn deeper(&mut self) -> Result<(), ScadError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(self.err("nested too deeply")) } else { Ok(()) }
    }

    pub(crate) fn program(&mut self) -> Result<Vec<Stmt>, ScadError> {
        let mut out = Vec::new();
        while !matches!(self.peek(), Tok::Eof) {
            if let Some(s) = self.statement()? {
                out.push(s);
            }
        }
        Ok(out)
    }

    /// One statement; `None` for an empty one or one disabled with `*`.
    fn statement(&mut self) -> Result<Option<Stmt>, ScadError> {
        self.deeper()?;
        let r = self.statement_inner();
        self.depth -= 1;
        r
    }

    fn statement_inner(&mut self) -> Result<Option<Stmt>, ScadError> {
        let line = self.line();
        if self.eat(";") {
            return Ok(None);
        }
        if self.eat("{") {
            let mut body = Vec::new();
            while !self.eat("}") {
                if matches!(self.peek(), Tok::Eof) {
                    return Err(self.err("expected '}'"));
                }
                if let Some(s) = self.statement()? {
                    body.push(s);
                }
            }
            return Ok(Some(Stmt::Block(body)));
        }
        // Modifiers.
        if self.eat("*") {
            self.statement()?;
            return Ok(None);
        }
        if self.eat("!") || self.eat("#") || self.eat("%") {
            return self.statement();
        }
        let name = self.ident()?;
        match name.as_str() {
            "include" | "use" if self.is("<") => {
                // `<path>`: skip to the closing '>' on this line.
                let mut path = String::new();
                self.next();
                while !self.eat(">") {
                    match self.next() {
                        Tok::Eof => return Err(self.err("expected '>'")),
                        Tok::Ident(s) => path.push_str(&s),
                        Tok::Sym(s) => path.push_str(s),
                        Tok::Num(n) => path.push_str(&n.to_string()),
                        Tok::Str(s) => path.push_str(&s),
                    }
                }
                return Ok(Some(Stmt::Uses(path, line)));
            }
            "module" => {
                let n = self.ident()?;
                let params = self.params()?;
                let body = self.child()?;
                return Ok(Some(Stmt::Module(n, params, body)));
            }
            "function" => {
                let n = self.ident()?;
                let params = self.params()?;
                self.expect("=")?;
                let e = self.expr()?;
                self.expect(";")?;
                return Ok(Some(Stmt::Function(n, params, e)));
            }
            "if" => {
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                let then = self.child()?;
                let els = if matches!(self.peek(), Tok::Ident(s) if s == "else") {
                    self.next();
                    self.child()?
                } else {
                    Vec::new()
                };
                return Ok(Some(Stmt::If(c, then, els)));
            }
            "for" | "intersection_for" => {
                let binds = self.bindings()?;
                let body = self.child()?;
                if name == "for" {
                    return Ok(Some(Stmt::For(binds, body)));
                }
                return Ok(Some(Stmt::Inst {
                    name: "intersection".into(),
                    args: vec![],
                    children: vec![Stmt::For(binds, body)],
                    line,
                }));
            }
            "let" if self.is("(") => {
                let binds = self.bindings()?;
                let body = self.child()?;
                return Ok(Some(Stmt::Let(binds, body)));
            }
            _ => {}
        }
        if self.eat("=") {
            let e = self.expr()?;
            self.expect(";")?;
            return Ok(Some(Stmt::Assign(name, e, line)));
        }
        let args = self.args()?;
        let children = self.child()?;
        Ok(Some(Stmt::Inst { name, args, children, line }))
    }

    /// The children of a module call or control statement: `;`, a block, or one statement.
    fn child(&mut self) -> Result<Vec<Stmt>, ScadError> {
        if self.eat(";") {
            return Ok(Vec::new());
        }
        Ok(match self.statement()? {
            Some(Stmt::Block(b)) => b,
            Some(s) => vec![s],
            None => Vec::new(),
        })
    }

    fn params(&mut self) -> Result<Vec<Param>, ScadError> {
        self.expect("(")?;
        let mut out = Vec::new();
        while !self.eat(")") {
            let name = self.ident()?;
            let default = if self.eat("=") { Some(self.expr()?) } else { None };
            out.push(Param { name, default });
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(out)
    }

    fn args(&mut self) -> Result<Vec<Arg>, ScadError> {
        self.expect("(")?;
        let mut out = Vec::new();
        while !self.eat(")") {
            let named = matches!((self.peek(), self.peek_at(1)), (Tok::Ident(_), Tok::Sym("=")));
            let name = if named {
                let n = self.ident()?;
                self.next();
                Some(n)
            } else {
                None
            };
            out.push(Arg { name, value: self.expr()? });
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(out)
    }

    /// `(a = e, b = e)` for `for` / `let`.
    fn bindings(&mut self) -> Result<Vec<(String, Expr)>, ScadError> {
        self.expect("(")?;
        let mut out = Vec::new();
        while !self.eat(")") {
            let n = self.ident()?;
            self.expect("=")?;
            out.push((n, self.expr()?));
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(out)
    }

    pub(crate) fn expr(&mut self) -> Result<Expr, ScadError> {
        self.deeper()?;
        let r = self.ternary();
        self.depth -= 1;
        r
    }

    fn ternary(&mut self) -> Result<Expr, ScadError> {
        if let Tok::Ident(k) = self.peek()
            && k == "let"
            && matches!(self.peek_at(1), Tok::Sym("("))
        {
            self.next();
            let b = self.bindings()?;
            let e = self.expr()?;
            return Ok(Expr::Let(b, Box::new(e)));
        }
        let c = self.binary(0)?;
        if self.eat("?") {
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.expr()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, ScadError> {
        const LEVELS: [&[&str]; 6] =
            [&["||"], &["&&"], &["==", "!="], &["<", "<=", ">", ">="], &["+", "-"], &["*", "/", "%"]];
        let Some(ops) = LEVELS.get(level) else { return self.power() };
        let mut a = self.binary(level + 1)?;
        while let Tok::Sym(s) = self.peek().clone() {
            let Some(op) = ops.iter().find(|o| **o == s) else { break };
            self.next();
            let b = self.binary(level + 1)?;
            a = Expr::Binary(op, Box::new(a), Box::new(b));
        }
        Ok(a)
    }

    fn power(&mut self) -> Result<Expr, ScadError> {
        let a = self.unary()?;
        if self.eat("^") {
            self.deeper()?;
            let b = self.power();
            self.depth -= 1;
            return Ok(Expr::Binary("^", Box::new(a), Box::new(b?)));
        }
        Ok(a)
    }

    fn unary(&mut self) -> Result<Expr, ScadError> {
        for op in ["-", "!", "+"] {
            if self.eat(op) {
                self.deeper()?;
                let e = self.unary();
                self.depth -= 1;
                return Ok(Expr::Unary(
                    if op == "-" {
                        "-"
                    } else if op == "!" {
                        "!"
                    } else {
                        "+"
                    },
                    Box::new(e?),
                ));
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, ScadError> {
        let mut e = self.primary()?;
        loop {
            if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.eat(".") {
                e = Expr::Member(Box::new(e), self.ident()?);
            } else if self.is("(") {
                let Expr::Var(name) = e else { return Err(self.err("only named functions can be called")) };
                e = Expr::Call(name, self.args()?);
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, ScadError> {
        match self.next() {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Ident(s) => Ok(match s.as_str() {
                "true" => Expr::Bool(true),
                "false" => Expr::Bool(false),
                "undef" => Expr::Undef,
                "function" => return Err(self.err("function literals are not supported")),
                _ => Expr::Var(s),
            }),
            Tok::Sym("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Sym("[") => self.vector(),
            _ => {
                self.pos = self.pos.saturating_sub(1);
                Err(self.err("expected a value"))
            }
        }
    }

    /// After `[`: a vector, a range or a list comprehension.
    fn vector(&mut self) -> Result<Expr, ScadError> {
        if self.eat("]") {
            return Ok(Expr::Vec(Vec::new()));
        }
        let first = self.vec_item()?;
        if let VecItem::Expr(a) = &first
            && self.eat(":")
        {
            let b = self.expr()?;
            if self.eat(":") {
                let c = self.expr()?;
                self.expect("]")?;
                return Ok(Expr::Range(Box::new(a.clone()), Some(Box::new(b)), Box::new(c)));
            }
            self.expect("]")?;
            return Ok(Expr::Range(Box::new(a.clone()), None, Box::new(b)));
        }
        let mut items = vec![first];
        while self.eat(",") {
            if self.is("]") {
                break;
            }
            items.push(self.vec_item()?);
        }
        self.expect("]")?;
        Ok(Expr::Vec(items))
    }

    fn vec_item(&mut self) -> Result<VecItem, ScadError> {
        self.deeper()?;
        let r = self.vec_item_inner();
        self.depth -= 1;
        r
    }

    fn vec_item_inner(&mut self) -> Result<VecItem, ScadError> {
        if let Tok::Ident(k) = self.peek().clone() {
            match k.as_str() {
                "for" if matches!(self.peek_at(1), Tok::Sym("(")) => {
                    self.next();
                    let b = self.bindings()?;
                    let item = self.vec_item()?;
                    return Ok(VecItem::For(b, Box::new(item)));
                }
                "if" if matches!(self.peek_at(1), Tok::Sym("(")) => {
                    self.next();
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let a = self.vec_item()?;
                    let els = if matches!(self.peek(), Tok::Ident(s) if s == "else") {
                        self.next();
                        Some(Box::new(self.vec_item()?))
                    } else {
                        None
                    };
                    return Ok(VecItem::If(c, Box::new(a), els));
                }
                "each" => {
                    self.next();
                    return Ok(VecItem::Each(self.expr()?));
                }
                "let" if matches!(self.peek_at(1), Tok::Sym("(")) => {
                    // `let` inside a comprehension applies to the item that follows.
                    let save = self.pos;
                    self.next();
                    let b = self.bindings()?;
                    if matches!(self.peek(), Tok::Ident(s) if s == "for" || s == "if" || s == "each") {
                        let item = self.vec_item()?;
                        return Ok(VecItem::Let(b, Box::new(item)));
                    }
                    self.pos = save;
                }
                _ => {}
            }
        }
        if self.eat("(") {
            // A parenthesised comprehension part.
            let save = self.pos;
            if matches!(self.peek(), Tok::Ident(s) if s == "for" || s == "if" || s == "each") {
                let item = self.vec_item()?;
                self.expect(")")?;
                return Ok(item);
            }
            self.pos = save - 1;
        }
        Ok(VecItem::Expr(self.expr()?))
    }
}
