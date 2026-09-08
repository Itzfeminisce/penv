use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the epoch, injected so freshness and expiry are testable.
pub trait Clock {
    fn now(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    }
}

/// A clock a test moves by hand.
pub struct Fixed(Cell<u64>);

impl Fixed {
    pub fn new(at: u64) -> Fixed {
        Fixed(Cell::new(at))
    }

    pub fn advance(&self, seconds: u64) {
        self.0.set(self.0.get() + seconds);
    }
}

impl Clock for Fixed {
    fn now(&self) -> u64 {
        self.0.get()
    }
}
