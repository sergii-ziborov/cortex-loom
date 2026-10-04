use cortex_weavatrix::CompiledEvidenceBundle;

use super::agent_tools::AgentPrepare;
use crate::packet_store::{StoredPacket, certificate_hash};

pub(super) fn stored_packet(
    arguments: &AgentPrepare,
    compiled: &CompiledEvidenceBundle,
    id: &str,
    task_digest: &str,
    symbols: &[String],
    max_tokens: u32,
) -> StoredPacket {
    StoredPacket {
        id: id.to_owned(),
        repository: arguments.repository.clone(),
        task: arguments.task.clone(),
        task_hash: task_digest.to_owned(),
        run_id: arguments.run_id.clone(),
        symbols: symbols.to_vec(),
        snapshot_id: compiled.context.snapshot_id.clone(),
        certificate_hash: compiled
            .sufficiency
            .as_ref()
            .map(|report| certificate_hash(&report.certificate)),
        max_tokens,
    }
}
