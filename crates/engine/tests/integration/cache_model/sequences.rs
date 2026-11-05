//! Warm equals cold over edit sequences.
//!
//! The model of a warm run is the cache-free run on the same tree. After each
//! step of a sequence, the warm run of each cached analysis (graph, extract
//! and dupes caches) must give the same JSON as the cache-free run.
//!
//! PR CI runs the regression sequences and a fixed set of seeds. Set
//! `FALLOW_CACHE_MODEL_SEEDS=<count>` and run the ignored sweep test for a
//! wide search. A failing sequence prints its template and steps. Add it to
//! [`REGRESSIONS`] when you fix the cause.

use super::project::{Observation, Project};
use super::templates::{BUN, DENO, NODE, Step, TEMPLATES, Template, apply};

/// The seeds of the generated sequences in PR CI.
const PR_SEEDS: [u64; 4] = [0x5eed_0001, 0x5eed_0002, 0x5eed_0003, 0x5eed_0004];
/// The steps in one generated sequence.
const SEQUENCE_LEN: usize = 6;

/// A named sequence that once found, or was written for, a cache-key gap.
struct Regression {
    name: &'static str,
    template: &'static Template,
    steps: &'static [Step],
}

const REGRESSIONS: &[Regression] = &[
    Regression {
        name: "source edit",
        template: &NODE,
        steps: &[Step::SwapUtilUse { keep_mtime: false }],
    },
    Regression {
        name: "same-length edit with the old modification time",
        template: &NODE,
        steps: &[Step::SwapUtilUse { keep_mtime: true }],
    },
    Regression {
        name: "file add and delete",
        template: &NODE,
        steps: &[Step::ToggleExtraFile, Step::ToggleExtraFile],
    },
    Regression {
        name: "file rename",
        template: &NODE,
        steps: &[Step::RenameOrphan],
    },
    Regression {
        name: "production mode",
        template: &NODE,
        steps: &[Step::ToggleProduction],
    },
    Regression {
        name: "ignore pattern",
        template: &NODE,
        steps: &[Step::ToggleIgnoreOrphan],
    },
    Regression {
        name: "resolve condition",
        template: &NODE,
        steps: &[Step::ToggleLibExports, Step::ToggleCondition],
    },
    Regression {
        name: "package.json imports (#3477)",
        template: &NODE,
        steps: &[Step::SwapPackageImports],
    },
    Regression {
        name: "workspace names (#3477)",
        template: &NODE,
        steps: &[Step::SwapWorkspaceNames],
    },
    Regression {
        name: "extended tsconfig (#3477)",
        template: &NODE,
        steps: &[Step::SwapExtendedTsconfig],
    },
    Regression {
        name: "nested tsconfig (#3477)",
        template: &NODE,
        steps: &[Step::SwapNestedTsconfig],
    },
    Regression {
        name: "deno import map (#3477)",
        template: &DENO,
        steps: &[Step::SwapDenoImportMap],
    },
    Regression {
        name: "npm install into node_modules (#4192)",
        template: &NODE,
        steps: &[Step::ToggleNpmInstall],
    },
    Regression {
        name: "bun install with the same lockfile (#4192)",
        template: &BUN,
        steps: &[Step::ToggleBunInstall],
    },
    Regression {
        name: "same-length clone edit with the old modification time",
        template: &NODE,
        steps: &[Step::EditClone { keep_mtime: true }],
    },
];

/// The facts of one sequence run.
#[derive(Debug, Default)]
struct Outcome {
    /// Steps that changed the cache-free result. A sequence where no step
    /// changes it cannot show a stale cache.
    changed: usize,
    /// Steps where the warm run used the graph cache.
    graph_hits: usize,
}

/// Run `steps` on a fresh copy of `template`. After each step, compare the
/// warm run with the cache-free run, and panic with the sequence on a
/// difference.
fn run_sequence(label: &str, template: &Template, steps: &[Step]) -> Outcome {
    let mut project = Project::new(template.files);
    let mut previous = project.observe_cold();
    let first_warm = project.observe_warm();
    assert_observations_equal(label, template, &[], &first_warm, &previous);
    assert_caches_persisted(label, &project);

    let mut outcome = Outcome::default();
    for (index, step) in steps.iter().enumerate() {
        apply(&mut project, *step);
        let before = project.graph_cache_bytes();
        let warm = project.observe_warm();
        if before.is_some() && before == project.graph_cache_bytes() {
            outcome.graph_hits += 1;
        }
        let cold = project.observe_cold();
        assert_observations_equal(label, template, &steps[..=index], &warm, &cold);
        if cold != previous {
            outcome.changed += 1;
        }
        previous = cold;
    }
    outcome
}

