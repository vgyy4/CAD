# ADR 0018: Gear generator

- Status: accepted
- Date: 2026-10-12

## Context
The owner asked for gears as the first generator (docs/DESIGN.md §1, "Easy for beginners, never
in the way of experts"): involute spur and helical external gears with module, tooth count,
pressure angle (default 20 deg, ISO 53), face width, bore, profile shift, backlash and a root
fillet. A generator must not be a black box: it writes ordinary parameters, sketches and
features, everything stays editable in the parameter table, the code view and over MCP, and
experts skip the form because the generator is a registry command with all options as arguments.

## Decisions

### 1. One command, no feature type
`generate.gear` (MCP tool `generate_gear`; the GUI form is generated from its schema like every
command's) edits the document in one undo step and then is gone: the document contains only
`param`, `sketch`, `extrude` and `sweep` statements. Deleting the generator's output, renaming it,
reordering it or editing a number works like for hand-made features. Running it again with
another `name` makes another gear; the name is the feature name of the solid and its lowercase
the parameter prefix (`Gear1` gives `gear1_module`), so several gears coexist.

Options (all expressions, so they may refer to document parameters, with the defaults shown
in the form): `module` 2 mm, `teeth` 20, `pressure_angle` 20 deg, `face_width` 10 mm,
`bore` 6 mm (0 = solid), then the advanced ones: `shift` 0, `backlash` 0 mm, `fillet` 0.38
(root fillet radius as a fraction of the module, ISO 53), `helix` 0 deg, and `name`. The
essential five carry a form-only schema hint (`x-essential`, removed from the MCP tool list to
keep it small; `required` cannot be used because every option has a default), which is what
the generic form shows before its "More" section, so a beginner sees module, tooth count,
pressure angle, width and bore, and the advanced ones sit behind the disclosure with their
defaults pre-filled. A value must carry its unit (`2 mm`, `20 deg`); a bare number where a length or
an angle belongs is refused, because the text is written into the parameter as given.

### 2. Parameters
The options become parameters (`gear1_module`, `_teeth`, `_pressure_angle`, `_face_width`,
`_bore`, `_shift`, `_backlash`, `_fillet`, and `_helix` for helical gears) with a short comment,
followed by the derived dimensions as ordinary parameters that anyone can read or reuse:
`_transverse_module`, `_transverse_angle`, `_pitch_diameter`, `_base_diameter`, `_tip_diameter`,
`_root_diameter`, `_tooth_angle`, `_fillet_radius`, `_cutter_depth`, `_cutter_offset`,
`_fillet_span`, `_flank_roll`, `_tip_roll`. A parameter that already exists is an error (the
hint suggests another name); nothing is written then.

### 3. The tooth profile
In the plane of rotation the gear is cut by a basic rack with pitch `pi * m_t` and flank angle
`alpha_t`, displaced by `x * m_n` (profile shift); for spur gears these are the module and the
pressure angle, for helical gears `m_t = m_n / cos(beta)` and `alpha_t = atan(tan(alpha_n) /
cos(beta))`, while addendum (`m_n (1 + x)`), dedendum (`1.25 m_n`) and tip radius (`0.38 m_n`)
keep the normal module. Backlash `b` thins each tooth by `b` on the pitch circle (the half-angle
at the base circle shrinks by `b / (m_n z)` per flank).
* The straight flank of the rack generates the **involute**: radius `r_b / cos(phi)`, angle
  `-(theta_b - (tan phi - phi))` from the tooth axis, with `theta_b` half the tooth angle at
  the base circle. Tooth thickness on the pitch circle is `pi m/2 + 2 x m tan(alpha) - b`,
  verified by measurement (golden test: a thin ring intersected with the solid).
* The rounded tip of the rack generates the **root fillet** as an exact trochoid, the envelope of
  the tip circle: for the contact normal at angle `psi` (the pressure angle where it meets the
  involute, 90 deg where it touches the root circle) the point is at
  `R = sqrt((r - D)^2 + (D / tan psi)^2)`, angle `-pi/z + u_I / r + atan2(D / tan psi, r - D)`
  (+ backlash term), `D = cutter_depth + rho sin psi`, `u_I = cutter_offset + rho cos psi - D / tan psi`. It meets the involute without a kink (tested numerically)
  and the root circle tangentially; the flat tip of the rack gives the root circle.
* **Undercut**: when the rack flank reaches below the tangent point of the line of action
  (`D_J > r sin^2 alpha`) the foot of the involute would be cut away and the profile is not a
  clean involute plus fillet. The generator refuses with a hint naming the smallest tooth count
  and the profile shift that fix it. Other geometric limits are checked the same way: pointed
  teeth (a tip land below 0.1 m; below 0.25 m the generator warns, `warnings` in its result,
  because narrow tips chip, especially on hardened gears), a fillet that folds over itself or
  leaves no root land, a rack tip with no flat (pressure angle too large for the chosen fillet),
  a bore too large for the root circle. All are `invalid_args` with a hint.
* **Parameter edits are checked the same way** (review v0.2 M1, 2026-10-13). The first
  version relied on the derived parameters failing (`acos()` of a value above 1 in
  `gear1_flank_roll`); that never happens: `R(D)^2 = (r - D)^2 + D^2 / tan^2 a` is smallest
  at `D = r sin^2 a`, where `R = r cos a = r_b`, so the `acos` argument never exceeds 1 and an
  undercut gear solved to a wrong profile without a word. Now the profile sketch carries the
  key `gear: <prefix>` (`gear1`), and the evaluator runs the generator's checks
  (`gusset_eval::gear`, the same code) before solving it, on every rebuild: an impossible edit
  fails the sketch with error code `invalid_gear` and the generator's message and hint. The
  checked parameters are `<prefix>_module`, `_teeth`, `_pressure_angle`, `_shift`, `_backlash`,
  `_fillet`, `_bore` and `_transverse_module`; the helix angle is taken from `cos(beta) = m /
  m_t`, i.e. as the sketch sees it (a spur gear has no `_helix` parameter and its sketch uses
  `m_t = m`). They count as parameters of the sketch, so the incremental evaluator re-checks
  when one changes and they cannot be deleted while the key is there. The key is explicit
  text, not a naming convention: removing it turns the check off, renaming the parameters
  without it is an "unknown parameter" error. Alternatives rejected: a guard parameter whose
  expression fails (`sqrt()` of a negative margin) gives "sqrt() of a negative value" instead
  of the hint, and the hint's numbers (smallest tooth count, smallest shift) come from a
  search no expression can express; recognising gears by parameter names would be hidden
  semantics.

### 4. The sketch is driven by the parameters
Fit points cannot be solver variables with expressions, but their position can be constrained in
polar coordinates: each point of the lower flank of a tooth is a point `P` with a construction
line from the gear centre, a `distance(centre, P, R(...))` constraint and an `angle(axis, ray,
theta(...))` constraint, where `R` and `theta` are expressions of the gear's parameters (the
formulas above, generated as text). The upper flank is the mirror image (`symmetric` about the
axis), the tip and root are arcs about the centre (whose radius equation completes the last point
of each, so no constraint is redundant), a circular **sketch pattern** (ADR 0016) with `count:
gear1_teeth` repeats one tooth, and the bore is a circle with `diameter(c, gear1_bore)`.
Result: the sketch is well constrained (0 DOF, no redundant constraint, tested), editing
`gear1_module`, `gear1_teeth`, `gear1_shift`, ... in the parameter table reshapes the teeth, and
the region to extrude is named by the curve with the smallest id on its outline.

Edits in either direction solve from the coordinates stored at generation time, however far
they go (tested: 30 → 28 and 100 → 20 teeth, 20 → 100 teeth, module 2 → 1 mm and 2 → 5 mm,
as parameter edits and as text): the solver holds the fixed centre while it iterates and falls
back to dimension continuation (ADR 0016 §5, review v0.2 H1). Parameter edits do not write the
solved coordinates back into the sketch.

The flanks are Catmull-Rom splines (ADR 0016 §3), two per flank, one for the fillet and one for
the involute: their curvature jumps where they meet, which a single spline would smooth over
with an error of 0.1 mm. Fit points are placed by refinement: start from an even spacing and
bisect the intervals where the spline strays from the exact curve, until every point of the
exact curve is within 0.5 um of the spline (and the spline within 0.5 um of the curve; the
requirement was 1 um). Typical counts per flank: 45 points for m = 2 mm, 20 teeth. The accuracy is
relative to the generation parameters; large edits keep the shape but not the 0.5 um figure.

### 5. Spur and helical
* `helix` = 0: the solid is an `extrude` of the toothed region by `gear1_face_width`.
* `helix` != 0: the generator adds `gear1_helix`, a path sketch (a vertical line of
  `gear1_face_width` on XZ, driven by that parameter) and a `sweep` (ADR 0017) with
  `twist: gear1_face_width * tan(gear1_helix) / (gear1_pitch_diameter / 2) * 1 rad`: a right-hand
  helix for positive angles, left-hand for negative. The helix angle is on the pitch cylinder.
  A spur gear has no helix parameter; to make one helical, generate it again with a helix angle.
The tooth profile is the same sketch in both cases.

### 6. No `gear` statement in the text language
The task asked for a text form "if it fits the grammar cleanly". It does not: the text language
is declarative and `parse(print(doc)) == doc` holds for every document (ADR 0004), so a
statement that expands into other statements would vanish at the first print, and a
non-expanding `gear` feature would be exactly the black box DESIGN.md forbids (a feature whose
geometry is not in sketches, constraints and expressions). The text form of a gear is therefore
what the generator writes: grammar.md shows a complete example. An agent or user writing text by
hand can run the command first, read the result with `document_get_text` and edit it; the
command's JSON arguments are the compact "form" (`generate_gear`). If hand-written macros are
ever wanted, they belong in a separate input layer that expands before parsing, with an ADR.

### 7. Verification (OCCT golden tests, `gusset-cli/tests/modeling_gear.rs`)
Volume of the solid equals (outline area from the formulas - bore) x width to 2e-5; one tip face
and one root face per tooth (20 each for 20 teeth), bounding box 44 x 44 x 10; tooth thickness
on the pitch circle measured with a 0.1 mm ring equals `pi m / 2` (to 2e-3), `+ 2 x m tan(a)` with
shift and `- b` with backlash; a 20/31-tooth pair at centre distance `m (z1 + z2) / 2` has no
interference (boolean intersection volume below 0.5 mm3 where one gear has about 1300 mm3), the
wrong phase or a distance 0.5 mm short collide, backlash opens a gap; a helical pair of opposite
hand meshes, equal hands collide; the helical solid has the straight gear's volume (Cavalieri)
and its tooth tip at the end cap is turned by the twist angle.

## Not included (later)
Internal gears, racks, bevel and worm gears, tip relief and crowning, standard-centre-distance
modification with shifts (the user adds the shifts so that they cancel, or moves the second
gear), the "live preview" in the form (the command is fast and undoable), and a form for editing
a generated gear as a whole: the parameter table does that.
