# ADR 0015: Code signing — ship unsigned for now; options documented

- Status: accepted (unsigned), revisit on owner decision
- Date: 2026-10-10

## Decision
Releases ship **unsigned** until the owner decides otherwise. Workflow steps for signing
exist but are disabled (`if: ${{ vars.SIGNING_ENABLED == 'true' }}`) and need secrets
that are not configured. Nothing is signed up for or paid for without asking the owner.

## Options (checked 2026-10-10; verify again before acting)
| Option | Cost | Eligibility / notes |
|---|---|---|
| **SignPath Foundation** (free OSS program) | Free | OSI license, no proprietary components, actively maintained, documented features, a code-signing policy published on the project home page, a fully automated build with signing integrated (GitHub Actions). The certificate is issued to SignPath Foundation, which appears as publisher. **The source must be public:** this repository is currently private, so we are not eligible yet. ([signpath.org/terms](https://signpath.org/terms), [SignPath OSS](https://about.signpath.io/product/open-source)) |
| **Azure Artifact Signing** (formerly Trusted Signing) | about US$9.99/month (Basic) per Microsoft Learn; billing starts when the account is created | Organizations in US/CA/EU/UK; individuals only in US/CA. Identity validation takes days. ([Microsoft Learn: code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options), [pricing](https://azure.microsoft.com/pricing/details/artifact-signing/)) |
| **Apple Developer ID + notarization** | US$99/year (Apple Developer Program) | Needed to avoid Gatekeeper warnings on macOS. The hardened runtime is enabled regardless |
| Classic OV/EV certificates | roughly US$200–600/year plus a hardware token or cloud HSM | Not preferred |

Recommendation for the owner, when ready: make the repository public and apply to
SignPath Foundation for Windows (free); add Apple Developer ID only if macOS demand
justifies it.

## What users see while unsigned
- **Windows:** SmartScreen shows "Windows protected your PC". Click **More info → Run
  anyway**. MSI per-user installs do not need admin. Our own Ed25519 update signatures
  still protect updates.
- **macOS:** "Gusset can't be opened because Apple cannot check it for malicious
  software". Right-click the app → **Open** → **Open**, or System Settings → Privacy &
  Security → **Open Anyway**.
- **Linux:** no OS-level signing prompts. SHA-256 sums and GitHub attestations can be
  verified with `gh attestation verify`.
These steps are documented in the README "Install" section.
