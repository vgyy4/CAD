//! The hardened ZIP reader and deterministic writer of the 3MF code, for other ZIP-based
//! formats (FreeCAD `.FCStd`, ADR 0033). Entry names are checked (no absolute paths,
//! `..` or control characters), sizes and compression ratios are capped by [`Limits`].

use crate::zip::{Archive, write_zip};
use crate::{Limits, Result};

/// The entries of the ZIP `data` whose names `want` accepts, decompressed, in archive
/// order. Total decompressed bytes are capped by `limits.max_decompressed_bytes`.
///
/// # Errors
/// Not a ZIP, unsafe names, too many entries, encrypted or oversized entries.
pub fn read_entries(data: &[u8], limits: &Limits, want: &dyn Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>> {
    let mut budget = limits.max_decompressed_bytes;
    read_entries_within(data, limits, want, u64::MAX, &mut budget)
}

/// [`read_entries`] with a cap on each entry's declared size, checked before inflating,
/// and a decompressed-bytes `budget` shared across calls (security review X4).
///
/// # Errors
/// As [`read_entries`]; an entry over `max_entry` bytes.
pub fn read_entries_within(
    data: &[u8],
    limits: &Limits,
    want: &dyn Fn(&str) -> bool,
    max_entry: u64,
    budget: &mut u64,
) -> Result<Vec<(String, Vec<u8>)>> {
    let a = Archive::parse(data, limits)?;
    let mut out = Vec::new();
    for e in a.all() {
        if want(&e.name) {
            if e.usize > max_entry {
                return Err(crate::error::limit("entry size (bytes)", e.usize, max_entry));
            }
            out.push((e.name.clone(), a.read(e, budget, limits)?));
        }
    }
    Ok(out)
}

/// A deterministic ZIP of `entries` (tests and exporters).
///
/// # Errors
/// An entry over 4 GiB.
pub fn write(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_zip(entries, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_filter() {
        let z = write(&[("Document.xml", b"<Document/>"), ("PartShape.brp", b"CASCADE Topology V1")]).unwrap();
        let got = read_entries(&z, &Limits::default(), &|n| n.ends_with(".brp")).unwrap();
        assert_eq!(got, vec![("PartShape.brp".to_string(), b"CASCADE Topology V1".to_vec())]);
        assert!(read_entries(b"not a zip at all, sorry", &Limits::default(), &|_| true).is_err());
    }
}
