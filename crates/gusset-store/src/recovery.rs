//! The two-slot recovery file (ADR 0007): exactly one file per open project,
//! `<data_dir>/recovery/<project-id>.grec`.
//!
//! ```text
//! header : magic "GUSREC\r\n" | version u16 | reserved u16 | crc32c u32 (of the first 12 bytes)
//! slot   : magic "GSLT" | seq u64 | len u64 | crc32c u32 | payload [len]
//!          (crc32c over magic, seq, len and payload)
//! file   : header | slot (previous newest) | slot (new)
//! ```
//!
//! Every [`write_snapshot`] creates a new file through `durable_write` holding **the
//! previous newest valid slot plus the new snapshot**, so the older slot is the one
//! replaced and a crash at any point leaves the previous snapshot intact. The loader
//! ([`read_newest`]) picks the valid slot with the highest sequence number whose CRC
//! and payload verify. Slots are found by scanning for their marker, so a damaged first
//! slot does not hide the second.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::checksum::{crc32c, crc32c_update};
use crate::durable::{DurableError, durable_write};
use crate::fs::Fs;
use crate::wire::Reader;

/// File magic.
pub const MAGIC: [u8; 8] = *b"GUSREC\r\n";
/// Format version written.
pub const VERSION: u16 = 1;
/// Header length.
pub const HEADER_LEN: usize = 16;
/// Slot marker.
pub const SLOT_MAGIC: [u8; 4] = *b"GSLT";
/// Slot header length (marker, seq, len, crc).
pub const SLOT_HEADER_LEN: usize = 24;
/// File extension of recovery files.
pub const EXTENSION: &str = "grec";
/// Maximum slot candidates examined when scanning a damaged file.
const MAX_CANDIDATES: usize = 64;

/// One recovery snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Slot sequence number (increases by one per snapshot written to this file).
    pub seq: u64,
    /// The payload: a full `.gus` image of the in-memory project.
    pub payload: Vec<u8>,
}

/// Errors from the recovery file.
#[derive(Debug)]
pub enum RecoveryError {
    /// Reading the existing file failed.
    Io {
        /// The recovery file.
        path: PathBuf,
        /// The error.
        source: io::Error,
    },
    /// The file exists but holds no valid snapshot.
    NoValidSlot {
        /// The recovery file.
        path: PathBuf,
    },
    /// The new snapshot was rejected by the payload validator before writing.
    PayloadInvalid(String),
    /// The slot sequence number would overflow.
    SequenceOverflow,
    /// Writing the new file failed (the previous file is intact).
    Write(DurableError<String>),
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryError::Io { path, source } => write!(f, "cannot read recovery file {}: {source}", path.display()),
            RecoveryError::NoValidSlot { path } => {
                write!(f, "the recovery file {} is damaged and holds no usable snapshot", path.display())
            }
            RecoveryError::PayloadInvalid(e) => write!(f, "the recovery snapshot is invalid: {e}"),
            RecoveryError::SequenceOverflow => f.write_str("recovery sequence number overflow"),
            RecoveryError::Write(e) => write!(f, "writing the recovery snapshot failed: {e}"),
        }
    }
}

impl std::error::Error for RecoveryError {}

fn header() -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    let mut v = Vec::with_capacity(HEADER_LEN);
    v.extend_from_slice(&MAGIC);
    v.extend_from_slice(&VERSION.to_le_bytes());
    v.extend_from_slice(&[0, 0]);
    let crc = crc32c(&v);
    v.extend_from_slice(&crc.to_le_bytes());
    for (d, s) in h.iter_mut().zip(&v) {
        *d = *s;
    }
    h
}

fn slot_crc(seq: u64, payload: &[u8]) -> u32 {
    let mut head = Vec::with_capacity(20);
    head.extend_from_slice(&SLOT_MAGIC);
    head.extend_from_slice(&seq.to_le_bytes());
    head.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    crc32c_update(crc32c(&head), payload)
}

