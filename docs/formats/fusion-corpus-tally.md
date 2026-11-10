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

Designs gained (17): the "loop does not close" and "curve piece" designs that had no further
blocker, the mirror and two-distance ones, and those whose hole tools now lie in the board
(typed references). None of the 59 was lost.
