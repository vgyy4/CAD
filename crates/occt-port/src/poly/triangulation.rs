//! `Poly_Triangulation` (with the node storage of `Poly_ArrayOfNodes` / `Poly_ArrayOfUVNodes`).
use super::Triangle;
use crate::bnd::BndBox;
use crate::gp::{Dir, Pnt, Pnt2d, Trsf, TrsfForm};
use crate::mathlin::MathError;

/// Resizes like `NCollection_AliasedArray::Resize` / `NCollection_Array1::Resize`: nothing changes when the length is
/// equal; otherwise the first `min(old, new)` elements are kept when `copy`, and the others are zero.
fn resize_vec<T: Clone + Default>(v: &mut Vec<T>, n: usize, copy: bool) {
    if v.len() == n {
        return;
    }
    if copy {
        v.resize(n, T::default());
    } else {
        *v = vec![T::default(); n];
    }
}

/// `v[i - 1]` with the range check of `NCollection_Array1::Value`.
fn at<T: Copy>(v: &[T], i: i32) -> Result<T, MathError> {
    v.get(index(i, v.len())?).copied().ok_or(MathError::Range)
}

fn index(i: i32, len: usize) -> Result<usize, MathError> {
    let k = usize::try_from(i.wrapping_sub(1)).map_err(|_| MathError::Range)?;
    if k < len { Ok(k) } else { Err(MathError::Range) }
}

/// `Poly_Triangulation`.
#[derive(Debug, Clone)]
pub struct Triangulation {
    deflection: f64,
    purpose: u32,
    double_nodes: bool,
    nodes: Vec<Pnt>,
    triangles: Vec<Triangle>,
    tri_lower: i32,
    double_uv: bool,
    uv_nodes: Vec<Pnt2d>,
    normals: Vec<[f32; 3]>,
    cached: Option<BndBox>,
}

impl Default for Triangulation {
    fn default() -> Self {
        Self::new()
    }
}

impl Triangulation {
    /// `Poly_Triangulation()`.
    pub fn new() -> Self {
        Self {
            deflection: 0.0,
            purpose: 0,
            double_nodes: true,
            nodes: Vec::new(),
            triangles: Vec::new(),
            tri_lower: 1,
            double_uv: true,
            uv_nodes: Vec::new(),
            normals: Vec::new(),
            cached: None,
        }
    }
    /// `Poly_Triangulation(NbNodes, NbTriangles, HasUVNodes, HasNormals)`.
    pub fn with_sizes(nb_nodes: i32, nb_triangles: i32, has_uv: bool, has_normals: bool) -> Result<Self, MathError> {
        if nb_nodes <= 0 || nb_triangles < 0 {
            return Err(MathError::Range);
        }
        let mut t = Self::new();
        t.nodes = vec![Pnt::default(); nb_nodes as usize];
        t.triangles = vec![Triangle::new(); nb_triangles as usize];
        if has_uv {
            t.uv_nodes = vec![Pnt2d::default(); nb_nodes as usize];
        }
        if has_normals {
            t.normals = vec![[0.0; 3]; nb_nodes as usize];
        }
        Ok(t)
    }
    /// `Poly_Triangulation(Nodes, Triangles)`; the triangles array has the lower bound `lower`.
    pub fn from_arrays(nodes: &[Pnt], triangles: &[Triangle], lower: i32) -> Result<Self, MathError> {
        if nodes.is_empty() {
            return Err(MathError::Range);
        }
        let mut t = Self::new();
        t.nodes = nodes.to_vec();
        t.triangles = triangles.to_vec();
        t.tri_lower = lower;
        Ok(t)
    }
    /// `Poly_Triangulation(Nodes, UVNodes, Triangles)`.
    pub fn from_arrays_uv(
        nodes: &[Pnt],
        uv_nodes: &[Pnt2d],
        triangles: &[Triangle],
        lower: i32,
    ) -> Result<Self, MathError> {
        if uv_nodes.len() != nodes.len() {
            return Err(MathError::Dimension);
        }
        let mut t = Self::from_arrays(nodes, triangles, lower)?;
        t.uv_nodes = uv_nodes.to_vec();
        Ok(t)
    }

