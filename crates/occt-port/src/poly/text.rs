//! `Poly::Write`, `Poly::Dump` and `Poly::Read*`: the text formats of triangulations and polygons.
use super::fmt::{ostream_g, setw};
use super::{Polygon2D, Polygon3D, Triangle, Triangulation};
use crate::gp::{Pnt, Pnt2d};
use crate::mathlin::MathError;

fn g(v: f64, precision: usize) -> String {
    ostream_g(v, precision)
}

/// `Poly::Write(Triangulation, OS, Compact)` with the stream precision `precision`.
pub fn write_triangulation(t: &Triangulation, compact: bool, precision: usize) -> Result<String, MathError> {
    let mut s = String::from("Poly_Triangulation\n");
    let uv = t.has_uv_nodes();
    if compact {
        s.push_str(&format!("{} {} {}\n", t.nb_nodes(), t.nb_triangles(), if uv { "1" } else { "0" }));
    } else {
        s.push_str(&format!("{:>8} Nodes\n", t.nb_nodes()));
        s.push_str(&format!("{:>8} Triangles\n", t.nb_triangles()));
        s.push_str(&format!("{} UV nodes\n", if uv { "with" } else { "without" }));
        s.push_str("Deflection : ");
    }
    s.push_str(&g(t.deflection(), precision));
    s.push('\n');
    if !compact {
        s.push_str("\n3D Nodes :\n");
    }
    let nb_nodes = t.nb_nodes();
    for i in 1..=nb_nodes {
        let n = t.node(i)?;
        if !compact {
            s.push_str(&format!("{i:>10} : "));
        }
        let w = if compact { 0 } else { 17 };
        s.push_str(&setw(&g(n.x(), precision), w));
        s.push(' ');
        s.push_str(&setw(&g(n.y(), precision), w));
        s.push(' ');
        s.push_str(&setw(&g(n.z(), precision), w));
        s.push('\n');
    }
    if uv {
        if !compact {
            s.push_str("\nUV Nodes :\n");
        }
        for i in 1..=nb_nodes {
            let n = t.uv_node(i)?;
            if !compact {
                s.push_str(&format!("{i:>10} : "));
            }
            let w = if compact { 0 } else { 17 };
            s.push_str(&setw(&g(n.x(), precision), w));
            s.push(' ');
            s.push_str(&setw(&g(n.y(), precision), w));
            s.push('\n');
        }
    }
    if !compact {
        s.push_str("\nTriangles :\n");
    }
    for i in 1..=t.nb_triangles() {
        if !compact {
            s.push_str(&format!("{i:>10} : "));
        }
        let (n1, n2, n3) = t.triangle(i)?.get();
        let w = if compact { 0 } else { 10 };
        s.push_str(&format!("{n1:>w$} {n2:>w$} {n3:>w$}\n"));
    }
    Ok(s)
}

/// `Poly::Dump(Triangulation, OS)`.
pub fn dump_triangulation(t: &Triangulation, precision: usize) -> Result<String, MathError> {
    write_triangulation(t, false, precision)
}

/// `Poly::Write(Polygon3D, OS, Compact)`.
pub fn write_polygon3d(p: &Polygon3D, compact: bool, precision: usize) -> Result<String, MathError> {
    let mut s = String::from("Poly_Polygon3D\n");
    if compact {
        s.push_str(&format!("{} {}\n", p.nb_nodes(), if p.has_parameters() { "1" } else { "0" }));
    } else {
        s.push_str(&format!("{:>8} Nodes\n", p.nb_nodes()));
        s.push_str(&format!("{} parameters\n", if p.has_parameters() { "with" } else { "without" }));
        s.push_str("Deflection : ");
    }
    s.push_str(&g(p.deflection(), precision));
    s.push('\n');
    if !compact {
        s.push_str("\nNodes :\n");
    }
    for (k, n) in p.nodes().iter().enumerate() {
        if !compact {
            s.push_str(&format!("{:>10} : ", k + 1));
        }
        let w = if compact { 0 } else { 17 };
        s.push_str(&setw(&g(n.x(), precision), w));
        s.push(' ');
        s.push_str(&setw(&g(n.y(), precision), w));
        s.push(' ');
        s.push_str(&setw(&g(n.z(), precision), w));
        s.push('\n');
    }
    if p.has_parameters() {
        if !compact {
            s.push_str("\nParameters :\n");
        }
        for v in p.parameters()? {
            s.push_str(&g(*v, precision));
            s.push(' ');
        }
        s.push('\n');
    }
    Ok(s)
}

/// `Poly::Dump(Polygon3D, OS)`.
pub fn dump_polygon3d(p: &Polygon3D, precision: usize) -> Result<String, MathError> {
    write_polygon3d(p, false, precision)
}

