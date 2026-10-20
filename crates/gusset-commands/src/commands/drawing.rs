//! 2D drawings (ADR 0032): sheets of views, dimensions and notes in the document, their
//! query and their export to SVG and DXF.

use super::*;
use gusset_doc::drawing::{
    DimDef, DimDirection, DimKind, Drawing, DrawingItem, NoteDef, PartsListDef, Projection, SheetSize, Side, ViewDef,
    ViewDir, ViewPlacement,
};
use gusset_history::Actor;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<DrawingCreate>(),
        spec::<DrawingEdit>(),
        spec::<DrawingAddView>(),
        spec::<DrawingEditView>(),
        spec::<DrawingAddDimension>(),
        spec::<DrawingEditDimension>(),
        spec::<DrawingAddNote>(),
        spec::<DrawingAddPartsList>(),
        spec::<DrawingMoveItem>(),
        spec::<DrawingDeleteItem>(),
        spec::<DrawingExport>(),
        spec::<QueryDrawing>(),
    ]
}

fn find<'a>(doc: &'a mut Document, name: &str) -> Result<&'a mut Drawing, CommandError> {
    let have: Vec<String> = doc.drawings.iter().map(|d| d.name.clone()).collect();
    doc.drawings.iter_mut().find(|d| d.name == name).ok_or_else(|| {
        CommandError::not_found(format!("no drawing '{name}'")).hint(if have.is_empty() {
            "create one with drawing_create".to_string()
        } else {
            format!("drawings: {}", have.join(", "))
        })
    })
}

/// A name for a new item of drawing `d`: `name`, or `base` plus the first free number.
fn item_name(d: &Drawing, name: Option<String>, base: &str) -> Result<String, CommandError> {
    if let Some(n) = name {
        gusset_doc::validate::validate_feature_name(&n).map_err(CommandError::args)?;
        if d.item(&n).is_some() {
            return Err(CommandError::args(format!("drawing '{}' already has an item '{n}'", d.name)));
        }
        return Ok(n);
    }
    Ok((1..).map(|i| format!("{base}{i}")).find(|n| d.item(n).is_none()).unwrap_or_else(|| base.to_string()))
}

fn pair(what: &str, v: &[String; 2]) -> Result<[gusset_units::expr::Expr; 2], CommandError> {
    Ok([parse_expr(what, &v[0])?, parse_expr(what, &v[1])?])
}

fn opt_expr(what: &str, v: &Option<String>) -> Result<Option<gusset_units::expr::Expr>, CommandError> {
    v.as_deref().map(|t| parse_expr(what, t)).transpose()
}

/// `Some("")` clears a text field.
fn set_text(slot: &mut Option<String>, v: Option<String>) {
    if let Some(v) = v {
        *slot = (!v.is_empty()).then_some(v);
    }
}

fn size_of(s: &str) -> Result<SheetSize, CommandError> {
    SheetSize::from_keyword(s)
        .ok_or_else(|| CommandError::args(format!("unknown sheet size '{s}'")).hint("use A0-A4 or ANSI_A-ANSI_E"))
}

fn projection_of(s: &str) -> Result<Projection, CommandError> {
    match s {
        "first" => Ok(Projection::First),
        "third" => Ok(Projection::Third),
        _ => Err(CommandError::args(format!("unknown projection '{s}'")).hint("use first (ISO) or third (ASME)")),
    }
}

fn dir_of(s: &str) -> Result<ViewDir, CommandError> {
    ViewDir::from_keyword(s).ok_or_else(|| {
        CommandError::args(format!("unknown view direction '{s}'"))
            .hint("use front, back, top, bottom, left, right, iso or iso_back")
    })
}

fn side_of(s: &str) -> Result<Side, CommandError> {
    Side::from_keyword(s)
        .ok_or_else(|| CommandError::args(format!("unknown side '{s}'")).hint("use top, bottom, left or right"))
}

fn num(v: f64) -> String {
    gusset_units::quantity::format_exact(libm::round(v * 10.0) / 10.0)
}

/// Creates a drawing (one sheet).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingCreate {
    #[serde(default)]
    pub name: Option<String>,
    /// A0-A4 or ANSI_A-ANSI_E (default A3).
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub portrait: bool,
    /// first (ISO, default) or third (ASME).
    #[serde(default)]
    pub projection: Option<String>,
    /// Sheet scale, e.g. "0.5" for 1:2.
    #[serde(default)]
    pub scale: Option<String>,
    #[serde(default)]
    pub units: Option<String>,
    #[serde(default)]
    pub decimals: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
    /// Also add front, top, right and iso views of every body.
    #[serde(default)]
    pub standard_views: bool,
}

