//! Step 4: LCP array construction using Kasai's algorithm.

/// Build the LCP (Longest Common Prefix) array using Kasai's algorithm.
///
/// `lcp[i]` is the length of the longest common prefix between suffixes
/// `sa[i]` and `sa[i-1]`. `lcp[0]` is always 0.
///
/// The LCP computation stops at sentinel boundaries (negative values in
/// `text`) to prevent matches from crossing file boundaries.
pub(super) fn build_lcp(text: &[i64], sa: &[usize]) -> Vec<usize> {
    let n = sa.len();
    if n == 0 {
        return vec![];
    }

    let mut rank = vec![0usize; n];
    for i in 0..n {
        rank[sa[i]] = i;
    }

    let mut lcp = vec![0usize; n];
    let mut k: usize = 0;

    for i in 0..n {
        if rank[i] == 0 {
            k = 0;
            continue;
        }
        let j = sa[rank[i] - 1];
        while i + k < n && j + k < n {
            if text[i + k] < 0 || text[j + k] < 0 {
                break;
            }
            if text[i + k] != text[j + k] {
                break;
            }
            k += 1;
        }
        lcp[rank[i]] = k;
        k = k.saturating_sub(1);
    }

    lcp
}
