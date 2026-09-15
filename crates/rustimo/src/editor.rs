//! Local source editor and Cargo build supervisor for notebook examples.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::server::{PAGE, Request, read_request, send_response, valid_request_origin};
use crate::source::{parse_cells, replace_cell};

#[derive(Clone, Debug, Serialize)]
pub struct BuildDiagnostic {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u64>,
}

#[derive(Deserialize)]
struct SaveRequest {
    source: String,
    base_source: String,
}

#[derive(Deserialize)]
struct SaveCellRequest {
    name: String,
    source: String,
    base_source: String,
}

struct Worker {
    child: Child,
    executable: PathBuf,
    addr: String,
}

impl Worker {
    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(self.executable);
    }
}

struct Editor {
    source_file: PathBuf,
    workspace: PathBuf,
    example: String,
    worker: Option<Worker>,
    source_status: &'static str,
    diagnostics: Vec<BuildDiagnostic>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
        }
    }
}

/// Edit a Rustimo example with a browser on localhost. The editor and worker
/// are separate processes; only a successfully built worker replaces the old one.
pub fn serve_edit(source_file: impl AsRef<Path>, addr: &str) -> io::Result<()> {
    let source_file = source_file.as_ref().canonicalize()?;
    let examples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .canonicalize()?;
    if source_file.parent() != Some(examples.as_path())
        || source_file.extension().and_then(|ext| ext.to_str()) != Some("rs")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the first editor version accepts a .rs file in crates/rustimo/examples",
        ));
    }
    let example = source_file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid example name"))?
        .to_owned();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing workspace"))?
        .to_path_buf();
    let listener = TcpListener::bind(addr)?;
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "editor must bind to localhost",
        ));
    }
    let mut editor = Editor {
        source_file,
        workspace,
        example,
        worker: None,
        source_status: "building",
        diagnostics: Vec::new(),
    };
    editor.rebuild()?;
    if editor.worker.is_none() {
        eprintln!("Initial notebook build failed; the editor is available for fixing the source");
    }
    let actual_addr = listener.local_addr()?.to_string();
    let editor = Arc::new(Mutex::new(editor));
    println!("Rustimo editor: http://{actual_addr}");
    for stream in listener.incoming() {
        let stream = stream?;
        let editor = Arc::clone(&editor);
        let host = actual_addr.clone();
        std::thread::spawn(move || {
            if let Err(error) = handle(stream, &editor, &host) {
                eprintln!("Rustimo editor HTTP error: {error}");
            }
        });
    }
    Ok(())
}

