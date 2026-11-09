# Code signing and update signing

Gusset's releases carry two kinds of signature (ADR 0014, ADR 0015):

| What | Proves | Who issues it | Status |
|---|---|---|---|
| **Update signature** (minisign / Ed25519) on every release package and on `update-manifest.json` | A package or manifest comes from the maintainer; the in-app updater installs nothing else | The maintainer creates the key pair (free) | waiting for the key |
| **Windows code signature** (Authenticode) | Windows shows the publisher instead of "unknown publisher" | SignPath Foundation (free for open-source projects) | waiting for approval |
| **macOS Developer ID + notarization** | macOS opens the app without Gatekeeper warnings | Apple (Apple Developer Program, US$99/year) | optional, not planned yet |

Until a signature exists, releases are still built and published; the in-app updater refuses
unsigned manifests (it never installs anything unverified), and installers show the operating
system's usual warnings for unsigned software (README, "Install", says how to get past them).

## Code signing policy

This policy is published for SignPath Foundation's open-source programme and applies to every
signed release of Gusset.

- **What is signed:** only release artifacts built by the `release` GitHub Actions workflow of
  this repository from a tagged commit on the public source tree. Nothing built on a developer's
  machine is signed. The workflow, the build inputs (pinned dependencies in `Cargo.lock`, the
  pinned OCCT source and its patches in `deps/occt/`) and the build logs are public.
- **Third-party components:** the packages include open-source libraries listed in
  `docs/ATTRIBUTION-THIRD-PARTY.md` (each under its own licence) and OpenCASCADE (LGPL-2.1 with the
  OCCT exception, shipped as separate shared libraries with its source). No proprietary
  components are included.
- **Roles:**
  - *Authors:* anyone may propose changes through pull requests.
  - *Reviewers:* every change merged to the release branch is reviewed by the maintainer; CI
    (build, tests, lint, fuzzing, dependency and licence checks) must pass.
  - *Approvers:* the maintainer (the repository owner) approves each signing request. Signing
    requests are made only by the release workflow, and the signing secrets live in the
    protected `release` environment, which needs the maintainer's approval to run.
- **Privacy:** Gusset sends no telemetry. The only network requests it makes on its own are
  update checks (to GitHub Releases, without identifiers; they can be turned off), and the MCP
  server listens on `127.0.0.1` only, with a token. See the README and `docs/THREAT_MODEL.md`.
- **Attribution (required by the programme):** Free code signing on Windows provided by
  [SignPath.io](https://signpath.io), certificate by
  [SignPath Foundation](https://signpath.org).
- **Revocation:** if a signing key or an approver account is compromised, the maintainer asks
  SignPath to revoke the certificate and publishes a notice in the repository's Security
  advisories.

## Maintainer instructions

### 1. Update signing key (free, about two minutes)
Do this on your own computer; the secret key must never enter the repository, a chat or a
cloud session.

1. Install minisign: `brew install minisign` (macOS), `sudo apt install minisign` (Debian/
   Ubuntu) or `scoop install minisign` (Windows).
2. Create the key pair without a password (the release workflow signs unattended):
   ```sh
   minisign -G -W -p gusset-update.pub -s gusset-update.key
   ```
3. On GitHub: **Settings → Environments → New environment** named `release` (if it is not there),
   enable **Required reviewers** with yourself, then **Add environment secret**:
   name `UPDATE_SIGNING_KEY`, value: the full contents of `gusset-update.key`.
4. Give the contents of `gusset-update.pub` (it is public) to the developer, or commit it as
   `crates/gusset-update/keys/gusset-2026.pub` and add its key line to `BUILTIN_KEYS` in
   `crates/gusset-update/src/lib.rs` (see `crates/gusset-update/keys/README.md`).
5. Keep an offline backup of `gusset-update.key` (a password manager or an encrypted USB
   stick). Losing it means shipping a new public key and asking users to update by hand once.

From the next tagged release on, packages and the update manifest are signed, and the app
accepts the updates.

### 2. Windows code signing through SignPath Foundation (free)
1. Make sure the repository is public (it is) and this document is linked from the README
   (it is, under "Code signing policy").
2. Apply at <https://signpath.org/apply> with the GitHub repository URL. They check the open-
   source licence, the public build and this policy; approval takes days to weeks.
3. After approval SignPath gives you an **organization ID**, a **project slug** and an **API
   token** and asks you to install their GitHub App on the repository.
4. On GitHub, in the `release` environment: add the secret `SIGNPATH_API_TOKEN`, and the
   variables `SIGNPATH_ORGANIZATION_ID` and `SIGNPATH_PROJECT_SLUG`. Then set the repository
   variable `SIGNING_ENABLED` to `true`.
5. Tell the developer, who replaces the disabled placeholder step in
   `.github/workflows/release.yml` ("Sign Windows packages") with SignPath's official action and
   tests it on a pre-release tag.

### 3. macOS (optional, US$99/year)
Only if Mac users ask for it: join the Apple Developer Program as an individual, create a
"Developer ID Application" certificate, export it as a `.p12`, and add to the `release`
environment the secrets `APPLE_CERTIFICATE_P12` (base64), `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_ID`, `APPLE_TEAM_ID` and `APPLE_APP_PASSWORD` (an app-specific password). The developer
then wires `codesign` and `notarytool` into the macOS step of the release workflow.
