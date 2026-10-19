/* Gusset ⇄ OCCT shim: a small C ABI over Open CASCADE Technology.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * Rules (ADR 0002):
 * - Only plain C types cross the boundary. Shapes are opaque handles freed with
 *   gs_shape_free; buffers are freed with gs_buf_free.
 * - Every function catches every C++ exception and returns GS_OK or GS_ERR. On GS_ERR the
 *   message is available from gs_last_error() (thread-local, valid until the next call
 *   on the same thread).
 * - Model-defining operations run single-threaded for determinism.
 */
#ifndef GUSSET_OCCT_H
#define GUSSET_OCCT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define GS_OK 0
#define GS_ERR 1

typedef struct GsShape GsShape;

/* A growable array allocated by the shim. `len` counts elements, not bytes. */
typedef struct {
  void* ptr;
  size_t len;
} GsBuf;

/* Profile segment kinds. */
#define GS_SEG_LINE 0
#define GS_SEG_ARC 1    /* center c, from a to b; ccw != 0 for counter-clockwise */
#define GS_SEG_CIRCLE 2 /* center c, radius r */
#define GS_SEG_BEZIER 3 /* cubic Bezier: the 4 points GsProfile.poles[pole_first ..] */

typedef struct {
  int32_t kind;
  int32_t ccw;
  double a[2];
  double b[2];
  double c[2];
  double r;
  uint64_t tag;
  uint32_t pole_first; /* Bezier only: index into GsProfile.poles (points, not doubles) */
  uint32_t pole_count;
} GsSeg;

/* A loop: segs[first .. first+count). `outer` != 0 for the outer loop of `region`. */
typedef struct {
  uint32_t first;
  uint32_t count;
  uint32_t region;
  uint32_t outer;
} GsLoop;

typedef struct {
  double origin[3];
  double x[3];
  double y[3];
  const GsSeg* segs;
  size_t nsegs;
  const GsLoop* loops;
  size_t nloops;
  const double* poles; /* x, y pairs of all Bezier segments */
  size_t npoles;       /* number of points */
} GsProfile;

/* Face-origin records: GsBuf of int64 quads (face, kind, a, b). */
#define GS_ORIGIN_LATERAL 0     /* a = tag */
#define GS_ORIGIN_CAP 1         /* a = 0 start / 1 end, b = region */
#define GS_ORIGIN_INPUT 2       /* a = input, b = face */
#define GS_ORIGIN_FROM_EDGE 3   /* a = input, b = edge */
#define GS_ORIGIN_FROM_VERTEX 4 /* a = input, b = vertex */
#define GS_ORIGIN_OFFSET 5      /* a = input, b = face: the inner offset of that face (shell) */
#define GS_ORIGIN_SWEPT 6       /* a = profile edge tag, b = path edge (0-based, from the path start) */
#define GS_ORIGIN_SECTION 7     /* a = profile edge tag, b = loft section */

/* Transform kinds for gs_transform; params: f64 x 7. */
#define GS_XF_MIRROR 0    /* params: plane origin xyz, plane normal xyz */
#define GS_XF_TRANSLATE 1 /* params: vector xyz */
#define GS_XF_ROTATE 2    /* params: axis origin xyz, axis direction xyz, angle (radians) */

#define GS_BOOL_UNION 0
#define GS_BOOL_CUT 1
#define GS_BOOL_INTERSECT 2

const char* gs_last_error(void);
const char* gs_occt_version(void);
void gs_shape_free(GsShape* s);
void gs_buf_free(GsBuf* b);

int gs_extrude(const GsProfile* p, double distance, int symmetric, GsShape** out, GsBuf* origins);
int gs_revolve(const GsProfile* p, const double axis_origin[2], const double axis_dir[2], double angle,
               GsShape** out, GsBuf* origins);
/* Sweeps the regions of `p` along a path: either sketch curves `path_curves` (one loop, an
 * ordered chain) or the edges `path_edges` (indices into `path_shape`'s edges). The path is
 * directed so that it starts on the profile plane. orientation: 0 = fixed (the profile only
 * slides along the path), 1 = follow the path (corrected Frenet). `twist` (radians) turns the
 * profile about the path between its start and end. */
int gs_sweep(const GsProfile* p, const GsProfile* path_curves, const GsShape* path_shape, const uint32_t* path_edges,
             size_t npath_edges, int orientation, double twist, GsShape** out, GsBuf* origins);
/* Lofts through `nsections` profiles (each one region without holes); ruled != 0 joins
 * neighbouring sections with ruled surfaces instead of a smooth interpolation. */
int gs_loft(const GsProfile* sections, size_t nsections, int ruled, GsShape** out, GsBuf* origins);
int gs_boolean(int op, const GsShape* target, const GsShape* tool, GsShape** out, GsBuf* origins);
/* Union or cut of `target` with all `tools` in one operation. INPUT origins: 0 = target,
 * i + 1 = tools[i]. */
int gs_boolean_many(int op, const GsShape* target, const GsShape* const* tools, size_t ntools, GsShape** out,
                    GsBuf* origins);
