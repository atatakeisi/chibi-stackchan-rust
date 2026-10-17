//! Hardware-independent microphone, recording and WAV logic.
//!
//! Call `Vad::push` only after an I2S read has completed. Unlike the Arduino
//! recording queue, these routines never inspect a buffer being filled by DMA.

pub const SAMPLE_RATE: usize = 16_000;
pub const VAD_CHUNK_SAMPLES: usize = 320;
pub const MAX_SAMPLES: usize = SAMPLE_RATE * 12;
pub const PREROLL_SAMPLES: usize = SAMPLE_RATE;
pub const WARMUP_CHUNKS: usize = 40;
pub const SILENCE_SAMPLES: usize = SAMPLE_RATE * 2;
pub const PROFILE_WINDOW: usize = 1_600;

pub fn peak(samples: &[i16]) -> u32 {
    samples
        .iter()
        .map(|&s| i32::from(s).unsigned_abs())
        .max()
        .unwrap_or(0)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Profile {
    pub pre_peak: u32,
    pub live_peak: u32,
    pub voiced: usize,
    pub count: usize,
    pub peaks: Vec<u32>,
}

/// Equivalent to the existing C++/Rust profile, without a fixed array limit.
pub fn profile(samples: &[i16], pre: usize, threshold: i32) -> Profile {
    let mut result = Profile {
        pre_peak: 0,
        live_peak: 0,
        voiced: 0,
        count: 0,
        peaks: Vec::with_capacity(samples.len().div_ceil(PROFILE_WINDOW)),
    };
    for (window, chunk) in samples.chunks(PROFILE_WINDOW).enumerate() {
        let mut window_peak = 0;
        for (j, &sample) in chunk.iter().enumerate() {
            let magnitude = i32::from(sample).unsigned_abs();
            window_peak = window_peak.max(magnitude);
            let section = if window * PROFILE_WINDOW + j < pre {
                &mut result.pre_peak
            } else {
                &mut result.live_peak
            };
            *section = (*section).max(magnitude);
        }
        if i64::from(window_peak) >= i64::from(threshold) {
            result.voiced += 1;
        }
        result.peaks.push(window_peak);
    }
    result.count = result.peaks.len();
    result
}

/// 20 ms chunks, 0.8 s warmup, 8 loud chunks in a rolling 0.5 s window.
pub struct Vad {
    pub threshold: i32,
    warmup: usize,
    recent: [bool; 25],
    recent_pos: usize,
    loud_count: usize,
    history: Vec<i16>,
    history_pos: usize,
    history_len: usize,
}

impl Vad {
    pub fn new(threshold: i32) -> Self {
        Self {
            threshold,
            warmup: WARMUP_CHUNKS,
            recent: [false; 25],
            recent_pos: 0,
            loud_count: 0,
            history: vec![0; PREROLL_SAMPLES],
            history_pos: 0,
            history_len: 0,
        }
    }

    /// Reset whenever the codec/microphone restarts, including after playback.
    pub fn reset(&mut self) {
        self.warmup = WARMUP_CHUNKS;
        self.recent.fill(false);
        self.recent_pos = 0;
        self.loud_count = 0;
        self.history_pos = 0;
        self.history_len = 0;
    }

    /// Invalid/partial chunks do not advance the window or warmup counter.
    /// Servo noise suppresses triggering but remains in preroll, as before.
    pub fn push(&mut self, samples: &[i16], servo_noise: bool) -> bool {
        if samples.len() != VAD_CHUNK_SAMPLES {
            return false;
        }
        if self.warmup > 0 {
            self.warmup -= 1;
            return false;
        }
        for &sample in samples {
            self.history[self.history_pos] = sample;
            self.history_pos = (self.history_pos + 1) % PREROLL_SAMPLES;
            self.history_len = (self.history_len + 1).min(PREROLL_SAMPLES);
        }
        let loud = !servo_noise && i64::from(peak(samples)) >= i64::from(self.threshold);
        self.loud_count -= usize::from(self.recent[self.recent_pos]);
        self.loud_count += usize::from(loud);
        self.recent[self.recent_pos] = loud;
        self.recent_pos = (self.recent_pos + 1) % self.recent.len();
        if self.loud_count >= 8 {
            self.recent.fill(false);
            self.loud_count = 0;
            return true;
        }
        false
    }

    pub fn preroll(&self) -> Vec<i16> {
        let start = (self.history_pos + PREROLL_SAMPLES - self.history_len) % PREROLL_SAMPLES;
        (0..self.history_len)
            .map(|i| self.history[(start + i) % PREROLL_SAMPLES])
            .collect()
    }
}

/// Completed PCM is owned by the recording. Input may arrive in any chunk size;
/// silence decisions always use complete 20 ms windows, independently of I2S
/// read boundaries. No silence cutoff applies until a voice has been observed.
pub struct Recording {
    samples: Vec<i16>,
    pre_samples: usize,
    stop_peak: i32,
    voice_seen: bool,
    silent_samples: usize,
    window_peak: u32,
    window_samples: usize,
    finished: bool,
}

impl Recording {
    pub fn new(preroll: &[i16], stop_peak: i32) -> Self {
        let pre_samples = preroll.len().min(MAX_SAMPLES);
        let mut samples = Vec::with_capacity(MAX_SAMPLES);
        samples.extend_from_slice(&preroll[preroll.len() - pre_samples..]);
        Self {
            samples,
            pre_samples,
            stop_peak,
            voice_seen: pre_samples > 0,
            silent_samples: 0,
            window_peak: 0,
            window_samples: 0,
            finished: pre_samples == MAX_SAMPLES,
        }
    }

    /// Append completed samples; returns true while further recording is needed.
    /// Excess input after a cutoff is ignored and is never included in the WAV.
    pub fn push(&mut self, samples: &[i16]) -> bool {
        for &sample in samples {
            if self.finished {
                break;
            }
            self.samples.push(sample);
            self.window_peak = self.window_peak.max(i32::from(sample).unsigned_abs());
            self.window_samples += 1;
            if self.window_samples == VAD_CHUNK_SAMPLES {
                if self.stop_peak > 0 {
                    if i64::from(self.window_peak) >= i64::from(self.stop_peak) {
                        self.voice_seen = true;
                        self.silent_samples = 0;
                    } else if self.voice_seen {
                        self.silent_samples += VAD_CHUNK_SAMPLES;
                        if self.silent_samples >= SILENCE_SAMPLES {
                            self.finished = true;
                        }
                    }
                }
                self.window_samples = 0;
                self.window_peak = 0;
            }
            if self.samples.len() >= MAX_SAMPLES {
                self.finished = true;
            }
        }
        !self.finished
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
    pub fn pre_samples(&self) -> usize {
        self.pre_samples
    }
    pub fn into_wav(self) -> Vec<u8> {
        wav(&self.samples)
    }
}

/// RIFF PCM, mono signed 16-bit little-endian, 16 kHz.
pub fn wav_header(sample_count: usize) -> [u8; 44] {
    // The application caps recordings at MAX_SAMPLES. Saturating lengths keep
    // this public function well-defined even if accidentally given a huge size.
    let data_bytes = sample_count.saturating_mul(2).min(u32::MAX as usize - 36) as u32;
    let mut h = [0u8; 44];
    h[..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    h[28..32].copy_from_slice(&((SAMPLE_RATE * 2) as u32).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

pub fn wav(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(&wav_header(samples.len()));
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_vad() -> Vad {
        let mut vad = Vad::new(5_000);
        for _ in 0..WARMUP_CHUNKS {
            assert!(!vad.push(&[i16::MIN; 320], false));
        }
        assert!(vad.preroll().is_empty());
        vad
    }

    #[test]
    fn vad_counts_nonconsecutive_voice_and_rejects_knocks_and_servo() {
        let mut vad = ready_vad();
        for _ in 0..4 {
            assert!(!vad.push(&[6000; 320], false));
        }
        for _ in 0..25 {
            assert!(!vad.push(&[0; 320], false));
        }
        for _ in 0..10 {
            assert!(!vad.push(&[6000; 320], true));
        }
        for _ in 0..7 {
            assert!(!vad.push(&[6000; 320], false));
            assert!(!vad.push(&[0; 320], false));
        }
        assert!(vad.push(&[6000; 320], false));
        assert_eq!(vad.preroll().len(), SAMPLE_RATE);
        vad.reset();
        assert!(vad.preroll().is_empty());
        assert!(!vad.push(&[6000; 320], false));
    }

    #[test]
    fn preroll_preserves_chronological_order_when_ring_wraps() {
        let mut vad = ready_vad();
        for i in 0..60 {
            vad.push(&[i; 320], false);
        }
        let p = vad.preroll();
        assert_eq!(&p[..320], &[10; 320]);
        assert_eq!(&p[p.len() - 320..], &[59; 320]);
    }

    #[test]
    fn recording_stops_after_voice_then_two_seconds_and_ignores_excess() {
        let mut rec = Recording::new(&[6000; 320], 1500);
        assert!(rec.push(&[0; SILENCE_SAMPLES - 1]));
        assert!(!rec.push(&[0; 700]));
        assert!(rec.is_finished());
        assert_eq!(rec.samples().len(), 320 + SILENCE_SAMPLES);
        assert_eq!(rec.pre_samples(), 320);
        let mut rec = Recording::new(&[], 1500);
        assert!(rec.push(&[0; SILENCE_SAMPLES])); // No speech yet.
        assert!(rec.push(&[6000; 320]));
        assert!(rec.push(&[0; 16000]));
        assert!(rec.push(&[6000; 320])); // Breath/reset the silence run.
        assert!(!rec.push(&[0; SILENCE_SAMPLES]));
    }

    #[test]
    fn recording_obeys_twelve_second_cap_and_wav_is_little_endian() {
        let mut rec = Recording::new(&[], 0);
        assert!(!rec.push(&vec![123; MAX_SAMPLES + 100]));
        assert_eq!(rec.samples().len(), MAX_SAMPLES);
        let bytes = wav(&[i16::MIN, 0x1234]);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 40);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 4);
        assert_eq!(&bytes[44..], &[0, 128, 0x34, 0x12]);
    }

    #[test]
    fn profile_matches_original_two_pass_at_boundaries() {
        let samples: Vec<i16> = (0..192001u32)
            .map(|i| i.wrapping_mul(1664525).wrapping_add(1013904223) as i16)
            .collect();
        for len in [0, 1, 1599, 1600, 1601, 191999, 192000, 192001] {
            for pre in [0, 1, 777, 1600, 2300, 200000] {
                for threshold in [-1, 0, 5000, 32768, 32769] {
                    let input = &samples[..len];
                    let split = pre.min(len);
                    let got = profile(input, pre, threshold);
                    let peaks: Vec<u32> = input.chunks(1600).map(peak).collect();
                    assert_eq!(got.pre_peak, peak(&input[..split]));
                    assert_eq!(got.live_peak, peak(&input[split..]));
                    assert_eq!(
                        got.voiced,
                        peaks
                            .iter()
                            .filter(|&&p| i64::from(p) >= i64::from(threshold))
                            .count()
                    );
                    assert_eq!(got.peaks, peaks);
                }
            }
        }
    }
}
