# ADR 0013: Release workflow and installer formats

- Status: accepted
- Date: 2026-10-10
- Research: `docs/research/mcp-simd-storage-crates.md` §4–5

## Decision
Release artifacts are built **only** by GitHub Actions (`.github/workflows/release.yml`),
triggered by `v*` tags, with a nightly pre-release job from main. Each target builds
natively on its own runner:

| Target | Runner | Artifacts |
|---|---|---|
| Windows x64 | `windows-2025` | per-user **MSI** (WiX v5, MS-RL, pinned and checksum-verified) + portable `.zip` |
| Windows ARM64 | `windows-11-arm` | per-user MSI + portable `.zip` |
| Linux x64 | `ubuntu-24.04` | AppImage, `.deb` (`cargo-deb`), `.rpm` (`cargo-generate-rpm`), Flatpak manifest (`packaging/flatpak/`) |
| Linux ARM64 | `ubuntu-24.04-arm` | AppImage, `.deb`, `.rpm` |
| macOS ARM64 | `macos-15` | `.dmg` (`hdiutil`) |

- **MSI rather than MSIX.** Unsigned MSIX packages cannot be installed without the user
  importing a certificate, and the MSIX update model conflicts with our own verified
  updater. Per-user MSI installs to `%LOCALAPPDATA%\Programs\Gusset` without elevation,
  and the in-app updater can replace files there. WiX **v5** is used because it is
  MS-RL-licensed. v6+ carries the OSMF EULA, which would be free for us today but
  adds terms. Revisit if v5 stops building.
- **Not cargo-dist or cargo-packager.** cargo-dist's packaging (archives, MSI,
  shell/PowerShell installers) does not cover deb/rpm/AppImage/dmg/Flatpak; it is
  community-maintained since 2025; and our OCCT shared-library bundling and
  updater manifests need custom steps anyway. cargo-packager has been quiet since
  2025-11. Instead, small focused tools (cargo-deb, cargo-generate-rpm, WiX,
  appimagetool, hdiutil) are each pinned by version and checksum, with the
  workflow kept in-repo. Easy to reverse: cargo-dist could later produce the
  archives.
- Every artifact bundles the OCCT shared libraries, `NOTICE-OCCT.md`, the LGPL text, the
  OCCT source URL with its commit and our build recipe, and `THIRD_PARTY_LICENSES.md`.
- **Integrity:** `SHA256SUMS`, a CycloneDX SBOM (`cargo-cyclonedx`), and GitHub artifact
  attestations (`actions/attest-build-provenance`, pinned by SHA) for every artifact.
- **Installer smoke tests on every target before publishing:** silent install, `gusset
  --version`, open a bundled sample, `gusset build sample.gus --export out.step`,
  uninstall, then assert that no files, shortcuts, registry keys, services, tasks or
  autostart entries are left behind (user projects and settings excepted). Machine-wide
  installs are also checked for user-writable files (ACL/permission check). A release
  is published only if every smoke test passes.
- **Updater payloads:** the workflow also writes the signed update manifest and
  per-target update packages (ADR 0014).
- **CI budget:** the repository is private, so Actions minutes count against the
  owner's plan, with Windows ×2 and macOS ×10 multipliers. Releases are tag-only; the
  OCCT build is cached per target; macOS runs only on release and nightly jobs; and
  `Swatinem/rust-cache` and sccache are used. The download page/README detects the
  OS and architecture with a small static JS snippet on the GitHub Pages site (later).
  The README lists the files per platform.

## Amendment (2026-10-12): manual releases by tag name
The session environment cannot push tags, and the owner could not edit tags by hand,
so the Release workflow also runs on `workflow_dispatch` with a `tag` input. Only
users with write access can start it. The tag must match `v<semver>` and the
workspace version, exactly as for tag pushes. The build uses the dispatched commit
(the default branch head), and the publish job (protected `release` environment)
checks that the commit is on the default branch, points the tag at it through the
GitHub API, and uploads into an existing release for the tag or creates a draft.
