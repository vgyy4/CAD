# Fusion importer corpus tally (adafruit-more)

Why Fusion designs do not come in as editable history. Measured with
`crates/gusset-cli/tests/f3d_history.rs` over a broad sample of public designs. No sample
bytes are kept in the repository; only names, counts and reasons.

## Corpus

- Set `adafruit-more` of `scripts/fetch-samples.py`: 150 `.f3d`/`.f3z` designs spread evenly
  (every ~Nth path of the sorted tree) over github.com/adafruit/Adafruit_CAD_Parts at the
  fetched head, 184 MiB, MIT licence (Adafruit Industries). The repo has more designs than
  this; raise `MORE_FILES` to widen the sample. Fetch stops at 1.5 GB or below 4 GB free disk.
- Files are named `<folder>__<file>` in `DEST/adafruit-more`. Examples below use the file name only.
- Run: `GUSSET_F3D_SAMPLES=<DEST>/adafruit-more GUSSET_F3D_VERBOSE=1 GUSSET_F3D_RAW=1 cargo test -p gusset-cli --test f3d_history fusion_designs_come_in -- --nocapture`
  (debug build, OCCT kernel, 300 s import timeout). Whole run: about 48 minutes.
- Reader/importer state: the committed tree at the time of the run (uncommitted edits by others
  excluded as far as the build allowed).

## Result

| outcome | designs |
|---|---|
| as history (design timeline rebuilt) | 59 (39 %) |
| as bodies only (history refused or absent) | 90 (60 %) |
| failed (no import at all) | 1 (timeout) |

The 59 history designs mostly carry notes such as "N sheet (surface) bodies left out" and "the file
stores the steps of the design, not its end result" (stored solids that are intermediate states);
those are warnings, not refusals.

## First blocking reason per design (91 designs not as history)

Grouped by category, sorted by count. The importer reports only the first blocking reason.

| # | category | examples |
|---|---|---|
| 24 | Profile decoding (total, see below) | |
| 24 | State check mismatch: the features give a volume different from the stored shapes, or a stored solid is neither a state of the model nor a cut tool | microUSB breakout.f3d, 2000mAh Battery.f3d |
| 12 | Unsupported feature type (total, see below) | |
| 8 | Unresolved edges: fillet/chamfer edges not all resolved, or "no edge has its midpoint at ..." | 16x NeoPixel Ring.f3d, 2200mAh Battery.f3d |
| 7 | No feature history stored in the file | Adafruit 2020 Extrusion.f3d, Battery 9V.f3d |
| 4 | Extrude limits (total, see below) | |
| 3 | Rebuild failure (total, see below) | |
| 2 | Revolve axis is a topological reference (not decoded) | 16mm button.f3d, Snow Globe.f3d |
| 2 | Mirror: plane is not an origin/construction plane frame, or it mirrors bodies | HalloWing M0 Express Rev C.f3d, PyBadge.f3d |
| 2 | Base Feature body not in its state list / not among the last bodies | STEMMA Buttons.f3d, PyPortal Pynt.f3d |
| 1 | Sweep path is not a list of sketch curves of one sketch | 8mm soft button.f3d |
| 1 | Pattern of components / several bodies / partial angle | Adafruit PyGamer.f3d |
| 1 | Import timeout (300 s, "importing took too long and was stopped") | PC Joystick Adapter.f3d |

### Profile decoding (24)

| # | reason | examples |
|---|---|---|
| 15 | profile regions not decoded: loop does not close | Tactile On Off Switch.f3d, Verter 5V USB.f3d |
| 4 | profile regions not decoded: curve piece n/m not reproduced | 5in HDMI Backpack.f3d, Adafruit Motor HAT.f3d |
| 2 | too many profile picks | Perma-Proto FullSize.f3d, Minty Perma-Proto.f3d |
| 1 | profile regions not decoded: no interior point | Adafruit FONA Feather 32u4.f3d |
| 1 | profile regions not decoded: curve unknown | Mag Connector (3-pin, Right Angle, Pads).f3d |
| 1 | profile is not a sketch profile (face or PCB data) | BQ25185 3V Buck Case.f3z |

### Unsupported feature type (12)

