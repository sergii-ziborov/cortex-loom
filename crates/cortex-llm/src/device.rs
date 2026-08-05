//! Where a local model runs, and who is allowed to say so.
//!
//! Device placement is a deployment decision and a measurement — never an
//! inference. A profile *declares* the device its runtime was configured for;
//! only a runtime that reports back can confirm it. The two are kept apart on
//! purpose, because the tempting shortcut — assuming the accelerator was used
//! because it was requested — is exactly how a project ends up claiming NPU
//! execution it never had.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

/// A compute device a local model can be placed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    /// Neural accelerator. Low latency, small models, hot path.
    Npu,
    /// Integrated or discrete GPU. Latency-tolerant, larger models.
    Gpu,
    /// Host CPU.
    Cpu,
}

impl Device {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Npu => "npu",
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
        }
    }

    /// Parse a device name, accepting the spellings the runtimes use.
    ///
    /// `OpenVINO` reports `NPU`, `GPU`, `GPU.0`, `CPU`; Ollama reports words
    /// like `cpu` and `gpu` in its process listing.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        let head = normalized.split(['.', ':', ' ']).next().unwrap_or_default();
        match head {
            "npu" | "vpu" | "ai_boost" => Some(Self::Npu),
            "gpu" | "igpu" | "dgpu" => Some(Self::Gpu),
            "cpu" => Some(Self::Cpu),
            _ => None,
        }
    }
}

impl Display for Device {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Which devices this deployment permits.
///
/// A machine whose CPU is permanently busy with the actual work is a real
/// operating constraint, not a preference: a local model that steals CPU from
/// the compiler and the test suite costs more than it saves. Excluding a
/// device here removes every profile bound to it from selection, rather than
/// leaving it to be remembered at each call site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePolicy {
    allowed: Vec<Device>,
}

impl Default for DevicePolicy {
    /// Accelerators only. The CPU has to be opted back in.
    fn default() -> Self {
        Self {
            allowed: vec![Device::Npu, Device::Gpu],
        }
    }
}

impl DevicePolicy {
    #[must_use]
    pub fn new(allowed: impl IntoIterator<Item = Device>) -> Self {
        let mut allowed: Vec<Device> = allowed.into_iter().collect();
        allowed.sort_unstable();
        allowed.dedup();
        Self { allowed }
    }

    #[must_use]
    pub fn permits(&self, device: Device) -> bool {
        self.allowed.contains(&device)
    }

    #[must_use]
    pub fn allowed(&self) -> &[Device] {
        &self.allowed
    }

    /// True when nothing is permitted, so a caller can say "no local model"
    /// rather than silently behaving as if one had failed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }
}

/// What a runtime actually reported, if anything.
///
/// `None` means the runtime did not say. It never means CPU, and it never
/// means the declared device: an unreported placement is unknown, and
/// reporting it as anything else would turn a configuration wish into a
/// measurement it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub declared: Device,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<Device>,
}

impl Placement {
    #[must_use]
    pub const fn declared(device: Device) -> Self {
        Self {
            declared: device,
            observed: None,
        }
    }

    #[must_use]
    pub const fn observed(declared: Device, observed: Device) -> Self {
        Self {
            declared,
            observed: Some(observed),
        }
    }

    /// True only when a runtime confirmed the device that was asked for.
    ///
    /// This is the predicate any claim about acceleration must be gated on.
    #[must_use]
    pub fn is_confirmed(&self) -> bool {
        self.observed == Some(self.declared)
    }

    /// A short, honest rendering for telemetry and reports.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.observed {
            Some(observed) if observed == self.declared => format!("{} (confirmed)", self.declared),
            Some(observed) => format!("{} requested, {observed} used", self.declared),
            None => format!("{} (unconfirmed)", self.declared),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Device, DevicePolicy, Placement};

    #[test]
    fn runtime_device_spellings_are_recognised() {
        assert_eq!(Device::parse("NPU"), Some(Device::Npu));
        assert_eq!(Device::parse("GPU.0"), Some(Device::Gpu));
        assert_eq!(Device::parse(" cpu "), Some(Device::Cpu));
        assert_eq!(Device::parse("AI_BOOST"), Some(Device::Npu));
        assert_eq!(Device::parse("tpu"), None, "unknown stays unknown");
    }

    #[test]
    fn the_default_policy_excludes_the_cpu() {
        let policy = DevicePolicy::default();
        assert!(policy.permits(Device::Npu));
        assert!(policy.permits(Device::Gpu));
        assert!(
            !policy.permits(Device::Cpu),
            "the CPU is busy with the real work; it opts in explicitly"
        );
    }

    #[test]
    fn an_unreported_placement_is_unknown_and_never_a_claim() {
        let wished = Placement::declared(Device::Npu);
        assert!(!wished.is_confirmed());
        assert_eq!(wished.describe(), "npu (unconfirmed)");

        let fell_back = Placement::observed(Device::Npu, Device::Cpu);
        assert!(!fell_back.is_confirmed());
        assert_eq!(
            fell_back.describe(),
            "npu requested, cpu used",
            "a silent fallback has to be visible"
        );

        let real = Placement::observed(Device::Npu, Device::Npu);
        assert!(real.is_confirmed());
        assert_eq!(real.describe(), "npu (confirmed)");
    }
}