int gs_fillet(const GsShape* s, const uint32_t* edges, size_t nedges, double radius, GsShape** out, GsBuf* origins);
int gs_chamfer(const GsShape* s, const uint32_t* edges, size_t nedges, double dist, GsShape** out, GsBuf* origins);
/* Hollows `s` to a wall of `thickness` (> 0 inward, < 0 outward), removing the faces
 * `faces` (indices into the shape's face order). */
int gs_shell(const GsShape* s, const uint32_t* faces, size_t nfaces, double thickness, GsShape** out,
             GsBuf* origins);
/* Tapers the faces `faces` of `s` about their intersection with the neutral plane through
 * (params[3..6]) normal to the pull direction params[0..3], by params[6] radians (> 0: the
 * faces lean inward along the pull direction). Every face reports INPUT origins. */
int gs_draft(const GsShape* s, const uint32_t* faces, size_t nfaces, const double params[7], GsShape** out,
             GsBuf* origins);
/* Moves the faces `faces` of `s` along their normals by `distance` (> 0 outward); every
 * face reports INPUT origins. */
int gs_offset_faces(const GsShape* s, const uint32_t* faces, size_t nfaces, double distance, GsShape** out,
                    GsBuf* origins);
/* Moves the faces `faces` of `s` rigidly by `nsteps` translate/rotate steps applied in order
 * (kinds[i], params[7 * i ..]); the faces around keep their surfaces and stretch. Every face
 * reports INPUT origins. */
int gs_move_faces(const GsShape* s, const uint32_t* faces, size_t nfaces, const int* kinds, const double* params,
                  size_t nsteps, GsShape** out, GsBuf* origins);
/* Returns a transformed copy; output face i is input face i (INPUT origins, input 0). */
int gs_transform(int kind, const double params[7], const GsShape* s, GsShape** out, GsBuf* origins);
int gs_compound(const GsShape* const* shapes, size_t n, GsShape** out, GsBuf* origins);

/* Topology. faces: f64 x 15 per face (kind, area, cx, cy, cz, nx, ny, nz, then the axis of
 * a revolved face: has_axis, ox, oy, oz, dx, dy, dz);
 * edges: f64 x 13 per edge (kind, length, mx, my, mz, then a circle: has_circle, cx, cy, cz,
 * nx, ny, nz, radius);
 * edge_adj: i64 x 6 per edge (nfaces, f0, f1, nverts, v0, v1);
 * verts: f64 x 3 per vertex; counts: i64 x 2 (solids, shells). */
int gs_topology(const GsShape* s, GsBuf* faces, GsBuf* edges, GsBuf* edge_adj, GsBuf* verts, GsBuf* counts);

/* mass: f64 x 5 (volume, area, cx, cy, cz). bbox: f64 x 6 (min xyz, max xyz). */
int gs_mass(const GsShape* s, GsBuf* mass);
int gs_bbox(const GsShape* s, GsBuf* bbox);

/* Validates the shape; GS_ERR with a reason when invalid. */
int gs_check(const GsShape* s);

/* Hidden-line projection of `s` seen along -view[0..3] (view[0..3] points at the viewer),
 * view[3..6] the view's X axis on the sheet. exact != 0: HLRBRep_Algo on the exact
 * geometry; 0: HLRBRep_PolyAlgo on a mesh of `deflection`. curves: f64 x 10 per curve
 * (visible, class, type, a .. g): class 0 sharp, 1 smooth (tangent), 2 outline
 * (silhouette); type 0 line (x0, y0, x1, y1), 1 circle arc (cx, cy, r, a0, a1, ccw),
 * 2 polyline (first point, point count) into points (f64 x 2), in view coordinates. */
int gs_hlr(const GsShape* s, const double view[6], int exact, double deflection, GsBuf* curves, GsBuf* points);

/* Mesh: positions/normals f32 x 3, indices u32, face_ranges u32 x 2 (start, end triangle),
 * edge_offsets u32 (n+1 offsets into edge_points), edge_points f32 x 3. */
int gs_tessellate(const GsShape* s, double lin_defl, double ang_defl, GsBuf* positions, GsBuf* normals,
                  GsBuf* indices, GsBuf* face_ranges, GsBuf* edge_offsets, GsBuf* edge_points);

/* Exchange. Output bytes as GsBuf of uint8. names are UTF-8, one per shape. */
int gs_step_write(const GsShape* const* shapes, const char* const* names, size_t n, GsBuf* out);
int gs_iges_write(const GsShape* const* shapes, size_t n, GsBuf* out);
/* Reads every root shape; *out_shapes is a malloc'd array of `*n` shape pointers (free with
 * gs_shape_array_free, which also frees the shapes). */
int gs_step_read(const uint8_t* data, size_t len, GsShape*** out_shapes, size_t* n);
/* IGES is read from a file (the reader has no stream support); `path` is NUL-terminated. */
int gs_iges_read_file(const char* path, GsShape*** out_shapes, size_t* n);
void gs_shape_array_free(GsShape** shapes, size_t n);

/* Lossless native (BRep binary) serialization. */
int gs_brep_write(const GsShape* s, GsBuf* out);
int gs_brep_read(const uint8_t* data, size_t len, GsShape** out);

#ifdef __cplusplus
}
#endif

#endif /* GUSSET_OCCT_H */
