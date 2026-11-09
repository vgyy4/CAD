//! Command dialogs generated from the registry's JSON schemas (DESIGN.md rule 7:
//! essentials first, optional fields under "More").
//!
//! The form keeps every field as text while the user types and converts to JSON only on
//! apply, so a half-typed number never loses input.

use serde_json::{Map, Value};

/// What kind of input a schema property needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Integer,
    Bool,
    /// Comma-separated list of strings.
    StringList,
    /// Comma-separated list of numbers (a CAM holder, tabs).
    NumberList,
    /// Anything else, edited as JSON.
    Json,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub label: String,
    pub help: String,
    pub kind: FieldKind,
    pub required: bool,
    /// Current text (for `Bool`: "true"/"false").
    pub value: String,
}

/// Known values of free-text fields, offered as a dropdown next to the text box. The
/// schemas stay plain strings (the MCP tool list is size-budgeted), so the GUI knows them.
#[must_use]
pub fn suggestions(command: &str, field: &str) -> &'static [&'static str] {
    match (command, field) {
        ("body.appearance", "finish") => &["plastic", "metal", "glass", "rubber"],
        ("body.appearance", "texture") => &["wood", "brushed", "carbon", "speckle", "none"],
        ("cam.toolpath" | "cam.export_gcode", "operation") => &[
            "pocket",
            "pocket_adaptive",
            "pocket_rest",
            "profile_outside",
            "profile_inside",
            "drill",
            "bore",
            "surface_ball",
            "surface_flat",
            "waterline_ball",
            "waterline_flat",
            "steep_ball",
            "steep_flat",
            "pencil_ball",
        ],
        _ => &[],
    }
}

/// The chamfer form's mode switch: equal, two distances, distance and angle.
pub const CHAMFER_MODES: [(&str, &str); 3] = [
    ("Equal", "One distance on both faces"),
    ("Two distances", "distance on the reference face, distance2 on the other"),
    ("Distance and angle", "distance on the reference face, angle from it"),
];

impl Form {
    /// The mode a chamfer form's fields describe: `angle` set = 2, `distance2` set = 1.
    fn chamfer_mode_of_fields(&self) -> usize {
        let set = |n: &str| self.value(n).is_some_and(|v| !v.trim().is_empty());
        if set("angle") {
            2
        } else if set("distance2") {
            1
        } else {
            0
        }
    }

    /// Switches the chamfer form to `mode` (0 = equal, 1 = two distances, 2 = distance and
    /// angle): the fields of the other modes are emptied and the new one gets a starting value.
    pub fn set_chamfer_mode(&mut self, mode: usize) {
        self.mode = mode;
        match mode {
            0 => {
                self.set("distance2", "");
                self.set("angle", "");
                self.set("faces", "");
            }
            1 => {
                self.set("angle", "");
                if self.value("distance2").is_none_or(|v| v.trim().is_empty()) {
                    let d = self.value("distance").unwrap_or_default().trim().to_string();
                    self.set("distance2", if d.is_empty() { "1 mm" } else { &d });
                }
            }
            _ => {
                self.set("distance2", "");
                if self.value("angle").is_none_or(|v| v.trim().is_empty()) {
                    self.set("angle", "45 deg");
                }
            }
        }
    }

    /// Whether the form shows `field` in its current mode (always, except on the chamfer form).
    #[must_use]
    pub fn field_visible(&self, field: &str) -> bool {
        if self.command != "feature.chamfer" {
            return true;
        }
        match field {
            "distance2" => self.mode == 1,
            "angle" => self.mode == 2,
            "faces" => self.mode > 0,
            _ => true,
        }
    }

    /// Whether `field` belongs to the mode switch and so is shown without "More".
    fn in_mode_switch(&self, field: &str) -> bool {
        self.command == "feature.chamfer" && matches!(field, "distance2" | "angle" | "faces")
    }
}

/// A form for one command.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub command: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
    pub show_more: bool,
    /// The chamfer form's mode ([`CHAMFER_MODES`]); unused by other forms.
    pub mode: usize,
    pub error: Option<String>,
    /// Points placed with viewport clicks so far (sketch commands, [`crate::sketch_place`]).
    pub clicks: Vec<crate::sketch_place::Click>,
    /// Submitted with Ctrl+Enter: open the command again after it succeeds.
    pub repeat: bool,
}

