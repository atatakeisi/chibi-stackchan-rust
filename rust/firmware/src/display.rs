//! ATOMS3R LCD and face rendering, entirely in Rust.
//!
//! Panel detection and board offsets follow M5GFX `M5GFX.cpp` (ATOMS3R),
//! with init register values from LovyanGFX `Panel_GC9A01.hpp` (GC9107)
//! and `Panel_ST7735.hpp` (ST7735S). The ASCII font is Adafruit's classic
//! 5x7 font (`lgfx/Fonts/glcdfont.h`). Retained third-party notices below.
/*
Copyright (c) 2021 M5Stack
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Software License Agreement (FreeBSD/BSD License)
Copyright (c) 2020 lovyan03 (https://github.com/lovyan03)
Copyright (c) 2012 Adafruit Industries. All rights reserved.
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:
1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
The views and conclusions contained in the software and documentation are
those of the authors and should not be interpreted as representing official
policies, either expressed or implied, of the FreeBSD Project.
*/

use anyhow::{bail, Context, Result};
use esp_idf_svc::{
    hal::{
        delay::{Ets, FreeRtos, BLOCK},
        gpio::{self, Output, PinDriver, Pull},
        i2c::I2cDriver,
        spi::{self, SpiBusDriver, SpiDriver},
        units::Hertz,
    },
    sys,
};

use crate::conversation::Expression;

const WIDTH: usize = 128;
const HEIGHT: usize = 128;
const WHITE: u16 = 0xffff;
const BLACK: u16 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceState {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Setup,
}

#[derive(Clone, Copy, Debug)]
enum Panel {
    Gc9107,
    St7735s,
}

#[derive(Clone, Copy, Default)]
pub struct FrameTiming {
    pub frames: u32,
    pub max_render_us: u64,
    pub max_transfer_us: u64,
}

impl Panel {
    fn identify(bytes: [u8; 4]) -> Option<Self> {
        // Bus_SPI::readData(32) in M5GFX returns the little-endian receive
        // register. Interpret the wire bytes identically, rather than guessing
        // a controller when it returns all zeroes/all ones.
        let id = u32::from_le_bytes(bytes);
        if id & 0x00ff_ffff == 0x0007_9100 {
            Some(Self::Gc9107)
        } else if matches!(id & 0xffff, 0x7683 | 0x897c) {
            Some(Self::St7735s)
        } else {
            None
        }
    }

    fn offset(self) -> (u16, u16) {
        match self {
            Self::Gc9107 => (0, 32),
            // ST7735S memory=132x132, panel=128x128, configured offset=(2,1),
            // internal rotation=2: effective y offset is 132-(128+1)=3.
            Self::St7735s => (2, 3),
        }
    }
}

pub struct Display {
    bus: SpiBusDriver<'static, SpiDriver<'static>>,
    cs: PinDriver<'static, Output>,
    dc: PinDriver<'static, Output>,
    _reset: PinDriver<'static, Output>,
    panel: Panel,
    frame: Vec<u8>,
    rotated_frame: Vec<u8>,
    rotation: u8,
    last_draw: Option<(u64, Expression, FaceState)>,
    animator: stackchan_avatar_face::animation::Animator,
    timing: FrameTiming,
}

