use std::fmt;

use super::Kind;
use crate::Hash;

/// What is wrong with one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordProblem {
    /// The record runs past the end of its region.
    Truncated,
    /// The `GREC` marker is missing.
    BadMagic,
    /// The kind byte is invalid (0 is reserved).
    BadKind(u8),
    /// Unknown compression codec.
    UnknownCodec(u8),
    /// `stored_len` is inconsistent with the codec and `raw_len`.
    StoredLenInvalid {
        /// Stored length.
        stored_len: u32,
        /// Uncompressed length.
        raw_len: u32,
    },
    /// The uncompressed size exceeds the limit for its kind.
    TooLarge {
        /// Declared uncompressed length.
        raw_len: u32,
        /// The limit.
        max: u32,
    },
    /// The record checksum does not match.
    Checksum {
        /// CRC stored in the file.
        stored: u32,
        /// CRC computed from the bytes.
        computed: u32,
    },
    /// The compressed payload could not be decoded.
    Decompress(String),
    /// The payload decoded to the wrong length.
    LengthMismatch {
        /// Declared length.
        expected: u32,
        /// Decoded length (capped at `expected + 1`).
        actual: u64,
    },
    /// The payload does not match its BLAKE3 hash.
    HashMismatch,
    /// The index and the record disagree about the kind or hash.
    IndexMismatch,
}

impl fmt::Display for RecordProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordProblem::Truncated => f.write_str("it is cut short"),
            RecordProblem::BadMagic => f.write_str("its start marker is missing"),
            RecordProblem::BadKind(k) => write!(f, "its type ({k}) is invalid"),
            RecordProblem::UnknownCodec(c) => write!(f, "it uses an unknown compression method ({c})"),
            RecordProblem::StoredLenInvalid { stored_len, raw_len } => {
                write!(f, "its stored size ({stored_len}) is inconsistent with its size ({raw_len})")
            }
            RecordProblem::TooLarge { raw_len, max } => {
                write!(f, "it declares {raw_len} bytes, more than the limit of {max}")
            }
            RecordProblem::Checksum { stored, computed } => {
                write!(f, "its checksum does not match (stored {stored:#010x}, computed {computed:#010x})")
            }
            RecordProblem::Decompress(e) => write!(f, "it cannot be decompressed ({e})"),
            RecordProblem::LengthMismatch { expected, actual } => {
                write!(f, "it decompresses to {actual} bytes instead of {expected}")
            }
            RecordProblem::HashMismatch => f.write_str("its content does not match its hash"),
            RecordProblem::IndexMismatch => f.write_str("it does not match the file's index"),
        }
    }
}

