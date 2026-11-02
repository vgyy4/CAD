//! `AdvApp2Var_Node`: a grid node with true points and errors.

use super::{Arr2, Result};
use crate::gp::{Pnt, XY};

/// `AdvApp2Var_Node`
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    true_points: Arr2<Pnt>,
    errors: Arr2<f64>,
    coord: XY,
    ord_in_u: i32,
    ord_in_v: i32,
}

impl Node {
    /// `AdvApp2Var_Node::AdvApp2Var_Node()`
    pub fn new_0() -> Self {
        Self {
            true_points: Arr2::new(0, 2, 0, 2, Pnt::new(0.0, 0.0, 0.0)),
            errors: Arr2::new(0, 2, 0, 2, 0.0),
            coord: XY::default(),
            ord_in_u: 2,
            ord_in_v: 2,
        }
    }

    /// `AdvApp2Var_Node::AdvApp2Var_Node(const int iu, const int iv)`
    pub fn new_2(iu: i32, iv: i32) -> Self {
        Self {
            true_points: Arr2::new(0, iu.max(0), 0, iv.max(0), Pnt::new(0.0, 0.0, 0.0)),
            errors: Arr2::new(0, iu.max(0), 0, iv.max(0), 0.0),
            coord: XY::default(),
            ord_in_u: iu,
            ord_in_v: iv,
        }
    }

    /// `AdvApp2Var_Node::AdvApp2Var_Node(const gp_XY& UV, const int iu, const int iv)`
    pub fn new_3(uv: &XY, iu: i32, iv: i32) -> Self {
        Self {
            true_points: Arr2::new(0, iu, 0, iv, Pnt::new(0.0, 0.0, 0.0)),
            errors: Arr2::new(0, iu, 0, iv, 0.0),
            coord: *uv,
            ord_in_u: iu,
            ord_in_v: iv,
        }
    }

    /// `Coord()`
    pub fn coord(&self) -> XY {
        self.coord
    }

    /// `SetCoord(x1, x2)`
    pub fn set_coord(&mut self, x1: f64, x2: f64) {
        self.coord.set_x(x1);
        self.coord.set_y(x2);
    }

    /// `UOrder()`
    pub fn u_order(&self) -> i32 {
        self.ord_in_u
    }

    /// `VOrder()`
    pub fn v_order(&self) -> i32 {
        self.ord_in_v
    }

    /// `SetPoint(iu, iv, Pt)`
    pub fn set_point(&mut self, iu: i32, iv: i32, pt: &Pnt) -> Result<()> {
        self.true_points.set(iu, iv, *pt)
    }

    /// `Point(iu, iv)`
    pub fn point(&self, iu: i32, iv: i32) -> Result<Pnt> {
        self.true_points.get(iu, iv).copied()
    }

    /// `SetError(iu, iv, error)`
    pub fn set_error(&mut self, iu: i32, iv: i32, error: f64) -> Result<()> {
        self.errors.set(iu, iv, error)
    }

    /// `Error(iu, iv)`
    pub fn error(&self, iu: i32, iv: i32) -> Result<f64> {
        self.errors.get(iu, iv).copied()
    }
}
