//! Bounded retry for a spawn whose executable is momentarily busy.
//!
//! Unix refuses to `exec` a file while any process holds it open for writing,
//! and the writer is not always the process that opened it: a fork from any
//! other thread inherits the descriptor and releases it only when that child
//! reaches its own `exec`. A process that writes an executable and runs it, or
//! that starts a binary a package manager is still writing, meets
//! `ExecutableFileBusy` until the window closes. Nothing about the command is
//! wrong and the condition clears without help, so a bounded retry is the whole
//! remedy. The blocking and Tokio spawns share the schedule below so the two
//! cannot drift apart.

use std::io;
use std::time::{Duration, Instant};

/// Total time a spawn keeps retrying an `ExecutableFileBusy` failure.
const EXECUTABLE_BUSY_BUDGET: Duration = Duration::from_secs(1);

/// First pause between `ExecutableFileBusy` retries.
const EXECUTABLE_BUSY_FIRST_BACKOFF: Duration = Duration::from_micros(200);

/// Upper bound for the doubling `ExecutableFileBusy` backoff.
const EXECUTABLE_BUSY_MAX_BACKOFF: Duration = Duration::from_millis(20);

/// Pause schedule for a spawn that keeps meeting a busy executable.
struct ExecutableBusyRetry {
    deadline: Instant,
    backoff: Duration,
}

impl ExecutableBusyRetry {
    fn start(budget: Duration) -> Self {
        Self {
            deadline: Instant::now() + budget,
            backoff: EXECUTABLE_BUSY_FIRST_BACKOFF,
        }
    }

    /// How long to wait before spawning again, or `None` when `error` is not a
    /// busy executable or the budget is spent.
    fn pause_after(&mut self, error: &io::Error) -> Option<Duration> {
        if error.kind() != io::ErrorKind::ExecutableFileBusy || Instant::now() >= self.deadline {
            return None;
        }
        let pause = self.backoff;
        self.backoff = (self.backoff * 2).min(EXECUTABLE_BUSY_MAX_BACKOFF);
        Some(pause)
    }
}

/// Spawn `command`, waiting out an executable that is momentarily busy.
///
/// Every other spawn failure is returned from the first attempt, and a spawn
/// that succeeds immediately pays nothing.
pub fn spawn_retrying_busy_executable(
    command: &mut std::process::Command,
) -> io::Result<std::process::Child> {
    retry_while_executable_busy(EXECUTABLE_BUSY_BUDGET, || command.spawn())
}

/// Spawn `command` on the Tokio runtime, waiting out an executable that is
/// momentarily busy.
///
/// The pause yields to the runtime rather than blocking the worker thread.
#[cfg(feature = "tokio")]
pub async fn spawn_tokio_retrying_busy_executable(
    command: &mut tokio::process::Command,
) -> io::Result<tokio::process::Child> {
    retry_while_executable_busy_async(EXECUTABLE_BUSY_BUDGET, || command.spawn()).await
}

/// Twin of [`retry_while_executable_busy`] whose pause yields to the runtime
/// instead of blocking the worker thread. Both drive the same schedule, so a
/// change to one cannot leave the other behind.
#[cfg(feature = "tokio")]
async fn retry_while_executable_busy_async<T>(
    budget: Duration,
    mut attempt: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut retry = ExecutableBusyRetry::start(budget);
    loop {
        let error = match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let Some(pause) = retry.pause_after(&error) else {
            return Err(error);
        };
        tokio::time::sleep(pause).await;
    }
}

/// Repeat `attempt` while it reports `ExecutableFileBusy` and `budget` has not
/// elapsed. Every other outcome is returned from the first attempt.
fn retry_while_executable_busy<T>(
    budget: Duration,
    mut attempt: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut retry = ExecutableBusyRetry::start(budget);
    loop {
        let error = match attempt() {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let Some(pause) = retry.pause_after(&error) else {
            return Err(error);
        };
        std::thread::sleep(pause);
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test setup failures should fail at the exact setup operation"
)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_executable_is_retried_until_it_is_free() {
        let mut attempts = 0;
        let result = retry_while_executable_busy(Duration::from_secs(30), || {
            attempts += 1;
            if attempts < 3 {
                return Err(io::Error::from(io::ErrorKind::ExecutableFileBusy));
            }
            Ok(())
        });

        assert!(result.is_ok(), "a target that frees itself should spawn");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn a_permanently_busy_executable_fails_after_the_budget() {
        let mut attempts = 0;
        let started = Instant::now();
        let error = retry_while_executable_busy(Duration::from_millis(20), || {
            attempts += 1;
            Err::<(), io::Error>(io::Error::from(io::ErrorKind::ExecutableFileBusy))
        })
        .expect_err("a target that stays busy keeps failing");

        assert_eq!(error.kind(), io::ErrorKind::ExecutableFileBusy);
        assert!(
            attempts > 1,
            "the budget should cover more than one attempt"
        );
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    #[test]
    fn other_spawn_failures_are_reported_without_a_retry() {
        let mut attempts = 0;
        let error = retry_while_executable_busy(Duration::from_secs(30), || {
            attempts += 1;
            Err::<(), io::Error>(io::Error::from(io::ErrorKind::NotFound))
        })
        .expect_err("a missing executable stays missing");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn the_pause_doubles_up_to_the_ceiling() {
        let busy = io::Error::from(io::ErrorKind::ExecutableFileBusy);
        let mut retry = ExecutableBusyRetry::start(Duration::from_secs(30));

        let mut pauses = Vec::new();
        for _ in 0..12 {
            pauses.push(retry.pause_after(&busy).expect("the budget is not spent"));
        }

        assert_eq!(pauses[0], EXECUTABLE_BUSY_FIRST_BACKOFF);
        assert_eq!(pauses[1], EXECUTABLE_BUSY_FIRST_BACKOFF * 2);
        assert!(pauses.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(
            *pauses.last().expect("pauses were recorded"),
            EXECUTABLE_BUSY_MAX_BACKOFF
        );
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn an_awaited_busy_executable_is_retried_until_it_is_free() {
        let mut attempts = 0;
        let result = retry_while_executable_busy_async(Duration::from_secs(30), || {
            attempts += 1;
            if attempts < 3 {
                return Err(io::Error::from(io::ErrorKind::ExecutableFileBusy));
            }
            Ok(())
        })
        .await;

        assert!(result.is_ok(), "a target that frees itself should spawn");
        assert_eq!(attempts, 3);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn an_awaited_permanently_busy_executable_fails_after_the_budget() {
        let mut attempts = 0;
        let started = Instant::now();
        let error = retry_while_executable_busy_async(Duration::from_millis(20), || {
            attempts += 1;
            Err::<(), io::Error>(io::Error::from(io::ErrorKind::ExecutableFileBusy))
        })
        .await
        .expect_err("a target that stays busy keeps failing");

        assert_eq!(error.kind(), io::ErrorKind::ExecutableFileBusy);
        assert!(
            attempts > 1,
            "the budget should cover more than one attempt"
        );
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn a_tokio_spawn_reports_a_missing_executable_without_a_retry() {
        let mut command = tokio::process::Command::new("fallow-executable-that-does-not-exist");
        let started = Instant::now();

        let error = spawn_tokio_retrying_busy_executable(&mut command)
            .await
            .expect_err("a missing executable stays missing");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(started.elapsed() < EXECUTABLE_BUSY_BUDGET);
    }
}
