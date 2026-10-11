use std::collections::{BTreeMap, HashMap};

use super::{
    Codec, HEADER_LEN, Header, Kind, Limits, OpenError, RecordProblem, TRAILER_LEN, Trailer, decode_record,
    parse_index, parse_record_at,
};
use crate::Hash;
use crate::wire::usize_of;

/// Metadata of one record in an opened container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordInfo {
    /// Record kind.
    pub kind: Kind,
    /// BLAKE3 hash of the uncompressed payload.
    pub hash: Hash,
    /// How the payload is stored.
    pub codec: Codec,
    /// Byte offset of the record in the file.
    pub offset: u64,
    /// Stored payload length.
    pub stored_len: u32,
    /// Uncompressed payload length.
    pub raw_len: u32,
}

/// A fully validated container. Payloads are decoded on demand by [`Container::get`].
#[derive(Debug)]
pub struct Container<'a> {
    bytes: &'a [u8],
    header: Header,
    records: Vec<RecordInfo>,
    by_hash: HashMap<Hash, usize>,
    roots: BTreeMap<String, Vec<u8>>,
    limits: Limits,
}

/// Opens and fully validates a container with default [`Limits`].
///
/// # Errors
/// An [`OpenError`] describing the first problem found.
pub fn open(bytes: &[u8]) -> Result<Container<'_>, OpenError> {
    open_with(bytes, Limits::default())
}

/// Validates a container completely (header, trailer, index, every record's CRC and
/// hash). This is the validator `durable_write` uses for project files.
///
/// # Errors
/// See [`open`].
pub fn verify(bytes: &[u8]) -> Result<(), OpenError> {
    open(bytes).map(|_| ())
}

/// Checks a container's framing without decompressing anything: header, trailer,
/// index, layout and the CRC32C of every record (so every byte of the file is
/// covered by a checksum), but not the BLAKE3 hash of each decoded payload.
///
/// This is the read-back validator for images this process just built (ADR 0007 as
/// amended): each record's hash was verified when the record was encoded, and the bytes
/// read back are compared byte-for-byte with that image. Opening a file always uses the
/// full [`verify`].
///
/// # Errors
/// See [`open`].
pub fn verify_framing(bytes: &[u8]) -> Result<(), OpenError> {
    open_impl(bytes, Limits::default(), false).map(|_| ())
}

/// Opens and fully validates a container with the given limits.
///
/// # Errors
/// An [`OpenError`] describing the first problem found.
pub fn open_with(bytes: &[u8], limits: Limits) -> Result<Container<'_>, OpenError> {
    open_impl(bytes, limits, true)
}

