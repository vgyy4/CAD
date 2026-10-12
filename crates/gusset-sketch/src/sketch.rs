//! Sketch entities, constraints and the [`Sketch`] container with its builder API.
//!
//! All lengths are millimetres and all angles radians. Entity and constraint ids
//! are allocated from monotonically increasing counters and are never reused within
//! a sketch, so they can serve as stable names for downstream features.

use std::fmt;

/// Most entities a sketch may hold (bounds memory and solve time on hostile input).
pub const MAX_ENTITIES: usize = 50_000;
/// Most constraints a sketch may hold.
pub const MAX_CONSTRAINTS: usize = 100_000;
/// Largest allowed absolute coordinate, radius or dimension value, in millimetres
/// (1000 km). Keeps squares of coordinates far away from overflow.
pub const MAX_COORD: f64 = 1.0e9;

/// Stable identifier of a sketch entity. Never reused within a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u32);

/// Stable identifier of a constraint. Never reused within a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(pub u32);

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e{}", self.0)
    }
}

impl fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// Geometry of one entity. Lines, circles and arcs refer to [`Geom::Point`] entities
/// for their defining points, so coincidence between shapes is expressed by sharing
/// points or by [`Constraint::Coincident`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geom {
    Point {
        p: [f64; 2],
    },
    /// Segment from point `a` to point `b`.
    Line {
        a: EntityId,
        b: EntityId,
    },
    /// Full circle; the radius is a solver unknown.
    Circle {
        center: EntityId,
        radius: f64,
    },
    /// Counter-clockwise arc from `start` to `end` around `center`. The radius is
    /// `|start - center|`; the solver keeps `|end - center|` equal to it.
    Arc {
        center: EntityId,
        start: EntityId,
        end: EntityId,
    },
    /// Interpolating spline through the fit points `count` entries of the sketch's spline
    /// point table starting at `first` (see [`Sketch::spline_points`]). The list lives in a
    /// side table so that `Geom` stays `Copy`. The spline adds no solver equations: its
    /// points are ordinary points.
    Spline {
        first: u32,
        count: u32,
        closed: bool,
    },
}

/// The kind of an entity, without its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityKind {
    Point,
    Line,
    Circle,
    Arc,
    Spline,
}

impl Geom {
    pub fn kind(&self) -> EntityKind {
        match self {
            Geom::Point { .. } => EntityKind::Point,
            Geom::Line { .. } => EntityKind::Line,
            Geom::Circle { .. } => EntityKind::Circle,
            Geom::Arc { .. } => EntityKind::Arc,
            Geom::Spline { .. } => EntityKind::Spline,
        }
    }

    /// The point entities this geometry refers to (empty for a point). A spline's fit
    /// points are in [`Sketch::spline_points`] instead.
    pub fn referenced_points(&self) -> Vec<EntityId> {
        match *self {
            Geom::Point { .. } => Vec::new(),
            Geom::Line { a, b } => vec![a, b],
            Geom::Circle { center, .. } => vec![center],
            Geom::Arc { center, start, end } => vec![center, start, end],
            Geom::Spline { .. } => Vec::new(),
        }
    }
}

/// One sketch entity. Construction entities take part in solving but are ignored
/// by [`crate::profiles`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub geom: Geom,
    pub construction: bool,
}

