//! The match of a saved health baseline against the current findings.
//!
//! A saved slot covers one current finding of the same or a lower severity
//! in the same bucket and dimension. The filter, the staleness counts and
//! the prune read one result of this match.

/// The result of the greedy match for one bucket and one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SeveritySlotMatch {
    /// Saved slots per severity that a current finding uses.
    pub(super) used: [usize; 3],
    /// `true` for a severity that has more current findings than
    /// compatible slots.
    pub(super) overflow: [bool; 3],
}

/// Match current findings to saved slots, lowest severity first. A finding
/// takes the lowest free slot of its own or a higher severity.
pub(super) fn match_severity_slots(current: [usize; 3], saved: [usize; 3]) -> SeveritySlotMatch {
    let mut available = saved;
    let mut overflow = [false; 3];
    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        overflow[severity_idx] = compatible < current[severity_idx];
        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }
    SeveritySlotMatch {
        used: [
            saved[0] - available[0],
            saved[1] - available[1],
            saved[2] - available[2],
        ],
        overflow,
    }
}

#[cfg(test)]
#[path = "health_match_tests.rs"]
mod tests;
