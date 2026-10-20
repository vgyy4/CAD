//! The part of OCCT's `BVH` package that `Bnd_OBB::ReBuild` uses for its optimal mode: a
//! `BVH_BoxSet<double, 3, gp_XYZ>` built by `BVH_LinearBuilder<double, 3>(30)` (Morton-code
//! radix sort, binary tree over the sorted elements) and the `OBB_ExtremePointsSelector`, a
//! `BVH_Traverse` with a `Bnd_Range` metric.
use super::{
    box3d::{cmax, cmin},
    range::BndRange,
};
use crate::{gp::XYZ, mathlin::MathError};

/// `BVH_Constants_MaxTreeDepth`.
const MAX_TREE_DEPTH: usize = 32;
/// `BVH::THE_NODE_MIN_SIZE`.
const NODE_MIN_SIZE: f64 = 1e-5;
/// Number of voxels per axis of the radix sorter.
const DIMENSION: i32 = 1024;
/// Leaf size of the linear builder used by `Bnd_OBB`.
const LEAF_NODE_SIZE: i32 = 30;

type Vec3 = [f64; 3];

/// `BVH::BoxMinMax::CwiseMin`.
fn cwise_min(a: &mut Vec3, b: &Vec3) {
    for (x, y) in a.iter_mut().zip(b) {
        *x = cmin(*x, *y);
    }
}

/// `BVH::BoxMinMax::CwiseMax`.
fn cwise_max(a: &mut Vec3, b: &Vec3) {
    for (x, y) in a.iter_mut().zip(b) {
        *x = cmax(*x, *y);
    }
}
/// `BVH_EncodedLink`: Morton code and element index.
type Link = (u32, i32);

/// `BVH_Box<double, 3>`.
#[derive(Debug, Clone, Copy)]
struct BvhBox {
    min: Vec3,
    max: Vec3,
}

impl BvhBox {
    fn is_valid(&self) -> bool {
        self.min[0] <= self.max[0]
    }
    fn center(&self) -> Vec3 {
        [(self.min[0] + self.max[0]) * 0.5, (self.min[1] + self.max[1]) * 0.5, (self.min[2] + self.max[2]) * 0.5]
    }
    /// `BVH_Box::Combine`.
    fn combine(&mut self, other: &BvhBox) {
        if other.is_valid() {
            cwise_min(&mut self.min, &other.min);
            cwise_max(&mut self.max, &other.max);
        }
    }
}

/// `BVH::EncodeMortonCode` (the table spreads each bit `k` to bit `3k`).
fn morton_spread(v: u32) -> u32 {
    let mut out = 0u32;
    for k in 0..10 {
        if (v >> k) & 1 != 0 {
            out |= 1u32 << (3 * k);
        }
    }
    out
}

fn encode_morton_code(x: u32, y: u32, z: u32) -> u32 {
    morton_spread(x) | (morton_spread(y) << 1) | (morton_spread(z) << 2)
}

/// `BVH::IntFloor`.
fn int_floor(v: f64) -> i32 {
    let res = v as i32;
    res - i32::from(f64::from(res) > v)
}

/// `std::partition` of libstdc++ for bidirectional iterators, with the predicate "bit is not set".
fn partition(links: &mut [Link], bit: u32) -> usize {
    let pred = |l: &Link| l.0 & bit == 0;
    let mut first = 0usize;
    let mut last = links.len();
    loop {
        loop {
            if first == last {
                return first;
            }
            match links.get(first) {
                Some(l) if pred(l) => first += 1,
                _ => break,
            }
        }
        last -= 1;
        loop {
            if first == last {
                return first;
            }
            match links.get(last) {
                Some(l) if !pred(l) => last -= 1,
                _ => break,
            }
        }
        links.swap(first, last);
        first += 1;
    }
}

/// `BVH::RadixSorter::Sort` / `perform`: MSB-first partitioning of the links.
fn radix_sort(links: &mut [Link], mut digit: i32) {
    let mut start = 0usize;
    let end = links.len();
    while start != end && digit >= 0 {
        let bit = 1u32 << digit;
        digit -= 1;
        let offset = match links.get_mut(start..end) {
            Some(range) => start + partition(range, bit),
            None => return,
        };
        if let Some(left) = links.get_mut(start..offset) {
            radix_sort(left, digit);
        }
        start = offset;
    }
}

/// `BVH_Tree<double, 3, BVH_BinaryTree>`: node info `(is_leaf, a, b, 0)` and node boxes.
#[derive(Debug, Clone, Default)]
struct BvhTree {
    info: Vec<[i32; 4]>,
    min: Vec<Vec3>,
    max: Vec<Vec3>,
}

