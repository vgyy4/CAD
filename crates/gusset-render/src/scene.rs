//! The renderable scene: meshes by id with transform, material, visibility and
//! per-face / per-edge highlights. Pure CPU data; renderers sync from it on demand.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::math::{self, Aabb, Mat4};
use crate::mesh::{MeshData, MeshError};
use crate::theme::{DEFAULT_PART_COLOR, Srgb};

/// Caller-chosen mesh id (e.g. a body id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshId(pub u64);

/// Surface appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Material {
    /// Base color (sRGB).
    pub color: Srgb,
    /// How the path tracer treats the surface (the viewport shades every finish alike).
    pub finish: Finish,
}

impl Default for Material {
    fn default() -> Self {
        Material { color: DEFAULT_PART_COLOR, finish: Finish::Plastic }
    }
}

/// Surface finish (ADR 0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Finish {
    /// Diffuse under a thin clear coat.
    #[default]
    Plastic,
    /// Reflects in its own colour, slightly rough.
    Metal,
    /// Clear, refracting; tinted by the colour.
    Glass,
    /// Diffuse only.
    Rubber,
}

impl Finish {
    /// From the document's keyword (`plastic`, `metal`, `glass`, `rubber`).
    #[must_use]
    pub fn from_keyword(k: &str) -> Option<Finish> {
        match k {
            "plastic" => Some(Finish::Plastic),
            "metal" => Some(Finish::Metal),
            "glass" => Some(Finish::Glass),
            "rubber" => Some(Finish::Rubber),
            _ => None,
        }
    }
}

#[cfg(test)]
impl Finish {
    pub(crate) const ALL_FOR_TESTS: [Finish; 4] = [Finish::Plastic, Finish::Metal, Finish::Glass, Finish::Rubber];
}

/// Highlight state of a face or edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Highlight {
    /// Not highlighted.
    #[default]
    None,
    /// Hover / pre-selection (`vp-preselect`).
    Preselect,
    /// Selected (`vp-select`). Wins over pre-selection.
    Select,
}

/// One mesh in the scene.
#[derive(Debug, Clone)]
pub struct SceneMesh {
    pub(crate) data: Arc<MeshData>,
    /// Unique per geometry upload; renderers cache GPU buffers by it.
    pub(crate) geometry_rev: u64,
    pub(crate) transform: Mat4,
    pub(crate) material: Material,
    pub(crate) visible: bool,
    pub(crate) face_highlights: BTreeMap<u32, Highlight>,
    pub(crate) edge_highlights: BTreeMap<u32, Highlight>,
    /// Colours of single faces (a STEP file's face colours), over the material's colour.
    pub(crate) face_colors: BTreeMap<u32, Srgb>,
    /// The inverse transform and the world bounds, kept with the transform so a ray test
    /// against many meshes rejects most of them cheaply.
    pub(crate) inverse: Option<Mat4>,
    pub(crate) world: Aabb,
    /// A coarse copy for when the body is drawn small (lod.rs).
    pub(crate) lod: Option<Arc<MeshData>>,
}

/// One mesh's display geometry (data, BVH and coarse copy), built once and shareable:
/// a caller that rebuilds its scene puts the geometry of unchanged meshes back with
/// [`Scene::insert_geometry`], which skips the build and lets GPU renderers keep their
/// buffers (they cache by the geometry's revision).
#[derive(Debug, Clone)]
pub struct Geometry {
    data: Arc<MeshData>,
    lod: Option<Arc<MeshData>>,
    rev: u64,
}

impl Geometry {
    /// Validates and builds the display geometry of a mesh.
    ///
    /// # Errors
    /// The mesh is invalid (see [`MeshError`]).
    pub fn from_mesh(mesh: &gusset_kernel::Mesh) -> Result<Geometry, MeshError> {
        let data = Arc::new(MeshData::from_mesh(mesh)?);
        let lod = crate::lod::coarse(mesh).and_then(|c| MeshData::from_mesh(&c).ok()).map(Arc::new);
        Ok(Geometry { data, lod, rev: next_geometry_rev() })
    }

    /// The display data.
    #[must_use]
    pub fn data(&self) -> &MeshData {
        &self.data
    }
}

impl SceneMesh {
    fn refresh(&mut self) {
        self.inverse = math::invert(&self.transform);
        self.world = self.data.bounds.transformed(&self.transform);
    }

    /// The compact mesh data.
    #[must_use]
    pub fn data(&self) -> &MeshData {
        &self.data
    }

