//! A compact, explicit binary codec for the kernel's data types (ADR 0022 §1).
//!
//! The kernel worker process and the app exchange profiles, specs, topology, meshes and
//! errors through this module. The format is little-endian, has no schema negotiation
//! (both sides are built from the same source) and encodes floats bit-exactly via
//! [`f64::to_bits`], so a value that crosses the process boundary is identical on both
//! sides (PROMPT.md "Determinism").
//!
//! Decoding is total: truncated input, an unknown enum tag, invalid UTF-8, a length that
//! exceeds the bytes left, a sequence longer than [`MAX_SEQ`] or trailing bytes are a
//! [`WireError`], never a panic and never an allocation larger than the input justifies.
//!
//! [`Shape`] handles are opaque and live in the worker, so they are not serialized: a
//! [`ShapeRefs`] maps them to ids and back.
//!
//! Enum tags are assigned in one place per enum (`wire_c_enum!` for fieldless enums, one
//! `match` in `put` and one in `get` for the others); a new variant is a new tag there.

use crate::types::*;
use crate::{Shape, SweepPath};
use std::fmt;
use std::ops::Range;

/// Longest sequence (elements) a message may carry.
pub const MAX_SEQ: usize = 50_000_000;

/// A malformed or unencodable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub String);

impl WireError {
    fn truncated(need: usize, have: usize) -> Self {
        WireError(format!("truncated message: needed {need} more bytes, {have} left"))
    }
    fn bad_tag(what: &str, tag: u8) -> Self {
        WireError(format!("invalid {what} tag {tag}"))
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WireError {}

impl From<WireError> for KernelError {
    fn from(e: WireError) -> Self {
        KernelError::new(KernelErrorKind::Internal, format!("kernel protocol: {}", e.0))
    }
}

/// Maps [`Shape`] handles to the ids used on the wire and back.
pub trait ShapeRefs {
    /// The id of `shape` (registering it where needed), or an error if it cannot be sent.
    fn id_of(&self, shape: &Shape) -> Result<u64, WireError>;
    /// The shape an id received from the other side stands for.
    fn shape(&self, id: u64) -> Result<Shape, WireError>;
}

/// For messages that carry no shapes: any shape or id is an error.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoShapes;

impl ShapeRefs for NoShapes {
    fn id_of(&self, _shape: &Shape) -> Result<u64, WireError> {
        Err(WireError("this message cannot carry a shape".into()))
    }
    fn shape(&self, id: u64) -> Result<Shape, WireError> {
        Err(WireError(format!("this message cannot carry a shape (id {id})")))
    }
}

/// Appends encoded values to a byte buffer.
#[derive(Debug, Clone, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer::default()
    }
    pub fn with_capacity(n: usize) -> Self {
        Writer { buf: Vec::with_capacity(n) }
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }

    /// A sequence length prefix (u32).
    pub fn seq_len(&mut self, n: usize) -> Result<(), WireError> {
        if n > MAX_SEQ {
            return Err(WireError(format!("sequence of {n} elements exceeds the limit of {MAX_SEQ}")));
        }
        let n = u32::try_from(n).map_err(|_| WireError(format!("sequence of {n} elements is too long")))?;
        self.u32(n);
        Ok(())
    }
    /// Raw bytes with a u32 length prefix.
    pub fn bytes(&mut self, v: &[u8]) -> Result<(), WireError> {
        let n = u32::try_from(v.len()).map_err(|_| WireError(format!("{} bytes are too long", v.len())))?;
        self.u32(n);
        self.buf.extend_from_slice(v);
        Ok(())
    }
    /// UTF-8 text with a u32 length prefix.
    pub fn str(&mut self, v: &str) -> Result<(), WireError> {
        self.bytes(v.as_bytes())
    }
    /// A length-prefixed sequence of `T`.
    pub fn seq<T: Wire>(&mut self, items: &[T], s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.seq_len(items.len())?;
        items.iter().try_for_each(|it| it.put(self, s))
    }
}

