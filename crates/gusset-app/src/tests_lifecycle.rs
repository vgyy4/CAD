//! GUI tests for the project lifecycle: close requests, unsaved changes, running work,
//! save failures, opening another file, crash recovery and export overwrites. Close
//! requests are real window events (`ViewportEvent::Close`), so `handle_close` runs.

use crate::app::{App, Dialog, PathPurpose, Pending};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use gusset_kernel::mock::MockKernel;
use gusset_store::paths::DataDirs;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn temp_dir(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let d = std::env::temp_dir().join(format!("gusset-life-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn dirs_in(root: &Path) -> DataDirs {
    DataDirs::at(root.join("data"), root.join("cache"))
}

fn app_with(dirs: DataDirs) -> App {
    App::new(Arc::new(MockKernel), Some(dirs))
}

fn harness(app: App) -> Harness<'static, App> {
    // Host change events wake the UI from another thread and can add a frame or two.
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(12)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

/// The OS close button: a close request in the root viewport's events.
fn request_close(h: &mut Harness<'static, App>) {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.step();
}

fn sent(h: &Harness<'static, App>, cmd: &egui::ViewportCommand) -> bool {
    h.output().viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.commands.contains(cmd))
}

fn settle(h: &mut Harness<'static, App>) {
    // Dialogs fade in and out over a few frames.
    h.run_steps(8);
    h.state_mut().wait_idle();
    h.run_steps(8);
}

/// Steps the harness (real time) until `f` holds.
fn run_until(h: &mut Harness<'static, App>, what: &str, f: impl Fn(&Harness<'static, App>) -> bool) {
    for _ in 0..300 {
        h.step();
        if f(h) {
            h.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn recovery_path(a: &App) -> PathBuf {
    a.host().exec(|_, p| p.map(|p| p.recovery_path())).unwrap().unwrap()
}

fn edit(a: &mut App, name: &str) {
    a.run("param.set", json!({"name": name, "expr": "10 mm"})).unwrap();
    a.wait_idle();
}

/// A saved project at `<root>/p.gus`, with dirs under `root`.
fn saved_project(root: &Path) -> (App, PathBuf) {
    let mut a = app_with(dirs_in(root));
    edit(&mut a, "w");
    let path = root.join("p.gus");
    a.save_as(path.clone());
    a.wait_idle();
    assert!(!a.is_dirty() && a.dialog.is_none(), "{:?}", a.notice);
    (a, path)
}

fn is_unsaved(a: &App, want: &Pending) -> bool {
    matches!(&a.dialog, Some(Dialog::Unsaved { pending, .. }) if pending == want)
}

#[test]
fn close_request_on_a_clean_project_closes() {
    let root = temp_dir("clean");
    let (a, _) = saved_project(&root);
    let mut h = harness(a);
    h.run();
    request_close(&mut h);
    assert!(sent(&h, &egui::ViewportCommand::CancelClose));
    assert!(sent(&h, &egui::ViewportCommand::Close));
    assert!(h.state().allow_close);
    assert!(h.state().dialog.is_none());
}

#[test]
fn close_with_unsaved_changes_asks_first() {
    let root = temp_dir("unsaved");
    let mut a = app_with(dirs_in(&root));
    edit(&mut a, "w");
    a.maybe_snapshot(true);
    let recovery = recovery_path(&a);
    assert!(recovery.exists());
    let mut h = harness(a);
    h.run();
    request_close(&mut h);
    assert!(sent(&h, &egui::ViewportCommand::CancelClose));
    assert!(!h.state().allow_close);
    h.run();
    h.get_by_label("Save changes before closing?");
    // Cancel keeps the window and the work.
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().dialog.is_none());
    assert!(!h.state().allow_close);
    assert!(recovery.exists());
    // Asked again; "Close without saving" closes and drops the recovery copy.
    request_close(&mut h);
    h.run();
    h.get_by_label("Close without saving").click();
    h.run();
    assert!(h.state().allow_close);
    assert!(!recovery.exists());
}

#[test]
fn save_and_close_without_a_file_asks_for_a_path() {
    let root = temp_dir("saveas");
    let mut a = app_with(dirs_in(&root));
    edit(&mut a, "w");
    let mut h = harness(a);
    h.run();
    request_close(&mut h);
    h.run();
    h.get_by_label("Save and close").click();
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::Path { purpose: PathPurpose::SaveAs, .. })));
    assert!(!h.state().allow_close);
}

#[test]
fn failed_save_keeps_the_window_open_and_says_why() {
    let root = temp_dir("savefail");
    let (mut a, _) = saved_project(&root);
    // The project file's folder disappears: the next save fails.
    let gone = root.join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    a.save_as(gone.join("q.gus"));
    a.wait_idle();
    std::fs::remove_dir_all(&gone).unwrap();
    edit(&mut a, "x");
    let mut h = harness(a);
    h.run();
    request_close(&mut h);
    h.run();
    h.get_by_label("Save and close").click();
    settle(&mut h);
    assert!(
        matches!(&h.state().dialog, Some(Dialog::Unsaved { pending: Pending::Close, save_failed: Some(_) })),
        "{:?}",
        h.state().dialog
    );
    h.get_by_label_contains("Saving failed");
    assert!(!h.state().allow_close);
    assert!(h.state().is_dirty());
}

