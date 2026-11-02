//! Bounding volume hierarchy primitives translated from OCCT 8.0.1.

use core::ops::{Add, Div, Mul, Sub};

/// Scalar operations used by the BVH numeric types.
pub trait Scalar:
    Copy + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self>
{
    fn zero() -> Self;
    fn one() -> Self;
    fn max_value() -> Self;
    fn lowest() -> Self;
    fn infinity() -> Self;
    fn epsilon() -> Self;
    fn from_i32(value: i32) -> Self;
    fn abs(self) -> Self;
}

impl Scalar for f32 {
    fn zero() -> Self {
        0.0
    }
    fn one() -> Self {
        1.0
    }
    fn max_value() -> Self {
        Self::MAX
    }
    fn lowest() -> Self {
        Self::MIN
    }
    fn infinity() -> Self {
        Self::INFINITY
    }
    fn epsilon() -> Self {
        Self::EPSILON
    }
    fn from_i32(value: i32) -> Self {
        value as Self
    }
    fn abs(self) -> Self {
        self.abs()
    }
}
impl Scalar for f64 {
    fn zero() -> Self {
        0.0
    }
    fn one() -> Self {
        1.0
    }
    fn max_value() -> Self {
        Self::MAX
    }
    fn lowest() -> Self {
        Self::MIN
    }
    fn infinity() -> Self {
        Self::INFINITY
    }
    fn epsilon() -> Self {
        Self::EPSILON
    }
    fn from_i32(value: i32) -> Self {
        value as Self
    }
    fn abs(self) -> Self {
        self.abs()
    }
}

/// Axis aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box<T, const N: usize> {
    min: [T; N],
    max: [T; N],
}

impl<T: Scalar, const N: usize> Default for Box<T, N> {
    fn default() -> Self {
        Self { min: [T::max_value(); N], max: [T::lowest(); N] }
    }
}

