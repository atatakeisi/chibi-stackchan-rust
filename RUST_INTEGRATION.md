# Rust audio integration (experimental)

The original `atoms3r` environment remains available. The default `atoms3r-rust`
environment links a no_std Rust static library into the Arduino firmware.
Hardware drivers, Wi-Fi, credentials, and AI service clients remain in C++.

## Scope

Recorded 16 kHz mono PCM is analyzed once to compute preroll/live peaks,
100 ms window peaks, and the voiced-window count. The original code scans it
twice. This reduces memory passes; it alone is not evidence of an end-to-end latency improvement.
Network calls and recording silence detection are unchanged.

For initial device validation, the `atoms3r-rust-check` environment ALSO executes the original
C++ analysis and prints `[AUDIO CHECK] match=yes cpp_us=... rust_us=...`.
This validation overhead is intentional: do not claim an overall speedup with
it enabled. Comparisons exclude serial logging, but are single measurements,
not controlled benchmarks (cache/task scheduling can affect results).
A mismatch uses the C++ result. Invalid Rust input falls back to original C++.
`[RUST AUDIO]` confirms that the Rust call ran successfully.

## Host verification

```
cargo test --manifest-path rust/audio-core/Cargo.toml --offline
```

Tests compare against the old two-pass algorithm across buffer/window/preroll
boundaries, thresholds, empty input and full 12-second input. Additional tests
cover -32768, silence and FFI argument rejection. The C ABI requires a valid,
aligned, nonoverlapping buffer that is no longer being recorded into.

## Device prerequisites and build

ESP32-S3 requires the Espressif Xtensa Rust toolchain named `esp`, with Rust
sources. Standard desktop stable Rust is insufficient. Install via espup
according to https://docs.espressif.com/projects/rust/book/getting-started/toolchain.html.
Do not change shell environment variables without the owner's permission.
The build invokes `rustup run esp cargo` explicitly, without switching the
user's default Rust toolchain or sourcing an export script.

Create the ignored `include/secrets.h` using the example, leaving empty
credentials when configuring via the device portal. Do not commit secrets.

```
pio run -e atoms3r-rust
```

Connect ATOMS3R by a data-capable USB cable, identify its port, then upload the
application firmware (not the filesystem). Uploading replaces the application;
verify the intended port and firmware first.

```
pio run -e atoms3r-rust -t upload --upload-port PORT
pio device monitor --port PORT --baud 115200
```

Verify boot, microphone/Whisper, response playback and servo movement, and
collect AUDIO CHECK logs from silence, short and long utterances. These checks
have not been performed until explicitly recorded. Original C++ build:

```
pio run -e atoms3r
```

## Device validation recorded

- Desktop Rust tests passed (2 tests, including differential boundary cases).
- Original C++ and Rust-enabled firmware builds passed.
- Rust-enabled firmware uploaded to ESP32-S3 on `/dev/cu.usbmodem21101`.
- Actual speech recording: 124000 samples; original C++ and Rust profiles matched.
- One measurement: C++ 22120 us; Rust 18563 us (3557 us difference).
  This is not a statistically established benchmark or end-to-end speedup.
- Whisper recognized the weather request; the AI and TTS synthesis returned HTTP 200.
- The user confirmed the usual audible response. Physical servo motion was not explicitly confirmed.
- Normal `atoms3r-rust` runs only Rust analysis; `atoms3r-rust-check` runs both.
- Final normal firmware (comparison disabled) built and uploaded successfully.
- Final firmware reset and printed `[RUST AUDIO] boot self-test=PASS` on the device.
- Existing NVS microphone gain, AI provider and wake settings were read at boot.
