//! The schematic open in a session (ADR 0047 phase 2a), like the board
//! ([`crate::pcb_session`]): the document names the root sheet and the sheets it uses by
//! the hashes of their texts; every `sch.*` edit stores a new sheet and commits a
//! document that names it, so undo, redo and branches work on schematics too. The parsed
//! design, its netlist and ERC are cached per set of hashes.

use crate::{CommandError, Session};
use gusset_eval::BlobSource;
use gusset_history::Actor;
use gusset_pcb::erc::{self, Issue};
use gusset_pcb::netlist::{Design, Netlist};
use gusset_pcb::sch::Schematic;
use std::sync::Arc;

/// Most ERC issues kept with an open schematic.
pub const MAX_ISSUES: usize = 500;

/// A schematic as the editor and the commands use it.
#[derive(Debug)]
pub struct OpenSchematic {
    /// The document's statement it was read from.
    pub doc: gusset_doc::PcbSchematic,
    /// Root first, then the sub-sheets the document names.
    pub design: Design,
    pub netlist: Netlist,
    pub erc: Vec<Issue>,
}

impl OpenSchematic {
    /// The root sheet's file name (shown; untrusted).
    pub fn file(&self) -> &str {
        &self.doc.file
    }

    /// A sheet by file name (`None`: the root).
    pub fn sheet(&self, file: Option<&str>) -> Option<(usize, &Schematic)> {
        match file {
            None => self.design.sheets.first().map(|(_, s)| (0, s)),
            Some(f) => {
                let i = self.design.sheet_named(f)?;
                self.design.sheets.get(i).map(|(_, s)| (i, s))
            }
        }
    }
}

/// The parsed schematic of the last statement read.
#[derive(Debug, Default)]
pub struct Cache(std::sync::Mutex<Option<Arc<OpenSchematic>>>);

/// The schematic open in the actor's document, if any.
///
/// # Errors
/// When a stored sheet is missing or does not read.
pub fn open_schematic(s: &Session, actor: &Actor) -> Result<Option<Arc<OpenSchematic>>, CommandError> {
    let Some(p) = s.document(actor)?.schematic else { return Ok(None) };
    schematic_of(s, &p).map(Some)
}

/// Reads a stored sheet.
fn sheet_text(s: &Session, hash: &str) -> Result<Schematic, CommandError> {
    let bytes = s.blobs().blob(hash).ok_or_else(|| {
        CommandError::new("io", "the schematic's stored data is missing from the project")
            .hint("open the schematic file again with sch.open")
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CommandError::new("io", "the stored schematic is not text"))?;
    Schematic::read(text, &gusset_pcb::Limits::default())
        .map_err(|e| CommandError::new("io", format!("KiCad schematic: {e}")))
}

/// The schematic a document names.
///
/// # Errors
/// When a stored sheet is missing or does not read.
pub fn schematic_of(s: &Session, p: &gusset_doc::PcbSchematic) -> Result<Arc<OpenSchematic>, CommandError> {
    if let Ok(c) = s.sch_cache.0.lock()
        && let Some(o) = c.as_ref().filter(|o| o.doc == *p)
    {
        return Ok(Arc::clone(o));
    }
    let mut design = Design::single(p.file.clone(), sheet_text(s, &p.sheet)?);
    for (file, hash) in &p.sheets {
        design.sheets.push((file.clone(), sheet_text(s, hash)?));
    }
    let netlist = gusset_pcb::netlist::netlist(&design);
    let erc = erc::check(&design, &netlist, MAX_ISSUES);
    let open = Arc::new(OpenSchematic { doc: p.clone(), design, netlist, erc });
    if let Ok(mut c) = s.sch_cache.0.lock() {
        *c = Some(Arc::clone(&open));
    }
    Ok(open)
}