/// Encodes a recovery file from `(seq, payload)` slots (normally two; one for a new
/// file).
#[must_use]
pub fn encode(slots: &[(u64, &[u8])]) -> Vec<u8> {
    let total: usize = slots.iter().map(|(_, p)| SLOT_HEADER_LEN + p.len()).sum();
    let mut out = Vec::with_capacity(HEADER_LEN + total);
    out.extend_from_slice(&header());
    for &(seq, payload) in slots {
        out.extend_from_slice(&SLOT_MAGIC);
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&slot_crc(seq, payload).to_le_bytes());
        out.extend_from_slice(payload);
    }
    out
}

fn parse_slot(bytes: &[u8], at: usize) -> Option<(Snapshot, usize)> {
    let mut r = Reader::new(bytes.get(at..)?);
    if r.array::<4>()? != SLOT_MAGIC {
        return None;
    }
    let seq = r.u64()?;
    let len = crate::wire::usize_of(r.u64()?)?;
    let crc = r.u32()?;
    let payload = r.take(len)?;
    (slot_crc(seq, payload) == crc).then(|| (Snapshot { seq, payload: payload.to_vec() }, r.pos()))
}

/// Whether the recovery-file header is intact.
#[must_use]
pub fn header_ok(bytes: &[u8]) -> bool {
    bytes.get(..HEADER_LEN) == Some(&header()[..])
}

/// Every slot whose marker and CRC verify, in file order (payloads not validated).
#[must_use]
pub fn decode_slots(bytes: &[u8]) -> Vec<Snapshot> {
    let mut out = Vec::new();
    let mut at = HEADER_LEN.min(bytes.len());
    let mut candidates = 0;
    while candidates < MAX_CANDIDATES {
        let Some(p) = bytes.get(at..).and_then(|s| s.windows(4).position(|w| w == SLOT_MAGIC)).map(|p| p + at) else {
            break;
        };
        candidates += 1;
        match parse_slot(bytes, p) {
            Some((snap, used)) => {
                out.push(snap);
                at = p + used;
            }
            None => at = p + 1,
        }
    }
    out
}

fn newest_valid<E>(slots: Vec<Snapshot>, validate: &dyn Fn(&[u8]) -> Result<(), E>) -> Option<Snapshot> {
    slots.into_iter().filter(|s| validate(&s.payload).is_ok()).max_by_key(|s| s.seq)
}

/// Reads the newest valid snapshot. `Ok(None)` if there is no recovery file.
///
/// # Errors
/// [`RecoveryError::Io`] if the file cannot be read, [`RecoveryError::NoValidSlot`] if
/// it exists but no slot verifies.
pub fn read_newest<F, E>(
    fs: &F,
    path: &Path,
    validate: &dyn Fn(&[u8]) -> Result<(), E>,
) -> Result<Option<Snapshot>, RecoveryError>
where
    F: Fs + ?Sized,
{
    let bytes = match fs.read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(RecoveryError::Io { path: path.to_path_buf(), source }),
    };
    newest_valid(decode_slots(&bytes), validate)
        .map(Some)
        .ok_or_else(|| RecoveryError::NoValidSlot { path: path.to_path_buf() })
}

