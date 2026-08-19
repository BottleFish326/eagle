use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use thiserror::Error;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceMode {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkKind {
    Scan,
    Hash,
    Decode,
}

impl WorkKind {
    const fn index(self) -> usize {
        match self {
            Self::Scan => 0,
            Self::Hash => 1,
            Self::Decode => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub foreground_total: usize,
    pub background_total: usize,
    pub scan: usize,
    pub hash: usize,
    pub decode: usize,
    pub max_waiters: usize,
    pub wait_timeout: Duration,
}

impl ResourceLimits {
    #[must_use]
    pub fn for_decode_capacity(capacity: usize) -> Self {
        Self {
            foreground_total: capacity,
            background_total: 1,
            scan: 1,
            hash: 1,
            decode: capacity,
            max_waiters: capacity.saturating_mul(16).max(16),
            wait_timeout: Duration::from_secs(30),
        }
    }

    const fn kind_limit(self, kind: WorkKind) -> usize {
        match kind {
            WorkKind::Scan => self.scan,
            WorkKind::Hash => self.hash,
            WorkKind::Decode => self.decode,
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        let processors = std::thread::available_parallelism().map_or(2, usize::from);
        let foreground_total = processors.clamp(1, 8);
        Self {
            foreground_total,
            background_total: (foreground_total / 2).max(1),
            scan: foreground_total.min(2),
            hash: foreground_total.min(2),
            decode: foreground_total.min(4),
            max_waiters: 256,
            wait_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSnapshot {
    pub active: usize,
    pub waiting: usize,
    pub peak_active: usize,
    pub peak_waiting: usize,
    pub completed: u64,
    pub rejected: u64,
    pub timed_out: u64,
    pub cancelled: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    pub mode: ResourceMode,
    pub active_total: usize,
    pub waiting_total: usize,
    pub peak_active_total: usize,
    pub peak_waiting_total: usize,
    pub foreground_limit: usize,
    pub background_limit: usize,
    pub max_waiters: usize,
    pub scan: WorkSnapshot,
    pub hash: WorkSnapshot,
    pub decode: WorkSnapshot,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResourceError {
    #[error(
        "resource limits must be positive and background capacity cannot exceed foreground capacity"
    )]
    InvalidLimits,
    #[error("the bounded resource wait queue is full ({max_waiters} waiters)")]
    QueueFull { max_waiters: usize },
    #[error("timed out waiting for a {kind:?} permit after {timeout_ms} ms")]
    TimedOut { kind: WorkKind, timeout_ms: u128 },
    #[error("cancelled while waiting for a {0:?} permit")]
    Cancelled(WorkKind),
    #[error("resource controller lock is poisoned")]
    Poisoned,
}

#[derive(Debug, Clone, Copy, Default)]
struct WorkState {
    snapshot: WorkSnapshot,
}

#[derive(Debug)]
struct State {
    mode: ResourceMode,
    work: [WorkState; 3],
    active_total: usize,
    waiting_total: usize,
    peak_active_total: usize,
    peak_waiting_total: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            mode: ResourceMode::Foreground,
            work: [WorkState::default(); 3],
            active_total: 0,
            waiting_total: 0,
            peak_active_total: 0,
            peak_waiting_total: 0,
        }
    }
}

#[derive(Debug)]
struct Inner {
    limits: ResourceLimits,
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Debug, Clone)]
pub struct ResourceController {
    inner: Arc<Inner>,
}

impl ResourceController {
    /// Creates one process-wide scheduler for scan, hash, and decode work.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::InvalidLimits`] when any capacity is zero or the
    /// background capacity is greater than the foreground capacity.
    pub fn new(limits: ResourceLimits) -> Result<Self, ResourceError> {
        if limits.foreground_total == 0
            || limits.background_total == 0
            || limits.background_total > limits.foreground_total
            || limits.scan == 0
            || limits.hash == 0
            || limits.decode == 0
            || limits.max_waiters == 0
            || limits.wait_timeout.is_zero()
        {
            return Err(ResourceError::InvalidLimits);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                limits,
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
            }),
        })
    }

    #[must_use]
    pub fn with_defaults() -> Self {
        Self {
            inner: Arc::new(Inner {
                limits: ResourceLimits::default(),
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
            }),
        }
    }

    /// Acquires a permit, subject to the global and work-class limits.
    ///
    /// # Errors
    ///
    /// Returns an error when the wait queue is full, the wait times out, or the
    /// controller lock is poisoned.
    pub fn acquire(&self, kind: WorkKind) -> Result<ResourcePermit, ResourceError> {
        self.acquire_cancellable(kind, || false)
    }

    /// Acquires a permit while polling a cooperative cancellation source.
    ///
    /// # Errors
    ///
    /// Returns an error for cancellation, timeout, queue saturation, or a poisoned lock.
    pub fn acquire_cancellable<F>(
        &self,
        kind: WorkKind,
        mut is_cancelled: F,
    ) -> Result<ResourcePermit, ResourceError>
    where
        F: FnMut() -> bool,
    {
        let cancelled = is_cancelled();
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ResourceError::Poisoned)?;
        if cancelled {
            state.work[kind.index()].snapshot.cancelled += 1;
            return Err(ResourceError::Cancelled(kind));
        }
        if can_start(&state, self.inner.limits, kind) {
            activate(&mut state, kind);
            return Ok(ResourcePermit {
                controller: self.clone(),
                kind,
            });
        }
        if state.waiting_total >= self.inner.limits.max_waiters {
            state.work[kind.index()].snapshot.rejected += 1;
            return Err(ResourceError::QueueFull {
                max_waiters: self.inner.limits.max_waiters,
            });
        }

        register_waiter(&mut state, kind);
        let started = Instant::now();
        loop {
            if is_cancelled() {
                unregister_waiter(&mut state, kind);
                state.work[kind.index()].snapshot.cancelled += 1;
                self.inner.changed.notify_all();
                return Err(ResourceError::Cancelled(kind));
            }
            let remaining = self
                .inner
                .limits
                .wait_timeout
                .saturating_sub(started.elapsed());
            if remaining.is_zero() {
                unregister_waiter(&mut state, kind);
                state.work[kind.index()].snapshot.timed_out += 1;
                self.inner.changed.notify_all();
                return Err(ResourceError::TimedOut {
                    kind,
                    timeout_ms: self.inner.limits.wait_timeout.as_millis(),
                });
            }
            let wait_for = remaining.min(CANCELLATION_POLL_INTERVAL);
            let (next_state, _) = self
                .inner
                .changed
                .wait_timeout(state, wait_for)
                .map_err(|_| ResourceError::Poisoned)?;
            state = next_state;
            if can_start(&state, self.inner.limits, kind) {
                unregister_waiter(&mut state, kind);
                activate(&mut state, kind);
                return Ok(ResourcePermit {
                    controller: self.clone(),
                    kind,
                });
            }
        }
    }

    /// Switches between foreground capacity and a reduced background capacity.
    ///
    /// Already-running work is allowed to finish; no new work starts until active
    /// work falls below the new limit.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::Poisoned`] when shared state is unavailable.
    pub fn set_mode(&self, mode: ResourceMode) -> Result<(), ResourceError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ResourceError::Poisoned)?;
        state.mode = mode;
        self.inner.changed.notify_all();
        Ok(())
    }

    /// Returns scheduler counters for stability monitoring without changing work.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::Poisoned`] when shared state is unavailable.
    pub fn snapshot(&self) -> Result<ResourceSnapshot, ResourceError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ResourceError::Poisoned)?;
        Ok(ResourceSnapshot {
            mode: state.mode,
            active_total: state.active_total,
            waiting_total: state.waiting_total,
            peak_active_total: state.peak_active_total,
            peak_waiting_total: state.peak_waiting_total,
            foreground_limit: self.inner.limits.foreground_total,
            background_limit: self.inner.limits.background_total,
            max_waiters: self.inner.limits.max_waiters,
            scan: state.work[WorkKind::Scan.index()].snapshot,
            hash: state.work[WorkKind::Hash.index()].snapshot,
            decode: state.work[WorkKind::Decode.index()].snapshot,
        })
    }
}

