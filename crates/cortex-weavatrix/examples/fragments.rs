//! Print the per-fragment cost of a targeted evidence plan.
//!
//! Built before the digest cache, to check the assumption behind it: that the
//! revision-stable structural fragments are the expensive part. Measuring
//! first is cheaper than caching the wrong thing.

use std::path::Path;

use cortex_weavatrix::{WeavatrixAdapter, WeavatrixConfig};

fn main() {
    let repository = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    let task = std::env::args().nth(2).unwrap_or_else(|| {
        "Change bounded retry so a target that reached its `maxAttempts` resolves the run"
            .to_owned()
    });
    let adapter = WeavatrixAdapter::new(WeavatrixConfig::discover().expect("weavatrix config"));
    let bundle = adapter
        .prepare_targeted_context(Path::new(&repository), &task, Some("apply_command"), 4_000)
        .expect("evidence");

    let mut total = 0usize;
    println!("{:<18} {:>8} {:>8}  kind", "fragment", "chars", "~tokens");
    for fragment in &bundle.evidence {
        let chars = fragment.content.chars().count();
        total += chars;
        println!(
            "{:<18} {chars:>8} {:>8}  {:?}",
            fragment.id,
            chars.div_ceil(4),
            fragment.kind
        );
    }
    println!("{:<18} {total:>8} {:>8}", "TOTAL", total.div_ceil(4));
    // An operation that failed is a warning, not a missing row: without this
    // a broken call reads as "cheap".
    for warning in &bundle.warnings {
        println!("warning: {warning}");
    }
}
