//! Net commands (ADR 0047 phase 3): pad nets, renaming and merging nets, net classes
//! (kept in the KiCad project file beside the board, created when missing) and copper
//! zones, so a board started in Gusset gets nets to route. Net names reach agents only in
//! `_user_text` fields.

use super::pcb::no_board;
use super::pcb_edit::commit_board;
use super::*;
use crate::pcb_session;
use gusset_eval::BlobSource;
use gusset_pcb::board::{Hatch, ZoneConnect, ZoneSettings};
use gusset_pcb::nets::NewZone;
use gusset_pcb::{Board, Limits};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<PcbSetNet>(), spec::<PcbRenameNet>(), spec::<PcbNetClass>(), spec::<PcbAddZone>()]
}

fn pcb_err(e: gusset_pcb::board::Error) -> CommandError {
    match e {
        gusset_pcb::board::Error::Edit(m) => CommandError::args(m),
        e => CommandError::new("io", format!("KiCad board: {e}")),
    }
}

fn limits() -> Limits {
    Limits::default()
}

/// The open board's project file as JSON (an empty KiCad 6+ project when it has none).
fn project_json(s: &Session, open: &pcb_session::OpenBoard) -> Value {
    open.project_hash
        .as_ref()
        .and_then(|h| s.blobs().blob(h))
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| {
            json!({
                "meta": { "filename": open.file.replace(".kicad_pcb", ".kicad_pro"), "version": 1 },
                "net_settings": { "classes": [{ "name": "Default" }], "meta": { "version": 3 } },
            })
        })
}

/// The `net_settings` object of a project, created when missing.
fn net_settings(pro: &mut Value) -> Result<&mut serde_json::Map<String, Value>, CommandError> {
    let bad = || CommandError::new("io", "the project file is not a JSON object");
    let root = pro.as_object_mut().ok_or_else(bad)?;
    let ns = root.entry("net_settings").or_insert_with(|| json!({}));
    if !ns.is_object() {
        *ns = json!({});
    }
    ns.as_object_mut().ok_or_else(bad)
}

/// Sets the net of pads.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbSetNet {
    /// Pads as REF.PAD (R1.2).
    pub pads: Vec<String>,
    /// Net name; omit for none.
    #[serde(default)]
    pub net: Option<String>,
}

impl Command for PcbSetNet {
    const ID: &'static str = "pcb.set_net";
    const TITLE: &'static str = "Set pad net";
    const DESCRIPTION: &'static str = "Assign pads of the open board to a net (new names create the net).";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.pads.is_empty() || self.pads.len() > 10_000 {
            return Err(CommandError::args("name 1 to 10000 pads"));
        }
        let pads: Vec<(String, String)> = self
            .pads
            .iter()
            .map(|p| {
                p.rsplit_once('.')
                    .map(|(r, n)| (r.to_string(), n.to_string()))
                    .filter(|x| !x.0.is_empty())
                    .ok_or_else(|| CommandError::args(format!("'{}' is not REF.PAD", crate::paths::display_name(p))))
            })
            .collect::<Result<_, _>>()?;
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut b = open.board.clone();
        let changed =
            b.set_pad_nets(&pads, self.net.as_deref().filter(|n| !n.is_empty()), &limits()).map_err(|e| match e {
                gusset_pcb::board::Error::Edit(m) if m.contains("no footprint") || m.contains("no pad") => {
                    CommandError::not_found(m)
                }
                e => pcb_err(e),
            })?;
        let extra = json!({ "pads_changed": changed, "net_user_text": self.net });
        commit_board(s, ctx, "Set pad net", open.file.clone(), &b, None, extra)
    }
}

/// Renames a net, or merges it into another.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbRenameNet {
    pub from: String,
    /// An existing net: the two merge.
    pub to: String,
}

