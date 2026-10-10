//! Signed update manifests for the in-app updater (ADR 0014).
//!
//! This crate only *decides*; it never touches the network, the file system or a
//! clock, and it never downloads, installs or runs anything. The caller fetches
//! `update-manifest.json` and its minisign signature, passes the bytes here, and gets
//! back either an error or a report of what is available. Downloading and applying an
//! update happen elsewhere and only after the user chooses "Install".
//!
//! Verification order ([`verify_manifest`], then [`check`]):
//! 1. The manifest is small, and its minisign signature (Ed25519 over the BLAKE2b-512
//!    prehash) is valid for one of the built-in public keys that has not been revoked.
//! 2. The manifest parses, has a supported schema, and its own revocation list does not
//!    revoke the key that signed it. Revocations are remembered in the [`KeyRing`]; a
//!    manifest can only remove keys, never add them.
//! 3. [`check`]: the channel matches, `expires` is in the future (freeze protection),
//!    the validity window is short, the version is strictly newer than the installed one
//!    (rollback protection), and there is an entry for this OS, architecture and install
//!    type with a well-formed package description.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::Version;
use serde::Deserialize;

#[cfg(test)]
mod tests;

/// Largest manifest accepted, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// Largest signature file accepted, in bytes.
pub const MAX_SIGNATURE_BYTES: usize = 4096;
/// Most targets in one manifest.
pub const MAX_TARGETS: usize = 64;
/// Most files listed for one package.
pub const MAX_FILES: usize = 100_000;
/// Longest accepted span between `released` and `expires` (120 days). The release
/// workflow writes 90 days, so a stolen old manifest stops working soon.
pub const MAX_VALIDITY_SECS: u64 = 120 * 24 * 3600;
/// The manifest schema this build understands.
pub const SCHEMA: u32 = 1;

/// Public keys built into this build of Gusset, as `(name, base64 minisign public key)`.
///
/// Empty until the maintainer generates the release keypair (ADR 0014, "Key
/// management"). With no keys, every manifest is refused: no update is ever installed
/// unverified. To add one, put the minisign `.pub` file in `keys/` and its second line
/// here.
pub const BUILTIN_KEYS: &[(&str, &str)] = &[];

// ------------------------------------------------------------------------- errors

/// Why a manifest was refused or a check could not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// No trusted (built-in, non-revoked) keys are available.
    NoTrustedKeys,
    TooLarge {
        what: &'static str,
        len: usize,
        max: usize,
    },
    /// The signature file is malformed or uses an unsupported algorithm.
    BadSignatureFormat(String),
    /// The signature is not valid for any trusted key.
    BadSignature,
    /// The key that signed the manifest has been revoked.
    RevokedKey(KeyId),
    /// The signed manifest is malformed.
    BadManifest(String),
    UnsupportedSchema(u32),
    WrongChannel {
        expected: Channel,
        found: Channel,
    },
    Expired {
        expires: String,
    },
    ValidityTooLong,
    /// The manifest has no package for this OS, architecture and install type.
    NoTarget(String),
    /// A public key given to [`KeyRing::new`] is malformed.
    BadPublicKey(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::NoTrustedKeys => f.write_str("this build has no trusted update keys; updates are disabled"),
            UpdateError::TooLarge { what, len, max } => {
                write!(f, "the {what} is too large ({len} bytes, at most {max})")
            }
            UpdateError::BadSignatureFormat(e) => write!(f, "the update signature is malformed: {e}"),
            UpdateError::BadSignature => f.write_str("the update manifest signature is not valid"),
            UpdateError::RevokedKey(k) => write!(f, "the update manifest is signed with revoked key {k}"),
            UpdateError::BadManifest(e) => write!(f, "the update manifest is malformed: {e}"),
            UpdateError::UnsupportedSchema(v) => {
                write!(f, "the update manifest uses schema {v}, which this version does not understand")
            }
            UpdateError::WrongChannel { expected, found } => {
                write!(f, "the update manifest is for the {found} channel, not {expected}")
            }
            UpdateError::Expired { expires } => write!(f, "the update manifest expired at {expires}"),
            UpdateError::ValidityTooLong => f.write_str("the update manifest is valid for longer than allowed"),
            UpdateError::NoTarget(t) => write!(f, "no update package for {t}"),
            UpdateError::BadPublicKey(e) => write!(f, "invalid update public key: {e}"),
        }
    }
}

