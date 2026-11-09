//! Inventor parts (`.ipt`): the part's body as binary ShapeManager records.
//!
//! Layout (from samples). The OLE2 file has a storage `RSeStorage` of paired streams: `M<id>`
//! (a small meta stream, "RSe Meta Stream Version") and `B<id>` (the data). A `B` stream is a
//! 16-byte constant, two bytes and one zlib stream (`78 xx`) that fills the rest. The inflated
//! data is a sequence of binary records of the document's database; two of the streams of a
//! part hold a ShapeManager (ACIS) file, recognisable by its magic `ASM BinaryFile4` and
//! read by `gusset-sat` like Fusion's `.smb`: the file is not length-prefixed but ends with its
//! own `End-of-ASM-data` record. The four bytes before a blob's last 4-byte count are a record
//! type: `1d 03 00 00` for the part's result bodies, `57 03 00 00` for the feature history's
//! intermediate bodies (sketch sheets, partial extrusions, the removed material of holes),
//! which must not be imported. Lengths are centimetres in the file header's unit words (10),
//! converted by `gusset-sat`.

use crate::{Limits, OleError, open};
use miniz_oxide::inflate::{TINFLStatus, decompress_to_vec_zlib_with_limit};

/// The ShapeManager blob of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// Binary ACIS (`ASM BinaryFile4 ...`), ready for `gusset_sat::to_step`. It runs to the
    /// end of its stream; the reader stops at the file's own end record.
    pub sab: Vec<u8>,
    /// The stream it came from.
    pub stream: String,
    pub warnings: Vec<String>,
}

/// The record type of a part's result bodies, read little endian just before the count.
const RESULT_TAG: [u8; 4] = [0x1d, 0x03, 0x00, 0x00];
const MAGICS: [&[u8]; 2] = [b"ASM BinaryFile", b"ACIS BinaryFile"];
/// Bytes of a `B` stream before its zlib data.
const B_HEADER: usize = 18;
/// Bytes between a blob's record type and its magic (the 4-byte count).
const BLOB_GAP: usize = 4;

fn is_zlib(b: &[u8]) -> bool {
    match (b.first(), b.get(1)) {
        (Some(&c), Some(&f)) => c & 0x0f == 8 && c >> 4 <= 7 && ((u16::from(c) << 8) | u16::from(f)) % 31 == 0,
        _ => false,
    }
}

/// Where the result blob starts in an inflated stream, if one does.
fn find_blob(data: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while let Some(off) = data.get(i..)?.iter().position(|&b| b == b'A') {
        let at = i + off;
        if let Some(rest) = data.get(at..)
            && MAGICS.iter().any(|m| rest.starts_with(m))
            && at >= BLOB_GAP + RESULT_TAG.len()
            && data.get(at - BLOB_GAP - RESULT_TAG.len()..at - BLOB_GAP) == Some(&RESULT_TAG[..])
        {
            return Some(at);
        }
        i = at + 1;
    }
    None
}

/// The result body blob of an Inventor part (see the module notes).
///
/// # Errors
/// [`OleError`] for other files, damaged or over-limit containers, and files with no result
/// body (an assembly keeps its parts in other files).
pub fn extract_sab(bytes: &[u8], limits: &Limits) -> Result<Extracted, OleError> {
    let mut warnings = Vec::new();
    let f = open(bytes, limits, &mut warnings)?;
    let mut names: Vec<usize> = (0..f.streams.len())
        .filter(|&i| {
            f.streams.get(i).is_some_and(|e| e.name.starts_with('B') && e.name.len() > 8 && e.size() > B_HEADER as u64)
        })
        .collect();
    names.sort_by(|&a, &b| f.streams.get(a).map(|e| &e.name).cmp(&f.streams.get(b).map(|e| &e.name)));
    names.truncate(limits.max_streams);
    let mut spent = 0usize;
    let mut found: Option<(String, Vec<u8>)> = None;
    let mut extra = 0usize;
    for i in names {
        let raw = f.read(i, limits)?;
        let Some(z) = raw.get(B_HEADER..).filter(|z| is_zlib(z)) else { continue };
        let cap = limits.max_stream_bytes.min(z.len().saturating_mul(limits.max_ratio));
        let data = match decompress_to_vec_zlib_with_limit(z, cap) {
            Ok(d) => d,
            Err(e) if e.status == TINFLStatus::HasMoreOutput => {
                return Err(OleError::TooLarge("a stream inflates past the size or ratio limit".into()));
            }
            // A stream that is not zlib after all is not geometry.
            Err(_) => continue,
        };
        spent = spent.saturating_add(data.len());
        if spent > limits.max_total_bytes {
            return Err(OleError::TooLarge("the file's streams exceed the size limit".into()));
        }
        let Some(at) = find_blob(&data) else { continue };
        if found.is_some() {
            extra += 1;
            continue;
        }
        let name = f.streams.get(i).map(|e| e.name.clone()).unwrap_or_default();
        found = Some((name, data.get(at..).unwrap_or_default().to_vec()));
    }
    let Some((stream, sab)) = found else {
        return Err(OleError::NoGeometry("no result body (an assembly keeps its parts in their own files)".into()));
    };
    if extra > 0 {
        warnings.push(format!("{extra} more body blob(s) were not imported (only the first is)"));
    }
    Ok(Extracted { sab, stream, warnings })
}
