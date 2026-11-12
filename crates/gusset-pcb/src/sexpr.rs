//! A lossless S-expression reader and writer for KiCad's board and footprint files.
//!
//! Written from the published format description (dev-docs.kicad.org, "S-Expression
//! Format") and sample files only. Every node is kept, known or not, so a file Gusset does
//! not fully understand writes back with the same tree: only the layout (white space and
//! line breaks) may change. Atoms keep their exact text and whether they were quoted.
//!
//! Inputs are untrusted: the reader is iterative (no recursion on the input's depth) and
//! bounded by [`Limits`].

use std::fmt;

/// Bounds on what the reader accepts.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Largest input in bytes.
    pub max_bytes: usize,
    /// Deepest list nesting.
    pub max_depth: usize,
    /// Most nodes (atoms plus lists) in the whole file.
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        // Real boards are a few MB (large ones tens of MB) and nest about ten deep.
        Limits { max_bytes: 256 << 20, max_depth: 128, max_nodes: 30_000_000 }
    }
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TooLarge,
    TooDeep {
        offset: usize,
    },
    TooManyNodes,
    /// Bytes after the top-level list, an unbalanced `)` or a missing `)`.
    Syntax {
        offset: usize,
        what: &'static str,
    },
    NotUtf8 {
        offset: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooLarge => write!(f, "the file is larger than the reader's limit"),
            Error::TooDeep { offset } => write!(f, "lists nest too deeply (byte {offset})"),
            Error::TooManyNodes => write!(f, "the file has more items than the reader's limit"),
            Error::Syntax { offset, what } => write!(f, "{what} (byte {offset})"),
            Error::NotUtf8 { offset } => write!(f, "text that is not UTF-8 (byte {offset})"),
        }
    }
}

impl std::error::Error for Error {}

/// One atom: a symbol, number or string, with its text as written (escapes resolved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
    pub text: String,
    /// Written between double quotes.
    pub quoted: bool,
}

/// A node of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Atom(Atom),
    List(Vec<Node>),
}

impl Node {
    /// A bare symbol or number.
    pub fn sym(t: impl Into<String>) -> Node {
        Node::Atom(Atom { text: t.into(), quoted: false })
    }
    /// A quoted string.
    pub fn str(t: impl Into<String>) -> Node {
        Node::Atom(Atom { text: t.into(), quoted: true })
    }
    /// A number written the way KiCad writes them (no exponent, no trailing zeros).
    pub fn num(v: f64) -> Node {
        Node::sym(fmt_num(v))
    }

    /// The atom's text.
    pub fn atom(&self) -> Option<&str> {
        match self {
            Node::Atom(a) => Some(&a.text),
            Node::List(_) => None,
        }
    }

    /// The list's items.
    pub fn items(&self) -> &[Node] {
        match self {
            Node::List(v) => v,
            Node::Atom(_) => &[],
        }
    }

    /// Mutable list items (`None` for an atom).
    pub fn items_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Node::List(v) => Some(v),
            Node::Atom(_) => None,
        }
    }

    /// The list's head symbol (`footprint` for `(footprint ...)`).
    pub fn head(&self) -> Option<&str> {
        self.items().first().and_then(Node::atom)
    }

    /// The items after the head.
    pub fn args(&self) -> &[Node] {
        self.items().get(1..).unwrap_or(&[])
    }

    /// The first child list with this head.
    pub fn child(&self, head: &str) -> Option<&Node> {
        self.args().iter().find(|n| n.head() == Some(head))
    }

    /// Every child list with this head.
    pub fn children<'a>(&'a self, head: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.args().iter().filter(move |n| n.head() == Some(head))
    }

    /// The first child list with this head, mutably.
    pub fn child_mut(&mut self, head: &str) -> Option<&mut Node> {
        self.items_mut()?.iter_mut().skip(1).find(|n| n.head() == Some(head))
    }

    /// The `i`-th argument as text.
    pub fn arg(&self, i: usize) -> Option<&str> {
        self.args().get(i).and_then(Node::atom)
    }

    /// The `i`-th argument as a finite number.
    pub fn num_arg(&self, i: usize) -> Option<f64> {
        self.arg(i).and_then(parse_num)
    }

    /// Whether a bare symbol `flag` is among the arguments (`(attr smd board_only)`).
    pub fn has_flag(&self, flag: &str) -> bool {
        self.args().iter().any(|n| matches!(n, Node::Atom(a) if !a.quoted && a.text == flag))
    }

    /// Number of nodes in this subtree (for tests and budgets).
    pub fn count(&self) -> usize {
        let mut n = 0;
        let mut stack = vec![self];
        while let Some(x) = stack.pop() {
            n += 1;
            stack.extend(x.items());
        }
        n
    }
}

