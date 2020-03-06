use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration as StdDuration, Instant};

use futures_timer::Delay;
use log::warn;

pub struct Interval {
    max_duration: StdDuration,
    is_running: AtomicBool,
}

impl Interval {
    pub fn new(rate: u64) -> Self {
        Self {
            max_duration: StdDuration::from_nanos(1_000_000_000 / rate),
            is_running: AtomicBool::new(true),
        }
    }

    pub async fn run<F>(&self, fun: F)
    where
        F: Fn() -> (),
    {
        self.is_running.store(true, Ordering::SeqCst);

        while self.is_running.load(Ordering::SeqCst) {
            let future = async { fun() };
            let start = Instant::now();
            future.await;
            let diff = Instant::now() - start;

            if diff >= self.max_duration {
                warn!(
                    "Rate exceeded: {} ns >= {} ns",
                    diff.as_nanos(),
                    self.max_duration.as_nanos()
                );
            } else {
                Delay::new(self.max_duration - diff).await;
            }
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}
