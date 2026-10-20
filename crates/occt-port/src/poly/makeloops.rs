//! `Poly_MakeLoops`, `Poly_MakeLoops3D` and `Poly_MakeLoops2D`: loops made of connected oriented links.
use super::{HeapOfInteger, PackedMapOfInteger};
use crate::gp::{Dir, Dir2d};
use crate::mathlin::MathError;
use std::collections::HashMap;
use std::f64::consts::PI;

const LF_FWD: i32 = 1;
const LF_REV: i32 = 2;
const LF_BOTH: i32 = 3;
const LF_REVERSED: i32 = 4;

/// `Poly_MakeLoops::Link`: a pair of node indices and orientation flags (equality ignores the order of the nodes).
#[derive(Debug, Clone, Copy, Default)]
pub struct Link {
    /// The first node.
    pub node1: i32,
    /// The second node.
    pub node2: i32,
    /// `LF_None` 0, `LF_Fwd` 1, `LF_Rev` 2, `LF_Both` 3, plus `LF_Reversed` 4.
    pub flags: i32,
}

impl Link {
    /// `Link(theNode1, theNode2)`: forward.
    pub fn new(node1: i32, node2: i32) -> Self {
        Self { node1, node2, flags: 1 }
    }
    /// `Reverse()`.
    pub fn reverse(&mut self) {
        self.flags ^= LF_REVERSED;
    }
    /// `IsReversed()`.
    pub fn is_reversed(&self) -> bool {
        self.flags & LF_REVERSED != 0
    }
    /// `Nullify()`.
    pub fn nullify(&mut self) {
        self.node1 = 0;
        self.node2 = 0;
    }
    /// `IsNull()`.
    pub fn is_null(&self) -> bool {
        self.node1 == 0 || self.node2 == 0
    }
    fn key(&self) -> (i32, i32) {
        if self.node1 <= self.node2 { (self.node1, self.node2) } else { (self.node2, self.node1) }
    }
}

impl PartialEq for Link {
    fn eq(&self, other: &Link) -> bool {
        self.key() == other.key()
    }
}

/// `Poly_MakeLoops::Helper`.
pub trait MakeLoopsHelper {
    /// `GetAdjacentLinks(theNode)`.
    fn adjacent_links(&self, node: i32) -> &[Link];
}

/// `Poly_MakeLoops3D::Helper`.
pub trait MakeLoops3DHelper: MakeLoopsHelper {
    /// `GetFirstTangent`.
    fn first_tangent(&self, link: &Link) -> Option<Dir>;
    /// `GetLastTangent`.
    fn last_tangent(&self, link: &Link) -> Option<Dir>;
    /// `GetNormal`.
    fn normal(&self, node: i32) -> Option<Dir>;
}

/// `Poly_MakeLoops2D::Helper`.
pub trait MakeLoops2DHelper: MakeLoopsHelper {
    /// `GetFirstTangent`.
    fn first_tangent(&self, link: &Link) -> Option<Dir2d>;
    /// `GetLastTangent`.
    fn last_tangent(&self, link: &Link) -> Option<Dir2d>;
}

/// What differs between the 3D and the 2D loop makers.
trait Policy {
    fn adjacent(&self, node: i32) -> &[Link];
    fn choose_left_way(&self, core: &Core, node: i32, seg: i32, lst: &[i32]) -> i32;
}

/// An `NCollection_IndexedMap<int>`.
#[derive(Debug, Default)]
struct IndexedInts {
    items: Vec<i32>,
    index: HashMap<i32, usize>,
}

impl IndexedInts {
    fn add(&mut self, v: i32) {
        if !self.index.contains_key(&v) {
            self.items.push(v);
            self.index.insert(v, self.items.len());
        }
    }
    fn contains(&self, v: i32) -> bool {
        self.index.contains_key(&v)
    }
    fn find_index(&self, v: i32) -> usize {
        self.index.get(&v).copied().unwrap_or(0)
    }
    fn extent(&self) -> usize {
        self.items.len()
    }
    /// `aContour(i)`, 1-based.
    fn at(&self, i: usize) -> i32 {
        i.checked_sub(1).and_then(|k| self.items.get(k)).copied().unwrap_or(0)
    }
}

/// The state shared by both loop makers.
#[derive(Debug)]
struct Core {
    /// `NCollection_IndexedMap<Link, Hasher>`: the links in insertion order and the index of each key.
    links: Vec<Link>,
    link_index: HashMap<(i32, i32), usize>,
    loops: Vec<Vec<Link>>,
    start_indices: HeapOfInteger,
    hang_indices: PackedMapOfInteger,
}