impl Command for DrawingCreate {
    const ID: &'static str = "drawing.create";
    const TITLE: &'static str = "New drawing";
    const DESCRIPTION: &'static str = "Create a drawing sheet (size, projection, scale, units, title block); standard_views adds front, top, right, iso.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let size = self.size.as_deref().map(size_of).transpose()?.unwrap_or(SheetSize::A3);
        let projection = self.projection.as_deref().map(projection_of).transpose()?.unwrap_or(Projection::First);
        let scale = opt_expr("scale", &self.scale)?;
        let units = self.units.as_deref().map(units_of).transpose()?;
        let decimals = self.decimals.map(decimals_of).transpose()?;
        let created = std::cell::RefCell::new(String::new());
        edit_checked(
            s,
            ctx,
            "New drawing",
            |doc, _| {
                let name = match self.name {
                    Some(n) => {
                        gusset_doc::validate::validate_feature_name(&n).map_err(CommandError::args)?;
                        if doc.feature(&n).is_some() || doc.drawings.iter().any(|d| d.name == n) {
                            return Err(CommandError::args(format!("the name '{n}' is taken")));
                        }
                        n
                    }
                    None => (1..)
                        .map(|i| format!("Drawing{i}"))
                        .find(|n| doc.feature(n).is_none() && !doc.drawings.iter().any(|d| d.name == *n))
                        .unwrap_or_else(|| "Drawing".into()),
                };
                let mut d = Drawing::new(&name);
                *created.borrow_mut() = name.clone();
                d.size = size;
                d.landscape = !self.portrait;
                d.projection = projection;
                d.scale = scale;
                d.units = units;
                d.decimals = decimals;
                d.title = self.title.filter(|t| !t.is_empty());
                if self.standard_views {
                    let [w, h] = gusset_drawing::sheet_size(&d);
                    let e = |v: f64| gusset_units::expr::parse(&num(v)).map_err(|e| CommandError::args(e.to_string()));
                    let base = |n: &str, from, at: [f64; 2], hidden| -> Result<DrawingItem, CommandError> {
                        Ok(DrawingItem::View(ViewDef {
                            name: n.into(),
                            placement: ViewPlacement::Base { bodies: Vec::new(), from, at: [e(at[0])?, e(at[1])?] },
                            scale: None,
                            hidden,
                            tangent: false,
                            centers: true,
                            section: None,
                        }))
                    };
                    let projected = |n: &str, side| {
                        DrawingItem::View(ViewDef {
                            name: n.into(),
                            placement: ViewPlacement::Projected { of: "Front".into(), side, gap: None },
                            scale: None,
                            hidden: None,
                            tangent: false,
                            centers: true,
                            section: None,
                        })
                    };
                    // Front in the upper middle, so first and third angle both have room.
                    d.items.push(base("Front", ViewDir::Front, [w * 0.42, h * 0.62], None)?);
                    d.items.push(projected("Top", Side::Top));
                    d.items.push(projected("Right", Side::Right));
                    d.items.push(base("Iso", ViewDir::Iso, [w * 0.8, h * 0.72], None)?);
                }
                doc.drawings.push(d);
                Ok(json!({ "drawing": name }))
            },
            |s, doc, ev| sheet_check(s, &ctx.actor, doc, ev, &created.borrow(), None),
            |_, _, _| Value::Null,
        )
    }
}

/// Changes a drawing's sheet and title block.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingEdit {
    pub drawing: String,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub portrait: Option<bool>,
    #[serde(default)]
    pub projection: Option<String>,
    /// "" removes it (1:1).
    #[serde(default)]
    pub scale: Option<String>,
    #[serde(default)]
    pub units: Option<String>,
    #[serde(default)]
    pub decimals: Option<u32>,
    /// Title block fields; "" clears one.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

impl Command for DrawingEdit {
    const ID: &'static str = "drawing.edit";
    const TITLE: &'static str = "Edit drawing";
    const DESCRIPTION: &'static str = "Change a drawing's size, orientation, projection, scale, units or title block.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let size = self.size.as_deref().map(size_of).transpose()?;
        let projection = self.projection.as_deref().map(projection_of).transpose()?;
        let scale = match self.scale.as_deref().map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(t) => Some(Some(parse_expr("scale", t)?)),
        };
        let units = match self.units.as_deref().map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(u) => Some(Some(units_of(u)?)),
        };
        let decimals = self.decimals.map(decimals_of).transpose()?;
        let drawing = self.drawing.clone();
        edit_checked(
            s,
            ctx,
            "Edit drawing",
            |doc, _| {
                let d = find(doc, &self.drawing)?;
                if let Some(z) = size {
                    d.size = z;
                }
                if let Some(p) = self.portrait {
                    d.landscape = !p;
                }
                if let Some(p) = projection {
                    d.projection = p;
                }
                if let Some(sc) = scale {
                    d.scale = sc;
                }
                if let Some(u) = units {
                    d.units = u;
                }
                if let Some(n) = decimals {
                    d.decimals = Some(n);
                }
                set_text(&mut d.title, self.title);
                set_text(&mut d.number, self.number);
                set_text(&mut d.material, self.material);
                set_text(&mut d.author, self.author);
                Ok(json!({ "drawing": d.name }))
            },
            |s, doc, ev| sheet_check(s, &ctx.actor, doc, ev, &drawing, None),
            |_, _, _| Value::Null,
        )
    }
}

