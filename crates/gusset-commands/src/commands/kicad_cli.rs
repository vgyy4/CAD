//! `kicad-cli` as a separate, user-named process (ADR 0056): KiCad's own DRC and ERC as
//! the authoritative check, and its STEP, IPC-2581 and ODB++ exporters.
//!
//! KiCad is GPL and never bundled, downloaded or searched for: the user names their
//! `kicad-cli` in Settings, and without one these commands refuse. The open board or
//! schematic is first written to a private temporary folder (the user's files are never
//! passed to it or overwritten), the program is run there with its arguments as a list
//! (no shell), no stdin, its output in files, a time limit and size caps, and the
//! JSON report it writes is read back into Gusset's report shapes, marked `source: kicad`.

use super::pcb::no_board;
use super::pcb_autoroute::{Stopped, TempDir, log_tail, run_until};
use super::sch::no_schematic;
use super::*;
use crate::{pcb_session, sch_session};
use gusset_eval::BlobSource;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<PcbKicadDrc>(), spec::<SchKicadErc>(), spec::<PcbKicadExport>()]
}

/// Largest JSON report read back.
const MAX_REPORT_BYTES: u64 = 32 << 20;
/// Largest exported file taken over (a run producing more is stopped).
const MAX_EXPORT_BYTES: u64 = 512 << 20;
/// Most of its console output kept per stream; a program writing more is stopped.
const MAX_LOG_BYTES: u64 = 4 << 20;
/// Most findings listed per report.
const MAX_LISTED: usize = 200;
/// Longest run allowed whatever the settings say (seconds).
const MAX_TIMEOUT_S: u64 = 3600;

/// The configured program, or the refusal.
fn configured(s: &Session, ctx: &CmdCtx, what: &str) -> Result<pcb_session::KicadCli, CommandError> {
    let k = s.kicad_cli.clone().ok_or_else(|| {
        CommandError::new("not_available", "no kicad-cli is set up")
            .hint("name your kicad-cli in the PCB tab's KiCad section; Gusset does not download or look for KiCad")
    })?;
    // An agent starting a program on the user's machine needs the user's yes.
    if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
        return Err(CommandError::approval(what));
    }
    Ok(k)
}

fn size_of(p: &Path) -> u64 {
    std::fs::metadata(p).map_or(0, |m| m.len())
}

/// Runs `kicad-cli` with `args` in `dir`; its console output goes to `out.log` and
/// `err.log` there. `watch` is a file whose size is capped at [`MAX_EXPORT_BYTES`].
fn run_kicad(
    k: &pcb_session::KicadCli,
    args: Vec<OsString>,
    dir: &TempDir,
    watch: Option<&Path>,
    timeout_s: Option<u64>,
) -> Result<(), CommandError> {
    let timeout = timeout_s.unwrap_or(k.timeout_s).clamp(1, MAX_TIMEOUT_S);
    let mut cmd = std::process::Command::new(&k.path);
    cmd.args(args).current_dir(&dir.0).stdin(std::process::Stdio::null());
    let log = |n: &str| std::fs::File::create(dir.0.join(n)).map(std::process::Stdio::from);
    if let (Ok(o), Ok(e)) = (log("out.log"), log("err.log")) {
        cmd.stdout(o).stderr(e);
    }
    let (out, err) = (dir.0.join("out.log"), dir.0.join("err.log"));
    let runaway = || {
        size_of(&out) > MAX_LOG_BYTES
            || size_of(&err) > MAX_LOG_BYTES
            || watch.is_some_and(|w| size_of(w) > MAX_EXPORT_BYTES)
    };
    let cancel = AtomicBool::new(false);
    let status = run_until(cmd, &cancel, Duration::from_secs(timeout), &runaway).map_err(|e| {
        CommandError::new("io", format!("could not start kicad-cli: {e}"))
            .hint("check the path in the PCB tab's KiCad section")
    })?;
    match status {
        Err(Stopped::TimedOut) => Err(CommandError::new("timeout", format!("kicad-cli did not finish in {timeout} s"))
            .hint("give a longer timeout_s")),
        Err(Stopped::Runaway) => {
            Err(CommandError::new("io", "kicad-cli wrote more output than allowed and was stopped"))
        }
        Err(Stopped::Cancelled) => Err(CommandError::new("cancelled", "the run was cancelled")),
        Ok(st) if !st.success() => Err(CommandError::new("io", format!("kicad-cli failed (exit {:?})", st.code()))
            .details(json!({ "stderr_user_text": log_tail(&err) }))),
        Ok(_) => Ok(()),
    }
}

