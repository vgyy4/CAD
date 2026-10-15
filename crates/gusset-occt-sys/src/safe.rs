//! Safe wrappers over the raw shim functions. This is the only place outside the
//! `extern` block that touches raw pointers; dependents (`gusset-kernel-occt`, the import
//! worker) use only this module and stay free of `unsafe` (ADR 0008).
//!
//! Invariants upheld here:
//! - Every `*mut GsShape` we hold was returned by the shim and is freed exactly once, by
//!   [`RawShape`]'s `Drop`.
//! - Every `GsBuf` filled by the shim is copied into Rust memory and freed immediately.
//! - Pointers passed to the shim point to live Rust data for the duration of the call.
//! - OCCT mutates shapes lazily (e.g. triangulation caches), so every call that touches
//!   a shape holds that shape's mutex. Data exchange uses process-global OCCT settings,
//!   so it is serialized by [`EXCHANGE_LOCK`].

use crate::raw as sys;
use crate::{GS_OK, GsBuf, GsLoop, GsProfile, GsSeg, GsShape};
use std::sync::Mutex;

/// Why a shim call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShimError {
    /// The shim reported an error; the message comes from OCCT or the shim.
    Failed(String),
    /// The shim reported success but returned no shape.
    NoShape,
}

impl std::fmt::Display for ShimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShimError::Failed(m) => f.write_str(m),
            ShimError::NoShape => f.write_str("the geometry kernel returned no shape"),
        }
    }
}

impl std::error::Error for ShimError {}

pub type ShimResult<T> = Result<T, ShimError>;

static EXCHANGE_LOCK: Mutex<()> = Mutex::new(());

/// An owned shim shape.
#[derive(Debug)]
pub struct RawShape {
    ptr: Mutex<*mut GsShape>,
}

// SAFETY: the shim's shapes are heap objects not tied to any thread. All access goes
// through the mutex, so no two threads use the same OCCT shape concurrently.
unsafe impl Send for RawShape {}
// SAFETY: see `Send`; shared access is serialized by the mutex.
unsafe impl Sync for RawShape {}

impl Drop for RawShape {
    fn drop(&mut self) {
        let p = match self.ptr.get_mut() {
            Ok(p) => *p,
            Err(poisoned) => *poisoned.into_inner(),
        };
        if !p.is_null() {
            // SAFETY: `p` came from the shim, is non-null and is freed only here.
            unsafe { sys::gs_shape_free(p) };
        }
    }
}

impl RawShape {
    fn from_ptr(p: *mut GsShape) -> ShimResult<RawShape> {
        if p.is_null() {
            return Err(ShimError::NoShape);
        }
        Ok(RawShape { ptr: Mutex::new(p) })
    }

    /// Runs `f` with exclusive access to the raw pointer.
    fn with<R>(&self, f: impl FnOnce(*const GsShape) -> R) -> R {
        let guard = match self.ptr.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(*guard as *const GsShape)
    }
}

/// Runs `f` with exclusive access to several shapes, locking in address order to avoid
/// deadlock (the same shape may appear twice; it is then locked once).
fn with_many<R>(shapes: &[&RawShape], f: impl FnOnce(&[*const GsShape]) -> R) -> R {
    let mut order: Vec<usize> = (0..shapes.len()).collect();
    order.sort_by_key(|&i| shapes.get(i).map_or(0, |s| std::ptr::from_ref(*s) as usize));
    order.dedup_by_key(|i| shapes.get(*i).map_or(0, |s| std::ptr::from_ref(*s) as usize));
    let mut guards = Vec::with_capacity(order.len());
    for &i in &order {
        if let Some(s) = shapes.get(i) {
            guards.push((
                std::ptr::from_ref(*s) as usize,
                s.ptr.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
            ));
        }
    }
    let ptrs: Vec<*const GsShape> = shapes
        .iter()
        .map(|s| {
            let key = std::ptr::from_ref(*s) as usize;
            guards.iter().find(|(k, _)| *k == key).map_or(std::ptr::null(), |(_, g)| **g as *const GsShape)
        })
        .collect();
    f(&ptrs)
}

