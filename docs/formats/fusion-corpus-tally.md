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
