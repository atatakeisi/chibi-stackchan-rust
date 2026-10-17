//! Autonomous facial motion based on M5Stack-Avatar's `facialLoop`.
//!
//! The caller supplies monotonic milliseconds. No timers, tasks, hardware, or
//! allocations are needed; an explicit seed makes previews reproducible.
use crate::{Expression, FaceParams};

pub struct Animator {
    random: u32,
    origin_ms: Option<u64>,
    next_saccade_ms: u64,
    next_blink_ms: u64,
    first_blink_phase: bool,
    eyes_open: bool,
    horizontal: f32,
    vertical: f32,
}

impl Animator {
    pub fn new(seed: u32) -> Self {
        Self {
            random: if seed == 0 { 0x6d2b_79f5 } else { seed },
            origin_ms: None,
            next_saccade_ms: 1_000,
            next_blink_ms: 1_000,
            first_blink_phase: true,
            eyes_open: true,
            horizontal: 1.0,
            vertical: 1.0,
        }
    }

    fn random(&mut self) -> u32 {
        let mut value = self.random;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.random = value;
        value
    }

    fn gaze(&mut self) -> f32 {
        self.random() as f32 / (u32::MAX as f32 / 2.0) - 1.0
    }

    pub fn frame(&mut self, now_ms: u64, expression: Expression, mouth_open: f32) -> FaceParams {
        let origin = *self.origin_ms.get_or_insert(now_ms);
        let elapsed = now_ms.saturating_sub(origin);
        if elapsed >= self.next_saccade_ms {
            self.vertical = self.gaze();
            self.horizontal = self.gaze();
            self.next_saccade_ms =
                elapsed.saturating_add(500 + 100 * u64::from(self.random() % 20));
        }
        if elapsed >= self.next_blink_ms {
            // The legacy loop's first event keeps the eyes open; subsequent
            // events alternate an open interval and a short closed interval.
            if self.first_blink_phase {
                self.first_blink_phase = false;
            } else {
                self.eyes_open = !self.eyes_open;
            }
            let interval = if self.eyes_open {
                2_500 + 100 * u64::from(self.random() % 20)
            } else {
                300 + 10 * u64::from(self.random() % 20)
            };
            self.next_blink_ms = elapsed.saturating_add(interval);
        }
        let phase = ((elapsed / 33 + 1) % 100) as f32;
        FaceParams {
            expression,
            mouth_open,
            breath: (phase * std::f32::consts::TAU / 100.0).sin(),
            gaze_x: self.horizontal,
            gaze_y: self.vertical,
            eyes_open: self.eyes_open,
            rotation_deg: self.horizontal * 5.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breathing_keeps_the_original_33_ms_100_step_cycle() {
        let mut animator = Animator::new(7);
        let first = animator.frame(1_000, Expression::Neutral, 0.0).breath;
        let repeat = animator.frame(4_300, Expression::Neutral, 0.0).breath;
        assert!((first - repeat).abs() < 0.0001);
        assert!(animator.frame(5_125, Expression::Neutral, 0.0).breath > 0.99);
        assert!(animator.frame(6_775, Expression::Neutral, 0.0).breath < -0.99);
    }

    #[test]
    fn blink_intervals_and_gaze_follow_legacy_ranges_without_blocking() {
        let mut animator = Animator::new(123);
        let mut transitions = Vec::new();
        let mut previous = true;
        for now in 0..20_000 {
            let frame = animator.frame(now, Expression::Happy, 0.7);
            assert!((-1.0..=1.0).contains(&frame.gaze_x));
            assert!((-1.0..=1.0).contains(&frame.gaze_y));
            assert!((-5.0..=5.0).contains(&frame.rotation_deg));
            assert_eq!(frame.expression, Expression::Happy);
            assert_eq!(frame.mouth_open, 0.7);
            if frame.eyes_open != previous {
                transitions.push((now, frame.eyes_open));
                previous = frame.eyes_open;
            }
        }
        assert!((3_500..=5_400).contains(&transitions[0].0));
        assert!(transitions.len() >= 6);
        for pair in transitions.windows(2) {
            let duration = pair[1].0 - pair[0].0;
            let range = if pair[0].1 { 2_500..=4_400 } else { 300..=490 };
            assert!(range.contains(&duration));
        }
    }

    #[test]
    fn delayed_frames_and_clock_reversal_keep_finite_bounded_motion() {
        let mut animator = Animator::new(0);
        for now in [50, 0, u64::MAX, 51] {
            let frame = animator.frame(now, Expression::Sleepy, 0.0);
            assert!(frame.breath.is_finite());
            assert!(frame.rotation_deg.is_finite());
            assert!((-1.0..=1.0).contains(&frame.breath));
        }
    }
}
