//! Streamable HTTP transport for the MCP server.
//!
//! One HTTP session bridges to one in-process MCP loop
//! (`serve_controlled_streams`) over bounded channel pipes, so both
//! transports share the exact same tool registry and runtime limits. POST
//! carries JSON-RPC and returns `application/json`; the server initiates no
//! streams, so GET answers 405. Sessions are issued on `initialize` via
//! `Mcp-Session-Id`, bounded in number, idle-expired, and client-terminable
//! with DELETE. Origins other than loopback are rejected to block DNS
//! rebinding; the listener itself should stay on loopback unless a reverse
//! proxy adds authentication.

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use mcport::{ConcurrentMcpServer, serve_controlled_streams};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::runtime_config;

const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_SESSIONS: usize = 64;
const STDIN_QUEUE: usize = 64;
const SESSION_IDLE: Duration = Duration::from_secs(900);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(130);
const SUPPORTED_PROTOCOL_HEADERS: &[&str] =
    &["2025-03-26", "2025-06-18", "2025-11-25", "2026-07-28"];

/// Serve the Streamable HTTP transport on `address` until the process exits.
pub fn serve_http(state: crate::CortexMcpState, address: SocketAddr) -> io::Result<()> {
    serve_http_with(state, address, crate::ServerProfile::Full)
}

/// Serve one profile over Streamable HTTP.
pub fn serve_http_with(
    state: crate::CortexMcpState,
    address: SocketAddr,
    profile: crate::ServerProfile,
) -> io::Result<()> {
    let server = Arc::new(crate::build_server_with(state, profile));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(address).await?;
        println!("Cortex Loom MCP (streamable http): http://{address}/mcp");
        axum::serve(listener, app(server)).await
    })
}

fn app(server: Arc<ConcurrentMcpServer>) -> Router {
    let shared = Arc::new(HttpState {
        server,
        sessions: Mutex::new(HashMap::new()),
        counter: AtomicU64::new(0),
    });
    Router::new()
        .route("/mcp", any(endpoint))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(shared)
}

struct HttpState {
    server: Arc<ConcurrentMcpServer>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    counter: AtomicU64,
}

struct Session {
    stdin: SyncSender<Vec<u8>>,
    pending: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Value>>>,
    last_used: Mutex<Instant>,
}

async fn endpoint(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    method: axum::http::Method,
    body: Bytes,
) -> Response {
    if let Err(response) = check_origin(&headers) {
        return response;
    }
    match method {
        axum::http::Method::POST => post_message(&state, &headers, &body).await,
        axum::http::Method::DELETE => delete_session(&state, &headers),
        axum::http::Method::GET => {
            // No server-initiated streams exist on this server.
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn post_message(state: &Arc<HttpState>, headers: &HeaderMap, body: &Bytes) -> Response {
    let Ok(message) = serde_json::from_slice::<Value>(body) else {
        return problem(StatusCode::BAD_REQUEST, "body must be one JSON-RPC message");
    };
    if let Err(response) = check_protocol_header(headers) {
        return response;
    }
    let is_initialize = message.get("method").and_then(Value::as_str) == Some("initialize");
    let request_id = message.get("id").cloned().filter(|id| !id.is_null());

    let session_header = header_value(headers, "mcp-session-id");
    let (session_id, session) = match (session_header, is_initialize) {
        (Some(id), _) => {
            let Some(session) = state
                .sessions
                .lock()
                .ok()
                .and_then(|sessions| sessions.get(&id).map(Arc::clone))
            else {
                return problem(StatusCode::NOT_FOUND, "unknown or expired session");
            };
            (id, session)
        }
        (None, true) => match create_session(state) {
            Ok(created) => created,
            Err(response) => return response,
        },
        (None, false) => {
            return problem(StatusCode::BAD_REQUEST, "Mcp-Session-Id header is required");
        }
    };
    if let Ok(mut last_used) = session.last_used.lock() {
        *last_used = Instant::now();
    }
    sweep_idle(state);

    let waiter = request_id.as_ref().map(|id| {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        if let Ok(mut pending) = session.pending.lock() {
            pending.insert(pending_key(id), sender);
        }
        receiver
    });

    let mut line = message.to_string().into_bytes();
    line.push(b'\n');
    match session.stdin.try_send(line) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            return problem(StatusCode::TOO_MANY_REQUESTS, "session queue is full");
        }
        Err(TrySendError::Disconnected(_)) => {
            drop_session(state, &session_id);
            return problem(StatusCode::NOT_FOUND, "session has terminated");
        }
    }

    let Some(waiter) = waiter else {
        // Notifications and client responses produce no reply body.
        return StatusCode::ACCEPTED.into_response();
    };
    match tokio::time::timeout(REQUEST_TIMEOUT, waiter).await {
        Ok(Ok(reply)) => {
            let mut response = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                reply.to_string(),
            )
                .into_response();
            if let Ok(value) = header::HeaderValue::from_str(&session_id) {
                response.headers_mut().insert("mcp-session-id", value);
            }
            response
        }
        Ok(Err(_)) => {
            drop_session(state, &session_id);
            problem(StatusCode::NOT_FOUND, "session has terminated")
        }
        Err(_) => problem(StatusCode::GATEWAY_TIMEOUT, "request timed out"),
    }
}

