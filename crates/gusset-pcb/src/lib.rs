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
//! * [`route`]: interactive routing in 45° segments around other nets' copper, with
//!   arc corners.
//! * [`specctra`]: Specctra DSN out and SES in, for an external autorouter.
//! * [`zones`]: zone fills (clearances, thermal reliefs, minimum width, islands) over
//!   [`poly`] (polygon booleans and offsets, holes bridged in, triangles for drawing).
//! * [`sch`]: KiCad schematics (`.kicad_sch`) and symbol libraries (`.kicad_sym`), read
//!   and written losslessly; [`sch_edit`] edits them.
//! * [`netlist`] (nets across the sheet hierarchy), [`erc`] (a basic electrical rule
//!   check) and [`sync`] (the board's pad nets and missing footprints from the netlist).
//!
//! Clean room: written from KiCad's published file-format documentation and sample files;
//! no KiCad (GPL) source was read or copied.

pub mod annotate;
pub mod board;
pub mod board3d;
pub mod check;
pub mod copper;
pub mod drc;
mod drc_more;
pub mod drill;
pub mod dru;
pub mod edit;
pub mod erc;
pub mod fab;
pub mod font;
pub mod geom;
pub mod gerber;
#[cfg(test)]
mod gerber_read;
pub mod length;
pub mod netlist;
pub mod nets;
pub mod pair;
pub mod poly;
pub mod ratsnest;
pub mod refill;
pub mod route;
pub mod rules;
pub mod sch;
pub mod sch_edit;
pub mod sch_sheet;
pub mod sexpr;
pub mod shove;
pub mod specctra;
pub mod symlib;
pub mod sync;
pub mod text_edit;
pub mod tune;
pub mod zones;

pub use board::{Board, Limits};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_annotate;
#[cfg(test)]
mod tests_drc_more;
#[cfg(test)]
mod tests_dru;
#[cfg(test)]
mod tests_dsn;
#[cfg(test)]
mod tests_edit;
#[cfg(test)]
mod tests_gerber;
#[cfg(test)]
mod tests_local;
#[cfg(test)]
mod tests_pair;
#[cfg(test)]
mod tests_phase3;
#[cfg(test)]
mod tests_refill;
#[cfg(test)]
mod tests_route;
#[cfg(test)]
mod tests_sch;
#[cfg(test)]
mod tests_sch_edit;
#[cfg(test)]
mod tests_sch_sheet;
#[cfg(test)]
mod tests_symlib;
#[cfg(test)]
mod tests_text;
#[cfg(test)]
mod tests_zone;
#[cfg(test)]
mod tests_zone_more;
