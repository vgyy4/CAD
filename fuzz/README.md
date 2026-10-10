# Fuzz targets

cargo-fuzz targets for every Gusset parser of untrusted input (ADR 0008 §11). This
crate is excluded from the workspace because libFuzzer builds need a nightly toolchain.

| target | what it checks |
|---|---|
| `units_expr` | unit expressions: no panic; printing round-trips to the same tree |
| `doc_text` | code-view text: no panic; printed text is a fixed point |
| `container_open` | `.gus` container reader |
| `container_salvage` | damaged-container and recovery-slot salvage |
| `project_decode` | `.gus` → history decoding |
| `mesh_stl`, `mesh_obj`, `mesh_3mf` | mesh importers |
| `mesh_any` | format sniffing, then import, re-export in every format, re-import |
| `worker_request`, `worker_response` | the import worker's protocol codec |

Run locally:

```sh
cd fuzz
cargo +nightly run --release --example make_seeds -- seeds   # seed corpus
cargo +nightly fuzz run doc_text corpus/doc_text seeds/doc_text
```

CI (`.github/workflows/fuzz-miri.yml`) runs every target for 60 s on each push and
pull request, and for 20 min nightly, which also keeps a minimized corpus. Crashing
inputs are uploaded as workflow artifacts. Add a regression unit test in the owning
crate for each bug found.
