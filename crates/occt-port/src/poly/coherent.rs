//! `Poly_CoherentTriangulation` with `Poly_CoherentNode`, `Poly_CoherentLink` and `Poly_CoherentTriangle`.
//!
//! The pointers of the C++ classes are slots: triangles, nodes and links live in arrays that only grow (empty slots stay)
//! and refer to each other by index.
use super::fmt::fixed;
use super::{Triangle, Triangulation};
use crate::gp::{Dir, Pnt, Pnt2d, XYZ};
use crate::mathlin::MathError;

/// `Precision::Infinite()`.
const INFINITE: f64 = 2e100;
/// `Precision::Confusion()`.
const CONFUSION: f64 = 1e-7;

fn program_error() -> MathError {
    MathError::Argument
}

/// `Poly_CoherentNode`: a point with UV values, a normal, an index and the ring of its triangles.
#[derive(Debug, Clone)]
pub struct CoherentNode {
    xyz: XYZ,
    uv: [f64; 2],
    ring: Vec<usize>,
    index: i32,
    normal: [f32; 3],
}

impl Default for CoherentNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CoherentNode {
    /// `Poly_CoherentNode()`.
    pub fn new() -> Self {
        Self { xyz: XYZ::new(0., 0., 0.), uv: [INFINITE, INFINITE], ring: Vec::new(), index: -1, normal: [0.0; 3] }
    }
    /// `Poly_CoherentNode(thePnt)`.
    pub fn from_xyz(p: &XYZ) -> Self {
        Self { xyz: *p, ..Self::new() }
    }
    /// `X()`.
    pub fn x(&self) -> f64 {
        self.xyz.x()
    }
    /// `Y()`.
    pub fn y(&self) -> f64 {
        self.xyz.y()
    }
    /// `Z()`.
    pub fn z(&self) -> f64 {
        self.xyz.z()
    }
    /// The coordinates (`gp_XYZ`).
    pub fn xyz(&self) -> &XYZ {
        &self.xyz
    }
    /// `SetUV()`.
    pub fn set_uv(&mut self, u: f64, v: f64) {
        self.uv = [u, v];
    }
    /// `GetU()`.
    pub fn get_u(&self) -> f64 {
        self.uv[0]
    }
    /// `GetV()`.
    pub fn get_v(&self) -> f64 {
        self.uv[1]
    }
    /// `SetNormal()` (stored as `float`).
    pub fn set_normal(&mut self, v: &XYZ) {
        self.normal = [v.x() as f32, v.y() as f32, v.z() as f32];
    }
    /// `HasNormal()`.
    pub fn has_normal(&self) -> bool {
        let n = self.normal;
        f64::from(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) > CONFUSION
    }
    /// `GetNormal()`.
    pub fn get_normal(&self) -> XYZ {
        XYZ::new(f64::from(self.normal[0]), f64::from(self.normal[1]), f64::from(self.normal[2]))
    }
    /// `SetIndex()`.
    pub fn set_index(&mut self, i: i32) {
        self.index = i;
    }
    /// `GetIndex()`.
    pub fn get_index(&self) -> i32 {
        self.index
    }
    /// `IsFreeNode()`: no triangle uses the node.
    pub fn is_free_node(&self) -> bool {
        self.ring.is_empty()
    }
    /// The slots of the triangles of the node in ring order (the first triangle added comes first).
    pub fn triangles(&self) -> Vec<usize> {
        self.ring.clone()
    }
    /// `Poly_CoherentNode::RemoveTriangle`: the first occurrence leaves the ring.
    fn remove_triangle(&mut self, t: usize) -> bool {
        match self.ring.iter().position(|&u| u == t) {
            Some(p) => {
                self.ring.remove(p);
                true
            }
            None => false,
        }
    }
}

/// `Poly_CoherentLink`.
#[derive(Debug, Clone, Copy)]
pub struct CoherentLink {
    node: [i32; 2],
    opposite: [i32; 2],
    attribute: Option<usize>,
}

