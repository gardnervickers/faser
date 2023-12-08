use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Clock {
    start: Instant,
    time: TimeSource,
}

impl Clock {
    pub fn system() -> Self {
        Self {
            start: Instant::now(),
            time: TimeSource::System,
        }
    }

    pub fn simulated() -> Self {
        Self {
            start: Instant::now(),
            time: TimeSource::Simulated {
                offset: Rc::new(Cell::new(Duration::from_secs(0))),
            },
        }
    }

    /// Convert the provided instant to a tick which can be used inside the time driver.
    fn instant_to_tick(&self, t: Instant) -> u64 {
        let dur: Duration = t
            .checked_duration_since(self.start)
            .unwrap_or_else(|| Duration::from_secs(0));
        let ms = dur.as_millis();
        ms.try_into().expect("Duration too far into the future")
    }

    /// Convert a tick to a duration value.
    pub(crate) fn tick_to_duration(&self, t: u64) -> Duration {
        Duration::from_millis(t)
    }

    /// Return the current tick.
    pub(crate) fn tick(&self) -> u64 {
        let now = self.now();
        self.instant_to_tick(now)
    }

    /// Return the current instant.
    pub fn now(&self) -> Instant {
        match &self.time {
            TimeSource::System => Instant::now(),
            TimeSource::Simulated { offset } => {
                let offset = offset.get();
                self.start + offset
            }
        }
    }
    /// Advances simulated time.
    pub fn advance(&self, duration: Duration) {
        match &self.time {
            TimeSource::System => panic!("Cannot advance system clock"),
            TimeSource::Simulated { offset } => {
                offset.set(offset.get() + duration);
            }
        }
    }
}

#[derive(Debug, Clone)]
enum TimeSource {
    System,
    Simulated { offset: Rc<Cell<Duration>> },
}
