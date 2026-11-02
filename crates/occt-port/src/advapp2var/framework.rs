//! `AdvApp2Var_Framework`: the constraint frame (corner nodes and the iso strips).

use super::{Iso, Node, Result, Seq, feq};
use crate::adaptor3d::GeomAbsIsoType as IsoType;
use crate::gp::XY;

/// `AdvApp2Var_Framework`. Handles of the C++ sequences are owned values here.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Framework {
    node_constraints: Seq<Node>,
    u_constraints: Seq<Seq<Iso>>,
    v_constraints: Seq<Seq<Iso>>,
}

/// `findStripByBounds`: the first strip whose first iso has the bounds `[first, last]`
/// (the last strip when none does).
fn find_strip_by_bounds(strips: &Seq<Seq<Iso>>, first: f64, last: f64) -> Result<i32> {
    let mut idx: i32 = 1;
    while idx < strips.len() {
        let f = strips.value(idx)?.first()?;
        if feq(f.t0(), first) && feq(f.t1(), last) {
            break;
        }
        idx += 1;
    }
    Ok(idx)
}

/// `findIsoByConstant`: the first iso (among `nb_iso`) of the strip with that constant.
fn find_iso_by_constant(strip: &Seq<Iso>, nb_iso: i32, cte: f64) -> Result<i32> {
    let mut idx: i32 = 1;
    while idx <= nb_iso && !feq(strip.value(idx)?.constante(), cte) {
        idx += 1;
    }
    Ok(idx)
}

impl Framework {
    /// `AdvApp2Var_Framework::AdvApp2Var_Framework()`
    pub fn new_0() -> Self {
        Self::default()
    }

    /// `FirstNode(Type, IndexIso, IndexStrip)`
    pub fn first_node(&self, ty: IsoType, index_iso: i32, index_strip: i32) -> i32 {
        let nb_iso_in_v = self.u_constraints.len() + 1;
        if ty == IsoType::IsoU {
            return nb_iso_in_v * (index_strip - 1) + index_iso;
        }
        nb_iso_in_v * (index_iso - 1) + index_strip
    }

    /// `LastNode(Type, IndexIso, IndexStrip)`
    pub fn last_node(&self, ty: IsoType, index_iso: i32, index_strip: i32) -> i32 {
        let nb_iso_in_v = self.u_constraints.len() + 1;
        if ty == IsoType::IsoU {
            return nb_iso_in_v * index_strip + index_iso;
        }
        nb_iso_in_v * (index_iso - 1) + index_strip + 1
    }

    /// `Node(U, V)`: the node at `(U, V)`, else the last node of the frame.
    pub(crate) fn node(&self, u: f64, v: f64) -> Result<&Node> {
        for node in self.node_constraints.iter() {
            let c: XY = node.coord();
            if feq(c.x(), u) && feq(c.y(), v) {
                return Ok(node);
            }
        }
        self.node_constraints.last()
    }

    /// `IsoU(U, V0, V1)`: the iso of the V strip `[V0, V1]` at constant `U`.
    pub(crate) fn iso_u(&self, u: f64, v0: f64, v1: f64) -> Result<&Iso> {
        let strip_index = find_strip_by_bounds(&self.v_constraints, v0, v1)?;
        let strip = self.v_constraints.value(strip_index)?;
        let iso_index = find_iso_by_constant(strip, self.u_constraints.len(), u)?;
        strip.value(iso_index)
    }

    /// `IsoV(U0, U1, V)`: the iso of the U strip `[U0, U1]` at constant `V`.
    pub(crate) fn iso_v(&self, u0: f64, u1: f64, v: f64) -> Result<&Iso> {
        let strip_index = find_strip_by_bounds(&self.u_constraints, u0, u1)?;
        let strip = self.u_constraints.value(strip_index)?;
        let iso_index = find_iso_by_constant(strip, self.v_constraints.len(), v)?;
        strip.value(iso_index)
    }