impl Command for PcbRenameNet {
    const ID: &'static str = "pcb.rename_net";
    const TITLE: &'static str = "Rename net";
    const DESCRIPTION: &'static str =
        "Rename a net of the open board on pads, tracks, vias, zones and class assignments; an existing name merges.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let merged = open.board.nets.iter().any(|n| n.name == self.to);
        let mut b: Board = open.board.clone();
        let n = b.rename_net(&self.from, &self.to, &limits()).map_err(|e| match e {
            gusset_pcb::board::Error::Edit(m) if m.contains("no such net") => CommandError::not_found(m),
            e => pcb_err(e),
        })?;
        // Class assignments in the project follow the name.
        let project = match open.project_hash.as_ref() {
            Some(_) => {
                let mut pro = project_json(s, &open);
                let ns = net_settings(&mut pro)?;
                if let Some(a) = ns.get_mut("netclass_assignments").and_then(Value::as_object_mut)
                    && let Some(c) = a.remove(&self.from)
                {
                    a.entry(self.to.clone()).or_insert(c);
                }
                if let Some(cs) = ns.get_mut("classes").and_then(Value::as_array_mut) {
                    for c in cs.iter_mut() {
                        if let Some(nets) = c.get_mut("nets").and_then(Value::as_array_mut) {
                            for x in nets.iter_mut().filter(|x| x.as_str() == Some(self.from.as_str())) {
                                *x = json!(self.to);
                            }
                        }
                    }
                }
                let bytes = serde_json::to_vec_pretty(&pro).map_err(|e| CommandError::new("io", e.to_string()))?;
                Some(Some(s.put_blob(&bytes).to_hex()))
            }
            None => None,
        };
        let extra = json!({ "references_changed": n, "merged": merged });
        commit_board(s, ctx, "Rename net", open.file.clone(), &b, project, extra)
    }
}

/// Creates, edits or deletes a net class and assigns nets to it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbNetClass {
    /// Class name (Default sets the board's).
    pub name: String,
    /// mm.
    #[serde(default)]
    pub clearance: Option<f64>,
    #[serde(default)]
    pub track_width: Option<f64>,
    #[serde(default)]
    pub via_diameter: Option<f64>,
    #[serde(default)]
    pub via_drill: Option<f64>,
    #[serde(default)]
    pub nets: Vec<String>,
    /// Net name patterns (* and ?).
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub delete: bool,
}

impl Command for PcbNetClass {
    const ID: &'static str = "pcb.net_class";
    const TITLE: &'static str = "Net class";
    const DESCRIPTION: &'static str = "Set a net class's clearance, track width and via; assign nets (.kicad_pro).";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err(CommandError::args("a class name is 1 to 100 characters"));
        }
        let sizes = [
            ("clearance", self.clearance),
            ("track_width", self.track_width),
            ("via_diameter", self.via_diameter),
            ("via_drill", self.via_drill),
        ];
        if sizes.iter().any(|(_, v)| v.is_some_and(|v| !(v.is_finite() && v > 0.0 && v < 100.0))) {
            return Err(CommandError::args("sizes are above 0 and below 100 mm"));
        }
        if let (Some(d), Some(h)) = (self.via_diameter, self.via_drill)
            && h >= d
        {
            return Err(CommandError::args("the via drill must be smaller than the via"));
        }
        if self.nets.len() + self.patterns.len() > 100_000 {
            return Err(CommandError::args("at most 100000 nets and patterns"));
        }
        if self.delete && name == "Default" {
            return Err(CommandError::args("the Default class stays"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut pro = project_json(s, &open);
        let ns = net_settings(&mut pro)?;
        let classes = ns.entry("classes").or_insert_with(|| json!([]));
        if !classes.is_array() {
            *classes = json!([]);
        }
        let list = classes.as_array_mut().ok_or_else(|| CommandError::new("io", "bad project file"))?;
        let at = list.iter().position(|c| c.get("name").and_then(Value::as_str) == Some(name));
        if self.delete {
            let i = at.ok_or_else(|| CommandError::not_found("no such net class"))?;
            list.remove(i);
            for key in ["netclass_assignments"] {
                if let Some(a) = ns.get_mut(key).and_then(Value::as_object_mut) {
                    a.retain(|_, c| c.as_str() != Some(name));
                }
            }
            if let Some(p) = ns.get_mut("netclass_patterns").and_then(Value::as_array_mut) {
                p.retain(|x| x.get("netclass").and_then(Value::as_str) != Some(name));
            }
        } else {
            let class = match at {
                Some(i) => list.get_mut(i),
                None => {
                    list.push(json!({ "name": name }));
                    list.last_mut()
                }
            }
            .and_then(Value::as_object_mut)
            .ok_or_else(|| CommandError::new("io", "bad project file"))?;
            for (k, v) in sizes {
                if let Some(v) = v {
                    class.insert(k.into(), json!(v));
                }
            }
            if !self.nets.is_empty() {
                // The class's own net list is the older way; assignments are KiCad 8+.
                let a = ns.entry("netclass_assignments").or_insert_with(|| json!({}));
                if !a.is_object() {
                    *a = json!({});
                }
                if let Some(a) = a.as_object_mut() {
                    for n in &self.nets {
                        a.insert(n.clone(), json!(name));
                    }
                }
            }
            if !self.patterns.is_empty() {
                let p = ns.entry("netclass_patterns").or_insert_with(|| json!([]));
                if !p.is_array() {
                    *p = json!([]);
                }
                if let Some(p) = p.as_array_mut() {
                    for x in &self.patterns {
                        p.push(json!({ "pattern": x, "netclass": name }));
                    }
                }
            }
        }
        let bytes = serde_json::to_vec_pretty(&pro).map_err(|e| CommandError::new("io", e.to_string()))?;
        let hash = s.put_blob(&bytes).to_hex();
        let label = if self.delete { "Delete net class" } else { "Net class" };
        let extra = json!({ "class_user_text": name, "deleted": self.delete });
        let out = commit_board(s, ctx, label, open.file.clone(), &open.board, Some(Some(hash)), extra)?;
        Ok(out)
    }
}

/// Hatch grid of a zone.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ZoneHatch {
    /// Copper bar width, mm.
    pub thickness: f64,
    /// Hole size, mm.
    pub gap: f64,
    /// Degrees.
    #[serde(default)]
    pub angle: f64,
}

