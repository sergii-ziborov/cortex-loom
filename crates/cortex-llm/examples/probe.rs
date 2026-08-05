//! Probe the deployed local runtimes through the provider contract.
//!
//! Not a test: it needs OVMS running. It exists so the claim "the provider
//! talks to the NPU and the GPU" is something you can re-run rather than
//! something you have to believe.

use cortex_llm::device::Device;
use cortex_llm::profile::{LlmProfile, Role, Runtime};
use cortex_llm::{ClassifyRequest, EmbedRequest, LlmProvider, OpenAiProvider};

fn profile(id: &str, role: Role, model: &str, device: Device, port: u16, secs: u32) -> LlmProfile {
    LlmProfile {
        id: id.to_owned(),
        role,
        model: model.to_owned(),
        device,
        runtime: Runtime::OpenAiCompatible,
        base_url: format!("http://127.0.0.1:{port}"),
        timeout_seconds: secs,
        gate_passed: false,
        note: None,
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

fn main() {
    let embed = OpenAiProvider::new(profile(
        "gpu-embedding",
        Role::Embedding,
        "qwen3-embed",
        Device::Gpu,
        8001,
        60,
    ))
    .expect("loopback endpoint");
    match embed.embed(&EmbedRequest {
        inputs: vec![
            "bounded retry reaches maxAttempts and resolves the run".to_owned(),
            "the retry controller stops after the attempt limit".to_owned(),
            "unrelated text about gardening".to_owned(),
        ],
    }) {
        Ok(reply) => {
            println!(
                "embeddings: {} vectors, dim {}, {} ms, placement {}",
                reply.value.len(),
                reply.value[0].len(),
                reply.latency_ms,
                reply.placement.describe()
            );
            println!(
                "  cosine(related)   = {:.4}",
                cosine(&reply.value[0], &reply.value[1])
            );
            println!(
                "  cosine(unrelated) = {:.4}",
                cosine(&reply.value[0], &reply.value[2])
            );
        }
        Err(error) => println!("embeddings FAILED: {error}"),
    }

    let classifier = OpenAiProvider::new(profile(
        "npu-classifier",
        Role::Classification,
        "qwen25-1.5b",
        Device::Npu,
        8000,
        120,
    ))
    .expect("loopback endpoint");
    let labels = vec![
        "deterministic".to_owned(),
        "local_small".to_owned(),
        "upstream".to_owned(),
    ];
    for input in [
        "Rename a private helper with no callers outside its module.",
        "Change the retention policy for audited production run evidence.",
    ] {
        match classifier.classify(&ClassifyRequest {
            instruction: "Which execution tier should handle this engineering task?".to_owned(),
            input: input.to_owned(),
            labels: labels.clone(),
        }) {
            Ok(reply) => println!(
                "classify: {:<12} {} ms, placement {} <- {input}",
                reply.value,
                reply.latency_ms,
                reply.placement.describe()
            ),
            Err(error) => println!("classify FAILED: {error} <- {input}"),
        }
    }
}
