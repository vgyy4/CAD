// Adapted from SolidCraft (https://github.com/bherbruck/solidcraft), Copyright (c) 2026 SolidCraft contributors, MIT OR Apache-2.0.
// The view cube adapts SolidCraft's navigation cube: nine coplanar picking patches per face
// (the middle one picks the face, the corner squares pick the corner, the edge strips the
// edge), faces shaded by how directly they face the viewer, and the hovered target
// highlighted as one. The drawing is restyled with Gusset's design tokens, and the camera
// part (named views, orbit by dragging the cube) uses gusset-render's camera. See
// docs/ATTRIBUTION-THIRD-PARTY.md.

//! The viewport's overlays (DESIGN.md §2): the view cube at the top right, with clickable
//! faces, edges and corners, and the navigation bar at the bottom centre (orbit, pan,
//! zoom, fit, home, projection, edges).

use crate::icons::{self, Ink, Tool};
use crate::theme::{self, Palette};
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2, WidgetInfo, WidgetType};
use gusset_render::math::{Vec3, dot};
use gusset_render::{Camera, NamedView};

/// Side of the view cube's box (logical px).
pub const CUBE_BOX: f32 = 112.0;
/// Margin between the cube and the viewport's corner.
const MARGIN: f32 = 16.0;
/// Screen size of a cube half-edge: the cube's longest diagonal then still fits the box.
const HALF: f32 = 32.0;
/// Where the corner squares and edge strips end on a face (face coordinates run -1..1).
const INSET: f32 = 0.62;

/// Where a click on the cube sends the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CubeTarget {
    /// One of the six standard views.
    Named(NamedView),
    /// Look at the model from this direction (an edge or a corner of the cube: the
    /// components are -1, 0 or +1 along x, y, z; the eye sits on that side).
    Dir(Vec3),
}

impl CubeTarget {
    /// The target for an eye direction `d` (components -1, 0, +1).
    pub fn from_dir(d: Vec3) -> CubeTarget {
        let key = d.map(|v| v.round() as i32);
        match key {
            [0, -1, 0] => CubeTarget::Named(NamedView::Front),
            [0, 1, 0] => CubeTarget::Named(NamedView::Back),
            [-1, 0, 0] => CubeTarget::Named(NamedView::Left),
            [1, 0, 0] => CubeTarget::Named(NamedView::Right),
            [0, 0, 1] => CubeTarget::Named(NamedView::Top),
            [0, 0, -1] => CubeTarget::Named(NamedView::Bottom),
            [1, -1, 1] => CubeTarget::Named(NamedView::Iso),
            [-1, 1, 1] => CubeTarget::Named(NamedView::IsoBack),
            _ => CubeTarget::Dir(d),
        }
    }
}

/// "Top", "Front, Right", ... for an eye direction (the tooltip).
pub fn direction_label(d: Vec3) -> String {
    [(d[2], "Bottom", "Top"), (d[1], "Front", "Back"), (d[0], "Left", "Right")]
        .into_iter()
        .filter_map(|(v, neg, pos)| match v.round() as i32 {
            -1 => Some(neg),
            1 => Some(pos),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A cube face turned towards the viewer.
struct Face {
    /// The face's outward normal.
    normal: Vec3,
    /// Screen corners.
    quad: [Pos2; 4],
    /// How directly it looks at the viewer (0..1).
    facing: f32,
    centre: Pos2,
}

/// One picking patch of a visible face.
struct Patch {
    /// Eye direction a click on it selects.
    dir: Vec3,
    /// `dir` rounded to -1, 0, +1 (for comparing).
    key: [i32; 3],
    /// Screen corners.
    pts: [Pos2; 4],
    /// Index into the faces.
    face: usize,
}

fn project(cam: &Camera, v: Vec3, c: Pos2) -> Pos2 {
    let (r, u) = (cam.right(), cam.up);
    c + Vec2::new(dot(v, r) as f32, -dot(v, u) as f32) * HALF
}

/// The faces turned towards the viewer (far to near) and their nine patches each.
fn faces_and_patches(cam: &Camera, c: Pos2) -> (Vec<Face>, Vec<Patch>) {
    // (normal, tangent r, tangent u)
    let defs: [(Vec3, Vec3, Vec3); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ];
    let eye_dir = cam.forward.map(|v| -v);
    let at = |n: Vec3, r: Vec3, u: Vec3, x: f32, y: f32| -> Vec3 {
        let (x, y) = (f64::from(x), f64::from(y));
        [n[0] + r[0] * x + u[0] * y, n[1] + r[1] * x + u[1] * y, n[2] + r[2] * x + u[2] * y]
    };
    let mut visible: Vec<(f32, Vec3, Vec3, Vec3)> = defs
        .into_iter()
        .map(|(n, r, u)| (dot(n, eye_dir) as f32, n, r, u))
        .filter(|(facing, ..)| *facing > 1e-3)
        .collect();
    visible.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut faces = Vec::new();
    let mut patches = Vec::new();
    for (facing, n, r, u) in visible {
        let quad =
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| project(cam, at(n, r, u, x, y), c));
        faces.push(Face { normal: n, quad, facing, centre: project(cam, n, c) });
        let bands = [(-1.0, -INSET, -1.0), (-INSET, INSET, 0.0), (INSET, 1.0, 1.0)];
        for (x0, x1, sx) in bands {
            for (y0, y1, sy) in bands {
                let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| project(cam, at(n, r, u, x, y), c));
                let dir = at(n, r, u, sx, sy);
                patches.push(Patch { dir, key: dir.map(|v| v.round() as i32), pts, face: faces.len() - 1 });
            }
        }
    }
    (faces, patches)
}

/// Whether `p` lies inside the convex quad.
fn contains(pts: &[Pos2; 4], p: Pos2) -> bool {
    let mut sign = 0.0_f32;
    for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
        let cross = (*b - *a).x * (p - *a).y - (*b - *a).y * (p - *a).x;
        if cross.abs() > 1e-3 {
            if sign != 0.0 && sign.signum() != cross.signum() {
                return false;
            }
            sign = cross;
        }
    }
    sign != 0.0
}

