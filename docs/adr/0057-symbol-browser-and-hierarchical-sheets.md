# ADR 0057: Symbol library browser and hierarchical sheet editing

Status: accepted, implemented (2026-11-13). Follows ADR 0047 ("Next" → Schematic: "a symbol
library browser; sheet creation and editing") and ADR 0052 (schematic editing).

## Context
Placing a symbol meant typing a `.kicad_sym` path and a symbol name; hierarchical sheets could be
read, drawn and followed by the netlist, but not made or edited. Behaviour comes from KiCad's file
format documentation and sample files; no KiCad source was used, and no KiCad library content is
in the repository (tests use small hand-written libraries).

## Decisions

### Symbol library browser
* **The user's folder.** A preference (`ui-prefs.json`, `symbol_library`): a folder of
  `.kicad_sym` files, or one such file. Gusset never downloads, bundles or searches for KiCad
  libraries (CC-BY-SA data the user has). The folder is saved once it exists.
* **Index (`gusset-pcb` `symlib`)**: `index(file, text)` gives per library the symbols' name,
  description (`Description`), keywords (`ki_keywords`), units (a derived symbol counts its
  parent's) and the power flag; descriptions are cut at 300 characters and control characters
  dropped; at most 20 000 symbols per library (`truncated`). `search` needs every
  whitespace-separated word of the filter in the symbol's name, description or keywords or in
  the library's name (case-insensitive), ranks exact name, name prefix, name contains, rest (file
  order within a rank) and returns at most `max`. `load_symbol` resolves `extends` for the
  preview; `LibSymbol::preview(unit)` and `bounds` give the symbol at the origin and its extent.
* **Worker and cache (`gusset-app` `symbol_browser`)**: a scan thread reads the folder's files in
  name order (at most 1000; a file over 32 MB or not UTF-8 is skipped and listed with its reason)
  and publishes each library as it is indexed, so the list fills while it works; a newer scan
  (folder change, Reload) stops the older one by a generation number. The cache (file path →
  size, time, index) is kept between scans; unchanged files are not parsed again. The preview of
  the picked symbol is loaded on its own thread. Nothing parses on the UI thread; the list shows
  at most 2000 matches (virtualised rows) and the filter re-runs only when it or the libraries
  changed.
* **Panel (`symbol_panel`, in the Schematic tab's side panel)**: folder field and Reload, filter
  box, the list (`Library:Symbol`), the picked symbol's name and description, a preview drawn with
  the editor's own symbol renderer on the paper colour (unit buttons for multi-unit symbols) and
  Place (or a double click on a row). Place arms the existing Place tool (`arm_place`: library
  file, symbol, unit); the next click sends `sch.add_symbol` (with `unit` when above 1). The
  manual path fields remain under "Library file by path".

### Hierarchical sheets
* **`sch.add_sheet {name, file, at, size?, sheet?}`**: places a sheet (default 30 x 20 mm, sides
  5.08 to 2000 mm) on the root or the named sheet. `file` must be a relative `.kicad_sch` name
  without `..`, drive or leading slash (`.kicad_sch` is appended if missing). If the design has
  no such file an empty sub-sheet is started in the same undo step (so the step also adds a sheet
  file to the document); naming an existing file uses it again (KiCad allows the same file twice).
  A sheet cannot contain itself, directly or through other sheets, or be the root file. The sheet
  node follows KiCad 9: `Sheetname` and `Sheetfile` properties, stroke and fill, and `instances`
  with the project name, the path of the sheet it sits on (`/root-uuid`, or the first use's path
  on a sub-sheet) and the next page number. `sch.save` already writes every sheet file of the
  design, so the sub-sheet file appears on save.
* **`sch.sheet_pin {item, op, name, new_name?, kind?, side?, offset?, sheet?}`**: `item` is
  `sheet#i` or the sheet's name (an ambiguous name is refused); `add` puts a pin on an edge
  (`left` default; kind input, output, bidirectional (default), tri_state, passive; `offset` mm
  along the edge, else the first free 2.54 mm step; angle 180/0/90/270 by side; a name the sheet
  has is refused), `rename` and `remove`. Wires stay where they are.
* **Items**: `Item::Sheet(i)` (`sheet#i`) joins `sch.move` (by, with, or `at`; wires on the
  sheet's pins follow orthogonally like symbols' (ADR 0052), the name and file fields and pins
  move with it) and `sch.delete`; deleting a sheet drops sub-sheet files no sheet reaches any
  more (undo brings them back). Everything is one registry command and one undo step (ADR 0012).
* **ERC**: `hier_label_mismatch` (error): a sheet pin without a hierarchical label of that name
  inside the sheet, and a hierarchical label without a pin of that name on the sheet that uses
  it (reported per use). The netlist already joined pins and labels (ADR 0047).
* **Editor (`sch_view`)**: the Sheet tool drags a rectangle (snapped to the grid) and sends
  `sch.add_sheet` with the panel's name and file (empty file: the name in lower case with
  `_` for other characters, made unique). Sheets are picked (by their body), box-selected, dragged
  and deleted like other items; a picked sheet shows its pins in the panel (click a name to
  rename, `x` to remove, name/kind/side and Add pin) and **Enter sheet**; a double click on a
  sheet enters it. Breadcrumbs (root file, then sheet names, by the first use of the shown file)
  jump to a level; **Up one sheet** or Alt+Up leaves, back to where the sheet was entered from
  (else the parent by the hierarchy). Sheets draw with a light fill, name above, file below and
  pin names inside; the picked one in the selection colour.
* **Round trip**: the sheet is inserted before `sheet_instances` / `embedded_fonts` like other new
  items and everything else is written as read; adding and then deleting a sheet gives the
  original file back byte for byte (tested).
* MCP: `sch_add_sheet` and `sch_sheet_pin` are in the `sch` toolset: 18 tools, 11 203 bytes (18 000
  budget; the 600 bytes per tool average applies to sets of 20 or more and this set has fewer);
  the tools/list of all 179 registry tools averages 582 bytes (limit 600); the modeling list is
  unchanged (31 tools, 17 912 bytes).

## Tests
`gusset-pcb` +8 (index, search ranking and filter, preview load and bounds, symbol cap; sheet
creation round trip and byte-exact delete, pins on every edge with refusals, rename and remove,
sheet pins joining hierarchical labels in the netlist and the ERC mismatches, moving a sheet with
pins and wires); `gusset-commands` +3 (sheet and pins with undo at each step, files reused,
refused and saved and reopened, delete and move with undo); `gusset-app` +5 (folder scan off the
UI thread with errors, filter and cache, oversize and non-UTF-8 files, picked symbol preview with
units and Place through `sch.add_symbol` with undo, the panel in a frame; the Sheet tool, pins,
picking and moving, enter, leave and breadcrumbs); `every_toolset_fits_the_budget` passes.

## Not done / Next
* Moving a sheet's pins along its edge, resizing a sheet by dragging, and a sheet pin created
  from a hierarchical label (or the label from the pin) in one step.
* The browser searches names, descriptions and keywords only (no pin names, no footprint filters);
  KiCad's `.kicad_symdir` folders and library tables are not read; no favourites or recent list.
* (Fixed after review) `sch.add_symbol` writes one instance per use of the edited sheet, its path
  running from the root through the sheet symbols; every use gets the same reference (annotate to
  number a sheet used twice apart).
* Page numbers are numbered by use count; reordering pages is not offered.
