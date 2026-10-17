//! Rust settings are committed as one NVS blob. Arduino namespaces remain readable.
use crate::{config::Config, conversation::DEFAULT_ROLE};
use anyhow::{Context, Result};
use esp_idf_svc::{
    nvs::{EspDefaultNvs, EspDefaultNvsPartition},
    sys,
};
use std::{fs, io::Read};

pub struct Storage {
    partition: EspDefaultNvsPartition,
    rust: EspDefaultNvs,
    pub filesystem_available: bool,
}

impl Storage {
    pub fn new(partition: EspDefaultNvsPartition) -> Result<Self> {
        let rust = EspDefaultNvs::new(partition.clone(), "rustchan", true)?;
        // Never format on a mount error: existing role/voice files belong to the owner.
        let conf = sys::esp_vfs_spiffs_conf_t {
            base_path: c"/spiffs".as_ptr(),
            partition_label: std::ptr::null(),
            max_files: 4,
            format_if_mount_failed: false,
        };
        let status = unsafe { sys::esp_vfs_spiffs_register(&conf) };
        let filesystem_available = status == sys::ESP_OK || status == sys::ESP_ERR_INVALID_STATE;
        if !filesystem_available {
            log::warn!("SPIFFS unavailable ({status}); voice cache disabled");
        }
        Ok(Self {
            partition,
            rust,
            filesystem_available,
        })
    }

    pub fn load(&self) -> Result<(Config, String)> {
        let config: Config = if let Some(length) = self.rust.blob_len("config")? {
            anyhow::ensure!(length <= 16_384, "saved settings exceed the supported size");
            let mut bytes = vec![0; length];
            self.rust.get_blob("config", &mut bytes)?;
            serde_json::from_slice(&bytes).context("reading Rust settings")?
        } else {
            self.load_arduino()?
        };
        config
            .validate()
            .map_err(anyhow::Error::msg)
            .context("validating saved settings")?;
        let role = self
            .string(&self.rust, "role")?
            .or_else(|| self.legacy_role())
            .unwrap_or_else(|| DEFAULT_ROLE.to_owned());
        Ok((config, role))
    }

    pub fn save_config(&self, config: &Config) -> Result<()> {
        config
            .validate()
            .map_err(anyhow::Error::msg)
            .context("validating settings before save")?;
        let bytes = serde_json::to_vec(config)?;
        anyhow::ensure!(bytes.len() <= 16_384, "settings exceed NVS storage limit");
        self.rust.set_blob("config", &bytes)?;
        Ok(())
    }

    pub fn save_role(&self, role: &str) -> Result<()> {
        anyhow::ensure!(role.len() <= 8192, "role exceeds storage limit");
        self.rust.set_str("role", role)?;
        Ok(())
    }

    /// Explicit Web "clear keys" action also clears the credentials read by
    /// the legacy application, so returning to Arduino cannot restore them.
    pub fn clear_credentials(&self, cleared: &Config) -> Result<()> {
        self.save_config(cleared)?;
        let wifi = EspDefaultNvs::new(self.partition.clone(), "wifi", true)?;
        wifi.set_str("ssid", "")?;
        wifi.set_str("pass", "")?;
        let chibi = EspDefaultNvs::new(self.partition.clone(), "chibi", true)?;
        for key in ["gemini", "claude", "openai", "voicevox"] {
            chibi.set_str(key, "")?;
        }
        chibi.set_u8("nofallback", 1)?;
        Ok(())
    }

    fn string(&self, nvs: &EspDefaultNvs, key: &str) -> Result<Option<String>> {
        let Some(length) = nvs.str_len(key)? else {
            return Ok(None);
        };
        anyhow::ensure!(length <= 16_384, "NVS string exceeds supported size");
        let mut bytes = vec![0; length];
        Ok(nvs.get_str(key, &mut bytes)?.map(str::to_owned))
    }

    fn namespace(&self, name: &str) -> Option<EspDefaultNvs> {
        EspDefaultNvs::new(self.partition.clone(), name, false).ok()
    }

    fn load_arduino(&self) -> Result<Config> {
        let mut c = Config::default();
        if let Some(nvs) = self.namespace("wifi") {
            c.wifi_ssid = self.string(&nvs, "ssid")?.unwrap_or_default();
            c.wifi_pass = self.string(&nvs, "pass")?.unwrap_or_default();
        }
        if let Some(nvs) = self.namespace("chibi") {
            for (key, field) in [
                ("gemini", &mut c.gemini_key),
                ("claude", &mut c.claude_key),
                ("openai", &mut c.openai_key),
                ("voicevox", &mut c.voicevox_key),
                ("provider", &mut c.provider),
                ("wake", &mut c.wake_phrase),
                ("place", &mut c.weather_place),
            ] {
                if let Some(value) = self.string(&nvs, key)?.filter(|s| !s.is_empty()) {
                    *field = value;
                }
            }
            c.wake_mode = nvs.get_u8("wake_en")?.filter(|v| *v <= 2).unwrap_or(1);
            if let Some(v) = self
                .string(&nvs, "vad")?
                .and_then(|s| s.parse::<u16>().ok())
            {
                c.vad_threshold = v.clamp(200, 8000);
            }
            if let Some(v) = self
                .string(&nvs, "lat")?
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && (-90.0..=90.0).contains(v))
            {
                c.latitude = v;
            }
            if let Some(v) = self
                .string(&nvs, "lon")?
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && (-180.0..=180.0).contains(v))
            {
                c.longitude = v;
            }
        }
        if let Some(nvs) = self.namespace("setting") {
            c.set_volume(nvs.get_u32("volume")?.unwrap_or(100).min(255) as u8);
            c.speaker = nvs.get_u8("speaker")?.filter(|v| *v <= 60).unwrap_or(3);
            c.mic_gain = nvs.get_u8("micgain")?.filter(|v| *v > 0).unwrap_or(16);
        }
        if !matches!(c.provider.as_str(), "gemini" | "claude") {
            c.provider = "gemini".into();
        }
        Ok(c)
    }

    fn legacy_role(&self) -> Option<String> {
        let file = fs::File::open("/spiffs/data.json").ok()?;
        let mut bytes = Vec::new();
        file.take(16_385).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 16_384 {
            return None;
        }
        let doc: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let content = doc["messages"][0]["content"].as_str()?;
        // Arduino stored its generated rules along with the user-authored role.
        Some(content.split("\n返答の先頭に感情タグ").next()?.to_owned())
    }

    pub fn cached_voice(&self, name: &str) -> Option<Vec<u8>> {
        if !matches!(name, "wake" | "nonet") || !self.filesystem_available {
            return None;
        }
        let file = fs::File::open(format!("/spiffs/{name}.mp3")).ok()?;
        let mut bytes = Vec::new();
        file.take(512 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.is_empty() || bytes.len() > 512 * 1024 {
            None
        } else {
            Some(bytes)
        }
    }

    pub fn cache_voice(&self, name: &str, bytes: &[u8]) -> Result<()> {
        anyhow::ensure!(matches!(name, "wake" | "nonet"), "invalid voice cache name");
        if self.filesystem_available && self.cached_voice(name).is_none() {
            // create_new preserves an existing owner file, including an unreadable one.
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(format!("/spiffs/{name}.mp3"))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Ok(())
    }
}
