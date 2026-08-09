use cortex_run::RunStatus;

pub(super) const fn status_name(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
    }
}

pub(super) fn percentile(sorted: &[u64], percentile: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * usize::from(percentile))
        .div_ceil(100)
        .max(1);
    sorted[rank - 1]
}
