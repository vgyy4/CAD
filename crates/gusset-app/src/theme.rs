//! Design tokens (docs/DESIGN.md §3) and their application to egui.
//!
//! Every color and size in the UI comes from here. A test checks these values against
//! the tables in DESIGN.md, and another checks WCAG contrast.

use egui::{Color32, CornerRadius, Margin, Stroke, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

/// Color tokens for one theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_raised: Color32,
    pub bg_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub agent: Color32,
    pub vp_bg_top: Color32,
    pub vp_bg_bottom: Color32,
    pub vp_preselect: Color32,
    pub vp_select: Color32,
    pub vp_edge: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb(((rgb >> 16) & 0xff) as u8, ((rgb >> 8) & 0xff) as u8, (rgb & 0xff) as u8)
}

pub const DARK: Palette = Palette {
    bg_app: hex(0x16181C),
    bg_panel: hex(0x1D2026),
    bg_raised: hex(0x252932),
    bg_hover: hex(0x2D323C),
    border: hex(0x323844),
    text: hex(0xE6E8EC),
    text_muted: hex(0xA0A7B4),
    accent: hex(0x4C9AFF),
    accent_text: hex(0x0B1220),
    success: hex(0x3FB27F),
    warning: hex(0xE3A33B),
    danger: hex(0xE5534B),
    agent: hex(0xB07CFF),
    vp_bg_top: hex(0x2A2F38),
    vp_bg_bottom: hex(0x1A1D22),
    vp_preselect: hex(0x7FB8FF),
    vp_select: hex(0xFFB000),
    vp_edge: hex(0x0E1013),
};

pub const LIGHT: Palette = Palette {
    bg_app: hex(0xEEF0F3),
    bg_panel: hex(0xF8F9FB),
    bg_raised: hex(0xFFFFFF),
    bg_hover: hex(0xE4E8EE),
    border: hex(0xD3D8E0),
    text: hex(0x16181C),
    text_muted: hex(0x545C69),
    accent: hex(0x1F6FEB),
    accent_text: hex(0xFFFFFF),
    success: hex(0x1E8A5A),
    warning: hex(0xA86A00),
    danger: hex(0xC93C37),
    agent: hex(0x7A4FD6),
    vp_bg_top: hex(0xF4F6F9),
    vp_bg_bottom: hex(0xD9DEE6),
    vp_preselect: hex(0x2F7DEB),
    vp_select: hex(0xD98300),
    vp_edge: hex(0x20242B),
};

/// Spacing scale (logical px): space-0 … space-7.
pub const SPACE: [f32; 8] = [0.0, 2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0];
pub const RADIUS_SM: u8 = 3;
pub const RADIUS_MD: u8 = 6;
pub const RADIUS_LG: u8 = 10;

/// Type scale (pt): caption, body, title, heading.
pub const FONT_CAPTION: f32 = 11.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_TITLE: f32 = 15.0;
pub const FONT_HEADING: f32 = 18.0;

/// Motion durations (seconds); zero under "reduce motion".
pub const MOTION_FAST: f32 = 0.090;
pub const MOTION_BASE: f32 = 0.160;
pub const MOTION_SLOW: f32 = 0.260;

pub fn palette(kind: ThemeKind) -> &'static Palette {
    match kind {
        ThemeKind::Dark => &DARK,
        ThemeKind::Light => &LIGHT,
    }
}