/// Adds a view: a base view (`from`, `at`) or one projected from another view (`of`, `side`).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingAddView {
    pub drawing: String,
    #[serde(default)]
    pub name: Option<String>,
    /// front, back, top, bottom, left, right, iso, iso_back.
    #[serde(default)]
    pub from: Option<String>,
    /// Centre, sheet mm from the lower-left.
    #[serde(default)]
    pub at: Option<[String; 2]>,
    #[serde(default)]
    pub bodies: Vec<String>,
    #[serde(default)]
    pub of: Option<String>,
    /// top/bottom/left/right, as seen in the parent.
    #[serde(default)]
    pub side: Option<String>,
    /// Sheet mm (20).
    #[serde(default)]
    pub gap: Option<String>,
    #[serde(default)]
    pub scale: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub tangent: bool,
    #[serde(default)]
    pub centers: Option<bool>,
    /// Section label.
    #[serde(default)]
    pub section: Option<String>,
    /// Cut offset, doc units (default mid).
    #[serde(default)]
    pub cut_at: Option<String>,
    /// Detail: the view it enlarges.
    #[serde(default)]
    pub detail_of: Option<String>,
    /// Detail centre, parent view coords.
    #[serde(default)]
    pub center: Option<[String; 2]>,
    #[serde(default)]
    pub radius: Option<String>,
}

impl Command for DrawingAddView {
    const ID: &'static str = "drawing.add_view";
    const TITLE: &'static str = "Add view";
    const DESCRIPTION: &'static str =
        "Add a base (from, at), projected (of, side, section) or detail view (detail_of).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let placement = match (&self.of, &self.from) {
            (None, None) if self.detail_of.is_some() => {
                let need = |w: &str| CommandError::args(format!("a detail view needs {w}"));
                ViewPlacement::Detail {
                    of: self.detail_of.clone().unwrap_or_default(),
                    center: pair("center", self.center.as_ref().ok_or_else(|| need("center"))?)?,
                    radius: parse_expr("radius", self.radius.as_deref().ok_or_else(|| need("radius"))?)?,
                    at: pair("at", self.at.as_ref().ok_or_else(|| need("at"))?)?,
                }
            }
            _ if self.detail_of.is_some() => {
                return Err(CommandError::args("a detail view takes detail_of, center, radius and at"));
            }
            (Some(of), None) => {
                if self.at.is_some() || !self.bodies.is_empty() {
                    return Err(CommandError::args("a projected view takes its bodies and place from its parent")
                        .hint("remove at/bodies, or give from instead of of"));
                }
                let side =
                    side_of(self.side.as_deref().ok_or_else(|| CommandError::args("a projected view needs side"))?)?;
                ViewPlacement::Projected { of: of.clone(), side, gap: opt_expr("gap", &self.gap)? }
            }
            (None, Some(from)) => {
                let at =
                    self.at.as_ref().ok_or_else(|| CommandError::args("a base view needs at: [x, y] (sheet mm)"))?;
                ViewPlacement::Base { bodies: self.bodies.clone(), from: dir_of(from)?, at: pair("at", at)? }
            }
            _ => {
                return Err(CommandError::args(
                    "give either from and at (a base view) or of and side (a projected view)",
                ));
            }
        };
        let scale = opt_expr("scale", &self.scale)?;
        let section = match (&self.section, &placement) {
            (Some(l), ViewPlacement::Projected { .. }) => {
                let first_letter = l.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
                if !first_letter || l.len() > 8 || !l.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err(CommandError::args("a section label is a letter, then up to 7 letters or digits")
                        .hint("e.g. A or B2"));
                }
                Some(gusset_doc::drawing::SectionDef { label: l.clone(), cut_at: opt_expr("cut_at", &self.cut_at)? })
            }
            (Some(_), _) => {
                return Err(CommandError::args("a section is projected from the view it cuts").hint("give of and side"));
            }
            (None, _) if self.cut_at.is_some() => return Err(CommandError::args("cut_at needs section")),
            (None, _) => None,
        };
        let drawing = self.drawing.clone();
        let created = std::cell::RefCell::new(String::new());
        edit_checked(
            s,
            ctx,
            "Add view",
            |doc, _| {
                let d = find(doc, &self.drawing)?;
                let name = item_name(d, self.name, "View")?;
                d.items.push(DrawingItem::View(ViewDef {
                    name: name.clone(),
                    placement,
                    scale,
                    hidden: self.hidden,
                    tangent: self.tangent,
                    centers: self.centers.unwrap_or(true),
                    section,
                }));
                *created.borrow_mut() = name.clone();
                Ok(json!({ "view": name }))
            },
            |s, doc, ev| sheet_check(s, &ctx.actor, doc, ev, &drawing, Some(&created.borrow())),
            |_, _, _| Value::Null,
        )
        .and_then(|v| {
            let q = query(s, ctx, Some(&drawing))?;
            let name = created.borrow().clone();
            let view = q
                .get("views")
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|v| v.get("name").and_then(Value::as_str) == Some(name.as_str())))
                .cloned();
            Ok(merge(v, json!({ "placed": view })))
        })
    }
}

