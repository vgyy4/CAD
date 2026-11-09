//! Solid Edge parts (`.par`, `.psm`): the Parasolid transmit stored in the storage `PARASOLID`.
//!
//! Layout (from samples): the OLE2 file has a storage `PARASOLID` with the streams
//! `STREAM0.X_B` (a complete Parasolid *transmit*: the usual text header
//! `**ABCDEF...**PART1;...**END_OF_HEADER`, then neutral binary), `SKETCH.P_B` (the
//! 2D sketches' partition) and, in some files, `STREAM0.P_B` + `STREAM0.D_B` (the same
//! model as a partition and its deltas, written instead of the `.X_B`). `gusset-xt` reads the
//! `.X_B` and the `.P_B`; the sketch and delta streams are ignored. Solid Edge writes metres
//! like every XT file.

use crate::{Limits, OleError, open};

/// The transmit found in a Solid Edge file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// A Parasolid transmit (binary XT with its header), ready for `gusset_xt::to_step`.
    pub xt: Vec<u8>,
    /// The stream it came from (`STREAM0.X_B`).
    pub stream: String,
    pub warnings: Vec<String>,
}

/// `STREAM<n>.<ext>` -> `(n, ext)` in upper case.
fn stream_kind(name: &str) -> Option<(u32, String)> {
    let up = name.to_ascii_uppercase();
    let rest = up.strip_prefix("STREAM")?;
    let (n, ext) = rest.split_once('.')?;
    if n.is_empty() || n.len() > 6 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((n.parse().ok()?, ext.to_string()))
}

/// The Parasolid transmit of a Solid Edge part: `STREAM<n>.X_B` (lowest `n`), else
/// `STREAM<n>.P_B`.
///
/// # Errors
/// [`OleError`] for other files, damaged or over-limit containers, and files without a
/// Parasolid stream (assemblies keep their parts in other files).
pub fn extract_xt(bytes: &[u8], limits: &Limits) -> Result<Extracted, OleError> {
    let mut warnings = Vec::new();
    let f = open(bytes, limits, &mut warnings)?;
    let mut best: Option<(u8, u32, usize)> = None;
    for (i, e) in f.streams.iter().enumerate() {
        let Some((n, ext)) = stream_kind(&e.name) else { continue };
        let rank = match ext.as_str() {
            "X_B" | "X_T" => 0u8,
            "P_B" | "P_T" => 1,
            _ => continue,
        };
        if best.is_none_or(|(r, m, _)| (rank, n) < (r, m)) {
            best = Some((rank, n, i));
        }
    }
    let Some((_, _, i)) = best else {
        return Err(OleError::NoGeometry(
            "no Parasolid stream (an assembly keeps its parts in their own files)".into(),
        ));
    };
    let xt = f.read(i, limits)?;
    let stream = f.streams.get(i).map(|e| e.name.clone()).unwrap_or_default();
    if !xt.starts_with(b"**") || !contains(xt.get(..4096).unwrap_or(&xt), b"**END_OF_HEADER") {
        return Err(OleError::Malformed(format!("stream '{stream}' is not a Parasolid transmit")));
    }
    Ok(Extracted { xt, stream, warnings })
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
