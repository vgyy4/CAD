# ADR 0048: PCB design rule check additions

Status: accepted (2026-11-13). Follows ADR 0047 ("Next": hole-to-hole, courtyard overlap, silk on
pads, drill sizes). Clean room: written from KiCad's documented behaviour and file formats, no KiCad
source.

## Decision
Four rule groups join the design rule check (`gusset_pcb::drc::check`), in
`crates/gusset-pcb/src/drc_more.rs`. They use the existing `Violation` shape (`code`, `message`, `at`,
`layer`, `items`) and the existing `pcb.drc` command, so the app panel, CLI and MCP show them with no
new command, argument or tool text (the pcb toolset stays at its byte budget; the command's output
only gains keys in `rules_mm`). `ItemRef` gains `Footprint`, `Graphic` and `Text` for items that are
not copper.

| code | checks | items |
|---|---|---|
| `hole_to_hole` | edge to edge gap of two drilled holes (pad holes, slots as capsules, via drills) under `hole_to_hole` | the two pads/vias |
| `drill_size` | pad hole (a slot by its narrow side) under `min_through_hole`; via drill under `min_via_drill` | the pad or via |
| `via_diameter` | via copper under `min_via_diameter` (0: off) | the via |
| `courtyard_overlap` | footprints whose `F.CrtYd` (or both `B.CrtYd`) areas overlap by 0.001 mm2 or more; shared edges are fine | the two footprints |
| `silk_over_pad` | silkscreen strokes, filled shapes and text (our stroke font, hidden text skipped) of `F.SilkS`/`B.SilkS` closer than `silk_clearance` to a pad's exposed area on that side | the drawing or text, then the pad |

* Courtyards are closed polygons: `fp_rect`/`fp_poly`/`fp_circle` as drawn, lines and arcs chained end
  to end within 1 µm (open chains are ignored). Footprints without a courtyard are never reported.
* A pad's exposed area is its copper grown by the mask margin (`pad.mask_margin`, else the board's
  `pad_to_mask_clearance`) when it has a mask layer on that side, else its copper; through-hole pads are
  exposed on both sides. Bare silk touching nothing is never reported; with `silk_clearance` 0 only
  overlap counts.
* Values (`Rules`): `hole_to_hole` 0.25, `min_through_hole` 0.3, `min_via_drill` 0.2,
  `min_via_diameter` 0 (off), `silk_clearance` 0 mm. Sources: the board's `(setup (via_min_size ..)
  (via_min_drill ..))` (KiCad 5 writes them there; `Rules::for_board`), then the project file
  (`apply_project`): `min_hole_to_hole`, `min_through_hole_diameter` (sets the via drill minimum too),
  `min_via_diameter`, `min_silk_clearance`. KiCad 6+ keeps these in `.kicad_pro` only. The per-call
  overrides of `pcb.drc` are unchanged (no new arguments, for the tool byte budget).

## Results (2026-11-13)
* `gusset-pcb` tests 66 -> 73 (new `tests_drc_more.rs`, 7 tests on small hand-built boards): hole to hole
  (via/via at 0.2 mm gap violates, 0.3 mm passes, a larger rule violates, rule 0 is off; pad holes and an
  oval slot against a pad and a via); drill sizes (pad holes 0.25 / slot 0.25 narrow side violate, 0.3
  passes; via drills) and the via minimums read from the setup; courtyards (overlap of two rectangles
  with area and centre, chained lines, shared edge, front against back, two backs, no courtyard); silk
  (line through a pad, clear line, rule turning a clear line into a gap report, back silk against a
  front pad, mask opening reaching a line, reference text on a pad, hidden text, through-hole pad
  on both sides). `gusset-commands`: the project-rule test also checks the new minimums in `rules_mm`.
* One existing fixture (`tests_phase3::channel`) has reference texts over neighbouring pads, so its
  copper-only assertions filter `silk_over_pad` out.

## Not done
* Per-call overrides of the new rules in `pcb.drc` (needs tool text), per-class drill rules, microvias,
  blind/buried via drills, hole-to-hole between a hole and copper of another net on the same layer.
* `.kicad_dru` custom rules remain a Next item of ADR 0047.
* Silk against mask opening for pads whose mask margin differs by paste/ratio is as the pad's own margin.
* Unverified against KiCad's own results on real boards (no local corpus run): defaults follow
  KiCad's board setup defaults as documented (hole to hole 0.25 mm, through hole 0.3 mm).
