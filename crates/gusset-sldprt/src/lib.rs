//! SolidWorks `.sldprt` reader (ADR 0043): finds the embedded Parasolid partition and
//! returns it as an XT transmit file for `gusset-xt`. Nothing else is read (no
//! display lists; assemblies only as a component list, see [`read_assembly`]; the feature
//! history only as a feature tree, see [`features::feature_tree`]).
//!
//! Two containers exist. Files up to about SolidWorks 2014 are OLE2 compound files whose
//! `Config-<n>-Partition` stream holds the partition; newer files use a custom chunk
//! container with the same stream under `Contents/Config-<n>-Partition`. In both, the
//! stream is a list of blocks (a length, a 16-byte tag, the inflated and deflated sizes
//! and a zlib stream); the first block is the Parasolid partition, the second its undo
//! deltas. The layout was derived from sample files and the STEP files SolidWorks wrote
//! for them, with no other SolidWorks reader consulted. Input is untrusted: every count,
//! chain and inflated size is bounded by [`Limits`].

mod assembly;
/// The OLE2 compound file reader, shared with `gusset-ole` (Solid Edge, Inventor; ADR 0046).
pub mod cfb;
mod chunks;
mod crc;
/// The feature history as a Gusset feature tree (ADR 0043 amendment).
pub mod features;

pub use assembly::{Assembly, Component, ComponentKind, MAX_COMPONENTS, embedded_part, read_assembly};

/// Why a file cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SldprtError {
    /// Neither container's signature is present.
    NotSldprt,
    Malformed(String),
    TooLarge(String),
    /// A container without a Parasolid partition (an assembly, a drawing, a file saved
    /// without geometry).
    NoGeometry(String),
}

impl std::fmt::Display for SldprtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SldprtError::NotSldprt => write!(f, "not a SolidWorks part file (unknown container)"),
            SldprtError::Malformed(m) => write!(f, "the SolidWorks file is damaged: {m}"),
            SldprtError::TooLarge(m) => write!(f, "the SolidWorks file is too large: {m}"),
            SldprtError::NoGeometry(m) => write!(f, "the SolidWorks file has no solid geometry: {m}"),
        }
    }
}

impl std::error::Error for SldprtError {}

/// Bounds on what a file may make the reader allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Streams listed or accepted, and (times 16) marker candidates scanned.
    pub max_streams: usize,
    /// One stream, inflated.
    pub max_stream_bytes: usize,
    /// All inflated streams together.
    pub max_total_bytes: usize,
    /// Inflated size divided by deflated size (deflate itself cannot exceed about 1032).
    pub max_ratio: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_streams: 8192, max_stream_bytes: 256 << 20, max_total_bytes: 512 << 20, max_ratio: 2048 }
    }
}

/// Which container a file uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// OLE2 compound file (SolidWorks up to about 2014).
    Ole2,
    /// Chunk container (about 2015 on).
    Chunks,
}

/// The partition found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// A binary XT transmit file (`PS` header), ready for `gusset_xt::to_step`.
    pub xt: Vec<u8>,
    pub container: Container,
    /// The configuration the partition belongs to (`Config-0`, ...).
    pub config: String,
    pub warnings: Vec<String>,
}

/// Most bodies read from a `LocalBodies` stream.
pub const MAX_LOCAL_BODIES: usize = 4096;

/// The bodies of a part whose final partition holds none: a part made by an imported
/// feature keeps them under `Config-<n>-FeatureBodies/LocalBodies`, one Parasolid transmit
/// each. The stream is a count and, per body, a short name record followed by a block of
/// the partition's kind (a length, a 16-byte tag shared by all blocks, the inflated and
/// deflated sizes and a zlib stream). Derived from a sample with no other reader consulted.
/// Empty when the file has no such stream.
///
/// # Errors
/// [`SldprtError`] for unknown containers and damaged or over-limit files.
pub fn local_bodies(
    bytes: &[u8],
    limits: &Limits,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Vec<u8>>, SldprtError> {
    let is_local = |n: &str| n.ends_with("LocalBodies") && (n == "LocalBodies" || n.contains("-FeatureBodies/"));
    let stream = match detect(bytes).ok_or(SldprtError::NotSldprt)? {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, limits)?;
            match f.streams.iter().position(|e| is_local(&e.name)) {
                Some(i) => f.read(i, limits)?,
                None => return Ok(Vec::new()),
            }
        }
        Container::Chunks => {
            // The name first (no inflating), then only that record (security review T2).
            let Some(chosen) = chunks::names(bytes, limits, is_local).into_iter().min() else {
                return Ok(Vec::new());
            };
            match chunks::streams(bytes, limits, |n| n == chosen)?.pop() {
                Some(st) => st.data,
                None => return Ok(Vec::new()),
            }
        }
    };
    local_blocks(&stream, limits, cancel)
}

