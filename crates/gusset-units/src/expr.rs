//! Unit-aware arithmetic expressions: `width/2 + 0.5 in`, `atan2(h, w)`, `30 deg`.
//!
//! The language is deliberately tiny and pure: no side effects, no I/O, no clock, no
//! loops. Evaluation is deterministic on every CPU because transcendental functions
//! come from the pure-Rust `libm` crate rather than the platform C library.

use crate::quantity::{Dim, Quantity, unit};
use std::fmt;

/// Hard limits that keep hostile input (from project files or agents) cheap.
pub const MAX_EXPR_LEN: usize = 4096;
pub const MAX_DEPTH: usize = 64;

/// Byte range in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    fn to(self, o: Span) -> Span {
        Span { start: self.start.min(o.start), end: self.end.max(o.end) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl BinOp {
    fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
        }
    }
    fn precedence(self) -> u8 {
        match self {
            BinOp::Add | BinOp::Sub => 1,
            BinOp::Mul | BinOp::Div => 2,
            BinOp::Pow => 3,
        }
    }
}

/// Expression syntax tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal; `unit` keeps the spelling the user typed so it can be printed back.
    Number {
        value: f64,
        unit: Option<String>,
        span: Span,
    },
    Var {
        name: String,
        span: Span,
    },
    Neg {
        inner: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Call {
        func: String,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Expr {
    /// Whether two trees are equal apart from their source spans (number values are
    /// compared bit for bit).
    pub fn same_tree(&self, other: &Expr) -> bool {
        match (self, other) {
            (Expr::Number { value: a, unit: ua, .. }, Expr::Number { value: b, unit: ub, .. }) => {
                a.to_bits() == b.to_bits() && ua == ub
            }
            (Expr::Var { name: a, .. }, Expr::Var { name: b, .. }) => a == b,
            (Expr::Neg { inner: a, .. }, Expr::Neg { inner: b, .. }) => a.same_tree(b),
            (Expr::Binary { op: oa, lhs: la, rhs: ra, .. }, Expr::Binary { op: ob, lhs: lb, rhs: rb, .. }) => {
                oa == ob && la.same_tree(lb) && ra.same_tree(rb)
            }
            (Expr::Call { func: fa, args: aa, .. }, Expr::Call { func: fb, args: ab, .. }) => {
                fa == fb && aa.len() == ab.len() && aa.iter().zip(ab).all(|(a, b)| a.same_tree(b))
            }
            _ => false,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Expr::Number { span, .. }
            | Expr::Var { span, .. }
            | Expr::Neg { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Call { span, .. } => *span,
        }
    }

    /// Names of all variables the expression refers to, in first-use order, deduplicated.
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_vars(&mut out);
        out
    }

    fn collect_vars(&self, out: &mut Vec<String>) {
        match self {
            Expr::Number { .. } => {}
            Expr::Var { name, .. } => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Neg { inner, .. } => inner.collect_vars(out),
            Expr::Binary { lhs, rhs, .. } => {
                lhs.collect_vars(out);
                rhs.collect_vars(out);
            }
            Expr::Call { args, .. } => args.iter().for_each(|a| a.collect_vars(out)),
        }
    }

    /// Returns a copy with every reference to `from` renamed to `to`.
    pub fn rename_var(&self, from: &str, to: &str) -> Expr {
        match self {
            Expr::Var { name, span } if name == from => Expr::Var { name: to.to_string(), span: *span },
            Expr::Number { .. } | Expr::Var { .. } => self.clone(),
            Expr::Neg { inner, span } => Expr::Neg { inner: Box::new(inner.rename_var(from, to)), span: *span },
            Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
                op: *op,
                lhs: Box::new(lhs.rename_var(from, to)),
                rhs: Box::new(rhs.rename_var(from, to)),
                span: *span,
            },
            Expr::Call { func, args, span } => Expr::Call {
                func: func.clone(),
                args: args.iter().map(|a| a.rename_var(from, to)).collect(),
                span: *span,
            },
        }
    }
}

/// Canonical, minimal-parentheses printing. `parse(e.to_string())` yields an equal tree
/// (modulo spans).
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn go(e: &Expr, parent_prec: u8, right_side: bool, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match e {
                Expr::Number { value, unit, .. } => {
                    write!(f, "{}", crate::quantity::format_exact(*value))?;
                    if let Some(u) = unit {
                        write!(f, " {u}")?;
                    }
                    Ok(())
                }
                Expr::Var { name, .. } => f.write_str(name),
                Expr::Neg { inner, .. } => {
                    // A leading `-` takes a whole power (`-x ^ 2` is `-(x ^ 2)`), so a
                    // negation needs parentheses only as the base of `^`.
                    let needs = parent_prec > BinOp::Pow.precedence();
                    if needs {
                        f.write_str("(")?;
                    }
                    f.write_str("-")?;
                    // The operand is parsed like the right side of `^`: powers need no
                    // parentheses, anything looser does.
                    go(inner, BinOp::Pow.precedence(), true, f)?;
                    if needs {
                        f.write_str(")")?;
                    }
                    Ok(())
                }
                Expr::Binary { op, lhs, rhs, .. } => {
                    let p = op.precedence();
                    // `^` is right-associative; the others are left-associative.
                    let needs = p < parent_prec || (p == parent_prec && (right_side != (*op == BinOp::Pow)));
                    if needs {
                        f.write_str("(")?;
                    }
                    go(lhs, if *op == BinOp::Pow { p + 1 } else { p }, false, f)?;
                    write!(f, " {} ", op.symbol())?;
                    go(rhs, p, true, f)?;
                    if needs {
                        f.write_str(")")?;
                    }
                    Ok(())
                }
                Expr::Call { func, args, .. } => {
                    write!(f, "{func}(")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            f.write_str(", ")?;
                        }
                        go(a, 0, false, f)?;
                    }
                    f.write_str(")")
                }
            }
        }
        go(self, 0, false, f)
    }
}

