//! Printed circuit boards for Gusset (ADR 0047).
//!
//! * [`sexpr`]: a lossless reader/writer for KiCad's S-expression files.
//! * [`board`]: the typed board model (layers, stackup, nets, footprints, pads, tracks,
//!   vias, zones, drawings, 3D model references) over that tree; `.kicad_pcb` and
//!   `.kicad_mod` in, `.kicad_pcb` out.
//! * [`gerber`] (Gerber X2 per layer) and [`drill`] (Excellon) for fabrication.
//! * [`check`]: a basic sanity check (outline closes, pads on copper, nets known).
//! * [`board3d`]: the board as 3D bodies through the kernel, for enclosure work.
//!
//! Clean room: written from KiCad's published file-format documentation and sample files;
//! no KiCad (GPL) source was read or copied.

pub mod board;
pub mod board3d;
pub mod check;
pub mod drill;
pub mod geom;
pub mod gerber;
pub mod sexpr;

pub use board::{Board, Limits};

#[cfg(test)]
mod tests;
