//! `Poly_MergeNodesTool`: merges the nodes (and optionally the elements) of triangles pushed one by one.
use super::{Triangle, Triangulation};
use crate::gp::{Pnt, Trsf, XYZ};
use crate::mathlin::MathError;
use std::collections::{HashMap, HashSet};

type Vec3f = [f32; 3];

fn dot3(a: &Vec3f, b: &Vec3f) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `NCollection_Vec3<float>::Normalized()`.
fn normalized(v: Vec3f) -> Vec3f {
    let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if m != 0.0 { [v[0] / m, v[1] / m, v[2] / m] } else { v }
}

const NEIGHBOURS: [[i64; 3]; 26] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, -1, 0],
    [1, -1, 0],
    [1, 1, 0],
    [-1, 1, 0],
    [0, -1, -1],
    [0, 1, -1],
    [0, 1, 1],
    [0, -1, 1],
    [-1, 0, -1],
    [1, 0, -1],
    [1, 0, 1],
    [-1, 0, 1],
    [-1, -1, -1],
    [1, -1, -1],
    [-1, 1, -1],
    [1, 1, -1],
    [-1, -1, 1],
    [1, -1, 1],
    [-1, 1, 1],
    [1, 1, 1],
];

#[derive(Debug, Clone)]
struct MergedNode {
    pos: Vec3f,
    norm: Vec3f,
    index: i32,
}

/// `Poly_MergeNodesTool::MergedNodesMap`: the buckets are replaced by a grid of cells (`pos / tolerance`, truncated)
/// when the tolerance is positive and by the exact `f32` position otherwise.
#[derive(Debug)]
struct MergedNodesMap {
    tolerance: f32,
    inv_tol: f32,
    angle: f32,
    angle_cos: f32,
    merge_opposite: bool,
    nodes: Vec<MergedNode>,
    cells: HashMap<[i64; 3], Vec<usize>>,
    exact: HashMap<[u32; 3], Vec<usize>>,
}

impl MergedNodesMap {
    fn new() -> Self {
        Self {
            tolerance: 0.0,
            inv_tol: 0.0,
            angle: 1.0,
            angle_cos: 0.0,
            merge_opposite: false,
            nodes: Vec::new(),
            cells: HashMap::new(),
            exact: HashMap::new(),
        }
    }
    fn set_merge_angle(&mut self, angle: f64) {
        self.angle = angle as f32;
        self.angle_cos = libm::cos(angle) as f32;
    }
    fn has_merge_angle(&self) -> bool {
        self.angle > 0.0
    }
    fn to_merge_any_angle(&self) -> bool {
        self.angle_cos <= 0.01
    }
    fn set_merge_tolerance(&mut self, tol: f64) {
        self.tolerance = tol as f32;
        self.inv_tol = 0.0;
        if self.tolerance > 0.0 {
            self.inv_tol = (1.0 / tol) as f32;
        }
    }
    fn has_merge_tolerance(&self) -> bool {
        self.tolerance > 0.0
    }
    fn cell(&self, p: &Vec3f) -> [i64; 3] {
        [(p[0] * self.inv_tol) as i64, (p[1] * self.inv_tol) as i64, (p[2] * self.inv_tol) as i64]
    }
    fn exact_key(p: &Vec3f) -> [u32; 3] {
        let k = |v: f32| if v == 0.0 { 0.0f32.to_bits() } else { v.to_bits() };
        [k(p[0]), k(p[1]), k(p[2])]
    }
    fn pos_equal(&self, a: &Vec3f, b: &Vec3f) -> bool {
        if self.inv_tol <= 0.0 {
            return a[0] == b[0] && a[1] == b[1] && a[2] == b[2];
        }
        !((a[0] - b[0]).abs() > self.tolerance
            || (a[1] - b[1]).abs() > self.tolerance
            || (a[2] - b[2]).abs() > self.tolerance)
    }
    fn is_equal(&self, key: &MergedNode, pos: &Vec3f, norm: &Vec3f) -> bool {
        if !self.pos_equal(&key.pos, pos) {
            return false;
        }
        let c = dot3(&key.norm, norm);
        if c >= self.angle_cos {
            return true;
        }
        self.merge_opposite && c <= -self.angle_cos
    }
    fn find_in(&self, list: Option<&Vec<usize>>, pos: &Vec3f, norm: &Vec3f) -> Option<i32> {
        for &k in list?.iter().rev() {
            let n = self.nodes.get(k)?;
            if self.is_equal(n, pos, norm) {
                return Some(n.index);
            }
        }
        None
    }
    /// `Bind`: `Some(index)` of the existing node, or `None` after storing the new node under `index`.
    fn bind(&mut self, index: i32, pos: Vec3f, norm: Vec3f) -> Option<i32> {
        if self.inv_tol <= 0.0 {
            let key = Self::exact_key(&pos);
            if let Some(i) = self.find_in(self.exact.get(&key), &pos, &norm) {
                return Some(i);
            }
            self.exact.entry(key).or_default().push(self.nodes.len());
        } else {
            let c = self.cell(&pos);
            if let Some(i) = self.find_in(self.cells.get(&c), &pos, &norm) {
                return Some(i);
            }
            for d in &NEIGHBOURS {
                let nc = [c[0].wrapping_add(d[0]), c[1].wrapping_add(d[1]), c[2].wrapping_add(d[2])];
                if let Some(i) = self.find_in(self.cells.get(&nc), &pos, &norm) {
                    return Some(i);
                }
            }
            self.cells.entry(c).or_default().push(self.nodes.len());
        }
        self.nodes.push(MergedNode { pos, norm, index });
        None
    }
}

