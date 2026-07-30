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

use crate::error::ConfigError;
use crate::storage::{QueryActivitySample, StatusSnapshot, list_query_activity, status_snapshot};

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
        // Wake accept with a local connection attempt.
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
pub fn serve_status(database_path: &Path, port: u16) -> Result<StatusServer, ConfigError> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).map_err(|error| ConfigError::Io {
        path: Path::new("127.0.0.1").to_path_buf(),
        message: error.to_string(),
    })?;
    listener
        .set_nonblocking(false)
        .map_err(|error| ConfigError::Io {
            path: Path::new("127.0.0.1").to_path_buf(),
            message: error.to_string(),
        })?;
    let bound = listener.local_addr().map_err(|error| ConfigError::Io {
        path: Path::new("127.0.0.1").to_path_buf(),
        message: error.to_string(),
    })?;
    let db = database_path.to_path_buf();
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    let join = thread::spawn(move || accept_loop(listener, db, flag));
    // Give the thread a moment to start.
    thread::sleep(Duration::from_millis(10));
    Ok(StatusServer {
        addr: bound,
        shutdown,
        join: Some(join),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn accept_loop(listener: TcpListener, database_path: PathBuf, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let path = database_path.clone();
                let _ = handle_client(stream, &path);
            }
            Err(_) => break,
        }
    }
}

fn handle_client(mut stream: TcpStream, database_path: &Path) -> Result<(), ()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0_u8; 4096];
    let n = stream.read(&mut buf).map_err(|_| ())?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let response = match path {
        "/" => html_home(),
        "/health" => text_response(200, "ok\n", "text/plain; charset=utf-8"),
        "/api/status" => json_status(database_path),
        "/api/roots" => json_roots(database_path),
        "/api/activity" => json_activity(database_path),
        _ => text_response(404, "not found\n", "text/plain; charset=utf-8"),
    };
    stream.write_all(response.as_bytes()).map_err(|_| ())?;
    Ok(())
}

fn security_headers() -> String {
    "X-Content-Type-Options: nosniff\r\n\
     X-Frame-Options: DENY\r\n\
     Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
     Cache-Control: no-store\r\n\
     Connection: close\r\n"
        .into()
}

fn text_response(code: u16, body: &str, content_type: &str) -> String {
    format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{}{}",
        reason(code),
        body.len(),
        security_headers(),
        body
    )
}

fn reason(code: u16) -> &'static str {
    match code {
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn html_home() -> String {
    let body = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Omni-Sem Status</title></head>
<body>
<h1>Omni-Sem local status</h1>
<p>Read-only view. Source paths and query text are not exposed.</p>
<ul>
<li><a href="/health">/health</a></li>
<li><a href="/api/status">/api/status</a></li>
<li><a href="/api/roots">/api/roots</a></li>
<li><a href="/api/activity">/api/activity</a></li>
</ul>
</body></html>"#;
    text_response(200, body, "text/html; charset=utf-8")
}

#[derive(Serialize)]
struct StatusPayload {
    snapshot: StatusSnapshot,
    warning: &'static str,
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    let uri = format!("file:{}?mode=ro", path.display());
    Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|error| error.to_string())
}

fn json_status(database_path: &Path) -> String {
    match open_readonly(database_path).and_then(|connection| {
        status_snapshot(&connection, database_path).map_err(|error| error.to_string())
    }) {
        Ok(snapshot) => {
            let payload = StatusPayload {
                snapshot,
                warning: "Local read-only status. No source text or query text is served.",
            };
            match serde_json::to_string_pretty(&payload) {
                Ok(body) => text_response(200, &body, "application/json; charset=utf-8"),
                Err(error) => text_response(500, &error.to_string(), "text/plain; charset=utf-8"),
            }
        }
        Err(error) => text_response(500, &error, "text/plain; charset=utf-8"),
    }
}

fn json_roots(database_path: &Path) -> String {
    match open_readonly(database_path).and_then(|connection| {
        let mut statement = connection
            .prepare("SELECT id, display_name, enabled FROM roots ORDER BY display_name")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "display_name": row.get::<_, String>(1)?,
                    "enabled": row.get::<_, i64>(2)? == 1,
                }))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(rows)
    }) {
        Ok(rows) => match serde_json::to_string_pretty(&rows) {
            Ok(body) => text_response(200, &body, "application/json; charset=utf-8"),
            Err(error) => text_response(500, &error.to_string(), "text/plain; charset=utf-8"),
        },
        Err(error) => text_response(500, &error, "text/plain; charset=utf-8"),
    }
}

fn json_activity(database_path: &Path) -> String {
    match open_readonly(database_path).and_then(|connection| {
        list_query_activity(&connection, 20).map_err(|error| error.to_string())
    }) {
        Ok(rows) => {
            let sanitized: Vec<QueryActivitySample> = rows;
            match serde_json::to_string_pretty(&sanitized) {
                Ok(body) => text_response(200, &body, "application/json; charset=utf-8"),
                Err(error) => text_response(500, &error.to_string(), "text/plain; charset=utf-8"),
            }
        }
        Err(error) => text_response(500, &error, "text/plain; charset=utf-8"),
    }
}