/// A geometric constraint. All lengths in mm, angles in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Constraint {
    /// Two points at the same location.
    Coincident(EntityId, EntityId),
    /// Point (first) lies on the infinite line through a line entity (second).
    PointOnLine(EntityId, EntityId),
    /// Point (first) lies on a circle or on the full circle of an arc (second).
    PointOnCircle(EntityId, EntityId),
    /// Point (first) is the midpoint of a line (second).
    Midpoint(EntityId, EntityId),
    /// Line is parallel to the x axis.
    Horizontal(EntityId),
    /// Line is parallel to the y axis.
    Vertical(EntityId),
    /// Two lines are parallel (same or opposite direction).
    Parallel(EntityId, EntityId),
    /// Two lines are perpendicular.
    Perpendicular(EntityId, EntityId),
    /// Line–circle/arc or circle/arc–circle/arc tangency. For two curves, internal
    /// or external tangency is chosen from the initial positions (internal when the
    /// centre distance is below the larger radius) and then kept. For a line, the
    /// side of the line the centre lies on is likewise kept.
    Tangent(EntityId, EntityId),
    /// Two lines of equal length, or two circles/arcs of equal radius.
    Equal(EntityId, EntityId),
    /// Distance:
    /// * point–point: Euclidean distance;
    /// * point–line (either order): perpendicular distance to the infinite line,
    ///   keeping the side the point starts on;
    /// * a line with itself (`a == b`): the line's length.
    Distance { a: EntityId, b: EntityId, value: f64 },
    /// Directed angle, counter-clockwise, from line `a` (direction `a.a → a.b`) to
    /// line `b` (direction `b.a → b.b`), in radians.
    Angle { a: EntityId, b: EntityId, value: f64 },
    /// Radius of a circle or arc.
    Radius { c: EntityId, value: f64 },
    /// Diameter of a circle or arc.
    Diameter { c: EntityId, value: f64 },
    /// Fixes every defining point (and a circle's radius) at its current value.
    Fixed(EntityId),
    /// Points `a` and `b` are mirror images across the line `axis`.
    Symmetric { a: EntityId, b: EntityId, axis: EntityId },
}

impl Constraint {
    /// Every entity this constraint refers to, in field order.
    pub fn entities(&self) -> Vec<EntityId> {
        match *self {
            Constraint::Coincident(a, b)
            | Constraint::PointOnLine(a, b)
            | Constraint::PointOnCircle(a, b)
            | Constraint::Midpoint(a, b)
            | Constraint::Parallel(a, b)
            | Constraint::Perpendicular(a, b)
            | Constraint::Tangent(a, b)
            | Constraint::Equal(a, b)
            | Constraint::Distance { a, b, .. }
            | Constraint::Angle { a, b, .. } => vec![a, b],
            Constraint::Horizontal(a)
            | Constraint::Vertical(a)
            | Constraint::Fixed(a)
            | Constraint::Radius { c: a, .. }
            | Constraint::Diameter { c: a, .. } => vec![a],
            Constraint::Symmetric { a, b, axis } => vec![a, b, axis],
        }
    }
}

/// Why a sketch edit or a sketch as a whole is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchError {
    /// An id that does not name an entity of this sketch.
    UnknownEntity(EntityId),
    /// An id that does not name a constraint of this sketch.
    UnknownConstraint(ConstraintId),
    /// An entity of the wrong kind was referenced.
    WrongKind { entity: EntityId, expected: &'static str },
    /// The same entity was used where two different ones are required, or a line/arc
    /// uses the same point twice.
    RepeatedEntity(EntityId),
    /// A coordinate or radius is NaN, infinite, or beyond [`MAX_COORD`].
    NonFinite(EntityId),
    /// A constraint value is NaN, infinite, out of range or has the wrong sign.
    InvalidValue { what: &'static str },
    /// The sketch already holds [`MAX_ENTITIES`] entities.
    TooManyEntities,
    /// The sketch already holds [`MAX_CONSTRAINTS`] constraints.
    TooManyConstraints,
}

impl fmt::Display for SketchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SketchError::UnknownEntity(e) => write!(f, "unknown entity {e}"),
            SketchError::UnknownConstraint(c) => write!(f, "unknown constraint {c}"),
            SketchError::WrongKind { entity, expected } => write!(f, "entity {entity} must be {expected}"),
            SketchError::RepeatedEntity(e) => write!(f, "entity {e} is used twice where two different ones are needed"),
            SketchError::NonFinite(e) => {
                write!(f, "entity {e} has a non-finite or out-of-range value (limit {MAX_COORD} mm)")
            }
            SketchError::InvalidValue { what } => write!(f, "invalid value: {what}"),
            SketchError::TooManyEntities => write!(f, "too many entities (limit {MAX_ENTITIES})"),
            SketchError::TooManyConstraints => write!(f, "too many constraints (limit {MAX_CONSTRAINTS})"),
        }
    }
}

impl std::error::Error for SketchError {}