impl Default for ResourceController {
    fn default() -> Self {
        Self::with_defaults()
    }
}

fn current_total_limit(state: &State, limits: ResourceLimits) -> usize {
    match state.mode {
        ResourceMode::Foreground => limits.foreground_total,
        ResourceMode::Background => limits.background_total,
    }
}

fn can_start(state: &State, limits: ResourceLimits, kind: WorkKind) -> bool {
    state.active_total < current_total_limit(state, limits)
        && state.work[kind.index()].snapshot.active < limits.kind_limit(kind)
}

fn register_waiter(state: &mut State, kind: WorkKind) {
    state.waiting_total += 1;
    state.peak_waiting_total = state.peak_waiting_total.max(state.waiting_total);
    let snapshot = &mut state.work[kind.index()].snapshot;
    snapshot.waiting += 1;
    snapshot.peak_waiting = snapshot.peak_waiting.max(snapshot.waiting);
}

fn unregister_waiter(state: &mut State, kind: WorkKind) {
    state.waiting_total = state.waiting_total.saturating_sub(1);
    let snapshot = &mut state.work[kind.index()].snapshot;
    snapshot.waiting = snapshot.waiting.saturating_sub(1);
}

fn activate(state: &mut State, kind: WorkKind) {
    state.active_total += 1;
    state.peak_active_total = state.peak_active_total.max(state.active_total);
    let snapshot = &mut state.work[kind.index()].snapshot;
    snapshot.active += 1;
    snapshot.peak_active = snapshot.peak_active.max(snapshot.active);
}

