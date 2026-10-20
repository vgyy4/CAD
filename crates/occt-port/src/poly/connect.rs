//! `Poly_Connect`: adjacency of the triangles and nodes of a triangulation.
use super::Triangulation;
use crate::mathlin::MathError;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The `polyedge` of `Poly_Connect.cxx`.
struct PolyEdge {
    nt: [i32; 2],
    nn: [i32; 2],
}

/// `array(i)` of a 1-based `NCollection_Array1<int>` with its range check.
fn get1(a: &[i32], i: i32) -> Result<i32, MathError> {
    let k = usize::try_from(i.wrapping_sub(1)).map_err(|_| MathError::Range)?;
    a.get(k).copied().ok_or(MathError::Range)
}

fn set1(a: &mut [i32], i: i32, v: i32) -> Result<(), MathError> {
    let k = usize::try_from(i.wrapping_sub(1)).map_err(|_| MathError::Range)?;
    *a.get_mut(k).ok_or(MathError::Range)? = v;
    Ok(())
}

/// `Poly_Connect`.
#[derive(Debug, Default)]
pub struct Connect {
    triangulation: Option<Rc<Triangulation>>,
    triangles: Vec<i32>,
    adjacents: Vec<i32>,
    mytr: i32,
    myfirst: i32,
    mynode: i32,
    myothernode: i32,
    mysense: bool,
    mymore: bool,
    passed: HashSet<i32>,
}

