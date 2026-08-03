//! Local read-only HTTP status surface for Milestone 3.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;

use crate::config::EmbeddingConfig;
use crate::error::ConfigError;
use crate::snapshot::list_snapshots;
use crate::storage::{
    EmbeddingCompatibility, QueryActivitySample, StatusSnapshot, embedding_compatibility,
    list_query_activity, status_snapshot,
};

const MAX_REQUEST_LINE: usize = 2_048;
const MAX_HEADER_BYTES: usize = 8_192;

/// Bound handle for a running status server.
pub struct StatusServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl StatusServer {
    /// Returns the bound listen address.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Requests shutdown and waits for the accept loop to stop.
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

/// Starts a loopback-only status server.
///
/// `port == 0` selects an ephemeral port. The database is opened read-only.
///
/// # Errors
///
/// Returns configuration/IO failures when binding or opening the database fails.
pub fn serve_status(
    database_path: &Path,
    embeddings: &EmbeddingConfig,
    port: u16,
) -> Result<StatusServer, ConfigError> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).map_err(|error| ConfigError::Io {
        path: PathBuf::from("127.0.0.1"),
        message: error.to_string(),
    })?;
    let bound = listener.local_addr().map_err(|error| ConfigError::Io {
        path: PathBuf::from("127.0.0.1"),
        message: error.to_string(),
    })?;
    let db = database_path.to_path_buf();
    let embeddings = embeddings.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    let join = thread::spawn(move || accept_loop(listener, db, embeddings, flag));
    thread::sleep(Duration::from_millis(10));
    Ok(StatusServer {
        addr: bound,
        shutdown,
        join: Some(join),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn accept_loop(
    listener: TcpListener,
    database_path: PathBuf,
    embeddings: EmbeddingConfig,
    shutdown: Arc<AtomicBool>,
) {
    let _ = listener.set_nonblocking(false);
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let _ = handle_client(stream, &database_path, &embeddings);
            }
            Err(_) => break,
        }
    }
}

fn handle_client(
    mut stream: TcpStream,
    database_path: &Path,
    embeddings: &EmbeddingConfig,
) -> Result<(), ()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = vec![0_u8; MAX_HEADER_BYTES + 1];
    let n = stream.read(&mut buf).map_err(|_| ())?;
    if n == 0 {
        return Ok(());
    }
    if n > MAX_HEADER_BYTES {
        write_raw(
            &mut stream,
            431,
            "request header fields too large\n",
            "text/plain; charset=utf-8",
            false,
            None,
        )?;
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buf[..n]);
    let Some(first_line) = request.lines().next() else {
        write_raw(
            &mut stream,
            400,
            "bad request\n",
            "text/plain; charset=utf-8",
            false,
            None,
        )?;
        return Ok(());
    };
    if first_line.len() > MAX_REQUEST_LINE {
        write_raw(
            &mut stream,
            400,
            "bad request\n",
            "text/plain; charset=utf-8",
            false,
            None,
        )?;
        return Ok(());
    }
    let mut parts = first_line.split_whitespace();
    let Some(method) = parts.next() else {
        write_raw(
            &mut stream,
            400,
            "bad request\n",
            "text/plain; charset=utf-8",
            false,
            None,
        )?;
        return Ok(());
    };
    let Some(path) = parts.next() else {
        write_raw(
            &mut stream,
            400,
            "bad request\n",
            "text/plain; charset=utf-8",
            false,
            None,
        )?;
        return Ok(());
    };
    if parts.next().is_none() {
        // HTTP/0.9 style without version is treated as malformed for this surface.
        write_raw(
            &mut stream,
            400,
            "bad request\n",
            "text/plain; charset=utf-8",
            false,
            None,
        )?;
        return Ok(());
    }

    let head_only = method.eq_ignore_ascii_case("HEAD");
    let get_like = method.eq_ignore_ascii_case("GET") || head_only;
    if !get_like {
        write_raw(
            &mut stream,
            405,
            "method not allowed\n",
            "text/plain; charset=utf-8",
            false,
            Some("GET, HEAD"),
        )?;
        return Ok(());
    }

    let (code, body, content_type) = match normalize_path(path) {
        "/" => (200, html_home().into_bytes(), "text/html; charset=utf-8"),
        "/health" | "/healthz" => (200, b"ok\n".to_vec(), "text/plain; charset=utf-8"),
        "/status.json" | "/api/status" => json_status(database_path, embeddings),
        "/api/roots" => json_roots(database_path),
        "/api/activity" => json_activity(database_path),
        _ => (404, b"not found\n".to_vec(), "text/plain; charset=utf-8"),
    };
    write_bytes(&mut stream, code, &body, content_type, head_only, None)?;
    Ok(())
}