impl BvhTree {
    fn add_inner_node(&mut self) -> usize {
        self.info.push([0, 0, 0, 0]);
        self.info.len() - 1
    }
    fn add_leaf_node(&mut self, beg: i32, end: i32) -> usize {
        self.info.push([1, beg, end, 0]);
        self.info.len() - 1
    }
}

/// `BVH_BoxSet<double, 3, gp_XYZ>` with its tree, built by `BVH_LinearBuilder<double, 3>(30)`.
#[derive(Debug, Clone)]
pub(crate) struct PointBvh {
    elements: Vec<XYZ>,
    boxes: Vec<BvhBox>,
    tree: BvhTree,
}

fn range_err() -> MathError {
    MathError::Range
}

impl PointBvh {
    /// `Add` of every point with the box `[P - tol, P + tol]`, then `Build`.
    pub(crate) fn build(points: &[XYZ], tolerances: &[f64]) -> Result<Self, MathError> {
        let mut elements = Vec::with_capacity(points.len());
        let mut boxes = Vec::with_capacity(points.len());
        for (p, tol) in points.iter().zip(tolerances) {
            elements.push(*p);
            boxes.push(BvhBox {
                min: [p.x() - tol, p.y() - tol, p.z() - tol],
                max: [p.x() + tol, p.y() + tol, p.z() + tol],
            });
        }
        let mut set = Self { elements, boxes, tree: BvhTree::default() };
        set.update()?;
        Ok(set)
    }

    /// `BVH_PrimitiveSet::Update`: `BVH_LinearBuilder::Build` over the set box.
    fn update(&mut self) -> Result<(), MathError> {
        let size = self.boxes.len();
        if size == 0 {
            return Ok(());
        }
        let mut scene = BvhBox { min: [f64::MAX; 3], max: [-f64::MAX; 3] };
        for b in &self.boxes {
            scene.combine(b);
        }
        self.tree = BvhTree::default();
        let links = self.radix_sort_set(&scene)?;
        let size_i32 = i32::try_from(size).map_err(|_| range_err())?;
        self.emit_hierarchy(&links, 29, 0, 0, size_i32)?;
        self.tree.min = vec![[0.0; 3]; self.tree.info.len()];
        self.tree.max = vec![[0.0; 3]; self.tree.info.len()];
        self.update_bounds(0)?;
        Ok(())
    }

    /// `BVH_RadixSorter::Perform`: encodes, sorts and reorders the set; returns the sorted links.
    fn radix_sort_set(&mut self, scene: &BvhBox) -> Result<Vec<Link>, MathError> {
        let mut size_vec = [NODE_MIN_SIZE; 3];
        for (s, (hi, lo)) in size_vec.iter_mut().zip(scene.max.iter().zip(&scene.min)) {
            *s = cmax(*s, hi - lo);
        }
        let reverse = [
            f64::from(DIMENSION) / size_vec[0],
            f64::from(DIMENSION) / size_vec[1],
            f64::from(DIMENSION) / size_vec[2],
        ];
        let mut links: Vec<Link> = Vec::with_capacity(self.boxes.len());
        for (idx, b) in self.boxes.iter().enumerate() {
            let center = b.center();
            let voxel_f = [
                (center[0] - scene.min[0]) * reverse[0],
                (center[1] - scene.min[1]) * reverse[1],
                (center[2] - scene.min[2]) * reverse[2],
            ];
            let clamp = |v: f64| int_floor(v).clamp(0, DIMENSION - 1);
            let code = encode_morton_code(clamp(voxel_f[0]) as u32, clamp(voxel_f[1]) as u32, clamp(voxel_f[2]) as u32);
            links.push((code, i32::try_from(idx).map_err(|_| range_err())?));
        }
        radix_sort(&mut links, 29);
        // The swap loop of the C++ puts the element `links[k].1` at position `k`.
        let mut elements = Vec::with_capacity(links.len());
        let mut boxes = Vec::with_capacity(links.len());
        for link in &links {
            let src = usize::try_from(link.1).map_err(|_| range_err())?;
            elements.push(*self.elements.get(src).ok_or_else(range_err)?);
            boxes.push(*self.boxes.get(src).ok_or_else(range_err)?);
        }
        self.elements = elements;
        self.boxes = boxes;
        Ok(links)
    }

    /// `BVH_LinearBuilder::lowerBound`.
    fn lower_bound(links: &[Link], mut start: i32, final_: i32, digit: i32) -> Result<i32, MathError> {
        let mut nb_prims = final_ - start;
        let bit = 1u32 << digit;
        while nb_prims > 0 {
            let step = nb_prims / 2;
            let idx = usize::try_from(start + step).map_err(|_| range_err())?;
            if links.get(idx).ok_or_else(range_err)?.0 & bit != 0 {
                nb_prims = step;
            } else {
                start += step + 1;
                nb_prims -= step + 1;
            }
        }
        Ok(start)
    }