fn last_error() -> String {
    // SAFETY: returns a pointer to a thread-local NUL-terminated string owned by the shim,
    // valid until the next shim call on this thread; we copy it immediately.
    let p = unsafe { sys::gs_last_error() };
    if p.is_null() {
        return "unknown geometry kernel error".into();
    }
    // SAFETY: non-null and NUL-terminated per the shim contract.
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn check(rc: i32) -> ShimResult<()> {
    if rc == GS_OK { Ok(()) } else { Err(ShimError::Failed(last_error())) }
}

/// Copies a shim buffer of `T` into a Vec and frees it.
fn take<T: Copy>(mut b: GsBuf) -> Vec<T> {
    let out = if b.ptr.is_null() || b.len == 0 {
        Vec::new()
    } else {
        // SAFETY: the shim allocated `len` contiguous, initialized elements of type `T`
        // (as documented per function in gusset_occt.h), suitably aligned (malloc).
        unsafe { std::slice::from_raw_parts(b.ptr as *const T, b.len) }.to_vec()
    };
    // SAFETY: `b` was filled by the shim (or is empty) and is freed exactly once here.
    unsafe { sys::gs_buf_free(&mut b) };
    out
}

pub fn version() -> String {
    // SAFETY: returns a static NUL-terminated string.
    let p = unsafe { sys::gs_occt_version() };
    if p.is_null() {
        return String::new();
    }
    // SAFETY: non-null static C string.
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

#[derive(Debug, Clone)]
pub struct ProfileData {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub segs: Vec<GsSeg>,
    pub loops: Vec<GsLoop>,
    /// Spline control points (x, y pairs) referenced by `GsSeg::pole_first`.
    pub poles: Vec<f64>,
}

impl ProfileData {
    fn raw(&self) -> GsProfile {
        GsProfile {
            origin: self.origin,
            x: self.x,
            y: self.y,
            segs: self.segs.as_ptr(),
            nsegs: self.segs.len(),
            loops: self.loops.as_ptr(),
            nloops: self.loops.len(),
            poles: self.poles.as_ptr(),
            npoles: self.poles.len() / 2,
        }
    }
}

pub type OpOut = (RawShape, Vec<i64>);

pub fn extrude(p: &ProfileData, distance: f64, symmetric: bool) -> ShimResult<OpOut> {
    let raw = p.raw();
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: `raw` points into `p`'s vectors, alive for the call; out-params are valid.
    let rc = unsafe { sys::gs_extrude(&raw, distance, i32::from(symmetric), &mut out, &mut origins) };
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn revolve(p: &ProfileData, axis_origin: [f64; 2], axis_dir: [f64; 2], angle: f64) -> ShimResult<OpOut> {
    let raw = p.raw();
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: all pointers reference live locals for the duration of the call.
    let rc = unsafe { sys::gs_revolve(&raw, axis_origin.as_ptr(), axis_dir.as_ptr(), angle, &mut out, &mut origins) };
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

/// The path of a sweep: sketch curves (the only loop of the profile data) or edges of a shape.
#[derive(Debug)]
pub enum SweepPath<'a> {
    Curves(&'a ProfileData),
    Edges(&'a RawShape, &'a [u32]),
}

/// Sweeps the regions of `p` along `path`. `follow` turns the profile with the path
/// (corrected Frenet), else it only slides; `twist` (radians) turns it about the path.
pub fn sweep(p: &ProfileData, path: &SweepPath<'_>, follow: bool, twist: f64) -> ShimResult<OpOut> {
    let raw = p.raw();
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    let orientation = i32::from(follow);
    let rc = match path {
        SweepPath::Curves(c) => {
            let craw = c.raw();
            // SAFETY: `raw` and `craw` point into live vectors; out-params are valid.
            unsafe {
                sys::gs_sweep(
                    &raw,
                    &craw,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    orientation,
                    twist,
                    &mut out,
                    &mut origins,
                )
            }
        }
        SweepPath::Edges(shape, edges) => shape.with(|sp| {
            // SAFETY: the shape is locked and valid; `edges` and `raw` outlive the call.
            unsafe {
                sys::gs_sweep(
                    &raw,
                    std::ptr::null(),
                    sp,
                    edges.as_ptr(),
                    edges.len(),
                    orientation,
                    twist,
                    &mut out,
                    &mut origins,
                )
            }
        }),
    };
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

/// Lofts through the sections (one region each).
pub fn loft(sections: &[ProfileData], ruled: bool) -> ShimResult<OpOut> {
    let raws: Vec<GsProfile> = sections.iter().map(ProfileData::raw).collect();
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: `raws` points into the sections' vectors, alive for the call.
    let rc = unsafe { sys::gs_loft(raws.as_ptr(), raws.len(), i32::from(ruled), &mut out, &mut origins) };
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn boolean(op: i32, a: &RawShape, b: &RawShape) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    let rc = with_many(&[a, b], |p| {
        let (pa, pb) = (p.first().copied().unwrap_or(std::ptr::null()), p.get(1).copied().unwrap_or(std::ptr::null()));
        // SAFETY: both shapes are locked and valid; out-params are valid.
        unsafe { sys::gs_boolean(op, pa, pb, &mut out, &mut origins) }
    });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

/// Union or cut of `target` with all `tools` in one operation (origins: input 0 is the
/// target, `i + 1` is `tools[i]`).
pub fn boolean_many(op: i32, target: &RawShape, tools: &[&RawShape]) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    let mut all: Vec<&RawShape> = Vec::with_capacity(tools.len() + 1);
    all.push(target);
    all.extend_from_slice(tools);
    let rc = with_many(&all, |p| {
        let (first, rest) = p.split_first().map_or((std::ptr::null(), &[][..]), |(f, r)| (*f, r));
        // SAFETY: every shape is locked and valid; `rest` holds `tools.len()` pointers and
        // stays alive for the call; out-params are valid.
        unsafe { sys::gs_boolean_many(op, first, rest.as_ptr(), rest.len(), &mut out, &mut origins) }
    });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn blend(s: &RawShape, edges: &[u32], size: f64, fillet: bool) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    let rc = s.with(|p| {
        if fillet {
            // SAFETY: shape locked and valid; `edges` alive for the call.
            unsafe { sys::gs_fillet(p, edges.as_ptr(), edges.len(), size, &mut out, &mut origins) }
        } else {
            // SAFETY: as above.
            unsafe { sys::gs_chamfer(p, edges.as_ptr(), edges.len(), size, &mut out, &mut origins) }
        }
    });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn shell(s: &RawShape, faces: &[u32], thickness: f64) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: shape locked and valid; `faces` alive for the call; out-params valid.
    let rc = s.with(|p| unsafe { sys::gs_shell(p, faces.as_ptr(), faces.len(), thickness, &mut out, &mut origins) });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn offset_faces(s: &RawShape, faces: &[u32], distance: f64) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: shape locked and valid; `faces` alive for the call; out-params valid.
    let rc =
        s.with(|p| unsafe { sys::gs_offset_faces(p, faces.as_ptr(), faces.len(), distance, &mut out, &mut origins) });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

/// `params`: pull direction (3), a neutral-plane point (3), angle in radians.
pub fn draft(s: &RawShape, faces: &[u32], params: &[f64; 7]) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: shape locked and valid; `faces` and the 7 `params` alive for the call;
    // out-params valid.
    let rc =
        s.with(|p| unsafe { sys::gs_draft(p, faces.as_ptr(), faces.len(), params.as_ptr(), &mut out, &mut origins) });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn transform(kind: i32, params: &[f64; 7], s: &RawShape) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    // SAFETY: shape locked and valid; `params` points to 7 doubles alive for the call.
    let rc = s.with(|p| unsafe { sys::gs_transform(kind, params.as_ptr(), p, &mut out, &mut origins) });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub fn compound(shapes: &[&RawShape]) -> ShimResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = GsBuf::default();
    let rc = with_many(shapes, |p| {
        // SAFETY: every shape is locked and valid; `p` holds `shapes.len()` pointers.
        unsafe { sys::gs_compound(p.as_ptr(), p.len(), &mut out, &mut origins) }
    });
    check(rc)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

#[derive(Debug, Clone)]
pub struct RawTopology {
    pub faces: Vec<f64>,
    pub edges: Vec<f64>,
    pub adj: Vec<i64>,
    pub verts: Vec<f64>,
    pub counts: Vec<i64>,
}

pub fn topology(s: &RawShape) -> ShimResult<RawTopology> {
    let mut b: [GsBuf; 5] = Default::default();
    let rc = s.with(|p| {
        let [f, e, a, v, c] = &mut b;
        // SAFETY: shape locked; out-params are distinct valid GsBufs.
        unsafe { sys::gs_topology(p, f, e, a, v, c) }
    });
    let [f, e, a, v, c] = b;
    let t = RawTopology { faces: take(f), edges: take(e), adj: take(a), verts: take(v), counts: take(c) };
    check(rc)?;
    Ok(t)
}

pub fn mass(s: &RawShape) -> ShimResult<Vec<f64>> {
    let mut b = GsBuf::default();
    // SAFETY: shape locked; out-param valid.
    let rc = s.with(|p| unsafe { sys::gs_mass(p, &mut b) });
    let v = take(b);
    check(rc)?;
    Ok(v)
}

pub fn bbox(s: &RawShape) -> ShimResult<Vec<f64>> {
    let mut b = GsBuf::default();
    // SAFETY: shape locked; out-param valid.
    let rc = s.with(|p| unsafe { sys::gs_bbox(p, &mut b) });
    let v = take(b);
    check(rc)?;
    Ok(v)
}

#[derive(Debug, Clone)]
pub struct RawMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub face_ranges: Vec<u32>,
    pub edge_offsets: Vec<u32>,
    pub edge_points: Vec<f32>,
}

pub fn tessellate(s: &RawShape, lin: f64, ang: f64) -> ShimResult<RawMesh> {
    let mut b: [GsBuf; 6] = Default::default();
    let rc = s.with(|p| {
        let [pos, nrm, idx, fr, eo, ep] = &mut b;
        // SAFETY: shape locked; out-params are distinct valid GsBufs.
        unsafe { sys::gs_tessellate(p, lin, ang, pos, nrm, idx, fr, eo, ep) }
    });
    let [pos, nrm, idx, fr, eo, ep] = b;
    let m = RawMesh {
        positions: take(pos),
        normals: take(nrm),
        indices: take(idx),
        face_ranges: take(fr),
        edge_offsets: take(eo),
        edge_points: take(ep),
    };
    check(rc)?;
    Ok(m)
}

pub fn step_write(shapes: &[&RawShape], names: &[&str]) -> ShimResult<Vec<u8>> {
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let cnames: Vec<std::ffi::CString> =
        names.iter().map(|n| std::ffi::CString::new(n.replace('\0', "")).unwrap_or_default()).collect();
    let name_ptrs: Vec<*const std::ffi::c_char> = cnames.iter().map(|c| c.as_ptr()).collect();
    let mut out = GsBuf::default();
    let rc = with_many(shapes, |p| {
        // SAFETY: shapes locked; `p` and `name_ptrs` hold one pointer per shape and live
        // for the call; the names are NUL-terminated CStrings.
        unsafe { sys::gs_step_write(p.as_ptr(), name_ptrs.as_ptr(), p.len(), &mut out) }
    });
    let v = take(out);
    check(rc)?;
    Ok(v)
}

pub fn iges_write(shapes: &[&RawShape]) -> ShimResult<Vec<u8>> {
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = GsBuf::default();
    let rc = with_many(shapes, |p| {
        // SAFETY: shapes locked; `p` holds one pointer per shape.
        unsafe { sys::gs_iges_write(p.as_ptr(), p.len(), &mut out) }
    });
    let v = take(out);
    check(rc)?;
    Ok(v)
}

/// Reads every root shape of a STEP file from memory.
pub fn read_shapes(data: &[u8]) -> ShimResult<Vec<RawShape>> {
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut arr: *mut *mut GsShape = std::ptr::null_mut();
    let mut n = 0usize;
    // SAFETY: `data` alive for the call; out-params valid.
    let rc = unsafe { sys::gs_step_read(data.as_ptr(), data.len(), &mut arr, &mut n) };
    check(rc)?;
    Ok(collect_shapes(arr, n))
}

/// Reads every root shape of the IGES file at `path`. OCCT's IGES reader cannot read from
/// memory, so the caller provides a file (the sandboxed worker is allowed exactly that
/// one file, read-only).
pub fn read_iges_file(path: &std::path::Path) -> ShimResult<Vec<RawShape>> {
    let cpath = path
        .to_str()
        .and_then(|p| std::ffi::CString::new(p).ok())
        .ok_or_else(|| ShimError::Failed("the IGES file path is not valid UTF-8 text".into()))?;
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut arr: *mut *mut GsShape = std::ptr::null_mut();
    let mut n = 0usize;
    // SAFETY: `cpath` is a NUL-terminated string alive for the call; out-params valid.
    let rc = unsafe { sys::gs_iges_read_file(cpath.as_ptr(), &mut arr, &mut n) };
    check(rc)?;
    Ok(collect_shapes(arr, n))
}

/// Takes ownership of the `n` shapes in the shim-allocated array `arr` and frees the array.
fn collect_shapes(arr: *mut *mut GsShape, n: usize) -> Vec<RawShape> {
    let mut out = Vec::with_capacity(n);
    if !arr.is_null() {
        for i in 0..n {
            // SAFETY: the shim returned an array of `n` shape pointers.
            let p = unsafe { *arr.add(i) };
            if let Ok(s) = RawShape::from_ptr(p) {
                out.push(s);
            }
        }
        // Ownership of the shapes moved into `RawShape`s; free only the array itself by
        // passing n = 0 (the shim frees `n` shapes, then the array).
        // SAFETY: `arr` was allocated by the shim; with n = 0 only the array is freed.
        unsafe { sys::gs_shape_array_free(arr, 0) };
    }
    out
}

pub fn brep_write(s: &RawShape) -> ShimResult<Vec<u8>> {
    let mut out = GsBuf::default();
    // SAFETY: shape locked; out-param valid.
    let rc = s.with(|p| unsafe { sys::gs_brep_write(p, &mut out) });
    let v = take(out);
    check(rc)?;
    Ok(v)
}

pub fn brep_read(data: &[u8]) -> ShimResult<RawShape> {
    let mut out = std::ptr::null_mut();
    // SAFETY: `data` alive for the call; out-param valid.
    let rc = unsafe { sys::gs_brep_read(data.as_ptr(), data.len(), &mut out) };
    check(rc)?;
    RawShape::from_ptr(out)
}

/// Runs OCCT's validity check on a shape (`BRepCheck_Analyzer`).
pub fn check_valid(s: &RawShape) -> ShimResult<()> {
    // SAFETY: shape locked and valid for the call.
    let rc = s.with(|p| unsafe { sys::gs_check(p) });
    check(rc)
}
