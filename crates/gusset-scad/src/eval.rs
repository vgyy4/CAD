//! Evaluation of a parsed OpenSCAD program into a CSG tree (ADR 0033).
//!
//! Values, scopes and calls follow the OpenSCAD User Manual: variables are resolved per
//! scope with the last assignment winning, `$` variables are dynamically scoped, modules
//! and functions are lexically scoped, `children()` evaluates the caller's child
//! statements in the caller's scope. Angles are degrees.

use crate::parse::{Arg, Expr, Param, Stmt, VecItem};
use crate::{Csg, Csg2, IDENTITY, M4, Model, Op, Prim, ScadError, Shape2, mul};
use std::collections::HashMap;
use std::rc::Rc;

/// Bounds on evaluation.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_steps: u64,
    pub max_depth: u32,
    pub max_primitives: usize,
    /// Most elements in one vector or range expansion.
    pub max_vector: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_steps: 10_000_000, max_depth: 256, max_primitives: 100_000, max_vector: 1_000_000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Undef,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Vec(Rc<Vec<Value>>),
    Range(f64, f64, f64),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Value::Undef => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Vec(v) => !v.is_empty(),
            Value::Range(..) => true,
        }
    }
    fn num(&self) -> Option<f64> {
        if let Value::Num(n) = self { Some(*n) } else { None }
    }
    fn show(&self) -> String {
        match self {
            Value::Undef => "undef".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{n:.0}")
                } else {
                    format!("{n}")
                }
            }
            Value::Str(s) => s.to_string(),
            Value::Vec(v) => format!("[{}]", v.iter().map(Value::show).collect::<Vec<_>>().join(", ")),
            Value::Range(a, s, b) => format!("[{a} : {s} : {b}]"),
        }
    }
}

/// A user module or function and the scope it was defined in.
#[derive(Clone)]
struct ModDef {
    params: Rc<Vec<Param>>,
    body: Rc<Vec<Stmt>>,
    scope: usize,
}

#[derive(Clone)]
struct FnDef {
    params: Rc<Vec<Param>>,
    body: Rc<Expr>,
    scope: usize,
}

#[derive(Default)]
struct Scope {
    parent: Option<usize>,
    vars: HashMap<String, Value>,
    mods: HashMap<String, ModDef>,
    funcs: HashMap<String, FnDef>,
}

/// The children passed to the module call being evaluated.
#[derive(Clone)]
struct Children {
    stmts: Rc<Vec<Stmt>>,
    scope: usize,
    /// The children of the caller's own call (for `children()` inside the children).
    outer: Option<Rc<Children>>,
}

/// What a statement produced.
#[derive(Debug, Clone)]
enum Obj {
    D3(Csg),
    D2(Csg2),
}

struct Eval<'a> {
    lim: &'a Limits,
    scopes: Vec<Scope>,
    /// `$` variables: (name, value), innermost last.
    dynamic: Vec<(String, Value)>,
    steps: u64,
    depth: u32,
    prims: usize,
    warnings: Vec<String>,
    children: Option<Rc<Children>>,
}

type R<T> = Result<T, ScadError>;

fn err<T>(line: u32, m: impl Into<String>) -> R<T> {
    Err(ScadError::at(line, m))
}

fn rad(d: f64) -> f64 {
    d.to_radians()
}

fn translate(v: [f64; 3]) -> M4 {
    [[1.0, 0.0, 0.0, v[0]], [0.0, 1.0, 0.0, v[1]], [0.0, 0.0, 1.0, v[2]], [0.0, 0.0, 0.0, 1.0]]
}