/// Moves a view or changes how it is drawn.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingEditView {
    pub drawing: String,
    pub view: String,
    /// Base views: the new centre (sheet mm).
    #[serde(default)]
    pub at: Option<[String; 2]>,
    /// Projected views: the gap to the parent.
    #[serde(default)]
    pub gap: Option<String>,
    /// "" restores the sheet's scale.
    #[serde(default)]
    pub scale: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub tangent: Option<bool>,
    #[serde(default)]
    pub centers: Option<bool>,
}

impl Command for DrawingEditView {
    const ID: &'static str = "drawing.edit_view";
    const TITLE: &'static str = "Edit view";
    const DESCRIPTION: &'static str =
        "Move a view (at; gap if projected) or change its scale, hidden/tangent lines, centre marks.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let at = self.at.as_ref().map(|a| pair("at", a)).transpose()?;
        let gap = opt_expr("gap", &self.gap)?;
        let scale = match self.scale.as_deref().map(str::trim) {
            None => None,
            Some("") => Some(None),
            Some(t) => Some(Some(parse_expr("scale", t)?)),
        };
        let (drawing, item) = (self.drawing.clone(), self.view.clone());
        edit_checked(
            s,
            ctx,
            "Edit view",
            |doc, _| {
                let d = find(doc, &self.drawing)?;
                let dname = d.name.clone();
                let v = d
                    .items
                    .iter_mut()
                    .find_map(|i| match i {
                        DrawingItem::View(v) if v.name == self.view => Some(v),
                        _ => None,
                    })
                    .ok_or_else(|| CommandError::not_found(format!("drawing '{dname}' has no view '{}'", self.view)))?;
                match (&mut v.placement, at, gap) {
                    (ViewPlacement::Base { at: slot, .. } | ViewPlacement::Detail { at: slot, .. }, Some(a), None) => {
                        *slot = a
                    }
                    (ViewPlacement::Projected { gap: slot, .. }, None, Some(g)) => *slot = Some(g),
                    (_, None, None) => {}
                    (ViewPlacement::Base { .. } | ViewPlacement::Detail { .. }, _, Some(_)) => {
                        return Err(CommandError::args("gap applies to projected views; move a base view with at"));
                    }
                    (ViewPlacement::Projected { .. }, Some(_), _) => {
                        return Err(CommandError::args(
                            "a projected view follows its parent; change gap, or move the parent",
                        ));
                    }
                }
                if let Some(sc) = scale {
                    v.scale = sc;
                }
                if let Some(h) = self.hidden {
                    v.hidden = Some(h);
                }
                if let Some(t) = self.tangent {
                    v.tangent = t;
                }
                if let Some(c) = self.centers {
                    v.centers = c;
                }
                Ok(json!({ "view": v.name }))
            },
            |s, doc, ev| sheet_check(s, &ctx.actor, doc, ev, &drawing, Some(&item)),
            |_, _, _| Value::Null,
        )
    }
}

