//! Minimal BMI270 accelerometer driver. No gyro/magnetometer needed for rotation.
use crate::orientation::Orientation;
use anyhow::{ensure, Result};
use esp_idf_svc::hal::{
    delay::{FreeRtos, TickType},
    i2c::I2cDriver,
};

const ADDRESS: u8 = 0x68;
const CONFIG: &[u8; 8192] = include_bytes!("bmi270-config.bin");

pub struct Imu {
    orientation: Orientation,
    last_poll: u64,
    failures: u8,
    enabled: bool,
}

fn write(bus: &mut I2cDriver<'_>, data: &[u8]) -> Result<()> {
    bus.write(ADDRESS, data, TickType::new_millis(20).ticks())?;
    Ok(())
}

fn read(bus: &mut I2cDriver<'_>, register: u8, data: &mut [u8]) -> Result<()> {
    bus.write_read(ADDRESS, &[register], data, TickType::new_millis(10).ticks())?;
    Ok(())
}

impl Imu {
    pub fn new(bus: &mut I2cDriver<'_>) -> Result<Self> {
        let mut value = [0];
        read(bus, 0, &mut value)?;
        ensure!(value[0] == 0x24, "BMI270 chip ID: {:02x}", value[0]);
        write(bus, &[0x7e, 0xb6])?;
        FreeRtos::delay_ms(10);
        write(bus, &[0x7c, 0])?;
        FreeRtos::delay_ms(2);
        write(bus, &[0x59, 0])?;
        for (index, chunk) in CONFIG.chunks(32).enumerate() {
            let offset = index * 32;
            write(
                bus,
                &[0x5b, ((offset >> 1) & 15) as u8, (offset >> 5) as u8],
            )?;
            let mut packet = [0; 33];
            packet[0] = 0x5e;
            packet[1..].copy_from_slice(chunk);
            write(bus, &packet)?;
        }
        write(bus, &[0x59, 1])?;
        FreeRtos::delay_ms(150);
        read(bus, 0x21, &mut value)?;
        ensure!(value[0] & 15 == 1, "BMI270 init status: {:02x}", value[0]);
        write(bus, &[0x40, 0xa7])?; // 50 Hz, normal averaging, performance mode.
        write(bus, &[0x41, 0x00])?; // +/-2 g, 16384 LSB/g.
        write(bus, &[0x7d, 0x04])?; // Accelerometer only.
        FreeRtos::delay_ms(20);
        crate::orientation_status::set_imu_enabled(true);
        log::info!("BMI270 ready: automatic face rotation (0/90/180/270, dwell 300 ms)");
        Ok(Self {
            orientation: Orientation::default(),
            last_poll: 0,
            failures: 0,
            enabled: true,
        })
    }

    pub fn poll(&mut self, bus: &mut I2cDriver<'_>, now: u64) -> Option<u8> {
        let interval = if self.enabled { 100 } else { 1_000 };
        if now.saturating_sub(self.last_poll) < interval {
            return None;
        }
        self.last_poll = now;
        let mut bytes = [0; 6];
        if let Err(error) = read(bus, 0x0c, &mut bytes) {
            self.orientation.update([0.; 3], now);
            self.failures = self.failures.saturating_add(1);
            if self.enabled && self.failures >= 3 {
                self.enabled = false;
                crate::orientation_status::set_imu_enabled(false);
                log::warn!(
                    "Face auto-rotation paused after IMU read failures; retrying: {error:#}"
                );
            }
            return None;
        }
        self.failures = 0;
        if !self.enabled {
            self.enabled = true;
            crate::orientation_status::set_imu_enabled(true);
            log::info!("Face auto-rotation resumed");
        }
        let axes = std::array::from_fn(|i| {
            i16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]) as f32 / 16384.
        });
        crate::orientation_status::record_sample(
            axes.map(|g| (g * 1_000.).round() as i32),
            (unsafe { esp_idf_svc::sys::esp_timer_get_time() } / 1_000) as u64,
        );
        let rotation = self
            .orientation
            .update(crate::orientation::atoms3r_screen_acceleration(axes), now);
        if let Some(r) = rotation {
            log::info!(
                "Face rotation: {} degrees; accel={axes:?}",
                u16::from(r) * 90
            );
        }
        rotation
    }
}
