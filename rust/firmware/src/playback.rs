//! Streaming mono voice resampling for the board's fixed 16 kHz I2S clock.
//!
//! Linear interpolation preserves fractional phase across MP3 packets. Startup
//! extends the first sample backwards by one source frame, so this causal
//! implementation needs neither a lookahead packet nor a final flush. It emits
//! exactly floor(input_frames * 16000 / input_rate) samples cumulatively.
//! This simple voice resampler is not a band-limited anti-aliasing filter; music
//! or source energy above 8 kHz can alias when downsampling.

use anyhow::{ensure, Context, Result};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

pub const OUTPUT_RATE: u32 = 16_000;

/// PCM accepted by I2S during the last successfully completed MP3 playback.
/// The source peak and fingerprint describe resampled 16 kHz stereo i16
/// little-endian PCM before attenuation. The fingerprint is non-cryptographic
/// FNV-1a, useful for comparing the same voice independently of MP3 metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PlaybackStats {
    pub source_peak: u32,
    pub output_peak: u32,
    pub volume_min: u8,
    pub volume_max: u8,
    pub frames: u64,
    pub source_fingerprint: u32,
}

/// Live output volume shared with request handlers. Hardware stays on its owner
/// task; every PCM chunk uses one consistent snapshot of this value.
#[derive(Clone, Debug)]
pub struct VolumeControl(Arc<AtomicU32>);

/// A consistent live volume, mute restore level and revision. The revision also
/// detects intervening requests that happen to restore the same volume.
#[derive(Clone, Copy, Debug)]
pub struct VolumeSnapshot {
    pub volume: u8,
    pub last_nonzero_volume: u8,
    state: u32,
}

impl VolumeControl {
    pub fn new(volume: u8) -> Self {
        Self::new_with_restore(volume, 10)
    }

    pub fn new_with_restore(volume: u8, restore: u8) -> Self {
        let last_nonzero = if volume > 0 {
            volume
        } else if restore > 0 {
            restore
        } else {
            10
        };
        let state = (u32::from(last_nonzero) << 8) | u32::from(volume);
        Self(Arc::new(AtomicU32::new(state)))
    }

    pub fn get(&self) -> u8 {
        self.0.load(Ordering::Relaxed) as u8
    }

    pub fn snapshot(&self) -> VolumeSnapshot {
        Self::snapshot_from_state(self.0.load(Ordering::Relaxed))
    }

    fn snapshot_from_state(state: u32) -> VolumeSnapshot {
        VolumeSnapshot {
            volume: state as u8,
            last_nonzero_volume: (state >> 8) as u8,
            state,
        }
    }

    pub fn set(&self, volume: u8) {
        self.replace(volume);
    }