/// Adds a driven dimension on model edges, measured in a view.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingAddDimension {
    pub drawing: String,
    pub view: String,
    /// length, distance (2 edges), diameter, radius or angle (2 lines).
    pub kind: String,
    /// Edge names, e.g. "edge(Plate/cap(end), Plate/side(S/l5))".
    pub a: String,
    #[serde(default)]
    pub b: Option<String>,
    /// aligned (default), horizontal or vertical.
    #[serde(default)]
    pub direction: Option<String>,
    /// Sheet mm from the geometry.
    #[serde(default)]
    pub offset: Option<String>,
    #[serde(default)]
    pub tolerance: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for DrawingAddDimension {
    const ID: &'static str = "drawing.add_dimension";
    const TITLE: &'static str = "Add dimension";
    const DESCRIPTION: &'static str =
        "Add a driven dimension in a drawing view on edge names; measured from the model each time.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let kind = DimKind::from_keyword(&self.kind).ok_or_else(|| {
            CommandError::args(format!("unknown dimension kind '{}'", self.kind))
                .hint("use length, distance, diameter, radius or angle")
        })?;
        let edge = |what: &str, t: &str| -> Result<EdgeRef, CommandError> {
            parse_edges(&[t.to_string()])?
                .into_iter()
                .next()
                .ok_or_else(|| CommandError::args(format!("{what}: expected an edge name")))
        };
        let a = edge("a", &self.a)?;
        let b = self.b.as_deref().map(|t| edge("b", t)).transpose()?;
        if kind.takes_two() != b.is_some() {
            return Err(CommandError::args(format!(
                "a {} dimension takes {}",
                kind.keyword(),
                if kind.takes_two() { "two edges (a and b)" } else { "one edge (a)" }
            )));
        }
        let direction = match self.direction.as_deref() {
            None => DimDirection::Aligned,
            Some(d) => DimDirection::from_keyword(d).ok_or_else(|| {
                CommandError::args(format!("unknown direction '{d}'")).hint("use aligned, horizontal or vertical")
            })?,
        };
        let offset = opt_expr("offset", &self.offset)?;
        let drawing = self.drawing.clone();
        let created = std::cell::RefCell::new(String::new());
        edit_checked(
            s,
            ctx,
            "Add dimension",
            |doc, _| {
                let d = find(doc, &self.drawing)?;
                if d.view(&self.view).is_none() {
                    return Err(CommandError::not_found(format!("drawing '{}' has no view '{}'", d.name, self.view)));
                }
                let name = item_name(d, self.name, "D")?;
                *created.borrow_mut() = name.clone();
                d.items.push(DrawingItem::Dim(DimDef {
                    name: name.clone(),
                    view: self.view,
                    kind,
                    a,
                    b,
                    direction,
                    offset,
                    text_at: None,
                    tolerance: self.tolerance.filter(|t| !t.is_empty()),
                    prefix: self.prefix.filter(|t| !t.is_empty()),
                }));
                Ok(json!({ "dimension": name }))
            },
            |s, doc, ev| sheet_check(s, &ctx.actor, doc, ev, &drawing, Some(&created.borrow())),
            |_, _, _| Value::Null,
        )
    }
}

/// Moves a dimension or changes its text.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingEditDimension {
    pub drawing: String,
    pub dimension: String,
    #[serde(default)]
    pub offset: Option<String>,
    /// Sheet mm; [] = automatic.
    #[serde(default)]
    pub text_at: Option<Vec<String>>,
    #[serde(default)]
    pub direction: Option<String>,
    /// "" clears.
    #[serde(default)]
    pub tolerance: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
}

impl Command for DrawingEditDimension {
    const ID: &'static str = "drawing.edit_dimension";
    const TITLE: &'static str = "Edit dimension";
    const DESCRIPTION: &'static str = "Move a dimension or change its direction, tolerance or prefix.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let offset = opt_expr("offset", &self.offset)?;
        let text_at = match self.text_at.as_deref() {
            None => None,
            Some([]) => Some(None),
            Some([x, y]) => Some(Some([parse_expr("text_at", x)?, parse_expr("text_at", y)?])),
            Some(_) => return Err(CommandError::args("text_at takes [x, y], or [] for the automatic place")),
        };
        let direction = match self.direction.as_deref() {
            None => None,
            Some(d) => Some(DimDirection::from_keyword(d).ok_or_else(|| {
                CommandError::args(format!("unknown direction '{d}'")).hint("use aligned, horizontal or vertical")
            })?),
        };
        let (drawing, item) = (self.drawing.clone(), self.dimension.clone());
        edit_checked(
            s,
            ctx,
            "Edit dimension",
            |doc, _| {
                let d = find(doc, &self.drawing)?;
                let dname = d.name.clone();
                let dim = d
                    .items
                    .iter_mut()
                    .find_map(|i| match i {
                        DrawingItem::Dim(x) if x.name == self.dimension => Some(x),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        CommandError::not_found(format!("drawing '{dname}' has no dimension '{}'", self.dimension))
                    })?;
                if offset.is_some() {
                    dim.offset = offset;
                }
                if let Some(t) = text_at {
                    dim.text_at = t;
                }
                if let Some(dir) = direction {
                    dim.direction = dir;
                }
                set_text(&mut dim.tolerance, self.tolerance);
                set_text(&mut dim.prefix, self.prefix);
                Ok(json!({ "dimension": dim.name }))
            },
            |s, doc, ev| sheet_check(s, &ctx.actor, doc, ev, &drawing, Some(&item)),
            |_, _, _| Value::Null,
        )
    }
}

