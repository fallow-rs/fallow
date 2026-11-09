//! The clock of the scheduler. The time budget and the confirmation reserve
//! read the time only through [`Clock`], so a test can give the scheduler a
//! clock that does not depend on the speed of the host.

use std::fmt;
use std::time::Instant;

/// The source of the current time for the time budget.
pub trait Clock: fmt::Debug + Send + Sync {
    /// The current time.
    fn now(&self) -> Instant;
}

/// The clock of a real run: the monotonic system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}
