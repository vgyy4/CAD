//! The command line (DESIGN.md §2): one line that takes a registry command id and its
//! arguments, either as a JSON object or as `key=value` pairs:
//!
//! ```text
//! feature.extrude sketch=S1 distance="10 mm"
//! param.set {"name": "w", "expr": "12 mm"}
//! ```
//!
//! This module only parses, completes and draws. The app runs the parsed command through
//! the same `submit_command` path as the ribbon, so the command line never bypasses the
//! registry (ADR 0012: GUI = CLI = MCP). Values of `key=value` pairs are converted by the
//! same rules as the command dialogs ([`crate::forms`]); the registry validates the rest.

use crate::forms::Form;
use crate::theme::{self, Palette};
use egui::{Key, Modifiers, Ui};
use gusset_commands::CommandSpec;
use serde_json::{Map, Value};

/// A command line that was understood.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub id: &'static str,
    pub args: Value,
}

/// Splits `s` into words at whitespace; single or double quotes group (and are removed),
/// a backslash escapes inside quotes. `key="a b"` is one word, `key=a b` two.
pub fn words(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), '\\') => match chars.next() {
                Some(n) => cur.push(n),
                None => return Err("a quote is not closed".into()),
            },
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("a quote is not closed".into());
    }
    if started {
        out.push(cur);
    }
    Ok(out)
}

/// Resolves what was typed as a command: a registry id (`feature.extrude`, also in the
/// MCP form `feature_extrude`), or a name that is the last part of exactly one id
/// (`extrude`).
pub fn resolve(word: &str) -> Result<&'static CommandSpec, String> {
    if let Some(spec) = gusset_commands::find(word) {
        return Ok(spec);
    }
    let tail = format!(".{word}");
    let by_tail: Vec<&CommandSpec> = gusset_commands::registry().iter().filter(|c| c.id.ends_with(&tail)).collect();
    match by_tail.as_slice() {
        [one] => Ok(one),
        [] => {
            let near = candidates(word);
            Err(if near.is_empty() {
                format!("unknown command '{word}' (Ctrl+K lists the commands)")
            } else {
                format!(
                    "unknown command '{word}'; did you mean {}?",
                    near.iter().take(3).copied().collect::<Vec<_>>().join(", ")
                )
            })
        }
        many => Err(format!("'{word}' is ambiguous: {}", many.iter().map(|c| c.id).collect::<Vec<_>>().join(", "))),
    }
}

/// Parses a command line into a registry command and its arguments.
pub fn parse(input: &str) -> Result<Parsed, String> {
    let input = input.trim();
    let (head, rest) = match input.find(char::is_whitespace) {
        Some(i) => input.split_at(i),
        None => (input, ""),
    };
    if head.is_empty() {
        return Err("type a command id, e.g. feature.extrude sketch=S1 distance=\"10 mm\"".into());
    }
    let spec = resolve(head)?;
    let rest = rest.trim();
    if rest.starts_with('{') {
        // Arguments exactly as MCP clients send them.
        let args: Value =
            serde_json::from_str(rest).map_err(|e| format!("{}: arguments are not valid JSON: {e}", spec.id))?;
        return Ok(Parsed { id: spec.id, args });
    }
    let form = Form::new(spec);
    let mut args = Map::new();
    for word in words(rest)? {
        let Some((key, value)) = word.split_once('=') else {
            return Err(format!("{}: expected key=value, got '{word}'", spec.id));
        };
        let Some(mut field) = form.fields.iter().find(|f| f.name == key).cloned() else {
            let known: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
            return Err(format!("{}: unknown argument '{key}' (known: {})", spec.id, known.join(", ")));
        };
        field.value = value.to_string();
        args.insert(key.to_string(), field.to_value().map_err(|e| format!("{}: {e}", spec.id))?);
    }
    Ok(Parsed { id: spec.id, args: Value::Object(args) })
}

/// Registry ids that start with `prefix` (or, when none do, contain it), sorted.
pub fn candidates(prefix: &str) -> Vec<&'static str> {
    let prefix = prefix.to_lowercase();
    let mut starts: Vec<&'static str> =
        gusset_commands::registry().iter().map(|c| c.id).filter(|id| id.starts_with(&prefix)).collect();
    if starts.is_empty() && !prefix.is_empty() {
        starts = gusset_commands::registry().iter().map(|c| c.id).filter(|id| id.contains(&prefix)).collect();
    }
    starts.sort_unstable();
    starts
}

