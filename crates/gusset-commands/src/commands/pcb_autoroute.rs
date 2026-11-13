//! Autorouting through the user's Freerouting (ADR 0047 phase 3): the open board as a
//! Specctra DSN in a private temporary folder, Freerouting run on it as a separate
//! process (`-de board.dsn -do board.ses`), its session read back and laid on the board
//! in one undoable edit. Freerouting is GPL and never bundled or downloaded: the user
//! names an install in Settings, and without one the command refuses. The run stops at
//! its timeout or when the session's cancel flag is set (the GUI's Cancel button).

use super::pcb::no_board;
use super::pcb_edit::commit_board;
use super::*;
use crate::pcb_session;
use gusset_pcb::{Limits, specctra};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<PcbAutoroute>()]
}

/// Largest session file read back.
const MAX_SES_BYTES: u64 = 64 << 20;
/// Longest run allowed whatever the settings say (seconds).
const MAX_TIMEOUT_S: u64 = 24 * 3600;

/// A private temporary folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<TempDir, CommandError> {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let p = std::env::temp_dir().join(format!("gusset-autoroute-{}-{n}", std::process::id()));
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&p).map_err(|e| CommandError::new("io", format!("could not make a temporary folder: {e}")))?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How a run ended, when it did not finish.
#[derive(Debug, PartialEq, Eq)]
enum Stopped {
    Cancelled,
    TimedOut,
}

/// Runs `cmd` until it exits, `cancel` is set or `timeout` passes (then it is killed).
fn run_until(
    mut cmd: std::process::Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Result<std::process::ExitStatus, Stopped>, std::io::Error> {
    // A program written just before may be busy for a moment (ETXTBSY while another
    // thread's fork still holds it open).
    let mut tries = 0;
    let mut child = loop {
        match cmd.spawn() {
            Err(e) if e.raw_os_error() == Some(26) && tries < 20 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            r => break r?,
        }
    };
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait()? {
            return Ok(Ok(st));
        }
        let stop = if cancel.load(Ordering::Relaxed) {
            Some(Stopped::Cancelled)
        } else if start.elapsed() >= timeout {
            Some(Stopped::TimedOut)
        } else {
            None
        };
        if let Some(why) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Err(why));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The command line for an install: `java -jar <jar>` or the executable itself.
fn command_for(fr: &pcb_session::Freerouting, dsn: &Path, ses: &Path, passes: u32) -> std::process::Command {
    let is_jar = fr.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar"));
    let mut cmd = if is_jar {
        let mut c = std::process::Command::new(fr.java.clone().unwrap_or_else(|| PathBuf::from("java")));
        c.arg("-jar").arg(&fr.path);
        c
    } else {
        std::process::Command::new(&fr.path)
    };
    cmd.arg("-de").arg(dsn).arg("-do").arg(ses).arg("-mp").arg(passes.to_string());
    cmd.args(&fr.args);
    cmd.stdin(std::process::Stdio::null());
    cmd
}

/// Routes the open board with the user's Freerouting.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbAutoroute {
    /// Seconds (default: the setting's).
    #[serde(default)]
    pub timeout_s: Option<u64>,
    /// Routing passes (default 20).
    #[serde(default)]
    pub passes: Option<u32>,
}

impl Command for PcbAutoroute {
    const ID: &'static str = "pcb.autoroute";
    const TITLE: &'static str = "Autoroute";
    const DESCRIPTION: &'static str =
        "Route the open board with the Freerouting the user set up in Settings (DSN out, SES back; undoable).";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let fr = s.freerouting.clone().ok_or_else(|| {
            CommandError::new("not_available", "no Freerouting is set up")
                .hint("name your Freerouting (jar or program) in Settings > PCB; Gusset does not download it")
        })?;
        // An agent starting a program on the user's machine needs the user's yes.
        if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
            return Err(CommandError::approval("running the Freerouting autorouter"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let timeout = self.timeout_s.unwrap_or(fr.timeout_s).clamp(1, MAX_TIMEOUT_S);
        let passes = self.passes.unwrap_or(20).clamp(1, 10_000);
        let dir = TempDir::new()?;
        let (dsn, ses) = (dir.0.join("board.dsn"), dir.0.join("board.ses"));
        let text = specctra::write_dsn(&open.board, &open.rules, "board");
        std::fs::write(&dsn, text).map_err(|e| CommandError::new("io", format!("could not write the DSN: {e}")))?;
        let mut cmd = command_for(&fr, &dsn, &ses, passes);
        cmd.current_dir(&dir.0);
        let log = |n: &str| std::fs::File::create(dir.0.join(n)).map(std::process::Stdio::from);
        if let (Ok(o), Ok(e)) = (log("out.log"), log("err.log")) {
            cmd.stdout(o).stderr(e);
        }
        let cancel = std::sync::Arc::clone(&s.autoroute_cancel);
        cancel.store(false, Ordering::Relaxed);
        let status = run_until(cmd, &cancel, Duration::from_secs(timeout)).map_err(|e| {
            CommandError::new("io", format!("could not start Freerouting: {e}"))
                .hint("check the path (and Java for a .jar) in Settings > PCB")
        })?;
        match status {
            Err(Stopped::Cancelled) => return Err(CommandError::new("cancelled", "the autoroute was cancelled")),
            Err(Stopped::TimedOut) => {
                return Err(CommandError::new("timeout", format!("Freerouting did not finish in {timeout} s"))
                    .hint("give a longer timeout_s, or fewer passes"));
            }
            Ok(st) if !st.success() || !ses.is_file() => {
                let tail: String = std::fs::read_to_string(dir.0.join("err.log"))
                    .unwrap_or_default()
                    .chars()
                    .rev()
                    .take(400)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .collect();
                return Err(CommandError::new("io", format!("Freerouting wrote no session (exit {:?})", st.code()))
                    .details(json!({ "stderr_user_text": tail })));
            }
            Ok(_) => {}
        }
        let f = crate::import::open_regular(&ses, "board.ses", MAX_SES_BYTES)?;
        let bytes = crate::import::read_capped(f, MAX_SES_BYTES, &AtomicBool::new(false))?;
        let text = String::from_utf8(bytes).map_err(|_| CommandError::new("io", "the session is not UTF-8 text"))?;
        let sesn = specctra::read_ses(&text, &specctra::Limits::default())
            .map_err(|e| CommandError::new("io", format!("Specctra session: {e}")))?;
        let mut b = open.board.clone();
        let (tracks, vias) = b.apply_session(&sesn, &open.rules, true, &Limits::default()).map_err(|e| match e {
            gusset_pcb::board::Error::Edit(m) => CommandError::args(m),
            e => CommandError::new("io", format!("KiCad board: {e}")),
        })?;
        let extra = json!({ "tracks": tracks, "vias": vias, "seconds_limit": timeout });
        commit_board(s, ctx, "Autoroute", open.file.clone(), &b, None, extra)
    }
}
