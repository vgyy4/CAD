//! `query.draft_analysis`: each face's draft against a pull direction (ADR 0028 amendment),
//! what a mould maker checks before tooling: faces that release (draft at least the angle
//! along or against the pull), faces that would drag (less than the angle), faces mixed.

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<DraftAnalysis>()]
}

/// Draft of every face against a pull direction.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DraftAnalysis {
    /// Default: latest body.
    #[serde(default)]
    pub body: Option<String>,
    /// Pull [x, y, z].
    pub direction: [f64; 3],
    /// Least draft, default "1 deg".
    #[serde(default)]
    pub angle: Option<String>,
    /// Triangles by class.
    #[serde(default)]
    pub field: bool,
}

/// Most faces listed one by one.
const MAX_LISTED: usize = 200;
/// Most triangles in the picture.
const MAX_PICTURE: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Positive,
    Negative,
    Insufficient,
}

impl Class {
    fn of(draft_deg: f64, min: f64) -> Class {
        // 0.001 deg: the tessellation keeps normals in single precision.
        if draft_deg >= min - 1e-3 {
            Class::Positive
        } else if draft_deg <= -min + 1e-3 {
            Class::Negative
        } else {
            Class::Insufficient
        }
    }
    fn name(self) -> &'static str {
        match self {
            Class::Positive => "positive",
            Class::Negative => "negative",
            Class::Insufficient => "insufficient",
        }
    }
}

impl Command for DraftAnalysis {
    const ID: &'static str = "query.draft_analysis";
    const TITLE: &'static str = "Draft analysis";
    const DESCRIPTION: &'static str =
        "Each face's draft (deg) against a pull direction: positive, negative, insufficient or mixed.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let len = libm::sqrt(self.direction.iter().map(|v| v * v).sum());
        if !(len.is_finite() && len > 1e-12) {
            return Err(CommandError::args("direction must be a non-zero vector"));
        }
        let pull = self.direction.map(|v| v / len);
        let min = match &self.angle {
            Some(a) => {
                let e = parse_expr("angle", a)?;
                let v = gusset_eval::expr_value(
                    "angle",
                    &e,
                    gusset_units::Dim::ANGLE,
                    &Default::default(),
                    Default::default(),
                )
                .map_err(|e| CommandError::args(e.to_string()))?
                .to_degrees();
                if !(v.is_finite() && (0.0..90.0).contains(&v)) {
                    return Err(CommandError::args("angle must be from 0 to 90 deg"));
                }
                v
            }
            None => 1.0,
        };
        let ev = s.evaluate(&ctx.actor)?;
        let body = match &self.body {
            Some(n) => ev.body(n).cloned().ok_or_else(|| CommandError::not_found(format!("no body '{n}'")))?,
            None => ev.bodies.last().cloned().ok_or_else(|| CommandError::not_found("the model has no bodies"))?,
        };
        let mesh = s
            .kernel()
            .tessellate(&body.shape, &gusset_kernel::TessellationParams::default())
            .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
        let at = |i: u32| mesh.positions.get(i as usize).map(|p| p.map(f64::from)).unwrap_or([0.0; 3]);
        let mut faces = Vec::new();
        let mut counts = [0usize; 4];
        let mut picture: [Vec<Vec<f64>>; 3] = Default::default();
        for (fi, range) in mesh.face_ranges.iter().enumerate() {
            let (mut lo, mut hi, mut area) = (f64::INFINITY, f64::NEG_INFINITY, 0.0);
            for t in range.clone() {
                let i = 3 * t as usize;
                let Some(&[a, b, c]) = mesh.indices.get(i..i + 3) else { continue };
                let (pa, pb, pc) = (at(a), at(b), at(c));
                let ([ax, ay, az], [bx, by, bz], [cx, cy, cz]) = (pa, pb, pc);
                let (u, v) = ([bx - ax, by - ay, bz - az], [cx - ax, cy - ay, cz - az]);
                let ([ux, uy, uz], [vx, vy, vz]) = (u, v);
                let n = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
                let nl = libm::sqrt(n.iter().map(|x| x * x).sum());
                if nl < 1e-12 {
                    continue;
                }
                // Draft: the angle between the face and the pull direction, positive when
                // the face looks along the pull (it leaves the mould half moving that way).
                let facet = n.map(|x| x / nl);
                let draft_of = |v: [f64; 3]| {
                    libm::asin(v.iter().zip(pull).map(|(x, p)| x * p).sum::<f64>().clamp(-1.0, 1.0)).to_degrees()
                };
                // Curved faces: the surface's own normals at the corners (the tessellation's
                // vertex normals), not the facet's chord normal (dogfooding F12-2b); turned to
                // the facet's side.
                let mut d_sum = 0.0;
                let mut samples = 0.0;
                for k in [a, b, c] {
                    let vn = mesh.normals.get(k as usize).map(|q| q.map(f64::from)).unwrap_or([0.0; 3]);
                    let vl = libm::sqrt(vn.iter().map(|x| x * x).sum());
                    let mut v = if vl > 1e-6 { vn.map(|x| x / vl) } else { facet };
                    if v.iter().zip(facet).map(|(x, y)| x * y).sum::<f64>() < 0.0 {
                        v = v.map(|x| -x);
                    }
                    let dv = draft_of(v);
                    lo = lo.min(dv);
                    hi = hi.max(dv);
                    d_sum += dv;
                    samples += 1.0;
                }
                let d = d_sum / samples;
                area += nl / 2.0;
                if self.field && picture.iter().map(Vec::len).sum::<usize>() < MAX_PICTURE {
                    let k = match Class::of(d, min) {
                        Class::Negative => 0,
                        Class::Insufficient => 1,
                        Class::Positive => 2,
                    };
                    if let Some(list) = picture.get_mut(k) {
                        let r = |x: f64| (x * 100.0).round() / 100.0;
                        list.push([pa, pb, pc, pc].iter().flatten().map(|x| r(*x)).collect());
                    }
                }
            }
            if !lo.is_finite() {
                continue;
            }
            let (cl, ch) = (Class::of(lo, min), Class::of(hi, min));
            let class = if cl == ch { cl.name() } else { "mixed" };
            let slot = match class {
                "positive" => 0,
                "negative" => 1,
                "insufficient" => 2,
                _ => 3,
            };
            if let Some(c) = counts.get_mut(slot) {
                *c += 1;
            }
            if faces.len() < MAX_LISTED {
                let name = body.names.faces.get(fi).map_or_else(|| format!("#{fi}"), ToString::to_string);
                let r = |x: f64| (x * 100.0).round() / 100.0;
                faces.push(json!({"face": name, "class": class, "draft_deg": [r(lo), r(hi)], "area_mm2": r(area)}));
            }
        }
        let mut out = json!({
            "body": body.name,
            "pull": pull,
            "min_draft_deg": min,
            "positive": counts[0],
            "negative": counts[1],
            "insufficient": counts[2],
            "mixed": counts[3],
            "faces": faces,
        });
        if self.field
            && let Some(o) = out.as_object_mut()
        {
            // Three bands for the viewport: negative, insufficient, positive.
            let [neg, ins, pos] = picture;
            o.insert(
                "draft_bands".into(),
                json!([{"class": "negative", "quads": neg}, {"class": "insufficient", "quads": ins}, {"class": "positive", "quads": pos}]),
            );
        }
        Ok(out)
    }
}
