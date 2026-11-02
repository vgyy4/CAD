//! The constraint toolbar of sketch mode: when points and curves of the edited sketch are
//! picked, the constraints that fit them are offered as buttons at the top of the viewport
//! (two lines: parallel, perpendicular, equal, angle; a point and a line: on line, midpoint,
//! distance; ...). A geometric constraint is added at once; a dimension opens the constraint
//! form with its kind and entities filled in, for the value.

use egui::{Pos2, Rect};

/// One offered constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub label: &'static str,
    /// `sketch.add_constraint` kind.
    pub kind: &'static str,
    pub entities: Vec<String>,
    /// A dimension: it needs a value.
    pub value: bool,
}

/// What an id names, from its prefix (`p3` point, `l2` line, `c1` circle, `a4` arc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Point,
    Line,
    Round,
    Other,
}

fn ty(id: &str) -> Ty {
    match id.chars().next() {
        Some('p') => Ty::Point,
        Some('l') => Ty::Line,
        Some('c' | 'a') => Ty::Round,
        _ => Ty::Other,
    }
}

/// The constraints that fit the picked ids, in order of usefulness.
#[must_use]
pub fn suggestions(ids: &[&str]) -> Vec<Suggestion> {
    let s = |label, kind, entities: &[&str], value| Suggestion {
        label,
        kind,
        entities: entities.iter().map(|e| (*e).to_string()).collect(),
        value,
    };
    let tys: Vec<Ty> = ids.iter().map(|i| ty(i)).collect();
    match (tys.as_slice(), ids) {
        ([Ty::Point], [p]) => {
            vec![s("Fix", "fixed", &[p], false), s("At X", "at_x", &[p], true), s("At Y", "at_y", &[p], true)]
        }
        ([Ty::Line], [l]) => vec![
            s("Horizontal", "horizontal", &[l], false),
            s("Vertical", "vertical", &[l], false),
            s("Length", "distance", &[l, l], true),
        ],
        ([Ty::Round], [c]) => vec![s("Radius", "radius", &[c], true), s("Diameter", "diameter", &[c], true)],
        ([Ty::Point, Ty::Point], [a, b]) => vec![
            s("Coincident", "coincident", &[a, b], false),
            s("Distance", "distance", &[a, b], true),
            s("Horizontal distance", "h_distance", &[a, b], true),
            s("Vertical distance", "v_distance", &[a, b], true),
        ],
        ([Ty::Point, Ty::Line], [p, l]) | ([Ty::Line, Ty::Point], [l, p]) => vec![
            s("On line", "on_line", &[p, l], false),
            s("Midpoint", "midpoint", &[p, l], false),
            s("Distance", "distance", &[p, l], true),
        ],
        ([Ty::Point, Ty::Round], [p, c]) | ([Ty::Round, Ty::Point], [c, p]) => {
            vec![s("On circle", "on_circle", &[p, c], false)]
        }
        ([Ty::Line, Ty::Line], [a, b]) => vec![
            s("Parallel", "parallel", &[a, b], false),
            s("Perpendicular", "perpendicular", &[a, b], false),
            s("Equal", "equal", &[a, b], false),
            s("Angle", "angle", &[a, b], true),
        ],
        ([Ty::Line, Ty::Round], [l, c]) | ([Ty::Round, Ty::Line], [c, l]) => {
            vec![s("Tangent", "tangent", &[l, c], false)]
        }
        ([Ty::Round, Ty::Round], [a, b]) => {
            vec![s("Equal", "equal", &[a, b], false), s("Tangent", "tangent", &[a, b], false)]
        }
        ([Ty::Point, Ty::Point, Ty::Line], [a, b, l]) => vec![s("Symmetric", "symmetric", &[a, b, l], false)],
        _ => Vec::new(),
    }
}

/// The toolbar at the top of the viewport `rect`; returns the chosen constraint.
pub fn toolbar(ui: &egui::Ui, rect: Rect, offered: &[Suggestion], pal: &crate::theme::Palette) -> Option<Suggestion> {
    if offered.is_empty() {
        return None;
    }
    let mut chosen = None;
    egui::Area::new(ui.id().with("constrain_bar"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(Pos2::new(rect.center().x, rect.top() + 8.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(pal.bg_raised.gamma_multiply(0.94))
                .stroke(egui::Stroke::new(1.0, pal.border))
                .corner_radius(crate::theme::RADIUS_LG)
                .inner_margin(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("sketch.constrain"));
                        for s in offered {
                            let text = if s.value { format!("{}…", s.label) } else { s.label.to_string() };
                            if ui.button(text).clicked() {
                                chosen = Some(s.clone());
                            }
                        }
                    });
                });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(ids: &[&str]) -> Vec<&'static str> {
        suggestions(ids).iter().map(|s| s.kind).collect()
    }

    #[test]
    fn offers_what_fits_the_picked_items() {
        assert_eq!(kinds(&["l1"]), ["horizontal", "vertical", "distance"]);
        assert_eq!(suggestions(&["l1"])[2].entities, ["l1", "l1"], "a length is the line twice");
        assert_eq!(kinds(&["l1", "l2"]), ["parallel", "perpendicular", "equal", "angle"]);
        // A point and a line in either order: the point goes first.
        assert_eq!(suggestions(&["l4", "p2"])[0].entities, ["p2", "l4"]);
        assert_eq!(kinds(&["a1", "p2"]), ["on_circle"]);
        assert_eq!(kinds(&["c1"]), ["radius", "diameter"]);
        assert_eq!(kinds(&["p1", "p2", "l3"]), ["symmetric"]);
        assert!(kinds(&["p1", "p2", "p3"]).is_empty());
        assert!(kinds(&[]).is_empty());
        assert!(suggestions(&["p1"]).iter().any(|s| s.value));
    }
}
