//! Per-owner submit rate limit (NFR-18).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RateLimit {
    window: Duration,
    max: usize,
    hits: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl RateLimit {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            window,
            max,
            hits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn allow(&self, owner: &str) -> bool {
        let now = Instant::now();
        let mut g = self.hits.lock().expect("limit");
        let v = g.entry(owner.to_string()).or_default();
        v.retain(|t| now.duration_since(*t) < self.window);
        if v.len() >= self.max {
            return false;
        }
        v.push(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_max() {
        let l = RateLimit::new(2, Duration::from_secs(10));
        assert!(l.allow("a"));
        assert!(l.allow("a"));
        assert!(!l.allow("a"));
        assert!(l.allow("b"));
    }
}
