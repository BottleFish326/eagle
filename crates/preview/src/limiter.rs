use std::sync::{Condvar, Mutex};

use crate::PreviewError;

#[derive(Debug)]
pub(crate) struct DecodeLimiter {
    capacity: usize,
    active: Mutex<usize>,
    available: Condvar,
}

impl DecodeLimiter {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            active: Mutex::new(0),
            available: Condvar::new(),
        }
    }

    pub(crate) fn acquire(&self) -> Result<DecodePermit<'_>, PreviewError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| PreviewError::PoisonedLock("decode limiter"))?;
        while *active >= self.capacity {
            active = self
                .available
                .wait(active)
                .map_err(|_| PreviewError::PoisonedLock("decode limiter"))?;
        }
        *active += 1;
        Ok(DecodePermit { limiter: self })
    }
}

pub(crate) struct DecodePermit<'a> {
    limiter: &'a DecodeLimiter,
}

impl Drop for DecodePermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.limiter.active.lock() {
            *active = active.saturating_sub(1);
            self.limiter.available.notify_one();
        }
    }
}
