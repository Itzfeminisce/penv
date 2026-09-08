//! An HTTP server in this process: a `TcpListener` on a loopback port serving
//! canned answers. No test touches the network.

// Two crates share this file, and each uses a part of it.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Debug, Clone)]
pub struct Canned {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Default)]
struct State {
    routes: BTreeMap<String, VecDeque<Canned>>,
    log: Vec<Recorded>,
}

pub struct Mock {
    port: u16,
    state: Arc<Mutex<State>>,
}

impl Mock {
    pub fn new() -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        let served = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = serve(stream, &served);
            }
        });
        Mock { port, state }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// A port nothing listens on, for the offline paths.
    pub fn closed_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("http://127.0.0.1:{port}")
    }

    /// Queue an answer. The last one queued for a route repeats.
    pub fn on(&self, method: &str, path: &str, status: u16, body: &str) -> &Mock {
        self.on_with(method, path, status, body, &[])
    }

    pub fn on_with(
        &self,
        method: &str,
        path: &str,
        status: u16,
        body: &str,
        headers: &[(&str, &str)],
    ) -> &Mock {
        self.state
            .lock()
            .unwrap()
            .routes
            .entry(key(method, path))
            .or_default()
            .push_back(Canned {
                status,
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body: body.to_string(),
            });
        self
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.state.lock().unwrap().log.clone()
    }

    pub fn hits(&self, method: &str, path: &str) -> Vec<Recorded> {
        self.requests()
            .into_iter()
            .filter(|r| r.method == method && r.path == path)
            .collect()
    }

    pub fn last(&self, method: &str, path: &str) -> Recorded {
        self.hits(method, path)
            .pop()
            .unwrap_or_else(|| panic!("nothing asked for {method} {path}"))
    }
}

impl Default for Mock {
    fn default() -> Mock {
        Mock::new()
    }
}

fn key(method: &str, path: &str) -> String {
    format!("{method} {path}")
}

fn serve(stream: TcpStream, state: &Arc<Mutex<State>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let path = target.split('?').next().unwrap_or_default().to_string();

    let mut headers = BTreeMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }

    let asked_for = headers.get("if-none-match").cloned();
    let canned = {
        let mut state = state.lock().unwrap();
        state.log.push(Recorded {
            method: method.clone(),
            target,
            path: path.clone(),
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        });
        match state.routes.get_mut(&key(&method, &path)) {
            Some(queue) if queue.len() > 1 => queue.pop_front(),
            Some(queue) => queue.front().cloned(),
            None => None,
        }
    };

    let mut canned = canned.unwrap_or(Canned {
        status: 404,
        headers: Vec::new(),
        body: "{\"error\":\"not_found\"}".into(),
    });

    // The real server honours If-None-Match: a read whose ETag still matches is
    // a 304, whatever body was queued behind it.
    let etag = canned
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("etag"))
        .map(|(_, value)| value.as_str());
    if canned.status == 200 && etag.is_some() && asked_for.as_deref() == etag {
        canned.status = 304;
    }

    // A HEAD or a 304 carries headers and nothing else.
    let empty = method == "HEAD" || canned.status == 304;
    let payload = if empty { "" } else { canned.body.as_str() };
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        canned.status,
        reason(canned.status),
        payload.len()
    );
    for (name, value) in &canned.headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(payload);

    let mut stream = stream;
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        410 => "Gone",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}
