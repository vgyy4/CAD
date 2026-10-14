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
///
/// Version 2 stores the feature list as a separate list-root object naming
/// content-defined chunks, so a parameter edit shares the whole list with the previous
/// snapshot and a feature edit only adds its chunk and a new root, instead of repeating
/// every feature hash (PERFORMANCE.md "History overhead"). Version 1 (the full list
/// inline) is still read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub units: LengthUnit,
    pub params: Hash,
    /// The flattened feature list.
    pub features: Vec<Hash>,
    /// Objects holding the feature list (version 2: the list root and its chunks; empty
    /// for version 1).
    pub chunks: Vec<Hash>,
}

/// Feature-list chunks end after a feature whose hash's first byte is a multiple of
/// this (content-defined, about 16 features per chunk), or at [`MAX_CHUNK`].
const CHUNK_DIV: u8 = 16;
const MAX_CHUNK: usize = 64;

/// Splits a feature list into chunks at content-defined boundaries, so inserting or
/// changing one feature changes only its own chunk.
fn chunk_features(features: &[Hash]) -> Vec<&[Hash]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, h) in features.iter().enumerate() {
        let len = i + 1 - start;
        if h.0.first().is_some_and(|b| b % CHUNK_DIV == 0) || len >= MAX_CHUNK {
            out.push(features.get(start..=i).unwrap_or(&[]));
            start = i + 1;
        }
    }
    if start < features.len() {
        out.push(features.get(start..).unwrap_or(&[]));
    }
    out
}

fn chunk_text(features: &[Hash]) -> String {
    let mut s = String::from("featurelist 1\n");
    for f in features {
        s.push_str(&format!("feature {f}\n"));
    }
    s
}

fn parse_chunk(s: &str) -> Result<Vec<Hash>, String> {
    let mut lines = s.lines();
    if lines.next() != Some("featurelist 1") {
        return Err("not a feature-list object".into());
    }
    lines
        .map(|l| l.strip_prefix("feature ").and_then(Hash::from_hex).ok_or_else(|| "bad feature line".into()))
        .collect()
}

impl Snapshot {
    /// The version-2 text.
    fn to_text_v2(units: LengthUnit, params: Hash, list: Hash) -> String {
        let u = match units {
            LengthUnit::Millimetre => "mm",
            LengthUnit::Inch => "in",
        };
        format!("snapshot 2\nunits {u}\nparams {params}\nlist {list}\n")
    }

