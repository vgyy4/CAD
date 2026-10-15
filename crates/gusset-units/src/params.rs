//! Named parameters (`width = 40 mm`, `depth = width / 2`) with dependency-ordered
//! evaluation, cycle detection and per-parameter errors.

use crate::expr::{self, Expr, ExprError, ExprErrorKind, Span};
use crate::quantity::{Dim, LengthUnit, Quantity, unit};
use std::collections::BTreeMap;

pub const MAX_NAME_LEN: usize = 64;
/// Longest allowed chain of parameters referring to each other (bounds recursion).
pub const MAX_CHAIN_DEPTH: usize = 1000;

const RESERVED: &[&str] = &[
    "pi", "PI", "e", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "abs", "min", "max", "floor",
    "ceil", "round",
];

/// Checks that `name` can be used as a parameter name.
pub fn validate_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("parameter name is empty".into());
    };
    if name.len() > MAX_NAME_LEN {
        return Err(format!("parameter name is longer than {MAX_NAME_LEN} characters"));
    }
    if !(first.is_ascii_alphabetic() || first == '_') || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("'{name}' is not a valid name: use letters, digits and '_', starting with a letter"));
    }
    if unit(name).is_some() {
        return Err(format!("'{name}' is a unit name and cannot be a parameter"));
    }
    if RESERVED.contains(&name) {
        return Err(format!("'{name}' is a built-in name and cannot be a parameter"));
    }
    Ok(())
}

/// A parameter definition as the user wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub expr: Expr,
    /// Free-text note (untrusted user content).
    pub comment: String,
    /// Allowed values and a slider step (the customizer, ADR 0027).
    pub range: ParamRange,
}

/// `min`, `max` (a value outside fails the parameter) and `step` of a parameter; each an
/// expression in the parameter's unit, which may refer to other parameters.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParamRange {
    pub min: Option<Expr>,
    pub max: Option<Expr>,
    pub step: Option<Expr>,
}

impl ParamRange {
    pub fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none() && self.step.is_none()
    }

    fn exprs(&self) -> impl Iterator<Item = &Expr> {
        [&self.min, &self.max, &self.step].into_iter().flatten()
    }
}

/// The evaluated value of every parameter, or why it failed.
pub type ParamValues = BTreeMap<String, Result<Quantity, ExprError>>;

/// Evaluates every parameter. Parameters may refer to each other in any order;
/// cycles and references to failing parameters are reported per parameter.
///
/// Bare numbers in a parameter that is otherwise unitless are kept as plain numbers;
/// callers that need a length coerce with [`coerce`]. `lengths` is the document's unit,
/// which plain-number range bounds take ([`range_bounds`]).
pub fn evaluate(params: &[Param], lengths: LengthUnit) -> ParamValues {
    let defs: BTreeMap<&str, &Param> = params.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut out: ParamValues = BTreeMap::new();
    let mut visiting: Vec<&str> = Vec::new();

    fn visit<'a>(
        name: &'a str,
        defs: &BTreeMap<&'a str, &'a Param>,
        out: &mut ParamValues,
        visiting: &mut Vec<&'a str>,
        lengths: LengthUnit,
    ) -> Result<Quantity, ExprError> {
        if let Some(r) = out.get(name) {
            return r.clone();
        }
        let Some(p) = defs.get(name) else {
            return Err(ExprError {
                kind: ExprErrorKind::UnknownVariable,
                span: Span::default(),
                message: format!("unknown parameter '{name}'"),
                hint: None,
            });
        };
        if let Some(pos) = visiting.iter().position(|n| *n == name) {
            let mut chain: Vec<&str> = visiting.get(pos..).unwrap_or_default().to_vec();
            chain.push(name);
            return Err(ExprError {
                kind: ExprErrorKind::Cycle,
                span: p.expr.span(),
                message: format!("parameters refer to each other in a loop: {}", chain.join(" → ")),
                hint: Some("break the loop by giving one of them a fixed value".into()),
            });
        }
        if visiting.len() >= MAX_CHAIN_DEPTH {
            return Err(ExprError {
                kind: ExprErrorKind::TooComplex,
                span: p.expr.span(),
                message: format!("parameter chain is deeper than {MAX_CHAIN_DEPTH} levels"),
                hint: Some("simplify how parameters refer to each other".into()),
            });
        }
        visiting.push(name);
        // Resolve dependencies first so their errors are attributed correctly.
        let mut dep_err = None;
        let vars: Vec<String> = std::iter::once(&p.expr).chain(p.range.exprs()).flat_map(Expr::variables).collect();
        for v in vars {
            if defs.contains_key(v.as_str()) {
                let key = defs.get_key_value(v.as_str()).map(|(k, _)| *k).unwrap_or_default();
                if let Err(e) = visit(key, defs, out, visiting, lengths) {
                    dep_err = Some(if e.kind == ExprErrorKind::Cycle {
                        e
                    } else {
                        ExprError {
                            kind: e.kind,
                            span: p.expr.span(),
                            message: format!("depends on '{v}', which failed: {}", e.message),
                            hint: e.hint,
                        }
                    });
                    break;
                }
            }
        }
        visiting.pop();
        let r = match dep_err {
            Some(e) => Err(e),
            None => {
                let scope = |n: &str| out.get(n).and_then(|r| r.as_ref().ok().copied());
                expr::eval(&p.expr, &scope).and_then(|q| check_range(p, q, &scope, lengths).map(|()| q))
            }
        };
        out.insert(name.to_string(), r.clone());
        r
    }

    for p in params {
        let _ = visit(p.name.as_str(), &defs, &mut out, &mut visiting, lengths);
    }
    out
}

