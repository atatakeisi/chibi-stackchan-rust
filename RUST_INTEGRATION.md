# Historical C++ / Rust audio integration

The full Rust application firmware is documented in
[rust/firmware/README.md](rust/firmware/README.md). Build it with
`python3 scripts/firmware.py build` from the repository root.

## Legacy source checkpoint and completed cleanup

The C++ and mixed C++/Rust sources are preserved in the immutable
[pre-cleanup checkpoint `ffb5c747`](https://github.com/atatakeisi/chibi-stackchan-rust/tree/ffb5c74706a106e09211d558a028f2fccbaac537).
The original C++ project is independently preserved at
[`atatakeisi/chibi-stackchan`, commit `10460b3`](https://github.com/atatakeisi/chibi-stackchan/tree/10460b3bdd97fd59e05dcc9f69395839ea51a8fb).

After explicit user approval, the old root `src/`, PlatformIO configuration, and
`rust/audio-core/` were removed from the active Rust project. Their source,
build instructions, licenses, and measurements remain available at the checkpoint;
this document retains the historical results below. Commands mentioning
`rust/audio-core`, `include/secrets.h`, `atoms3r`, or `pio` describe that checkpoint,
and are not build instructions for the Rust-only application after cleanup.

The active local crates are `rust/firmware` and its face-rendering dependency
`rust/avatar-face`. See [the migration map](docs/RUST_MIGRATION.md) for the
C++-to-Rust module correspondence and the scope of the cleanup.

## Rust application validation record

The later sections retain the earlier Arduino C++ / Rust audio-library uploads,
measurements, and hardware observations; those results apply only to the mixed
firmware. The following record describes the full Rust application separately.
The new Rust application has passed 86 firmware host tests,
12 independent Avatar face-library tests, and an
ESP32-S3 release build, and has been flashed to app0 at `0x10000` with the write
hash verified. A private full-flash backup was made first; the existing bootloader,
partition table, NVS, and SPIFFS were retained. Hardware checks have confirmed
Wi-Fi/DHCP, ST7735S detection, VOICEVOX HTTPS audio retrieval, complete MP3 playback
(77,568 samples at 16 kHz), microphone recovery and standby, and HTTP 200 from `/`,
`/config`, and `/role`. The user confirmed audible speech and silence when muted.
Excessive output gain was traced to the ES8311 DAC's +32 dB setting and corrected
to 0 dB. Shared volume control now applies changes to speech and beeps during
playback, and volume 0 explicitly disables the amplifier. Device logs verified
muted startup with zero PCM output, DAC register readback at 0 dB, and live volume
changes of 5 → 2 → 0 → 5 within the same speech playback. Earlier command tests
restored volume to 10 / 255. The user subsequently changed it; both the latest
previous wake-prefix test readbacks were 15 / 255 with VOICEVOX speaker 54;
the latest numeric-homophone test preserved the subsequently selected 17 / 255.
The user confirmed that the corrected
listening level is acceptable and, on 2026-10-17, confirmed settings changes and device
operation through the iPhone Web interface. The Rust source and build configuration
are saved directly in the desktop `chibi-stackchan-rust-main` project.
The original M5Stack-Avatar white eyes, rectangular mouth and expression geometry
have been ported into the independent [stackchan-avatar-face](rust/avatar-face/README.md)
crate, alongside blink, gaze, breath and tilt animation. The LCD driver delegates
face rendering to this crate; the added colored eyes, cheeks and normal-screen
labels have been removed. The updated application was flashed and its renderer
startup log, complete speech playback, microphone recovery, standby, and Web
responses were verified again with the saved volume unchanged at 10 / 255.
A [six-expression preview](docs/images/rust-avatar-face-preview.png)
shows the port. The user confirmed on the physical device that the original face has been restored.
Raspy playback after that face update was traced to delayed PCM feeding during
face rendering and polling SPI transfers. Bounded rendering preserves the previous
output pixels across 512 comparison cases, and SPI now avoids polling. The same
4,848 ms audio took 4,821 ms to process (previously 6,270 ms), with a maximum PCM
feed interval of 34,811 µs and no task-watchdog warning during that boot/playback.
The user confirmed that the voice sounds natural again. Saved volume 10 / 255 and
VOICEVOX speaker 54 were unchanged. These timings are a single playback measurement.
The full Rust application also handles recognized volume commands locally before
the conversational LLM: relative changes, explicit 0–255 values or percentages,
mute, restoration of the last nonzero level, and status queries. The current and
restore levels are persisted, including Web changes. Voice requests still use the
normal wake-word/button gate and cloud speech recognition. Muting leaves microphone
input active and skips silent TTS acknowledgements. See the firmware README's
[voice-volume instructions](rust/firmware/README.md#音声で音量を変える) for examples.
On the physical device, `/chat` exercised the same post-transcription command path:
10 → 8 → 0 → 8 → 10, with saved readback, unchanged status queries, and rejection
of the out-of-range request "音量300". That test baseline was volume 10 / 255
and VOICEVOX speaker 54. The user later reported unreliable microphone volume commands.
The updated parser handles configured wake prefixes in manual/followup/chat
paths and filler/polite forms such as "スタックちゃん音量下げて" and "おーい、ミュートして".
These changes have been flashed. Device `/chat` tests verified the exact phrases:
"スタックちゃん音量下げて" changed 15 → 13 and "おーい、ミュートして" changed 13 → 0;
unmute restored 13 and an absolute request restored 15. Saved/live/PCM values agreed.
For identical source PCM (26,496 frames, peak 27,554, fingerprint `0x860d4b01`),
volume 5 produced output peak 540 and volume 15 produced 1,620, exactly three times
as large. This verifies digital attenuation, not measured acoustic loudness.
The user subsequently confirmed mute/unmute but reported that numeric setting
("おーい音量20にして") and volume-up ("スタックちゃん音量上げて") fell through to ordinary AI
conversation without changing the device. Further changes handle commas after
the volume noun, kana spellings of volume and configured wake phrases, and
validated AI volume-action tags as a fallback. Those tags use the same persisted
device control before acknowledgement, rather than treating an AI reply as evidence
that a setting changed. Real microphone input subsequently changed volume 17 → 15 → 17, and the user
confirmed both spoken acknowledgements. The user reported that this small gain
difference was difficult to hear. Spoken volume 20 was actually transcribed as
"大い・音量二重にして"; the latest flashed firmware corrects that numeric homophone.
Replaying this exact transcription through `/chat` verified saved/live/PCM volume
20, then restored the prior 17. A new microphone test produced "大い、音量20にして", applied volume20 locally,
and the user confirmed hearing "音量20にしたよ". The subsequent microphone command
"おーい、ミュートして" applied0; that user-selected mute was preserved.
The AI action fallback also changed 20 → 18 for a noncanonical softer-volume request.
The new read-only `/status` exposes saved/live/restore volume and numeric statistics
from the last completed MP3 playback: pre/post-attenuation peaks, minimum/maximum
volume, frames and a pre-attenuation PCM FNV-1a fingerprint. It contains no credentials,
transcripts or spoken text, and collecting it does not change output gain.
Volume-command diagnosis uses USB/local logs only: volume-related recognition,
post-wake command text and routing are bounded to 256 characters. No recorded audio
or NVS trace is retained, and no Web endpoint exposes speech transcripts.
The previous boot speech took 4,843 ms for 4,848 ms of
audio, with a maximum PCM feed interval of 30,372 µs and no task-watchdog warning.
The previous wake-prefix build's boot speech took 4,824 ms for 4,848 ms of audio, with a maximum PCM feed
interval of 34,526 µs.
The user confirmed speaking "スタックちゃんタイマー1分" to set a timer and hearing its
spoken alarm after one minute, before the current volume-command fix. The previous wake-prefix
firmware also accepted that exact text through `/chat` and, after approximately
one minute, completed its spoken alarm (46,848 frames at volume 15) and restored
the microphone. That test ended at volume 15. The latest numeric-homophone test
restored saved/live volume 17 / 255 with speaker 54 unchanged. The user subsequently
confirmed mute and unmute, numeric volumes 30, 20, and 5, normal conversational
responses, weather for the configured location, and iPhone control. Volume 5 was
confirmed to be very quiet and acceptable; earlier restored test values do not
describe a fixed or enforced volume.
Physical servo movement remains unverified.
See the full Rust firmware README for its
current validation status; the historical measurements below remain separate.

At the historical checkpoint, `atoms3r` contains the original C++ build and the
PlatformIO default `atoms3r-rust` links a no_std Rust static library into the
Arduino firmware. In that mixed build, hardware drivers, Wi-Fi, credentials,
and AI service clients are C++. Those environments belong to the checkpoint,
not the active Rust application described above.

## Historical mixed-firmware scope

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

## Historical host verification (checkpoint checkout only)

```
cargo test --manifest-path rust/audio-core/Cargo.toml --offline
```

Tests compare against the old two-pass algorithm across buffer/window/preroll
boundaries, thresholds, empty input and full 12-second input. Additional tests
cover -32768, silence and FFI argument rejection. The C ABI requires a valid,
aligned, nonoverlapping buffer that is no longer being recorded into.

## Historical device prerequisites and build (checkpoint checkout only)

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

## Historical device validation recorded

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
