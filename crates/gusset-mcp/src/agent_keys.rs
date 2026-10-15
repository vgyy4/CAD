//! Agent keys (ADR 0012 "Agent keys", dogfooding F2-2): a credential per agent, created by
//! the user in the app. An HTTP request bearing an agent key acts as that agent: the same
//! name on every connection (`key_<name>_<id>`), so it continues on its own branch, and the
//! standing approvals granted to it apply only to the holder of the key. `<id>` is random
//! per key: a key created again under the same name is a new agent, without the old one's
//! grants or branch, and a project file cannot plant a keyed agent's branch by guessing
//! its name (security review v0.3 W5, W6).
//!
//! Rules (each one has a test):
//! - Only the app creates keys; the token is shown once and only its BLAKE3 hash is kept.
//!   Checks hash the presented token and compare hashes in constant time.
//! - Key names are sanitized client names that do not end in `-<digits>`; the agent name
//!   `key_<name>` contains `_`, which no sanitized client name has, so an agent without
//!   the key can never take a keyed agent's name, branch or grants.
//! - The kill switch (no shared token) blocks keys too.
//! - Keys live in an owner-only file written with `durable_write`; a file that is not a
//!   regular file, too large, malformed, of another version, or writable or owned by
//!   someone else gives no keys and is kept as `agent-keys.json.bad` before the next key
//!   is written (security review v0.3 W7). At most [`MAX_KEYS`].

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Most keys kept.
pub const MAX_KEYS: usize = 64;
const MAX_FILE_BYTES: u64 = 64 * 1024;
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentKey {
    /// The key's name (`cad-bot`); the agent is `key_cad-bot`.
    pub name: String,
    /// BLAKE3 of the token, hex.
    hash: String,
    pub created: u64,
    /// Random, per key (12 lowercase hex); part of the agent name. Keys from files written
    /// before it existed use the start of their hash.
    #[serde(default)]
    id: String,
}

impl AgentKey {
    /// The agent name a request with this key acts as.
    #[must_use]
    pub fn agent(&self) -> String {
        let id = if valid_id(&self.id) { self.id.as_str() } else { self.hash.get(..ID_LEN).unwrap_or_default() };
        agent_name(&self.name, id)
    }
}

const ID_LEN: usize = 12;