/// The graph, extract and dupes caches each wrote their file, so the warm
/// runs read real caches.
fn assert_caches_persisted(label: &str, project: &Project) {
    let snapshot = project.cache_snapshot();
    let names: Vec<String> = snapshot
        .iter()
        .filter_map(|(path, _)| path.strip_prefix(&project.cache_dir).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    for (cache, matches) in [
        ("graph", "graph-cache.bin"),
        ("extract", "cache.bin"),
        ("dupes", "dupes-tokens-"),
    ] {
        assert!(
            names
                .iter()
                .any(|name| name == matches || (matches.ends_with('-') && name.contains(matches))),
            "{label}: the warm run must persist the {cache} cache, found {names:?}"
        );
    }
}

fn assert_observations_equal(
    label: &str,
    template: &Template,
    steps: &[Step],
    warm: &Observation,
    cold: &Observation,
) {
    if warm == cold {
        return;
    }
    let mut report = format!(
        "{label}: the warm run differs from the cache-free run\n  template: {}\n  steps: {steps:?}\n",
        template.name
    );
    for (part, warm, cold) in [
        ("analysis", &warm.analysis, &cold.analysis),
        ("dupes", &warm.dupes, &cold.dupes),
    ] {
        describe_difference(&mut report, part, warm, cold);
    }
    panic!("{report}");
}

/// Name each top-level field that differs, with both values in short form.
fn describe_difference(
    report: &mut String,
    part: &str,
    warm: &serde_json::Value,
    cold: &serde_json::Value,
) {
    use std::fmt::Write as _;

    let (Some(warm_fields), Some(cold_fields)) = (warm.as_object(), cold.as_object()) else {
        if warm != cold {
            let _ = writeln!(report, "  {part}: {} != {}", short(warm), short(cold));
        }
        return;
    };
    for (key, cold_value) in cold_fields {
        let warm_value = warm_fields.get(key).unwrap_or(&serde_json::Value::Null);
        if warm_value != cold_value {
            let _ = writeln!(
                report,
                "  {part}.{key}:\n    warm: {}\n    cold: {}",
                short(warm_value),
                short(cold_value)
            );
        }
    }
}

fn short(value: &serde_json::Value) -> String {
    const LIMIT: usize = 600;
    let text = value.to_string();
    if text.len() <= LIMIT {
        return text;
    }
    let end = (0..=LIMIT)
        .rev()
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}...", &text[..end])
}

/// A xorshift generator, so a seed gives the same sequence on every platform.
struct SeqRng(u64);

impl SeqRng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let index = usize::try_from(self.next() % items.len() as u64).expect("index fits");
        &items[index]
    }
}

fn generate(seed: u64, template: &Template) -> Vec<Step> {
    let mut rng = SeqRng(seed | 1);
    std::iter::repeat_with(|| *rng.pick(template.steps))
        .take(SEQUENCE_LEN)
        .collect()
}

fn run_seeds(seeds: impl IntoIterator<Item = u64>) -> Outcome {
    let mut total = Outcome::default();
    for seed in seeds {
        for template in TEMPLATES {
            let steps = generate(seed, template);
            let outcome = run_sequence(&format!("seed {seed:#x}"), template, &steps);
            total.changed += outcome.changed;
            total.graph_hits += outcome.graph_hits;
        }
    }
    total
}

#[test]
fn regression_sequences_match_cache_free_runs() {
    for regression in REGRESSIONS {
        let outcome = run_sequence(regression.name, regression.template, regression.steps);
        assert!(
            outcome.changed > 0,
            "{}: no step changed the cache-free result, so the sequence proves nothing",
            regression.name
        );
    }
}

#[test]
fn generated_sequences_match_cache_free_runs() {
    let outcome = run_seeds(PR_SEEDS);
    assert!(
        outcome.changed > 0,
        "no generated step changed the cache-free result"
    );
    assert!(
        outcome.graph_hits > 0,
        "no warm run used the graph cache, so the comparison tests no cache"
    );
}

/// The wide seed sweep. Run it with
/// `FALLOW_CACHE_MODEL_SEEDS=200 cargo test -p fallow-engine --test integration
/// cache_model -- --ignored`.
#[test]
#[ignore = "wide seed sweep, run by hand or on a schedule"]
fn seed_sweep_matches_cache_free_runs() {
    let count: u64 = std::env::var("FALLOW_CACHE_MODEL_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50);
    run_seeds((1..=count).map(|n| n.wrapping_mul(0x9e37_79b9_7f4a_7c15)));
}
