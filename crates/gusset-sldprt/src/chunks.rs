//! The chunk container of newer SolidWorks files (about 2015 on): a sequence of stream
//! records, each a 10-byte marker, a CRC-32, the deflated and inflated sizes, a
//! nibble-swapped name and the raw-deflate data. Records are found by scanning for the
//! marker and *validated* (size, inflate, length, CRC), because the file also holds
//! records that only look like streams (the directory at its end, stale headers).

use crate::crc::crc32;
use crate::{Limits, SldprtError};

/// The fixed start of a record's marker; the next four bytes differ from file to file.
const MARKER: [u8; 6] = [0x14, 0x00, 0x06, 0x00, 0x08, 0x00];
const HEADER: usize = 26;
const MAX_NAME: usize = 256;

/// True if the file starts like a chunk container: four varying bytes, then `00 00 00 04`
/// and a marker within the first bytes.
pub(crate) fn looks_like(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(&[0, 0, 0, 4][..]) && find(bytes, 8, 32).is_some()
}

/// False when the file lacks the end record that closes a whole chunk file (dogfooding
/// F12-9: a cut download otherwise imports without a word). The record lies in the last
/// bytes: two zero disk numbers, the entry count twice, the directory's size and offset and
/// a zero comment length.
pub(crate) fn has_end_record(bytes: &[u8]) -> bool {
    let u16_at = |i: usize| bytes.get(i..i + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes);
    let n = bytes.len();
    (n.saturating_sub(300)..n.saturating_sub(18)).rev().any(|i| {
        matches!(
            (u16_at(i), u16_at(i + 2), u16_at(i + 4), u16_at(i + 6), u16_at(i + 16)),
            (Some(0), Some(0), Some(a), Some(b), Some(0)) if a == b && a > 0
        )
    })
}

/// Position of the next marker at or after `from`, searching no further than `until`.
fn find(bytes: &[u8], from: usize, until: usize) -> Option<usize> {
    let end = bytes.len().min(until.saturating_add(MARKER.len()));
    let hay = bytes.get(from..end)?;
    hay.windows(MARKER.len()).position(|w| w == MARKER).map(|i| from + i)
}

fn u32_at(b: &[u8], off: usize) -> Option<usize> {
    let v = u32::from_le_bytes(b.get(off..off.checked_add(4)?)?.try_into().ok()?);
    usize::try_from(v).ok()
}

/// Each name is stored with the nibbles of every byte swapped.
fn decode_name(raw: &[u8]) -> String {
    raw.iter().map(|&b| char::from(b.rotate_left(4))).collect()
}

/// A validated stream.
pub(crate) struct Stream {
    pub name: String,
    pub data: Vec<u8>,
}

/// The names of the records `wanted` accepts whose data lies in the file, without inflating
/// anything (so one record can be chosen before any is read; security review T2).
pub(crate) fn names(bytes: &[u8], limits: &Limits, wanted: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut at = 0usize;
    let mut candidates = 0usize;
    while let Some(p) = find(bytes, at, usize::MAX) {
        at = p + 1;
        candidates += 1;
        if candidates > limits.max_streams.saturating_mul(16) || out.len() >= limits.max_streams {
            break;
        }
        let (Some(csize), Some(nlen)) = (u32_at(bytes, p + 14), u32_at(bytes, p + 22)) else { continue };
        if nlen == 0 || nlen > MAX_NAME {
            continue;
        }
        let Some(raw_name) = bytes.get(p + HEADER..p + HEADER + nlen) else { continue };
        if bytes.get(p + HEADER + nlen..).and_then(|d| d.get(..csize)).is_none() {
            continue;
        }
        let name = decode_name(raw_name);
        if wanted(&name) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Walks the records of `bytes`. `wanted` decides from the name whether a record's data is
/// inflated (inflating is the expensive step); only wanted records that validate are
/// returned, in file order.
pub(crate) fn streams(
    bytes: &[u8],
    limits: &Limits,
    wanted: impl Fn(&str) -> bool,
) -> Result<Vec<Stream>, SldprtError> {
    let mut out = Vec::new();
    let mut at = 0usize;
    let mut candidates = 0usize;
    let mut inflated = 0usize;
    while let Some(p) = find(bytes, at, usize::MAX) {
        at = p + 1;
        candidates += 1;
        if candidates > limits.max_streams.saturating_mul(16) {
            return Err(SldprtError::TooLarge("too many stream markers".into()));
        }
        let (Some(crc), Some(csize), Some(usize_), Some(nlen)) =
            (u32_at(bytes, p + 10), u32_at(bytes, p + 14), u32_at(bytes, p + 18), u32_at(bytes, p + 22))
        else {
            continue;
        };
        if nlen == 0 || nlen > MAX_NAME || usize_ > limits.max_stream_bytes {
            continue;
        }
        let Some(raw_name) = bytes.get(p + HEADER..p + HEADER + nlen) else { continue };
        let name = decode_name(raw_name);
        if !wanted(&name) {
            continue;
        }
        let Some(data) = bytes.get(p + HEADER + nlen..).and_then(|d| d.get(..csize)) else { continue };
        if usize_ / limits.max_ratio.max(1) > csize {
            continue;
        }
        inflated = inflated.saturating_add(usize_);
        if inflated > limits.max_total_bytes {
            return Err(SldprtError::TooLarge("the streams inflate to more than the limit".into()));
        }
        let Ok(plain) = miniz_oxide::inflate::decompress_to_vec_with_limit(data, usize_.saturating_add(1)) else {
            continue;
        };
        if plain.len() != usize_ || crc32(&plain) as usize != crc {
            continue;
        }
        out.push(Stream { name, data: plain });
        if out.len() > limits.max_streams {
            return Err(SldprtError::TooLarge("too many streams".into()));
        }
    }
    Ok(out)
}
