# Performance budgets

Budgets are enforced, not aspirational. Baselines are measured as features land; until
then the spec's starting targets apply. **Reference machine:** a 4-core laptop with an
Intel HD 620-class iGPU and 8 GB RAM (spec). CI measures what it can on GitHub runners,
under the software renderer for frame times.

| Metric | Budget (starting target) | Measured | How measured |
|---|---|---|---|
| Cold start to interactive window | < 1.5 s | not yet measured | `gusset --bench-startup` |
| Idle RAM, empty document | < 250 MB (incl. GPU memory on iGPU) | not yet measured | |
| Idle CPU | ~0 % (no continuous redraw) | not yet measured | automated idle test (no frames requested over 5 s) |
| Orbit frame time, typical part | 60 fps (16.7 ms) | not yet measured | scripted scenario |
| Large assembly | ≥ 30 fps | not yet measured | |
| Software renderer | ≥ 15 fps | not yet measured | CI (llvmpipe) |
| Rebuild, 100-feature part | < 1 s for a one-feature parameter change (incremental); < 5 s full | not yet measured | criterion `rebuild_100` |
| Binary size (app + OCCT libs, compressed) | < 60 MB | not yet measured | release workflow |
| History overhead | ≤ 15 % of the model's parametric size per 100 single-parameter edits | not yet measured | criterion `history_overhead` |
| Project save (parametric only) | < 50 ms for 1,000 features | not yet measured | criterion `save_1000` |
| Recovery snapshot | < 100 ms for 1,000 features; never blocks the UI | not yet measured | |

## Rules
- A CI benchmark regression of more than 10 % fails the build unless the commit
  message justifies it.
- Profile before optimizing (cargo flamegraph, tracy, heaptrack). Findings go below.

## Profiling log
(none yet)