/// What the cube did this frame.
#[derive(Debug)]
pub struct CubeOutput {
    /// A face, edge or corner was clicked.
    pub clicked: Option<CubeTarget>,
    /// The cube was dragged: orbit by this many pixels.
    pub orbit: Option<Vec2>,
    /// The cube's box (the form window stays below it).
    pub rect: Rect,
}

/// The box of the view cube in a viewport `rect`.
pub fn cube_rect(rect: Rect) -> Rect {
    Rect::from_min_size(Pos2::new(rect.right() - MARGIN - CUBE_BOX, rect.top() + MARGIN), Vec2::splat(CUBE_BOX))
}

fn shade(c: Color32, k: f32) -> Color32 {
    let m = |v: u8| (f32::from(v) * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Draws the view cube and handles its input.
pub fn view_cube(ui: &mut egui::Ui, viewport: Rect, cam: &Camera, pal: &Palette) -> CubeOutput {
    let boxr = cube_rect(viewport);
    let c = boxr.center();
    let id = ui.id().with("view-cube");
    let resp = ui.interact(boxr, id, Sense::click_and_drag());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "View cube"));
    let (faces, patches) = faces_and_patches(cam, c);
    let hover = resp.hover_pos().filter(|_| !resp.dragged());
    // The patch under the pointer (patches of different faces never overlap on screen
    // except at shared edges, so the first hit is the one).
    let hovered = hover.and_then(|p| patches.iter().find(|q| contains(&q.pts, p)));
    let hovered_key = hovered.map(|q| q.key);
    let painter = ui.painter_at(boxr.expand(2.0));
    let mut fills = egui::Mesh::default();
    for q in &patches {
        let lit = hovered_key == Some(q.key);
        let facing = faces.get(q.face).map_or(1.0, |f| f.facing);
        let col = if lit { pal.accent.gamma_multiply(0.85) } else { shade(pal.bg_raised, 0.8 + 0.2 * facing) };
        let o = fills.vertices.len() as u32;
        for p in q.pts {
            fills.colored_vertex(p, col);
        }
        fills.add_triangle(o, o + 1, o + 2);
        fills.add_triangle(o, o + 2, o + 3);
    }
    painter.add(Shape::mesh(fills));
    // The cube's edges, and the hovered target's outline.
    for f in &faces {
        painter.add(Shape::closed_line(f.quad.to_vec(), Stroke::new(1.0, pal.text_muted)));
    }
    if let Some(h) = hovered {
        for q in patches.iter().filter(|p| p.key == h.key) {
            painter.add(Shape::closed_line(q.pts.to_vec(), Stroke::new(1.5, pal.accent)));
        }
    }
    // Face labels (flat, fading as the face turns edge-on).
    for f in &faces {
        let opacity = ((f.facing - 0.3) / 0.3).clamp(0.0, 1.0);
        if opacity <= 0.0 {
            continue;
        }
        let lit = hovered_key == Some(f.normal.map(|v| v.round() as i32));
        let col = if lit { pal.accent_text } else { pal.text };
        painter.text(
            f.centre,
            egui::Align2::CENTER_CENTER,
            direction_label(f.normal).to_uppercase(),
            egui::FontId::proportional(theme::FONT_CAPTION - 1.0),
            col.gamma_multiply(opacity),
        );
    }
    let mut out = CubeOutput { clicked: None, orbit: None, rect: boxr };
    if let Some(h) = hovered {
        resp.clone().on_hover_text(direction_label(h.dir));
    }
    if resp.clicked()
        && let Some(h) = hovered
    {
        out.clicked = Some(CubeTarget::from_dir(h.dir));
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        if d != Vec2::ZERO {
            out.orbit = Some(d);
        }
    }
    out
}