impl Default for CoherentLink {
    fn default() -> Self {
        Self::new()
    }
}

impl CoherentLink {
    /// `Poly_CoherentLink()`: empty.
    pub fn new() -> Self {
        Self { node: [-1, -1], opposite: [-1, -1], attribute: None }
    }
    /// `Poly_CoherentLink(iNode0, iNode1)`.
    pub fn from_nodes(n0: i32, n1: i32) -> Self {
        Self { node: [n0, n1], opposite: [-1, -1], attribute: None }
    }
    /// `Poly_CoherentLink(theTri, iSide)`.
    pub fn from_triangle(tri: &CoherentTriangle, side: i32) -> Result<Self, MathError> {
        const IND: [usize; 4] = [1, 2, 0, 1];
        if !(0..=2).contains(&side) {
            return Err(program_error());
        }
        let s = side as usize;
        let ind = |k: usize| IND.get(k).copied().unwrap_or(0);
        let a0 = tri.node(ind(s));
        let a1 = tri.node(ind(s + 1));
        let mut l = Self::new();
        if a0 < a1 {
            l.node = [a0, a1];
            l.opposite = [tri.node(s), tri.connected_node(s)];
        } else {
            l.node = [a1, a0];
            l.opposite = [tri.connected_node(s), tri.node(s)];
        }
        Ok(l)
    }
    /// `Node(ind & 1)`.
    pub fn node(&self, ind: usize) -> i32 {
        self.node.get(ind & 1).copied().unwrap_or(-1)
    }
    /// `OppositeNode(ind & 1)`.
    pub fn opposite_node(&self, ind: usize) -> i32 {
        self.opposite.get(ind & 1).copied().unwrap_or(-1)
    }
    /// `GetAttribute()`.
    pub fn get_attribute(&self) -> Option<usize> {
        self.attribute
    }
    /// `SetAttribute()`.
    pub fn set_attribute(&mut self, a: Option<usize>) {
        self.attribute = a;
    }
    /// `IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.node[0] < 0 || self.node[1] < 0
    }
    /// `Nullify()`.
    pub fn nullify(&mut self) {
        self.node = [-1, -1];
        self.opposite = [-1, -1];
    }
}

/// `Poly_CoherentTriangle`: three nodes, the triangles connected over the edges and the links.
#[derive(Debug, Clone, Copy)]
pub struct CoherentTriangle {
    n_connections: i32,
    nodes: [i32; 3],
    nodes_on_connected: [i32; 3],
    connected: [Option<usize>; 3],
    link: [Option<usize>; 3],
}

impl Default for CoherentTriangle {
    fn default() -> Self {
        Self::new()
    }
}

impl CoherentTriangle {
    /// `Poly_CoherentTriangle()`: empty.
    pub fn new() -> Self {
        Self::from_nodes(-1, -1, -1)
    }
    /// `Poly_CoherentTriangle(iNode0, iNode1, iNode2)`.
    pub fn from_nodes(n0: i32, n1: i32, n2: i32) -> Self {
        Self {
            n_connections: 0,
            nodes: [n0, n1, n2],
            nodes_on_connected: [-1; 3],
            connected: [None; 3],
            link: [None; 3],
        }
    }
    /// `Node(ind)`.
    pub fn node(&self, ind: usize) -> i32 {
        self.nodes.get(ind).copied().unwrap_or(-1)
    }
    /// `IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.nodes.iter().any(|&n| n < 0)
    }
    /// `NConnections()`.
    pub fn n_connections(&self) -> i32 {
        self.n_connections
    }
    /// `GetConnectedNode(iConn)`.
    pub fn connected_node(&self, ind: usize) -> i32 {
        self.nodes_on_connected.get(ind).copied().unwrap_or(-1)
    }
    /// `GetConnectedTri(iConn)`: the slot of the connected triangle.
    pub fn connected_triangle(&self, ind: usize) -> Option<usize> {
        self.connected.get(ind).copied().flatten()
    }
    /// `GetLink(iLink)`: the slot of the link.
    pub fn link(&self, ind: usize) -> Option<usize> {
        self.link.get(ind).copied().flatten()
    }
    fn set_node_at(&mut self, i: usize, v: i32) {
        if let Some(s) = self.nodes.get_mut(i) {
            *s = v;
        }
    }
    fn set_connected_at(&mut self, i: usize, v: Option<usize>, node: i32) {
        if let Some(s) = self.connected.get_mut(i) {
            *s = v;
        }
        if let Some(s) = self.nodes_on_connected.get_mut(i) {
            *s = node;
        }
    }
    fn set_link_at(&mut self, i: usize, v: Option<usize>) {
        if let Some(s) = self.link.get_mut(i) {
            *s = v;
        }
    }
}