    /// The mesh's edge segments in world space (pairs of points).
    #[must_use]
    pub fn world_edges(&self) -> Vec<[f32; 3]> {
        self.data()
            .edge_points
            .iter()
            .map(|p| crate::math::transform_point(self.transform(), p.map(f64::from)).map(|c| c as f32))
            .collect()
    }

    /// Object → world transform.
    #[must_use]
    pub fn transform(&self) -> &Mat4 {
        &self.transform
    }

    /// Material.
    #[must_use]
    pub fn material(&self) -> Material {
        self.material
    }

    /// Visibility.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The colour face `face` is drawn in: its own colour, else the material's.
    #[must_use]
    pub fn face_color(&self, face: u32) -> Srgb {
        self.face_colors.get(&face).copied().unwrap_or(self.material.color)
    }

    /// Highlight of face `face`.
    #[must_use]
    pub fn face_highlight(&self, face: u32) -> Highlight {
        self.face_highlights.get(&face).copied().unwrap_or_default()
    }

    /// Highlight of edge `edge`.
    #[must_use]
    pub fn edge_highlight(&self, edge: u32) -> Highlight {
        self.edge_highlights.get(&edge).copied().unwrap_or_default()
    }

    /// World-space bounds.
    #[must_use]
    pub fn world_bounds(&self) -> Aabb {
        self.data.bounds.transformed(&self.transform)
    }

    /// Split `0..count` items (triangles or segments) into runs with one highlight
    /// each, given per-element ranges and a highlight map. Used to draw highlighted
    /// faces/edges with extra draw calls instead of per-vertex attributes.
    pub(crate) fn runs(
        ranges: &[std::ops::Range<u32>],
        highlights: &BTreeMap<u32, Highlight>,
        count: u32,
    ) -> Vec<(std::ops::Range<u32>, Highlight)> {
        let mut out: Vec<(std::ops::Range<u32>, Highlight)> = Vec::new();
        let mut cursor = 0;
        for (&idx, &h) in highlights {
            if h == Highlight::None {
                continue;
            }
            let Some(r) = ranges.get(idx as usize) else { continue };
            if r.start > cursor {
                out.push((cursor..r.start, Highlight::None));
            }
            if r.end > r.start {
                out.push((r.clone(), h));
            }
            cursor = cursor.max(r.end);
        }
        if count > cursor {
            out.push((cursor..count, Highlight::None));
        }
        out
    }
}

/// The scene: an ordered map of meshes.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    meshes: BTreeMap<MeshId, SceneMesh>,
    /// Top-level BVH over the visible meshes' world boxes, built on the first ray after a
    /// change that moves, shows, hides, adds or removes a mesh.
    top: std::sync::OnceLock<crate::top::TopBvh>,
}

