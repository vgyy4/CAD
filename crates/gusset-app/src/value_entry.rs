//! Typing dimensions in the viewport while drawing (DESIGN.md §1.7): after a line's,
//! rectangle's or circle's first click, typing a number opens fields at the cursor: a line's
//! length (and optional angle), a rectangle's width and height, a circle's radius. Values
//! are expressions with units (`width/2 + 3 mm`, a bare number is mm); Tab moves to the next
//! field, Enter places the point, Esc closes the fields. The cursor still gives the
//! direction: a line points toward it, a rectangle opens toward its quadrant.

use gusset_units::expr::{Scope, eval_str};
use gusset_units::params::ParamValues;
use gusset_units::quantity::{Dim, Quantity};

/// The fields being typed for one placement.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub command: &'static str,
    /// (label, text) per field.
    pub fields: Vec<(&'static str, String)>,
}

/// The fields a command takes, if it takes typed values.
#[must_use]
pub fn fields(command: &str) -> Option<&'static [&'static str]> {
    match command {
        "sketch.add_line" => Some(&["Length", "Angle"]),
        "sketch.add_rectangle" => Some(&["Width", "Height"]),
        "sketch.add_circle" => Some(&["Radius"]),
        _ => None,
    }
}

/// An entry started by typing `first` (the first field holds it).
#[must_use]
pub fn start(command: &'static str, first: &str) -> Option<Entry> {
    let labels = fields(command)?;
    let fields =
        labels.iter().enumerate().map(|(i, l)| (*l, if i == 0 { first.to_string() } else { String::new() })).collect();
    Some(Entry { command, fields })
}

/// Whether typed text starts a value (a digit, a sign or a decimal point).
#[must_use]
pub fn starts_value(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
}

struct Params<'a>(&'a ParamValues);

impl Scope for Params<'_> {
    fn lookup(&self, name: &str) -> Option<Quantity> {
        self.0.get(name).and_then(|r| r.as_ref().ok()).copied()
    }
}

/// A typed value in mm (or degrees for `angle`): an expression of the given dimension, or a
/// bare number.
fn value(src: &str, params: &ParamValues, angle: bool) -> Result<f64, String> {
    let q = eval_str(src.trim(), &Params(params)).map_err(|e| e.to_string())?;
    let want = if angle { Dim::ANGLE } else { Dim::LENGTH };
    let v = if q.dim == Dim::NONE {
        if angle { q.value.to_radians() } else { q.value }
    } else if q.dim == want {
        q.value
    } else {
        return Err(format!("'{}' is {}, not {}", src.trim(), q.dim.name(), want.name()));
    };
    if v.is_finite() { Ok(v) } else { Err("not a finite value".into()) }
}

/// The point the typed values place, from the first click and the cursor (sketch mm).
///
/// # Errors
/// A message for an empty, malformed or wrong-kind value, or a zero size.
pub fn point(e: &Entry, first: [f64; 2], cursor: Option<[f64; 2]>, params: &ParamValues) -> Result<[f64; 2], String> {
    let text = |i: usize| e.fields.get(i).map(|(_, t)| t.as_str()).unwrap_or("");
    let towards = cursor.map(|c| [c[0] - first[0], c[1] - first[1]]).unwrap_or([1.0, 0.0]);
    let at = match e.command {
        "sketch.add_line" => {
            let len = value(text(0), params, false)?;
            let dir = if text(1).trim().is_empty() {
                let n = towards[0].hypot(towards[1]);
                if n > 1e-12 { [towards[0] / n, towards[1] / n] } else { [1.0, 0.0] }
            } else {
                let a = value(text(1), params, true)?;
                [a.cos(), a.sin()]
            };
            [first[0] + len * dir[0], first[1] + len * dir[1]]
        }
        "sketch.add_rectangle" => {
            let (w, h) = (value(text(0), params, false)?, value(text(1), params, false)?);
            let sx = if towards[0] < 0.0 { -1.0 } else { 1.0 };
            let sy = if towards[1] < 0.0 { -1.0 } else { 1.0 };
            [first[0] + sx * w.abs(), first[1] + sy * h.abs()]
        }
        "sketch.add_circle" => [first[0] + value(text(0), params, false)?.abs(), first[1]],
        other => return Err(format!("{other} takes no typed values")),
    };
    if (at[0] - first[0]).abs() < 1e-9 && (at[1] - first[1]).abs() < 1e-9 {
        return Err("the size is zero".into());
    }
    Ok(at)
}

