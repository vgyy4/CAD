//! Agent keys (ADR 0012 "Agent keys", dogfooding F2-2): a credential per agent, created by
//! the user in the app. An HTTP request bearing an agent key acts as that agent: the same
//! name on every connection (`key_<name>`), so it continues on its own branch, and the
//! standing approvals granted to it apply only to the holder of the key.
//!
//! Rules (each one has a test):
//! - Only the app creates keys; the token is shown once and only its BLAKE3 hash is kept.
//!   Checks hash the presented token and compare hashes in constant time.
//! - Key names are sanitized client names that do not end in `-<digits>`; the agent name
//!   `key_<name>` contains `_`, which no sanitized client name has, so an agent without
//!   the key can never take a keyed agent's name, branch or grants.
//! - The kill switch (no shared token) blocks keys too.
//! - Keys live in an owner-only file written with `durable_write`; a file that is not a
//!   regular file, too large or malformed is ignored (no keys), at most [`MAX_KEYS`].

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
}

impl AgentKey {
    /// The agent name a request with this key acts as.
    #[must_use]
    pub fn agent(&self) -> String {
        agent_name(&self.name)
    }
}

/// The agent name of key `name`.
#[must_use]
pub fn agent_name(name: &str) -> String {
    format!("key_{name}")
}

/// Whether `agent` is a keyed agent's name (`key_<valid key name>`).
#[must_use]
pub fn is_keyed_agent(agent: &str) -> bool {
    agent.strip_prefix("key_").is_some_and(|n| valid_name(n).is_ok())
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
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Err("the agent key file is not a small regular file".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let data: FileData = serde_json::from_slice(&bytes).map_err(|e| format!("agent key file: {e}"))?;
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
        g.keys.push(AgentKey { name: name.to_string(), hash: hash_hex(token.as_bytes()), created: now });
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
        assert_eq!(k.check(token.as_bytes()).as_deref(), Some("key_cad-bot"));
        assert_eq!(k.check(b"wrong"), None);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains(&token), "only the hash is stored");
        let k2 = AgentKeys::default();
        assert_eq!(k2.load(&p).unwrap(), 1);
        assert_eq!(k2.check(token.as_bytes()).as_deref(), Some("key_cad-bot"));
        assert!(k2.remove("cad-bot").unwrap());
        assert_eq!(k2.check(token.as_bytes()), None);
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
        assert!(is_keyed_agent("key_cad-bot") && !is_keyed_agent("cad-bot") && !is_keyed_agent("key_Bad"));
    }

    #[test]
    fn forged_files_give_no_keys() {
        let p = tmp("forged");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"{nope").unwrap();
        let k = AgentKeys::default();
        assert!(k.load(&p).is_err());
        let forged = serde_json::json!({"version": 1, "keys": [
            {"name": "Bad Name", "hash": "00".repeat(32), "created": 0},
            {"name": "short", "hash": "abc", "created": 0},
            {"name": "ok", "hash": "11".repeat(32), "created": 0}
        ]});
        std::fs::write(&p, forged.to_string()).unwrap();
        assert_eq!(k.load(&p).unwrap(), 1);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
