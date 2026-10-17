//! Bounded cloud requests, shared by the device and offline protocol tests.
//!
//! API contracts: OpenAI audio/transcriptions, Google generateContent,
//! Anthropic Messages, TTS Quest v3 VOICEVOX, and Open-Meteo forecast.
//! Credentials, request URLs, and response bodies are never logged here.

use crate::{config::Config, conversation::Message};
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const JSON_LIMIT: usize = 64 * 1024;
const AUDIO_LIMIT: usize = 2 * 1024 * 1024;
const WAV_LIMIT: usize = 1024 * 1024;
const CHAT_LIMIT: usize = 48 * 1024;
const TEXT_LIMIT: usize = 8 * 1024;
const TRANSCRIBE_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
// Preserve the configured C++ model. Google currently routes this supported,
// deprecated alias to gemini-3.6-flash; avoid silently changing model policy.
const GEMINI_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash:generateContent";
const CLAUDE_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_MODEL: &str = "claude-haiku-4-5";
const VOICEVOX_URL: &str = "https://api.tts.quest/v3/voicevox/synthesis";

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait Transport {
    /// Return a complete response or an error. Never return a truncated body.
    fn request(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response: usize,
    ) -> Result<Response>;
}

pub struct Services<T: Transport> {
    transport: T,
}

