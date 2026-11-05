//! Warm equals cold over edit sequences of the result cache.
//!
//! The model of a warm run is the `--no-cache` run on the same tree. After
//! each step, the mutant statuses of the warm run must equal those of the
//! cache-free run.
//!
//! Each step runs the real runner twice, so PR CI runs the regression
//! sequences and [`PR_SEEDS`] only. Set
//! `FALLOW_MUTATION_CACHE_MODEL_SEEDS=<count>` and run the ignored sweep
//! test for a wide search. `FALLOW_MUTATION_CACHE_MODEL_SEED_START` sets its
//! first seed number. `FALLOW_MUTATION_CACHE_MODEL_REGRESSIONS=<text>` runs
//! only the regression sequences whose name contains the text. A failing
//! sequence prints its runner and steps. Add it to [`REGRESSIONS`] when you
//! fix the cause.

use super::project::{Project, available_runners, vitest_package};
use super::template::{Runner, Step, apply};

/// The seeds of the generated sequences in PR CI. One starts on Vitest and
/// one on Bun.
const PR_SEEDS: [u64; 2] = [0x5eed_0000, 0x5eed_0002];
/// The steps in one generated sequence.
const SEQUENCE_LEN: usize = 4;

/// A named sequence that once found, or was written for, a cache-key gap.
struct Regression {
    name: &'static str,
    runner: Runner,
    steps: &'static [Step],
}

const REGRESSIONS: &[Regression] = &[
    Regression {
        name: "vitest setup file",
        runner: Runner::Vitest,
        steps: &[Step::ToggleSetupMode],
    },
    Regression {
        name: "vitest config without the setup file",
        runner: Runner::Vitest,
        steps: &[Step::ToggleSetupListed],
    },
    Regression {
        name: "module that the test imports",
        runner: Runner::Vitest,
        steps: &[Step::ToggleHelperExact],
    },
    Regression {
        name: "package install with a lockfile change",
        runner: Runner::Vitest,
        steps: &[Step::ReinstallPackage],
    },
    Regression {
        name: "bun preload file (#3473)",
        runner: Runner::Bun,
        steps: &[Step::TogglePreloadMode],
    },
    Regression {
        name: "bunfig.toml without the preload file (#3473)",
        runner: Runner::Bun,
        steps: &[Step::TogglePreloadListed],
    },
    Regression {
        name: "bun preload file without @types/bun",
        runner: Runner::Bun,
        steps: &[Step::ToggleBunTypes, Step::TogglePreloadMode],
    },
    Regression {
        name: "runner switch (#3473)",
        runner: Runner::Vitest,
        steps: &[Step::ToggleSetupMode, Step::SwitchRunner],
    },
];

/// Runs one sequence. Returns the first step where the warm run differs from
/// the cache-free run.
fn run_sequence(
    vitest: &std::path::Path,
    label: &str,
    runner: Runner,
    steps: &[Step],
) -> Result<(), String> {
    let project = Project::new(runner, vitest);
    let mut done: Vec<Step> = Vec::new();
    for step in std::iter::once(None).chain(steps.iter().copied().map(Some)) {
        if let Some(step) = step {
            apply(&project.root, step);
            done.push(step);
        }
        let warm = project.run(true);
        let cold = project.run(false);
        if warm.observation != cold.observation {
            return Err(format!(
                "{label}: the warm run differs from the cache-free run\n  runner: {runner:?}\n  steps: {done:?}\n  warm: {:?}\n  cold: {:?}",
                warm.observation, cold.observation
            ));
        }
    }
    Ok(())
}

/// A xorshift generator, so a seed gives the same sequence on every platform.
struct SeqRng(u64);

impl SeqRng {
    /// Mixes `seed` with splitmix64 first. Raw xorshift keeps the low bits
    /// of a small seed, so near seeds would give the same first picks.
    fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Self((z ^ (z >> 31)).max(1))
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let index = usize::try_from((self.next() >> 32) % items.len() as u64).expect("index fits");
        items[index]
    }
}

/// The start runner and the steps of `seed`. Without Bun, the sequence
/// stays on Vitest.
fn generate(seed: u64, runners: &[Runner]) -> (Runner, Vec<Step>) {
    let mut rng = SeqRng::new(seed);
    let runner = rng.pick(runners);
    let steps: Vec<Step> = Step::ALL
        .into_iter()
        .filter(|&step| step != Step::SwitchRunner || runners.contains(&Runner::Bun))
        .collect();
    (
        runner,
        std::iter::repeat_with(|| rng.pick(&steps))
            .take(SEQUENCE_LEN)
            .collect(),
    )
}

fn run_seeds(seeds: impl IntoIterator<Item = u64>) {
    let Some(vitest) = vitest_package() else {
        return;
    };
    let runners = available_runners();
    let failures: Vec<String> = seeds
        .into_iter()
        .filter_map(|seed| {
            let (runner, steps) = generate(seed, &runners);
            run_sequence(&vitest, &format!("seed {seed:#x}"), runner, &steps).err()
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn regression_sequences_match_cache_free_runs() {
    let Some(vitest) = vitest_package() else {
        return;
    };
    let runners = available_runners();
    let filter = std::env::var("FALLOW_MUTATION_CACHE_MODEL_REGRESSIONS").unwrap_or_default();
    let selected: Vec<&Regression> = REGRESSIONS
        .iter()
        .filter(|regression| regression.name.contains(&filter))
        .collect();
    assert!(
        !selected.is_empty(),
        "no regression sequence name contains {filter:?}"
    );
    let failures: Vec<String> = selected
        .into_iter()
        .filter(|regression| {
            runners.contains(&regression.runner)
                && (runners.contains(&Runner::Bun)
                    || !regression.steps.contains(&Step::SwitchRunner))
        })
        .filter_map(|regression| {
            run_sequence(
                &vitest,
                regression.name,
                regression.runner,
                regression.steps,
            )
            .err()
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn the_generator_varies_the_start_runner_and_the_steps() {
    let both = [Runner::Vitest, Runner::Bun];
    let starts: Vec<Runner> = PR_SEEDS
        .iter()
        .map(|&seed| generate(seed, &both).0)
        .collect();
    assert!(
        both.iter().all(|runner| starts.contains(runner)),
        "the PR seeds start on each runner: {starts:?}"
    );
    let sequences: std::collections::BTreeSet<String> = (0..100)
        .map(|n| format!("{:?}", generate(0x5eed_0000 + n, &both)))
        .collect();
    assert_eq!(sequences.len(), 100, "near seeds give other sequences");
    let vitest_starts = (0..100)
        .filter(|&n| generate(0x5eed_0000 + n, &both).0 == Runner::Vitest)
        .count();
    assert!(
        (25..=75).contains(&vitest_starts),
        "a seed range starts on both runners: {vitest_starts} of 100 on Vitest"
    );
}

#[test]
fn generated_sequences_match_cache_free_runs() {
    run_seeds(PR_SEEDS);
}

/// The wide search. It is too slow for PR CI.
#[test]
#[ignore = "wide seed search; set FALLOW_MUTATION_CACHE_MODEL_SEEDS"]
fn seed_sweep_matches_cache_free_runs() {
    let number = |name: &str, default: u64| {
        std::env::var(name).map_or(default, |value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("{name} must be a number, got {value:?}"))
        })
    };
    let count = number("FALLOW_MUTATION_CACHE_MODEL_SEEDS", 20);
    let start = number("FALLOW_MUTATION_CACHE_MODEL_SEED_START", 0);
    run_seeds((start..start + count).map(|n| 0x5eed_0000 + n));
}
