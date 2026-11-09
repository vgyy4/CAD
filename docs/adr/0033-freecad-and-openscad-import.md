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
   true surfaces, not facets; an explicit `$fn` of 3 to 30 on a circle or cylinder builds
   OpenSCAD's regular polygon or prism (hex nut traps, dogfooding F5-1), and a small `$fn`
   on a sphere or cone is warned about). `hull`, `minkowski`, `offset`, `projection`, `surface`, `import`, `text`
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

## Amendment (2026-11-08): OpenSCAD objects as editable features
The spec asks for `.scad` to be translated to the code view (tier 1). Each top-level object is
now written as features when they can say it: every primitive is drawn at the origin (a
sketch and an extrude for cubes, cylinders and `linear_extrude` shapes; an XZ-plane profile
revolved about Z for cones, spheres and `rotate_extrude`), turned and moved to its place by
one `move` (the placement split into an even scale, which scales the primitive's sizes, a
turn about one axis and a shift), and joined by `combine` as the file's `union`,
`difference` and `intersection` join it (`gusset-commands/src/scad_text.rs`).

Before anything is committed, the import rebuilds each object's features in a scratch
document with the kernel and keeps them only if they rebuild without errors into one body
whose volume (within a millionth) and box (within a millionth of its size) match the exact
build. Anything else - a mirror, an uneven scale or skew, a check that fails, more than 5000
curves - comes in as the exact body, with a warning naming the object and why. The features
go in with the exact bodies in one edit (one undo); with the timeline rolled back every object
comes in as an exact body. Not translated yet: `hull`, `minkowski` and text, which the reader
already refuses or evaluates elsewhere; loops and modules are unrolled, so a `for` gives
repeated features, not a pattern.


## Amendment (2026-11-09): FreeCAD PartDesign history as editable features

A `.FCStd` file holds the whole PartDesign history in `Document.xml`: each `PartDesign::Body`
lists its features in order (`Group`), sketches keep their geometry in their own frame plus a
placement, and every feature names its inputs and sizes. FreeCAD is open source, so the format
is read directly (`crates/gusset-commands/src/fcstd_tree.rs`).

**Decision.** The history is written as a Gusset feature tree (`.gtree.json` format version 2,
ADR 0042) and the tree importer builds it. Translated:

* sketches (lines, circles, counter-clockwise arcs, construction geometry);
* pad and pocket (a length, midplane, reversed; pocket through all);
* revolution and groove about a sketch axis or line, or an origin axis;
* mirrored, linear and polar patterns and MultiTransform (each step in turn, about sketch
  axes, origin axes and planes or datum lines and planes; the whole body or listed features);
* pipes along sketch curves (as sweeps; not through sections) and open lofts;
* fillets and chamfers (equal distance): FreeCAD names edges (`EdgeN`, `FaceN`) of the base
  feature's stored shape, so that shape is read through the import worker and each edge is
  passed on by its midpoint;
* additive and subtractive boxes, cylinders, cones and spheres (as a sketch and an extrusion
  or revolution);
* datums and the origin carry no geometry and are skipped;
* files from before FreeCAD had bodies: their sketches and PartDesign features form one
  implicit body in document order;
* fillet and chamfer edges on refined shapes (merged faces) are passed with points along them,
  and the model's edges lying on such an edge stand in for it.

**Check.** Before anything is committed the tree is built in a scratch session and compared
with the shapes FreeCAD stored: every feature must build, the volume must agree within 2e-5
(blends and sweeps are approximated surfaces and FreeCAD's OCCT build differs from ours by
up to about 1e-5; any difference above 1e-7 is reported as a warning) and the bounding box within 1e-4 of its diagonal (the box catches a mirror about the wrong
plane, which keeps the volume). Documents with solids made outside PartDesign bodies (Part workbench booleans, sweeps,
arrays) keep their shapes. Any untranslated feature, failure or mismatch keeps the exact
shapes (tier 2, as before) with a warning naming the first reason; nothing is half-imported.

**Coverage** on FreeCAD's own example files (160 files, 140 with a PartDesign body; run with
`GUSSET_FCSTD_SAMPLES`, `crates/gusset-cli/tests/freecad_history.rs`): 96 come in as history,
the rest as exact shapes. The main remaining reasons: features that rebuild with a different
volume (fillets whose edges change with the feature order, and stale shapes), lofts,
pad and pocket types up to a face (two lengths are written as two extrusions), ellipse arcs, and
axes on edges of solids.
