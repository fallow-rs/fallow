/// Build a suffix array for the concatenated token stream.
///
/// Returns `sa` where `sa[i]` is the starting position of the i-th
/// lexicographically smallest suffix in `text`. Suffixes that are a prefix of
/// a longer suffix sort first (equivalent to appending a unique smallest
/// terminator), matching the convention used by the downstream LCP and clone
/// extraction passes.
///
/// Implemented with the linear-time SA-IS (suffix array by induced sorting)
/// algorithm. The input alphabet (token ranks plus per-file negative
/// sentinels) is remapped to a dense `0..=k` range and a virtual smallest
/// terminator is appended; the terminator's position is dropped from the
/// returned array.
pub(super) fn build_suffix_array(text: &[i64]) -> Vec<usize> {
    let n = text.len();
    if n == 0 {
        return vec![];
    }

    // Remap the i64 alphabet to a dense, non-negative range and reserve symbol
    // `0` for the appended terminator (shift every real symbol up by one).
    let min_val = text.iter().copied().min().unwrap_or(0);
    let max_val = text.iter().copied().max().unwrap_or(0);
    debug_assert!(max_val >= min_val);
    // Real symbols occupy `1..=(max_val - min_val + 1)`; `0` is the terminator.
    let alphabet = (max_val - min_val) as usize + 2;

    let mut s: Vec<usize> = Vec::with_capacity(n + 1);
    s.extend(text.iter().map(|&v| (v - min_val) as usize + 1));
    s.push(0); // unique smallest terminator

    let sa_full = sais(&s, alphabet);
    debug_assert_eq!(sa_full[0], n, "terminator must sort first");

    // Drop the terminator's position (always sa_full[0]); the remainder is a
    // permutation of `0..n`.
    sa_full[1..].to_vec()
}

const SA_EMPTY: usize = usize::MAX;

/// Linear-time suffix array construction via induced sorting (SA-IS).
///
/// `s` must be a sequence over the alphabet `0..alphabet` whose final element
/// is the unique smallest symbol (a terminator). Returns the suffix array of
/// `s` (length `s.len()`).
fn sais(s: &[usize], alphabet: usize) -> Vec<usize> {
    let n = s.len();
    let mut sa = vec![SA_EMPTY; n];
    if n == 0 {
        return sa;
    }
    if n == 1 {
        sa[0] = 0;
        return sa;
    }

    let is_s = classify_types(s);

    // Symbol counts and bucket boundaries (start = first index of a bucket,
    // end = one past the last index of a bucket).
    let mut counts = vec![0usize; alphabet];
    for &c in s {
        counts[c] += 1;
    }
    let bucket_starts = bucket_bounds(&counts, false);
    let bucket_ends = bucket_bounds(&counts, true);

    // --- Stage 1: sort the LMS substrings via a first induced-sort pass. ---
    let lms_positions: Vec<usize> = (1..n).filter(|&i| is_lms(&is_s, i)).collect();
    let mut tails = bucket_ends.clone();
    for &pos in &lms_positions {
        let c = s[pos];
        tails[c] -= 1;
        sa[tails[c]] = pos;
    }
    induce_l_type(s, &mut sa, &is_s, &bucket_starts);
    induce_s_type(s, &mut sa, &is_s, &bucket_ends);

    // Collect LMS suffixes in their now-sorted order and assign names by
    // comparing adjacent LMS substrings.
    let mut names = vec![SA_EMPTY; n];
    let mut next_name = 0usize;
    let mut prev: Option<usize> = None;
    for &pos in &sa {
        if pos == SA_EMPTY || !is_lms(&is_s, pos) {
            continue;
        }
        if prev.is_some_and(|p| !lms_substrings_equal(s, &is_s, p, pos)) {
            next_name += 1;
        }
        names[pos] = next_name;
        prev = Some(pos);
    }
    let name_count = next_name + 1;

    // Reduced problem: one symbol (its name) per LMS suffix, in original order.
    let s1: Vec<usize> = lms_positions.iter().map(|&p| names[p]).collect();

    let sa1 = if name_count == s1.len() {
        // All names distinct: the suffix array is the inverse permutation.
        let mut inv = vec![0usize; s1.len()];
        for (i, &name) in s1.iter().enumerate() {
            inv[name] = i;
        }
        inv
    } else {
        sais(&s1, name_count)
    };

    // --- Stage 3: induce the final suffix array from the sorted LMS order. ---
    sa.fill(SA_EMPTY);
    let mut tails = bucket_ends.clone();
    for &idx in sa1.iter().rev() {
        let pos = lms_positions[idx];
        let c = s[pos];
        tails[c] -= 1;
        sa[tails[c]] = pos;
    }
    induce_l_type(s, &mut sa, &is_s, &bucket_starts);
    induce_s_type(s, &mut sa, &is_s, &bucket_ends);

    sa
}