impl Display {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        spi: spi::SPI3<'static>,
        mut sclk: gpio::Gpio15<'static>,
        mut mosi: gpio::Gpio21<'static>,
        cs: gpio::Gpio14<'static>,
        dc: gpio::Gpio42<'static>,
        rst: gpio::Gpio48<'static>,
        internal_i2c: &mut I2cDriver<'_>,
    ) -> Result<Self> {
        let mut cs = PinDriver::output(cs)?;
        let mut dc = PinDriver::output(dc)?;
        let mut reset = PinDriver::output(rst)?;
        cs.set_high()?;
        dc.set_high()?;
        reset.set_low()?;
        FreeRtos::delay_ms(10);
        reset.set_high()?;
        FreeRtos::delay_ms(120);

        // Safety: these short-lived reborrows are dropped by probe_panel
        // before the original tokens are consumed by the hardware SPI driver.
        let panel = probe_panel(&mut sclk, &mut mosi, &mut cs, &mut dc)?;
        log::info!("ATOMS3R LCD detected: {panel:?}");
        let driver = SpiDriver::new(
            spi,
            sclk,
            mosi,
            gpio::AnyInputPin::none(),
            &spi::config::DriverConfig::new().dma(spi::Dma::Auto(4096)),
        )?;
        let bus = SpiBusDriver::new(
            driver,
            &spi::config::Config::new()
                .baudrate(Hertz(40_000_000))
                .polling(false)
                .write_only(true),
        )?;

        // LP5562 backlight, on the board's internal I2C0 bus (SDA45/SCL0).
        for (register, value) in [(0x00, 0x40), (0x08, 0x01), (0x70, 0x00), (0x0e, 0x00)] {
            internal_i2c
                .write(0x30, &[register, value], BLOCK)
                .context("ATOMS3R LCD backlight controller (0x30)")?;
            if register == 0x00 {
                FreeRtos::delay_ms(1);
            }
        }
        let mut display = Self {
            bus,
            cs,
            dc,
            _reset: reset,
            panel,
            frame: vec![0; WIDTH * HEIGHT * 2],
            rotated_frame: vec![0; WIDTH * HEIGHT * 2],
            rotation: 0,
            last_draw: None,
            animator: stackchan_avatar_face::animation::Animator::new(unsafe { sys::esp_random() }),
            timing: FrameTiming::default(),
        };
        display.initialize()?;
        log::info!("Face renderer: M5Stack-Avatar Rust port (128x128, scale 0.4)");
        display.show_status(&["CHIBI STACKCHAN", "RUST FIRMWARE", "Starting..."])?;
        internal_i2c.write(0x30, &[0x0e, 180], BLOCK)?;
        Ok(display)
    }

    fn initialize(&mut self) -> Result<()> {
        match self.panel {
            Panel::Gc9107 => self.init_sequence(GC9107_INIT)?,
            Panel::St7735s => self.init_sequence(ST7735S_INIT)?,
        }
        self.command(0x3a, &[0x55])?; // RGB565
        self.command(
            0x36,
            &[match self.panel {
                Panel::Gc9107 => 0x08,
                Panel::St7735s => 0xdc,
            }],
        )?;
        self.command(
            match self.panel {
                Panel::Gc9107 => 0x20,
                Panel::St7735s => 0x21,
            },
            &[],
        )?;
        Ok(())
    }

    fn init_sequence(&mut self, sequence: &[(u8, &[u8], u32)]) -> Result<()> {
        for &(command, data, delay_ms) in sequence {
            self.command(command, data)?;
            if delay_ms > 0 {
                FreeRtos::delay_ms(delay_ms);
            }
        }
        Ok(())
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Result<()> {
        self.cs.set_low()?;
        let result: Result<()> = (|| {
            self.dc.set_low()?;
            self.bus.write(&[command])?;
            self.dc.set_high()?;
            if !data.is_empty() {
                self.bus.write(data)?;
            }
            Ok(())
        })();
        let release = self.cs.set_high();
        result?;
        release?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.rotation != 0 {
            crate::orientation::rotate_frame(
                &self.frame,
                &mut self.rotated_frame,
                WIDTH,
                self.rotation,
            );
        }
        let (x, y) = self.panel.offset();
        self.command(0x2a, &[0, x as u8, 0, (x + 127) as u8])?;
        self.command(0x2b, &[0, y as u8, 0, (y + 127) as u8])?;
        self.cs.set_low()?;
        let result: Result<()> = (|| {
            self.dc.set_low()?;
            self.bus.write(&[0x2c])?;
            self.dc.set_high()?;
            self.bus.write(if self.rotation == 0 {
                &self.frame
            } else {
                &self.rotated_frame
            })?;
            Ok(())
        })();
        let release = self.cs.set_high();
        result?;
        release?;
        Ok(())
    }

    pub fn set_rotation(&mut self, rotation: u8) {
        if self.rotation != rotation {
            self.rotation = rotation;
            self.last_draw = None;
        }
    }

    /// Refresh static setup text after rotation; animated faces flush in draw.
    pub fn refresh(&mut self) -> Result<()> {
        self.flush()
    }

    pub fn draw(
        &mut self,
        expression: Expression,
        state: FaceState,
        mouth: f32,
        now_ms: u64,
    ) -> Result<()> {
        if self
            .last_draw
            .is_some_and(|(last, old_expression, old_state)| {
                old_expression == expression
                    && old_state == state
                    && now_ms.saturating_sub(last) < 33
            })
        {
            return Ok(());
        }
        let background = match state {
            FaceState::Listening => 0x03e0, // Original TFT_DARKGREEN.
            FaceState::Thinking => 0x000c,  // Original RGB565(0, 0, 96).
            _ => BLACK,
        };
        let render_start = unsafe { sys::esp_timer_get_time() };
        let params = self.animator.frame(now_ms, expression, mouth);
        stackchan_avatar_face::render_rgb565_be(
            &mut self.frame,
            WIDTH,
            HEIGHT,
            params,
            background,
        )?;
        let transfer_start = unsafe { sys::esp_timer_get_time() };
        self.flush()?;
        let transfer_end = unsafe { sys::esp_timer_get_time() };
        self.timing.frames += 1;
        self.timing.max_render_us = self
            .timing
            .max_render_us
            .max((transfer_start - render_start) as u64);
        self.timing.max_transfer_us = self
            .timing
            .max_transfer_us
            .max((transfer_end - transfer_start) as u64);
        self.last_draw = Some((now_ms, expression, state));
        Ok(())
    }

    pub fn reset_timing(&mut self) {
        self.timing = FrameTiming::default();
    }

    pub fn timing(&self) -> FrameTiming {
        self.timing
    }

    /// ASCII text preserves upper/lower case for AP credentials and IPs.
    /// Lines wrap at 20 columns; long URLs/SSIDs remain legible on the panel.
    pub fn show_status(&mut self, lines: &[&str]) -> Result<()> {
        self.clear(BLACK);
        let mut y = 5;
        for line in lines {
            let mut column = 0;
            for character in line.chars() {
                if character == '\n' || column == 20 {
                    column = 0;
                    y += 10;
                    if character == '\n' {
                        continue;
                    }
                }
                if y > 118 {
                    break;
                }
                let character = if character.is_ascii() && !character.is_control() {
                    character
                } else {
                    '?'
                };
                self.glyph(4 + column * 6, y, character, 1, WHITE);
                column += 1;
            }
            y += 11;
            if y > 118 {
                break;
            }
        }
        self.last_draw = None;
        self.flush()
    }

    fn clear(&mut self, color: u16) {
        let [high, low] = color.to_be_bytes();
        for pixel in self.frame.chunks_exact_mut(2) {
            pixel[0] = high;
            pixel[1] = low;
        }
    }

    fn pixel(&mut self, x: i32, y: i32, color: u16) {
        if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
            return;
        }
        let offset = (y as usize * WIDTH + x as usize) * 2;
        self.frame[offset..offset + 2].copy_from_slice(&color.to_be_bytes());
    }

    fn glyph(&mut self, x: i32, y: i32, character: char, scale: i32, color: u16) {
        let code = if (' '..='~').contains(&character) {
            character as usize
        } else {
            b'?' as usize
        };
        for column in 0..5 {
            let bits = ASCII_FONT[code - 32][column];
            for row in 0..8 {
                if bits & (1 << row) != 0 {
                    for oy in 0..scale {
                        for ox in 0..scale {
                            self.pixel(x + column as i32 * scale + ox, y + row * scale + oy, color);
                        }
                    }
                }
            }
        }
    }
}