/// The Parasolid transmits of a `LocalBodies` stream.
fn local_blocks(
    stream: &[u8],
    limits: &Limits,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Vec<u8>>, SldprtError> {
    let u32_at = |off: usize| -> Option<usize> {
        usize::try_from(u32::from_le_bytes(stream.get(off..off.checked_add(4)?)?.try_into().ok()?)).ok()
    };
    // A block at `at`: its length covers the header and the zlib data, which starts `78`.
    let block = |at: usize| -> Option<(usize, usize, usize)> {
        let (len, plain, deflated) = (u32_at(at)?, u32_at(at + 20)?, u32_at(at + 24)?);
        let end = at.checked_add(4)?.checked_add(len)?;
        (len >= 24usize.checked_add(deflated)? && end <= stream.len() && stream.get(at + 28) == Some(&0x78))
            .then_some((end, plain, deflated))
    };
    let Some(first) = (0..stream.len().min(4096)).find(|i| block(*i).is_some()) else { return Ok(Vec::new()) };
    let Some(tag) = stream.get(first + 4..first + 20) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    let (mut at, mut spent) = (first, 0usize);
    let mut blocks = 0usize;
    while let Some((end, plain, deflated)) = block(at) {
        // Every block counts, not only the bodies (security review T2), and a stop is
        // honoured between blocks (T1).
        blocks += 1;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(SldprtError::TooLarge("cancelled".into()));
        }
        if blocks > MAX_LOCAL_BODIES {
            return Err(SldprtError::TooLarge(format!("more than {MAX_LOCAL_BODIES} bodies")));
        }
        spent = spent.saturating_add(plain);
        if plain > limits.max_stream_bytes
            || spent > limits.max_total_bytes
            || plain / limits.max_ratio.max(1) > deflated
        {
            return Err(SldprtError::TooLarge("the bodies inflate beyond the limit".into()));
        }
        let data = stream.get(at + 28..at + 28 + deflated).unwrap_or_default();
        let xt = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, plain.saturating_add(1))
            .map_err(|e| SldprtError::Malformed(format!("a body block does not inflate ({:?})", e.status)))?;
        if xt.get(..2) == Some(b"PS") {
            out.push(xt);
        }
        // The next block: after its name record, carrying the same tag.
        let rest = stream.get(end..).unwrap_or_default();
        let Some(k) = rest.windows(tag.len()).position(|w| w == tag) else { break };
        let Some(next) = (end + k).checked_sub(4).filter(|n| *n >= end) else { break };
        at = next;
    }
    Ok(out)
}

/// Which container `bytes` looks like, from its first bytes only.
#[must_use]
pub fn detect(bytes: &[u8]) -> Option<Container> {
    if bytes.get(..8) == Some(&cfb::SIGNATURE[..]) {
        Some(Container::Ole2)
    } else if chunks::looks_like(bytes) {
        Some(Container::Chunks)
    } else {
        None
    }
}

/// The configuration number of a `Config-<n>-Partition` stream name (not the ghost
/// partition), with or without the `Contents/` prefix.
fn partition_config(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("Contents/").unwrap_or(name).strip_prefix("Config-")?;
    rest.strip_suffix("-Partition")?.parse().ok()
}

/// The warning for a file that ends early (dogfooding F12-9).
const CUT_SHORT: &str = "the file ends early (an incomplete download or copy?): the geometry found may be incomplete";

const PARTITION_TAG: &[u8] = b": TRANSMIT FILE (partition)";