/// Reads encoded values from a byte slice.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf }
    }
    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }
    /// Errors if bytes remain: a message must be consumed exactly.
    pub fn finish(self) -> Result<(), WireError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(WireError(format!("{} trailing bytes after the message", self.buf.len())))
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        match self.buf.split_at_checked(n) {
            Some((head, tail)) => {
                self.buf = tail;
                Ok(head)
            }
            None => Err(WireError::truncated(n, self.buf.len())),
        }
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let head = self.take(N)?;
        <[u8; N]>::try_from(head).map_err(|_| WireError::truncated(N, head.len()))
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        let [b] = self.array::<1>()?;
        Ok(b)
    }
    pub fn bool(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            t => Err(WireError::bad_tag("bool", t)),
        }
    }
    pub fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub fn i64(&mut self) -> Result<i64, WireError> {
        Ok(i64::from_le_bytes(self.array()?))
    }
    pub fn f32(&mut self) -> Result<f32, WireError> {
        Ok(f32::from_bits(u32::from_le_bytes(self.array()?)))
    }
    pub fn f64(&mut self) -> Result<f64, WireError> {
        Ok(f64::from_bits(u64::from_le_bytes(self.array()?)))
    }

    /// A sequence length prefix for elements of at least `min_size` encoded bytes each.
    /// The count is checked against [`MAX_SEQ`] and against the bytes left, so a
    /// `Vec::with_capacity(count)` right after cannot be inflated by a hostile prefix.
    pub fn seq_len(&mut self, min_size: usize) -> Result<usize, WireError> {
        let n = usize::try_from(self.u32()?).map_err(|_| WireError("sequence length overflows".into()))?;
        if n > MAX_SEQ {
            return Err(WireError(format!("sequence of {n} elements exceeds the limit of {MAX_SEQ}")));
        }
        if n > self.buf.len() / min_size.max(1) {
            return Err(WireError(format!(
                "sequence of {n} elements does not fit in the {} bytes left",
                self.buf.len()
            )));
        }
        Ok(n)
    }
    /// Length-prefixed raw bytes.
    pub fn bytes(&mut self) -> Result<&'a [u8], WireError> {
        let n = usize::try_from(self.u32()?).map_err(|_| WireError("length overflows".into()))?;
        self.take(n)
    }
    /// Length-prefixed UTF-8 text.
    pub fn str(&mut self) -> Result<&'a str, WireError> {
        std::str::from_utf8(self.bytes()?).map_err(|e| WireError(format!("invalid UTF-8: {e}")))
    }
    /// A length-prefixed sequence of `T`.
    pub fn seq<T: Wire>(&mut self, s: &dyn ShapeRefs) -> Result<Vec<T>, WireError> {
        let n = self.seq_len(T::MIN_SIZE)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(T::get(self, s)?);
        }
        Ok(out)
    }
}

/// A type with a binary encoding.
pub trait Wire: Sized {
    /// A lower bound of the encoded size in bytes, used to bound sequence allocations.
    /// Never over-estimate.
    const MIN_SIZE: usize = 1;

    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError>;
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError>;
}

/// Encodes one value.
pub fn encode<T: Wire>(v: &T, s: &dyn ShapeRefs) -> Result<Vec<u8>, WireError> {
    let mut w = Writer::new();
    v.put(&mut w, s)?;
    Ok(w.into_bytes())
}

/// Decodes one value that must span all of `bytes`.
pub fn decode<T: Wire>(bytes: &[u8], s: &dyn ShapeRefs) -> Result<T, WireError> {
    let mut r = Reader::new(bytes);
    let v = T::get(&mut r, s)?;
    r.finish()?;
    Ok(v)
}

macro_rules! wire_prim {
    ($ty:ty, $size:expr, $method:ident) => {
        impl Wire for $ty {
            const MIN_SIZE: usize = $size;
            fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
                w.$method(*self);
                Ok(())
            }
            fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
                r.$method()
            }
        }
    };
}

wire_prim!(u8, 1, u8);
wire_prim!(bool, 1, bool);
wire_prim!(u32, 4, u32);
wire_prim!(u64, 8, u64);
wire_prim!(i64, 8, i64);
wire_prim!(f32, 4, f32);
wire_prim!(f64, 8, f64);

/// A fieldless enum: the variant list is the only place its tags are assigned.
macro_rules! wire_c_enum {
    ($ty:ident { $($variant:ident = $tag:literal),+ $(,)? }) => {
        impl Wire for $ty {
            fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
                w.u8(match self {
                    $($ty::$variant => $tag,)+
                });
                Ok(())
            }
            fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
                match r.u8()? {
                    $($tag => Ok($ty::$variant),)+
                    t => Err(WireError::bad_tag(stringify!($ty), t)),
                }
            }
        }
    };
}

impl Wire for String {
    const MIN_SIZE: usize = 4;
    fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.str(self)
    }
    fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(r.str()?.to_owned())
    }
}

impl<T: Wire> Wire for Vec<T> {
    const MIN_SIZE: usize = 4;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.seq(self, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        r.seq(s)
    }
}

impl<T: Wire> Wire for Option<T> {
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        match self {
            None => w.u8(0),
            Some(v) => {
                w.u8(1);
                v.put(w, s)?;
            }
        }
        Ok(())
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::get(r, s)?)),
            t => Err(WireError::bad_tag("Option", t)),
        }
    }
}

/// Fixed-size arrays carry no length prefix.
impl<T: Wire + Copy + Default, const N: usize> Wire for [T; N] {
    const MIN_SIZE: usize = T::MIN_SIZE * N;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.iter().try_for_each(|v| v.put(w, s))
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        let mut out = [T::default(); N];
        for slot in &mut out {
            *slot = T::get(r, s)?;
        }
        Ok(out)
    }
}

impl Wire for Range<u32> {
    const MIN_SIZE: usize = 8;
    fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.u32(self.start);
        w.u32(self.end);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
        let start = r.u32()?;
        Ok(start..r.u32()?)
    }
}

// Bulk helpers for the mesh arrays (hot: millions of elements).

fn put_u32s(w: &mut Writer, v: &[u32]) -> Result<(), WireError> {
    w.seq_len(v.len())?;
    w.buf.reserve(v.len() * 4);
    v.iter().for_each(|x| w.u32(*x));
    Ok(())
}