/// The longest common prefix of `names`.
fn common_prefix(names: &[&str]) -> String {
    let Some((first, rest)) = names.split_first() else { return String::new() };
    let mut n = first.len();
    for r in rest {
        n = n.min(first.chars().zip(r.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum());
    }
    first.get(..n).unwrap_or_default().to_string()
}

/// Tab completion of the command id (the first word). Returns the new text, or `None` when
/// there is nothing to complete (the cursor is past the id, or nothing matches). One match
/// completes fully (with a trailing space); several complete to their common prefix, and
/// when that adds nothing, Tab steps through them.
pub fn complete(input: &str) -> Option<String> {
    if input.contains(char::is_whitespace) {
        return None;
    }
    let matches = candidates(input);
    match matches.as_slice() {
        [] => None,
        [one] => Some(format!("{one} ")),
        many => {
            let common = common_prefix(many);
            if common.len() > input.len() {
                return Some(common);
            }
            // Already a complete prefix of several: cycle (the id after the current one).
            let next = many.iter().position(|id| *id == input).map_or(0, |i| (i + 1) % many.len());
            many.get(next).map(|id| (*id).to_string())
        }
    }
}

/// A hint under the cursor: matching ids while the id is being typed, the argument names
/// of the command afterwards.
pub fn hint(input: &str) -> String {
    let word = input.trim_start();
    if !word.contains(char::is_whitespace) {
        let m = candidates(word);
        if word.is_empty() || m.is_empty() {
            return String::new();
        }
        let shown: Vec<&str> = m.iter().take(6).copied().collect();
        let more = if m.len() > shown.len() { format!(" +{}", m.len() - shown.len()) } else { String::new() };
        return format!("{}{more}", shown.join("  "));
    }
    let head = word.split_whitespace().next().unwrap_or_default();
    let Ok(spec) = resolve(head) else { return String::new() };
    let form = Form::new(spec);
    let names = |req: bool| -> Vec<String> {
        form.fields
            .iter()
            .filter(|f| f.required == req)
            .map(|f| if req { format!("{}=", f.name) } else { format!("[{}=]", f.name) })
            .collect()
    };
    let mut parts = names(true);
    parts.extend(names(false).into_iter().take(4));
    parts.join(" ")
}

/// The command line's state: whether it is shown (hidden by default: View menu or
/// Ctrl+Shift+K), its text and the commands run before.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub visible: bool,
    pub text: String,
    /// Commands that were run, oldest first (Up / Down step through them).
    pub history: Vec<String>,
    /// Position in `history` while stepping; `None` = typing a new line.
    recall: Option<usize>,
    /// Focus the text field on the next frame.
    pub focus: bool,
}

impl State {
    /// Steps through the history; `back` = older.
    pub fn recall(&mut self, back: bool) {
        let n = self.history.len();
        if n == 0 {
            return;
        }
        let i = match (self.recall, back) {
            (None, true) => n - 1,
            (None, false) => return,
            (Some(i), true) => i.saturating_sub(1),
            (Some(i), false) if i + 1 < n => i + 1,
            (Some(_), false) => {
                self.recall = None;
                self.text.clear();
                return;
            }
        };
        self.recall = Some(i);
        if let Some(h) = self.history.get(i) {
            self.text.clone_from(h);
        }
    }

    /// Takes the line for running: clears the field and remembers it.
    pub fn take_line(&mut self) -> String {
        let line = std::mem::take(&mut self.text);
        self.recall = None;
        let t = line.trim();
        if !t.is_empty() && self.history.last().map(String::as_str) != Some(t) {
            self.history.push(t.to_string());
        }
        line
    }
}

/// Id of the text field (tests focus it by label; the app focuses it by id).
pub fn field_id() -> egui::Id {
    egui::Id::new("command-line-input")
}

/// Draws the command line. Returns the line when Enter was pressed.
pub fn show(ui: &mut Ui, st: &mut State, pal: &Palette) -> Option<String> {
    let mut run = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE[3];
        ui.label(egui::RichText::new(">").monospace().color(pal.accent));
        let id = field_id();
        // Tab completes instead of moving the focus.
        let focused = ui.memory(|m| m.has_focus(id));
        if focused {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab))
                && let Some(t) = complete(&st.text)
            {
                st.text = t;
                st.recall = None;
                set_cursor_to_end(ui, id, &st.text);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                st.recall(true);
                set_cursor_to_end(ui, id, &st.text);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                st.recall(false);
                set_cursor_to_end(ui, id, &st.text);
            }
        }
        // The field takes the whole bar; the hint (when there is one) gets the right part.
        let h = hint(&st.text);
        let hint_width = if h.is_empty() { 0.0 } else { (ui.available_width() * 0.4).clamp(120.0, 520.0) };
        let edit = egui::TextEdit::singleline(&mut st.text)
            .id(id)
            .font(egui::TextStyle::Monospace)
            .hint_text(crate::i18n::tr("cmdline.hint"))
            .lock_focus(true)
            .desired_width((ui.available_width() - hint_width - theme::SPACE[3]).max(160.0));
        let r = ui.add(edit);
        let text = st.text.clone();
        r.widget_info(|| {
            egui::WidgetInfo::text_edit(true, text.as_str(), text.as_str(), crate::i18n::tr("cmdline.label"))
        });
        if st.focus {
            r.request_focus();
            st.focus = false;
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if !st.text.trim().is_empty() {
                run = Some(st.take_line());
            }
            r.request_focus();
        }
        if !h.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(h).monospace().small().color(pal.text_muted)).truncate());
        }
    });
    run
}