/// A file name from the document as a safe relative path inside the temporary folder.
fn safe_rel(name: &str) -> Result<PathBuf, CommandError> {
    let p = Path::new(name);
    let ok = !name.is_empty()
        && !name.contains('\\')
        && p.components().all(|c| matches!(c, Component::Normal(_)))
        && p.components().count() <= 8;
    if ok {
        Ok(p.to_path_buf())
    } else {
        Err(CommandError::args(format!("the sheet file name '{}' is not usable", crate::paths::display_name(name))))
    }
}

fn write_temp(path: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CommandError::new("io", format!("temporary folder: {e}")))?;
    }
    std::fs::write(path, bytes).map_err(|e| CommandError::new("io", format!("temporary file: {e}")))
}

/// The open board (and its project files) written into `dir` as `board.kicad_pcb`.
fn stage_board(s: &Session, ctx: &CmdCtx, dir: &TempDir) -> Result<PathBuf, CommandError> {
    let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
    let pcb = dir.0.join("board.kicad_pcb");
    write_temp(&pcb, open.board.write().as_bytes())?;
    if let Some(pro) = open.project_hash.as_ref().and_then(|h| s.blobs().blob(h)) {
        let mut text = String::from_utf8_lossy(&pro).into_owned();
        let mut dru = None;
        if let Ok(mut v) = serde_json::from_str::<Value>(&text)
            && let Some(m) = v.as_object_mut()
            && let Some(t) = m.remove(pcb_session::CUSTOM_RULES_KEY)
        {
            dru = t.as_str().map(str::to_string);
            text = serde_json::to_string_pretty(&v).unwrap_or(text);
        }
        write_temp(&dir.0.join("board.kicad_pro"), text.as_bytes())?;
        if let Some(t) = dru {
            write_temp(&dir.0.join("board.kicad_dru"), t.as_bytes())?;
        }
    }
    Ok(pcb)
}

/// Reads a report the program wrote, capped.
fn read_report(path: &Path) -> Result<Value, CommandError> {
    if !path.is_file() {
        return Err(CommandError::new("io", "kicad-cli wrote no report"));
    }
    let f = crate::import::open_regular(path, "report.json", MAX_REPORT_BYTES)?;
    let bytes = crate::import::read_capped(f, MAX_REPORT_BYTES, &AtomicBool::new(false))?;
    serde_json::from_slice(&bytes).map_err(|e| CommandError::new("io", format!("the KiCad report is not JSON: {e}")))
}

/// Millimetres per unit of the report's `coordinate_units`.
fn unit_scale(report: &Value) -> f64 {
    match report.get("coordinate_units").and_then(Value::as_str).unwrap_or("mm") {
        "in" | "inch" | "inches" => 25.4,
        "mils" | "mil" => 0.0254,
        _ => 1.0,
    }
}