    /// `BVH_LinearBuilder::emitHierachy`.
    fn emit_hierarchy(
        &mut self,
        links: &[Link],
        digit: i32,
        shift: i32,
        start: i32,
        final_: i32,
    ) -> Result<usize, MathError> {
        if final_ - start > LEAF_NODE_SIZE {
            let position =
                if digit < 0 { (start + final_) / 2 } else { Self::lower_bound(links, start, final_, digit)? };
            if position == start || position == final_ {
                return self.emit_hierarchy(links, digit - 1, shift, start, final_);
            }
            let node = self.tree.add_inner_node();
            let rgh_node = shift + position - start;
            let lft_child = self.emit_hierarchy(links, digit - 1, shift, start, position)?;
            let rgh_child = self.emit_hierarchy(links, digit - 1, rgh_node, position, final_)?;
            let info = self.tree.info.get_mut(node).ok_or_else(range_err)?;
            info[1] = i32::try_from(lft_child).map_err(|_| range_err())?;
            info[2] = i32::try_from(rgh_child).map_err(|_| range_err())?;
            Ok(node)
        } else {
            Ok(self.tree.add_leaf_node(shift, shift + final_ - start - 1))
        }
    }

    /// `BVH::UpdateBounds`; returns the height of the subtree.
    fn update_bounds(&mut self, node: usize) -> Result<i32, MathError> {
        let data = *self.tree.info.get(node).ok_or_else(range_err)?;
        if data[0] == 0 {
            let lft = usize::try_from(data[1]).map_err(|_| range_err())?;
            let rgh = usize::try_from(data[2]).map_err(|_| range_err())?;
            let lft_depth = self.update_bounds(lft)?;
            let rgh_depth = self.update_bounds(rgh)?;
            let mut lft_min = *self.tree.min.get(lft).ok_or_else(range_err)?;
            let mut lft_max = *self.tree.max.get(lft).ok_or_else(range_err)?;
            let rgh_min = *self.tree.min.get(rgh).ok_or_else(range_err)?;
            let rgh_max = *self.tree.max.get(rgh).ok_or_else(range_err)?;
            cwise_min(&mut lft_min, &rgh_min);
            cwise_max(&mut lft_max, &rgh_max);
            *self.tree.min.get_mut(node).ok_or_else(range_err)? = lft_min;
            *self.tree.max.get_mut(node).ok_or_else(range_err)? = lft_max;
            Ok(if rgh_depth > lft_depth { rgh_depth } else { lft_depth } + 1)
        } else {
            let mut min_point = [0.0; 3];
            let mut max_point = [0.0; 3];
            for prim in data[1]..=data[2] {
                let b = self.boxes.get(usize::try_from(prim).map_err(|_| range_err())?).ok_or_else(range_err)?;
                if prim == data[1] {
                    min_point = b.min;
                    max_point = b.max;
                } else {
                    cwise_min(&mut min_point, &b.min);
                    cwise_max(&mut max_point, &b.max);
                }
            }
            *self.tree.min.get_mut(node).ok_or_else(range_err)? = min_point;
            *self.tree.max.get_mut(node).ok_or_else(range_err)? = max_point;
            Ok(0)
        }
    }

    /// One run of `OBB_ExtremePointsSelector` along `axis`: `(MinPrm, MaxPrm, MinPnt, MaxPnt)`.
    pub(crate) fn extreme_points(&self, axis: &XYZ) -> Result<(f64, f64, XYZ, XYZ), MathError> {
        let mut sel = ExtremePointsSelector::new(*axis);
        sel.select(self)?;
        Ok((sel.prm_min, sel.prm_max, sel.pnt_min, sel.pnt_max))
    }
}

/// `OBB_ExtremePointsSelector`, driven by `BVH_Traverse::Select`.
struct ExtremePointsSelector {
    axis: XYZ,
    prm_min: f64,
    prm_max: f64,
    pnt_min: XYZ,
    pnt_max: XYZ,
}

impl ExtremePointsSelector {
    fn new(axis: XYZ) -> Self {
        Self { axis, prm_min: f64::MAX, prm_max: -f64::MAX, pnt_min: XYZ::zero(), pnt_max: XYZ::zero() }
    }