#[test]
fn save_and_close_saves_then_closes() {
    let root = temp_dir("saveclose");
    let (mut a, path) = saved_project(&root);
    edit(&mut a, "x");
    let mut h = harness(a);
    h.run();
    request_close(&mut h);
    h.run();
    h.get_by_label("Save and close").click();
    settle(&mut h);
    assert!(h.state().allow_close, "{:?} {:?}", h.state().dialog, h.state().notice);
    let mut b = app_with(dirs_in(&root));
    drop(h);
    b.open(path).unwrap();
    b.wait_idle();
    assert!(b.snapshot.doc.params.iter().any(|p| p.name == "x"));
}

#[test]
fn close_decides_from_the_host_not_a_stale_snapshot() {
    // Review H6: an agent edit that the window has not drawn yet must still be asked about.
    let root = temp_dir("stale");
    let (mut a, _) = saved_project(&root);
    let mut agent = gusset_commands::CmdCtx::agent("bot", 1);
    a.host().call(agent.clone(), "branch.switch", json!({"branch": "main"})).unwrap();
    agent.approved = true;
    a.host().call(agent, "param.set", json!({"name": "agent_p", "expr": "1 mm"})).unwrap();
    assert!(!a.snapshot.dirty, "the snapshot is stale on purpose");
    a.request(Pending::Close);
    assert!(is_unsaved(&a, &Pending::Close), "{:?}", a.dialog);
    assert!(!a.allow_close);
}

#[test]
fn close_while_work_is_running_offers_to_wait() {
    let root = temp_dir("busy");
    let (mut a, _) = saved_project(&root);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    a.submit(
        move |_, _| {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(10));
        },
        crate::app::Done::Refresh,
    );
    let mut h = harness(a);
    h.run();
    request_close(&mut h);
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::Busy { pending: Pending::Close })), "{:?}", h.state().dialog);
    h.get_by_label("Wait, then continue").click();
    h.run();
    assert!(!h.state().allow_close);
    tx.send(()).unwrap();
    run_until(&mut h, "the close after the job", |h| h.state().allow_close);
}

#[test]
fn opening_another_file_with_unsaved_changes_asks_first() {
    // Review C1.
    let root = temp_dir("open");
    let (other, other_path) = saved_project(&root.join("other"));
    drop(other);
    let mut a = app_with(dirs_in(&root));
    edit(&mut a, "mine");
    a.maybe_snapshot(true);
    let recovery = recovery_path(&a);
    a.dialog =
        Some(Dialog::Path { purpose: PathPurpose::Open, text: other_path.to_string_lossy().into(), error: None });
    let mut h = harness(a);
    h.run();
    h.get_by_label("OK").click();
    h.run();
    assert!(is_unsaved(h.state(), &Pending::Open(other_path.clone())), "{:?}", h.state().dialog);
    h.get_by_label("Save changes before opening another project?");
    // Cancel: nothing happens.
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().dialog.is_none());
    assert!(h.state().project_file().is_none());
    assert!(recovery.exists());
    // Ask again and open without saving: the other file is loaded in the background.
    h.state_mut().request(Pending::Open(other_path.clone()));
    h.run();
    h.get_by_label("Open without saving").click();
    run_until(&mut h, "the other project", |h| h.state().project_file() == Some(other_path.as_path()));
    settle(&mut h);
    assert!(h.state().snapshot.doc.params.iter().any(|p| p.name == "w"));
    assert!(!recovery.exists(), "the discarded work's recovery copy is gone, as chosen");
}

#[test]
fn unasked_changes_survive_opening_another_file() {
    // If the window opens a file while the old project is dirty and the user never chose
    // to discard (here: through the automation API), the work stays in recovery.
    let root = temp_dir("keeprec");
    let (other, other_path) = saved_project(&root.join("other"));
    drop(other);
    let mut a = app_with(dirs_in(&root));
    edit(&mut a, "mine");
    let recovery = recovery_path(&a);
    a.open(other_path).unwrap();
    assert!(recovery.exists());
}

/// Leaves a recovery copy newer than the saved file at `root/p.gus`.
fn crashed_project(root: &Path) -> PathBuf {
    let (mut a, path) = saved_project(root);
    edit(&mut a, "unsaved");
    a.maybe_snapshot(true);
    // Dropping without closing is what a crash leaves behind (the recovery file stays).
    drop(a);
    path
}

fn reopen_with_recovery(root: &Path, path: &Path) -> Harness<'static, App> {
    let mut b = app_with(dirs_in(root));
    b.open(path.to_path_buf()).unwrap();
    assert!(matches!(b.dialog, Some(Dialog::Recovery { .. })), "{:?}", b.dialog);
    let mut h = harness(b);
    h.run();
    h.get_by_label("Recover unsaved work?");
    h
}

