use std::collections::{BTreeMap, HashSet};

use super::{
    HEADER_LEN, Header, INDEX_MAGIC, Kind, Limits, RECORD_MAGIC, TRAILER_LEN, Trailer, decode_record, parse_index,
    parse_record_at,
};
use crate::Hash;

/// One intact record recovered by [`salvage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalvagedRecord {
    /// Record kind.
    pub kind: Kind,
    /// BLAKE3 hash of `data` (verified).
    pub hash: Hash,
    /// Where it was found.
    pub offset: u64,
    /// The uncompressed payload.
    pub data: Vec<u8>,
}

/// What [`salvage`] recovered from a (possibly damaged) file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Salvaged {
    /// The header, if its checksum verified.
    pub header: Option<Header>,
    /// Every intact record (CRC and hash verified), deduplicated, in file order.
    pub records: Vec<SalvagedRecord>,
    /// The roots, if an intact index was found.
    pub roots: Option<BTreeMap<String, Vec<u8>>>,
    /// False if the scan stopped early because a work or size budget ran out.
    pub complete: bool,
}

/// Recovers every intact record from `bytes` with default [`Limits`].
#[must_use]
pub fn salvage(bytes: &[u8]) -> Salvaged {
    salvage_with(bytes, &Limits::default())
}

/// How many `GIDX` candidates salvage examines when the trailer is damaged.
const MAX_INDEX_ATTEMPTS: usize = 16;

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Recovers every intact record from `bytes`: scans for record markers and keeps each
/// record whose CRC and BLAKE3 hash verify. Work is bounded (checksummed bytes ≤
/// 8 × file size + 64 MiB, decoded bytes ≤ `limits.max_total_raw`).
#[must_use]
pub fn salvage_with(bytes: &[u8], limits: &Limits) -> Salvaged {
    let mut out = Salvaged { complete: true, ..Salvaged::default() };
    out.header = Header::decode(bytes).ok();

    // Roots: from the trailer-located index if intact, otherwise from any intact index.
    if let Some(t) = bytes.len().checked_sub(TRAILER_LEN).and_then(|at| bytes.get(at..))
        && let Ok(tr) = Trailer::decode(t)
        && let Some(idx) = crate::wire::usize_of(tr.index_offset).and_then(|o| bytes.get(o..))
        && let Ok((index, _)) = parse_index(idx, limits)
    {
        out.roots = Some(index.roots);
    }
    if out.roots.is_none() {
        // Each attempt costs at most one pass over the rest of the file; cap them.
        let mut at = HEADER_LEN.min(bytes.len());
        let mut attempts = 0;
        while let Some(p) = find(bytes, &INDEX_MAGIC, at) {
            attempts += 1;
            if attempts > MAX_INDEX_ATTEMPTS {
                out.complete = false;
                break;
            }
            if let Some(Ok((index, _))) = bytes.get(p..).map(|s| parse_index(s, limits)) {
                out.roots = Some(index.roots);
            }
            at = p + 1;
        }
    }

    let mut budget: u64 = (bytes.len() as u64).saturating_mul(8).saturating_add(64 << 20);
    let mut decoded: u64 = 0;
    let mut seen = HashSet::new();
    let mut at = 0;
    while let Some(p) = find(bytes, &RECORD_MAGIC, at) {
        // Cost of a CRC attempt is bounded by the declared stored length.
        let declared = bytes
            .get(p + 38..p + 42)
            .and_then(|s| s.try_into().ok())
            .map_or(0, |a| u64::from(u32::from_le_bytes(a)))
            .min((bytes.len() - p) as u64);
        if declared > budget {
            out.complete = false;
            break;
        }
        budget -= declared;
        match parse_record_at(bytes, p, limits) {
            Ok(rec) => {
                if decoded.saturating_add(u64::from(rec.raw_len)) > limits.max_total_raw {
                    out.complete = false;
                    break;
                }
                decoded += u64::from(rec.raw_len);
                if let Ok(data) = decode_record(&rec, limits) {
                    if seen.insert(rec.hash) {
                        out.records.push(SalvagedRecord { kind: rec.kind, hash: rec.hash, offset: p as u64, data });
                    }
                    at = p + rec.total_len;
                    continue;
                }
                at = p + 1;
            }
            Err(_) => at = p + 1,
        }
    }
    out
}
