//! The blast-radius risk band and the traffic in the importer files of a
//! function. Both follow the Fallow Cloud runtime-context rows, so one field
//! has one meaning in every output (fallow-cloud#1642).

use std::path::Path;

use fallow_output::RuntimeCoverageRiskBand;
use rustc_hash::FxHashMap;

/// Importer files from which the band is `high`.
const HIGH_CALLERS: u32 = 20;
/// Importer files from which the band is `medium`.
const MEDIUM_CALLERS: u32 = 5;

/// The band of a function from its count of importer files. Traffic does not
/// move the band: `caller_count_weighted_by_traffic` is a separate field, so
/// a traffic peak does not change how far a change ripples.
#[must_use]
pub const fn risk_band(caller_count: u32) -> RuntimeCoverageRiskBand {
    if caller_count >= HIGH_CALLERS {
        RuntimeCoverageRiskBand::High
    } else if caller_count >= MEDIUM_CALLERS {
        RuntimeCoverageRiskBand::Medium
    } else {
        RuntimeCoverageRiskBand::Low
    }
}

/// Production runtime hits per repo-relative file, from the cloud functions
/// that matched the local AST. A file with a matched function is observed,
/// also when its hits are zero.
#[derive(Debug, Default)]
pub(super) struct FileTraffic {
    hits: FxHashMap<String, u64>,
}

impl FileTraffic {
    /// Adds the hits of one matched cloud function in `path`.
    pub(super) fn add(&mut self, path: &Path, hits: u64) {
        let total = self.hits.entry(normalized(path)).or_default();
        *total = total.saturating_add(hits);
    }

    /// `caller_count_weighted_by_traffic`: the sum of the hits in the
    /// importer files. `None` when not one importer file was observed, so a
    /// missing measurement does not read as zero traffic.
    pub(super) fn through(&self, importers: &[String]) -> Option<u64> {
        importers
            .iter()
            .filter_map(|importer| self.hits.get(importer.as_str()))
            .copied()
            .reduce(u64::saturating_add)
    }
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_follows_the_importer_count_only() {
        assert_eq!(risk_band(25), RuntimeCoverageRiskBand::High);
        assert_eq!(risk_band(20), RuntimeCoverageRiskBand::High);
        assert_eq!(risk_band(19), RuntimeCoverageRiskBand::Medium);
        assert_eq!(risk_band(5), RuntimeCoverageRiskBand::Medium);
        assert_eq!(risk_band(4), RuntimeCoverageRiskBand::Low);
        assert_eq!(risk_band(0), RuntimeCoverageRiskBand::Low);
    }

    #[test]
    fn traffic_sums_the_observed_importer_files_only() {
        let mut traffic = FileTraffic::default();
        traffic.add(Path::new("src/a.ts"), 100);
        traffic.add(Path::new("src/a.ts"), 20);
        traffic.add(Path::new("src/quiet.ts"), 0);
        let importers = |paths: &[&str]| -> Vec<String> {
            paths.iter().map(|path| (*path).to_owned()).collect()
        };

        assert_eq!(
            traffic.through(&importers(&["src/a.ts", "src/unseen.ts"])),
            Some(120)
        );
        assert_eq!(traffic.through(&importers(&["src/quiet.ts"])), Some(0));
        assert_eq!(traffic.through(&importers(&["src/unseen.ts"])), None);
        assert_eq!(traffic.through(&[]), None);
    }
}
