//! `Poly_Polygon3D`, `Poly_Polygon2D` and `Poly_PolygonOnTriangulation`.
use crate::gp::{Pnt, Pnt2d};
use crate::mathlin::MathError;

fn len_of(n: i32) -> Result<usize, MathError> {
    usize::try_from(n).map_err(|_| MathError::Range)
}

fn get1<T: Copy>(a: &[T], lower: i32, i: i32) -> Result<T, MathError> {
    let k = usize::try_from(i.wrapping_sub(lower)).map_err(|_| MathError::Range)?;
    a.get(k).copied().ok_or(MathError::Range)
}

fn set1<T>(a: &mut [T], lower: i32, i: i32, v: T) -> Result<(), MathError> {
    let k = usize::try_from(i.wrapping_sub(lower)).map_err(|_| MathError::Range)?;
    *a.get_mut(k).ok_or(MathError::Range)? = v;
    Ok(())
}

/// `Poly_Polygon3D`: 3D nodes (1-based) and optional parameters.
#[derive(Debug, Clone)]
pub struct Polygon3D {
    deflection: f64,
    nodes: Vec<Pnt>,
    parameters: Option<Vec<f64>>,
}

impl Polygon3D {
    /// `Poly_Polygon3D(NbNodes, HasParams)`.
    pub fn with_size(nb_nodes: i32, has_params: bool) -> Result<Self, MathError> {
        let n = len_of(nb_nodes)?;
        Ok(Self { deflection: 0.0, nodes: vec![Pnt::default(); n], parameters: has_params.then(|| vec![0.0; n]) })
    }
    /// `Poly_Polygon3D(Nodes)`.
    pub fn from_nodes(nodes: &[Pnt]) -> Self {
        Self { deflection: 0.0, nodes: nodes.to_vec(), parameters: None }
    }
    /// `Poly_Polygon3D(Nodes, Parameters)`: `Err` unless equal lengths and equal lower bounds.
    pub fn from_nodes_parameters(
        nodes: &[Pnt],
        nodes_lower: i32,
        params: &[f64],
        params_lower: i32,
    ) -> Result<Self, MathError> {
        if nodes.len() != params.len() || nodes_lower != params_lower {
            return Err(MathError::Dimension);
        }
        Ok(Self { deflection: 0.0, nodes: nodes.to_vec(), parameters: Some(params.to_vec()) })
    }
    /// `Deflection()`.
    pub fn deflection(&self) -> f64 {
        self.deflection
    }
    /// `Deflection(value)`.
    pub fn set_deflection(&mut self, d: f64) {
        self.deflection = d;
    }
    /// `NbNodes()`.
    pub fn nb_nodes(&self) -> i32 {
        self.nodes.len() as i32
    }
    /// `Nodes()`.
    pub fn nodes(&self) -> &[Pnt] {
        &self.nodes
    }
    /// `ChangeNodes()`.
    pub fn change_nodes(&mut self) -> &mut [Pnt] {
        &mut self.nodes
    }
    /// `HasParameters()`.
    pub fn has_parameters(&self) -> bool {
        self.parameters.is_some()
    }
    /// `Parameters()` (none: `Err`).
    pub fn parameters(&self) -> Result<&[f64], MathError> {
        self.parameters.as_deref().ok_or(MathError::Domain)
    }
    /// `ChangeParameters()` (none: `Err`).
    pub fn change_parameters(&mut self) -> Result<&mut [f64], MathError> {
        self.parameters.as_deref_mut().ok_or(MathError::Domain)
    }
}

/// `Poly_Polygon2D`.
#[derive(Debug, Clone)]
pub struct Polygon2D {
    deflection: f64,
    nodes: Vec<Pnt2d>,
}

impl Polygon2D {
    /// `Poly_Polygon2D(NbNodes)`.
    pub fn with_size(nb_nodes: i32) -> Result<Self, MathError> {
        Ok(Self { deflection: 0.0, nodes: vec![Pnt2d::default(); len_of(nb_nodes)?] })
    }
    /// `Poly_Polygon2D(Nodes)`.
    pub fn from_nodes(nodes: &[Pnt2d]) -> Self {
        Self { deflection: 0.0, nodes: nodes.to_vec() }
    }
    /// `Deflection()`.
    pub fn deflection(&self) -> f64 {
        self.deflection
    }
    /// `Deflection(value)`.
    pub fn set_deflection(&mut self, d: f64) {
        self.deflection = d;
    }
    /// `NbNodes()`.
    pub fn nb_nodes(&self) -> i32 {
        self.nodes.len() as i32
    }
    /// `Nodes()`.
    pub fn nodes(&self) -> &[Pnt2d] {
        &self.nodes
    }
    /// `ChangeNodes()`.
    pub fn change_nodes(&mut self) -> &mut [Pnt2d] {
        &mut self.nodes
    }
}

