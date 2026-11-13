# ADR 0054: PCB rules per net class and KiCad custom rules

Status: accepted, implemented (2026-11-13). Follows ADR 0047 ("Next" → DRC), 0048 (more DRC) and
0053 (zones). Behaviour comes from KiCad's published rule-language and project-file documentation;
no KiCad source was used (clean room, ADR 0047).

## Decisions
* **Per-class limits.** `NetClass` gains `min_track_width`, `min_via_diameter`, `min_via_drill`
  and `diff_pair_width`, `diff_pair_gap` (the last two are stored, not used yet). KiCad's own class
  `track_width`, `via_diameter`, `via_drill` are *defaults for new items*, not limits: a DRC that
  treated them as minimums would flag every thinner track the user routed on purpose. So the
  limits are separate Gusset keys in the `.kicad_pro` class object (`min_track_width`, ...; KiCad
  ignores unknown keys) and are set with `pcb.net_class` (five optional fields, no description
  text). A class limit raises the board's minimum for its nets, never lowers it
  (`Rules::min_track_width_of`, `min_via_diameter_of`, `min_via_drill_of`).
* **Codes.** Existing codes are reused: `track_width`, `via_diameter` (ADR 0048), `drill_size`
  (via drills). `pcb.add_track` / `pcb.add_via`, the router and the editor already default to the
  class's width and sizes (`track_width_of`, `via_of`); a test pins it.