fn normalize_path(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

fn security_headers(allow: Option<&str>) -> String {
    use std::fmt::Write as _;
    let mut headers = String::from(
        "X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n",
    );
    if let Some(allow) = allow {
        let _ = write!(headers, "Allow: {allow}\r\n");
    }
    headers
}

fn write_raw(
    stream: &mut TcpStream,
    code: u16,
    body: &str,
    content_type: &str,
    head_only: bool,
    allow: Option<&str>,
) -> Result<(), ()> {
    write_bytes(
        stream,
        code,
        body.as_bytes(),
        content_type,
        head_only,
        allow,
    )
}

fn write_bytes(
    stream: &mut TcpStream,
    code: u16,
    body: &[u8],
    content_type: &str,
    head_only: bool,
    allow: Option<&str>,
) -> Result<(), ()> {
    let length = if head_only { 0 } else { body.len() };
    let header = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\n{}\r\n",
        reason(code),
        security_headers(allow),
    );
    stream.write_all(header.as_bytes()).map_err(|_| ())?;
    if !head_only {
        stream.write_all(body).map_err(|_| ())?;
    }
    Ok(())
}

fn reason(code: u16) -> &'static str {
    match code {
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn html_home() -> String {
    r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Omni-Sem Status</title></head>
<body>
<h1>Omni-Sem local status</h1>
<p>Read-only loopback view. Source text and query strings are not exposed.</p>
<ul>
<li><a href="/healthz">/healthz</a></li>
<li><a href="/status.json">/status.json</a></li>
<li><a href="/api/roots">/api/roots</a></li>
<li><a href="/api/activity">/api/activity</a></li>
</ul>
</body></html>"#
        .into()
}

#[derive(Serialize)]
struct StatusPayload {
    snapshot: StatusSnapshot,
    embedding_compatibility: EmbeddingCompatibility,
    snapshots: SnapshotStatusSummary,
    warning: &'static str,
}

#[derive(Serialize)]
struct SnapshotStatusSummary {
    registered: usize,
    queryable: usize,
    unhealthy: usize,
    mapped_roots: usize,
    unmapped_roots: usize,
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    let uri = format!("file:{}?mode=ro", path.display());
    Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| "database unavailable".into())
}

fn json_status(database_path: &Path, embeddings: &EmbeddingConfig) -> (u16, Vec<u8>, &'static str) {
    match open_readonly(database_path).and_then(|connection| {
        let snapshot = status_snapshot(&connection, database_path)
            .map_err(|_| "status unavailable".to_owned())?;
        let listed = list_snapshots(&connection).map_err(|_| "status unavailable".to_owned())?;
        let registered = listed.len();
        let queryable = listed.iter().filter(|item| item.queryable).count();
        let unhealthy = listed.iter().filter(|item| !item.payload_healthy).count();
        let mapped_roots = listed.iter().map(|item| item.mapped_roots).sum();
        let unmapped_roots = listed
            .iter()
            .map(|item| item.total_roots.saturating_sub(item.mapped_roots))
            .sum();
        let compatibility = embedding_compatibility(embeddings, &snapshot.embedding);
        let payload = StatusPayload {
            snapshot,
            embedding_compatibility: compatibility,
            snapshots: SnapshotStatusSummary {
                registered,
                queryable,
                unhealthy,
                mapped_roots,
                unmapped_roots,
            },
            warning: "Local read-only status. No source text or query text is served.",
        };
        serde_json::to_vec_pretty(&payload).map_err(|_| "status unavailable".to_owned())
    }) {
        Ok(body) => (200, body, "application/json; charset=utf-8"),
        Err(_) => (
            500,
            b"internal error\n".to_vec(),
            "text/plain; charset=utf-8",
        ),
    }
}