/// Adds a copper zone, filled at once.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbAddZone {
    /// Polygon, mm.
    pub outline: Vec<[f64; 2]>,
    /// More polygons of the same zone.
    #[serde(default)]
    pub more_outlines: Vec<Vec<[f64; 2]>>,
    /// Default ["F.Cu"].
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub net: Option<String>,
    /// mm (default 0.5).
    #[serde(default)]
    pub clearance: Option<f64>,
    /// thermal/solid/none (pads).
    #[serde(default)]
    pub connect: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub hatch: Option<ZoneHatch>,
}

impl Command for PcbAddZone {
    const ID: &'static str = "pcb.add_zone";
    const TITLE: &'static str = "Add zone";
    const DESCRIPTION: &'static str = "Add a copper zone (solid or hatched) to the open board and fill the zones.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let mut st = ZoneSettings::default();
        if let Some(c) = self.clearance {
            st.clearance = c;
        }
        st.connect = match self.connect.as_deref() {
            None | Some("thermal") => ZoneConnect::Thermal,
            Some("solid") => ZoneConnect::Solid,
            Some("none") => ZoneConnect::None,
            Some(_) => return Err(CommandError::args("connect is thermal, solid or none")),
        };
        st.priority = self.priority.unwrap_or(0).clamp(-1_000_000, 1_000_000);
        if let Some(h) = &self.hatch {
            if !(h.thickness.is_finite() && h.gap.is_finite() && h.angle.is_finite())
                || !(0.05..=100.0).contains(&h.thickness)
                || !(0.05..=100.0).contains(&h.gap)
            {
                return Err(CommandError::args("hatch thickness and gap are 0.05 to 100 mm"));
            }
            st.hatch = Some(Hatch { thickness: h.thickness, gap: h.gap, angle: h.angle, min_hole_area: 0.3 });
        }
        let layers = if self.layers.is_empty() { vec!["F.Cu".to_string()] } else { self.layers.clone() };
        let z = NewZone {
            outline: self.outline.clone(),
            more_outlines: self.more_outlines.clone(),
            layers,
            net: self.net.clone().filter(|n| !n.is_empty()),
            settings: st,
        };
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut b = open.board.clone();
        let index = b.add_zone(&z, &limits()).map_err(pcb_err)?;
        // Filled at once, so it shows and plots.
        let filled = gusset_pcb::zones::fill(&b, &open.rules, Some(&[index]));
        b.set_zone_fills(&filled.fills, &limits()).map_err(pcb_err)?;
        let area: f64 = filled.fills.iter().map(gusset_pcb::zones::fill_area).sum();
        let extra = json!({
            "zone": index,
            "filled_mm2": (area * 100.0).round() / 100.0,
            "notes_user_text": filled.notes,
        });
        commit_board(s, ctx, "Add zone", open.file.clone(), &b, None, extra)
    }
}