/// `Poly_MergeNodesTool`.
#[derive(Debug)]
pub struct MergeNodesTool {
    poly_data: Triangulation,
    node_index_map: MergedNodesMap,
    elem_map: HashSet<[i32; 4]>,
    node_inds: [i32; 4],
    tri_normal: Vec3f,
    places: [XYZ; 4],
    scratch: XYZ,
    unit_factor: f64,
    nb_nodes: i32,
    nb_elems: i32,
    nb_degen_elems: i32,
    nb_merged_elems: i32,
    to_drop_degenerative: bool,
    to_merge_elems: bool,
}

impl MergeNodesTool {
    /// `Poly_MergeNodesTool(theSmoothAngle, theMergeTolerance, theNbFacets)`.
    pub fn new(smooth_angle: f64, merge_tolerance: f64, _nb_facets: i32) -> Self {
        let mut map = MergedNodesMap::new();
        map.set_merge_angle(smooth_angle);
        map.set_merge_tolerance(merge_tolerance);
        Self {
            poly_data: Triangulation::new(),
            node_index_map: map,
            elem_map: HashSet::new(),
            node_inds: [0, 0, 0, -1],
            tri_normal: [0.0, 0.0, 1.0],
            places: [XYZ::default(); 4],
            scratch: XYZ::default(),
            unit_factor: 1.0,
            nb_nodes: 0,
            nb_elems: 0,
            nb_degen_elems: 0,
            nb_merged_elems: 0,
            to_drop_degenerative: true,
            to_merge_elems: false,
        }
    }
    /// `MergeNodes()` (static).
    pub fn merge_nodes(
        tris: Option<&Triangulation>,
        trsf: &Trsf,
        to_reverse: bool,
        smooth_angle: f64,
        merge_tolerance: f64,
        to_force: bool,
    ) -> Result<Option<Triangulation>, MathError> {
        let tris = match tris {
            Some(t) if t.nb_nodes() >= 3 && t.nb_triangles() >= 1 => t,
            _ => return Ok(None),
        };
        let mut tool = MergeNodesTool::new(smooth_angle, merge_tolerance, tris.nb_triangles());
        tool.add_triangulation(Some(tris), trsf, to_reverse)?;
        if !to_force && tool.nb_nodes() == tris.nb_nodes() && tool.nb_elements() == tris.nb_triangles() {
            return Ok(None);
        }
        tool.result()
    }

