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
* pad and pocket (a length, two lengths, midplane, reversed; pocket through all; up to a face,
  the first or the last face as a plain extrusion to the far end of the solid FreeCAD stored
  for it, `AddSubShape`);
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
* Part workbench primitives (box, cylinder, sphere, cone), booleans (cut, fuse, common and their
  multi forms, as Gusset `combine`), extrusions of sketches along their normal and Draft arrays
  (polar: a circular body pattern; ortho: up to three linear ones; their sample files do not
  rebuild yet);
* files from before FreeCAD had bodies: their sketches and PartDesign features form one
  implicit body in document order;
* fillet and chamfer edges on refined shapes (merged faces) are passed with points along them,
  and the model's edges lying on such an edge stand in for it; the other way round, a picked
  piece of an edge FreeCAD split (where the faces of a joined pattern meet) names the whole
  line or arc of the model that holds it, and a whole-face pick leaves out the seam lines
  between two halves of one cylinder or plane (Gusset has no edge there);
* two-length pads and pockets ignore `Reversed` (the stored solids of the examples show the
  second length is offset instead), `SideType` 2 of FreeCAD 1.1+ is the midplane, a symmetric
  revolution or groove is two half turns (one about the reversed axis), a linear-pattern
  direction or axis that names a straight edge of a feature's stored shape is that edge's
  line, a sketch arc whose ends meet is a circle, and a body group listing a fillet before its
  base or a pad before its sketch is put in dependency order;
* Part booleans and Draft arrays take a renamed copy of the PartDesign body features up to
  their input when the plain tree does not rebuild (the body goes on and shows beside the
  result, as in FreeCAD).

**Check.** Before anything is committed the tree is built in a scratch session and compared
with the shapes FreeCAD stored: every feature must build, the volume must agree within 2e-5
(blends and sweeps are approximated surfaces and FreeCAD's OCCT build differs from ours by
up to about 1e-5; any difference above 1e-7 is reported as a warning) and the bounding box within 1e-4 of its diagonal (the box catches a mirror about the wrong
plane, which keeps the volume). Documents with other solids outside PartDesign bodies (Part sweeps, helices, thickness,
Draft arrays) keep their shapes. Any untranslated feature, failure or mismatch keeps the exact
shapes (tier 2, as before) with a warning naming the first reason; nothing is half-imported.

**Coverage** on FreeCAD's own example files (160 files, 140 with a PartDesign body; run with
`GUSSET_FCSTD_SAMPLES`, `crates/gusset-cli/tests/freecad_history.rs`): 126 come in as history
(11 more files show no solid), the rest as exact shapes. Unequal chamfers (two distances,
distance and angle) are written but no example file uses them. Three files were gained by
kernel and translator fixes: a boolean whose face unification leaves an invalid solid is
redone without it (Page_009, Page_015), the volume of a trimmed torus is integrated
adaptively (the fixed quadrature was off by 1e-4, Page_033), and a pattern of a groove or
revolution copies the whole tool as pads and pockets do (Page_078). A fillet or chamfer that
BRepCheck rejects is repaired with ShapeFix as FreeCAD's is (Page_108 now builds). The main
remaining reasons: FreeCAD's `Refine=false` and its lighter refine keep faces and edges
that Gusset always unifies, so a fillet of the unrefined shape differs in its corners by
about 1e-4 of the volume (Page_108) or picks an edge Gusset has merged
(Page_025, Page_026, Page_089; the picks of a split circle are found on the whole circle
now, but a fillet of half an edge is not expressible); a 10 mm fillet of one 22 mm edge that
OCCT refuses (Page_020, stripe status ok, cause not found); a first fillet of two faces that
removes 54 mm3 too much although none of its 18 edges lies between tangent faces
(Page_028); a volume 4e-5 off after ring pads over fillets (Page_015 fillets variant); these
three are not traced; a primitive
attached to a datum line at a tilt (Page_056-2), a MultiTransform of primitives about a
placed body (Page_089), lofts, helices, thickness and offset objects, hyperbola and
parabola arcs, shape binders, and profiles that are datum planes. Ellipses and elliptical
arcs of a sketch (`Part::GeomEllipse`, `GeomArcOfEllipse`: major axis from `AngleXU`, the
arc's parameters are eccentric anomalies, counter-clockwise; a mirrored plane is refused like
an arc) and `PartDesign` ellipsoids (a half ellipse turned about z, two radii) come in as
tree ellipses and a revolve (Page_022 x2, Page_029). Page_108 needs a boolean that keeps
FreeCAD's `Refine=false` result: the shim's `run_boolean` has a `simplify` flag but the
kernel API, the document feature and the eval op do not carry it.

### Amendment: `Refine=false` (2026-11-09)

The translator reads `Refine` of every PartDesign feature it turns into a joining, cutting or
intersecting tree feature (pad, pocket, revolution, groove, additive and subtractive pipes and
lofts) and of `Part::Cut/Fuse/Common`, and writes `"refine": false` when the stored value is
false (a missing property stays refined), also on MultiTransform and the pattern and mirror
objects it makes. An object that becomes two features here (a two-length pad, pocket, groove or
revolution: `_back` is the second side, where FreeCAD makes one solid) stays refined, since the
seam between its halves would be an edge FreeCAD's shape does not have (Page_093's chamfers). Gusset then rebuilds on the unrefined shape as
FreeCAD does (ADR 0004 and 0042 amendments), so face counts and the edges and faces that
fillets pick match. A first pad (`operation: new`) is not marked: it does no boolean.