fn probe_panel(
    sclk: &mut gpio::Gpio15<'static>,
    mosi: &mut gpio::Gpio21<'static>,
    cs: &mut PinDriver<'static, Output>,
    dc: &mut PinDriver<'static, Output>,
) -> Result<Panel> {
    // The reborrowed GPIO drivers are always dropped, including on error.
    // Their drop resets the pin mux before SpiDriver takes the original tokens.
    let mut clock = PinDriver::output(unsafe { sclk.reborrow() })?;
    let mut data = PinDriver::input_output(unsafe { mosi.reborrow() }, Pull::Floating)?;
    clock.set_low()?;
    let mut observations = Vec::new();
    for half_period_us in [0, 5] {
        for command in [0x04, 0xd3] {
            let bytes = read_id(&mut clock, &mut data, cs, dc, command, half_period_us)?;
            observations.push(format!(
                "cmd={command:02x} half_us={half_period_us}: {:08x}",
                u32::from_le_bytes(bytes)
            ));
            if let Some(panel) = Panel::identify(bytes) {
                return Ok(panel);
            }
        }
    }
    bail!("Unknown ATOMS3R LCD panel ID ({}). Expected GC9107 079100 or ST7735S 7683/897c; check LCD wiring/power.", observations.join(", "))
}

fn read_id(
    clock: &mut PinDriver<'_, Output>,
    data: &mut PinDriver<'_, gpio::InputOutput>,
    cs: &mut PinDriver<'static, Output>,
    dc: &mut PinDriver<'static, Output>,
    command: u8,
    half_period_us: u32,
) -> Result<[u8; 4]> {
    let delay = || {
        if half_period_us > 0 {
            Ets::delay_us(half_period_us);
        }
    };
    cs.set_high()?;
    clock.set_low()?;
    dc.set_low()?;
    // Safety: the exclusive Gpio21 reborrow is held by `data` throughout this
    // call. Only its output-enable is changed for the 3-wire turnaround.
    sys::esp!(unsafe { sys::gpio_set_direction(21, sys::gpio_mode_t_GPIO_MODE_OUTPUT) })?;
    cs.set_low()?;
    let result: Result<[u8; 4]> = (|| {
        for bit in (0..8).rev() {
            data.set_level(if command & (1 << bit) != 0 {
                gpio::Level::High
            } else {
                gpio::Level::Low
            })?;
            delay();
            clock.set_high()?;
            delay();
            clock.set_low()?;
        }
        dc.set_high()?;
        sys::esp!(unsafe { sys::gpio_set_direction(21, sys::gpio_mode_t_GPIO_MODE_INPUT) })?;
        // RDDID has one dummy bit, matching M5GFX::_read_panel_id.
        delay();
        clock.set_high()?;
        delay();
        clock.set_low()?;
        let mut bytes = [0u8; 4];
        for byte in &mut bytes {
            for _ in 0..8 {
                delay();
                clock.set_high()?;
                delay();
                *byte = (*byte << 1) | u8::from(data.is_high());
                clock.set_low()?;
            }
        }
        Ok(bytes)
    })();
    let release = cs.set_high();
    result.and_then(|bytes| {
        release?;
        Ok(bytes)
    })
}

