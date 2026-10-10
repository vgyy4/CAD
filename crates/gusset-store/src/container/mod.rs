//! The `.gus` checksummed chunk container (ADR 0005). Byte layout: `docs/FORMAT.md`.
//!
//! ```text
//! [Header 64 B] [Record]* [Index] [Trailer 32 B]
//! ```
//!
//! - [`ContainerWriter`] builds a file: content-addressed records (BLAKE3 of the
//!   uncompressed payload, deduplicated), zstd-compressed unless that does not shrink
//!   them, plus a small map of named roots. Records from an existing container can be
//!   copied byte-for-byte without recompression.
//! - [`open`] validates everything (header, trailer, index, and every record's CRC and
//!   hash) under enforced [`Limits`] and gives lazy access to record payloads.
//! - [`verify`] is the validator `durable_write` uses for project saves.
//! - [`salvage`] recovers every intact record from a damaged file.
//!
//! No function in this module panics on any input.

use std::fmt;

mod error;
mod reader;
mod salvage;
mod writer;

pub use error::{OpenError, RecordProblem, WriteError};
pub use reader::{Container, RecordInfo, open, open_with, verify};
pub use salvage::{Salvaged, SalvagedRecord, salvage, salvage_with};
pub use writer::{ContainerWriter, SaveInfo};

/// File magic: `GUSSET\r\n\x1a\0` (the CR LF / ^Z / NUL bytes detect text-mode
/// mangling, like PNG's signature).
pub const HEADER_MAGIC: [u8; 10] = *b"GUSSET\r\n\x1a\0";
/// Header size in bytes.
pub const HEADER_LEN: usize = 64;
/// The container format version this crate writes and the newest it reads.
pub const FORMAT_VERSION: u16 = 1;
/// Record magic.
pub const RECORD_MAGIC: [u8; 4] = *b"GREC";
/// Size of a record's fixed header (magic through `raw_len`).
pub const RECORD_HEADER_LEN: usize = 46;
/// Index magic.
pub const INDEX_MAGIC: [u8; 4] = *b"GIDX";
/// Size of one index entry (`kind u8 | hash [32] | offset u64`).
pub const INDEX_ENTRY_LEN: usize = 41;
/// Trailer magic.
pub const TRAILER_MAGIC: [u8; 8] = *b"GSEND\0\0\0";
/// Trailer size in bytes.
pub const TRAILER_LEN: usize = 32;
/// Maximum length of a root name in bytes.
pub const MAX_ROOT_NAME: usize = 255;
/// Default zstd level used by [`ContainerWriter`].
pub const DEFAULT_LEVEL: i32 = 3;

/// A record kind. Known kinds are associated constants; unknown non-zero kinds are
/// accepted (forward compatibility) and limited like small objects.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Kind(pub u8);

impl Kind {
    /// A feature definition (canonical text).
    pub const FEATURE: Kind = Kind(1);
    /// A parameter table (canonical text).
    pub const PARAMS: Kind = Kind(2);
    /// A model snapshot (canonical text).
    pub const SNAPSHOT: Kind = Kind(3);
    /// A history node (canonical text).
    pub const NODE: Kind = Kind(4);
    /// The branch table (canonical text).
    pub const BRANCHES: Kind = Kind(5);
    /// The event log (canonical text).
    pub const EVENTS: Kind = Kind(6);
    /// An opaque binary blob (imported files, meshes).
    pub const BLOB: Kind = Kind(7);
    /// Project metadata, including the project ID (canonical text).
    pub const PROJECT_META: Kind = Kind(8);
    /// Per-project settings (canonical text).
    pub const SETTINGS: Kind = Kind(9);
    /// A derived, optional thumbnail image.
    pub const THUMBNAIL: Kind = Kind(10);

    /// Whether this kind gets the large per-record size limit (blobs, thumbnails).
    #[must_use]
    pub fn is_large(self) -> bool {
        self == Kind::BLOB || self == Kind::THUMBNAIL
    }

    /// The kind's name, if it is a known kind.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            1 => "Feature",
            2 => "Params",
            3 => "Snapshot",
            4 => "Node",
            5 => "Branches",
            6 => "Events",
            7 => "Blob",
            8 => "ProjectMeta",
            9 => "Settings",
            10 => "Thumbnail",
            _ => return None,
        })
    }
}

impl fmt::Debug for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => f.write_str(n),
            None => write!(f, "Kind({})", self.0),
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// How a record's payload is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Stored as is (`stored_len == raw_len`).
    Raw,
    /// A zstd frame (`stored_len < raw_len`).
    Zstd,
}

impl Codec {
    pub(crate) fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Codec::Raw),
            1 => Some(Codec::Zstd),
            _ => None,
        }
    }

    pub(crate) fn to_u8(self) -> u8 {
        match self {
            Codec::Raw => 0,
            Codec::Zstd => 1,
        }
    }
}