impl Core {
    fn new() -> Self {
        Self {
            links: Vec::new(),
            link_index: HashMap::new(),
            loops: Vec::new(),
            start_indices: HeapOfInteger::new(4000),
            hang_indices: PackedMapOfInteger::new(1),
        }
    }
    /// `myMapLink.FindIndex` (1-based, 0 when absent).
    fn find_index(&self, l: &Link) -> usize {
        self.link_index.get(&l.key()).map_or(0, |i| i + 1)
    }
    /// `myMapLink.Add`.
    fn map_add(&mut self, l: &Link) -> usize {
        let i = self.find_index(l);
        if i > 0 {
            return i;
        }
        self.links.push(*l);
        self.link_index.insert(l.key(), self.links.len() - 1);
        self.links.len()
    }
    fn link_at(&self, index: usize) -> Link {
        index.checked_sub(1).and_then(|k| self.links.get(k)).copied().unwrap_or_default()
    }
    fn add_link(&mut self, l: &Link) {
        if l.node1 == l.node2 {
            return;
        }
        let i = self.map_add(l);
        if let Some(stored) = i.checked_sub(1).and_then(|k| self.links.get_mut(k)) {
            stored.flags |= l.flags;
        }
    }
    fn replace_link(&mut self, l: &Link, new: &Link) {
        if new.node1 == new.node2 {
            return;
        }
        let i = self.map_add(l);
        if let Some(k) = i.checked_sub(1) {
            if let Some(stored) = self.links.get(k).copied() {
                self.link_index.remove(&stored.key());
            }
            if let Some(slot) = self.links.get_mut(k) {
                *slot = *new;
            }
            self.link_index.insert(new.key(), k);
        }
    }
    fn set_link_orientation(&mut self, l: &Link, orient: i32) -> i32 {
        let i = self.find_index(l);
        match i.checked_sub(1).and_then(|k| self.links.get_mut(k)) {
            Some(stored) => {
                let old = stored.flags & LF_BOTH;
                stored.flags = orient;
                old
            }
            None => 0,
        }
    }
    fn find_link(&self, l: &Link) -> Link {
        self.link_at(self.find_index(l))
    }
    fn get_link(&self, seg: i32) -> Link {
        let mut l = self.link_at(seg.unsigned_abs() as usize);
        if seg < 0 {
            l.reverse();
        }
        l
    }
    fn first_node(&self, index_s: i32) -> i32 {
        let l = self.link_at(index_s.unsigned_abs() as usize);
        if index_s > 0 { l.node1 } else { l.node2 }
    }
    fn last_node(&self, index_s: i32) -> i32 {
        let l = self.link_at(index_s.unsigned_abs() as usize);
        if index_s > 0 { l.node2 } else { l.node1 }
    }
    fn can_link_be_taken(&self, index_s: i32) -> bool {
        self.start_indices.contains(index_s)
    }

    fn find_contour(&self, policy: &dyn Policy, index_s0: i32, contour: &mut IndexedInts) -> usize {
        *contour = IndexedInts::default();
        let mut index_s = index_s0;
        let mut node_link: HashMap<i32, i32> = HashMap::new();
        let mut last_node = self.last_node(index_s);
        loop {
            contour.add(index_s);
            node_link.insert(self.first_node(index_s), index_s);
            let index = index_s.unsigned_abs() as usize;
            let mut lst: Vec<i32> = Vec::new();
            for l in policy.adjacent(last_node) {
                let ind = self.find_index(l);
                if ind == 0 || ind == index {
                    continue;
                }
                let mut ind_s = ind as i32;
                let node1 = self.first_node(ind as i32);
                if node1 != last_node {
                    ind_s = -ind_s;
                }
                if self.can_link_be_taken(ind_s) {
                    lst.push(ind_s);
                }
            }
            let (Some(&first), Some(&last)) = (lst.first(), lst.last()) else {
                return contour.extent() + 1;
            };
            let next = if first == last { first } else { policy.choose_left_way(self, last_node, index_s, &lst) };
            index_s = next;
            if index_s == 0 {
                return contour.extent() + 1;
            }
            if contour.contains(index_s) {
                return contour.find_index(index_s);
            }
            if contour.contains(-index_s) {
                return contour.find_index(-index_s) + 1;
            }
            last_node = self.last_node(index_s);
            if let Some(&nl) = node_link.get(&last_node) {
                contour.add(index_s);
                return contour.find_index(nl);
            }
        }
    }

    fn accept_contour(&mut self, contour: &IndexedInts, start_number: usize) {
        let mut lp = Vec::new();
        for i in start_number..=contour.extent() {
            let index_s = contour.at(i);
            let mut oriented = self.link_at(index_s.unsigned_abs() as usize);
            if index_s < 0 {
                oriented.reverse();
            }
            lp.push(oriented);
            self.start_indices.remove(index_s);
        }
        self.loops.push(lp);
    }