/// A parameter's `min`, `max` and `step` in the terms of its value `q` (ADR 0027 §1,
/// review v0.3 M4). A plain-number bound takes the value's unit the way a plain-number
/// value does: a length in the document's unit `lengths`, an angle in degrees. For a
/// plain-number value with a length or angle bound, the bound becomes the plain number
/// that value would be read as.
///
/// # Errors
/// A bound that does not evaluate or has another dimension.
pub fn range_bounds(
    p: &Param,
    q: Quantity,
    scope: &dyn expr::Scope,
    lengths: LengthUnit,
) -> Result<[Option<Quantity>; 3], ExprError> {
    let bound = |e: &Option<Expr>, what: &str| -> Result<Option<Quantity>, ExprError> {
        let Some(e) = e else { return Ok(None) };
        let b = expr::eval(e, scope)?;
        let mismatch = || ExprError {
            kind: ExprErrorKind::DimensionMismatch,
            span: e.span(),
            message: format!("the {what} of '{}' is {}, the value {}", p.name, b.dim.name(), q.dim.name()),
            hint: Some("give the range in the parameter's unit".into()),
        };
        if b.dim == q.dim {
            Ok(Some(b))
        } else if b.dim.is_none() {
            coerce(b, q.dim, lengths).map(Some).map_err(|_| mismatch())
        } else if q.dim.is_none() {
            let one = coerce(Quantity::number(1.0), b.dim, lengths).map_err(|_| mismatch())?;
            Ok(Some(Quantity::number(b.value / one.value)))
        } else {
            Err(mismatch())
        }
    };
    Ok([bound(&p.range.min, "min")?, bound(&p.range.max, "max")?, bound(&p.range.step, "step")?])
}

/// Fails a value outside the parameter's `min` / `max`, and a range that is no range
/// (`min > max`, `step <= 0`; dogfooding F4-8). `step` is the slider's step, not enforced.
fn check_range(p: &Param, q: Quantity, scope: &dyn expr::Scope, lengths: LengthUnit) -> Result<(), ExprError> {
    let [lo, hi, step] = range_bounds(p, q, scope, lengths)?;
    let bad = |message: String, hint: &str| ExprError {
        kind: ExprErrorKind::Domain,
        span: p.expr.span(),
        message,
        hint: Some(hint.into()),
    };
    if let (Some(l), Some(h)) = (lo, hi)
        && l.value > h.value
    {
        return Err(bad(format!("the range of '{}' has min above max", p.name), "swap min and max"));
    }
    if step.is_some_and(|s| s.value <= 0.0 || s.value.is_nan()) {
        return Err(bad(format!("the step of '{}' must be positive", p.name), "remove the step or make it positive"));
    }
    let tol = 1e-9 * q.value.abs().max(1.0);
    let outside = lo.is_some_and(|l| q.value < l.value - tol) || hi.is_some_and(|h| q.value > h.value + tol);
    if outside {
        let show = |b: &Option<Expr>| b.as_ref().map_or("…".to_string(), ToString::to_string);
        return Err(ExprError {
            kind: ExprErrorKind::Domain,
            span: p.expr.span(),
            message: format!("'{}' is outside its range ({} to {})", p.name, show(&p.range.min), show(&p.range.max)),
            hint: Some("choose a value between min and max, or widen the range".into()),
        });
    }
    Ok(())
}