impl std::error::Error for UpdateError {}

// --------------------------------------------------------------------------- keys

/// A minisign key ID, shown like minisign does: 16 upper-case hex digits of the
/// little-endian key number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub [u8; 8]);

impl KeyId {
    /// Parses the minisign display form (case-insensitive).
    pub fn parse(s: &str) -> Option<KeyId> {
        if s.len() != 16 {
            return None;
        }
        let n = u64::from_str_radix(s, 16).ok()?;
        Some(KeyId(n.to_le_bytes()))
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016X}", u64::from_le_bytes(self.0))
    }
}

#[derive(Debug, Clone)]
struct TrustedKey {
    name: String,
    id: KeyId,
    key: minisign_verify::PublicKey,
}

/// The trusted update keys plus every key revoked so far.
///
/// The app persists [`KeyRing::revoked`] (in its settings directory) and passes it back
/// to [`KeyRing::with_revoked`] on the next start, so a revocation sticks even if an
/// attacker later serves an older manifest.
#[derive(Debug, Clone)]
pub struct KeyRing {
    keys: Vec<TrustedKey>,
    revoked: BTreeSet<KeyId>,
}

impl KeyRing {
    /// The keys compiled into this build ([`BUILTIN_KEYS`]).
    ///
    /// # Errors
    /// [`UpdateError::BadPublicKey`] if a built-in key is malformed (a build defect).
    pub fn builtin() -> Result<KeyRing, UpdateError> {
        KeyRing::new(BUILTIN_KEYS)
    }

    /// A key ring from `(name, base64 minisign public key)` pairs.
    ///
    /// # Errors
    /// [`UpdateError::BadPublicKey`] if a key does not decode.
    pub fn new(keys: &[(&str, &str)]) -> Result<KeyRing, UpdateError> {
        let mut out = Vec::with_capacity(keys.len());
        for (name, b64) in keys {
            let b64 = b64.trim();
            let key = minisign_verify::PublicKey::from_base64(b64)
                .map_err(|e| UpdateError::BadPublicKey(format!("{name}: {e}")))?;
            let raw = base64_decode(b64).ok_or_else(|| UpdateError::BadPublicKey(format!("{name}: not base64")))?;
            let id: [u8; 8] = raw
                .get(2..10)
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| UpdateError::BadPublicKey(format!("{name}: too short")))?;
            out.push(TrustedKey { name: (*name).to_string(), id: KeyId(id), key });
        }
        Ok(KeyRing { keys: out, revoked: BTreeSet::new() })
    }

    /// Adds previously persisted revocations.
    #[must_use]
    pub fn with_revoked(mut self, revoked: impl IntoIterator<Item = KeyId>) -> KeyRing {
        self.revoked.extend(revoked);
        self
    }

    /// Every key revoked so far (to persist).
    pub fn revoked(&self) -> &BTreeSet<KeyId> {
        &self.revoked
    }

    /// IDs and names of the keys that are currently trusted.
    pub fn trusted(&self) -> Vec<(KeyId, &str)> {
        self.keys.iter().filter(|k| !self.revoked.contains(&k.id)).map(|k| (k.id, k.name.as_str())).collect()
    }
}

// ----------------------------------------------------------------------- manifest

/// Release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Nightly,
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Channel::Stable => "stable",
            Channel::Nightly => "nightly",
        })
    }
}

/// One downloadable package (full or delta).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    /// HTTPS URL of the package.
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the package.
    pub sha256: String,
    /// The package's own minisign signature (the whole `.minisig` text).
    pub sig: String,
}