/// `Poly_CoherentTriangulation`.
#[derive(Debug, Clone)]
pub struct CoherentTriangulation {
    triangles: Vec<CoherentTriangle>,
    nodes: Vec<CoherentNode>,
    links: Vec<CoherentLink>,
    deflection: f64,
}

impl Default for CoherentTriangulation {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! tri {
    ($s:expr, $t:expr) => {
        $s.triangles.get_mut($t).ok_or(MathError::Range)?
    };
}

impl CoherentTriangulation {
    /// `Poly_CoherentTriangulation()`.
    pub fn new() -> Self {
        Self { triangles: Vec::new(), nodes: Vec::new(), links: Vec::new(), deflection: 0.0 }
    }
    /// `Poly_CoherentTriangulation(theTriangulation)`.
    pub fn from_triangulation(t: Option<&Triangulation>) -> Result<Self, MathError> {
        let mut c = Self::new();
        let Some(t) = t else { return Ok(c) };
        let nb_nodes = t.nb_nodes();
        for i in 0..nb_nodes {
            let old = i + 1;
            let new_ind = c.set_node(t.node(old)?.xyz(), i)?;
            c.change_node(new_ind)?.set_index(old);
        }
        for i in 1..=t.nb_triangles() {
            let (a, b, d) = t.triangle(i)?.get();
            if a != b && b != d && d != a {
                c.add_triangle(a - 1, b - 1, d - 1)?;
            }
        }
        if t.has_uv_nodes() {
            for i in 0..nb_nodes {
                let uv = t.uv_node(i + 1)?;
                c.change_node(i)?.set_uv(uv.x(), uv.y());
            }
        }
        if t.has_normals() {
            for i in 0..nb_nodes {
                let n = t.normal(i + 1)?;
                c.change_node(i)?.set_normal(&XYZ::new(f64::from(n[0]), f64::from(n[1]), f64::from(n[2])));
            }
        }
        c.deflection = t.deflection();
        Ok(c)
    }

    /// `GetTriangulation()`: `None` without nodes or triangles.
    pub fn get_triangulation(&self) -> Result<Option<Triangulation>, MathError> {
        let (n_nodes, n_triangles) = (self.n_nodes(), self.n_triangles());
        if n_nodes == 0 || n_triangles == 0 {
            return Ok(None);
        }
        let mut result = Triangulation::with_sizes(n_nodes, n_triangles, false, false)?;
        let mut node_id = vec![0i32; self.nodes.len()];
        let mut count = 0;
        for (i, node) in self.nodes.iter().enumerate() {
            if node.is_free_node() {
                continue;
            }
            count += 1;
            if let Some(s) = node_id.get_mut(i) {
                *s = count;
            }
            let normal = node.get_normal();
            if normal.square_modulus() > CONFUSION {
                result.add_normals();
                let d = Dir::from_xyz(normal).map_err(|_| MathError::Domain)?;
                result.set_normal_dir(count, &d)?;
            }
            result.set_node(count, &Pnt::from_xyz(*node.xyz()))?;
            if node.get_u() * node.get_u() + node.get_v() * node.get_v() > CONFUSION {
                result.add_uv_nodes();
                result.set_uv_node(count, &Pnt2d::new(node.get_u(), node.get_v()))?;
            }
        }
        let id = |n: i32| usize::try_from(n).ok().and_then(|k| node_id.get(k)).copied().unwrap_or(0);
        let mut count = 0;
        for tri in &self.triangles {
            if !tri.is_empty() {
                count += 1;
                result.set_triangle(count, Triangle::from_nodes(id(tri.node(0)), id(tri.node(1)), id(tri.node(2))))?;
            }
        }
        result.set_deflection(self.deflection);
        Ok(Some(result))
    }

