use std::sync::{Arc, Mutex};

/// Which entries of one config list had an effect in the latest pass.
///
/// The stage that decides the effect (source discovery for `ignorePatterns`,
/// entry point discovery for `entry`) runs before the dead-code envelope is
/// built and does not return its answer to that point. It writes the answer
/// here, and the envelope reads it through the resolved config.
///
/// Each pass replaces the whole answer. It never clears it first, so two
/// passes over the same config that run at the same time cannot leave a
/// half-written answer.
#[derive(Debug, Clone, Default)]
pub struct EntryEffectRecord {
    effective: Arc<Mutex<Option<Vec<bool>>>>,
}

impl EntryEffectRecord {
    /// Store the answer of one pass: one flag per config entry, in config
    /// order, `true` when the entry had an effect.
    pub fn record(&self, effective: Vec<bool>) {
        if let Ok(mut slot) = self.effective.lock() {
            *slot = Some(effective);
        }
    }

    /// The entries of `entries` that had no effect in the latest pass, in
    /// config order. Empty when no pass recorded an answer, or when the
    /// answer has another length than `entries`.
    #[must_use]
    pub fn ineffective<'a>(&self, entries: &'a [String]) -> Vec<&'a str> {
        let Ok(slot) = self.effective.lock() else {
            return Vec::new();
        };
        let Some(effective) = slot.as_ref() else {
            return Vec::new();
        };
        if effective.len() != entries.len() {
            return Vec::new();
        }
        entries
            .iter()
            .zip(effective)
            .filter(|(_, effective)| !**effective)
            .map(|(entry, _)| entry.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn no_answer_reports_nothing() {
        let record = EntryEffectRecord::default();
        assert!(record.ineffective(&entries(&["a"])).is_empty());
    }

    #[test]
    fn the_latest_answer_wins_and_clones_share_it() {
        let record = EntryEffectRecord::default();
        let clone = record.clone();
        let list = entries(&["a", "b", "c"]);
        clone.record(vec![true, false, false]);
        assert_eq!(record.ineffective(&list), vec!["b", "c"]);
        record.record(vec![true, true, false]);
        assert_eq!(clone.ineffective(&list), vec!["c"]);
    }

    #[test]
    fn an_answer_of_another_length_reports_nothing() {
        let record = EntryEffectRecord::default();
        record.record(vec![false]);
        assert!(record.ineffective(&entries(&["a", "b"])).is_empty());
    }
}