fn handle(mut stream: TcpStream, editor: &Arc<Mutex<Editor>>, host: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let Request {
        method,
        path,
        headers,
        body,
    } = read_request(&mut stream)?;
    if !valid_request_origin(&headers, host) {
        return send_response(
            &mut stream,
            "403 Forbidden",
            "text/plain",
            b"invalid host or origin",
        );
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/") => send_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        ("GET", "/api/source") => {
            let editor = editor.lock().unwrap_or_else(|e| e.into_inner());
            let source = fs::read_to_string(&editor.source_file)?;
            let json = serde_json::json!({
                "source": source,
                "cells": parse_cells(&source).unwrap_or_default(),
                "filename": editor.source_file.file_name().and_then(|name| name.to_str()),
                "source_status": editor.source_status,
                "diagnostics": editor.diagnostics,
            });
            send_json(&mut stream, "200 OK", &json)
        }
        ("GET", "/api/state") => {
            let editor = editor.lock().unwrap_or_else(|e| e.into_inner());
            let state = editor.state()?;
            send_json(&mut stream, "200 OK", &state)
        }
        ("POST", "/api/signal" | "/api/run") => {
            let editor = editor.lock().unwrap_or_else(|e| e.into_inner());
            let Some(worker) = editor.worker.as_ref() else {
                return send_json(
                    &mut stream,
                    "503 Service Unavailable",
                    &serde_json::json!({"message": "notebook has not compiled yet"}),
                );
            };
            let (status, mut state) = worker_request(&worker.addr, "POST", &path, &body)?;
            if status == 200 {
                editor.decorate_state(&mut state);
            }
            send_json(
                &mut stream,
                if status == 200 {
                    "200 OK"
                } else {
                    "422 Unprocessable Entity"
                },
                &state,
            )
        }
        ("POST", "/api/source") => {
            let request: SaveRequest = match serde_json::from_slice(&body) {
                Ok(request) => request,
                Err(error) => {
                    return send_response(
                        &mut stream,
                        "400 Bad Request",
                        "text/plain",
                        error.to_string().as_bytes(),
                    );
                }
            };
            let mut editor = editor.lock().unwrap_or_else(|e| e.into_inner());
            let current = fs::read_to_string(&editor.source_file)?;
            if current != request.base_source {
                return send_json(
                    &mut stream,
                    "409 Conflict",
                    &serde_json::json!({"message": "source changed since it was opened; reload before saving"}),
                );
            }
            if current != request.source {
                fs::write(&editor.source_file, &request.source)?;
                editor.source_status = "building";
                editor.diagnostics.clear();
                editor.rebuild()?;
            }
            let state = editor.state()?;
            let json = serde_json::json!({
                "source": request.source,
                "cells": parse_cells(&request.source).unwrap_or_default(),
                "source_status": editor.source_status,
                "diagnostics": editor.diagnostics,
                "state": state,
            });
            send_json(&mut stream, "200 OK", &json)
        }
        ("POST", "/api/cell") => {
            let request: SaveCellRequest = match serde_json::from_slice(&body) {
                Ok(request) => request,
                Err(error) => {
                    return send_json(
                        &mut stream,
                        "400 Bad Request",
                        &serde_json::json!({"message": error.to_string()}),
                    );
                }
            };
            let mut editor = editor.lock().unwrap_or_else(|e| e.into_inner());
            let current = fs::read_to_string(&editor.source_file)?;
            if current != request.base_source {
                return send_json(
                    &mut stream,
                    "409 Conflict",
                    &serde_json::json!({"message": "source changed since it was opened; reload before saving"}),
                );
            }
            let updated = match replace_cell(&current, &request.name, &request.source) {
                Ok(updated) => updated,
                Err(error) => {
                    return send_json(
                        &mut stream,
                        "422 Unprocessable Entity",
                        &serde_json::json!({"message": error}),
                    );
                }
            };
            if updated != current {
                fs::write(&editor.source_file, &updated)?;
                editor.source_status = "building";
                editor.diagnostics.clear();
                editor.rebuild()?;
            }
            let state = editor.state()?;
            let json = serde_json::json!({
                "source": updated,
                "cells": parse_cells(&updated).unwrap_or_default(),
                "source_status": editor.source_status,
                "diagnostics": editor.diagnostics,
                "state": state,
            });
            send_json(&mut stream, "200 OK", &json)
        }
        _ => send_response(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn send_json(stream: &mut TcpStream, status: &str, value: &serde_json::Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    send_response(stream, status, "application/json", &bytes)
}

impl Editor {
    fn state(&self) -> io::Result<serde_json::Value> {
        let mut state = if let Some(worker) = &self.worker {
            worker_request(&worker.addr, "GET", "/api/state", &[])?.1
        } else {
            serde_json::json!({"cells": [], "topo_order": []})
        };
        self.decorate_state(&mut state);
        Ok(state)
    }

    fn decorate_state(&self, state: &mut serde_json::Value) {
        state["source_status"] = serde_json::json!(self.source_status);
        state["diagnostics"] = serde_json::json!(self.diagnostics);
        if self.source_status == "stale"
            && let Some(cells) = state
                .get_mut("cells")
                .and_then(serde_json::Value::as_array_mut)
        {
            for cell in cells {
                if cell.get("status").and_then(serde_json::Value::as_str) == Some("success") {
                    cell["status"] = serde_json::json!("stale");
                }
            }
        }
    }

    fn rebuild(&mut self) -> io::Result<()> {
        let output = Command::new("cargo")
            .arg("build")
            .arg("--manifest-path")
            .arg(self.workspace.join("Cargo.toml"))
            .args([
                "-p",
                "rustimo",
                "--example",
                &self.example,
                "--message-format=json",
            ])
            .current_dir(&self.workspace)
            .output()?;
        self.diagnostics = parse_diagnostics(&output.stdout);
        if !output.status.success() {
            if self.diagnostics.is_empty() {
                self.diagnostics.push(BuildDiagnostic {
                    message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                    file: None,
                    line: None,
                });
            }
            self.source_status = "stale";
            return Ok(());
        }
        let old_state = self.worker.as_ref().and_then(|worker| {
            worker_request(&worker.addr, "GET", "/api/state", &[])
                .ok()
                .map(|(_, value)| value)
        });
        let worker = match self.start_worker() {
            Ok(worker) => worker,
            Err(error) => {
                self.diagnostics.push(BuildDiagnostic {
                    message: error.to_string(),
                    file: None,
                    line: None,
                });
                self.source_status = "stale";
                return Ok(());
            }
        };
        if let Some(state) = old_state {
            replay_signals(&state, &worker.addr);
        }
        if let Some(previous) = self.worker.replace(worker) {
            previous.stop();
        }
        self.diagnostics.clear();
        self.source_status = "current";
        Ok(())
    }

    fn start_worker(&self) -> io::Result<Worker> {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    self.workspace.join(path)
                }
            })
            .unwrap_or_else(|| self.workspace.join("target"));
        let built = target.join("debug").join("examples").join(format!(
            "{}{}",
            self.example,
            std::env::consts::EXE_SUFFIX
        ));
        let copies = target.join("rustimo-workers");
        fs::create_dir_all(&copies)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let executable = copies.join(format!(
            "{}-{}-{nonce}{}",
            self.example,
            std::process::id(),
            std::env::consts::EXE_SUFFIX
        ));
        fs::copy(built, &executable)?;
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let addr = format!("127.0.0.1:{port}");
        let child = match Command::new(&executable)
            .env("RUSTIMO_ADDR", &addr)
            .current_dir(&self.workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&executable);
                return Err(error);
            }
        };
        let mut worker = Worker {
            child,
            executable,
            addr,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if worker.child.try_wait()?.is_some() {
                worker.stop();
                return Err(io::Error::other(
                    "new notebook worker exited before it became ready",
                ));
            }
            if worker_request(&worker.addr, "GET", "/api/state", &[]).is_ok() {
                return Ok(worker);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        worker.stop();
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "new notebook worker did not become ready",
        ))
    }
}

