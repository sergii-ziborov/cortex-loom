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
    let server = Arc::new(crate::build_server(state));
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
mod tests {
    use super::*;
    use serde_json::json;

    fn spawn_test_server() -> String {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
        let address = reserved.local_addr().expect("port");
        drop(reserved);
        let database = std::env::temp_dir().join(format!(
            "cortex-mcp-http-test-{}-{}.db",
            std::process::id(),
            address.port()
        ));
        let state = crate::CortexMcpState::open(database).expect("open state");
        thread::spawn(move || {
            let _ = serve_http(state, address);
        });
        let base = format!("http://{address}/mcp");
        let agent = client();
        for _ in 0..50 {
            if agent.get(&base).call().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        base
    }

    fn client() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into()
    }

    fn body_of(mut response: ureq::http::Response<ureq::Body>) -> Value {
        let text = response
            .body_mut()
            .read_to_string()
            .expect("response body reads");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    // One sequential lifecycle walk over a single live server.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn streamable_http_speaks_the_full_session_lifecycle() {
        let base = spawn_test_server();
        let agent = client();

        // GET carries no server stream on this server.
        let response = agent.get(&base).call().expect("get");
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

        // A non-loopback origin is rejected before anything else.
        let response = agent
            .post(&base)
            .header("origin", "https://evil.example")
            .send("{}")
            .expect("origin post");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // A request without a session (and not initialize) is rejected.
        let ping = json!({"jsonrpc": "2.0", "id": 9, "method": "ping", "params": {}});
        let response = agent.post(&base).send(&ping.to_string()).expect("post");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Initialize creates the session and returns its id.
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "http-test", "version": "1"}
            }
        });
        let response = agent
            .post(&base)
            .header("origin", "http://localhost")
            .send(&initialize.to_string())
            .expect("initialize");
        assert_eq!(response.status(), StatusCode::OK);
        let session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("session id header")
            .to_owned();
        let reply = body_of(response);
        assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");

        // The initialized notification is accepted without a body.
        let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let response = agent
            .post(&base)
            .header("mcp-session-id", &session)
            .send(&initialized.to_string())
            .expect("initialized");
        assert_eq!(response.status(), StatusCode::ACCEPTED);

        // An unsupported protocol header is rejected.
        let listing = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
        let response = agent
            .post(&base)
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "1999-01-01")
            .send(&listing.to_string())
            .expect("bad version");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Tools listing works over the session.
        let response = agent
            .post(&base)
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .send(&listing.to_string())
            .expect("tools/list");
        assert_eq!(response.status(), StatusCode::OK);
        let reply = body_of(response);
        let tools = reply["result"]["tools"].as_array().expect("tools array");
        assert!(
            tools
                .iter()
                .any(|tool| tool["name"] == "weavatrix_context_compile")
        );

        // A tool call executes end to end.
        let call = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "route_work",
                "arguments": {
                    "task": "Deploy the server to the staging cluster",
                    "evidence": "not_required",
                    "schemaValid": true,
                    "budget": {
                        "estimatedInputTokens": 8,
                        "estimatedOutputTokens": 8,
                        "maxInputTokens": 1024,
                        "maxOutputTokens": 1024
                    },
                    "mutation": "none",
                    "availability": {"weavatrix": true, "ollama": true}
                }
            }
        });
        let response = agent
            .post(&base)
            .header("mcp-session-id", &session)
            .send(&call.to_string())
            .expect("tools/call");
        assert_eq!(response.status(), StatusCode::OK);
        let reply = body_of(response);
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert!(text.contains("upstream"), "routing decision came back");

        // An unknown session is a 404; DELETE terminates the real one.
        let response = agent
            .post(&base)
            .header("mcp-session-id", "does-not-exist")
            .send(&listing.to_string())
            .expect("unknown session");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = agent
            .delete(&base)
            .header("mcp-session-id", &session)
            .call()
            .expect("delete");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let response = agent
            .post(&base)
            .header("mcp-session-id", &session)
            .send(&listing.to_string())
            .expect("post after delete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
