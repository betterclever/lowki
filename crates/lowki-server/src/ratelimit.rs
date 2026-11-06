//! In-process fixed-window rate limiter.
//!
//! State lives in memory, so limits are per instance. Running several replicas multiplies
//! the effective limit; a shared store (Redis, or the database) is a TODO for that setup.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{config::Limit, error::ApiError};

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    /// Counts one event for `key` and fails once more than `limit.max` events happened in the
    /// current window.
    pub fn check(&self, scope: &str, key: &str, limit: Limit) -> Result<(), ApiError> {
        let now = Instant::now();
        let mut map = self.buckets.lock().expect("rate limiter poisoned");
        if map.len() > 50_000 {
            map.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(86_400));
        }
        let entry = map.entry(format!("{scope}|{key}")).or_insert((now, 0));
        if now.duration_since(entry.0) >= limit.window {
            *entry = (now, 0);
        }
        if entry.1 >= limit.max {
            let retry = limit.window.saturating_sub(now.duration_since(entry.0));
            return Err(ApiError::rate_limited(retry));
        }
        entry.1 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_and_resets() {
        let rl = RateLimiter::default();
        let lim = Limit {
            max: 2,
            window: Duration::from_millis(50),
        };
        assert!(rl.check("s", "k", lim).is_ok());
        assert!(rl.check("s", "k", lim).is_ok());
        assert!(rl.check("s", "k", lim).is_err());
        assert!(rl.check("s", "other", lim).is_ok());
        std::thread::sleep(Duration::from_millis(60));
        assert!(rl.check("s", "k", lim).is_ok());
    }
}
