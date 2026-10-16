#![cfg_attr(not(test), no_std)]

pub const WINDOW: usize = 1600;
pub const MAX_WINDOWS: usize = 120; // 12 seconds at 16 kHz

#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct Profile {
    pub pre_peak: u32,
    pub live_peak: u32,
    pub voiced: u32,
    pub count: u32,
    pub peaks: [u32; MAX_WINDOWS],
}

pub fn analyze(samples: &[i16], pre: usize, threshold: i32) -> Profile {
    let mut result = Profile {
        pre_peak: 0, live_peak: 0, voiced: 0, count: 0,
        peaks: [0; MAX_WINDOWS],
    };
    for (window, chunk) in samples.chunks(WINDOW).enumerate() {
        let mut peak = 0;
        for (j, &sample) in chunk.iter().enumerate() {
            // Widen before abs: i16::MIN has magnitude 32768.
            let magnitude = i32::from(sample).unsigned_abs();
            peak = peak.max(magnitude);
            let section = if window * WINDOW + j < pre {
                &mut result.pre_peak
            } else {
                &mut result.live_peak
            };
            *section = (*section).max(magnitude);
        }
        if i64::from(peak) >= i64::from(threshold) { result.voiced += 1; }
        result.peaks[window] = peak;
        result.count += 1;
    }
    result
}

/// Analyze at most 192000 PCM samples. Returns false for invalid arguments.
///
/// # Safety
/// For a nonempty input, `samples` must point to `len` aligned, initialized i16s.
/// `out` must point to an aligned, writable Profile, disjoint from the input.
/// Neither region may be accessed concurrently while this function executes.
#[no_mangle]
pub unsafe extern "C" fn stackchan_audio_profile(
    samples: *const i16, len: usize, pre: usize, threshold: i32, out: *mut Profile,
) -> bool {
    if out.is_null() || len > MAX_WINDOWS * WINDOW || (len != 0 && samples.is_null()) {
        return false;
    }
    let input = if len == 0 { &[] } else {
        unsafe { core::slice::from_raw_parts(samples, len) }
    };
    unsafe { out.write(analyze(input, pre, threshold)); }
    true
}

#[cfg(all(not(test), target_arch = "xtensa"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[cfg(all(not(test), not(target_arch = "xtensa")))]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(samples: &[i16], pre: usize, threshold: i32) -> Profile {
        let mut p = Profile { pre_peak: 0, live_peak: 0, voiced: 0, count: 0,
                              peaks: [0; MAX_WINDOWS] };
        for (i, &s) in samples.iter().enumerate() {
            let v = i32::from(s).unsigned_abs();
            if i < pre { p.pre_peak = p.pre_peak.max(v); }
            else { p.live_peak = p.live_peak.max(v); }
        }
        for (w, c) in samples.chunks(WINDOW).enumerate() {
            let peak = c.iter().map(|&s| i32::from(s).unsigned_abs()).max().unwrap();
            p.peaks[w] = peak;
            p.count += 1;
            if i64::from(peak) >= i64::from(threshold) { p.voiced += 1; }
        }
        p
    }

    #[test]
    fn matches_existing_two_pass_analysis() {
        let samples: std::vec::Vec<i16> = (0..192000)
            .map(|i| (i as u32).wrapping_mul(1664525).wrapping_add(1013904223) as i16)
            .collect();
        for len in [0, 1, 1599, 1600, 1601, 191999, 192000] {
            for pre in [0, 1, 777, 1600, 2300, 200000] {
                for threshold in [-1, 0, 5000, 32768, 32769] {
                    assert_eq!(analyze(&samples[..len], pre, threshold),
                               reference(&samples[..len], pre, threshold));
                }
            }
        }
    }

    #[test]
    fn handles_extremes_silence_and_ffi_validation() {
        assert_eq!(analyze(&[i16::MIN, i16::MAX], 1, 32768).pre_peak, 32768);
        assert_eq!(analyze(&[0; 1601], 0, 1).voiced, 0);
        let mut out = analyze(&[], 0, 1);
        unsafe {
            assert!(!stackchan_audio_profile(core::ptr::null(), 1, 0, 1, &mut out));
            assert!(!stackchan_audio_profile(core::ptr::null(), 192001, 0, 1, &mut out));
            assert!(!stackchan_audio_profile(core::ptr::null(), 0, 0, 1, core::ptr::null_mut()));
            assert!(stackchan_audio_profile(core::ptr::null(), 0, 0, 1, &mut out));
        }
    }
}