fn clean(v: Option<&Value>, max: usize) -> String {
    v.and_then(Value::as_str).unwrap_or_default().chars().filter(|c| !c.is_control()).take(max).collect()
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// One KiCad finding in Gusset's shape: `code` is KiCad's `type`, `at` the first item's
/// position in mm (board or sheet coordinates, Y down), `items_user_text` the item texts.
/// Fields: `type`, `description`, `severity`, `items[].description`, `items[].pos.{x,y}`,
/// `excluded` (the last is not certain for every KiCad version).
fn finding(v: &Value, scale: f64, default_severity: &str) -> Option<Value> {
    if v.get("excluded").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let severity = clean(v.get("severity"), 16);
    let severity = if severity.is_empty() { default_severity.to_string() } else { severity };
    if matches!(severity.as_str(), "ignore" | "exclusion") {
        return None;
    }
    let items = v.get("items").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let at = items.iter().find_map(|i| {
        let p = i.get("pos")?;
        Some([round4(p.get("x")?.as_f64()? * scale), round4(p.get("y")?.as_f64()? * scale)])
    });
    let texts: Vec<String> = items.iter().take(8).map(|i| clean(i.get("description"), 200)).collect();
    Some(json!({
        "code": clean(v.get("type"), 64),
        "severity": severity,
        "message_user_text": clean(v.get("description"), 400),
        "items_user_text": texts,
        "at": at,
        "source": "kicad",
    }))
}

/// A KiCad DRC report (`pcb drc --format json`) in the shape of `pcb.drc`.
pub(crate) fn drc_from_report(r: &Value) -> Value {
    let scale = unit_scale(r);
    let list = |key: &str, sev: &str| -> Vec<Value> {
        r.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| finding(v, scale, sev)).collect())
            .unwrap_or_default()
    };
    let violations = list("violations", "error");
    let unconnected = list("unconnected_items", "error");
    let parity = list("schematic_parity", "warning");
    let all: Vec<&Value> = violations.iter().chain(&unconnected).chain(&parity).collect();
    json!({
        "source": "kicad",
        "kicad_version_user_text": clean(r.get("kicad_version"), 40),
        "errors": all.iter().filter(|v| v["severity"] == "error").count(),
        "warnings": all.iter().filter(|v| v["severity"] == "warning").count(),
        "more": violations.len() > MAX_LISTED || unconnected.len() > MAX_LISTED || parity.len() > MAX_LISTED,
        "unrouted": unconnected.len(),
        "violations": violations.iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "unconnected": unconnected.iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "schematic_parity": parity.iter().take(MAX_LISTED).collect::<Vec<_>>(),
    })
}

/// A KiCad ERC report (`sch erc --format json`) in the shape of `sch.erc`.
/// Findings sit in `sheets[].violations[]` (the sheet in `sheets[].path`).
pub(crate) fn erc_from_report(r: &Value) -> Value {
    let scale = unit_scale(r);
    let mut issues = Vec::new();
    let mut total = 0usize;
    for sh in r.get("sheets").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
        let path = clean(sh.get("path"), 200);
        for v in sh.get("violations").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
            if let Some(mut f) = finding(v, scale, "error") {
                total += 1;
                if issues.len() < MAX_LISTED {
                    if let Some(m) = f.as_object_mut() {
                        m.insert("sheet_user_text".into(), json!(path));
                    }
                    issues.push(f);
                }
            }
        }
    }
    let errors = issues.iter().filter(|v| v["severity"] == "error").count();
    json!({
        "source": "kicad",
        "kicad_version_user_text": clean(r.get("kicad_version"), 40),
        "issues": issues,
        "more": total > MAX_LISTED,
        "errors": errors,
    })
}

/// Runs KiCad's own design rule check on the open board.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbKicadDrc {
    /// Seconds (default: the setting's).
    #[serde(default)]
    pub timeout_s: Option<u64>,
}

impl Command for PcbKicadDrc {
    const ID: &'static str = "pcb.kicad_drc";
    const TITLE: &'static str = "KiCad design rule check";
    const DESCRIPTION: &'static str = "Check the open board with the user's own kicad-cli (authoritative DRC).";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let k = configured(s, ctx, "running kicad-cli")?;
        let dir = TempDir::new("kicad")?;
        let pcb = stage_board(s, ctx, &dir)?;
        let report = dir.0.join("report.json");
        let args = ["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "--output"]
            .map(OsString::from)
            .into_iter()
            .chain([report.clone().into_os_string(), pcb.into_os_string()])
            .collect();
        run_kicad(&k, args, &dir, None, self.timeout_s)?;
        Ok(drc_from_report(&read_report(&report)?))
    }
}

/// Runs KiCad's own electrical rule check on the open schematic.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchKicadErc {
    /// Seconds (default: the setting's).
    #[serde(default)]
    pub timeout_s: Option<u64>,
}

