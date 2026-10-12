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

/// A form for one command.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub command: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
    pub show_more: bool,
    pub error: Option<String>,
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
        _ => FieldKind::Json,
    }
}

/// `distance_mm` → "Distance mm".
pub fn humanize(name: &str) -> String {
    let s = name.replace('_', " ");
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
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
            title: spec.title,
            description: spec.description,
            fields,
            show_more: false,
            error: None,
        }
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
            let v = match f.kind {
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
                    t.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| Value::String(s.into())).collect(),
                ),
                FieldKind::Json => serde_json::from_str(t).map_err(|e| format!("{}: {e}", f.label))?,
            };
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
    ui.add_space(crate::theme::SPACE[2]);
    let has_optional = form.fields.iter().any(|f| !f.required);
    egui::Grid::new(("form", form.command))
        .num_columns(2)
        .spacing([crate::theme::SPACE[3], crate::theme::SPACE[2]])
        .show(ui, |ui| {
            let show_more = form.show_more;
            for f in form.fields.iter_mut().filter(|f| f.required || show_more) {
                ui.label(&f.label).on_hover_text(&f.help);
                let r = match f.kind {
                    FieldKind::Bool => {
                        let mut b = f.value == "true";
                        let r = ui.checkbox(&mut b, "");
                        f.value = b.to_string();
                        r
                    }
                    _ => ui.add(egui::TextEdit::singleline(&mut f.value).desired_width(220.0).hint_text(&f.help)),
                };
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
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
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
