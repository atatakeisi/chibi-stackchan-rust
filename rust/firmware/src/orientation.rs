//! Gravity-based screen orientation. Angles are clockwise in screen space.

/// BMI270 mounting correction from M5Unified 0.2.13 IMU_Class.cpp:
/// ATOMS3R uses axis_order_yxz | axis_invert_y for acceleration.
pub fn atoms3r_screen_acceleration([x, y, z]: [f32; 3]) -> [f32; 3] {
    [y, -x, z]
}

#[derive(Default)]
pub struct Orientation {
    rotation: u8,
    pending: Option<(u8, u64)>,
}

impl Orientation {
    /// Acceleration in g after mounting correction: X right, Y screen top.
    pub fn update(&mut self, [x, y, z]: [f32; 3], now: u64) -> Option<u8> {
        let length2 = x * x + y * y + z * z;
        let major = x.abs().max(y.abs());
        let minor = x.abs().min(y.abs());
        // Reject shocks, freefall, flat placement and the diagonal boundary.
        if !(0.64..=1.44).contains(&length2) || major < 0.55 || major < minor * 1.3 {
            self.pending = None;
            return None;
        }
        let candidate = if y.abs() >= x.abs() {
            if y > 0.0 {
                0
            } else {
                2
            }
        } else if x > 0.0 {
            1
        } else {
            3
        };
        if candidate == self.rotation {
            self.pending = None;
            return None;
        }
        match self.pending {
            Some((old, since)) if old == candidate => {
                if now.saturating_sub(since) >= 300 {
                    self.rotation = candidate;
                    self.pending = None;
                    return Some(candidate);
                }
            }
            _ => self.pending = Some((candidate, now)),
        }
        None
    }
}

/// Rotate RGB565 pairs without changing the panel's proven address/offset setup.
pub fn rotate_frame(source: &[u8], destination: &mut [u8], size: usize, rotation: u8) {
    assert_eq!(source.len(), size * size * 2);
    assert_eq!(source.len(), destination.len());
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = match rotation % 4 {
                1 => (size - 1 - y, x),
                2 => (size - 1 - x, size - 1 - y),
                3 => (y, size - 1 - x),
                _ => (x, y),
            };
            let src = (y * size + x) * 2;
            let dst = (dy * size + dx) * 2;
            destination[dst..dst + 2].copy_from_slice(&source[src..src + 2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoms3r_raw_axes_follow_the_official_mounting_correction() {
        let mut state = Orientation::default();
        for (i, raw) in [[0., 1., 0.], [1., 0., 0.], [0., -1., 0.], [-1., 0., 0.]]
            .into_iter()
            .enumerate()
        {
            let now = i as u64 * 1_000;
            let acceleration = atoms3r_screen_acceleration(raw);
            assert_eq!(state.update(acceleration, now), None);
            assert_eq!(
                state.update(acceleration, now + 300),
                Some((i as u8 + 1) % 4)
            );
        }
    }

    #[test]
    fn four_poses_require_a_stable_dwell() {
        let mut state = Orientation::default();
        for (i, pose) in [[1., 0., 0.], [0., -1., 0.], [-1., 0., 0.], [0., 1., 0.]]
            .into_iter()
            .enumerate()
        {
            let now = i as u64 * 1000;
            assert_eq!(state.update(pose, now), None);
            assert_eq!(state.update(pose, now + 299), None);
            assert_eq!(state.update(pose, now + 300), Some((i as u8 + 1) % 4));
        }
    }

    #[test]
    fn flat_shocks_diagonals_and_interruptions_hold_orientation() {
        let mut state = Orientation::default();
        for pose in [
            [0., 0., 1.],
            [0., 0., -1.],
            [0., 0., 0.],
            [2., 0., 0.],
            [0.7, 0.7, 0.],
            [f32::NAN, 0., 0.],
        ] {
            assert_eq!(state.update([1., 0., 0.], 0), None);
            assert_eq!(state.update(pose, 300), None);
            assert_eq!(state.update([1., 0., 0.], 301), None);
            assert_eq!(state.update([0., 1., 0.], 302), None);
        }
    }

    #[test]
    fn rotations_preserve_pixel_bytes_and_direction() {
        let source = [0, 1, 0, 2, 0, 3, 0, 4];
        let expected = [
            [0, 1, 0, 2, 0, 3, 0, 4],
            [0, 3, 0, 1, 0, 4, 0, 2],
            [0, 4, 0, 3, 0, 2, 0, 1],
            [0, 2, 0, 4, 0, 1, 0, 3],
        ];
        for r in 0..4 {
            let mut dest = [0; 8];
            rotate_frame(&source, &mut dest, 2, r);
            assert_eq!(dest, expected[r as usize]);
        }
    }
}
