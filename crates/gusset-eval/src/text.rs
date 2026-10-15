//! Text features (ADR 0029): glyph outlines from `gusset-text` as a profile on a plane,
//! extruded and combined like an extrude.

use super::*;
use gusset_doc::{TextAlign, TextDef, TextFont};
use gusset_kernel::{Loop2, Profile, ProfileEdge, Region2, Seg2};

/// The persistent tag of segment `seg` of contour `contour` of character `char_index`:
/// the entity `g<char>_<contour + 1>`, segment `_g<seg>`.
fn glyph_entity(char_index: u32, contour: u32) -> String {
    format!("g{char_index}_{}", contour.saturating_add(1).min(0xfff))
}

impl Evaluator {
    pub(crate) fn run_text(
        &self,
        s: &mut State,
        name: &str,
        d: &TextDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let height = eval_expr(name, &d.height, Dim::LENGTH, params, units)?;
        let distance = eval_expr(name, &d.distance, Dim::LENGTH, params, units)?;
        if distance.abs() < 1e-9 {
            return Err(EvalError::new(name, ErrorCode::Expression, "text distance is zero")
                .with_hint("enter a height for the letters, such as 1 mm (negative cuts into a face)"));
        }
        let x = eval_expr(name, &d.at[0], Dim::LENGTH, params, units)?;
        let y = eval_expr(name, &d.at[1], Dim::LENGTH, params, units)?;
        let angle = match &d.angle {
            Some(a) => eval_expr(name, a, Dim::ANGLE, params, units)?,
            None => 0.0,
        };
        let font = match d.font {
            TextFont::Sans => gusset_text::Font::Sans,
            TextFont::Mono => gusset_text::Font::Mono,
        };
        let align = match d.align {
            TextAlign::Left => gusset_text::Align::Left,
            TextAlign::Center => gusset_text::Align::Center,
            TextAlign::Right => gusset_text::Align::Right,
        };
        let glyphs = gusset_text::outline(&d.text, font, height, align).map_err(|e| {
            let hint = match e {
                gusset_text::TextError::Missing(_) => "use letters, digits and common punctuation",
                gusset_text::TextError::BadHeight => "enter the height of capital letters, such as 5 mm",
                _ => "use one line of at most 200 characters",
            };
            EvalError::new(name, ErrorCode::Expression, e.to_string()).with_hint(hint)
        })?;
        if glyphs.is_empty() {
            return Err(EvalError::new(name, ErrorCode::Expression, "the text has no visible characters")
                .with_hint("enter letters or digits"));
        }
        // Turn by `angle` about the baseline start, then move it to `at`.
        let (c, sn) = (libm::cos(angle), libm::sin(angle));
        let place = |p: gusset_text::P2| [x + c * p[0] - sn * p[1], y + sn * p[0] + c * p[1]];
        let edges = |contour: &gusset_text::Contour| -> Result<Loop2, EvalError> {
            let base = gusset_naming::entity_tag(&glyph_entity(contour.char_index, contour.contour))
                .ok_or_else(|| EvalError::new(name, ErrorCode::TooComplex, "the text is too long to name"))?;
            let edges = contour
                .segs
                .iter()
                .enumerate()
                .map(|(i, sg)| {
                    let seg = match *sg {
                        gusset_text::Seg::Line { a, b } => Seg2::Line { a: place(a), b: place(b) },
                        gusset_text::Seg::Cubic { p } => Seg2::Bezier { p: p.map(place) },
                    };
                    ProfileEdge { seg, tag: gusset_naming::with_segment(base, u32::try_from(i).unwrap_or(u32::MAX)) }
                })
                .collect();
            Ok(Loop2 { edges })
        };
        let mut regions = Vec::with_capacity(glyphs.len());
        let mut ids = Vec::with_capacity(glyphs.len());
        for g in &glyphs {
            regions
                .push(Region2 { outer: edges(&g.outer)?, holes: g.holes.iter().map(edges).collect::<Result<_, _>>()? });
            ids.push(glyph_entity(g.outer.char_index, g.outer.contour));
        }
        let frame = plane_frame(name, &d.plane, s)?;
        let profile = Profile { frame, regions };
        let r = k
            .extrude(&profile, &ExtrudeSpec { distance, symmetric: false })
            .map_err(|ke| kernel_err(name, "text extrusion", ke))?;
        let ctx = OpContext::Sweep { feature: name, sketch: name, regions: &ids };
        self.combine(s, name, &ctx, r, d.op, (d.body.as_deref(), d.body_name.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::glyph_entity;

    #[test]
    fn glyph_entities_round_trip_through_tags() {
        let tag = gusset_naming::entity_tag(&glyph_entity(12, 0)).unwrap_or_default();
        let seg = gusset_naming::with_segment(tag, 7);
        assert_eq!(gusset_naming::tag_entity(seg).as_deref(), Some("g12_1_g7"));
    }
}