    /// Version-1 text (the full feature list inline).
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
        Ok(Snapshot { units, params, features, chunks: Vec::new() })
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

/// A document's objects prepared by [`ObjectStore::stage_document`].
#[derive(Debug, Clone)]
pub struct StagedDocument {
    snapshot: Hash,
    objects: Vec<(Hash, ObjectKind, Vec<u8>)>,
}

impl StagedDocument {
    /// The snapshot hash the document will have.
    pub fn hash(&self) -> Hash {
        self.snapshot
    }
}

fn unreachable_hash() -> Hash {
    // `stage_document` with an unlimited size never fails.
    Hash([0; 32])
}

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
        match self.stage_document(doc, usize::MAX) {
            Ok(s) => self.commit_staged(s),
            Err(_) => unreachable_hash(),
        }
    }

    /// Prepares a document's objects without storing them, refusing any object larger
    /// than `max_object` bytes (`Err((what, len))`). Store them with
    /// [`ObjectStore::commit_staged`].
    pub fn stage_document(&self, doc: &Document, max_object: usize) -> Result<StagedDocument, (String, usize)> {
        let mut objects = Vec::new();
        let mut stage = |kind: ObjectKind, what: &dyn Fn() -> String, text: String| -> Result<Hash, (String, usize)> {
            if text.len() > max_object {
                return Err((what(), text.len()));
            }
            let h = Hash::of(text.as_bytes());
            if !self.objects.contains_key(&h) {
                objects.push((h, kind, text.into_bytes()));
            }
            Ok(h)
        };
        let params = stage(ObjectKind::Params, &|| "the parameter table".into(), print_params_object(&doc.params))?;
        let mut features = Vec::with_capacity(doc.features.len());
        for f in &doc.features {
            features.push(stage(ObjectKind::Feature, &|| format!("feature '{}'", f.name), print_feature_object(f))?);
        }
        let mut root = String::from("featurelist-root 1\n");
        for c in chunk_features(&features) {
            let h = stage(ObjectKind::Snapshot, &|| "a feature-list chunk".into(), chunk_text(c))?;
            root.push_str(&format!("chunk {h}\n"));
        }
        let list = stage(ObjectKind::Snapshot, &|| "the feature list".into(), root)?;
        let text = Snapshot::to_text_v2(doc.units, params, list);
        let snapshot = stage(ObjectKind::Snapshot, &|| "the model snapshot".into(), text)?;
        Ok(StagedDocument { snapshot, objects })
    }

    /// Stores staged objects; returns the snapshot hash.
    pub fn commit_staged(&mut self, staged: StagedDocument) -> Hash {
        for (h, k, b) in staged.objects {
            self.objects.entry(h).or_insert_with(|| (k, Arc::from(b)));
        }
        staged.snapshot
    }

    pub fn snapshot(&self, h: Hash) -> Result<Snapshot, ObjectError> {
        let text = self.text(h)?;
        let corrupt = |reason: String| ObjectError::Corrupt { hash: h, reason };
        let Some(rest) = text.strip_prefix("snapshot 2\n") else {
            return Snapshot::from_text(text).map_err(corrupt);
        };
        // Version 2: the same header lines, then the list root.
        let v1 = format!("snapshot 1\n{}", rest.lines().take(2).collect::<Vec<_>>().join("\n"));
        let head = Snapshot::from_text(&v1).map_err(corrupt)?;
        let mut tail = rest.lines().skip(2);
        let list = tail
            .next()
            .and_then(|l| l.strip_prefix("list "))
            .and_then(Hash::from_hex)
            .ok_or_else(|| corrupt("bad list line".into()))?;
        if tail.next().is_some() {
            return Err(corrupt("trailing lines".into()));
        }
        let root = self.text(list)?;
        let bad_root = |r: &str| ObjectError::Corrupt { hash: list, reason: r.to_string() };
        let mut rl = root.lines();
        if rl.next() != Some("featurelist-root 1") {
            return Err(bad_root("not a feature-list root"));
        }
        let mut features = Vec::new();
        let mut chunks = vec![list];
        for l in rl {
            let c = l.strip_prefix("chunk ").and_then(Hash::from_hex).ok_or_else(|| bad_root("bad chunk line"))?;
            features.extend(parse_chunk(self.text(c)?).map_err(|r| ObjectError::Corrupt { hash: c, reason: r })?);
            chunks.push(c);
        }
        Ok(Snapshot { units: head.units, params: head.params, features, chunks })
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

    /// Hashes of every object a snapshot references (including itself). Returns false
    /// when the snapshot (or one of its features) could not be read, so the set may be
    /// incomplete and must not be used to delete objects (review N6).
    pub fn snapshot_closure(&self, snapshot: Hash, out: &mut std::collections::BTreeSet<Hash>) -> bool {
        if !out.insert(snapshot) {
            return true;
        }
        let Ok(s) = self.snapshot(snapshot) else {
            return false;
        };
        out.insert(s.params);
        out.extend(s.chunks.iter().copied());
        out.extend(s.features.iter().copied());
        let mut complete = true;
        for f in &s.features {
            match self.feature(*f) {
                Ok(f) => complete &= self.feature_blobs(&f.kind, out, 0),
                Err(_) => complete = false,
            }
        }
        complete
    }

    /// The stored data a feature needs: an import's file, a component's pinned document
    /// and, inside it, what its own features need (ADR 0026 §7). False when a stored
    /// component document cannot be read or nesting is deeper than any valid project.
    fn feature_blobs(
        &self,
        kind: &gusset_doc::FeatureKind,
        out: &mut std::collections::BTreeSet<Hash>,
        depth: u32,
    ) -> bool {
        match kind {
            gusset_doc::FeatureKind::Import(i) => {
                if let Some(b) = Hash::from_hex(&i.blob) {
                    out.insert(b);
                }
                true
            }
            gusset_doc::FeatureKind::Component(c) => {
                let Some(h) = Hash::from_hex(&c.document) else { return true };
                if !out.insert(h) {
                    return true;
                }
                if depth >= 32 {
                    return false;
                }
                if self.get(h).is_err() {
                    return true; // not stored: nothing else to keep for it
                }
                // Stored but unreadable: it may reference data a repair could still use.
                let Some(doc) = self.text(h).ok().and_then(|t| Document::from_text(t).ok()) else {
                    return false;
                };
                // Every feature, even after an incomplete one: keep all that can be seen.
                let mut complete = true;
                for f in &doc.features {
                    complete &= self.feature_blobs(&f.kind, out, depth + 1);
                }
                complete
            }
            _ => true,
        }
    }
}
