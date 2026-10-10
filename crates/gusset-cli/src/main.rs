//! `gusset` — the command-line entry point.
//!
//! ```text
//! gusset build <model.gus|model.gus.txt> [--branch NAME] [--export OUT]... [--tolerance MM]
//! gusset text  <model.gus> [--branch NAME]          print the code view
//! gusset pack  <model.gus.txt> -o <model.gus>      create a project from text
//! gusset info                                       hardware / kernel diagnostics
//! gusset self-test                                  quick end-to-end check (used by the updater)
//! gusset --version
//! Global: --force-dispatch <scalar|sse42|avx2|avx512|neon|...>
//! ```

use gusset_commands::commands::{mesh_export_bytes, mesh_format};
use gusset_doc::Document;
use gusset_eval::{BlobSource, Evaluator, FeatureStatus};
use gusset_history::History;
use gusset_kernel::Kernel;
use gusset_kernel_occt::OcctKernel;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "usage:
  gusset build <model.gus|model.gus.txt> [--branch NAME] [--export OUT]... [--tolerance MM]
  gusset text <model.gus> [--branch NAME]
  gusset pack <model.gus.txt> -o <model.gus>
  gusset info
  gusset self-test
  gusset --version
global options: --force-dispatch LEVEL";

/// Exit codes: 0 ok, 1 model has feature errors, 2 usage / IO / damaged input.
fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--force-dispatch") {
        let Some(level) = args.get(i + 1).cloned() else {
            eprintln!("--force-dispatch needs a level");
            return ExitCode::from(2);
        };
        args.drain(i..=i + 1);
        let features = gusset_hw::cpu::CpuFeatures::detect();
        match gusset_hw::dispatch::choose(&features, Some(&level)) {
            Ok(l) => {
                gusset_hw::dispatch::init(gusset_hw::dispatch::ActiveDispatch { level: l, forced: true });
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        }
    }
    let r = match args.first().map(String::as_str) {
        Some("--version" | "-V" | "version") => {
            println!("gusset {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("build") => build(args.get(1..).unwrap_or_default()),
        Some("text") => text(args.get(1..).unwrap_or_default()),
        Some("pack") => pack(args.get(1..).unwrap_or_default()),
        Some("info") => info(),
        Some("self-test" | "--self-test") => self_test(),
        Some("--help" | "-h" | "help") | None => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => Err(format!("unknown command '{other}'\n{USAGE}")),
    };
    match r {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

struct Opts {
    file: PathBuf,
    branch: Option<String>,
    exports: Vec<PathBuf>,
    tolerance: f64,
    output: Option<PathBuf>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { file: PathBuf::new(), branch: None, exports: vec![], tolerance: 0.01, output: None };
    let mut it = args.iter();
    let mut have_file = false;
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--branch" => o.branch = Some(val("--branch")?),
            "--export" | "-e" => o.exports.push(PathBuf::from(val("--export")?)),
            "-o" | "--output" => o.output = Some(PathBuf::from(val("-o")?)),
            "--tolerance" => {
                o.tolerance =
                    val("--tolerance")?.parse().map_err(|_| "--tolerance must be a number (mm)".to_string())?;
                if !(1e-4..=10.0).contains(&o.tolerance) {
                    return Err("--tolerance must be between 0.0001 and 10 mm".into());
                }
            }
            s if s.starts_with('-') => return Err(format!("unknown option '{s}'")),
            s => {
                if have_file {
                    return Err(format!("unexpected argument '{s}'"));
                }
                o.file = PathBuf::from(s);
                have_file = true;
            }
        }
    }
    if !have_file {
        return Err(format!("missing input file\n{USAGE}"));
    }
    Ok(o)
}

fn is_text(p: &Path) -> bool {
    p.to_string_lossy().ends_with(".gus.txt") || p.extension().is_some_and(|e| e == "txt")
}

/// Loads a project (read-only: no lock, no recovery) or a text document.
fn load(path: &Path) -> Result<History, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if is_text(path) {
        let text = String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8 text", path.display()))?;
        let doc = Document::from_text(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(History::new(&doc, &gusset_history::Ctx::user(0)))
    } else {
        gusset_project::decode(&bytes).map(|(h, _)| h).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn pick_branch(h: &History, name: Option<&str>) -> Result<gusset_history::BranchId, String> {
    match name {
        None | Some("main") => Ok(h.main()),
        Some(n) => h.branch_by_name(n).map(|b| b.id).ok_or_else(|| format!("no branch named '{n}'")),
    }
}

struct Blobs<'a>(&'a History);

impl BlobSource for Blobs<'_> {
    fn blob(&self, hex: &str) -> Option<Arc<[u8]>> {
        self.0.objects().get(gusset_history::Hash::from_hex(hex)?).ok().cloned()
    }
}

fn kernel() -> Result<Arc<dyn Kernel>, String> {
    OcctKernel::new().map(|k| Arc::new(k) as Arc<dyn Kernel>).map_err(|e| e.to_string())
}

fn build(args: &[String]) -> Result<ExitCode, String> {
    let o = parse_opts(args)?;
    let h = load(&o.file)?;
    let b = pick_branch(&h, o.branch.as_deref())?;
    let doc = h.head_document(b).map_err(|e| e.to_string())?;
    let rollback = h.branch(b).map_err(|e| e.to_string())?.rollback;
    let k = kernel()?;
    let mut ev = Evaluator::new(k.clone());
    let t0 = std::time::Instant::now();
    let r = ev.evaluate(&doc, rollback, &Blobs(&h));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut failed = 0;
    for (name, st) in &r.statuses {
        match st {
            FeatureStatus::Ok => println!("  ok        {name}"),
            FeatureStatus::Suppressed => println!("  suppressed {name}"),
            FeatureStatus::RolledBack => println!("  rolled back {name}"),
            FeatureStatus::Blocked { by } => {
                failed += 1;
                println!("  blocked   {name} (depends on {by})");
            }
            FeatureStatus::Failed(e) => {
                failed += 1;
                println!("  FAILED    {e}");
                if let Some(s) = &e.suggestion {
                    println!("            suggested reference: {s}");
                }
            }
        }
    }
    println!("{} features, {} bodies, rebuilt in {ms:.1} ms", r.statuses.len(), r.bodies.len());
    for body in &r.bodies {
        if let (Ok(m), Ok(bb)) = (k.mass_properties(&body.shape), k.bbox(&body.shape)) {
            let sz = bb.size();
            println!(
                "  body {}: volume {:.3} mm³, area {:.3} mm², size {:.3} × {:.3} × {:.3} mm",
                body.name, m.volume, m.area, sz[0], sz[1], sz[2]
            );
        }
    }
    for out in &o.exports {
        if r.bodies.is_empty() {
            return Err("there is nothing to export (no bodies)".into());
        }
        let bodies: Vec<(&gusset_kernel::Shape, &str)> = r.bodies.iter().map(|b| (&b.shape, b.name.as_str())).collect();
        let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let bytes = match ext.as_str() {
            "step" | "stp" => k.export_step(&bodies).map_err(|e| e.to_string())?,
            "iges" | "igs" => k.export_iges(&bodies).map_err(|e| e.to_string())?,
            other => {
                let f = mesh_format(other)
                    .ok_or_else(|| format!("unknown export format '.{other}' (step, iges, stl, 3mf, obj)"))?;
                mesh_export_bytes(k.as_ref(), &bodies, f, o.tolerance).map_err(|e| e.to_string())?
            }
        };
        write_atomic(out, &bytes)?;
        println!("wrote {} ({} bytes)", out.display(), bytes.len());
    }
    Ok(if failed > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

/// Writes through gusset-store's durable write (temp file, fsync, verify, atomic rename).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let fs = gusset_store::fs::RealFs::default();
    let ok = |_: &[u8]| -> Result<(), String> { Ok(()) };
    gusset_store::durable::durable_write(&fs, path, bytes, &ok)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn text(args: &[String]) -> Result<ExitCode, String> {
    let o = parse_opts(args)?;
    let h = load(&o.file)?;
    let b = pick_branch(&h, o.branch.as_deref())?;
    print!("{}", h.head_document(b).map_err(|e| e.to_string())?.to_text());
    Ok(ExitCode::SUCCESS)
}

fn pack(args: &[String]) -> Result<ExitCode, String> {
    let o = parse_opts(args)?;
    let out = o.output.ok_or("pack needs -o <model.gus>")?;
    let h = load(&o.file)?;
    let meta = gusset_project::ProjectMeta {
        id: gusset_store::paths::ProjectId::new_random().map_err(|e| e.to_string())?,
        save_seq: 1,
        base_seq: 1,
        created: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    };
    let bytes = gusset_project::encode(&h, &meta).map_err(|e| e.to_string())?;
    let fs = gusset_store::fs::RealFs::default();
    gusset_store::durable::durable_write(&fs, &out, &bytes, &gusset_store::container::verify)
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(ExitCode::SUCCESS)
}

fn info() -> Result<ExitCode, String> {
    println!("gusset {}", env!("CARGO_PKG_VERSION"));
    let v = OcctKernel::version();
    println!(
        "geometry kernel: {}",
        if v.is_empty() { "not included in this build".into() } else { format!("OCCT {v}") }
    );
    let d = gusset_hw::diagnostics::Diagnostics::collect();
    print!("{}", d.to_text());
    Ok(ExitCode::SUCCESS)
}

/// End-to-end check used by the installer smoke tests and the updater (ADR 0014):
/// kernel loads, a parametric part rebuilds, STEP and STL export work.
fn self_test() -> Result<ExitCode, String> {
    let k = kernel()?;
    let doc = Document::from_text(SELF_TEST_MODEL).map_err(|e| format!("built-in sample: {e}"))?;
    let mut ev = Evaluator::new(k.clone());
    let r = ev.evaluate(&doc, None, &());
    if let Some(e) = r.errors().first() {
        return Err(format!("sample rebuild failed: {e}"));
    }
    let body = r.bodies.first().ok_or("sample produced no body")?;
    let m = k.mass_properties(&body.shape).map_err(|e| e.to_string())?;
    let expected = 40.0 * 20.0 * 10.0 - (4.0 - std::f64::consts::PI) * 10.0;
    if (m.volume - expected).abs() > 1e-3 * expected {
        return Err(format!("sample volume {} != expected {expected}", m.volume));
    }
    let step = k.export_step(&[(&body.shape, "SelfTest")]).map_err(|e| e.to_string())?;
    let stl = mesh_export_bytes(k.as_ref(), &[(&body.shape, "SelfTest")], gusset_meshio::ExportFormat::StlBinary, 0.05)
        .map_err(|e| e.to_string())?;
    println!(
        "self-test ok: kernel {}, volume {:.3} mm³, STEP {} B, STL {} B",
        OcctKernel::version(),
        m.volume,
        step.len(),
        stl.len()
    );
    Ok(ExitCode::SUCCESS)
}

const SELF_TEST_MODEL: &str = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)

fillet Fillet1 (edges: [edge(Extrude1/side(Sketch1/l5), Extrude1/side(Sketch1/l6))], radius: 2 mm)
";
