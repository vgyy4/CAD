//! SolidWorks `.sldprt` reader (ADR 0043): finds the embedded Parasolid partition and
//! returns it as an XT transmit file for `gusset-xt`. Nothing else is read (no feature
//! history, no display lists, no assemblies).
//!
//! Two containers exist. Files up to about SolidWorks 2014 are OLE2 compound files whose
//! `Config-<n>-Partition` stream holds the partition; newer files use a custom chunk
//! container with the same stream under `Contents/Config-<n>-Partition`. In both, the
//! stream is a list of blocks (a length, a 16-byte tag, the inflated and deflated sizes
//! and a zlib stream); the first block is the Parasolid partition, the second its undo
//! deltas. The layout was derived from sample files and the STEP files SolidWorks wrote
//! for them, with no other SolidWorks reader consulted. Input is untrusted: every count,
//! chain and inflated size is bounded by [`Limits`].

mod cfb;
mod chunks;
mod crc;

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

const PARTITION_TAG: &[u8] = b": TRANSMIT FILE (partition)";

/// Splits a partition stream into its blocks and returns the first Parasolid partition.
fn partition_block(stream: &[u8], limits: &Limits) -> Result<Vec<u8>, SldprtError> {
    let u32_at = |off: usize| -> Option<usize> {
        usize::try_from(u32::from_le_bytes(stream.get(off..off.checked_add(4)?)?.try_into().ok()?)).ok()
    };
    let mut at = 0usize;
    let mut total = 0usize;
    while let Some(len) = u32_at(at) {
        // Block: length of the rest, tag (16), inflated size, deflated size, zlib data,
        // padding.
        let (Some(plain), Some(deflated)) = (u32_at(at + 20), u32_at(at + 24)) else { break };
        let Some(end) = at.checked_add(4).and_then(|v| v.checked_add(len)) else { break };
        if len < 24usize.saturating_add(deflated) || end > stream.len() || plain > limits.max_stream_bytes {
            break;
        }
        total = total.saturating_add(plain);
        if total > limits.max_total_bytes || plain / limits.max_ratio.max(1) > deflated {
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
    let mut parts: Vec<(u32, Vec<u8>)> = Vec::new();
    match container {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, limits)?;
            for (i, e) in f.streams.iter().enumerate() {
                if let Some(n) = partition_config(&e.name) {
                    parts.push((n, f.read(i, limits)?));
                }
            }
        }
        Container::Chunks => {
            for s in chunks::streams(bytes, limits, |n| partition_config(n).is_some())? {
                if let Some(n) = partition_config(&s.name) {
                    // A later valid record of the same stream replaces an earlier one.
                    parts.retain(|(m, _)| *m != n);
                    parts.push((n, s.data));
                }
            }
        }
    }
    parts.sort_by_key(|(n, _)| *n);
    let mut warnings = Vec::new();
    let mut found = None;
    for (n, stream) in &parts {
        match partition_block(stream, limits) {
            Ok(xt) if found.is_none() => found = Some((*n, xt)),
            Ok(_) => warnings.push(format!("configuration Config-{n} was not imported (only the first is)")),
            Err(e) if found.is_none() && parts.len() == 1 => return Err(e),
            Err(e) => warnings.push(format!("Config-{n}: {e}")),
        }
    }
    let (n, xt) = found.ok_or_else(|| {
        SldprtError::NoGeometry(
            "no configuration has a Parasolid partition (an assembly or a part without a solid)".into(),
        )
    })?;
    Ok(Extracted { xt, container, config: format!("Config-{n}"), warnings })
}

#[cfg(test)]
mod tests;