    /// `MergeTolerance()` (the `float` it was rounded to).
    pub fn merge_tolerance(&self) -> f64 {
        f64::from(self.node_index_map.tolerance)
    }
    /// `SetMergeTolerance()`.
    pub fn set_merge_tolerance(&mut self, tol: f64) {
        self.node_index_map.set_merge_tolerance(tol);
    }
    /// `MergeAngle()` (the `float` it was rounded to).
    pub fn merge_angle(&self) -> f64 {
        f64::from(self.node_index_map.angle)
    }
    /// `SetMergeAngle()`.
    pub fn set_merge_angle(&mut self, angle: f64) {
        self.node_index_map.set_merge_angle(angle);
    }
    /// `ToMergeOpposite()`.
    pub fn to_merge_opposite(&self) -> bool {
        self.node_index_map.merge_opposite
    }
    /// `SetMergeOpposite()`.
    pub fn set_merge_opposite(&mut self, v: bool) {
        self.node_index_map.merge_opposite = v;
    }
    /// `SetUnitFactor()`.
    pub fn set_unit_factor(&mut self, f: f64) {
        self.unit_factor = f;
    }
    /// `ToDropDegenerative()`.
    pub fn to_drop_degenerative(&self) -> bool {
        self.to_drop_degenerative
    }
    /// `SetDropDegenerative()`.
    pub fn set_drop_degenerative(&mut self, v: bool) {
        self.to_drop_degenerative = v;
    }
    /// `ToMergeElems()`.
    pub fn to_merge_elems(&self) -> bool {
        self.to_merge_elems
    }
    /// `SetMergeElems()`.
    pub fn set_merge_elems(&mut self, v: bool) {
        self.to_merge_elems = v;
    }

    fn place(&self, i: usize) -> XYZ {
        self.places.get(i).copied().unwrap_or_default()
    }
    fn compute_tri_normal(&self) -> Vec3f {
        let v01 = self.place(1).subtracted(&self.place(0));
        let v02 = self.place(2).subtracted(&self.place(0));
        let c = v01.crossed(&v02);
        normalized([c.x() as f32, c.y() as f32, c.z() as f32])
    }
    fn set_ind(&mut self, tri_node: usize, v: i32) {
        if let Some(s) = self.node_inds.get_mut(tri_node) {
            *s = v;
        }
    }

    /// `AddTriangulation()`.
    pub fn add_triangulation(
        &mut self,
        tris: Option<&Triangulation>,
        trsf: &Trsf,
        to_reverse: bool,
    ) -> Result<(), MathError> {
        let Some(tris) = tris else { return Ok(()) };
        if self.poly_data.nb_nodes() == 0 {
            self.poly_data.set_double_precision(tris.is_double_precision())?;
            self.poly_data.resize_nodes(tris.nb_nodes(), false)?;
            self.poly_data.resize_triangles(tris.nb_triangles(), false)?;
        }
        for elem_iter in 1..=tris.nb_triangles() {
            let mut elem = tris.triangle(elem_iter)?;
            if to_reverse {
                elem = Triangle::from_nodes(elem.value(1)?, elem.value(3)?, elem.value(2)?);
            }
            for k in 0..3usize {
                let node: Pnt = tris.node(elem.value(k as i32 + 1)?)?.transformed(trsf);
                if let Some(slot) = self.places.get_mut(k) {
                    *slot = *node.xyz();
                }
            }
            self.push_last(3)?;
        }
        Ok(())
    }
    /// `Result()`: `Err` when no node was pushed (`ResizeNodes(0)` throws).
    pub fn result(&mut self) -> Result<Option<Triangulation>, MathError> {
        self.poly_data.resize_nodes(self.nb_nodes, true)?;
        self.poly_data.resize_triangles(self.nb_elems, true)?;
        Ok(Some(self.poly_data.clone()))
    }
    /// `AddTriangle()`.
    pub fn add_triangle(&mut self, nodes: &[XYZ; 3]) {
        for (k, n) in nodes.iter().enumerate() {
            if let Some(s) = self.places.get_mut(k) {
                *s = *n;
            }
        }
        // 3 nodes cannot fail
        let _ = self.push_last(3);
    }
    /// `AddQuad()`.
    pub fn add_quad(&mut self, nodes: &[XYZ; 4]) {
        self.places = *nodes;
        let _ = self.push_last(4);
    }
    /// `AddElement(nodes, n)`: `n` other than 3, 4 is `Err`.
    pub fn add_element(&mut self, nodes: &[XYZ], nb_nodes: i32) -> Result<(), MathError> {
        if nb_nodes != 3 && nb_nodes != 4 {
            return Err(MathError::Argument);
        }
        for k in 0..nb_nodes as usize {
            let n = *nodes.get(k).ok_or(MathError::Range)?;
            if let Some(s) = self.places.get_mut(k) {
                *s = n;
            }
        }
        self.push_last(nb_nodes)
    }
    /// `ChangeElementNode(i)`.
    pub fn change_element_node(&mut self, i: i32) -> &mut XYZ {
        match usize::try_from(i).ok().and_then(|k| self.places.get_mut(k)) {
            Some(s) => s,
            None => &mut self.scratch,
        }
    }
    /// `PushLastElement(n)`.
    pub fn push_last_element(&mut self, nb_nodes: i32) -> Result<(), MathError> {
        self.push_last(nb_nodes)
    }
    /// `ElementNodeIndex(i)`.
    pub fn element_node_index(&self, i: i32) -> i32 {
        usize::try_from(i).ok().and_then(|k| self.node_inds.get(k)).copied().unwrap_or(0)
    }
    /// `NbNodes()`.
    pub fn nb_nodes(&self) -> i32 {
        self.nb_nodes
    }
    /// `NbElements()`.
    pub fn nb_elements(&self) -> i32 {
        self.nb_elems
    }
    /// `NbDegenerativeElems()`.
    pub fn nb_degenerative_elems(&self) -> i32 {
        self.nb_degen_elems
    }
    /// `NbMergedElems()`.
    pub fn nb_merged_elems(&self) -> i32 {
        self.nb_merged_elems
    }