fn get_u32s(r: &mut Reader<'_>) -> Result<Vec<u32>, WireError> {
    let n = r.seq_len(4)?;
    let raw = r.take(n * 4)?;
    Ok(raw.chunks_exact(4).filter_map(|c| <[u8; 4]>::try_from(c).ok()).map(u32::from_le_bytes).collect())
}

fn put_v3f32s(w: &mut Writer, v: &[[f32; 3]]) -> Result<(), WireError> {
    w.seq_len(v.len())?;
    w.buf.reserve(v.len() * 12);
    v.iter().flatten().for_each(|x| w.f32(*x));
    Ok(())
}

fn get_v3f32s(r: &mut Reader<'_>) -> Result<Vec<[f32; 3]>, WireError> {
    let n = r.seq_len(12)?;
    let raw = r.take(n * 12)?;
    let floats: Vec<f32> = raw
        .chunks_exact(4)
        .filter_map(|c| <[u8; 4]>::try_from(c).ok())
        .map(|b| f32::from_bits(u32::from_le_bytes(b)))
        .collect();
    Ok(floats.chunks_exact(3).filter_map(|c| <[f32; 3]>::try_from(c).ok()).collect())
}

// ---- types.rs ----

impl Wire for Frame {
    const MIN_SIZE: usize = 72;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.origin.put(w, s)?;
        self.x.put(w, s)?;
        self.y.put(w, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(Frame { origin: Wire::get(r, s)?, x: Wire::get(r, s)?, y: Wire::get(r, s)? })
    }
}

impl Wire for Seg2 {
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        match self {
            Seg2::Line { a, b } => {
                w.u8(0);
                a.put(w, s)?;
                b.put(w, s)
            }
            Seg2::Arc { center, start, end, ccw } => {
                w.u8(1);
                center.put(w, s)?;
                start.put(w, s)?;
                end.put(w, s)?;
                w.bool(*ccw);
                Ok(())
            }
            Seg2::Circle { center, radius } => {
                w.u8(2);
                center.put(w, s)?;
                w.f64(*radius);
                Ok(())
            }
            Seg2::Bezier { p } => {
                w.u8(3);
                p.put(w, s)
            }
        }
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(Seg2::Line { a: Wire::get(r, s)?, b: Wire::get(r, s)? }),
            1 => Ok(Seg2::Arc {
                center: Wire::get(r, s)?,
                start: Wire::get(r, s)?,
                end: Wire::get(r, s)?,
                ccw: r.bool()?,
            }),
            2 => Ok(Seg2::Circle { center: Wire::get(r, s)?, radius: r.f64()? }),
            3 => Ok(Seg2::Bezier { p: Wire::get(r, s)? }),
            t => Err(WireError::bad_tag("Seg2", t)),
        }
    }
}

impl Wire for ProfileEdge {
    const MIN_SIZE: usize = Seg2::MIN_SIZE + 8;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.seg.put(w, s)?;
        w.u64(self.tag);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(ProfileEdge { seg: Wire::get(r, s)?, tag: r.u64()? })
    }
}

impl Wire for Loop2 {
    const MIN_SIZE: usize = 4;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.seq(&self.edges, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(Loop2 { edges: r.seq(s)? })
    }
}

impl Wire for Region2 {
    const MIN_SIZE: usize = 8;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.outer.put(w, s)?;
        w.seq(&self.holes, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(Region2 { outer: Wire::get(r, s)?, holes: r.seq(s)? })
    }
}

impl Wire for Profile {
    const MIN_SIZE: usize = Frame::MIN_SIZE + 4;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.frame.put(w, s)?;
        w.seq(&self.regions, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(Profile { frame: Wire::get(r, s)?, regions: r.seq(s)? })
    }
}

wire_c_enum!(BoolOp { Union = 0, Cut = 1, Intersect = 2 });

impl Wire for ExtrudeSpec {
    const MIN_SIZE: usize = 9;
    fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.f64(self.distance);
        w.bool(self.symmetric);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(ExtrudeSpec { distance: r.f64()?, symmetric: r.bool()? })
    }
}

impl Wire for RevolveSpec {
    const MIN_SIZE: usize = 40;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.axis_origin.put(w, s)?;
        self.axis_dir.put(w, s)?;
        w.f64(self.angle);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(RevolveSpec { axis_origin: Wire::get(r, s)?, axis_dir: Wire::get(r, s)?, angle: r.f64()? })
    }
}

wire_c_enum!(SweepOrientation { Fixed = 0, Frenet = 1 });

impl Wire for SweepSpec {
    const MIN_SIZE: usize = 9;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.orientation.put(w, s)?;
        w.f64(self.twist);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(SweepSpec { orientation: Wire::get(r, s)?, twist: r.f64()? })
    }
}

impl Wire for LoftSpec {
    fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.bool(self.ruled);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(LoftSpec { ruled: r.bool()? })
    }
}

impl Wire for Path2 {
    const MIN_SIZE: usize = Frame::MIN_SIZE + 4;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.frame.put(w, s)?;
        w.seq(&self.edges, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(Path2 { frame: Wire::get(r, s)?, edges: r.seq(s)? })
    }
}

wire_c_enum!(CapEnd { Start = 0, End = 1 });