/// Adds a parts list (the bill of materials as a table).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingAddPartsList {
    pub drawing: String,
    /// Top-left corner, sheet mm.
    pub at: [String; 2],
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for DrawingAddPartsList {
    const ID: &'static str = "drawing.add_parts_list";
    const TITLE: &'static str = "Add parts list";
    const DESCRIPTION: &'static str = "Add a parts list (item, part, quantity; as query_bom) to a drawing.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let at = pair("at", &self.at)?;
        edit(s, ctx, "Add parts list", |doc, _| {
            let d = find(doc, &self.drawing)?;
            let name = item_name(d, self.name, "Parts")?;
            d.items.push(DrawingItem::PartsList(PartsListDef { name: name.clone(), at }));
            Ok(json!({ "parts_list": name }))
        })
    }
}

/// Adds a text note.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingAddNote {
    pub drawing: String,
    pub text: String,
    /// Sheet mm from the lower-left corner.
    pub at: [String; 2],
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for DrawingAddNote {
    const ID: &'static str = "drawing.add_note";
    const TITLE: &'static str = "Add note";
    const DESCRIPTION: &'static str = "Add a text note to a drawing at a sheet position (mm).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let at = pair("at", &self.at)?;
        edit(s, ctx, "Add note", |doc, _| {
            let d = find(doc, &self.drawing)?;
            let name = item_name(d, self.name, "Note")?;
            d.items.push(DrawingItem::Note(NoteDef { name: name.clone(), text: self.text, at }));
            Ok(json!({ "note": name }))
        })
    }
}

/// Places a base view, a note or a dimension's text at a sheet position.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingMoveItem {
    pub drawing: String,
    pub item: String,
    /// Sheet mm.
    pub at: [String; 2],
}

impl Command for DrawingMoveItem {
    const ID: &'static str = "drawing.move_item";
    const TITLE: &'static str = "Move on sheet";
    const DESCRIPTION: &'static str =
        "Place a base view (its centre), a note or a dimension's text at a sheet position.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let at = pair("at", &self.at)?;
        edit(s, ctx, "Move on sheet", |doc, _| {
            let d = find(doc, &self.drawing)?;
            let dname = d.name.clone();
            let item = d
                .items
                .iter_mut()
                .find(|i| i.name() == self.item)
                .ok_or_else(|| CommandError::not_found(format!("drawing '{dname}' has no item '{}'", self.item)))?;
            match item {
                DrawingItem::View(ViewDef {
                    placement: ViewPlacement::Base { at: slot, .. } | ViewPlacement::Detail { at: slot, .. },
                    ..
                }) => *slot = at,
                DrawingItem::View(_) => {
                    return Err(CommandError::args("a projected view follows its parent")
                        .hint("move the parent, or change this view's gap"));
                }
                DrawingItem::Note(n) => n.at = at,
                DrawingItem::PartsList(p) => p.at = at,
                DrawingItem::Dim(dim) => dim.text_at = Some(at),
            }
            Ok(json!({ "moved": self.item }))
        })
    }
}

/// Deletes a view, dimension or note, or (without `item`) the whole drawing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingDeleteItem {
    pub drawing: String,
    #[serde(default)]
    pub item: Option<String>,
}

impl Command for DrawingDeleteItem {
    const ID: &'static str = "drawing.delete";
    const TITLE: &'static str = "Delete from drawing";
    const DESCRIPTION: &'static str =
        "Delete a view, dimension or note (item), or the whole drawing. A view others depend on cannot go first.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        edit(s, ctx, "Delete from drawing", |doc, _| {
            let Some(item) = self.item else {
                find(doc, &self.drawing)?;
                doc.drawings.retain(|d| d.name != self.drawing);
                return Ok(json!({ "deleted": self.drawing }));
            };
            let d = find(doc, &self.drawing)?;
            if d.item(&item).is_none() {
                return Err(CommandError::not_found(format!("drawing '{}' has no item '{item}'", d.name)));
            }
            let users: Vec<&str> = d
                .items
                .iter()
                .filter(|i| match i {
                    DrawingItem::View(v) if v.placement.parent().is_some() => {
                        v.placement.parent() == Some(item.as_str())
                    }
                    DrawingItem::Dim(dim) => dim.view == item,
                    _ => false,
                })
                .map(DrawingItem::name)
                .collect();
            if !users.is_empty() {
                return Err(
                    CommandError::args(format!("{} depend on '{item}'", users.join(", "))).hint("delete those first")
                );
            }
            d.items.retain(|i| i.name() != item);
            Ok(json!({ "deleted": item }))
        })
    }
}

