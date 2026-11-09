//! Test hook that permutes discovery lists with a seed.
//!
//! The parallel source walk collects paths in thread order, and workspace
//! discovery collects packages in the order of the declared globs. The output
//! must not depend on either order. The permutation oracle in
//! `crates/cli/tests/integration/permutation/` runs one project with and
//! without a seed and compares the output bytes.
//!
//! The hook works only when the `permutation-hook` feature is on and
//! `FALLOW_PERMUTATION_SEED` holds a whole number. Release builds do not
//! have the feature, so [`permute`] does nothing there.

/// The environment variable that holds the permutation seed.
pub const PERMUTATION_SEED_ENV: &str = "FALLOW_PERMUTATION_SEED";

/// One list that the hook can permute. Each site gets its own stream, so two
/// lists of the same length get different orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermutationSite {
    /// Source files that the parallel walk collected, before the sort.
    Files = 1,
    /// Config candidates that the parallel walk collected, before the sort.
    Configs = 2,
    /// Hidden directories that the parallel walk skipped, before the sort.
    Dotdirs = 3,
    /// Workspaces that the workspace discovery collected.
    Workspaces = 4,
}

/// Permute `items` when the hook is on. Otherwise do nothing.
#[cfg_attr(
    not(feature = "permutation-hook"),
    expect(unused_variables, reason = "no-op without the feature")
)]
#[inline]
pub fn permute<T>(items: &mut [T], site: PermutationSite) {
    #[cfg(feature = "permutation-hook")]
    if let Some(seed) = seed_from_env() {
        shuffle(items, seed ^ ((site as u64) << 56));
    }
}

/// Permute `items` when the hook is on, then sort them with `compare`.
///
/// The parallel walk collects its lists in thread order. The permutation
/// stands in for another thread schedule, and the sort must make that order
/// irrelevant. The oracle fails when a later change weakens the sort.
#[inline]
pub fn sort_permuted<T>(
    items: &mut [T],
    site: PermutationSite,
    compare: impl FnMut(&T, &T) -> std::cmp::Ordering,
) {
    permute(items, site);
    items.sort_unstable_by(compare);
}

#[cfg(feature = "permutation-hook")]
fn seed_from_env() -> Option<u64> {
    std::env::var(PERMUTATION_SEED_ENV)
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Fisher-Yates shuffle with a SplitMix64 stream. The stream is fixed for a
/// seed, so a failing seed gives the same order again.
#[cfg(any(feature = "permutation-hook", test))]
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for upper in (1..items.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let pick = usize::try_from(z % (upper as u64 + 1)).unwrap_or(upper);
        items.swap(upper, pick);
    }
}

#[cfg(test)]
mod tests {
    use super::shuffle;

    #[test]
    fn shuffle_is_a_fixed_permutation_per_seed() {
        let original: Vec<u32> = (0..32).collect();
        let mut first = original.clone();
        let mut second = original.clone();
        shuffle(&mut first, 7);
        shuffle(&mut second, 7);
        assert_eq!(first, second);
        assert_ne!(first, original);
        let mut sorted = first;
        sorted.sort_unstable();
        assert_eq!(sorted, original);
    }
}