/// Limits enforced when reading (and writing) containers. Hostile files cannot make the
/// reader allocate or work beyond these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Maximum file size (default 4 GiB).
    pub max_file_size: u64,
    /// Maximum number of records (default 10 M).
    pub max_records: u32,
    /// Maximum uncompressed size of one record that is not a blob/thumbnail (16 MiB).
    pub max_raw_len: u32,
    /// Maximum uncompressed size of one blob or thumbnail record (1 GiB).
    pub max_raw_len_large: u32,
    /// Maximum total uncompressed bytes decoded while opening or salvaging (16 GiB).
    pub max_total_raw: u64,
    /// zstd window cap as log2 bytes (default 25 = 32 MiB; writers use ≤ 2^23).
    pub window_log_max: u32,
    /// Maximum number of roots (default 4096).
    pub max_roots: u32,
    /// Maximum size of one root value (default 1 MiB).
    pub max_root_len: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_size: 4 << 30,
            max_records: 10_000_000,
            max_raw_len: 16 << 20,
            max_raw_len_large: 1 << 30,
            max_total_raw: 16 << 30,
            window_log_max: 25,
            max_roots: 4096,
            max_root_len: 1 << 20,
        }
    }
}

impl Limits {
    /// The per-record uncompressed size limit for `kind`.
    #[must_use]
    pub fn max_raw_for(&self, kind: Kind) -> u32 {
        if kind.is_large() { self.max_raw_len_large } else { self.max_raw_len }
    }
}

/// The decoded file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Container format version.
    pub format_version: u16,
    /// Flags (all zero in version 1; unknown bits make a file unreadable).
    pub flags: u32,
    /// Save sequence number, incremented on every save; also stored in the trailer.
    pub save_seq: u64,
    /// Project creation time (Unix seconds).
    pub created_unix: u64,
    /// Version of the app that wrote the file (up to 16 bytes of UTF-8).
    pub app_version: String,
}

impl Header {
    pub(crate) fn encode(&self) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        let mut w = Vec::with_capacity(HEADER_LEN);
        w.extend_from_slice(&HEADER_MAGIC);
        w.extend_from_slice(&self.format_version.to_le_bytes());
        w.extend_from_slice(&self.flags.to_le_bytes());
        w.extend_from_slice(&self.save_seq.to_le_bytes());
        w.extend_from_slice(&self.created_unix.to_le_bytes());
        let mut ver = [0u8; 16];
        for (d, s) in ver.iter_mut().zip(truncate_utf8(&self.app_version, 16).as_bytes()) {
            *d = *s;
        }
        w.extend_from_slice(&ver);
        w.resize(HEADER_LEN - 4, 0); // reserved
        let crc = crate::checksum::crc32c(&w);
        w.extend_from_slice(&crc.to_le_bytes());
        for (d, s) in h.iter_mut().zip(&w) {
            *d = *s;
        }
        h
    }

    /// Parses and checks a header (magic, CRC, version, flags).
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, OpenError> {
        let h = bytes.get(..HEADER_LEN).ok_or(OpenError::TooSmall { len: bytes.len() as u64 })?;
        let mut r = crate::wire::Reader::new(h);
        if r.array::<10>() != Some(HEADER_MAGIC) {
            return Err(OpenError::NotAGussetFile);
        }
        let body = h.get(..HEADER_LEN - 4).unwrap_or_default();
        let stored = h.get(HEADER_LEN - 4..).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes).unwrap_or(0);
        let computed = crate::checksum::crc32c(body);
        if stored != computed {
            return Err(OpenError::HeaderChecksum { stored, computed });
        }
        let format_version = r.u16().unwrap_or(0);
        let flags = r.u32().unwrap_or(0);
        let save_seq = r.u64().unwrap_or(0);
        let created_unix = r.u64().unwrap_or(0);
        let ver = r.array::<16>().unwrap_or_default();
        if format_version == 0 || format_version > FORMAT_VERSION {
            return Err(OpenError::UnsupportedVersion { found: format_version, supported: FORMAT_VERSION });
        }
        if flags != 0 {
            return Err(OpenError::UnknownFlags { flags });
        }
        let end = ver.iter().position(|&b| b == 0).unwrap_or(ver.len());
        let app_version = String::from_utf8_lossy(ver.get(..end).unwrap_or_default()).into_owned();
        Ok(Header { format_version, flags, save_seq, created_unix, app_version })
    }
}

fn truncate_utf8(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// The decoded trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Trailer {
    pub(crate) index_offset: u64,
    pub(crate) index_len: u32,
    pub(crate) save_seq: u64,
}