fn valid_id(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The agent name of key `name` with id `id`.
#[must_use]
pub fn agent_name(name: &str, id: &str) -> String {
    format!("key_{name}_{id}")
}

/// Whether `agent` is a keyed agent's name (`key_<valid key name>_<id>`).
#[must_use]
pub fn is_keyed_agent(agent: &str) -> bool {
    agent
        .strip_prefix("key_")
        .and_then(|rest| rest.rsplit_once('_'))
        .is_some_and(|(n, id)| valid_name(n).is_ok() && valid_id(id))
}

/// A key name: a sanitized client name (lowercase letters, digits, `-`) that does not end
/// in `-<digits>`.
///
/// # Errors
/// Why the name is not allowed.
pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || crate::agents::sanitize_client_name(name) != name {
        return Err("use lowercase letters, digits and single dashes (e.g. cad-bot)".into());
    }
    if crate::limits::identity_of(name) != name {
        return Err("a key name may not end in a dash and digits".into());
    }
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FileData {
    version: u32,
    keys: Vec<AgentKey>,
}

#[derive(Debug, Default)]
struct Inner {
    keys: Vec<AgentKey>,
    file: Option<PathBuf>,
}

/// The agent keys of this installation.
#[derive(Debug, Default)]
pub struct AgentKeys {
    inner: RwLock<Inner>,
}

/// The key file, when it is a small regular file of this version that only we own and
/// can write.
fn read_file(path: &Path, meta: &std::fs::Metadata) -> Result<FileData, String> {
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err("the agent key file is not a small regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let ours = path.parent().and_then(crate::bridge::trusted_dir);
        if meta.permissions().mode() & 0o077 != 0 || ours != Some(meta.uid()) {
            return Err("the agent key file or its folder is open to other users".into());
        }
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let data: FileData = serde_json::from_slice(&bytes).map_err(|e| format!("agent key file: {e}"))?;
    if data.version != VERSION {
        return Err(format!("agent key file version {} (this app writes {VERSION})", data.version));
    }
    Ok(data)
}

fn hash_hex(token: &[u8]) -> String {
    blake3::hash(token).to_hex().to_string()
}

impl AgentKeys {
    /// Loads keys from `path` and saves there from now on. A missing file is no keys.
    ///
    /// # Errors
    /// The file exists but is not a regular file, too large or malformed (no keys loaded).
    pub fn load(&self, path: &Path) -> Result<usize, String> {
        let mut g = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.file = Some(path.to_path_buf());
        g.keys.clear();
        let meta = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.to_string()),
        };
        let data = match read_file(path, &meta) {
            Ok(d) => d,
            Err(e) => {
                // Kept for the user (the next key would overwrite it otherwise).
                let mut bad = path.as_os_str().to_owned();
                bad.push(".bad");
                let _ = std::fs::rename(path, &bad);
                return Err(e);
            }
        };
        for k in data.keys {
            let hash_ok = k.hash.len() == 64 && k.hash.bytes().all(|b| b.is_ascii_hexdigit());
            if g.keys.len() >= MAX_KEYS || !hash_ok || valid_name(&k.name).is_err() {
                continue;
            }
            if !g.keys.iter().any(|x| x.name == k.name) {
                g.keys.push(k);
            }
        }
        Ok(g.keys.len())
    }

    fn save(g: &Inner) -> Result<(), String> {
        let Some(path) = &g.file else { return Ok(()) };
        let data = FileData { version: VERSION, keys: g.keys.clone() };
        let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
        let fs = gusset_store::fs::RealFs::private();
        if let Some(dir) = path.parent() {
            gusset_store::fs::create_private_dir_all(dir).map_err(|e| e.to_string())?;
        }
        gusset_store::durable::durable_write_unchecked(&fs, path, &bytes).map_err(|e| e.to_string())
    }

    /// Creates key `name` and returns its token (shown to the user once).
    ///
    /// # Errors
    /// An invalid or taken name, too many keys, no randomness, or the file cannot be saved.
    pub fn create(&self, name: &str, now: u64) -> Result<String, String> {
        valid_name(name)?;
        let mut g = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.keys.iter().any(|k| k.name == name) {
            return Err(format!("a key named '{name}' exists; remove it first"));
        }
        if g.keys.len() >= MAX_KEYS {
            return Err(format!("at most {MAX_KEYS} agent keys"));
        }
        let token = crate::approval::random_hex(32).ok_or("no secure randomness available")?;
        let id = crate::approval::random_hex(ID_LEN / 2).ok_or("no secure randomness available")?;
        g.keys.push(AgentKey { name: name.to_string(), hash: hash_hex(token.as_bytes()), created: now, id });
        if let Err(e) = Self::save(&g) {
            g.keys.pop();
            return Err(e);
        }
        Ok(token)
    }

    /// Removes key `name`; requests with it are refused from now on.
    ///
    /// # Errors
    /// The file cannot be saved (the key is removed for this session anyway).
    pub fn remove(&self, name: &str) -> Result<bool, String> {
        let mut g = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        let n = g.keys.len();
        g.keys.retain(|k| k.name != name);
        let removed = g.keys.len() != n;
        if removed {
            Self::save(&g)?;
        }
        Ok(removed)
    }

    #[must_use]
    pub fn list(&self) -> Vec<AgentKey> {
        self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner).keys.clone()
    }

    /// The agent name for a presented token, if it is one of the keys.
    #[must_use]
    pub fn check(&self, token: &[u8]) -> Option<String> {
        if token.is_empty() {
            return None;
        }
        let presented = hash_hex(token);
        let g = self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Every key is compared, so the time does not tell which one matched.
        let mut found = None;
        for k in &g.keys {
            if crate::http::ct_eq(presented.as_bytes(), k.hash.as_bytes()) {
                found = Some(k.agent());
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("gusset-keys-{tag}-{}-{n}", std::process::id())).join("agent-keys.json")
    }

    #[test]
    fn a_key_is_shown_once_stored_hashed_and_survives_a_restart() {
        let p = tmp("restart");
        let k = AgentKeys::default();
        k.load(&p).unwrap();
        let token = k.create("cad-bot", 1).unwrap();
        let agent = k.check(token.as_bytes()).unwrap();
        assert!(agent.starts_with("key_cad-bot_") && is_keyed_agent(&agent), "{agent}");
        assert_eq!(k.check(b"wrong"), None);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains(&token), "only the hash is stored");
        let k2 = AgentKeys::default();
        assert_eq!(k2.load(&p).unwrap(), 1);
        assert_eq!(k2.check(token.as_bytes()), Some(agent.clone()));
        assert!(k2.remove("cad-bot").unwrap());
        assert_eq!(k2.check(token.as_bytes()), None);
        // W5: the same name again is another agent (no inherited grants or branch).
        let again = k2.create("cad-bot", 2).unwrap();
        let other = k2.check(again.as_bytes()).unwrap();
        assert!(other.starts_with("key_cad-bot_") && other != agent, "{other} {agent}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o077, 0, "owner-only");
        }
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn keyed_names_cannot_be_taken_by_clients() {
        for bad in ["Cad Bot", "bot-2", "", "a_b", "x".repeat(40).as_str()] {
            assert!(valid_name(bad).is_err(), "{bad}");
        }
        assert!(valid_name("cad-bot").is_ok());
        // No sanitized client name contains '_', so none equals a keyed agent's name.
        assert!(!crate::agents::sanitize_client_name("key_cad-bot").contains('_'));
        assert!(is_keyed_agent("key_cad-bot_0123456789ab"));
        for bad in ["key_cad-bot", "cad-bot", "key_Bad_0123456789ab", "key_cad-bot_0123", "key_cad-bot_0123456789AB"] {
            assert!(!is_keyed_agent(bad), "{bad}");
        }
    }

    #[test]
    fn forged_files_give_no_keys() {
        let p = tmp("forged");
        gusset_store::fs::create_private_dir_all(p.parent().unwrap()).unwrap();
        // Written like the app writes it: owner-only.
        let write = |bytes: &[u8]| {
            std::fs::write(&p, bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        };
        write(b"{nope");
        let k = AgentKeys::default();
        assert!(k.load(&p).is_err());
        // W7: the damaged file is kept, and the next key does not overwrite it.
        let bad = p.with_file_name("agent-keys.json.bad");
        assert_eq!(std::fs::read(&bad).unwrap(), b"{nope");
        k.create("new", 1).unwrap();
        assert_eq!(std::fs::read(&bad).unwrap(), b"{nope");
        write(serde_json::json!({"version": 2, "keys": []}).to_string().as_bytes());
        assert!(k.load(&p).unwrap_err().contains("version"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            write(serde_json::json!({"version": 1, "keys": []}).to_string().as_bytes());
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o666)).unwrap();
            assert!(k.load(&p).unwrap_err().contains("other users"));
        }
        let forged = serde_json::json!({"version": 1, "keys": [
            {"name": "Bad Name", "hash": "00".repeat(32), "created": 0},
            {"name": "short", "hash": "abc", "created": 0},
            {"name": "ok", "hash": "11".repeat(32), "created": 0}
        ]});
        write(forged.to_string().as_bytes());
        assert_eq!(k.load(&p).unwrap(), 1);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
