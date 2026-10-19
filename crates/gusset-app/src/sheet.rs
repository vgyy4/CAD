//! The Drawing tab's sheet (ADR 0032 phase 3): the drawing's scene painted with egui on
//! white paper, panned and zoomed like the viewport (wheel zooms about the pointer, a
//! drag on the paper or with the middle button pans, double-click fits). Dragging a base
//! view, a note or a dimension's text moves it; projected views follow their parent when
//! the drawing rebuilds.
//!
//! The widget only reports what was moved ([`Out`]); the app sends the command.

use crate::theme::Palette;
use egui::{Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use gusset_drawing::{Anchor, Prim, Scene, Style};

/// Pan and zoom, and a view being dragged.
#[derive(Debug, Default)]
pub struct State {
    /// Pixels per sheet millimetre.
    zoom: f32,
    /// Screen offset of the sheet's top-left corner from the widget's top-left.
    offset: Vec2,
    /// The drawing the view was fitted for (a new drawing is fitted again).
    fitted: Option<String>,
    drag: Option<Drag>,
    /// The last frame's mapping: sheet origin on screen, zoom, sheet height.
    last: Option<(Pos2, f32, f64)>,
}

#[derive(Debug)]
struct Drag {
    /// The item being moved, or `None` while panning.
    view: Option<String>,
    /// The point that places it (sheet mm) when the drag started.
    center: [f64; 2],
    /// Total movement in sheet mm.
    moved: [f64; 2],
    grab: Option<Grab>,
}

/// Something that can be moved: a base view, a note or a dimension's text.
#[derive(Debug, Clone)]
struct Grab {
    name: String,
    at: [f64; 2],
    min: [f64; 2],
    max: [f64; 2],
}

/// What the user did.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    /// A base view, note or dimension text dropped at a new place (sheet mm, as
    /// `drawing.move_item` takes it).
    pub moved: Option<(String, [f64; 2])>,
}

impl State {
    /// Where sheet point `p` (mm) was drawn in the last frame.
    pub fn screen(&self, p: [f64; 2]) -> Option<Pos2> {
        let (origin, zoom, height) = self.last?;
        Some(Map { origin, zoom, height }.to(p))
    }

    fn fit(&mut self, rect: Rect, size: [f64; 2]) {
        let (w, h) = (size[0] as f32, size[1] as f32);
        self.zoom = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).max(0.05);
        self.offset = Vec2::new((rect.width() - w * self.zoom) / 2.0, (rect.height() - h * self.zoom) / 2.0);
    }
}

/// Sheet mm to screen and back for one frame.
struct Map {
    origin: Pos2,
    zoom: f32,
    height: f64,
}

impl Map {
    fn to(&self, p: [f64; 2]) -> Pos2 {
        Pos2::new(self.origin.x + p[0] as f32 * self.zoom, self.origin.y + (self.height - p[1]) as f32 * self.zoom)
    }

    fn from(&self, p: Pos2) -> [f64; 2] {
        [f64::from((p.x - self.origin.x) / self.zoom), self.height - f64::from((p.y - self.origin.y) / self.zoom)]
    }
}

fn inside(min: [f64; 2], max: [f64; 2], p: [f64; 2]) -> bool {
    p[0] >= min[0] && p[0] <= max[0] && p[1] >= min[1] && p[1] <= max[1]
}