/// `Poly_PolygonOnTriangulation`: node indices and optional parameters, each array keeping the lower bound of the
/// array it was built from.
#[derive(Debug)]
pub struct PolygonOnTriangulation {
    deflection: f64,
    nodes: Vec<i32>,
    nodes_lower: i32,
    parameters: Option<(Vec<f64>, i32)>,
}

impl Clone for PolygonOnTriangulation {
    /// `Copy()`: new 1-based arrays of `NbNodes()` elements, the first elements copied (`CopyValues`).
    fn clone(&self) -> Self {
        let n = self.nodes.len();
        let parameters = self.parameters.as_ref().map(|(p, _)| {
            let mut q = vec![0.0; n];
            for (d, s) in q.iter_mut().zip(p) {
                *d = *s;
            }
            (q, 1)
        });
        Self { deflection: self.deflection, nodes: self.nodes.clone(), nodes_lower: 1, parameters }
    }
}

impl PolygonOnTriangulation {
    /// `Poly_PolygonOnTriangulation(NbNodes, HasParams)`.
    pub fn with_size(nb_nodes: i32, has_params: bool) -> Result<Self, MathError> {
        let n = len_of(nb_nodes)?;
        Ok(Self {
            deflection: 0.0,
            nodes: vec![0; n],
            nodes_lower: 1,
            parameters: has_params.then(|| (vec![0.0; n], 1)),
        })
    }
    /// `Poly_PolygonOnTriangulation(Nodes)`: the nodes array keeps the lower bound `lower`.
    pub fn from_nodes(nodes: &[i32], lower: i32) -> Self {
        Self { deflection: 0.0, nodes: nodes.to_vec(), nodes_lower: lower, parameters: None }
    }
    /// `Poly_PolygonOnTriangulation(Nodes, Parameters)`: both arrays keep the lower bounds of their sources.
    pub fn from_nodes_parameters(nodes: &[i32], nodes_lower: i32, params: &[f64], params_lower: i32) -> Self {
        Self { deflection: 0.0, nodes: nodes.to_vec(), nodes_lower, parameters: Some((params.to_vec(), params_lower)) }
    }
    /// `Deflection()`.
    pub fn deflection(&self) -> f64 {
        self.deflection
    }
    /// `Deflection(value)`.
    pub fn set_deflection(&mut self, d: f64) {
        self.deflection = d;
    }
    /// `NbNodes()`.
    pub fn nb_nodes(&self) -> i32 {
        self.nodes.len() as i32
    }
    /// The lower bound of the nodes array.
    pub fn nodes_lower(&self) -> i32 {
        self.nodes_lower
    }
    /// The upper bound of the nodes array.
    pub fn nodes_upper(&self) -> i32 {
        self.nodes_lower.wrapping_add(self.nodes.len() as i32).wrapping_sub(1)
    }
    /// `Node(i)`.
    pub fn node(&self, i: i32) -> Result<i32, MathError> {
        get1(&self.nodes, self.nodes_lower, i)
    }
    /// `SetNode(i, n)`.
    pub fn set_node(&mut self, i: i32, n: i32) -> Result<(), MathError> {
        set1(&mut self.nodes, self.nodes_lower, i, n)
    }
    /// `HasParameters()`.
    pub fn has_parameters(&self) -> bool {
        self.parameters.is_some()
    }
    /// The bounds of the parameters array.
    pub fn parameters_bounds(&self) -> Option<(i32, i32)> {
        self.parameters.as_ref().map(|(p, lo)| (*lo, lo.wrapping_add(p.len() as i32).wrapping_sub(1)))
    }
    /// `Parameter(i)` (none: `Err`).
    pub fn parameter(&self, i: i32) -> Result<f64, MathError> {
        let (p, lo) = self.parameters.as_ref().ok_or(MathError::Domain)?;
        get1(p, *lo, i)
    }
    /// `SetParameter(i, v)` (none: `Err`).
    pub fn set_parameter(&mut self, i: i32, v: f64) -> Result<(), MathError> {
        let (p, lo) = self.parameters.as_mut().ok_or(MathError::Domain)?;
        set1(p, *lo, i, v)
    }
    /// `SetParameters(theParameters)`: an array whose bounds differ from the nodes' is `Err`; `None` removes them.
    pub fn set_parameters(&mut self, params: Option<(i32, Vec<f64>)>) -> Result<(), MathError> {
        match params {
            None => self.parameters = None,
            Some((lower, values)) => {
                let upper = lower.wrapping_add(values.len() as i32).wrapping_sub(1);
                if lower != self.nodes_lower || upper != self.nodes_upper() {
                    return Err(MathError::Range);
                }
                self.parameters = Some((values, lower));
            }
        }
        Ok(())
    }
}