fn coord_ok(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

/// A 2D sketch: entities and constraints, both kept sorted by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sketch {
    entities: Vec<Entity>,
    constraints: Vec<(ConstraintId, Constraint)>,
    next_entity: u32,
    next_constraint: u32,
    /// Fit points of all splines, appended when a spline is added (slots of removed splines
    /// stay behind, unused).
    spline_pts: Vec<EntityId>,
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    /// All entities, sorted by id.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// All constraints, sorted by id.
    pub fn constraints(&self) -> &[(ConstraintId, Constraint)] {
        &self.constraints
    }

    pub(crate) fn entity_index(&self, id: EntityId) -> Option<usize> {
        self.entities.binary_search_by_key(&id, |e| e.id).ok()
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(self.entity_index(id)?)
    }

    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        let i = self.constraints.binary_search_by_key(&id, |c| c.0).ok()?;
        self.constraints.get(i).map(|c| &c.1)
    }

    /// Position of a point entity.
    pub fn point(&self, id: EntityId) -> Option<[f64; 2]> {
        match self.entity(id)?.geom {
            Geom::Point { p } => Some(p),
            _ => None,
        }
    }

    /// Endpoint ids of a line entity.
    pub fn line_points(&self, id: EntityId) -> Option<(EntityId, EntityId)> {
        match self.entity(id)?.geom {
            Geom::Line { a, b } => Some((a, b)),
            _ => None,
        }
    }

    /// Fit points of a spline entity, in order.
    pub fn spline_points(&self, id: EntityId) -> Option<&[EntityId]> {
        match self.entity(id)?.geom {
            Geom::Spline { first, count, .. } => {
                let (a, b) = (first as usize, (first as usize).checked_add(count as usize)?);
                self.spline_pts.get(a..b)
            }
            _ => None,
        }
    }

    /// Every point entity an entity refers to (a spline's fit points included).
    pub fn referenced_points(&self, id: EntityId) -> Vec<EntityId> {
        match self.entity(id) {
            Some(e) => match e.geom {
                Geom::Spline { .. } => self.spline_points(id).map(<[EntityId]>::to_vec).unwrap_or_default(),
                g => g.referenced_points(),
            },
            None => Vec::new(),
        }
    }

    /// Radius of a circle (stored) or arc (`|start - center|`).
    pub fn radius(&self, id: EntityId) -> Option<f64> {
        match self.entity(id)?.geom {
            Geom::Circle { radius, .. } => Some(radius),
            Geom::Arc { center, start, .. } => {
                let c = self.point(center)?;
                let s = self.point(start)?;
                Some(crate::geom2::dist(c, s))
            }
            _ => None,
        }
    }

    /// Moves a point entity.
    pub fn set_point(&mut self, id: EntityId, p: [f64; 2]) -> Result<(), SketchError> {
        if !coord_ok(p[0]) || !coord_ok(p[1]) {
            return Err(SketchError::NonFinite(id));
        }
        let i = self.entity_index(id).ok_or(SketchError::UnknownEntity(id))?;
        match self.entities.get_mut(i) {
            Some(Entity { geom: Geom::Point { p: old }, .. }) => {
                *old = p;
                Ok(())
            }
            _ => Err(SketchError::WrongKind { entity: id, expected: "a point" }),
        }
    }

    /// Changes a circle's radius.
    pub fn set_radius(&mut self, id: EntityId, r: f64) -> Result<(), SketchError> {
        if !coord_ok(r) || r <= 0.0 {
            return Err(SketchError::NonFinite(id));
        }
        let i = self.entity_index(id).ok_or(SketchError::UnknownEntity(id))?;
        match self.entities.get_mut(i) {
            Some(Entity { geom: Geom::Circle { radius, .. }, .. }) => {
                *radius = r;
                Ok(())
            }
            _ => Err(SketchError::WrongKind { entity: id, expected: "a circle" }),
        }
    }

    /// Marks an entity as construction geometry (or back).
    pub fn set_construction(&mut self, id: EntityId, construction: bool) -> Result<(), SketchError> {
        let i = self.entity_index(id).ok_or(SketchError::UnknownEntity(id))?;
        if let Some(e) = self.entities.get_mut(i) {
            e.construction = construction;
        }
        Ok(())
    }

    fn push(&mut self, geom: Geom) -> Result<EntityId, SketchError> {
        if self.entities.len() >= MAX_ENTITIES || self.next_entity == u32::MAX {
            return Err(SketchError::TooManyEntities);
        }
        let id = EntityId(self.next_entity);
        self.next_entity += 1;
        self.entities.push(Entity { id, geom, construction: false });
        Ok(id)
    }

    fn expect_kind(&self, id: EntityId, ok: fn(EntityKind) -> bool, expected: &'static str) -> Result<(), SketchError> {
        let e = self.entity(id).ok_or(SketchError::UnknownEntity(id))?;
        if ok(e.geom.kind()) { Ok(()) } else { Err(SketchError::WrongKind { entity: id, expected }) }
    }

    fn expect_point(&self, id: EntityId) -> Result<(), SketchError> {
        self.expect_kind(id, |k| k == EntityKind::Point, "a point")
    }

    fn expect_line(&self, id: EntityId) -> Result<(), SketchError> {
        self.expect_kind(id, |k| k == EntityKind::Line, "a line")
    }

    fn expect_curve(&self, id: EntityId) -> Result<(), SketchError> {
        self.expect_kind(id, |k| matches!(k, EntityKind::Circle | EntityKind::Arc), "a circle or arc")
    }

    fn kind(&self, id: EntityId) -> Result<EntityKind, SketchError> {
        Ok(self.entity(id).ok_or(SketchError::UnknownEntity(id))?.geom.kind())
    }

    /// Adds a free point.
    pub fn add_point(&mut self, p: [f64; 2]) -> Result<EntityId, SketchError> {
        if !coord_ok(p[0]) || !coord_ok(p[1]) {
            return Err(SketchError::NonFinite(EntityId(self.next_entity)));
        }
        self.push(Geom::Point { p })
    }

    /// Adds a line between two new points.
    pub fn add_line(&mut self, a: [f64; 2], b: [f64; 2]) -> Result<EntityId, SketchError> {
        self.check_room(3)?;
        let pa = self.add_point(a)?;
        let pb = self.add_point(b)?;
        self.add_line_between(pa, pb)
    }

    /// Adds a line between two existing points.
    pub fn add_line_between(&mut self, a: EntityId, b: EntityId) -> Result<EntityId, SketchError> {
        self.check_geom(&Geom::Line { a, b })?;
        self.push(Geom::Line { a, b })
    }

    /// Adds a circle around a new centre point.
    pub fn add_circle(&mut self, center: [f64; 2], radius: f64) -> Result<EntityId, SketchError> {
        if !coord_ok(radius) || radius <= 0.0 {
            return Err(SketchError::InvalidValue { what: "circle radius must be positive and finite" });
        }
        self.check_room(2)?;
        let c = self.add_point(center)?;
        self.add_circle_at(c, radius)
    }

    /// Adds a circle around an existing centre point.
    pub fn add_circle_at(&mut self, center: EntityId, radius: f64) -> Result<EntityId, SketchError> {
        let g = Geom::Circle { center, radius };
        self.check_geom(&g)?;
        self.push(g)
    }

    /// Adds a counter-clockwise arc with three new points. `end` is moved radially
    /// onto the circle through `start` so the arc is consistent.
    pub fn add_arc(&mut self, center: [f64; 2], start: [f64; 2], end: [f64; 2]) -> Result<EntityId, SketchError> {
        self.check_room(4)?;
        let r = crate::geom2::dist(center, start);
        let de = crate::geom2::dist(center, end);
        let end = if de > 0.0 && r.is_finite() {
            [center[0] + (end[0] - center[0]) * (r / de), center[1] + (end[1] - center[1]) * (r / de)]
        } else {
            end
        };
        let c = self.add_point(center)?;
        let s = self.add_point(start)?;
        let e = self.add_point(end)?;
        self.add_arc_between(c, s, e)
    }

    /// Adds a counter-clockwise arc between existing points.
    pub fn add_arc_between(
        &mut self,
        center: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, SketchError> {
        let g = Geom::Arc { center, start, end };
        self.check_geom(&g)?;
        self.push(g)
    }

    /// Adds a spline through existing points: at least 3, at most
    /// [`crate::spline::MAX_SPLINE_POINTS`], no point twice in a row (nor first = last when
    /// closed). A closed spline connects the last point back to the first.
    pub fn add_spline(&mut self, points: &[EntityId], closed: bool) -> Result<EntityId, SketchError> {
        if points.len() < 3 {
            return Err(SketchError::InvalidValue { what: "a spline needs at least 3 points (use a line for 2)" });
        }
        if points.len() > crate::spline::MAX_SPLINE_POINTS {
            return Err(SketchError::InvalidValue { what: "a spline has too many points" });
        }
        for &p in points {
            self.expect_point(p)?;
        }
        for w in points.windows(2) {
            if let &[a, b] = w
                && a == b
            {
                return Err(SketchError::RepeatedEntity(a));
            }
        }
        if closed
            && let (Some(&f), Some(&l)) = (points.first(), points.last())
            && f == l
        {
            return Err(SketchError::RepeatedEntity(f));
        }
        let first = u32::try_from(self.spline_pts.len()).map_err(|_| SketchError::TooManyEntities)?;
        let count = u32::try_from(points.len()).map_err(|_| SketchError::TooManyEntities)?;
        let id = self.push(Geom::Spline { first, count, closed })?;
        self.spline_pts.extend_from_slice(points);
        Ok(id)
    }

    fn check_room(&self, n: usize) -> Result<(), SketchError> {
        if self.entities.len().saturating_add(n) > MAX_ENTITIES
            || u64::from(self.next_entity).saturating_add(n as u64) >= u64::from(u32::MAX)
        {
            return Err(SketchError::TooManyEntities);
        }
        Ok(())
    }

    fn check_geom(&self, g: &Geom) -> Result<(), SketchError> {
        match *g {
            Geom::Point { p } => {
                if !coord_ok(p[0]) || !coord_ok(p[1]) {
                    return Err(SketchError::InvalidValue { what: "point coordinate is not finite" });
                }
            }
            Geom::Line { a, b } => {
                self.expect_point(a)?;
                self.expect_point(b)?;
                if a == b {
                    return Err(SketchError::RepeatedEntity(a));
                }
            }
            Geom::Circle { center, radius } => {
                self.expect_point(center)?;
                if !coord_ok(radius) || radius <= 0.0 {
                    return Err(SketchError::InvalidValue { what: "circle radius must be positive and finite" });
                }
            }
            Geom::Arc { center, start, end } => {
                self.expect_point(center)?;
                self.expect_point(start)?;
                self.expect_point(end)?;
                if center == start || center == end {
                    return Err(SketchError::RepeatedEntity(center));
                }
                if start == end {
                    return Err(SketchError::RepeatedEntity(start));
                }
            }
            Geom::Spline { first, count, .. } => {
                let (a, b) = (first as usize, (first as usize).saturating_add(count as usize));
                let pts = self.spline_pts.get(a..b).ok_or(SketchError::InvalidValue { what: "spline point list" })?;
                if pts.len() < 3 {
                    return Err(SketchError::InvalidValue { what: "a spline needs at least 3 points" });
                }
                for &p in pts {
                    self.expect_point(p)?;
                }
            }
        }
        Ok(())
    }

    /// Checks that a constraint refers to existing entities of the right kinds and
    /// has a valid value.
    pub fn check_constraint(&self, c: &Constraint) -> Result<(), SketchError> {
        let distinct = |a: EntityId, b: EntityId| if a == b { Err(SketchError::RepeatedEntity(a)) } else { Ok(()) };
        let len_ok = |v: f64| v.is_finite() && (0.0..=4.0 * MAX_COORD).contains(&v);
        match *c {
            Constraint::Coincident(a, b) => {
                self.expect_point(a)?;
                self.expect_point(b)?;
                distinct(a, b)
            }
            Constraint::PointOnLine(p, l) | Constraint::Midpoint(p, l) => {
                self.expect_point(p)?;
                self.expect_line(l)
            }
            Constraint::PointOnCircle(p, c) => {
                self.expect_point(p)?;
                self.expect_curve(c)
            }
            Constraint::Horizontal(l) | Constraint::Vertical(l) => self.expect_line(l),
            Constraint::Parallel(a, b) | Constraint::Perpendicular(a, b) => {
                self.expect_line(a)?;
                self.expect_line(b)?;
                distinct(a, b)
            }
            Constraint::Tangent(a, b) => {
                distinct(a, b)?;
                let (ka, kb) = (self.kind(a)?, self.kind(b)?);
                let curve = |k| matches!(k, EntityKind::Circle | EntityKind::Arc);
                match (ka, kb) {
                    (EntityKind::Line, k) if curve(k) => Ok(()),
                    (k, EntityKind::Line) if curve(k) => Ok(()),
                    (k1, k2) if curve(k1) && curve(k2) => Ok(()),
                    (EntityKind::Line, _) | (EntityKind::Point, _) => {
                        Err(SketchError::WrongKind { entity: b, expected: "a circle or arc" })
                    }
                    _ => Err(SketchError::WrongKind { entity: a, expected: "a line, circle or arc" }),
                }
            }
            Constraint::Equal(a, b) => {
                distinct(a, b)?;
                match self.kind(a)? {
                    EntityKind::Line => self.expect_line(b),
                    EntityKind::Circle | EntityKind::Arc => self.expect_curve(b),
                    EntityKind::Point | EntityKind::Spline => {
                        Err(SketchError::WrongKind { entity: a, expected: "a line, circle or arc" })
                    }
                }
            }
            Constraint::Distance { a, b, value } => {
                if !len_ok(value) {
                    return Err(SketchError::InvalidValue { what: "distance must be finite and non-negative" });
                }
                match (self.kind(a)?, self.kind(b)?) {
                    (EntityKind::Point, EntityKind::Point) => distinct(a, b),
                    (EntityKind::Point, EntityKind::Line) | (EntityKind::Line, EntityKind::Point) => Ok(()),
                    (EntityKind::Line, EntityKind::Line) if a == b => Ok(()),
                    (EntityKind::Point | EntityKind::Line, _) => {
                        Err(SketchError::WrongKind { entity: b, expected: "a point, or the same line" })
                    }
                    _ => Err(SketchError::WrongKind { entity: a, expected: "a point or line" }),
                }
            }
            Constraint::Angle { a, b, value } => {
                if !value.is_finite() || value.abs() > 4.0 * core::f64::consts::PI {
                    return Err(SketchError::InvalidValue { what: "angle must be finite (radians)" });
                }
                self.expect_line(a)?;
                self.expect_line(b)?;
                distinct(a, b)
            }
            Constraint::Radius { c, value } | Constraint::Diameter { c, value } => {
                if !len_ok(value) || value <= 0.0 {
                    return Err(SketchError::InvalidValue { what: "radius/diameter must be positive and finite" });
                }
                self.expect_curve(c)
            }
            Constraint::Fixed(e) => self.kind(e).map(|_| ()),
            Constraint::Symmetric { a, b, axis } => {
                self.expect_point(a)?;
                self.expect_point(b)?;
                self.expect_line(axis)?;
                distinct(a, b)
            }
        }
    }

    /// Adds a constraint after checking it.
    pub fn add_constraint(&mut self, c: Constraint) -> Result<ConstraintId, SketchError> {
        if self.constraints.len() >= MAX_CONSTRAINTS || self.next_constraint == u32::MAX {
            return Err(SketchError::TooManyConstraints);
        }
        self.check_constraint(&c)?;
        let id = ConstraintId(self.next_constraint);
        self.next_constraint += 1;
        self.constraints.push((id, c));
        Ok(id)
    }

    /// Removes a constraint and returns it.
    pub fn remove_constraint(&mut self, id: ConstraintId) -> Result<Constraint, SketchError> {
        let i = self.constraints.binary_search_by_key(&id, |c| c.0).map_err(|_| SketchError::UnknownConstraint(id))?;
        Ok(self.constraints.remove(i).1)
    }

    /// Removes an entity, every entity that refers to it (e.g. the lines on a point)
    /// and every constraint that refers to any removed entity. Returns the removed
    /// entity ids in ascending order. Points used by a removed line stay.
    pub fn remove_entity(&mut self, id: EntityId) -> Result<Vec<EntityId>, SketchError> {
        if self.entity_index(id).is_none() {
            return Err(SketchError::UnknownEntity(id));
        }
        let mut removed: Vec<EntityId> = vec![id];
        for e in &self.entities {
            if self.referenced_points(e.id).contains(&id) {
                removed.push(e.id);
            }
        }
        removed.sort_unstable();
        removed.dedup();
        self.entities.retain(|e| removed.binary_search(&e.id).is_err());
        self.constraints.retain(|(_, c)| c.entities().iter().all(|e| removed.binary_search(e).is_err()));
        Ok(removed)
    }

    /// Checks the whole sketch: references, kinds, finite values and limits.
    pub fn validate(&self) -> Result<(), SketchError> {
        if self.entities.len() > MAX_ENTITIES {
            return Err(SketchError::TooManyEntities);
        }
        if self.constraints.len() > MAX_CONSTRAINTS {
            return Err(SketchError::TooManyConstraints);
        }
        for e in &self.entities {
            self.check_geom(&e.geom).map_err(|err| match err {
                SketchError::InvalidValue { .. } => SketchError::NonFinite(e.id),
                other => other,
            })?;
        }
        for (_, c) in &self.constraints {
            self.check_constraint(c)?;
        }
        Ok(())
    }

    /// Mutable entity access for writing solved values back (ids and kinds must not change).
    pub(crate) fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_never_reused() {
        let mut s = Sketch::new();
        let a = s.add_point([0.0, 0.0]).unwrap();
        let b = s.add_point([1.0, 0.0]).unwrap();
        s.remove_entity(b).unwrap();
        let c = s.add_point([2.0, 0.0]).unwrap();
        assert!(c > b && b > a);
        assert!(s.entity(b).is_none());
    }

    #[test]
    fn rejects_bad_input() {
        let mut s = Sketch::new();
        assert!(matches!(s.add_point([f64::NAN, 0.0]), Err(SketchError::NonFinite(_))));
        assert!(s.add_point([1e300, 0.0]).is_err());
        let p = s.add_point([0.0, 0.0]).unwrap();
        assert_eq!(s.add_line_between(p, p), Err(SketchError::RepeatedEntity(p)));
        assert!(s.add_circle([0.0, 0.0], -1.0).is_err());
        let l = s.add_line([0.0, 0.0], [1.0, 0.0]).unwrap();
        assert!(matches!(s.add_constraint(Constraint::Horizontal(p)), Err(SketchError::WrongKind { .. })));
        assert!(s.add_constraint(Constraint::Distance { a: p, b: l, value: f64::NAN }).is_err());
        assert!(s.add_constraint(Constraint::Distance { a: p, b: l, value: -1.0 }).is_err());
        assert!(s.add_constraint(Constraint::Distance { a: l, b: l, value: 3.0 }).is_ok());
        assert_eq!(
            s.add_constraint(Constraint::Coincident(p, EntityId(999))),
            Err(SketchError::UnknownEntity(EntityId(999)))
        );
        assert!(s.set_point(p, [f64::INFINITY, 0.0]).is_err());
    }

    #[test]
    fn remove_cascades() {
        let mut s = Sketch::new();
        let l = s.add_line([0.0, 0.0], [1.0, 0.0]).unwrap();
        let (a, _) = s.line_points(l).unwrap();
        s.add_constraint(Constraint::Horizontal(l)).unwrap();
        s.add_constraint(Constraint::Fixed(a)).unwrap();
        let removed = s.remove_entity(a).unwrap();
        assert_eq!(removed, vec![a, l]);
        assert!(s.constraints().is_empty());
        assert!(s.validate().is_ok());
    }

    #[test]
    fn add_arc_snaps_end_to_radius() {
        let mut s = Sketch::new();
        let arc = s.add_arc([0.0, 0.0], [2.0, 0.0], [0.0, 5.0]).unwrap();
        let Geom::Arc { end, .. } = s.entity(arc).unwrap().geom else { panic!() };
        assert_eq!(s.point(end).unwrap(), [0.0, 2.0]);
        assert_eq!(s.radius(arc).unwrap(), 2.0);
    }
}