    /// `Deflection()`.
    pub fn deflection(&self) -> f64 {
        self.deflection
    }
    /// `Deflection(value)`.
    pub fn set_deflection(&mut self, d: f64) {
        self.deflection = d;
    }
    /// `MeshPurpose()`.
    pub fn mesh_purpose(&self) -> u32 {
        self.purpose
    }
    /// `SetMeshPurpose()`.
    pub fn set_mesh_purpose(&mut self, p: u32) {
        self.purpose = p;
    }
    /// `Clear()`: empties nodes, triangles, UV nodes and normals (the precisions stay).
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.triangles.clear();
        self.tri_lower = 1;
        self.remove_uv_nodes();
        self.remove_normals();
    }
    /// `HasGeometry()`.
    pub fn has_geometry(&self) -> bool {
        !self.nodes.is_empty() && !self.triangles.is_empty()
    }
    /// `NbNodes()`.
    pub fn nb_nodes(&self) -> i32 {
        self.nodes.len() as i32
    }
    /// `NbTriangles()`.
    pub fn nb_triangles(&self) -> i32 {
        self.triangles.len() as i32
    }
    /// `HasUVNodes()`.
    pub fn has_uv_nodes(&self) -> bool {
        !self.uv_nodes.is_empty()
    }
    /// `HasNormals()`.
    pub fn has_normals(&self) -> bool {
        !self.normals.is_empty()
    }
    /// `IsDoublePrecision()`.
    pub fn is_double_precision(&self) -> bool {
        self.double_nodes
    }
    /// `SetDoublePrecision()`: `Err` when nodes (or UV nodes) are allocated.
    pub fn set_double_precision(&mut self, double: bool) -> Result<(), MathError> {
        if !self.nodes.is_empty() {
            return Err(MathError::Argument);
        }
        self.double_nodes = double;
        if !self.uv_nodes.is_empty() {
            return Err(MathError::Argument);
        }
        self.double_uv = double;
        Ok(())
    }

    /// `Node(i)` (1-based).
    pub fn node(&self, i: i32) -> Result<Pnt, MathError> {
        at(&self.nodes, i)
    }
    /// `SetNode(i, p)`; single precision storage rounds to `f32`.
    pub fn set_node(&mut self, i: i32, p: &Pnt) -> Result<(), MathError> {
        let k = index(i, self.nodes.len())?;
        let q = if self.double_nodes {
            *p
        } else {
            Pnt::new(f64::from(p.x() as f32), f64::from(p.y() as f32), f64::from(p.z() as f32))
        };
        if let Some(slot) = self.nodes.get_mut(k) {
            *slot = q;
        }
        Ok(())
    }
    /// `UVNode(i)`.
    pub fn uv_node(&self, i: i32) -> Result<Pnt2d, MathError> {
        at(&self.uv_nodes, i)
    }
    /// `SetUVNode(i, p)`.
    pub fn set_uv_node(&mut self, i: i32, p: &Pnt2d) -> Result<(), MathError> {
        let k = index(i, self.uv_nodes.len())?;
        let q = if self.double_uv { *p } else { Pnt2d::new(f64::from(p.x() as f32), f64::from(p.y() as f32)) };
        if let Some(slot) = self.uv_nodes.get_mut(k) {
            *slot = q;
        }
        Ok(())
    }
    /// The lower bound of the triangle array (`triangle(i)` is addressed `lower..lower + nb_triangles - 1`).
    pub fn triangle_lower(&self) -> i32 {
        self.tri_lower
    }
    /// `Triangle(i)`.
    pub fn triangle(&self, i: i32) -> Result<Triangle, MathError> {
        at(&self.triangles, i.wrapping_sub(self.tri_lower).wrapping_add(1))
    }
    /// `SetTriangle(i, t)`.
    pub fn set_triangle(&mut self, i: i32, t: Triangle) -> Result<(), MathError> {
        let k = index(i.wrapping_sub(self.tri_lower).wrapping_add(1), self.triangles.len())?;
        if let Some(slot) = self.triangles.get_mut(k) {
            *slot = t;
        }
        Ok(())
    }
    /// `Normal(i, Vec3)`: the normal as floats.
    pub fn normal(&self, i: i32) -> Result<[f32; 3], MathError> {
        at(&self.normals, i)
    }
    /// `Normal(i)`: a `gp_Dir` of the widened components.
    pub fn normal_dir(&self, i: i32) -> Result<Dir, MathError> {
        let n = self.normal(i)?;
        Dir::new(f64::from(n[0]), f64::from(n[1]), f64::from(n[2])).map_err(|_| MathError::Domain)
    }
    /// `SetNormal(i, Vec3)`.
    pub fn set_normal(&mut self, i: i32, n: [f32; 3]) -> Result<(), MathError> {
        let k = index(i, self.normals.len())?;
        if let Some(slot) = self.normals.get_mut(k) {
            *slot = n;
        }
        Ok(())
    }
    /// `SetNormal(i, gp_Dir)`.
    pub fn set_normal_dir(&mut self, i: i32, d: &Dir) -> Result<(), MathError> {
        self.set_normal(i, [d.x() as f32, d.y() as f32, d.z() as f32])
    }

    /// `ResizeNodes(n, copy)`.
    pub fn resize_nodes(&mut self, n: i32, copy: bool) -> Result<(), MathError> {
        if n <= 0 {
            return Err(MathError::Range);
        }
        let n = n as usize;
        resize_vec(&mut self.nodes, n, copy);
        if !self.uv_nodes.is_empty() {
            resize_vec(&mut self.uv_nodes, n, copy);
        }
        if !self.normals.is_empty() {
            resize_vec(&mut self.normals, n, copy);
        }
        Ok(())
    }
    /// `ResizeTriangles(n, copy)`.
    pub fn resize_triangles(&mut self, n: i32, copy: bool) -> Result<(), MathError> {
        if n < 0 {
            return Err(MathError::Range);
        }
        resize_vec(&mut self.triangles, n as usize, copy);
        self.tri_lower = 1;
        Ok(())
    }
    /// `AddUVNodes()`.
    pub fn add_uv_nodes(&mut self) {
        if self.uv_nodes.is_empty() || self.uv_nodes.len() != self.nodes.len() {
            self.uv_nodes = vec![Pnt2d::default(); self.nodes.len()];
        }
    }
    /// `RemoveUVNodes()`.
    pub fn remove_uv_nodes(&mut self) {
        self.uv_nodes.clear();
    }
    /// `AddNormals()`.
    pub fn add_normals(&mut self) {
        if self.normals.is_empty() || self.normals.len() != self.nodes.len() {
            self.normals = vec![[0.0; 3]; self.nodes.len()];
        }
    }
    /// `RemoveNormals()`.
    pub fn remove_normals(&mut self) {
        self.normals.clear();
    }
    /// `ComputeNormals()`.
    pub fn compute_normals(&mut self) -> Result<(), MathError> {
        self.add_normals();
        for n in &mut self.normals {
            *n = [0.0; 3];
        }
        for k in 0..self.triangles.len() {
            let (a, b, c) = self.triangles.get(k).map_or((0, 0, 0), Triangle::get);
            let n0 = self.node(a)?;
            let n1 = self.node(b)?;
            let n2 = self.node(c)?;
            let v01 = n1.xyz().subtracted(n0.xyz());
            let v02 = n2.xyz().subtracted(n0.xyz());
            let tn = v01.crossed(&v02);
            let n3 = [tn.x() as f32, tn.y() as f32, tn.z() as f32];
            for e in [a, b, c] {
                let i = index(e, self.normals.len())?;
                if let Some(slot) = self.normals.get_mut(i) {
                    slot[0] += n3[0];
                    slot[1] += n3[1];
                    slot[2] += n3[2];
                }
            }
        }
        for n in &mut self.normals {
            let m = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            *n = if m == 0.0 {
                [0.0, 0.0, 1.0]
            } else {
                let inv = 1.0f32 / m;
                [n[0] * inv, n[1] * inv, n[2] * inv]
            };
        }
        Ok(())
    }

    /// `HasCachedMinMax()`.
    pub fn has_cached_min_max(&self) -> bool {
        self.cached.is_some()
    }
    /// `CachedMinMax()`: an empty box when there is no cache.
    pub fn cached_min_max(&self) -> BndBox {
        self.cached.clone().unwrap_or_default()
    }
    /// `SetCachedMinMax(box)`: a void box removes the cache.
    pub fn set_cached_min_max(&mut self, b: &BndBox) {
        self.cached = if b.is_void() { None } else { Some(b.clone()) };
    }
    /// `UpdateCachedMinMax()`.
    pub fn update_cached_min_max(&mut self) {
        let mut b = BndBox::new();
        self.min_max(&mut b, &Trsf::new(), true);
        self.set_cached_min_max(&b);
    }
    /// `MinMax(box, trsf, accurate)`.
    pub fn min_max(&self, bx: &mut BndBox, trsf: &Trsf, accurate: bool) -> bool {
        let form = trsf.form();
        let b = match &self.cached {
            Some(c)
                if !self.has_geometry()
                    || !accurate
                    || matches!(
                        form,
                        TrsfForm::Identity | TrsfForm::Translation | TrsfForm::PntMirror | TrsfForm::Scale
                    ) =>
            {
                c.transformed(trsf)
            }
            _ => self.compute_bounding_box(trsf),
        };
        if b.is_void() {
            return false;
        }
        bx.add_box(&b);
        true
    }
    fn compute_bounding_box(&self, trsf: &Trsf) -> BndBox {
        let mut b = BndBox::new();
        for p in &self.nodes {
            if trsf.form() == TrsfForm::Identity {
                b.add_point(p);
            } else {
                b.add_point(&p.transformed(trsf));
            }
        }
        b
    }
}