/// Applies the tokens to an egui context.
pub fn apply(ctx: &egui::Context, kind: ThemeKind, reduce_motion: bool) {
    let p = palette(kind);
    let mut v = match kind {
        ThemeKind::Dark => Visuals::dark(),
        ThemeKind::Light => Visuals::light(),
    };
    v.panel_fill = p.bg_panel;
    v.window_fill = p.bg_raised;
    v.extreme_bg_color = p.bg_app;
    v.faint_bg_color = p.bg_hover;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.accent_text);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.error_fg_color = p.danger;
    v.warn_fg_color = p.warning;
    v.override_text_color = Some(p.text);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS_MD);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, p.bg_panel),
        (&mut v.widgets.inactive, p.bg_raised),
        (&mut v.widgets.hovered, p.bg_hover),
        (&mut v.widgets.active, p.bg_hover),
        (&mut v.widgets.open, p.bg_raised),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(RADIUS_SM);
        w.bg_stroke = Stroke::new(1.0, p.border);
        w.fg_stroke = Stroke::new(1.0, p.text);
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_muted);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.active.bg_stroke = Stroke::new(1.5, p.accent);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(SPACE[3], SPACE[2]);
        s.spacing.button_padding = egui::vec2(SPACE[3], SPACE[2]);
        s.spacing.window_margin = Margin::same(SPACE[4] as i8);
        s.spacing.menu_margin = Margin::same(SPACE[3] as i8);
        s.spacing.interact_size.y = 24.0;
        s.animation_time = if reduce_motion { 0.0 } else { MOTION_FAST };
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Small, FontId::new(FONT_CAPTION, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(FONT_BODY, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(FONT_BODY, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(FONT_BODY, FontFamily::Monospace)),
            (TextStyle::Heading, FontId::new(FONT_HEADING, FontFamily::Proportional)),
        ]
        .into();
    });
}

/// WCAG relative luminance.
pub fn luminance(c: Color32) -> f64 {
    let ch = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// WCAG contrast ratio between two colors.
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(p: &Palette, name: &str) -> Option<Color32> {
        Some(match name {
            "bg-app" => p.bg_app,
            "bg-panel" => p.bg_panel,
            "bg-raised" => p.bg_raised,
            "bg-hover" => p.bg_hover,
            "border" => p.border,
            "text" => p.text,
            "text-muted" => p.text_muted,
            "accent" => p.accent,
            "accent-text" => p.accent_text,
            "success" => p.success,
            "warning" => p.warning,
            "danger" => p.danger,
            "agent" => p.agent,
            "vp-preselect" => p.vp_preselect,
            "vp-select" => p.vp_select,
            "vp-edge" => p.vp_edge,
            _ => return None,
        })
    }

    /// Parses `| \`token\` | \`#RRGGBB\` |` rows of a DESIGN.md table section.
    fn design_rows(section: &str) -> Vec<(String, Color32)> {
        let doc = include_str!("../../../docs/DESIGN.md");
        let start = doc.find(section).unwrap_or(0);
        let rest = &doc[start..];
        let end = rest[section.len()..].find("###").map_or(rest.len(), |e| e + section.len());
        rest[..end]
            .lines()
            .filter_map(|l| {
                let cells: Vec<&str> = l.split('|').map(str::trim).collect();
                let name = cells.get(1)?.trim_matches('`');
                let val = cells.get(2)?.trim_matches('`');
                let hexv = val.split_whitespace().next()?.strip_prefix('#')?;
                if hexv.len() != 6 {
                    return None;
                }
                Some((name.to_string(), hex(u32::from_str_radix(hexv, 16).ok()?)))
            })
            .collect()
    }

    #[test]
    fn tokens_match_design_doc() {
        for (section, pal) in [("### Color: dark theme", &DARK), ("### Color: light theme", &LIGHT)] {
            let rows = design_rows(section);
            assert!(rows.len() >= 13, "{section}: {rows:?}");
            for (name, c) in rows {
                if let Some(t) = token(pal, &name) {
                    assert_eq!(t, c, "{section} {name}");
                }
            }
        }
    }

    #[test]
    fn contrast_meets_wcag_aa() {
        for p in [&DARK, &LIGHT] {
            assert!(contrast(p.text, p.bg_panel) >= 7.0);
            assert!(contrast(p.text, p.bg_app) >= 7.0);
            assert!(contrast(p.text_muted, p.bg_panel) >= 4.5);
            assert!(contrast(p.accent_text, p.accent) >= 4.5, "{}", contrast(p.accent_text, p.accent));
            assert!(contrast(p.danger, p.bg_panel) >= 3.0);
            // Selection and pre-selection must sit on opposite sides of the blue–yellow
            // axis, which survives red–green color blindness.
            let by = |c: Color32| f32::from(c.b()) - (f32::from(c.r()) + f32::from(c.g())) / 2.0;
            assert!(by(p.vp_select) < -40.0 && by(p.vp_preselect) > 40.0);
        }
    }
}