| # | type | examples |
|---|---|---|
| 7 | a feature with no name and a `...DBAF...` type id | MLX90393 QT.f3d, Adafruit LPS28 Pressure Sensor.f3d |
| 2 | ElectronicsPackageGenerator | Proto PiCowBell.f3d, Proto Tripler PiCowBell.f3d |
| 1 | Thread | RGB button.f3d |
| 1 | Derived Base Feature | Sharp Memory Display Breakout.f3d |
| 1 | MirrorComponent | MONSTER M4SK revD.f3d |

### Extrude limits (4)

| # | reason | examples |
|---|---|---|
| 2 | extrudes to both sides by different distances | Trinket 5V.f3d, 7-segment display backpack.f3d |
| 1 | extent is to-object / through-all, not a distance | Mag Connector (4-pin, Right Angle, Pads).f3d |
| 1 | tapered | microUSB B Connector Cable.f3d |

### Rebuild failure (3)

| # | reason | examples |
|---|---|---|
| 2 | "the import has built the model too many times" on a Base Feature | TFP401 HDMI Breakout.f3d, MEMENTO.f3z |
| 1 | Shell does not build in the kernel ("shell failed") | Adafruit Arcade Bonnet.f3d |

## Takeaways for the reader

1. Profile decoding (24) and the state-check mismatch (24) are each a quarter of the refusals;
   "loop does not close" alone is 15 designs, so it is the single most common concrete reason.
2. Unsupported types (12): the unnamed `DBAF` type is the biggest, then ElectronicsPackageGenerator.
3. Unresolved fillet/chamfer edges (8) come next; the state-check mismatch group hides several
   sub-causes (volume differs, stored solid not a state, fillet copy count) not split here.
4. 7 designs have no stored history and cannot be improved by the reader.

## Slow designs

PC Joystick Adapter.f3d hit the 300 s import timeout; Terminal Block FeatherWing.f3d took about
250 s (imports as history, 156 bodies); Feather RP2040 USB Host.f3d, MagTag.f3d, Adafruit Crickit
HAT.f3z and Solenoid Driver 8 Channel.f3d took 70-80 s each. Most others took 5-30 s.

## Re-run after round 13 (2026-11-10)

Same set and command, after the reader changes of ADR 0039's round 13 amendment
(`docs/formats/fusion-design-stream.md` section 21).

| outcome | before | after |
|---|---|---|
| as history | 59 | 76 (51 %) |
| as bodies only | 90 | 73 |
| failed (300 s timeout, PC Joystick Adapter) | 1 | 1 |

First blocking reason of the 74 designs not as history:

| # | category | before | notes |
|---|---|---|---|
| 16 | State check mismatch | 24 | 11 "a stored solid is neither a state nor a cut tool" (5 of them 2025 files whose layer bodies come from `PCBExtrude`/`BoxPrimitive` features the reader does not walk; the others store bodies of components the tree lacks, or a solid read inside out), 1 box mismatch (Crickit HAT: a joint), 2 with blends after the last stored state, 2 others |
| 9 | Unresolved fillet/chamfer edges | 8 | 7 in the reader (edges whose faces are neither modelled nor stored under their names), 2 at the rebuild ("no edge has its midpoint at") |
| 7 | No feature history stored | 7 | unchanged |
| 6 | Extrude limits | 4 | 3 layouts not recognised, 3 to-object / through-all extents, 1 taper (two-distance extrudes are read now) |
| 6 | Unsupported feature type | 12 | 2 ElectronicsPackageGenerator, Thread, MirrorComponent, Derived Base Feature, one unnamed class; the `DBAF` entry (7) is read |
| 5 | Profile decoding | 24 | 2 curve unknown, 1 piece not reproduced, 1 no interior point, 1 face/text profile; "loop does not close" (15) is gone; 2 more hit the importer's 256-pick limit |
| 5 | Rebuild limits / kernel | 3 | 3 "built the model too many times", 1 shell, 1 extrusion that does not build |
| 3 | Combine | 0 | reached now (the header part "2x12 short female"); its operation is not confirmed |
| 3 | Base Feature state lists | 2 | |
| 2 | Revolve axis | 2 | |
| 2 | Draft | 0 | reached now |
| 6 | others, one each | | sweep path, pattern of components, mirror making another number of bodies, extrude changing several bodies, shell face, chamfer without distance |

