//! Value enums of the `fallow report` flags.

/// Why mutation testing did not run in a CI job, for
/// `fallow report --mutation-not-run`. A closed set, so a CI script passes
/// a reason and never free text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum MutationNotRunArg {
    /// The CI event can run the code of a fork with the secrets of the base
    /// repository, so the job refused the run.
    EventRefused,
    /// The merge request comes from a fork, and the pipeline runs in the
    /// target project, so the job refused the run.
    ForkRefused,
    /// The changed scope needs a pull request diff or a base ref, and the
    /// job has neither.
    NoChangeScope,
    /// A signal stopped the run before it wrote a report.
    Interrupted,
}