/// Draws drawing `name` (its `scene`) and handles panning, zooming and view dragging.
/// `base_views` are the views that can be moved.
pub fn show(
    ui: &mut Ui,
    pal: &Palette,
    name: &str,
    scene: Option<&Scene>,
    base_views: &[String],
    st: &mut State,
) -> Out {
    let mut out = Out::default();
    let rect = ui.max_rect();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.bg_app);
    let Some(scene) = scene else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Building the drawing…",
            FontId::proportional(14.0),
            pal.text_muted,
        );
        return out;
    };
    if st.fitted.as_deref() != Some(name) || st.zoom <= 0.0 {
        st.fit(rect, scene.size);
        st.fitted = Some(name.to_string());
    }
    if resp.double_clicked() {
        st.fit(rect, scene.size);
    }
    // Zoom about the pointer.
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
        if scroll != 0.0
            && let Some(p) = resp.hover_pos()
        {
            let f = (scroll * 0.002).exp();
            let new = (st.zoom * f).clamp(0.05, 200.0);
            let k = new / st.zoom;
            let rel = p - rect.min - st.offset;
            st.offset = p - rect.min - rel * k;
            st.zoom = new;
        }
    }
    let map = Map { origin: rect.min + st.offset, zoom: st.zoom, height: scene.size[1] };
    st.last = Some((map.origin, map.zoom, map.height));
    let movable = |v: &&gusset_drawing::PlacedView| v.error.is_none() && base_views.contains(&v.name);
    // A drag picks what was under the pointer when the button went down (the drag only
    // starts after the pointer has moved a few pixels).
    let press = ui.input(|i| i.pointer.press_origin()).filter(|_| resp.drag_started());
    let pointer = press.or(resp.hover_pos()).or(resp.interact_pointer_pos()).map(|p| map.from(p));
    // What a drag would move: a note or dimension text before the view around it.
    let hovered: Option<Grab> = pointer.and_then(|p| {
        scene
            .handles
            .iter()
            .rev()
            .find(|h| inside(h.min, h.max, p))
            .map(|h| Grab { name: h.item.clone(), at: h.at, min: h.min, max: h.max })
            .or_else(|| {
                scene.views.iter().filter(movable).find(|v| inside(v.min, v.max, p)).map(|v| Grab {
                    name: v.name.clone(),
                    at: [(v.min[0] + v.max[0]) / 2.0, (v.min[1] + v.max[1]) / 2.0],
                    min: v.min,
                    max: v.max,
                })
            })
    });

    // Drags: the item under the pointer moves, anything else pans.
    let middle = ui.input(|i| i.pointer.middle_down());
    if resp.drag_started() {
        let grab = if middle { None } else { hovered.clone() };
        st.drag = Some(Drag {
            view: grab.as_ref().map(|g| g.name.clone()),
            center: grab.as_ref().map_or([0.0; 2], |g| g.at),
            moved: [0.0; 2],
            grab,
        });
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        match st.drag.as_mut() {
            Some(Drag { view: Some(_), moved, .. }) => {
                moved[0] += f64::from(d.x / st.zoom);
                moved[1] -= f64::from(d.y / st.zoom);
            }
            _ => st.offset += d,
        }
    }
    if resp.drag_stopped()
        && let Some(Drag { view: Some(v), center, moved, .. }) = st.drag.take()
        && (moved[0].abs() > 1e-3 || moved[1].abs() > 1e-3)
    {
        // Whole tenths of a millimetre keep the document text short.
        let r = |x: f64| (x * 10.0).round() / 10.0;
        out.moved = Some((v, [r(center[0] + moved[0]), r(center[1] + moved[1])]));
    } else if !resp.dragged() {
        st.drag = None;
    }

    // Paper with a shadow.
    let paper = Rect::from_two_pos(map.to([0.0, 0.0]), map.to(scene.size));
    painter.rect_filled(paper.translate(Vec2::splat(3.0)), 0.0, Color32::from_black_alpha(60));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    paint(&painter, &map, scene);

    // The item under the pointer, and the one being dragged where it would land.
    if let Some(g) = &hovered
        && st.drag.is_none()
    {
        painter.rect_stroke(
            Rect::from_two_pos(map.to(g.min), map.to(g.max)).expand(3.0),
            2.0,
            Stroke::new(1.0, pal.accent),
            egui::StrokeKind::Outside,
        );
        resp.clone().on_hover_text(format!("{}: drag to move", g.name));
    }
    if let Some(Drag { grab: Some(g), moved, .. }) = &st.drag {
        let shift = |p: [f64; 2]| map.to([p[0] + moved[0], p[1] + moved[1]]);
        painter.rect_stroke(
            Rect::from_two_pos(shift(g.min), shift(g.max)).expand(3.0),
            2.0,
            Stroke::new(1.5, pal.accent),
            egui::StrokeKind::Outside,
        );
    }
    for (i, issue) in scene.issues.iter().take(3).enumerate() {
        painter.text(
            rect.left_bottom() + Vec2::new(10.0, -10.0 - 16.0 * i as f32),
            egui::Align2::LEFT_BOTTOM,
            issue,
            FontId::proportional(12.0),
            pal.danger,
        );
    }
    out
}