/// Why a container could not be opened. Every variant says what is damaged and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Shorter than a header plus a trailer.
    TooSmall {
        /// File length.
        len: u64,
    },
    /// Larger than [`super::Limits::max_file_size`].
    TooLarge {
        /// File length.
        len: u64,
        /// The limit.
        max: u64,
    },
    /// The file does not start with the Gusset signature.
    NotAGussetFile,
    /// The header checksum does not match.
    HeaderChecksum {
        /// Stored CRC.
        stored: u32,
        /// Computed CRC.
        computed: u32,
    },
    /// Written by a newer (or invalid) format version.
    UnsupportedVersion {
        /// Version in the file.
        found: u16,
        /// Newest version this build reads.
        supported: u16,
    },
    /// Header flags this build does not understand.
    UnknownFlags {
        /// The flags.
        flags: u32,
    },
    /// The trailer (last 32 bytes) is damaged.
    TrailerDamaged {
        /// What is wrong.
        reason: &'static str,
    },
    /// Header and trailer carry different save sequence numbers (mixed writes).
    SequenceMismatch {
        /// From the header.
        header: u64,
        /// From the trailer.
        trailer: u64,
    },
    /// The index position in the trailer is impossible.
    IndexOutOfBounds {
        /// Claimed offset.
        offset: u64,
        /// Claimed length.
        len: u64,
    },
    /// The index is damaged.
    IndexDamaged {
        /// Index offset.
        offset: u64,
        /// What is wrong.
        reason: &'static str,
    },
    /// The file layout is inconsistent (gaps, overlaps, or records the index does not
    /// list).
    Layout {
        /// Where.
        offset: u64,
        /// What is wrong.
        reason: &'static str,
    },
    /// The same record is listed twice.
    DuplicateRecord {
        /// Its hash.
        hash: Hash,
    },
    /// One record is damaged.
    Record {
        /// Zero-based position in the index.
        number: usize,
        /// Byte offset in the file.
        offset: u64,
        /// The record kind according to the index.
        kind: Kind,
        /// What is wrong.
        problem: RecordProblem,
    },
    /// Opening would decode more than [`super::Limits::max_total_raw`] bytes.
    DecodeBudget {
        /// Total declared bytes so far.
        total: u64,
        /// The limit.
        max: u64,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::TooSmall { len } => {
                write!(f, "the file is too small ({len} bytes) to be a Gusset project; it is empty or cut short")
            }
            OpenError::TooLarge { len, max } => {
                write!(f, "the file is {len} bytes, larger than the {max}-byte limit")
            }
            OpenError::NotAGussetFile => f.write_str("this is not a Gusset project file (signature missing)"),
            OpenError::HeaderChecksum { stored, computed } => write!(
                f,
                "the file header (bytes 0-63) is damaged: checksum {computed:#010x} does not match {stored:#010x}"
            ),
            OpenError::UnsupportedVersion { found, supported } => write!(
                f,
                "the file uses format version {found}, but this version of Gusset reads up to {supported}; \
                 please update Gusset"
            ),
            OpenError::UnknownFlags { flags } => {
                write!(f, "the file uses features this version of Gusset does not know (flags {flags:#x})")
            }
            OpenError::TrailerDamaged { reason } => {
                write!(f, "the end of the file is damaged ({reason}); the save may have been interrupted")
            }
            OpenError::SequenceMismatch { header, trailer } => {
                write!(f, "the start and end of the file come from different saves (#{header} and #{trailer})")
            }
            OpenError::IndexOutOfBounds { offset, len } => {
                write!(f, "the file's table of contents points outside the file (offset {offset}, length {len})")
            }
            OpenError::IndexDamaged { offset, reason } => {
                write!(f, "the file's table of contents at byte {offset} is damaged: {reason}")
            }
            OpenError::Layout { offset, reason } => {
                write!(f, "the file structure is damaged at byte {offset}: {reason}")
            }
            OpenError::DuplicateRecord { hash } => write!(f, "record {hash} is listed twice"),
            OpenError::Record { number, offset, kind, problem } => {
                write!(f, "record #{number} ({kind}) at byte {offset} is damaged: {problem}")
            }
            OpenError::DecodeBudget { total, max } => {
                write!(f, "the file would decompress to more than {max} bytes ({total} declared)")
            }
        }
    }
}

impl std::error::Error for OpenError {}

/// Why a container could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Kind 0 is reserved.
    InvalidKind,
    /// The payload exceeds the per-record limit for its kind.
    RecordTooLarge {
        /// The kind.
        kind: Kind,
        /// Payload length.
        len: u64,
        /// The limit.
        max: u32,
    },
    /// The same content was added with two different kinds.
    KindConflict {
        /// Content hash.
        hash: Hash,
        /// Kind already stored.
        existing: Kind,
        /// Kind requested now.
        new: Kind,
    },
    /// Too many records.
    TooManyRecords,
    /// A root name is empty, longer than 255 bytes, or contains control characters.
    InvalidRootName,
    /// A root value exceeds the limit.
    RootTooLarge,
    /// Too many roots.
    TooManyRoots,
    /// The record is not in the source container.
    NotInSource {
        /// Requested hash.
        hash: Hash,
    },
    /// zstd failed.
    Compress(String),
    /// The finished file would exceed the size limit.
    FileTooLarge {
        /// Size.
        len: u64,
        /// The limit.
        max: u64,
    },
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::InvalidKind => f.write_str("record kind 0 is reserved"),
            WriteError::RecordTooLarge { kind, len, max } => {
                write!(f, "a {kind} record of {len} bytes exceeds the {max}-byte limit")
            }
            WriteError::KindConflict { hash, existing, new } => {
                write!(f, "content {hash} is already stored as {existing}, cannot also store it as {new}")
            }
            WriteError::TooManyRecords => f.write_str("too many records"),
            WriteError::InvalidRootName => f.write_str("invalid root name"),
            WriteError::RootTooLarge => f.write_str("root value too large"),
            WriteError::TooManyRoots => f.write_str("too many roots"),
            WriteError::NotInSource { hash } => write!(f, "record {hash} is not in the source file"),
            WriteError::Compress(e) => write!(f, "compression failed: {e}"),
            WriteError::FileTooLarge { len, max } => {
                write!(f, "the file would be {len} bytes, over the {max}-byte limit")
            }
        }
    }
}

impl std::error::Error for WriteError {}
