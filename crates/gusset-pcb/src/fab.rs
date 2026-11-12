//! Assembly outputs: the pick-and-place (component position) file and the bill of
//! materials, as CSV.
//!
//! Positions are in mm from the board's auxiliary origin (`aux_axis_origin`, else the
//! file's origin) with **Y up**, as assembly houses expect; rotation in degrees
//! counter-clockwise; side `top` or `bottom`. Footprints marked `exclude_from_pos_files`
//! or `board_only` are left out of the positions, `exclude_from_bom` or `board_only` out
//! of the BOM. Text fields are quoted, and a leading `=`, `+`, `-` or `@` is escaped so a
//! spreadsheet does not read a value as a formula.

use crate::board::Board;
use std::cmp::Ordering;
use std::fmt::Write as _;

/// A CSV field: quoted when needed, formula-looking text prefixed with `'`.
pub fn csv_field(t: &str) -> String {
    let t: String = t.chars().filter(|c| !c.is_control()).collect();
    let t = if t.starts_with(['=', '+', '-', '@']) { format!("'{t}") } else { t };
    if t.contains([',', '"', ' ', ';']) || t.is_empty() { format!("\"{}\"", t.replace('"', "\"\"")) } else { t }
}

fn num(v: f64) -> String {
    let v = (v * 10_000.0).round() / 10_000.0;
    let s = format!("{v:.4}");
    if s == "-0.0000" { "0.0000".into() } else { s }
}

/// References in natural order: `R2` before `R10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let split = |s: &str| -> (String, u64, String) {
        let p: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
        let rest = s.get(p.len()..).unwrap_or_default();
        let d: String = rest.chars().take_while(char::is_ascii_digit).take(18).collect();
        let tail = rest.get(d.len()..).unwrap_or_default().to_string();
        (p, d.parse().unwrap_or(0), tail)
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

fn has(attrs: &[String], a: &str) -> bool {
    attrs.iter().any(|x| x == a)
}

/// The pick-and-place CSV: `Ref,Val,Package,PosX,PosY,Rot,Side`.
pub fn positions(board: &Board) -> (String, usize) {
    let mut rows: Vec<&crate::board::Footprint> = board
        .footprints
        .iter()
        .filter(|f| !has(&f.attrs, "exclude_from_pos_files") && !has(&f.attrs, "board_only"))
        .collect();
    rows.sort_by(|a, b| natural_cmp(&a.reference, &b.reference));
    let mut s = String::from("Ref,Val,Package,PosX,PosY,Rot,Side\n");
    let o = board.aux_origin;
    for f in &rows {
        let package = f.lib_id.rsplit_once(':').map_or(f.lib_id.as_str(), |x| x.1);
        let rot = f.angle.rem_euclid(360.0);
        let _ = writeln!(
            s,
            "{},{},{},{},{},{},{}",
            csv_field(&f.reference),
            csv_field(&f.value),
            csv_field(package),
            num(f.at[0] - o[0]),
            num(-(f.at[1] - o[1])),
            num(rot),
            if f.bottom { "bottom" } else { "top" }
        );
    }
    (s, rows.len())
}

/// The BOM CSV, one line per value and footprint: `Qty,Value,Footprint,References,DNP`.
pub fn bom(board: &Board) -> (String, usize) {
    let mut groups: std::collections::BTreeMap<(String, String, bool), Vec<String>> = std::collections::BTreeMap::new();
    for f in board.footprints.iter().filter(|f| !has(&f.attrs, "exclude_from_bom") && !has(&f.attrs, "board_only")) {
        groups.entry((f.value.clone(), f.lib_id.clone(), has(&f.attrs, "dnp"))).or_default().push(f.reference.clone());
    }
    let mut rows: Vec<_> = groups.into_iter().collect();
    for (_, refs) in &mut rows {
        refs.sort_by(|a, b| natural_cmp(a, b));
    }
    rows.sort_by(|a, b| natural_cmp(a.1.first().map_or("", String::as_str), b.1.first().map_or("", String::as_str)));
    let mut s = String::from("Qty,Value,Footprint,References,DNP\n");
    for ((value, fp, dnp), refs) in &rows {
        let _ = writeln!(
            s,
            "{},{},{},{},{}",
            refs.len(),
            csv_field(value),
            csv_field(fp),
            csv_field(&refs.join(" ")),
            if *dnp { "yes" } else { "" }
        );
    }
    (s, rows.len())
}
