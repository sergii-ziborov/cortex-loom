//! Bounded in-process store of compiled packets for `cortex_expand`.
//!
//! The handle is the compiler's revision-bound `context.packetId`, not a
//! hash of repository+task. Two prepares of the same question on different
//! trees must not share an identity or overwrite each other.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use cortex_context::snapshot_is_stale;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_PACKETS: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct StoredPacket {
    pub id: String,
    pub repository: PathBuf,
    pub task: String,
    pub task_hash: String,
    pub run_id: Option<String>,
    pub symbols: Vec<String>,
    pub snapshot_id: Option<String>,
    pub certificate_hash: Option<String>,
    pub max_tokens: u32,
}

#[derive(Default)]
pub(crate) struct PacketStore {
    inner: Mutex<HashMap<String, StoredPacket>>,
}

impl PacketStore {
    pub(crate) fn insert(&self, packet: StoredPacket) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        if guard.len() >= MAX_PACKETS
            && let Some(oldest) = guard.keys().next().cloned()
        {
            guard.remove(&oldest);
        }
        guard.insert(packet.id.clone(), packet);
    }

    pub(crate) fn get(&self, id: &str) -> Option<StoredPacket> {
        self.inner.lock().ok()?.get(id).cloned()
    }
}

#[must_use]
pub(crate) fn task_hash(task: &str) -> String {
    let digest = Sha256::digest(task.as_bytes());
    format!("{digest:x}").chars().take(16).collect()
}

#[must_use]
pub(crate) fn certificate_hash(certificate: &cortex_context::CoverageCertificate) -> String {
    let body = format!(
        "{:?}|{:?}|{:?}",
        certificate.required, certificate.satisfied, certificate.missing
    );
    let digest = Sha256::digest(body.as_bytes());
    format!("ch_{digest:x}").chars().take(15).collect()
}

/// Structured refuse when the tree moved under a stored packet.
#[must_use]
pub(crate) fn packet_stale(stored: &StoredPacket, current_snapshot: &str) -> Option<Value> {
    snapshot_is_stale(stored.snapshot_id.as_deref(), current_snapshot).then(|| {
        json!({
            "error": "packetStale",
            "packetId": stored.id,
            "compiledSnapshot": stored.snapshot_id,
            "currentSnapshot": current_snapshot,
            "taskHash": stored.task_hash,
            "certificateHash": stored.certificate_hash,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_packet_ids_do_not_overwrite_each_other() {
        let store = PacketStore::default();
        store.insert(sample("pk_aaa", "git:a+dirty:0"));
        store.insert(sample("pk_bbb", "git:b+dirty:0"));
        assert_eq!(
            store.get("pk_aaa").and_then(|packet| packet.snapshot_id),
            Some("git:a+dirty:0".to_owned())
        );
        assert_eq!(
            store.get("pk_bbb").and_then(|packet| packet.snapshot_id),
            Some("git:b+dirty:0".to_owned())
        );
    }

    #[test]
    fn a_changed_snapshot_is_packet_stale() {
        let stored = sample("pk_old", "git:a+dirty:0");
        let blocked = packet_stale(&stored, "git:b+dirty:1").expect("stale");
        assert_eq!(blocked["error"], "packetStale");
        assert_eq!(blocked["packetId"], "pk_old");
        assert_eq!(blocked["compiledSnapshot"], "git:a+dirty:0");
        assert_eq!(blocked["currentSnapshot"], "git:b+dirty:1");
        assert!(packet_stale(&stored, "git:a+dirty:0").is_none());
    }

    fn sample(id: &str, snapshot: &str) -> StoredPacket {
        StoredPacket {
            id: id.to_owned(),
            repository: PathBuf::from("."),
            task: "Who calls alpha?".to_owned(),
            task_hash: task_hash("Who calls alpha?"),
            run_id: None,
            symbols: vec!["alpha".to_owned()],
            snapshot_id: Some(snapshot.to_owned()),
            certificate_hash: None,
            max_tokens: 4_000,
        }
    }
}
