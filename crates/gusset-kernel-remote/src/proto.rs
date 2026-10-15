//! Requests and replies of the kernel worker protocol. One request per frame, one reply
//! per request, in order. Shapes travel as ids (see [`ShapeRefs`]): the worker keeps the
//! shapes, the client holds handles.

use gusset_kernel::wire::{Reader, ShapeRefs, Wire, WireError, Writer};
use gusset_kernel::{
    BBox, BoolOp, DraftSpec, ExtrudeSpec, KernelError, LoftSpec, MassProps, Mesh, OpResult, Profile, RevolveSpec,
    Shape, SweepPath, SweepSpec, TessellationParams, Topology, Transform,
};

/// Protocol version, checked by `Ping`.
pub const VERSION: u32 = 4;

/// A fault the worker injects on request (tests only, ADR 0022 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// `std::process::abort`.
    Abort,
    /// Unbounded recursion: a stack overflow.
    StackOverflow,
    /// Allocate until the allocator fails.
    ExhaustMemory,
    /// Sleep for this many milliseconds, then answer.
    Sleep(u32),
    /// Exit with a non-zero status without answering.
    Exit,
}

#[derive(Debug)]
pub(crate) enum Request {
    Ping,
    /// The worker's own report: what of its sandbox was applied (review L5).
    Status,
    Free(Vec<u64>),
    Extrude(Profile, ExtrudeSpec),
    Revolve(Profile, RevolveSpec),
    Sweep(Profile, SweepPath, SweepSpec),
    Loft(Vec<Profile>, LoftSpec),
    Boolean(BoolOp, Shape, Shape),
    BooleanMany(BoolOp, Shape, Vec<Shape>),
    Fillet(Shape, Vec<u32>, f64),
    Chamfer(Shape, Vec<u32>, f64),
    Shell(Shape, Vec<u32>, f64),
    Draft(Shape, Vec<u32>, DraftSpec),
    Transform(Shape, Transform),
    Compound(Vec<Shape>),
    Topology(Shape),
    Mass(Shape),
    BBox(Shape),
    Tessellate(Shape, TessellationParams),
    ExportStep(Vec<(Shape, String)>),
    ImportStep(Vec<u8>),
    ExportIges(Vec<(Shape, String)>),
    ImportIges(Vec<u8>),
    WriteNative(Shape),
    ReadNative(Vec<u8>),
    Fault(Fault),
}

#[derive(Debug)]
pub(crate) enum Reply {
    Unit,
    Version(u32),
    Op(OpResult),
    Topology(Topology),
    Mass(MassProps),
    BBox(BBox),
    Mesh(Mesh),
    Bytes(Vec<u8>),
    Bodies(Vec<(Shape, String)>),
    Shape(Shape),
}

fn put_shape(w: &mut Writer, s: &dyn ShapeRefs, shape: &Shape) -> Result<(), WireError> {
    w.u64(s.id_of(shape)?);
    Ok(())
}