impl Wire for FaceOrigin {
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        match self {
            FaceOrigin::Lateral { tag } => {
                w.u8(0);
                w.u64(*tag);
            }
            FaceOrigin::Cap { end, region } => {
                w.u8(1);
                end.put(w, s)?;
                w.u32(*region);
            }
            FaceOrigin::Input { input, face } => {
                w.u8(2);
                w.u32(*input);
                w.u32(*face);
            }
            FaceOrigin::FromEdge { input, edge } => {
                w.u8(3);
                w.u32(*input);
                w.u32(*edge);
            }
            FaceOrigin::FromVertex { input, vertex } => {
                w.u8(4);
                w.u32(*input);
                w.u32(*vertex);
            }
            FaceOrigin::Offset { input, face } => {
                w.u8(5);
                w.u32(*input);
                w.u32(*face);
            }
            FaceOrigin::Swept { tag, path } => {
                w.u8(6);
                w.u64(*tag);
                w.u32(*path);
            }
            FaceOrigin::Section { section, tag } => {
                w.u8(7);
                w.u32(*section);
                w.u64(*tag);
            }
        }
        Ok(())
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(FaceOrigin::Lateral { tag: r.u64()? }),
            1 => Ok(FaceOrigin::Cap { end: Wire::get(r, s)?, region: r.u32()? }),
            2 => Ok(FaceOrigin::Input { input: r.u32()?, face: r.u32()? }),
            3 => Ok(FaceOrigin::FromEdge { input: r.u32()?, edge: r.u32()? }),
            4 => Ok(FaceOrigin::FromVertex { input: r.u32()?, vertex: r.u32()? }),
            5 => Ok(FaceOrigin::Offset { input: r.u32()?, face: r.u32()? }),
            6 => Ok(FaceOrigin::Swept { tag: r.u64()?, path: r.u32()? }),
            7 => Ok(FaceOrigin::Section { section: r.u32()?, tag: r.u64()? }),
            t => Err(WireError::bad_tag("FaceOrigin", t)),
        }
    }
}

impl Wire for Transform {
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        match self {
            Transform::Mirror { origin, normal } => {
                w.u8(0);
                origin.put(w, s)?;
                normal.put(w, s)
            }
            Transform::Translate(v) => {
                w.u8(1);
                v.put(w, s)
            }
            Transform::Rotate { origin, axis, angle } => {
                w.u8(2);
                origin.put(w, s)?;
                axis.put(w, s)?;
                w.f64(*angle);
                Ok(())
            }
        }
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(Transform::Mirror { origin: Wire::get(r, s)?, normal: Wire::get(r, s)? }),
            1 => Ok(Transform::Translate(Wire::get(r, s)?)),
            2 => Ok(Transform::Rotate { origin: Wire::get(r, s)?, axis: Wire::get(r, s)?, angle: r.f64()? }),
            t => Err(WireError::bad_tag("Transform", t)),
        }
    }
}

wire_c_enum!(SurfaceKind { Plane = 0, Cylinder = 1, Cone = 2, Sphere = 3, Torus = 4, BSpline = 5, Other = 6 });
wire_c_enum!(CurveKind { Line = 0, Circle = 1, Ellipse = 2, BSpline = 3, Other = 4 });

impl Wire for FaceInfo {
    const MIN_SIZE: usize = 57;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.kind.put(w, s)?;
        w.f64(self.area);
        self.centroid.put(w, s)?;
        self.normal.put(w, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(FaceInfo { kind: Wire::get(r, s)?, area: r.f64()?, centroid: Wire::get(r, s)?, normal: Wire::get(r, s)? })
    }
}

impl Wire for EdgeInfo {
    const MIN_SIZE: usize = 41;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.kind.put(w, s)?;
        w.f64(self.length);
        self.midpoint.put(w, s)?;
        w.seq(&self.faces, s)?;
        w.seq(&self.vertices, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(EdgeInfo {
            kind: Wire::get(r, s)?,
            length: r.f64()?,
            midpoint: Wire::get(r, s)?,
            faces: r.seq(s)?,
            vertices: r.seq(s)?,
        })
    }
}

impl Wire for VertexInfo {
    const MIN_SIZE: usize = 24;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.point.put(w, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(VertexInfo { point: Wire::get(r, s)? })
    }
}

impl Wire for Topology {
    const MIN_SIZE: usize = 20;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.seq(&self.faces, s)?;
        w.seq(&self.edges, s)?;
        w.seq(&self.vertices, s)?;
        w.u32(self.solids);
        w.u32(self.shells);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(Topology { faces: r.seq(s)?, edges: r.seq(s)?, vertices: r.seq(s)?, solids: r.u32()?, shells: r.u32()? })
    }
}

/// The shape is sent as its id from [`ShapeRefs`].
impl Wire for OpResult {
    const MIN_SIZE: usize = 12;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.u64(s.id_of(&self.shape)?);
        w.seq(&self.face_origins, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        let shape = s.shape(r.u64()?)?;
        Ok(OpResult { shape, face_origins: r.seq(s)? })
    }
}

