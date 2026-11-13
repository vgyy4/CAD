//! `kicad-cli` as a user-named separate process (ADR 0056), with fake programs: shell
//! scripts written by the test into a temporary folder (nothing is downloaded or run but
//! these), which write canned JSON reports. The report formats are written by hand after
//! KiCad's documented JSON (see the notes in the ADR).

use crate::commands::kicad_cli::{drc_from_report, erc_from_report};
use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::{Value, json};

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

/// A hand-written `pcb drc --format json` report (field names after KiCad 8/9's
/// `drc.v1.json`; `excluded` is a guess).
const DRC_JSON: &str = r#"{
  "$schema": "https://schemas.kicad.org/drc.v1.json",
  "coordinate_units": "mm",
  "date": "2026-11-13T10:00:00+0000",
  "kicad_version": "9.0.0",
  "source": "board.kicad_pcb",
  "violations": [
    {
      "description": "Clearance violation (netclass 'Default' clearance 0.2000 mm; actual 0.1500 mm)",
      "items": [
        { "description": "Track [GND] on F.Cu, length 5.0000 mm", "pos": { "x": 12.5, "y": 8.25 }, "uuid": "aaaa" },
        { "description": "Via [VCC] on F.Cu - B.Cu", "pos": { "x": 12.6, "y": 8.4 }, "uuid": "bbbb" }
      ],
      "severity": "error",
      "type": "clearance"
    },
    {
      "description": "Silkscreen overlap",
      "items": [ { "description": "Text 'R1' on F.Silkscreen", "pos": { "x": 3, "y": 4 }, "uuid": "cccc" } ],
      "severity": "warning",
      "type": "silk_overlap"
    },
    {
      "description": "Skipped one",
      "items": [],
      "severity": "error",
      "excluded": true,
      "type": "clearance"
    }
  ],
  "unconnected_items": [
    {
      "description": "Missing connection between items",
      "items": [
        { "description": "Pad 1 [SIG] of TP1", "pos": { "x": 5, "y": 10 }, "uuid": "dddd" },
        { "description": "Pad 1 [SIG] of TP2", "pos": { "x": 25, "y": 10 }, "uuid": "eeee" }
      ],
      "severity": "error",
      "type": "unconnected_items"
    }
  ],
  "schematic_parity": []
}"#;

/// A hand-written `sch erc --format json` report (`sheets[].violations[]`).
const ERC_JSON: &str = r#"{
  "$schema": "https://schemas.kicad.org/erc.v1.json",
  "coordinate_units": "mm",
  "kicad_version": "9.0.0",
  "source": "demo.kicad_sch",
  "sheets": [
    {
      "path": "/",
      "uuid_path": "/1234",
      "violations": [
        {
          "description": "Pin not connected",
          "items": [ { "description": "Symbol R1 Pin 1 [Passive, Line]", "pos": { "x": 100.33, "y": 46.99 }, "uuid": "1" } ],
          "severity": "error",
          "type": "pin_not_connected"
        },
        {
          "description": "Input Power pin not driven by any Output Power pins",
          "items": [ { "description": "Symbol #PWR01 Pin 1 [Power input, Line]", "pos": { "x": 100, "y": 60 }, "uuid": "2" } ],
          "severity": "warning",
          "type": "power_pin_not_driven"
        }
      ]
    },
    { "path": "/sub/", "uuid_path": "/1234/5678", "violations": [] }
  ]
}"#;

#[test]
fn reports_are_read_into_gussets_shapes() {
    let r = drc_from_report(&serde_json::from_str(DRC_JSON).unwrap());
    assert_eq!(r["source"], "kicad");
    assert_eq!(r["kicad_version_user_text"], "9.0.0");
    let v = r["violations"].as_array().unwrap();
    assert_eq!(v.len(), 2, "the excluded one is left out: {r}");
    assert_eq!((v[0]["code"].as_str(), v[0]["severity"].as_str()), (Some("clearance"), Some("error")));
    assert_eq!(v[0]["at"], json!([12.5, 8.25]));
    assert_eq!(v[0]["items_user_text"].as_array().unwrap().len(), 2);
    assert!(v[0]["message_user_text"].as_str().unwrap().contains("actual 0.1500 mm"));
    assert_eq!(v[1]["severity"], "warning");
    assert_eq!((r["errors"].as_u64(), r["warnings"].as_u64(), r["unrouted"].as_u64()), (Some(2), Some(1), Some(1)));
    assert_eq!(r["unconnected"][0]["code"], "unconnected_items");
    // The GUI's markers read the same list.
    // Inches are converted; a finding without position keeps `at` null.
    let inch = json!({"coordinate_units": "in", "violations": [
        {"type": "x", "description": "d", "severity": "error", "items": [{"description": "i", "pos": {"x": 1, "y": 2}}]},
        {"type": "y", "description": "d", "severity": "error", "items": []}]});
    let r = drc_from_report(&inch);
    assert_eq!(r["violations"][0]["at"], json!([25.4, 50.8]));
    assert!(r["violations"][1]["at"].is_null());
    // Control characters in KiCad's texts never reach the result.
    let r = drc_from_report(
        &json!({"violations": [{"type": "t\u{1b}x", "description": "a\nb\u{7}", "severity": "error"}]}),
    );
    assert_eq!(
        (r["violations"][0]["code"].as_str(), r["violations"][0]["message_user_text"].as_str()),
        (Some("tx"), Some("ab"))
    );
    // Not a report at all: empty lists.
    assert_eq!(drc_from_report(&json!(7))["violations"], json!([]));

    let r = erc_from_report(&serde_json::from_str(ERC_JSON).unwrap());
    let i = r["issues"].as_array().unwrap();
    assert_eq!(i.len(), 2, "{r}");
    assert_eq!((i[0]["code"].as_str(), i[0]["sheet_user_text"].as_str()), (Some("pin_not_connected"), Some("/")));
    assert_eq!(i[0]["at"], json!([100.33, 46.99]));
    assert_eq!((r["errors"].as_u64(), r["source"].as_str()), (Some(1), Some("kicad")));
    // More than the list holds.
    let many: Vec<Value> = (0..300).map(|_| json!({"type": "t", "description": "d", "severity": "error"})).collect();
    let r = erc_from_report(&json!({"sheets": [{"path": "/", "violations": many}]}));
    assert_eq!((r["issues"].as_array().unwrap().len(), r["more"].as_bool()), (200, Some(true)));
}