fn get_shape(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Shape, WireError> {
    s.shape(r.u64()?)
}

fn put_shapes(w: &mut Writer, s: &dyn ShapeRefs, shapes: &[Shape]) -> Result<(), WireError> {
    w.seq_len(shapes.len())?;
    shapes.iter().try_for_each(|x| put_shape(w, s, x))
}

fn get_shapes(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Vec<Shape>, WireError> {
    let n = r.seq_len(8)?;
    (0..n).map(|_| get_shape(r, s)).collect()
}

fn put_named(w: &mut Writer, s: &dyn ShapeRefs, bodies: &[(Shape, String)]) -> Result<(), WireError> {
    w.seq_len(bodies.len())?;
    for (shape, name) in bodies {
        put_shape(w, s, shape)?;
        w.str(name)?;
    }
    Ok(())
}

fn get_named(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Vec<(Shape, String)>, WireError> {
    let n = r.seq_len(12)?;
    (0..n).map(|_| Ok((get_shape(r, s)?, r.str()?.to_string()))).collect()
}

fn put_u32s(w: &mut Writer, v: &[u32]) -> Result<(), WireError> {
    w.seq_len(v.len())?;
    v.iter().for_each(|x| w.u32(*x));
    Ok(())
}

fn get_u32s(r: &mut Reader<'_>) -> Result<Vec<u32>, WireError> {
    let n = r.seq_len(4)?;
    (0..n).map(|_| r.u32()).collect()
}

impl Request {
    pub(crate) fn encode(&self, s: &dyn ShapeRefs) -> Result<Vec<u8>, WireError> {
        let mut w = Writer::new();
        match self {
            Request::Ping => w.u8(0),
            Request::Status => w.u8(24),
            Request::Draft(shape, faces, spec) => {
                w.u8(25);
                put_shape(&mut w, s, shape)?;
                put_u32s(&mut w, faces)?;
                spec.put(&mut w, s)?;
            }
            Request::Free(ids) => {
                w.u8(1);
                w.seq_len(ids.len())?;
                ids.iter().for_each(|i| w.u64(*i));
            }
            Request::Extrude(p, spec) => {
                w.u8(2);
                p.put(&mut w, s)?;
                spec.put(&mut w, s)?;
            }
            Request::Revolve(p, spec) => {
                w.u8(3);
                p.put(&mut w, s)?;
                spec.put(&mut w, s)?;
            }
            Request::Sweep(p, path, spec) => {
                w.u8(4);
                p.put(&mut w, s)?;
                path.put(&mut w, s)?;
                spec.put(&mut w, s)?;
            }
            Request::Loft(sections, spec) => {
                w.u8(5);
                sections.put(&mut w, s)?;
                spec.put(&mut w, s)?;
            }
            Request::Boolean(op, a, b) => {
                w.u8(6);
                op.put(&mut w, s)?;
                put_shape(&mut w, s, a)?;
                put_shape(&mut w, s, b)?;
            }
            Request::BooleanMany(op, a, tools) => {
                w.u8(7);
                op.put(&mut w, s)?;
                put_shape(&mut w, s, a)?;
                put_shapes(&mut w, s, tools)?;
            }
            Request::Fillet(shape, edges, v) | Request::Chamfer(shape, edges, v) | Request::Shell(shape, edges, v) => {
                w.u8(match self {
                    Request::Fillet(..) => 8,
                    Request::Chamfer(..) => 9,
                    _ => 10,
                });
                put_shape(&mut w, s, shape)?;
                put_u32s(&mut w, edges)?;
                w.f64(*v);
            }
            Request::Transform(shape, t) => {
                w.u8(11);
                put_shape(&mut w, s, shape)?;
                t.put(&mut w, s)?;
            }
            Request::Compound(shapes) => {
                w.u8(12);
                put_shapes(&mut w, s, shapes)?;
            }
            Request::Topology(x) | Request::Mass(x) | Request::BBox(x) | Request::WriteNative(x) => {
                w.u8(match self {
                    Request::Topology(_) => 13,
                    Request::Mass(_) => 14,
                    Request::BBox(_) => 15,
                    _ => 16,
                });
                put_shape(&mut w, s, x)?;
            }
            Request::Tessellate(x, params) => {
                w.u8(17);
                put_shape(&mut w, s, x)?;
                params.put(&mut w, s)?;
            }
            Request::ExportStep(bodies) | Request::ExportIges(bodies) => {
                w.u8(if matches!(self, Request::ExportStep(_)) { 18 } else { 19 });
                put_named(&mut w, s, bodies)?;
            }
            Request::ImportStep(data) | Request::ImportIges(data) | Request::ReadNative(data) => {
                w.u8(match self {
                    Request::ImportStep(_) => 20,
                    Request::ImportIges(_) => 21,
                    _ => 22,
                });
                w.bytes(data)?;
            }
            Request::Fault(f) => {
                w.u8(23);
                match f {
                    Fault::Abort => w.u8(0),
                    Fault::StackOverflow => w.u8(1),
                    Fault::ExhaustMemory => w.u8(2),
                    Fault::Sleep(ms) => {
                        w.u8(3);
                        w.u32(*ms);
                    }
                    Fault::Exit => w.u8(4),
                }
            }
        }
        Ok(w.into_bytes())
    }

    pub(crate) fn decode(b: &[u8], s: &dyn ShapeRefs) -> Result<Request, WireError> {
        let mut r = Reader::new(b);
        let req = match r.u8()? {
            0 => Request::Ping,
            1 => {
                let n = r.seq_len(8)?;
                Request::Free((0..n).map(|_| r.u64()).collect::<Result<_, _>>()?)
            }
            2 => Request::Extrude(Profile::get(&mut r, s)?, ExtrudeSpec::get(&mut r, s)?),
            3 => Request::Revolve(Profile::get(&mut r, s)?, RevolveSpec::get(&mut r, s)?),
            4 => Request::Sweep(Profile::get(&mut r, s)?, SweepPath::get(&mut r, s)?, SweepSpec::get(&mut r, s)?),
            5 => Request::Loft(Vec::<Profile>::get(&mut r, s)?, LoftSpec::get(&mut r, s)?),
            6 => Request::Boolean(BoolOp::get(&mut r, s)?, get_shape(&mut r, s)?, get_shape(&mut r, s)?),
            7 => Request::BooleanMany(BoolOp::get(&mut r, s)?, get_shape(&mut r, s)?, get_shapes(&mut r, s)?),
            t @ 8..=10 => {
                let (shape, list, v) = (get_shape(&mut r, s)?, get_u32s(&mut r)?, r.f64()?);
                match t {
                    8 => Request::Fillet(shape, list, v),
                    9 => Request::Chamfer(shape, list, v),
                    _ => Request::Shell(shape, list, v),
                }
            }
            11 => Request::Transform(get_shape(&mut r, s)?, Transform::get(&mut r, s)?),
            12 => Request::Compound(get_shapes(&mut r, s)?),
            13 => Request::Topology(get_shape(&mut r, s)?),
            14 => Request::Mass(get_shape(&mut r, s)?),
            15 => Request::BBox(get_shape(&mut r, s)?),
            16 => Request::WriteNative(get_shape(&mut r, s)?),
            17 => Request::Tessellate(get_shape(&mut r, s)?, TessellationParams::get(&mut r, s)?),
            18 => Request::ExportStep(get_named(&mut r, s)?),
            19 => Request::ExportIges(get_named(&mut r, s)?),
            20 => Request::ImportStep(r.bytes()?.to_vec()),
            21 => Request::ImportIges(r.bytes()?.to_vec()),
            22 => Request::ReadNative(r.bytes()?.to_vec()),
            23 => Request::Fault(match r.u8()? {
                0 => Fault::Abort,
                1 => Fault::StackOverflow,
                2 => Fault::ExhaustMemory,
                3 => Fault::Sleep(r.u32()?),
                4 => Fault::Exit,
                t => return Err(WireError(format!("unknown fault {t}"))),
            }),
            24 => Request::Status,
            25 => Request::Draft(get_shape(&mut r, s)?, get_u32s(&mut r)?, DraftSpec::get(&mut r, s)?),
            t => return Err(WireError(format!("unknown request {t}"))),
        };
        r.finish()?;
        Ok(req)
    }
}

impl Reply {
    pub(crate) fn encode(result: &Result<Reply, KernelError>, s: &dyn ShapeRefs) -> Result<Vec<u8>, WireError> {
        let mut w = Writer::new();
        match result {
            Err(e) => {
                w.u8(1);
                e.put(&mut w, s)?;
            }
            Ok(reply) => {
                w.u8(0);
                match reply {
                    Reply::Unit => w.u8(0),
                    Reply::Version(v) => {
                        w.u8(1);
                        w.u32(*v);
                    }
                    Reply::Op(x) => {
                        w.u8(2);
                        x.put(&mut w, s)?;
                    }
                    Reply::Topology(x) => {
                        w.u8(3);
                        x.put(&mut w, s)?;
                    }
                    Reply::Mass(x) => {
                        w.u8(4);
                        x.put(&mut w, s)?;
                    }
                    Reply::BBox(x) => {
                        w.u8(5);
                        x.put(&mut w, s)?;
                    }
                    Reply::Mesh(x) => {
                        w.u8(6);
                        x.put(&mut w, s)?;
                    }
                    Reply::Bytes(x) => {
                        w.u8(7);
                        w.bytes(x)?;
                    }
                    Reply::Bodies(x) => {
                        w.u8(8);
                        put_named(&mut w, s, x)?;
                    }
                    Reply::Shape(x) => {
                        w.u8(9);
                        put_shape(&mut w, s, x)?;
                    }
                }
            }
        }
        Ok(w.into_bytes())
    }

    pub(crate) fn decode(b: &[u8], s: &dyn ShapeRefs) -> Result<Result<Reply, KernelError>, WireError> {
        let mut r = Reader::new(b);
        let out = match r.u8()? {
            1 => Err(KernelError::get(&mut r, s)?),
            0 => Ok(match r.u8()? {
                0 => Reply::Unit,
                1 => Reply::Version(r.u32()?),
                2 => Reply::Op(OpResult::get(&mut r, s)?),
                3 => Reply::Topology(Topology::get(&mut r, s)?),
                4 => Reply::Mass(MassProps::get(&mut r, s)?),
                5 => Reply::BBox(BBox::get(&mut r, s)?),
                6 => Reply::Mesh(Mesh::get(&mut r, s)?),
                7 => Reply::Bytes(r.bytes()?.to_vec()),
                8 => Reply::Bodies(get_named(&mut r, s)?),
                9 => Reply::Shape(get_shape(&mut r, s)?),
                t => return Err(WireError(format!("unknown reply {t}"))),
            }),
            t => return Err(WireError(format!("unknown status {t}"))),
        };
        r.finish()?;
        Ok(out)
    }
}
