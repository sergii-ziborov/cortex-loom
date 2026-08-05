//! Human-readable rendering of a [`BenchReport`].
//!
//! The renderer prints the caveats next to the numbers rather than in a
//! footnote, because the numbers are easy to quote and the caveats are what
//! make them true.

use std::fmt::Write as _;

use crate::{ArmKind, ArmMeasurement, BenchReport, TaskResult, token_delta};

/// Render the whole run as plain text.
#[must_use]
pub fn render(report: &BenchReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Cortex Loom context benchmark");
    let _ = writeln!(out, "repository: {}", report.repository);
    let _ = writeln!(out, "budget:     {} tokens", report.budget);
    if let Some(stamp) = &report.stamp {
        let _ = writeln!(out, "stamp:      {stamp}");
    }
    out.push('\n');
    for task in &report.tasks {
        render_task(&mut out, task);
    }
    render_totals(&mut out, report);
    out.push_str(CAVEATS);
    out
}

fn render_task(out: &mut String, task: &TaskResult) {
    let _ = writeln!(out, "── {} ──", task.task_id);
    let _ = writeln!(out, "   {}", task.prompt);
    let _ = writeln!(
        out,
        "   {:<14} {:>9} {:>7} {:>8} {:>12}",
        "arm", "tokens", "units", "recall", "tokens/fact"
    );
    for arm in &task.arms {
        render_arm(out, arm);
    }
    render_deltas(out, task);
    out.push('\n');
}

fn render_arm(out: &mut String, arm: &ArmMeasurement) {
    if !arm.available {
        let reason = arm.unavailable_reason.as_deref().unwrap_or("unavailable");
        let _ = writeln!(out, "   {:<14} {reason}", arm.arm.id());
        return;
    }
    let per_fact = arm
        .tokens_per_fact()
        .map_or_else(|| "n/a".to_owned(), |value| format!("{value:.0}"));
    let _ = writeln!(
        out,
        "   {:<14} {:>9} {:>7} {:>7.0}% {:>12}",
        arm.arm.id(),
        arm.context_tokens,
        arm.units,
        arm.recall() * 100.0,
        per_fact
    );
    if !arm.missing_anchors.is_empty() {
        let _ = writeln!(out, "     missing: {}", arm.missing_anchors.join(", "));
    }
    for note in &arm.notes {
        let _ = writeln!(out, "     note: {note}");
    }
}

fn render_deltas(out: &mut String, task: &TaskResult) {
    let pairs = [
        (ArmKind::Naive, ArmKind::CortexLoom),
        (ArmKind::WeavatrixRaw, ArmKind::CortexLoom),
        // The comparison the project stands or falls on: does planning the
        // operations from the task beat asking the same four every time?
        (ArmKind::CortexLoom, ArmKind::CortexLoomTargeted),
        // And the control: the same planned evidence with Weavatrix's own
        // budget and no compiler. Whatever separates these two is ours.
        (ArmKind::WeavatrixPlanned, ArmKind::CortexLoomTargeted),
        (ArmKind::Naive, ArmKind::CortexLoomTargeted),
    ];
    for (from, to) in pairs {
        let (Some(from_arm), Some(to_arm)) = (task.arm(from), task.arm(to)) else {
            continue;
        };
        let Some(delta) = token_delta(from_arm, to_arm) else {
            continue;
        };
        let direction = if delta >= 0.0 { "fewer" } else { "more" };
        let _ = writeln!(
            out,
            "   {} → {}: {:.0}% {direction} tokens, recall {:.0}% → {:.0}%",
            from.id(),
            to.id(),
            delta.abs() * 100.0,
            from_arm.recall() * 100.0,
            to_arm.recall() * 100.0
        );
    }
}

fn render_totals(out: &mut String, report: &BenchReport) {
    let _ = writeln!(out, "── totals across {} tasks ──", report.tasks.len());
    for kind in [
        ArmKind::Naive,
        ArmKind::WeavatrixRaw,
        ArmKind::CortexLoom,
        ArmKind::WeavatrixPlanned,
        ArmKind::CortexLoomTargeted,
    ] {
        let arms: Vec<&ArmMeasurement> = report
            .tasks
            .iter()
            .filter_map(|task| task.arm(kind))
            .filter(|arm| arm.available)
            .collect();
        if arms.is_empty() {
            let _ = writeln!(out, "   {:<14} no measured task", kind.id());
            continue;
        }
        let tokens: u32 = arms
            .iter()
            .map(|arm| arm.context_tokens)
            .fold(0, u32::saturating_add);
        let satisfied: usize = arms.iter().map(|arm| arm.satisfied_anchors.len()).sum();
        let declared: usize = arms
            .iter()
            .map(|arm| arm.satisfied_anchors.len() + arm.missing_anchors.len())
            .sum();
        let _ = writeln!(
            out,
            "   {:<14} {tokens:>9} tokens  {satisfied}/{declared} facts  ({})",
            kind.id(),
            kind.description()
        );
    }
    out.push('\n');
}

const CAVEATS: &str = "\
Read before quoting any number above:
  * The naive arm is handed the right directories by the fixture. A real
    agent must find them first, so its true cost is higher than shown.
  * Recall counts declared facts present in the context, not task success.
    No model ran; nothing here measures whether the change would be correct.
  * Tokens are the 4-chars-per-token estimate used throughout the workspace,
    not a tokenizer count for any specific model.
  * A low-token arm with low recall is a failure, not a saving.
";