// Register tables adapted from the panel-specific M5GFX/LovyanGFX sources
// cited above. Tuples are (command, argument bytes, delay milliseconds).
const GC9107_INIT: &[(u8, &[u8], u32)] = &[
    (0xfe, &[], 5),
    (0xef, &[], 5),
    (0xb0, &[0xc0], 0),
    (0xb2, &[0x2f], 0),
    (0xb3, &[0x03], 0),
    (0xb6, &[0x19], 0),
    (0xb7, &[0x01], 0),
    (0xac, &[0xcb], 0),
    (0xab, &[0x0e], 0),
    (0xb4, &[0x04], 0),
    (0xa8, &[0x19], 0),
    (0xb8, &[0x08], 0),
    (0xe8, &[0x24], 0),
    (0xe9, &[0x48], 0),
    (0xea, &[0x22], 0),
    (0xc6, &[0x30], 0),
    (0xc7, &[0x18], 0),
    (
        0xf0,
        &[
            0x01, 0x2b, 0x23, 0x3c, 0xb7, 0x12, 0x17, 0x60, 0x00, 0x06, 0x0c, 0x17, 0x12, 0x1f,
        ],
        0,
    ),
    (
        0xf1,
        &[
            0x05, 0x2e, 0x2d, 0x44, 0xd6, 0x15, 0x17, 0xa0, 0x02, 0x0d, 0x0d, 0x1a, 0x18, 0x1f,
        ],
        0,
    ),
    (0x11, &[], 120),
    (0x29, &[], 0),
];

