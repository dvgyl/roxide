/// Error in the transform factors or candidate search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NextLengthError {
    /// The factor list is empty.
    NoFactors,
    /// At least one factor is smaller than two.
    InvalidFactors,
    /// The candidate search exceeded its work limit.
    SearchLimit,
}

/// Find the least integer at or above `minimum` accepted by greedy division.
///
/// Divide repeatedly by each factor in input order. Return `None` if no
/// accepted value fits below `u64::MAX`.
pub fn nextn(minimum: u64, factors: &[u64]) -> Result<Option<u64>, NextLengthError> {
    if factors.is_empty() {
        return Err(NextLengthError::NoFactors);
    }
    if factors.iter().any(|&f| f < 2) {
        return Err(NextLengthError::InvalidFactors);
    }
    let minimum = minimum.max(1);
    if minimum == u64::MAX {
        return Ok(None);
    }
    let accepted = |mut n: u64| {
        for &factor in factors {
            if n == 1 {
                return true;
            }
            while n.is_multiple_of(factor) {
                n /= factor;
            }
        }
        n == 1
    };
    if accepted(minimum) {
        return Ok(Some(minimum));
    }

    // Enumerate products in nondecreasing factor-index order, with a bound
    // from the best accepted candidate found so far.
    let mut best = u64::MAX;
    let mut products = factors.to_vec();
    products.sort_unstable();
    products.dedup();
    for &factor in &products {
        let mut power = 1u64;
        while power < minimum {
            let Some(next) = power.checked_mul(factor) else {
                break;
            };
            power = next;
        }
        if power >= minimum && power < best && accepted(power) {
            best = power;
        }
    }
    let mut stack = vec![(1u64, 0usize)];
    let mut work = 0;
    while let Some((base, index)) = stack.last_mut() {
        let base = *base;
        let i = *index;
        let Some(&factor) = products.get(i) else {
            stack.pop();
            continue;
        };
        if factor > (best - 1) / base {
            stack.pop();
            continue;
        }
        *index += 1;
        work += 1;
        if work > 10_000_000 {
            return Err(NextLengthError::SearchLimit);
        }
        let value = base * factor;
        if value >= minimum && accepted(value) {
            best = value;
        } else {
            stack.push((value, i));
        }
    }
    Ok((best != u64::MAX).then_some(best))
}
