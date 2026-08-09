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
                "crates/cortex-router/src/*.rs",
                "crates/cortex-mcp/src/*.rs",
                "apps/cortex-server/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "seed-route",
                    any_of: &["fn route", "pub fn route"],
                },
                Anchor {
                    id: "mcp-caller",
                    any_of: &["build_server"],
                },
                Anchor {
                    id: "http-serve",
                    any_of: &["serve_http"],
                },
                Anchor {
                    id: "owning-crate",
                    any_of: &["cortex-router"],
                },
            ],
        },
        BenchTask {
            id: "probe-compile-bundle-callers",
            prompt: "Who calls `compile_evidence_bundle`, and what breaks if it starts refusing more packets?",
            symbol: Some("compile_evidence_bundle"),
            naive_globs: &[
                "crates/cortex-weavatrix/src/*.rs",
                "crates/cortex-mcp/src/*.rs",
                "crates/cortex-bench/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "fn-compile",
                    any_of: &["compile_evidence_bundle"],
                },
                Anchor {
                    id: "mcp-build",
                    any_of: &["build_server"],
                },
                Anchor {
                    id: "bench-arm",
                    any_of: &["cortex_arm"],
                },
                Anchor {
                    id: "owning-module",
                    any_of: &["cortex-weavatrix"],
                },
            ],
        },
        BenchTask {
            id: "probe-usage-quality-contract",
            prompt: "What breaks if the `GET /api/usage/quality` HTTP contract changes?",
            symbol: None,
            naive_globs: &[
                "apps/cortex-server/src/*.rs",
                "crates/cortex-store/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "http-route",
                    any_of: &["/api/usage/quality"],
                },
                Anchor {
                    id: "handler",
                    any_of: &["usage_quality"],
                },
                Anchor {
                    id: "sibling-summary",
                    any_of: &["/api/usage/summary"],
                },
                Anchor {
                    id: "owning-surface",
                    any_of: &["cortex-server", "apps/cortex-server"],
                },
            ],
        },
        BenchTask {
            id: "probe-adapter-export-contract",
            prompt: "What breaks if `GET /api/adapters/{agent}` or `export_adapter` changes?",
            symbol: Some("export_adapter"),
            naive_globs: &[
                "apps/cortex-server/src/*.rs",
                "crates/cortex-adapters/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "http-route",
                    any_of: &["/api/adapters/{agent}", "/api/adapters/"],
                },
                Anchor {
                    id: "export-fn",
                    any_of: &["export_adapter"],
                },
                Anchor {
                    id: "agent-kind",
                    any_of: &["AgentKind"],
                },
                Anchor {
                    id: "mcp-launch",
                    any_of: &["McpLaunch"],
                },
            ],
        },
        BenchTask {
            id: "probe-llm-profile-gate",
            prompt: "How does `ProfileRegistry` refuse an uncalibrated classification profile?",
            symbol: Some("ProfileRegistry"),
            naive_globs: &["crates/cortex-llm/src/*.rs", "config/*.json"],
            anchors: &[
                Anchor {
                    id: "struct",
                    any_of: &["ProfileRegistry"],
                },
                Anchor {
                    id: "gate-field",
                    any_of: &["gate_passed", "gatePassed"],
                },
                Anchor {
                    id: "select-fn",
                    any_of: &["fn select", "pub fn select"],
                },
                Anchor {
                    id: "not-calibrated",
                    any_of: &["NotCalibrated", "not calibrated"],
                },
            ],
        },
        BenchTask {
            id: "probe-llm-route-wiring",
            prompt: "How does `CORTEX_LLM` wire the gated classifier into `route_work`?",
            symbol: None,
            naive_globs: &["crates/cortex-mcp/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "env-flag",
                    any_of: &["CORTEX_LLM"],
                },
                Anchor {
                    id: "router-type",
                    any_of: &["LlmRouter"],
                },
                Anchor {
                    id: "merge-floor",
                    any_of: &["merge_tiers"],
                },
                Anchor {
                    id: "tool-name",
                    any_of: &["route_work"],
                },
            ],
        },
        BenchTask {
            id: "probe-claim-lease",
            prompt: "Where is `claim_lease` enforced in the run engine, and what clears it?",
            symbol: Some("claim_lease"),
            naive_globs: &["crates/cortex-run/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "claim",
                    any_of: &["claim_lease"],
                },
                Anchor {
                    id: "enforce",
                    any_of: &["enforce_lease"],
                },
                Anchor {
                    id: "release",
                    any_of: &["release_lease"],
                },
                Anchor {
                    id: "clear",
                    any_of: &["clear_lease"],
                },
            ],
        },
        BenchTask {
            id: "probe-device-policy",
            prompt: "What devices does `DevicePolicy` allow by default, and how does it refuse CPU for hot-path roles?",
            symbol: Some("DevicePolicy"),
            naive_globs: &["crates/cortex-llm/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "struct",
                    any_of: &["DevicePolicy"],
                },
                Anchor {
                    id: "npu",
                    any_of: &["Npu", "npu"],
                },
                Anchor {
                    id: "gpu",
                    any_of: &["Gpu", "gpu"],
                },
                Anchor {
                    id: "permits",
                    any_of: &["fn permits", "pub fn permits"],
                },
            ],
        },
        BenchTask {
            id: "probe-shadow-handle",
            prompt: "How is `ShadowHandle` spawned, and which env flag turns shadow mode on?",
            symbol: Some("ShadowHandle"),
            naive_globs: &[
                "crates/cortex-shadow/src/*.rs",
                "crates/cortex-mcp/src/*.rs",
            ],
            anchors: &[
                Anchor {
                    id: "handle",
                    any_of: &["ShadowHandle"],
                },
                Anchor {
                    id: "env-flag",
                    any_of: &["CORTEX_SHADOW"],
                },
                Anchor {
                    id: "observe",
                    any_of: &["fn observe", "pub fn observe"],
                },
                Anchor {
                    id: "spawn",
                    any_of: &["pub fn spawn", "fn spawn"],
                },
            ],
        },
        BenchTask {
            id: "probe-store-module-map",
            prompt: "Which module owns `GraphStore` and the run persistence surface?",
            symbol: Some("GraphStore"),
            naive_globs: &["crates/cortex-store/src/*.rs"],
            anchors: &[
                Anchor {
                    id: "graph-store",
                    any_of: &["GraphStore"],
                },
                Anchor {
                    id: "run-store",
                    any_of: &["run_store", "RunStore"],
                },
                Anchor {
                    id: "crate-name",
                    any_of: &["cortex-store"],
                },
                Anchor {
                    id: "open",
                    any_of: &["fn open", "pub fn open"],
                },
            ],
        },
    ]
}