    fn mark_hang_chain(&mut self, policy: &dyn Policy, node: i32, index_s0: i32) {
        let mut node1 = node;
        let mut index_s = index_s0;
        let mut index = index_s.unsigned_abs() as usize;
        let is_out = node1 == self.first_node(index_s);
        loop {
            let mut n_edges = 0;
            for l in policy.adjacent(node1) {
                if n_edges != 0 {
                    break;
                }
                let ind = self.find_index(l);
                if ind == 0 || ind == index {
                    continue;
                }
                let mut ind = ind as i32;
                if (is_out && node1 == l.node1) || (!is_out && node1 == l.node2) {
                    ind = -ind;
                }
                if self.can_link_be_taken(ind) {
                    n_edges += 1;
                }
            }
            if n_edges > 0 {
                break;
            }
            self.start_indices.remove(index_s);
            self.hang_indices.add(index_s);
            node1 = if is_out { self.last_node(index_s) } else { self.first_node(index_s) };
            let mut next_index_s = 0;
            for l in policy.adjacent(node1) {
                let ind = self.find_index(l);
                if ind == 0 || ind == index {
                    continue;
                }
                let mut ind = ind as i32;
                if (is_out && node1 == l.node2) || (!is_out && node1 == l.node1) {
                    ind = -ind;
                }
                if self.can_link_be_taken(ind) {
                    if next_index_s == 0 {
                        next_index_s = ind;
                    } else {
                        next_index_s = 0;
                        break;
                    }
                }
            }
            if next_index_s == 0 {
                break;
            }
            index_s = next_index_s;
            index = index_s.unsigned_abs() as usize;
        }
    }

    fn perform(&mut self, policy: &dyn Policy) -> i32 {
        self.start_indices.clear();
        for i in 1..=self.links.len() {
            let flags = self.link_at(i).flags;
            if flags & LF_FWD != 0 {
                self.start_indices.add(i as i32);
            }
            if flags & LF_REV != 0 {
                self.start_indices.add(-(i as i32));
            }
        }
        let mut result = 0;
        for pass in 0..2 {
            self.hang_indices.clear();
            while !self.start_indices.is_empty() {
                let Ok(mut index_s) = self.start_indices.top() else { break };
                let mut contour = IndexedInts::default();
                let start_number = self.find_contour(policy, index_s, &mut contour);
                if start_number == 0 {
                    result |= 4;
                    return result;
                }
                if start_number <= contour.extent() {
                    self.accept_contour(&contour, start_number);
                }
                if start_number > 1 {
                    let node;
                    if start_number <= contour.extent() {
                        node = self.first_node(index_s);
                    } else {
                        index_s = contour.at(start_number - 1);
                        node = self.last_node(index_s);
                    }
                    self.mark_hang_chain(policy, node, index_s);
                }
            }
            if pass == 0 {
                for key in self.hang_indices.keys() {
                    self.start_indices.add(key);
                }
            }
        }
        if !self.loops.is_empty() {
            result |= 1;
        }
        if !self.hang_indices.is_empty() {
            result |= 2;
        }
        result
    }

    fn hanging_links(&self) -> Vec<Link> {
        self.hang_indices
            .keys()
            .into_iter()
            .map(|index_s| {
                let mut l = self.link_at(index_s.unsigned_abs() as usize);
                if index_s < 0 {
                    l.reverse();
                }
                l
            })
            .collect()
    }

    fn loop_links(&self, i: i32) -> Result<&[Link], MathError> {
        usize::try_from(i.wrapping_sub(1))
            .ok()
            .and_then(|k| self.loops.get(k))
            .map(Vec::as_slice)
            .ok_or(MathError::Range)
    }
}

struct Policy3D<'a>(&'a dyn MakeLoops3DHelper);

impl Policy for Policy3D<'_> {
    fn adjacent(&self, node: i32) -> &[Link] {
        self.0.adjacent_links(node)
    }
    fn choose_left_way(&self, core: &Core, node: i32, seg: i32, lst: &[i32]) -> i32 {
        let first = lst.first().copied().unwrap_or(0);
        let mut angle_min = PI * 2.0;
        let helper = self.0;
        let Some(normal) = helper.normal(node) else { return first };
        let mut link = core.get_link(seg);
        let Some(tgt_ref) = helper.last_tangent(&link) else { return first };
        let tgt_ref_xyz = normal.xyz().cross_crossed(tgt_ref.xyz(), normal.xyz());
        if tgt_ref_xyz.square_modulus() < 1e-14 {
            return first;
        }
        let Ok(tgt_ref) = Dir::from_xyz(tgt_ref_xyz) else { return first };
        let mut res_index = 0;
        for &ind_s in lst {
            link = core.get_link(ind_s);
            let Some(tgt) = helper.first_tangent(&link) else { continue };
            let tgt_xyz = normal.xyz().cross_crossed(tgt.xyz(), normal.xyz());
            if tgt_xyz.square_modulus() < 1e-14 {
                continue;
            }
            let Ok(tgt) = Dir::from_xyz(tgt_xyz) else { continue };
            let mut angle = tgt.angle_with_ref(&tgt_ref, &normal);
            if angle < 1e-4 - PI {
                angle = PI;
            }
            if angle < angle_min {
                angle_min = angle;
                res_index = ind_s;
            }
        }
        if res_index == 0 { first } else { res_index }
    }
}

