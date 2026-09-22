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
