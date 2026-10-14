//! Declarative helpers shared by the 3D curves and elementary surfaces (gp_Lin ... gp_Torus).
//! Each macro expands to the members that the C++ classes have in common; `$fld` names the field
//! that holds the frame (`0` for `Lin`, `a` for the conics, `pos` for the surfaces).

// Mirror, Mirrored, Rotate, Rotated, Translate(gp_Vec), Translated(gp_Vec), and the mirror trait
// (a point, an axis or a plane, as in the `PntMirror` pattern).
macro_rules! motion_common {
    ($t:ident, $trait:ident, $mname:ident, $fld:tt) => {
        /// Things this shape can be mirrored about: a point, an axis or a plane (an `Ax2`).
        pub trait $trait {
            /// Mirrors `s` about `self`.
            fn $mname(&self, s: &mut $t);
        }

        impl $trait for Pnt {
            fn $mname(&self, s: &mut $t) {
                s.$fld.mirror(self);
            }
        }

        impl $trait for Ax1 {
            fn $mname(&self, s: &mut $t) {
                s.$fld.mirror(self);
            }
        }

        impl $trait for Ax2 {
            fn $mname(&self, s: &mut $t) {
                s.$fld.mirror(self);
            }
        }

        impl $t {
            /// Mirror
            pub fn mirror<M: $trait>(&mut self, m: &M) {
                m.$mname(self);
            }
            /// Mirrored
            pub fn mirrored<M: $trait>(&self, m: &M) -> $t {
                let mut c = *self;
                m.$mname(&mut c);
                c
            }
            /// Rotate
            pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
                self.$fld.rotate(a1, ang);
            }
            /// Rotated
            pub fn rotated(&self, a1: &Ax1, ang: f64) -> $t {
                let mut c = *self;
                c.$fld.rotate(a1, ang);
                c
            }
            /// Translate(gp_Vec)
            pub fn translate(&mut self, v: &Vec) {
                self.$fld.translate(v);
            }
            /// Translated(gp_Vec)
            pub fn translated(&self, v: &Vec) -> $t {
                let mut c = *self;
                c.$fld.translate(v);
                c
            }
        }
    };
}

// Translate(P1, P2) and Translated(P1, P2) through the frame's own `translate_points`
// (every type but `Lin`, whose copy form goes through `gp_Vec(P1, P2)`).
macro_rules! translate_points_common {
    ($t:ident, $fld:tt) => {
        impl $t {
            /// Translate(P1, P2)
            pub fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
                self.$fld.translate_points(p1, p2);
            }
            /// Translated(P1, P2)
            pub fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> $t {
                let mut c = *self;
                c.$fld.translate_points(p1, p2);
                c
            }
        }
    };
}

// Location, Position, SetPosition.
macro_rules! frame_loc {
    ($t:ident, $fld:tt, $frame:ty) => {
        impl $t {
            /// Location
            pub fn location(&self) -> &Pnt {
                self.$fld.location()
            }
            /// Position
            pub fn position(&self) -> &$frame {
                &self.$fld
            }
            /// SetPosition
            pub fn set_position(&mut self, a: $frame) {
                self.$fld = a;
            }
        }
    };
}

// SetLocation (a plain move of the frame's location).
macro_rules! frame_set_location {
    ($t:ident, $fld:tt) => {
        impl $t {
            /// SetLocation
            pub fn set_location(&mut self, p: Pnt) {
                self.$fld.set_location(p);
            }
        }
    };
}

// XAxis and YAxis.
macro_rules! frame_xy {
    ($t:ident, $fld:tt) => {
        impl $t {
            /// XAxis
            pub fn x_axis(&self) -> Ax1 {
                Ax1::new(*self.$fld.location(), *self.$fld.x_direction())
            }
            /// YAxis
            pub fn y_axis(&self) -> Ax1 {
                Ax1::new(*self.$fld.location(), *self.$fld.y_direction())
            }
        }
    };
}

// Axis and SetAxis.
macro_rules! frame_axis {
    ($t:ident, $fld:tt) => {
        impl $t {
            /// Axis
            pub fn axis(&self) -> Ax1 {
                self.$fld.axis()
            }
            /// SetAxis
            pub fn set_axis(&mut self, a1: Ax1) -> Result<(), ConstructionError> {
                self.$fld.set_axis(a1)
            }
        }
    };
}

/// The radius update of `Scale` / `Scaled` / `Transform` / `Transformed`: `r *= s; if (r < 0) r = -r`.
pub(crate) fn scaled_radius(r: f64, s: f64) -> f64 {
    let r = r * s;
    if r < 0.0 { -r } else { r }
}