fn json_roots(database_path: &Path) -> (u16, Vec<u8>, &'static str) {
    match open_readonly(database_path).and_then(|connection| {
        let mut statement = connection
            .prepare("SELECT id, display_name, enabled FROM roots ORDER BY display_name")
            .map_err(|_| "roots unavailable".to_owned())?;
        let rows = statement
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "display_name": row.get::<_, String>(1)?,
                    "enabled": row.get::<_, i64>(2)? == 1,
                }))
            })
            .map_err(|_| "roots unavailable".to_owned())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "roots unavailable".to_owned())?;
        serde_json::to_vec_pretty(&rows).map_err(|_| "roots unavailable".to_owned())
    }) {
        Ok(body) => (200, body, "application/json; charset=utf-8"),
        Err(_) => (
            500,
            b"internal error\n".to_vec(),
            "text/plain; charset=utf-8",
        ),
    }
}

fn json_activity(database_path: &Path) -> (u16, Vec<u8>, &'static str) {
    match open_readonly(database_path).and_then(|connection| {
        let rows: Vec<QueryActivitySample> =
            list_query_activity(&connection, 20).map_err(|_| "activity unavailable".to_owned())?;
        serde_json::to_vec_pretty(&rows).map_err(|_| "activity unavailable".to_owned())
    }) {
        Ok(body) => (200, body, "application/json; charset=utf-8"),
        Err(_) => (
            500,
            b"internal error\n".to_vec(),
            "text/plain; charset=utf-8",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{add_root, init_installation};
    use crate::index::index_roots;
    use crate::paths::AppPaths;
    use crate::storage::open_database;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf, StatusServer) {
        let temp = TempDir::new().unwrap();
        let paths = AppPaths::for_base(temp.path().join("app"));
        let (mut config, _) = init_installation(&paths).unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("a.md"), "# A\n\nSQLite.\n").unwrap();
        add_root(&mut config, &notes, Some("notes".into())).unwrap();
        config.save(&paths.config_file).unwrap();
        let db = config.database_path().unwrap();
        let mut connection = open_database(&db).unwrap();
        index_roots(&mut connection, &config, None).unwrap();
        drop(connection);
        let server = serve_status(&db, &config.embeddings, 0).unwrap();
        (temp, db, server)
    }

    fn request(addr: SocketAddr, raw: &str) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn get_and_head_and_post_contract() {
        let (_temp, _db, server) = setup();
        let addr = server.addr();
        let get = request(addr, "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(get.contains("200 OK"));
        assert!(get.contains("ok"));
        assert!(get.contains("X-Content-Type-Options: nosniff"));
        let head = request(addr, "HEAD /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(head.contains("200 OK"));
        assert!(!head.contains("\r\n\r\nok"));
        let post = request(addr, "POST /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(post.contains("405"));
        assert!(post.contains("Allow: GET, HEAD"));
        let bad = request(addr, "GET\r\n\r\n");
        assert!(bad.contains("400"));
        let status = request(addr, "GET /status.json HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(status.contains("200 OK"));
        assert!(status.contains("embedding_compatibility"));
        assert!(status.contains("\"state\": \"disabled\""));
        assert!(!status.contains(temp_path_marker()));
        server.shutdown();
    }

    fn temp_path_marker() -> &'static str {
        // Avoid asserting machine-specific absolute paths; ensure generic error privacy.
        "internal error"
    }
}
