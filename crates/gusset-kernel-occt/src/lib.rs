//! The production [`Kernel`] implementation, backed by OCCT through `gusset-occt-sys`.
//!
//! Builds without OCCT too; [`OcctKernel::new`] then returns an `Unsupported` error so
//! callers can fall back (tests use the mock kernel).

#[cfg(gusset_occt)]
mod ffi;

use gusset_kernel::*;

/// OCCT-backed geometry kernel.
///
/// [`Kernel::read_native`] parses a B-rep blob in-process only if this process produced
/// it ([`Kernel::write_native`]) or the sandboxed worker already validated it
/// ([`gusset_worker::trust`]). Any other blob, such as one from a project file
/// someone sent, is first parsed and re-serialized by `gusset-worker`, and the app then
/// parses the worker's output (ADR 0008 §4, security review #3). Without a worker,
/// untrusted blobs are refused.
#[derive(Debug, Clone)]
pub struct OcctKernel {
    #[cfg_attr(not(gusset_occt), allow(dead_code))]
    worker: Option<std::path::PathBuf>,
}

/// Time allowed for validating one B-rep blob in the worker.
pub const VALIDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

impl OcctKernel {
    /// Returns the kernel, or an error if this build does not include OCCT. Untrusted
    /// B-rep blobs are validated by the `gusset-worker` installed next to the current
    /// executable.
    pub fn new() -> KResult<OcctKernel> {
        let worker = gusset_worker::default_worker_path().filter(|p| p.is_file());
        Self::with_worker(worker)
    }

    /// Like [`OcctKernel::new`], with an explicit worker (an absolute path), or `None`
    /// to refuse every B-rep blob that is not already trusted.
    pub fn with_worker(worker: Option<std::path::PathBuf>) -> KResult<OcctKernel> {
        if cfg!(gusset_occt) {
            Ok(OcctKernel { worker })
        } else {
            Err(KernelError::new(KernelErrorKind::Unsupported, "this build of Gusset does not include the OCCT kernel")
                .with_hint("build with GUSSET_OCCT_DIR pointing to an OCCT 8 installation"))
        }
    }

    /// The OCCT version string (empty without OCCT).
    pub fn version() -> String {
        #[cfg(gusset_occt)]
        {
            ffi::version()
        }
        #[cfg(not(gusset_occt))]
        {
            String::new()
        }
    }
}

#[cfg(gusset_occt)]
mod imp {
    use super::ffi::{self, RawShape};
    use super::*;
    use gusset_occt_sys as sys;
    use std::any::Any;

    #[derive(Debug)]
    pub(crate) struct OcctShape(pub(crate) RawShape);