impl Wire for MassProps {
    const MIN_SIZE: usize = 40;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.f64(self.volume);
        w.f64(self.area);
        self.centroid.put(w, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(MassProps { volume: r.f64()?, area: r.f64()?, centroid: Wire::get(r, s)? })
    }
}

impl Wire for BBox {
    const MIN_SIZE: usize = 48;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.min.put(w, s)?;
        self.max.put(w, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(BBox { min: Wire::get(r, s)?, max: Wire::get(r, s)? })
    }
}

impl Wire for Mesh {
    const MIN_SIZE: usize = 20;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        put_v3f32s(w, &self.positions)?;
        put_v3f32s(w, &self.normals)?;
        put_u32s(w, &self.indices)?;
        w.seq(&self.face_ranges, s)?;
        w.seq_len(self.edges.len())?;
        self.edges.iter().try_for_each(|e| put_v3f32s(w, e))
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        let positions = get_v3f32s(r)?;
        let normals = get_v3f32s(r)?;
        let indices = get_u32s(r)?;
        let face_ranges = r.seq(s)?;
        let n = r.seq_len(4)?;
        let mut edges = Vec::with_capacity(n);
        for _ in 0..n {
            edges.push(get_v3f32s(r)?);
        }
        Ok(Mesh { positions, normals, indices, face_ranges, edges })
    }
}

impl Wire for TessellationParams {
    const MIN_SIZE: usize = 16;
    fn put(&self, w: &mut Writer, _s: &dyn ShapeRefs) -> Result<(), WireError> {
        w.f64(self.linear_deflection);
        w.f64(self.angular_deflection);
        Ok(())
    }
    fn get(r: &mut Reader<'_>, _s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(TessellationParams { linear_deflection: r.f64()?, angular_deflection: r.f64()? })
    }
}

wire_c_enum!(KernelErrorKind {
    InvalidInput = 0,
    OperationFailed = 1,
    InvalidResult = 2,
    Unsupported = 3,
    Cancelled = 4,
    Io = 5,
    Internal = 6,
});

impl Wire for KernelError {
    const MIN_SIZE: usize = 10;
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        self.kind.put(w, s)?;
        w.str(&self.message)?;
        self.hint.put(w, s)
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        Ok(KernelError { kind: Wire::get(r, s)?, message: Wire::get(r, s)?, hint: Wire::get(r, s)? })
    }
}

// ---- lib.rs ----

impl Wire for SweepPath {
    fn put(&self, w: &mut Writer, s: &dyn ShapeRefs) -> Result<(), WireError> {
        match self {
            SweepPath::Curves(p) => {
                w.u8(0);
                p.put(w, s)
            }
            SweepPath::Edges { shape, edges } => {
                w.u8(1);
                w.u64(s.id_of(shape)?);
                w.seq(edges, s)
            }
        }
    }
    fn get(r: &mut Reader<'_>, s: &dyn ShapeRefs) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(SweepPath::Curves(Wire::get(r, s)?)),
            1 => {
                let shape = s.shape(r.u64()?)?;
                Ok(SweepPath::Edges { shape, edges: r.seq(s)? })
            }
            t => Err(WireError::bad_tag("SweepPath", t)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kernel;
    use crate::mock::MockKernel;

    /// Maps the shapes it was given to ids (1-based position).
    struct Shapes(Vec<Shape>);

    impl ShapeRefs for Shapes {
        fn id_of(&self, shape: &Shape) -> Result<u64, WireError> {
            self.0
                .iter()
                .position(|x| x.ptr_eq(shape))
                .map(|i| i as u64 + 1)
                .ok_or_else(|| WireError("unknown shape".into()))
        }
        fn shape(&self, id: u64) -> Result<Shape, WireError> {
            usize::try_from(id)
                .ok()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| self.0.get(i))
                .cloned()
                .ok_or_else(|| WireError(format!("unknown shape id {id}")))
        }
    }

    fn rt<T: Wire + PartialEq + fmt::Debug>(v: &T) {
        let bytes = encode(v, &NoShapes).unwrap();
        let back: T = decode(&bytes, &NoShapes).unwrap();
        assert_eq!(&back, v);
        // Strict: no trailing byte accepted, no byte missing.
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode::<T>(&longer, &NoShapes).is_err());
        if let Some((_, short)) = bytes.split_last() {
            assert!(decode::<T>(short, &NoShapes).is_err());
        }
    }

    fn frame() -> Frame {
        Frame { origin: [1.5, -2.25, 1e300], x: [0.0, 1.0, 0.0], y: [-1.0, 0.0, 0.0] }
    }

    fn all_segs() -> Vec<Seg2> {
        vec![
            Seg2::Line { a: [0.0, -0.0], b: [f64::MAX, f64::MIN_POSITIVE] },
            Seg2::Arc { center: [1.0, 2.0], start: [3.0, 4.0], end: [5.0, 6.0], ccw: true },
            Seg2::Arc { center: [1.0, 2.0], start: [3.0, 4.0], end: [5.0, 6.0], ccw: false },
            Seg2::Circle { center: [0.5, 0.5], radius: f64::INFINITY },
            Seg2::Bezier { p: [[0.0, 0.0], [1.0, 2.0], [3.0, 4.0], [5.0, 6.0]] },
        ]
    }