/// One file of an installed version, checked after extraction and before switching.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileEntry {
    /// Relative path with `/` separators.
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// The update for one `<os>-<arch>-<install_type>`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TargetEntry {
    #[serde(flatten)]
    pub package: Package,
    #[serde(default)]
    pub files: Vec<FileEntry>,
    /// Delta packages keyed by the version they apply to.
    #[serde(default)]
    pub delta_from: BTreeMap<String, Package>,
}

/// The parsed `update-manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub channel: Channel,
    pub version: String,
    /// RFC 3339 UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub released: String,
    /// RFC 3339 UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub expires: String,
    /// Installed versions older than this cannot update in place and must reinstall.
    pub min_supported: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub revoked_keys: Vec<String>,
    pub targets: BTreeMap<String, TargetEntry>,
}

/// A manifest whose signature has been verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    pub manifest: Manifest,
    pub signed_by: KeyId,
}

/// Verifies `manifest` (the exact bytes downloaded) against `signature` (the minisign
/// `.minisig` text) with the keys in `ring`, then parses it.
///
/// Revocations listed in a valid manifest are added to `ring`; persist
/// [`KeyRing::revoked`] afterwards.
///
/// # Errors
/// See [`UpdateError`]; nothing is trusted unless this returns `Ok`.
pub fn verify_manifest(manifest: &[u8], signature: &str, ring: &mut KeyRing) -> Result<VerifiedManifest, UpdateError> {
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::TooLarge { what: "manifest", len: manifest.len(), max: MAX_MANIFEST_BYTES });
    }
    if signature.len() > MAX_SIGNATURE_BYTES {
        return Err(UpdateError::TooLarge { what: "signature", len: signature.len(), max: MAX_SIGNATURE_BYTES });
    }
    let sig =
        minisign_verify::Signature::decode(signature).map_err(|e| UpdateError::BadSignatureFormat(e.to_string()))?;
    let trusted: Vec<&TrustedKey> = ring.keys.iter().filter(|k| !ring.revoked.contains(&k.id)).collect();
    if trusted.is_empty() {
        return Err(UpdateError::NoTrustedKeys);
    }
    // Legacy (non-prehashed) signatures are refused: `allow_legacy = false`.
    let signer = trusted.iter().find(|k| k.key.verify(manifest, &sig, false).is_ok()).map(|k| k.id);
    let Some(signed_by) = signer else {
        // Distinguish "signed by a key we revoked" for a clearer report.
        if let Some(k) =
            ring.keys.iter().find(|k| ring.revoked.contains(&k.id) && k.key.verify(manifest, &sig, false).is_ok())
        {
            return Err(UpdateError::RevokedKey(k.id));
        }
        return Err(UpdateError::BadSignature);
    };
    let parsed: Manifest = serde_json::from_slice(manifest).map_err(|e| UpdateError::BadManifest(e.to_string()))?;
    if parsed.schema != SCHEMA {
        return Err(UpdateError::UnsupportedSchema(parsed.schema));
    }
    let mut revoked = BTreeSet::new();
    for k in &parsed.revoked_keys {
        revoked.insert(KeyId::parse(k).ok_or_else(|| UpdateError::BadManifest(format!("bad revoked key id {k:?}")))?);
    }
    if revoked.contains(&signed_by) {
        return Err(UpdateError::RevokedKey(signed_by));
    }
    // Only removes trust: unknown IDs are recorded too, but never become trusted.
    ring.revoked.extend(revoked);
    Ok(VerifiedManifest { manifest: parsed, signed_by })
}

// -------------------------------------------------------------------------- check

/// How this copy of Gusset was installed. Decides how an update is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallType {
    /// Per-user MSI (`%LOCALAPPDATA%\Programs\Gusset`).
    MsiUser,
    /// Machine-wide MSI (elevation only at apply time).
    MsiMachine,
    /// Portable zip.
    Zip,
    AppImage,
    Deb,
    Rpm,
    Flatpak,
    /// macOS `.app` from the `.dmg`.
    Dmg,
}