impl<T: Scalar, const N: usize> Box<T, N> {
    pub fn new(min: [T; N], max: [T; N]) -> Self {
        Self { min, max }
    }
    pub fn from_point(point: [T; N]) -> Self {
        Self { min: point, max: point }
    }
    pub fn clear(&mut self) {
        self.min = [T::max_value(); N];
        self.max = [T::lowest(); N];
    }
    pub fn is_valid(&self) -> bool {
        self.min.first().zip(self.max.first()).is_some_and(|(a, b)| *a <= *b)
    }
    pub fn add(&mut self, point: [T; N]) {
        for i in 0..N {
            if let (Some(a), Some(b), Some(p)) = (self.min.get_mut(i), self.max.get_mut(i), point.get(i)) {
                if *p < *a {
                    *a = *p;
                }
                if *p > *b {
                    *b = *p;
                }
            }
        }
    }
    pub fn combine(&mut self, other: &Self) {
        if other.is_valid() {
            for i in 0..N {
                if let (Some(a), Some(b), Some(omin), Some(omax)) =
                    (self.min.get_mut(i), self.max.get_mut(i), other.min.get(i), other.max.get(i))
                {
                    if *omin < *a {
                        *a = *omin;
                    }
                    if *omax > *b {
                        *b = *omax;
                    }
                }
            }
        }
    }
    pub fn corner_min(&self) -> [T; N] {
        self.min
    }
    pub fn corner_max(&self) -> [T; N] {
        self.max
    }
    pub fn size(&self) -> [T; N] {
        core::array::from_fn(|i| self.max.get(i).zip(self.min.get(i)).map_or(T::zero(), |(a, b)| *a - *b))
    }
    pub fn center(&self) -> [T; N] {
        core::array::from_fn(|i| {
            self.min
                .get(i)
                .zip(self.max.get(i))
                .map_or(T::zero(), |(a, b)| (*a + *b) * (T::one() / (T::one() + T::one())))
        })
    }
    pub fn center_axis(&self, axis: usize) -> T {
        self.min
            .get(axis)
            .zip(self.max.get(axis))
            .map_or(T::zero(), |(a, b)| (*a + *b) * (T::one() / (T::one() + T::one())))
    }
    pub fn area(&self) -> T {
        if !self.is_valid() {
            return T::zero();
        }
        let s = self.size();
        if N == 1 {
            return s.first().copied().map_or(T::zero(), Scalar::abs);
        }
        if N == 2 {
            let a = s.first().copied().unwrap_or(T::zero()).abs() * s.get(1).copied().unwrap_or(T::zero()).abs();
            return if a < T::epsilon() {
                s.first().copied().unwrap_or(T::zero()).abs() + s.get(1).copied().unwrap_or(T::zero()).abs()
            } else {
                a
            };
        }
        let x = s.first().copied().unwrap_or(T::zero());
        let y = s.get(1).copied().unwrap_or(T::zero());
        let z = s.get(2).copied().unwrap_or(T::zero());
        let a = ((x * y).abs() + (x * z).abs() + (z * y).abs()) * (T::one() + T::one());
        if a < T::epsilon() { x.abs() + y.abs() + z.abs() } else { a }
    }
    pub fn is_out_box(&self, other: &Self) -> bool {
        if !other.is_valid() {
            return true;
        }
        self.is_out_bounds(other.min, other.max)
    }
    pub fn is_out_bounds(&self, min: [T; N], max: [T; N]) -> bool {
        if !self.is_valid() {
            return true;
        }
        (0..N).any(|i| {
            self.min
                .get(i)
                .zip(max.get(i))
                .zip(self.max.get(i).zip(min.get(i)))
                .is_some_and(|((a, b), (c, d))| *a > *b || *c < *d)
        })
    }
    pub fn is_out_point(&self, p: [T; N]) -> bool {
        !self.is_valid()
            || (0..N).any(|i| {
                self.min.get(i).zip(self.max.get(i)).zip(p.get(i)).is_some_and(|((a, b), p)| *p < *a || *p > *b)
            })
    }
    pub fn contains_box(&self, other: &Self) -> (bool, bool) {
        if !other.is_valid() {
            return (false, false);
        }
        self.contains_bounds(other.min, other.max)
    }
    pub fn contains_bounds(&self, min: [T; N], max: [T; N]) -> (bool, bool) {
        if !self.is_valid() {
            return (false, false);
        }
        let mut inside = true;
        for i in 0..N {
            let Some((a, b)) = self.min.get(i).zip(self.max.get(i)) else { return (false, false) };
            let Some((c, d)) = min.get(i).zip(max.get(i)) else { return (false, false) };
            let overlap = *a <= *d && *b >= *c;
            if !overlap {
                return (false, false);
            }
            inside &= *a <= *c && *b >= *d;
        }
        (inside, true)
    }
}

impl<T: Scalar> Box<T, 3> {
    pub fn transformed(&self, matrix: &[[T; 4]; 4]) -> Result<Self, &'static str> {
        if !self.is_valid() {
            return Ok(*self);
        }
        let mut result = Self::default();
        for mask in 0..8 {
            let point = [
                if mask & 1 == 0 { self.min[0] } else { self.max[0] },
                if mask & 2 == 0 { self.min[1] } else { self.max[1] },
                if mask & 4 == 0 { self.min[2] } else { self.max[2] },
            ];
            let out = MatrixOp::<T, 4>::multiply(matrix, [point[0], point[1], point[2], T::one()])?;
            result.add([
                out.first().copied().ok_or("matrix shape mismatch")?,
                out.get(1).copied().ok_or("matrix shape mismatch")?,
                out.get(2).copied().ok_or("matrix shape mismatch")?,
            ]);
        }
        Ok(result)
    }
    pub fn transform(&mut self, matrix: &[[T; 4]; 4]) -> Result<(), &'static str> {
        *self = self.transformed(matrix)?;
        Ok(())
    }
}

