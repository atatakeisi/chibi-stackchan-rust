//! ATOMS3R + Atomic Echo Base drivers. No Arduino/M5Unified C++ is linked.
//! ES8311/PI4IOE register sequences follow M5Stack's hardware reference:
//! https://github.com/m5stack/M5Unified (M5Unified.inl, Atomic Echo callbacks).
pub use crate::display::FaceState;
use crate::{conversation::Expression, display::Display};
use anyhow::{Context, Result};
use esp_idf_svc::{
    hal::{
        delay::TickType,
        gpio::{Input, PinDriver},
        i2c::{I2cConfig, I2cDriver},
        i2s::{
            config::{
                Config as I2sConfig, DataBitWidth, SlotMode, StdClkConfig, StdConfig,
                StdGpioConfig, StdSlotConfig,
            },
            I2sBiDir, I2sDriver,
        },
        ledc::{self, LedcDriver, LedcTimerDriver, Resolution},
        modem::Modem,
        peripherals::Peripherals,
        units::Hertz,
    },
    sys,
};
use std::{
    io::Cursor,
    time::{Duration, Instant},
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as AudioError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

const RATE: u32 = 16_000;
const I2C_TIMEOUT: u64 = 100;
// ES8311 DAC register 0x32: 0xbf = 0 dB, 0xff = +32 dB.
// Keep unity gain here; the shared PCM output path applies the user's volume.
const DAC_UNITY: u8 = 0xbf;
const FNV1A_OFFSET: u32 = 0x811c_9dc5;

pub struct Board {
    pub display: Display,
    pub expression: Expression,
    pub state: FaceState,
    bus: I2cDriver<'static>,
    i2s: I2sDriver<'static, I2sBiDir>,
    button: PinDriver<'static, Input>,
    servo: Servo,
    boot: Instant,
    dc: i32,
    mouth: f32,
    last_frame: u64,
    volume: crate::playback::VolumeControl,
    output_volume: u8,
    playback_peak: u32,
    playback_source_peak: u32,
    playback_volume_min: u8,
    playback_volume_max: u8,
    playback_frames: u64,
    playback_fingerprint: u32,
    last_playback: Option<crate::playback::PlaybackStats>,
    playback_started_us: i64,
    last_pcm_feed_us: i64,
    max_pcm_feed_gap_us: i64,
}

impl Board {
    pub fn new(
        p: Peripherals,
        volume: crate::playback::VolumeControl,
    ) -> Result<(Self, Modem<'static>)> {
        let mut internal = I2cDriver::new(
            p.i2c0,
            p.pins.gpio45,
            p.pins.gpio0,
            &I2cConfig::new().baudrate(Hertz(100_000)),
        )?;
        let mut display = Display::new(
            p.spi3,
            p.pins.gpio15,
            p.pins.gpio21,
            p.pins.gpio14,
            p.pins.gpio42,
            p.pins.gpio48,
            &mut internal,
        )?;
        display.show_status(&["CHIBI STACKCHAN", "Rust firmware", "Starting..."])?;
        let bus = I2cDriver::new(
            p.i2c1,
            p.pins.gpio38,
            p.pins.gpio39,
            &I2cConfig::new().baudrate(Hertz(100_000)),
        )?;
        // Both channels share a fixed clock. TTS is resampled in Rust, so changing
        // playback rate cannot disturb capture's 16 kHz timing or codec clocks.
        let cfg = StdConfig::new(
            I2sConfig::new()
                .dma_buffer_count(4)
                .frames_per_buffer(320)
                .auto_clear(true),
            StdClkConfig::from_sample_rate_hz(RATE),
            StdSlotConfig::philips_slot_default(DataBitWidth::Bits16, SlotMode::Stereo),
            StdGpioConfig::default(),
        );
        let mut i2s = I2sDriver::new_std_bidir(
            p.i2s0,
            &cfg,
            p.pins.gpio8,
            p.pins.gpio7,
            p.pins.gpio5,
            None::<esp_idf_svc::hal::gpio::AnyIOPin<'static>>,
            p.pins.gpio6,
        )?;
        i2s.tx_enable()?;
        i2s.rx_enable()?;
        let button = PinDriver::input(p.pins.gpio41, esp_idf_svc::hal::gpio::Pull::Up)?;
        let timer = LedcTimerDriver::new(
            p.ledc.timer0,
            &ledc::config::TimerConfig::new()
                .frequency(Hertz(50))
                .resolution(Resolution::Bits14),
        )?;
        let servo = Servo::new(LedcDriver::new(p.ledc.channel0, timer, p.pins.gpio2)?)?;
        let mut board = Self {
            display,
            bus,
            i2s,
            button,
            servo,
            expression: Expression::Neutral,
            state: FaceState::Idle,
            boot: Instant::now(),
            dc: 0,
            mouth: 0.0,
            last_frame: 0,
            volume,
            output_volume: 0,
            playback_peak: 0,
            playback_source_peak: 0,
            playback_volume_min: u8::MAX,
            playback_volume_max: 0,
            playback_frames: 0,
            playback_fingerprint: FNV1A_OFFSET,
            last_playback: None,
            playback_started_us: 0,
            last_pcm_feed_us: 0,
            max_pcm_feed_gap_us: 0,
        };
        board
            .microphone()
            .context("initializing Atomic Echo microphone")?;
        Ok((board, p.modem))
    }

    pub fn button_pressed(&self) -> bool {
        self.button.is_low()
    }

    pub fn last_playback(&self) -> Option<crate::playback::PlaybackStats> {
        self.last_playback
    }
    pub fn focus(&mut self, on: bool) {
        self.servo.focused = on;
    }
    pub fn servo_noise(&self, _now_ms: u64) -> bool {
        self.now_ms().saturating_sub(self.servo.last_move) < 450
    }
    fn now_ms(&self) -> u64 {
        self.boot.elapsed().as_millis() as u64
    }

    pub fn tick(&mut self, _now_ms: u64) -> Result<()> {
        let now = self.now_ms();
        self.servo.update(
            now,
            self.state == FaceState::Speaking,
            self.mouth,
            self.expression == Expression::Sleepy,
        )?;
        if self.state != FaceState::Setup && now.saturating_sub(self.last_frame) >= 33 {
            self.display
                .draw(self.expression, self.state, self.mouth, now)?;
            self.last_frame = now;
        }
        Ok(())
    }

    fn registers(&mut self, address: u8, registers: &[(u8, u8)]) -> Result<()> {
        for &(register, value) in registers {
            self.bus
                .write(
                    address,
                    &[register, value],
                    TickType::new_millis(I2C_TIMEOUT).ticks(),
                )
                .with_context(|| format!("I2C {address:02x} register {register:02x}"))?;
        }
        Ok(())
    }

    pub fn microphone(&mut self) -> Result<()> {
        // Disable the external amplifier before configuring the ADC. Codec power
        // up is written while BCLK runs; the ES8311 requires that ordering.
        self.registers(0x43, &[(0x03, 0xff), (0x05, 0), (0x07, 0), (0x0b, 0)])?;
        self.registers(
            0x18,
            &[
                (0x00, 0x80),
                (0x01, 0xba),
                (0x02, 0x18),
                (0x0d, 0x01),
                (0x0e, 0x02),
                (0x09, 0x0c),
                (0x0a, 0x0c),
                (0x12, 0x02),
                (0x14, 0x10),
                (0x17, 0xff),
                (0x1c, 0x6a),
            ],
        )?;
        // ES8311 can absorb SYSTEM0D while its BCLK-powered clock is settling.
        // Re-arm with a running, settled clock before the application warmup.
        std::thread::sleep(Duration::from_millis(30));
        self.registers(0x18, &[(0x0d, 0x01)])?;
        self.dc = 0;
        Ok(())
    }

    pub fn read_microphone(&mut self, output: &mut [i16; 320], gain: u8) -> Result<()> {
        let mut bytes = [0u8; 320 * 4];
        let length = self
            .i2s
            .read(&mut bytes, TickType::new_millis(200).ticks())?;
        anyhow::ensure!(
            length == bytes.len(),
            "partial I2S capture ({length} bytes)"
        );
        // ES8311 mono output is in the right I2S slot, as in M5Unified's default.
        for (out, frame) in output.iter_mut().zip(bytes.chunks_exact(4)) {
            let sample = i32::from(i16::from_le_bytes([frame[2], frame[3]]));
            self.dc += (sample * 256 - self.dc) / 64;
            let amplified = (sample - self.dc / 256) * i32::from(gain) / 2;
            *out = amplified.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
        Ok(())
    }

    fn speaker(&mut self) -> Result<()> {
        let volume = self.volume.get();
        let dac = if volume == 0 { 0 } else { DAC_UNITY };
        self.registers(
            0x18,
            &[
                (0x00, 0x80),
                (0x01, 0xb5),
                (0x02, 0x18),
                (0x0d, 0x01),
                (0x09, 0x0c),
                (0x12, 0),
                (0x13, 0x10),
                (0x32, dac),
                (0x37, 0x08),
            ],
        )?;
        self.registers(
            0x43,
            &[
                (0x03, 0xff),
                (0x05, if volume == 0 { 0 } else { 0xff }),
                (0x07, 0),
                (0x0b, 0),
            ],
        )?;
        let mut readback = [0u8];
        self.bus.write_read(
            0x18,
            &[0x32],
            &mut readback,
            TickType::new_millis(I2C_TIMEOUT).ticks(),
        )?;
        anyhow::ensure!(readback[0] == dac, "DAC volume register did not match");
        self.output_volume = volume;
        self.playback_peak = 0;
        self.playback_source_peak = 0;
        self.playback_volume_min = u8::MAX;
        self.playback_volume_max = 0;
        self.playback_frames = 0;
        self.playback_fingerprint = FNV1A_OFFSET;
        log::info!(
            "Speaker ready: volume={volume}/255 DAC=0x{dac:02x} amplifier={}",
            volume != 0
        );
        self.display.reset_timing();
        self.playback_started_us = unsafe { sys::esp_timer_get_time() };
        self.last_pcm_feed_us = 0;
        self.max_pcm_feed_gap_us = 0;
        Ok(())
    }

    fn write_pcm(&mut self, stereo: &mut [u8]) -> Result<()> {
        // Lip sync follows the decoded voice before speaker attenuation, as
        // in the original Avatar task. Quiet volume must still move the mouth.
        let voice_peak = stereo
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]).unsigned_abs())
            .max()
            .unwrap_or(0);
        let source_fingerprint = stereo.iter().fold(self.playback_fingerprint, |hash, byte| {
            (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
        });
        let (volume, amplitude) = self.volume.apply(stereo)?;
        if volume != self.output_volume {
            // Only the device task touches I2C. HTTP publishes the requested
            // volume atomically, so even an ongoing voice/beep changes here.
            if (volume == 0) != (self.output_volume == 0) {
                if volume == 0 {
                    self.registers(0x43, &[(0x05, 0)])?;
                    self.registers(0x18, &[(0x32, 0)])?;
                } else {
                    // Drain every DMA descriptor while the amplifier is still
                    // muted, so rapid mute/unmute cannot replay old loud PCM.
                    self.i2s
                        .write_all(&[0; 1280 * 4], TickType::new_millis(500).ticks())?;
                    self.registers(0x18, &[(0x32, DAC_UNITY)])?;
                    self.registers(0x43, &[(0x05, 0xff)])?;
                }
            }
            log::info!("Playback volume applied: {volume}/255");
            self.output_volume = volume;
        }
        let feed_us = unsafe { sys::esp_timer_get_time() };
        if self.last_pcm_feed_us != 0 {
            self.max_pcm_feed_gap_us = self
                .max_pcm_feed_gap_us
                .max(feed_us - self.last_pcm_feed_us);
        }
        self.last_pcm_feed_us = feed_us;
        self.i2s
            .write_all(stereo, TickType::new_millis(500).ticks())?;
        // Count only complete chunks accepted by I2S. No diagnostic state is
        // shared with HTTP while audio is running, so this path takes no lock.
        self.playback_source_peak = self.playback_source_peak.max(u32::from(voice_peak));
        self.playback_peak = self.playback_peak.max(amplitude);
        self.playback_volume_min = self.playback_volume_min.min(volume);
        self.playback_volume_max = self.playback_volume_max.max(volume);
        self.playback_frames += (stereo.len() / 4) as u64;
        self.playback_fingerprint = source_fingerprint;
        self.mouth = (f32::from(voice_peak) / 15_000.0).clamp(0.0, 1.0);
        self.tick(self.now_ms())?;
        Ok(())
    }

    pub fn play_mp3(&mut self, mp3: &[u8]) -> Result<()> {
        let previous_state = self.state;
        self.state = FaceState::Speaking;
        let result = self.play_mp3_inner(mp3);
        // Always restore ADC/amplifier state, including malformed MP3/I2S errors.
        let restore = self.finish_playback();
        self.state = previous_state;
        let stats = result.and_then(|stats| restore.map(|()| stats))?;
        // Retain the last completed MP3 across beeps or failed playback. The
        // source fingerprint excludes mute-drain and end-of-playback silence.
        self.last_playback = Some(stats);
        log::info!(
            "Playback summary: source_peak={} output_peak={} volume_min={} volume_max={} frames={} source_fingerprint={:08x}",
            stats.source_peak,
            stats.output_peak,
            stats.volume_min,
            stats.volume_max,
            stats.frames,
            stats.source_fingerprint
        );
        Ok(())
    }

    fn finish_playback(&mut self) -> Result<()> {
        // Drain TX descriptors before muting, then discard capture through the
        // normal 0.8-second VAD warmup in the application.
        let drain = self
            .i2s
            .write_all(&[0; 1280], TickType::new_millis(500).ticks());
        std::thread::sleep(Duration::from_millis(100));
        self.mouth = 0.0;
        let restore = self.microphone();
        drain?;
        restore
    }

    fn play_mp3_inner(&mut self, bytes: &[u8]) -> Result<crate::playback::PlaybackStats> {
        anyhow::ensure!(
            !bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024,
            "invalid MP3 size"
        );
        let source =
            MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        let mut format = symphonia::default::get_probe()
            .format(
                &hint,
                source,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )?
            .format;
        let track = format.default_track().context("MP3 has no audio track")?;
        let track_id = track.id;
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;
        self.speaker()?;
        let mut resampler = crate::playback::Resampler::new();
        let mut output = Vec::with_capacity(1280);
        let mut frames = 0usize;
        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(AudioError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id() != track_id {
                continue;
            }
            let decoded = decoder.decode(&packet)?;
            let spec = *decoded.spec();
            let channels = spec.channels.count();
            anyhow::ensure!(
                channels > 0 && channels <= 2 && (8000..=96000).contains(&spec.rate),
                "unsupported MP3 format"
            );
            let mut samples = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
            samples.copy_interleaved_ref(decoded);
            for scaled in resampler.push(samples.samples(), channels, spec.rate, 255)? {
                output.extend_from_slice(&scaled.to_le_bytes());
                output.extend_from_slice(&scaled.to_le_bytes());
                frames += 1;
                anyhow::ensure!(
                    frames <= RATE as usize * 180,
                    "MP3 exceeds three-minute playback limit"
                );
                if output.len() == 1280 {
                    self.write_pcm(&mut output)?;
                    output.clear();
                }
            }
        }
        anyhow::ensure!(frames > 0, "MP3 contains no decoded samples");
        if !output.is_empty() {
            self.write_pcm(&mut output)?;
        }
        let elapsed_us = unsafe { sys::esp_timer_get_time() } - self.playback_started_us;
        let timing = self.display.timing();
        log::info!(
            "MP3 playback complete: {frames} samples at {RATE} Hz, PCM peak={}, volume={}/255",
            self.playback_peak,
            self.output_volume
        );
        log::info!(
            "Playback timing: expected_ms={} elapsed_ms={} max_feed_gap_us={} face_frames={} render_max_us={} transfer_max_us={}",
            frames * 1_000 / RATE as usize,
            elapsed_us / 1_000,
            self.max_pcm_feed_gap_us,
            timing.frames,
            timing.max_render_us,
            timing.max_transfer_us
        );
        Ok(crate::playback::PlaybackStats {
            source_peak: self.playback_source_peak,
            output_peak: self.playback_peak,
            volume_min: self.playback_volume_min,
            volume_max: self.playback_volume_max,
            frames: self.playback_frames,
            source_fingerprint: self.playback_fingerprint,
        })
    }

    pub fn beep(&mut self) -> Result<()> {
        let previous_state = self.state;
        self.state = FaceState::Speaking;
        let result = (|| -> Result<()> {
            self.speaker()?;
            let mut bytes = Vec::with_capacity(1280);
            for chunk in 0..20 {
                bytes.clear();
                for offset in 0..320 {
                    let t = chunk * 320 + offset;
                    let value: i16 = if chunk < 6 || (10..16).contains(&chunk) {
                        (2500.0 * (t as f32 * 880.0 * std::f32::consts::TAU / RATE as f32).sin())
                            as i16
                    } else {
                        0
                    };
                    bytes.extend_from_slice(&value.to_le_bytes());
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                self.write_pcm(&mut bytes)?;
            }
            Ok(())
        })();
        let restore = self.finish_playback();
        self.state = previous_state;
        result.and(restore)
    }
}

struct Servo {
    pwm: LedcDriver<'static>,
    degree: f32,
    phase: f32,
    focused: bool,
    enabled: bool,
    last_tick: u64,
    last_move: u64,
    idle_since: u64,
    next_glance: u64,
    target: Option<f32>,
    hold_until: u64,
    returning: bool,
}

impl Servo {
    fn new(pwm: LedcDriver<'static>) -> Result<Self> {
        let mut result = Self {
            pwm,
            degree: 90.0,
            phase: 0.0,
            focused: false,
            enabled: false,
            last_tick: 0,
            last_move: 0,
            idle_since: 0,
            next_glance: 15_000,
            target: None,
            hold_until: 0,
            returning: false,
        };
        result.position(90.0, 0)?;
        Ok(result)
    }
    fn position(&mut self, degree: f32, now: u64) -> Result<()> {
        let degree = degree.clamp(40.0, 140.0);
        if !self.enabled || (degree - self.degree).abs() > 0.2 {
            self.last_move = now;
        }
        self.degree = degree;
        self.enabled = true;
        let micros = 500 + (1900.0 * degree / 180.0) as u32;
        self.pwm
            .set_duty(micros * self.pwm.get_max_duty() / 20_000)?;
        Ok(())
    }
    fn detach(&mut self, now: u64) -> Result<()> {
        if self.enabled {
            self.pwm.disable()?;
            self.enabled = false;
            self.last_move = now;
        }
        Ok(())
    }
    fn approach(&mut self, target: f32, speed: f32, dt: f32, now: u64) -> Result<bool> {
        let difference = target - self.degree;
        let step = speed * dt;
        self.position(self.degree + difference.clamp(-step, step), now)?;
        Ok(difference.abs() <= step)
    }
    fn update(&mut self, now: u64, talking: bool, level: f32, sleeping: bool) -> Result<()> {
        let dt = (now.saturating_sub(self.last_tick) as f32 / 1000.0).min(0.1);
        self.last_tick = now;
        if talking {
            self.phase += std::f32::consts::TAU * 0.45 * (0.8 + 0.6 * level) * dt;
            self.position(90.0 + 28.0 * (0.55 + 0.45 * level) * self.phase.sin(), now)?;
            self.idle_since = now;
            self.next_glance = now + 15_000;
            self.target = None;
        } else if self.focused || sleeping {
            self.target = None;
            if (self.degree - 90.0).abs() > 0.5 {
                self.approach(90.0, 60.0, dt, now)?;
                self.idle_since = now;
            } else if now.saturating_sub(self.idle_since) > 400 {
                self.detach(now)?;
            }
        } else if let Some(target) = self.target {
            if self.returning {
                if self.approach(90.0, 25.0, dt, now)? {
                    self.target = None;
                    self.idle_since = now;
                    self.next_glance =
                        now + 12_000 + u64::from(unsafe { sys::esp_random() } % 18_001);
                }
            } else if self.hold_until > 0 {
                if now >= self.hold_until {
                    self.returning = true;
                    self.hold_until = 0;
                }
            } else if self.approach(target, 25.0, dt, now)? {
                self.hold_until = now + 1000;
            }
        } else if now >= self.next_glance {
            let random = unsafe { sys::esp_random() };
            self.target = Some(
                90.0 + if random & 1 == 0 { 1.0 } else { -1.0 } * (6 + (random / 2) % 13) as f32,
            );
            self.returning = false;
            self.hold_until = 0;
        } else if (self.degree - 90.0).abs() > 0.5 {
            self.approach(90.0, 25.0, dt, now)?;
            self.idle_since = now;
        } else if now.saturating_sub(self.idle_since) > 1500 {
            self.detach(now)?;
        }
        Ok(())
    }
}