#[derive(Debug)]
pub struct ResourcePermit {
    controller: ResourceController,
    kind: WorkKind,
}

impl Drop for ResourcePermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.controller.inner.state.lock() {
            state.active_total = state.active_total.saturating_sub(1);
            let snapshot = &mut state.work[self.kind.index()].snapshot;
            snapshot.active = snapshot.active.saturating_sub(1);
            snapshot.completed += 1;
            self.controller.inner.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use super::{ResourceController, ResourceError, ResourceLimits, ResourceMode, WorkKind};

    fn limits() -> ResourceLimits {
        ResourceLimits {
            foreground_total: 2,
            background_total: 1,
            scan: 1,
            hash: 2,
            decode: 2,
            max_waiters: 1,
            wait_timeout: std::time::Duration::from_millis(150),
        }
    }

    #[test]
    fn applies_global_and_work_class_bounds() {
        let controller = ResourceController::new(limits()).expect("controller");
        let scan = controller.acquire(WorkKind::Scan).expect("scan permit");
        let decode = controller.acquire(WorkKind::Decode).expect("decode permit");
        let snapshot = controller.snapshot().expect("snapshot");
        assert_eq!(snapshot.active_total, 2);
        assert_eq!(snapshot.scan.active, 1);
        assert_eq!(snapshot.decode.active, 1);
        drop(scan);
        drop(decode);
        assert_eq!(controller.snapshot().expect("snapshot").active_total, 0);
    }

    #[test]
    fn background_mode_reduces_new_work_capacity() {
        let controller = ResourceController::new(limits()).expect("controller");
        controller
            .set_mode(ResourceMode::Background)
            .expect("background");
        let first = controller.acquire(WorkKind::Decode).expect("first");
        let second = controller.clone();
        let handle = thread::spawn(move || second.acquire(WorkKind::Hash));
        thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(controller.snapshot().expect("snapshot").waiting_total, 1);
        drop(first);
        assert!(handle.join().expect("join").is_ok());
    }

    #[test]
    fn bounds_waiters_and_observes_cancellation() {
        let controller = ResourceController::new(limits()).expect("controller");
        let first = controller.acquire(WorkKind::Scan).expect("first");
        let cancelled = Arc::new(AtomicBool::new(false));
        let waiter_controller = controller.clone();
        let waiter_cancelled = Arc::clone(&cancelled);
        let handle = thread::spawn(move || {
            waiter_controller
                .acquire_cancellable(WorkKind::Scan, || waiter_cancelled.load(Ordering::Acquire))
        });
        thread::sleep(std::time::Duration::from_millis(30));
        assert!(matches!(
            controller.acquire(WorkKind::Scan),
            Err(ResourceError::QueueFull { max_waiters: 1 })
        ));
        cancelled.store(true, Ordering::Release);
        assert!(matches!(
            handle.join().expect("join"),
            Err(ResourceError::Cancelled(WorkKind::Scan))
        ));
        drop(first);
        let snapshot = controller.snapshot().expect("snapshot");
        assert_eq!(snapshot.scan.rejected, 1);
        assert_eq!(snapshot.scan.cancelled, 1);
    }

    #[test]
    fn times_out_without_leaking_waiter_state() {
        let mut timeout_limits = limits();
        timeout_limits.wait_timeout = std::time::Duration::from_millis(20);
        let controller = ResourceController::new(timeout_limits).expect("controller");
        let _first = controller.acquire(WorkKind::Scan).expect("first");
        assert!(matches!(
            controller.acquire(WorkKind::Scan),
            Err(ResourceError::TimedOut {
                kind: WorkKind::Scan,
                ..
            })
        ));
        let snapshot = controller.snapshot().expect("snapshot");
        assert_eq!(snapshot.waiting_total, 0);
        assert_eq!(snapshot.scan.timed_out, 1);
    }
}
