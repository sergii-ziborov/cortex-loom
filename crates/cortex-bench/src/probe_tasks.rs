//! Ten probe tasks for a fresh savings/quality check.
//!
//! These are deliberately different from the core fixtures: mix of blast
//! radius, HTTP contracts, local-LLM wiring, leases, and module ownership.
//! Anchors are Rust literals that exist in this repository today.

use crate::Anchor;
use crate::tasks::BenchTask;

/// Probe set used by `cortex-bench --set probe`.
#[must_use]
pub const fn probe_tasks() -> &'static [BenchTask] {
    &[
        BenchTask {
            id: "probe-route-blast-radius",
            prompt: "Who depends on `route` if its signature changes?",
            symbol: Some("route"),
            naive_globs: &[
                concat!("crates/cortex-", "router/src/*.rs"),
                "crates/cortex-mcp/src/*.rs",
                concat!("apps/cortex-", "server/src/*.rs"),
            ],
            anchors: &[
                Anchor {
                    id: "seed-route",
                    any_of: &[concat!("fn ", "route"), concat!("pub fn ", "route")],
                },
                Anchor {
                    id: "mcp-caller",
                    any_of: &[concat!(
                        "route_metric_tools::register",
                        "(server, &state, route)"
                    )],
                },
                Anchor {
                    id: "http-serve",
                    any_of: &[concat!("serve_", "http")],
                },
                Anchor {
                    id: "owning-crate",
                    any_of: &[concat!("cortex-", "router")],
                },
            ],
        },
        BenchTask {
            id: "probe-compile-bundle-callers",
            prompt: concat!(
                "Who calls `compile_",
                "evidence_bundle` versus `compile_probe_bundle`, and what breaks if the generic path starts refusing more packets?"
            ),
            symbol: Some(concat!("compile_", "evidence_bundle")),
            naive_globs: &[
                concat!("crates/cortex-", "weavatrix/src/*.rs"),
                "crates/cortex-mcp/src/*.rs",
                "crates/cortex-bench/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "fn-compile",
                    any_of: &[concat!("compile_", "evidence_bundle")],
                },
                Anchor {
                    id: "mcp-build",
                    any_of: &[concat!("match compile_", "probe_bundle(")],
                },
                Anchor {
                    id: "bench-arm",
                    any_of: &[concat!("cortex_", "arm")],
                },
                Anchor {
                    id: "owning-module",
                    any_of: &[concat!("cortex-", "weavatrix")],
                },
            ],
        },
        BenchTask {
            id: "probe-usage-quality-contract",
            prompt: concat!(
                "What breaks if the `GET /api/usage/",
                "quality` HTTP contract changes?"
            ),
            symbol: None,
            naive_globs: &[
                concat!("apps/cortex-", "server/src/*.rs"),
                concat!("crates/cortex-", "store/src/*.rs"),
            ],
            anchors: &[
                Anchor {
                    id: "http-route",
                    any_of: &[concat!("/api/usage/", "quality")],
                },
                Anchor {
                    id: "handler",
                    any_of: &[concat!("usage_", "quality")],
                },
                Anchor {
                    id: "sibling-summary",
                    any_of: &[concat!("/api/usage/", "summary")],
                },
                Anchor {
                    id: "owning-surface",
                    any_of: &[
                        concat!("cortex-", "server"),
                        concat!("apps/cortex-", "server"),
                    ],
                },
            ],
        },
        BenchTask {
            id: "probe-adapter-export-contract",
            prompt: concat!(
                "What breaks if `GET /api/",
                "adapters/",
                "{agent}` or `export_",
                "adapter` changes?"
            ),
            symbol: Some(concat!("export_", "adapter")),
            naive_globs: &[
                concat!("apps/cortex-", "server/src/*.rs"),
                "crates/cortex-adapters/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "http-route",
                    any_of: &[
                        concat!("/api/", "adapters/", "{agent}"),
                        concat!("/api/", "adapters/"),
                    ],
                },
                Anchor {
                    id: "export-fn",
                    any_of: &[concat!("export_", "adapter")],
                },
                Anchor {
                    id: "agent-kind",
                    any_of: &[concat!("Agent", "Kind")],
                },
                Anchor {
                    id: "mcp-launch",
                    any_of: &[concat!("Mcp", "Launch")],
                },
            ],
        },
        BenchTask {
            id: "probe-llm-profile-gate",
            prompt: concat!(
                "How does `Profile",
                "Registry` refuse an uncalibrated classification profile?"
            ),
            symbol: Some(concat!("Profile", "Registry")),
            naive_globs: &["crates/cortex-llm/src/*.rs", "config/*.json"],
            anchors: &[
                Anchor {
                    id: "struct",
                    any_of: &[concat!("Profile", "Registry")],
                },
                Anchor {
                    id: "gate-field",
                    any_of: &[concat!("gate_", "passed"), concat!("gate", "Passed")],
                },
                Anchor {
                    id: "select-fn",
                    any_of: &[concat!("fn ", "select"), concat!("pub fn ", "select")],
                },
                Anchor {
                    id: "not-calibrated",
                    any_of: &[concat!("Not", "Calibrated"), concat!("not ", "calibrated")],
                },
            ],
        },
        BenchTask {
            id: "probe-llm-route-wiring",
            prompt: concat!(
                "How does `CORTEX_",
                "LLM` wire the gated classifier into `route_",
                "work`?"
            ),
            symbol: None,
            naive_globs: &["crates/cortex-mcp/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "env-flag",
                    any_of: &[concat!("CORTEX_", "LLM")],
                },
                Anchor {
                    id: "router-type",
                    any_of: &[concat!("Llm", "Router")],
                },
                Anchor {
                    id: "merge-floor",
                    any_of: &[concat!("merge_", "tiers")],
                },
                Anchor {
                    id: "tool-name",
                    any_of: &[concat!("route_", "work")],
                },
            ],
        },
        BenchTask {
            id: "probe-claim-lease",
            prompt: concat!(
                "Where is `claim_",
                "lease` enforced in the run engine, and what clears it?"
            ),
            symbol: Some(concat!("claim_", "lease")),
            naive_globs: &["crates/cortex-run/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "claim",
                    any_of: &[concat!("claim_", "lease")],
                },
                Anchor {
                    id: "enforce",
                    any_of: &[concat!("enforce_", "lease")],
                },
                Anchor {
                    id: "release",
                    any_of: &[concat!("release_", "lease")],
                },
                Anchor {
                    id: "clear",
                    any_of: &[concat!("clear_", "lease")],
                },
            ],
        },
        BenchTask {
            id: "probe-device-policy",
            prompt: concat!(
                "What devices does `Device",
                "Policy` allow by default, and how does it refuse CPU for hot-path roles?"
            ),
            symbol: Some(concat!("Device", "Policy")),
            naive_globs: &["crates/cortex-llm/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "struct",
                    any_of: &[concat!("Device", "Policy")],
                },
                Anchor {
                    id: "accelerator-n",
                    any_of: &[concat!("N", "pu"), concat!("n", "pu")],
                },
                Anchor {
                    id: "accelerator-g",
                    any_of: &[concat!("G", "pu"), concat!("g", "pu")],
                },
                Anchor {
                    id: "permits",
                    any_of: &[concat!("fn ", "permits"), concat!("pub fn ", "permits")],
                },
            ],
        },
        BenchTask {
            id: "probe-shadow-handle",
            prompt: concat!(
                "How is `Shadow",
                "Handle` spawned, and which env flag turns shadow mode on?"
            ),
            symbol: Some(concat!("Shadow", "Handle")),
            naive_globs: &[
                "crates/cortex-shadow/src/*.rs",
                "crates/cortex-mcp/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "handle",
                    any_of: &[concat!("Shadow", "Handle")],
                },
                Anchor {
                    id: "env-flag",
                    // Split the literal in the fixture so search evidence
                    // cannot satisfy the anchor by finding this benchmark.
                    any_of: &[concat!("CORTEX_", "SHADOW")],
                },
                Anchor {
                    id: "observe",
                    any_of: &[concat!("fn ", "observe"), concat!("pub fn ", "observe")],
                },
                Anchor {
                    id: "spawn",
                    any_of: &[concat!("pub fn ", "spawn"), concat!("fn ", "spawn")],
                },
            ],
        },
        BenchTask {
            id: "probe-store-module-map",
            prompt: concat!(
                "Which module owns `Graph",
                "Store` and the run persistence surface?"
            ),
            symbol: Some(concat!("Graph", "Store")),
            naive_globs: &[concat!("crates/cortex-", "store/src/*.rs")],
            anchors: &[
                Anchor {
                    id: "graph-store",
                    any_of: &[concat!("Graph", "Store")],
                },
                Anchor {
                    id: "run-store",
                    any_of: &[concat!("run_", "store"), concat!("Run", "Store")],
                },
                Anchor {
                    id: "crate-name",
                    any_of: &[concat!("cortex-", "store")],
                },
                Anchor {
                    id: "open",
                    any_of: &[concat!("fn ", "open"), concat!("pub fn ", "open")],
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_source_cannot_satisfy_its_own_anchors() {
        let source = include_str!("probe_tasks.rs").to_ascii_lowercase();
        for task in probe_tasks() {
            for anchor in task.anchors {
                for candidate in anchor.any_of {
                    assert!(
                        !source.contains(&candidate.to_ascii_lowercase()),
                        "{} / {} is present in the fixture source: {candidate}",
                        task.id,
                        anchor.id
                    );
                }
            }
        }
    }
}