const ST7735S_INIT: &[(u8, &[u8], u32)] = &[
    (0x01, &[], 150),
    (0x11, &[], 500),
    (0xb1, &[0x01, 0x2c, 0x2d], 0),
    (0xb2, &[0x01, 0x2c, 0x2d], 0),
    (0xb3, &[0x01, 0x2c, 0x2d, 0x01, 0x2c, 0x2d], 0),
    (0xb4, &[0x07], 0),
    (0xc0, &[0xa2, 0x02, 0x84], 0),
    (0xc1, &[0xc5], 0),
    (0xc2, &[0x0a, 0x00], 0),
    (0xc3, &[0x8a, 0x2a], 0),
    (0xc4, &[0x8a, 0xee], 0),
    (0xc5, &[0x0e], 0),
    (
        0xe0,
        &[
            0x02, 0x1c, 0x07, 0x12, 0x37, 0x32, 0x29, 0x2d, 0x29, 0x25, 0x2b, 0x39, 0x00, 0x01,
            0x03, 0x10,
        ],
        0,
    ),
    (
        0xe1,
        &[
            0x03, 0x1d, 0x07, 0x06, 0x2e, 0x2c, 0x29, 0x2d, 0x2e, 0x2e, 0x37, 0x3f, 0x00, 0x00,
            0x02, 0x10,
        ],
        0,
    ),
    (0x13, &[], 10),
    (0x29, &[], 100),
];

