//! Components (ADR 0026 §7): a body of another document, rebuilt from the pinned copy of
//! that document stored in this project.

use super::*;
use gusset_units::expr::Expr;
use std::cell::Cell;

/// How deep components may nest (a component of a component of ...).
const MAX_DEPTH: u32 = 8;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// The literal a `set:` expression gives the component: its value in base units (mm,
/// radians, plain numbers), evaluated with this document's parameters.
fn override_value(feature: &str, e: &Expr, params: &ParamValues) -> Result<Expr, EvalError> {
    let scope = |n: &str| params.get(n).and_then(|r| r.as_ref().ok().copied());
    let bad = |m: String| EvalError::new(feature, ErrorCode::Expression, m);
    let q = gusset_units::expr::eval(e, &scope).map_err(|err| bad(format!("'{e}': {}", err.message)))?;
    let unit = if q.dim == Dim::LENGTH {
        " mm"
    } else if q.dim == Dim::ANGLE {
        " rad"
    } else if q.dim == Dim::NONE {
        ""
    } else {
        return Err(bad(format!("'{e}': only lengths, angles and plain numbers can be set")));
    };
    let text = format!("{}{unit}", gusset_units::quantity::format_exact(q.value));
    gusset_units::expr::parse(&text).map_err(|err| bad(format!("'{e}': {}", err.message)))
}

impl Evaluator {
    pub(crate) fn run_component(
        &self,
        s: &mut State,
        name: &str,
        d: &gusset_doc::ComponentDef,
        params: &ParamValues,
        blobs: &dyn BlobSource,
    ) -> Result<(), EvalError> {
        let bytes = blobs.blob(&d.document).ok_or_else(|| {
            EvalError::new(name, ErrorCode::MissingBlob, "the component's document is missing from the project")
                .with_hint("insert the component again")
        })?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| EvalError::new(name, ErrorCode::ComponentFailed, "the component's document is not text"))?;
        let mut doc = Document::from_text(text).map_err(|e| {
            EvalError::new(name, ErrorCode::ComponentFailed, format!("the component's document does not read: {e}"))
        })?;
        for (p, e) in &d.set {
            let value = override_value(name, e, params)?;
            let names: Vec<String> = doc.params.iter().map(|q| q.name.clone()).collect();
            let slot = doc.params.iter_mut().find(|q| q.name == *p).ok_or_else(|| {
                EvalError::new(name, ErrorCode::InvalidReference, format!("the component has no parameter '{p}'"))
                    .with_hint(format!("its parameters: {}", names.join(", ")))
            })?;
            slot.expr = value;
        }
        let depth = DEPTH.with(Cell::get);
        if depth >= MAX_DEPTH {
            return Err(EvalError::new(
                name,
                ErrorCode::TooComplex,
                format!("components are nested more than {MAX_DEPTH} deep"),
            ));
        }
        // The component's own document, on the same kernel, stopped by the same flag.
        let mut inner = Evaluator::new(self.kernel.clone());
        inner.cancel = self.cancel.clone();
        inner.limits = self.limits;
        inner.set_parallel(1);
        DEPTH.with(|c| c.set(depth + 1));
        let ev = inner.evaluate(&doc, None, blobs);
        DEPTH.with(|c| c.set(depth));
        if ev.cancelled {
            return Err(EvalError::new(name, ErrorCode::Cancelled, "the rebuild was cancelled"));
        }
        if let Some(e) = ev.errors().first() {
            return Err(EvalError::new(
                name,
                ErrorCode::ComponentFailed,
                format!("the component's document does not rebuild: {e}"),
            )
            .with_hint("fix it in its own document, then update the component"));
        }
        let body = match &d.body {
            Some(b) => ev.body(b),
            None => ev.bodies.last(),
        }
        .ok_or_else(|| {
            let names: Vec<&str> = ev.bodies.iter().map(|b| b.name.as_str()).collect();
            let what = d.body.as_deref().map_or_else(|| "has no bodies".to_string(), |b| format!("has no body '{b}'"));
            EvalError::new(name, ErrorCode::InvalidReference, format!("the component's document {what}"))
                .with_hint(format!("its bodies: {}", names.join(", ")))
        })?;
        s.bodies.push(Arc::new(Body {
            name: name.to_string(),
            feature: name.to_string(),
            shape: body.shape.clone(),
            names: gusset_naming::part_names(name, &body.names),
            topology: body.topology.clone(),
            holes: body.holes.clone(),
        }));
        Ok(())
    }
}
