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
/// Without that stream, the bodies an imported body feature keeps in its own record of the
/// lowest configuration's `ResolvedFeatures` (round sw8). Empty when there are none.
///
/// # Errors
/// [`SldprtError`] for unknown containers and damaged or over-limit files.
pub fn local_bodies(
    bytes: &[u8],
    limits: &Limits,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<Vec<u8>>, SldprtError> {
    Ok(named_local_bodies(bytes, limits, cancel)?.into_iter().map(|b| b.xt).collect())
}

/// A body of a `LocalBodies` stream: the name of its record (the imported feature's node
/// name, `Imported1`, or a body name such as `0780-001-1-solid1`) and its Parasolid transmit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBody {
    pub name: String,
    pub xt: Vec<u8>,
}

/// [`local_bodies`] with the name of each body's record (empty when none is found).
///
/// # Errors
/// [`SldprtError`] for unknown containers and damaged or over-limit files.
pub fn named_local_bodies(
    bytes: &[u8],
    limits: &Limits,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<LocalBody>, SldprtError> {
    let is_local = |n: &str| n.ends_with("LocalBodies") && (n == "LocalBodies" || n.contains("-FeatureBodies/"));
    let local = match detect(bytes).ok_or(SldprtError::NotSldprt)? {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, limits)?;
            match f.streams.iter().position(|e| is_local(&e.name)) {
                Some(i) => Some(f.read(i, limits)?),
                None => None,
            }
        }
        Container::Chunks => one_stream(bytes, limits, is_local, str::to_string)?,
    };
    if let Some(stream) = local {
        return local_blocks(&stream, limits, cancel, false);
    }
    // Round sw8: an imported body feature (`moMBimport_c`) of a part without a
    // `LocalBodies` stream keeps its body inside the feature's own record in the lowest
    // configuration's `ResolvedFeatures`, as a block of the partition's kind (F688ZZ
    // Bearing, prop_12x6); the final partition then holds no body.
    let is_resolved = |n: &str| stream_config(n, "-ResolvedFeatures").is_some();
    let key = |n: &str| format!("{:010}", stream_config(n, "-ResolvedFeatures").unwrap_or(u32::MAX));
    match one_stream(bytes, limits, is_resolved, key)? {
        Some(stream) => local_blocks(&stream, limits, cancel, true),
        None => Ok(Vec::new()),
    }
}

/// The stream `wanted` accepts that sorts first by `key`, read and inflated; `None` when
/// the file has none.
fn one_stream(
    bytes: &[u8],
    limits: &Limits,
    wanted: impl Fn(&str) -> bool,
    key: impl Fn(&str) -> String,
) -> Result<Option<Vec<u8>>, SldprtError> {
    Ok(match detect(bytes).ok_or(SldprtError::NotSldprt)? {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, limits)?;
            let chosen =
                f.streams.iter().enumerate().filter(|(_, e)| wanted(&e.name)).min_by_key(|(_, e)| key(&e.name));
            match chosen {
                Some((i, _)) => Some(f.read(i, limits)?),
                None => None,
            }
        }
        Container::Chunks => {
            // The name first (no inflating), then only that record (security review T2).
            let Some(chosen) = chunks::names(bytes, limits, wanted).into_iter().min_by_key(|n| key(n)) else {
                return Ok(None);
            };
            chunks::streams(bytes, limits, |n| n == chosen)?.pop().map(|st| st.data)
        }
    })
}

/// The name of the record that ends just before the block at `at`: `FF FE FF <n>`, n UTF-16
/// units, then at most 32 bytes of flags and sizes (10 in the older parts, 18 in
/// newer ones: `3v motor hole -v2`).
fn record_name(stream: &[u8], at: usize) -> String {
    let lo = at.saturating_sub(4 + 2 * 255 + 32);
    for p in (lo..at).rev() {
        if stream.get(p..p + 3) != Some(&[0xff, 0xfe, 0xff][..]) {
            continue;
        }
        let n = usize::from(stream.get(p + 3).copied().unwrap_or(0));
        let end = p + 4 + 2 * n;
        if end <= at && at - end <= 32 {
            let units: Vec<u16> = stream
                .get(p + 4..end)
                .unwrap_or_default()
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
                .collect();
            return String::from_utf16_lossy(&units);
        }
    }
    String::new()
}

