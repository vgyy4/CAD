# ADR 0027: Customizer ranges and configurations

Status: accepted, done 2026-10-14

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

### 3. Configurations
Named parameter presets in the document, `configuration Small (set: [width = 20 mm])`,
written after the parameters and stored with them in the history's parameter object (no
change to the snapshot format). `configuration.save` stores the current values of chosen
parameters (default: all), `configuration.apply` sets them in one undoable edit,
`configuration.delete` removes one, `query.configurations` lists them with `current`
(the parameters hold exactly those expressions). Renames follow into configurations; a
parameter a configuration sets cannot be deleted. Merges treat them by name like
parameters. A component may pick one of its document's configurations
(`configuration: Long`, `assembly.configure_component`), applied before its `set:`. The
browser lists configurations under the parameters with an Apply button.

## Consequences
- The parameter text gains optional trailing options; older files parse unchanged.
- Out-of-range values are errors, not clamps: nothing changes silently.
