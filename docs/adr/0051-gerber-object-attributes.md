# ADR 0051: Gerber object attributes and a test-only Gerber re-reader

Status: accepted (2026-11-13). Follows ADR 0047 ("Next" → "From phase 1a"): Gerber X2
object attributes, re-reading our own Gerbers with a permissive parser in tests, and a
corpus test over local KiCad samples. Clean room: the public Ucamco Gerber Layer Format
Specification only; no KiCad or gerbv source.

## Decision
**Object attributes** (`gusset-pcb::gerber`). The writer keeps the object attributes in
force and emits `%TO` for those that change and `%TD.<name>*%` for those that no longer
apply, before each object:
* `.N,<net>` on every copper object: tracks, arcs, vias, pads (and pad primitives) and zone
  fills. An object on no net carries none; a pad on no net carries `.N,N/C` (the spec's
  value for a pad connected to nothing).
* `.P,<reference>,<pad number>` on pads on copper layers, only when both are non-empty
  (a mounting hole's pad has no number, so `%TD.P*%` follows it).
* `.C,<reference>` on every object of a footprint, on any layer: pads, footprint graphics
  and footprint texts.
* No `.N`/`.P` on mask, paste, silkscreen or outline layers. The optional pad function field
  of `.P` is not written. File and aperture attributes are unchanged (`.AperFunction` is
  still set around apertures and regions and deleted by name, so it does not disturb the
  object attributes).
* Field strings are escaped per the spec (`escape_field`): `%`, `*`, `,`, `\`, control
  characters and everything outside printable ASCII become `\uXXXX`, one per UTF-16 unit
  (a character past U+FFFF is a surrogate pair). Spaces stay.

**Re-reader** (`gerber_read.rs`, `#[cfg(test)]`, ~400 lines, written from the spec). It
parses `%FSLA`, `%MO`, `%AM` (recorded, body not interpreted: we emit none), `%AD` (C, R, O,
P, macro), `%TF/TA/TO/TD`, `G01/G02/G03/G04/G36/G37/G74/G75`, `Dnn` selection and
D01/D02/D03 with modal coordinates, `M02`. It rejects: a missing format, unit or `M02`;
an object on an undefined aperture or none selected; arcs without `G75` (and `G74`);
regions whose contours are not closed; arcs whose start and end radii differ by more than
rounding; coordinates beyond ±9999 mm; file attributes after the first object; a raw
delimiter in a field or a bad `\u` escape.

`write_layer` runs `gerber_read::verify` on its own output **in every test build**, so
all existing and future tests that write Gerbers check them (a panic names the layer and
prints the file). `verify` also compares against the board, per copper layer: the
multiset of nets of track objects, via pads and zone fills, and the set of
(reference, pad, net) triples of pad objects; every `.C` must name a footprint, and no
net or pad attribute may appear off copper.

## Results
* `gusset-pcb`: 85 unit tests (was 79), 6 new in `tests_gerber.rs`: escaping round trip,
  attributes on the test board (nets, pads, components, zone fill, silk with `.C` only),
  a net renamed to `+5V,Ünit*` escaped and decoded, every layer of the fab set re-read,
  broken files rejected, and the local corpus test.
* Local KiCad samples (`GUSSET_KICAD_SAMPLES`, release build; skipped otherwise): StickHub,
  complex_hierarchy, ecc83-pp, interf_u, kit-dev-coldfire, pic_programmer and video all
  write, re-read and match their boards (9 to 11 files each; video: 24 803 objects) with no
  findings; the whole run takes about 0.6 s.
* `gusset-commands` 281 tests unchanged and passing. The `pcb_read` fuzz target calls
  `gerber::write_all`, which is unchanged in signature and still builds. No MCP tool changes.

## Consequences / Next
* Files are larger by one short statement per change of owner. Viewers and CAM tools can now
  select by net and component.
* The checks are structural: they do not render, so they do not prove geometry is right
  (rasterised review remains manual).
* Not done: the pad function field of `.P`, `.CXxx` component characteristics, `.N` on
  mask and paste layers (the spec limits them to copper), `.AperFunction` refinements
  (`Heatsink`, `EtchedComponent`), macros in the writer (the reader records them).
