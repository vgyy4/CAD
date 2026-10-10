//! The only module with `unsafe`: thin safe wrappers over `gusset-occt-sys`.
//!
//! Invariants upheld here:
//! - Every `*mut GsShape` we hold was returned by the shim and is freed exactly once, by
//!   [`RawShape`]'s `Drop`.
//! - Every `GsBuf` filled by the shim is copied into Rust memory and freed immediately.
//! - Pointers passed to the shim point to live Rust data for the duration of the call.
//! - OCCT mutates shapes lazily (e.g. triangulation caches), so every call that touches
//!   a shape holds that shape's mutex. Data exchange uses process-global OCCT settings,
//!   so it is serialized by [`EXCHANGE_LOCK`].

#![allow(unsafe_code)]

use gusset_kernel::{KResult, KernelError, KernelErrorKind};
use gusset_occt_sys as sys;
use std::sync::Mutex;

pub(crate) static EXCHANGE_LOCK: Mutex<()> = Mutex::new(());

/// An owned shim shape.
#[derive(Debug)]
pub(crate) struct RawShape {
    ptr: Mutex<*mut sys::GsShape>,
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
    fn from_ptr(p: *mut sys::GsShape) -> KResult<RawShape> {
        if p.is_null() {
            return Err(internal("the geometry kernel returned no shape"));
        }
        Ok(RawShape { ptr: Mutex::new(p) })
    }

    /// Runs `f` with exclusive access to the raw pointer.
    pub(crate) fn with<R>(&self, f: impl FnOnce(*const sys::GsShape) -> R) -> R {
        let guard = match self.ptr.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(*guard as *const sys::GsShape)
    }
}

/// Runs `f` with exclusive access to several shapes, locking in address order to avoid
/// deadlock (the same shape may appear twice; it is then locked once).
pub(crate) fn with_many<R>(shapes: &[&RawShape], f: impl FnOnce(&[*const sys::GsShape]) -> R) -> R {
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
    let ptrs: Vec<*const sys::GsShape> = shapes
        .iter()
        .map(|s| {
            let key = std::ptr::from_ref(*s) as usize;
            guards.iter().find(|(k, _)| *k == key).map_or(std::ptr::null(), |(_, g)| **g as *const sys::GsShape)
        })
        .collect();
    f(&ptrs)
}

