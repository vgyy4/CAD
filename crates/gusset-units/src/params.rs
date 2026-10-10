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
}

/// The evaluated value of every parameter, or why it failed.
pub type ParamValues = BTreeMap<String, Result<Quantity, ExprError>>;

/// Evaluates every parameter. Parameters may refer to each other in any order;
/// cycles and references to failing parameters are reported per parameter.
///
/// Bare numbers in a parameter that is otherwise unitless are kept as plain numbers;
/// callers that need a length coerce with [`coerce`].
pub fn evaluate(params: &[Param]) -> ParamValues {
    let defs: BTreeMap<&str, &Param> = params.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut out: ParamValues = BTreeMap::new();
    let mut visiting: Vec<&str> = Vec::new();

    fn visit<'a>(
        name: &'a str,
        defs: &BTreeMap<&'a str, &'a Param>,
        out: &mut ParamValues,
        visiting: &mut Vec<&'a str>,
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
        for v in p.expr.variables() {
            if defs.contains_key(v.as_str()) {
                let key = defs.get_key_value(v.as_str()).map(|(k, _)| *k).unwrap_or_default();
                if let Err(e) = visit(key, defs, out, visiting) {
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
                expr::eval(&p.expr, &scope)
            }
        };
        out.insert(name.to_string(), r.clone());
        r
    }

    for p in params {
        let _ = visit(p.name.as_str(), &defs, &mut out, &mut visiting);
    }
    out
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
        Param { name: name.into(), expr: expr::parse(src).unwrap(), comment: String::new() }
    }

    #[test]
    fn forward_references() {
        let v = evaluate(&[p("depth", "width / 2"), p("width", "40 mm")]);
        assert_eq!(v["depth"].as_ref().unwrap().value, 20.0);
    }

    #[test]
    fn cycles_are_reported() {
        let v = evaluate(&[p("a", "b + 1 mm"), p("b", "a * 2"), p("c", "5 mm")]);
        let e = v["a"].as_ref().unwrap_err();
        assert_eq!(e.kind, ExprErrorKind::Cycle);
        assert!(e.message.contains('→'));
        assert!(v["b"].is_err());
        assert!(v["c"].is_ok());
    }

    #[test]
    fn dependency_failure_names_cause() {
        let v = evaluate(&[p("a", "1 mm + 2"), p("b", "a * 2")]);
        let e = v["b"].as_ref().unwrap_err();
        assert!(e.message.contains("depends on 'a'"), "{}", e.message);
    }

    #[test]
    fn self_reference() {
        let v = evaluate(&[p("a", "a + 1")]);
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
        let v = evaluate(&ps);
        assert_eq!(v["p499"].as_ref().unwrap().value, 500.0);
    }
}