/// Classify each position as S-type (`true`) or L-type (`false`). The final
/// position (the terminator) is S-type by definition.
fn classify_types(s: &[usize]) -> Vec<bool> {
    let n = s.len();
    let mut is_s = vec![false; n];
    is_s[n - 1] = true;
    for i in (0..n - 1).rev() {
        is_s[i] = s[i] < s[i + 1] || (s[i] == s[i + 1] && is_s[i + 1]);
    }
    is_s
}

/// An LMS (left-most S-type) position is an S-type position preceded by an
/// L-type position.
#[inline]
fn is_lms(is_s: &[bool], i: usize) -> bool {
    i > 0 && is_s[i] && !is_s[i - 1]
}

/// Compute bucket boundaries from per-symbol counts. With `end == true`, entry
/// `c` is one past the last index of symbol `c`'s bucket; otherwise it is the
/// first index.
fn bucket_bounds(counts: &[usize], end: bool) -> Vec<usize> {
    let mut bounds = vec![0usize; counts.len()];
    let mut sum = 0usize;
    for (c, &count) in counts.iter().enumerate() {
        sum += count;
        bounds[c] = if end { sum } else { sum - count };
    }
    bounds
}

/// Induce-sort L-type suffixes left-to-right into the front of their buckets.
fn induce_l_type(s: &[usize], sa: &mut [usize], is_s: &[bool], bucket_starts: &[usize]) {
    let mut heads = bucket_starts.to_vec();
    for i in 0..sa.len() {
        let p = sa[i];
        if p != SA_EMPTY && p > 0 {
            let j = p - 1;
            if !is_s[j] {
                let c = s[j];
                sa[heads[c]] = j;
                heads[c] += 1;
            }
        }
    }
}

/// Induce-sort S-type suffixes right-to-left into the back of their buckets.
fn induce_s_type(s: &[usize], sa: &mut [usize], is_s: &[bool], bucket_ends: &[usize]) {
    let mut tails = bucket_ends.to_vec();
    for i in (0..sa.len()).rev() {
        let p = sa[i];
        if p != SA_EMPTY && p > 0 {
            let j = p - 1;
            if is_s[j] {
                let c = s[j];
                tails[c] -= 1;
                sa[tails[c]] = j;
            }
        }
    }
}

/// Compare the LMS substrings starting at `lhs` and `rhs` for equality. An LMS
/// substring runs from one LMS position up to and including the next.
fn lms_substrings_equal(s: &[usize], is_s: &[bool], lhs: usize, rhs: usize) -> bool {
    let len = s.len();
    let mut offset = 0usize;
    loop {
        let li = lhs + offset;
        let ri = rhs + offset;
        if li >= len || ri >= len {
            return li >= len && ri >= len;
        }
        if s[li] != s[ri] || is_s[li] != is_s[ri] {
            return false;
        }
        let lhs_lms = offset > 0 && is_lms(is_s, li);
        let rhs_lms = offset > 0 && is_lms(is_s, ri);
        if lhs_lms || rhs_lms {
            return lhs_lms && rhs_lms;
        }
        offset += 1;
    }
}