fn internal(msg: &str) -> KernelError {
    KernelError::new(KernelErrorKind::Internal, msg)
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

pub(crate) fn check(rc: i32, kind: KernelErrorKind) -> KResult<()> {
    if rc == sys::GS_OK { Ok(()) } else { Err(KernelError::new(kind, last_error())) }
}

/// Copies a shim buffer of `T` into a Vec and frees it.
fn take<T: Copy>(mut b: sys::GsBuf) -> Vec<T> {
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

pub(crate) fn version() -> String {
    // SAFETY: returns a static NUL-terminated string.
    let p = unsafe { sys::gs_occt_version() };
    if p.is_null() {
        return String::new();
    }
    // SAFETY: non-null static C string.
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

pub(crate) struct ProfileData {
    pub(crate) origin: [f64; 3],
    pub(crate) x: [f64; 3],
    pub(crate) y: [f64; 3],
    pub(crate) segs: Vec<sys::GsSeg>,
    pub(crate) loops: Vec<sys::GsLoop>,
}

impl ProfileData {
    fn raw(&self) -> sys::GsProfile {
        sys::GsProfile {
            origin: self.origin,
            x: self.x,
            y: self.y,
            segs: self.segs.as_ptr(),
            nsegs: self.segs.len(),
            loops: self.loops.as_ptr(),
            nloops: self.loops.len(),
        }
    }
}

pub(crate) type OpOut = (RawShape, Vec<i64>);

pub(crate) fn extrude(p: &ProfileData, distance: f64, symmetric: bool) -> KResult<OpOut> {
    let raw = p.raw();
    let mut out = std::ptr::null_mut();
    let mut origins = sys::GsBuf::default();
    // SAFETY: `raw` points into `p`'s vectors, alive for the call; out-params are valid.
    let rc = unsafe { sys::gs_extrude(&raw, distance, i32::from(symmetric), &mut out, &mut origins) };
    check(rc, KernelErrorKind::OperationFailed)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub(crate) fn revolve(p: &ProfileData, axis_origin: [f64; 2], axis_dir: [f64; 2], angle: f64) -> KResult<OpOut> {
    let raw = p.raw();
    let mut out = std::ptr::null_mut();
    let mut origins = sys::GsBuf::default();
    // SAFETY: all pointers reference live locals for the duration of the call.
    let rc = unsafe { sys::gs_revolve(&raw, axis_origin.as_ptr(), axis_dir.as_ptr(), angle, &mut out, &mut origins) };
    check(rc, KernelErrorKind::OperationFailed)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub(crate) fn boolean(op: i32, a: &RawShape, b: &RawShape) -> KResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = sys::GsBuf::default();
    let rc = with_many(&[a, b], |p| {
        let (pa, pb) = (p.first().copied().unwrap_or(std::ptr::null()), p.get(1).copied().unwrap_or(std::ptr::null()));
        // SAFETY: both shapes are locked and valid; out-params are valid.
        unsafe { sys::gs_boolean(op, pa, pb, &mut out, &mut origins) }
    });
    check(rc, KernelErrorKind::OperationFailed)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub(crate) fn blend(s: &RawShape, edges: &[u32], size: f64, fillet: bool) -> KResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = sys::GsBuf::default();
    let rc = s.with(|p| {
        if fillet {
            // SAFETY: shape locked and valid; `edges` alive for the call.
            unsafe { sys::gs_fillet(p, edges.as_ptr(), edges.len(), size, &mut out, &mut origins) }
        } else {
            // SAFETY: as above.
            unsafe { sys::gs_chamfer(p, edges.as_ptr(), edges.len(), size, &mut out, &mut origins) }
        }
    });
    check(rc, KernelErrorKind::OperationFailed)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub(crate) fn compound(shapes: &[&RawShape]) -> KResult<OpOut> {
    let mut out = std::ptr::null_mut();
    let mut origins = sys::GsBuf::default();
    let rc = with_many(shapes, |p| {
        // SAFETY: every shape is locked and valid; `p` holds `shapes.len()` pointers.
        unsafe { sys::gs_compound(p.as_ptr(), p.len(), &mut out, &mut origins) }
    });
    check(rc, KernelErrorKind::OperationFailed)?;
    Ok((RawShape::from_ptr(out)?, take(origins)))
}

pub(crate) struct RawTopology {
    pub(crate) faces: Vec<f64>,
    pub(crate) edges: Vec<f64>,
    pub(crate) adj: Vec<i64>,
    pub(crate) verts: Vec<f64>,
    pub(crate) counts: Vec<i64>,
}

pub(crate) fn topology(s: &RawShape) -> KResult<RawTopology> {
    let mut b: [sys::GsBuf; 5] = Default::default();
    let rc = s.with(|p| {
        let [f, e, a, v, c] = &mut b;
        // SAFETY: shape locked; out-params are distinct valid GsBufs.
        unsafe { sys::gs_topology(p, f, e, a, v, c) }
    });
    let [f, e, a, v, c] = b;
    let t = RawTopology { faces: take(f), edges: take(e), adj: take(a), verts: take(v), counts: take(c) };
    check(rc, KernelErrorKind::Internal)?;
    Ok(t)
}

pub(crate) fn mass(s: &RawShape) -> KResult<Vec<f64>> {
    let mut b = sys::GsBuf::default();
    // SAFETY: shape locked; out-param valid.
    let rc = s.with(|p| unsafe { sys::gs_mass(p, &mut b) });
    let v = take(b);
    check(rc, KernelErrorKind::Internal)?;
    Ok(v)
}

pub(crate) fn bbox(s: &RawShape) -> KResult<Vec<f64>> {
    let mut b = sys::GsBuf::default();
    // SAFETY: shape locked; out-param valid.
    let rc = s.with(|p| unsafe { sys::gs_bbox(p, &mut b) });
    let v = take(b);
    check(rc, KernelErrorKind::Internal)?;
    Ok(v)
}

pub(crate) struct RawMesh {
    pub(crate) positions: Vec<f32>,
    pub(crate) normals: Vec<f32>,
    pub(crate) indices: Vec<u32>,
    pub(crate) face_ranges: Vec<u32>,
    pub(crate) edge_offsets: Vec<u32>,
    pub(crate) edge_points: Vec<f32>,
}

pub(crate) fn tessellate(s: &RawShape, lin: f64, ang: f64) -> KResult<RawMesh> {
    let mut b: [sys::GsBuf; 6] = Default::default();
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
    check(rc, KernelErrorKind::Internal)?;
    Ok(m)
}

pub(crate) fn step_write(shapes: &[&RawShape], names: &[&str]) -> KResult<Vec<u8>> {
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let cnames: Vec<std::ffi::CString> =
        names.iter().map(|n| std::ffi::CString::new(n.replace('\0', "")).unwrap_or_default()).collect();
    let name_ptrs: Vec<*const std::ffi::c_char> = cnames.iter().map(|c| c.as_ptr()).collect();
    let mut out = sys::GsBuf::default();
    let rc = with_many(shapes, |p| {
        // SAFETY: shapes locked; `p` and `name_ptrs` hold one pointer per shape and live
        // for the call; the names are NUL-terminated CStrings.
        unsafe { sys::gs_step_write(p.as_ptr(), name_ptrs.as_ptr(), p.len(), &mut out) }
    });
    let v = take(out);
    check(rc, KernelErrorKind::Io)?;
    Ok(v)
}

pub(crate) fn iges_write(shapes: &[&RawShape]) -> KResult<Vec<u8>> {
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = sys::GsBuf::default();
    let rc = with_many(shapes, |p| {
        // SAFETY: shapes locked; `p` holds one pointer per shape.
        unsafe { sys::gs_iges_write(p.as_ptr(), p.len(), &mut out) }
    });
    let v = take(out);
    check(rc, KernelErrorKind::Io)?;
    Ok(v)
}

pub(crate) fn read_shapes(data: &[u8], iges: bool) -> KResult<Vec<RawShape>> {
    let _g = EXCHANGE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut arr: *mut *mut sys::GsShape = std::ptr::null_mut();
    let mut n = 0usize;
    let rc = if iges {
        // SAFETY: `data` alive for the call; out-params valid.
        unsafe { sys::gs_iges_read(data.as_ptr(), data.len(), &mut arr, &mut n) }
    } else {
        // SAFETY: as above.
        unsafe { sys::gs_step_read(data.as_ptr(), data.len(), &mut arr, &mut n) }
    };
    check(rc, KernelErrorKind::Io)?;
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
    Ok(out)
}

pub(crate) fn brep_write(s: &RawShape) -> KResult<Vec<u8>> {
    let mut out = sys::GsBuf::default();
    // SAFETY: shape locked; out-param valid.
    let rc = s.with(|p| unsafe { sys::gs_brep_write(p, &mut out) });
    let v = take(out);
    check(rc, KernelErrorKind::Io)?;
    Ok(v)
}

pub(crate) fn brep_read(data: &[u8]) -> KResult<RawShape> {
    let mut out = std::ptr::null_mut();
    // SAFETY: `data` alive for the call; out-param valid.
    let rc = unsafe { sys::gs_brep_read(data.as_ptr(), data.len(), &mut out) };
    check(rc, KernelErrorKind::Io)?;
    RawShape::from_ptr(out)
}