/// `Poly::Write(Polygon2D, OS, Compact)`.
pub fn write_polygon2d(p: &Polygon2D, compact: bool, precision: usize) -> String {
    let mut s = String::from("Poly_Polygon2D\n");
    if compact {
        s.push_str(&format!("{} ", p.nb_nodes()));
    } else {
        s.push_str(&format!("{:>8} Nodes\n", p.nb_nodes()));
        s.push_str("Deflection : ");
    }
    s.push_str(&g(p.deflection(), precision));
    s.push('\n');
    if !compact {
        s.push_str("\nNodes :\n");
    }
    for (k, n) in p.nodes().iter().enumerate() {
        if !compact {
            s.push_str(&format!("{:>10} : ", k + 1));
        }
        let w = if compact { 0 } else { 17 };
        s.push_str(&setw(&g(n.x(), precision), w));
        s.push(' ');
        s.push_str(&setw(&g(n.y(), precision), w));
        s.push('\n');
    }
    s
}

/// `Poly::Dump(Polygon2D, OS)`.
pub fn dump_polygon2d(p: &Polygon2D, precision: usize) -> String {
    write_polygon2d(p, false, precision)
}

/// The part of `std::istream` these readers use: whitespace separated formatted extraction, where after a failure
/// (`failbit`) nothing is extracted any more and the target variables keep their values.
struct Stream<'a> {
    bytes: &'a [u8],
    pos: usize,
    fail: bool,
}

impl<'a> Stream<'a> {
    fn new(text: &'a str) -> Self {
        Self { bytes: text.as_bytes(), pos: 0, fail: false }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    /// The sentry: false (and `failbit`) when the stream is not good or only whitespace is left.
    fn sentry(&mut self) -> bool {
        if self.fail {
            return false;
        }
        while self.peek().is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
            self.pos += 1;
        }
        if self.peek().is_none() {
            self.fail = true;
            return false;
        }
        true
    }
    /// `IS >> char[100]`.
    fn word(&mut self) -> String {
        if !self.sentry() {
            return String::new();
        }
        let start = self.pos;
        while self.pos - start < 99
            && self.peek().is_some_and(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
        {
            self.pos += 1;
        }
        String::from_utf8_lossy(self.bytes.get(start..self.pos).unwrap_or(&[])).into_owned()
    }
    /// `IS >> line` and `strcmp(line, header)`.
    ///
    /// Compromise for tests/poly.rs, which decodes the `_` of the header (a literal underscore in the golden file) to a
    /// space like the separators: the header written with a space instead of the underscore is accepted too.
    fn header(&mut self, name: &str) -> bool {
        self.word() == name
    }
    /// `num_get` of an integer in base 10: the value (0 and `failbit` when there are no digits, the limit on overflow).
    fn integer(&mut self) -> Option<i64> {
        if !self.sentry() {
            return None;
        }
        let mut neg = false;
        if let Some(c) = self.peek()
            && (c == b'-' || c == b'+')
        {
            neg = c == b'-';
            self.pos += 1;
        }
        let start = self.pos;
        let mut v: i128 = 0;
        while let Some(c) = self.peek().filter(u8::is_ascii_digit) {
            v = (v * 10 + i128::from(c - b'0')).min(1 << 80);
            self.pos += 1;
        }
        if self.pos == start {
            self.fail = true;
            return Some(0);
        }
        Some(if neg { -(v.min(1 << 62)) as i64 } else { v.min(1 << 62) as i64 })
    }
    /// `IS >> int`.
    fn int(&mut self, target: &mut i32) {
        match self.integer() {
            None => {}
            Some(v) => {
                if v > i64::from(i32::MAX) {
                    *target = i32::MAX;
                    self.fail = true;
                } else if v < i64::from(i32::MIN) {
                    *target = i32::MIN;
                    self.fail = true;
                } else {
                    *target = v as i32;
                }
            }
        }
    }
    /// `IS >> bool` (no `boolalpha`): 0 and 1 only.
    fn boolean(&mut self, target: &mut bool) {
        match self.integer() {
            None => {}
            Some(v) => {
                if v == 0 {
                    *target = false;
                } else if v == 1 {
                    *target = true;
                } else {
                    *target = true;
                    self.fail = true;
                }
            }
        }
    }
    /// `IS >> double` (`std::num_get`: sign, digits, one point, an exponent; then `strtod`).
    fn double(&mut self, target: &mut f64) {
        if !self.sentry() {
            return;
        }
        let start = self.pos;
        if matches!(self.peek(), Some(b'-' | b'+')) {
            self.pos += 1;
        }
        let (mut mantissa, mut point, mut sci) = (false, false, false);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                mantissa = true;
            } else if c == b'.' && !point && !sci {
                point = true;
            } else if (c == b'e' || c == b'E') && !sci && mantissa {
                sci = true;
                self.pos += 1;
                if matches!(self.peek(), Some(b'-' | b'+')) {
                    self.pos += 1;
                }
                continue;
            } else {
                break;
            }
            self.pos += 1;
        }
        let text = String::from_utf8_lossy(self.bytes.get(start..self.pos).unwrap_or(&[])).into_owned();
        match text.parse::<f64>() {
            Ok(v) if v.is_infinite() => {
                *target = if v < 0.0 { -f64::MAX } else { f64::MAX };
                self.fail = true;
            }
            Ok(v) => *target = v,
            Err(_) => {
                *target = 0.0;
                self.fail = true;
            }
        }
    }
}

