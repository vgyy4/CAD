//! What the command search (Ctrl+K) finds (DESIGN.md §1.2): every command, and also the
//! model's features (opens the editor), its parameters (edits one), the window settings
//! (theme, full screen, wireframe, section view, code view, command line, navigation
//! presets) and the recent project files. Every word typed must appear in an entry's label
//! or detail; commands whose title has them all come first.

use crate::theme::ThemeKind;
use crate::viewport::NavPreset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Theme(ThemeKind),
    FullScreen,
    Wireframe,
    Section,
    CodeView,
    CommandLine,
    Nav(NavPreset),
    ParamTable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Command(&'static str),
    Feature(String),
    Param(String),
    Setting(Setting),
    Recent(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub label: String,
    pub detail: String,
    pub target: Target,
}

fn settings() -> Vec<(String, Setting)> {
    let mut v = vec![
        ("Dark theme".to_string(), Setting::Theme(ThemeKind::Dark)),
        ("Light theme".to_string(), Setting::Theme(ThemeKind::Light)),
        ("Full-screen viewport (F11)".to_string(), Setting::FullScreen),
        ("Wireframe view".to_string(), Setting::Wireframe),
        ("Section view".to_string(), Setting::Section),
        ("Code view".to_string(), Setting::CodeView),
        ("Command line".to_string(), Setting::CommandLine),
        ("Parameters table".to_string(), Setting::ParamTable),
    ];
    v.extend(NavPreset::ALL.iter().map(|p| (format!("Mouse navigation: {}", p.name()), Setting::Nav(*p))));
    v
}

/// The entries matching `query`, best first.
#[must_use]
pub fn entries(query: &str, doc: &gusset_doc::Document, recent: &[String]) -> Vec<Entry> {
    let q = query.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let matches = |label: &str, detail: &str| {
        let hay = format!("{} {}", label.to_lowercase(), detail.to_lowercase());
        words.iter().all(|w| hay.contains(w))
    };
    let mut out: Vec<Entry> = crate::app::search(query)
        .into_iter()
        .map(|s| Entry { label: s.title.to_string(), detail: s.id.to_string(), target: Target::Command(s.id) })
        .collect();
    if words.is_empty() {
        // An empty search: the commands, then the recent files.
        out.extend(recent.iter().map(|p| recent_entry(p)));
        return out;
    }
    for f in &doc.features {
        let (label, detail) = (format!("Edit {}", f.name), f.kind.keyword().to_string());
        if matches(&label, &detail) {
            out.push(Entry { label, detail, target: Target::Feature(f.name.clone()) });
        }
    }
    for p in &doc.params {
        let (label, detail) = (format!("Parameter {}", p.name), format!("= {}", p.expr));
        if matches(&label, &detail) {
            out.push(Entry { label, detail, target: Target::Param(p.name.clone()) });
        }
    }
    for (label, s) in settings() {
        if matches(&label, "setting") {
            out.push(Entry { label, detail: "setting".into(), target: Target::Setting(s) });
        }
    }
    for p in recent {
        let e = recent_entry(p);
        if matches(&e.label, &e.detail) {
            out.push(e);
        }
    }
    out
}

fn recent_entry(path: &str) -> Entry {
    let name =
        std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    Entry { label: format!("Open {name}"), detail: path.to_string(), target: Target::Recent(path.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_commands_features_parameters_settings_and_recent_files() {
        let doc = gusset_doc::Document::from_text(
            "gusset 1\nunits mm\nparam width = 30 mm\n\nsketch S1 (plane: XY, next_id: 1) {\n}\n",
        )
        .unwrap();
        let recent = vec!["/home/me/bracket.gus".to_string()];
        let targets = |q: &str| entries(q, &doc, &recent).into_iter().map(|e| e.target).collect::<Vec<_>>();
        assert!(targets("extrude").contains(&Target::Command("feature.extrude")));
        assert!(targets("s1").contains(&Target::Feature("S1".into())));
        assert!(targets("width").contains(&Target::Param("width".into())));
        assert!(targets("light").contains(&Target::Setting(Setting::Theme(ThemeKind::Light))));
        assert!(targets("solidworks").contains(&Target::Setting(Setting::Nav(NavPreset::SolidWorks))));
        assert!(targets("bracket").contains(&Target::Recent("/home/me/bracket.gus".into())));
        assert!(targets("").contains(&Target::Recent("/home/me/bracket.gus".into())));
        assert!(targets("zzzz").is_empty());
    }
}
