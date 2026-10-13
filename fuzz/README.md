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
| `worker_request`, `worker_response` | the import worker's protocol codec, including B-rep validation requests |

OCCT's `BinTools::Read` (B-rep blobs in project files, security review #3) is not fuzzed
here: these targets build without OCCT, so the worker's OCCT code is a stub in them, and
CI has no OCCT build for fuzzing. In the app that parser only sees untrusted bytes inside
the sandboxed worker (ADR 0008 §4). Fuzzing it needs an OCCT-linked libFuzzer build of
`gusset-occt-sys::safe::brep_read` with ASan; that is tracked as open in
docs/SECURITY_REVIEWS.md.

Run locally:

```sh
cd fuzz
cargo +nightly run --release --example make_seeds -- seeds   # seed corpus
cargo +nightly fuzz run doc_text corpus/doc_text seeds/doc_text
```

The `doc_text` seeds are the example documents of `crates/gusset-doc/src/grammar.md` plus the
files in `seeds-src/doc_text/` (v0.2 statements and face keys, and a generated helical gear);
`gusset-doc/tests/fuzz_seeds.rs` checks that they parse, print to a fixed point and use every
v0.2 statement. Add a file there when the grammar grows. `corpus/` is the fuzzer's own working
corpus (kept by CI between runs, not in git).

CI (`.github/workflows/fuzz-miri.yml`) runs every target for 60 s on each push and
pull request, and for 20 min nightly, which also keeps a minimized corpus. Crashing
inputs are uploaded as workflow artifacts. Add a regression unit test in the owning
crate for each bug found.
