# Open CASCADE Technology in Gusset

Gusset makes use of facilities provided by the Open CASCADE Technology software.

## What is shipped

| | |
|---|---|
| Component | Open CASCADE Technology (OCCT), the geometry kernel |
| Version | `@OCCT_TAG@` |
| Source commit | `@OCCT_COMMIT@` |
| Upstream repository | `@OCCT_REPO@` |
| License | GNU LGPL 2.1 (`LICENSE_LGPL_21.txt`) with the Open CASCADE exception 1.0 (`OCCT_LGPL_EXCEPTION.txt`) |
| Modifications | None. Gusset ships the upstream source built as-is, without patches. |
| Build recipe | `build-occt.sh` and `occt.env` (shipped next to this file; also `deps/occt/` in the Gusset source) |

OCCT is not compiled into the Gusset programs. It is built as separate shared libraries
and linked dynamically:

- **Windows:** `TK*.dll` in the installation directory, next to `gusset.exe`. Gusset
  loads them only from that directory (ADR 0008).
- **Linux:** `lib/gusset/libTK*.so*` in the installation (or AppImage). The programs find
  them through the install-relative search path `$ORIGIN/../lib/gusset`.
- **macOS:** `Gusset.app/Contents/Frameworks/libTK*.dylib`, found through
  `@executable_path/../Frameworks`.

Gusset's own C++ glue to OCCT (`crates/gusset-occt-sys/cpp/`) is part of Gusset and is
licensed `MIT OR Apache-2.0`. Its source is in the Gusset repository.

## Getting the OCCT source

Every Gusset release carries the corresponding OCCT source as a release asset
(`occt-src-@OCCT_TAG@.tar.gz`). You can also get it from upstream:

```sh
git clone --branch @OCCT_TAG@ @OCCT_REPO@ occt
git -C occt rev-parse HEAD   # must print @OCCT_COMMIT@
```

## Rebuilding and relinking (replacing OCCT)

The LGPL lets you use Gusset with a modified OCCT. To do that:

1. Get the source as above and make your changes. Keep the ABI compatible with the
   shipped version (the same major.minor release, built as shared libraries with the
   same compiler family).
2. Build it with the same options Gusset uses:
   `./build-occt.sh <install-prefix>` (edit `occt.env` if you use a different
   checkout, or run the `cmake` commands from the script by hand).
3. Replace the shipped libraries with yours:
   - Windows: copy `<install-prefix>/bin/TK*.dll` over the DLLs in the Gusset
     installation directory.
   - Linux: copy `<install-prefix>/lib/libTK*.so*` into `lib/gusset/` of the
     installation. For the AppImage, extract it first (`./Gusset-*.AppImage
     --appimage-extract`), replace the files in `squashfs-root/usr/lib/gusset/`, and run
     `squashfs-root/AppRun`.
   - macOS: copy `<install-prefix>/lib/libTK*.dylib` into
     `Gusset.app/Contents/Frameworks/` (then re-sign ad hoc with
     `codesign --force --deep --sign - Gusset.app`).
4. Run `gusset self-test` to check that the kernel loads and works.

Because OCCT is used through a shared-library mechanism (LGPL 2.1 section 6(b)),
replacing the libraries is all that is needed; the Gusset programs do not have to be
relinked. To build Gusset itself against a different OCCT, build it from source with
`GUSSET_OCCT_DIR=<install-prefix> cargo build --release`.

## Contact

Questions about OCCT licensing in Gusset: open an issue in the Gusset repository.