impl Trailer {
    pub(crate) fn encode(&self) -> [u8; TRAILER_LEN] {
        let mut w = Vec::with_capacity(TRAILER_LEN);
        w.extend_from_slice(&self.index_offset.to_le_bytes());
        w.extend_from_slice(&self.index_len.to_le_bytes());
        w.extend_from_slice(&self.save_seq.to_le_bytes());
        w.extend_from_slice(&TRAILER_MAGIC);
        let crc = crate::checksum::crc32c(&w);
        w.extend_from_slice(&crc.to_le_bytes());
        let mut t = [0u8; TRAILER_LEN];
        for (d, s) in t.iter_mut().zip(&w) {
            *d = *s;
        }
        t
    }

    pub(crate) fn decode(t: &[u8]) -> Result<Self, OpenError> {
        let mut r = crate::wire::Reader::new(t);
        let (Some(index_offset), Some(index_len), Some(save_seq), Some(magic), Some(stored)) =
            (r.u64(), r.u32(), r.u64(), r.array::<8>(), r.u32())
        else {
            return Err(OpenError::TrailerDamaged { reason: "truncated" });
        };
        if magic != TRAILER_MAGIC {
            return Err(OpenError::TrailerDamaged {
                reason: "end marker missing (the file was cut short or overwritten)",
            });
        }
        let computed = crate::checksum::crc32c(t.get(..TRAILER_LEN - 4).unwrap_or_default());
        if stored != computed {
            return Err(OpenError::TrailerDamaged { reason: "checksum mismatch" });
        }
        Ok(Trailer { index_offset, index_len, save_seq })
    }
}

/// A parsed record (fixed header + borrowed stored payload), CRC-checked.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RawRecord<'a> {
    pub(crate) kind: Kind,
    pub(crate) codec: Codec,
    pub(crate) hash: crate::Hash,
    pub(crate) stored: &'a [u8],
    pub(crate) raw_len: u32,
    /// Bytes from the magic to the end of the CRC.
    pub(crate) total_len: usize,
}

/// Parses and CRC-checks the record starting at `offset`. Never decompresses.
pub(crate) fn parse_record_at<'a>(
    bytes: &'a [u8],
    offset: usize,
    limits: &Limits,
) -> Result<RawRecord<'a>, RecordProblem> {
    let rest = bytes.get(offset..).ok_or(RecordProblem::Truncated)?;
    let mut r = crate::wire::Reader::new(rest);
    if r.array::<4>().ok_or(RecordProblem::Truncated)? != RECORD_MAGIC {
        return Err(RecordProblem::BadMagic);
    }
    let kind = Kind(r.u8().ok_or(RecordProblem::Truncated)?);
    let codec_byte = r.u8().ok_or(RecordProblem::Truncated)?;
    let hash = crate::Hash(r.array::<32>().ok_or(RecordProblem::Truncated)?);
    let stored_len = r.u32().ok_or(RecordProblem::Truncated)?;
    let raw_len = r.u32().ok_or(RecordProblem::Truncated)?;
    if kind.0 == 0 {
        return Err(RecordProblem::BadKind(0));
    }
    let codec = Codec::from_u8(codec_byte).ok_or(RecordProblem::UnknownCodec(codec_byte))?;
    let max = limits.max_raw_for(kind);
    if raw_len > max {
        return Err(RecordProblem::TooLarge { raw_len, max });
    }
    let len_ok = match codec {
        Codec::Raw => stored_len == raw_len,
        Codec::Zstd => stored_len < raw_len,
    };
    if !len_ok {
        return Err(RecordProblem::StoredLenInvalid { stored_len, raw_len });
    }
    let stored_usize = usize::try_from(stored_len).map_err(|_| RecordProblem::Truncated)?;
    if r.remaining() < stored_usize.saturating_add(4) {
        return Err(RecordProblem::Truncated);
    }
    let stored = r.take(stored_usize).ok_or(RecordProblem::Truncated)?;
    let body_len = r.pos();
    let crc_stored = r.u32().ok_or(RecordProblem::Truncated)?;
    let crc_computed = crate::checksum::crc32c(rest.get(..body_len).unwrap_or_default());
    if crc_stored != crc_computed {
        return Err(RecordProblem::Checksum { stored: crc_stored, computed: crc_computed });
    }
    Ok(RawRecord { kind, codec, hash, stored, raw_len, total_len: r.pos() })
}