    /// `GetFreeNodes()`: whether there are some, and their slots.
    pub fn get_free_nodes(&self) -> (bool, Vec<i32>) {
        let v: Vec<i32> =
            self.nodes.iter().enumerate().filter(|(_, n)| n.is_free_node()).map(|(i, _)| i as i32).collect();
        (!v.is_empty(), v)
    }

    /// `RemoveDegenerated(theTol, pLstRemovedNode)`: the result and the list of `(ip1, im1)` pairs (empty unless
    /// `want_list`).
    pub fn remove_degenerated(&mut self, tol: f64, want_list: bool) -> Result<(bool, Vec<(i32, i32)>), MathError> {
        const IND: [usize; 5] = [2, 0, 1, 2, 0];
        let mut result = false;
        let tol2 = tol * tol;
        let mut list = Vec::new();
        let mut idx = 0;
        // the iterator of the triangle array ends where the array ended when it was created
        let end = self.triangles.len();
        while idx < end {
            let cur = idx;
            idx += 1;
            let tri = *self.triangles.get(cur).ok_or(MathError::Range)?;
            if tri.is_empty() {
                continue;
            }
            let p = [self.node(tri.node(0))?.xyz, self.node(tri.node(1))?.xyz, self.node(tri.node(2))?.xyz];
            let len2 = |a: &XYZ, b: &XYZ| a.subtracted(b).square_modulus();
            let a_len2 = [len2(&p[2], &p[1]), len2(&p[0], &p[2]), len2(&p[1], &p[0])];
            for (i, l2) in a_len2.iter().enumerate() {
                if *l2 < tol2 {
                    // ind[k] = IND[k + 1]
                    let ind = |k: usize| IND.get(k).copied().unwrap_or(0);
                    let im1 = tri.node(ind(i));
                    let ip1 = tri.node(ind(i + 2));
                    self.remove_triangle(cur)?;
                    while let Some(&conn) = self.node(ip1)?.ring.first() {
                        let mut new_tri = {
                            let t = self.triangles.get(conn).ok_or(MathError::Range)?;
                            [t.node(0), t.node(1), t.node(2)]
                        };
                        if conn != cur {
                            if new_tri[0] == ip1 {
                                new_tri[0] = im1;
                            } else if new_tri[1] == ip1 {
                                new_tri[1] = im1;
                            } else if new_tri[2] == ip1 {
                                new_tri[2] = im1;
                            }
                            self.remove_triangle(conn)?;
                            self.add_triangle(new_tri[0], new_tri[1], new_tri[2])?;
                        } else {
                            break;
                        }
                    }
                    if want_list {
                        list.push((ip1, im1));
                    }
                    result = true;
                    break;
                }
            }
        }
        Ok((result, list))
    }