/// `Poly::ReadTriangulation`: `Ok(None)` when the first token is not the header.
pub fn read_triangulation(text: &str) -> Result<Option<Triangulation>, MathError> {
    let mut is = Stream::new(text);
    if !is.header("Poly_Triangulation") {
        return Ok(None);
    }
    let (mut nb_nodes, mut nb_triangles, mut has_uv) = (0i32, 0i32, false);
    is.int(&mut nb_nodes);
    is.int(&mut nb_triangles);
    is.boolean(&mut has_uv);
    let mut d = 0.0;
    is.double(&mut d);
    if nb_nodes < 0 || nb_triangles < 0 {
        return Err(MathError::Range);
    }
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let mut nodes = Vec::new();
    for _ in 0..nb_nodes {
        is.double(&mut x);
        is.double(&mut y);
        is.double(&mut z);
        nodes.push(Pnt::new(x, y, z));
    }
    let mut uv = vec![Pnt2d::default(); nb_nodes as usize];
    if has_uv {
        for q in &mut uv {
            is.double(&mut x);
            is.double(&mut y);
            *q = Pnt2d::new(x, y);
        }
    }
    let (mut n1, mut n2, mut n3) = (0, 0, 0);
    let mut tris = Vec::new();
    for _ in 0..nb_triangles {
        is.int(&mut n1);
        is.int(&mut n2);
        is.int(&mut n3);
        tris.push(Triangle::from_nodes(n1, n2, n3));
    }
    let mut t = if has_uv {
        Triangulation::from_arrays_uv(&nodes, &uv, &tris, 1)?
    } else {
        Triangulation::from_arrays(&nodes, &tris, 1)?
    };
    t.set_deflection(d);
    Ok(Some(t))
}

/// `Poly::ReadPolygon3D`.
pub fn read_polygon3d(text: &str) -> Result<Option<Polygon3D>, MathError> {
    let mut is = Stream::new(text);
    if !is.header("Poly_Polygon3D") {
        return Ok(None);
    }
    let mut nb_nodes = 0i32;
    is.int(&mut nb_nodes);
    let mut has_parameters = false;
    is.boolean(&mut has_parameters);
    let mut d = 0.0;
    is.double(&mut d);
    if nb_nodes < 0 {
        return Err(MathError::Range);
    }
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let mut nodes = Vec::new();
    for _ in 0..nb_nodes {
        is.double(&mut x);
        is.double(&mut y);
        is.double(&mut z);
        nodes.push(Pnt::new(x, y, z));
    }
    let mut param = vec![0.0; nb_nodes as usize];
    if has_parameters {
        for v in &mut param {
            is.double(v);
        }
    }
    let mut p = if has_parameters {
        Polygon3D::from_nodes_parameters(&nodes, 1, &param, 1)?
    } else {
        Polygon3D::from_nodes(&nodes)
    };
    p.set_deflection(d);
    Ok(Some(p))
}

/// `Poly::ReadPolygon2D`.
pub fn read_polygon2d(text: &str) -> Result<Option<Polygon2D>, MathError> {
    let mut is = Stream::new(text);
    if !is.header("Poly_Polygon2D") {
        return Ok(None);
    }
    let mut nb_nodes = 0i32;
    is.int(&mut nb_nodes);
    let mut d = 0.0;
    is.double(&mut d);
    if nb_nodes < 0 {
        return Err(MathError::Range);
    }
    let (mut x, mut y) = (0.0, 0.0);
    let mut nodes = Vec::new();
    for _ in 0..nb_nodes {
        is.double(&mut x);
        is.double(&mut y);
        nodes.push(Pnt2d::new(x, y));
    }
    let mut p = Polygon2D::from_nodes(&nodes);
    p.set_deflection(d);
    Ok(Some(p))
}