Designs gained (17), none of the 59 lost: 1833, 2829, 2884, 3191, 3800, 3857, 4062, 4900, 5217,
5302, 5325, 5397, 5645, 5665, 5769, 5877, 6421. Most of the "stored solid of 1.2331 mm3" refusals
of the first run are answered by the round 12 step check (hole tools that lie in slots
of the outline), which the first run did not have yet (its own first re-run, before the typed references, already showed them as history); round 13 adds the loop and piece rules
(2829, 2884) and the typed references (1833, 3857).

## Re-run after round 14 (2026-11-10)

Same set and command, after the changes of ADR 0039's round 14 amendment
(`docs/formats/fusion-design-stream.md` section 22). The run was made with another session's uncommitted
work on the importer's limits in the build, which changes the reasons of three designs (2219, 5420, 6250:
from "built the model too many times" to a state-check refusal) and no outcome.

| outcome | round 13 | round 14 |
|---|---|---|
| as history | 76 | 78 (52 %) |
| as bodies only | 73 | 71 |
| failed (300 s timeout, PC Joystick Adapter) | 1 | 1 |

Designs gained (2), none lost: 3900 Hallowing (Combine as a join, the implicit second pattern direction, the
construction flag of its line layout) and 878 7-segment display backpack (extrude to a body, tools told apart
by where they are). The 18-design set stays at 11.

What moved without becoming history:

| design | before | now |
|---|---|---|
| 3651 3.5in TFT FeatherWing | Combine | Split (not read) |
| 4200 PyBadge | Combine | box check: a jointed header stands elsewhere than its stored body (joints) |
| 4242 PyGamer | pattern | a Base Feature body not among the last of its state list |

Still open, by first blocker (71 designs as bodies, 1 timeout): state check 20 (16 "a stored solid is neither a
state nor a tool": the five 2025 PCB files, whose PCBExtrude and BoxPrimitive features are decoded in section 22
but not converted, stored package bodies the tree lacks, 805, whose cut removes 2 mm3 where its stored tool holds
20, and three designs that hit the rebuild limit before; 2 box mismatches from joints; 2 with blends after the last
stored state), unresolved blend edges 10 (7 in the reader: side faces of an extrude whose generator numbers the
profile does not explain and that no stored plane carries; 3 at the rebuild), no history stored 7, unsupported
types 8 (Split, Thread, MirrorComponent, OffsetFaces, Derived Base Feature, one unnamed class,
ElectronicsPackageGenerator x2), profile decoding 5 and the pick limit 2, Base Feature state lists 4, extrude
limits 4 (two to-object extents, a taper, a cut of several bodies), revolve axis 2, draft 2, shell 2, and one each
of sweep path, mirror count, chamfer without distance, an extrusion that does not build, a pattern that makes a
new body.

## Re-run after round 15 (2026-11-10)

Same set and command, after the changes of ADR 0039's round 15 amendment
(`docs/formats/fusion-design-stream.md` section 23) and, from another session committed since the
round 14 run, the importer's wider limits and faster trial builds (ADR 0042 amendment).

| outcome | round 14 | round 15 |
|---|---|---|
| as history | 78 | 90 (60 %) |
| as bodies only | 71 | 60 |
| failed (300 s timeout) | 1 | 0 |

Designs gained (12), none lost:

| design | by |
|---|---|
| 6067 LPS28, 6250 PCM510x, 6494 ADS7128, 6501 TMF8806, 6522 TMF8801, 6538 RV-8803 | the PCB features (BoxPrimitive, PCBExtrude: drill, masks, copper, plated holes), stored bodies per lump, one copy of a stored tool per body; 6501 and 6522 also the component of the edited timeline |
| 805 slide switch | regions that share an edge are swept as one (its last cut removed 2 of 20 mm3) and join tools count for the box |
| 2900 Feather OLED Wing | the solids of a body that is several solids apart are states |
| 4367 TFT Gizmo | the same two step-check changes |
| 1606 Perma-Proto FullSize, 723 Minty Perma-Proto | the other session's limit of profile picks |
| 5753 PC Joystick Adapter | the other session's import speed (it timed out before) |

The 18-design set stays at 11.

