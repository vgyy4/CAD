//! Content-addressed immutable objects: features, parameter tables, snapshots, blobs.

use gusset_doc::text::{parse_feature_object, parse_params_object, print_feature_object, print_params_object};
use gusset_doc::{Document, Feature, ParseError};
use gusset_units::LengthUnit;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// BLAKE3 hash of an object's canonical bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    pub fn of(bytes: &[u8]) -> Hash {
        Hash(*blake3::hash(bytes).as_bytes())
    }

    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    pub fn from_hex(s: &str) -> Option<Hash> {
        if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(Hash(out))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.to_hex().get(..12).unwrap_or(""))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// What an object is; mirrors the `.gus` record kinds (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectKind {
    Feature,
    Params,
    Snapshot,
    Blob,
}

/// A document state: units, the parameter table and the ordered feature list, all by hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub units: LengthUnit,
    pub params: Hash,
    pub features: Vec<Hash>,
}

impl Snapshot {
    pub fn to_text(&self) -> String {
        let mut s = String::from("snapshot 1\n");
        s.push_str(match self.units {
            LengthUnit::Millimetre => "units mm\n",
            LengthUnit::Inch => "units in\n",
        });
        s.push_str(&format!("params {}\n", self.params));
        for f in &self.features {
            s.push_str(&format!("feature {f}\n"));
        }
        s
    }

    pub fn from_text(s: &str) -> Result<Snapshot, String> {
        let mut lines = s.lines();
        if lines.next() != Some("snapshot 1") {
            return Err("not a snapshot object".into());
        }
        let units = match lines.next() {
            Some("units mm") => LengthUnit::Millimetre,
            Some("units in") => LengthUnit::Inch,
            _ => return Err("snapshot: bad units line".into()),
        };
        let params = lines
            .next()
            .and_then(|l| l.strip_prefix("params "))
            .and_then(Hash::from_hex)
            .ok_or("snapshot: bad params line")?;
        let mut features = Vec::new();
        for l in lines {
            let h = l.strip_prefix("feature ").and_then(Hash::from_hex).ok_or("snapshot: bad feature line")?;
            features.push(h);
        }
        Ok(Snapshot { units, params, features })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObjectError {
    Missing(Hash),
    Corrupt { hash: Hash, reason: String },
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectError::Missing(h) => write!(f, "history object {h:?} is missing"),
            ObjectError::Corrupt { hash, reason } => write!(f, "history object {hash:?} is damaged: {reason}"),
        }
    }
}

impl std::error::Error for ObjectError {}

/// An in-memory content-addressed object store. Objects are never mutated; storing the
/// same bytes twice is free.
#[derive(Debug, Clone, Default)]
pub struct ObjectStore {
    objects: BTreeMap<Hash, (ObjectKind, Arc<[u8]>)>,
}

impl ObjectStore {
    pub fn put(&mut self, kind: ObjectKind, bytes: &[u8]) -> Hash {
        let h = Hash::of(bytes);
        self.objects.entry(h).or_insert_with(|| (kind, Arc::from(bytes)));
        h
    }

    /// Inserts an object read from storage, verifying its hash.
    pub fn insert_verified(&mut self, kind: ObjectKind, hash: Hash, bytes: Arc<[u8]>) -> Result<(), ObjectError> {
        if Hash::of(&bytes) != hash {
            return Err(ObjectError::Corrupt { hash, reason: "content does not match its hash".into() });
        }
        self.objects.entry(hash).or_insert((kind, bytes));
        Ok(())
    }

    pub fn get(&self, h: Hash) -> Result<&Arc<[u8]>, ObjectError> {
        self.objects.get(&h).map(|(_, b)| b).ok_or(ObjectError::Missing(h))
    }

    pub fn kind(&self, h: Hash) -> Option<ObjectKind> {
        self.objects.get(&h).map(|(k, _)| *k)
    }

    pub fn contains(&self, h: Hash) -> bool {
        self.objects.contains_key(&h)
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Total stored bytes (uncompressed).
    pub fn byte_size(&self) -> usize {
        self.objects.values().map(|(_, b)| b.len()).sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Hash, ObjectKind, &Arc<[u8]>)> {
        self.objects.iter().map(|(h, (k, b))| (*h, *k, b))
    }

    /// Drops every object not in `keep`.
    pub fn retain(&mut self, keep: &std::collections::BTreeSet<Hash>) {
        self.objects.retain(|h, _| keep.contains(h));
    }

    fn text(&self, h: Hash) -> Result<&str, ObjectError> {
        std::str::from_utf8(self.get(h)?).map_err(|_| ObjectError::Corrupt { hash: h, reason: "not UTF-8".into() })
    }

    /// Stores a document and returns its snapshot hash.
    pub fn put_document(&mut self, doc: &Document) -> Hash {
        let params = self.put(ObjectKind::Params, print_params_object(&doc.params).as_bytes());
        let features =
            doc.features.iter().map(|f| self.put(ObjectKind::Feature, print_feature_object(f).as_bytes())).collect();
        let snap = Snapshot { units: doc.units, params, features };
        self.put(ObjectKind::Snapshot, snap.to_text().as_bytes())
    }

    pub fn snapshot(&self, h: Hash) -> Result<Snapshot, ObjectError> {
        Snapshot::from_text(self.text(h)?).map_err(|reason| ObjectError::Corrupt { hash: h, reason })
    }

    pub fn feature(&self, h: Hash) -> Result<Feature, ObjectError> {
        parse_feature_object(self.text(h)?)
            .map_err(|e: ParseError| ObjectError::Corrupt { hash: h, reason: e.to_string() })
    }

    /// Materializes the document of a snapshot.
    pub fn document(&self, snapshot: Hash) -> Result<Document, ObjectError> {
        let snap = self.snapshot(snapshot)?;
        let params = parse_params_object(self.text(snap.params)?)
            .map_err(|e| ObjectError::Corrupt { hash: snap.params, reason: e.to_string() })?;
        let features = snap.features.iter().map(|h| self.feature(*h)).collect::<Result<Vec<_>, _>>()?;
        Ok(Document { units: snap.units, params, features })
    }

    /// Hashes of every object a snapshot references (including itself).
    pub fn snapshot_closure(&self, snapshot: Hash, out: &mut std::collections::BTreeSet<Hash>) {
        if !out.insert(snapshot) {
            return;
        }
        if let Ok(s) = self.snapshot(snapshot) {
            out.insert(s.params);
            out.extend(s.features.iter().copied());
            for f in &s.features {
                if let Ok(Feature { kind: gusset_doc::FeatureKind::Import(i), .. }) = self.feature(*f)
                    && let Some(b) = Hash::from_hex(&i.blob)
                {
                    out.insert(b);
                }
            }
        }
    }
}