/// Interprets a result where `expected` is required. A plain number is taken in the
/// document's default unit (length → `lengths`, angle → degrees), the way users of
/// every mainstream CAD tool expect.
pub fn coerce(q: Quantity, expected: Dim, lengths: LengthUnit) -> Result<Quantity, String> {
    if q.dim == expected {
        return Ok(q);
    }
    if q.dim.is_none() {
        if expected == Dim::LENGTH {
            return Ok(Quantity::new(q.value * lengths.unit().factor, Dim::LENGTH));
        }
        if expected == Dim::ANGLE {
            return Ok(Quantity::deg(q.value));
        }
    }
    Err(format!("expected {}, got {}", expected.name(), q.dim.name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, src: &str) -> Param {
        Param { name: name.into(), expr: expr::parse(src).unwrap(), comment: String::new(), range: Default::default() }
    }

    fn ranged(name: &str, src: &str, min: Option<&str>, max: Option<&str>) -> Param {
        let e = |x: Option<&str>| x.map(|x| expr::parse(x).unwrap());
        Param { range: ParamRange { min: e(min), max: e(max), step: None }, ..p(name, src) }
    }

    #[test]
    fn plain_number_bounds_take_the_document_unit_and_degrees() {
        // Review v0.3 M4: they were read as mm and radians.
        let ok = |p: Param, u| evaluate(&[p], u).into_values().next().unwrap();
        assert!(ok(ranged("a", "2 in", Some("1"), Some("3")), LengthUnit::Inch).is_ok());
        assert!(ok(ranged("a", "4 in", Some("1"), Some("3")), LengthUnit::Inch).is_err());
        assert!(ok(ranged("a", "45 deg", Some("10"), Some("90")), LengthUnit::Millimetre).is_ok());
        let e = ok(ranged("a", "100 deg", None, Some("90")), LengthUnit::Millimetre).unwrap_err();
        assert!(e.message.contains("outside its range (… to 90)"), "{e:?}");
        // A plain-number value with a length bound: the value is read in the document unit.
        assert!(ok(ranged("n", "40", Some("10 mm"), None), LengthUnit::Millimetre).is_ok());
        assert!(ok(ranged("n", "5", Some("10 mm"), None), LengthUnit::Millimetre).is_err());
        assert!(ok(ranged("n", "1", Some("10 mm"), None), LengthUnit::Inch).is_ok());
        // Another dimension still is a mismatch.
        assert!(ok(ranged("a", "5 mm", Some("1 deg"), None), LengthUnit::Millimetre).is_err());
    }

    #[test]
    fn a_range_must_be_a_range() {
        // Dogfooding F4-8: min above max and a zero step were accepted.
        let ok = |p: Param| evaluate(&[p], LengthUnit::Millimetre).into_values().next().unwrap();
        let e = ok(ranged("w", "15 mm", Some("20 mm"), Some("10 mm"))).unwrap_err();
        assert!(e.message.contains("min above max"), "{e:?}");
        let mut q = ranged("w", "15 mm", Some("10 mm"), Some("20 mm"));
        q.range.step = Some(expr::parse("0 mm").unwrap());
        assert!(ok(q.clone()).unwrap_err().message.contains("step"));
        q.range.step = Some(expr::parse("5 mm").unwrap());
        assert!(ok(q).is_ok());
    }

    #[test]
    fn forward_references() {
        let v = evaluate(&[p("depth", "width / 2"), p("width", "40 mm")], LengthUnit::Millimetre);
        assert_eq!(v["depth"].as_ref().unwrap().value, 20.0);
    }

    #[test]
    fn cycles_are_reported() {
        let v = evaluate(&[p("a", "b + 1 mm"), p("b", "a * 2"), p("c", "5 mm")], LengthUnit::Millimetre);
        let e = v["a"].as_ref().unwrap_err();
        assert_eq!(e.kind, ExprErrorKind::Cycle);
        assert!(e.message.contains('→'));
        assert!(v["b"].is_err());
        assert!(v["c"].is_ok());
    }

    #[test]
    fn dependency_failure_names_cause() {
        let v = evaluate(&[p("a", "1 mm + 2"), p("b", "a * 2")], LengthUnit::Millimetre);
        let e = v["b"].as_ref().unwrap_err();
        assert!(e.message.contains("depends on 'a'"), "{}", e.message);
    }

    #[test]
    fn self_reference() {
        let v = evaluate(&[p("a", "a + 1")], LengthUnit::Millimetre);
        assert_eq!(v["a"].as_ref().unwrap_err().kind, ExprErrorKind::Cycle);
    }

    #[test]
    fn names() {
        assert!(validate_name("width").is_ok());
        assert!(validate_name("_w2").is_ok());
        assert!(validate_name("mm").is_err());
        assert!(validate_name("sin").is_err());
        assert!(validate_name("2w").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("a b").is_err());
    }

    #[test]
    fn coercion() {
        let q = coerce(Quantity::number(2.0), Dim::LENGTH, LengthUnit::Inch).unwrap();
        assert_eq!(q.value, 50.8);
        assert!(coerce(Quantity::mm(1.0), Dim::ANGLE, LengthUnit::Millimetre).is_err());
    }

    #[test]
    fn deep_chain_is_fine() {
        // Long chains must not blow the stack for realistic sizes.
        let mut ps = vec![p("p0", "1 mm")];
        for i in 1..500 {
            ps.push(p(&format!("p{i}"), &format!("p{} + 1 mm", i - 1)));
        }
        let v = evaluate(&ps, LengthUnit::Millimetre);
        assert_eq!(v["p499"].as_ref().unwrap().value, 500.0);
    }
}
