//! Per-user directories, owner-only permissions, project IDs and the risky-storage
//! classifier (ADR 0007).
//!
//! Layout under the data directory:
//! ```text
//! <data>/projects.txt            project registry (see housekeeping)
//! <data>/projects.lock           guards registry updates
//! <data>/recovery/<id>.grec      one recovery file per project
//! <data>/lkg/<id>.gus            one last-known-good copy per project
//! <data>/locks/<id>.lock         project locks
//! <cache>/shared/<hash>.bin      shared cache entries
//! <cache>/<id>/<hash>.bin        per-project cache entries (thumbnails, ...)
//! ```

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

pub use crate::fs::{create_private_dir_all, make_private_file};

/// A project's random 128-bit ID, stored in the project file. Displayed as 32 lowercase
/// hex digits, which is also how it appears in file names (so it can never contain a
/// path separator).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub [u8; 16]);

impl ProjectId {
    /// A fresh random ID from the OS CSPRNG.
    ///
    /// # Errors
    /// If the OS random number generator fails.
    pub fn new_random() -> io::Result<Self> {
        crate::rng::os_random_128().map(Self)
    }

    /// Parses exactly 32 hex digits.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let mut b = [0u8; 16];
        crate::wire::from_hex_into(s, &mut b).then_some(Self(b))
    }

    /// The 32-digit lowercase hex form.
    #[must_use]
    pub fn to_hex(&self) -> String {
        crate::wire::to_hex(&self.0)
    }
}

impl fmt::Debug for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProjectId({})", self.to_hex())
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// The app's per-user data and cache directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDirs {
    data: PathBuf,
    cache: PathBuf,
}

impl DataDirs {
    /// The OS-standard locations: `$XDG_DATA_HOME/gusset` and `$XDG_CACHE_HOME/gusset`
    /// on Linux, `%LOCALAPPDATA%\Gusset\Gusset\data` and `%LOCALAPPDATA%\Gusset\Gusset\cache`
    /// on Windows, `~/Library/Application Support/org.Gusset.Gusset` and
    /// `~/Library/Caches/org.Gusset.Gusset` on macOS. `None` if no home directory is
    /// known.
    #[must_use]
    pub fn from_os() -> Option<Self> {
        let d = directories::ProjectDirs::from("org", "Gusset", "Gusset")?;
        Some(Self { data: d.data_local_dir().to_path_buf(), cache: d.cache_dir().to_path_buf() })
    }

    /// Explicit locations (tests, portable mode).
    #[must_use]
    pub fn at(data: impl Into<PathBuf>, cache: impl Into<PathBuf>) -> Self {
        Self { data: data.into(), cache: cache.into() }
    }

    /// Creates all directories with owner-only permissions.
    ///
    /// # Errors
    /// I/O errors.
    pub fn ensure(&self) -> io::Result<()> {
        for d in [self.data.clone(), self.recovery_dir(), self.lkg_dir(), self.locks_dir(), self.cache.clone()] {
            create_private_dir_all(&d)?;
        }
        Ok(())
    }

    /// The data directory.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// The cache directory.
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    /// `<data>/recovery`.
    #[must_use]
    pub fn recovery_dir(&self) -> PathBuf {
        self.data.join("recovery")
    }

    /// `<data>/lkg`.
    #[must_use]
    pub fn lkg_dir(&self) -> PathBuf {
        self.data.join("lkg")
    }

    /// `<data>/locks`.
    #[must_use]
    pub fn locks_dir(&self) -> PathBuf {
        self.data.join("locks")
    }

    /// `<data>/projects.txt`.
    #[must_use]
    pub fn registry_file(&self) -> PathBuf {
        self.data.join("projects.txt")
    }

    /// `<data>/recovery/<id>.grec`.
    #[must_use]
    pub fn recovery_file(&self, id: &ProjectId) -> PathBuf {
        self.recovery_dir().join(format!("{id}.{}", crate::recovery::EXTENSION))
    }