fn open_impl(bytes: &[u8], limits: Limits, decode: bool) -> Result<Container<'_>, OpenError> {
    let len = bytes.len() as u64;
    if len > limits.max_file_size {
        return Err(OpenError::TooLarge { len, max: limits.max_file_size });
    }
    if bytes.len() < HEADER_LEN + TRAILER_LEN {
        return Err(OpenError::TooSmall { len });
    }
    let header = Header::decode(bytes)?;
    let trailer_at = bytes.len() - TRAILER_LEN;
    let trailer = Trailer::decode(bytes.get(trailer_at..).unwrap_or_default())?;
    if trailer.save_seq != header.save_seq {
        return Err(OpenError::SequenceMismatch { header: header.save_seq, trailer: trailer.save_seq });
    }
    let oob = OpenError::IndexOutOfBounds { offset: trailer.index_offset, len: u64::from(trailer.index_len) };
    let index_start = usize_of(trailer.index_offset).ok_or(oob.clone())?;
    let index_end = index_start.checked_add(trailer.index_len as usize).ok_or(oob.clone())?;
    if index_start < HEADER_LEN || index_end != trailer_at {
        return Err(oob);
    }
    let index_bytes = bytes.get(index_start..index_end).ok_or(oob)?;
    let (index, used) = parse_index(index_bytes, &limits)
        .map_err(|reason| OpenError::IndexDamaged { offset: trailer.index_offset, reason })?;
    if used != index_bytes.len() {
        return Err(OpenError::IndexDamaged {
            offset: trailer.index_offset,
            reason: "unexpected bytes after the index",
        });
    }

    let mut records = Vec::with_capacity(index.entries.len());
    let mut by_hash = HashMap::with_capacity(index.entries.len());
    let mut pos = HEADER_LEN;
    let mut total_raw: u64 = 0;
    for (number, &(kind, hash, offset)) in index.entries.iter().enumerate() {
        if offset != pos as u64 {
            return Err(OpenError::Layout {
                offset: pos as u64,
                reason: "records are not contiguous or not in index order",
            });
        }
        if pos >= index_start {
            return Err(OpenError::Layout { offset: pos as u64, reason: "a record overlaps the index" });
        }
        let rec_err = |problem: RecordProblem| OpenError::Record { number, offset, kind, problem };
        let rec = parse_record_at(bytes.get(..index_start).unwrap_or_default(), pos, &limits).map_err(rec_err)?;
        if rec.kind != kind || rec.hash != hash {
            return Err(rec_err(RecordProblem::IndexMismatch));
        }
        if by_hash.insert(hash, records.len()).is_some() {
            return Err(OpenError::DuplicateRecord { hash });
        }
        total_raw = total_raw.saturating_add(u64::from(rec.raw_len));
        if total_raw > limits.max_total_raw {
            return Err(OpenError::DecodeBudget { total: total_raw, max: limits.max_total_raw });
        }
        if decode {
            decode_record(&rec, &limits).map_err(rec_err)?;
        }
        records.push(RecordInfo {
            kind,
            hash,
            codec: rec.codec,
            offset,
            stored_len: u32::try_from(rec.stored.len()).unwrap_or(u32::MAX),
            raw_len: rec.raw_len,
        });
        pos += rec.total_len;
    }
    if pos != index_start {
        return Err(OpenError::Layout {
            offset: pos as u64,
            reason: "unlisted bytes between the last record and the index",
        });
    }
    Ok(Container { bytes, header, records, by_hash, roots: index.roots, limits })
}

impl<'a> Container<'a> {
    /// The file header.
    #[must_use]
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// All records in file order.
    #[must_use]
    pub fn records(&self) -> &[RecordInfo] {
        &self.records
    }

    /// Metadata of the record with `hash`.
    #[must_use]
    pub fn info(&self, hash: &Hash) -> Option<&RecordInfo> {
        self.by_hash.get(hash).and_then(|&i| self.records.get(i))
    }

    /// Whether the container holds a record with `hash`.
    #[must_use]
    pub fn contains(&self, hash: &Hash) -> bool {
        self.by_hash.contains_key(hash)
    }

    /// Decodes the payload of the record with `hash` (`Ok(None)` if absent).
    /// Decompression is bounded by the record's declared size.
    ///
    /// # Errors
    /// Only if the underlying bytes fail to decode, which cannot happen for bytes that
    /// passed [`open`] (kept for defence in depth).
    pub fn get(&self, hash: &Hash) -> Result<Option<Vec<u8>>, OpenError> {
        let Some(&number) = self.by_hash.get(hash) else {
            return Ok(None);
        };
        let Some(info) = self.records.get(number) else {
            return Ok(None);
        };
        let err = |problem| OpenError::Record { number, offset: info.offset, kind: info.kind, problem };
        let at = usize_of(info.offset).ok_or(err(RecordProblem::Truncated))?;
        let rec = parse_record_at(self.bytes, at, &self.limits).map_err(err)?;
        decode_record(&rec, &self.limits).map(Some).map_err(err)
    }

    /// The named roots.
    #[must_use]
    pub fn roots(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.roots
    }

    /// One root's value.
    #[must_use]
    pub fn root(&self, name: &str) -> Option<&[u8]> {
        self.roots.get(name).map(Vec::as_slice)
    }

    /// The raw encoded record (magic through CRC), for copy-through.
    pub(crate) fn record_bytes(&self, hash: &Hash) -> Option<(RecordInfo, &'a [u8])> {
        let info = *self.info(hash)?;
        let start = usize_of(info.offset)?;
        let len = super::RECORD_HEADER_LEN + info.stored_len as usize + 4;
        let slice = self.bytes.get(start..start.checked_add(len)?)?;
        Some((info, slice))
    }
}