fn scale(v: [f64; 3]) -> M4 {
    [[v[0], 0.0, 0.0, 0.0], [0.0, v[1], 0.0, 0.0], [0.0, 0.0, v[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// Rotation by `deg` about the unit axis `a`.
fn rot_axis(a: [f64; 3], deg: f64) -> M4 {
    let (s, c) = (libm::sin(rad(deg)), libm::cos(rad(deg)));
    let t = 1.0 - c;
    let [x, y, z] = a;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn mirror(n: [f64; 3]) -> M4 {
    let l = libm::sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
    if l < 1e-12 {
        return IDENTITY;
    }
    let [x, y, z] = [n[0] / l, n[1] / l, n[2] / l];
    [
        [1.0 - 2.0 * x * x, -2.0 * x * y, -2.0 * x * z, 0.0],
        [-2.0 * x * y, 1.0 - 2.0 * y * y, -2.0 * y * z, 0.0],
        [-2.0 * x * z, -2.0 * y * z, 1.0 - 2.0 * z * z, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn apply2(m: &M4, p: [f64; 2]) -> [f64; 2] {
    let r = |i: usize| m.get(i).copied().unwrap_or([0.0; 4]);
    let (a, b) = (r(0), r(1));
    [a[0] * p[0] + a[1] * p[1] + a[3], b[0] * p[0] + b[1] * p[1] + b[3]]
}

/// A 2D shape under a planar transform (circles need a similarity).
fn place2(m: &M4, s: Shape2, line: u32) -> R<Shape2> {
    Ok(match s {
        Shape2::Polygon(paths) => {
            Shape2::Polygon(paths.into_iter().map(|p| p.into_iter().map(|q| apply2(m, q)).collect()).collect())
        }
        Shape2::Circle { center, radius } => {
            let r = |i: usize| m.get(i).copied().unwrap_or([0.0; 4]);
            let (a, b) = (r(0), r(1));
            let (sx, sy) = (libm::hypot(a[0], b[0]), libm::hypot(a[1], b[1]));
            let ortho = (a[0] * a[1] + b[0] * b[1]).abs() < 1e-9 * (sx * sy).max(1e-30);
            if (sx - sy).abs() > 1e-9 * sx.max(sy) || !ortho {
                return err(line, "a circle scaled unevenly (an ellipse) is not supported");
            }
            Shape2::Circle { center: apply2(m, center), radius: radius * sx }
        }
    })
}

impl Eval<'_> {
    fn step(&mut self, line: u32) -> R<()> {
        self.steps += 1;
        if self.steps > self.lim.max_steps {
            return err(line, "the program takes too long to evaluate");
        }
        Ok(())
    }

    fn enter(&mut self, line: u32) -> R<()> {
        self.depth += 1;
        if self.depth > self.lim.max_depth {
            return err(line, "recursion is too deep");
        }
        Ok(())
    }

    fn push_scope(&mut self, parent: usize) -> usize {
        self.scopes.push(Scope { parent: Some(parent), ..Scope::default() });
        self.scopes.len() - 1
    }

    fn lookup(&self, mut sc: usize, name: &str) -> Value {
        if name.starts_with('$') {
            return self.dynamic.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_else(|| {
                match name {
                    "$fn" => Value::Num(0.0),
                    "$fa" => Value::Num(12.0),
                    "$fs" => Value::Num(2.0),
                    "$t" => Value::Num(0.0),
                    _ => Value::Undef,
                }
            });
        }
        loop {
            let Some(s) = self.scopes.get(sc) else { return Value::Undef };
            if let Some(v) = s.vars.get(name) {
                return v.clone();
            }
            match s.parent {
                Some(p) => sc = p,
                None => {
                    return match name {
                        "PI" => Value::Num(std::f64::consts::PI),
                        _ => Value::Undef,
                    };
                }
            }
        }
    }

    fn find_mod(&self, mut sc: usize, name: &str) -> Option<ModDef> {
        loop {
            let s = self.scopes.get(sc)?;
            if let Some(m) = s.mods.get(name) {
                return Some(m.clone());
            }
            sc = s.parent?;
        }
    }

    fn find_fn(&self, mut sc: usize, name: &str) -> Option<FnDef> {
        loop {
            let s = self.scopes.get(sc)?;
            if let Some(m) = s.funcs.get(name) {
                return Some(m.clone());
            }
            sc = s.parent?;
        }
    }

    /// Registers definitions and assignments of a statement list in scope `sc`, then
    /// evaluates its objects. Assignments: the last one of a name wins, evaluated where
    /// the name first appears (as OpenSCAD does).
    fn block(&mut self, stmts: &[Stmt], sc: usize) -> R<Vec<Obj>> {
        let dyn_pushed = self.prepare(stmts, sc)?;
        let mut out = Vec::new();
        for s in stmts {
            out.extend(self.stmt(s, sc)?);
        }
        let keep = self.dynamic.len().saturating_sub(dyn_pushed);
        self.dynamic.truncate(keep);
        Ok(out)
    }

    /// The definitions and assignments of a statement list; returns how many `$`
    /// variables it pushed.
    fn prepare(&mut self, stmts: &[Stmt], sc: usize) -> R<usize> {
        let mut dyn_pushed = 0;
        for s in stmts {
            match s {
                Stmt::Module(n, p, b) => {
                    let def = ModDef { params: Rc::new(p.clone()), body: Rc::new(b.clone()), scope: sc };
                    if let Some(scope) = self.scopes.get_mut(sc) {
                        scope.mods.insert(n.clone(), def);
                    }
                }
                Stmt::Function(n, p, e) => {
                    let def = FnDef { params: Rc::new(p.clone()), body: Rc::new(e.clone()), scope: sc };
                    if let Some(scope) = self.scopes.get_mut(sc) {
                        scope.funcs.insert(n.clone(), def);
                    }
                }
                _ => {}
            }
        }
        let mut order: Vec<&str> = Vec::new();
        let mut last: HashMap<&str, (&Expr, u32)> = HashMap::new();
        for s in stmts {
            if let Stmt::Assign(n, e, line) = s {
                if !last.contains_key(n.as_str()) {
                    order.push(n);
                }
                last.insert(n, (e, *line));
            }
        }
        for n in order {
            let Some((e, line)) = last.get(n).copied() else { continue };
            self.step(line)?;
            let v = self.expr(e, sc, line)?;
            if n.starts_with('$') {
                self.dynamic.push((n.to_string(), v));
                dyn_pushed += 1;
            } else if let Some(scope) = self.scopes.get_mut(sc) {
                scope.vars.insert(n.to_string(), v);
            }
        }
        Ok(dyn_pushed)
    }

    fn stmt(&mut self, s: &Stmt, sc: usize) -> R<Vec<Obj>> {
        match s {
            Stmt::Assign(..) | Stmt::Module(..) | Stmt::Function(..) => Ok(Vec::new()),
            Stmt::Uses(path, line) => {
                self.warnings.push(format!("line {line}: include/use <{path}> is not followed"));
                Ok(Vec::new())
            }
            Stmt::Block(b) => {
                let inner = self.push_scope(sc);
                let r = self.block(b, inner);
                self.scopes.truncate(inner);
                r
            }
            Stmt::If(c, a, b) => {
                let v = self.expr(c, sc, 0)?;
                let body = if v.truthy() { a } else { b };
                let inner = self.push_scope(sc);
                let r = self.block(body, inner);
                self.scopes.truncate(inner);
                r
            }
            Stmt::Let(binds, body) => {
                let inner = self.push_scope(sc);
                for (n, e) in binds {
                    let v = self.expr(e, inner, 0)?;
                    if let Some(s) = self.scopes.get_mut(inner) {
                        s.vars.insert(n.clone(), v);
                    }
                }
                let r = self.block(body, inner);
                self.scopes.truncate(inner);
                r
            }
            Stmt::For(binds, body) => {
                let mut out = Vec::new();
                self.for_loop(binds, sc, &mut |me, inner| {
                    out.extend(me.block(body, inner)?);
                    Ok(())
                })?;
                Ok(out)
            }
            Stmt::Inst { name, args, children, line } => self.inst(name, args, children, sc, *line),
        }
    }

    /// Runs `f` for every combination of the `for` bindings, in a fresh scope each time.
    fn for_loop(&mut self, binds: &[(String, Expr)], sc: usize, f: &mut dyn FnMut(&mut Self, usize) -> R<()>) -> R<()> {
        let Some(((name, e), rest)) = binds.split_first() else {
            let inner = self.push_scope(sc);
            let r = f(self, inner);
            self.scopes.truncate(inner);
            return r;
        };
        let v = self.expr(e, sc, 0)?;
        let items = self.iterate(&v, 0)?;
        for v in items {
            self.step(0)?;
            let inner = self.push_scope(sc);
            if let Some(s) = self.scopes.get_mut(inner) {
                s.vars.insert(name.clone(), v);
            }
            let r = self.for_loop(rest, inner, f);
            self.scopes.truncate(inner);
            r?;
        }
        Ok(())
    }

    fn iterate(&mut self, v: &Value, line: u32) -> R<Vec<Value>> {
        Ok(match v {
            Value::Vec(items) => items.as_ref().clone(),
            Value::Range(a, s, b) => {
                if *s == 0.0 || !s.is_finite() || !a.is_finite() || !b.is_finite() {
                    return Ok(Vec::new());
                }
                let n = ((b - a) / s).floor();
                if n < 0.0 {
                    return Ok(Vec::new());
                }
                if n + 1.0 > self.lim.max_vector as f64 {
                    return err(line, "a range is too long");
                }
                (0..=n as usize).map(|i| Value::Num(a + s * i as f64)).collect()
            }
            Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string().into())).collect(),
            Value::Undef => Vec::new(),
            other => vec![other.clone()],
        })
    }

    // ----------------------------------------------------------------- expressions

    fn expr(&mut self, e: &Expr, sc: usize, line: u32) -> R<Value> {
        self.step(line)?;
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Str(s.as_str().into()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Undef => Value::Undef,
            Expr::Var(n) => self.lookup(sc, n),
            Expr::Vec(items) => {
                let mut out = Vec::new();
                for it in items {
                    self.vec_item(it, sc, line, &mut out)?;
                }
                Value::Vec(Rc::new(out))
            }
            Expr::Range(a, s, b) => {
                let a = self.expr(a, sc, line)?.num().unwrap_or(f64::NAN);
                let s = match s {
                    Some(s) => self.expr(s, sc, line)?.num().unwrap_or(f64::NAN),
                    None => 1.0,
                };
                let b = self.expr(b, sc, line)?.num().unwrap_or(f64::NAN);
                Value::Range(a, s, b)
            }
            Expr::Unary(op, a) => {
                let v = self.expr(a, sc, line)?;
                match (*op, v) {
                    ("!", v) => Value::Bool(!v.truthy()),
                    ("-", Value::Num(n)) => Value::Num(-n),
                    ("-", Value::Vec(v)) => Value::Vec(Rc::new(
                        v.iter().map(|x| x.num().map_or(Value::Undef, |n| Value::Num(-n))).collect(),
                    )),
                    ("+", v) => v,
                    _ => Value::Undef,
                }
            }
            Expr::Binary(op, a, b) => {
                if *op == "&&" {
                    return Ok(Value::Bool(self.expr(a, sc, line)?.truthy() && self.expr(b, sc, line)?.truthy()));
                }
                if *op == "||" {
                    return Ok(Value::Bool(self.expr(a, sc, line)?.truthy() || self.expr(b, sc, line)?.truthy()));
                }
                let (x, y) = (self.expr(a, sc, line)?, self.expr(b, sc, line)?);
                binary(op, &x, &y)
            }
            Expr::Cond(c, a, b) => {
                if self.expr(c, sc, line)?.truthy() {
                    self.expr(a, sc, line)?
                } else {
                    self.expr(b, sc, line)?
                }
            }
            Expr::Index(a, i) => {
                let (v, i) = (self.expr(a, sc, line)?, self.expr(i, sc, line)?);
                let k = i.num().filter(|k| *k >= 0.0).map(|k| k as usize);
                match (v, k) {
                    (Value::Vec(v), Some(k)) => v.get(k).cloned().unwrap_or(Value::Undef),
                    (Value::Str(s), Some(k)) => {
                        s.chars().nth(k).map_or(Value::Undef, |c| Value::Str(c.to_string().into()))
                    }
                    (Value::Range(a, s, b), Some(k)) => match k {
                        0 => Value::Num(a),
                        1 => Value::Num(s),
                        2 => Value::Num(b),
                        _ => Value::Undef,
                    },
                    _ => Value::Undef,
                }
            }
            Expr::Member(a, m) => {
                let v = self.expr(a, sc, line)?;
                let k = match m.as_str() {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    _ => return Ok(Value::Undef),
                };
                match v {
                    Value::Vec(v) => v.get(k).cloned().unwrap_or(Value::Undef),
                    _ => Value::Undef,
                }
            }
            Expr::Let(binds, body) => {
                let inner = self.push_scope(sc);
                for (n, e) in binds {
                    let v = self.expr(e, inner, line)?;
                    if let Some(s) = self.scopes.get_mut(inner) {
                        s.vars.insert(n.clone(), v);
                    }
                }
                let r = self.expr(body, inner, line);
                self.scopes.truncate(inner);
                r?
            }
            Expr::Call(name, args) => self.call(name, args, sc, line)?,
        })
    }

    fn vec_item(&mut self, it: &VecItem, sc: usize, line: u32, out: &mut Vec<Value>) -> R<()> {
        if out.len() > self.lim.max_vector {
            return err(line, "a vector is too long");
        }
        match it {
            VecItem::Expr(e) => out.push(self.expr(e, sc, line)?),
            VecItem::Each(e) => {
                let v = self.expr(e, sc, line)?;
                out.extend(self.iterate(&v, line)?);
            }
            VecItem::If(c, a, b) => {
                if self.expr(c, sc, line)?.truthy() {
                    self.vec_item(a, sc, line, out)?;
                } else if let Some(b) = b {
                    self.vec_item(b, sc, line, out)?;
                }
            }
            VecItem::Let(binds, item) => {
                let inner = self.push_scope(sc);
                for (n, e) in binds {
                    let v = self.expr(e, inner, line)?;
                    if let Some(s) = self.scopes.get_mut(inner) {
                        s.vars.insert(n.clone(), v);
                    }
                }
                let r = self.vec_item(item, inner, line, out);
                self.scopes.truncate(inner);
                r?;
            }
            VecItem::For(binds, item) => {
                let mut acc = Vec::new();
                self.for_loop(binds, sc, &mut |me, inner| me.vec_item(item, inner, line, &mut acc))?;
                if out.len() + acc.len() > self.lim.max_vector {
                    return err(line, "a vector is too long");
                }
                out.extend(acc);
            }
        }
        Ok(())
    }

    /// Binds call arguments to parameters in a new scope (child of `def_scope`).
    fn bind(&mut self, params: &[Param], args: &[Arg], caller: usize, def_scope: usize, line: u32) -> R<usize> {
        let mut vals: Vec<(String, Value)> = Vec::new();
        let mut positional = 0;
        for a in args {
            let v = self.expr(&a.value, caller, line)?;
            match &a.name {
                Some(n) => vals.push((n.clone(), v)),
                None => {
                    if let Some(p) = params.get(positional) {
                        vals.push((p.name.clone(), v));
                    }
                    positional += 1;
                }
            }
        }
        let inner = self.push_scope(def_scope);
        for p in params {
            let v = match vals.iter().find(|(n, _)| *n == p.name) {
                Some((_, v)) => v.clone(),
                None => match &p.default {
                    Some(d) => self.expr(d, inner, line)?,
                    None => Value::Undef,
                },
            };
            if let Some(s) = self.scopes.get_mut(inner) {
                s.vars.insert(p.name.clone(), v);
            }
        }
        // `$` arguments are dynamic for the call.
        for (n, v) in vals {
            if n.starts_with('$') {
                self.dynamic.push((n, v));
            }
        }
        Ok(inner)
    }

    fn call(&mut self, name: &str, args: &[Arg], sc: usize, line: u32) -> R<Value> {
        if let Some(f) = self.find_fn(sc, name) {
            self.enter(line)?;
            let dyn_len = self.dynamic.len();
            let inner = self.bind(&f.params, args, sc, f.scope, line)?;
            let r = self.expr(&f.body, inner, line);
            self.scopes.truncate(inner);
            self.dynamic.truncate(dyn_len);
            self.depth -= 1;
            return r;
        }
        let mut vals = Vec::new();
        for a in args {
            vals.push(self.expr(&a.value, sc, line)?);
        }
        builtin_fn(name, &vals, line, &mut self.warnings)
    }

    // ------------------------------------------------------------------ geometry

    fn inst(&mut self, name: &str, args: &[Arg], children: &[Stmt], sc: usize, line: u32) -> R<Vec<Obj>> {
        self.step(line)?;
        if name == "children" {
            return self.children_call(args, sc, line);
        }
        if let Some(m) = self.find_mod(sc, name) {
            self.enter(line)?;
            let dyn_len = self.dynamic.len();
            let inner = self.bind(&m.params, args, sc, m.scope, line)?;
            let saved = self.children.take();
            self.children =
                Some(Rc::new(Children { stmts: Rc::new(children.to_vec()), scope: sc, outer: saved.clone() }));
            let r = self.block(&m.body, inner);
            self.children = saved;
            self.scopes.truncate(inner);
            self.dynamic.truncate(dyn_len);
            self.depth -= 1;
            return Ok(group(r?));
        }
        // Builtins: evaluate the arguments by name and position.
        let mut named: Vec<(Option<String>, Value)> = Vec::new();
        for a in args {
            named.push((a.name.clone(), self.expr(&a.value, sc, line)?));
        }
        let dyn_len = self.dynamic.len();
        for (n, v) in &named {
            if let Some(n) = n
                && n.starts_with('$')
            {
                self.dynamic.push((n.clone(), v.clone()));
            }
        }
        let r = self.builtin_module(name, &named, children, sc, line);
        self.dynamic.truncate(dyn_len);
        r
    }

    fn children_call(&mut self, args: &[Arg], sc: usize, line: u32) -> R<Vec<Obj>> {
        let Some(ch) = self.children.clone() else { return Ok(Vec::new()) };
        let pick = match args.first() {
            Some(a) => Some(self.expr(&a.value, sc, line)?),
            None => None,
        };
        // Children are evaluated in the caller's scope, with the caller's own children.
        let saved = self.children.take();
        self.children = ch.outer.clone();
        let inner = self.push_scope(ch.scope);
        let objs: R<Vec<Vec<Obj>>> = ch.stmts.iter().map(|s| self.stmt(s, inner)).collect();
        self.scopes.truncate(inner);
        self.children = saved;
        let objs = objs?;
        Ok(match pick {
            None => objs.into_iter().flatten().collect(),
            Some(v) => {
                let idx: Vec<usize> = match &v {
                    Value::Num(n) => vec![*n as usize],
                    other => self.iterate(other, line)?.iter().filter_map(Value::num).map(|n| n as usize).collect(),
                };
                idx.into_iter().filter_map(|i| objs.get(i).cloned()).flatten().collect()
            }
        })
    }

    fn arg(&self, named: &[(Option<String>, Value)], name: &str, pos: usize) -> Value {
        named
            .iter()
            .find(|(n, _)| n.as_deref() == Some(name))
            .or_else(|| named.iter().filter(|(n, _)| n.is_none()).nth(pos))
            .map_or(Value::Undef, |(_, v)| v.clone())
    }

    fn kids(&mut self, children: &[Stmt], sc: usize) -> R<Vec<Obj>> {
        let inner = self.push_scope(sc);
        let r = self.block(children, inner);
        self.scopes.truncate(inner);
        r
    }

    fn prim(&mut self, prim: Prim, line: u32) -> R<Vec<Obj>> {
        self.prims += 1;
        if self.prims > self.lim.max_primitives {
            return err(line, "the model has too many primitives");
        }
        Ok(vec![Obj::D3(Csg::Leaf { prim, at: IDENTITY, line })])
    }

    fn builtin_module(
        &mut self,
        name: &str,
        a: &[(Option<String>, Value)],
        children: &[Stmt],
        sc: usize,
        line: u32,
    ) -> R<Vec<Obj>> {
        let num = |v: &Value, d: f64| v.num().unwrap_or(d);
        let vec3 = |v: &Value, d: f64| -> [f64; 3] {
            match v {
                Value::Num(n) => [*n, *n, *n],
                Value::Vec(x) => {
                    let g = |i: usize| x.get(i).and_then(Value::num).unwrap_or(d);
                    [g(0), g(1), g(2)]
                }
                _ => [d, d, d],
            }
        };
        let positive = |v: f64, what: &str| -> R<f64> {
            if v.is_finite() && v > 0.0 { Ok(v) } else { err(line, format!("{name}: {what} must be positive")) }
        };
        match name {
            "cube" => {
                let size = vec3(&self.arg(a, "size", 0), 1.0);
                for s in size {
                    positive(s, "size")?;
                }
                let center = self.arg(a, "center", 1).truthy();
                self.prim(Prim::Cube { size, center }, line)
            }
            "sphere" => {
                let r = match (self.arg(a, "r", 0), self.arg(a, "d", 99)) {
                    (Value::Num(r), _) => r,
                    (_, Value::Num(d)) => d / 2.0,
                    _ => 1.0,
                };
                self.prim(Prim::Sphere { radius: positive(r, "the radius")? }, line)
            }
            "cylinder" => {
                let h = num(&self.arg(a, "h", 0), 1.0);
                let one = |me: &Self, r: &str, d: &str| match (me.arg(a, r, 99), me.arg(a, d, 99)) {
                    (Value::Num(r), _) => Some(r),
                    (_, Value::Num(d)) => Some(d / 2.0),
                    _ => None,
                };
                let r = one(self, "r", "d").or_else(|| self.arg(a, "", 1).num());
                let r1 = one(self, "r1", "d1").or(r).unwrap_or(1.0);
                let r2 = one(self, "r2", "d2").or(r).or_else(|| self.arg(a, "", 2).num()).unwrap_or(1.0);
                let center = self.arg(a, "center", 3).truthy();
                positive(h, "the height")?;
                if !(r1 >= 0.0 && r2 >= 0.0 && r1 + r2 > 0.0 && r1.is_finite() && r2.is_finite()) {
                    return err(line, "cylinder: radii must be non-negative and not both zero");
                }
                self.prim(Prim::Cylinder { h, r1, r2, center }, line)
            }
            "polyhedron" => err(line, "polyhedron is not supported yet"),
            "square" | "circle" | "polygon" => {
                let shape = match name {
                    "square" => {
                        let s = match self.arg(a, "size", 0) {
                            Value::Num(n) => [n, n],
                            Value::Vec(v) => [
                                v.first().and_then(Value::num).unwrap_or(1.0),
                                v.get(1).and_then(Value::num).unwrap_or(1.0),
                            ],
                            _ => [1.0, 1.0],
                        };
                        let (w, h) = (positive(s[0], "size")?, positive(s[1], "size")?);
                        let (x0, y0) =
                            if self.arg(a, "center", 1).truthy() { (-w / 2.0, -h / 2.0) } else { (0.0, 0.0) };
                        Shape2::Polygon(vec![vec![[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]]])
                    }
                    "circle" => {
                        let r = match (self.arg(a, "r", 0), self.arg(a, "d", 99)) {
                            (Value::Num(r), _) => r,
                            (_, Value::Num(d)) => d / 2.0,
                            _ => 1.0,
                        };
                        Shape2::Circle { center: [0.0, 0.0], radius: positive(r, "the radius")? }
                    }
                    _ => {
                        let pts: Vec<[f64; 2]> = match self.arg(a, "points", 0) {
                            Value::Vec(v) => v
                                .iter()
                                .map(|p| match p {
                                    Value::Vec(q) => [
                                        q.first().and_then(Value::num).unwrap_or(f64::NAN),
                                        q.get(1).and_then(Value::num).unwrap_or(f64::NAN),
                                    ],
                                    _ => [f64::NAN, f64::NAN],
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                        if pts.len() < 3 || pts.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
                            return err(line, "polygon: needs at least 3 finite points");
                        }
                        let paths: Vec<Vec<[f64; 2]>> = match self.arg(a, "paths", 1) {
                            Value::Vec(ps) => ps
                                .iter()
                                .map(|p| {
                                    self.iterate_ro(p)
                                        .iter()
                                        .filter_map(|i| i.num())
                                        .filter_map(|i| pts.get(i as usize).copied())
                                        .collect()
                                })
                                .collect(),
                            _ => vec![pts.clone()],
                        };
                        Shape2::Polygon(paths)
                    }
                };
                self.prims += 1;
                Ok(vec![Obj::D2(Csg2::Shape(shape))])
            }
            "translate" | "rotate" | "scale" | "mirror" | "multmatrix" => {
                let m = match name {
                    "translate" => translate(vec3(&self.arg(a, "v", 0), 0.0)),
                    "scale" => scale(vec3(&self.arg(a, "v", 0), 1.0)),
                    "mirror" => mirror(vec3(&self.arg(a, "v", 0), 0.0)),
                    "rotate" => {
                        let ang = self.arg(a, "a", 0);
                        let axis = self.arg(a, "v", 1);
                        match (ang, axis) {
                            (Value::Num(deg), Value::Vec(_)) => {
                                let v = vec3(&self.arg(a, "v", 1), 0.0);
                                let l = libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
                                if l < 1e-12 { IDENTITY } else { rot_axis([v[0] / l, v[1] / l, v[2] / l], deg) }
                            }
                            (Value::Num(deg), _) => rot_axis([0.0, 0.0, 1.0], deg),
                            (v @ Value::Vec(_), _) => {
                                let [x, y, z] = vec3(&v, 0.0);
                                // X, then Y, then Z.
                                mul(
                                    &rot_axis([0.0, 0.0, 1.0], z),
                                    &mul(&rot_axis([0.0, 1.0, 0.0], y), &rot_axis([1.0, 0.0, 0.0], x)),
                                )
                            }
                            _ => IDENTITY,
                        }
                    }
                    _ => {
                        let Value::Vec(rows) = self.arg(a, "m", 0) else {
                            return err(line, "multmatrix: m must be a matrix");
                        };
                        let mut m = IDENTITY;
                        for (i, r) in rows.iter().take(3).enumerate() {
                            if let (Value::Vec(r), Some(row)) = (r, m.get_mut(i)) {
                                for (j, v) in r.iter().take(4).enumerate() {
                                    if let (Some(x), Some(slot)) = (v.num(), row.get_mut(j)) {
                                        *slot = x;
                                    }
                                }
                            }
                        }
                        m
                    }
                };
                if m.iter().flatten().any(|v| !v.is_finite()) {
                    return err(line, format!("{name}: the transform is not finite"));
                }
                let kids = self.kids(children, sc)?;
                kids.into_iter().map(|o| transform(o, &m, line)).collect()
            }
            "union" | "group" | "render" | "color" | "difference" | "intersection" => {
                let kids = self.kids(children, sc)?;
                let op = match name {
                    "difference" => Op::Difference,
                    "intersection" => Op::Intersection,
                    _ => Op::Union,
                };
                if op == Op::Union {
                    return Ok(group(kids));
                }
                combine(op, kids, line)
            }
            "linear_extrude" => {
                let h = positive(num(&self.arg(a, "height", 0), 100.0), "the height")?;
                if self.arg(a, "twist", 99).num().is_some_and(|t| t != 0.0) {
                    return err(line, "linear_extrude: twist is not supported");
                }
                if !matches!(self.arg(a, "scale", 99), Value::Undef | Value::Num(1.0)) {
                    return err(line, "linear_extrude: scale is not supported");
                }
                let center = self.arg(a, "center", 1).truthy();
                let kids = self.kids(children, sc)?;
                let tree = two_d(kids, line)?;
                Ok(tree
                    .map(|t| Obj::D3(extrude(t, &|s| Prim::Prism { shape: s, h, center }, line)))
                    .into_iter()
                    .collect())
            }
            "rotate_extrude" => {
                let angle = num(&self.arg(a, "angle", 99), 360.0);
                if !(angle.is_finite() && angle > 0.0 && angle <= 360.0) {
                    return err(line, "rotate_extrude: the angle must be in (0, 360]");
                }
                let kids = self.kids(children, sc)?;
                let tree = two_d(kids, line)?;
                Ok(tree
                    .map(|t| Obj::D3(extrude(t, &|s| Prim::Revolve { shape: s, angle }, line)))
                    .into_iter()
                    .collect())
            }
            "echo" => {
                let text: Vec<String> =
                    a.iter().map(|(n, v)| n.as_ref().map_or(v.show(), |n| format!("{n} = {}", v.show()))).collect();
                if self.warnings.len() < 100 {
                    self.warnings.push(format!("line {line}: ECHO: {}", text.join(", ")));
                }
                Ok(Vec::new())
            }
            "assert" => {
                if !self.arg(a, "condition", 0).truthy() {
                    return err(line, format!("assertion failed: {}", self.arg(a, "message", 1).show()));
                }
                self.kids(children, sc)
            }
            _ => {
                if self.warnings.len() < 100 {
                    self.warnings.push(format!("line {line}: '{name}' is not supported; skipped"));
                }
                Ok(Vec::new())
            }
        }
    }

    fn iterate_ro(&self, v: &Value) -> Vec<Value> {
        match v {
            Value::Vec(items) => items.as_ref().clone(),
            _ => Vec::new(),
        }
    }
}

/// Several objects as one: a union, or the single object itself.
fn group(objs: Vec<Obj>) -> Vec<Obj> {
    let (d3, d2): (Vec<Obj>, Vec<Obj>) = objs.into_iter().partition(|o| matches!(o, Obj::D3(_)));
    let mut out = Vec::new();
    let d3: Vec<Csg> = d3.into_iter().filter_map(|o| if let Obj::D3(c) = o { Some(c) } else { None }).collect();
    let d2: Vec<Csg2> = d2.into_iter().filter_map(|o| if let Obj::D2(c) = o { Some(c) } else { None }).collect();
    match d3.len() {
        0 => {}
        1 => out.extend(d3.into_iter().map(Obj::D3)),
        _ => out.push(Obj::D3(Csg::Op(Op::Union, d3))),
    }
    match d2.len() {
        0 => {}
        1 => out.extend(d2.into_iter().map(Obj::D2)),
        _ => out.push(Obj::D2(Csg2::Op(Op::Union, d2))),
    }
    out
}

fn combine(op: Op, kids: Vec<Obj>, line: u32) -> R<Vec<Obj>> {
    if kids.is_empty() {
        return Ok(Vec::new());
    }
    let all3 = kids.iter().all(|o| matches!(o, Obj::D3(_)));
    let all2 = kids.iter().all(|o| matches!(o, Obj::D2(_)));
    if all3 {
        let v: Vec<Csg> = kids.into_iter().filter_map(|o| if let Obj::D3(c) = o { Some(c) } else { None }).collect();
        Ok(vec![Obj::D3(Csg::Op(op, v))])
    } else if all2 {
        let v: Vec<Csg2> = kids.into_iter().filter_map(|o| if let Obj::D2(c) = o { Some(c) } else { None }).collect();
        Ok(vec![Obj::D2(Csg2::Op(op, v))])
    } else {
        err(line, "2D and 3D objects are mixed in one operation")
    }
}

fn transform(o: Obj, m: &M4, line: u32) -> R<Obj> {
    Ok(match o {
        Obj::D3(c) => Obj::D3(transform3(c, m)),
        Obj::D2(c) => Obj::D2(transform2(c, m, line)?),
    })
}

fn transform3(c: Csg, m: &M4) -> Csg {
    match c {
        Csg::Leaf { prim, at, line } => Csg::Leaf { prim, at: mul(m, &at), line },
        Csg::Op(op, v) => Csg::Op(op, v.into_iter().map(|c| transform3(c, m)).collect()),
    }
}

fn transform2(c: Csg2, m: &M4, line: u32) -> R<Csg2> {
    Ok(match c {
        Csg2::Shape(s) => Csg2::Shape(place2(m, s, line)?),
        Csg2::Op(op, v) => Csg2::Op(op, v.into_iter().map(|c| transform2(c, m, line)).collect::<R<_>>()?),
    })
}

/// The children of an extrusion as one 2D tree.
fn two_d(kids: Vec<Obj>, line: u32) -> R<Option<Csg2>> {
    let mut v = Vec::new();
    for k in kids {
        match k {
            Obj::D2(c) => v.push(c),
            Obj::D3(_) => return err(line, "an extrusion takes 2D children"),
        }
    }
    Ok(match v.len() {
        0 => None,
        1 => v.pop(),
        _ => Some(Csg2::Op(Op::Union, v)),
    })
}

/// A 2D tree extruded leaf by leaf (extrusion distributes over the booleans).
fn extrude(t: Csg2, f: &dyn Fn(Shape2) -> Prim, line: u32) -> Csg {
    match t {
        Csg2::Shape(s) => Csg::Leaf { prim: f(s), at: IDENTITY, line },
        Csg2::Op(op, v) => Csg::Op(op, v.into_iter().map(|c| extrude(c, f, line)).collect()),
    }
}

fn binary(op: &str, x: &Value, y: &Value) -> Value {
    use Value::*;
    match (op, x, y) {
        ("+", Num(a), Num(b)) => Num(a + b),
        ("-", Num(a), Num(b)) => Num(a - b),
        ("*", Num(a), Num(b)) => Num(a * b),
        ("/", Num(a), Num(b)) => Num(a / b),
        ("%", Num(a), Num(b)) => Num(a % b),
        ("^", Num(a), Num(b)) => Num(libm::pow(*a, *b)),
        ("+" | "-", Vec(a), Vec(b)) => Vec(Rc::new(a.iter().zip(b.iter()).map(|(p, q)| binary(op, p, q)).collect())),
        ("*" | "/", Vec(a), Num(_)) => Vec(Rc::new(a.iter().map(|p| binary(op, p, y)).collect())),
        ("*", Num(_), Vec(b)) => Vec(Rc::new(b.iter().map(|q| binary(op, x, q)).collect())),
        ("*", Vec(a), Vec(b)) => {
            let nums = |v: &[Value]| v.iter().all(|e| matches!(e, Num(_)));
            if nums(a) && nums(b) {
                // Dot product.
                if a.len() != b.len() {
                    return Undef;
                }
                Num(a.iter().zip(b.iter()).filter_map(|(p, q)| Some(p.num()? * q.num()?)).sum())
            } else if nums(b) {
                // Matrix times vector.
                Vec(Rc::new(a.iter().map(|row| binary("*", row, y)).collect()))
            } else {
                Undef
            }
        }
        ("==", a, b) => Bool(a == b),
        ("!=", a, b) => Bool(a != b),
        ("<", Num(a), Num(b)) => Bool(a < b),
        ("<=", Num(a), Num(b)) => Bool(a <= b),
        (">", Num(a), Num(b)) => Bool(a > b),
        (">=", Num(a), Num(b)) => Bool(a >= b),
        ("<", Str(a), Str(b)) => Bool(a < b),
        (">", Str(a), Str(b)) => Bool(a > b),
        _ => Undef,
    }
}

fn builtin_fn(name: &str, v: &[Value], line: u32, warnings: &mut Vec<String>) -> R<Value> {
    use Value::*;
    let n = |i: usize| v.get(i).and_then(Value::num);
    let f1 = |f: fn(f64) -> f64| n(0).map_or(Undef, |x| Num(f(x)));
    let deg = |x: f64| x.to_degrees();
    Ok(match name {
        "sin" => n(0).map_or(Undef, |x| Num(libm::sin(rad(x)))),
        "cos" => n(0).map_or(Undef, |x| Num(libm::cos(rad(x)))),
        "tan" => n(0).map_or(Undef, |x| Num(libm::tan(rad(x)))),
        "asin" => n(0).map_or(Undef, |x| Num(deg(libm::asin(x)))),
        "acos" => n(0).map_or(Undef, |x| Num(deg(libm::acos(x)))),
        "atan" => n(0).map_or(Undef, |x| Num(deg(libm::atan(x)))),
        "atan2" => match (n(0), n(1)) {
            (Some(y), Some(x)) => Num(deg(libm::atan2(y, x))),
            _ => Undef,
        },
        "abs" => f1(f64::abs),
        "ceil" => f1(f64::ceil),
        "floor" => f1(f64::floor),
        "round" => f1(f64::round),
        "sqrt" => f1(f64::sqrt),
        "exp" => f1(libm::exp),
        "ln" => f1(libm::log),
        "log" => match (n(0), n(1)) {
            (Some(b), Some(x)) => Num(libm::log(x) / libm::log(b)),
            (Some(x), None) => Num(libm::log10(x)),
            _ => Undef,
        },
        "pow" => match (n(0), n(1)) {
            (Some(a), Some(b)) => Num(libm::pow(a, b)),
            _ => Undef,
        },
        "sign" => f1(|x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }),
        "min" | "max" => {
            let list: std::vec::Vec<f64> = match v {
                [Vec(items)] => items.iter().filter_map(Value::num).collect(),
                _ => v.iter().filter_map(Value::num).collect(),
            };
            let r = if name == "min" {
                list.iter().copied().reduce(f64::min)
            } else {
                list.iter().copied().reduce(f64::max)
            };
            r.map_or(Undef, Num)
        }
        "norm" => match v.first() {
            Some(Vec(x)) => Num(libm::sqrt(x.iter().filter_map(Value::num).map(|e| e * e).sum())),
            _ => Undef,
        },
        "cross" => match (v.first(), v.get(1)) {
            (Some(Vec(a)), Some(Vec(b))) => {
                let g = |x: &[Value], i: usize| x.get(i).and_then(Value::num).unwrap_or(0.0);
                let (a, b) = ([g(a, 0), g(a, 1), g(a, 2)], [g(b, 0), g(b, 1), g(b, 2)]);
                Vec(Rc::new(
                    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
                        .into_iter()
                        .map(Num)
                        .collect(),
                ))
            }
            _ => Undef,
        },
        "len" => match v.first() {
            Some(Vec(x)) => Num(x.len() as f64),
            Some(Str(s)) => Num(s.chars().count() as f64),
            _ => Undef,
        },
        "concat" => {
            let mut out = std::vec::Vec::new();
            for x in v {
                match x {
                    Vec(items) => out.extend(items.iter().cloned()),
                    other => out.push(other.clone()),
                }
            }
            Vec(Rc::new(out))
        }
        "str" => Str(v.iter().map(Value::show).collect::<String>().into()),
        "chr" => {
            Str(v.iter().filter_map(Value::num).filter_map(|c| char::from_u32(c as u32)).collect::<String>().into())
        }
        "ord" => match v.first() {
            Some(Str(s)) if s.chars().count() == 1 => s.chars().next().map_or(Undef, |c| Num(f64::from(c as u32))),
            _ => Undef,
        },
        "lookup" => match (n(0), v.get(1)) {
            (Some(k), Some(Vec(table))) => {
                let pts: std::vec::Vec<(f64, f64)> = table
                    .iter()
                    .filter_map(|r| match r {
                        Vec(p) => Some((p.first()?.num()?, p.get(1)?.num()?)),
                        _ => None,
                    })
                    .collect();
                match (pts.first(), pts.last()) {
                    (Some(f), _) if k <= f.0 => Num(f.1),
                    (_, Some(l)) if k >= l.0 => Num(l.1),
                    _ => pts
                        .windows(2)
                        .find(|w| matches!(w, [a, b] if k >= a.0 && k <= b.0))
                        .and_then(|w| match w {
                            [a, b] => Some(Num(a.1 + (k - a.0) / (b.0 - a.0) * (b.1 - a.1))),
                            _ => None,
                        })
                        .unwrap_or(Undef),
                }
            }
            _ => Undef,
        },
        "is_undef" => Bool(matches!(v.first(), Some(Undef) | None)),
        "is_num" => Bool(matches!(v.first(), Some(Num(x)) if x.is_finite())),
        "is_bool" => Bool(matches!(v.first(), Some(Bool(_)))),
        "is_string" => Bool(matches!(v.first(), Some(Str(_)))),
        "is_list" => Bool(matches!(v.first(), Some(Vec(_)))),
        "is_function" => Bool(false),
        "version" => Vec(Rc::new(vec![Num(2021.0), Num(1.0), Num(0.0)])),
        "version_num" => Num(20210100.0),
        _ => {
            if warnings.len() < 100 {
                warnings.push(format!("line {line}: function '{name}' is not supported; it gives undef"));
            }
            Undef
        }
    })
}

/// Evaluates a parsed program.
pub(crate) fn evaluate(prog: &[Stmt], lim: &Limits) -> Result<Model, ScadError> {
    let mut e = Eval {
        lim,
        scopes: vec![Scope::default()],
        dynamic: Vec::new(),
        steps: 0,
        depth: 0,
        prims: 0,
        warnings: Vec::new(),
        children: None,
    };
    e.prepare(prog, 0)?;
    // One body per top-level statement (its objects united).
    let mut objects = Vec::new();
    for s in prog {
        for o in group(e.stmt(s, 0)?) {
            match o {
                Obj::D3(c) => objects.push(c),
                Obj::D2(_) => e.warnings.push("a 2D object at the top level is not a solid; skipped".into()),
            }
        }
    }
    Ok(Model { objects, warnings: e.warnings })
}