/// Appends a block's continuation chunks to `xt`. A block whose data inflates to more than
/// 1 MiB goes on after its first zlib stream in further chunks (`u32` inflated size, `u32`
/// deflated size, zlib data), ended by two zero sizes, all inside the block's length:
/// GT2_Spur's imported gear (1 MiB, then 582 kB) and, round sw8, the final partitions of
/// large parts (`Dremel Body 4000`: 1 MiB, then 237 kB; the helical gears: 1 MiB, then
/// 4 kB chunks). `c` is where the first zlib stream ends, `end` the block's end.
fn continuation(
    stream: &[u8],
    mut c: usize,
    end: usize,
    xt: &mut Vec<u8>,
    limits: &Limits,
    spent: &mut usize,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<(), SldprtError> {
    let u32_at = |off: usize| -> Option<usize> {
        usize::try_from(u32::from_le_bytes(stream.get(off..off.checked_add(4)?)?.try_into().ok()?)).ok()
    };
    while let (Some(p2), Some(d2)) = (u32_at(c), u32_at(c.saturating_add(4))) {
        if p2 == 0 || c.saturating_add(8).saturating_add(d2) > end {
            break;
        }
        *spent = spent.saturating_add(p2);
        if xt.len().saturating_add(p2) > limits.max_stream_bytes
            || *spent > limits.max_total_bytes
            || p2 / limits.max_ratio.max(1) > d2
        {
            return Err(SldprtError::TooLarge("a block inflates beyond the limit".into()));
        }
        if cancel.is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(SldprtError::TooLarge("cancelled".into()));
        }
        let data = stream.get(c + 8..c + 8 + d2).unwrap_or_default();
        let more = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, p2.saturating_add(1))
            .map_err(|e| SldprtError::Malformed(format!("a block's continuation does not inflate ({:?})", e.status)))?;
        xt.extend(more);
        c += 8 + d2;
    }
    Ok(())
}

/// The Parasolid transmits of a `LocalBodies` stream.
fn local_blocks(
    stream: &[u8],
    limits: &Limits,
    cancel: &std::sync::atomic::AtomicBool,
    in_features: bool,
) -> Result<Vec<LocalBody>, SldprtError> {
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
    // A `LocalBodies` stream starts with its first block; in a feature stream the first
    // block lies anywhere and is found by the tag every partition block carries.
    let first = if in_features {
        stream
            .windows(BLOCK_TAG.len())
            .enumerate()
            .filter(|(_, w)| *w == BLOCK_TAG)
            .filter_map(|(i, _)| i.checked_sub(4))
            .find(|i| block(*i).is_some())
    } else {
        (0..stream.len().min(4096)).find(|i| block(*i).is_some())
    };
    let Some(first) = first else { return Ok(Vec::new()) };
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
        let mut xt = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
            stream.get(at + 28..at + 28 + deflated).unwrap_or_default(),
            plain.saturating_add(1),
        )
        .map_err(|e| SldprtError::Malformed(format!("a body block does not inflate ({:?})", e.status)))?;
        continuation(stream, at + 28 + deflated, end, &mut xt, limits, &mut spent, Some(cancel))?;
        // In a feature stream, a partition or its deltas is not a body.
        let head = xt.get(..80.min(xt.len())).unwrap_or_default();
        let state = [PARTITION_TAG, b": TRANSMIT FILE (deltas)"].iter().any(|t| head.windows(t.len()).any(|w| w == *t));
        if xt.get(..2) == Some(b"PS") && !(in_features && state) {
            out.push(LocalBody { name: record_name(stream, at), xt });
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
fn stream_config(name: &str, suffix: &str) -> Option<u32> {
    let rest = name.strip_prefix("Contents/").unwrap_or(name).strip_prefix("Config-")?;
    rest.strip_suffix(suffix)?.parse().ok()
}

/// The warning for a file that ends early (dogfooding F12-9).
const CUT_SHORT: &str = "the file ends early (an incomplete download or copy?): the geometry found may be incomplete";

const PARTITION_TAG: &[u8] = b": TRANSMIT FILE (partition)";

/// The 16-byte tag every block of a partition, `LocalBodies` or feature stream carries (the
/// same in all samples).
const BLOCK_TAG: [u8; 16] =
    [0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99];

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
        let mut xt = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, plain.saturating_add(1))
            .map_err(|e| SldprtError::Malformed(format!("partition block does not inflate ({:?})", e.status)))?;
        let head = xt.get(..80.min(xt.len())).unwrap_or_default();
        if xt.get(..2) == Some(b"PS") && head.windows(PARTITION_TAG.len()).any(|w| w == PARTITION_TAG) {
            continuation(stream, at + 28 + deflated, end, &mut xt, limits, spent, None)?;
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
    extract_stream(bytes, limits, "-Partition")
}

/// The Parasolid transmit of a part's `StockBodies` stream (the body a mirror-part feature
/// starts from; the final partition holds the mirrored result): `None` when the part has no
/// such stream.
///
/// # Errors
/// [`SldprtError`] for damaged or over-limit files.
pub fn extract_stock(bytes: &[u8], limits: &Limits) -> Result<Option<Extracted>, SldprtError> {
    match extract_stream(bytes, limits, "-StockBodies") {
        Ok(x) => Ok(Some(x)),
        Err(SldprtError::NoGeometry(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn extract_stream(bytes: &[u8], limits: &Limits, suffix: &str) -> Result<Extracted, SldprtError> {
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
            let mut list: Vec<(u32, usize)> = f
                .streams
                .iter()
                .enumerate()
                .filter_map(|(i, e)| stream_config(&e.name, suffix).map(|n| (n, i)))
                .collect();
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
            for st in chunks::streams(bytes, limits, |n| stream_config(n, suffix).is_some())? {
                if let Some(n) = stream_config(&st.name, suffix) {
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