fn delete_session(state: &Arc<HttpState>, headers: &HeaderMap) -> Response {
    let Some(id) = header_value(headers, "mcp-session-id") else {
        return problem(StatusCode::BAD_REQUEST, "Mcp-Session-Id header is required");
    };
    if state
        .sessions
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(&id))
        .is_some()
    {
        StatusCode::NO_CONTENT.into_response()
    } else {
        problem(StatusCode::NOT_FOUND, "unknown or expired session")
    }
}

#[allow(clippy::result_large_err)]
fn create_session(state: &Arc<HttpState>) -> Result<(String, Arc<Session>), Response> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "session lock poisoned"))?;
    if sessions.len() >= MAX_SESSIONS {
        return Err(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "session limit reached",
        ));
    }
    let id = session_id(state.counter.fetch_add(1, Ordering::Relaxed));
    let (stdin_tx, stdin_rx) = sync_channel::<Vec<u8>>(STDIN_QUEUE);
    let (stdout_tx, stdout_rx) = sync_channel::<Vec<u8>>(STDIN_QUEUE);

    let server = Arc::clone(&state.server);
    thread::Builder::new()
        .name("cortex-mcp-http-session".to_owned())
        .spawn(move || {
            let reader = BufReader::new(ChannelReader {
                receiver: stdin_rx,
                buffer: Vec::new(),
                position: 0,
            });
            let writer = ChannelWriter { sender: stdout_tx };
            let _ = serve_controlled_streams(server, reader, writer, runtime_config());
        })
        .map_err(|error| problem(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()))?;

    let session = Arc::new(Session {
        stdin: stdin_tx,
        pending: Mutex::new(HashMap::new()),
        last_used: Mutex::new(Instant::now()),
    });
    sessions.insert(id.clone(), Arc::clone(&session));
    drop(sessions);
    spawn_router(Arc::clone(&session), stdout_rx);
    Ok((id, session))
}

/// Route each outgoing JSON-RPC line to the awaiting HTTP request by id.
fn spawn_router(session: Arc<Session>, stdout_rx: Receiver<Vec<u8>>) {
    tokio::task::spawn_blocking(move || {
        let mut buffer: Vec<u8> = Vec::new();
        while let Ok(chunk) = stdout_rx.recv() {
            buffer.extend_from_slice(&chunk);
            while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = buffer.drain(..=newline).collect();
                let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                let Some(id) = value.get("id").filter(|id| !id.is_null()) else {
                    // Server notifications have no waiting HTTP request.
                    continue;
                };
                let key = pending_key(id);
                if let Some(waiter) = session
                    .pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&key))
                {
                    let _ = waiter.send(value);
                }
            }
        }
        // The loop ended: fail every waiter so requests do not hang.
        if let Ok(mut pending) = session.pending.lock() {
            pending.clear();
        }
    });
}

fn drop_session(state: &Arc<HttpState>, id: &str) {
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.remove(id);
    }
}

fn sweep_idle(state: &Arc<HttpState>) {
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.retain(|_, session| {
            session
                .last_used
                .lock()
                .is_ok_and(|last_used| last_used.elapsed() < SESSION_IDLE)
        });
    }
}

#[allow(clippy::result_large_err)] // cold error path carrying the HTTP reply
fn check_origin(headers: &HeaderMap) -> Result<(), Response> {
    let Some(origin) = header_value(headers, "origin") else {
        return Ok(());
    };
    let allowed = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .map(|rest| rest.split([':', '/']).next().unwrap_or_default())
        .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "[::1]"));
    if allowed {
        Ok(())
    } else {
        Err(problem(StatusCode::FORBIDDEN, "origin is not allowed"))
    }
}

#[allow(clippy::result_large_err)]
fn check_protocol_header(headers: &HeaderMap) -> Result<(), Response> {
    match header_value(headers, "mcp-protocol-version") {
        None => Ok(()),
        Some(version) if SUPPORTED_PROTOCOL_HEADERS.contains(&version.as_str()) => Ok(()),
        Some(version) => Err(problem(
            StatusCode::BAD_REQUEST,
            &format!("unsupported protocol version: {version}"),
        )),
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn pending_key(id: &Value) -> String {
    id.to_string()
}

fn problem(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::json!({ "error": message }).to_string(),
    )
        .into_response()
}

/// Non-guessable session id from process-local entropy sources.
fn session_id(counter: u64) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(counter.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let digest = hasher.finalize();
    let mut rendered = String::with_capacity(32);
    for byte in digest.iter().take(16) {
        let _ = std::fmt::Write::write_fmt(&mut rendered, format_args!("{byte:02x}"));
    }
    rendered
}

struct ChannelReader {
    receiver: Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    position: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.buffer.len() {
            match self.receiver.recv() {
                Ok(chunk) => {
                    self.buffer = chunk;
                    self.position = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let available = &self.buffer[self.position..];
        let count = available.len().min(output.len());
        output[..count].copy_from_slice(&available[..count]);
        self.position += count;
        Ok(count)
    }
}

struct ChannelWriter {
    sender: SyncSender<Vec<u8>>,
}

impl Write for ChannelWriter {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.sender
            .send(input.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "http session closed"))?;
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