Still open, by first blocker (60 designs as bodies): reader refusals 36 (unresolved blend edges, unsupported types
such as Split, Thread, MirrorComponent, OffsetFaces, Derived Base Feature and ElectronicsPackageGenerator, extrude
extents to an object, profiles with a spline or an unread curve, Base Feature state lists, revolve axes, drafts,
shells), state check 11 (stored solids no feature of the tree makes: 2219, 3211, 4313, 4745, 5021, 5420; a part
that a joint moved: 3957, 4200, 4800; blends after the last stored state: 2011, 931), no history stored 7, rebuild
failures 6 (blend edges not found on the model, a shell and an extrusion that do not build, a pattern that makes a
new body).

## After the joints (2026-11-11, the designs with joints only; no full re-run)
Joints are decoded and checked against the placements the stream stores per occurrence path (section 24 of
`fusion-design-stream.md`); a design whose joints all pass is placed by those records.

| design | before | now |
|---|---|---|
| 3957 | box check (the inserted header stood where its insert put it) | history |
| 4200 | box check (two jointed headers had no placement) | history |
| 4400 | box check | history |
| 4800 | box check | unchanged: its joint is verified and its bodies already stood right; the mismatch is not a joint's |

The 18-design set: 12 as history (3957 added). The four designs of the larger set that have joints and came in as
history before (4062, 4367, 4465, 5302) still do. No design's first blocker is a joint: the designs whose joints
stay unverified (occurrences of inserted designs, the other origin layout, section 24) stop at other features or
come in as history already (4862).

## Re-run after round 16 (2026-11-11)

Run on a tree without the joints above.

Same set and command. "Before" is the tree at the start of round 16 (with the other session's commits
since round 15: 99, not 90); "after" adds extrude starts (`ProfileOffset`), OffsetFaces, the end bodies'
box in the step check, copies of a placed blob moved by the kernel and edges past a closed curve's end
(ADR 0039 round 16 amendment).

| outcome | before | after |
|---|---|---|
| as history | 99 (66 %) | 100 (67 %) |
| as bodies only | 51 | 50 |
| failed | 0 | 0 |

Gained: 3464 Joy Bonnet (Extrude2-8 start on the board's top or an earlier cap, OffsetFaces3, the end
body confirms 6406.567 mm3 and the box after Move1). None lost. First blocker moved: 6299 Mini Sparkle
Motion Case (OffsetFaces read; next: a tapered extrude), 2190 Verter 5V USB (now refused earlier: an
extrude whose start and to-object end are both face recipes, which is which not known; it failed the
rebuild before). Whole run 2,297 s instead of 2,834 s. The 18-design set stays at 11.

Still open, by first blocker (50 designs as bodies): reader refusals 26, state check 11 (stored solids
no feature makes, bodies a joint moved), no history stored 7, rebuild failures 6.

## Combined re-run after the joints and round 16 (2026-11-11)

Same command on both sets, with the joints and round 16 together: adafruit-more 103 of 150 as history (3957,
4200, 4400 added by the joints; none lost), adafruit 12 of 18. Whole adafruit-more run 2,304 s.

Remaining first blockers (adafruit-more and adafruit): no feature history 7; stored-state mismatch 8 (2219, 3211,
4242, 4313, 4745, 5021, 4800 box, 2441 copies); profile regions not decoded 6 (324, 512, 858, 2756, 3027, 5360);
profile not a sketch profile 2 (6124, 6126); revolve axis a topological reference 2 (1145, 3722); tapered
extrude 2 (592, 6299); ElectronicsPackageGenerator 2 (5905, 5967); rebuild failures 4 (sweep 4863, chamfer 1463,
3632, shell 3422); one each: Thread 3350, Split 3651, MirrorComponent 4343, Derived Base Feature 3502, Base
Feature 4431, pattern of inserted-design occurrences 5128, sweep path 3101, shell face 4632, extrude body 610,
extrude between two faces 2190, draft 355 and 258, chamfer edges 5981 and 3093, unknown feature class 5358.

## Combined re-run after round 17 (2026-11-11)

Same command on both sets with round 17 A (profile regions, tapered extrudes, whole-turn revolves) and B (dropped
Base Feature inputs, the many-loop cut rule, in-plane extrude directions, body selections) together: adafruit-more
112 of 150 as history (was 103: 592, 2219, 2441, 3211, 4242, 4313, 4745, 4800, 5021 added; none lost), adafruit
12 of 18. Whole adafruit-more run 2,344 s.