impl<T: Transport> Services<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transcribe(&mut self, wav: &[u8], config: &Config) -> Result<String> {
        require_key(&config.openai_key, "OpenAI")?;
        ensure!(
            wav.len() > 44 && wav.len() <= WAV_LIMIT,
            "音声データのサイズが不正です"
        );
        ensure!(
            wav.starts_with(b"RIFF") && wav.get(8..12) == Some(b"WAVE"),
            "WAV音声が必要です"
        );
        // Select a delimiter absent from the binary data, even for synthetic
        // recordings used in tests. No random number source is necessary.
        let boundary = (0..64)
            .map(|index| format!("stackchan-rust-multipart-{index}"))
            .find(|candidate| {
                !wav.windows(candidate.len())
                    .any(|part| part == candidate.as_bytes())
            })
            .context("音声データからmultipart境界を選択できませんでした")?;
        let prefix = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nja\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\n\
             Content-Type: audio/wav\r\n\r\n"
        );
        let suffix = format!("\r\n--{boundary}--\r\n");
        let mut body = Vec::with_capacity(prefix.len() + wav.len() + suffix.len());
        body.extend_from_slice(prefix.as_bytes());
        body.extend_from_slice(wav);
        body.extend_from_slice(suffix.as_bytes());
        let auth = format!("Bearer {}", config.openai_key);
        let content_type = format!("multipart/form-data; boundary={boundary}");
        let response = self
            .transport
            .request(
                "POST",
                TRANSCRIBE_URL,
                &[("Authorization", &auth), ("Content-Type", &content_type)],
                &body,
                JSON_LIMIT,
            )
            .context("音声認識サービスへの接続に失敗しました")?;
        let value = parse_json(response, "音声認識")?;
        let text = value["text"]
            .as_str()
            .ok_or_else(|| anyhow!("音声認識の応答にテキストがありません"))?;
        ensure!(text.len() <= TEXT_LIMIT, "音声認識テキストが長すぎます");
        // Empty text is a valid transcription of silence, unlike an API error.
        Ok(text.trim().to_owned())
    }

    pub fn chat(&mut self, messages: &[Message], config: &Config) -> Result<String> {
        let mut system = String::new();
        let mut turns = Vec::new();
        let mut total = 0usize;
        for message in messages {
            total = total
                .checked_add(message.content.len())
                .context("会話が長すぎます")?;
            ensure!(total <= CHAT_LIMIT, "会話が長すぎます");
            if message.content.trim().is_empty() {
                continue;
            }
            match message.role.as_str() {
                "system" => {
                    if !system.is_empty() {
                        system.push('\n');
                    }
                    system.push_str(&message.content);
                }
                "user" | "assistant" => turns.push(message),
                _ => bail!("未対応の会話ロールです"),
            }
        }
        ensure!(!turns.is_empty(), "送信する会話がありません");
        let (url, headers, body) = match config.provider.as_str() {
            "gemini" => {
                require_key(&config.gemini_key, "Gemini")?;
                let contents: Vec<Value> = turns
                    .iter()
                    .map(|m| {
                        json!({
                            "role": if m.role == "assistant" { "model" } else { "user" },
                            "parts": [{"text": m.content}],
                        })
                    })
                    .collect();
                let mut payload = json!({"contents": contents});
                if !system.is_empty() {
                    payload["systemInstruction"] = json!({"parts": [{"text": system}]});
                }
                // A header avoids including API credentials in URL diagnostics.
                (
                    GEMINI_URL,
                    vec![("x-goog-api-key", config.gemini_key.as_str())],
                    serde_json::to_vec(&payload)?,
                )
            }
            "claude" => {
                require_key(&config.claude_key, "Claude")?;
                let turns: Vec<Value> = turns
                    .iter()
                    .map(|m| json!({"role": m.role, "content": m.content}))
                    .collect();
                let mut payload =
                    json!({"model": CLAUDE_MODEL, "max_tokens": 300, "messages": turns});
                if !system.is_empty() {
                    payload["system"] = Value::String(system);
                }
                (
                    CLAUDE_URL,
                    vec![
                        ("x-api-key", config.claude_key.as_str()),
                        ("anthropic-version", "2023-06-01"),
                    ],
                    serde_json::to_vec(&payload)?,
                )
            }
            _ => bail!("会話サービスの設定が不正です"),
        };
        let mut headers = headers;
        headers.push(("Content-Type", "application/json"));
        let response = self
            .transport
            .request("POST", url, &headers, &body, JSON_LIMIT)
            .context("会話サービスへの接続に失敗しました")?;
        let value = parse_json(response, "会話")?;
        let mut text = String::new();
        let parts = if config.provider == "gemini" {
            value["candidates"][0]["content"]["parts"].as_array()
        } else {
            value["content"].as_array()
        }
        .ok_or_else(|| anyhow!("会話サービスの応答にテキストがありません"))?;
        for part in parts {
            // Gemini can include internal thought parts; never speak them.
            if part["thought"].as_bool() == Some(true) {
                continue;
            }
            if config.provider == "claude" && part["type"].as_str() != Some("text") {
                continue;
            }
            if let Some(fragment) = part["text"].as_str() {
                ensure!(
                    text.len() + fragment.len() <= TEXT_LIMIT,
                    "会話の返答が長すぎます"
                );
                text.push_str(fragment);
            }
        }
        ensure!(!text.trim().is_empty(), "会話サービスの返答が空です");
        Ok(text.trim().to_owned())
    }

    /// Download one complete MP3. The board owns decoding and playback.
    pub fn synthesize(&mut self, text: &str, config: &Config) -> Result<Vec<u8>> {
        ensure!(
            !text.trim().is_empty() && text.len() <= TEXT_LIMIT,
            "読み上げテキストの長さが不正です"
        );
        ensure!(
            !config.voicevox_key.chars().any(char::is_control),
            "VOICEVOXキーが不正です"
        );
        let mut url = format!(
            "{VOICEVOX_URL}?speaker={}&text={}",
            config.speaker,
            percent_encode(text)
        );
        // TTS Quest supports an optional key; preserve the free mode as well.
        if !config.voicevox_key.is_empty() {
            url.push_str("&key=");
            url.push_str(&percent_encode(&config.voicevox_key));
        }
        let response = self
            .transport
            .request("GET", &url, &[], &[], JSON_LIMIT)
            .context("音声合成サービスへの接続に失敗しました")?;
        let value = parse_json(response, "音声合成")?;
        ensure!(
            value["success"].as_bool() == Some(true),
            "音声合成サービスが要求を受け付けませんでした"
        );
        let audio_url = value["mp3DownloadUrl"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| value["mp3StreamingUrl"].as_str().filter(|s| !s.is_empty()))
            .ok_or_else(|| anyhow!("音声合成の応答にMP3 URLがありません"))?;
        validate_tts_url(audio_url)?;
        if let Some(status_url) = value["audioStatusUrl"].as_str().filter(|s| !s.is_empty()) {
            validate_tts_url(status_url)?;
            let deadline = Instant::now() + Duration::from_secs(25);
            loop {
                ensure!(
                    Instant::now() < deadline,
                    "音声の合成待ちがタイムアウトしました"
                );
                let response = self
                    .transport
                    .request("GET", status_url, &[], &[], JSON_LIMIT)
                    .context("音声合成の進捗を取得できませんでした")?;
                let status = parse_json(response, "音声合成の進捗")?;
                ensure!(
                    status["success"].as_bool() != Some(false)
                        && status["isAudioError"].as_bool() != Some(true),
                    "音声の合成に失敗しました"
                );
                if status["isAudioReady"].as_bool() == Some(true) {
                    break;
                }
                ensure!(
                    status["isAudioReady"].is_boolean(),
                    "音声合成の進捗データが不正です"
                );
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        let response = self
            .transport
            .request(
                "GET",
                audio_url,
                &[("Accept", "audio/mpeg")],
                &[],
                AUDIO_LIMIT,
            )
            .context("合成音声を取得できませんでした")?;
        let body = checked_body(response, "合成音声", AUDIO_LIMIT)?;
        ensure!(!body.is_empty(), "合成された音声が空です");
        Ok(body)
    }

    pub fn weather(&mut self, config: &Config) -> Result<String> {
        ensure!(
            config.latitude.is_finite()
                && (-90.0..=90.0).contains(&config.latitude)
                && config.longitude.is_finite()
                && (-180.0..=180.0).contains(&config.longitude),
            "天気の緯度または経度が不正です"
        );
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,weather_code&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max&timezone=Asia%2FTokyo&forecast_days=1",
            config.latitude, config.longitude,
        );
        let response = self
            .transport
            .request("GET", &url, &[], &[], JSON_LIMIT)
            .context("天気サービスへの接続に失敗しました")?;
        let value = parse_json(response, "天気")?;
        let current = &value["current"];
        let daily = &value["daily"];
        let now_code = current["weather_code"].as_i64();
        let day_code = daily["weather_code"][0]
            .as_i64()
            .or(now_code)
            .ok_or_else(|| anyhow!("天気データがありません"))?;
        let mut summary = format!(
            "{}の今日の天気は{}",
            config.weather_place,
            weather_code_ja(day_code)
        );
        if let (Some(high), Some(low)) = (
            temperature(&daily["temperature_2m_max"][0]),
            temperature(&daily["temperature_2m_min"][0]),
        ) {
            summary.push_str(&format!("、最高気温{high}度、最低気温{low}度"));
        }
        if let Some(pop) = daily["precipitation_probability_max"][0]
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        {
            summary.push_str(&format!("、降水確率{}パーセント", pop.round() as i32));
        }
        if let (Some(code), Some(temp)) = (now_code, temperature(&current["temperature_2m"])) {
            summary.push_str(&format!("。現在は{}で気温{temp}度", weather_code_ja(code)));
        }
        summary.push('。');
        Ok(summary)
    }
}

