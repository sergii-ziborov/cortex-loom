//! Deterministic arm ordering for repeated competitive measurements.

/// Natural order, reverse order, then recorded cyclic rotations.
#[must_use]
pub fn alternating_orders<T: Clone>(arms: &[T], trials: usize) -> Vec<Vec<T>> {
    if arms.is_empty() {
        return vec![Vec::new(); trials];
    }
    (0..trials)
        .map(|trial| match trial {
            0 => arms.to_vec(),
            _ if arms.len() == 2 && trial % 2 == 0 => arms.to_vec(),
            1 => arms.iter().rev().cloned().collect(),
            _ if arms.len() == 2 => arms.iter().rev().cloned().collect(),
            _ => {
                let mut order = arms.to_vec();
                let offset = (arms.len() / 2 + trial - 2) % arms.len();
                order.rotate_left(offset);
                order
            }
        })
        .collect()
}
