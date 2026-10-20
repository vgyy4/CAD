//! The bill of materials: bodies grouped into identical parts. Shared by `query.bom` and
//! the drawing's parts list, so both always agree.

use gusset_eval::Evaluation;
use gusset_kernel::Kernel;

/// One part: the bodies that are copies of it.
#[derive(Debug, Clone, PartialEq)]
pub struct BomRow {
    /// 1-based item number, in body order.
    pub item: usize,
    /// The first body's name.
    pub part: String,
    pub bodies: Vec<String>,
    pub volume: f64,
    pub area: f64,
    pub faces: usize,
}

/// Whether two measures agree to 1e-7 relative (identical parts, placed differently).
fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-7 * a.abs().max(b.abs()).max(1.0)
}

/// The parts of `ev`: bodies with the same volume, area and face and edge counts are one
/// part, wherever they are placed (mirror images too).
///
/// # Errors
/// The kernel cannot measure a body.
pub fn rows(k: &dyn Kernel, ev: &Evaluation) -> Result<Vec<BomRow>, String> {
    let mut rows: Vec<(BomRow, usize)> = Vec::new();
    for b in &ev.bodies {
        let m = k.mass_properties(&b.shape).map_err(|e| e.to_string())?;
        let (faces, edges) = (b.topology.faces.len(), b.topology.edges.len());
        let found = rows
            .iter_mut()
            .find(|(r, e)| r.faces == faces && *e == edges && same(r.volume, m.volume) && same(r.area, m.area));
        match found {
            Some((r, _)) => r.bodies.push(b.name.clone()),
            None => {
                let item = rows.len() + 1;
                let row = BomRow {
                    item,
                    part: b.name.clone(),
                    bodies: vec![b.name.clone()],
                    volume: m.volume,
                    area: m.area,
                    faces,
                };
                rows.push((row, edges));
            }
        }
    }
    Ok(rows.into_iter().map(|(r, _)| r).collect())
}