fn parse_diagnostics(stdout: &[u8]) -> Vec<BuildDiagnostic> {
    stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let event: serde_json::Value = serde_json::from_slice(line).ok()?;
            if event.get("reason")?.as_str()? != "compiler-message" {
                return None;
            }
            let message = event.get("message")?;
            if message.get("level")?.as_str()? != "error" {
                return None;
            }
            let span = message.get("spans")?.as_array()?.iter().find(|span| {
                span.get("is_primary").and_then(serde_json::Value::as_bool) == Some(true)
            });
            Some(BuildDiagnostic {
                message: message.get("message")?.as_str()?.to_owned(),
                file: span.and_then(|span| span.get("file_name")?.as_str().map(str::to_owned)),
                line: span.and_then(|span| span.get("line_start")?.as_u64()),
            })
        })
        .collect()
}

fn replay_signals(old_state: &serde_json::Value, new_addr: &str) {
    let Some(cells) = old_state.get("cells").and_then(serde_json::Value::as_array) else {
        return;
    };
    for cell in cells {
        let Some(widget) = cell
            .get("view")
            .filter(|view| view.get("kind").and_then(serde_json::Value::as_str) == Some("widget"))
            .and_then(|view| view.get("value"))
        else {
            continue;
        };
        let (Some(name), Some(value)) = (
            widget.get("name").and_then(serde_json::Value::as_str),
            widget.get("value"),
        ) else {
            continue;
        };
        if let Ok(body) = serde_json::to_vec(&serde_json::json!({"name": name, "value": value})) {
            let _ = worker_request(new_addr, "POST", "/api/signal", &body);
        }
    }
}

fn worker_request(
    addr: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> io::Result<(u16, serde_json::Value)> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid worker HTTP response")
        })?;
    let header = std::str::from_utf8(&response[..split]).map_err(io::Error::other)?;
    let status = header
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid worker status"))?;
    let json = serde_json::from_slice(&response[split + 4..]).map_err(io::Error::other)?;
    Ok((status, json))
}

#[cfg(test)]
mod tests {
    use super::parse_diagnostics;

    #[test]
    fn compiler_error_keeps_primary_source_location() {
        let event = br#"{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `x`","spans":[{"is_primary":true,"file_name":"examples/basic.rs","line_start":7}]}}"#;
        let diagnostics = parse_diagnostics(event);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].file.as_deref(), Some("examples/basic.rs"));
        assert_eq!(diagnostics[0].line, Some(7));
    }
}