/// Splits a partition stream into its blocks and returns the first Parasolid partition.
fn partition_block(stream: &[u8], limits: &Limits, spent: &mut usize) -> Result<Vec<u8>, SldprtError> {
    let u32_at = |off: usize| -> Option<usize> {
        usize::try_from(u32::from_le_bytes(stream.get(off..off.checked_add(4)?)?.try_into().ok()?)).ok()
    };
    let mut at = 0usize;
    while let Some(len) = u32_at(at) {
        // Block: length of the rest, tag (16), inflated size, deflated size, zlib data,
        // padding.
        let (Some(plain), Some(deflated)) = (u32_at(at + 20), u32_at(at + 24)) else { break };
        let Some(end) = at.checked_add(4).and_then(|v| v.checked_add(len)) else { break };
        if len < 24usize.saturating_add(deflated) || end > stream.len() || plain > limits.max_stream_bytes {
            break;
        }
        // One budget for the whole file, shared with the streams read (security review S1).
        *spent = spent.saturating_add(plain);
        if *spent > limits.max_total_bytes || plain / limits.max_ratio.max(1) > deflated {
            return Err(SldprtError::TooLarge("a partition block inflates beyond the limit".into()));
        }
        let data = stream.get(at + 28..at + 28 + deflated).unwrap_or_default();
        let xt = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, plain.saturating_add(1))
            .map_err(|e| SldprtError::Malformed(format!("partition block does not inflate ({:?})", e.status)))?;
        let head = xt.get(..80.min(xt.len())).unwrap_or_default();
        if xt.get(..2) == Some(b"PS") && head.windows(PARTITION_TAG.len()).any(|w| w == PARTITION_TAG) {
            return Ok(xt);
        }
        at = end;
    }
    Err(SldprtError::NoGeometry("the partition stream holds no Parasolid partition".into()))
}

/// Finds the Parasolid partition of a `.sldprt` file (the lowest-numbered configuration
/// that has one).
///
/// # Errors
/// [`SldprtError`] for unknown containers, damaged or over-limit files, and files without
/// a partition.
pub fn extract_xt(bytes: &[u8], limits: &Limits) -> Result<Extracted, SldprtError> {
    let container = detect(bytes).ok_or(SldprtError::NotSldprt)?;
    let mut spent = 0usize;
    let mut warnings = Vec::new();
    // The configurations, lowest first, one entry each; streams are read only until the
    // first partition is found (security review S1: entries sharing their data must not
    // multiply what is read and kept).
    let found = match container {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, limits)?;
            if f.cut_short() {
                warnings.push(CUT_SHORT.to_string());
            }
            let mut list: Vec<(u32, usize)> =
                f.streams.iter().enumerate().filter_map(|(i, e)| partition_config(&e.name).map(|n| (n, i))).collect();
            list.sort_unstable();
            list.dedup_by_key(|(n, _)| *n);
            let mut found = None;
            for (n, i) in &list {
                if found.is_some() {
                    warnings.push(format!("configuration Config-{n} was not imported (only the first is)"));
                    continue;
                }
                let stream = f.read(*i, limits)?;
                spent = spent.saturating_add(stream.len());
                if spent > limits.max_total_bytes {
                    return Err(SldprtError::TooLarge("the file's streams exceed the size limit".into()));
                }
                match partition_block(&stream, limits, &mut spent) {
                    Ok(xt) => found = Some((*n, xt)),
                    Err(e @ SldprtError::TooLarge(_)) => return Err(e),
                    Err(e) if list.len() == 1 => return Err(e),
                    Err(e) => warnings.push(format!("Config-{n}: {e}")),
                }
            }
            found
        }
        Container::Chunks => {
            if !chunks::has_end_record(bytes) {
                warnings.push(CUT_SHORT.to_string());
            }
            let mut parts: Vec<(u32, Vec<u8>)> = Vec::new();
            for st in chunks::streams(bytes, limits, |n| partition_config(n).is_some())? {
                if let Some(n) = partition_config(&st.name) {
                    // A later valid record of the same stream replaces an earlier one.
                    parts.retain(|(m, _)| *m != n);
                    parts.push((n, st.data));
                }
            }
            parts.sort_by_key(|(n, _)| *n);
            let single = parts.len() == 1;
            let mut found = None;
            for (n, stream) in &parts {
                if found.is_some() {
                    warnings.push(format!("configuration Config-{n} was not imported (only the first is)"));
                    continue;
                }
                match partition_block(stream, limits, &mut spent) {
                    Ok(xt) => found = Some((*n, xt)),
                    Err(e @ SldprtError::TooLarge(_)) => return Err(e),
                    Err(e) if single => return Err(e),
                    Err(e) => warnings.push(format!("Config-{n}: {e}")),
                }
            }
            found
        }
    };
    let (n, xt) = found.ok_or_else(|| {
        SldprtError::NoGeometry(
            "no configuration has a Parasolid partition (an assembly or a part without a solid)".into(),
        )
    })?;
    Ok(Extracted { xt, container, config: format!("Config-{n}"), warnings })
}

#[cfg(test)]
mod tests;
