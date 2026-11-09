//! Solid Edge and Autodesk Inventor part readers (ADR 0046). Both are OLE2 compound files
//! ([MS-CFB], read by `gusset_sldprt::cfb`) that carry their exact geometry as a B-rep
//! another crate already reads:
//!
//! * Solid Edge (`.par`, `.psm`): the storage `PARASOLID` holds the Parasolid transmit as a
//!   plain `.x_b` stream (`solidedge`), read by `gusset-xt`.
//! * Inventor (`.ipt`): one deflated stream of the `RSeStorage` holds the part's body as
//!   binary ShapeManager (ACIS) records (`inventor`), read by `gusset-sat`.
//!
//! The layouts were derived from sample files only (Zenodo records 8028435, 13941029,
//! 11061412, 4309294 and 8028536, with the STEP files that sit next to some of them), with no
//! other reader, write-up or SDK consulted. Input is untrusted: every count, chain and
//! inflated size is bounded by [`Limits`].

#[doc(hidden)]
pub mod fixtures;
pub mod inventor;
pub mod solidedge;

pub use gusset_sldprt::Limits;
use gusset_sldprt::SldprtError;

/// Why a file cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OleError {
    /// Not an OLE2 compound file.
    NotOle,
    Malformed(String),
    TooLarge(String),
    /// A valid container without a solid body (an assembly, a part saved without geometry).
    NoGeometry(String),
}

impl std::fmt::Display for OleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OleError::NotOle => write!(f, "not an OLE2 compound file"),
            OleError::Malformed(m) => write!(f, "the file is damaged: {m}"),
            OleError::TooLarge(m) => write!(f, "the file is too large: {m}"),
            OleError::NoGeometry(m) => write!(f, "the file has no solid geometry: {m}"),
        }
    }
}

impl std::error::Error for OleError {}

impl From<SldprtError> for OleError {
    fn from(e: SldprtError) -> OleError {
        match e {
            SldprtError::NotSldprt => OleError::NotOle,
            SldprtError::Malformed(m) => OleError::Malformed(m),
            SldprtError::TooLarge(m) => OleError::TooLarge(m),
            SldprtError::NoGeometry(m) => OleError::NoGeometry(m),
        }
    }
}

/// Opens the container, with its "cut short" note as a warning.
fn open<'a>(
    bytes: &'a [u8],
    limits: &Limits,
    warnings: &mut Vec<String>,
) -> Result<gusset_sldprt::cfb::Cfb<'a>, OleError> {
    if bytes.get(..8) != Some(&gusset_sldprt::cfb::SIGNATURE[..]) {
        return Err(OleError::NotOle);
    }
    let f = gusset_sldprt::cfb::Cfb::open(bytes, limits)?;
    if f.cut_short() {
        warnings.push("the file is cut short (a partial copy); what could be read is used".into());
    }
    Ok(f)
}

#[cfg(test)]
mod tests;
