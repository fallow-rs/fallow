use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Process-wide progress reporter for analysis stages.
pub struct AnalysisProgress {
    pb: Option<ProgressBar>,
}

impl AnalysisProgress {
    /// Create a new progress reporter.
    #[expect(
        clippy::expect_used,
        reason = "progress template is hard-coded and covered by CLI smoke tests"
    )]
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        if !enabled {
            return Self { pb: None };
        }

        let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr_with_hz(30));
        pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan} fallow: {msg} ({elapsed})")
                .expect("valid progress template")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        pb.set_message("starting");
        pb.tick();
        pb.enable_steady_tick(Duration::from_millis(80));

        Self { pb: Some(pb) }
    }

    /// Update the current analysis stage.
    pub fn set_stage(&self, message: &str) {
        if let Some(pb) = &self.pb {
            pb.set_message(message.to_string());
        }
    }

    /// Finish and clear the progress spinner.
    pub fn finish(&self) {
        if let Some(pb) = &self.pb {
            pb.finish_and_clear();
        }
    }
}

impl Default for AnalysisProgress {
    fn default() -> Self {
        Self::new(false)
    }
}