/// The navigation bar's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavButton {
    Orbit,
    Pan,
    Zoom,
    Fit,
    Home,
    Projection,
    Edges,
}

/// What the navigation bar shows as active.
#[derive(Debug, Clone, Copy)]
pub struct NavState {
    pub mode: NavMode,
    pub orthographic: bool,
    pub edges: bool,
}

/// What a plain drag in the viewport does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavMode {
    /// Pick (the default): drags do nothing, clicks select.
    #[default]
    Select,
    Orbit,
    Pan,
    Zoom,
}

impl NavButton {
    const ALL: [NavButton; 7] = [
        NavButton::Orbit,
        NavButton::Pan,
        NavButton::Zoom,
        NavButton::Fit,
        NavButton::Home,
        NavButton::Projection,
        NavButton::Edges,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NavButton::Orbit => "Orbit",
            NavButton::Pan => "Pan",
            NavButton::Zoom => "Zoom",
            NavButton::Fit => "Fit all",
            NavButton::Home => "Home view",
            NavButton::Projection => "Orthographic view",
            NavButton::Edges => "Show edges",
        }
    }

    fn help(self) -> &'static str {
        match self {
            NavButton::Orbit => "Orbit: drag to turn the view (also right-drag)",
            NavButton::Pan => "Pan: drag to move the view (also Shift + right-drag)",
            NavButton::Zoom => "Zoom: drag up and down (also the scroll wheel)",
            NavButton::Fit => "Fit all: frame the whole model",
            NavButton::Home => "Home: isometric view, framed",
            NavButton::Projection => "Switch between perspective and orthographic",
            NavButton::Edges => "Show or hide model edges",
        }
    }

    fn tool(self) -> Tool {
        match self {
            NavButton::Orbit => Tool::Orbit,
            NavButton::Pan => Tool::Pan,
            NavButton::Zoom => Tool::Zoom,
            NavButton::Fit => Tool::Fit,
            NavButton::Home => Tool::Home,
            NavButton::Projection => Tool::Perspective,
            NavButton::Edges => Tool::Edges,
        }
    }

    fn active(self, s: &NavState) -> Option<bool> {
        match self {
            NavButton::Orbit => Some(s.mode == NavMode::Orbit),
            NavButton::Pan => Some(s.mode == NavMode::Pan),
            NavButton::Zoom => Some(s.mode == NavMode::Zoom),
            NavButton::Projection => Some(s.orthographic),
            NavButton::Edges => Some(s.edges),
            NavButton::Fit | NavButton::Home => None,
        }
    }
}

const BTN: f32 = 32.0;
const BAR_PAD: f32 = 4.0;
const GROUP_GAP: f32 = 8.0;

/// The navigation bar's box in a viewport `rect`.
pub fn nav_rect(rect: Rect) -> Rect {
    let groups = 3.0;
    let w = NavButton::ALL.len() as f32 * BTN + (groups - 1.0) * GROUP_GAP + 2.0 * BAR_PAD;
    Rect::from_min_size(
        Pos2::new(rect.center().x - w / 2.0, rect.bottom() - 16.0 - BTN - 2.0 * BAR_PAD),
        Vec2::new(w, BTN + 2.0 * BAR_PAD),
    )
}

