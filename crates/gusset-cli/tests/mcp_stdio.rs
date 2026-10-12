//! `gusset mcp` end to end: spawns the binary, speaks newline-delimited JSON-RPC over
//! stdio (legacy `initialize` handshake), lists tools and calls read tools. Without OCCT
//! the CLI falls back to the mock kernel, so this runs in every build.

// Test helpers outside `#[test]` fns: the same allowances clippy.toml gives unit tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

struct McpProc {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gusset-cli-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

impl McpProc {
    /// Starts `gusset mcp <args>` with isolated per-user directories (no running app is
    /// found, and recovery data stays in `home`).
    fn spawn(args: &[&str], home: &Path) -> McpProc {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gusset"))
            .arg("mcp")
            .args(args)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        McpProc { child, stdin, lines, next_id: 1 }
    }

    fn send(&mut self, msg: &Value) {
        let w = self.stdin.as_mut().unwrap();
        writeln!(w, "{msg}").unwrap();
        w.flush().unwrap();
    }

    /// Sends a request and returns its response. Every line on stdout must be JSON-RPC
    /// (nothing else may leak into the protocol stream).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self.lines.recv_timeout(Duration::from_secs(120)).expect("no response from gusset mcp");
            let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("non-JSON on stdout ({e}): {line}"));
            assert_eq!(v["jsonrpc"], "2.0", "{line}");
            if v["id"] == json!(id) {
                assert!(v.get("error").is_none(), "{method}: {v}");
                return v["result"].clone();
            }
        }
    }

    fn call_tool(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": args}))
    }

    fn initialize(&mut self) -> Value {
        let r = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "cli-test", "version": "1.0"}
            }),
        );
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        r
    }

    /// Closes stdin (the client disconnects) and waits for a clean exit.
    fn finish(mut self) {
        drop(self.stdin.take());
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "gusset mcp exited with {status}");
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "gusset mcp did not exit after stdin closed");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn png_size(b64: &str) -> (u32, u32) {
    // Minimal base64 decode (standard alphabet, padded) to read the PNG header.
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    };
    let mut out = Vec::new();
    for chunk in b64.as_bytes().chunks(4).take(8) {
        let n = chunk.iter().fold(0u32, |acc, &c| (acc << 6) | val(c)) << (6 * (4 - chunk.len() as u32));
        out.extend_from_slice(&n.to_be_bytes()[1..]);
    }
    assert_eq!(&out[..8], b"\x89PNG\r\n\x1a\n");
    let be = |i: usize| u32::from_be_bytes([out[i], out[i + 1], out[i + 2], out[i + 3]]);
    (be(16), be(20))
}

#[test]
fn mcp_stdio_handshake_tools_and_read_only() {
    let home = tmpdir("ro");
    let mut p = McpProc::spawn(&["--no-attach", "--read-only"], &home);
    let init = p.initialize();
    assert_eq!(init["serverInfo"]["name"].as_str().map(|s| s.contains("gusset")), Some(true), "{init}");
    assert!(init["instructions"].as_str().unwrap().contains("untrusted"));

    let tools = p.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for want in ["query_model", "render_view", "get_text", "param_set", "feature_extrude"] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }

    // A read tool works.
    let r = p.call_tool("query_model", json!({}));
    assert_ne!(r["isError"], true, "{r}");
    assert_eq!(text_json(&r)["units"], "mm", "{r}");

    // render_view goes through the real renderer (GPU or CPU fallback) and returns a PNG.
    let r = p.call_tool("render_view", json!({"width": 64, "height": 48}));
    assert_ne!(r["isError"], true, "{r}");
    let img = r["content"].as_array().unwrap().iter().find(|c| c["type"] == "image").unwrap();
    assert_eq!(img["mimeType"], "image/png");
    assert_eq!(png_size(img["data"].as_str().unwrap()), (64, 48));

    // --read-only rejects mutating tools.
    let r = p.call_tool("param_set", json!({"name": "width", "expr": "40 mm"}));
    assert_eq!(r["isError"], true, "{r}");
    assert_eq!(text_json(&r)["error"]["code"], "read_only", "{r}");

    p.finish();
    let _ = std::fs::remove_dir_all(&home);
}

/// Headless on a new project file: the agent's work is saved on exit. Linux only, where
/// the per-user data directories follow the XDG variables set by the test.
#[cfg(target_os = "linux")]
#[test]
fn mcp_stdio_headless_saves_agent_work() {
    let home = tmpdir("save");
    let project = home.join("part.gus");
    let mut p = McpProc::spawn(&["--no-attach", project.to_str().unwrap()], &home);
    p.initialize();
    let r = p.call_tool("param_set", json!({"name": "width", "expr": "40 mm"}));
    assert_ne!(r["isError"], true, "{r}");
    p.finish();
    assert!(project.exists(), "agent work saved on exit");
    // The saved project is a valid .gus the CLI can read back.
    let out = Command::new(env!("CARGO_BIN_EXE_gusset")).arg("text").arg(&project).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&home);
}

/// A tool result's JSON, sent once as text content.
fn text_json(r: &Value) -> Value {
    let t = r["content"].as_array().and_then(|c| c.iter().find(|x| x["type"] == "text")).map(|x| x["text"].clone());
    serde_json::from_str(t.as_ref().and_then(Value::as_str).unwrap_or("null")).unwrap_or(Value::Null)
}
