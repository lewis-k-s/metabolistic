//! `cargo levels serve`: the viewer with editing.
//!
//! A small HTTP server on 127.0.0.1 (std only). It is for one user on one
//! machine, so it handles one request at a time.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /` | The viewer page, baked from the files now on disk. |
//! | `GET /api/bake?id=vents/caldera` | The bake JSON of one level. |
//! | `POST /api/level?id=vents/caldera` | Body: a level JSON. Parses and builds it, writes `assets/levels/<id>.json`, re-bakes it and answers with the bake. A bad file gets status 400 and `{"error": "..."}`; nothing is written. A file that breaks layout rules is written, and the bake lists the broken rules. |
//!
//! Two checks keep other web pages out: the `Host` header must name this
//! server, and a request with an `Origin` header must come from this server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use metabolistic3d::level_file::{self, LevelFile};
use serde_json::json;

use crate::{Loaded, bake, valid_new_id};

pub const DEFAULT_PORT: u16 = 7878;
/// A level file is some tens of kilobytes; this stops a runaway upload.
const MAX_BODY: usize = 4 * 1024 * 1024;

struct Request {
    method: String,
    path: String,
    query: String,
    host: String,
    origin: Option<String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: &serde_json::Value) -> Self {
        Self { status, content_type: "application/json", body: value.to_string().into_bytes() }
    }
    fn error(status: u16, message: &str) -> Self {
        Self::json(status, &json!({ "error": message }))
    }
}

pub fn run(assets: &Path, port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    println!("level editor: http://127.0.0.1:{port}/   (Ctrl-C stops it)");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let response = match read_request(&mut stream) {
            Ok(request) => handle(&request, assets, port),
            Err(e) => Response::error(400, &e),
        };
        if let Err(e) = write_response(&mut stream, &response) {
            eprintln!("write failed: {e}");
        }
    }
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let timeout = Some(Duration::from_secs(10));
    stream.set_read_timeout(timeout).ok();
    stream.set_write_timeout(timeout).ok();
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let (mut host, mut origin, mut length) = (String::new(), None, 0usize);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.trim().split_once(':') {
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "host" => host = value.to_string(),
                "origin" => origin = Some(value.to_string()),
                "content-length" => length = value.parse().map_err(|_| "bad Content-Length")?,
                _ => {}
            }
        }
    }
    if length > MAX_BODY {
        return Err("body too large".into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Request { method, path: path.to_string(), query: query.to_string(), host, origin, body })
}

fn write_response(stream: &mut TcpStream, r: &Response) -> std::io::Result<()> {
    let reason = match r.status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        r.status,
        r.content_type,
        r.body.len()
    )?;
    stream.write_all(&r.body)
}

/// The value of a query parameter. Level ids have no characters that need decoding.
fn param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
}

/// True if the `Host` and `Origin` headers name this server.
fn same_origin(request: &Request, port: u16) -> bool {
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    hosts.contains(&request.host)
        && request.origin.as_ref().is_none_or(|o| hosts.iter().any(|h| *o == format!("http://{h}")))
}

fn handle(request: &Request, assets: &Path, port: u16) -> Response {
    if !same_origin(request, port) {
        return Response::error(403, "request is not from this server");
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => match page(assets) {
            Ok(html) => Response { status: 200, content_type: "text/html", body: html.into_bytes() },
            Err(e) => Response::error(500, &e),
        },
        ("GET", "/api/bake") => {
            let Some(id) = param(&request.query, "id") else { return Response::error(400, "missing id") };
            match bake_one(assets, id) {
                Ok(b) => Response::json(200, &b),
                Err(e) => Response::error(404, &e),
            }
        }
        ("POST", "/api/level") => {
            let Some(id) = param(&request.query, "id") else { return Response::error(400, "missing id") };
            match save(assets, id, &request.body) {
                Ok(b) => Response::json(200, &b),
                Err(e) => Response::error(400, &e),
            }
        }
        _ => Response::error(404, "not found"),
    }
}

/// The viewer page with every level on disk. A level that does not load is left out.
fn page(assets: &Path) -> Result<String, String> {
    let mut bakes = Vec::new();
    for (id, path) in level_file::list(assets) {
        match Loaded::read(&id, &path) {
            Ok(level) => bakes.push(bake::bake(&level)),
            Err(e) => eprintln!("skipped {id}: {e}"),
        }
    }
    Ok(bake::document(&bake::viewer_html(&bakes, assets)))
}

fn bake_one(assets: &Path, id: &str) -> Result<serde_json::Value, String> {
    valid_new_id(id)?;
    let path = level_file::resolve_path(id, assets);
    Ok(bake::bake(&Loaded::read(id, &path)?))
}

/// Checks the JSON, writes the file in the canonical format and re-bakes it.
fn save(assets: &Path, id: &str, body: &[u8]) -> Result<serde_json::Value, String> {
    valid_new_id(id)?;
    let text = std::str::from_utf8(body).map_err(|_| "body is not UTF-8")?;
    let file = LevelFile::from_json(text).map_err(|e| format!("cannot parse level: {e}"))?;
    file.to_def().map_err(|e| format!("{id}: {e}"))?;
    let path = level_file::resolve_path(id, assets);
    std::fs::create_dir_all(path.parent().ok_or("bad path")?).map_err(|e| e.to_string())?;
    std::fs::write(&path, file.to_json().map_err(|e| e.to_string())? + "\n").map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let level = Loaded::read(id, &path)?;
    println!("saved {id} ({})", if level.errors.is_empty() { "valid".to_string() } else { format!("{} broken rules", level.errors.len()) });
    Ok(bake::bake(&level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(host: &str, origin: Option<&str>) -> Request {
        Request { method: "POST".into(), path: "/".into(), query: String::new(), host: host.into(), origin: origin.map(str::to_string), body: Vec::new() }
    }

    #[test]
    fn other_hosts_and_origins_are_refused() {
        assert!(same_origin(&request("127.0.0.1:7878", None), 7878));
        assert!(same_origin(&request("localhost:7878", Some("http://localhost:7878")), 7878));
        assert!(!same_origin(&request("evil.example:7878", None), 7878));
        assert!(!same_origin(&request("127.0.0.1:7878", Some("https://evil.example")), 7878));
    }

    #[test]
    fn ids_cannot_leave_the_level_folder() {
        assert!(valid_new_id("vents/caldera").is_ok());
        assert!(valid_new_id("vents/../x").is_err());
        assert!(valid_new_id("../vents/x").is_err());
        assert!(valid_new_id("vents/a/b").is_err());
    }
}
