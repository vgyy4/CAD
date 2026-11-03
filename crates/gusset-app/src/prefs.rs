//! Window preferences kept between runs in the data folder (`ui-prefs.json`): theme,
//! reduced motion and the mouse navigation preset. A missing or unreadable file means the
//! defaults; writing goes through the store's durable write.

use crate::theme::ThemeKind;
use crate::viewport::NavPreset;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const FILE: &str = "ui-prefs.json";

#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    pub theme: ThemeKind,
    pub reduce_motion: bool,
    pub nav: NavPreset,
    /// Recently opened or saved project files, newest first (at most [`MAX_RECENT`]).
    pub recent: Vec<String>,
    /// Widths of the browser and the properties panel (DESIGN.md §1.15: layouts stay).
    pub browser_width: Option<f32>,
    pub properties_width: Option<f32>,
    /// UI scale (DESIGN.md §1.22), [`MIN_SCALE`]..=[`MAX_SCALE`].
    pub ui_scale: f32,
    /// The S quick bar's tools (command ids) outside and inside sketches; empty means the
    /// defaults (DESIGN.md §1.3: customizable).
    pub quick_solid: Vec<String>,
    pub quick_sketch: Vec<String>,
    /// Panel layout (DESIGN.md §1.15): the browser and the timeline collapsed, and the
    /// browser docked at the right (the properties panel then at the left).
    pub browser_collapsed: bool,
    pub timeline_collapsed: bool,
    pub docks_swapped: bool,
}

pub const MIN_SCALE: f32 = 0.75;
pub const MAX_SCALE: f32 = 2.5;
/// The scales the View menu offers.
pub const SCALES: [f32; 7] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5];

pub const MAX_RECENT: usize = 10;

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeKind::Dark,
            reduce_motion: false,
            nav: NavPreset::Gusset,
            recent: Vec::new(),
            browser_width: None,
            properties_width: None,
            ui_scale: 1.0,
            quick_solid: Vec::new(),
            quick_sketch: Vec::new(),
            browser_collapsed: false,
            timeline_collapsed: false,
            docks_swapped: false,
        }
    }
}

/// `path` first in the recent list, without duplicates, at most [`MAX_RECENT`].
pub fn remember(recent: &mut Vec<String>, path: &str) {
    recent.retain(|p| p != path);
    recent.insert(0, path.to_string());
    recent.truncate(MAX_RECENT);
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
        recent: v
            .get("recent")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|p| !p.contains('\0') && p.len() < 4096)
            .take(MAX_RECENT)
            .map(str::to_string)
            .collect(),
        browser_width: width(&v, "browser_width"),
        properties_width: width(&v, "properties_width"),
        ui_scale: v
            .get("ui_scale")
            .and_then(Value::as_f64)
            .map(|s| s as f32)
            .filter(|s| (MIN_SCALE..=MAX_SCALE).contains(s))
            .unwrap_or(d.ui_scale),
        quick_solid: commands(&v, "quick_solid"),
        quick_sketch: commands(&v, "quick_sketch"),
        browser_collapsed: v.get("browser_collapsed").and_then(Value::as_bool).unwrap_or(false),
        timeline_collapsed: v.get("timeline_collapsed").and_then(Value::as_bool).unwrap_or(false),
        docks_swapped: v.get("docks_swapped").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// A list of known command ids from the file (unknown ones are dropped).
fn commands(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(gusset_commands::find)
        .map(|s| s.id.to_string())
        .take(crate::quick_menus::MAX_TOOLS)
        .collect()
}

/// A panel width from the file, if it is a sane one.
fn width(v: &Value, key: &str) -> Option<f32> {
    v.get(key).and_then(Value::as_f64).filter(|w| (120.0..=2000.0).contains(w)).map(|w| w as f32)
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
        "recent": p.recent,
        "browser_width": p.browser_width,
        "properties_width": p.properties_width,
        "ui_scale": p.ui_scale,
        "quick_solid": p.quick_solid,
        "quick_sketch": p.quick_sketch,
        "browser_collapsed": p.browser_collapsed,
        "timeline_collapsed": p.timeline_collapsed,
        "docks_swapped": p.docks_swapped,
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
        let mut want = Prefs {
            theme: ThemeKind::Light,
            reduce_motion: true,
            nav: NavPreset::SolidWorks,
            browser_width: Some(300.0),
            ui_scale: 1.5,
            quick_sketch: vec!["sketch.trim".into()],
            timeline_collapsed: true,
            docks_swapped: true,
            ..Prefs::default()
        };
        remember(&mut want.recent, "/a.gus");
        remember(&mut want.recent, "/b.gus");
        remember(&mut want.recent, "/a.gus");
        assert_eq!(want.recent, ["/a.gus", "/b.gus"]);
        save(&p, &want).unwrap();
        assert_eq!(load(&p), want);
        std::fs::write(&p, b"{not json").unwrap();
        assert_eq!(load(&p), Prefs::default());
        std::fs::write(&p, br#"{"theme": "neon", "navigation": "Onshape"}"#).unwrap();
        assert_eq!(load(&p), Prefs { nav: NavPreset::Onshape, ..Prefs::default() });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