/// Structured, human-readable expression error.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprError {
    pub kind: ExprErrorKind,
    pub span: Span,
    pub message: String,
    /// A concrete suggestion for fixing the problem, when we have one.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprErrorKind {
    Syntax,
    TooComplex,
    UnknownUnit,
    UnknownVariable,
    UnknownFunction,
    DimensionMismatch,
    Domain,
    Cycle,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ExprError {}

fn err(kind: ExprErrorKind, span: Span, message: impl Into<String>) -> ExprError {
    ExprError { kind, span, message: message.into(), hint: None }
}

impl ExprError {
    fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }
}

// ---------------------------------------------------------------- lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
    End,
}

fn lex(src: &str) -> Result<Vec<(Tok, Span)>, ExprError> {
    let mut out = Vec::new();
    let mut it = src.char_indices().peekable();
    while let Some(&(i, c)) = it.peek() {
        if c.is_whitespace() {
            it.next();
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            let mut end = i;
            let mut seen_e = false;
            while let Some(&(j, d)) = it.peek() {
                let prev_is_e = src[start..j].ends_with(['e', 'E']);
                if d.is_ascii_digit() || d == '.' {
                    end = j + d.len_utf8();
                    it.next();
                } else if (d == 'e' || d == 'E') && !seen_e {
                    // Only treat `e` as an exponent if a digit (or sign+digit) follows;
                    // otherwise `2e` would swallow a following identifier.
                    let rest = &src[j + 1..];
                    let mut rc = rest.chars();
                    let ok = match rc.next() {
                        Some(x) if x.is_ascii_digit() => true,
                        Some('+' | '-') => rc.next().is_some_and(|x| x.is_ascii_digit()),
                        _ => false,
                    };
                    if !ok {
                        break;
                    }
                    seen_e = true;
                    end = j + 1;
                    it.next();
                } else if (d == '+' || d == '-') && prev_is_e && seen_e {
                    end = j + 1;
                    it.next();
                } else {
                    break;
                }
            }
            let text = &src[start..end];
            let span = Span { start, end };
            let v: f64 = text
                .parse()
                .map_err(|_| err(ExprErrorKind::Syntax, span, format!("'{text}' is not a valid number")))?;
            if !v.is_finite() {
                return Err(err(ExprErrorKind::Domain, span, format!("'{text}' is too large")));
            }
            out.push((Tok::Num(v), span));
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '°' || c == 'µ' {
            let start = i;
            let mut end = i;
            if c == '°' {
                it.next();
                end = i + c.len_utf8();
            } else {
                while let Some(&(j, d)) = it.peek() {
                    if d.is_alphanumeric() || d == '_' || (j == start && d == 'µ') {
                        end = j + d.len_utf8();
                        it.next();
                    } else {
                        break;
                    }
                }
            }
            out.push((Tok::Ident(src[start..end].to_string()), Span { start, end }));
            continue;
        }
        let span = Span { start: i, end: i + c.len_utf8() };
        let t = match c {
            '+' | '-' | '*' | '/' | '^' => Tok::Op(c),
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            _ => return Err(err(ExprErrorKind::Syntax, span, format!("unexpected character '{c}'"))),
        };
        it.next();
        out.push((t, span));
    }
    out.push((Tok::End, Span { start: src.len(), end: src.len() }));
    Ok(out)
}

// ---------------------------------------------------------------- parser

struct Parser {
    toks: Vec<(Tok, Span)>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &(Tok, Span) {
        // The token list always ends with `End`, and `pos` never passes it.
        let last = self.toks.len().saturating_sub(1);
        self.toks.get(self.pos.min(last)).unwrap_or(&(Tok::End, Span { start: 0, end: 0 }))
    }

    fn bump(&mut self) -> (Tok, Span) {
        let t = self.peek().clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn enter(&mut self, span: Span) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(err(ExprErrorKind::TooComplex, span, "expression is nested too deeply")
                .hint(format!("use at most {MAX_DEPTH} levels of nesting")));
        }
        Ok(())
    }

    fn expr(&mut self, min_prec: u8) -> Result<Expr, ExprError> {
        let span0 = self.peek().1;
        self.enter(span0)?;
        let mut lhs = self.unary()?;
        loop {
            let (op, _) = match self.peek() {
                (Tok::Op('+'), s) => (BinOp::Add, *s),
                (Tok::Op('-'), s) => (BinOp::Sub, *s),
                (Tok::Op('*'), s) => (BinOp::Mul, *s),
                (Tok::Op('/'), s) => (BinOp::Div, *s),
                (Tok::Op('^'), s) => (BinOp::Pow, *s),
                _ => break,
            };
            let p = op.precedence();
            if p < min_prec {
                break;
            }
            self.bump();
            // Right-associative for `^`.
            let next_min = if op == BinOp::Pow { p } else { p + 1 };
            let rhs = self.expr(next_min)?;
            let span = lhs.span().to(rhs.span());
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        self.depth -= 1;
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, ExprError> {
        match self.peek().clone() {
            (Tok::Op('-'), s) => {
                self.bump();
                self.enter(s)?;
                // Unary minus binds looser than `^`: -2^2 == -(2^2).
                let inner = self.expr(BinOp::Pow.precedence())?;
                self.depth -= 1;
                let span = s.to(inner.span());
                Ok(Expr::Neg { inner: Box::new(inner), span })
            }
            (Tok::Op('+'), _) => {
                self.bump();
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, ExprError> {
        match self.bump() {
            (Tok::Num(v), s) => {
                // A unit name directly after a number binds to it: `10 mm`, `0.5in`.
                if let (Tok::Ident(name), us) = self.peek().clone()
                    && unit(&name).is_some()
                    && !matches!(self.toks.get(self.pos + 1), Some((Tok::LParen, _)))
                {
                    self.bump();
                    return Ok(Expr::Number { value: v, unit: Some(name), span: s.to(us) });
                }
                Ok(Expr::Number { value: v, unit: None, span: s })
            }
            (Tok::Ident(name), s) => {
                if let (Tok::LParen, _) = self.peek() {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek().0, Tok::RParen) {
                        loop {
                            args.push(self.expr(0)?);
                            match self.bump() {
                                (Tok::Comma, _) => continue,
                                (Tok::RParen, e) => {
                                    return Ok(Expr::Call { func: name, args, span: s.to(e) });
                                }
                                (_, e) => {
                                    return Err(err(ExprErrorKind::Syntax, e, "expected ',' or ')' in function call"));
                                }
                            }
                        }
                    }
                    let (_, e) = self.bump();
                    return Ok(Expr::Call { func: name, args, span: s.to(e) });
                }
                Ok(Expr::Var { name, span: s })
            }
            (Tok::LParen, s) => {
                let inner = self.expr(0)?;
                match self.bump() {
                    (Tok::RParen, _) => Ok(inner),
                    (_, e) => Err(err(ExprErrorKind::Syntax, s.to(e), "missing closing ')'")),
                }
            }
            (Tok::End, s) => Err(err(ExprErrorKind::Syntax, s, "expression ended unexpectedly")),
            (t, s) => Err(err(ExprErrorKind::Syntax, s, format!("unexpected {}", describe(&t)))),
        }
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(v) => format!("number {v}"),
        Tok::Ident(n) => format!("name '{n}'"),
        Tok::Op(c) => format!("'{c}'"),
        Tok::LParen => "'('".into(),
        Tok::RParen => "')'".into(),
        Tok::Comma => "','".into(),
        Tok::End => "end of expression".into(),
    }
}

/// Parses an expression. Fails on syntax errors, unknown units after numbers are
/// treated as variables (and fail later at evaluation with a clear message).
pub fn parse(src: &str) -> Result<Expr, ExprError> {
    if src.len() > MAX_EXPR_LEN {
        return Err(err(ExprErrorKind::TooComplex, Span { start: 0, end: src.len() }, "expression is too long")
            .hint(format!("keep expressions under {MAX_EXPR_LEN} characters")));
    }
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let e = p.expr(0)?;
    match p.peek().clone() {
        (Tok::End, _) => Ok(e),
        (Tok::Ident(n), s) if matches!(e, Expr::Number { unit: None, .. }) => {
            Err(err(ExprErrorKind::UnknownUnit, s, format!("unknown unit '{n}'"))
                .hint("known units: mm, cm, m, um, in, ft, thou, deg, rad, kg, g, lb, s"))
        }
        (t, s) => Err(err(ExprErrorKind::Syntax, s, format!("unexpected {} after expression", describe(&t)))
            .hint("did you forget an operator such as '*'?")),
    }
}

// ---------------------------------------------------------------- evaluation

/// Variable lookup for evaluation.
pub trait Scope {
    fn lookup(&self, name: &str) -> Option<Quantity>;
}

impl Scope for () {
    fn lookup(&self, _: &str) -> Option<Quantity> {
        None
    }
}

impl<F: Fn(&str) -> Option<Quantity>> Scope for F {
    fn lookup(&self, name: &str) -> Option<Quantity> {
        self(name)
    }
}

/// Evaluates an expression. Results are bit-identical on every platform.
pub fn eval(e: &Expr, scope: &dyn Scope) -> Result<Quantity, ExprError> {
    let q = eval_inner(e, scope)?;
    if !q.value.is_finite() {
        return Err(err(ExprErrorKind::Domain, e.span(), "result is not a finite number")
            .hint("check for division by zero or overflow"));
    }
    Ok(q)
}

fn finite(q: Quantity, span: Span, what: &str) -> Result<Quantity, ExprError> {
    if q.value.is_finite() {
        Ok(q)
    } else {
        Err(err(ExprErrorKind::Domain, span, format!("{what} produced a non-finite value")))
    }
}

fn eval_inner(e: &Expr, scope: &dyn Scope) -> Result<Quantity, ExprError> {
    match e {
        Expr::Number { value, unit: None, .. } => Ok(Quantity::number(*value)),
        Expr::Number { value, unit: Some(u), span } => {
            let un = unit(u).ok_or_else(|| err(ExprErrorKind::UnknownUnit, *span, format!("unknown unit '{u}'")))?;
            Ok(Quantity::new(value * un.factor, un.dim))
        }
        Expr::Var { name, span } => {
            if let Some(q) = scope.lookup(name) {
                return Ok(q);
            }
            match name.as_str() {
                "pi" | "PI" => Ok(Quantity::number(std::f64::consts::PI)),
                "e" => Ok(Quantity::number(std::f64::consts::E)),
                _ => {
                    let mut er = err(ExprErrorKind::UnknownVariable, *span, format!("unknown parameter '{name}'"));
                    if unit(name).is_some() {
                        er = er.hint(format!("'{name}' is a unit; put a number in front of it, e.g. '1 {name}'"));
                    }
                    Err(er)
                }
            }
        }
        Expr::Neg { inner, .. } => {
            let q = eval_inner(inner, scope)?;
            Ok(Quantity::new(-q.value, q.dim))
        }
        Expr::Binary { op, lhs, rhs, span } => {
            let a = eval_inner(lhs, scope)?;
            let b = eval_inner(rhs, scope)?;
            let overflow = || err(ExprErrorKind::DimensionMismatch, *span, "unit exponent is out of range");
            match op {
                BinOp::Add | BinOp::Sub => {
                    if a.dim != b.dim {
                        let mut er = err(
                            ExprErrorKind::DimensionMismatch,
                            *span,
                            format!(
                                "cannot {} {} and {}",
                                if *op == BinOp::Add { "add" } else { "subtract" },
                                a.dim.name(),
                                b.dim.name()
                            ),
                        );
                        if a.dim.is_none() || b.dim.is_none() {
                            let other = if a.dim.is_none() { b.dim } else { a.dim };
                            let u = if other == Dim::ANGLE { "deg" } else { "mm" };
                            er = er.hint(format!("give the plain number a unit, e.g. '5 {u}'"));
                        }
                        return Err(er);
                    }
                    let v = if *op == BinOp::Add { a.value + b.value } else { a.value - b.value };
                    finite(Quantity::new(v, a.dim), *span, "this operation")
                }
                BinOp::Mul => finite(
                    Quantity::new(a.value * b.value, a.dim.times(b.dim).ok_or_else(overflow)?),
                    *span,
                    "multiplication",
                ),
                BinOp::Div => {
                    if b.value == 0.0 {
                        return Err(err(ExprErrorKind::Domain, rhs.span(), "division by zero"));
                    }
                    finite(Quantity::new(a.value / b.value, a.dim.per(b.dim).ok_or_else(overflow)?), *span, "division")
                }
                BinOp::Pow => {
                    if !b.dim.is_none() {
                        return Err(err(
                            ExprErrorKind::DimensionMismatch,
                            rhs.span(),
                            "an exponent must be a plain number",
                        ));
                    }
                    let dim = if a.dim.is_none() {
                        Dim::NONE
                    } else {
                        let n = b.value;
                        if n.fract() != 0.0 || n.abs() > 16.0 {
                            return Err(err(
                                ExprErrorKind::DimensionMismatch,
                                rhs.span(),
                                format!("{} can only be raised to a whole-number power", a.dim.name()),
                            )
                            .hint("use sqrt() for square roots"));
                        }
                        // n is integral and |n| <= 16, so the cast is exact.
                        #[allow(clippy::cast_possible_truncation)]
                        a.dim.powi(n as i32).ok_or_else(overflow)?
                    };
                    finite(Quantity::new(libm::pow(a.value, b.value), dim), *span, "power")
                }
            }
        }
        Expr::Call { func, args, span } => call(func, args, *span, scope),
    }
}

fn call(func: &str, args: &[Expr], span: Span, scope: &dyn Scope) -> Result<Quantity, ExprError> {
    let arity = |n: usize| -> Result<(), ExprError> {
        if args.len() == n {
            Ok(())
        } else {
            Err(err(
                ExprErrorKind::Syntax,
                span,
                format!("{func}() takes {n} argument{}, got {}", if n == 1 { "" } else { "s" }, args.len()),
            ))
        }
    };
    let arg = |i: usize| -> Result<Quantity, ExprError> {
        let a =
            args.get(i).ok_or_else(|| err(ExprErrorKind::Syntax, span, format!("{func}() is missing an argument")))?;
        eval_inner(a, scope)
    };
    let arg_span = |i: usize| args.get(i).map_or(span, Expr::span);
    let angle_in = |i: usize| -> Result<f64, ExprError> {
        let q = arg(i)?;
        if q.dim == Dim::ANGLE {
            Ok(q.value)
        } else if q.dim.is_none() {
            Err(err(ExprErrorKind::DimensionMismatch, arg_span(i), format!("{func}() needs an angle")).hint(format!(
                "write the unit, e.g. '{func}({} deg)' or '{func}({} rad)'",
                crate::quantity::format_number(q.value),
                crate::quantity::format_number(q.value)
            )))
        } else {
            Err(err(
                ExprErrorKind::DimensionMismatch,
                arg_span(i),
                format!("{func}() needs an angle, got {}", q.dim.name()),
            ))
        }
    };
    let plain = |i: usize| -> Result<f64, ExprError> {
        let q = arg(i)?;
        if q.dim.is_none() {
            Ok(q.value)
        } else {
            Err(err(
                ExprErrorKind::DimensionMismatch,
                arg_span(i),
                format!("{func}() needs a plain number, got {}", q.dim.name()),
            ))
        }
    };
    let ratio = |v: f64| -> Result<f64, ExprError> {
        if (-1.0..=1.0).contains(&v) {
            Ok(v)
        } else {
            Err(err(ExprErrorKind::Domain, span, format!("{func}() needs a value between -1 and 1, got {v}")))
        }
    };
    let q = match func {
        "sin" => {
            arity(1)?;
            Quantity::number(libm::sin(angle_in(0)?))
        }
        "cos" => {
            arity(1)?;
            Quantity::number(libm::cos(angle_in(0)?))
        }
        "tan" => {
            arity(1)?;
            Quantity::number(libm::tan(angle_in(0)?))
        }
        "asin" => {
            arity(1)?;
            Quantity::rad(libm::asin(ratio(plain(0)?)?))
        }
        "acos" => {
            arity(1)?;
            Quantity::rad(libm::acos(ratio(plain(0)?)?))
        }
        "atan" => {
            arity(1)?;
            Quantity::rad(libm::atan(plain(0)?))
        }
        "atan2" => {
            arity(2)?;
            let (y, x) = (arg(0)?, arg(1)?);
            if y.dim != x.dim {
                return Err(err(ExprErrorKind::DimensionMismatch, span, "atan2() needs two values of the same kind"));
            }
            Quantity::rad(libm::atan2(y.value, x.value))
        }
        "sqrt" => {
            arity(1)?;
            let a = arg(0)?;
            if a.value < 0.0 {
                return Err(err(ExprErrorKind::Domain, arg_span(0), "sqrt() of a negative value"));
            }
            let dim = a.dim.sqrt().ok_or_else(|| {
                err(
                    ExprErrorKind::DimensionMismatch,
                    arg_span(0),
                    format!("cannot take the square root of {}", a.dim.name()),
                )
            })?;
            Quantity::new(a.value.sqrt(), dim)
        }
        "abs" => {
            arity(1)?;
            let a = arg(0)?;
            Quantity::new(a.value.abs(), a.dim)
        }
        "min" | "max" => {
            if args.is_empty() {
                return Err(err(ExprErrorKind::Syntax, span, format!("{func}() needs at least one argument")));
            }
            let mut best = arg(0)?;
            for i in 1..args.len() {
                let b = arg(i)?;
                if b.dim != best.dim {
                    return Err(err(
                        ExprErrorKind::DimensionMismatch,
                        arg_span(i),
                        format!("{func}() arguments must all be of the same kind"),
                    ));
                }
                let better = if func == "min" { b.value < best.value } else { b.value > best.value };
                if better {
                    best = b;
                }
            }
            best
        }
        "floor" => {
            arity(1)?;
            Quantity::number(plain(0)?.floor())
        }
        "ceil" => {
            arity(1)?;
            Quantity::number(plain(0)?.ceil())
        }
        "round" => {
            arity(1)?;
            Quantity::number(plain(0)?.round())
        }
        _ => {
            return Err(err(ExprErrorKind::UnknownFunction, span, format!("unknown function '{func}'"))
                .hint("available: sin, cos, tan, asin, acos, atan, atan2, sqrt, abs, min, max, floor, ceil, round"));
        }
    };
    finite(q, span, &format!("{func}()"))
}

/// Parses and evaluates in one step, for callers that do not keep the tree.
pub fn eval_str(src: &str, scope: &dyn Scope) -> Result<Quantity, ExprError> {
    eval(&parse(src)?, scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantity::Dim;

    fn ev(s: &str) -> Quantity {
        eval_str(s, &()).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn unit_arithmetic() {
        let q = ev("10 mm + 0.5 in");
        assert_eq!(q.dim, Dim::LENGTH);
        assert!(close(q.value, 22.7));
        assert!(close(ev("2in").value, 50.8));
        assert!(close(ev("1 m / 4").value, 250.0));
        assert_eq!(ev("10 mm * 2 mm").dim, Dim::AREA);
        assert_eq!(ev("(2 mm)^3").dim, Dim::VOLUME);
        assert!(close(ev("90 deg").value, std::f64::consts::FRAC_PI_2));
        assert!(close(ev("90°").value, std::f64::consts::FRAC_PI_2));
    }

    #[test]
    fn precedence() {
        assert!(close(ev("2 + 3 * 4").value, 14.0));
        assert!(close(ev("2 ^ 3 ^ 2").value, 512.0));
        assert!(close(ev("-2 ^ 2").value, -4.0));
        assert!(close(ev("(1 + 2) * 3").value, 9.0));
        assert!(close(ev("10 - 4 - 3").value, 3.0));
        assert!(close(ev("1.5e2").value, 150.0));
        assert!(close(ev("2e-1").value, 0.2));
    }

    #[test]
    fn functions() {
        assert!(close(ev("sin(30 deg)").value, 0.5));
        assert!(close(ev("acos(0)").value, std::f64::consts::FRAC_PI_2));
        assert_eq!(ev("atan2(1 mm, 1 mm)").dim, Dim::ANGLE);
        assert!(close(ev("sqrt(16 mm * 1 mm)").value, 4.0));
        assert!(close(ev("max(1 mm, 3 mm, 2 mm)").value, 3.0));
        assert!(close(ev("2 * pi").value, std::f64::consts::TAU));
    }

    #[test]
    fn variables() {
        let scope = |n: &str| match n {
            "width" => Some(Quantity::mm(40.0)),
            _ => None,
        };
        let q = eval_str("width / 2", &scope).unwrap();
        assert!(close(q.value, 20.0));
        let e = parse("width/2 + depth * width").unwrap();
        assert_eq!(e.variables(), vec!["width".to_string(), "depth".to_string()]);
        let er = eval_str("depth", &scope).unwrap_err();
        assert_eq!(er.kind, ExprErrorKind::UnknownVariable);
    }

    #[test]
    fn helpful_errors() {
        let e = eval_str("10 mm + 5", &()).unwrap_err();
        assert_eq!(e.kind, ExprErrorKind::DimensionMismatch);
        assert!(e.hint.unwrap().contains("5 mm"));
        let e = eval_str("sin(30)", &()).unwrap_err();
        assert!(e.hint.unwrap().contains("30 deg"));
        let e = parse("10 furlongs").unwrap_err();
        assert_eq!(e.kind, ExprErrorKind::UnknownUnit);
        let e = eval_str("1 / 0", &()).unwrap_err();
        assert_eq!(e.kind, ExprErrorKind::Domain);
        assert!(parse("(1 + 2").is_err());
        assert!(parse("1 +").is_err());
        assert!(parse("1 $ 2").is_err());
        assert_eq!(
            eval_str("mm", &()).unwrap_err().hint.unwrap(),
            "'mm' is a unit; put a number in front of it, e.g. '1 mm'"
        );
    }

    #[test]
    fn limits() {
        let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        assert_eq!(parse(&deep).unwrap_err().kind, ExprErrorKind::TooComplex);
        let negs = "-".repeat(500) + "1";
        assert_eq!(parse(&negs).unwrap_err().kind, ExprErrorKind::TooComplex);
        let long = "1+".repeat(3000) + "1";
        assert_eq!(parse(&long).unwrap_err().kind, ExprErrorKind::TooComplex);
        assert!(eval_str("1e308 * 10", &()).is_err());
    }

    #[test]
    fn print_roundtrip() {
        for s in [
            "1 + 2 * 3",
            "(1 + 2) * 3",
            "2 ^ 3 ^ 2",
            "(2 ^ 3) ^ 2",
            "a - (b - c)",
            "a - b - c",
            "-x ^ 2",
            "atan2(h, w / 2) + 30 deg",
            "10 mm + 0.5 in",
            "a / (b * c)",
            "(-2) ^ 2",
            "(-x) ^ (-y) ^ 2",
            "-(x ^ 2)",
            "2 ^ -x ^ 2",
            "a * -b - -(c + d)",
            "--x",
        ] {
            let e = parse(s).unwrap();
            let printed = e.to_string();
            let e2 = parse(&printed).unwrap();
            assert_eq!(printed, e2.to_string(), "{s}");
            assert!(e.same_tree(&e2), "{s} printed as {printed}");
            let v = |n: &str| Some(Quantity::number(n.len() as f64 + 1.5));
            let (a, b) = (eval(&e, &v), eval(&e2, &v));
            assert_eq!(a.map(|q| q.value.to_bits()).ok(), b.map(|q| q.value.to_bits()).ok(), "{s}");
        }
    }

    #[test]
    fn negation_prints_with_minimal_parentheses() {
        for (s, want) in [
            ("(-2) ^ 2", "(-2) ^ 2"),
            ("-(2 ^ 2)", "-2 ^ 2"),
            ("-(a * b)", "-(a * b)"),
            ("2 ^ (-x)", "2 ^ -x"),
            ("-(-x)", "--x"),
        ] {
            assert_eq!(parse(s).unwrap().to_string(), want, "{s}");
        }
        // Found by fuzzing: printing a negated power used to add parentheses, so a deep
        // but valid expression printed past the nesting limit.
        let deep = format!("2 ^{}2 ^ a", "-".repeat(29));
        let e = parse(&deep).unwrap();
        assert!(parse(&e.to_string()).unwrap().same_tree(&e));
        let v = |_: &str| Some(Quantity::number(3.0));
        assert_eq!(eval(&parse("(-2) ^ 2").unwrap(), &v).unwrap().value, 4.0);
        assert_eq!(eval(&parse("-2 ^ 2").unwrap(), &v).unwrap().value, -4.0);
    }

    #[test]
    fn rename() {
        let e = parse("a + b * a").unwrap();
        assert_eq!(e.rename_var("a", "z").to_string(), "z + b * z");
    }
}
