//! Device settings shared by the host-testable application and ESP-IDF storage.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub wifi_ssid: String,
    pub wifi_pass: String,
    pub gemini_key: String,
    pub claude_key: String,
    pub openai_key: String,
    pub voicevox_key: String,
    pub provider: String,
    pub wake_phrase: String,
    pub wake_mode: u8,
    pub vad_threshold: u16,
    pub weather_place: String,
    pub latitude: f64,
    pub longitude: f64,
    pub volume: u8,
    pub last_nonzero_volume: u8,
    pub speaker: u8,
    pub mic_gain: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wifi_ssid: String::new(),
            wifi_pass: String::new(),
            gemini_key: String::new(),
            claude_key: String::new(),
            openai_key: String::new(),
            voicevox_key: String::new(),
            provider: "gemini".into(),
            wake_phrase: "スタックちゃん".into(),
            wake_mode: 1,
            vad_threshold: 5000,
            weather_place: "東京".into(),
            latitude: 35.6812,
            longitude: 139.7671,
            volume: 100,
            last_nonzero_volume: 10,
            speaker: 3,
            mic_gain: 16,
        }
    }
}

// Never place credentials into logs through an accidental `{:?}`.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("wifi_ssid", &self.wifi_ssid)
            .field("wifi_pass", &"<redacted>")
            .field("gemini_key", &"<redacted>")
            .field("claude_key", &"<redacted>")
            .field("openai_key", &"<redacted>")
            .field("voicevox_key", &"<redacted>")
            .field("provider", &self.provider)
            .field("wake_phrase", &self.wake_phrase)
            .field("wake_mode", &self.wake_mode)
            .field("vad_threshold", &self.vad_threshold)
            .field("weather_place", &self.weather_place)
            .field("latitude", &self.latitude)
            .field("longitude", &self.longitude)
            .field("volume", &self.volume)
            .field("last_nonzero_volume", &self.last_nonzero_volume)
            .field("speaker", &self.speaker)
            .field("mic_gain", &self.mic_gain)
            .finish()
    }
}

impl Config {
    /// Remember the audible level so a voice command can undo mute, including
    /// after reboot. Both Web settings and voice commands use this path.
    pub fn set_volume(&mut self, volume: u8) {
        if self.volume > 0 {
            self.last_nonzero_volume = self.volume;
        }
        self.volume = volume;
        if volume > 0 {
            self.last_nonzero_volume = volume;
        }
    }

    /// Validate a complete snapshot before using it or committing it to NVS.
    /// Missing credentials are valid during setup. Errors identify the field,
    /// never its contents, because callers may display or log the error.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, limit) in [
            ("ssid", &self.wifi_ssid, 32),
            ("pass", &self.wifi_pass, 64),
            ("gemini", &self.gemini_key, 2048),
            ("claude", &self.claude_key, 2048),
            ("openai", &self.openai_key, 2048),
            ("voicevox", &self.voicevox_key, 2048),
            ("wake", &self.wake_phrase, 512),
            ("place", &self.weather_place, 128),
        ] {
            if value.len() > limit || value.chars().any(char::is_control) {
                return Err(format!("{name}: 長すぎる値、または使用できない文字です"));
            }
        }
        if !matches!(self.provider.as_str(), "gemini" | "claude") {
            return Err("provider: gemini または claude を指定してください".into());
        }
        for (name, value, min, max) in [
            ("wake_en", u16::from(self.wake_mode), 0, 2),
            ("vad", self.vad_threshold, 200, 8000),
            ("speaker", u16::from(self.speaker), 0, 60),
            ("mic", u16::from(self.mic_gain), 1, 255),
            (
                "last_nonzero_volume",
                u16::from(self.last_nonzero_volume),
                1,
                255,
            ),
        ] {
            if !(min..=max).contains(&value) {
                return Err(format!("{name}: {min}〜{max} の整数を指定してください"));
            }
        }
        // `volume` is u8, so every representable value is within 0..=255.
        for (name, value, min, max) in [
            ("lat", self.latitude, -90.0, 90.0),
            ("lon", self.longitude, -180.0, 180.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("{name}: {min}〜{max} の数値を指定してください"));
            }
        }
        Ok(())
    }

    /// Apply a partial URL-encoded form atomically. Empty fields retain saved values.
    /// `wifi_open=1` explicitly clears the Wi-Fi password for an open network.
    pub fn apply_form(&mut self, form: &BTreeMap<String, String>) -> Result<(), String> {
        let mut next = self.clone();
        for (name, target, limit) in [
            ("ssid", &mut next.wifi_ssid, 32),
            ("pass", &mut next.wifi_pass, 64),
            ("gemini", &mut next.gemini_key, 2048),
            ("claude", &mut next.claude_key, 2048),
            ("openai", &mut next.openai_key, 2048),
            ("voicevox", &mut next.voicevox_key, 2048),
            ("wake", &mut next.wake_phrase, 512),
            ("place", &mut next.weather_place, 128),
        ] {
            if let Some(value) = nonempty(form, name) {
                if value.len() > limit || value.chars().any(char::is_control) {
                    return Err(format!("{name}: 長すぎる値、または使用できない文字です"));
                }
                *target = value.to_owned();
            }
        }
        if let Some(value) = nonempty(form, "provider") {
            if !matches!(value, "gemini" | "claude") {
                return Err("provider: gemini または claude を指定してください".into());
            }
            next.provider = value.to_owned();
        }
        if let Some(value) = nonempty(form, "wifi_open") {
            match value {
                "1" => {
                    if nonempty(form, "pass").is_some() {
                        return Err("パスワード入力とパスワードなし指定は同時に使えません".into());
                    }
                    next.wifi_pass.clear();
                }
                "0" => {}
                _ => return Err("wifi_open: 0 または 1 を指定してください".into()),
            }
        }
        if let Some(value) = number(form, "wake_en", 0, 2)? {
            next.wake_mode = value as u8;
        }
        if let Some(value) = number(form, "vad", 200, 8000)? {
            next.vad_threshold = value;
        }
        if let Some(value) = number(form, "volume", 0, 255)? {
            next.set_volume(value as u8);
        }
        if let Some(value) = number(form, "speaker", 0, 60)? {
            next.speaker = value as u8;
        }
        if let Some(value) = number(form, "mic", 1, 255)? {
            next.mic_gain = value as u8;
        }
        if let Some(value) = coordinate(form, "lat", -90.0, 90.0)? {
            next.latitude = value;
        }
        if let Some(value) = coordinate(form, "lon", -180.0, 180.0)? {
            next.longitude = value;
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
}

fn nonempty<'a>(form: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    form.get(name).map(String::as_str).filter(|s| !s.is_empty())
}