    fn profile() -> Profile {
        let edges: Vec<ProfileEdge> =
            all_segs().into_iter().enumerate().map(|(i, seg)| ProfileEdge { seg, tag: u64::MAX - i as u64 }).collect();
        Profile {
            frame: frame(),
            regions: vec![
                Region2 {
                    outer: Loop2 { edges: edges.clone() },
                    holes: vec![Loop2 { edges: edges[..2].to_vec() }, Loop2::default()],
                },
                Region2::default(),
            ],
        }
    }

    #[test]
    fn primitives_and_containers() {
        rt(&0u8);
        rt(&u32::MAX);
        rt(&u64::MAX);
        rt(&i64::MIN);
        rt(&-1.5f32);
        rt(&true);
        rt(&false);
        rt(&String::new());
        rt(&"héllo wörld \u{1F600}".to_string());
        rt(&Vec::<u32>::new());
        rt(&vec![1u32, 2, 3]);
        rt(&None::<f64>);
        rt(&Some(2.5f64));
        rt(&Some(Some(1u32)));
        rt(&vec![Some("a".to_string()), None]);
        rt(&[1.0f64, 2.0, 3.0]);
        rt(&[[1.0f64, 2.0], [3.0, 4.0]]);
        rt(&(0..7u32));
    }

    #[test]
    fn floats_are_bit_exact() {
        for v in [
            f64::NAN,
            -0.0,
            0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::from_bits(1),
            f64::from_bits(0x7ff8_dead_beef_0001),
        ] {
            let bytes = encode(&v, &NoShapes).unwrap();
            let back: f64 = decode(&bytes, &NoShapes).unwrap();
            assert_eq!(back.to_bits(), v.to_bits());
        }
        let v = f32::from_bits(0x7fc0_1234);
        let back: f32 = decode(&encode(&v, &NoShapes).unwrap(), &NoShapes).unwrap();
        assert_eq!(back.to_bits(), v.to_bits());
        // The layout is fixed: little-endian IEEE bits.
        assert_eq!(encode(&1.0f64, &NoShapes).unwrap(), 1.0f64.to_bits().to_le_bytes());
    }

    #[test]
    fn profile_family() {
        rt(&frame());
        rt(&Frame::XY);
        for seg in all_segs() {
            rt(&seg);
            rt(&ProfileEdge { seg, tag: 7 });
        }
        rt(&Loop2::default());
        rt(&Region2::default());
        rt(&profile());
        rt(&Profile { frame: Frame::XZ, regions: vec![] });
        rt(&Path2 { frame: Frame::YZ, edges: vec![] });
        rt(&Path2 { frame: frame(), edges: profile().regions[0].outer.edges.clone() });
    }

    #[test]
    fn spec_family() {
        for op in [BoolOp::Union, BoolOp::Cut, BoolOp::Intersect] {
            rt(&op);
        }
        rt(&ExtrudeSpec { distance: -3.5, symmetric: true });
        rt(&ExtrudeSpec { distance: 10.0, symmetric: false });
        rt(&RevolveSpec { axis_origin: [1.0, 2.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU });
        for orientation in [SweepOrientation::Fixed, SweepOrientation::Frenet] {
            rt(&SweepSpec { orientation, twist: 0.75 });
        }
        rt(&LoftSpec { ruled: true });
        rt(&LoftSpec { ruled: false });
        rt(&TessellationParams::default());
        rt(&TessellationParams { linear_deflection: 1e-6, angular_deflection: 3.0 });
        for (origin, normal) in [([0.0; 3], [0.0, 0.0, 1.0]), ([1.0, 2.0, 3.0], [1.0, 1.0, 0.0])] {
            rt(&Transform::Mirror { origin, normal });
        }
        rt(&Transform::Translate([1.0, -2.0, 3.0]));
        rt(&Transform::Rotate { origin: [1.0, 0.0, 0.0], axis: [0.0, 0.0, 1.0], angle: 1.25 });
    }

    #[test]
    fn origin_and_topology_family() {
        let origins = vec![
            FaceOrigin::Lateral { tag: u64::MAX },
            FaceOrigin::Cap { end: CapEnd::Start, region: 3 },
            FaceOrigin::Cap { end: CapEnd::End, region: 0 },
            FaceOrigin::Input { input: 1, face: 2 },
            FaceOrigin::FromEdge { input: 3, edge: 4 },
            FaceOrigin::FromVertex { input: 5, vertex: 6 },
            FaceOrigin::Offset { input: 7, face: 8 },
            FaceOrigin::Swept { tag: 9, path: 10 },
            FaceOrigin::Section { section: 11, tag: 12 },
        ];
        for o in &origins {
            rt(o);
        }
        rt(&origins);
        rt(&vec![origins, vec![], vec![FaceOrigin::Lateral { tag: 1 }]]);
        rt(&CapEnd::Start);
        rt(&CapEnd::End);
        for kind in [
            SurfaceKind::Plane,
            SurfaceKind::Cylinder,
            SurfaceKind::Cone,
            SurfaceKind::Sphere,
            SurfaceKind::Torus,
            SurfaceKind::BSpline,
            SurfaceKind::Other,
        ] {
            rt(&kind);
            rt(&FaceInfo { kind, area: 12.5, centroid: [1.0, 2.0, 3.0], normal: [0.0, 0.0, -1.0] });
        }
        for kind in [CurveKind::Line, CurveKind::Circle, CurveKind::Ellipse, CurveKind::BSpline, CurveKind::Other] {
            rt(&kind);
            rt(&EdgeInfo { kind, length: 4.0, midpoint: [0.5; 3], faces: vec![0, 3], vertices: vec![1] });
        }
        rt(&EdgeInfo { kind: CurveKind::Line, length: 0.0, midpoint: [0.0; 3], faces: vec![], vertices: vec![] });
        rt(&VertexInfo { point: [9.0, 8.0, 7.0] });
        rt(&Topology::default());
        rt(&Topology {
            faces: vec![
                FaceInfo { kind: SurfaceKind::Plane, area: 1.0, centroid: [0.0; 3], normal: [0.0, 0.0, 1.0] };
                3
            ],
            edges: vec![EdgeInfo {
                kind: CurveKind::Line,
                length: 1.0,
                midpoint: [0.0; 3],
                faces: vec![0, 1],
                vertices: vec![0, 1],
            }],
            vertices: vec![VertexInfo { point: [1.0; 3] }, VertexInfo { point: [2.0; 3] }],
            solids: 1,
            shells: 2,
        });
        rt(&MassProps { volume: 8000.0, area: 2800.0, centroid: [20.0, 10.0, 5.0] });
        rt(&BBox { min: [-1.0, -2.0, -3.0], max: [1.0, 2.0, 3.0] });
    }

    #[test]
    fn mesh_family() {
        rt(&Mesh::default());
        rt(&mesh());
    }

    fn mesh() -> Mesh {
        Mesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, -1e-30], [f32::MAX, f32::MIN_POSITIVE, -0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            indices: vec![0, 1, 2, 2, 1, 3],
            face_ranges: vec![0..1, 1..2, 2..2],
            edges: vec![vec![[0.0; 3], [1.0, 0.0, 0.0]], vec![], vec![[1.0, 2.0, 3.0]; 5]],
        }
    }