/// A ray and its component-wise reciprocal direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray<T, const N: usize> {
    origin: [T; N],
    direction: [T; N],
    inverse: [T; N],
}
impl<T: Scalar, const N: usize> Ray<T, N> {
    pub fn new(origin: [T; N], direction: [T; N]) -> Self {
        let inverse = core::array::from_fn(|i| {
            direction
                .get(i)
                .copied()
                .map_or(T::infinity(), |x| if x == T::zero() { T::infinity() } else { T::one() / x })
        });
        Self { origin, direction, inverse }
    }
    pub fn origin(&self) -> [T; N] {
        self.origin
    }
    pub fn direction(&self) -> [T; N] {
        self.direction
    }
    pub fn inverse_direction(&self) -> [T; N] {
        self.inverse
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ProjectionState {
    Vertex = 0,
    Edge = 1,
    Inner = 2,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection<T, const N: usize> {
    pub point: [T; N],
    pub state: ProjectionState,
    pub first_node: i32,
    pub last_node: i32,
}

/// Box and triangle distance helpers.
#[derive(Debug)]
pub struct Tools<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> Tools<T, N> {
    pub fn box_box_square_distance(a: &Box<T, N>, b: &Box<T, N>) -> T {
        if !a.is_valid() || !b.is_valid() {
            return T::zero();
        }
        Self::box_box_square_distance_bounds(a.min, a.max, b.min, b.max)
    }
    pub fn box_box_square_distance_bounds(min1: [T; N], max1: [T; N], min2: [T; N], max2: [T; N]) -> T {
        let mut d = T::zero();
        for i in 0..N {
            if let (Some(a), Some(b), Some(c), Some(e)) = (min1.get(i), max1.get(i), min2.get(i), max2.get(i)) {
                let gap = if *a > *e {
                    Some(*a - *e)
                } else if *b < *c {
                    Some(*c - *b)
                } else {
                    None
                };
                if let Some(g) = gap {
                    d = d + g * g;
                }
            }
        }
        d
    }
    pub fn point_box_square_distance(p: [T; N], b: &Box<T, N>) -> T {
        if !b.is_valid() { T::zero() } else { Self::point_box_square_distance_bounds(p, b.min, b.max) }
    }
    pub fn point_box_square_distance_bounds(p: [T; N], min: [T; N], max: [T; N]) -> T {
        let mut d = T::zero();
        for i in 0..N {
            if let (Some(p), Some(lo), Some(hi)) = (p.get(i), min.get(i), max.get(i)) {
                let gap = if *p < *lo {
                    Some(*lo - *p)
                } else if *p > *hi {
                    Some(*p - *hi)
                } else {
                    None
                };
                if let Some(g) = gap {
                    d = d + g * g;
                }
            }
        }
        d
    }
    pub fn point_box_projection(p: [T; N], b: &Box<T, N>) -> [T; N] {
        if !b.is_valid() { p } else { Self::point_box_projection_bounds(p, b.min, b.max) }
    }
    pub fn point_box_projection_bounds(p: [T; N], min: [T; N], max: [T; N]) -> [T; N] {
        core::array::from_fn(|i| {
            p.get(i).zip(min.get(i)).zip(max.get(i)).map_or(T::zero(), |((x, l), h)| {
                if *x < *l {
                    *l
                } else if *x > *h {
                    *h
                } else {
                    *x
                }
            })
        })
    }
    fn dot(a: [T; N], b: [T; N]) -> T {
        let mut d = T::zero();
        for i in 0..N {
            if let (Some(x), Some(y)) = (a.get(i), b.get(i)) {
                d = d + *x * *y;
            }
        }
        d
    }
    fn sub(a: [T; N], b: [T; N]) -> [T; N] {
        core::array::from_fn(|i| a.get(i).zip(b.get(i)).map_or(T::zero(), |(x, y)| *x - *y))
    }
    fn add(a: [T; N], b: [T; N]) -> [T; N] {
        core::array::from_fn(|i| a.get(i).zip(b.get(i)).map_or(T::zero(), |(x, y)| *x + *y))
    }
    fn mul(a: [T; N], v: T) -> [T; N] {
        core::array::from_fn(|i| a.get(i).map_or(T::zero(), |x| *x * v))
    }
    fn edge(start: [T; N], edge: [T; N], d1: T, d2: T) -> [T; N] {
        let t = d1 / (d1 + d2);
        Self::add(start, Self::mul(edge, t))
    }
    fn vertex(point: [T; N], index: i32) -> Projection<T, N> {
        Projection { point, state: ProjectionState::Vertex, first_node: index, last_node: index }
    }
    fn edge_projection(point: [T; N], first: i32, last: i32) -> Projection<T, N> {
        Projection { point, state: ProjectionState::Edge, first_node: first, last_node: last }
    }
    pub fn point_triangle_projection(p: [T; N], a: [T; N], b: [T; N], c: [T; N]) -> Projection<T, N> {
        let ab = Self::sub(b, a);
        let ac = Self::sub(c, a);
        let bc = Self::sub(c, b);
        let ap = Self::sub(p, a);
        let bp = Self::sub(p, b);
        let cp = Self::sub(p, c);
        let abap = Self::dot(ab, ap);
        let acap = Self::dot(ac, ap);
        if abap <= T::zero() && acap <= T::zero() {
            return Self::vertex(a, 0);
        }
        let babp = T::zero() - Self::dot(ab, bp);
        let bcbp = Self::dot(bc, bp);
        if babp <= T::zero() && bcbp <= T::zero() {
            return Self::vertex(b, 1);
        }
        let cbcp = T::zero() - Self::dot(bc, cp);
        let cacp = T::zero() - Self::dot(ac, cp);
        if cacp <= T::zero() && cbcp <= T::zero() {
            return Self::vertex(c, 2);
        }
        let acbp = Self::dot(ac, bp);
        let vc = abap * acbp + babp * acap;
        if vc <= T::zero() && abap > T::zero() && babp > T::zero() {
            return Self::edge_projection(Self::edge(a, ab, abap, babp), 0, 1);
        }
        let abcp = Self::dot(ab, cp);
        let va = babp * cacp - abcp * acbp;
        if va <= T::zero() && bcbp > T::zero() && cbcp > T::zero() {
            return Self::edge_projection(Self::edge(b, bc, bcbp, cbcp), 1, 2);
        }
        let vb = abcp * acap + abap * cacp;
        if vb <= T::zero() && acbp > T::zero() && cacp > T::zero() {
            return Self::edge_projection(Self::edge(a, ac, acap, cacp), 2, 0);
        }
        let norm = va + vb + vc;
        if norm <= T::epsilon() * (va.abs() + vb.abs() + vc.abs()) {
            return Self::vertex(
                Self::mul(Self::add(Self::add(a, b), c), T::one() / (T::one() + T::one() + T::one())),
                0,
            );
        }
        let q = Self::add(Self::add(Self::mul(a, va), Self::mul(b, vb)), Self::mul(c, vc));
        Projection {
            point: Self::mul(q, T::one() / norm),
            state: ProjectionState::Inner,
            first_node: -9,
            last_node: -9,
        }
    }
    pub fn point_triangle_square_distance(p: [T; N], a: [T; N], b: [T; N], c: [T; N]) -> T {
        let q = Self::sub(Self::point_triangle_projection(p, a, b, c).point, p);
        Self::dot(q, q)
    }
}

pub type Aabb3 = Box<f64, 3>;
pub type Ray3 = Ray<f64, 3>;
pub type Tools3 = Tools<f64, 3>;

pub const BVH_CONSTANTS_MAX_TREE_DEPTH: i32 = 32;
pub const BVH_CONSTANTS_LEAF_NODE_SIZE_SINGLE: i32 = 1;
pub const BVH_CONSTANTS_LEAF_NODE_SIZE_AVERAGE: i32 = 4;
pub const BVH_CONSTANTS_LEAF_NODE_SIZE_DEFAULT: i32 = 5;
pub const BVH_CONSTANTS_LEAF_NODE_SIZE_SMALL: i32 = 8;
pub const BVH_CONSTANTS_NB_BINS_OPTIMAL: i32 = 32;
pub const BVH_CONSTANTS_NB_BINS_BEST: i32 = 48;
pub const THE_NODE_MIN_SIZE: f64 = 1.0e-5;

/// Serialized tree node data. Leaf ranges are inclusive, as in OCCT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeInfo {
    pub outer: bool,
    pub first: i32,
    pub second: i32,
    pub level: i32,
}

/// Checked Rust representation of an OCCT BVH tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree<T, const N: usize, const ARITY: usize = 2> {
    depth: i32,
    nodes: Vec<NodeInfo>,
    mins: Vec<[T; N]>,
    maxs: Vec<[T; N]>,
}

impl<T: Scalar, const N: usize, const ARITY: usize> Default for Tree<T, N, ARITY> {
    fn default() -> Self {
        Self { depth: 0, nodes: Vec::new(), mins: Vec::new(), maxs: Vec::new() }
    }
}

impl<T: Scalar, const N: usize, const ARITY: usize> Tree<T, N, ARITY> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn depth(&self) -> i32 {
        self.depth
    }
    pub fn set_depth(&mut self, depth: i32) {
        self.depth = depth;
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn length(&self) -> usize {
        self.nodes.len()
    }
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.mins.clear();
        self.maxs.clear();
        self.depth = 0;
    }
    pub fn reserve(&mut self, count: usize) {
        self.nodes.reserve(count);
        self.mins.reserve(count);
        self.maxs.reserve(count);
    }
    pub fn node(&self, index: usize) -> Result<&NodeInfo, &'static str> {
        self.nodes.get(index).ok_or("BVH node index out of range")
    }
    pub fn node_mut(&mut self, index: usize) -> Result<&mut NodeInfo, &'static str> {
        self.nodes.get_mut(index).ok_or("BVH node index out of range")
    }
    pub fn min_point(&self, index: usize) -> Result<&[T; N], &'static str> {
        self.mins.get(index).ok_or("BVH node index out of range")
    }
    pub fn max_point(&self, index: usize) -> Result<&[T; N], &'static str> {
        self.maxs.get(index).ok_or("BVH node index out of range")
    }
    pub fn begin_primitive(&self, index: usize) -> Result<i32, &'static str> {
        Ok(self.node(index)?.first)
    }
    pub fn end_primitive(&self, index: usize) -> Result<i32, &'static str> {
        Ok(self.node(index)?.second)
    }
    pub fn primitive_count(&self, index: usize) -> Result<i32, &'static str> {
        Ok(self.end_primitive(index)? - self.begin_primitive(index)? + 1)
    }
    pub fn level(&self, index: usize) -> Result<i32, &'static str> {
        Ok(self.node(index)?.level)
    }
    pub fn is_outer(&self, index: usize) -> Result<bool, &'static str> {
        Ok(self.node(index)?.outer)
    }
    pub fn child(&self, index: usize) -> Result<usize, &'static str> {
        let n = self.node(index)?;
        if n.outer {
            Err("leaf nodes do not have children")
        } else {
            usize::try_from(n.first).map_err(|_| "invalid child index")
        }
    }
    pub fn set_child(&mut self, index: usize, child: usize) -> Result<(), &'static str> {
        let child = i32::try_from(child).map_err(|_| "child index exceeds i32")?;
        let n = self.node_mut(index)?;
        n.first = child;
        n.outer = false;
        Ok(())
    }
    pub fn min_point_buffer(&self) -> &[[T; N]] {
        &self.mins
    }
    pub fn max_point_buffer(&self) -> &[[T; N]] {
        &self.maxs
    }
    pub fn node_buffer(&self) -> &[NodeInfo] {
        &self.nodes
    }
    pub fn add_leaf_node_bounds(&mut self, min: [T; N], max: [T; N], begin: i32, end: i32, level: i32) -> usize {
        let i = self.nodes.len();
        self.nodes.push(NodeInfo { outer: true, first: begin, second: end, level });
        self.mins.push(min);
        self.maxs.push(max);
        self.depth = self.depth.max(level);
        i
    }
    pub fn add_leaf_node(&mut self, bounds: &Box<T, N>, begin: i32, end: i32, level: i32) -> usize {
        self.add_leaf_node_bounds(bounds.min, bounds.max, begin, end, level)
    }
    pub fn add_inner_node_bounds(
        &mut self,
        min: [T; N],
        max: [T; N],
        left: usize,
        right: usize,
        level: i32,
    ) -> Result<usize, &'static str> {
        let l = i32::try_from(left).map_err(|_| "child index exceeds i32")?;
        let r = i32::try_from(right).map_err(|_| "child index exceeds i32")?;
        let i = self.nodes.len();
        self.nodes.push(NodeInfo { outer: false, first: l, second: r, level });
        self.mins.push(min);
        self.maxs.push(max);
        self.depth = self.depth.max(level);
        Ok(i)
    }
    pub fn add_inner_node(
        &mut self,
        bounds: &Box<T, N>,
        left: usize,
        right: usize,
        level: i32,
    ) -> Result<usize, &'static str> {
        self.add_inner_node_bounds(bounds.min, bounds.max, left, right, level)
    }
    pub fn estimate_sah(&self) -> T {
        let Some((root_min, root_max)) = self.mins.first().zip(self.maxs.first()) else { return T::zero() };
        let denom = Box::new(*root_min, *root_max).area();
        if denom.partial_cmp(&T::zero()) != Some(core::cmp::Ordering::Greater) {
            return T::zero();
        }
        let mut cost = T::zero();
        for (i, n) in self.nodes.iter().enumerate() {
            if n.outer
                && let Some((lo, hi)) = self.mins.get(i).zip(self.maxs.get(i))
            {
                cost = cost + Box::new(*lo, *hi).area() * T::from_i32(n.second - n.first + 1);
            }
        }
        cost / denom
    }
}