* **Custom rules** (`gusset-pcb::dru`, `CustomRules::parse`): the documented
  `(version 1) (rule "name" (condition "...") (layer ..) (constraint ...))` language, a subset.
  * Conditions: `A.NetClass`, `A.NetName`, `A.Type`, `A.Layer` (and `B.` for the same) compared with
    `==` / `!=` against a quoted string (`'x'` or `"x"`; `*` and `?` match as wildcards on the
    right-hand string) or another property; `&&`, `||`, `!`, parentheses. `Type` is compared
    case-insensitively (`track`, `via`, `pad`, `zone`). `Layer` is true if the item is on that
    layer; the DRC passes the layer being checked. Only copper layer names are accepted in layer
    comparisons (`outer`/`inner` are not), so a name that could never match is a warning, not a
    silent miss.
  * `(layer "F.Cu" | "outer" | "inner")` clause.
  * Constraints: `clearance`, `hole_to_hole`, `edge_clearance` (`min` only), `track_width`,
    `via_diameter`, `hole_size` (`min`, `max`; `opt` is read, not checked). Lengths take `mm`, `in`,
    `mil` or none (mm).
  * **Order.** The last matching rule that has the constraint wins (KiCad's documented order:
    later rules override earlier ones). A matching custom rule *replaces* the board and class
    value for that item (it may loosen as well as tighten). Pair constraints match when the
    condition holds for (A,B) or (B,A). `B.` in the condition is accepted only with the pair
    constraints `clearance` and `hole_to_hole`.
  * **Where applied.** `clearance`: copper pairs and zone fills against other copper (not fill
    against fill). `track_width`: tracks. `via_diameter`: vias. `hole_size`: via and pad drills
    (code `drill_size`). `hole_to_hole`: all drilled holes. `edge_clearance`: copper to edge
    (holes keep the board's `hole_to_edge`).
  * **Skipped with a warning, never half-applied:** unknown properties, functions
    (`insideArea`, `hasNetclass`, ...), unknown constraints (`skew`, `length`, `disallow`,
    `silk_clearance`, ...), `severity` other than `error`, `layer` clauses other than copper names,
    `outer`, `inner`, units other than mm/in/mil, `clearance`/`edge_clearance`/`hole_to_hole` with
    `max`, a `B.` condition on a single-item constraint, a missing `(version 1)` (then no rules
    at all), text that does not parse as S-expressions (no rules). Each skip is one line in
    `warnings`.
  * **Hostile input.** Whole file at most 1 MiB, 12 levels of lists, 200 000 nodes, 2000 rules
    (the rest dropped with a warning), 4096 characters and 64 comparisons per condition, nesting
    of parentheses and `!` at most 24 deep (the parser is recursive only to that depth; chained
    `&&`/`||` are iterative and capped by the comparison count), 100 warnings. `#` comment lines
    are dropped before reading. Fuzz target `pcb_dru` (own workspace, `Cargo.lock` unchanged)
    parses arbitrary text and evaluates every kind against sample items.
* **Where the file lives.** `pcb.open` reads `<board>.kicad_dru` beside the board (1 MiB cap, same
  path rules as the project file) and keeps its text in the project blob under
  `gusset_custom_rules` (the document model has no third hash; the project blob already rides
  history, undo and `.gus` snapshots). `pcb.save` writes it back to `<name>.kicad_dru` and strips
  the key from the written `.kicad_pro`. `pcb.net_class` edits keep it.
* **Report.** Each violation caused by a custom rule carries `rule_user_text` (null otherwise) and
  its message ends in `(rule "name")`. `pcb.drc` adds `rule_warnings_user_text` (the skip lines)
  and `rules_mm.custom_rules` (rules in use). Names and warnings are `_user_text` (untrusted
  file content). No new tool; the `pcb` toolset budget test passes unchanged (the five class fields
  add a few hundred bytes).

## Results (2026-11-13)
* Tests: `gusset-pcb` 98 -> 108 (nine in `tests_dru.rs`, one local-only: class track width / via diameter and drill, class
  default not a limit, class limit never loosens; custom clearance by class with the rule named,
  two signal nets unaffected, loosening; override order both ways; track width min/max by type and
  layer; `&&`, `||`, `!`, parentheses, wildcards, property to property, mil units; hole size, hole
  to hole, edge clearance; ten skipped-with-warning cases; hostile files: 100 000-deep parentheses,
  5000 `!`, 500-term chains, 2050 rules, NUL/BEL/NaN). `gusset-commands` `tests_pcb_rules.rs` (2):
  project file with class limits and diff pair, `.kicad_dru` read, defaults from the class,
  report with rule name and warning, save writes `.kicad_dru` and a clean `.kicad_pro`,
  `pcb.net_class` stores the new fields and keeps the rules, hostile file gives one warning.
* **Demo boards** (local only, `GUSSET_KICAD_SAMPLES`, test `local_samples_custom_rules`; they
  ship no `.kicad_dru` or classes, so a representative file was applied: GND tracks min 0.25,
  track pairs clearance 0.25, vias min 0.7, drills min 0.35, track edge clearance 0.5). New
  reports (rule in brackets), board by board:
  StickHub: gnd wide 49, close pairs 572, via size 87, drills 81, edge 16 (clearance 1376 -> 1027
  plus the 572, as the rule replaces the default for track pairs); kit-dev-coldfire: gnd wide 6,
  close pairs 310, via size 80; video: gnd wide 470, close pairs 891, edge 24; complex_hierarchy,
  ecc83-pp, interf_u, pic_programmer: none. Checked by hand: the reported tracks are 0.15/0.20
  wide on GND against 0.25, pairs 0.248-0.15 mm apart against 0.25, edge distances 0.41-0.43
  against 0.5, vias 0.5/0.635 against 0.7; no false positives found. Time: at most +65 ms
  (82 vs 19 ms on complex_hierarchy, where no rule matches but each pair is evaluated).

## Not done
* Area functions (`insideArea`), `A.Name`, `A.Pad_Type`, `hasNetclass`, `severity` other than
  error, `outer`/`inner` in conditions, `clearance` `max`, `disallow`, `length`/`skew`/diff-pair
  constraints (diff-pair width/gap are stored only), `annular_width`, `physical_clearance`.
* Custom rules for zone fill against fill, and `hole_to_edge`.
* The class limits are Gusset keys: KiCad shows and rewrites its own project file, so a class
  edited in KiCad may lose them (the `.kicad_dru` file is the KiCad-native way to carry such
  limits; a later step could write class limits out as rules).
* A condition's `==` with `*` / `?` wildcards follows common KiCad usage as understood from its
  documentation; unverified against KiCad's own results.
