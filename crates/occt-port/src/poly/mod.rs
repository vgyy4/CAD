//! `Poly` of OCCT 8.0.1: triangulations, polygons, `Poly_Connect`, `Poly_MergeNodesTool`, `Poly_MakeLoops` and
//! `Poly_CoherentTriangulation`, with the `Poly::` functions and the containers they need.
#![allow(clippy::float_cmp)]

mod coherent;
mod connect;
pub(crate) mod fmt;
mod functions;
mod makeloops;
mod merge;
mod packedmap;
mod polygon;
mod text;
mod triangle;
mod triangulation;

pub use coherent::{CoherentLink, CoherentNode, CoherentTriangle, CoherentTriangulation};
pub use connect::Connect;
pub use functions::{intersect, intersect_tri_line, point_on_triangle, polygon_properties};
pub use makeloops::{Link, MakeLoops2D, MakeLoops2DHelper, MakeLoops3D, MakeLoops3DHelper, MakeLoopsHelper};
pub use merge::MergeNodesTool;
pub use packedmap::{HeapOfInteger, PackedMapOfInteger};
pub use polygon::{Polygon2D, Polygon3D, PolygonOnTriangulation};
pub use text::{
    dump_polygon2d, dump_polygon3d, dump_triangulation, read_polygon2d, read_polygon3d, read_triangulation,
    write_polygon2d, write_polygon3d, write_triangulation,
};
pub use triangle::Triangle;
pub use triangulation::{Triangulation, catenate, compute_normals};