/// Builds the scene of drawing `name` on the actor's branch.
fn scene(s: &mut Session, ctx: &CmdCtx, name: &str) -> Result<gusset_drawing::Scene, CommandError> {
    drawing_scene(s, &ctx.actor, name)
}

/// The scene of drawing `name` on `actor`'s branch, as `render_view` and the app draw it.
///
/// # Errors
/// No such drawing, or the branch does not evaluate.
fn units_of(u: &str) -> Result<gusset_units::quantity::LengthUnit, CommandError> {
    gusset_doc::drawing::unit_of_keyword(u.trim())
        .ok_or_else(|| CommandError::args(format!("unknown drawing unit '{u}'")).hint("use mm or in"))
}

fn decimals_of(n: u32) -> Result<u32, CommandError> {
    if n > gusset_doc::drawing::MAX_DECIMALS {
        return Err(CommandError::args(format!("decimals is {n}; at most {}", gusset_doc::drawing::MAX_DECIMALS)));
    }
    Ok(n)
}

/// Refuses an edit the sheet cannot draw (dogfooding F5-5): an error of the item `item` of
/// drawing `drawing` (a view or dimension), or with `item` `None` a sheet scale that does
/// not evaluate, that the edit brings in. Errors already there before the edit (a
/// dimension left dangling by a model change) do not block moving or fixing the item.
fn sheet_check(
    s: &mut Session,
    actor: &Actor,
    doc: &Document,
    ev: &gusset_eval::Evaluation,
    drawing: &str,
    item: Option<&str>,
) -> Result<(), CommandError> {
    let problem = |s: &mut Session, doc: &Document| -> Option<String> {
        let d = doc.drawings.iter().find(|d| d.name == drawing)?;
        let kernel = s.kernel().clone();
        let cx = gusset_drawing::Context { ev, kernel: kernel.as_ref(), units: doc.units, cancel: None };
        let sc = gusset_drawing::build(d, &cx, &mut s.drawing_cache);
        match item {
            Some(item) => sc.views.iter().find(|v| v.name == item).and_then(|v| v.error.clone()).or_else(|| {
                sc.dims
                    .iter()
                    .find(|d| d.name == item)
                    .and_then(|d| d.error.as_ref())
                    .map(|e| format!("dimension {item}: {e}"))
            }),
            None => sc.issues.iter().find(|i| i.contains("sheet scale")).cloned(),
        }
    };
    let Some(after) = problem(s, doc) else { return Ok(()) };
    let old = s.document(actor)?;
    if problem(s, &old).as_deref() == Some(after.as_str()) {
        return Ok(());
    }
    Err(CommandError::args(after).hint("nothing was changed"))
}

/// `drawing/dimension` for every dimension of `actor`'s drawings that does not measure.
pub(crate) fn dangling_dims(s: &mut Session, actor: &Actor) -> std::collections::BTreeSet<String> {
    let names: Vec<String> =
        s.document(actor).map(|d| d.drawings.iter().map(|d| d.name.clone()).collect()).unwrap_or_default();
    let mut out = std::collections::BTreeSet::new();
    for n in names {
        if let Ok(sc) = drawing_scene(s, actor, &n) {
            out.extend(sc.dims.iter().filter(|d| d.error.is_some()).map(|d| format!("{n}/{}", d.name)));
        }
    }
    out
}

pub fn drawing_scene(s: &mut Session, actor: &Actor, name: &str) -> Result<gusset_drawing::Scene, CommandError> {
    let doc = s.document(actor)?;
    let d = doc
        .drawings
        .iter()
        .find(|d| d.name == name)
        .cloned()
        .ok_or_else(|| CommandError::not_found(format!("no drawing '{name}'")).hint("see query_drawing"))?;
    let ev = s.evaluate(actor)?;
    let kernel = s.kernel().clone();
    let stop = s.rebuild_cancel_flag();
    let cx = gusset_drawing::Context { ev: &ev, kernel: kernel.as_ref(), units: doc.units, cancel: Some(&stop) };
    Ok(gusset_drawing::build(&d, &cx, &mut s.drawing_cache))
}