Result on the 160 example files: coverage unchanged (126 as history); no file was gained.
Page_108 now builds the same pre-fillet shape as FreeCAD's stored one (70 faces, 138 edges
with `refine: false` on the pads, the pocket and the polar pattern; 34 faces refined), but
OCCT's fillet of its 48 edges then yields a face BRepCheck reports as unorientable that
ShapeFix does not repair (two edges of neighbouring teeth fail together, each alone and the
pair of the same tooth are fine), so the file stays at tier 2. Page_089 with its patterns
unrefined builds but is 5% off in volume (not traced).

### Amendment: pads and pockets up to a face (2026-11-09)

A pad or pocket of `Type` 3 (UpToFace) whose `UpToFace` names a planar `FaceN` of a feature's stored shape, ahead of the
sweep and with `Offset` 0, is written as `"end_condition": "to_face"` with the face's point (inside the face, from its
largest triangle) and outward normal, and `reverse` for a sweep against the sketch normal (a pocket cuts against it, `Reversed`
flips either); the stored-shape reader (`Edges`) now also returns each face (`SrcFace`). Anything else keeps the distance read
off the stored `AddSubShape`: an offset (FreeCAD's offset direction is not established), a curved face, a face behind the
sketch, a missing shape. A file whose tree has an extrusion up to a face is rebuilt as before; if the rebuild check fails
(the kernel wants the face to cover the whole profile, FreeCAD extends the surface) the importer retries every tree variant
with the distance path, so the change cannot lose a file the distance path rebuilds (checked by turning the through-all
pockets of Page_032 and others into up-to-face ones on each of their faces: the ones the kernel refuses come in through the
fallback). UpToFirst/UpToLast stay distances: their end face would have to be found among the base shape's faces by outline, which
`SrcFace` does not carry. `page67_bynormandc` (the one UpToFace in the 160 files) stays at tier 2: its sketch is tilted 45 degrees
to the face z = -25 behind it, so the pad is FreeCAD's two-sided up-to (-72.14 .. +28.28 mm along the sketch normal); Gusset
refuses a face behind part of the profile. Coverage unchanged: 126 of 160.

### Amendment: datum-plane profiles, pre-body pads, shape binders, thickness (2026-11-10)

Coverage 126 to 128 of 160. A pad or pocket whose profile is a `PartDesign::Plane` (AttachmentTutorial) takes
the plane's `Length` x `Width` rectangle, centred on the plane's placement, as a sketch named after the plane.
A pad of a file from before bodies names its sketch in `Sketch`, not `Profile`: the group-order pass reads
both, so a pad listed ahead of its sketch is put after it (Sketcher example 4). A `PartDesign::ShapeBinder`
is a datum like the origin: sketches attached to one keep their stored placement and constraints are not
translated, so nothing else is needed (Page_056-1 gets past it but stays at tier 2: its `Draft` array has
`Fuse=false`, so FreeCAD's shape is a compound of six overlapping copies whose volume is six times the
fused one, which no tree feature gives). `Part::Thickness` (skin mode, faces taken from the input feature's stored shape,
a negative value inward) is written as a `shell`; Page_037 still fails: Gusset's shell reports the walls too
thick for the part at any thickness inward and an invalid solid outward (a 600 mm dish pad with a pocket; not traced).
Helix sweeps (Page_085, Page_092): the tree sweep takes a `helix` (ADR 0042 amendment 2026-11-10) and
`fcstd_tree.rs` writes a `Part::Helix` used as the spine of a `PartDesign::AdditivePipe`/`SubtractivePipe`
(directly, or wrapped in a `PartDesign::FeatureBase`, which has a placement and stored shape of its own: Page_085's
helix is about world Z there, its own placement turns it to X) or of a `Part::Sweep` (one section, helix
spine of all its edges; `Frenet` or not both become `frenet`). The stored edges must lie on the cylinder of
the helix about the placed axis (a stored `Angle` the helix ignored, Page_085's 90, is dropped; an edge off the
cylinder passes it on as `taper_angle`, which the tree refuses). The axis must be a positive world axis
through the origin (the document's helix cannot say more). Both files still stay at tier 2, for FreeCAD's own
approximation of the sweep, not for the translation: Page_085 gives the same volume (7e-6) but FreeCAD's end
section is not the screw image of the profile (about 20 degrees of tilt by its top height; presumably its Frenet frame
on the BSpline helix; a mesh of
the stored shape at 0.001 mm tops at z 51.412, the exact solid at 51.5), so the bounding-box check fails by
0.09 mm; Page_092's profile is not centred on the helix point, so the same effect shows in the volume (1.2e-4: 639.222 against Pappus' 639.143 mm3 for the sweep).
Page_058 ("Sketch has no closed regions"),
Page_074 (the pipe path starts 8 mm from the profile plane) and Page_094 (loft similarity) were looked at only
for their reasons.