    /// `MaxNode()`.
    pub fn max_node(&self) -> i32 {
        self.nodes.len() as i32 - 1
    }
    /// `MaxTriangle()`.
    pub fn max_triangle(&self) -> i32 {
        self.triangles.len() as i32 - 1
    }
    /// `SetDeflection()`.
    pub fn set_deflection(&mut self, d: f64) {
        self.deflection = d;
    }
    /// `Deflection()`.
    pub fn deflection(&self) -> f64 {
        self.deflection
    }
    /// `SetNode(thePnt, iN)`: `iN < 0` appends; `iN <= MaxNode() + 1` replaces (or appends); a larger one is `Err`.
    pub fn set_node(&mut self, p: &XYZ, i: i32) -> Result<i32, MathError> {
        if i < 0 {
            self.nodes.push(CoherentNode::from_xyz(p));
            return Ok(self.nodes.len() as i32 - 1);
        }
        let k = i as usize;
        if k > self.nodes.len() {
            return Err(MathError::Range);
        }
        if k == self.nodes.len() {
            self.nodes.push(CoherentNode::from_xyz(p));
        } else if let Some(slot) = self.nodes.get_mut(k) {
            *slot = CoherentNode::from_xyz(p);
        }
        Ok(i)
    }
    /// `Node(i)`.
    pub fn node(&self, i: i32) -> Result<&CoherentNode, MathError> {
        usize::try_from(i).ok().and_then(|k| self.nodes.get(k)).ok_or(MathError::Range)
    }
    /// `ChangeNode(i)`.
    pub fn change_node(&mut self, i: i32) -> Result<&mut CoherentNode, MathError> {
        usize::try_from(i).ok().and_then(|k| self.nodes.get_mut(k)).ok_or(MathError::Range)
    }
    /// `NNodes()`: the nodes that are not free.
    pub fn n_nodes(&self) -> i32 {
        self.nodes.iter().filter(|n| !n.is_free_node()).count() as i32
    }
    /// `Triangle(i)`.
    pub fn triangle(&self, i: i32) -> Result<&CoherentTriangle, MathError> {
        usize::try_from(i).ok().and_then(|k| self.triangles.get(k)).ok_or(MathError::Range)
    }
    /// `NTriangles()`: the triangles that are not empty.
    pub fn n_triangles(&self) -> i32 {
        self.triangles.iter().filter(|t| !t.is_empty()).count() as i32
    }
    /// `NLinks()`: the links that are not empty.
    pub fn n_links(&self) -> i32 {
        self.links.iter().filter(|l| !l.is_empty()).count() as i32
    }
    /// The number of link slots (`myLinks.Length()`).
    pub fn nb_link_slots(&self) -> usize {
        self.links.len()
    }
    /// The link of a slot.
    pub fn link(&self, slot: usize) -> Result<&CoherentLink, MathError> {
        self.links.get(slot).ok_or(MathError::Range)
    }