fn require_key(key: &str, service: &str) -> Result<()> {
    ensure!(
        !key.trim().is_empty(),
        "{service}のAPIキーが設定されていません"
    );
    ensure!(
        key.len() <= 2048 && !key.chars().any(char::is_control),
        "{service}のAPIキーが不正です"
    );
    Ok(())
}

fn checked_body(response: Response, service: &str, limit: usize) -> Result<Vec<u8>> {
    // Never include an upstream error body: providers can echo credentials or
    // private conversation text in diagnostic responses.
    ensure!(
        (200..300).contains(&response.status),
        "{service}サービスがHTTP {}を返しました",
        response.status
    );
    ensure!(
        response.body.len() <= limit,
        "{service}の応答が大きすぎます"
    );
    Ok(response.body)
}

fn parse_json(response: Response, service: &str) -> Result<Value> {
    let body = checked_body(response, service, JSON_LIMIT)?;
    serde_json::from_slice(&body).map_err(|_| anyhow!("{service}サービスのJSON応答が不正です"))
}

fn percent_encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[(byte >> 4) as usize]));
            out.push(char::from(HEX[(byte & 15) as usize]));
        }
    }
    out
}

fn validate_tts_url(url: &str) -> Result<()> {
    let rest = url
        .strip_prefix("https://")
        .context("音声URLはHTTPSである必要があります")?;
    let host = rest.split('/').next().unwrap_or_default();
    let number = host
        .strip_prefix("audio")
        .and_then(|s| s.strip_suffix(".tts.quest"));
    ensure!(
        url.len() <= 2048
            && !url.chars().any(char::is_control)
            && number.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        "音声URLのホストが不正です"
    );
    Ok(())
}