/// A stand-in for `kicad-cli`: a shell script in a temporary folder. `body` runs with
/// `$out` set to the `--output` argument, `$sub` to the first two words and `$last` to
/// the input file.
#[cfg(unix)]
fn fake_kicad(body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-fake-kicad-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kicad-cli");
    let script = format!(
        "#!/bin/sh\nsub=\"$1 $2 $3\"; prev=; for a in \"$@\"; do [ \"$prev\" = --output ] && out=\"$a\"; prev=\"$a\"; last=\"$a\"; done\n{body}\n"
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn set_up(s: &mut Session, program: std::path::PathBuf, timeout_s: u64) {
    s.kicad_cli = Some(pcb_session::KicadCli { path: program, timeout_s });
}

/// Names and sizes of the files in the project folder, to see nothing else was touched.
fn listing(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
        .collect();
    v.sort();
    v
}

#[cfg(unix)]
#[test]
fn kicad_drc_and_erc_run_only_when_set_up_and_in_a_temporary_folder() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.new", json!({"width": "30 mm", "height": "20 mm"}));
    run(&mut s, &u(2), "sch.new", json!({"name": "demo"}));
    let dir = s.project_dir.clone().unwrap();
    let before = listing(&dir);
    // Not set up: refused, nothing run (no PATH lookup either).
    for id in ["pcb.kicad_drc", "sch.kicad_erc"] {
        let e = execute(&mut s, &u(3), id, json!({})).unwrap_err();
        assert_eq!(e.code, "not_available", "{id}: {e:?}");
    }
    let e = execute(&mut s, &u(3), "pcb.kicad_export", json!({"format": "step", "path": "a.step"})).unwrap_err();
    assert_eq!(e.code, "not_available");
    // The fake: refuses an input inside the project folder, records its folder, writes a report.
    let marker = dir.join("..").join(format!("gusset-fake-cwd-{}", std::process::id()));
    let guard = format!(
        "case \"$last\" in \"{}\"*) exit 8;; esac\ncase \"$PWD\" in *gusset-kicad-*) ;; *) exit 7;; esac\necho \"$PWD\" > \"{}\"\n",
        dir.display(),
        marker.display()
    );
    let drc = fake_kicad(&format!(
        "{guard}test \"$sub\" = 'pcb drc --format' || exit 6\ntest -f board.kicad_pcb || exit 9\ncat > \"$out\" <<'JSON'\n{DRC_JSON}\nJSON\n"
    ));
    set_up(&mut s, drc, 30);
    // Agents need the user's approval to start it.
    let e = execute(&mut s, &CmdCtx::agent("a", 4), "pcb.kicad_drc", json!({})).unwrap_err();
    assert_eq!(e.code, "approval_required", "{e:?}");
    let r = run(&mut s, &u(5), "pcb.kicad_drc", json!({}));
    assert_eq!(r["source"], "kicad", "{r}");
    assert_eq!(r["violations"].as_array().unwrap().len(), 2);
    // Run in a private folder of its own, removed afterwards; the user's files untouched.
    let cwd = std::fs::read_to_string(&marker).unwrap();
    assert!(!std::path::Path::new(cwd.trim()).exists(), "temporary folder left: {cwd}");
    let _ = std::fs::remove_file(&marker);
    assert_eq!(listing(&dir), before);

    // ERC: the schematic is staged under its own name.
    let erc = fake_kicad(&format!(
        "{guard}test \"$sub\" = 'sch erc --format' || exit 6\ncase \"$last\" in *demo.kicad_sch) ;; *) exit 5;; esac\ntest -f \"$last\" || exit 9\ncat > \"$out\" <<'JSON'\n{ERC_JSON}\nJSON\n"
    ));
    set_up(&mut s, erc, 30);
    let r = run(&mut s, &u(6), "sch.kicad_erc", json!({}));
    assert_eq!(
        (r["source"].as_str(), r["issues"].as_array().map(Vec::len), r["errors"].as_u64()),
        (Some("kicad"), Some(2), Some(1)),
        "{r}"
    );
    let _ = std::fs::remove_file(&marker);
    assert_eq!(listing(&dir), before);

    // A program that fails: its error output comes back. One that writes no report.
    set_up(&mut s, fake_kicad("echo 'cannot load board' >&2\nexit 3"), 30);
    let e = execute(&mut s, &u(7), "pcb.kicad_drc", json!({})).unwrap_err();
    assert_eq!(e.code, "io");
    assert!(e.details.unwrap()["stderr_user_text"].as_str().unwrap().contains("cannot load"));
    set_up(&mut s, fake_kicad("exit 0"), 30);
    assert_eq!(execute(&mut s, &u(8), "pcb.kicad_drc", json!({})).unwrap_err().code, "io");
    // A report that is not JSON.
    set_up(&mut s, fake_kicad("echo '<html>' > \"$out\""), 30);
    assert_eq!(execute(&mut s, &u(9), "pcb.kicad_drc", json!({})).unwrap_err().code, "io");
    // One that hangs: stopped at the timeout.
    set_up(&mut s, fake_kicad("exec sleep 30"), 30);
    let t = std::time::Instant::now();
    let e = execute(&mut s, &u(10), "pcb.kicad_drc", json!({"timeout_s": 1})).unwrap_err();
    assert_eq!(e.code, "timeout");
    assert!(t.elapsed() < std::time::Duration::from_secs(10));
    // One that floods its output: stopped, not left running.
    set_up(&mut s, fake_kicad("exec yes"), 30);
    let t = std::time::Instant::now();
    let e = execute(&mut s, &u(11), "pcb.kicad_drc", json!({})).unwrap_err();
    assert_eq!(e.code, "io", "{e:?}");
    assert!(e.message.contains("more output"), "{e:?}");
    assert!(t.elapsed() < std::time::Duration::from_secs(20));
    // A missing program.
    set_up(&mut s, "/nonexistent/kicad-cli".into(), 30);
    assert_eq!(execute(&mut s, &u(12), "pcb.kicad_drc", json!({})).unwrap_err().code, "io");
    assert_eq!(listing(&dir), before);
}

#[cfg(unix)]
#[test]
fn kicad_export_writes_the_output_file_and_never_overwrites_unasked() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.new", json!({"width": "30 mm", "height": "20 mm"}));
    let dir = s.project_dir.clone().unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let body = "case \"$sub\" in 'pcb export step') echo ISO-10303-21 > \"$out\";; 'pcb export ipc2581') echo '<IPC-2581/>' > \"$out\";; 'pcb export odb') printf 'PK' > \"$out\";; *) exit 4;; esac\ntest -f board.kicad_pcb || exit 9";
    set_up(&mut s, fake_kicad(body), 30);
    let r = run(&mut s, &u(2), "pcb.kicad_export", json!({"format": "step", "path": "out/b.step"}));
    assert_eq!((r["written"].as_str(), r["bytes"].as_u64()), (Some("out/b.step"), Some(13)), "{r}");
    assert_eq!(std::fs::read_to_string(dir.join("out/b.step")).unwrap(), "ISO-10303-21\n");
    run(&mut s, &u(3), "pcb.kicad_export", json!({"format": "ipc2581", "path": "out/b.xml"}));
    assert_eq!(std::fs::read_to_string(dir.join("out/b.xml")).unwrap(), "<IPC-2581/>\n");
    run(&mut s, &u(4), "pcb.kicad_export", json!({"format": "odb", "path": "out/b.zip"}));
    assert_eq!(std::fs::read(dir.join("out/b.zip")).unwrap(), b"PK");
    // An existing file needs overwrite, and an agent's overwrite the user's yes.
    let e = execute(&mut s, &u(5), "pcb.kicad_export", json!({"format": "step", "path": "out/b.step"})).unwrap_err();
    assert_eq!(e.code, "confirmation_required");
    let e = execute(
        &mut s,
        &CmdCtx::agent("a", 6),
        "pcb.kicad_export",
        json!({"format": "step", "path": "out/b.step", "overwrite": true}),
    )
    .unwrap_err();
    assert_eq!(e.code, "approval_required", "{e:?}");
    // Wrong extension for the format; paths outside the project for an agent.
    let e = execute(&mut s, &u(7), "pcb.kicad_export", json!({"format": "odb", "path": "out/b.step"})).unwrap_err();
    assert_eq!(e.code, "invalid_args", "{e:?}");
    let mut ctx = CmdCtx::agent("a", 8);
    ctx.approved = true;
    assert!(execute(&mut s, &ctx, "pcb.kicad_export", json!({"format": "step", "path": "../x.step"})).is_err());
    // A failing program leaves no output behind.
    set_up(&mut s, fake_kicad("exit 2"), 30);
    assert!(execute(&mut s, &u(9), "pcb.kicad_export", json!({"format": "step", "path": "out/c.step"})).is_err());
    assert!(!dir.join("out/c.step").exists());
}