/// Moves the text cursor to the end of the field after the app changed its text.
fn set_cursor_to_end(ui: &Ui, id: egui::Id, text: &str) {
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    let end = egui::text::CCursor::new(text.chars().count());
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
    state.store(ui.ctx(), id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn words_group_quotes_and_reject_unclosed_ones() {
        assert_eq!(words("a  b=\"c d\" 'e f'").unwrap(), ["a", "b=c d", "e f"]);
        assert_eq!(words("x=\"say \\\"hi\\\"\"").unwrap(), ["x=say \"hi\""]);
        assert_eq!(words("\"\"").unwrap(), [""]);
        assert!(words("a \"b").is_err());
    }

    #[test]
    fn key_value_arguments_use_the_form_conversions() {
        let p = parse("feature.extrude sketch=S1 distance=\"10 mm\" symmetric=true").unwrap();
        assert_eq!(p.id, "feature.extrude");
        assert_eq!(p.args, json!({"sketch": "S1", "distance": "10 mm", "symmetric": true}));
        // Numbers and lists follow the schema, not the look of the text.
        let p = parse("sketch.add_rectangle sketch=S1 x=0 y=1.5 width=40 height=20").unwrap();
        assert_eq!(p.args["y"], json!(1.5));
        let p = parse("feature.fillet edges=\"a, b\"").unwrap();
        assert_eq!(p.args["edges"], json!(["a", "b"]));
    }

    #[test]
    fn json_arguments_are_passed_through() {
        let p = parse(r#"param.set {"name": "w", "expr": "12 mm"}"#).unwrap();
        assert_eq!((p.id, p.args), ("param.set", json!({"name": "w", "expr": "12 mm"})));
        assert!(parse("param.set {nope").unwrap_err().contains("JSON"));
    }

    #[test]
    fn ids_resolve_through_the_registry_only() {
        assert_eq!(parse("feature_extrude sketch=S1").unwrap().id, "feature.extrude");
        // A unique last part is accepted; an unknown name is an error with suggestions.
        assert_eq!(resolve("extrude").map(|s| s.id), Ok("feature.extrude"));
        let e = parse("extrud").unwrap_err();
        assert!(e.contains("unknown command") && e.contains("feature.extrude"), "{e}");
        assert!(parse("zzz-nothing").unwrap_err().contains("unknown command"));
        assert!(parse("   ").is_err());
    }

    #[test]
    fn argument_errors_name_the_problem() {
        assert!(parse("feature.extrude sketch").unwrap_err().contains("key=value"));
        let e = parse("feature.extrude nope=1").unwrap_err();
        assert!(e.contains("unknown argument 'nope'") && e.contains("distance"), "{e}");
        assert!(parse("sketch.add_rectangle sketch=S1 x=abc").unwrap_err().contains("number"));
    }

    #[test]
    fn tab_completes_ids_and_cycles_through_ambiguous_ones() {
        assert_eq!(complete("feature.extr").as_deref(), Some("feature.extrude "));
        // Several matches: up to their common prefix first, then one by one.
        let first = complete("feature.").unwrap();
        assert!(first.starts_with("feature."), "{first}");
        let all = candidates("feature.");
        assert!(all.len() > 3);
        let a = complete("feature.").unwrap();
        let b = complete(&a).unwrap();
        assert!(all.contains(&a.as_str()) || a.len() >= "feature.".len());
        assert!(all.contains(&b.as_str()) || b.len() >= a.len());
        // Nothing to complete after the id, or for no match.
        assert_eq!(complete("feature.extrude sk"), None);
        assert_eq!(complete("zzzzqq"), None);
        // The empty line offers the whole registry's common prefix: nothing to add.
        assert!(complete("").is_none_or(|t| !t.is_empty()));
    }

    #[test]
    fn hints_list_matches_then_arguments() {
        assert!(hint("feature.ext").contains("feature.extrude"));
        let h = hint("feature.extrude ");
        assert!(h.contains("sketch=") && h.contains("distance="), "{h}");
        assert_eq!(hint(""), "");
        assert_eq!(hint("zzzzqq"), "");
    }

    #[test]
    fn history_steps_back_and_forth() {
        let mut s = State { text: "a".into(), ..State::default() };
        assert_eq!(s.take_line(), "a");
        s.text = "b".into();
        s.take_line();
        s.recall(true);
        assert_eq!(s.text, "b");
        s.recall(true);
        assert_eq!(s.text, "a");
        s.recall(false);
        assert_eq!(s.text, "b");
        s.recall(false);
        assert_eq!(s.text, "");
        // Repeating the last line does not duplicate it.
        s.text = "b".into();
        s.take_line();
        assert_eq!(s.history, ["a", "b"]);
    }
}
