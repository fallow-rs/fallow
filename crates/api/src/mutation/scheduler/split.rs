//! The split of a wide mutant over idle workers (issue #5422).
//!
//! A job whose covering tests are in many test files can outlast the rest of
//! the queue, and the other workers then stay idle while it runs. When the
//! runner can take the test index, the scheduler gives the job to more than
//! one worker: each part holds whole test files, and any companion can run
//! it. The parts together give one attempt with the rules of one run:
//!
//! - a kill in a part decides `killed`; `killed_by` is the first killer by
//!   test id, so the result depends neither on the part that ends first nor
//!   on the measured test durations;
//! - the mutant is `survived` only when each part ends with no failure;
//! - `duration_ms` is the wall time from the split to the end of the last
//!   part.
//!
//! The report and the cache key read the whole job, never a part.

use super::*;

/// The smallest estimated time of one part, in milliseconds, on the scale of
/// [`Job::estimate_ms`]. Each part pays
/// one request and the setup of its test files in its companion, so a
/// shorter part gains less than it costs.
const MIN_PART_MS: u64 = 1000;

/// One part of a split job.
#[derive(Debug, Clone)]
pub(super) struct Part {
    group: Arc<Split>,
    /// The smallest test id of this part.
    first: String,
}

/// The shared state of the parts of one job.
#[derive(Debug)]
pub(super) struct Split {
    job: Job,
    parts: usize,
    started: Instant,
    state: Mutex<SplitState>,
}

#[derive(Debug, Default)]
struct SplitState {
    /// The attempt of each part that ran, with the `first` of the part.
    attempts: Vec<(String, Attempt)>,
    /// The count of the parts that ran or that a kill made unnecessary.
    ended: usize,
    /// The smallest killer id so far.
    kill_at: Option<String>,
}

impl Split {
    /// True when a part whose smallest test id is `first` cannot change the
    /// result: a test with a smaller id killed the mutant. A run that
    /// collects the kill matrix needs each killing test, so it runs each
    /// part.
    fn skips(&self, first: &str, collects_evidence: bool) -> bool {
        !collects_evidence
            && self.state.lock().is_ok_and(|state| {
                state
                    .kill_at
                    .as_deref()
                    .is_some_and(|kill_at| kill_at < first)
            })
    }

    /// Ends one part. `attempt` is `None` for a part that did not run. Gives
    /// the whole job, its merged attempt and the wall time after the last
    /// part.
    fn end(&self, first: &str, attempt: Option<Attempt>, now: Instant) -> Option<Ended> {
        let mut state = self.state.lock().ok()?;
        state.ended += 1;
        if let Some(attempt) = attempt {
            if attempt.status == MutantStatus::Killed {
                let at = kill_key(first, &attempt).to_owned();
                if state.kill_at.as_ref().is_none_or(|known| at < *known) {
                    state.kill_at = Some(at);
                }
            }
            state.attempts.push((first.to_owned(), attempt));
        }
        if state.ended < self.parts {
            return None;
        }
        let attempts = std::mem::take(&mut state.attempts);
        drop(state);
        let wall = now.saturating_duration_since(self.started);
        let merged = self.merge(attempts, wall);
        Some((self.job.clone(), merged, wall))
    }

    /// The attempt of the whole job from the attempts of its parts.
    fn merge(&self, mut attempts: Vec<(String, Attempt)>, wall: Duration) -> Attempt {
        attempts.sort_by(|left, right| left.0.cmp(&right.0));
        let every_part_ran = attempts.len() == self.parts;
        let decisive = attempts
            .iter()
            .filter(|(_, attempt)| attempt.status == MutantStatus::Killed)
            .min_by_key(|(first, attempt)| kill_key(first, attempt))
            .or_else(|| {
                attempts
                    .iter()
                    .min_by_key(|(_, attempt)| status_rank(attempt.status))
            })
            .map(|(_, attempt)| attempt.clone());
        let Some(decisive) = decisive else {
            return Attempt::decided(MutantStatus::Pending, None, None);
        };
        let same_status = |attempt: &Attempt| attempt.status == decisive.status;
        let mut killing_tests: Vec<String> = attempts
            .iter()
            .flat_map(|(_, attempt)| attempt.killing_tests.iter().cloned())
            .collect();
        killing_tests.sort();
        killing_tests.dedup();
        let mut dropped_tests: Vec<String> = attempts
            .iter()
            .flat_map(|(_, attempt)| attempt.dropped_tests.iter().cloned())
            .collect();
        dropped_tests.sort();
        dropped_tests.dedup();
        Attempt {
            killing_tests,
            kill_matrix_complete: every_part_ran
                && attempts
                    .iter()
                    .all(|(_, attempt)| attempt.kill_matrix_complete),
            duration_ms: Some(u64::try_from(wall.as_millis()).unwrap_or(u64::MAX)),
            dropped_tests,
            settled: attempts
                .iter()
                .any(|(_, attempt)| same_status(attempt) && attempt.settled),
            ..decisive
        }
    }
}

