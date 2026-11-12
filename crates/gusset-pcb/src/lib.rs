//! Printed circuit boards for Gusset (ADR 0047).
//!
//! * [`sexpr`]: a lossless reader/writer for KiCad's S-expression files.
//! * [`board`]: the typed board model (layers, stackup, nets, footprints, pads, tracks,
//!   vias, zones, drawings, 3D model references) over that tree; `.kicad_pcb` and
//!   `.kicad_mod` in, `.kicad_pcb` out.
//! * [`gerber`] (Gerber X2 per layer) and [`drill`] (Excellon) for fabrication.
//! * [`check`]: a basic sanity check (outline closes, pads on copper, nets known).
//! * [`board3d`]: the board as 3D bodies through the kernel, for enclosure work.
//! * [`edit`]: board edits on the tree (new board, move/rotate/flip footprints, tracks,
//!   vias, outline), lossless for what they do not touch.
//! * [`copper`], [`ratsnest`] (connectivity and unrouted connections), [`drc`] (a basic
//!   design rule check) and [`rules`] (net classes and minimums).
//! * [`fab`]: pick-and-place and BOM CSV files.
//! * [`sch`]: KiCad schematics (`.kicad_sch`) and symbol libraries (`.kicad_sym`), read
//!   and written losslessly; [`sch_edit`] edits them.
//! * [`netlist`] (nets across the sheet hierarchy), [`erc`] (a basic electrical rule
//!   check) and [`sync`] (the board's pad nets and missing footprints from the netlist).
//!
//! Clean room: written from KiCad's published file-format documentation and sample files;
//! no KiCad (GPL) source was read or copied.

pub mod board;
pub mod board3d;
pub mod check;
pub mod copper;
pub mod drc;
pub mod drill;
pub mod edit;
pub mod erc;
pub mod fab;
pub mod geom;
pub mod gerber;
pub mod netlist;
pub mod ratsnest;
pub mod rules;
pub mod sch;
pub mod sch_edit;
pub mod sexpr;
pub mod sync;

pub use board::{Board, Limits};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edit;
#[cfg(test)]
mod tests_sch;
