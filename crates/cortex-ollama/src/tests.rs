use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use cortex_router::{ExecutionTarget, RoutingRequest, route};

use super::*;

fn config(base_url: String) -> OllamaConfig {
    OllamaConfig {
        base_url,
        ..OllamaConfig::default()
    }
    .with_profile("draft", ModelProfile::new("exact-model:9b", 512, 128, 768))
}

fn local_route() -> cortex_router::RoutingDecision {
    let mut request = RoutingRequest::new("Summarize the supplied evidence");
    request.evidence = cortex_router::EvidenceStatus::Verified;
    route(&request)
}

fn mock_server(responses: Vec<&'static str>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for body in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let headers_end = request.windows(4).position(|part| part == b"\r\n\r\n");
                if let Some(end) = headers_end {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + content_length {
                        break;
                    }
                }
                assert!(read > 0);
            }
            requests.push(String::from_utf8(request).unwrap());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    (format!("http://{address}"), handle)
}

#[test]
fn discovers_tags_version_and_cpu_gpu_placement() {
    let (base_url, server) = mock_server(vec![
        r#"{"models":[{"name":"exact-model:9b","model":"exact-model:9b","size":9,"digest":"abc"}]}"#,
        r#"{"version":"0.13.0"}"#,
        r#"{"models":[{"name":"cpu","model":"cpu","size":4,"size_vram":0,"digest":"a"},{"name":"gpu","model":"gpu","size":9,"size_vram":7,"digest":"b"}]}"#,
    ]);
    let client = OllamaClient::new(config(base_url)).unwrap();
    assert_eq!(client.tags().unwrap()[0].model, "exact-model:9b");
    assert_eq!(client.version().unwrap().version, "0.13.0");
    let running = client.running_models().unwrap();
    assert_eq!(running[0].placement, DevicePlacement::Cpu);
    assert_eq!(running[1].placement, DevicePlacement::Gpu);
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /api/tags "));
    assert!(requests[1].starts_with("GET /api/version "));
    assert!(requests[2].starts_with("GET /api/ps "));
}

#[test]
fn chat_is_structured_bounded_and_uses_the_exact_model() {
    let content = r#"{\"summary\":\"Grounded\",\"evidenceIds\":[\"E1\"],\"unresolved\":[]}"#;
    let response = format!(r#"{{"message":{{"content":"{content}"}}}}"#);
    let response: &'static str = Box::leak(response.into_boxed_str());
    let (base_url, server) = mock_server(vec![response]);
    let client = OllamaClient::new(config(base_url)).unwrap();
    let request = DraftRequest::new(
        "draft",
        vec![ChatMessage::user("Use evidence E1")],
        vec!["E1".to_owned()],
        64,
        100,
    );
    let assessment = client.draft(&request, &local_route()).unwrap();
    assert!(assessment.is_accepted());

    let captured = server.join().unwrap().pop().unwrap();
    assert!(captured.starts_with("POST /api/chat "));
    let body = captured.split("\r\n\r\n").nth(1).unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["model"], "exact-model:9b");
    assert_eq!(json["stream"], false);
    assert_eq!(json["think"], false);
    assert_eq!(json["format"]["additionalProperties"], false);
    assert_eq!(json["options"]["temperature"], 0);
    assert_eq!(json["options"]["num_predict"], 100);
}

#[test]
fn structured_chat_forwards_the_caller_schema_and_stays_bounded() {
    let response = r#"{"message":{"content":"{\"tier\":\"upstream_strong\"}"}}"#;
    let (base_url, server) = mock_server(vec![response]);
    let client = OllamaClient::new(config(base_url)).unwrap();
    let schema = serde_json::json!({
        "type": "object",
        "properties": {"tier": {"type": "string"}},
        "required": ["tier"],
        "additionalProperties": false
    });
    let request = StructuredChatRequest {
        profile: "draft".to_owned(),
        messages: vec![ChatMessage::user("Classify the task")],
        schema: schema.clone(),
        estimated_input_tokens: 64,
        requested_output_tokens: 100,
    };
    let content = client.structured_chat(&request).unwrap();
    assert_eq!(content, r#"{"tier":"upstream_strong"}"#);

    let captured = server.join().unwrap().pop().unwrap();
    let body = captured.split("\r\n\r\n").nth(1).unwrap();
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["model"], "exact-model:9b");
    assert_eq!(json["format"], schema);
    assert_eq!(json["options"]["temperature"], 0);

    let over_budget = StructuredChatRequest {
        estimated_input_tokens: 513,
        ..request
    };
    assert!(matches!(
        client.structured_chat(&over_budget),
        Err(OllamaError::InputBudgetExceeded { .. })
    ));
}

#[test]
fn quality_gate_falls_back_for_every_unverified_case() {
    let routing = local_route();
    let evidence = vec!["E1".to_owned()];
    let cases = [
        r#"{"summary":"ok","evidenceIds":["invented"],"unresolved":[]}"#,
        r#"{"summary":"ok","evidenceIds":["E1"],"unresolved":["check this"]}"#,
        r#"{"summary":"ok","evidenceIds":["E1"]}"#,
        r#"{"summary":"","evidenceIds":["E1"],"unresolved":[]}"#,
    ];
    for content in cases {
        let assessment = assess_local_draft(content, &evidence, &routing);
        assert_eq!(assessment.target, ExecutionTarget::Upstream, "{content}");
        assert!(!assessment.failures.is_empty());
    }

    let rejected = route(&RoutingRequest::new("Deploy the evidence summary"));
    let valid = r#"{"summary":"ok","evidenceIds":["E1"],"unresolved":[]}"#;
    assert_eq!(
        assess_local_draft(valid, &evidence, &rejected).target,
        ExecutionTarget::Upstream
    );
}

#[test]
fn rejects_remote_hosts_and_profile_budget_overruns() {
    assert!(matches!(
        OllamaClient::new(config("http://example.com:11434".to_owned())),
        Err(OllamaError::InvalidConfiguration(_))
    ));

    let (base_url, server) = mock_server(Vec::new());
    let client = OllamaClient::new(config(base_url)).unwrap();
    let request = DraftRequest::new(
        "draft",
        vec![ChatMessage::user("large")],
        Vec::new(),
        513,
        1,
    );
    assert!(matches!(
        client.draft(&request, &local_route()),
        Err(OllamaError::InputBudgetExceeded { .. })
    ));
    server.join().unwrap();
}

#[test]
fn router_rejection_does_not_contact_ollama() {
    let (base_url, server) = mock_server(Vec::new());
    let client = OllamaClient::new(config(base_url)).unwrap();
    let request = DraftRequest::new(
        "draft",
        vec![ChatMessage::user("publish")],
        Vec::new(),
        1,
        1,
    );
    let routing = route(&RoutingRequest::new("Publish this package"));
    let assessment = client.draft(&request, &routing).unwrap();
    assert_eq!(assessment.target, ExecutionTarget::Upstream);
    server.join().unwrap();
}