/// What the user did with the fields this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Typing,
    Commit,
    Cancel,
}

/// The fields at `at` (screen), the first one focused when the entry starts.
pub fn show(ctx: &egui::Context, at: egui::Pos2, e: &mut Entry, error: Option<&str>) -> Outcome {
    let mut out = Outcome::Typing;
    egui::Area::new(egui::Id::new("value_entry"))
        .order(egui::Order::Foreground)
        .fixed_pos(at + egui::vec2(16.0, 16.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                egui::Grid::new("value_entry_grid").num_columns(2).show(ui, |ui| {
                    for (i, (label, text)) in e.fields.iter_mut().enumerate() {
                        ui.label(*label);
                        let r = crate::theme::text_field(
                            ui,
                            egui::TextEdit::singleline(text).desired_width(110.0).id_salt(("value_entry", i)),
                        );
                        if i == 0 && !ctx.memory(|m| m.focused().is_some()) {
                            r.request_focus();
                        }
                        if r.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                            out = Outcome::Commit;
                        }
                        ui.end_row();
                    }
                });
                if let Some(msg) = error {
                    ui.colored_label(ui.visuals().error_fg_color, msg);
                }
            });
        });
    // Enter places the point whichever field has focus (or none yet).
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        out = Outcome::Commit;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        out = Outcome::Cancel;
    }
    out
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use gusset_units::quantity::Quantity;

    fn params() -> ParamValues {
        [("width".to_string(), Ok(Quantity::mm(40.0)))].into_iter().collect()
    }

    #[test]
    fn typed_values_place_points_toward_the_cursor() {
        let p = params();
        let mut e = start("sketch.add_line", "10").unwrap();
        // Toward the cursor (straight up): 10 mm up.
        assert_eq!(point(&e, [0.0, 0.0], Some([0.0, 5.0]), &p).unwrap(), [0.0, 10.0]);
        // An angle overrides the cursor; expressions with parameters and units work.
        e.fields[0].1 = "width / 2".into();
        e.fields[1].1 = "90 deg".into();
        let at = point(&e, [1.0, 1.0], Some([9.0, 1.0]), &p).unwrap();
        assert!((at[0] - 1.0).abs() < 1e-9 && (at[1] - 21.0).abs() < 1e-9, "{at:?}");
        // A rectangle opens toward the cursor's quadrant.
        let mut r = start("sketch.add_rectangle", "20").unwrap();
        r.fields[1].1 = "1 cm".into();
        assert_eq!(point(&r, [0.0, 0.0], Some([-3.0, -3.0]), &p).unwrap(), [-20.0, -10.0]);
        let c = start("sketch.add_circle", "5").unwrap();
        assert_eq!(point(&c, [2.0, 2.0], None, &p).unwrap(), [7.0, 2.0]);
    }

    #[test]
    fn bad_values_are_refused() {
        let p = params();
        for (cmd, a, b) in [
            ("sketch.add_line", "", ""),
            ("sketch.add_line", "3 deg", ""),
            ("sketch.add_line", "0", ""),
            ("sketch.add_rectangle", "5", ""),
        ] {
            let mut e = start(cmd, a).unwrap();
            if let Some(f) = e.fields.get_mut(1) {
                f.1 = b.into();
            }
            assert!(point(&e, [0.0, 0.0], Some([1.0, 1.0]), &p).is_err(), "{cmd} {a:?} {b:?}");
        }
        assert!(start("sketch.add_arc", "1").is_none());
        assert!(starts_value("12") && starts_value("-3") && !starts_value("x"));
    }
}