    /// Publish a requested volume and return that exact revision, so a failed
    /// command enqueue can roll itself back without undoing another request.
    pub fn replace(&self, volume: u8) -> VolumeSnapshot {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            let next = Self::next_state(current, volume);
            match self
                .0
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return Self::snapshot_from_state(next),
                Err(updated) => current = updated,
            }
        }
    }

    /// Apply a delayed change only if no other request has changed the control
    /// since the snapshot. Even setting the same value advances the revision.
    pub fn set_if_unchanged(&self, snapshot: VolumeSnapshot, volume: u8) -> bool {
        self.0
            .compare_exchange(
                snapshot.state,
                Self::next_state(snapshot.state, volume),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    /// Undo a rejected submission, restoring its mute history as well as its
    /// live volume. A newer request prevents this rollback.
    pub fn restore_if_unchanged(&self, published: VolumeSnapshot, before: VolumeSnapshot) -> bool {
        let restored =
            (published.state.wrapping_add(1 << 16) & 0xffff_0000) | (before.state & 0xffff);
        self.0
            .compare_exchange(
                published.state,
                restored,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    fn next_state(state: u32, volume: u8) -> u32 {
        // One 32-bit atomic keeps both volume levels and the 16-bit revision
        // together on the ESP32-S3 without locks in the PCM output path.
        let last_nonzero = if volume > 0 {
            u32::from(volume) << 8
        } else {
            state & 0xff00
        };
        (state.wrapping_add(1 << 16) & 0xffff_0000) | last_nonzero | u32::from(volume)
    }

    /// Scale both channels of signed 16-bit little-endian stereo PCM in place.
    /// Returns the volume snapshot and the actual output peak. Invalid frame
    /// lengths leave the entire buffer untouched. Zero is exact silence and
    /// 255 preserves every sample, including i16::MIN.
    pub fn apply(&self, stereo: &mut [u8]) -> Result<(u8, u32)> {
        ensure!(stereo.len() % 4 == 0, "incomplete stereo PCM frame");
        let volume = self.get();
        let mut peak = 0;
        for bytes in stereo.chunks_exact_mut(2) {
            let sample = i32::from(i16::from_le_bytes([bytes[0], bytes[1]]));
            let scaled = sample * i32::from(volume) / 255;
            peak = peak.max(scaled.unsigned_abs());
            bytes.copy_from_slice(&(scaled as i16).to_le_bytes());
        }
        Ok((volume, peak))
    }
}

#[derive(Default)]
pub struct Resampler {
    format: Option<(usize, u32)>,
    phase: u32,
    previous: Option<i16>,
}

impl Resampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns mono 16-bit PCM, scaling volume from silence (0) to unity (255).
    /// `channels` is 1 or 2, and input frames must be complete. Once a nonempty
    /// packet starts a stream its rate and channels cannot change; construct a
    /// new resampler for another track. Validation errors leave state untouched.
    pub fn push(
        &mut self,
        interleaved: &[i16],
        channels: usize,
        input_rate: u32,
        volume: u8,
    ) -> Result<Vec<i16>> {
        ensure!(
            matches!(channels, 1 | 2),
            "audio must have one or two channels"
        );
        ensure!(
            (8_000..=96_000).contains(&input_rate),
            "unsupported audio sample rate: {input_rate}"
        );
        ensure!(
            interleaved.len() % channels == 0,
            "incomplete interleaved audio frame"
        );
        if let Some(format) = self.format {
            ensure!(
                format == (channels, input_rate),
                "audio format changed during playback"
            );
        }
        let frames = interleaved.len() / channels;
        let count = (frames as u128 * u128::from(OUTPUT_RATE) + u128::from(self.phase))
            / u128::from(input_rate);
        let count = usize::try_from(count).context("resampled audio length overflow")?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(count)
            .context("allocating resampled audio")?;
        if frames == 0 {
            return Ok(output);
        }
        self.format = Some((channels, input_rate));

        for frame in interleaved.chunks_exact(channels) {
            let sum = frame.iter().map(|&sample| i32::from(sample)).sum::<i32>();
            let mono = sum / channels as i32;
            let current =
                (mono * i32::from(volume) / 255).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            let previous = self.previous.unwrap_or(current);
            self.phase += OUTPUT_RATE;
            while self.phase >= input_rate {
                self.phase -= input_rate;
                // Remaining phase locates this output instant inside the
                // previous/current source-frame interval. It is at most16000
                // here, including when an8 kHz frame emits two output samples.
                let fraction = OUTPUT_RATE - self.phase;
                let weighted = i64::from(previous) * i64::from(OUTPUT_RATE - fraction)
                    + i64::from(current) * i64::from(fraction);
                let half = i64::from(OUTPUT_RATE / 2);
                let rounded = if weighted >= 0 {
                    weighted + half
                } else {
                    weighted - half
                };
                let sample = rounded / i64::from(OUTPUT_RATE);
                output.push(sample.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16);
            }
            self.previous = Some(current);
        }
        debug_assert_eq!(output.len(), count);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn live_volume_zero_mutes_every_signed_sample_exactly() {
        let mut bytes = pcm_bytes(&(i16::MIN..=i16::MAX).collect::<Vec<_>>());
        assert_eq!(VolumeControl::new(0).apply(&mut bytes).unwrap(), (0, 0));
        assert!(bytes.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn live_volume_preserves_unity_and_bounds_quiet_signed_peaks() {
        let original = pcm_bytes(&[i16::MIN, i16::MAX, -255, 255, -1, 1, -12_345, 12_345]);
        let mut bytes = original.clone();
        assert_eq!(
            VolumeControl::new(255).apply(&mut bytes).unwrap(),
            (255, 32_768)
        );
        assert_eq!(bytes, original);
        assert_eq!(VolumeControl::new(5).apply(&mut bytes).unwrap(), (5, 642));
        assert_eq!(bytes, pcm_bytes(&[-642, 642, -5, 5, 0, 0, -242, 242]));
    }

    #[test]
    fn live_volume_clones_update_the_next_output_chunk() {
        let control = VolumeControl::new(255);
        let request_control = control.clone();
        std::thread::spawn(move || request_control.set(5))
            .join()
            .unwrap();
        assert_eq!(control.get(), 5);
        let original = pcm_bytes(&[25_500, -25_500, i16::MIN, i16::MAX]);
        let mut first = original.clone();
        assert_eq!(control.apply(&mut first).unwrap(), (5, 642));
        assert_eq!(first, pcm_bytes(&[500, -500, -642, 642]));
        control.clone().set(0);
        let mut next = original;
        assert_eq!(control.apply(&mut next).unwrap(), (0, 0));
        assert!(next.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn identical_source_pcm_at_fifteen_has_three_times_the_output_at_five() {
        let original = pcm_bytes(&(i16::MIN..=i16::MAX).collect::<Vec<_>>());
        let control = VolumeControl::new(5);
        let mut quiet = original.clone();
        assert_eq!(control.apply(&mut quiet).unwrap(), (5, 642));

        control.clone().set(15);
        let mut louder = original;
        assert_eq!(control.apply(&mut louder).unwrap(), (15, 1_927));
        assert_ne!(quiet, louder);
        for (quiet, louder) in quiet.chunks_exact(2).zip(louder.chunks_exact(2)) {
            let quiet = i32::from(i16::from_le_bytes([quiet[0], quiet[1]]));
            let louder = i32::from(i16::from_le_bytes([louder[0], louder[1]]));
            // Integer truncation can lose at most two counts when tripled.
            assert!((louder - quiet * 3).abs() <= 2);
        }
    }

    #[test]
    fn delayed_volume_change_does_not_overwrite_an_intervening_web_request() {
        let control = VolumeControl::new(10);
        let voice_snapshot = control.snapshot();
        assert_eq!(voice_snapshot.volume, 10);
        let web = control.clone();
        web.set(20);
        web.set(10);
        assert!(!control.set_if_unchanged(voice_snapshot, 8));
        assert_eq!(control.get(), 10);

        // An explicit repeated request is still newer than the voice snapshot.
        let voice_snapshot = control.snapshot();
        web.set(10);
        assert!(!control.set_if_unchanged(voice_snapshot, 8));
        assert_eq!(control.get(), 10);
    }

    #[test]
    fn conditional_volume_change_updates_shared_playback_and_consumes_snapshot() {
        let control = VolumeControl::new(10);
        let snapshot = control.snapshot();
        assert!(control.clone().set_if_unchanged(snapshot, 8));
        assert!(!control.set_if_unchanged(snapshot, 6));
        assert_eq!(control.get(), 8);
        let mut audio = pcm_bytes(&[25_500, -25_500]);
        assert_eq!(control.apply(&mut audio).unwrap(), (8, 800));
        assert_eq!(audio, pcm_bytes(&[800, -800]));
    }

    #[test]
    fn mute_remembers_the_latest_web_volume_before_settings_are_saved() {
        let control = VolumeControl::new(10);
        control.set(20);
        control.set(0);
        control.set(0);
        let muted = control.snapshot();
        assert_eq!((muted.volume, muted.last_nonzero_volume), (0, 20));
        assert!(control.set_if_unchanged(muted, muted.last_nonzero_volume));
        assert_eq!(control.get(), 20);
    }

    #[test]
    fn muted_boot_preserves_saved_restore_level_and_validates_fallback() {
        let saved = VolumeControl::new_with_restore(0, 8).snapshot();
        assert_eq!((saved.volume, saved.last_nonzero_volume), (0, 8));
        assert_eq!(
            VolumeControl::new_with_restore(0, 0)
                .snapshot()
                .last_nonzero_volume,
            10
        );
        assert_eq!(
            VolumeControl::new_with_restore(5, 8)
                .snapshot()
                .last_nonzero_volume,
            5
        );
    }

    #[test]
    fn rejected_request_restores_both_muted_volume_and_its_previous_restore_level() {
        let control = VolumeControl::new_with_restore(0, 8);
        let before = control.snapshot();
        let published = control.replace(20);
        assert_eq!(published.last_nonzero_volume, 20);
        assert!(control.restore_if_unchanged(published, before));
        let restored = control.snapshot();
        assert_eq!((restored.volume, restored.last_nonzero_volume), (0, 8));
        assert!(!control.set_if_unchanged(before, 5));

        let published = control.replace(20);
        control.set(30);
        assert!(!control.restore_if_unchanged(published, restored));
        let newer = control.snapshot();
        assert_eq!((newer.volume, newer.last_nonzero_volume), (30, 30));
    }

    #[test]
    fn live_volume_rejects_partial_frames_before_mutating_audio() {
        let control = VolumeControl::new(5);
        for length in [1, 2, 3, 5, 6, 7] {
            let mut bytes: Vec<u8> = (1..=length).collect();
            let original = bytes.clone();
            assert!(control.apply(&mut bytes).is_err());
            assert_eq!(bytes, original);
        }
        assert_eq!(control.apply(&mut []).unwrap(), (5, 0));
    }

    fn signal(frames: usize, channels: usize) -> Vec<i16> {
        (0..frames * channels)
            .map(|i| {
                (i as u32)
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223) as i16
            })
            .collect()
    }

    #[test]
    fn output_lengths_follow_fixed_clock_at_all_voice_rates() {
        for input_rate in [8_000, 16_000, 24_000, 44_100, 48_000, 96_000] {
            let mut resampler = Resampler::new();
            let input = vec![123; input_rate as usize];
            let output = resampler.push(&input, 1, input_rate, 255).unwrap();
            assert_eq!(output.len(), OUTPUT_RATE as usize, "{input_rate}");
            assert!(output.iter().all(|&value| value == 123));
        }
        let mut resampler = Resampler::new();
        for frames in 1..100 {
            resampler.push(&[10], 1, 44_100, 255).unwrap();
            assert_eq!(
                resampler.phase as usize,
                frames * OUTPUT_RATE as usize % 44_100
            );
        }
    }

    #[test]
    fn packet_boundaries_never_change_samples_or_cumulative_length() {
        for input_rate in [8_000, 24_000, 44_100, 48_000] {
            for channels in [1, 2] {
                let input = signal(10_007, channels);
                let expected = Resampler::new()
                    .push(&input, channels, input_rate, 173)
                    .unwrap();
                for packet_frames in [1, 2, 7, 31, 1152] {
                    let mut resampler = Resampler::new();
                    let mut actual = Vec::new();
                    let mut frames = 0;
                    for packet in input.chunks(packet_frames * channels) {
                        frames += packet.len() / channels;
                        actual.extend(resampler.push(packet, channels, input_rate, 173).unwrap());
                        assert_eq!(
                            actual.len(),
                            frames * OUTPUT_RATE as usize / input_rate as usize
                        );
                    }
                    assert_eq!(
                        actual, expected,
                        "rate={input_rate}, channels={channels}, packet={packet_frames}"
                    );
                }
            }
        }
    }

    #[test]
    fn interpolation_and_initial_padding_are_deterministic() {
        let output = Resampler::new()
            .push(&[0, 1000, 2000], 1, 8_000, 255)
            .unwrap();
        assert_eq!(output, [0, 0, 500, 1000, 1500, 2000]);
        let output = Resampler::new()
            .push(&[0, -1000, -2000], 1, 8_000, 255)
            .unwrap();
        assert_eq!(output, [0, 0, -500, -1000, -1500, -2000]);
        let output = Resampler::new()
            .push(&[0, 1000, 2000], 1, 24_000, 255)
            .unwrap();
        assert_eq!(output, [500, 2000]);
        let input = signal(500, 1);
        assert_eq!(
            Resampler::new().push(&input, 1, 16_000, 255).unwrap(),
            input
        );
    }

    #[test]
    fn stereo_extremes_downmix_without_overflow_and_volume_zero_is_silent() {
        let stereo = [i16::MIN, i16::MIN, i16::MAX, i16::MAX, i16::MIN, i16::MAX];
        assert_eq!(
            Resampler::new().push(&stereo, 2, 16_000, 255).unwrap(),
            [i16::MIN, i16::MAX, 0]
        );
        let input = signal(1000, 2);
        assert!(Resampler::new()
            .push(&input, 2, 8_000, 0)
            .unwrap()
            .iter()
            .all(|&sample| sample == 0));
        let quiet = Resampler::new()
            .push(&[i16::MIN, i16::MAX], 1, 16_000, 128)
            .unwrap();
        assert_eq!(quiet, [-16448, 16447]);
    }

    #[test]
    fn invalid_packets_and_format_changes_leave_stream_unchanged() {
        let mut resampler = Resampler::new();
        assert!(resampler.push(&[], 0, 16_000, 255).is_err());
        assert!(resampler.push(&[], 3, 16_000, 255).is_err());
        assert!(resampler.push(&[], 1, 0, 255).is_err());
        assert!(resampler.push(&[], 1, 96_001, 255).is_err());
        assert!(resampler.push(&[1], 2, 16_000, 255).is_err());
        assert!(resampler.push(&[], 2, 48_000, 255).unwrap().is_empty());
        assert!(resampler.format.is_none());
        resampler.push(&[0], 1, 24_000, 255).unwrap();
        assert!(resampler.push(&[5], 1, 48_000, 255).is_err());
        assert!(resampler.push(&[5, 5], 2, 24_000, 255).is_err());
        assert!(resampler.push(&[], 2, 48_000, 255).is_err());
        assert_eq!(
            resampler.push(&[1000, 2000], 1, 24_000, 255).unwrap(),
            [500, 2000]
        );
    }
}
