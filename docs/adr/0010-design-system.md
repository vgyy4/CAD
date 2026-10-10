# ADR 0010: Design system

- Status: accepted
- Date: 2026-10-10

## Decision
- `docs/DESIGN.md` is the design system: interaction rules, layout, tokens (spacing,
  radii, typography, colors for dark and light, elevation, motion), icon rules and
  viewport look.
- The tokens are implemented once in code (`gusset-ui::theme`). Every widget reads
  them; ad-hoc colors and sizes are rejected in review, and a unit test checks that the
  code tokens match the tables in DESIGN.md.
- The dark theme is the default; light is first-class. Both meet WCAG AA contrast, and
  a test computes contrast ratios from the tokens.
- Fonts are Inter and JetBrains Mono, both OFL-1.1 and bundled, which keeps text
  rendering the same on every platform. Their licenses are listed in
  THIRD_PARTY_LICENSES.
- The icon set is original SVG, drawn in-repo, and rasterized at device scale.
- Motion is defined by tokens; animations request repaints only while running, and
  are off under "reduce motion".

## Consequences
Visual changes go through DESIGN.md first. Screenshot tests catch unintended changes.