/// Decodes a record's payload (bounded by `raw_len`) and checks its hash.
pub(crate) fn decode_record(rec: &RawRecord<'_>, limits: &Limits) -> Result<Vec<u8>, RecordProblem> {
    use std::io::Read;
    let raw_len = usize::try_from(rec.raw_len).map_err(|_| RecordProblem::TooLarge { raw_len: rec.raw_len, max: 0 })?;
    let out = match rec.codec {
        Codec::Raw => rec.stored.to_vec(),
        Codec::Zstd => {
            let derr = |e: std::io::Error| RecordProblem::Decompress(e.to_string());
            let mut dec = zstd::stream::read::Decoder::with_buffer(rec.stored).map_err(derr)?;
            dec.window_log_max(limits.window_log_max).map_err(derr)?;
            // Grow with the real output instead of trusting raw_len for the allocation.
            let mut out = Vec::with_capacity(raw_len.min(1 << 20));
            let cap = u64::from(rec.raw_len) + 1;
            dec.take(cap).read_to_end(&mut out).map_err(derr)?;
            out
        }
    };
    if out.len() != raw_len {
        return Err(RecordProblem::LengthMismatch { expected: rec.raw_len, actual: out.len() as u64 });
    }
    if crate::Hash::of(&out) != rec.hash {
        return Err(RecordProblem::HashMismatch);
    }
    Ok(out)
}

/// The decoded index: entries plus roots.
#[derive(Debug, Clone, Default)]
pub(crate) struct Index {
    pub(crate) entries: Vec<(Kind, crate::Hash, u64)>,
    pub(crate) roots: std::collections::BTreeMap<String, Vec<u8>>,
}

/// Parses an index starting at `bytes[0]` (its magic). Returns it and its length.
/// Checks the CRC before trusting any count.
pub(crate) fn parse_index(bytes: &[u8], limits: &Limits) -> Result<(Index, usize), &'static str> {
    // Pass 1: find the extent using only bounds-checked length fields.
    let mut r = crate::wire::Reader::new(bytes);
    if r.array::<4>() != Some(INDEX_MAGIC) {
        return Err("index marker missing");
    }
    let count = r.u32().ok_or("truncated")?;
    if count > limits.max_records {
        return Err("too many records");
    }
    let entries_len = (count as usize).checked_mul(INDEX_ENTRY_LEN).ok_or("too many records")?;
    let entries_bytes = r.take(entries_len).ok_or("truncated in the record list")?;
    let root_count = r.u32().ok_or("truncated")?;
    if root_count > limits.max_roots {
        return Err("too many roots");
    }
    let mut root_spans = Vec::new();
    for _ in 0..root_count {
        let name_len = usize::from(r.u16().ok_or("truncated in the roots")?);
        if name_len == 0 || name_len > MAX_ROOT_NAME {
            return Err("invalid root name length");
        }
        let name = r.take(name_len).ok_or("truncated in the roots")?;
        let value_len = r.u32().ok_or("truncated in the roots")?;
        if value_len > limits.max_root_len {
            return Err("root value too large");
        }
        let value = r.take(value_len as usize).ok_or("truncated in the roots")?;
        root_spans.push((name, value));
    }
    let body_len = r.pos();
    let stored = r.u32().ok_or("truncated")?;
    if crate::checksum::crc32c(bytes.get(..body_len).unwrap_or_default()) != stored {
        return Err("checksum mismatch");
    }
    // Pass 2: decode.
    let mut entries = Vec::with_capacity(count as usize);
    let (chunks, _) = entries_bytes.as_chunks::<INDEX_ENTRY_LEN>();
    for chunk in chunks {
        let mut e = crate::wire::Reader::new(chunk);
        let (Some(kind), Some(hash), Some(offset)) = (e.u8(), e.array::<32>(), e.u64()) else {
            return Err("truncated entry");
        };
        entries.push((Kind(kind), crate::Hash(hash), offset));
    }
    let mut roots = std::collections::BTreeMap::new();
    let mut prev: Option<&str> = None;
    for (name, value) in root_spans {
        let name = std::str::from_utf8(name).map_err(|_| "root name is not UTF-8")?;
        if prev.is_some_and(|p| p >= name) {
            return Err("root names not strictly sorted");
        }
        prev = Some(name);
        roots.insert(name.to_owned(), value.to_vec());
    }
    Ok((Index { entries, roots }, r.pos()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let h = Header {
            format_version: 1,
            flags: 0,
            save_seq: 42,
            created_unix: 1_700_000_000,
            app_version: "0.1.0-αβγ-long-version".into(),
        };
        let enc = h.encode();
        let dec = Header::decode(&enc).unwrap();
        assert_eq!(dec.save_seq, 42);
        assert!(dec.app_version.len() <= 16);
        assert!(h.app_version.starts_with(&dec.app_version));
    }

    #[test]
    fn kind_names() {
        assert_eq!(format!("{:?}", Kind::BLOB), "Blob");
        assert_eq!(format!("{}", Kind(200)), "Kind(200)");
        assert!(Kind::THUMBNAIL.is_large());
        assert!(!Kind::FEATURE.is_large());
    }
}