/// A finite number in a sane range (|v| < 1e9), or `None`.
pub fn parse_num(t: &str) -> Option<f64> {
    let v: f64 = t.parse().ok()?;
    (v.is_finite() && v.abs() < 1e9).then_some(v)
}

/// A number as KiCad writes them: plain decimal, up to 6 places, no trailing zeros.
pub fn fmt_num(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let mut s = format!("{v:.6}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".into() } else { s }
}

/// Reads one top-level list (the whole file).
///
/// # Errors
/// [`Error`] for malformed input or input past `limits`.
pub fn parse(src: &str, limits: &Limits) -> Result<Node, Error> {
    if src.len() > limits.max_bytes {
        return Err(Error::TooLarge);
    }
    let b = src.as_bytes();
    let mut i = 0;
    // The open lists, innermost last.
    let mut stack: Vec<Vec<Node>> = Vec::new();
    let mut top: Option<Node> = None;
    let mut nodes = 0usize;
    let bump = |nodes: &mut usize| {
        *nodes += 1;
        if *nodes > limits.max_nodes { Err(Error::TooManyNodes) } else { Ok(()) }
    };
    while let Some(&c) = b.get(i) {
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'(' => {
                if top.is_some() {
                    return Err(Error::Syntax { offset: i, what: "text after the end of the file's list" });
                }
                if stack.len() >= limits.max_depth {
                    return Err(Error::TooDeep { offset: i });
                }
                bump(&mut nodes)?;
                stack.push(Vec::new());
                i += 1;
            }
            b')' => {
                let done = stack.pop().ok_or(Error::Syntax { offset: i, what: "a ')' without its '('" })?;
                let node = Node::List(done);
                match stack.last_mut() {
                    Some(parent) => parent.push(node),
                    None => top = Some(node),
                }
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut text = String::new();
                let mut run = i;
                loop {
                    match b.get(i) {
                        None => {
                            return Err(Error::Syntax { offset: start, what: "a string without its closing quote" });
                        }
                        Some(b'"') => {
                            text.push_str(src.get(run..i).ok_or(Error::NotUtf8 { offset: run })?);
                            i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            text.push_str(src.get(run..i).ok_or(Error::NotUtf8 { offset: run })?);
                            let e = b
                                .get(i + 1)
                                .copied()
                                .ok_or(Error::Syntax { offset: start, what: "a string without its closing quote" })?;
                            match e {
                                b'n' => text.push('\n'),
                                b'r' => text.push('\r'),
                                b't' => text.push('\t'),
                                b'"' => text.push('"'),
                                b'\\' => text.push('\\'),
                                _ => {
                                    // Unknown escapes stay as written (the backslash kept).
                                    text.push('\\');
                                    i += 1;
                                    run = i;
                                    continue;
                                }
                            }
                            i += 2;
                            run = i;
                        }
                        Some(_) => i += 1,
                    }
                }
                bump(&mut nodes)?;
                let atom = Node::Atom(Atom { text, quoted: true });
                stack
                    .last_mut()
                    .ok_or(Error::Syntax { offset: start, what: "text outside the file's list" })?
                    .push(atom);
            }
            _ => {
                let start = i;
                while let Some(&d) = b.get(i) {
                    if matches!(d, b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"') {
                        break;
                    }
                    i += 1;
                }
                let text = src.get(start..i).ok_or(Error::NotUtf8 { offset: start })?;
                bump(&mut nodes)?;
                let atom = Node::Atom(Atom { text: text.to_string(), quoted: false });
                stack
                    .last_mut()
                    .ok_or(Error::Syntax { offset: start, what: "text outside the file's list" })?
                    .push(atom);
            }
        }
    }
    if !stack.is_empty() {
        return Err(Error::Syntax { offset: src.len(), what: "the file ends inside a list (a ')' is missing)" });
    }
    top.ok_or(Error::Syntax { offset: 0, what: "the file has no list" })
}