/// Draws the navigation bar; returns the button that was clicked.
pub fn nav_bar(ui: &mut egui::Ui, viewport: Rect, state: NavState, pal: &Palette) -> Option<NavButton> {
    let bar = nav_rect(viewport);
    let painter = ui.painter_at(bar.expand(2.0));
    painter.rect(
        bar,
        theme::RADIUS_LG,
        pal.bg_raised.gamma_multiply(0.94),
        Stroke::new(1.0, pal.border),
        egui::StrokeKind::Inside,
    );
    let mut clicked = None;
    let mut x = bar.left() + BAR_PAD;
    for (i, b) in NavButton::ALL.into_iter().enumerate() {
        if i == 3 || i == 5 {
            x += GROUP_GAP;
            let sep_x = x - GROUP_GAP / 2.0;
            painter.line_segment(
                [Pos2::new(sep_x, bar.top() + 8.0), Pos2::new(sep_x, bar.bottom() - 8.0)],
                Stroke::new(1.0, pal.border),
            );
        }
        let r = Rect::from_min_size(Pos2::new(x, bar.top() + BAR_PAD), Vec2::splat(BTN));
        x += BTN;
        let resp = ui.interact(r, ui.id().with(("nav", i)), Sense::click());
        let active = b.active(&state);
        resp.widget_info(|| match active {
            Some(on) => WidgetInfo::selected(WidgetType::Button, true, on, b.label()),
            None => WidgetInfo::labeled(WidgetType::Button, true, b.label()),
        });
        if active == Some(true) {
            painter.rect_filled(r, theme::RADIUS_MD, pal.accent.gamma_multiply(0.25));
        } else if resp.hovered() {
            painter.rect_filled(r, theme::RADIUS_MD, pal.bg_hover);
        }
        icons::paint_tool(&painter, r.shrink(5.0), b.tool(), Ink::new(pal));
        let resp = resp.on_hover_text(b.help());
        if resp.clicked() {
            clicked = Some(b);
        }
    }
    clicked
}

/// Where the center of a cube face (normal `n`) is drawn when the cube is centered at `c`.
#[cfg(test)]
pub(crate) fn face_centre(cam: &Camera, c: Pos2, n: Vec3) -> Pos2 {
    project(cam, n, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centroid(p: &Patch) -> Pos2 {
        let sum = p.pts.iter().fold(Vec2::ZERO, |s, q| s + q.to_vec2());
        (sum / 4.0).to_pos2()
    }

    #[test]
    fn every_visible_patch_picks_its_own_target() {
        let cam = Camera::default(); // isometric
        let (faces, patches) = faces_and_patches(&cam, Pos2::new(50.0, 50.0));
        assert_eq!(faces.len(), 3, "an isometric view shows three faces");
        assert_eq!(patches.len(), 27);
        for p in &patches {
            let hit = patches.iter().find(|q| contains(&q.pts, centroid(p)));
            assert_eq!(hit.map(|q| q.dir), Some(p.dir));
        }
    }

    #[test]
    fn targets_are_faces_edges_and_corners() {
        let cam = Camera::default();
        let (_, patches) = faces_and_patches(&cam, Pos2::ZERO);
        let kinds = |n: usize| patches.iter().filter(|p| p.dir.iter().filter(|v| v.abs() > 0.5).count() == n).count();
        // Per face: 1 middle, 4 edge strips, 4 corner squares.
        assert_eq!((kinds(1), kinds(2), kinds(3)), (3, 12, 12));
        assert_eq!(CubeTarget::from_dir([0.0, 0.0, 1.0]), CubeTarget::Named(NamedView::Top));
        assert_eq!(CubeTarget::from_dir([0.0, -1.0, 0.0]), CubeTarget::Named(NamedView::Front));
        assert_eq!(CubeTarget::from_dir([1.0, -1.0, 1.0]), CubeTarget::Named(NamedView::Iso));
        assert_eq!(CubeTarget::from_dir([1.0, 1.0, 0.0]), CubeTarget::Dir([1.0, 1.0, 0.0]));
        assert_eq!(direction_label([1.0, -1.0, 1.0]), "Top, Front, Right");
        assert_eq!(direction_label([0.0, 0.0, -1.0]), "Bottom");
    }

    #[test]
    fn faces_turn_with_the_camera() {
        let mut cam = Camera::default();
        cam.set_view(NamedView::Top);
        let (faces, _) = faces_and_patches(&cam, Pos2::ZERO);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces.first().map(|f| f.normal), Some([0.0, 0.0, 1.0]));
        cam.set_view(NamedView::Back);
        let (faces, _) = faces_and_patches(&cam, Pos2::ZERO);
        assert_eq!(faces.first().map(|f| f.normal), Some([0.0, 1.0, 0.0]));
    }
}