fn query(s: &mut Session, ctx: &CmdCtx, name: Option<&str>) -> Result<Value, CommandError> {
    let doc = s.document(&ctx.actor)?;
    let Some(name) = name else {
        let list: Vec<Value> = doc
            .drawings
            .iter()
            .map(|d| {
                json!({
                    "name": d.name,
                    "size": d.size.keyword(),
                    "title_user_text": d.title,
                    "items": d.items.iter().map(DrawingItem::name).collect::<Vec<_>>(),
                })
            })
            .collect();
        return Ok(json!({ "drawings": list }));
    };
    let d = doc.drawings.iter().find(|d| d.name == name).cloned();
    let d = d.ok_or_else(|| CommandError::not_found(format!("no drawing '{name}'")))?;
    let sc = scene(s, ctx, name)?;
    let round = |v: f64| libm::round(v * 1000.0) / 1000.0;
    let views: Vec<Value> = sc
        .views
        .iter()
        .map(|v| match &v.error {
            Some(e) => json!({ "name": v.name, "error": e }),
            None => json!({
                "name": v.name,
                "box_mm": [round(v.min[0]), round(v.min[1]), round(v.max[0]), round(v.max[1])],
                "scale": gusset_drawing::scale_text(v.scale),
                "off_sheet": v.min[0] < 0.0 || v.min[1] < 0.0 || v.max[0] > sc.size[0] || v.max[1] > sc.size[1],
            }),
        })
        .collect();
    let notes: Vec<Value> = d
        .items
        .iter()
        .filter_map(|i| match i {
            DrawingItem::Note(n) => Some(json!({ "name": n.name, "text_user_text": n.text })),
            _ => None,
        })
        .collect();
    Ok(json!({
        "name": d.name,
        "size": d.size.keyword(),
        "sheet_mm": sc.size,
        "projection": if d.projection == Projection::First { "first" } else { "third" },
        "views": views,
        "notes": notes,
        "dimensions": sc.dims.iter().map(|d| match &d.error {
            Some(e) => json!({ "name": d.name, "dangling": true, "error": e }),
            None => json!({ "name": d.name, "value": d.value.map(round), "text_user_text": d.text }),
        }).collect::<Vec<_>>(),
        "issues": sc.issues,
        "text_user_text": gusset_doc::drawing::print_drawing_object(&d),
    }))
}

/// Lists the drawings, or describes one as built from the model.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryDrawing {
    /// Omit to list the drawings.
    #[serde(default)]
    pub drawing: Option<String>,
}

impl Command for QueryDrawing {
    const ID: &'static str = "query.drawing";
    const TITLE: &'static str = "Drawing";
    const DESCRIPTION: &'static str = "List drawings, or one as built: view boxes (sheet mm), scales, measured or dangling dimensions, notes, issues, text.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        query(s, ctx, self.drawing.as_deref())
    }
}

/// Writes a drawing as SVG or DXF into the project folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingExport {
    pub drawing: String,
    /// File name in the project folder: .svg, .dxf or .pdf.
    pub path: String,
    /// DXF: r2000 (default) or r12.
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for DrawingExport {
    const ID: &'static str = "drawing.export";
    const TITLE: &'static str = "Export drawing";
    const DESCRIPTION: &'static str =
        "Write a drawing as SVG, DXF (a layer per line type) or PDF to a file in the project folder.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let path =
            crate::paths::export_target(s.project_dir.as_deref(), &self.path, &ctx.actor, &["svg", "dxf", "pdf"])?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        let format = match ext.as_deref() {
            Some(f @ ("dxf" | "svg" | "pdf")) => f.to_string(),
            _ => return Err(CommandError::args("the file name must end in .svg, .dxf or .pdf")),
        };
        let profile = match self.profile.as_deref() {
            None => gusset_drawing::dxf::Profile::default(),
            Some(p) => gusset_drawing::dxf::Profile::from_keyword(p)
                .ok_or_else(|| CommandError::args(format!("unknown DXF profile '{p}'")).hint("use r2000 or r12"))?,
        };
        if path.exists() {
            if !self.overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{}' already exists", self.path))
                    .hint("pass overwrite: true to replace it"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting an exported file"));
            }
        }
        let sc = scene(s, ctx, &self.drawing)?;
        let bytes = match format.as_str() {
            "dxf" => gusset_drawing::dxf::to_dxf(&sc, profile).into_bytes(),
            "pdf" => {
                let doc = s.document(&ctx.actor)?;
                let title = doc.drawings.iter().find(|d| d.name == self.drawing).and_then(|d| d.title.clone());
                gusset_drawing::pdf::to_pdf(&sc, title.as_deref().unwrap_or(&self.drawing))
            }
            _ => gusset_drawing::svg::to_svg(&sc).into_bytes(),
        };
        crate::paths::write_file(&path, &self.path, &bytes)?;
        Ok(json!({ "written": self.path, "bytes": bytes.len(), "issues": sc.issues }))
    }
}
