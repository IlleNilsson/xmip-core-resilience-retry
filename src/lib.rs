#![forbid(unsafe_code)]

//! The retry guard — a technology of `xmip-core-resilience` (ADR-0048).
//!
//! A retryable failure below the attempt limit is tried again after a delay;
//! a permanent failure, or the last attempt, stands as it is. The delay is
//! flat unless the guard is told to back off, in which case it grows by a
//! factor each attempt up to a cap. The guard never runs the operation: it
//! counts, and it says how long to wait.

use std::time::Duration;

use resilience::{Attempt, Decision, Guard, RetryPolicy};

/// The retry guard: attempts up to a limit, a delay between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retry {
    max_attempts: u32,
    delay: Duration,
    factor: u32,
    cap: Option<Duration>,
}

impl Retry {
    /// Up to `max_attempts` attempts, `delay` between them. A limit of zero is
    /// taken as one: the first attempt always goes.
    #[must_use]
    pub const fn new(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts: if max_attempts == 0 { 1 } else { max_attempts },
            delay,
            factor: 1,
            cap: None,
        }
    }

    /// The guard the platform's policy declares.
    #[must_use]
    pub const fn from_policy(policy: &RetryPolicy) -> Self {
        Self::new(policy.max_attempts, policy.delay)
    }

    /// Multiply the delay by `factor` after every failed attempt, never past
    /// `cap`. A factor of zero is taken as one.
    #[must_use]
    pub const fn backing_off(mut self, factor: u32, cap: Duration) -> Self {
        self.factor = if factor == 0 { 1 } else { factor };
        self.cap = Some(cap);
        self
    }

    /// How many attempts may go in all.
    #[must_use]
    pub const fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    /// The delay after attempt `number` failed, counted from one.
    #[must_use]
    pub fn delay_after(&self, number: u32) -> Duration {
        let grown = (1..number).try_fold(self.delay, |delay, _| delay.checked_mul(self.factor));
        let delay = grown.unwrap_or(Duration::MAX);
        match self.cap {
            Some(cap) => delay.min(cap),
            None => delay,
        }
    }
}

impl Guard for Retry {
    fn technology(&self) -> &'static str {
        "retry"
    }

    fn before(&self, _: u32) -> Decision {
        Decision::Proceed
    }

    fn after(&self, attempt: &Attempt) -> Decision {
        match &attempt.failure {
            Some(failure) if failure.is_retryable() && attempt.number < self.max_attempts => {
                Decision::Wait(self.delay_after(attempt.number))
            }
            _ => Decision::Proceed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resilience::{Failure, Guarded, execute};
    use std::cell::Cell;

    fn failed(number: u32, failure: Failure) -> Attempt {
        Attempt {
            number,
            elapsed: Duration::ZERO,
            failure: Some(failure),
        }
    }

    #[test]
    fn a_retryable_failure_below_the_limit_waits_and_the_last_attempt_stands() {
        let retry = Retry::new(3, Duration::from_millis(2));
        assert_eq!(retry.technology(), "retry");
        assert_eq!(retry.before(1), Decision::Proceed);
        assert_eq!(
            retry.after(&failed(1, Failure::retryable("again"))),
            Decision::Wait(Duration::from_millis(2))
        );
        assert_eq!(
            retry.after(&failed(2, Failure::retryable("again"))),
            Decision::Wait(Duration::from_millis(2))
        );
        assert_eq!(
            retry.after(&failed(3, Failure::retryable("again"))),
            Decision::Proceed
        );
        let succeeded = Attempt {
            number: 1,
            elapsed: Duration::ZERO,
            failure: None,
        };
        assert_eq!(retry.after(&succeeded), Decision::Proceed);
    }

    #[test]
    fn a_permanent_failure_is_not_tried_again() {
        let retry = Retry::from_policy(&RetryPolicy {
            max_attempts: 5,
            delay: Duration::ZERO,
        });
        assert_eq!(retry.max_attempts(), 5);
        assert_eq!(
            retry.after(&failed(1, Failure::permanent("broken"))),
            Decision::Proceed
        );
        assert_eq!(Retry::new(0, Duration::ZERO).max_attempts(), 1);
    }

    #[test]
    fn backing_off_grows_the_delay_by_the_factor_and_stops_at_the_cap() {
        let retry =
            Retry::new(6, Duration::from_millis(1)).backing_off(2, Duration::from_millis(5));
        assert_eq!(retry.delay_after(1), Duration::from_millis(1));
        assert_eq!(retry.delay_after(2), Duration::from_millis(2));
        assert_eq!(retry.delay_after(3), Duration::from_millis(4));
        assert_eq!(retry.delay_after(4), Duration::from_millis(5));
        assert_eq!(
            retry.after(&failed(3, Failure::retryable("again"))),
            Decision::Wait(Duration::from_millis(4))
        );
        let flat = Retry::new(6, Duration::from_millis(1));
        assert_eq!(flat.delay_after(4), Duration::from_millis(1));
    }

    #[test]
    fn under_execute_the_operation_runs_until_it_succeeds_or_the_limit_is_reached() {
        let retry = Retry::new(3, Duration::ZERO);
        let guards: [&dyn Guard; 1] = [&retry];
        let calls = Cell::new(0);
        let outcome = execute(&guards, || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(Failure::retryable("again"))
            } else {
                Ok("done")
            }
        });
        assert_eq!(outcome, Ok(Guarded::Done("done")));
        assert_eq!(calls.get(), 3);

        calls.set(0);
        let exhausted: Result<Guarded<()>, Failure> = execute(&guards, || {
            calls.set(calls.get() + 1);
            Err(Failure::retryable("again"))
        });
        assert_eq!(exhausted, Err(Failure::retryable("again")));
        assert_eq!(calls.get(), 3);
    }

    /// Answers any failure with the fallback, as the fallback technology does.
    struct Instead;

    impl Guard for Instead {
        fn technology(&self) -> &'static str {
            "fallback"
        }

        fn before(&self, _: u32) -> Decision {
            Decision::Proceed
        }

        fn after(&self, attempt: &Attempt) -> Decision {
            if attempt.succeeded() {
                Decision::Proceed
            } else {
                Decision::Fallback
            }
        }
    }

    #[test]
    fn retry_ahead_of_a_fallback_gets_its_attempts_before_the_fallback_answers() {
        let retry = Retry::new(2, Duration::ZERO);
        let guards: [&dyn Guard; 2] = [&retry, &Instead];
        let calls = Cell::new(0);
        let outcome: Result<Guarded<()>, Failure> = execute(&guards, || {
            calls.set(calls.get() + 1);
            Err(Failure::retryable("again"))
        });
        assert_eq!(outcome, Ok(Guarded::Fallback));
        assert_eq!(calls.get(), 2, "retry ran out, then the fallback answered");
    }
}