/// Geometry revisions are unique in the process, not per scene: GPU renderers cache
/// uploads by (mesh id, revision), and callers that build a fresh scene per frame (the
/// MCP server) reuse mesh ids. Per-scene counters restarted at 1, so a larger mesh hit
/// the smaller cached buffers and the edge pass sliced past their end (dogfooding F3-1).
fn next_geometry_rev() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Scene {
    /// An empty scene.
    #[must_use]
    pub fn new() -> Scene {
        Scene::default()
    }

    /// Add a mesh, or replace the geometry of an existing one (keeping its transform,
    /// material and visibility; highlights are cleared because face indices may have
    /// changed).
    ///
    /// # Errors
    /// The mesh is invalid (see [`MeshError`]); the scene is unchanged.
    pub fn insert_mesh(&mut self, id: MeshId, mesh: &gusset_kernel::Mesh) -> Result<(), MeshError> {
        self.insert_geometry(id, Geometry::from_mesh(mesh)?);
        Ok(())
    }

    /// The shareable geometry of a mesh, for [`Scene::insert_geometry`] into a later scene.
    #[must_use]
    pub fn geometry(&self, id: MeshId) -> Option<Geometry> {
        self.meshes.get(&id).map(|m| Geometry { data: m.data.clone(), lod: m.lod.clone(), rev: m.geometry_rev })
    }

    /// [`Scene::insert_mesh`] with geometry built before (no validation or BVH build).
    pub fn insert_geometry(&mut self, id: MeshId, geometry: Geometry) {
        self.top = std::sync::OnceLock::new();
        let Geometry { data, lod, rev } = geometry;
        match self.meshes.get_mut(&id) {
            Some(m) => {
                m.data = data;
                m.lod = lod;
                m.geometry_rev = rev;
                m.face_highlights.clear();
                m.edge_highlights.clear();
                m.face_colors.clear();
                m.refresh();
            }
            None => {
                let mut m = SceneMesh {
                    data,
                    geometry_rev: rev,
                    transform: math::IDENTITY,
                    material: Material::default(),
                    visible: true,
                    face_highlights: BTreeMap::new(),
                    edge_highlights: BTreeMap::new(),
                    face_colors: BTreeMap::new(),
                    inverse: None,
                    world: Aabb::EMPTY,
                    lod,
                };
                m.refresh();
                self.meshes.insert(id, m);
            }
        }
    }

    /// Remove a mesh. Returns whether it existed.
    pub fn remove_mesh(&mut self, id: MeshId) -> bool {
        self.top = std::sync::OnceLock::new();
        self.meshes.remove(&id).is_some()
    }

    /// Remove everything.
    pub fn clear(&mut self) {
        self.top = std::sync::OnceLock::new();
        self.meshes.clear();
    }

    /// The top-level BVH (built on first use after a change).
    pub(crate) fn top(&self) -> &crate::top::TopBvh {
        self.top.get_or_init(|| {
            crate::top::TopBvh::build(
                self.meshes.iter().filter(|(_, m)| m.visible).map(|(id, m)| (*id, m.world)).collect(),
            )
        })
    }

    /// A mesh by id.
    #[must_use]
    pub fn mesh(&self, id: MeshId) -> Option<&SceneMesh> {
        self.meshes.get(&id)
    }

    /// All meshes in id order.
    pub fn meshes(&self) -> impl Iterator<Item = (MeshId, &SceneMesh)> {
        self.meshes.iter().map(|(k, v)| (*k, v))
    }

    /// Number of meshes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    /// Whether the scene has no meshes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    /// Set a mesh's object → world transform. Returns whether the mesh exists.
    pub fn set_transform(&mut self, id: MeshId, transform: Mat4) -> bool {
        self.top = std::sync::OnceLock::new();
        self.meshes
            .get_mut(&id)
            .map(|m| {
                m.transform = transform;
                m.refresh();
            })
            .is_some()
    }

    /// Set a mesh's material. Returns whether the mesh exists.
    pub fn set_material(&mut self, id: MeshId, material: Material) -> bool {
        self.meshes.get_mut(&id).map(|m| m.material = material).is_some()
    }

    /// Show or hide a mesh. Returns whether the mesh exists.
    pub fn set_visible(&mut self, id: MeshId, visible: bool) -> bool {
        self.top = std::sync::OnceLock::new();
        self.meshes.get_mut(&id).map(|m| m.visible = visible).is_some()
    }

    /// Colour one face (`None` back to the material's colour). Returns whether the mesh
    /// exists; a face index past the mesh's faces is ignored.
    pub fn set_face_color(&mut self, id: MeshId, face: u32, color: Option<Srgb>) -> bool {
        let Some(m) = self.meshes.get_mut(&id) else { return false };
        match color {
            Some(c) if (face as usize) < m.data.face_count() => {
                m.face_colors.insert(face, c);
            }
            Some(_) => {}
            None => {
                m.face_colors.remove(&face);
            }
        }
        true
    }

    /// Highlight one B-rep face. Returns whether the mesh and face exist.
    pub fn set_face_highlight(&mut self, id: MeshId, face: u32, h: Highlight) -> bool {
        let Some(m) = self.meshes.get_mut(&id) else { return false };
        if face as usize >= m.data.face_count() {
            return false;
        }
        if h == Highlight::None {
            m.face_highlights.remove(&face);
        } else {
            m.face_highlights.insert(face, h);
        }
        true
    }

    /// Highlight one topological edge. Returns whether the mesh and edge exist.
    pub fn set_edge_highlight(&mut self, id: MeshId, edge: u32, h: Highlight) -> bool {
        let Some(m) = self.meshes.get_mut(&id) else { return false };
        if edge as usize >= m.data.edge_count() {
            return false;
        }
        if h == Highlight::None {
            m.edge_highlights.remove(&edge);
        } else {
            m.edge_highlights.insert(edge, h);
        }
        true
    }

    /// Clear highlights of the given kind everywhere (`Highlight::None` clears all).
    pub fn clear_highlights(&mut self, kind: Highlight) {
        for m in self.meshes.values_mut() {
            for map in [&mut m.face_highlights, &mut m.edge_highlights] {
                if kind == Highlight::None {
                    map.clear();
                } else {
                    map.retain(|_, h| *h != kind);
                }
            }
        }
    }

    /// World bounds of all visible meshes.
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        self.meshes.values().filter(|m| m.visible).fold(Aabb::EMPTY, |acc, m| acc.union(&m.world_bounds()))
    }

    /// Approximate CPU memory held by mesh data, in bytes.
    #[must_use]
    pub fn cpu_bytes(&self) -> usize {
        self.meshes.values().map(|m| m.data.cpu_bytes()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn geometry_moves_to_a_new_scene_unbuilt() {
        let mut a = Scene::new();
        a.insert_mesh(MeshId(1), &demo::box_mesh([1.0; 3])).expect("ok");
        a.set_material(MeshId(1), Material { color: Srgb(0x11_22_33), finish: Finish::Metal });
        let mut b = Scene::new();
        b.insert_geometry(MeshId(7), a.geometry(MeshId(1)).expect("geometry"));
        let (ma, mb) = (a.mesh(MeshId(1)).expect("a"), b.mesh(MeshId(7)).expect("b"));
        // Same data (not rebuilt), same revision (GPU buffers kept); scene state is the new scene's.
        assert!(std::ptr::eq(ma.data(), mb.data()));
        assert_eq!(ma.geometry_rev, mb.geometry_rev);
        assert_eq!(mb.material(), Material::default());
        assert!(a.geometry(MeshId(2)).is_none());
    }

    #[test]
    fn revisions_are_unique_across_scenes() {
        // Renderers cache uploads by (id, revision); fresh scenes must not repeat one.
        let mut a = Scene::new();
        a.insert_mesh(MeshId(1), &demo::box_mesh([1.0; 3])).expect("ok");
        let mut b = Scene::new();
        b.insert_mesh(MeshId(1), &demo::cylinder_mesh(1.0, 1.0, 64)).expect("ok");
        let ra = a.mesh(MeshId(1)).expect("a").geometry_rev;
        let rb = b.mesh(MeshId(1)).expect("b").geometry_rev;
        assert_ne!(ra, rb);
    }

    #[test]
    fn insert_replace_remove() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([1.0; 3])).expect("ok");
        assert!(s.set_face_highlight(MeshId(1), 2, Highlight::Select));
        assert!(!s.set_face_highlight(MeshId(1), 6, Highlight::Select));
        assert!(s.set_transform(MeshId(1), math::translation([5.0, 0.0, 0.0])));
        let rev = s.mesh(MeshId(1)).expect("mesh").geometry_rev;
        s.insert_mesh(MeshId(1), &demo::cylinder_mesh(1.0, 1.0, 8)).expect("ok");
        let m = s.mesh(MeshId(1)).expect("mesh");
        assert_ne!(m.geometry_rev, rev);
        assert_eq!(m.face_highlight(2), Highlight::None);
        assert!((m.world_bounds().min[0] - 4.0).abs() < 1e-6);
        let mut bad = demo::box_mesh([1.0; 3]);
        bad.indices.pop();
        assert!(s.insert_mesh(MeshId(1), &bad).is_err());
        assert_eq!(s.mesh(MeshId(1)).map(|m| m.data.face_count()), Some(3));
        assert!(s.remove_mesh(MeshId(1)));
        assert!(!s.remove_mesh(MeshId(1)));
        assert!(s.bounds().is_empty());
    }

    #[test]
    fn highlight_runs_split_ranges() {
        let ranges = vec![0..2, 2..4, 4..10, 10..12];
        let mut h = BTreeMap::new();
        h.insert(1, Highlight::Preselect);
        h.insert(3, Highlight::Select);
        let runs = SceneMesh::runs(&ranges, &h, 12);
        assert_eq!(
            runs,
            vec![
                (0..2, Highlight::None),
                (2..4, Highlight::Preselect),
                (4..10, Highlight::None),
                (10..12, Highlight::Select)
            ]
        );
        assert_eq!(SceneMesh::runs(&ranges, &BTreeMap::new(), 12), vec![(0..12, Highlight::None)]);
    }

    #[test]
    fn clear_highlights_by_kind() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(7), &demo::box_mesh([1.0; 3])).expect("ok");
        s.set_face_highlight(MeshId(7), 0, Highlight::Preselect);
        s.set_face_highlight(MeshId(7), 1, Highlight::Select);
        s.set_edge_highlight(MeshId(7), 3, Highlight::Preselect);
        s.clear_highlights(Highlight::Preselect);
        let m = s.mesh(MeshId(7)).expect("mesh");
        assert_eq!(m.face_highlight(0), Highlight::None);
        assert_eq!(m.face_highlight(1), Highlight::Select);
        assert_eq!(m.edge_highlight(3), Highlight::None);
    }
}
