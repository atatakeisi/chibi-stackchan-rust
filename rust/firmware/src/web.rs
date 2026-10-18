//! Local setup UI and bounded command delivery to the device task.
//!
//! Rendering and request validation are host-testable. HTTP callbacks never
//! persist settings or perform network/audio work on the HTTP server thread.

use crate::{config::Config, conversation::Expression};
#[cfg(any(test, target_os = "espidf"))]
use std::sync::mpsc::SyncSender;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub const MAX_FORM_BYTES: usize = 32 * 1024;
const MAX_FIELD_BYTES: usize = 8 * 1024;
#[cfg(any(test, target_os = "espidf"))]
const MAX_TEXT_BYTES: usize = 4 * 1024;

// Do not derive Debug: SaveConfig can contain credentials.
pub enum Command {
    SaveConfig(BTreeMap<String, String>),
    SetRole(String),
    Speak(String),
    Chat(String),
    ClearKeys,
    SetExpression(Expression),
}

pub struct WebState {
    pub config: Config,
    pub role: String,
    pub portal: bool,
    pub notice: String,
    pub last_playback: Option<crate::playback::PlaybackStats>,
}

pub type SharedState = Arc<Mutex<WebState>>;

/// Read-only numeric diagnostics. Never serialize configuration, transcripts,
/// credentials or spoken text into the status response.
pub fn status_json(state: &WebState, volume: &crate::playback::VolumeControl) -> String {
    let live = volume.snapshot();
    serde_json::json!({
        "saved_volume": state.config.volume,
        "live_volume": live.volume,
        "last_nonzero_volume": live.last_nonzero_volume,
        "last_playback": state.last_playback,
        "orientation": crate::orientation_status::snapshot(),
    })
    .to_string()
}

#[cfg(any(test, target_os = "espidf"))]
fn requested_volume(command: &Command) -> Option<u8> {
    match command {
        Command::SaveConfig(form) => form
            .get("volume")
            .and_then(|value| value.trim().parse().ok()),
        _ => None,
    }
}

/// Decode an `application/x-www-form-urlencoded` body strictly and without
/// replacing malformed UTF-8. Duplicate names are rejected to avoid ambiguity.
pub fn parse_form(encoded: &str) -> Result<BTreeMap<String, String>, String> {
    if encoded.len() > MAX_FORM_BYTES {
        return Err("送信内容が大きすぎます".into());
    }
    let mut form = BTreeMap::new();
    for entry in encoded.split('&').filter(|entry| !entry.is_empty()) {
        if form.len() >= 32 {
            return Err("項目が多すぎます".into());
        }
        let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
        let name = decode_component(name)?;
        let value = decode_component(value)?;
        if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
            return Err("項目名が正しくありません".into());
        }
        if value.len() > MAX_FIELD_BYTES {
            return Err("入力内容が長すぎます".into());
        }
        if form.insert(name, value).is_some() {
            return Err("同じ項目を複数回指定できません".into());
        }
    }
    Ok(form)
}

fn decode_component(encoded: &str) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut source = encoded.bytes();
    while let Some(byte) = source.next() {
        bytes.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hi = source.next().and_then(hex_digit);
                let lo = source.next().and_then(hex_digit);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => (hi << 4) | lo,
                    _ => return Err("URL エンコードが正しくありません".into()),
                }
            }
            byte => byte,
        });
    }
    if bytes.contains(&0) {
        return Err("使用できない文字が含まれています".into());
    }
    String::from_utf8(bytes).map_err(|_| "UTF-8 の文字列を送信してください".into())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character => escaped.push(character),
        }
    }
    escaped
}