/// The id that orders the kill of `attempt`: its killer, or for a kill with
/// no named test, the smallest test id of its part.
fn kill_key<'a>(first: &'a str, attempt: &'a Attempt) -> &'a str {
    attempt.killed_by.as_deref().unwrap_or(first)
}

/// The whole job, its merged attempt, and its wall time.
type Ended = (Job, Attempt, Duration);

/// The order in which the status of a part decides the job when no part
/// killed the mutant. A part that timed out or gave no result leaves the
/// mutant undecided, so it wins over a survivor. `no_coverage` needs each
/// part to agree.
const fn status_rank(status: MutantStatus) -> u8 {
    match status {
        MutantStatus::Killed => 0,
        MutantStatus::Timeout => 1,
        MutantStatus::Pending => 3,
        MutantStatus::Survived => 4,
        MutantStatus::NoCoverage => 5,
        _ => 2,
    }
}

/// The part count of a job: one part for each `share` of work, at least
/// [`MIN_PART_MS`] each, and at most one for each file and each worker. One
/// means no split. `share` is the work of each worker when the job and the
/// queue spread evenly.
fn part_count(estimate_ms: u64, share_ms: u64, files: usize, workers: usize) -> usize {
    if estimate_ms <= share_ms || estimate_ms < 2 * MIN_PART_MS {
        return 1;
    }
    let per_part = share_ms.max(MIN_PART_MS);
    let count = usize::try_from(estimate_ms.div_ceil(per_part)).unwrap_or(usize::MAX);
    count.min(files).min(workers)
}

/// Divides the tests of `job` into `count` sets of whole test files with
/// about equal dry-run time: the largest file first, each to the set with
/// the least time. Each set keeps the test order of the job. The sets are
/// in the order of their smallest test id.
fn partition(facts: &DryRunFacts, job: &Job, count: usize) -> Vec<Vec<String>> {
    let mut cost: BTreeMap<&str, f64> = BTreeMap::new();
    for test in &job.tests {
        let (file, duration) = facts
            .passing
            .get(test)
            .map_or(("", 0.0), |(file, duration)| (file.as_str(), *duration));
        let entry = cost
            .entry(file)
            .or_insert_with(|| facts.file_setup.get(file).copied().unwrap_or(0.0));
        *entry += duration;
    }
    let mut files: Vec<(&str, f64)> = cost.into_iter().collect();
    files.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(right.0)));
    let mut loads = vec![0.0_f64; count];
    let mut set_of: FxHashMap<&str, usize> = FxHashMap::default();
    for (file, file_cost) in files {
        let set = (0..count)
            .min_by(|left, right| loads[*left].total_cmp(&loads[*right]))
            .unwrap_or(0);
        loads[set] += file_cost;
        set_of.insert(file, set);
    }
    let mut sets = vec![Vec::new(); count];
    for test in &job.tests {
        let file = facts
            .passing
            .get(test)
            .map_or("", |(file, _)| file.as_str());
        sets[set_of.get(file).copied().unwrap_or(0)].push(test.clone());
    }
    sets.retain(|set| !set.is_empty());
    sets.sort_by_cached_key(|set| set.iter().min().cloned());
    sets
}

/// The sorted test files of `tests`.
pub(super) fn files_of(facts: &DryRunFacts, tests: &[String]) -> Vec<String> {
    let mut files: Vec<String> = tests
        .iter()
        .filter_map(|test| facts.passing.get(test).map(|(file, _)| file.clone()))
        .collect();
    files.sort_unstable();
    files.dedup();
    files
}

/// The job of one part. The part keeps the timeouts of the whole job: a
/// part never runs longer than the whole job, and a doubled timeout of a
/// timeout retry stays doubled.
fn part_job(facts: &DryRunFacts, job: &Job, tests: Vec<String>, part: Part) -> Job {
    let estimate = tests
        .iter()
        .filter_map(|test| facts.passing.get(test).map(|(_, duration)| *duration))
        .sum::<f64>()
        + facts.setup_of(&tests);
    Job {
        id: job.id.clone(),
        test_files: files_of(facts, &tests),
        tests,
        targets: Vec::new(),
        timeout_ms: job.timeout_ms,
        loop_budget_ms: job.loop_budget_ms,
        setup_ms: job.setup_ms,
        static_mutant: job.static_mutant,
        cache_key: None,
        confirmation: None,
        first_tests: job.first_tests.clone(),
        retry_extra_ms: 0,
        estimate_ms: scaled_ms(estimate),
        part: Some(part),
    }
}

