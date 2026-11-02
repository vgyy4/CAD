//! OCCT MathInteg quadrature algorithms.
use crate::mathlin::{self, Vector};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    NotConverged = 1,
    MaxIterations = 2,
    NumericalError = 3,
    InvalidInput = 4,
    InfiniteSolutions = 5,
    NoSolution = 6,
    NotPositiveDefinite = 7,
    Singular = 8,
    NonDescentDirection = 9,
}
#[derive(Debug, Clone, Copy)]
pub struct IntegConfig {
    pub initial_order: i32,
    pub max_order: i32,
    pub max_iterations: i32,
    pub tolerance: f64,
}
impl Default for IntegConfig {
    fn default() -> Self {
        Self { initial_order: 15, max_order: 61, max_iterations: 100, tolerance: 1e-10 }
    }
}
impl IntegConfig {
    pub fn new(tolerance: f64, max_iterations: i32) -> Self {
        Self { tolerance, max_iterations, ..Self::default() }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct KronrodConfig {
    pub initial_order: i32,
    pub max_order: i32,
    pub max_iterations: i32,
    pub tolerance: f64,
    pub nb_gauss_points: i32,
    pub adaptive: bool,
}
impl Default for KronrodConfig {
    fn default() -> Self {
        Self {
            initial_order: 15,
            max_order: 61,
            max_iterations: 100,
            tolerance: 1e-10,
            nb_gauss_points: 7,
            adaptive: true,
        }
    }
}
impl KronrodConfig {
    pub fn new(tolerance: f64, max_iterations: i32) -> Self {
        Self { tolerance, max_iterations, ..Self::default() }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct DoubleExpConfig {
    pub tolerance: f64,
    pub max_iterations: i32,
    pub nb_levels: i32,
    pub step_factor: f64,
}
impl Default for DoubleExpConfig {
    fn default() -> Self {
        Self { tolerance: 1e-10, max_iterations: 100, nb_levels: 6, step_factor: 0.5 }
    }
}
impl DoubleExpConfig {
    pub fn new(tolerance: f64, max_iterations: i32) -> Self {
        Self { tolerance, max_iterations, ..Self::default() }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct MultipleConfig {
    pub max_order: i32,
}
impl Default for MultipleConfig {
    fn default() -> Self {
        Self { max_order: 61 }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct IntegResult {
    pub status: Status,
    pub value: Option<f64>,
    pub absolute_error: Option<f64>,
    pub relative_error: Option<f64>,
    pub nb_points: usize,
    pub nb_iterations: usize,
}
impl Default for IntegResult {
    fn default() -> Self {
        Self {
            status: Status::NotConverged,
            value: None,
            absolute_error: None,
            relative_error: None,
            nb_points: 0,
            nb_iterations: 0,
        }
    }
}
impl IntegResult {
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
}
#[derive(Debug, Clone)]
pub struct SetResult {
    pub status: Status,
    pub values: Option<Vector>,
    pub nb_equations: usize,
}
impl Default for SetResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, values: None, nb_equations: 0 }
    }
}
impl SetResult {
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
}
pub trait Function {
    fn value(&mut self, x: f64) -> Option<f64>;
}
pub trait MultipleFunction {
    fn value(&mut self, x: &[f64]) -> Option<f64>;
}
pub trait SetFunction {
    fn nb_equations(&self) -> usize;
    fn value(&mut self, x: &[f64]) -> Option<Vec<f64>>;
}
fn scalar<F: Function + ?Sized>(f: &mut F, x: f64) -> Option<f64> {
    f.value(x)
}
fn gauss_nodes(n: i32, ordered: bool) -> Option<(Vector, Vector)> {
    if ordered {
        mathlin::ordered_gauss_points_and_weights(n, 1).ok().flatten()
    } else {
        Some((mathlin::gauss_points(n, 1).ok()?, mathlin::gauss_weights(n, 1).ok()?))
    }
}
pub fn gauss<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, order: i32) -> IntegResult {
    let mut r = IntegResult::default();
    if order < 1 {
        r.status = Status::InvalidInput;
        return r;
    }
    let Some((p, w)) = gauss_nodes(order, true) else {
        r.status = Status::InvalidInput;
        return r;
    };
    let half = 0.5 * (upper - lower);
    let mid = 0.5 * (upper + lower);
    let mut sum = 0.0;
    for i in 1..=order {
        let x = mid + half * p.value(i).unwrap_or(0.0);
        let Some(y) = scalar(f, x) else {
            r.status = Status::NumericalError;
            return r;
        };
        sum += w.value(i).unwrap_or(0.0) * y;
    }
    r.status = Status::Ok;
    r.value = Some(half * sum);
    r.nb_points = order as usize;
    r.nb_iterations = 1;
    r
}
pub fn gauss_composite<F: Function + ?Sized>(
    f: &mut F,
    lower: f64,
    upper: f64,
    intervals: i32,
    order: i32,
) -> IntegResult {
    let mut r = IntegResult::default();
    if intervals < 1 {
        r.status = Status::InvalidInput;
        return r;
    }
    let h = (upper - lower) / f64::from(intervals);
    let mut sum = 0.0;
    for i in 0..intervals {
        let aa = lower + f64::from(i) * h;
        let bb = aa + h;
        let q = gauss(f, aa, bb, order);
        if !q.is_done() {
            r.status = q.status;
            r.value = Some(sum);
            r.nb_points += q.nb_points;
            return r;
        }
        sum += q.value.unwrap_or(0.0);
        r.nb_points += q.nb_points;
    }
    r.status = Status::Ok;
    r.value = Some(sum);
    r.nb_iterations = 1;
    r
}
pub fn gauss_adaptive<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, c: &IntegConfig) -> IntegResult {
    if c.initial_order < 1 || c.max_order < c.initial_order || c.max_order > 61 || c.max_iterations < 1 {
        return IntegResult { status: Status::InvalidInput, ..IntegResult::default() };
    }
    let mut co = c.initial_order;
    let mut fi = c.max_order.min((2 * co).min(61));
    if fi == co {
        if co > 1 {
            co -= 1
        } else if c.max_order > 1 {
            fi = 2
        }
    }
    let a = gauss(f, lower, upper, co);
    if !a.is_done() {
        return a;
    }
    let b = gauss(f, lower, upper, fi);
    if !b.is_done() {
        return b;
    }
    let av = a.value.unwrap_or(0.0);
    let bv = b.value.unwrap_or(0.0);
    let e = (bv - av).abs();
    let scale = bv.abs().max(1e-15);
    let mut r = IntegResult {
        status: Status::Ok,
        value: Some(bv),
        absolute_error: Some(e),
        relative_error: Some(e / scale),
        nb_points: fi as usize,
        nb_iterations: 1,
    };
    if e < c.tolerance * scale {
        return r;
    }
    if c.max_iterations <= 1 {
        r.status = Status::MaxIterations;
        return r;
    }
    let mut sub = *c;
    sub.max_iterations -= 1;
    let mid = 0.5 * (lower + upper);
    let l = gauss_adaptive(f, lower, mid, &sub);
    if !l.is_done() {
        return IntegResult { status: l.status, value: l.value, ..IntegResult::default() };
    }
    let rr = gauss_adaptive(f, mid, upper, &sub);
    if !rr.is_done() {
        return IntegResult {
            status: rr.status,
            value: Some(l.value.unwrap_or(0.0) + rr.value.unwrap_or(0.0)),
            ..IntegResult::default()
        };
    }
    let v = l.value.unwrap_or(0.0) + rr.value.unwrap_or(0.0);
    let er = l.absolute_error.unwrap_or(0.0) + rr.absolute_error.unwrap_or(0.0);
    IntegResult {
        status: Status::Ok,
        value: Some(v),
        absolute_error: Some(er),
        relative_error: Some(er / v.abs().max(1e-15)),
        nb_points: l.nb_points + rr.nb_points,
        nb_iterations: l.nb_iterations.max(rr.nb_iterations) + 1,
    }
}
pub fn gauss_multiple<F: MultipleFunction + ?Sized>(
    f: &mut F,
    lower: &Vector,
    upper: &Vector,
    orders: &[i32],
) -> IntegResult {
    gauss_multiple_with_config(f, lower, upper, orders, &MultipleConfig::default())
}
pub fn gauss_multiple_with_config<F: MultipleFunction + ?Sized>(
    f: &mut F,
    lower: &Vector,
    upper: &Vector,
    orders: &[i32],
    c: &MultipleConfig,
) -> IntegResult {
    let dim = orders.len();
    if dim == 0 || lower.as_slice().len() != dim || upper.as_slice().len() != dim {
        return IntegResult { status: Status::InvalidInput, ..IntegResult::default() };
    }
    let ord: Vec<i32> = orders.iter().map(|x| (*x).min(c.max_order).max(1)).collect();
    let mut tables = Vec::new();
    for n in &ord {
        let Some((p, w)) = gauss_nodes(*n, true) else {
            return IntegResult { status: Status::InvalidInput, ..IntegResult::default() };
        };
        tables.push((p, w));
    }
    let mut mid = Vec::new();
    let mut half = Vec::new();
    for i in 0..dim {
        let lo = lower.as_slice().get(i).copied().unwrap_or(0.0);
        let hi = upper.as_slice().get(i).copied().unwrap_or(0.0);
        mid.push(0.5 * (lo + hi));
        half.push(0.5 * (hi - lo));
    }
    let mut idx = vec![0usize; dim];
    let mut sum = 0.0;
    loop {
        let mut x = Vec::new();
        let mut wt = 1.0;
        for j in 0..dim {
            let k = idx.get(j).copied().unwrap_or(0);
            let (p, w) = tables.get(j).unwrap_or_else(|| unreachable!());
            x.push(
                mid.get(j).copied().unwrap_or(0.0)
                    + half.get(j).copied().unwrap_or(0.0) * p.as_slice().get(k).copied().unwrap_or(0.0),
            );
            wt *= w.as_slice().get(k).copied().unwrap_or(0.0);
        }
        let Some(v) = f.value(&x) else {
            return IntegResult { status: Status::NotConverged, ..IntegResult::default() };
        };
        sum += wt * v;
        let mut advanced = false;
        for j in (0..dim).rev() {
            let Some(slot) = idx.get_mut(j) else { continue };
            *slot += 1;
            if *slot < ord.get(j).copied().unwrap_or(0).max(0) as usize {
                advanced = true;
                break;
            }
            *slot = 0;
        }
        if !advanced {
            break;
        }
    }
    for h in half {
        sum *= h;
    }
    IntegResult { status: Status::Ok, value: Some(sum), ..IntegResult::default() }
}
pub fn gauss_multiple_uniform<F: MultipleFunction + ?Sized>(
    f: &mut F,
    lower: &Vector,
    upper: &Vector,
    order: i32,
) -> IntegResult {
    let n = lower.as_slice().len();
    gauss_multiple(f, lower, upper, &vec![order; n])
}
pub fn gauss_set<F: SetFunction + ?Sized>(f: &mut F, lower: f64, upper: f64, order: i32) -> SetResult {
    let ne = f.nb_equations();
    let mut r = SetResult { nb_equations: ne, ..SetResult::default() };
    if ne == 0 {
        r.status = Status::InvalidInput;
        return r;
    }
    let n = order.clamp(1, 61);
    let Some((p, w)) = gauss_nodes(n, true) else {
        r.status = Status::InvalidInput;
        return r;
    };
    let xm = 0.5 * (lower + upper);
    let xr = 0.5 * (upper - lower);
    let mut vals = vec![0.0; ne];
    let ind = (n / 2) as usize;
    let ind1 = ((n + 1) / 2) as usize;
    let point = |i: usize| p.value(i as i32 + 1).unwrap_or(0.0);
    let weight = |i: usize| w.value(i as i32 + 1).unwrap_or(0.0);
    if ind1 > ind {
        let Some(v) = f.value(&[xm]) else {
            r.status = Status::NotConverged;
            return r;
        };
        for (j, slot) in vals.iter_mut().enumerate() {
            *slot = v.get(j).copied().unwrap_or(0.0) * weight(ind1 - 1);
        }
    }
    for i in 0..ind {
        let Some(f1) = f.value(&[xm + xr * point(i)]) else {
            r.status = Status::NotConverged;
            return r;
        };
        let Some(f2) = f.value(&[xm - xr * point(i)]) else {
            r.status = Status::NotConverged;
            return r;
        };
        for (j, slot) in vals.iter_mut().enumerate() {
            *slot += (f1.get(j).copied().unwrap_or(0.0) + f2.get(j).copied().unwrap_or(0.0)) * weight(i);
        }
    }
    for v in &mut vals {
        *v *= xr;
    }
    r.values = Vector::from_slice(1, &vals).ok();
    r.status = Status::Ok;
    r
}
pub fn gauss_set_vectors<F: SetFunction + ?Sized>(f: &mut F, lower: &Vector, upper: &Vector, order: i32) -> SetResult {
    let lo = lower.as_slice().first().copied().unwrap_or(0.0);
    let hi = upper.as_slice().first().copied().unwrap_or(0.0);
    gauss_set(f, lo, hi, order)
}
fn de_infinite<F: Function + ?Sized>(f: &mut F, lower: f64, c: &DoubleExpConfig, both: bool) -> IntegResult {
    let hp = core::f64::consts::PI / 2.0;
    let (mut h, mut prev, mut total) = (1.0, 0.0, 0usize);
    for level in 0..c.nb_levels {
        let (mut sum, mut nb) = (0.0, 0);
        let start = if level == 0 { 0 } else { 1 };
        let step = if level == 0 { 1 } else { 2 };
        for sign in [-1.0, 1.0] {
            let first = if sign < 0.0 && level == 0 { 1 } else { start };
            let mut k = first;
            loop {
                let t = if sign < 0.0 { -f64::from(k) * h } else { f64::from(k) * h };
                let st = libm::sinh(t);
                let ct = libm::cosh(t);
                let u = hp * st;
                if (!both && u > 700.0) || u.abs() > 700.0 {
                    break;
                }
                let eu = libm::exp(u);
                let x = if both { libm::sinh(u) } else { lower + eu };
                let weight = if both { hp * ct * libm::cosh(u) } else { hp * ct * eu };
                if weight < 1e-15 || !weight.is_finite() || (!both && u < -30.0) {
                    break;
                }
                if let Some(v) = f.value(x)
                    && v.is_finite()
                {
                    sum += weight * v;
                    nb += 1;
                }
                if nb > 10000 {
                    break;
                }
                k += step;
            }
        }
        let ls = h * sum;
        total += nb as usize;
        let new = if level == 0 { ls } else { 0.5 * prev + ls };
        if level > 0 {
            let ae = (new - prev).abs();
            let re = ae / new.abs().max(1e-15);
            if re < c.tolerance {
                return IntegResult {
                    status: Status::Ok,
                    value: Some(new),
                    absolute_error: Some(ae),
                    relative_error: Some(re),
                    nb_points: total,
                    nb_iterations: (level + 1) as usize,
                };
            }
        }
        prev = new;
        h *= 0.5;
    }
    IntegResult {
        status: Status::Ok,
        value: Some(prev),
        nb_points: total,
        nb_iterations: c.nb_levels.max(0) as usize,
        ..IntegResult::default()
    }
}
pub fn kronrod_rule<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, n: i32) -> IntegResult {
    let mut r = IntegResult::default();
    let Some(kn) = n.checked_mul(2).and_then(|x| x.checked_add(1)) else {
        r.status = Status::NumericalError;
        return r;
    };
    let Some((kp, kw)) = mathlin::kronrod_points_and_weights(kn, 1) else {
        r.status = Status::NumericalError;
        return r;
    };
    let Some((_, gw)) = mathlin::ordered_gauss_points_and_weights(n, 1).ok().flatten() else {
        r.status = Status::NumericalError;
        return r;
    };
    let half = 0.5 * (upper - lower);
    let mid = 0.5 * (upper + lower);
    let np = (kn + 1) / 2;
    let mut gv = 0.0;
    let mut kv = 0.0;
    let mut f1 = vec![0.0; np as usize];
    let mut f2 = vec![0.0; np as usize];
    let mut i = 2;
    while i < np {
        let dx = half * kp.value(i).unwrap_or(0.0);
        let Some(v1) = f.value(mid + dx) else {
            r.status = Status::NumericalError;
            return r;
        };
        let Some(v2) = f.value(mid - dx) else {
            r.status = Status::NumericalError;
            return r;
        };
        if let Some(slot) = f1.get_mut(i as usize) {
            *slot = v1;
        }
        if let Some(slot) = f2.get_mut(i as usize) {
            *slot = v2;
        }
        gv += (v1 + v2) * gw.value(i / 2).unwrap_or(0.0);
        kv += (v1 + v2) * kw.value(i).unwrap_or(0.0);
        i += 2;
    }
    let Some(fc) = f.value(mid) else {
        r.status = Status::NumericalError;
        return r;
    };
    kv += fc * kw.value(np).unwrap_or(0.0);
    if np % 2 == 0 {
        gv += fc * gw.value(np / 2).unwrap_or(0.0)
    }
    i = 1;
    while i < np {
        let dx = half * kp.value(i).unwrap_or(0.0);
        let Some(v1) = f.value(mid + dx) else {
            r.status = Status::NumericalError;
            return r;
        };
        let Some(v2) = f.value(mid - dx) else {
            r.status = Status::NumericalError;
            return r;
        };
        if let Some(slot) = f1.get_mut(i as usize) {
            *slot = v1;
        }
        if let Some(slot) = f2.get_mut(i as usize) {
            *slot = v2;
        }
        kv += (v1 + v2) * kw.value(i).unwrap_or(0.0);
        i += 2;
    }
    let mean = 0.5 * kv;
    let mut asc = (fc - mean).abs() * kw.value(np).unwrap_or(0.0);
    for j in 1..np as usize {
        asc += kw.value(j as i32).unwrap_or(0.0)
            * ((f1.get(j).copied().unwrap_or(0.0) - mean).abs() + (f2.get(j).copied().unwrap_or(0.0) - mean).abs());
    }
    asc *= half;
    kv *= half;
    gv *= half;
    let mut err = (kv - gv).abs();
    if asc != 0.0 && err != 0.0 {
        let scale = libm::pow(200.0 * err / asc, 1.5);
        if scale < 1.0 {
            err = err.min(asc * scale)
        }
    }
    r.status = Status::Ok;
    r.value = Some(kv);
    r.absolute_error = Some(err);
    r.relative_error = Some(err / kv.abs().max(1e-15));
    r.nb_points = kn as usize;
    r.nb_iterations = 1;
    r
}
pub fn kronrod<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, c: &KronrodConfig) -> IntegResult {
    if !c.adaptive {
        return kronrod_rule(f, lower, upper, c.nb_gauss_points);
    }
    let init = kronrod_rule(f, lower, upper, c.nb_gauss_points);
    if !init.is_done() {
        return init;
    }
    let mut heap = vec![(lower, upper, init.value.unwrap_or(0.0), init.absolute_error.unwrap_or(0.0))];
    let first = heap.first().copied().unwrap_or((lower, upper, 0.0, 0.0));
    let mut tv = first.2;
    let mut te = first.3;
    let mut points = init.nb_points;
    let mut it = 1usize;
    while it < c.max_iterations.max(0) as usize {
        if te < c.tolerance * tv.abs().max(1e-15) {
            break;
        }
        let (mut idx, mut me) = (0usize, 0.0);
        for (i, v) in heap.iter().enumerate() {
            if v.3 > me {
                me = v.3;
                idx = i
            }
        }
        if me < 1e-15 {
            break;
        }
        let (a, b, wv, we) = heap.get(idx).copied().unwrap_or((lower, upper, 0.0, 0.0));
        let mid = 0.5 * (a + b);
        let l = kronrod_rule(f, a, mid, c.nb_gauss_points);
        let rr = kronrod_rule(f, mid, b, c.nb_gauss_points);
        if !l.is_done() || !rr.is_done() {
            return IntegResult {
                status: Status::NumericalError,
                value: Some(tv),
                absolute_error: Some(te),
                nb_points: points,
                nb_iterations: it,
                relative_error: None,
            };
        }
        tv -= wv;
        te -= we;
        tv += l.value.unwrap_or(0.0) + rr.value.unwrap_or(0.0);
        te += l.absolute_error.unwrap_or(0.0) + rr.absolute_error.unwrap_or(0.0);
        points += l.nb_points + rr.nb_points;
        it += 1;
        if let Some(slot) = heap.get_mut(idx) {
            *slot = (a, mid, l.value.unwrap_or(0.0), l.absolute_error.unwrap_or(0.0));
        }
        heap.push((mid, b, rr.value.unwrap_or(0.0), rr.absolute_error.unwrap_or(0.0)));
    }
    IntegResult {
        status: Status::Ok,
        value: Some(tv),
        absolute_error: Some(te),
        relative_error: Some(te / tv.abs().max(1e-15)),
        nb_points: points,
        nb_iterations: it,
    }
}
pub fn kronrod_auto<F: Function + ?Sized>(f: &mut F, a: f64, b: f64, tol: f64, max_order: i32) -> IntegResult {
    let mut n = 7;
    while n <= max_order {
        let r = kronrod_rule(f, a, b, n);
        if r.is_done() && r.relative_error.is_some_and(|e| e < tol) {
            return r;
        }
        n += 4;
    }
    let c = KronrodConfig {
        tolerance: tol,
        nb_gauss_points: 7,
        adaptive: true,
        max_iterations: 50,
        ..KronrodConfig::default()
    };
    kronrod(f, a, b, &c)
}
pub fn kronrod_semi_infinite<F: Function + ?Sized>(f: &mut F, a: f64, c: &KronrodConfig) -> IntegResult {
    struct T<'a, F: ?Sized> {
        f: &'a mut F,
        a: f64,
    }
    impl<F: Function + ?Sized> Function for T<'_, F> {
        fn value(&mut self, t: f64) -> Option<f64> {
            if t >= 1.0 {
                return Some(0.0);
            }
            let x = self.a + t / (1.0 - t);
            let jac = 1.0 / ((1.0 - t) * (1.0 - t));
            Some(self.f.value(x)? * jac)
        }
    }
    let mut t = T { f, a };
    kronrod(&mut t, 0.0, 1.0, c)
}
pub fn kronrod_infinite<F: Function + ?Sized>(f: &mut F, c: &KronrodConfig) -> IntegResult {
    struct T<'a, F: ?Sized> {
        f: &'a mut F,
    }
    impl<F: Function + ?Sized> Function for T<'_, F> {
        fn value(&mut self, t: f64) -> Option<f64> {
            if t.abs() >= 1.0 {
                return Some(0.0);
            }
            let t2 = t * t;
            let x = t / (1.0 - t2);
            let jac = (1.0 + t2) / ((1.0 - t2) * (1.0 - t2));
            Some(self.f.value(x)? * jac)
        }
    }
    let mut t = T { f };
    kronrod(&mut t, -1.0, 1.0, c)
}
pub fn tanh_sinh<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, c: &DoubleExpConfig) -> IntegResult {
    let mut r = IntegResult::default();
    if lower >= upper {
        r.status = Status::InvalidInput;
        return r;
    }
    let hp = core::f64::consts::PI / 2.0;
    let mid = 0.5 * (upper + lower);
    let half = 0.5 * (upper - lower);
    let (mut h, mut prev, mut total) = (1.0, 0.0, 0usize);
    for level in 0..c.nb_levels {
        let (mut sum, mut nb) = (0.0, 0);
        let start = if level == 0 { 0 } else { 1 };
        let step = if level == 0 { 1 } else { 2 };
        for sign in [1.0, -1.0] {
            let first = if sign < 0.0 && level == 0 { 1 } else { start };
            let mut k = first;
            loop {
                let t = if sign < 0.0 { -f64::from(k) * h } else { f64::from(k) * h };
                let st = libm::sinh(t);
                let ct = libm::cosh(t);
                let u = hp * st;
                if u.abs() > 700.0 {
                    break;
                }
                let tu = libm::tanh(u);
                let cu = libm::cosh(u);
                let sech = 1.0 / (cu * cu);
                let x = mid + half * tu;
                if x <= lower + 1e-15 || x >= upper - 1e-15 {
                    break;
                }
                let weight = half * hp * ct * sech;
                if weight < 1e-15 {
                    break;
                }
                if let Some(v) = f.value(x)
                    && v.is_finite()
                {
                    sum += weight * v;
                    nb += 1;
                }
                if nb > 10000 {
                    break;
                }
                k += step;
            }
        }
        let ls = h * sum;
        total += nb as usize;
        let new = if level == 0 { ls } else { 0.5 * prev + ls };
        if level > 0 {
            let ae = (new - prev).abs();
            let re = ae / new.abs().max(1e-15);
            if re < c.tolerance {
                return IntegResult {
                    status: Status::Ok,
                    value: Some(new),
                    absolute_error: Some(ae),
                    relative_error: Some(re),
                    nb_points: total,
                    nb_iterations: (level + 1) as usize,
                };
            }
        }
        prev = new;
        h *= 0.5;
    }
    IntegResult {
        status: Status::Ok,
        value: Some(prev),
        nb_points: total,
        nb_iterations: c.nb_levels.max(0) as usize,
        ..IntegResult::default()
    }
}
pub fn exp_sinh<F: Function + ?Sized>(f: &mut F, lower: f64, c: &DoubleExpConfig) -> IntegResult {
    de_infinite(f, lower, c, false)
}
pub fn sinh_sinh<F: Function + ?Sized>(f: &mut F, c: &DoubleExpConfig) -> IntegResult {
    de_infinite(f, 0.0, c, true)
}
pub fn double_exponential<F: Function + ?Sized>(f: &mut F, a: f64, b: f64, c: &DoubleExpConfig) -> IntegResult {
    tanh_sinh(f, a, b, c)
}
pub fn tanh_sinh_singular<F: Function + ?Sized>(f: &mut F, a: f64, b: f64, tol: f64) -> IntegResult {
    let c = DoubleExpConfig { nb_levels: 8, ..DoubleExpConfig::new(tol, 100) };
    tanh_sinh(f, a, b, &c)
}
pub fn tanh_sinh_with_singularity<F: Function + ?Sized>(
    f: &mut F,
    a: f64,
    b: f64,
    singularity: f64,
    c: &DoubleExpConfig,
) -> IntegResult {
    if singularity <= a || singularity >= b {
        return tanh_sinh(f, a, b, c);
    }
    let mut l = tanh_sinh(f, a, singularity, c);
    let r = tanh_sinh(f, singularity, b, c);
    if !l.is_done() {
        return l;
    }
    if !r.is_done() {
        return r;
    }
    l.value = Some(l.value.unwrap_or(0.0) + r.value.unwrap_or(0.0));
    l.nb_points += r.nb_points;
    l.nb_iterations = l.nb_iterations.max(r.nb_iterations);
    if let (Some(le), Some(re)) = (l.absolute_error, r.absolute_error) {
        let error = le + re;
        l.absolute_error = Some(error);
        l.relative_error = Some(error / l.value.unwrap_or(0.0).abs().max(1e-15));
    }
    l
}