impl InstallType {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallType::MsiUser => "msi-user",
            InstallType::MsiMachine => "msi-machine",
            InstallType::Zip => "zip",
            InstallType::AppImage => "appimage",
            InstallType::Deb => "deb",
            InstallType::Rpm => "rpm",
            InstallType::Flatpak => "flatpak",
            InstallType::Dmg => "dmg",
        }
    }

    /// Package-managed installs: the system package manager owns updates, so the app
    /// only notifies (ADR 0014 §5).
    pub fn is_package_managed(self) -> bool {
        matches!(self, InstallType::Deb | InstallType::Rpm | InstallType::Flatpak)
    }
}

/// What is installed, and where updates may come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: Version,
    pub channel: Channel,
    /// `windows`, `linux` or `macos`.
    pub os: &'static str,
    /// `x86_64` or `aarch64`.
    pub arch: &'static str,
    pub install_type: InstallType,
    /// Package URLs must start with this (e.g.
    /// `https://github.com/<owner>/<repo>/releases/download/`).
    pub url_prefix: String,
}

impl Installed {
    /// The manifest target key: `<os>-<arch>-<install_type>`.
    pub fn target(&self) -> String {
        format!("{}-{}-{}", self.os, self.arch, self.install_type.as_str())
    }
}

/// How an available update can be applied (the user still has to choose to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    /// The updater can download, verify and install it after the user agrees.
    InApp,
    /// The system package manager owns this install; tell the user how to update.
    NotifyOnly,
    /// The installed version is older than the manifest's `min_supported`.
    ReinstallRequired,
}

/// An update the user may choose to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: Version,
    pub released: String,
    pub notes: String,
    pub apply: ApplyMode,
    pub package: TargetEntry,
    /// A delta package for the installed version, if the manifest lists one.
    pub delta: Option<Package>,
}

/// The result of an update check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    UpToDate,
    Available(Box<AvailableUpdate>),
}

/// Checks a verified manifest against what is installed, at `now_unix` (seconds).
///
/// # Errors
/// [`UpdateError::WrongChannel`], [`UpdateError::Expired`],
/// [`UpdateError::ValidityTooLong`], [`UpdateError::NoTarget`] or
/// [`UpdateError::BadManifest`].
pub fn check(v: &VerifiedManifest, installed: &Installed, now_unix: u64) -> Result<UpdateCheck, UpdateError> {
    let m = &v.manifest;
    if m.channel != installed.channel {
        return Err(UpdateError::WrongChannel { expected: installed.channel, found: m.channel });
    }
    let released = parse_utc(&m.released).ok_or_else(|| bad(format!("bad released time {:?}", m.released)))?;
    let expires = parse_utc(&m.expires).ok_or_else(|| bad(format!("bad expires time {:?}", m.expires)))?;
    if expires <= released {
        return Err(bad("expires is not after released".into()));
    }
    if expires - released > MAX_VALIDITY_SECS {
        return Err(UpdateError::ValidityTooLong);
    }
    if now_unix >= expires {
        return Err(UpdateError::Expired { expires: m.expires.clone() });
    }
    let version = Version::parse(&m.version).map_err(|e| bad(format!("bad version {:?}: {e}", m.version)))?;
    let min_supported =
        Version::parse(&m.min_supported).map_err(|e| bad(format!("bad min_supported {:?}: {e}", m.min_supported)))?;
    if m.targets.len() > MAX_TARGETS {
        return Err(bad(format!("more than {MAX_TARGETS} targets")));
    }
    // Rollback protection: only strictly newer versions are offered.
    if version <= installed.version {
        return Ok(UpdateCheck::UpToDate);
    }
    let target = installed.target();
    let entry = m.targets.get(&target).ok_or_else(|| UpdateError::NoTarget(target.clone()))?;
    validate_package(&entry.package, &installed.url_prefix)?;
    if entry.files.len() > MAX_FILES {
        return Err(bad(format!("more than {MAX_FILES} files")));
    }
    let mut seen = BTreeSet::new();
    for f in &entry.files {
        if !is_safe_relative_path(&f.path) {
            return Err(bad(format!("unsafe file path {:?}", f.path)));
        }
        if !seen.insert(f.path.to_ascii_lowercase()) {
            return Err(bad(format!("file listed twice {:?}", f.path)));
        }
        if !is_sha256_hex(&f.sha256) {
            return Err(bad(format!("bad sha256 for {:?}", f.path)));
        }
    }
    let mut delta = None;
    for (from, p) in &entry.delta_from {
        validate_package(p, &installed.url_prefix)?;
        let from = Version::parse(from).map_err(|e| bad(format!("bad delta version {from:?}: {e}")))?;
        if from == installed.version {
            delta = Some(p.clone());
        }
    }
    let apply = if installed.install_type.is_package_managed() {
        ApplyMode::NotifyOnly
    } else if installed.version < min_supported {
        ApplyMode::ReinstallRequired
    } else {
        ApplyMode::InApp
    };
    Ok(UpdateCheck::Available(Box::new(AvailableUpdate {
        version,
        released: m.released.clone(),
        notes: m.notes.clone(),
        apply,
        package: entry.clone(),
        delta,
    })))
}