#[test]
fn recovery_keep_both_writes_a_separate_copy() {
    // Review C4.
    let root = temp_dir("keepboth");
    let path = crashed_project(&root);
    let mut h = reopen_with_recovery(&root, &path);
    let recovery = recovery_path(h.state());
    assert!(recovery.exists());
    h.get_by_label("Keep both").click();
    settle(&mut h);
    let copy = root.join("p (recovered).gus");
    assert!(copy.exists(), "{:?}", h.state().notice);
    assert!(!recovery.exists());
    h.get_by_label_contains("Recovered work saved as");
    // The window keeps the saved version; the copy holds the recovered work.
    assert!(!h.state().snapshot.doc.params.iter().any(|p| p.name == "unsaved"));
    drop(h);
    let mut c = app_with(dirs_in(&root));
    c.open(copy).unwrap();
    c.wait_idle();
    assert!(c.dialog.is_none());
    assert!(c.snapshot.doc.params.iter().any(|p| p.name == "unsaved"));
    // A second "Keep both" picks the next free name.
    let path2 = crashed_project(&root.join("again"));
    let mut h = reopen_with_recovery(&root.join("again"), &path2);
    std::fs::write(root.join("again").join("p (recovered).gus"), b"taken").unwrap();
    h.get_by_label("Keep both").click();
    settle(&mut h);
    assert!(root.join("again").join("p (recovered 2).gus").exists(), "{:?}", h.state().dialog);
}

#[test]
fn recovery_recover_and_open_saved() {
    let root = temp_dir("recover");
    let path = crashed_project(&root);
    let mut h = reopen_with_recovery(&root, &path);
    h.get_by_label("Recover unsaved work").click();
    settle(&mut h);
    assert!(h.state().snapshot.doc.params.iter().any(|p| p.name == "unsaved"));
    assert!(h.state().is_dirty());
    drop(h);

    let root = temp_dir("opensaved");
    let path = crashed_project(&root);
    let mut h = reopen_with_recovery(&root, &path);
    let recovery = recovery_path(h.state());
    h.get_by_label("Open last saved version").click();
    settle(&mut h);
    assert!(!h.state().snapshot.doc.params.iter().any(|p| p.name == "unsaved"));
    assert!(!recovery.exists());
}

#[test]
fn export_asks_before_replacing_a_file() {
    // Review L7.
    let root = temp_dir("export");
    let (a, _) = saved_project(&root);
    let existing = root.join("part.stl");
    std::fs::write(&existing, b"solid old\nendsolid old\n").unwrap();
    let mut h = harness(a);
    h.state_mut().dialog =
        Some(Dialog::Path { purpose: PathPurpose::ExportStl, text: existing.to_string_lossy().into(), error: None });
    h.run();
    h.get_by_label("OK").click();
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::Overwrite { .. })), "{:?}", h.state().dialog);
    h.get_by_label("Replace the existing file?");
    // Cancel goes back to the path dialog and leaves the file alone.
    h.get_by_label("Cancel").click();
    h.run();
    assert!(matches!(h.state().dialog, Some(Dialog::Path { purpose: PathPurpose::ExportStl, .. })));
    assert_eq!(std::fs::read(&existing).unwrap(), b"solid old\nendsolid old\n");
    // A new file name does not ask.
    let fresh = root.join("new.stl");
    h.state_mut().dialog =
        Some(Dialog::Path { purpose: PathPurpose::ExportStl, text: fresh.to_string_lossy().into(), error: None });
    h.run();
    h.get_by_label("OK").click();
    h.run();
    assert!(!matches!(h.state().dialog, Some(Dialog::Overwrite { .. })));
}

#[test]
fn startup_offers_unsaved_work_of_a_never_saved_project() {
    let root = temp_dir("untitled");
    let mut a = app_with(dirs_in(&root));
    edit(&mut a, "lost");
    a.maybe_snapshot(true);
    drop(a); // a crash: the recovery copy of the Untitled project stays

    let b = app_with(dirs_in(&root));
    assert!(matches!(&b.dialog, Some(Dialog::Unrecovered { items }) if items.len() == 1), "{:?}", b.dialog);
    let mut h = harness(b);
    h.run();
    h.get_by_label_contains("Untitled project");
    h.get_by_label("Recover").click();
    settle(&mut h);
    for _ in 0..200 {
        if h.state().snapshot.doc.params.iter().any(|p| p.name == "lost") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.step();
        h.state_mut().wait_idle();
    }
    let a = h.state();
    assert!(
        a.snapshot.doc.params.iter().any(|p| p.name == "lost"),
        "recovered the parameter; dialog {:?}, notice {:?}",
        a.dialog,
        a.notice.as_ref().map(|n| &n.text)
    );
    assert!(a.snapshot.dirty);
    assert!(a.project_file().is_none(), "still Untitled");
    drop(h);

    // Discarded copies are not offered again.
    let c = app_with(dirs_in(&root));
    let mut h = harness(c);
    h.run();
    if h.query_by_label("Discard").is_some() {
        h.get_by_label("Discard").click();
        settle(&mut h);
    }
    drop(h);
    let d = app_with(dirs_in(&root));
    assert!(d.dialog.is_none(), "{:?}", d.dialog);
}