/// Paints every item of the scene.
/// The part of segment `a`-`b` inside `r` (Liang-Barsky), or `None`.
fn clip(a: Pos2, b: Pos2, r: Rect) -> Option<(Pos2, Pos2)> {
    if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
        return None;
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x - r.min.x), (d.x, r.max.x - a.x), (-d.y, a.y - r.min.y), (d.y, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

fn paint(painter: &egui::Painter, map: &Map, scene: &Scene) {
    let view = painter.clip_rect().expand(8.0);
    let ink = Color32::from_gray(20);
    let warn = Color32::from_rgb(176, 48, 96);
    for item in &scene.items {
        let color = if item.style == Style::Dangling { warn } else { ink };
        let stroke = Stroke::new((item.style.width() as f32 * map.zoom).max(0.75), color);
        let dashes = item.style.dashes();
        let line = |pts: Vec<Pos2>| -> Vec<Shape> {
            match dashes {
                [] => vec![Shape::line(pts, stroke)],
                [dash, gap, ..] => {
                    // Dashes are cut only where the line is on screen (review D1).
                    let (d, g) = ((*dash as f32 * map.zoom).max(2.0), (*gap as f32 * map.zoom).max(1.5));
                    pts.windows(2)
                        .filter_map(|w| match w {
                            [a, b] => clip(*a, *b, view),
                            _ => None,
                        })
                        .flat_map(|(a, b)| Shape::dashed_line(&[a, b], stroke, d, g))
                        .collect()
                }
                [_] => vec![Shape::line(pts, stroke)],
            }
        };
        match &item.prim {
            Prim::Line { a, b } => painter.extend(line(vec![map.to(*a), map.to(*b)])),
            Prim::Arc { center, radius, start, end } => {
                let r = *radius as f32 * map.zoom;
                let n = (r / 2.0).clamp(8.0, 256.0) as usize;
                let sweep = (end - start).min(std::f64::consts::TAU);
                let pts = (0..=n)
                    .map(|i| {
                        let a = start + sweep * i as f64 / n as f64;
                        map.to([center[0] + radius * a.cos(), center[1] + radius * a.sin()])
                    })
                    .collect();
                painter.extend(line(pts));
            }
            Prim::Polyline { points, closed } => {
                let mut pts: Vec<Pos2> = points.iter().map(|p| map.to(*p)).collect();
                if *closed && let Some(&f) = pts.first() {
                    pts.push(f);
                }
                painter.extend(line(pts));
            }
            Prim::Fill { points } => {
                painter.add(Shape::convex_polygon(points.iter().map(|p| map.to(*p)).collect(), color, Stroke::NONE));
            }
            Prim::Text { at, height, text, anchor, angle } => {
                // The font size is the em; capitals are about 0.7 of it.
                let size = *height as f32 / 0.7 * map.zoom;
                if size < 2.0 {
                    continue;
                }
                let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(size), color);
                let w = galley.size().x;
                let along = match anchor {
                    Anchor::Left => 0.0,
                    Anchor::Center => -w / 2.0,
                    Anchor::Right => -w,
                };
                // From the baseline anchor to the galley's top-left, in the text's frame
                // (screen Y points down, so the sheet angle turns the other way).
                let a = -(*angle as f32);
                let (s, c) = a.sin_cos();
                let off = Vec2::new(along, -size * 0.8);
                let pos = map.to(*at) + Vec2::new(off.x * c - off.y * s, off.x * s + off.y * c);
                painter.add(egui::epaint::TextShape::new(pos, galley, color).with_angle(a));
            }
        }
    }
}