fn bad(msg: String) -> UpdateError {
    UpdateError::BadManifest(msg)
}

fn validate_package(p: &Package, url_prefix: &str) -> Result<(), UpdateError> {
    if !p.url.starts_with("https://") || !p.url.starts_with(url_prefix) || p.url.len() > 2048 {
        return Err(bad(format!("package URL {:?} is not an allowed HTTPS URL", p.url)));
    }
    if p.url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(bad("package URL contains whitespace or control characters".into()));
    }
    if p.size == 0 {
        return Err(bad("package size is zero".into()));
    }
    if !is_sha256_hex(&p.sha256) {
        return Err(bad(format!("bad package sha256 {:?}", p.sha256)));
    }
    if p.sig.len() > MAX_SIGNATURE_BYTES {
        return Err(bad("package signature too large".into()));
    }
    minisign_verify::Signature::decode(&p.sig).map_err(|e| bad(format!("bad package signature: {e}")))?;
    Ok(())
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A relative path that stays inside the install directory on every OS: `/`
/// separators, no empty, `.` or `..` components, no drive letters, backslashes, colons
/// or control characters, and no reserved Windows device names.
pub fn is_safe_relative_path(p: &str) -> bool {
    if p.is_empty() || p.len() > 1024 || p.starts_with('/') {
        return false;
    }
    if p.chars().any(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return false;
    }
    p.split('/').all(|c| {
        let stem = c.split('.').next().unwrap_or(c).to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes().get(3).is_some_and(u8::is_ascii_digit));
        !c.is_empty() && c != "." && c != ".." && !c.ends_with('.') && !c.ends_with(' ') && !reserved
    })
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` (UTC only) to Unix seconds.
pub fn parse_utc(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || b.get(19) != Some(&b'Z')
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u64> {
        let part = s.get(r)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if y < 1970 || !(1..=12).contains(&mo) || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let dim = *mdays.get(usize::try_from(mo - 1).ok()?)?;
    if d == 0 || d > dim {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm, proleptic Gregorian).
    let (yy, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy / 400;
    let yoe = yy - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec)
}

/// Standard base64 (with padding) decoder, used only to read key IDs.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let b = s.as_bytes();
    if !b.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for chunk in b.chunks_exact(4) {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n <<= 6;
            if i < 4 - pad {
                n |= val(c)?;
            }
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(bytes.get(1..4 - pad)?);
    }
    Some(out)
}