fn type_names(prop: &Value) -> Vec<&str> {
    match prop.get("type") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn kind_of(prop: &Value) -> FieldKind {
    let types: Vec<&str> = type_names(prop).into_iter().filter(|t| *t != "null").collect();
    match types.as_slice() {
        ["string"] => FieldKind::Text,
        ["number"] => FieldKind::Number,
        ["integer"] => FieldKind::Integer,
        ["boolean"] => FieldKind::Bool,
        ["array"] if prop.get("items").map(type_names).is_some_and(|t| t == ["string"]) => FieldKind::StringList,
        ["array"] if prop.get("items").map(type_names).is_some_and(|t| t == ["number"]) => FieldKind::NumberList,
        _ => FieldKind::Json,
    }
}

/// `distance_mm` → "Distance mm".
pub fn humanize(name: &str) -> String {
    let s = name.replace('_', " ");
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl Field {
    /// The field's text as a JSON value of its kind (not for empty optional fields).
    pub fn to_value(&self) -> Result<Value, String> {
        let (f, t) = (self, self.value.trim());
        Ok(match f.kind {
            FieldKind::Text => Value::String(f.value.clone()),
            FieldKind::Bool => Value::Bool(t == "true"),
            FieldKind::Number => t
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .ok_or_else(|| format!("{} must be a number", f.label))?,
            FieldKind::Integer => {
                t.parse::<i64>().map(Value::from).map_err(|_| format!("{} must be a whole number", f.label))?
            }
            FieldKind::StringList => Value::Array(
                split_list(t).into_iter().filter(|s| !s.is_empty()).map(|s| Value::String(s.into())).collect(),
            ),
            FieldKind::NumberList => Value::Array(
                t.trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        s.parse::<f64>()
                            .ok()
                            .and_then(serde_json::Number::from_f64)
                            .map(Value::Number)
                            .ok_or_else(|| format!("{}: '{s}' is not a number", f.label))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            FieldKind::Json => serde_json::from_str(t).map_err(|e| format!("{}: {e}", f.label))?,
        })
    }
}

impl Form {
    pub fn new(spec: &gusset_commands::CommandSpec) -> Form {
        let schema = (spec.schema)();
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut fields = Vec::new();
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (name, prop) in props {
                let kind = kind_of(prop);
                let value = match (&kind, prop.get("default")) {
                    (FieldKind::Bool, Some(Value::Bool(b))) => b.to_string(),
                    (FieldKind::Bool, _) => "false".into(),
                    (_, Some(Value::Number(n))) => n.to_string(),
                    (_, Some(Value::String(s))) => s.clone(),
                    _ => String::new(),
                };
                fields.push(Field {
                    name: name.clone(),
                    label: humanize(name),
                    help: prop.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                    kind,
                    required: required.contains(&name.as_str()) || prop.get("x-essential") == Some(&Value::Bool(true)),
                    value,
                });
            }
        }
        // Essentials first, in schema order otherwise.
        fields.sort_by_key(|f| !f.required);
        Form {
            command: spec.id,
            title: crate::i18n::command_title(spec),
            description: spec.description,
            fields,
            show_more: false,
            mode: 0,
            clicks: Vec::new(),
            repeat: false,
            error: None,
        }
    }

    /// The current text of field `name`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|f| f.name == name).map(|f| f.value.as_str())
    }

    pub fn set(&mut self, name: &str, value: &str) -> bool {
        match self.fields.iter_mut().find(|f| f.name == name) {
            Some(f) => {
                value.clone_into(&mut f.value);
                true
            }
            None => false,
        }
    }

    /// Converts the fields to command arguments. Empty optional fields are omitted.
    pub fn to_args(&self) -> Result<Value, String> {
        let mut m = Map::new();
        for f in &self.fields {
            let t = f.value.trim();
            if t.is_empty() && f.kind != FieldKind::Bool {
                if f.required && f.kind != FieldKind::StringList {
                    return Err(format!("{} is required", f.label));
                }
                continue;
            }
            let v = f.to_value()?;
            m.insert(f.name.clone(), v);
        }
        Ok(Value::Object(m))
    }
}

/// Draws a form; returns true when the user pressed Apply (or Enter in a field).
pub fn show(ui: &mut egui::Ui, form: &mut Form) -> bool {
    use crate::i18n::tr;
    let mut submit = false;
    ui.label(egui::RichText::new(form.description).color(ui.visuals().widgets.noninteractive.fg_stroke.color));
    if crate::sketch_place::clicks_needed(form.command).is_some() {
        ui.label(egui::RichText::new(crate::i18n::tr("sketch.place_hint")).small().weak());
    }
    ui.add_space(crate::theme::SPACE[2]);
    if form.command == "feature.chamfer" {
        // A form filled from an existing chamfer starts in the mode its fields describe.
        if form.mode == 0 {
            form.mode = form.chamfer_mode_of_fields();
        }
        let mode = form.mode;
        ui.horizontal(|ui| {
            for (i, (label, help)) in CHAMFER_MODES.iter().enumerate() {
                if ui.selectable_label(mode == i, *label).on_hover_text(*help).clicked() && mode != i {
                    form.set_chamfer_mode(i);
                }
            }
        });
        ui.add_space(crate::theme::SPACE[1]);
    }
    let has_optional = form.fields.iter().any(|f| !f.required && !form.in_mode_switch(&f.name));
    egui::Grid::new(("form", form.command))
        .num_columns(2)
        .spacing([crate::theme::SPACE[3], crate::theme::SPACE[2]])
        .show(ui, |ui| {
            let show_more = form.show_more;
            let shown: Vec<bool> = form
                .fields
                .iter()
                .map(|f| form.field_visible(&f.name) && (f.required || show_more || form.in_mode_switch(&f.name)))
                .collect();
            for (f, _) in form.fields.iter_mut().zip(shown).filter(|(_, v)| *v) {
                ui.label(&f.label).on_hover_text(&f.help);
                let r = match f.kind {
                    FieldKind::Bool => {
                        let mut b = f.value == "true";
                        let r = ui.checkbox(&mut b, "");
                        f.value = b.to_string();
                        r
                    }
                    // A colour picker beside the "#rrggbb" text (ADR 0045).
                    _ if form.command == "body.appearance" && f.name == "color" => {
                        ui.horizontal(|ui| {
                            let r = ui
                                .add(egui::TextEdit::singleline(&mut f.value).desired_width(180.0).hint_text(&f.help));
                            let mut rgb =
                                gusset_doc::text::parse_color(f.value.trim()).map_or([0xb4, 0xbc, 0xc8], |c| {
                                    let [_, r, g, b] = c.to_be_bytes();
                                    [r, g, b]
                                });
                            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                                f.value = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                            }
                            r
                        })
                        .inner
                    }
                    _ => {
                        let known = suggestions(form.command, &f.name);
                        if known.is_empty() {
                            ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(220.0).hint_text(&f.help))
                        } else {
                            ui.horizontal(|ui| {
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut f.value).desired_width(180.0).hint_text(&f.help),
                                );
                                egui::ComboBox::from_id_salt(("form-suggest", form.command, &f.name))
                                    .selected_text("")
                                    .width(24.0)
                                    .show_ui(ui, |ui| {
                                        for k in known {
                                            if ui.selectable_label(f.value == *k, *k).clicked() {
                                                (*k).clone_into(&mut f.value);
                                            }
                                        }
                                    });
                                r
                            })
                            .inner
                        }
                    }
                };
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                    // Ctrl+Enter confirms and repeats (DESIGN.md §1.18).
                    form.repeat = ui.input(|i| i.modifiers.command);
                }
                ui.end_row();
            }
        });
    if has_optional && crate::icons::disclosure_button(ui, form.show_more, tr("dialog.more")).clicked() {
        form.show_more = !form.show_more;
    }
    if let Some(e) = &form.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    ui.add_space(crate::theme::SPACE[2]);
    // The primary action, on the right in the accent color.
    // One row high: in the tall properties panel a plain layout would centre it vertically.
    let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if crate::theme::primary_button(ui, tr("dialog.ok"), 72.0).clicked() {
            submit = true;
        }
    });
    submit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_gets_a_form_that_builds_args() {
        for spec in gusset_commands::registry() {
            let form = Form::new(spec);
            // Forms with only optional fields must produce valid (empty) args.
            if form.fields.iter().all(|f| !f.required) {
                assert!(form.to_args().is_ok(), "{}", spec.id);
            }
        }
    }

    #[test]
    fn the_cam_operation_field_offers_the_known_operations() {
        let ops = suggestions("cam.toolpath", "operation");
        assert!(ops.contains(&"waterline_ball") && ops.contains(&"pocket_rest") && ops.contains(&"steep_flat"));
        assert_eq!(ops, suggestions("cam.export_gcode", "operation"));
        assert!(suggestions("cam.toolpath", "sketch").is_empty());
    }

    #[test]
    fn the_cam_holder_is_typed_as_numbers_separated_by_commas() {
        let spec = gusset_commands::find("cam.toolpath").unwrap();
        let mut form = Form::new(spec);
        let h = form.fields.iter_mut().find(|f| f.name == "holder").unwrap();
        assert_eq!(h.kind, FieldKind::NumberList);
        h.value = "6, 10, 20, 12".into();
        assert_eq!(h.to_value().unwrap(), serde_json::json!([6.0, 10.0, 20.0, 12.0]));
        h.value = "[6,10]".into();
        assert_eq!(h.to_value().unwrap(), serde_json::json!([6.0, 10.0]));
        h.value = "6, ten".into();
        assert!(h.to_value().unwrap_err().contains("'ten' is not a number"));
    }

    #[test]
    fn chamfer_form_mode_switch_shows_and_clears_the_right_fields() {
        let spec = gusset_commands::registry().iter().find(|s| s.id == "feature.chamfer").unwrap();
        let mut f = Form::new(spec);
        f.set("edges", "edge(A/cap(end), A/side(S/l1))");
        f.set("distance", "2 mm");
        // Equal: the legs and faces are hidden and not sent.
        assert!(["distance2", "angle", "faces"].iter().all(|n| !f.field_visible(n)));
        assert_eq!(
            f.to_args().unwrap(),
            serde_json::json!({"edges": ["edge(A/cap(end), A/side(S/l1))"], "distance": "2 mm"})
        );
        // Two distances: distance2 and faces show; distance2 starts at the distance.
        f.set_chamfer_mode(1);
        assert!(f.field_visible("distance2") && f.field_visible("faces") && !f.field_visible("angle"));
        assert_eq!(f.value("distance2"), Some("2 mm"));
        f.set("distance2", "3 mm");
        assert_eq!(f.to_args().unwrap()["distance2"], "3 mm");
        // Distance and angle replaces distance2 by an angle.
        f.set_chamfer_mode(2);
        assert!(f.field_visible("angle") && !f.field_visible("distance2"));
        let args = f.to_args().unwrap();
        assert_eq!((args.get("angle"), args.get("distance2")), (Some(&serde_json::json!("45 deg")), None));
        // Back to equal clears everything.
        f.set("faces", "A/cap(end)");
        f.set_chamfer_mode(0);
        let args = f.to_args().unwrap();
        assert!(["distance2", "angle", "faces"].iter().all(|n| args.get(n).is_none()), "{args}");
        // A form for another command is unaffected.
        let other = Form::new(gusset_commands::registry().iter().find(|s| s.id == "feature.fillet").unwrap());
        assert!(other.field_visible("angle"));
    }

    #[test]
    fn gear_form_shows_the_five_essentials_first_with_defaults() {
        let spec = gusset_commands::find("generate.gear").unwrap();
        let form = Form::new(spec);
        let first: Vec<&str> = form.fields.iter().take_while(|f| f.required).map(|f| f.name.as_str()).collect();
        assert_eq!(first.len(), 5, "{first:?}");
        assert!(first.contains(&"module") && first.contains(&"teeth") && first.contains(&"bore"), "{first:?}");
        let args = form.to_args().unwrap();
        assert_eq!(args["module"], "2 mm");
        assert_eq!(args["teeth"], "20");
    }

    #[test]
    fn extrude_form_essentials_and_conversion() {
        let spec = gusset_commands::find("feature.extrude").unwrap();
        let mut form = Form::new(spec);
        let first: Vec<&str> = form.fields.iter().take_while(|f| f.required).map(|f| f.name.as_str()).collect();
        assert!(first.contains(&"sketch") && first.contains(&"distance"), "{first:?}");
        assert!(form.to_args().unwrap_err().contains("required"));
        assert!(form.set("sketch", "Sketch1") && form.set("distance", "10 mm"));
        form.set("regions", "L1, L2");
        form.set("symmetric", "true");
        let args = form.to_args().unwrap();
        assert_eq!(args["distance"], "10 mm");
        assert_eq!(args["regions"], serde_json::json!(["L1", "L2"]));
        assert_eq!(args["symmetric"], true);
        assert!(args.get("name").is_none());
    }
}

/// A comma-separated list whose items may contain commas inside brackets or quotes: face and
/// edge names such as `Fillet1/blend(edge(A/top, A/side))` stay whole (review of face colours).
fn split_list(t: &str) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0i32, false, 0usize);
    for (i, c) in t.char_indices() {
        match c {
            '"' => quote = !quote,
            '(' | '[' | '{' if !quote => depth += 1,
            ')' | ']' | '}' if !quote => depth -= 1,
            ',' if !quote && depth <= 0 => {
                out.push(t.get(start..i).unwrap_or_default().trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(t.get(start..).unwrap_or_default().trim());
    out
}

#[cfg(test)]
mod split_tests {
    #[test]
    fn list_items_keep_their_inner_commas() {
        assert_eq!(super::split_list("a, b,c"), ["a", "b", "c"]);
        assert_eq!(
            super::split_list("Fillet1/blend(edge(A/top, A/side)), Extrude1/cap(start, r)"),
            ["Fillet1/blend(edge(A/top, A/side))", "Extrude1/cap(start, r)"]
        );
        assert_eq!(super::split_list("\"x, y\", z"), ["\"x, y\"", "z"]);
        assert_eq!(super::split_list(""), [""]);
    }
}
