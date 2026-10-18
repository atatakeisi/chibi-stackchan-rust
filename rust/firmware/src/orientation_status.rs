//! Small, read-only diagnostics for face rotation. No user content is retained.
use serde::Serialize;
use std::{sync::Mutex, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub accel_mg: Option<[i32; 3]>,
    pub rotation: u8,
    pub sample_count: u64,
    pub imu_enabled: bool,
    pub reset_reason: u32,
    pub uptime_ms: u64,
    pub last_sample_ms: Option<u64>,
}

struct Status {
    snapshot: Snapshot,
    clock_started: Option<Instant>,
}

impl Status {
    const fn new() -> Self {
        Self {
            snapshot: Snapshot {
                accel_mg: None,
                rotation: 0,
                sample_count: 0,
                imu_enabled: false,
                reset_reason: 0,
                uptime_ms: 0,
                last_sample_ms: None,
            },
            clock_started: None,
        }
    }

    fn initialize(&mut self, reset_reason: u32, uptime_ms: u64, now: Instant) {
        *self = Self::new();
        self.snapshot.reset_reason = reset_reason;
        self.snapshot.uptime_ms = uptime_ms;
        self.clock_started = Some(now);
    }

    fn record_sample(&mut self, accel_mg: [i32; 3], sampled_at_ms: u64) {
        self.snapshot.accel_mg = Some(accel_mg);
        self.snapshot.last_sample_ms = Some(sampled_at_ms);
        self.snapshot.sample_count = self.snapshot.sample_count.saturating_add(1);
    }

    fn snapshot(&self, now: Instant) -> Snapshot {
        let mut snapshot = self.snapshot;
        if let Some(started) = self.clock_started {
            let elapsed_ms = now
                .saturating_duration_since(started)
                .as_millis()
                .min(u64::MAX as u128) as u64;
            snapshot.uptime_ms = snapshot.uptime_ms.saturating_add(elapsed_ms);
        }
        snapshot
    }
}

static STATUS: Mutex<Status> = Mutex::new(Status::new());

fn with_status<T>(update: impl FnOnce(&mut Status) -> T) -> T {
    let mut status = STATUS.lock().unwrap_or_else(|error| error.into_inner());
    update(&mut status)
}

/// Call once at boot with the device's current monotonic uptime.
pub fn initialize(reset_reason: u32, uptime_ms: u64) {
    with_status(|status| status.initialize(reset_reason, uptime_ms, Instant::now()));
}

pub fn set_imu_enabled(enabled: bool) {
    with_status(|status| status.snapshot.imu_enabled = enabled);
}

/// Record successful sensor reads, including samples that do not change rotation.
pub fn record_sample(accel_mg: [i32; 3], sampled_at_ms: u64) {
    with_status(|status| status.record_sample(accel_mg, sampled_at_ms));
}

/// Rotation is the displayed clockwise quarter-turn count, always 0 through 3.
pub fn set_rotation(rotation: u8) {
    with_status(|status| status.snapshot.rotation = rotation % 4);
}

pub fn snapshot() -> Snapshot {
    with_status(|status| status.snapshot(Instant::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn samples_and_clock_distinguish_stale_sensor_data_from_a_reboot() {
        let now = Instant::now();
        let mut status = Status::new();
        status.initialize(7, 1_000, now);
        assert_eq!(status.snapshot(now).accel_mg, None);
        assert_eq!(status.snapshot(now).last_sample_ms, None);
        status.snapshot.imu_enabled = true;
        status.record_sample([0, -1_000, 25], 1_100);
        status.record_sample([1_000, 0, -20], 1_200);
        status.snapshot.rotation = 1;
        status.snapshot.imu_enabled = false;

        let snapshot = status.snapshot(now + Duration::from_millis(2_000));
        assert_eq!(snapshot.accel_mg, Some([1_000, 0, -20]));
        assert_eq!(snapshot.last_sample_ms, Some(1_200));
        assert_eq!(snapshot.sample_count, 2);
        assert_eq!(snapshot.rotation, 1);
        assert!(!snapshot.imu_enabled);
        assert_eq!(snapshot.reset_reason, 7);
        assert_eq!(snapshot.uptime_ms, 3_000);
    }

    #[test]
    fn boot_initialization_clears_previous_sample_and_rotation() {
        let now = Instant::now();
        let mut status = Status::new();
        status.record_sample([0, 1_000, 0], 50_000);
        status.snapshot.rotation = 3;
        status.snapshot.imu_enabled = true;
        status.initialize(12, 5, now);

        let snapshot = status.snapshot(now);
        assert_eq!(snapshot.sample_count, 0);
        assert_eq!(snapshot.accel_mg, None);
        assert_eq!(snapshot.last_sample_ms, None);
        assert_eq!(snapshot.rotation, 0);
        assert!(!snapshot.imu_enabled);
        assert_eq!(snapshot.reset_reason, 12);
        assert_eq!(snapshot.uptime_ms, 5);
    }
}
