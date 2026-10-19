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
    /// Parse every blob: the caller vetted it with [`vet_native`] (the kernel worker).
    #[cfg_attr(not(gusset_occt), allow(dead_code))]
    inputs_vetted: bool,
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
            Ok(OcctKernel { worker, inputs_vetted: false })
        } else {
            Err(KernelError::new(KernelErrorKind::Unsupported, "this build of Gusset does not include the OCCT kernel")
                .with_hint("build with GUSSET_OCCT_DIR pointing to an OCCT 8 installation"))
        }
    }

    /// A kernel that parses every B-rep blob it is given, for the kernel worker process
    /// (ADR 0022): the app vets each blob with [`vet_native`] before sending it, and the
    /// worker's sandbox contains whatever the parse does.
    pub fn with_vetted_inputs() -> KResult<OcctKernel> {
        Self::with_worker(None).map(|k| OcctKernel { inputs_vetted: true, ..k })
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

    /// A shape and its derived summaries, each computed at most once (shapes are
    /// immutable). Every operation needs the face count and the evaluator then asks for the
    /// same topology to name the result; edits report the volume of the bodies before and
    /// after. On a gear each of these took 0.2-1.7 s and was repeated (dogfooding F3-5).
    #[derive(Debug)]
    pub(crate) struct OcctShape(pub(crate) RawShape, Derived);

    #[derive(Debug, Default)]
    pub(crate) struct Derived {
        topology: std::sync::OnceLock<Topology>,
        mass: std::sync::OnceLock<MassProps>,
        bbox: std::sync::OnceLock<BBox>,
    }

    fn once<T: Clone>(cell: &std::sync::OnceLock<T>, f: impl FnOnce() -> KResult<T>) -> KResult<T> {
        if let Some(v) = cell.get() {
            return Ok(v.clone());
        }
        let v = f()?;
        Ok(cell.get_or_init(|| v).clone())
    }

    impl OcctShape {
        pub(crate) fn new(raw: RawShape) -> OcctShape {
            OcctShape(raw, Derived::default())
        }

        fn topology(&self) -> KResult<Topology> {
            once(&self.1.topology, || topology(&self.0))
        }
    }

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

    /// The shim's transform kind and parameters.
    fn xf(t: &Transform) -> (i32, [f64; 7]) {
        match *t {
            Transform::Mirror { origin, normal } => {
                (sys::GS_XF_MIRROR, [origin[0], origin[1], origin[2], normal[0], normal[1], normal[2], 0.0])
            }
            Transform::Translate(v) => (sys::GS_XF_TRANSLATE, [v[0], v[1], v[2], 0.0, 0.0, 0.0, 0.0]),
            Transform::Rotate { origin, axis, angle } => {
                (sys::GS_XF_ROTATE, [origin[0], origin[1], origin[2], axis[0], axis[1], axis[2], angle])
            }
        }
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
        let mut poles: Vec<f64> = Vec::new();
        let mut add_loop =
            |l: &Loop2, region: u32, outer: bool, segs: &mut Vec<sys::GsSeg>, poles: &mut Vec<f64>| -> KResult<()> {
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
                        Seg2::Bezier { p: pts } => {
                            s.kind = sys::GS_SEG_BEZIER;
                            s.pole_first =
                                u32::try_from(poles.len() / 2).map_err(|_| invalid("profile is too large"))?;
                            s.pole_count = 4;
                            for q in pts {
                                if !(q[0].is_finite() && q[1].is_finite()) {
                                    return Err(invalid("profile contains non-finite coordinates"));
                                }
                                poles.extend_from_slice(&q);
                            }
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
            add_loop(&r.outer, region, true, &mut segs, &mut poles)?;
            for h in &r.holes {
                add_loop(h, region, false, &mut segs, &mut poles)?;
            }
        }
        if loops.is_empty() {
            return Err(invalid("profile has no regions").with_hint("close the sketch profile"));
        }
        Ok(ffi::ProfileData { origin: p.frame.origin, x: p.frame.x, y: p.frame.y, segs, loops, poles })
    }

    fn u32_of(v: i64) -> u32 {
        u32::try_from(v).unwrap_or(u32::MAX)
    }

    pub(crate) fn op_result(out: ffi::OpOut) -> KResult<OpResult> {
        let (shape, recs) = out;
        let shape = OcctShape::new(shape);
        let nfaces = shape.topology()?.faces.len();
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
                sys::GS_ORIGIN_OFFSET => FaceOrigin::Offset { input: u32_of(a), face: u32_of(b) },
                sys::GS_ORIGIN_SWEPT => FaceOrigin::Swept { tag: a as u64, path: u32_of(b) },
                sys::GS_ORIGIN_SECTION => FaceOrigin::Section { section: u32_of(b), tag: a as u64 },
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
        Ok(OpResult { shape: Shape::new(shape), face_origins: origins })
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
        for f in t.faces.chunks_exact(15) {
            let &[k, area, cx, cy, cz, nx, ny, nz, has, ox, oy, oz, dx, dy, dz] = f else { continue };
            let axis = (has != 0.0).then_some(Axis3 { origin: [ox, oy, oz], dir: [dx, dy, dz] });
            out.faces.push(FaceInfo {
                kind: surface_kind(k),
                area,
                centroid: [cx, cy, cz],
                normal: [nx, ny, nz],
                axis,
            });
        }
        for (e, a) in t.edges.chunks_exact(13).zip(t.adj.chunks_exact(6)) {
            let (&[k, length, mx, my, mz, has, cx, cy, cz, nx, ny, nz, radius], &[nf, f0, f1, nv, v0, v1]) = (e, a)
            else {
                continue;
            };
            let faces: Vec<u32> = [f0, f1].into_iter().take(nf.clamp(0, 2) as usize).map(u32_of).collect();
            let vertices: Vec<u32> =
                [v0, v1].into_iter().filter(|&x| x >= 0).take(nv.clamp(0, 2) as usize).map(u32_of).collect();
            let circle = (has != 0.0).then_some(Circle3 { center: [cx, cy, cz], normal: [nx, ny, nz], radius });
            out.edges.push(EdgeInfo { kind: curve_kind(k), length, midpoint: [mx, my, mz], faces, vertices, circle });
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

        fn sweep(&self, profile: &Profile, path: &SweepPath, spec: &SweepSpec) -> KResult<OpResult> {
            let p = profile_data(profile)?;
            if !spec.twist.is_finite() {
                return Err(invalid("sweep twist must be a finite angle"));
            }
            if spec.twist != 0.0 && spec.orientation == SweepOrientation::Fixed {
                return Err(invalid("a twist needs the profile to follow the path")
                    .with_hint("set orientation to frenet, or remove the twist"));
            }
            let follow = spec.orientation == SweepOrientation::Frenet;
            let out = match path {
                SweepPath::Curves(c) => {
                    if c.edges.is_empty() {
                        return Err(invalid("the sweep path has no curves").with_hint("draw the path in a sketch"));
                    }
                    let region = Region2 { outer: Loop2 { edges: c.edges.clone() }, holes: Vec::new() };
                    let pd = profile_data(&Profile { frame: c.frame, regions: vec![region] })?;
                    ffi::sweep(&p, &sys::safe::SweepPath::Curves(&pd), follow, spec.twist)?
                }
                SweepPath::Edges { shape, edges } => {
                    if edges.is_empty() {
                        return Err(invalid("the sweep path has no edges").with_hint("select the edges to sweep along"));
                    }
                    ffi::sweep(&p, &sys::safe::SweepPath::Edges(raw(shape)?, edges), follow, spec.twist)?
                }
            };
            op_result(out)
        }

        fn loft(&self, sections: &[Profile], spec: &LoftSpec) -> KResult<OpResult> {
            if sections.len() < 2 {
                return Err(
                    invalid("a loft needs at least two profiles").with_hint("add a second profile on another plane")
                );
            }
            let mut data = Vec::with_capacity(sections.len());
            for s in sections {
                if s.regions.len() != 1 || s.regions.iter().any(|r| !r.holes.is_empty()) {
                    return Err(invalid("each loft profile must be a single outline without holes")
                        .with_hint("pick one region per profile"));
                }
                data.push(profile_data(s)?);
            }
            op_result(ffi::loft(&data, spec.ruled)?)
        }

        fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
            let code = match op {
                BoolOp::Union => sys::GS_BOOL_UNION,
                BoolOp::Cut => sys::GS_BOOL_CUT,
                BoolOp::Intersect => sys::GS_BOOL_INTERSECT,
            };
            op_result(ffi::boolean(code, raw(target)?, raw(tool)?)?)
        }

        fn boolean_many(&self, op: BoolOp, target: &Shape, tools: &[Shape]) -> KResult<OpResult> {
            let code = match op {
                BoolOp::Union => sys::GS_BOOL_UNION,
                BoolOp::Cut => sys::GS_BOOL_CUT,
                // OCCT's common with a tool list keeps what is inside *any* tool; the trait
                // means every tool, so intersect one tool at a time.
                BoolOp::Intersect => return gusset_kernel::sequential_boolean(self, op, target, tools),
            };
            if tools.is_empty() {
                return gusset_kernel::sequential_boolean(self, op, target, tools);
            }
            let raws = tools.iter().map(raw).collect::<KResult<Vec<_>>>()?;
            op_result(ffi::boolean_many(code, raw(target)?, &raws)?)
        }

        fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
            op_result(ffi::blend(raw(shape)?, edges, radius, true)?)
        }

        fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
            op_result(ffi::blend(raw(shape)?, edges, distance, false)?)
        }

        fn shell(&self, shape: &Shape, remove: &[u32], thickness: f64) -> KResult<OpResult> {
            op_result(ffi::shell(raw(shape)?, remove, thickness)?)
        }

        fn offset_faces(&self, shape: &Shape, faces: &[u32], distance: f64) -> KResult<OpResult> {
            op_result(ffi::offset_faces(raw(shape)?, faces, distance)?)
        }

        fn draft(&self, shape: &Shape, faces: &[u32], spec: &DraftSpec) -> KResult<OpResult> {
            let (d, n) = (spec.pull, spec.neutral);
            op_result(ffi::draft(raw(shape)?, faces, &[d[0], d[1], d[2], n[0], n[1], n[2], spec.angle])?)
        }

        fn move_faces(&self, shape: &Shape, faces: &[u32], steps: &[Transform]) -> KResult<OpResult> {
            let steps: Vec<(i32, [f64; 7])> = steps.iter().map(xf).collect();
            op_result(ffi::move_faces(raw(shape)?, faces, &steps)?)
        }

        fn hidden_lines(&self, shape: &Shape, view: &HlrView) -> KResult<Vec<HlrCurve>> {
            let (t, x) = (view.toward, view.x);
            let (curves, points) =
                ffi::hlr(raw(shape)?, &[t[0], t[1], t[2], x[0], x[1], x[2]], view.exact, view.deflection)?;
            let class = |c: f64| match c as i32 {
                1 => HlrClass::Smooth,
                2 => HlrClass::Outline,
                _ => HlrClass::Sharp,
            };
            let mut out = Vec::with_capacity(curves.len() / 10);
            for c in curves.chunks_exact(10) {
                let &[vis, cls, kind, a, b, cc, d, e, f, _] = c else { continue };
                let curve = match kind as i32 {
                    0 => ProjCurve::Line([a, b], [cc, d]),
                    1 => ProjCurve::Arc { center: [a, b], radius: cc, start: d, end: e, ccw: f != 0.0 },
                    _ => {
                        let (first, n) = (a as usize, b as usize);
                        let pts = points.get(2 * first..2 * (first + n)).unwrap_or(&[]);
                        ProjCurve::Polyline(
                            pts.chunks_exact(2)
                                .filter_map(|p| if let &[x, y] = p { Some([x, y]) } else { None })
                                .collect(),
                        )
                    }
                };
                out.push(HlrCurve { visible: vis != 0.0, class: class(cls), curve });
            }
            Ok(out)
        }

        fn transform(&self, shape: &Shape, t: &Transform) -> KResult<OpResult> {
            let (kind, p) = xf(t);
            op_result(ffi::transform(kind, &p, raw(shape)?)?)
        }

        fn compound(&self, shapes: &[Shape]) -> KResult<OpResult> {
            let raws = shapes.iter().map(raw).collect::<KResult<Vec<_>>>()?;
            op_result(ffi::compound(&raws)?)
        }

        fn topology(&self, shape: &Shape) -> KResult<Topology> {
            shape.downcast::<OcctShape>()?.topology()
        }

        fn mass_properties(&self, shape: &Shape) -> KResult<MassProps> {
            let s = shape.downcast::<OcctShape>()?;
            once(&s.1.mass, || {
                let m = ffi::mass(&s.0)?;
                match m.as_slice() {
                    [v, a, x, y, z] => Ok(MassProps { volume: *v, area: *a, centroid: [*x, *y, *z] }),
                    _ => Err(KernelError::new(KernelErrorKind::Internal, "malformed mass properties")),
                }
            })
        }

        fn bbox(&self, shape: &Shape) -> KResult<BBox> {
            let s = shape.downcast::<OcctShape>()?;
            once(&s.1.bbox, || {
                let b = ffi::bbox(&s.0)?;
                match b.as_slice() {
                    [x0, y0, z0, x1, y1, z1] => Ok(BBox { min: [*x0, *y0, *z0], max: [*x1, *y1, *z1] }),
                    _ => Err(KernelError::new(KernelErrorKind::Internal, "malformed bounding box")),
                }
            })
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
                .map(|(i, s)| (Shape::new(OcctShape::new(s)), format!("Body {}", i + 1)))
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
                .map(|(i, s)| (Shape::new(OcctShape::new(s)), format!("Body {}", i + 1)))
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
            if self.inputs_vetted {
                return Ok(Shape::new(OcctShape::new(ffi::brep_read(data)?)));
            }
            let vetted = vet_native(self.worker.as_deref(), data)?;
            Ok(Shape::new(OcctShape::new(ffi::brep_read(&vetted)?)))
        }
    }
}

/// The bytes of a B-rep blob that may be parsed in a trusted process: the blob itself if
/// this process wrote it or already had it validated ([`gusset_worker::trust`]), else the
/// canonical re-serialization made by the sandboxed `gusset-worker` at `worker`.
///
/// # Errors
/// The blob is untrusted and there is no worker, or the worker rejected it.
pub fn vet_native<'a>(worker: Option<&std::path::Path>, data: &'a [u8]) -> KResult<std::borrow::Cow<'a, [u8]>> {
    use std::borrow::Cow;
    if gusset_worker::trust::is_trusted(data) {
        return Ok(Cow::Borrowed(data));
    }
    if let Some(c) = gusset_worker::trust::canonical_for(data) {
        return Ok(Cow::Owned(c.as_ref().clone()));
    }
    let Some(worker) = worker else {
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
    Ok(Cow::Owned(gusset_worker::trust::remember_canonical(data, canonical).as_ref().clone()))
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