    impl ShapeData for OcctShape {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn kernel(&self) -> &'static str {
            "occt"
        }
    }

    fn raw(s: &Shape) -> KResult<&RawShape> {
        Ok(&s.downcast::<OcctShape>()?.0)
    }

    fn invalid(msg: impl Into<String>) -> KernelError {
        KernelError::new(KernelErrorKind::InvalidInput, msg)
    }

    pub(crate) fn profile_data(p: &Profile) -> KResult<ffi::ProfileData> {
        if !p.frame.is_valid() {
            return Err(invalid("profile frame is not orthonormal"));
        }
        let mut segs = Vec::new();
        let mut loops = Vec::new();
        let mut add_loop = |l: &Loop2, region: u32, outer: bool, segs: &mut Vec<sys::GsSeg>| -> KResult<()> {
            let first = u32::try_from(segs.len()).map_err(|_| invalid("profile is too large"))?;
            for e in &l.edges {
                let mut s = sys::GsSeg { tag: e.tag, ..Default::default() };
                match e.seg {
                    Seg2::Line { a, b } => {
                        s.kind = sys::GS_SEG_LINE;
                        s.a = a;
                        s.b = b;
                    }
                    Seg2::Arc { center, start, end, ccw } => {
                        s.kind = sys::GS_SEG_ARC;
                        s.c = center;
                        s.a = start;
                        s.b = end;
                        s.ccw = i32::from(ccw);
                    }
                    Seg2::Circle { center, radius } => {
                        s.kind = sys::GS_SEG_CIRCLE;
                        s.c = center;
                        s.r = radius;
                    }
                }
                let finite = [s.a, s.b, s.c].iter().flatten().chain([&s.r]).all(|v| v.is_finite());
                if !finite {
                    return Err(invalid("profile contains non-finite coordinates"));
                }
                segs.push(s);
            }
            let count = u32::try_from(l.edges.len()).map_err(|_| invalid("profile is too large"))?;
            if count == 0 {
                return Err(invalid("profile has an empty loop"));
            }
            loops.push(sys::GsLoop { first, count, region, outer: u32::from(outer) });
            Ok(())
        };
        for (ri, r) in p.regions.iter().enumerate() {
            let region = u32::try_from(ri).map_err(|_| invalid("too many regions"))?;
            add_loop(&r.outer, region, true, &mut segs)?;
            for h in &r.holes {
                add_loop(h, region, false, &mut segs)?;
            }
        }
        if loops.is_empty() {
            return Err(invalid("profile has no regions").with_hint("close the sketch profile"));
        }
        Ok(ffi::ProfileData { origin: p.frame.origin, x: p.frame.x, y: p.frame.y, segs, loops })
    }

    fn u32_of(v: i64) -> u32 {
        u32::try_from(v).unwrap_or(u32::MAX)
    }

    pub(crate) fn op_result(out: ffi::OpOut) -> KResult<OpResult> {
        let (shape, recs) = out;
        let topo = ffi::topology(&shape)?;
        let nfaces = topo.faces.len() / 8;
        let mut origins: Vec<Vec<FaceOrigin>> = vec![Vec::new(); nfaces];
        for q in recs.chunks_exact(4) {
            let &[face, kind, a, b] = q else { continue };
            let o = match kind {
                sys::GS_ORIGIN_LATERAL => FaceOrigin::Lateral { tag: a as u64 },
                sys::GS_ORIGIN_CAP => {
                    FaceOrigin::Cap { end: if a == 0 { CapEnd::Start } else { CapEnd::End }, region: u32_of(b) }
                }
                sys::GS_ORIGIN_INPUT => FaceOrigin::Input { input: u32_of(a), face: u32_of(b) },
                sys::GS_ORIGIN_FROM_EDGE => FaceOrigin::FromEdge { input: u32_of(a), edge: u32_of(b) },
                sys::GS_ORIGIN_FROM_VERTEX => FaceOrigin::FromVertex { input: u32_of(a), vertex: u32_of(b) },
                _ => continue,
            };
            if let Some(slot) = usize::try_from(face).ok().and_then(|f| origins.get_mut(f)) {
                slot.push(o);
            }
        }
        for o in &mut origins {
            o.sort();
            o.dedup();
        }
        Ok(OpResult { shape: Shape::new(OcctShape(shape)), face_origins: origins })
    }

    fn surface_kind(k: f64) -> SurfaceKind {
        match k as i64 {
            0 => SurfaceKind::Plane,
            1 => SurfaceKind::Cylinder,
            2 => SurfaceKind::Cone,
            3 => SurfaceKind::Sphere,
            4 => SurfaceKind::Torus,
            5 => SurfaceKind::BSpline,
            _ => SurfaceKind::Other,
        }
    }

    fn curve_kind(k: f64) -> CurveKind {
        match k as i64 {
            0 => CurveKind::Line,
            1 => CurveKind::Circle,
            2 => CurveKind::Ellipse,
            3 => CurveKind::BSpline,
            _ => CurveKind::Other,
        }
    }

    pub(crate) fn topology(s: &RawShape) -> KResult<Topology> {
        let t = ffi::topology(s)?;
        let mut out = Topology::default();
        for f in t.faces.chunks_exact(8) {
            let &[k, area, cx, cy, cz, nx, ny, nz] = f else { continue };
            out.faces.push(FaceInfo { kind: surface_kind(k), area, centroid: [cx, cy, cz], normal: [nx, ny, nz] });
        }
        for (e, a) in t.edges.chunks_exact(5).zip(t.adj.chunks_exact(6)) {
            let (&[k, length, mx, my, mz], &[nf, f0, f1, nv, v0, v1]) = (e, a) else { continue };
            let faces: Vec<u32> = [f0, f1].into_iter().take(nf.clamp(0, 2) as usize).map(u32_of).collect();
            let vertices: Vec<u32> =
                [v0, v1].into_iter().filter(|&x| x >= 0).take(nv.clamp(0, 2) as usize).map(u32_of).collect();
            out.edges.push(EdgeInfo { kind: curve_kind(k), length, midpoint: [mx, my, mz], faces, vertices });
        }
        for v in t.verts.chunks_exact(3) {
            if let &[x, y, z] = v {
                out.vertices.push(VertexInfo { point: [x, y, z] });
            }
        }
        out.solids = t.counts.first().map_or(0, |&c| u32_of(c));
        out.shells = t.counts.get(1).map_or(0, |&c| u32_of(c));
        Ok(out)
    }

    impl Kernel for OcctKernel {
        fn name(&self) -> &'static str {
            "occt"
        }

        fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult> {
            let p = profile_data(profile)?;
            op_result(ffi::extrude(&p, spec.distance, spec.symmetric)?)
        }

        fn revolve(&self, profile: &Profile, spec: &RevolveSpec) -> KResult<OpResult> {
            let p = profile_data(profile)?;
            op_result(ffi::revolve(&p, spec.axis_origin, spec.axis_dir, spec.angle)?)
        }

        fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
            let code = match op {
                BoolOp::Union => sys::GS_BOOL_UNION,
                BoolOp::Cut => sys::GS_BOOL_CUT,
                BoolOp::Intersect => sys::GS_BOOL_INTERSECT,
            };
            op_result(ffi::boolean(code, raw(target)?, raw(tool)?)?)
        }

        fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
            op_result(ffi::blend(raw(shape)?, edges, radius, true)?)
        }

        fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
            op_result(ffi::blend(raw(shape)?, edges, distance, false)?)
        }

        fn compound(&self, shapes: &[Shape]) -> KResult<OpResult> {
            let raws = shapes.iter().map(raw).collect::<KResult<Vec<_>>>()?;
            op_result(ffi::compound(&raws)?)
        }

        fn topology(&self, shape: &Shape) -> KResult<Topology> {
            topology(raw(shape)?)
        }

        fn mass_properties(&self, shape: &Shape) -> KResult<MassProps> {
            let m = ffi::mass(raw(shape)?)?;
            match m.as_slice() {
                [v, a, x, y, z] => Ok(MassProps { volume: *v, area: *a, centroid: [*x, *y, *z] }),
                _ => Err(KernelError::new(KernelErrorKind::Internal, "malformed mass properties")),
            }
        }

        fn bbox(&self, shape: &Shape) -> KResult<BBox> {
            let b = ffi::bbox(raw(shape)?)?;
            match b.as_slice() {
                [x0, y0, z0, x1, y1, z1] => Ok(BBox { min: [*x0, *y0, *z0], max: [*x1, *y1, *z1] }),
                _ => Err(KernelError::new(KernelErrorKind::Internal, "malformed bounding box")),
            }
        }

        fn tessellate(&self, shape: &Shape, params: &TessellationParams) -> KResult<Mesh> {
            let m = ffi::tessellate(raw(shape)?, params.linear_deflection, params.angular_deflection)?;
            let v3 = |v: &[f32]| {
                v.chunks_exact(3)
                    .filter_map(|c| if let &[x, y, z] = c { Some([x, y, z]) } else { None })
                    .collect::<Vec<_>>()
            };
            let face_ranges =
                m.face_ranges.chunks_exact(2).filter_map(|c| if let &[a, b] = c { Some(a..b) } else { None }).collect();
            let pts = v3(&m.edge_points);
            let edges = m
                .edge_offsets
                .windows(2)
                .filter_map(|w| {
                    if let &[a, b] = w {
                        Some(pts.get(a as usize..b as usize).unwrap_or_default().to_vec())
                    } else {
                        None
                    }
                })
                .collect();
            Ok(Mesh { positions: v3(&m.positions), normals: v3(&m.normals), indices: m.indices, face_ranges, edges })
        }

        fn export_step(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
            let raws = bodies.iter().map(|(s, _)| raw(s)).collect::<KResult<Vec<_>>>()?;
            let names: Vec<&str> = bodies.iter().map(|(_, n)| *n).collect();
            ffi::step_write(&raws, &names)
        }

        fn import_step(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
            Ok(ffi::read_shapes(data)?
                .into_iter()
                .enumerate()
                .map(|(i, s)| (Shape::new(OcctShape(s)), format!("Body {}", i + 1)))
                .collect())
        }

        fn export_iges(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
            let raws = bodies.iter().map(|(s, _)| raw(s)).collect::<KResult<Vec<_>>>()?;
            ffi::iges_write(&raws)
        }

        fn import_iges(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
            // OCCT's IGES reader needs a file: use a private temporary one (this in-process
            // path is for trusted bytes; untrusted files go through the sandboxed worker).
            let dir = gusset_worker::PrivateDir::new()
                .map_err(|e| KernelError::new(KernelErrorKind::Io, format!("no temporary directory ({e})")))?;
            let file = dir.write_file("input.igs", data).map_err(|e| {
                KernelError::new(KernelErrorKind::Io, format!("could not write a temporary file ({e})"))
            })?;
            Ok(ffi::read_iges_file(&file)?
                .into_iter()
                .enumerate()
                .map(|(i, s)| (Shape::new(OcctShape(s)), format!("Body {}", i + 1)))
                .collect())
        }

        fn write_native(&self, shape: &Shape) -> KResult<Vec<u8>> {
            let bytes = ffi::brep_write(raw(shape)?)?;
            gusset_worker::trust::trust(&bytes);
            Ok(bytes)
        }

        fn read_native(&self, data: &[u8]) -> KResult<Shape> {
            // Only writer-produced bytes are parsed in this process: our own output, or
            // the worker's canonical re-serialization of an untrusted blob. The
            // untrusted original itself never reaches BinTools::Read here.
            if gusset_worker::trust::is_trusted(data) {
                return Ok(Shape::new(OcctShape(ffi::brep_read(data)?)));
            }
            if let Some(c) = gusset_worker::trust::canonical_for(data) {
                return Ok(Shape::new(OcctShape(ffi::brep_read(&c)?)));
            }
            let Some(worker) = &self.worker else {
                return Err(KernelError::new(
                    KernelErrorKind::Unsupported,
                    "imported geometry in this project cannot be checked because the import worker is missing",
                )
                .with_hint("reinstall Gusset so that gusset-worker is next to the app"));
            };
            let canonical = gusset_worker::validate_native_out_of_process(
                worker,
                data,
                &gusset_sandbox::Limits::default(),
                VALIDATE_TIMEOUT,
            )
            .map_err(|e| {
                KernelError::new(KernelErrorKind::InvalidInput, format!("imported geometry is damaged or unsafe ({e})"))
                    .with_hint("re-import the original file")
            })?;
            let canonical = gusset_worker::trust::remember_canonical(data, canonical);
            Ok(Shape::new(OcctShape(ffi::brep_read(&canonical)?)))
        }
    }
}

#[cfg(not(gusset_occt))]
fn missing<T>() -> KResult<T> {
    Err(KernelError::new(KernelErrorKind::Unsupported, "this build of Gusset does not include the OCCT kernel"))
}

#[cfg(not(gusset_occt))]
impl Kernel for OcctKernel {
    fn name(&self) -> &'static str {
        "occt"
    }
    fn extrude(&self, _: &Profile, _: &ExtrudeSpec) -> KResult<OpResult> {
        missing()
    }
    fn compound(&self, _: &[Shape]) -> KResult<OpResult> {
        missing()
    }
    fn topology(&self, _: &Shape) -> KResult<Topology> {
        missing()
    }
    fn mass_properties(&self, _: &Shape) -> KResult<MassProps> {
        missing()
    }
    fn bbox(&self, _: &Shape) -> KResult<BBox> {
        missing()
    }
    fn tessellate(&self, _: &Shape, _: &TessellationParams) -> KResult<Mesh> {
        missing()
    }
}

#[cfg(all(test, gusset_occt))]
mod tests;
