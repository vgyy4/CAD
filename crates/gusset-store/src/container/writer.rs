use std::collections::{BTreeMap, HashMap};

use super::{
    Codec, Container, DEFAULT_LEVEL, FORMAT_VERSION, HEADER_LEN, Header, INDEX_MAGIC, Kind, Limits, MAX_ROOT_NAME,
    RECORD_MAGIC, Trailer, WriteError,
};
use crate::Hash;
use crate::checksum::crc32c;

/// Header fields supplied when finishing a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveInfo<'a> {
    /// Save sequence number (strictly increasing per project).
    pub save_seq: u64,
    /// Project creation time, Unix seconds.
    pub created_unix: u64,
    /// App version (truncated to 16 bytes).
    pub app_version: &'a str,
}

#[derive(Debug)]
struct Pending {
    kind: Kind,
    hash: Hash,
    /// The complete encoded record, magic through CRC.
    bytes: Vec<u8>,
}

/// Builds a `.gus` container in memory. Output is deterministic: records appear in
/// insertion order and roots sorted by name.
#[derive(Debug)]
pub struct ContainerWriter {
    records: Vec<Pending>,
    by_hash: HashMap<Hash, usize>,
    roots: BTreeMap<String, Vec<u8>>,
    level: i32,
    limits: Limits,
    total_len: u64,
}

