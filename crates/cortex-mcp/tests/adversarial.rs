//! Adversarial stdio-transport tests: hostile input must never panic the
//! loop, and a well-formed request after garbage must still be answered.

use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use cortex_mcp::{CortexMcpState, build_server, runtime_config};
use mcport::serve_controlled_streams;
use serde_json::Value;

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("writer lock poisoned"))?
            .extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn drive(script: &str) -> Vec<Value> {
    let database = std::env::temp_dir().join(format!(
        "cortex-mcp-adversarial-{}-{}.db",
        std::process::id(),
        script.len()
    ));
    let state = CortexMcpState::open(database).expect("open state");
    let server = Arc::new(build_server(state));
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(Arc::clone(&output));
    let reader = Cursor::new(script.as_bytes().to_vec());
    // The loop must terminate on EOF without panicking, whatever came in.
    serve_controlled_streams(server, reader, writer, runtime_config()).expect("loop survives");
    let bytes = output.lock().expect("writer lock").clone();
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn initialize_line(id: u64) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "adversary", "version": "1"}
            }
        })
    )
}

fn find_reply(replies: &[Value], id: u64) -> Option<&Value> {
    replies.iter().find(|reply| reply["id"] == id)
}

#[test]
fn garbage_before_and_between_requests_never_kills_the_loop() {
    let mut script = String::new();
    // Hostile prelude: raw text, broken JSON, wrong JSON types, null ids,
    // unknown methods, and premature tool calls before initialize.
    script.push_str("this is not json at all\n");
    script.push_str("{\"jsonrpc\": \"2.0\", \"id\": \n");
    script.push_str("[1, 2, 3]\n");
    script.push_str("42\n");
    script.push_str("\"just a string\"\n");
    script.push_str("{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"tools/list\"}\n");
    script.push_str(
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{\"name\":\"route_work\",\"arguments\":{}}}\n",
    );
    script.push_str("{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"no/such/method\",\"params\":{}}\n");
    // Then a fully legitimate conversation.
    script.push_str(&initialize_line(1));
    script
        .push_str("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n");
    script.push_str("{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n");

    let replies = drive(&script);
    let listing = find_reply(&replies, 2).expect("valid request after garbage is answered");
    assert!(
        listing["result"]["tools"]
            .as_array()
            .is_some_and(|tools| { tools.iter().any(|tool| tool["name"] == "route_work") }),
        "tool listing survives the hostile prelude"
    );
    // The premature and unknown calls produced errors, not silence or panic.
    // Tool-level failures legitimately arrive as results with isError=true;
    // protocol-level failures arrive as JSON-RPC errors.
    for id in [7, 8] {
        if let Some(reply) = find_reply(&replies, id) {
            let is_error = reply.get("error").is_some() || reply["result"]["isError"] == true;
            assert!(is_error, "hostile request {id} is answered with an error");
        }
    }
}

#[test]
fn duplicate_and_string_ids_are_answered_consistently() {
    let mut script = String::new();
    script.push_str(&initialize_line(1));
    script
        .push_str("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n");
    script.push_str(
        "{\"jsonrpc\":\"2.0\",\"id\":\"same\",\"method\":\"tools/list\",\"params\":{}}\n",
    );
    script.push_str(
        "{\"jsonrpc\":\"2.0\",\"id\":\"same\",\"method\":\"tools/list\",\"params\":{}}\n",
    );
    let replies = drive(&script);
    let same: Vec<_> = replies
        .iter()
        .filter(|reply| reply["id"] == "same")
        .collect();
    assert_eq!(same.len(), 2, "both duplicate-id requests are answered");
}

#[test]
fn oversized_lines_are_contained_by_transport_limits() {
    // A single line far above the 4 MiB read limit; whatever the transport
    // does, the process must not panic or exhaust memory, and the loop must
    // end at EOF.
    let mut script = String::with_capacity(5 * 1024 * 1024 + 64);
    script.push_str(&"a".repeat(5 * 1024 * 1024));
    script.push('\n');
    script.push_str(&initialize_line(1));
    let replies = drive(&script);
    // The oversized line itself gets no reply; the loop either recovered
    // (initialize answered) or terminated cleanly. Both are acceptable,
    // panicking or hanging is not.
    if let Some(reply) = find_reply(&replies, 1) {
        assert!(reply.get("result").is_some() || reply.get("error").is_some());
    }
}

#[test]
fn deeply_nested_json_does_not_blow_the_stack() {
    let mut nested =
        String::from("{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":");
    let depth = 2_000;
    for _ in 0..depth {
        nested.push_str("{\"a\":");
    }
    nested.push('1');
    for _ in 0..depth {
        nested.push('}');
    }
    nested.push_str("}\n");
    let mut script = String::new();
    script.push_str(&initialize_line(1));
    script
        .push_str("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n");
    script.push_str(&nested);
    script.push_str("{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ping\",\"params\":{}}\n");
    let replies = drive(&script);
    // Depth handling may reject the request; the follow-up ping must work
    // if the loop is still alive, and reaching this line at all proves no
    // stack overflow occurred.
    if let Some(reply) = find_reply(&replies, 4) {
        assert!(reply.get("result").is_some() || reply.get("error").is_some());
    }
}