struct Policy2D<'a> {
    helper: &'a dyn MakeLoops2DHelper,
    right_way: bool,
}

impl Policy for Policy2D<'_> {
    fn adjacent(&self, node: i32) -> &[Link] {
        self.helper.adjacent_links(node)
    }
    fn choose_left_way(&self, core: &Core, _node: i32, seg: i32, lst: &[i32]) -> i32 {
        let first = lst.first().copied().unwrap_or(0);
        let mut angle_min = PI * 2.0;
        let mut link = core.get_link(seg);
        let Some(tgt_ref) = self.helper.last_tangent(&link) else { return first };
        let mut res_index = 0;
        for &ind_s in lst {
            link = core.get_link(ind_s);
            let Some(tgt) = self.helper.first_tangent(&link) else { continue };
            let mut angle = tgt.angle(&tgt_ref);
            if self.right_way {
                angle = -angle;
            }
            if angle < 1e-4 - PI {
                angle = PI;
            }
            if angle < angle_min {
                angle_min = angle;
                res_index = ind_s;
            }
        }
        if res_index == 0 { first } else { res_index }
    }
}

macro_rules! loops_api {
    ($t:ident) => {
        impl $t<'_> {
            /// `AddLink`.
            pub fn add_link(&mut self, link: &Link) {
                self.core.add_link(link);
            }
            /// `ReplaceLink`.
            pub fn replace_link(&mut self, link: &Link, new_link: &Link) {
                self.core.replace_link(link, new_link);
            }
            /// `SetLinkOrientation`: the previous orientation (`flags & 3`), 0 for an unknown link.
            pub fn set_link_orientation(&mut self, link: &Link, orient: i32) -> i32 {
                self.core.set_link_orientation(link, orient)
            }
            /// `FindLink`: a null link when absent.
            pub fn find_link(&self, link: &Link) -> Link {
                self.core.find_link(link)
            }
            /// `Perform`: bit 1 loops done, 2 hanging links, 4 failure.
            pub fn perform(&mut self) -> i32 {
                self.core.perform(&self.policy)
            }
            /// `GetNbLoops`.
            pub fn nb_loops(&self) -> i32 {
                self.core.loops.len() as i32
            }
            /// `GetLoop(i)` (1-based).
            pub fn loop_links(&self, i: i32) -> Result<&[Link], MathError> {
                self.core.loop_links(i)
            }
            /// `GetNbHanging`.
            pub fn nb_hanging(&self) -> i32 {
                self.core.hang_indices.extent()
            }
            /// `GetHangingLinks`.
            pub fn hanging_links(&self) -> Vec<Link> {
                self.core.hanging_links()
            }
        }
    };
}

/// `Poly_MakeLoops3D`.
pub struct MakeLoops3D<'a> {
    core: Core,
    policy: Policy3D<'a>,
}

impl std::fmt::Debug for MakeLoops3D<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MakeLoops3D").field("core", &self.core).finish_non_exhaustive()
    }
}

impl<'a> MakeLoops3D<'a> {
    /// `Poly_MakeLoops3D(theHelper)`.
    pub fn new(helper: &'a dyn MakeLoops3DHelper) -> Self {
        Self { core: Core::new(), policy: Policy3D(helper) }
    }
}

loops_api!(MakeLoops3D);

/// `Poly_MakeLoops2D`.
pub struct MakeLoops2D<'a> {
    core: Core,
    policy: Policy2D<'a>,
}

impl std::fmt::Debug for MakeLoops2D<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MakeLoops2D").field("core", &self.core).finish_non_exhaustive()
    }
}

impl<'a> MakeLoops2D<'a> {
    /// `Poly_MakeLoops2D(theLeftWay, theHelper)`.
    pub fn new(left_way: bool, helper: &'a dyn MakeLoops2DHelper) -> Self {
        Self { core: Core::new(), policy: Policy2D { helper, right_way: !left_way } }
    }
}

loops_api!(MakeLoops2D);
