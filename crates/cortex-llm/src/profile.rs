//! Which model does which job, on which device.
//!
//! A profile binds four things that must never drift apart: the **role** the
//! model is allowed to play, the exact **model tag**, the **device** it was
//! deployed on, and the **runtime** that serves it. Selection is by role, and
//! the device policy filters before anything else — so removing a device from
//! the policy removes its models from every call site at once.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::device::{Device, DevicePolicy};

/// What a local model is permitted to do.
///
/// Roles are deliberately narrow. Each one is a job whose output can be
/// checked mechanically — a vector, a label from a closed set, a digest that
/// is compared against its source. There is no "assistant" role, because a
/// job nobody can check is a job that cannot be delegated to a local model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Produce embedding vectors for retrieval ordering.
    Embedding,
    /// Classify a bounded request into a closed set of labels.
    Classification,
    /// Precompute a digest of stable, per-revision structure, off the hot
    /// path, to be cached and reused.
    Digest,
}

impl Role {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedding => "embedding",
            Self::Classification => "classification",
            Self::Digest => "digest",
        }
    }

    /// True when a role sits on the request path and therefore has a latency
    /// budget measured in a user's patience rather than a build's.
    #[must_use]
    pub const fn is_hot_path(self) -> bool {
        matches!(self, Self::Embedding | Self::Classification)
    }
}

impl Display for Role {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Which local runtime serves a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    /// Ollama's native API.
    Ollama,
    /// Any OpenAI-compatible `/chat/completions` and `/embeddings` server:
    /// `OpenVINO` Model Server, llama.cpp, LM Studio, vLLM, `LocalAI`.
    OpenAiCompatible,
}

/// One deployed local model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfile {
    /// Stable name used in configuration, telemetry, and calibration reports.
    pub id: String,
    pub role: Role,
    /// Exact model tag. Never substituted for a smaller one.
    pub model: String,
    pub device: Device,
    pub runtime: Runtime,
    /// Loopback base URL of the serving runtime.
    pub base_url: String,
    /// How long this profile may take before a caller gives up. A digest
    /// profile on an integrated GPU is allowed minutes; an embedding profile
    /// on the hot path is not.
    pub timeout_seconds: u32,
    /// Set once the profile has passed the calibration gate for its role.
    /// Until then it may be observed in shadow, never trusted.
    #[serde(default)]
    pub gate_passed: bool,
    /// Free-text note carried into reports, e.g. which gate run cleared it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Why a role could not be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    /// No profile is configured for the role at all.
    NoProfile(Role),
    /// Profiles exist but every one sits on a device this deployment forbids.
    DeviceForbidden { role: Role, devices: Vec<Device> },
    /// Profiles exist on permitted devices but none has passed its gate.
    NotCalibrated { role: Role, candidates: Vec<String> },
}

impl Display for SelectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProfile(role) => write!(formatter, "no local profile configured for {role}"),
            Self::DeviceForbidden { role, devices } => {
                let names: Vec<&str> = devices.iter().map(|device| device.as_str()).collect();
                write!(
                    formatter,
                    "every {role} profile runs on a forbidden device ({})",
                    names.join(", ")
                )
            }
            Self::NotCalibrated { role, candidates } => write!(
                formatter,
                "no {role} profile has passed its gate ({})",
                candidates.join(", ")
            ),
        }
    }
}

impl std::error::Error for SelectionError {}

/// The deployed local models, and the policy that governs them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRegistry {
    #[serde(default)]
    policy: DevicePolicy,
    #[serde(default)]
    profiles: Vec<LlmProfile>,
}

impl ProfileRegistry {
    #[must_use]
    pub fn new(policy: DevicePolicy, profiles: Vec<LlmProfile>) -> Self {
        Self { policy, profiles }
    }

    #[must_use]
    pub fn policy(&self) -> &DevicePolicy {
        &self.policy
    }

    #[must_use]
    pub fn profiles(&self) -> &[LlmProfile] {
        &self.profiles
    }

