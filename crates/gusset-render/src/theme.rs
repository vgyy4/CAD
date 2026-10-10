//! Viewport colors from the design tokens in `docs/DESIGN.md` §3 (the `vp-*` tokens)
//! and the lighting rig shared by the GPU shader and the CPU fallback.

/// An sRGB color given as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Srgb(pub u32);

impl Srgb {
    /// The 8-bit sRGB channels.
    #[must_use]
    pub const fn bytes(self) -> [u8; 3] {
        [(self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8]
    }

    /// Linear-light RGB in `0..=1`.
    #[must_use]
    pub fn linear(self) -> [f32; 3] {
        self.bytes().map(|b| srgb_to_linear(f32::from(b) / 255.0))
    }

    /// `#RRGGBB`.
    #[must_use]
    pub fn hex(self) -> String {
        format!("#{:06X}", self.0 & 0x00FF_FFFF)
    }
}

/// sRGB transfer function, decode.
#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// sRGB transfer function, encode.
#[must_use]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// UI theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    /// Dark theme (the default).
    #[default]
    Dark,
    /// Light theme.
    Light,
}

impl Theme {
    /// Lower-case name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// The viewport color tokens for this theme.
    #[must_use]
    pub const fn colors(self) -> ViewportColors {
        match self {
            Self::Dark => ViewportColors {
                bg_top: Srgb(0x2A2F38),
                bg_bottom: Srgb(0x1A1D22),
                preselect: Srgb(0x7FB8FF),
                select: Srgb(0xFFB000),
                edge: Srgb(0x0E1013),
            },
            Self::Light => ViewportColors {
                bg_top: Srgb(0xF4F6F9),
                bg_bottom: Srgb(0xD9DEE6),
                preselect: Srgb(0x2F7DEB),
                select: Srgb(0xD98300),
                edge: Srgb(0x20242B),
            },
        }
    }
}

/// The `vp-*` design tokens (DESIGN.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportColors {
    /// `vp-bg-top`.
    pub bg_top: Srgb,
    /// `vp-bg-bottom`.
    pub bg_bottom: Srgb,
    /// `vp-preselect` (hover highlight).
    pub preselect: Srgb,
    /// `vp-select` (selection).
    pub select: Srgb,
    /// `vp-edge` (model edges).
    pub edge: Srgb,
}

/// Default part color when a mesh has no material color. Not a DESIGN.md token yet: a
/// neutral cool grey that reads well against both themes' backgrounds and keeps the
/// selection (amber) and pre-selection (blue) colors distinct.
pub const DEFAULT_PART_COLOR: Srgb = Srgb(0xB4BCC8);

/// The lighting rig: a view-space ("matcap-style") rig, so a part is always lit from
/// the same screen direction however it is oriented. Hemispheric ambient (sky/ground),
/// a soft key light from the upper left, a fill light from the lower right, a soft
/// specular on the key and a faint rim term that separates silhouettes from the
/// background. Shared by the WGSL shader (through the uniform buffer) and
/// [`crate::SoftwareRenderer`], so both paths shade alike.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// View-space direction *towards* the key light (unit).
    pub key_dir: [f32; 3],
    /// Key diffuse strength.
    pub key: f32,
    /// View-space direction towards the fill light (unit).
    pub fill_dir: [f32; 3],
    /// Fill diffuse strength.
    pub fill: f32,
    /// Ambient from the sky (normal pointing screen-up).
    pub sky: f32,
    /// Ambient from the ground (normal pointing screen-down).
    pub ground: f32,
    /// Specular strength (white, Blinn-Phong on the key).
    pub specular: f32,
    /// Specular exponent.
    pub shininess: f32,
    /// Rim strength.
    pub rim: f32,
}

impl Lighting {
    /// The default rig.
    pub const DEFAULT: Lighting = Lighting {
        key_dir: [-0.408_248_3, 0.408_248_3, 0.816_496_6],
        key: 0.62,
        fill_dir: [0.639_602, -0.426_401_4, 0.639_602],
        fill: 0.16,
        sky: 0.42,
        ground: 0.22,
        specular: 0.22,
        shininess: 48.0,
        rim: 0.10,
    };

    /// Shade a linear base color for a unit view-space normal `n` and unit direction
    /// towards the eye `v`. Mirrors `shade()` in `shader.wgsl`.
    #[must_use]
    pub fn shade(&self, base: [f32; 3], n: [f32; 3], v: [f32; 3]) -> [f32; 3] {
        let d = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let hemi = 0.5 + 0.5 * n[1];
        let ambient = self.ground + (self.sky - self.ground) * hemi;
        let key = d(n, self.key_dir).max(0.0) * self.key;
        let fill = d(n, self.fill_dir).max(0.0) * self.fill;
        let h = [self.key_dir[0] + v[0], self.key_dir[1] + v[1], self.key_dir[2] + v[2]];
        let hl = d(h, h).sqrt().max(1e-6);
        let h = [h[0] / hl, h[1] / hl, h[2] / hl];
        let spec = d(n, h).max(0.0).powf(self.shininess) * self.specular;
        let rim = (1.0 - d(n, v).max(0.0)).powi(3) * self.rim;
        let diffuse = ambient + key + fill;
        base.map(|b| b * diffuse + spec + rim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trip() {
        for b in 0..=255_u8 {
            let c = f32::from(b) / 255.0;
            let back = (linear_to_srgb(srgb_to_linear(c)) * 255.0).round();
            assert!((back - f32::from(b)).abs() < 0.5, "{b}");
        }
        assert_eq!(Srgb(0x2A2F38).bytes(), [0x2A, 0x2F, 0x38]);
        assert_eq!(Srgb(0x2A2F38).hex(), "#2A2F38");
    }

    #[test]
    fn light_directions_are_unit() {
        let l = Lighting::DEFAULT;
        for v in [l.key_dir, l.fill_dir] {
            let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            assert!((n - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn selection_colors_differ_in_luminance() {
        // DESIGN.md §5: selection and pre-highlight differ from each other in luminance.
        let lum = |c: Srgb| {
            let l = c.linear();
            0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2]
        };
        for t in [Theme::Dark, Theme::Light] {
            let c = t.colors();
            assert!((lum(c.select) - lum(c.preselect)).abs() > 0.05, "{t:?}");
        }
    }

    /// The `vp-*` tokens must match the tables in docs/DESIGN.md (the source of truth).
    #[test]
    fn tokens_match_design_md() {
        let md = include_str!("../../../docs/DESIGN.md");
        let section = |title: &str| {
            let start = md.find(title).expect("section");
            let rest = &md[start..];
            let end = rest[1..].find("\n### ").map_or(rest.len(), |e| e + 1);
            rest[..end].to_owned()
        };
        let dark = section("### Color: dark theme");
        let light = section("### Color: light theme");
        let find = |sec: &str, token: &str| -> Vec<String> {
            let line = sec.lines().find(|l| l.contains(&format!("`{token}`"))).expect(token);
            line.split('`').filter(|s| s.starts_with('#') && s.len() == 7).map(str::to_owned).collect()
        };
        for (theme, sec) in [(Theme::Dark, dark), (Theme::Light, light)] {
            let c = theme.colors();
            assert_eq!(find(&sec, "vp-bg-top"), vec![c.bg_top.hex(), c.bg_bottom.hex()], "{theme:?}");
            assert_eq!(find(&sec, "vp-preselect"), vec![c.preselect.hex()]);
            assert_eq!(find(&sec, "vp-select"), vec![c.select.hex()]);
            assert_eq!(find(&sec, "vp-edge"), vec![c.edge.hex()]);
        }
    }
}