fn page(title: &str, content: &str) -> String {
    format!(
        r#"<!doctype html><html lang="ja"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{}</title><style>
body{{font-family:system-ui,sans-serif;background:#111318;color:#edf0f6;margin:0;line-height:1.7}}
main{{max-width:600px;margin:auto;padding:20px}}h1{{font-size:24px}}a{{color:#9bbdff}}
fieldset{{border:1px solid #414651;border-radius:12px;padding:16px;margin:18px 0}}
label{{display:block;margin-top:12px}}input,select,textarea,button{{font:inherit;box-sizing:border-box}}
input:not([type=checkbox]),select,textarea{{width:100%;padding:10px;border-radius:7px;border:1px solid #596171;background:#222631;color:#fff}}
textarea{{min-height:220px}}button{{padding:11px 18px;background:#2868db;border:0;border-radius:8px;color:white;margin-top:14px;cursor:pointer}}
.danger{{background:#a1323d}}.badge{{padding:10px;border-radius:8px;background:#173b2b}}.setup{{background:#713e13}}
small{{color:#c2c8d3}}summary{{cursor:pointer}}.notice{{padding:12px;border:1px solid #7088b5;border-radius:8px;white-space:pre-wrap}}
</style></head><body><main><h1>{}</h1>{}<p><a href="/">メニューへ戻る</a></p></main></body></html>"#,
        escape_html(title),
        escape_html(title),
        content
    )
}

pub fn menu_page(portal: bool) -> String {
    let intro = if portal {
        r#"<p class="badge setup">はじめの設定モード</p><p>Wi-Fi に接続する設定を行ってください。</p>"#
    } else {
        r#"<p class="badge">通常モード</p>"#
    };
    page(
        "ちびスタックちゃん",
        &format!(
            r#"{intro}<p><a href="/config">設定（Wi-Fi・APIキー・声・呼びかけ・天気）</a></p>
<p><a href="/role">ロール（キャラ設定）</a></p>
<form method="post" action="/speech"><label for="say">しゃべらせる</label><input id="say" name="say" maxlength="1024" required><button>読み上げる</button></form>
<form method="post" action="/chat"><label for="text">文字で質問する</label><input id="text" name="text" maxlength="1024" required><button>質問する</button></form>
<form method="post" action="/face"><label for="expression">表情</label><select id="expression" name="expression">
<option value="0">ふつう</option><option value="1">うれしい</option><option value="2">ねむい</option><option value="3">ふしぎ</option><option value="4">かなしい</option><option value="5">おこる</option></select><button>表情を変える</button></form>"#
        ),
    )
}

/// Saved password and API-key values are deliberately never included in HTML.
pub fn config_page(config: &Config, portal: bool) -> String {
    let mut content = String::new();
    content.push_str(if portal {
        r#"<p class="badge setup">はじめの設定モード</p><p>2.4 GHz の Wi-Fi に接続します。保存後、スマホやパソコンをおうちの Wi-Fi に戻してください。</p>"#
    } else {
        r#"<p class="badge">通常モード</p>"#
    });
    content.push_str(r#"<p>空欄の項目は現在の値を引き継ぎます。</p><form method="post" action="/config_set"><fieldset><legend>Wi-Fi</legend>"#);
    input(
        &mut content,
        "ssid",
        "Wi-Fi の名前（SSID）",
        "text",
        &config.wifi_ssid,
        "maxlength=32",
    );
    input(
        &mut content,
        "pass",
        "新しい Wi-Fi パスワード",
        "password",
        "",
        "maxlength=64 autocomplete=new-password",
    );
    content.push_str(r#"<label><input type="checkbox" name="wifi_open" value="1"> パスワードなしの Wi-Fi を使う</label></fieldset>"#);
    if portal {
        content.push_str("<details><summary>APIキー・声などの詳細設定</summary>");
    }
    content.push_str("<fieldset><legend>会話・音声の APIキー</legend><small>設定済みのキーは表示しません。変更するときだけ入力してください。</small>");
    for (name, label) in [
        ("gemini", "Gemini（会話）"),
        ("claude", "Claude（会話）"),
        ("openai", "OpenAI（音声認識）"),
        ("voicevox", "VOICEVOX（音声合成）"),
    ] {
        input(
            &mut content,
            name,
            label,
            "password",
            "",
            "maxlength=2048 autocomplete=new-password",
        );
    }
    content
        .push_str(r#"<label for="provider">会話 AI</label><select id="provider" name="provider">"#);
    option(
        &mut content,
        "gemini",
        "Gemini",
        config.provider == "gemini",
    );
    option(
        &mut content,
        "claude",
        "Claude",
        config.provider == "claude",
    );
    content.push_str("</select></fieldset><fieldset><legend>呼びかけ</legend>");
    input(
        &mut content,
        "wake",
        "呼びかけワード（カンマ区切りで複数指定）",
        "text",
        &config.wake_phrase,
        "maxlength=128",
    );
    content.push_str(
        r#"<label for="wake_en">呼びかけで起動</label><select id="wake_en" name="wake_en">"#,
    );
    for (mode, label) in [
        (1, "呼びかけワードで起動"),
        (2, "なんでも返事"),
        (0, "無効（画面タッチで起動）"),
    ] {
        option(
            &mut content,
            &mode.to_string(),
            label,
            config.wake_mode == mode,
        );
    }
    content.push_str("</select>");
    input(
        &mut content,
        "vad",
        "音量トリガ感度（小さいほど敏感）",
        "number",
        &config.vad_threshold.to_string(),
        "min=200 max=8000",
    );
    content.push_str("</fieldset><fieldset><legend>声・音</legend>");
    input(
        &mut content,
        "speaker",
        "VOICEVOX 話者番号",
        "number",
        &config.speaker.to_string(),
        "min=0 max=60",
    );
    input(
        &mut content,
        "volume",
        "スピーカー音量",
        "number",
        &config.volume.to_string(),
        "min=0 max=255",
    );
    content.push_str(
        "<small>声でも「音量を下げて」「音量を10にして」「ミュート解除」で変更できます。</small>",
    );
    input(
        &mut content,
        "mic",
        "マイク感度",
        "number",
        &config.mic_gain.to_string(),
        "min=1 max=255",
    );
    content.push_str("</fieldset><fieldset><legend>天気の場所</legend>");
    input(
        &mut content,
        "place",
        "地名（読み上げ用）",
        "text",
        &config.weather_place,
        "maxlength=64",
    );
    input(
        &mut content,
        "lat",
        "緯度",
        "number",
        &config.latitude.to_string(),
        "min=-90 max=90 step=any",
    );
    input(
        &mut content,
        "lon",
        "経度",
        "number",
        &config.longitude.to_string(),
        "min=-180 max=180 step=any",
    );
    content.push_str("</fieldset>");
    if portal {
        content.push_str("</details>");
    }
    content.push_str(r#"<button type="submit">保存する</button></form>
<details><summary>Wi-Fi と APIキーを消去する</summary><p>消去後は設定モードで再起動します。Wi-Fi と APIキーの再入力が必要です。</p>
<form method="post" action="/clear_keys" onsubmit="return confirm('Wi-Fi と APIキーをすべて消去します。再入力できることを確認しましたか？')">
<label><input type="checkbox" name="confirm" value="clear" required> 再入力に必要な情報を用意しました</label><button class="danger">Wi-Fi・APIキーを消去する</button></form></details>"#);
    page("設定", &content)
}

fn input(content: &mut String, name: &str, label: &str, kind: &str, value: &str, attributes: &str) {
    content.push_str(&format!(
        r#"<label for="{name}">{}</label><input id="{name}" name="{name}" type="{kind}" value="{}" {attributes}>"#,
        escape_html(label), escape_html(value)
    ));
}

fn option(content: &mut String, value: &str, label: &str, selected: bool) {
    content.push_str(&format!(
        r#"<option value="{}"{}>{}</option>"#,
        escape_html(value),
        if selected { " selected" } else { "" },
        escape_html(label)
    ));
}

pub fn role_page(role: &str) -> String {
    page(
        "ロール（キャラ設定）",
        &format!(
            r#"<p>キャラ設定の文章を入力してください。空欄で保存すると既定に戻ります。表情・タイマー用のルールは自動で追加されます。</p>
<form method="post" action="/role_set"><label for="role">現在のキャラ設定</label><textarea id="role" name="role" maxlength="2700">{}</textarea><button>ロールを保存する</button></form>"#,
            escape_html(role)
        ),
    )
}

#[cfg(any(test, target_os = "espidf"))]
fn validate_text(text: &str, limit: usize, allow_empty: bool) -> Result<String, String> {
    let text = text.trim();
    if (!allow_empty && text.is_empty()) || text.len() > limit {
        return Err("入力が空欄、または長すぎます".into());
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("使用できない文字が含まれています".into());
    }
    Ok(text.to_owned())
}

/// Validate against a snapshot; settings become visible only after the device
/// task has successfully persisted them.
#[cfg(any(test, target_os = "espidf"))]
fn command_for(
    path: &str,
    content_type: &str,
    body: &str,
    config: &Config,
) -> Result<Command, String> {
    if body.len() > MAX_FORM_BYTES {
        return Err("送信内容が大きすぎます".into());
    }
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    if path == "/role_set" && media_type.eq_ignore_ascii_case("text/plain") {
        return validate_text(body, MAX_FIELD_BYTES, true).map(Command::SetRole);
    }
    if !media_type.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
        return Err("フォーム形式で送信してください".into());
    }
    let mut form = parse_form(body)?;
    match path {
        "/config_set" | "/wifi_set" | "/setting" => {
            if path == "/setting"
                && form
                    .keys()
                    .any(|name| !matches!(name.as_str(), "volume" | "speaker" | "mic"))
            {
                return Err("音量・話者・マイク感度を指定してください".into());
            }
            let mut candidate = config.clone();
            candidate.apply_form(&form)?;
            Ok(Command::SaveConfig(form))
        }
        "/role_set" => validate_text(
            form.get("role").ok_or("role がありません")?,
            MAX_FIELD_BYTES,
            true,
        )
        .map(Command::SetRole),
        "/speech" => validate_text(
            form.get("say").ok_or("say がありません")?,
            MAX_TEXT_BYTES,
            false,
        )
        .map(Command::Speak),
        "/chat" => validate_text(
            form.get("text").ok_or("text がありません")?,
            MAX_TEXT_BYTES,
            false,
        )
        .map(Command::Chat),
        "/clear_keys" => {
            if form.remove("confirm").as_deref() != Some("clear") {
                return Err("消去の確認にチェックを入れてください".into());
            }
            Ok(Command::ClearKeys)
        }
        "/face" => {
            let expression = match form.get("expression").map(String::as_str) {
                Some("0" | "neutral") => Expression::Neutral,
                Some("1" | "happy") => Expression::Happy,
                Some("2" | "sleepy") => Expression::Sleepy,
                Some("3" | "doubt") => Expression::Doubt,
                Some("4" | "sad") => Expression::Sad,
                Some("5" | "angry") => Expression::Angry,
                _ => return Err("表情は 0〜5 で指定してください".into()),
            };
            Ok(Command::SetExpression(expression))
        }
        _ => Err("ページが見つかりません".into()),
    }
}

#[cfg(any(test, target_os = "espidf"))]
fn enqueue(sender: &SyncSender<Command>, command: Command) -> Result<(), &'static str> {
    use std::sync::mpsc::TrySendError;
    sender.try_send(command).map_err(|error| match error {
        TrySendError::Full(_) => "いま処理中です。少し待ってから再度お試しください",
        TrySendError::Disconnected(_) => "処理を受け付けられません。本体を再起動してください",
    })
}

#[cfg(any(test, target_os = "espidf"))]
fn enqueue_with_volume(
    sender: &SyncSender<Command>,
    command: Command,
    volume: &crate::playback::VolumeControl,
) -> Result<(), &'static str> {
    let before = volume.snapshot();
    // Publish before the device can receive the command. A successful receive
    // must never persist an older live value just because HTTP was preempted.
    let published = requested_volume(&command).map(|requested| volume.replace(requested));
    let result = enqueue(sender, command);
    if result.is_err() {
        if let Some(published) = published {
            volume.restore_if_unchanged(published, before);
        }
    }
    result
}

#[cfg(target_os = "espidf")]
pub fn start(
    state: SharedState,
    sender: SyncSender<Command>,
    volume: crate::playback::VolumeControl,
) -> anyhow::Result<esp_idf_svc::http::server::EspHttpServer<'static>> {
    esp::start(state, sender, volume)
}

#[cfg(target_os = "espidf")]
mod esp {
    use super::*;
    use esp_idf_svc::{
        http::{
            server::{Configuration, EspHttpConnection, EspHttpServer, Request},
            Method,
        },
        io::Write,
    };

    type HttpRequest<'a, 'b> = Request<&'a mut EspHttpConnection<'b>>;

    pub fn start(
        state: SharedState,
        sender: SyncSender<Command>,
        volume: crate::playback::VolumeControl,
    ) -> anyhow::Result<EspHttpServer<'static>> {
        let mut server = EspHttpServer::new(&Configuration {
            stack_size: 10 * 1024,
            uri_match_wildcard: true,
            ..Default::default()
        })?;
        for path in ["/", "/config", "/wifi", "/role", "/role_get"] {
            let state = state.clone();
            server.fn_handler::<anyhow::Error, _>(path, Method::Get, move |request| {
                let state = state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("web state unavailable"))?;
                let mut html = match path {
                    "/config" | "/wifi" => config_page(&state.config, state.portal),
                    "/role" | "/role_get" => role_page(&state.role),
                    _ if state.portal => config_page(&state.config, true),
                    _ => menu_page(false),
                };
                if !state.notice.is_empty() {
                    html = html.replacen(
                        "<main>",
                        &format!(
                            "<main><p class=\"notice\" role=\"status\">{}</p>",
                            escape_html(&state.notice)
                        ),
                        1,
                    );
                }
                drop(state);
                respond(request, 200, &html)
            })?;
        }
        {
            let state = state.clone();
            let volume = volume.clone();
            server.fn_handler::<anyhow::Error, _>("/status", Method::Get, move |request| {
                let body = {
                    let state = state
                        .lock()
                        .map_err(|_| anyhow::anyhow!("web state unavailable"))?;
                    status_json(&state, &volume)
                };
                request
                    .into_response(
                        200,
                        None,
                        &[
                            ("Content-Type", "application/json; charset=utf-8"),
                            ("Cache-Control", "no-store"),
                            ("X-Content-Type-Options", "nosniff"),
                        ],
                    )?
                    .write_all(body.as_bytes())?;
                Ok(())
            })?;
        }
        for path in [
            "/config_set",
            "/wifi_set",
            "/role_set",
            "/speech",
            "/chat",
            "/clear_keys",
            "/face",
            "/setting",
        ] {
            let state = state.clone();
            let sender = sender.clone();
            let volume = volume.clone();
            server.fn_handler::<anyhow::Error, _>(path, Method::Post, move |mut request| {
                let content_type = request.header("Content-Type").unwrap_or("").to_owned();
                let body = match read_body(&mut request) {
                    Ok(body) => body,
                    Err((status, message)) => return respond(request, status, &page("入力を確認してください", &escape_html(message))),
                };
                let command = {
                    let state = state.lock().map_err(|_| anyhow::anyhow!("web state unavailable"))?;
                    command_for(path, &content_type, &body, &state.config)
                };
                let command = match command {
                    Ok(command) => command,
                    Err(message) => return respond(request, 400, &page("入力を確認してください", &escape_html(&message))),
                };
                let submitted = {
                    // Device commits use the same settings gate, so they can
                    // never persist the temporary value of a rejected request.
                    let _submission = state.lock().map_err(|_| anyhow::anyhow!("web state unavailable"))?;
                    enqueue_with_volume(&sender, command, &volume)
                };
                match submitted {
                    Ok(()) => {
                        respond(request, 202, &page("受け付けました", "<p>順番に処理しています。結果はメニューで確認できます。Wi-Fi の変更・消去後は本体が再起動します。</p>"))
                    },
                    Err(message) => respond(request, 503, &page("少し待ってください", &escape_html(message))),
                }
            })?;
        }
        // Captive-portal probes are read-only. Old GET mutation URLs never enqueue.
        server.fn_handler::<anyhow::Error, _>("/*", Method::Get, move |request| {
            let path = request.uri().split('?').next().unwrap_or("");
            if matches!(
                path,
                "/config_set"
                    | "/wifi_set"
                    | "/role_set"
                    | "/speech"
                    | "/chat"
                    | "/clear_keys"
                    | "/face"
                    | "/setting"
            ) {
                return respond(
                    request,
                    405,
                    &page(
                        "送信方法を確認してください",
                        "<p>メニューのフォームから操作してください。</p>",
                    ),
                );
            }
            let state = state
                .lock()
                .map_err(|_| anyhow::anyhow!("web state unavailable"))?;
            let (status, html) = if state.portal {
                (200, config_page(&state.config, true))
            } else {
                (404, page("ページが見つかりません", ""))
            };
            drop(state);
            respond(request, status, &html)
        })?;
        Ok(server)
    }

    fn read_body(request: &mut HttpRequest<'_, '_>) -> Result<String, (u16, &'static str)> {
        if request.header("Transfer-Encoding").is_some() {
            return Err((400, "Content-Length を指定して送信してください"));
        }
        let len = request
            .header("Content-Length")
            .ok_or((411, "本文の長さが指定されていません"))?
            .parse::<usize>()
            .map_err(|_| (400, "本文の長さが正しくありません"))?;
        if len > MAX_FORM_BYTES {
            return Err((413, "送信内容が大きすぎます"));
        }
        let mut body = vec![0; len];
        let mut read = 0;
        while read < len {
            match request.read(&mut body[read..]) {
                Ok(0) | Err(_) => return Err((400, "本文を最後まで読み込めませんでした")),
                Ok(count) => read += count,
            }
        }
        String::from_utf8(body).map_err(|_| (400, "UTF-8 の文字列を送信してください"))
    }

    fn respond(request: HttpRequest<'_, '_>, status: u16, html: &str) -> anyhow::Result<()> {
        request
            .into_response(
                status,
                None,
                &[
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-store"),
                    ("X-Content-Type-Options", "nosniff"),
                ],
            )?
            .write_all(html.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FORM: &str = "application/x-www-form-urlencoded; charset=UTF-8";

    #[test]
    fn status_reports_live_output_separately_without_private_content() {
        let state = WebState {
            config: Config {
                volume: 5,
                openai_key: "private-key".into(),
                ..Config::default()
            },
            role: "private-role".into(),
            portal: false,
            notice: "private-notice".into(),
            last_playback: Some(crate::playback::PlaybackStats {
                source_peak: 25_500,
                output_peak: 1_500,
                volume_min: 15,
                volume_max: 15,
                frames: 320,
                source_fingerprint: 123,
            }),
        };
        let volume = crate::playback::VolumeControl::new(15);
        let json = status_json(&state, &volume);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["saved_volume"], 5);
        assert_eq!(value["live_volume"], 15);
        assert_eq!(value["last_playback"]["volume_min"], 15);
        let orientation = value["orientation"].as_object().unwrap();
        assert_eq!(orientation.len(), 7);
        assert!(orientation["rotation"].as_u64().unwrap() < 4);
        assert!(orientation["sample_count"].is_u64());
        assert!(orientation["imu_enabled"].is_boolean());
        assert!(orientation["reset_reason"].is_u64());
        assert!(orientation["uptime_ms"].is_u64());
        assert!(orientation.contains_key("accel_mg"));
        assert!(orientation.contains_key("last_sample_ms"));
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert!(!json.contains("private"));
    }

    #[test]
    fn decodes_japanese_spaces_and_encoded_delimiters() {
        let form = parse_form(
            "text=%E3%81%93%E3%82%93%E3%81%AB%E3%81%A1%E3%81%AF+%26%3D%2B&empty=&plain=日本語",
        )
        .unwrap();
        assert_eq!(form["text"], "こんにちは &=+");
        assert_eq!(form["plain"], "日本語");
        assert_eq!(form["empty"], "");
    }

    #[test]
    fn rejects_malformed_ambiguous_and_oversized_forms() {
        for body in [
            "x=%",
            "x=%0",
            "x=%GG",
            "x=%FF",
            "x=%C0%AF",
            "x=%00",
            "x=1&%78=2",
            "=value",
        ] {
            assert!(parse_form(body).is_err(), "accepted {body}");
        }
        assert!(parse_form(&format!("x={}", "a".repeat(MAX_FORM_BYTES))).is_err());
        assert!(parse_form(&format!("x={}", "a".repeat(MAX_FIELD_BYTES + 1))).is_err());
        assert!(parse_form(
            &(0..33)
                .map(|n| format!("k{n}=v"))
                .collect::<Vec<_>>()
                .join("&")
        )
        .is_err());
    }

    #[test]
    fn pages_escape_stored_text_and_never_render_saved_secrets() {
        let config = Config {
            wifi_ssid: "\" onfocus=\"alert(1)<script>&".into(),
            wake_phrase: "<img src=x onerror=alert(1)>".into(),
            wifi_pass: "wifi-super-secret".into(),
            gemini_key: "gemini-super-secret".into(),
            claude_key: "claude-super-secret".into(),
            openai_key: "openai-super-secret".into(),
            voicevox_key: "voicevox-super-secret".into(),
            ..Config::default()
        };
        for portal in [false, true] {
            let html = config_page(&config, portal);
            for secret in [
                &config.wifi_pass,
                &config.gemini_key,
                &config.claude_key,
                &config.openai_key,
                &config.voicevox_key,
            ] {
                assert!(!html.contains(secret));
            }
            assert!(!html.contains("<img"));
            assert!(html.contains("&quot; onfocus=&quot;"));
            assert!(html.contains("method=\"post\" action=\"/clear_keys\""));
            assert!(html.contains("confirm('"));
        }
        let html = role_page("</textarea><script>alert('x')</script>");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;/textarea&gt;"));
        assert_eq!(escape_html("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    }

    #[test]
    fn config_commands_are_validated_without_mutating_saved_state() {
        let config = Config::default();
        let original = config.clone();
        assert!(command_for("/config_set", FORM, "ssid=new&volume=999", &config).is_err());
        let command = command_for("/wifi_set", FORM, "ssid=new&volume=55", &config).unwrap();
        assert!(
            matches!(command, Command::SaveConfig(form) if form["ssid"] == "new" && form["volume"] == "55")
        );
        assert_eq!(config, original);
        assert!(command_for("/setting", FORM, "gemini=key", &config).is_err());
        assert!(command_for("/config_set", "application/json", "{}", &config).is_err());
    }

    #[test]
    fn live_volume_matches_validated_settings_including_whitespace_and_mute() {
        let config = Config::default();
        for (body, expected) in [
            ("volume=+0+", Some(0)),
            ("volume=5", Some(5)),
            ("volume=", None),
            ("mic=3", None),
        ] {
            let command = command_for("/setting", FORM, body, &config).unwrap();
            assert_eq!(requested_volume(&command), expected);
            if let Command::SaveConfig(form) = command {
                let mut saved = config.clone();
                saved.apply_form(&form).unwrap();
                if let Some(volume) = expected {
                    assert_eq!(saved.volume, volume);
                }
            }
        }
        assert!(command_for("/setting", FORM, "volume=256", &config).is_err());
        assert_eq!(requested_volume(&Command::Speak("test".into())), None);
    }

    #[test]
    fn validates_commands_and_explicit_clear_confirmation() {
        let config = Config::default();
        assert!(command_for("/clear_keys", FORM, "", &config).is_err());
        assert!(matches!(
            command_for("/clear_keys", FORM, "confirm=clear", &config),
            Ok(Command::ClearKeys)
        ));
        assert!(
            matches!(command_for("/role_set", "text/plain;charset=UTF-8", "  日本語\nロール  ", &config), Ok(Command::SetRole(role)) if role == "日本語\nロール")
        );
        assert!(
            matches!(command_for("/role_set", FORM, "role=", &config), Ok(Command::SetRole(role)) if role.is_empty())
        );
        assert!(
            matches!(command_for("/chat", FORM, "text=こんにちは", &config), Ok(Command::Chat(text)) if text == "こんにちは")
        );
        assert!(command_for("/speech", FORM, "say=+", &config).is_err());
        assert!(command_for("/face", FORM, "expression=6", &config).is_err());
        assert!(matches!(
            command_for("/face", FORM, "expression=2", &config),
            Ok(Command::SetExpression(Expression::Sleepy))
        ));
    }

    #[test]
    fn full_queue_returns_immediately_and_preserves_saved_state() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        assert!(enqueue(&sender, Command::Speak("first".into())).is_ok());
        assert!(enqueue(&sender, Command::ClearKeys).is_err());
        assert!(matches!(receiver.try_recv(), Ok(Command::Speak(text)) if text == "first"));
        drop(receiver);
        assert!(enqueue(&sender, Command::ClearKeys).is_err());
    }

    #[test]
    fn queued_settings_publish_volume_before_the_consumer_can_persist_it() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let volume = crate::playback::VolumeControl::new(10);
        let command = command_for("/setting", FORM, "volume=8", &Config::default()).unwrap();
        enqueue_with_volume(&sender, command, &volume).unwrap();
        assert_eq!(volume.get(), 8);
        assert!(matches!(receiver.try_recv(), Ok(Command::SaveConfig(_))));
        let current = volume.snapshot();
        assert!(volume.set_if_unchanged(current, 6));
        let command = command_for("/setting", FORM, "volume=5", &Config::default()).unwrap();
        enqueue_with_volume(&sender, command, &volume).unwrap();
        assert_eq!(volume.get(), 5);
    }

    #[test]
    fn rejected_volume_request_restores_live_value_and_other_commands_do_not_change_it() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let volume = crate::playback::VolumeControl::new(10);
        enqueue_with_volume(&sender, Command::Speak("first".into()), &volume).unwrap();
        let command = command_for("/setting", FORM, "volume=20", &Config::default()).unwrap();
        assert!(enqueue_with_volume(&sender, command, &volume).is_err());
        assert_eq!(volume.get(), 10);
        drop(receiver);
        let command = command_for("/setting", FORM, "volume=0", &Config::default()).unwrap();
        assert!(enqueue_with_volume(&sender, command, &volume).is_err());
        assert_eq!(volume.get(), 10);
    }
}