impl Command for SchKicadErc {
    const ID: &'static str = "sch.kicad_erc";
    const TITLE: &'static str = "KiCad electrical rule check";
    const DESCRIPTION: &'static str = "Check the open schematic with the user's own kicad-cli (authoritative ERC).";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let k = configured(s, ctx, "running kicad-cli")?;
        let open = sch_session::open_schematic(s, &ctx.actor)?.ok_or_else(no_schematic)?;
        let dir = TempDir::new("kicad")?;
        // Every sheet of the design, the root first, under their own relative names.
        let mut root = None;
        for (i, (file, sheet)) in open.design.sheets.iter().enumerate() {
            let rel = safe_rel(file)?;
            let full = dir.0.join(&rel);
            write_temp(&full, sheet.write().as_bytes())?;
            if i == 0 {
                root = Some(full);
            }
        }
        let root = root.ok_or_else(no_schematic)?;
        let report = dir.0.join("report.json");
        let args = ["sch", "erc", "--format", "json", "--severity-all", "--units", "mm", "--output"]
            .map(OsString::from)
            .into_iter()
            .chain([report.clone().into_os_string(), root.into_os_string()])
            .collect();
        run_kicad(&k, args, &dir, None, self.timeout_s)?;
        Ok(erc_from_report(&read_report(&report)?))
    }
}

/// What kicad-cli writes.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum KicadFormat {
    Step,
    Ipc2581,
    Odb,
}

impl KicadFormat {
    /// The sub-command, the extensions allowed for the output and the file KiCad is told to write.
    fn parts(self) -> (&'static str, &'static [&'static str], &'static str) {
        match self {
            KicadFormat::Step => ("step", &["step", "stp"], "out.step"),
            KicadFormat::Ipc2581 => ("ipc2581", &["xml"], "out.xml"),
            KicadFormat::Odb => ("odb", &["zip"], "out.zip"),
        }
    }
}

/// Exports the open board with KiCad's own exporters.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbKicadExport {
    pub format: KicadFormat,
    /// Output .step/.stp, .xml (IPC-2581) or .zip (ODB++).
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
    /// Seconds (default: the setting's).
    #[serde(default)]
    pub timeout_s: Option<u64>,
}

impl Command for PcbKicadExport {
    const ID: &'static str = "pcb.kicad_export";
    const TITLE: &'static str = "KiCad export";
    const DESCRIPTION: &'static str = "Write the open board as STEP, IPC-2581 or ODB++ with the user's own kicad-cli.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (sub, exts, staged) = self.format.parts();
        let lower = self.path.to_ascii_lowercase();
        if !exts.iter().any(|e| lower.ends_with(&format!(".{e}"))) {
            return Err(CommandError::args(format!("the file name ends in .{}", exts.join(" or ."))));
        }
        let k = configured(s, ctx, "running kicad-cli")?;
        let target = crate::paths::export_target(s.project_dir.as_deref(), &self.path, &ctx.actor, exts)?;
        if target.exists() {
            if !self.overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{}' already exists", self.path))
                    .hint("pass overwrite: true to replace the file"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting an exported file"));
            }
        }
        let dir = TempDir::new("kicad")?;
        let pcb = stage_board(s, ctx, &dir)?;
        let out = dir.0.join(staged);
        let mut args: Vec<OsString> = ["pcb", "export", sub, "--output"].map(OsString::from).into();
        args.push(out.clone().into_os_string());
        if matches!(self.format, KicadFormat::Step) {
            args.push("--force".into());
        }
        args.push(pcb.into_os_string());
        run_kicad(&k, args, &dir, Some(&out), self.timeout_s)?;
        if !out.is_file() {
            return Err(CommandError::new("io", "kicad-cli wrote no output file"));
        }
        let f = crate::import::open_regular(&out, "export", MAX_EXPORT_BYTES)?;
        let bytes = crate::import::read_capped(f, MAX_EXPORT_BYTES, &AtomicBool::new(false))?;
        crate::paths::write_file(&target, &self.path, &bytes)?;
        Ok(json!({ "written": self.path, "bytes": bytes.len(), "source": "kicad" }))
    }
}