    fn push_node_check(&mut self, tri_node: usize) -> Result<(), MathError> {
        let mut node_index = self.nb_nodes;
        let place = self.place(tri_node);
        let pos = [place.x() as f32, place.y() as f32, place.z() as f32];
        match self.node_index_map.bind(node_index, pos, self.tri_normal) {
            None => {
                self.nb_nodes += 1;
                if self.poly_data.nb_nodes() < self.nb_nodes {
                    self.poly_data.resize_nodes(self.nb_nodes.wrapping_mul(2), true)?;
                }
                self.poly_data.set_node(self.nb_nodes, &Pnt::from_xyz(place.multiplied(self.unit_factor)))?;
            }
            Some(i) => node_index = i,
        }
        self.set_ind(tri_node, node_index);
        Ok(())
    }
    fn push_node_no_merge(&mut self, tri_node: usize) -> Result<(), MathError> {
        let node_index = self.nb_nodes;
        let place = self.place(tri_node).multiplied(self.unit_factor);
        self.nb_nodes += 1;
        if self.poly_data.nb_nodes() < self.nb_nodes {
            self.poly_data.resize_nodes(self.nb_nodes.wrapping_mul(2), true)?;
        }
        self.poly_data.set_node(self.nb_nodes, &Pnt::from_xyz(place))?;
        self.set_ind(tri_node, node_index);
        Ok(())
    }
    fn push_last(&mut self, nb_nodes: i32) -> Result<(), MathError> {
        if nb_nodes != 3 && nb_nodes != 4 {
            return Err(MathError::Argument);
        }
        self.set_ind(3, -1);
        if self.node_index_map.has_merge_angle() || self.node_index_map.has_merge_tolerance() {
            if !self.node_index_map.to_merge_any_angle() {
                self.tri_normal = self.compute_tri_normal();
            }
            for k in 0..nb_nodes as usize {
                self.push_node_check(k)?;
            }
        } else {
            for k in 0..nb_nodes as usize {
                self.push_node_no_merge(k)?;
            }
        }
        let [i0, i1, i2, i3] = self.node_inds;
        if self.to_drop_degenerative && (i0 == i1 || i0 == i2 || i1 == i2) && nb_nodes != 4 {
            self.nb_degen_elems += 1;
            return Ok(());
        }
        if self.to_merge_elems {
            let mut sorted = self.node_inds;
            let n = nb_nodes.min(4) as usize;
            if let Some(s) = sorted.get_mut(..n) {
                s.sort_unstable();
            }
            if !self.elem_map.insert(sorted) {
                self.nb_merged_elems += 1;
                return Ok(());
            }
        }
        self.nb_elems += 1;
        if self.poly_data.nb_triangles() < self.nb_elems {
            self.poly_data.resize_triangles(self.nb_elems.wrapping_mul(2), true)?;
        }
        self.poly_data.set_triangle(self.nb_elems, Triangle::from_nodes(i0 + 1, i1 + 1, i2 + 1))?;
        if nb_nodes == 4 {
            self.nb_elems += 1;
            if self.poly_data.nb_triangles() < self.nb_elems {
                self.poly_data.resize_triangles(self.nb_elems.wrapping_mul(2), true)?;
            }
            self.poly_data.set_triangle(self.nb_elems, Triangle::from_nodes(i0 + 1, i2 + 1, i3 + 1))?;
        }
        Ok(())
    }
}