/// True when `job` can run as parts. A static mutant splits only when the
/// runner loads fresh modules for each run: otherwise its companion reloads
/// around the run. A `--kill-matrix-for` job runs its target tests first, so
/// it does not split.
fn splittable(job: &Job, caps: Capabilities) -> bool {
    job.part.is_none()
        && (!job.static_mutant || caps.fresh_modules_per_run)
        && job.targets.is_empty()
        && job.test_files.len() > 1
}

/// The most workers that `jobs` can use: one for each job, or one for each
/// test file of a job that is long enough to split when the runner can take
/// the test index.
pub(super) fn capacity(jobs: &[Job], takes_index: bool, caps: Capabilities) -> usize {
    jobs.iter()
        .map(|job| {
            if takes_index && splittable(job, caps) && job.estimate_ms >= 2 * MIN_PART_MS {
                job.test_files.len()
            } else {
                1
            }
        })
        .fold(0, usize::saturating_add)
}

impl Phase<'_> {
    /// Gives `job`, or the first part of `job` when the job would outlast
    /// the work of the other workers. The other parts go to the front of
    /// `jobs`, the locked queue, so the next idle workers take them.
    pub(super) fn spread(&self, jobs: &mut VecDeque<Job>, job: Job) -> Job {
        let facts = self.facts;
        if self.index.is_none() || self.workers < 2 || !splittable(&job, self.caps) {
            return job;
        }
        let queued = jobs
            .iter()
            .map(|queued| queued.estimate_ms)
            .fold(job.estimate_ms, u64::saturating_add);
        let workers = u64::try_from(self.workers).unwrap_or(u64::MAX);
        let count = part_count(
            job.estimate_ms,
            queued / workers,
            job.test_files.len(),
            self.workers,
        );
        if count < 2 {
            return job;
        }
        let sets = partition(facts, &job, count);
        if sets.len() < 2 {
            return job;
        }
        let firsts: Vec<String> = sets
            .iter()
            .map(|set| set.iter().min().cloned().unwrap_or_default())
            .collect();
        let group = Arc::new(Split {
            parts: sets.len(),
            started: self.config.clock.now(),
            state: Mutex::new(SplitState::default()),
            job,
        });
        let mut parts = sets.into_iter().zip(firsts).map(|(tests, first)| {
            let part = Part {
                group: Arc::clone(&group),
                first,
            };
            part_job(facts, &group.job, tests, part)
        });
        let Some(own) = parts.next() else {
            return group.job.clone();
        };
        let rest: Vec<Job> = parts.collect();
        for part in rest.into_iter().rev() {
            jobs.push_front(part);
        }
        own
    }

    /// Runs one part of a split job. After the last part, the phase records
    /// the whole job one time, and the time reserve counts it as one job.
    pub(super) fn run_part(
        &self,
        worker: &mut Worker<'_>,
        job: &Job,
        part: &Part,
    ) -> Result<(), ScheduleError> {
        let group = &part.group;
        let attempt = if group.skips(&part.first, self.config.kill_matrix) {
            None
        } else if self.deadline.as_ref().is_some_and(PhaseDeadline::passed) {
            Some(Attempt::decided(MutantStatus::Pending, None, None))
        } else {
            Some(worker.run_with_retry(job, self.caps)?)
        };
        let now = self.config.clock.now();
        if let Some((whole, merged, wall)) = group.end(&part.first, attempt, now) {
            // A pending job did not run, the same as a pending whole job.
            if let Some(deadline) = self
                .deadline
                .as_ref()
                .filter(|_| merged.status != MutantStatus::Pending)
            {
                deadline.note(&whole, &merged, wall);
            }
            self.push(whole, merged);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TESTS: [&str; 4] = ["a::t", "b::t", "c::t", "d::t"];

    fn whole_job() -> Job {
        Job {
            id: "m1".to_owned(),
            tests: TESTS.map(str::to_owned).to_vec(),
            targets: Vec::new(),
            timeout_ms: 20_000,
            loop_budget_ms: 15_500,
            setup_ms: 0,
            static_mutant: false,
            test_files: vec!["a".to_owned(), "b".to_owned()],
            cache_key: Some("key".to_owned()),
            confirmation: None,
            first_tests: Vec::new(),
            retry_extra_ms: 0,
            estimate_ms: 10_000,
            part: None,
        }
    }

    fn split_of(parts: usize) -> (Split, Instant) {
        let started = Instant::now();
        let split = Split {
            job: whole_job(),
            parts,
            started,
            state: Mutex::new(SplitState::default()),
        };
        (split, started)
    }

    fn killed(test: &str) -> Attempt {
        Attempt {
            killing_tests: vec![test.to_owned()],
            kill_matrix_complete: true,
            ..Attempt::decided(MutantStatus::Killed, Some(test.to_owned()), Some(5))
        }
    }

    fn of(status: MutantStatus) -> Attempt {
        Attempt::decided(status, None, Some(5))
    }

    #[test]
    fn the_job_ends_one_time_after_its_last_part() {
        let (split, started) = split_of(2);
        let later = started + Duration::from_millis(750);
        assert!(
            split
                .end("a::t", Some(of(MutantStatus::Survived)), later)
                .is_none()
        );
        let (job, merged, wall) = split
            .end("c::t", Some(of(MutantStatus::Survived)), later)
            .expect("the last part ends the job");
        assert_eq!(job.tests, TESTS.map(str::to_owned).to_vec());
        assert_eq!(job.cache_key.as_deref(), Some("key"));
        assert_eq!(merged.status, MutantStatus::Survived);
        assert_eq!(merged.duration_ms, Some(750), "the wall time of the job");
        assert_eq!(wall, Duration::from_millis(750));
    }

    #[test]
    fn the_first_killer_in_the_test_order_decides_whatever_part_ends_first() {
        let (split, started) = split_of(3);
        assert!(split.end("d::t", Some(killed("d::t")), started).is_none());
        let passed = Attempt {
            kill_matrix_complete: true,
            ..of(MutantStatus::Survived)
        };
        assert!(split.end("a::t", Some(passed), started).is_none());
        let (_, merged, _) = split
            .end("b::t", Some(killed("b::t")), started)
            .expect("ended");
        assert_eq!(merged.status, MutantStatus::Killed);
        assert_eq!(merged.killed_by.as_deref(), Some("b::t"));
        assert_eq!(
            merged.killing_tests,
            vec!["b::t".to_owned(), "d::t".to_owned()]
        );
        assert!(merged.kill_matrix_complete);
    }

    #[test]
    fn a_part_after_a_known_kill_is_skipped_unless_the_run_collects_evidence() {
        let (split, started) = split_of(2);
        assert!(split.end("b::t", Some(killed("c::t")), started).is_none());
        assert!(
            !split.skips("a::t", false),
            "an earlier part can name an earlier killer"
        );
        assert!(split.skips("d::t", false));
        assert!(
            !split.skips("d::t", true),
            "the kill matrix needs each part"
        );
        let (_, merged, _) = split.end("d::t", None, started).expect("ended");
        assert_eq!(merged.killed_by.as_deref(), Some("c::t"));
        assert!(
            !merged.kill_matrix_complete,
            "a skipped part leaves the evidence incomplete"
        );
    }

    #[test]
    fn a_survivor_needs_each_part_to_pass() {
        for (other, expected) in [
            (MutantStatus::Timeout, MutantStatus::Timeout),
            (MutantStatus::RuntimeError, MutantStatus::RuntimeError),
            (MutantStatus::Pending, MutantStatus::Pending),
            (MutantStatus::NoCoverage, MutantStatus::Survived),
            (MutantStatus::Survived, MutantStatus::Survived),
        ] {
            let (split, started) = split_of(2);
            assert!(
                split
                    .end("a::t", Some(of(MutantStatus::Survived)), started)
                    .is_none()
            );
            let (_, merged, _) = split.end("c::t", Some(of(other)), started).expect("ended");
            assert_eq!(merged.status, expected, "survived and {other:?}");
        }
        let (split, started) = split_of(2);
        assert!(
            split
                .end("a::t", Some(of(MutantStatus::NoCoverage)), started)
                .is_none()
        );
        let (_, merged, _) = split
            .end("c::t", Some(of(MutantStatus::NoCoverage)), started)
            .expect("ended");
        assert_eq!(merged.status, MutantStatus::NoCoverage);
    }

    #[test]
    fn a_job_that_outlasts_its_share_splits_up_to_the_file_and_worker_count() {
        assert_eq!(part_count(40_000, 8_000, 40, 5), 5);
        assert_eq!(part_count(40_000, 8_000, 3, 5), 3);
        assert_eq!(part_count(70_000, 20_000, 204, 5), 4);
    }

    #[test]
    fn a_short_job_or_a_job_inside_its_share_does_not_split() {
        assert_eq!(part_count(1_500, 300, 4, 4), 1);
        assert_eq!(part_count(5_000, 5_000, 4, 4), 1);
        assert_eq!(part_count(5_000, 100, 4, 4), 4);
        assert_eq!(part_count(2_500, 100, 8, 8), 3);
    }
}