/// Work queue used by parallel BVH builders. Fetch updates the caller's busy state.
#[derive(Debug, Clone, Default)]
pub struct BuildQueue {
    queue: Vec<i32>,
    busy_threads: usize,
}

/// Dimensioned vector type used by BVH helper APIs.
pub type VectorType<T, const N: usize> = [T; N];
/// Dimensioned matrix type used by BVH helper APIs.
pub type MatrixType<T, const N: usize> = [[T; N]; N];
/// Dimensioned vector buffer used by BVH helper APIs.
pub type ArrayType<T, const N: usize> = Vec<[T; N]>;

#[derive(Debug, Clone, Copy, Default)]
pub struct VecComp<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> VecComp<T, N> {
    pub fn get(vector: &[T; N], axis: usize) -> Result<T, &'static str> {
        vector.get(axis).copied().ok_or("vector axis out of range")
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct CenterAxis<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> CenterAxis<T, N> {
    pub fn center(bounds: &Box<T, N>, axis: usize) -> Result<T, &'static str> {
        if axis >= N { Err("vector axis out of range") } else { Ok(bounds.center_axis(axis)) }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceCalculator<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> SurfaceCalculator<T, N> {
    pub fn area(size: [T; N]) -> T {
        Box::<T, N>::new([T::zero(); N], size).area()
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxMinMax<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> BoxMinMax<T, N> {
    pub fn cwise_min(lhs: &mut [T; N], rhs: &[T; N]) {
        for i in 0..N {
            if let (Some(a), Some(b)) = (lhs.get_mut(i), rhs.get(i))
                && *b < *a
            {
                *a = *b;
            }
        }
    }
    pub fn cwise_max(lhs: &mut [T; N], rhs: &[T; N]) {
        for i in 0..N {
            if let (Some(a), Some(b)) = (lhs.get_mut(i), rhs.get(i))
                && *b > *a
            {
                *a = *b;
            }
        }
    }
}

/// Checked operations on dimensioned arrays.
#[derive(Debug, Clone, Copy, Default)]
pub struct Array<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> Array<T, N> {
    pub fn value(array: &[[T; N]], index: usize) -> Result<&[T; N], &'static str> {
        array.get(index).ok_or("array index out of range")
    }
    pub fn change_value(array: &mut [[T; N]], index: usize) -> Result<&mut [T; N], &'static str> {
        array.get_mut(index).ok_or("array index out of range")
    }
    pub fn append(array: &mut Vec<[T; N]>, value: [T; N]) {
        array.push(value);
    }
    pub fn size(array: &[[T; N]]) -> usize {
        array.len()
    }
    pub fn clear(array: &mut Vec<[T; N]>) {
        array.clear();
    }
    pub fn reserve(array: &mut Vec<[T; N]>, count: usize) {
        array.reserve(count);
    }
}

pub fn int_floor(value: f64) -> i32 {
    libm::floor(value) as i32
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnitVector<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar, const N: usize> UnitVector<T, N> {
    pub fn dx() -> [T; N] {
        core::array::from_fn(|i| if i == 0 { T::one() } else { T::zero() })
    }
    pub fn dy() -> [T; N] {
        core::array::from_fn(|i| if i == 1 { T::one() } else { T::zero() })
    }
    pub fn dz() -> [T; N] {
        core::array::from_fn(|i| if i == 2 { T::one() } else { T::zero() })
    }
}

/// Homogeneous 4x4 matrix operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct MatrixOp<T, const N: usize>(core::marker::PhantomData<T>);
impl<T: Scalar> MatrixOp<T, 4> {
    pub fn multiply(matrix: &[[T; 4]; 4], vector: [T; 4]) -> Result<[T; 4], &'static str> {
        let mut out = [T::zero(); 4];
        for (row, result) in out.iter_mut().enumerate() {
            let Some(values) = matrix.get(row) else { return Err("matrix shape mismatch") };
            for (column, value) in values.iter().enumerate() {
                if let Some(v) = vector.get(column) {
                    *result = *result + *value * *v;
                }
            }
        }
        let w = out.get(3).copied().ok_or("matrix shape mismatch")?;
        if w.partial_cmp(&T::zero()) == Some(core::cmp::Ordering::Equal) {
            return Err("homogeneous coordinate is zero");
        }
        for value in &mut out {
            *value = *value / w;
        }
        Ok(out)
    }
    pub fn inverse(input: &[[T; 4]; 4]) -> Result<[[T; 4]; 4], &'static str> {
        let mut a = *input;
        let mut inv = [[T::zero(); 4]; 4];
        for i in 0..4 {
            if let Some(row) = inv.get_mut(i)
                && let Some(x) = row.get_mut(i)
            {
                *x = T::one();
            }
        }
        for pivot in 0..4 {
            let p = a.get(pivot).and_then(|r| r.get(pivot)).copied().ok_or("matrix shape mismatch")?;
            if p.partial_cmp(&T::zero()) == Some(core::cmp::Ordering::Equal) {
                return Err("singular matrix");
            }
            let ar = a.get_mut(pivot).ok_or("matrix shape mismatch")?;
            let ir = inv.get_mut(pivot).ok_or("matrix shape mismatch")?;
            for j in 0..4 {
                if let Some(v) = ar.get_mut(j) {
                    *v = *v / p;
                }
                if let Some(v) = ir.get_mut(j) {
                    *v = *v / p;
                }
            }
            let pr = *a.get(pivot).ok_or("matrix shape mismatch")?;
            let pir = *inv.get(pivot).ok_or("matrix shape mismatch")?;
            for row in 0..4 {
                if row != pivot {
                    let factor = *a.get(row).and_then(|r| r.get(pivot)).ok_or("matrix shape mismatch")?;
                    for col in 0..4 {
                        let av = a.get(row).and_then(|r| r.get(col)).copied().ok_or("matrix shape mismatch")?
                            - factor * pr.get(col).copied().ok_or("matrix shape mismatch")?;
                        let iv = inv.get(row).and_then(|r| r.get(col)).copied().ok_or("matrix shape mismatch")?
                            - factor * pir.get(col).copied().ok_or("matrix shape mismatch")?;
                        if let Some(x) = a.get_mut(row).and_then(|r| r.get_mut(col)) {
                            *x = av;
                        }
                        if let Some(x) = inv.get_mut(row).and_then(|r| r.get_mut(col)) {
                            *x = iv;
                        }
                    }
                }
            }
        }
        Ok(inv)
    }
}

/// Affine transform properties for 3D BVH bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform<T: Scalar> {
    matrix: [[T; 4]; 4],
    inverse: [[T; 4]; 4],
}
impl<T: Scalar> Default for Transform<T> {
    fn default() -> Self {
        let mut matrix = [[T::zero(); 4]; 4];
        for i in 0..4 {
            if let Some(x) = matrix.get_mut(i).and_then(|r| r.get_mut(i)) {
                *x = T::one();
            }
        }
        Self { matrix, inverse: matrix }
    }
}
impl<T: Scalar> Transform<T> {
    pub fn new(matrix: [[T; 4]; 4]) -> Result<Self, &'static str> {
        let inverse = MatrixOp::<T, 4>::inverse(&matrix)?;
        Ok(Self { matrix, inverse })
    }
    pub fn set_transform(&mut self, matrix: [[T; 4]; 4]) -> Result<(), &'static str> {
        let inverse = MatrixOp::<T, 4>::inverse(&matrix)?;
        self.matrix = matrix;
        self.inverse = inverse;
        Ok(())
    }
    pub fn transform(&self) -> &[[T; 4]; 4] {
        &self.matrix
    }
    pub fn inverse(&self) -> &[[T; 4]; 4] {
        &self.inverse
    }
    pub fn apply(&self, bounds: &Box<T, 3>) -> Result<Box<T, 3>, &'static str> {
        if !bounds.is_valid() {
            return Ok(*bounds);
        }
        let mut out = Box::<T, 3>::default();
        for mask in 0..8 {
            let p: [T; 3] = core::array::from_fn(|i| {
                if (mask >> i) & 1 == 0 {
                    bounds.min.get(i).copied().unwrap_or(T::zero())
                } else {
                    bounds.max.get(i).copied().unwrap_or(T::zero())
                }
            });
            let q = MatrixOp::<T, 4>::multiply(
                &self.matrix,
                [
                    p.first().copied().ok_or("point shape mismatch")?,
                    p.get(1).copied().ok_or("point shape mismatch")?,
                    p.get(2).copied().ok_or("point shape mismatch")?,
                    T::one(),
                ],
            )?;
            out.add([
                q.first().copied().ok_or("matrix shape mismatch")?,
                q.get(1).copied().ok_or("matrix shape mismatch")?,
                q.get(2).copied().ok_or("matrix shape mismatch")?,
            ]);
        }
        Ok(out)
    }
}
impl BuildQueue {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn size(&self) -> usize {
        self.queue.len()
    }
    pub fn enqueue(&mut self, item: i32) {
        self.queue.push(item)
    }
    pub fn fetch(&mut self, was_busy: bool) -> (i32, bool) {
        let item = if self.queue.is_empty() { -1 } else { self.queue.remove(0) };
        let now_busy = item != -1;
        if now_busy && !was_busy {
            self.busy_threads += 1
        } else if !now_busy && was_busy {
            self.busy_threads = self.busy_threads.saturating_sub(1)
        }
        (item, now_busy)
    }
    pub fn has_busy_threads(&self) -> bool {
        self.busy_threads != 0
    }
}
