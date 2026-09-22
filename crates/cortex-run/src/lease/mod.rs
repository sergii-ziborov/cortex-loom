//! Executor leases: cooperative, expiring exclusivity for run nodes.
//!
//! A lease lets one explicit executor identity work on a node without a
//! second agent duplicating or clobbering the attempt. Expiry is evaluated
//! lazily against each command's timestamp, so lease decisions are
//! deterministic under replay. A node without a lease stays open: leases add
//! exclusivity, never new authority.

use cortex_domain::{GraphDocument, NodeKind};

use crate::{
    ExecutorIdentity, MAX_EXECUTOR_ID_BYTES, MAX_LEASE_TTL_SECONDS, MIN_LEASE_TTL_SECONDS,
    NodeLeaseState, NodeRunStatus, RunDocument, RunError,
};

pub(super) fn claim_lease(
    run: &mut RunDocument,
    graph: &GraphDocument,
    node_id: &str,
    executor: &ExecutorIdentity,
    ttl_seconds: u32,
    now: i64,
) -> Result<(), RunError> {
    validate_executor(executor)?;
    if !(MIN_LEASE_TTL_SECONDS..=MAX_LEASE_TTL_SECONDS).contains(&ttl_seconds) {
        return Err(RunError::InvalidLeaseTtl(ttl_seconds));
    }
    let definition = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    if definition.kind == NodeKind::Retry {
        return Err(RunError::LeaseUnsupportedNode(node_id.to_owned()));
    }
    let node = run
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    if !matches!(node.status, NodeRunStatus::Ready | NodeRunStatus::Running) {
        return Err(RunError::InvalidNodeState {
            node: node_id.to_owned(),
            expected: NodeRunStatus::Ready,
            current: node.status,
        });
    }
    if let Some(lease) = &node.lease
        && lease.expires_at > now
        && lease.executor != *executor
    {
        return Err(held(node_id, lease));
    }
    node.lease = Some(NodeLeaseState {
        executor: executor.clone(),
        claimed_at: now,
        expires_at: now.saturating_add(i64::from(ttl_seconds)),
    });
    Ok(())
}

pub(super) fn release_lease(
    run: &mut RunDocument,
    node_id: &str,
    executor: &ExecutorIdentity,
) -> Result<(), RunError> {
    validate_executor(executor)?;
    let node = run
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
        .ok_or_else(|| RunError::NodeNotFound(node_id.to_owned()))?;
    let lease = node
        .lease
        .as_ref()
        .ok_or_else(|| RunError::LeaseNotFound(node_id.to_owned()))?;
    if lease.executor != *executor {
        return Err(held(node_id, lease));
    }
    node.lease = None;
    Ok(())
}

/// Reject node work from anyone but the holder of an unexpired lease. An
/// expired lease leaves the node open: expiry is the takeover mechanism.
pub(super) fn enforce_lease(
    run: &RunDocument,
    node_id: &str,
    executor: Option<&ExecutorIdentity>,
    now: i64,
) -> Result<(), RunError> {
    let Some(node) = run.nodes.iter().find(|node| node.node_id == node_id) else {
        return Ok(());
    };
    if let Some(lease) = &node.lease
        && lease.expires_at > now
        && executor != Some(&lease.executor)
    {
        return Err(held(node_id, lease));
    }
    Ok(())
}

/// Drop the lease once the attempt it guarded is over.
pub(super) fn clear_lease(run: &mut RunDocument, node_id: &str) {
    if let Some(node) = run.nodes.iter_mut().find(|node| node.node_id == node_id) {
        node.lease = None;
    }
}

fn validate_executor(executor: &ExecutorIdentity) -> Result<(), RunError> {
    if executor.id.trim().is_empty() {
        return Err(RunError::InvalidExecutor("id must not be empty"));
    }
    if executor.id.len() > MAX_EXECUTOR_ID_BYTES {
        return Err(RunError::InvalidExecutor("id exceeds the size limit"));
    }
    Ok(())
}

fn held(node_id: &str, lease: &NodeLeaseState) -> RunError {
    RunError::LeaseHeld {
        node: node_id.to_owned(),
        holder: format!("{:?}:{}", lease.executor.kind, lease.executor.id),
        expires_at: lease.expires_at,
    }
}