/// Writes the tree in KiCad's layout: short lists on one line, others with one child list
/// per line, indented by a tab per level.
pub fn write(node: &Node) -> String {
    enum Step<'a> {
        Item(&'a Node, usize),
        Newline(usize),
        Close,
    }
    let mut out = String::new();
    let mut stack = vec![Step::Item(node, 0)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Newline(depth) => {
                out.push('\n');
                push_indent(&mut out, depth);
            }
            Step::Close => out.push(')'),
            Step::Item(Node::Atom(a), _) => write_atom(&mut out, a),
            Step::Item(n @ Node::List(items), depth) => {
                if let Some(line) = inline(n, 100) {
                    out.push_str(&line);
                    continue;
                }
                out.push('(');
                // Leading atoms stay on the opening line; from the first list on, every
                // item gets its own line.
                let lead = items.iter().take_while(|x| matches!(x, Node::Atom(_))).count();
                for (k, a) in items.iter().take(lead).enumerate() {
                    if k > 0 {
                        out.push(' ');
                    }
                    if let Node::Atom(a) = a {
                        write_atom(&mut out, a);
                    }
                }
                // Pushed in reverse so they pop in order.
                stack.push(Step::Close);
                stack.push(Step::Newline(depth));
                for x in items.iter().skip(lead).rev() {
                    stack.push(Step::Item(x, depth + 1));
                    stack.push(Step::Newline(depth + 1));
                }
            }
        }
    }
    out.push('\n');
    out
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}

/// The list on one line if that fits in `width` characters and it holds no list of lists
/// (KiCad keeps `(at 1 2)` and `(stroke (width 0.1) (type solid))` on one line and
/// footprints and pads on several); point lists stay on one line when they fit.
fn inline(n: &Node, width: usize) -> Option<String> {
    let deep = n.items().iter().any(|x| x.items().iter().any(|y| matches!(y, Node::List(_))));
    if deep && n.head() != Some("pts") {
        return None;
    }
    let mut s = String::new();
    flat(&mut s, n, width).then_some(s)
}

fn flat(out: &mut String, n: &Node, width: usize) -> bool {
    let mut stack: Vec<Result<&Node, ()>> = vec![Ok(n)];
    let mut first_in_list: Vec<bool> = Vec::new();
    while let Some(x) = stack.pop() {
        if out.len() > width {
            return false;
        }
        match x {
            Err(()) => {
                out.push(')');
                first_in_list.pop();
            }
            Ok(node) => {
                if let Some(f) = first_in_list.last_mut() {
                    if !*f {
                        out.push(' ');
                    }
                    *f = false;
                }
                match node {
                    Node::Atom(a) => write_atom(out, a),
                    Node::List(items) => {
                        out.push('(');
                        first_in_list.push(true);
                        stack.push(Err(()));
                        for c in items.iter().rev() {
                            stack.push(Ok(c));
                        }
                    }
                }
            }
        }
    }
    out.len() <= width
}

fn write_atom(out: &mut String, a: &Atom) {
    let needs = a.text.is_empty()
        || a.text.chars().any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\\') || c.is_control());
    if !a.quoted && !needs {
        out.push_str(&a.text);
        return;
    }
    out.push('"');
    for c in a.text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}
