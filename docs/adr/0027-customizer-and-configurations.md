# ADR 0027: Customizer ranges and configurations

Status: accepted (ranges and sliders done 2026-10-14; configurations planned)

## Context
DESIGN.md §1.8: parameters are first-class, and "parameters can be exposed as customizer
sliders" (OpenSCAD's Customizer). RESEARCH.md lists configurations (Onshape, SolidWorks:
named variants of one part) for v1. Components (ADR 0026 §7) already take parameter values
per instance (`set:`).

## Decision
### 1. Ranges
A parameter may carry `min`, `max` and `step`, written after its value:

`param width = 40 mm, min: 10 mm, max: 100 mm, step: 5 mm`

Each is an expression in the parameter's unit (a plain number takes it) and may refer to
other parameters (`max: width / 2`); they are part of the parameter's dependencies. A value
outside `[min, max]` fails the parameter (and what uses it) with the range in the error,
so agents and the GUI get the same contract. `param.set` takes `min`, `max`, `step` (""
removes one); `query.model` reports `range`.

### 2. Sliders
The browser shows a slider under each parameter whose `min` and `max` evaluate: lengths in
the document's unit, angles in degrees, `step` as the slider step. The model changes when
the slider is released (one ordinary, undoable `param.set`), not on every drag frame.

### 3. Configurations (planned)
Named parameter presets in the document, `configuration Small (set: [width = 20 mm])`.
Applying one is an ordinary `param.set` batch (undoable); `query.configurations` names the
one the current values match; components may name a configuration instead of `set:`.

## Consequences
- The parameter text gains optional trailing options; older files parse unchanged.
- Out-of-range values are errors, not clamps: nothing changes silently.
