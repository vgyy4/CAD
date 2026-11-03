//! Window preferences kept between runs in the data folder (`ui-prefs.json`): theme,
//! reduced motion and the mouse navigation preset. A missing or unreadable file means the
//! defaults; writing goes through the store's durable write.

use crate::theme::ThemeKind;
use crate::viewport::NavPreset;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const FILE: &str = "ui-prefs.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefs {
    pub theme: ThemeKind,
    pub reduce_motion: bool,
    pub nav: NavPreset,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { theme: ThemeKind::Dark, reduce_motion: false, nav: NavPreset::Gusset }
    }
}

#[must_use]
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// The preferences in `path` (defaults for anything missing or unknown).
#[must_use]
pub fn load(path: &Path) -> Prefs {
    let d = Prefs::default();
    // A preferences file is tiny: refuse anything large instead of reading it.
    let Ok(meta) = std::fs::metadata(path) else { return d };
    if meta.len() > 64 << 10 {
        return d;
    }
    let Ok(v) = std::fs::read(path).map_err(|_| ()).and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|_| ()))
    else {
        return d;
    };
    Prefs {
        theme: match v.get("theme").and_then(Value::as_str) {
            Some("light") => ThemeKind::Light,
            Some("dark") => ThemeKind::Dark,
            _ => d.theme,
        },
        reduce_motion: v.get("reduce_motion").and_then(Value::as_bool).unwrap_or(d.reduce_motion),
        nav: v.get("navigation").and_then(Value::as_str).and_then(NavPreset::from_name).unwrap_or(d.nav),
    }
}

/// Writes the preferences (durably: the old file stays whole if this fails).
///
/// # Errors
/// A message when the file cannot be written.
pub fn save(path: &Path, p: &Prefs) -> Result<(), String> {
    let v = json!({
        "theme": match p.theme { ThemeKind::Dark => "dark", ThemeKind::Light => "light" },
        "reduce_motion": p.reduce_motion,
        "navigation": p.nav.name(),
    });
    let bytes = serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?;
    let check = |b: &[u8]| serde_json::from_slice::<Value>(b).map(|_| ()).map_err(|e| e.to_string());
    gusset_store::durable::durable_write(&gusset_store::fs::RealFs::private(), path, &bytes, &check)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_bad_files_mean_defaults() {
        let dir = std::env::temp_dir().join(format!("gusset-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = path(&dir);
        assert_eq!(load(&p), Prefs::default());
        let want = Prefs { theme: ThemeKind::Light, reduce_motion: true, nav: NavPreset::SolidWorks };
        save(&p, &want).unwrap();
        assert_eq!(load(&p), want);
        std::fs::write(&p, b"{not json").unwrap();
        assert_eq!(load(&p), Prefs::default());
        std::fs::write(&p, br#"{"theme": "neon", "navigation": "Onshape"}"#).unwrap();
        assert_eq!(load(&p), Prefs { nav: NavPreset::Onshape, ..Prefs::default() });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
