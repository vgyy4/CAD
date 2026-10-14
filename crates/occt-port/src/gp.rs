//! The 3D part of OCCT's `gp` package that ElCLib needs (gp_XYZ, gp_Pnt, gp_Vec, gp_Dir,
//! gp_Ax1, gp_Ax2 and the conics). `gp::Resolution()` is `f64::MIN_POSITIVE`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XYZ {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
}
impl XYZ {
    // gp_XYZ::X
    pub fn x(&self) -> f64 {
        self.x
    }
    // gp_XYZ::Y
    pub fn y(&self) -> f64 {
        self.y
    }
    // gp_XYZ::Z
    pub fn z(&self) -> f64 {
        self.z
    }
    // gp_XYZ::Dot
    pub(crate) fn dot(self, b: Self) -> f64 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }
    // gp_XYZ::Crossed
    pub(crate) fn cross(self, b: Self) -> Self {
        Self { x: self.y * b.z - self.z * b.y, y: self.z * b.x - self.x * b.z, z: self.x * b.y - self.y * b.x }
    }
    // gp_XYZ::Added
    pub(crate) fn add(self, b: Self) -> Self {
        Self { x: self.x + b.x, y: self.y + b.y, z: self.z + b.z }
    }
    // gp_XYZ::Multiplied
    pub(crate) fn scale(self, a: f64) -> Self {
        Self { x: self.x * a, y: self.y * a, z: self.z * a }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pnt(pub(crate) XYZ);
impl Pnt {
    // gp_Pnt::gp_Pnt
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(XYZ { x, y, z })
    }
    // gp_Pnt::XYZ
    pub fn xyz(&self) -> &XYZ {
        &self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec(pub(crate) XYZ);
impl Vec {
    // gp_Vec::XYZ
    pub fn xyz(&self) -> &XYZ {
        &self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dir(pub(crate) XYZ);
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstructionError;
impl Dir {
    // gp_Dir::gp_Dir
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, ConstructionError> {
        let m = x * x + y * y + z * z;
        if m <= f64::MIN_POSITIVE * f64::MIN_POSITIVE {
            return Err(ConstructionError);
        }
        // As written in OCCT; 1.0 ± Resolution rounds to 1.0, so this means "exactly 1".
        #[allow(clippy::double_comparisons)]
        if m >= 1.0 - f64::MIN_POSITIVE && m <= 1.0 + f64::MIN_POSITIVE {
            return Ok(Self(XYZ { x, y, z }));
        }
        let d = m.sqrt();
        Ok(Self(XYZ { x: x / d, y: y / d, z: z / d }))
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax1 {
    pub(crate) loc: Pnt,
    pub(crate) dir: Dir,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax2 {
    pub(crate) loc: Pnt,
    pub(crate) dir: Dir,
    pub(crate) x: Dir,
    pub(crate) y: Dir,
}
impl Ax2 {
    // gp_Ax2::gp_Ax2(P, N, Vx)
    pub fn new(p: Pnt, n: Dir, v: &Dir) -> Result<Self, ConstructionError> {
        let (vx, vy, vz) = (v.0.x, v.0.y, v.0.z);
        let (nx, ny, nz) = (n.0.x, n.0.y, n.0.z);
        let cx = vy * nz - vz * ny;
        let cy = vz * nx - vx * nz;
        let cz = vx * ny - vy * nx;
        let a = XYZ { x: ny * cz - nz * cy, y: nz * cx - nx * cz, z: nx * cy - ny * cx };
        // gp_Dir::CrossCross normalizes like the gp_Dir constructor.
        let x = Dir::new(a.x, a.y, a.z)?;
        let y_raw = n.0.cross(x.0);
        let y = Dir::new(y_raw.x, y_raw.y, y_raw.z)?;
        Ok(Self { loc: p, dir: n, x, y })
    }
    pub fn axis(&self) -> Ax1 {
        Ax1 { loc: self.loc, dir: self.dir }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lin(pub(crate) Ax1);
impl Lin {
    // gp_Lin::gp_Lin(Ax1)
    pub fn from_axis(a: Ax1) -> Self {
        Self(a)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circ {
    pub(crate) a: Ax2,
    pub(crate) r: f64,
}
impl Circ {
    // gp_Circ::gp_Circ
    pub fn new(a: Ax2, r: f64) -> Result<Self, ConstructionError> {
        if r < 0.0 { Err(ConstructionError) } else { Ok(Self { a, r }) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elips {
    pub(crate) a: Ax2,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}
impl Elips {
    // gp_Elips::gp_Elips
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < minor { Err(ConstructionError) } else { Ok(Self { a, major, minor }) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hypr {
    pub(crate) a: Ax2,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}
impl Hypr {
    // gp_Hypr::gp_Hypr
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if major < 0.0 || minor < 0.0 { Err(ConstructionError) } else { Ok(Self { a, major, minor }) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parab {
    pub(crate) a: Ax2,
    pub(crate) f: f64,
}
impl Parab {
    // gp_Parab::gp_Parab
    pub fn new(a: Ax2, f: f64) -> Result<Self, ConstructionError> {
        if f < 0.0 { Err(ConstructionError) } else { Ok(Self { a, f }) }
    }
}
pub(crate) fn xyz(x: f64, y: f64, z: f64) -> XYZ {
    XYZ { x, y, z }
}
pub(crate) fn point(x: XYZ) -> Pnt {
    Pnt(x)
}
pub(crate) fn vector(x: XYZ) -> Vec {
    Vec(x)
}