    /// Choose the profile for a role.
    ///
    /// Order of rejection matters and is part of the contract: a forbidden
    /// device and an uncalibrated model are different problems with different
    /// fixes, so they are never collapsed into one "unavailable".
    ///
    /// # Errors
    ///
    /// Returns the specific reason no profile could serve the role.
    pub fn select(&self, role: Role) -> Result<&LlmProfile, SelectionError> {
        let for_role: Vec<&LlmProfile> = self
            .profiles
            .iter()
            .filter(|profile| profile.role == role)
            .collect();
        if for_role.is_empty() {
            return Err(SelectionError::NoProfile(role));
        }
        let permitted: Vec<&LlmProfile> = for_role
            .iter()
            .copied()
            .filter(|profile| self.policy.permits(profile.device))
            .collect();
        if permitted.is_empty() {
            let mut devices: Vec<Device> = for_role.iter().map(|profile| profile.device).collect();
            devices.sort_unstable();
            devices.dedup();
            return Err(SelectionError::DeviceForbidden { role, devices });
        }
        permitted
            .iter()
            .copied()
            .find(|profile| profile.gate_passed)
            .ok_or_else(|| SelectionError::NotCalibrated {
                role,
                candidates: permitted.iter().map(|profile| profile.id.clone()).collect(),
            })
    }

    /// Every profile, grouped by device, for a report that shows what a
    /// deployment actually has rather than what it hoped for.
    #[must_use]
    pub fn by_device(&self) -> BTreeMap<Device, Vec<&LlmProfile>> {
        let mut grouped: BTreeMap<Device, Vec<&LlmProfile>> = BTreeMap::new();
        for profile in &self.profiles {
            grouped.entry(profile.device).or_default().push(profile);
        }
        grouped
    }
}

#[cfg(test)]
mod tests {
    use super::{LlmProfile, ProfileRegistry, Role, Runtime, SelectionError};
    use crate::device::{Device, DevicePolicy};

    fn profile(id: &str, role: Role, device: Device, gate_passed: bool) -> LlmProfile {
        LlmProfile {
            id: id.to_owned(),
            role,
            model: format!("{id}-model"),
            device,
            runtime: Runtime::OpenAiCompatible,
            base_url: "http://127.0.0.1:8000".to_owned(),
            timeout_seconds: 30,
            gate_passed,
            note: None,
        }
    }

    #[test]
    fn a_forbidden_device_and_a_failed_gate_are_different_problems() {
        // Collapsing them into "unavailable" would hide which one to fix.
        let registry = ProfileRegistry::new(
            DevicePolicy::default(),
            vec![profile(
                "cpu-small",
                Role::Classification,
                Device::Cpu,
                true,
            )],
        );
        assert_eq!(
            registry.select(Role::Classification),
            Err(SelectionError::DeviceForbidden {
                role: Role::Classification,
                devices: vec![Device::Cpu],
            })
        );

        let registry = ProfileRegistry::new(
            DevicePolicy::default(),
            vec![profile(
                "npu-small",
                Role::Classification,
                Device::Npu,
                false,
            )],
        );
        assert_eq!(
            registry.select(Role::Classification),
            Err(SelectionError::NotCalibrated {
                role: Role::Classification,
                candidates: vec!["npu-small".to_owned()],
            })
        );
    }

    #[test]
    fn an_ungated_profile_is_never_selected_however_well_placed() {
        let registry = ProfileRegistry::new(
            DevicePolicy::default(),
            vec![
                profile("npu-fast", Role::Embedding, Device::Npu, false),
                profile("gpu-proven", Role::Embedding, Device::Gpu, true),
            ],
        );
        let chosen = registry.select(Role::Embedding).unwrap();
        assert_eq!(
            chosen.id, "gpu-proven",
            "calibration decides, placement does not"
        );
    }

    #[test]
    fn excluding_a_device_removes_its_models_everywhere_at_once() {
        let profiles = vec![
            profile("npu-embed", Role::Embedding, Device::Npu, true),
            profile("gpu-digest", Role::Digest, Device::Gpu, true),
        ];
        let registry = ProfileRegistry::new(DevicePolicy::new([Device::Npu]), profiles);
        assert!(registry.select(Role::Embedding).is_ok());
        assert!(matches!(
            registry.select(Role::Digest),
            Err(SelectionError::DeviceForbidden { .. })
        ));
    }

    #[test]
    fn an_absent_role_is_reported_as_absent_not_as_a_failure() {
        let registry = ProfileRegistry::default();
        assert_eq!(
            registry.select(Role::Digest),
            Err(SelectionError::NoProfile(Role::Digest))
        );
    }

    #[test]
    fn hot_path_roles_are_marked_so_a_slow_device_is_a_visible_choice() {
        assert!(Role::Embedding.is_hot_path());
        assert!(Role::Classification.is_hot_path());
        assert!(
            !Role::Digest.is_hot_path(),
            "a digest is precomputed; waiting for it costs nobody"
        );
    }
}