    fn reject_node(&self, cmin_pt: &Vec3, cmax_pt: &Vec3, metric: &mut BndRange) -> Result<bool, MathError> {
        if self.prm_min > self.prm_max {
            return Ok(false);
        }
        let mut prm_min = self.prm_min;
        let mut prm_max = self.prm_max;
        let mut is_to_reject = true;
        for i in 0..2 {
            let x = if i == 0 { cmin_pt[0] } else { cmax_pt[0] };
            for j in 0..2 {
                let y = if j == 0 { cmin_pt[1] } else { cmax_pt[1] };
                for k in 0..2 {
                    let z = if k == 0 { cmin_pt[2] } else { cmax_pt[2] };
                    let prm = self.axis.dot(&XYZ::new(x, y, z));
                    if prm < prm_min {
                        prm_min = prm;
                        is_to_reject = false;
                    } else if prm > prm_max {
                        prm_max = prm;
                        is_to_reject = false;
                    }
                }
            }
        }
        *metric = BndRange::from_bounds(prm_min, prm_max)?;
        Ok(is_to_reject)
    }

    fn reject_metric(&self, metric: &BndRange) -> bool {
        if self.prm_min > self.prm_max {
            return false;
        }
        let Some((min, max)) = metric.get_bounds() else {
            return false;
        };
        min > self.prm_min && max < self.prm_max
    }

    fn accept(&mut self, leaf: &XYZ) {
        let prm = self.axis.dot(leaf);
        if prm < self.prm_min {
            self.prm_min = prm;
            self.pnt_min = *leaf;
        }
        if prm > self.prm_max {
            self.prm_max = prm;
            self.pnt_max = *leaf;
        }
    }

    fn is_metric_better(&self, left: &BndRange, right: &BndRange) -> bool {
        if self.prm_min > self.prm_max {
            return true;
        }
        let (Some((min0, max0)), Some((min1, max1))) = (left.get_bounds(), right.get_bounds()) else {
            return true;
        };
        let mut ext = [0.0f64, 0.0f64];
        for (e, (min, max)) in ext.iter_mut().zip([(min0, max0), (min1, max1)]) {
            if min < self.prm_min {
                *e += self.prm_min - min;
            }
            if max > self.prm_max {
                *e += max - self.prm_max;
            }
        }
        let [ext0, ext1] = ext;
        ext0 > ext1
    }

    /// `BVH_Traverse::Select` (`AcceptMetric` is always false, `Stop` is always false).
    fn select(&mut self, set: &PointBvh) -> Result<(), MathError> {
        let nodes = &set.tree.info;
        if nodes.is_empty() {
            return Ok(());
        }
        let mut stack: Vec<(usize, BndRange)> = Vec::with_capacity(MAX_TREE_DEPTH);
        let mut node: (usize, BndRange) = (0, BndRange::new());
        let mut prev_id = node.0;
        loop {
            let data = *nodes.get(node.0).ok_or_else(range_err)?;
            if data[0] == 0 {
                let lft = usize::try_from(data[1]).map_err(|_| range_err())?;
                let rgh = usize::try_from(data[2]).map_err(|_| range_err())?;
                let mut metric_lft = BndRange::new();
                let is_good_lft = !self.reject_node(
                    set.tree.min.get(lft).ok_or_else(range_err)?,
                    set.tree.max.get(lft).ok_or_else(range_err)?,
                    &mut metric_lft,
                )?;
                let mut metric_rgh = BndRange::new();
                let is_good_rgh = !self.reject_node(
                    set.tree.min.get(rgh).ok_or_else(range_err)?,
                    set.tree.max.get(rgh).ok_or_else(range_err)?,
                    &mut metric_rgh,
                )?;
                if is_good_lft && is_good_rgh {
                    if stack.len() >= MAX_TREE_DEPTH - 1 {
                        return Err(MathError::Range);
                    }
                    if self.is_metric_better(&metric_lft, &metric_rgh) {
                        node = (lft, metric_lft);
                        stack.push((rgh, metric_rgh));
                    } else {
                        node = (rgh, metric_rgh);
                        stack.push((lft, metric_lft));
                    }
                } else if is_good_lft || is_good_rgh {
                    node = if is_good_lft { (lft, metric_lft) } else { (rgh, metric_rgh) };
                }
            } else {
                for i in data[1]..=data[2] {
                    let leaf = set.elements.get(usize::try_from(i).map_err(|_| range_err())?).ok_or_else(range_err)?;
                    self.accept(leaf);
                }
            }
            if node.0 == prev_id {
                let Some(top) = stack.pop() else {
                    return Ok(());
                };
                node = top;
                while self.reject_metric(&node.1) {
                    let Some(top) = stack.pop() else {
                        return Ok(());
                    };
                    node = top;
                }
            }
            prev_id = node.0;
        }
    }
}
