//! `Poly_Triangle`.
use crate::mathlin::MathError;

/// `Poly_Triangle`: three node indices (1-based by convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Triangle {
    nodes: [i32; 3],
}

impl Triangle {
    /// `Poly_Triangle()`.
    pub fn new() -> Self {
        Self::default()
    }
    /// `Poly_Triangle(N1, N2, N3)`.
    pub fn from_nodes(n1: i32, n2: i32, n3: i32) -> Self {
        Self { nodes: [n1, n2, n3] }
    }
    /// `Set(N1, N2, N3)`.
    pub fn set(&mut self, n1: i32, n2: i32, n3: i32) {
        self.nodes = [n1, n2, n3];
    }
    /// `Set(Index, Node)` (index 1..=3).
    pub fn set_index(&mut self, index: i32, node: i32) -> Result<(), MathError> {
        *self.slot(index)? = node;
        Ok(())
    }
    /// `Get()`.
    pub fn get(&self) -> (i32, i32, i32) {
        let [a, b, c] = self.nodes;
        (a, b, c)
    }
    /// `Value(Index)` (index 1..=3).
    pub fn value(&self, index: i32) -> Result<i32, MathError> {
        usize::try_from(index - 1).ok().and_then(|i| self.nodes.get(i)).copied().ok_or(MathError::Range)
    }
    fn slot(&mut self, index: i32) -> Result<&mut i32, MathError> {
        usize::try_from(index - 1).ok().and_then(|i| self.nodes.get_mut(i)).ok_or(MathError::Range)
    }
}