    #[test]
    fn errors_round_trip() {
        for kind in [
            KernelErrorKind::InvalidInput,
            KernelErrorKind::OperationFailed,
            KernelErrorKind::InvalidResult,
            KernelErrorKind::Unsupported,
            KernelErrorKind::Cancelled,
            KernelErrorKind::Io,
            KernelErrorKind::Internal,
        ] {
            rt(&kind);
            rt(&KernelError::new(kind, "boom"));
            rt(&KernelError::new(kind, "fillet radius too large").with_hint("use a smaller radius \u{2014} 2 mm"));
        }
        let e: KernelError = WireError("bad".into()).into();
        assert_eq!(e.kind, KernelErrorKind::Internal);
        assert_eq!(e.message, "kernel protocol: bad");
    }

    fn mock_shapes() -> (Shapes, OpResult, OpResult) {
        let k = MockKernel;
        let pts = [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]];
        let edges = (0..4)
            .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 10 + i as u64 })
            .collect();
        let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
        let a = k.extrude(&p, &ExtrudeSpec { distance: 5.0, symmetric: false }).unwrap();
        let b = k.extrude(&p, &ExtrudeSpec { distance: 2.0, symmetric: true }).unwrap();
        (Shapes(vec![a.shape.clone(), b.shape.clone()]), a, b)
    }

    #[test]
    fn shapes_travel_as_ids() {
        let (refs, a, b) = mock_shapes();
        for r in [&a, &b] {
            let bytes = encode(r, &refs).unwrap();
            let back: OpResult = decode(&bytes, &refs).unwrap();
            assert!(back.shape.ptr_eq(&r.shape));
            assert_eq!(back.face_origins, r.face_origins);
        }
        // An unregistered shape cannot be sent, an unknown id cannot be received.
        let (_, other, _) = mock_shapes();
        assert!(encode(&other, &refs).is_err());
        assert!(encode(&a, &NoShapes).is_err());
        let bytes = encode(&a, &refs).unwrap();
        assert!(decode::<OpResult>(&bytes, &NoShapes).is_err());
        assert!(decode::<OpResult>(&bytes, &Shapes(vec![])).is_err());

        let path = SweepPath::Edges { shape: b.shape.clone(), edges: vec![3, 1, 2] };
        match decode::<SweepPath>(&encode(&path, &refs).unwrap(), &refs).unwrap() {
            SweepPath::Edges { shape, edges } => {
                assert!(shape.ptr_eq(&b.shape));
                assert_eq!(edges, vec![3, 1, 2]);
            }
            SweepPath::Curves(_) => panic!("wrong variant"),
        }
        let curves = SweepPath::Curves(Path2 { frame: frame(), edges: profile().regions[0].outer.edges.clone() });
        match decode::<SweepPath>(&encode(&curves, &NoShapes).unwrap(), &NoShapes).unwrap() {
            SweepPath::Curves(p) => assert_eq!(p.edges, profile().regions[0].outer.edges),
            SweepPath::Edges { .. } => panic!("wrong variant"),
        }
        assert!(encode(&path, &NoShapes).is_err());
    }

    #[test]
    fn every_truncation_is_an_error() {
        let (refs, a, _) = mock_shapes();
        let op = encode(&a, &refs).unwrap();
        for n in 0..op.len() {
            assert!(decode::<OpResult>(&op[..n], &refs).is_err(), "OpResult truncated to {n}");
        }
        let mesh_bytes = encode(&mesh(), &NoShapes).unwrap();
        for n in 0..mesh_bytes.len() {
            assert!(decode::<Mesh>(&mesh_bytes[..n], &NoShapes).is_err(), "Mesh truncated to {n}");
        }
        let prof = encode(&profile(), &NoShapes).unwrap();
        for n in 0..prof.len() {
            assert!(decode::<Profile>(&prof[..n], &NoShapes).is_err(), "Profile truncated to {n}");
        }
    }

    /// xorshift64*: deterministic, good enough to fuzz a decoder.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    #[test]
    fn random_bytes_never_panic() {
        let (refs, a, _) = mock_shapes();
        let seeds = [
            encode(&a, &refs).unwrap(),
            encode(&mesh(), &NoShapes).unwrap(),
            encode(&profile(), &NoShapes).unwrap(),
            encode(&KernelError::new(KernelErrorKind::Io, "x").with_hint("y"), &NoShapes).unwrap(),
        ];
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for case in 0..4000 {
            let bytes: Vec<u8> = if case % 2 == 0 {
                let n = (rng.next() % 200) as usize;
                (0..n).map(|_| rng.next() as u8).collect()
            } else {
                // A valid message with a few flipped bytes: reaches deeper decoder states.
                let mut b = seeds[case % seeds.len()].clone();
                for _ in 0..=(rng.next() % 4) {
                    let i = (rng.next() as usize) % b.len();
                    b[i] = rng.next() as u8;
                }
                b
            };
            // Any outcome but a panic is fine; arbitrary bytes are almost always rejected.
            let _ = decode::<OpResult>(&bytes, &refs);
            let _ = decode::<Mesh>(&bytes, &NoShapes);
            let _ = decode::<Profile>(&bytes, &NoShapes);
            let _ = decode::<Topology>(&bytes, &NoShapes);
            let _ = decode::<KernelError>(&bytes, &NoShapes);
            let _ = decode::<SweepPath>(&bytes, &refs);
            let _ = decode::<Vec<FaceOrigin>>(&bytes, &NoShapes);
            let _ = decode::<String>(&bytes, &NoShapes);
        }
        // Pure random strings are rejected as a whole message in practice.
        let mut rejected = 0;
        for _ in 0..2000 {
            let n = (rng.next() % 64) as usize;
            let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
            rejected += usize::from(decode::<Mesh>(&bytes, &NoShapes).is_err());
        }
        assert!(rejected > 1900);
    }

    #[test]
    fn hostile_lengths_do_not_allocate() {
        let start = std::time::Instant::now();
        let huge = u32::MAX.to_le_bytes();
        assert!(decode::<Vec<u8>>(&huge, &NoShapes).is_err());
        assert!(decode::<Vec<Profile>>(&huge, &NoShapes).is_err());
        assert!(decode::<String>(&huge, &NoShapes).is_err());
        assert!(decode::<Mesh>(&huge, &NoShapes).is_err());
        // A prefix just below the cap, with almost no bytes behind it.
        let mut cap = u32::try_from(MAX_SEQ).unwrap().to_le_bytes().to_vec();
        cap.extend_from_slice(&[0; 16]);
        assert!(decode::<Vec<f64>>(&cap, &NoShapes).is_err());
        // Over the cap.
        let mut over = u32::try_from(MAX_SEQ + 1).unwrap().to_le_bytes().to_vec();
        over.resize(4 + (MAX_SEQ + 1), 0);
        let err = decode::<Vec<u8>>(&over, &NoShapes).unwrap_err();
        assert!(err.0.contains("exceeds"), "{err}");
        // Nested: a valid outer length with a huge inner one.
        let mut nested = 1u32.to_le_bytes().to_vec();
        nested.extend_from_slice(&huge);
        assert!(decode::<Vec<Vec<u32>>>(&nested, &NoShapes).is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn bad_tags_and_utf8_are_errors() {
        assert!(decode::<bool>(&[2], &NoShapes).is_err());
        assert!(decode::<Option<u8>>(&[2], &NoShapes).is_err());
        assert!(decode::<BoolOp>(&[3], &NoShapes).is_err());
        assert!(decode::<Seg2>(&[4], &NoShapes).is_err());
        assert!(decode::<FaceOrigin>(&[8], &NoShapes).is_err());
        assert!(decode::<Transform>(&[3], &NoShapes).is_err());
        assert!(decode::<KernelErrorKind>(&[7], &NoShapes).is_err());
        assert!(decode::<SweepPath>(&[2], &NoShapes).is_err());
        assert!(decode::<String>(&[2, 0, 0, 0, 0xff, 0xfe], &NoShapes).is_err());
        assert!(decode::<u32>(&[], &NoShapes).is_err());
        assert!(Reader::new(&[1]).finish().is_err());
        assert!(Reader::new(&[]).finish().is_ok());
    }
}