    /// `<data>/lkg/<id>.gus`.
    #[must_use]
    pub fn lkg_file(&self, id: &ProjectId) -> PathBuf {
        self.lkg_dir().join(format!("{id}.gus"))
    }

    /// `<data>/locks/<id>.lock`.
    #[must_use]
    pub fn lock_file(&self, id: &ProjectId) -> PathBuf {
        self.locks_dir().join(format!("{id}.lock"))
    }
}

/// Why a storage location is risky for crash safety.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskyStorage {
    /// A network file system (NFS, SMB/CIFS, sshfs, ...) or a Windows network drive/UNC
    /// path.
    NetworkShare {
        /// File-system type or `"unc"`, when known.
        fs_type: String,
    },
    /// Inside a folder managed by a cloud-sync client.
    CloudSync {
        /// The provider.
        provider: CloudProvider,
    },
    /// FAT/exFAT (typical of removable media): no journaling, weak rename guarantees.
    FatFamily {
        /// File-system type.
        fs_type: String,
    },
}

/// A cloud-sync provider recognised by [`classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    /// Microsoft OneDrive.
    OneDrive,
    /// Dropbox.
    Dropbox,
    /// Apple iCloud Drive.
    ICloud,
    /// Google Drive.
    GoogleDrive,
    /// Another provider (macOS File Provider, rclone/FUSE cloud mounts, ...).
    Other,
}

impl fmt::Display for RiskyStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskyStorage::NetworkShare { fs_type } => write!(f, "network share ({fs_type})"),
            RiskyStorage::CloudSync { provider } => write!(f, "cloud-sync folder ({provider:?})"),
            RiskyStorage::FatFamily { fs_type } => write!(f, "{fs_type} file system (removable media)"),
        }
    }
}

/// Classifies where `path` lives. Best effort and conservative: `None` means "nothing
/// risky detected", not a guarantee.
///
/// - Cloud sync: path components such as `OneDrive`, `OneDrive - <org>`, `Dropbox`,
///   `Google Drive`, `My Drive`, `iCloud Drive`, `iCloudDrive`,
///   `Library/Mobile Documents` and `Library/CloudStorage` (macOS), or a `.dropbox`
///   marker in an ancestor.
/// - Linux: the file-system type of the mount containing the path, from
///   `/proc/self/mountinfo` (network: nfs, cifs, smb3, sshfs, 9p, ceph, ...; FAT:
///   vfat, msdos, exfat; cloud: rclone and other FUSE cloud mounts).
/// - Windows: UNC paths, `GetDriveTypeW == DRIVE_REMOTE`, and the volume's file-system
///   name (FAT, FAT32, exFAT).
/// - macOS: path rules only (no `statfs` without more FFI); network volumes under
///   `/Volumes` are not detected.
#[must_use]
pub fn classify(path: &Path) -> Option<RiskyStorage> {
    let abs = absolute_existing(path);
    if let Some(provider) = cloud_provider(&abs) {
        return Some(RiskyStorage::CloudSync { provider });
    }
    platform_classify(&abs)
}