/// `Poly::ComputeNormals`.
pub fn compute_normals(t: &mut Triangulation) -> Result<(), MathError> {
    t.compute_normals()
}

/// `Poly::Catenate`: the union of the triangulations (null entries are skipped).
pub fn catenate(list: &[Option<&Triangulation>]) -> Option<Triangulation> {
    let (mut n_nodes, mut n_trian) = (0i32, 0i32);
    for t in list.iter().flatten() {
        n_nodes += t.nb_nodes();
        n_trian += t.nb_triangles();
    }
    if n_nodes == 0 {
        return None;
    }
    let mut result = Triangulation::with_sizes(n_nodes, n_trian, false, false).ok()?;
    let (mut n_nodes, mut n_trian) = (0i32, 0i32);
    for t in list.iter().flatten() {
        let (nb_nodes, nb_trian) = (t.nb_nodes(), t.nb_triangles());
        for i in 1..=nb_nodes {
            result.set_node(i + n_nodes, &t.node(i).ok()?).ok()?;
        }
        for i in 1..=nb_trian {
            let (a, b, c) = t.triangle(i).ok()?.get();
            result.set_triangle(i + n_trian, Triangle::from_nodes(a + n_nodes, b + n_nodes, c + n_nodes)).ok()?;
        }
        n_nodes += nb_nodes;
        n_trian += nb_trian;
    }
    Some(result)
}
