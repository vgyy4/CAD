//! Durable storage for Gusset (ADR 0005, ADR 0007).
//!
//! - [`container`]: the `.gus` checksummed chunk container (writer, verifying reader,
//!   salvage). The byte layout is specified in `docs/FORMAT.md`.
//!
//! ```
//! use gusset_store::container::{self, ContainerWriter, Kind, SaveInfo};
//! let mut w = ContainerWriter::new();
//! let h = w.add(Kind::FEATURE, b"gusset feature 1\nextrude 10 mm\n").unwrap();
//! w.set_root("head", h.as_bytes()).unwrap();
//! let bytes = w.finish(&SaveInfo { save_seq: 1, created_unix: 0, app_version: "0.0.1" }).unwrap();
//! let c = container::open(&bytes).unwrap();
//! assert_eq!(c.get(&h).unwrap().unwrap(), b"gusset feature 1\nextrude 10 mm\n");
//! ```

#![deny(unsafe_code)]

pub mod checksum;
pub mod container;
mod hash;
mod wire;

pub use hash::Hash;
