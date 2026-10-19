//! 2D drawings (ADR 0032): sheets of views, dimensions and notes in the document, their
//! query and their export to SVG and DXF.

use super::*;
use gusset_doc::drawing::{
    DimDef, DimDirection, DimKind, Drawing, DrawingItem, NoteDef, Projection, SheetSize, Side, ViewDef, ViewDir,
    ViewPlacement,
};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<DrawingCreate>(),
        spec::<DrawingEdit>(),
        spec::<DrawingAddView>(),
        spec::<DrawingEditView>(),
        spec::<DrawingAddDimension>(),
        spec::<DrawingAddNote>(),
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
    pub title: Option<String>,
    /// Also add front, top, right and iso views of every body.
    #[serde(default)]
    pub standard_views: bool,
}

impl Command for DrawingCreate {
    const ID: &'static str = "drawing.create";
    const TITLE: &'static str = "New drawing";
    const DESCRIPTION: &'static str = "Create a 2D drawing sheet (ISO/ANSI size, first or third angle, scale, title block). standard_views adds front, top, right and iso views.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let size = self.size.as_deref().map(size_of).transpose()?.unwrap_or(SheetSize::A3);
        let projection = self.projection.as_deref().map(projection_of).transpose()?.unwrap_or(Projection::First);
        let scale = opt_expr("scale", &self.scale)?;
        edit(s, ctx, "New drawing", |doc, _| {
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
            d.size = size;
            d.landscape = !self.portrait;
            d.projection = projection;
            d.scale = scale;
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
                    }))
                };
                let projected = |n: &str, side| {
                    DrawingItem::View(ViewDef {
                        name: n.into(),
                        placement: ViewPlacement::Projected { of: "Front".into(), side, gap: None },
                        scale: None,
                        hidden: None,
                        tangent: false,
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
        })
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
    const DESCRIPTION: &'static str =
        "Change a drawing's size, orientation, projection, scale or title block fields (\"\" clears a field).";
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
        edit(s, ctx, "Edit drawing", |doc, _| {
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
            set_text(&mut d.title, self.title);
            set_text(&mut d.number, self.number);
            set_text(&mut d.material, self.material);
            set_text(&mut d.author, self.author);
            Ok(json!({ "drawing": d.name }))
        })
    }
}

/// Adds a view: a base view (`from`, `at`) or one projected from another view (`of`, `side`).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingAddView {
    pub drawing: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Base view: front, back, top, bottom, left, right, iso or iso_back.
    #[serde(default)]
    pub from: Option<String>,
    /// Base view: centre on the sheet in mm from the lower-left corner.
    #[serde(default)]
    pub at: Option<[String; 2]>,
    /// Base view: bodies to show (default all).
    #[serde(default)]
    pub bodies: Vec<String>,
    /// Projected view: the parent view and the side it is seen from.
    #[serde(default)]
    pub of: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub gap: Option<String>,
    #[serde(default)]
    pub scale: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub tangent: bool,
}

impl Command for DrawingAddView {
    const ID: &'static str = "drawing.add_view";
    const TITLE: &'static str = "Add view";
    const DESCRIPTION: &'static str = "Add a drawing view: base (from, at in sheet mm, bodies) or projected (of, side), placed by the projection angle. Returns its sheet box.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let placement = match (&self.of, &self.from) {
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
        let drawing = self.drawing.clone();
        let created = std::cell::RefCell::new(String::new());
        edit_post(
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
                }));
                *created.borrow_mut() = name.clone();
                Ok(json!({ "view": name }))
            },
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
}

impl Command for DrawingEditView {
    const ID: &'static str = "drawing.edit_view";
    const TITLE: &'static str = "Edit view";
    const DESCRIPTION: &'static str =
        "Move a view (at for base views, gap for projected ones) or change its scale, hidden or tangent lines.";
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
        edit(s, ctx, "Edit view", |doc, _| {
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
                (ViewPlacement::Base { at: slot, .. }, Some(a), None) => *slot = a,
                (ViewPlacement::Projected { gap: slot, .. }, None, Some(g)) => *slot = Some(g),
                (_, None, None) => {}
                (ViewPlacement::Base { .. }, _, Some(_)) => {
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
            Ok(json!({ "view": v.name }))
        })
    }
}

/// Adds a driven dimension on model edges, measured in a view.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingAddDimension {
    pub drawing: String,
    pub view: String,
    /// length (one line), distance (two edges), diameter/radius (one circle), angle (two lines).
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
        edit(s, ctx, "Add dimension", |doc, _| {
            let d = find(doc, &self.drawing)?;
            if d.view(&self.view).is_none() {
                return Err(CommandError::not_found(format!("drawing '{}' has no view '{}'", d.name, self.view)));
            }
            let name = item_name(d, self.name, "D")?;
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
    const DESCRIPTION: &'static str = "Delete a drawing's view, dimension or note (item), or the whole drawing. A view that others are projected from or measured in cannot go first.";
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
                    DrawingItem::View(ViewDef { placement: ViewPlacement::Projected { of, .. }, .. }) => *of == item,
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
    let doc = s.document(&ctx.actor)?;
    let d = doc
        .drawings
        .iter()
        .find(|d| d.name == name)
        .cloned()
        .ok_or_else(|| CommandError::not_found(format!("no drawing '{name}'")).hint("see query_drawing"))?;
    let ev = s.evaluate(&ctx.actor)?;
    let kernel = s.kernel().clone();
    let cx = gusset_drawing::Context { ev: &ev, kernel: kernel.as_ref() };
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
    const DESCRIPTION: &'static str = "List drawings, or one drawing as built: each view's box on the sheet (mm) and scale, off_sheet, view errors, notes, issues, and its text.";
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
    /// File name in the project folder: .svg or .dxf.
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for DrawingExport {
    const ID: &'static str = "drawing.export";
    const TITLE: &'static str = "Export drawing";
    const DESCRIPTION: &'static str =
        "Write a drawing as SVG (mm) or DXF (R12, layers per line type) to a file in the project folder.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let path = crate::paths::export_target(s.project_dir.as_deref(), &self.path, &ctx.actor, &["svg", "dxf"])?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        let dxf = match ext.as_deref() {
            Some("dxf") => true,
            Some("svg") => false,
            _ => return Err(CommandError::args("the file name must end in .svg or .dxf")),
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
        let text = if dxf {
            gusset_drawing::dxf::to_dxf(&sc, gusset_drawing::dxf::Profile::R12)
        } else {
            gusset_drawing::svg::to_svg(&sc)
        };
        crate::paths::write_file(&path, &self.path, text.as_bytes())?;
        Ok(json!({ "written": self.path, "bytes": text.len(), "issues": sc.issues }))
    }
}
