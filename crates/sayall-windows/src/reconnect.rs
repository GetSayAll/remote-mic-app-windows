use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ReconnectBackoff {
    base_delay: Duration,
    max_delay: Duration,
    next_delay: Duration,
    attempt: u32,
}

impl ReconnectBackoff {
    pub fn new(base_delay: Duration, max_delay: Duration) -> Self {
        let max_delay = max_delay.max(base_delay);
        Self {
            base_delay,
            max_delay,
            next_delay: base_delay,
            attempt: 0,
        }
    }

    pub fn reset(&mut self) {
        self.next_delay = self.base_delay;
        self.attempt = 0;
    }

    pub fn schedule_next(&mut self) -> (u32, Duration) {
        self.attempt = self.attempt.saturating_add(1);
        let delay = self.next_delay;
        self.next_delay = self
            .next_delay
            .checked_mul(2)
            .unwrap_or(self.max_delay)
            .min(self.max_delay);
        (self.attempt, delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_failures_back_off_to_the_cap() {
        let mut backoff = ReconnectBackoff::new(Duration::from_secs(2), Duration::from_secs(30));

        assert_eq!(backoff.schedule_next(), (1, Duration::from_secs(2)));
        assert_eq!(backoff.schedule_next(), (2, Duration::from_secs(4)));
        assert_eq!(backoff.schedule_next(), (3, Duration::from_secs(8)));
        assert_eq!(backoff.schedule_next(), (4, Duration::from_secs(16)));
        assert_eq!(backoff.schedule_next(), (5, Duration::from_secs(30)));
        assert_eq!(backoff.schedule_next(), (6, Duration::from_secs(30)));
    }

    #[test]
    fn successful_connection_resets_attempt_and_delay() {
        let mut backoff = ReconnectBackoff::new(Duration::from_secs(2), Duration::from_secs(30));
        let _ = backoff.schedule_next();
        let _ = backoff.schedule_next();

        backoff.reset();

        assert_eq!(backoff.schedule_next(), (1, Duration::from_secs(2)));
    }
}