impl Default for ContainerWriter {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_record(kind: Kind, codec: Codec, hash: &Hash, stored: &[u8], raw_len: u32) -> Result<Vec<u8>, WriteError> {
    let stored_len = u32::try_from(stored.len()).map_err(|_| WriteError::RecordTooLarge {
        kind,
        len: stored.len() as u64,
        max: u32::MAX,
    })?;
    let mut b = Vec::with_capacity(super::RECORD_HEADER_LEN + stored.len() + 4);
    b.extend_from_slice(&RECORD_MAGIC);
    b.push(kind.0);
    b.push(codec.to_u8());
    b.extend_from_slice(hash.as_bytes());
    b.extend_from_slice(&stored_len.to_le_bytes());
    b.extend_from_slice(&raw_len.to_le_bytes());
    b.extend_from_slice(stored);
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    Ok(b)
}

impl ContainerWriter {
    /// An empty writer with default limits and zstd level 3.
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            by_hash: HashMap::new(),
            roots: BTreeMap::new(),
            level: DEFAULT_LEVEL,
            limits: Limits::default(),
            total_len: 0,
        }
    }

    /// Sets the zstd level (clamped to 1..=19; higher levels would exceed the window cap
    /// readers enforce).
    #[must_use]
    pub fn with_level(mut self, level: i32) -> Self {
        self.level = level.clamp(1, 19);
        self
    }

    /// Uses `limits` (the writer refuses to produce files that a reader with the same
    /// limits would reject).
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    fn check_new(&self, kind: Kind, hash: &Hash) -> Result<bool, WriteError> {
        if kind.0 == 0 {
            return Err(WriteError::InvalidKind);
        }
        if let Some(&i) = self.by_hash.get(hash) {
            let existing = self.records.get(i).map_or(kind, |r| r.kind);
            if existing != kind {
                return Err(WriteError::KindConflict { hash: *hash, existing, new: kind });
            }
            return Ok(false);
        }
        if self.records.len() >= self.limits.max_records as usize {
            return Err(WriteError::TooManyRecords);
        }
        Ok(true)
    }

    fn push(&mut self, kind: Kind, hash: Hash, bytes: Vec<u8>) {
        self.total_len = self.total_len.saturating_add(bytes.len() as u64);
        self.by_hash.insert(hash, self.records.len());
        self.records.push(Pending { kind, hash, bytes });
    }

    /// Adds a record and returns its hash. Adding the same content again (same kind) is
    /// a no-op that returns the same hash.
    ///
    /// # Errors
    /// [`WriteError::RecordTooLarge`], [`WriteError::KindConflict`],
    /// [`WriteError::InvalidKind`], [`WriteError::TooManyRecords`] or
    /// [`WriteError::Compress`].
    pub fn add(&mut self, kind: Kind, data: &[u8]) -> Result<Hash, WriteError> {
        let max = self.limits.max_raw_for(kind);
        let raw_len = u32::try_from(data.len()).ok().filter(|&l| l <= max).ok_or(WriteError::RecordTooLarge {
            kind,
            len: data.len() as u64,
            max,
        })?;
        let hash = Hash::of(data);
        if !self.check_new(kind, &hash)? {
            return Ok(hash);
        }
        // Miri cannot execute zstd's C code. Under Miri the writer stores every record
        // raw, an encoding it already uses whenever compression does not help, so the
        // rest of the codec (framing, hashing, index, roots, trailer) is still checked.
        let compressed = if data.is_empty() || cfg!(miri) {
            None
        } else {
            Some(zstd::bulk::compress(data, self.level).map_err(|e| WriteError::Compress(e.to_string()))?)
        };
        let bytes = match compressed {
            Some(c) if c.len() < data.len() => encode_record(kind, Codec::Zstd, &hash, &c, raw_len)?,
            _ => encode_record(kind, Codec::Raw, &hash, data, raw_len)?,
        };
        self.push(kind, hash, bytes);
        Ok(hash)
    }

    /// Copies a record from an already-opened container byte-for-byte (no
    /// recompression). Returns its hash.
    ///
    /// # Errors
    /// [`WriteError::NotInSource`] if `src` has no such record, or the errors of
    /// [`ContainerWriter::add`] for conflicts and limits.
    pub fn copy_from(&mut self, src: &Container<'_>, hash: &Hash) -> Result<Hash, WriteError> {
        let (info, raw) = src.record_bytes(hash).ok_or(WriteError::NotInSource { hash: *hash })?;
        let max = self.limits.max_raw_for(info.kind);
        if info.raw_len > max {
            return Err(WriteError::RecordTooLarge { kind: info.kind, len: u64::from(info.raw_len), max });
        }
        if !self.check_new(info.kind, hash)? {
            return Ok(*hash);
        }
        self.push(info.kind, *hash, raw.to_vec());
        Ok(*hash)
    }

    /// Whether a record with this hash has been added.
    #[must_use]
    pub fn contains(&self, hash: &Hash) -> bool {
        self.by_hash.contains_key(hash)
    }

    /// Sets a named root (e.g. `"branches"` → hash bytes of the branch table).
    ///
    /// # Errors
    /// [`WriteError::InvalidRootName`] (empty, over 255 bytes, or containing control
    /// characters), [`WriteError::RootTooLarge`] or [`WriteError::TooManyRoots`].
    pub fn set_root(&mut self, name: &str, value: &[u8]) -> Result<(), WriteError> {
        if name.is_empty() || name.len() > MAX_ROOT_NAME || name.chars().any(char::is_control) {
            return Err(WriteError::InvalidRootName);
        }
        if value.len() > self.limits.max_root_len as usize {
            return Err(WriteError::RootTooLarge);
        }
        if !self.roots.contains_key(name) && self.roots.len() >= self.limits.max_roots as usize {
            return Err(WriteError::TooManyRoots);
        }
        self.roots.insert(name.to_owned(), value.to_vec());
        Ok(())
    }

    /// Assembles the file.
    ///
    /// # Errors
    /// [`WriteError::FileTooLarge`] if the result would exceed the size limit.
    pub fn finish(self, info: &SaveInfo<'_>) -> Result<Vec<u8>, WriteError> {
        let header = Header {
            format_version: FORMAT_VERSION,
            flags: 0,
            save_seq: info.save_seq,
            created_unix: info.created_unix,
            app_version: info.app_version.to_owned(),
        };
        let estimate = (HEADER_LEN as u64)
            .saturating_add(self.total_len)
            .saturating_add((self.records.len() as u64).saturating_mul(super::INDEX_ENTRY_LEN as u64));
        if estimate > self.limits.max_file_size {
            return Err(WriteError::FileTooLarge { len: estimate, max: self.limits.max_file_size });
        }
        let mut out = Vec::with_capacity(usize::try_from(estimate).unwrap_or(0).saturating_add(1024));
        out.extend_from_slice(&header.encode());
        let mut offsets = Vec::with_capacity(self.records.len());
        for r in &self.records {
            offsets.push(out.len() as u64);
            out.extend_from_slice(&r.bytes);
        }
        let index_offset = out.len() as u64;
        let mut index = Vec::new();
        index.extend_from_slice(&INDEX_MAGIC);
        index.extend_from_slice(&u32::try_from(self.records.len()).unwrap_or(u32::MAX).to_le_bytes());
        for (r, off) in self.records.iter().zip(&offsets) {
            index.push(r.kind.0);
            index.extend_from_slice(r.hash.as_bytes());
            index.extend_from_slice(&off.to_le_bytes());
        }
        index.extend_from_slice(&u32::try_from(self.roots.len()).unwrap_or(u32::MAX).to_le_bytes());
        for (name, value) in &self.roots {
            index.extend_from_slice(&u16::try_from(name.len()).unwrap_or(u16::MAX).to_le_bytes());
            index.extend_from_slice(name.as_bytes());
            index.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
            index.extend_from_slice(value);
        }
        let crc = crc32c(&index);
        index.extend_from_slice(&crc.to_le_bytes());
        let index_len = u32::try_from(index.len())
            .map_err(|_| WriteError::FileTooLarge { len: index.len() as u64, max: u64::from(u32::MAX) })?;
        out.extend_from_slice(&index);
        out.extend_from_slice(&Trailer { index_offset, index_len, save_seq: info.save_seq }.encode());
        if out.len() as u64 > self.limits.max_file_size {
            return Err(WriteError::FileTooLarge { len: out.len() as u64, max: self.limits.max_file_size });
        }
        Ok(out)
    }
}
