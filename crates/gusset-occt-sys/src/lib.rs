//! Raw FFI bindings to Gusset's C shim over OCCT (`cpp/gusset_occt.h`).
//!
//! This is one of the few crates allowed to contain `unsafe` (ADR 0008). The raw
//! `extern` functions are private; the [`safe`] module wraps every one of them, so
//! dependents (`gusset-kernel-occt`, `gusset-worker`) contain no `unsafe` code.
//!
//! When the crate is built without OCCT (`GUSSET_OCCT_DIR` unset), [`AVAILABLE`] is false
//! and no symbols are declared.

#![allow(non_camel_case_types)]

/// Whether this build links OCCT.
pub const AVAILABLE: bool = cfg!(gusset_occt);

pub const GS_OK: i32 = 0;
pub const GS_ERR: i32 = 1;

pub const GS_SEG_LINE: i32 = 0;
pub const GS_SEG_ARC: i32 = 1;
pub const GS_SEG_CIRCLE: i32 = 2;
pub const GS_SEG_BEZIER: i32 = 3;

pub const GS_ORIGIN_LATERAL: i64 = 0;
pub const GS_ORIGIN_CAP: i64 = 1;
pub const GS_ORIGIN_INPUT: i64 = 2;
pub const GS_ORIGIN_FROM_EDGE: i64 = 3;
pub const GS_ORIGIN_FROM_VERTEX: i64 = 4;
pub const GS_ORIGIN_OFFSET: i64 = 5;
pub const GS_ORIGIN_SWEPT: i64 = 6;
pub const GS_ORIGIN_SECTION: i64 = 7;

pub const GS_XF_MIRROR: i32 = 0;
pub const GS_XF_TRANSLATE: i32 = 1;
pub const GS_XF_ROTATE: i32 = 2;

pub const GS_BOOL_UNION: i32 = 0;
pub const GS_BOOL_CUT: i32 = 1;
pub const GS_BOOL_INTERSECT: i32 = 2;

/// Opaque shape handle owned by the shim.
#[repr(C)]
#[derive(Debug)]
pub struct GsShape {
    _private: [u8; 0],
}

/// A shim-allocated array; `len` counts elements.
#[repr(C)]
#[derive(Debug)]
pub struct GsBuf {
    pub ptr: *mut core::ffi::c_void,
    pub len: usize,
}

impl Default for GsBuf {
    fn default() -> Self {
        GsBuf { ptr: core::ptr::null_mut(), len: 0 }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GsSeg {
    pub kind: i32,
    pub ccw: i32,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub c: [f64; 2],
    pub r: f64,
    pub tag: u64,
    /// Bézier only: the four control points are `poles[pole_first .. pole_first + pole_count]`
    /// of the profile (x, y pairs; `pole_count` is 4).
    pub pole_first: u32,
    pub pole_count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GsLoop {
    pub first: u32,
    pub count: u32,
    pub region: u32,
    pub outer: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct GsProfile {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub segs: *const GsSeg,
    pub nsegs: usize,
    pub loops: *const GsLoop,
    pub nloops: usize,
    /// Control points of all spline segments, as x, y pairs; `npoles` counts points.
    pub poles: *const f64,
    pub npoles: usize,
}

#[cfg(gusset_occt)]
pub mod safe;

#[cfg(gusset_occt)]
mod raw {
    use super::{GsBuf, GsProfile, GsShape};

    unsafe extern "C" {
        pub(crate) fn gs_last_error() -> *const core::ffi::c_char;
        pub(crate) fn gs_occt_version() -> *const core::ffi::c_char;
        pub(crate) fn gs_shape_free(s: *mut GsShape);
        pub(crate) fn gs_buf_free(b: *mut GsBuf);
        pub(crate) fn gs_shape_array_free(shapes: *mut *mut GsShape, n: usize);

        pub(crate) fn gs_extrude(
            p: *const GsProfile,
            distance: f64,
            symmetric: i32,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_revolve(
            p: *const GsProfile,
            axis_origin: *const f64,
            axis_dir: *const f64,
            angle: f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_sweep(
            p: *const GsProfile,
            path_curves: *const GsProfile,
            path_shape: *const GsShape,
            path_edges: *const u32,
            npath_edges: usize,
            orientation: i32,
            twist: f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_loft(
            sections: *const GsProfile,
            nsections: usize,
            ruled: i32,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_boolean(
            op: i32,
            target: *const GsShape,
            tool: *const GsShape,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_boolean_many(
            op: i32,
            target: *const GsShape,
            tools: *const *const GsShape,
            ntools: usize,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_fillet(
            s: *const GsShape,
            edges: *const u32,
            nedges: usize,
            radius: f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_chamfer(
            s: *const GsShape,
            edges: *const u32,
            nedges: usize,
            dist: f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_shell(
            s: *const GsShape,
            faces: *const u32,
            nfaces: usize,
            thickness: f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_offset_faces(
            s: *const GsShape,
            faces: *const u32,
            nfaces: usize,
            distance: f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_move_faces(
            s: *const GsShape,
            faces: *const u32,
            nfaces: usize,
            kinds: *const i32,
            params: *const f64,
            nsteps: usize,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_draft(
            s: *const GsShape,
            faces: *const u32,
            nfaces: usize,
            params: *const f64,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_transform(
            kind: i32,
            params: *const f64,
            s: *const GsShape,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_compound(
            shapes: *const *const GsShape,
            n: usize,
            out: *mut *mut GsShape,
            origins: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_topology(
            s: *const GsShape,
            faces: *mut GsBuf,
            edges: *mut GsBuf,
            edge_adj: *mut GsBuf,
            verts: *mut GsBuf,
            counts: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_mass(s: *const GsShape, mass: *mut GsBuf) -> i32;
        pub(crate) fn gs_bbox(s: *const GsShape, bbox: *mut GsBuf) -> i32;
        pub(crate) fn gs_check(s: *const GsShape) -> i32;
        pub(crate) fn gs_hlr(
            s: *const GsShape,
            view: *const f64,
            exact: i32,
            deflection: f64,
            curves: *mut GsBuf,
            points: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_tessellate(
            s: *const GsShape,
            lin_defl: f64,
            ang_defl: f64,
            positions: *mut GsBuf,
            normals: *mut GsBuf,
            indices: *mut GsBuf,
            face_ranges: *mut GsBuf,
            edge_offsets: *mut GsBuf,
            edge_points: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_step_write(
            shapes: *const *const GsShape,
            names: *const *const core::ffi::c_char,
            n: usize,
            out: *mut GsBuf,
        ) -> i32;
        pub(crate) fn gs_iges_write(shapes: *const *const GsShape, n: usize, out: *mut GsBuf) -> i32;
        pub(crate) fn gs_step_read(
            data: *const u8,
            len: usize,
            out_shapes: *mut *mut *mut GsShape,
            n: *mut usize,
        ) -> i32;
        pub(crate) fn gs_iges_read_file(
            path: *const core::ffi::c_char,
            out_shapes: *mut *mut *mut GsShape,
            n: *mut usize,
        ) -> i32;
        pub(crate) fn gs_brep_write(s: *const GsShape, out: *mut GsBuf) -> i32;
        pub(crate) fn gs_brep_read(data: *const u8, len: usize, out: *mut *mut GsShape) -> i32;
        pub(crate) fn gs_brep_text_read(data: *const u8, len: usize, out: *mut *mut GsShape) -> i32;
        pub(crate) fn gs_brep_text_write(s: *const GsShape, out: *mut GsBuf) -> i32;
    }
}