/// Canonicalises the longest existing ancestor of `path` (so new files can be
/// classified) and re-appends the rest.
fn absolute_existing(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(c) = std::fs::canonicalize(&cur) {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (cur.file_name().map(std::ffi::OsStr::to_os_string), cur.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                cur = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The cloud provider implied by `path`'s components or markers.
#[must_use]
pub fn cloud_provider(path: &Path) -> Option<CloudProvider> {
    let comps: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    for (i, c) in comps.iter().enumerate() {
        let next = comps.get(i + 1).map(String::as_str);
        let p = match c.as_str() {
            "onedrive" => Some(CloudProvider::OneDrive),
            s if s.starts_with("onedrive - ") => Some(CloudProvider::OneDrive),
            "dropbox" => Some(CloudProvider::Dropbox),
            s if s.starts_with("dropbox (") => Some(CloudProvider::Dropbox),
            "google drive" | "googledrive" | "my drive" => Some(CloudProvider::GoogleDrive),
            "icloud drive" | "iclouddrive" => Some(CloudProvider::ICloud),
            "library" if next == Some("mobile documents") => Some(CloudProvider::ICloud),
            "library" if next == Some("cloudstorage") => {
                let provider = comps.get(i + 2).map(String::as_str).unwrap_or_default();
                Some(if provider.starts_with("onedrive") {
                    CloudProvider::OneDrive
                } else if provider.starts_with("dropbox") {
                    CloudProvider::Dropbox
                } else if provider.starts_with("googledrive") {
                    CloudProvider::GoogleDrive
                } else {
                    CloudProvider::Other
                })
            }
            _ => None,
        };
        if p.is_some() {
            return p;
        }
    }
    // Dropbox puts a `.dropbox` marker in its root folder.
    path.ancestors().skip(1).take(32).find(|a| a.join(".dropbox").is_file()).map(|_| CloudProvider::Dropbox)
}

/// Classifies a file-system type name (as in `/proc/self/mountinfo` or
/// `GetVolumeInformationW`).
#[must_use]
pub fn classify_fs_type(fs_type: &str) -> Option<RiskyStorage> {
    let t = fs_type.to_ascii_lowercase();
    let network = [
        "nfs",
        "nfs4",
        "cifs",
        "smb",
        "smb2",
        "smb3",
        "smbfs",
        "ncpfs",
        "afs",
        "9p",
        "ceph",
        "glusterfs",
        "lustre",
        "davfs",
        "fuse.sshfs",
        "fuse.davfs2",
        "fuse.glusterfs",
        "fuse.cephfs",
        "fuse.s3fs",
    ];
    let cloud = ["fuse.rclone", "fuse.onedriver", "fuse.google-drive-ocamlfuse", "fuse.gcsfuse", "fuse.dropbox"];
    let fat = ["vfat", "msdos", "fat", "fat12", "fat16", "fat32", "exfat", "fuse.exfat", "fuseblk.exfat"];
    if network.contains(&t.as_str()) {
        Some(RiskyStorage::NetworkShare { fs_type: fs_type.to_owned() })
    } else if cloud.contains(&t.as_str()) {
        Some(RiskyStorage::CloudSync { provider: CloudProvider::Other })
    } else if fat.contains(&t.as_str()) {
        Some(RiskyStorage::FatFamily { fs_type: fs_type.to_owned() })
    } else {
        None
    }
}

/// Unescapes `\ooo` octal sequences used in `/proc/self/mountinfo` fields.
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'\\'
            && let Some(oct) = b.get(i + 1..i + 4)
            && oct.iter().all(|d| (b'0'..=b'7').contains(d))
        {
            let v = oct.iter().fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'));
            out.push(u8::try_from(v).unwrap_or(b'?'));
            i += 4;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Given the text of `/proc/self/mountinfo`, the file-system type of the mount that
/// contains `path` (longest matching mount point). `path` should be canonical.
#[must_use]
pub fn mount_fs_type(mountinfo: &str, path: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        let mut fields = line.split(' ');
        let Some(mount_point) = fields.nth(4).map(unescape_mount) else {
            continue;
        };
        let Some(fs_type) = fields.skip_while(|f| *f != "-").nth(1) else {
            continue;
        };
        let mp = Path::new(&mount_point);
        if path.starts_with(mp) {
            let depth = mp.components().count();
            if best.as_ref().is_none_or(|(d, _)| depth >= *d) {
                best = Some((depth, fs_type.to_owned()));
            }
        }
    }
    best.map(|(_, t)| t)
}

#[cfg(target_os = "linux")]
fn platform_classify(path: &Path) -> Option<RiskyStorage> {
    let info = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    classify_fs_type(&mount_fs_type(&info, path)?)
}

#[cfg(windows)]
fn platform_classify(path: &Path) -> Option<RiskyStorage> {
    let s = path.as_os_str().to_string_lossy();
    if (s.starts_with(r"\\") && !s.starts_with(r"\\?\")) || s.starts_with(r"\\?\UNC\") {
        return Some(RiskyStorage::NetworkShare { fs_type: "unc".into() });
    }
    let (remote, fs) = crate::win::volume_info(path).ok()?;
    if remote {
        return Some(RiskyStorage::NetworkShare { fs_type: fs.unwrap_or_else(|| "remote".into()) });
    }
    classify_fs_type(&fs?)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_classify(_path: &Path) -> Option<RiskyStorage> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_round_trip_and_strict_parse() {
        let id = ProjectId::new_random().unwrap();
        assert_eq!(ProjectId::parse(&id.to_hex()), Some(id));
        assert_eq!(ProjectId::parse("../../etc/passwd"), None);
        assert_eq!(ProjectId::parse(&"0".repeat(31)), None);
        assert_ne!(ProjectId::new_random().unwrap(), id);
    }

    #[test]
    fn cloud_paths() {
        let c = |p: &str| cloud_provider(Path::new(p));
        assert_eq!(c("/home/a/OneDrive/x.gus"), Some(CloudProvider::OneDrive));
        assert_eq!(c("C:/Users/a/OneDrive - Contoso/x.gus"), Some(CloudProvider::OneDrive));
        assert_eq!(c("/home/a/Dropbox/p/x.gus"), Some(CloudProvider::Dropbox));
        assert_eq!(c("/Users/a/Library/Mobile Documents/com~apple~CloudDocs/x.gus"), Some(CloudProvider::ICloud));
        assert_eq!(c("/Users/a/Library/CloudStorage/GoogleDrive-a@b/My Drive/x.gus"), Some(CloudProvider::GoogleDrive));
        assert_eq!(c("/Users/a/Library/CloudStorage/Box-Box/x.gus"), Some(CloudProvider::Other));
        assert_eq!(c("/home/a/projects/x.gus"), None);
    }

    #[test]
    fn mountinfo_parsing() {
        let info = "\
22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw
40 22 0:35 / /mnt/share rw,relatime shared:2 - cifs //srv/share rw
41 22 8:17 / /media/usb\\040stick rw - vfat /dev/sdb1 rw
42 40 0:36 / /mnt/share/inner rw - nfs4 srv:/x rw
garbage line
";
        let t = |p: &str| mount_fs_type(info, Path::new(p));
        assert_eq!(t("/home/a/x.gus").as_deref(), Some("ext4"));
        assert_eq!(t("/mnt/share/p/x.gus").as_deref(), Some("cifs"));
        assert_eq!(t("/mnt/share/inner/x.gus").as_deref(), Some("nfs4"));
        assert_eq!(t("/media/usb stick/x.gus").as_deref(), Some("vfat"));
        assert_eq!(t("/mnt/sharex/a").as_deref(), Some("ext4"));
        assert!(matches!(classify_fs_type("cifs"), Some(RiskyStorage::NetworkShare { .. })));
        assert!(matches!(classify_fs_type("exFAT"), Some(RiskyStorage::FatFamily { .. })));
        assert!(matches!(classify_fs_type("fuse.rclone"), Some(RiskyStorage::CloudSync { .. })));
        assert_eq!(classify_fs_type("ext4"), None);
        assert_eq!(classify_fs_type("NTFS"), None);
    }

    #[test]
    fn local_temp_dir_is_not_risky_by_path_rules() {
        let d = std::env::temp_dir().join("gusset-classify-test");
        // A not-yet-existing file still classifies via its existing ancestor.
        let _ = classify(&d.join("new.gus"));
        assert_eq!(cloud_provider(&d), None);
    }

    #[cfg(unix)]
    #[test]
    fn private_dirs_are_0700() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("gusset-perm-{}", crate::rng::os_random_u64().unwrap()));
        let d = base.join("a/b");
        create_private_dir_all(&d).unwrap();
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
