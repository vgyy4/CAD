# ADR 0033: FreeCAD and OpenSCAD import

Status: accepted (2026-10-19)

## Context
The v1 tier (docs/RESEARCH.md) lists FreeCAD `.FCStd` and OpenSCAD `.scad` import:
FreeCAD is the free parametric CAD people leave, and OpenSCAD files are the most common
parametric parts on Thingiverse/Printables. docs/research/interop.md §3.1-3.2 covers both
formats, the licences of existing readers (FreeCAD LGPL-2.1+, OpenSCAD GPL-2.0+,
`openscad-rs` GPL-3.0: reference only or forbidden) and the plan.

## Decision
1. **FreeCAD, tier 2 first (exact bodies).** An `.FCStd` is a ZIP with `Document.xml`
   (objects, types, properties, shape file names), `GuiDocument.xml` (visibility) and
   one OCCT BRep text file (`*.brp`) per object shape. The app reads the ZIP and the XML
   in-process with the existing hardened readers (`gusset-meshio`'s ZIP reader, made
   public as `gusset_meshio::archive`; `quick-xml`), and picks the objects a FreeCAD user
   sees: visible in `GuiDocument.xml`, with a shape file, and not a sketch, datum or
   other 2D/helper object (the type names are a fixed allow-list:
   `Part::*` solids and features, `PartDesign::Body`, and any object whose shape the
   worker reports as solid). If no visibility is stored, the tips of PartDesign bodies
   and the objects no other object uses are taken. Each chosen BRep text goes through the
   sandboxed import worker (new request format `BrepText`), which parses it with OCCT
   (`BRepTools::Read` from a stream), walks the topology and writes the kernel-native
   blob, like STEP. Each becomes an `import` feature named after the object's Label
   (untrusted text). Tier 1 (mapping PartDesign/Sketcher/Spreadsheet to our features)
   comes later and falls back to this path per object.
2. **OpenSCAD, evaluated to exact bodies.** Our own lexer, parser and evaluator, written
   from the OpenSCAD User Manual (never from `parser.y`): numbers, strings, booleans,
   `undef`, vectors, ranges, arithmetic and comparison, `?:`, variables with OpenSCAD's
   last-assignment-wins scoping, `$fn/$fa/$fs` (dynamic), functions, modules with
   `children()`, `for`/`if`/`let`/`each`, list comprehensions, and the builtins
   `cube`, `sphere`, `cylinder`, `polyhedron` (closed, planar faces), `square`,
   `circle`, `polygon`, `linear_extrude` (height, center, no twist), `rotate_extrude`
   (angle), `translate`, `rotate`, `mirror`, `scale` (uniform), `multmatrix` (rigid),
   `union`, `difference`, `intersection`, `color` (ignored), and the math and list
   functions. Solids are built with the kernel as exact B-rep (spheres and cylinders are
   true surfaces, not facets; `$fn` only matters for `circle`/`polygon` used as
   polygons). `hull`, `minkowski`, `offset`, `projection`, `surface`, `import`, `text`
   and non-uniform scaling are reported as unsupported with the line. Evaluation is
   bounded: recursion depth 256, 10 M evaluation steps, 100 k primitives, 20 M value
   elements built or walked, CSG depth 256 (amended after security review v0.4 X1-X3;
   the first version claimed "50 MB of values" without enforcing it), and cancellable. The result is an `import` feature per top-level solid; the source is kept in
   the import feature's file name and the warnings list what was skipped. Translating
   `.scad` into our code language (tier 1, editable parameters) needs loops and modules
   in the language and comes later.
3. **Commands.** `import.file` accepts `.fcstd` and `.scad` with the same approvals,
   cancellation and audit as STEP. Results carry the tier, the chosen objects and
   warnings.
4. **Safety.** The XML, ZIP and `.scad` readers are safe Rust with caps and have fuzz
   targets; OCCT only ever parses BRep text inside the sandboxed worker.

## Consequences
- FreeCAD parts open as exact bodies that our features (fillets, holes, drawings) can
  use; their FreeCAD history is not kept yet.
- OpenSCAD parts become exact B-rep (better than OpenSCAD's own mesh output) but lose
  their parameters until tier 1.
- Deferred: FreeCAD tier 1, assemblies (`App::Link`), colours; OpenSCAD tier 1, `hull`,
  `minkowski`, `offset`, text.
