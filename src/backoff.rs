use std::time::Duration;

/// Shared by the focus and idle listeners: their failure domains differ, but
/// the backoff and log-suppression rules are identical.
#[derive(Debug, Default)]
pub struct FailureLog {
    previous: Option<String>,
}

impl FailureLog {
    /// True for the first failure and whenever the message changes.
    pub fn should_log(&mut self, message: &str) -> bool {
        if self.previous.as_deref() == Some(message) {
            return false;
        }
        self.previous = Some(message.to_owned());
        true
    }

    /// True once after a success that followed a reported failure.
    pub fn recovered(&mut self) -> bool {
        self.previous.take().is_some()
    }
}

/// Exponential delay: `base` after the first failure, doubled per consecutive
/// failure, capped at `max`. Zero failures also return `base`.
pub fn next_delay(failures: u32, base: Duration, max: Duration) -> Duration {
    let exponent = failures.saturating_sub(1).min(16);
    base.saturating_mul(1u32 << exponent).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delay(failures: u32) -> u64 {
        next_delay(failures, Duration::from_secs(5), Duration::from_secs(300)).as_secs()
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(delay(0), 5);
        assert_eq!(delay(1), 5);
        assert_eq!(delay(2), 10);
        assert_eq!(delay(3), 20);
        assert_eq!(delay(6), 160);
        assert_eq!(delay(7), 300);
        assert_eq!(delay(100), 300);
    }

    #[test]
    fn backoff_resets_to_base_after_success() {
        assert_eq!(delay(4), 40);
        assert_eq!(delay(0), 5);
    }

    #[test]
    fn identical_errors_log_once_and_changes_log_again() {
        let mut log = FailureLog::default();
        assert!(log.should_log("socket closed"));
        assert!(!log.should_log("socket closed"));
        assert!(!log.should_log("socket closed"));
        assert!(log.should_log("permission denied"));
        assert!(!log.should_log("permission denied"));
    }

    #[test]
    fn recovery_logs_once_and_rearms_failure_logging() {
        let mut log = FailureLog::default();
        assert!(log.should_log("socket closed"));
        assert!(log.recovered());
        assert!(!log.recovered());
        assert!(log.should_log("socket closed"));
        assert!(log.recovered());
        assert!(!log.recovered());
    }
}
