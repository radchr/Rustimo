use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;

use crate::Notebook;

const PAGE: &str = include_str!("frontend.html");
const MAX_HEADER: usize = 8192;
const MAX_BODY: usize = 65536;

#[derive(Deserialize)]
struct SignalRequest {
    name: String,
    value: serde_json::Value,
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub fn serve(notebook: Notebook, addr: &str) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    let actual_addr = listener.local_addr()?;
    let notebook = Arc::new(Mutex::new(notebook));
    println!("Rustimo app: http://{actual_addr}");
    for stream in listener.incoming() {
        let stream = stream?;
        let notebook = Arc::clone(&notebook);
        let expected_host = actual_addr.to_string();
        std::thread::spawn(move || {
            if let Err(error) = handle(stream, &notebook, &expected_host) {
                eprintln!("Rustimo HTTP error: {error}");
            }
        });
    }
    Ok(())
}

fn handle(mut stream: TcpStream, notebook: &Arc<Mutex<Notebook>>, host: &str) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let Request {
        method,
        path,
        headers,
        body,
    } = read_request(&mut stream)?;
    let port = host.rsplit(':').next().unwrap_or("3001");
    let alternate_host = format!("localhost:{port}");
    let allowed_host = |value: &str| value == host || value == alternate_host;
    if !headers
        .iter()
        .any(|(key, value)| key == "host" && allowed_host(value))
    {
        return send_response(&mut stream, "403 Forbidden", "text/plain", b"invalid host");
    }
    if let Some((_, origin)) = headers.iter().find(|(key, _)| key == "origin") {
        let expected = format!("http://{host}");
        let alternate = format!("http://{alternate_host}");
        if origin != &expected && origin != &alternate {
            return send_response(
                &mut stream,
                "403 Forbidden",
                "text/plain",
                b"invalid origin",
            );
        }
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/") => send_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        ("GET", "/api/state") => {
            let state = notebook
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .snapshot();
            let json = serde_json::to_vec(&state).map_err(io::Error::other)?;
            send_response(&mut stream, "200 OK", "application/json", &json)
        }
        ("POST", "/api/signal") => {
            let request: SignalRequest = match serde_json::from_slice(&body) {
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
            let mut notebook = notebook.lock().unwrap_or_else(|e| e.into_inner());
            match notebook.set_signal(&request.name, request.value) {
                Ok(_) => {
                    let json =
                        serde_json::to_vec(&notebook.snapshot()).map_err(io::Error::other)?;
                    send_response(&mut stream, "200 OK", "application/json", &json)
                }
                Err(error) => {
                    let json = serde_json::to_vec(&error).map_err(io::Error::other)?;
                    send_response(
                        &mut stream,
                        "422 Unprocessable Entity",
                        "application/json",
                        &json,
                    )
                }
            }
        }
        _ => send_response(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete HTTP header",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if position > MAX_HEADER {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP header too large",
                ));
            }
            break position + 4;
        }
        if bytes.len() > MAX_HEADER {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP header too large",
            ));
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP header"))?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    if !matches!(parts.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid request line",
        ));
    }
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(key, _)| key == "content-length")
        .map(|(_, value)| value.parse::<usize>())
        .transpose()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid content length"))?
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP body too large",
        ));
    }
    while bytes.len() - header_end < length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete HTTP body",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(Request {
        method,
        path,
        headers,
        body: bytes[header_end..header_end + length].to_vec(),
    })
}

fn send_response(stream: &mut TcpStream, status: &str, mime: &str, body: &[u8]) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}