impl Connect {
    /// `Poly_Connect()`.
    pub fn new() -> Self {
        Self::default()
    }
    /// `Poly_Connect(theTriangulation)`.
    pub fn from_triangulation(t: &Rc<Triangulation>) -> Result<Self, MathError> {
        let mut c = Self::new();
        c.load(t)?;
        Ok(c)
    }
    /// `Load()`.
    pub fn load(&mut self, t: &Rc<Triangulation>) -> Result<(), MathError> {
        self.triangulation = Some(t.clone());
        self.mytr = 0;
        self.myfirst = 0;
        self.mynode = 0;
        self.myothernode = 0;
        self.mysense = false;
        self.mymore = false;
        let nb_nodes = t.nb_nodes();
        let nb_tris = t.nb_triangles();
        let nb_adjs = 6 * nb_tris as usize;
        self.triangles = vec![0; nb_nodes as usize];
        self.adjacents = vec![0; nb_adjs];
        let mut edges: HashMap<(i32, i32), PolyEdge> = HashMap::new();
        for tri_iter in 1..=nb_tris {
            let (a, b, c) = t.triangle(tri_iter)?.get();
            let tn = [a, b, c];
            for n in tn {
                set1(&mut self.triangles, n, tri_iter)?;
            }
            for node_in_tri in 0..3usize {
                let node_next = (node_in_tri + 1) % 3;
                let (cur, nxt) = (tn.get(node_in_tri).copied().unwrap_or(0), tn.get(node_next).copied().unwrap_or(0));
                let key = if cur < nxt { (cur, nxt) } else { (nxt, cur) };
                let third = tn.get(3 - node_in_tri - node_next).copied().unwrap_or(0);
                match edges.get_mut(&key) {
                    Some(e) => {
                        e.nt[1] = tri_iter;
                        e.nn[1] = third;
                    }
                    None => {
                        edges.insert(key, PolyEdge { nt: [tri_iter, 0], nn: [third, 0] });
                    }
                }
            }
        }
        let mut adj_index = 1;
        for tri_iter in 1..=nb_tris {
            let (a, b, c) = t.triangle(tri_iter)?.get();
            let tn = [a, b, c];
            for node_in_tri in 0..3usize {
                let node_next = (node_in_tri + 1) % 3;
                let (cur, nxt) = (tn.get(node_in_tri).copied().unwrap_or(0), tn.get(node_next).copied().unwrap_or(0));
                let key = if cur < nxt { (cur, nxt) } else { (nxt, cur) };
                let e = edges.get(&key).ok_or(MathError::Range)?;
                let l = usize::from(e.nt[0] == tri_iter);
                set1(&mut self.adjacents, adj_index, e.nt.get(l).copied().unwrap_or(0))?;
                set1(&mut self.adjacents, adj_index + 3, e.nn.get(l).copied().unwrap_or(0))?;
                adj_index += 1;
            }
            adj_index += 3;
        }
        Ok(())
    }
    /// `Triangulation()`.
    pub fn triangulation(&self) -> Option<&Rc<Triangulation>> {
        self.triangulation.as_ref()
    }
    /// `Triangle(N)`: a triangle using the node N (the last one written), 0 if none.
    pub fn triangle(&self, node: i32) -> Result<i32, MathError> {
        get1(&self.triangles, node)
    }
    /// `Triangles(T, t1, t2, t3)`: the neighbours over the edges `n1n2`, `n2n3`, `n3n1`.
    pub fn triangles(&self, t: i32) -> Result<(i32, i32, i32), MathError> {
        let index = 6i32.wrapping_mul(t.wrapping_sub(1));
        Ok((
            get1(&self.adjacents, index.wrapping_add(1))?,
            get1(&self.adjacents, index.wrapping_add(2))?,
            get1(&self.adjacents, index.wrapping_add(3))?,
        ))
    }
    /// `Nodes(T, n1, n2, n3)`: the node opposite each edge.
    pub fn nodes(&self, t: i32) -> Result<(i32, i32, i32), MathError> {
        let index = 6i32.wrapping_mul(t.wrapping_sub(1));
        Ok((
            get1(&self.adjacents, index.wrapping_add(4))?,
            get1(&self.adjacents, index.wrapping_add(5))?,
            get1(&self.adjacents, index.wrapping_add(6))?,
        ))
    }
    fn tri_nodes(&self, t: i32) -> Result<[i32; 3], MathError> {
        let tri = self.triangulation.as_ref().ok_or(MathError::Domain)?;
        let (a, b, c) = tri.triangle(t)?.get();
        Ok([a, b, c])
    }
    /// `Initialize(N)`.
    pub fn initialize(&mut self, n: i32) -> Result<(), MathError> {
        self.mynode = n;
        self.myfirst = self.triangle(n)?;
        self.mytr = self.myfirst;
        self.mysense = true;
        self.mymore = self.myfirst != 0;
        self.passed.clear();
        self.passed.insert(self.mytr);
        if self.mymore {
            let no = self.tri_nodes(self.myfirst)?;
            let mut i = 0;
            while i < 3 && no.get(i).copied() != Some(self.mynode) {
                i += 1;
            }
            self.myothernode = no.get((i + 2) % 3).copied().unwrap_or(0);
        }
        Ok(())
    }
    /// `More()`.
    pub fn more(&self) -> bool {
        self.mymore
    }
    /// `Value()`.
    pub fn value(&self) -> i32 {
        self.mytr
    }
    /// `Next()`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<(), MathError> {
        let (t0, t1, t2) = self.triangles(self.mytr)?;
        let mut t = [t0, t1, t2];
        if self.mysense {
            for i in 0..3 {
                let ti = t.get(i).copied().unwrap_or(0);
                if ti != 0 {
                    let n = self.tri_nodes(ti)?;
                    for j in 0..3 {
                        if n.get(j).copied() == Some(self.mynode)
                            && n.get((j + 1) % 3).copied() == Some(self.myothernode)
                        {
                            self.mytr = ti;
                            self.myothernode = n.get((j + 2) % 3).copied().unwrap_or(0);
                            self.mymore = !self.passed.contains(&self.mytr);
                            self.passed.insert(self.mytr);
                            return Ok(());
                        }
                    }
                }
            }
            let n = self.tri_nodes(self.myfirst)?;
            let mut i = 0;
            while i < 3 && n.get(i).copied() != Some(self.mynode) {
                i += 1;
            }
            self.myothernode = n.get((i + 1) % 3).copied().unwrap_or(0);
            self.mysense = false;
            self.mytr = self.myfirst;
            let (a, b, c) = self.triangles(self.mytr)?;
            t = [a, b, c];
        }
        if !self.mysense {
            for i in 0..3 {
                let ti = t.get(i).copied().unwrap_or(0);
                if ti != 0 {
                    let n = self.tri_nodes(ti)?;
                    for j in 0..3 {
                        if n.get(j).copied() == Some(self.mynode)
                            && n.get((j + 2) % 3).copied() == Some(self.myothernode)
                        {
                            self.mytr = ti;
                            self.myothernode = n.get((j + 1) % 3).copied().unwrap_or(0);
                            self.mymore = !self.passed.contains(&self.mytr);
                            self.passed.insert(self.mytr);
                            return Ok(());
                        }
                    }
                }
            }
        }
        self.mymore = false;
        Ok(())
    }
}
