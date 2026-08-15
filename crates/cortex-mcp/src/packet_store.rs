//! Bounded in-process store of compiled packets for `cortex_expand`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

const MAX_PACKETS: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct StoredPacket {
    pub id: String,
    pub repository: PathBuf,
    pub task: String,
    pub run_id: Option<String>,
    pub symbols: Vec<String>,
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
pub(crate) fn packet_id(repository: &str, task: &str) -> String {
    let digest = Sha256::digest(format!("{repository}\n{task}").as_bytes());
    format!("pkt:{digest:x}").chars().take(20).collect()
}