fn temperature(value: &Value) -> Option<i32> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && (-100.0..=100.0).contains(n))
        .map(|n| n.round() as i32)
}

fn weather_code_ja(code: i64) -> &'static str {
    match code {
        0 => "快晴",
        1 | 2 => "晴れ",
        3 => "くもり",
        45 | 48 => "霧",
        51..=57 => "霧雨",
        61..=67 | 80..=82 => "雨",
        71..=77 | 85 | 86 => "雪",
        95 | 96 | 99 => "雷雨",
        _ => "不明",
    }
}

/// A new TLS connection per request avoids carrying authorization headers to
/// another service. ESP-IDF validates both the hostname and certificate bundle.
#[cfg(target_os = "espidf")]
#[derive(Default)]
pub struct HttpTransport;

#[cfg(target_os = "espidf")]
impl Transport for HttpTransport {
    fn request(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response: usize,
    ) -> Result<Response> {
        use esp_idf_svc::http::{
            client::{Configuration, EspHttpConnection, FollowRedirectsPolicy},
            Method,
        };
        ensure!(url.starts_with("https://"), "HTTPSが必要です");
        ensure!(
            body.len() <= WAV_LIMIT + 4096 && max_response <= AUDIO_LIMIT,
            "HTTPデータのサイズが不正です"
        );
        let method = match method {
            "GET" => Method::Get,
            "POST" => Method::Post,
            _ => bail!("未対応のHTTPメソッドです"),
        };
        let mut connection = EspHttpConnection::new(&Configuration {
            timeout: Some(Duration::from_secs(20)),
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            follow_redirects_policy: FollowRedirectsPolicy::FollowNone,
            buffer_size: Some(2048),
            buffer_size_tx: Some(2048),
            ..Default::default()
        })
        .context("HTTPSクライアントを作成できませんでした")?;
        let content_length = body.len().to_string();
        let mut all_headers: Vec<(&str, &str)> = headers
            .iter()
            .copied()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("Content-Length"))
            .collect();
        all_headers.push(("Content-Length", &content_length));
        all_headers.push(("Accept-Encoding", "identity"));
        let deadline = Instant::now() + Duration::from_secs(75);
        connection
            .initiate_request(method, url, &all_headers)
            .context("HTTPS接続に失敗しました")?;
        let mut written = 0;
        while written < body.len() {
            ensure!(Instant::now() < deadline, "HTTPS送信がタイムアウトしました");
            let end = (written + 2048).min(body.len());
            let count = connection
                .write(&body[written..end])
                .context("HTTPS送信に失敗しました")?;
            ensure!(count != 0, "HTTPS送信が途中で停止しました");
            written += count;
        }
        connection
            .initiate_response()
            .context("HTTPS応答を受信できませんでした")?;
        let status = connection.status();
        let expected = connection
            .header("Content-Length")
            .map(str::parse::<usize>)
            .transpose()
            .context("HTTPS応答サイズが不正です")?;
        if let Some(length) = expected {
            ensure!(length <= max_response, "HTTPS応答が大きすぎます");
        }
        let mut result = Vec::with_capacity(expected.unwrap_or(4096).min(max_response));
        let mut chunk = [0u8; 2048];
        loop {
            ensure!(Instant::now() < deadline, "HTTPS受信がタイムアウトしました");
            let count = connection
                .read(&mut chunk)
                .context("HTTPS受信に失敗しました")?;
            if count == 0 {
                break;
            }
            ensure!(
                result.len() + count <= max_response,
                "HTTPS応答が大きすぎます"
            );
            result.extend_from_slice(&chunk[..count]);
        }
        if let Some(length) = expected {
            ensure!(result.len() == length, "HTTPS応答が途中で切断されました");
        }
        Ok(Response {
            status,
            body: result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Request {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        limit: usize,
    }
    #[derive(Default)]
    struct Mock {
        responses: VecDeque<Response>,
        requests: Vec<Request>,
    }
    impl Mock {
        fn json(value: Value) -> Self {
            Self {
                responses: VecDeque::from([Response {
                    status: 200,
                    body: serde_json::to_vec(&value).unwrap(),
                }]),
                ..Self::default()
            }
        }
    }
    impl Transport for Mock {
        fn request(
            &mut self,
            method: &str,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
            max_response: usize,
        ) -> Result<Response> {
            self.requests.push(Request {
                method: method.into(),
                url: url.into(),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body: body.to_vec(),
                limit: max_response,
            });
            self.responses
                .pop_front()
                .ok_or_else(|| anyhow!("unexpected mock request"))
        }
    }
    fn config() -> Config {
        Config {
            openai_key: "openai-test-key".into(),
            gemini_key: "gemini-test-key".into(),
            claude_key: "claude-test-key".into(),
            ..Config::default()
        }
    }
    fn message(role: &str, content: &str) -> Message {
        Message {
            role: role.into(),
            content: content.into(),
        }
    }
    fn wav() -> Vec<u8> {
        let mut bytes = vec![0; 46];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[8..12].copy_from_slice(b"WAVE");
        bytes
    }

    #[test]
    fn transcription_uses_valid_multipart_and_preserves_empty_text() {
        let mut service = Services::new(Mock::json(json!({"text":" こんにちは "})));
        let audio = wav();
        assert_eq!(service.transcribe(&audio, &config()).unwrap(), "こんにちは");
        let request = &service.transport.requests[0];
        assert_eq!(
            (request.method.as_str(), request.url.as_str(), request.limit),
            ("POST", TRANSCRIBE_URL, JSON_LIMIT)
        );
        assert!(request
            .headers
            .iter()
            .any(|(key, value)| key == "Authorization" && value == "Bearer openai-test-key"));
        let body = String::from_utf8_lossy(&request.body);
        assert!(body.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"));
        assert!(body.contains("name=\"language\"\r\n\r\nja\r\n"));
        assert!(body.contains("Content-Type: audio/wav\r\n\r\n"));
        assert!(request.body.windows(audio.len()).any(|part| part == audio));
        assert!(body.ends_with("--stackchan-rust-multipart-0--\r\n"));
        let mut service = Services::new(Mock::json(json!({"text":""})));
        assert_eq!(service.transcribe(&wav(), &config()).unwrap(), "");
    }

    #[test]
    fn gemini_maps_roles_system_instruction_and_skips_thoughts() {
        let mut service = Services::new(Mock::json(
            json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"private reasoning"},{"text":"[happy]やあ"},{"text":"！"}]}}]}),
        ));
        let reply = service
            .chat(
                &[
                    message("system", "設定"),
                    message("user", "こんにちは"),
                    message("assistant", "やあ"),
                    message("user", "元気？"),
                ],
                &config(),
            )
            .unwrap();
        assert_eq!(reply, "[happy]やあ！");
        let request = &service.transport.requests[0];
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "設定");
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["contents"][2]["parts"][0]["text"], "元気？");
        assert!(!request.url.contains("gemini-test-key"));
        assert!(request
            .headers
            .iter()
            .any(|(key, value)| key == "x-goog-api-key" && value == "gemini-test-key"));
    }

    #[test]
    fn claude_keeps_system_outside_messages_and_joins_text_blocks() {
        let mut settings = config();
        settings.provider = "claude".into();
        let mut service = Services::new(Mock::json(
            json!({"content":[{"type":"thinking","text":"hidden"},{"type":"text","text":"おはよう"},{"type":"text","text":"。"}]}),
        ));
        assert_eq!(
            service
                .chat(
                    &[message("system", "設定"), message("user", "朝だよ")],
                    &settings
                )
                .unwrap(),
            "おはよう。"
        );
        let request = &service.transport.requests[0];
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(request.url, CLAUDE_URL);
        assert_eq!(body["model"], CLAUDE_MODEL);
        assert_eq!(body["max_tokens"], 300);
        assert_eq!(body["system"], "設定");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert!(request
            .headers
            .iter()
            .any(|(key, value)| key == "anthropic-version" && value == "2023-06-01"));
    }

    #[test]
    fn failed_status_and_json_do_not_echo_secrets() {
        for response in [
            Response {
                status: 401,
                body: b"echo openai-test-key".to_vec(),
            },
            Response {
                status: 200,
                body: b"not-json openai-test-key".to_vec(),
            },
        ] {
            let mut service = Services::new(Mock {
                responses: VecDeque::from([response]),
                ..Mock::default()
            });
            let error = service
                .transcribe(&wav(), &config())
                .unwrap_err()
                .to_string();
            assert!(!error.contains("openai-test-key"));
        }
        assert!(parse_json(
            Response {
                status: 200,
                body: vec![b' '; JSON_LIMIT + 1]
            },
            "test"
        )
        .is_err());
    }

    #[test]
    fn input_errors_do_not_make_network_requests() {
        let mut service = Services::new(Mock::default());
        assert!(service.transcribe(&[], &config()).is_err());
        assert!(service.transcribe(&wav(), &Config::default()).is_err());
        assert!(service.chat(&[], &config()).is_err());
        assert!(service
            .chat(&[message("tool", "unsupported")], &config())
            .is_err());
        assert!(service.synthesize("", &config()).is_err());
        assert!(service.transport.requests.is_empty());
    }

    #[test]
    fn tts_encodes_text_waits_for_success_and_downloads_mp3() {
        let mut mock = Mock::json(
            json!({"success":true,"isApiKeyValid":false,"audioStatusUrl":"https://audio1.tts.quest/id/status.json","mp3DownloadUrl":"https://audio1.tts.quest/id/audio.mp3"}),
        );
        mock.responses.push_back(Response {
            status: 200,
            body: br#"{"success":true,"isAudioReady":true,"isAudioError":false}"#.to_vec(),
        });
        mock.responses.push_back(Response {
            status: 200,
            body: b"ID3mock-audio".to_vec(),
        });
        let mut service = Services::new(mock);
        let mut settings = config();
        settings.voicevox_key = "a&b".into();
        assert_eq!(
            service.synthesize("あ &?", &settings).unwrap(),
            b"ID3mock-audio"
        );
        let requests = &service.transport.requests;
        assert!(requests[0].url.contains("text=%E3%81%82%20%26%3F"));
        assert!(requests[0].url.ends_with("&key=a%26b"));
        assert_eq!(requests[1].url, "https://audio1.tts.quest/id/status.json");
        assert_eq!(requests[2].limit, AUDIO_LIMIT);
    }

    #[test]
    fn tts_rejects_synthesis_errors_and_external_urls_before_download() {
        for value in [
            json!({"success":false}),
            json!({"success":true,"mp3DownloadUrl":"https://evil.example/audio.mp3"}),
        ] {
            let mut service = Services::new(Mock::json(value));
            assert!(service.synthesize("こんにちは", &config()).is_err());
            assert_eq!(service.transport.requests.len(), 1);
        }
        let mut mock = Mock::json(
            json!({"success":true,"audioStatusUrl":"https://audio2.tts.quest/status.json","mp3DownloadUrl":"https://audio2.tts.quest/audio.mp3"}),
        );
        mock.responses.push_back(Response {
            status: 200,
            body: br#"{"isAudioReady":false,"isAudioError":true}"#.to_vec(),
        });
        let mut service = Services::new(mock);
        assert!(service.synthesize("こんにちは", &config()).is_err());
        assert_eq!(service.transport.requests.len(), 2);
    }

    #[test]
    fn weather_builds_japanese_summary_and_handles_null_values() {
        let mut service = Services::new(Mock::json(
            json!({"current":{"temperature_2m":18.7,"weather_code":3},"daily":{"weather_code":[61],"temperature_2m_max":[22.4],"temperature_2m_min":[14.8],"precipitation_probability_max":[70]}}),
        ));
        assert_eq!(service.weather(&config()).unwrap(), "東京の今日の天気は雨、最高気温22度、最低気温15度、降水確率70パーセント。現在はくもりで気温19度。");
        assert!(service.transport.requests[0]
            .url
            .contains("latitude=35.6812&longitude=139.7671"));
        assert!(service.transport.requests[0]
            .url
            .contains("timezone=Asia%2FTokyo&forecast_days=1"));
        let mut service = Services::new(Mock::json(
            json!({"daily":{"weather_code":[0],"temperature_2m_max":[null]}}),
        ));
        assert_eq!(
            service.weather(&config()).unwrap(),
            "東京の今日の天気は快晴。"
        );
        assert_eq!(weather_code_ja(-1), "不明");
    }
}