// Printable ASCII 0x20..0x7e, five vertical columns per glyph.
const ASCII_FONT: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00],
    [0x00, 0x00, 0x5F, 0x00, 0x00],
    [0x00, 0x07, 0x00, 0x07, 0x00],
    [0x14, 0x7F, 0x14, 0x7F, 0x14],
    [0x24, 0x2A, 0x7F, 0x2A, 0x12],
    [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x56, 0x20, 0x50],
    [0x00, 0x08, 0x07, 0x03, 0x00],
    [0x00, 0x1C, 0x22, 0x41, 0x00],
    [0x00, 0x41, 0x22, 0x1C, 0x00],
    [0x2A, 0x1C, 0x7F, 0x1C, 0x2A],
    [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x80, 0x70, 0x30, 0x00],
    [0x08, 0x08, 0x08, 0x08, 0x08],
    [0x00, 0x00, 0x60, 0x60, 0x00],
    [0x20, 0x10, 0x08, 0x04, 0x02],
    [0x3E, 0x51, 0x49, 0x45, 0x3E],
    [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x72, 0x49, 0x49, 0x49, 0x46],
    [0x21, 0x41, 0x49, 0x4D, 0x33],
    [0x18, 0x14, 0x12, 0x7F, 0x10],
    [0x27, 0x45, 0x45, 0x45, 0x39],
    [0x3C, 0x4A, 0x49, 0x49, 0x31],
    [0x41, 0x21, 0x11, 0x09, 0x07],
    [0x36, 0x49, 0x49, 0x49, 0x36],
    [0x46, 0x49, 0x49, 0x29, 0x1E],
    [0x00, 0x00, 0x14, 0x00, 0x00],
    [0x00, 0x40, 0x34, 0x00, 0x00],
    [0x00, 0x08, 0x14, 0x22, 0x41],
    [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08],
    [0x02, 0x01, 0x59, 0x09, 0x06],
    [0x3E, 0x41, 0x5D, 0x59, 0x4E],
    [0x7C, 0x12, 0x11, 0x12, 0x7C],
    [0x7F, 0x49, 0x49, 0x49, 0x36],
    [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x49, 0x49, 0x49, 0x41],
    [0x7F, 0x09, 0x09, 0x09, 0x01],
    [0x3E, 0x41, 0x41, 0x51, 0x73],
    [0x7F, 0x08, 0x08, 0x08, 0x7F],
    [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01],
    [0x7F, 0x08, 0x14, 0x22, 0x41],
    [0x7F, 0x40, 0x40, 0x40, 0x40],
    [0x7F, 0x02, 0x1C, 0x02, 0x7F],
    [0x7F, 0x04, 0x08, 0x10, 0x7F],
    [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06],
    [0x3E, 0x41, 0x51, 0x21, 0x5E],
    [0x7F, 0x09, 0x19, 0x29, 0x46],
    [0x26, 0x49, 0x49, 0x49, 0x32],
    [0x03, 0x01, 0x7F, 0x01, 0x03],
    [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F],
    [0x3F, 0x40, 0x38, 0x40, 0x3F],
    [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x03, 0x04, 0x78, 0x04, 0x03],
    [0x61, 0x59, 0x49, 0x4D, 0x43],
    [0x00, 0x7F, 0x41, 0x41, 0x41],
    [0x02, 0x04, 0x08, 0x10, 0x20],
    [0x00, 0x41, 0x41, 0x41, 0x7F],
    [0x04, 0x02, 0x01, 0x02, 0x04],
    [0x40, 0x40, 0x40, 0x40, 0x40],
    [0x00, 0x03, 0x07, 0x08, 0x00],
    [0x20, 0x54, 0x54, 0x78, 0x40],
    [0x7F, 0x28, 0x44, 0x44, 0x38],
    [0x38, 0x44, 0x44, 0x44, 0x28],
    [0x38, 0x44, 0x44, 0x28, 0x7F],
    [0x38, 0x54, 0x54, 0x54, 0x18],
    [0x00, 0x08, 0x7E, 0x09, 0x02],
    [0x18, 0xA4, 0xA4, 0x9C, 0x78],
    [0x7F, 0x08, 0x04, 0x04, 0x78],
    [0x00, 0x44, 0x7D, 0x40, 0x00],
    [0x20, 0x40, 0x40, 0x3D, 0x00],
    [0x7F, 0x10, 0x28, 0x44, 0x00],
    [0x00, 0x41, 0x7F, 0x40, 0x00],
    [0x7C, 0x04, 0x78, 0x04, 0x78],
    [0x7C, 0x08, 0x04, 0x04, 0x78],
    [0x38, 0x44, 0x44, 0x44, 0x38],
    [0xFC, 0x18, 0x24, 0x24, 0x18],
    [0x18, 0x24, 0x24, 0x18, 0xFC],
    [0x7C, 0x08, 0x04, 0x04, 0x08],
    [0x48, 0x54, 0x54, 0x54, 0x24],
    [0x04, 0x04, 0x3F, 0x44, 0x24],
    [0x3C, 0x40, 0x40, 0x20, 0x7C],
    [0x1C, 0x20, 0x40, 0x20, 0x1C],
    [0x3C, 0x40, 0x30, 0x40, 0x3C],
    [0x44, 0x28, 0x10, 0x28, 0x44],
    [0x4C, 0x90, 0x90, 0x90, 0x7C],
    [0x44, 0x64, 0x54, 0x4C, 0x44],
    [0x00, 0x08, 0x36, 0x41, 0x00],
    [0x00, 0x00, 0x77, 0x00, 0x00],
    [0x00, 0x41, 0x36, 0x08, 0x00],
    [0x02, 0x01, 0x02, 0x04, 0x02],
];