Remaining first blockers: no feature history 7 (1221, 1321, 1400, 2629, 5412, 6049, 6114); rebuild failures 7
(sweep 4863, shell 2756 and 3422, chamfer 1463 and 3632, fillet 2232, the PCB board extrude 2348); edge and face
picks 9 (chamfer edges 3093, 5981, 6299; draft faces 258, 324, 355; shell faces 3722, 4632; offset face 3027);
unsupported features 9 (Thread 1145 and 3350, Split 3651, MirrorComponent 4343, Derived Base Feature 3502,
ElectronicsPackageGenerator 5905 and 5967, class #BC# 5358 and 5360); profiles that are faces or PCB data 6124,
6126; one each: sweep path 3101, pattern of inserted-design occurrences 5128, a pattern of components 512, the body
an extrude changes 610, a loop that does not close 858, an extrude between two faces 2190, a Base Feature body
4431.

## Combined re-run after round 18 (2026-11-11)

Same command on both sets with round 18 C (rebuild failures: sketches of another component, shell face points,
blend and multi-solid shell fallbacks in the shim) and D (faces of any surface picked by boundary points, drafts):
adafruit-more 115 of 150 as history (was 112: 2756, 3027, 3632 added; none lost), adafruit 12 of 18. Whole
adafruit-more run 2,267 s.

Remaining first blockers: no feature history 7; stored-state mismatch 5 (355, 4863, 1463 and 3422 after the last
stored state); unsupported features 11 (Thread 324, 1145, 3350; ElectronicsPackageGenerator 5905, 5967; class
#BC# 5358, 5360; Split 3651; MirrorComponent 4343; Derived Base Feature 3502; CoilPrimitive 3722); profiles that
are faces or PCB data 5981, 6124, 6126; one each: a body mirror on a work plane 2232, the board notch OCCT cannot
hold 2348, a Combine target 3093, chamfer edges 6299, draft of an end cap 258, a shell with no removed face 4632,
sweep path 3101, pattern of inserted-design occurrences 5128, a pattern of components 512, the body an extrude
changes 610, a loop that does not close 858, an extrude between two faces 2190, a Base Feature body 4431.

## Combined re-run after round 19 (2026-11-11)

Same command on both sets with round 19 E (threads read and modeled ones refused by what they are, a mirror of
faces made by several features) and F (package generators, bodies moved between components, Derived Base
Features, two-sided tapered extrudes, two-distance chamfers, sweep paths of 45-byte records): adafruit-more 116 of
150 as history (2232 added, none lost), adafruit 11 of 18. 4862 is lost: its inserted header used to fail to read
and came in as stored bodies; now it is rebuilt and the step check refuses the board's hole cut (260.85 mm³
removed against 92.10 mm³ of stored tools), the same excess that stops 5967. Round 19 F's findings moved the other
blockers on (section 30 of `fusion-design-stream.md`): 5905 MirrorComponent, 5358/5360 a pin fillet one pitch off,
3502 Bend, 5981 a thin-wall extrude of an open chain, 6124/6126 sketch text in a font the file does not hold.

## Combined re-run after round 20 G (2026-11-11)

Same command on both sets with round 20 G (end bodies compared solid by solid, symmetric extrudes measured each
side, body-pattern copies counted, blends split per body, revolves about an origin axis, end bodies that refuse):
adafruit-more 120 of 150 as history (was 116: 355, 1463, 3422, 4863 added; none lost), adafruit 13 of 18 (4863
added). 1145 and 3350 stop at their modeled threads; with the threads left out 1145 is refused by its end bodies
and 3350 stops at Fillet2 (edges not resolved).

## Combined re-run after round 20 G and H (2026-11-11)

Round 20 G on top of round 19 E and F: adafruit-more 121 of 150 as history (116 before: 355, 1463, 3422, 4863
and 5967 added; none lost), adafruit 13 of 18 (4862 and 4863 added). 4862 and 5967 came in with G's body-pattern
fix: the step check counted pattern copies in a run of new bodies but not after a single feature, so the board
hole cuts looked too large (section 32).