    /// `UpdateInU(CuttingValue)`: splits the frame at a cutting value in U.
    pub fn update_in_u(&mut self, cutting_value: f64) -> Result<()> {
        // the strip containing the cutting value (the first strip whose U range holds it)
        let mut u_strip_index: i32 = 1;
        for strip in self.u_constraints.iter() {
            let f = strip.first()?;
            if f.u0() <= cutting_value && f.u1() >= cutting_value {
                break;
            }
            u_strip_index += 1;
        }
        {
            let strip = self.u_constraints.value_mut(u_strip_index)?;
            let u_first = strip.first()?.u0();
            let u_last = strip.first()?.u1();
            for iso in strip.iter_mut() {
                iso.change_domain_2(u_first, cutting_value);
                iso.reset_approx();
            }
            let mut new_strip = Seq::default();
            for iso in strip.iter() {
                let mut new_iso = Iso::new_9(
                    iso.type_(),
                    iso.constante(),
                    cutting_value,
                    u_last,
                    iso.v0(),
                    iso.v1(),
                    0,
                    iso.u_order(),
                    iso.v_order(),
                );
                new_iso.reset_approx();
                new_strip.append(new_iso);
            }
            self.u_constraints.insert_after(u_strip_index, new_strip)?;
        }
        for v_strip_index in 1..=self.v_constraints.len() {
            let strip = self.v_constraints.value_mut(v_strip_index)?;
            let iso = strip.value(u_strip_index)?;
            let (ty, u0, v0, v1) = (iso.type_(), iso.u0(), iso.v0(), iso.v1());
            let (uo, vo) = (iso.u_order(), iso.v_order());
            let iso = strip.value_mut(u_strip_index)?;
            iso.change_domain_4(iso.u0(), cutting_value, iso.v0(), iso.v1());
            let mut new_iso = Iso::new_9(ty, cutting_value, u0, cutting_value, v0, v1, 0, uo, vo);
            new_iso.reset_approx();
            strip.insert_after(u_strip_index, new_iso)?;
            let iso = strip.value_mut(u_strip_index + 2)?;
            iso.change_domain_4(cutting_value, iso.u1(), iso.v0(), iso.v1());
        }
        let mut prev: Node = self.node_constraints.first()?.clone();
        // the bound is re-read each time: insertions grow the sequence
        let mut node_index: i32 = 1;
        while node_index < self.node_constraints.len() {
            let next: Node = self.node_constraints.value(node_index + 1)?.clone();
            if prev.coord().x() < cutting_value
                && next.coord().x() > cutting_value
                && feq(prev.coord().y(), next.coord().y())
            {
                let new_uv = XY::new(cutting_value, prev.coord().y());
                let new_node = Node::new_3(&new_uv, prev.u_order(), prev.v_order());
                self.node_constraints.insert_after(node_index, new_node)?;
            }
            prev = next;
            node_index += 1;
        }
        Ok(())
    }

    /// `UpdateInV(CuttingValue)`: splits the frame at a cutting value in V.
    pub fn update_in_v(&mut self, cutting_value: f64) -> Result<()> {
        let mut v_strip_index: i32 = 1;
        loop {
            let f = self.v_constraints.value(v_strip_index)?.first()?;
            if !(f.v0() > cutting_value || f.v1() < cutting_value) {
                break;
            }
            v_strip_index += 1;
        }
        {
            let strip = self.v_constraints.value_mut(v_strip_index)?;
            let v_first = strip.first()?.v0();
            let v_last = strip.first()?.v1();
            for iso in strip.iter_mut() {
                iso.change_domain_2(v_first, cutting_value);
                iso.reset_approx();
            }
            let mut new_strip = Seq::default();
            for iso in strip.iter() {
                let mut new_iso = Iso::new_9(
                    iso.type_(),
                    iso.constante(),
                    iso.u0(),
                    iso.u1(),
                    cutting_value,
                    v_last,
                    0,
                    iso.u_order(),
                    iso.v_order(),
                );
                new_iso.reset_approx();
                new_strip.append(new_iso);
            }
            self.v_constraints.insert_after(v_strip_index, new_strip)?;
        }
        for u_strip in self.u_constraints.iter_mut() {
            let iso = u_strip.value(v_strip_index)?;
            let (ty, u0, u1, v0) = (iso.type_(), iso.u0(), iso.u1(), iso.v0());
            let (uo, vo) = (iso.u_order(), iso.v_order());
            let iso = u_strip.value_mut(v_strip_index)?;
            iso.change_domain_4(iso.u0(), iso.u1(), iso.v0(), cutting_value);
            let mut new_iso = Iso::new_9(ty, cutting_value, u0, u1, v0, cutting_value, 0, uo, vo);
            new_iso.reset_approx();
            u_strip.insert_after(v_strip_index, new_iso)?;
            let iso = u_strip.value_mut(v_strip_index + 2)?;
            iso.change_domain_4(iso.u0(), iso.u1(), cutting_value, iso.v1());
        }
        let mut node_start_index: i32 = 1;
        while node_start_index <= self.node_constraints.len()
            && self.node_constraints.value(node_start_index)?.coord().y() < cutting_value
        {
            node_start_index += self.u_constraints.len() + 1;
        }
        for iso_index in 1..=self.u_constraints.len() + 1 {
            let j_node = self.node_constraints.value(iso_index)?;
            let new_uv = XY::new(j_node.coord().x(), cutting_value);
            let new_node = Node::new_3(&new_uv, j_node.u_order(), j_node.v_order());
            self.node_constraints.insert_after(node_start_index + iso_index - 2, new_node)?;
        }
        Ok(())
    }
}