/// Writes a new snapshot, keeping the previous newest valid one as the other slot.
/// Returns the new slot's sequence number. `validate` checks payloads (for `.gus`
/// images: [`crate::container::verify`]); the written file is re-read and must contain
/// the new snapshot in a valid slot before it replaces the old file.
///
/// # Errors
/// See [`RecoveryError`]. On error the previous recovery file is untouched.
pub fn write_snapshot<F, E>(
    fs: &F,
    path: &Path,
    payload: &[u8],
    validate: &dyn Fn(&[u8]) -> Result<(), E>,
) -> Result<u64, RecoveryError>
where
    F: Fs + ?Sized,
    E: fmt::Display,
{
    validate(payload).map_err(|e| RecoveryError::PayloadInvalid(e.to_string()))?;
    let existing = match fs.read(path) {
        Ok(b) => decode_slots(&b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(source) => return Err(RecoveryError::Io { path: path.to_path_buf(), source }),
    };
    let max_seq = existing.iter().map(|s| s.seq).max().unwrap_or(0);
    let seq = max_seq.checked_add(1).ok_or(RecoveryError::SequenceOverflow)?;
    let prev = newest_valid(existing, validate);
    let bytes = match &prev {
        Some(p) => encode(&[(p.seq, &p.payload), (seq, payload)]),
        None => encode(&[(seq, payload)]),
    };
    let check = |b: &[u8]| -> Result<(), String> {
        if !header_ok(b) {
            return Err("recovery header damaged".into());
        }
        let slots = decode_slots(b);
        match slots.iter().find(|s| s.seq == seq) {
            Some(s) => validate(&s.payload).map_err(|e| e.to_string()),
            None => Err("the new snapshot slot did not verify".into()),
        }
    };
    durable_write(fs, path, &bytes, &check).map_err(RecoveryError::Write)?;
    Ok(seq)
}

/// Deletes the recovery file (clean close). Missing files are fine.
///
/// # Errors
/// I/O errors other than `NotFound`.
pub fn remove<F: Fs + ?Sized>(fs: &F, path: &Path) -> io::Result<()> {
    match fs.remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => fs.sync_dir(dir),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FaultFs;

    fn ok(_: &[u8]) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn two_slots_rotate() {
        let fs = FaultFs::new(1);
        let p = Path::new("/r.grec");
        assert_eq!(read_newest(&fs, p, &ok).unwrap(), None);
        assert_eq!(write_snapshot(&fs, p, b"one", &ok).unwrap(), 1);
        assert_eq!(write_snapshot(&fs, p, b"two", &ok).unwrap(), 2);
        assert_eq!(write_snapshot(&fs, p, b"three", &ok).unwrap(), 3);
        let bytes = fs.read(p).unwrap();
        let slots = decode_slots(&bytes);
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].payload, b"two");
        assert_eq!(slots[1].payload, b"three");
        assert_eq!(read_newest(&fs, p, &ok).unwrap().unwrap().payload, b"three");
        assert_eq!(fs.files(), vec![p.to_path_buf()]);
    }

    #[test]
    fn damaged_newest_slot_falls_back() {
        let mut b = encode(&[(4, b"older"), (5, b"newer")]);
        let last = b.len() - 1;
        b[last] ^= 1;
        let slots = decode_slots(&b);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].seq, 4);
        // A damaged first slot does not hide the second.
        let mut b = encode(&[(4, b"older"), (5, b"newer")]);
        b[HEADER_LEN + 5] ^= 0xff;
        let slots = decode_slots(&b);
        assert_eq!(slots.iter().map(|s| s.seq).collect::<Vec<_>>(), vec![5]);
    }

    #[test]
    fn payload_validation_selects_slot() {
        let fs = FaultFs::new(2);
        let p = Path::new("/r.grec");
        let validate = |b: &[u8]| if b.starts_with(b"ok") { Ok(()) } else { Err("bad") };
        write_snapshot(&fs, p, b"ok-1", &validate).unwrap();
        assert!(matches!(write_snapshot(&fs, p, b"bad", &validate), Err(RecoveryError::PayloadInvalid(_))));
        // A file with a CRC-valid but semantically invalid newest slot.
        let fs2 = FaultFs::new(3);
        crate::durable::durable_write_unchecked(&fs2, p, &encode(&[(1, b"ok-a"), (2, b"garbage")])).unwrap();
        assert_eq!(read_newest(&fs2, p, &validate).unwrap().unwrap().payload, b"ok-a");
        assert_eq!(write_snapshot(&fs2, p, b"ok-b", &validate).unwrap(), 3);
        let slots = decode_slots(&fs2.read(p).unwrap());
        assert_eq!(slots.iter().map(|s| s.seq).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn remove_is_idempotent() {
        let fs = FaultFs::new(4);
        let p = Path::new("/r.grec");
        write_snapshot(&fs, p, b"x", &ok).unwrap();
        remove(&fs, p).unwrap();
        remove(&fs, p).unwrap();
        assert!(fs.files().is_empty());
    }
}
