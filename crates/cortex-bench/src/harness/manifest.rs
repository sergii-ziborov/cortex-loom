//! Runtime-observed benchmark identity and environment metadata.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// One value detected by the harness, or the reason detection failed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub value: Option<String>,
    pub reason: Option<String>,
}

impl Observation {
    #[must_use]
    pub fn known(value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            reason: None,
        }
    }

    #[must_use]
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            value: None,
            reason: Some(reason.into()),
        }
    }
}

/// Exact repository state observed for one benchmark input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryManifest {
    pub path: String,
    pub commit: Observation,
    pub dirty: Option<bool>,
    pub dirty_reason: Option<String>,
}

/// Model identity for a suite, including explicit absence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub name: Observation,
    pub runtime: Observation,
    pub digest: Observation,
    pub parameters: BTreeMap<String, String>,
}

impl ModelManifest {
    #[must_use]
    pub fn not_used() -> Self {
        let reason = "suite does not invoke a model";
        Self {
            name: Observation::unknown(reason),
            runtime: Observation::unknown(reason),
            digest: Observation::unknown(reason),
            parameters: BTreeMap::new(),
        }
    }
}

/// MCP transport and payload accounting used by a suite.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpManifest {
    pub protocol_version: String,
    pub transport: String,
    pub profile: String,
    pub payload_representation: String,
}

impl McpManifest {
    #[must_use]
    pub fn in_process() -> Self {
        Self {
            protocol_version: "not-used".to_owned(),
            transport: "in-process".to_owned(),
            profile: "embedded-weavatrix".to_owned(),
            payload_representation: "serialized-tool-payload".to_owned(),
        }
    }
}

/// Self-describing identity shared by all benchmark reports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkManifest {
    pub report_schema: String,
    pub suite_version: String,
    pub harness_version: String,
    pub command: Vec<String>,
    pub operating_system: String,
    pub architecture: String,
    pub cortex: RepositoryManifest,
    pub target: RepositoryManifest,
    pub engines: BTreeMap<String, Observation>,
    pub model: ModelManifest,
    pub mcp: McpManifest,
}

impl BenchmarkManifest {
    #[must_use]
    pub fn detect(
        suite_version: impl Into<String>,
        target: &Path,
        command: &[String],
        mcp: McpManifest,
    ) -> Self {
        let cortex_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        Self {
            report_schema: "cortex-benchmark.v2".to_owned(),
            suite_version: suite_version.into(),
            harness_version: env!("CARGO_PKG_VERSION").to_owned(),
            command: command.to_vec(),
            operating_system: std::env::consts::OS.to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            cortex: repository_manifest(&cortex_root),
            target: repository_manifest(target),
            engines: engine_versions(&cortex_root),
            model: ModelManifest::not_used(),
            mcp,
        }
    }
}

fn repository_manifest(path: &Path) -> RepositoryManifest {
    let display = path
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .display()
        .to_string();
    let commit = git_output(path, &["rev-parse", "HEAD"]);
    let status = git_output(path, &["status", "--porcelain"]);
    let (dirty, dirty_reason) = match status.value {
        Some(value) => (Some(!value.trim().is_empty()), None),
        None => (None, status.reason),
    };
    RepositoryManifest {
        path: display,
        commit,
        dirty,
        dirty_reason,
    }
}

fn git_output(path: &Path, args: &[&str]) -> Observation {
    match Command::new("git").arg("-C").arg(path).args(args).output() {
        Ok(output) if output.status.success() => {
            Observation::known(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        }
        Ok(output) => {
            Observation::unknown(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
        Err(error) => Observation::unknown(format!("git unavailable: {error}")),
    }
}

fn engine_versions(root: &Path) -> BTreeMap<String, Observation> {
    let mut engines = BTreeMap::new();
    let lock = std::fs::read_to_string(root.join("Cargo.lock"));
    let packages = lock.as_deref().map(package_versions).unwrap_or_default();
    for name in [
        "blazingly-json",
        "mcport",
        "weavatrix-edit",
        "weavatrix-refactor-plan",
        "weavatrix-rust",
    ] {
        let observation = packages.get(name).map_or_else(
            || Observation::unknown(format!("{name} absent from Cargo.lock")),
            |version| Observation::known(version.clone()),
        );
        engines.insert(name.to_owned(), observation);
    }
    engines.insert("npm-weavatrix".to_owned(), npm_weavatrix_version(root));
    engines
}

fn package_versions(lock: &str) -> BTreeMap<String, String> {
    lock.split("[[package]]")
        .skip(1)
        .filter_map(|block| {
            let name = quoted_field(block, "name")?;
            let version = quoted_field(block, "version")?;
            Some((name, version))
        })
        .collect()
}

fn quoted_field(block: &str, field: &str) -> Option<String> {
    let prefix = format!("{field} = \"");
    block.lines().map(str::trim).find_map(|line| {
        line.strip_prefix(&prefix)?
            .strip_suffix('"')
            .map(str::to_owned)
    })
}

fn npm_weavatrix_version(root: &Path) -> Observation {
    let body = match std::fs::read_to_string(root.join(".mcp.json")) {
        Ok(body) => body,
        Err(error) => return Observation::unknown(format!("cannot read .mcp.json: {error}")),
    };
    let value: serde_json::Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => return Observation::unknown(format!("invalid .mcp.json: {error}")),
    };
    value
        .pointer("/mcpServers/weavatrix/args")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .find_map(|argument| argument.strip_prefix("weavatrix@"))
        .map_or_else(
            || Observation::unknown("configured Weavatrix package not found"),
            Observation::known,
        )
}