fn number(
    form: &BTreeMap<String, String>,
    name: &str,
    min: u16,
    max: u16,
) -> Result<Option<u16>, String> {
    let Some(text) = nonempty(form, name) else {
        return Ok(None);
    };
    let parsed = text.trim().parse::<u16>();
    match parsed {
        Ok(value) if (min..=max).contains(&value) => Ok(Some(value)),
        _ => Err(format!("{name}: {min}〜{max} の整数を指定してください")),
    }
}

fn coordinate(
    form: &BTreeMap<String, String>,
    name: &str,
    min: f64,
    max: f64,
) -> Result<Option<f64>, String> {
    let Some(text) = nonempty(form, name) else {
        return Ok(None);
    };
    match text.trim().parse::<f64>() {
        Ok(value) if value.is_finite() && (min..=max).contains(&value) => Ok(Some(value)),
        _ => Err(format!("{name}: {min}〜{max} の数値を指定してください")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn empty_fields_retain_secrets_and_other_settings() {
        let mut config = Config {
            wifi_pass: "wifi-secret".into(),
            gemini_key: "secret".into(),
            ..Config::default()
        };
        let original = config.clone();
        config
            .apply_form(&form(&[
                ("pass", ""),
                ("gemini", ""),
                ("volume", ""),
                ("wake", ""),
            ]))
            .unwrap();
        assert_eq!(config, original);
        assert!(!format!("{config:?}").contains("wifi-secret"));
    }

    #[test]
    fn validation_is_atomic_and_rejects_nan() {
        let mut config = Config::default();
        let original = config.clone();
        assert!(config
            .apply_form(&form(&[("ssid", "changed"), ("volume", "256")]))
            .is_err());
        assert_eq!(config, original);
        assert!(config.apply_form(&form(&[("lat", "NaN")])).is_err());
        assert!(config.apply_form(&form(&[("lon", "181")])).is_err());
        assert!(config
            .apply_form(&form(&[("provider", "unknown")]))
            .is_err());
        assert!(config.apply_form(&form(&[("mic", "0")])).is_err());
    }

    #[test]
    fn accepts_boundaries_and_explicit_open_wifi() {
        let mut config = Config {
            wifi_pass: "saved".into(),
            ..Config::default()
        };
        config
            .apply_form(&form(&[
                ("volume", "0"),
                ("speaker", "60"),
                ("wake_en", "2"),
                ("lat", "-90"),
                ("wifi_open", "1"),
            ]))
            .unwrap();
        assert_eq!(
            (config.volume, config.speaker, config.wake_mode),
            (0, 60, 2)
        );
        assert_eq!(config.latitude, -90.0);
        assert!(config.wifi_pass.is_empty());
    }

    #[test]
    fn whole_snapshot_accepts_setup_defaults_and_numeric_boundaries() {
        assert!(Config::default().validate().is_ok());
        let mut config = Config {
            wake_mode: 0,
            vad_threshold: 200,
            volume: 0,
            speaker: 0,
            mic_gain: 1,
            latitude: -90.0,
            longitude: -180.0,
            ..Config::default()
        };
        assert!(config.validate().is_ok());
        config.provider = "claude".into();
        config.wake_mode = 2;
        config.vad_threshold = 8000;
        config.volume = 255;
        config.speaker = 60;
        config.mic_gain = 255;
        config.latitude = 90.0;
        config.longitude = 180.0;
        assert!(config.validate().is_ok());
        assert!(config.wifi_ssid.is_empty() && config.openai_key.is_empty());
    }

    #[test]
    fn rejects_semantically_invalid_persisted_json_without_changing_it() {
        // These snapshots deserialize successfully: serde's integer widths
        // cannot enforce application ranges or the provider enumeration.
        for json in [
            r#"{"provider":"unknown"}"#,
            r#"{"provider":""}"#,
            r#"{"wake_mode":3}"#,
            r#"{"vad_threshold":0}"#,
            r#"{"vad_threshold":199}"#,
            r#"{"vad_threshold":8001}"#,
            r#"{"speaker":61}"#,
            r#"{"mic_gain":0}"#,
            r#"{"latitude":90.01}"#,
            r#"{"longitude":-180.01}"#,
        ] {
            let config: Config = serde_json::from_str(json).unwrap();
            let before = serde_json::to_vec(&config).unwrap();
            assert!(config.validate().is_err(), "accepted {json}");
            assert_eq!(serde_json::to_vec(&config).unwrap(), before);
        }
    }

    #[test]
    fn whole_snapshot_rejects_every_nonfinite_coordinate() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for latitude in [true, false] {
                let mut config = Config::default();
                if latitude {
                    config.latitude = value;
                } else {
                    config.longitude = value;
                }
                let error = config.validate().unwrap_err();
                assert!(error.starts_with(if latitude { "lat:" } else { "lon:" }));
            }
        }
    }

    fn string_field<'a>(config: &'a mut Config, name: &str) -> &'a mut String {
        match name {
            "ssid" => &mut config.wifi_ssid,
            "pass" => &mut config.wifi_pass,
            "gemini" => &mut config.gemini_key,
            "claude" => &mut config.claude_key,
            "openai" => &mut config.openai_key,
            "voicevox" => &mut config.voicevox_key,
            "wake" => &mut config.wake_phrase,
            "place" => &mut config.weather_place,
            _ => panic!("unknown test field"),
        }
    }

    #[test]
    fn validates_utf8_byte_limits_and_rejects_controls_without_secret_echo() {
        for (name, limit) in [
            ("ssid", 32),
            ("pass", 64),
            ("gemini", 2048),
            ("claude", 2048),
            ("openai", 2048),
            ("voicevox", 2048),
            ("wake", 512),
            ("place", 128),
        ] {
            let mut config = Config::default();
            *string_field(&mut config, name) = "x".repeat(limit);
            assert!(config.validate().is_ok(), "boundary for {name}");
            string_field(&mut config, name).push('x');
            assert!(config.validate().unwrap_err().starts_with(name));
            // Two-byte characters must count as two bytes for NVS/Wi-Fi limits.
            *string_field(&mut config, name) = "é".repeat(limit / 2 + 1);
            assert!(config.validate().is_err(), "UTF-8 bytes for {name}");
            for control in ['\0', '\n', '\r', '\t', '\u{7f}', '\u{85}'] {
                *string_field(&mut config, name) = format!("secret{control}value");
                let error = config.validate().unwrap_err();
                assert!(error.starts_with(name));
                assert!(!error.contains("secret"));
                assert!(!error.contains("value"));
            }
        }
    }

    #[test]
    fn partial_updates_cannot_preserve_unrelated_invalid_saved_fields() {
        let mut config = Config {
            mic_gain: 0,
            ..Config::default()
        };
        let original = config.clone();
        assert!(config
            .apply_form(&form(&[("ssid", "new-network")]))
            .is_err());
        assert_eq!(config, original);
        assert!(config.apply_form(&BTreeMap::new()).is_err());
        assert_eq!(config, original);
        // A complete valid correction is allowed; validation never silently
        // resets an unrelated field to its default.
        config
            .apply_form(&form(&[("mic", "1"), ("ssid", "new-network")]))
            .unwrap();
        assert_eq!(config.mic_gain, 1);
        assert_eq!(config.wifi_ssid, "new-network");
        assert!(config.validate().is_ok());
    }
    #[test]
    fn mute_remembers_volume_across_repeated_requests_and_serialization() {
        let mut config = Config::default();
        config.set_volume(12);
        config.set_volume(0);
        config.set_volume(0);
        let saved = serde_json::to_vec(&config).unwrap();
        let mut restored: Config = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.volume, 0);
        assert_eq!(restored.last_nonzero_volume, 12);
        restored.set_volume(restored.last_nonzero_volume);
        assert_eq!(restored.volume, 12);
        restored.apply_form(&form(&[("volume", "8")])).unwrap();
        restored.apply_form(&form(&[("volume", "0")])).unwrap();
        assert_eq!(restored.last_nonzero_volume, 8);
    }

    #[test]
    fn old_settings_remain_compatible_and_have_a_quiet_unmute_fallback() {
        let mut config: Config = serde_json::from_str(r#"{"volume":0}"#).unwrap();
        assert_eq!(config.last_nonzero_volume, 10);
        assert!(config.validate().is_ok());
        config = serde_json::from_str(r#"{"volume":23}"#).unwrap();
        config.set_volume(0);
        assert_eq!(config.last_nonzero_volume, 23);
        config.last_nonzero_volume = 0;
        assert!(config.validate().is_err());
    }
}
