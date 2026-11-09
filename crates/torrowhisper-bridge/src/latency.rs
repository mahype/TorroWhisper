//! Timing-only diagnostics. Realtime paths update fixed-size relaxed atomics;
//! only the owning/control threads format or write summaries. No signal data.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

static NEXT_RECORDING: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(crate) struct TimingAggregate {
    count: AtomicU64,
    total_us: AtomicU64,
    max_us: AtomicU64,
}

impl TimingAggregate {
    #[cfg(test)]
    pub(crate) fn count(&self) -> u64 {
        self.count.load(Relaxed)
    }

    pub(crate) fn record(&self, duration: Duration) {
        let us = duration.as_micros().min(u64::MAX as u128) as u64;
        self.total_us.fetch_add(us, Relaxed);
        self.max_us.fetch_max(us, Relaxed);
        self.count.fetch_add(1, Relaxed);
    }

    fn summary(&self) -> String {
        let count = self.count.load(Relaxed);
        let mean = self.total_us.load(Relaxed) / count.max(1);
        format!(
            "n={count},mean_us={mean},max_us={}",
            self.max_us.load(Relaxed)
        )
    }
}

pub(crate) struct RecordingDiagnostics {
    pub id: u64,
    started: Instant,
    previous_callback_us: AtomicU64,
    previous_budget_us: AtomicU64,
    pub callback: TimingAggregate,
    pub callback_gap: TimingAggregate,
    pub callback_lateness: TimingAggregate,
    pub capture_wait: TimingAggregate,
    pub capture_hold: TimingAggregate,
    pub level_wait: TimingAggregate,
    pub level_hold: TimingAggregate,
    pub level_snapshot: TimingAggregate,
    pub bridge: TimingAggregate,
    pub finish_wait: TimingAggregate,
    pub finish_hold: TimingAggregate,
    pub streaming_wait: TimingAggregate,
    pub streaming_hold: TimingAggregate,
    pub level_misses: AtomicU64,
    pub capture_errors: AtomicU64,
    pub capture_xruns: AtomicU64,
    pub capture_lock_errors: AtomicU64,
}

impl RecordingDiagnostics {
    pub(crate) fn new() -> Self {
        Self {
            id: NEXT_RECORDING.fetch_add(1, Relaxed),
            started: Instant::now(),
            previous_callback_us: AtomicU64::new(0),
            previous_budget_us: AtomicU64::new(0),
            callback: TimingAggregate::default(),
            callback_gap: TimingAggregate::default(),
            callback_lateness: TimingAggregate::default(),
            capture_wait: TimingAggregate::default(),
            capture_hold: TimingAggregate::default(),
            level_wait: TimingAggregate::default(),
            level_hold: TimingAggregate::default(),
            level_snapshot: TimingAggregate::default(),
            bridge: TimingAggregate::default(),
            finish_wait: TimingAggregate::default(),
            finish_hold: TimingAggregate::default(),
            streaming_wait: TimingAggregate::default(),
            streaming_hold: TimingAggregate::default(),
            level_misses: AtomicU64::new(0),
            capture_errors: AtomicU64::new(0),
            capture_xruns: AtomicU64::new(0),
            capture_lock_errors: AtomicU64::new(0),
        }
    }

    pub(crate) fn callback_started(&self, at: Instant, budget: Duration) {
        self.record_callback_at(at.duration_since(self.started), budget);
    }

    pub(crate) fn capture_error(&self, kind: cpal::ErrorKind) {
        self.capture_errors.fetch_add(1, Relaxed);
        if kind == cpal::ErrorKind::Xrun {
            self.capture_xruns.fetch_add(1, Relaxed);
        }
    }

    fn record_callback_at(&self, elapsed: Duration, budget: Duration) {
        // Add one so zero remains the "no previous callback" sentinel.
        let at = elapsed.as_micros() as u64 + 1;
        let previous = self.previous_callback_us.swap(at, Relaxed);
        let expected = self
            .previous_budget_us
            .swap(budget.as_micros() as u64, Relaxed);
        if previous != 0 {
            let gap = at.saturating_sub(previous);
            self.callback_gap.record(Duration::from_micros(gap));
            self.callback_lateness
                .record(Duration::from_micros(gap.saturating_sub(expected)));
        }
    }

    pub(crate) fn log_summary(&self, phase: &'static str) {
        log::info!(target: "latency",
            "recording={} phase={} cumulative approximate=true callback=[{}] gap=[{}] lateness=[{}] capture_wait=[{}] capture_hold=[{}] level_try_wait=[{}] level_hold=[{}] level_snapshot=[{}] bridge=[{}] streaming_wait=[{}] streaming_hold=[{}] finish_wait=[{}] finish_hold=[{}] level_misses={} capture_errors={} capture_xruns={} capture_lock_errors={}",
            self.id, phase, self.callback.summary(), self.callback_gap.summary(), self.callback_lateness.summary(),
            self.capture_wait.summary(), self.capture_hold.summary(), self.level_wait.summary(), self.level_hold.summary(),
            self.level_snapshot.summary(), self.bridge.summary(), self.streaming_wait.summary(), self.streaming_hold.summary(),
            self.finish_wait.summary(), self.finish_hold.summary(), self.level_misses.load(Relaxed),
            self.capture_errors.load(Relaxed), self.capture_xruns.load(Relaxed), self.capture_lock_errors.load(Relaxed));
    }
}

/// Only for non-realtime stages. Begin records identify a call still blocked;
/// completion records are emitted even on error, without formatting the error.
pub(crate) fn stage<T, E>(id: u64, name: &str, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    log::info!(target: "latency", "recording={id} stage={name} begin");
    let started = Instant::now();
    let result = f();
    log::info!(target: "latency", "recording={id} stage={name} ms={:.3} ok={}", started.elapsed().as_secs_f64() * 1000.0, result.is_ok());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_and_variable_callback_budgets_are_deterministic() {
        let d = RecordingDiagnostics::new();
        d.record_callback_at(Duration::ZERO, Duration::from_millis(10));
        d.record_callback_at(Duration::from_millis(14), Duration::from_millis(20));
        d.record_callback_at(Duration::from_millis(32), Duration::from_millis(10));
        assert_eq!(d.callback_gap.summary(), "n=2,mean_us=16000,max_us=18000");
        assert_eq!(
            d.callback_lateness.summary(),
            "n=2,mean_us=2000,max_us=4000"
        );
        assert_eq!(d.callback.summary(), "n=0,mean_us=0,max_us=0");
    }

    #[test]
    fn recording_ids_are_distinct_and_errors_are_preserved() {
        assert_ne!(
            RecordingDiagnostics::new().id,
            RecordingDiagnostics::new().id
        );
        assert_eq!(stage(0, "test", || Err::<(), _>(42)), Err(42));
    }

    #[test]
    fn capture_errors_are_counted_by_kind_without_payloads() {
        let d = RecordingDiagnostics::new();
        d.capture_error(cpal::ErrorKind::Xrun);
        d.capture_error(cpal::ErrorKind::DeviceNotAvailable);
        assert_eq!(d.capture_errors.load(Relaxed), 2);
        assert_eq!(d.capture_xruns.load(Relaxed), 1);
    }
}
