use std::{
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

const OAUTH_STARTS_PER_MINUTE: u32 = 30;
const HOUSEKEEPING_INTERVAL: u64 = 256;

#[derive(Clone)]
pub struct OAuthStartRateLimiter {
    limiter: Arc<DefaultKeyedRateLimiter<String>>,
    checks: Arc<AtomicU64>,
}

impl OAuthStartRateLimiter {
    pub fn new(requests_per_minute: NonZeroU32) -> Self {
        Self {
            limiter: Arc::new(RateLimiter::keyed(Quota::per_minute(requests_per_minute))),
            checks: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let sequence = self.checks.fetch_add(1, Ordering::Relaxed) + 1;
        if sequence.is_multiple_of(HOUSEKEEPING_INTERVAL) {
            self.limiter.retain_recent();
        }
        self.limiter.check_key(&key.to_owned()).is_ok()
    }
}

impl Default for OAuthStartRateLimiter {
    fn default() -> Self {
        Self::new(
            NonZeroU32::new(OAUTH_STARTS_PER_MINUTE).expect("OAuth rate limit must be non-zero"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_each_client_key_independently() {
        let limiter = OAuthStartRateLimiter::new(NonZeroU32::new(2).expect("non-zero"));

        assert!(limiter.check("192.0.2.10"));
        assert!(limiter.check("192.0.2.10"));
        assert!(!limiter.check("192.0.2.10"));
        assert!(limiter.check("192.0.2.11"));
    }
}