    /// `Poly_CoherentTriangle::RemoveConnection(iConn)`.
    fn remove_connection(&mut self, t: usize, i: usize) -> Result<(), MathError> {
        let Some(c) = self.triangles.get(t).ok_or(MathError::Range)?.connected.get(i).copied().flatten() else {
            return Ok(());
        };
        let ct = *self.triangles.get(c).ok_or(MathError::Range)?;
        let i1 = if ct.connected_triangle(0) != Some(t) {
            if ct.connected_triangle(1) == Some(t) {
                1
            } else if ct.connected_triangle(2) == Some(t) {
                2
            } else {
                return Err(program_error());
            }
        } else {
            0
        };
        {
            let o = tri!(self, c);
            o.set_connected_at(i1, None, -1);
            o.n_connections -= 1;
        }
        let s = tri!(self, t);
        s.set_connected_at(i, None, -1);
        s.n_connections -= 1;
        Ok(())
    }
    /// The common steps of `Poly_CoherentTriangle::SetConnection`: connect side `i` of `a` with side `j` of `b`.
    fn connect(&mut self, a: usize, i: usize, b: usize, j: usize) -> Result<(), MathError> {
        self.remove_connection(a, i)?;
        let nb = self.triangles.get(b).ok_or(MathError::Range)?.node(j);
        {
            let s = tri!(self, a);
            s.set_connected_at(i, Some(b), nb);
            s.n_connections += 1;
        }
        self.remove_connection(b, j)?;
        let na = self.triangles.get(a).ok_or(MathError::Range)?.node(i);
        let o = tri!(self, b);
        o.set_connected_at(j, Some(a), na);
        o.n_connections += 1;
        Ok(())
    }
    /// `Poly_CoherentTriangle::SetConnection(theTr)`.
    fn set_connection(&mut self, a: usize, b: usize) -> Result<bool, MathError> {
        let s = *self.triangles.get(a).ok_or(MathError::Range)?;
        let t = *self.triangles.get(b).ok_or(MathError::Range)?;
        let (sn, tn) = (s.nodes, t.nodes);
        let pick: Option<(usize, usize)> = if sn[0] == tn[0] {
            if sn[1] == tn[2] && s.connected[2] != Some(b) {
                Some((2, 1))
            } else if sn[2] == tn[1] && s.connected[1] != Some(b) {
                Some((1, 2))
            } else {
                None
            }
        } else if sn[0] == tn[1] {
            if sn[1] == tn[0] && s.connected[2] != Some(b) {
                Some((2, 2))
            } else if sn[2] == tn[2] && s.connected[1] != Some(b) {
                Some((1, 0))
            } else {
                None
            }
        } else if sn[0] == tn[2] {
            if sn[1] == tn[1] && s.connected[2] != Some(b) {
                Some((2, 0))
            } else if sn[2] == tn[0] && s.connected[1] != Some(b) {
                Some((1, 1))
            } else {
                None
            }
        } else if s.connected[0] != Some(b) {
            if sn[1] == tn[0] && sn[2] == tn[2] {
                Some((0, 1))
            } else if sn[1] == tn[2] && sn[2] == tn[1] {
                Some((0, 0))
            } else if sn[1] == tn[1] && sn[2] == tn[0] {
                Some((0, 2))
            } else {
                None
            }
        } else {
            None
        };
        match pick {
            Some((i, j)) => {
                self.connect(a, i, b, j)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// `RemoveTriangle(theTr)`: the triangle slot becomes empty.
    pub fn remove_triangle(&mut self, t: usize) -> Result<bool, MathError> {
        let mut result = false;
        if t >= self.triangles.len() {
            return Err(MathError::Range);
        }
        for i in 0..3usize {
            let tri = *self.triangles.get(t).ok_or(MathError::Range)?;
            let ni = tri.node(i);
            if ni >= 0 {
                if self.node(ni).is_err() {
                    return Err(MathError::Range);
                }
                if let Some(l) = tri.link(i) {
                    let mut to_remove = true;
                    if let Some(o) = tri.connected_triangle(i) {
                        let opp = *self.triangles.get(o).ok_or(MathError::Range)?;
                        for j in 0..3usize {
                            if Some(l) == opp.link(j) {
                                if let Some(lk) = self.links.get_mut(l) {
                                    if lk.opposite[0] == ni {
                                        lk.opposite[0] = -1;
                                        to_remove = false;
                                    } else if lk.opposite[1] == ni {
                                        lk.opposite[1] = -1;
                                        to_remove = false;
                                    }
                                }
                                break;
                            }
                        }
                    }
                    if to_remove {
                        self.remove_link(l)?;
                    }
                }
                let removed = self.change_node(ni)?.remove_triangle(t);
                if removed {
                    tri!(self, t).set_node_at(i, -1);
                    result = true;
                }
            }
            self.remove_connection(t, i)?;
        }
        Ok(result)
    }

    /// `RemoveLink(theLink)`: the link slot becomes empty.
    pub fn remove_link(&mut self, slot: usize) -> Result<(), MathError> {
        let link = *self.links.get(slot).ok_or(MathError::Range)?;
        let (found, p0, p1) = self.find_triangle_of(&link)?;
        if found {
            for (i, p) in [p0, p1].into_iter().enumerate() {
                let node = link.opposite_node(i);
                if let (true, Some(p)) = (node >= 0, p) {
                    let t = tri!(self, p);
                    if node == t.nodes[0] {
                        t.link[0] = None;
                    } else if node == t.nodes[1] {
                        t.link[1] = None;
                    } else if node == t.nodes[2] {
                        t.link[2] = None;
                    } else {
                        return Err(program_error());
                    }
                }
            }
        }
        if let Some(s) = self.links.get_mut(slot) {
            *s = CoherentLink::new();
        }
        Ok(())
    }

    /// `AddTriangle(iNode0, iNode1, iNode2)`: the slot of the new triangle, `None` for a negative node.
    pub fn add_triangle(&mut self, n0: i32, n1: i32, n2: i32) -> Result<Option<usize>, MathError> {
        if n0 >= 0 && n1 >= 0 && n2 >= 0 {
            self.triangles.push(CoherentTriangle::new());
            let slot = self.triangles.len() - 1;
            self.replace_nodes(slot, n0, n1, n2)?;
            Ok(Some(slot))
        } else {
            Ok(None)
        }
    }

    /// `ReplaceNodes(theTriangle, iNode0, iNode1, iNode2)`.
    pub fn replace_nodes(&mut self, t: usize, n0: i32, n1: i32, n2: i32) -> Result<bool, MathError> {
        if !self.triangles.get(t).ok_or(MathError::Range)?.is_empty() {
            self.remove_triangle(t)?;
        }
        if n0 >= 0 && n1 >= 0 && n2 >= 0 {
            *tri!(self, t) = CoherentTriangle::from_nodes(n0, n1, n2);
            for i in 0..3usize {
                let ni = self.triangles.get(t).ok_or(MathError::Range)?.node(i);
                let ring = self.node(ni)?.ring.clone();
                for u in ring {
                    self.set_connection(u, t)?;
                }
                self.change_node(ni)?.ring.push(t);
            }
            if !self.links.is_empty() {
                for i in 0..3usize {
                    let tri = *self.triangles.get(t).ok_or(MathError::Range)?;
                    let mut to_add = true;
                    if let Some(o) = tri.connected_triangle(i) {
                        let opp = *self.triangles.get(o).ok_or(MathError::Range)?;
                        for j in 0..3usize {
                            if tri.node(i) != opp.connected_node(j) {
                                continue;
                            }
                            if let Some(lk) = opp.link(j).and_then(|l| self.links.get_mut(l)) {
                                if lk.opposite[0] == opp.node(j) {
                                    lk.opposite[1] = tri.node(i);
                                    to_add = false;
                                } else if lk.opposite[1] == opp.node(j) {
                                    lk.opposite[0] = tri.node(i);
                                    to_add = false;
                                }
                            }
                        }
                    }
                    if to_add {
                        self.add_link(t, i as i32)?;
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// `AddLink(theTri, theConn)`: the slot of the new link, `None` for an empty triangle.
    pub fn add_link(&mut self, t: usize, conn: i32) -> Result<Option<usize>, MathError> {
        let tri = *self.triangles.get(t).ok_or(MathError::Range)?;
        if tri.is_empty() {
            return Ok(None);
        }
        let link = CoherentLink::from_triangle(&tri, conn)?;
        self.links.push(link);
        let slot = self.links.len() - 1;
        let c = conn as usize;
        tri!(self, t).set_link_at(c, Some(slot));
        let Some(o) = tri.connected_triangle(c) else { return Ok(Some(slot)) };
        let opp = *self.triangles.get(o).ok_or(MathError::Range)?;
        if opp.is_empty() {
            return Ok(Some(slot));
        }
        let want = tri.connected_node(c);
        let k = if opp.nodes[0] == want {
            0
        } else if opp.nodes[1] == want {
            1
        } else if opp.nodes[2] == want {
            2
        } else {
            return Err(program_error());
        };
        tri!(self, o).set_link_at(k, Some(slot));
        Ok(Some(slot))
    }

    fn find_triangle_of(&self, link: &CoherentLink) -> Result<(bool, Option<usize>, Option<usize>), MathError> {
        let (mut p0, mut p1) = (None, None);
        let n0 = link.node(0);
        let nlen = self.nodes.len() as i32;
        if !link.is_empty() && n0 < nlen && link.node(1) < nlen {
            let n1 = link.node(1);
            for &u in &self.node(n0)?.ring {
                let t = self.triangles.get(u).ok_or(MathError::Range)?;
                let (a, b, c) = (t.node(0), t.node(1), t.node(2));
                if a == n0 {
                    if b == n1 {
                        p0 = Some(u);
                    } else if c == n1 {
                        p1 = Some(u);
                    }
                } else if b == n0 {
                    if c == n1 {
                        p0 = Some(u);
                    } else if a == n1 {
                        p1 = Some(u);
                    }
                } else if c == n0 {
                    if a == n1 {
                        p0 = Some(u);
                    } else if b == n1 {
                        p1 = Some(u);
                    }
                } else {
                    return Err(program_error());
                }
                if p0.is_some() && p1.is_some() {
                    break;
                }
            }
        }
        Ok((p0.is_some() || p1.is_some(), p0, p1))
    }

    /// `FindTriangle(theLink, pTri)` for the link of a slot: the result and the two triangles.
    pub fn find_triangle(&self, slot: usize) -> Result<(bool, Option<usize>, Option<usize>), MathError> {
        let link = *self.links.get(slot).ok_or(MathError::Range)?;
        self.find_triangle_of(&link)
    }

    /// `ComputeLinks()`: the number of link slots.
    pub fn compute_links(&mut self) -> Result<i32, MathError> {
        self.links.clear();
        for t in 0..self.triangles.len() {
            let tri = *self.triangles.get(t).ok_or(MathError::Range)?;
            if tri.is_empty() {
                continue;
            }
            if tri.node(0) < tri.node(1) {
                self.add_link(t, 2)?;
            }
            if tri.node(1) < tri.node(2) {
                self.add_link(t, 0)?;
            }
            if tri.node(2) < tri.node(0) {
                self.add_link(t, 1)?;
            }
        }
        for t in 0..self.triangles.len() {
            let tri = *self.triangles.get(t).ok_or(MathError::Range)?;
            if tri.is_empty() {
                continue;
            }
            for i in 0..3usize {
                let missing = self.triangles.get(t).ok_or(MathError::Range)?.link(i).is_none();
                if missing {
                    self.add_link(t, i as i32)?;
                }
            }
        }
        Ok(self.links.len() as i32)
    }

    /// `ClearLinks()`.
    pub fn clear_links(&mut self) {
        self.links.clear();
        for t in &mut self.triangles {
            t.link = [None; 3];
        }
    }

    /// `Clone()`: `None` unless there are nodes and triangles.
    pub fn clone_coherent(&self) -> Result<Option<CoherentTriangulation>, MathError> {
        if self.n_triangles() != 0 && self.n_nodes() != 0 {
            let t = self.get_triangulation()?;
            let mut c = Self::from_triangulation(t.as_ref())?;
            if let Some(t) = &t {
                c.set_deflection(t.deflection());
            }
            Ok(Some(c))
        } else {
            Ok(None)
        }
    }

    /// `Dump()`.
    pub fn dump(&self) -> String {
        let mut s = String::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if n.is_free_node() {
                continue;
            }
            s.push_str(&format!("Node {i}"));
            s.push_str(&format!("  X ={}; Y ={}; Z ={}\n", fixed(n.x(), 9, 4), fixed(n.y(), 9, 4), fixed(n.z(), 9, 4)));
            for &u in &n.ring {
                if let Some(t) = self.triangles.get(u) {
                    s.push_str(&format!("      {:>5} {:>5} {:>5}\n", t.node(0), t.node(1), t.node(2)));
                }
            }
        }
        s
    }
}
