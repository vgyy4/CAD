//! Full-round fillets (ADR 0004 amendment): a planar face between two parallel planar faces
//! is replaced by a half cylinder across the wall (`Kernel::full_round`).

use super::*;
use gusset_doc::EdgeBlendDef;

impl Evaluator {
    pub(crate) fn run_full_round(&self, s: &mut State, name: &str, b: &EdgeBlendDef) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        // The body that holds the first face (the latest one wins, like edges).
        let first = b.full_round.first();
        let bi = first
            .and_then(|r| s.bodies.iter().rposition(|body| body.names.face_index(r).is_some()))
            .or_else(|| s.bodies.len().checked_sub(1))
            .ok_or_else(|| {
                EvalError::new(name, ErrorCode::NoTargetBody, "there is no body to work on")
                    .with_hint("create a body first, for example with extrude")
            })?;
        let body = s.bodies.get(bi).cloned().ok_or_else(|| EvalError::new(name, ErrorCode::NoTargetBody, "no body"))?;
        let faces = ops::resolve_faces(name, &body, &b.full_round, "re-select the middle face of the full round")?;
        let res = k.full_round(&body.shape, &faces).map_err(|ke| {
            let mut e = kernel_err(name, "full round", ke);
            if e.hint.is_none() {
                e.hint = Some(
                    "pick a planar face that lies between two parallel planar faces (a wall's end) \
                     and is perpendicular to both"
                        .into(),
                );
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        ops::replace_body(s, bi, &body, res.shape, names, topo);
        Ok(())
    }
}
