//! Token-bucket global speed limiter, ported from the Python `TokenBucket`
//! (rate 0 = unlimited). One global bucket; every task's chunk worker
//! consumes from it.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Bucket {
    rate: f64,
    tokens: f64,
    last_update: Instant,
}

/// Handle to a shared bucket. All clones consume from the same tokens;
/// `set_rate` from any clone affects every consumer (global speed limit).
#[derive(Clone)]
pub struct TokenBucketHandle {
    bucket: Arc<Mutex<Bucket>>,
    stop: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for TokenBucketHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = self.bucket.lock();
        f.debug_struct("TokenBucketHandle")
            .field("rate", &b.rate)
            .finish()
    }
}

impl TokenBucketHandle {
    /// New bucket; `rate` in bytes/second, 0 disables limiting.
    pub fn new(rate: f64) -> Self {
        Self {
            bucket: Arc::new(Mutex::new(Bucket {
                rate,
                tokens: rate,
                last_update: Instant::now(),
            })),
            stop: None,
        }
    }

    /// Unlimited bucket (speed limit off).
    pub fn unlimited() -> Self {
        Self::new(0.0)
    }

    /// Attach the engine stop flag: waiting consumers wake out of their
    /// sleep slices once the flag is set, so pause/cancel land promptly.
    pub fn with_stop(self, stop: Arc<AtomicBool>) -> Self {
        Self {
            bucket: self.bucket,
            stop: Some(stop),
        }
    }

    /// New rate in bytes/second; refills the bucket (Python `set_rate`).
    pub fn set_rate(&self, rate: f64) {
        let mut b = self.bucket.lock();
        b.rate = rate;
        b.tokens = rate;
        b.last_update = Instant::now();
    }

    pub fn rate(&self) -> f64 {
        self.bucket.lock().rate
    }

    fn stopped(&self) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|s| s.load(Ordering::Relaxed))
    }

    /// Consume `amount` bytes, sleeping until the bucket allows it. The
    /// sleep is sliced at 100ms (Python does the same so pause/cancel stay
    /// responsive); a set stop flag aborts the wait.
    pub async fn consume(&self, amount: u64) {
        let mut owed = amount as f64;
        loop {
            let debt_secs = {
                let mut b = self.bucket.lock();
                if b.rate <= 0.0 {
                    return;
                }
                let now = Instant::now();
                b.tokens += now.duration_since(b.last_update).as_secs_f64() * b.rate;
                b.last_update = now;
                if b.tokens > b.rate {
                    b.tokens = b.rate;
                }
                // Take only what the bucket can cover now; the rest is
                // owed and re-attempted after the refill wait.
                let covered = owed.min(b.tokens.max(0.0));
                b.tokens -= covered;
                owed -= covered;
                (owed > 0.5).then(|| owed / b.rate)
            };
            match debt_secs {
                None => return,
                Some(secs) => {
                    let mut left = Duration::from_secs_f64(secs.min(86_400.0));
                    while !left.is_zero() {
                        if self.stopped() {
                            return;
                        }
                        let slice = left.min(Duration::from_millis(100));
                        tokio::time::sleep(slice).await;
                        left -= slice;
                    }
                }
            }
        }
    }
}