/// Build a suffix array using the O(N log N) prefix-doubling algorithm with
/// radix sort. Retained as a reference implementation for differential tests
/// against [`build_suffix_array`].
#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "ranks are bounded by text length which fits in usize"
)]
pub(super) fn build_suffix_array_doubling(text: &[i64]) -> Vec<usize> {
    let n = text.len();
    if n == 0 {
        return vec![];
    }

    let min_val = text.iter().copied().min().unwrap_or(0);
    let mut rank: Vec<i64> = text.iter().map(|&v| v - min_val).collect();
    let mut sa: Vec<usize> = (0..n).collect();
    let mut tmp: Vec<i64> = vec![0; n];
    let mut k: usize = 1;
    let mut iterations = 0u32;

    let mut sa_tmp: Vec<usize> = vec![0; n];
    let mut counts: Vec<usize> = Vec::new();

    let mut max_rank = rank.iter().copied().max().unwrap_or(0) as usize;

    while k < n {
        iterations += 1;

        let bucket_count = max_rank + 2; // ranks 0..=max_rank plus -1 mapped to 0

        counts.clear();
        counts.resize(bucket_count + 1, 0);
        for &i in &sa {
            let r2 = if i + k < n {
                rank[i + k] as usize + 1
            } else {
                0
            };
            counts[r2] += 1;
        }
        let mut sum = 0;
        for c in &mut counts {
            let v = *c;
            *c = sum;
            sum += v;
        }
        for &i in &sa {
            let r2 = if i + k < n {
                rank[i + k] as usize + 1
            } else {
                0
            };
            sa_tmp[counts[r2]] = i;
            counts[r2] += 1;
        }

        counts.fill(0);
        counts.resize(bucket_count + 1, 0);
        for &i in &sa_tmp {
            let r1 = rank[i] as usize;
            counts[r1] += 1;
        }
        sum = 0;
        for c in &mut counts {
            let v = *c;
            *c = sum;
            sum += v;
        }
        for &i in &sa_tmp {
            let r1 = rank[i] as usize;
            sa[counts[r1]] = i;
            counts[r1] += 1;
        }

        tmp[sa[0]] = 0;
        for i in 1..n {
            let prev = sa[i - 1];
            let curr = sa[i];
            let same = rank[prev] == rank[curr] && {
                let rp2 = if prev + k < n { rank[prev + k] } else { -1 };
                let rc2 = if curr + k < n { rank[curr + k] } else { -1 };
                rp2 == rc2
            };
            tmp[curr] = tmp[prev] + i64::from(!same);
        }

        let new_max_rank = tmp[sa[n - 1]];
        std::mem::swap(&mut rank, &mut tmp);

        if new_max_rank as usize == n - 1 {
            break;
        }

        max_rank = new_max_rank as usize;
        k *= 2;
    }

    tracing::trace!(n, iterations, "suffix array constructed");
    sa
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_suffix_order(text: &[i64], sa: &[usize]) {
        assert_eq!(
            text.len(),
            sa.len(),
            "suffix array length must equal text length"
        );
        for i in 1..sa.len() {
            let suffix_a = &text[sa[i - 1]..];
            let suffix_b = &text[sa[i]..];
            assert!(
                suffix_a <= suffix_b,
                "suffix order violated at SA[{}]={} vs SA[{}]={}: {:?} > {:?}",
                i - 1,
                sa[i - 1],
                i,
                sa[i],
                suffix_a,
                suffix_b,
            );
        }
    }

    fn assert_is_permutation(sa: &[usize], n: usize) {
        let mut seen = vec![false; n];
        for &idx in sa {
            assert!(idx < n, "suffix array index {idx} out of bounds (n={n})");
            assert!(!seen[idx], "duplicate index {idx} in suffix array");
            seen[idx] = true;
        }
    }

    #[test]
    fn empty_input() {
        let sa = build_suffix_array(&[]);
        assert!(sa.is_empty());
    }

    #[test]
    fn single_element() {
        let text = [42];
        let sa = build_suffix_array(&text);
        assert_eq!(sa, vec![0]);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn two_elements_already_sorted() {
        let text = [1, 2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 2);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn two_elements_reverse_sorted() {
        let text = [2, 1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 2);
        assert_suffix_order(&text, &sa);
        assert_eq!(sa[0], 1);
        assert_eq!(sa[1], 0);
    }

    #[test]
    fn already_sorted_input() {
        let text = [1, 2, 3, 4, 5];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 5);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn reverse_sorted_input() {
        let text = [5, 4, 3, 2, 1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 5);
        assert_suffix_order(&text, &sa);
        assert_eq!(sa[0], 4);
    }

    #[test]
    fn all_identical_elements() {
        let text = [7, 7, 7, 7];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
        assert_eq!(sa, vec![3, 2, 1, 0]);
    }

    #[test]
    fn mixed_input_banana_like() {
        let text = [2, 1, 3, 1, 3, 1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 6);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn input_with_negative_sentinels() {
        let text = [3, 1, 2, -1, 4, 5, -2, 6];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 8);
        assert_suffix_order(&text, &sa);
        assert_eq!(sa[0], 6);
    }

    #[test]
    fn single_sentinel_only() {
        let text = [-1];
        let sa = build_suffix_array(&text);
        assert_eq!(sa, vec![0]);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn multiple_sentinels_decreasing() {
        let text = [-1, -2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 2);
        assert_suffix_order(&text, &sa);
        assert_eq!(sa[0], 1);
        assert_eq!(sa[1], 0);
    }

    #[test]
    fn realistic_concatenated_files() {
        let text = [10, 20, 30, -1, 20, 30, 40];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 7);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn repeated_pattern() {
        let text = [1, 2, 1, 2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn large_input_stress() {
        let text: Vec<i64> = (0..256).map(|i| i64::from(i % 17)).collect();
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 256);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn large_identical_stress() {
        let text = vec![42i64; 128];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 128);
        assert_suffix_order(&text, &sa);
        for (i, &pos) in sa.iter().enumerate() {
            assert_eq!(pos, 127 - i);
        }
    }

    #[test]
    fn alternating_sentinels_and_tokens() {
        let text = [5, -1, 5, -2];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
    }

    #[test]
    fn all_same_with_trailing_sentinel() {
        let text = [3, 3, 3, -1];
        let sa = build_suffix_array(&text);
        assert_is_permutation(&sa, 4);
        assert_suffix_order(&text, &sa);
        assert_eq!(sa[0], 3);
    }

    #[test]
    fn suffix_array_is_inverse_of_rank() {
        let text = [4, 2, 3, 1, 5];
        let sa = build_suffix_array(&text);
        let n = text.len();
        let mut rank = vec![0usize; n];
        for i in 0..n {
            rank[sa[i]] = i;
        }
        for i in 0..n {
            assert_eq!(
                sa[rank[i]], i,
                "rank/sa inverse property violated at position {i}"
            );
        }
    }

    /// Reference suffix array: sort every starting position by its suffix
    /// slice. Shorter suffixes (prefixes of longer ones) sort first, matching
    /// the smallest-terminator convention.
    fn naive_suffix_array(text: &[i64]) -> Vec<usize> {
        let n = text.len();
        let mut sa: Vec<usize> = (0..n).collect();
        sa.sort_by(|&a, &b| text[a..].cmp(&text[b..]));
        sa
    }

    struct XorShift(u64);
    impl XorShift {
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, bound: u64) -> u64 {
            self.next_u64() % bound
        }
    }

    #[test]
    fn sais_matches_naive_on_small_exhaustive_alphabet() {
        // Tiny alphabet maximizes repeated substrings (the SA-IS recursion).
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        for _ in 0..4000 {
            let n = 1 + rng.below(40) as usize;
            let alphabet = 1 + rng.below(3); // values 0..alphabet
            let text: Vec<i64> = std::iter::repeat_with(|| rng.below(alphabet) as i64)
                .take(n)
                .collect();
            let expected = naive_suffix_array(&text);
            let got = build_suffix_array(&text);
            assert_eq!(got, expected, "SA-IS mismatch on {text:?}");
        }
    }

    #[test]
    fn sais_matches_doubling_with_negative_sentinels() {
        // Mimic concatenate_with_sentinels: blocks of non-negative ranks
        // separated by distinct decreasing negative sentinels.
        let mut rng = XorShift(0x1234_5678_9ABC_DEF0);
        for _ in 0..2000 {
            let file_count = 1 + rng.below(6) as usize;
            let mut text: Vec<i64> = Vec::new();
            let mut sentinel: i64 = -1;
            for f in 0..file_count {
                let len = rng.below(12) as usize;
                for _ in 0..len {
                    text.push(rng.below(5) as i64);
                }
                if f + 1 < file_count {
                    text.push(sentinel);
                    sentinel -= 1;
                }
            }
            if text.is_empty() {
                continue;
            }
            let expected = build_suffix_array_doubling(&text);
            let got = build_suffix_array(&text);
            assert_eq!(got, expected, "SA-IS vs doubling mismatch on {text:?}");
            assert_suffix_order(&text, &got);
        }
    }

    #[test]
    fn sais_matches_doubling_on_large_repetitive_input() {
        // A larger input with long repeats, exercising deeper recursion.
        let mut rng = XorShift(0xDEAD_BEEF_CAFE_F00D);
        let block: Vec<i64> = std::iter::repeat_with(|| rng.below(8) as i64)
            .take(50)
            .collect();
        let mut text: Vec<i64> = Vec::new();
        for r in 0..40 {
            text.extend_from_slice(&block);
            text.push(-(r + 1)); // distinct sentinel
        }
        let expected = build_suffix_array_doubling(&text);
        let got = build_suffix_array(&text);
        assert_eq!(got, expected);
        assert_is_permutation(&got, text.len());
        assert_suffix_order(&text, &got);
    }
}
