//! The single device task owns the codec, display, servo and persistent writes.
//! HTTP callbacks enqueue commands and publish a live volume value. Only this
//! task accesses hardware, so microphone and playback cannot race.

use crate::{
    audio::{self, Recording, Vad, VAD_CHUNK_SAMPLES, WARMUP_CHUNKS},
    board::{Board, FaceState},
    config::Config,
    conversation::{
        accept_transcript, strip_command_wake_prefix, Conversation, Expression, Reply,
        WakeDecision, DEFAULT_ROLE,
    },
    network::{self, Network},
    playback::VolumeControl,
    services::{HttpTransport, Services},
    storage::Storage,
    voice_action,
    voice_volume::{self, VolumeCommand},
    web::{self, Command, SharedState, WebState},
};
use anyhow::{anyhow, Context, Result};
use esp_idf_svc::{hal::peripherals::Peripherals, nvs::EspDefaultNvsPartition, sys};
use std::{
    sync::{
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const SLEEP_AFTER_MS: u64 = 180_000;
const HOLD_FOR_PORTAL_MS: u64 = 3_000;
const OFFLINE_NOTICE_INTERVAL_MS: u64 = 30_000;
const WAKE_SPEECH: &str = "じいいいいええええ！寝てしまった！";
const OFFLINE_SPEECH: &str = "あれぇ、ワイファイ来てますかぁ？";

pub fn run() -> Result<()> {
    Device::run()
}

struct Device {
    board: Board,
    network: Network,
    storage: Storage,
    services: Services<HttpTransport>,
    conversation: Conversation,
    config: Config,
    volume: VolumeControl,
    shared: SharedState,
    commands: Receiver<Command>,
    vad: Vad,
    warmup_remaining: usize,
    started: Instant,
    last_activity_ms: u64,
    last_network_ms: u64,
    last_portal_display_ms: u64,
    last_offline_notice_ms: Option<u64>,
    microphone_retry_ms: u64,
    button_since_ms: Option<u64>,
    button_consumed: bool,
    sleeping: bool,
    pending_alarm: Option<Reply>,
}

#[derive(Clone, Copy)]
enum ButtonAction {
    Tap,
    Portal,
}

impl Device {
    fn run() -> Result<()> {
        let peripherals = Peripherals::take().context("taking device peripherals")?;
        // Preserve existing settings if the storage format cannot be opened.
        // The default take() retries after erasing NVS on version/full errors.
        let nvs = EspDefaultNvsPartition::take_with(false)
            .context("opening device storage without erasing")?;
        let storage = Storage::new(nvs.clone())?;
        let (config, role, recovery) = match storage.load() {
            Ok((config, role)) => (config, role, false),
            Err(_) => {
                // Preserve unreadable saved data and offer setup, without
                // leaking stored values through a deserialization error.
                log::warn!("Saved settings could not be read; opening setup");
                (Config::default(), DEFAULT_ROLE.to_owned(), true)
            }
        };
        let volume = VolumeControl::new_with_restore(config.volume, config.last_nonzero_volume);
        let (mut board, modem) = Board::new(peripherals, volume.clone())?;
        board
            .display
            .show_status(&["Starting Rust", "Connecting Wi-Fi..."])?;
        let force_portal = recovery || board.button_pressed();
        let network = Network::new(modem, nvs, &config, force_portal)?;
        let shared = Arc::new(Mutex::new(WebState {
            config: config.clone(),
            role: role.clone(),
            portal: network.portal,
            notice: if recovery {
                "保存済み設定を読み込めなかったため設定モードで起動しました。".into()
            } else {
                String::new()
            },
            last_playback: None,
        }));
        let (sender, commands) = mpsc::sync_channel(8);
        // Keep the server alive for the complete lifetime of the device loop.
        let _server = web::start(shared.clone(), sender, volume.clone())?;
        let mut device = Self {
            board,
            network,
            storage,
            services: Services::new(HttpTransport),
            conversation: Conversation::new(&role),
            vad: Vad::new(i32::from(config.vad_threshold)),
            config,
            volume,
            shared,
            commands,
            warmup_remaining: WARMUP_CHUNKS,
            started: Instant::now(),
            last_activity_ms: 0,
            last_network_ms: 0,
            last_portal_display_ms: 0,
            last_offline_notice_ms: None,
            microphone_retry_ms: 0,
            button_since_ms: None,
            button_consumed: false,
            sleeping: false,
            pending_alarm: None,
        };
        device.board.microphone()?;
        if device.network.portal {
            device.show_portal()?;
        } else {
            device.board.state = FaceState::Idle;
            let address = format!("http://{}/", device.network.ip());
            device
                .board
                .display
                .show_status(&["Rust firmware ready", &address])?;
            device.notice(&format!("起動しました。設定画面: {address}"));
            std::thread::sleep(Duration::from_millis(1_000));
            // Match the existing boot greeting once STT has been configured.
            // An unconfigured device does not consume cloud calls at boot.
            if !device.config.openai_key.is_empty() && device.network.connected() {
                let name = device
                    .config
                    .wake_phrase
                    .split([',', '、'])
                    .next()
                    .unwrap_or("スタックちゃん")
                    .trim();
                let greeting =
                    format!("こんにちは、ちびスタックちゃんだよ。「{name}」って呼んでね。");
                if device.speak(&greeting, Expression::Happy, None).is_err() {
                    log::warn!("Boot greeting unavailable; continuing to standby");
                    device.recover_capture();
                }
            }
        }
        loop {
            if device.step().is_err() {
                // Runtime failures must never leave a busy/playback flag set.
                // Error chains may contain third-party diagnostics; log only
                // the operation, and expose a fixed message through the UI.
                log::warn!("Device operation failed; recovering standby");
                device.notice("処理に失敗しました。接続とAPIキーの設定を確認してください。");
                device.recover_capture();
                std::thread::sleep(Duration::from_millis(100));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    fn step(&mut self) -> Result<()> {
        let now = self.now_ms();
        if now.saturating_sub(self.last_network_ms) >= 1_000 {
            self.last_network_ms = now;
            let previous_portal = self.network.portal;
            if self.network.maintain().is_err() {
                log::warn!("Wi-Fi maintenance failed; retry scheduled");
            }
            if self.network.portal != previous_portal {
                self.sync_web()?;
                self.vad.reset();
                self.warmup_remaining = WARMUP_CHUNKS;
                if self.network.portal {
                    self.show_portal()?;
                }
            }
        }
        self.board.tick(now)?;
        if let Some(action) = self.poll_button(now) {
            match action {
                ButtonAction::Portal => self.enter_portal()?,
                ButtonAction::Tap if !self.network.portal => {
                    self.awake();
                    if self.network.connected() {
                        self.listen(false)?;
                    } else {
                        self.offline_notice()?;
                    }
                }
                ButtonAction::Tap => self.show_portal()?,
            }
        }
        // Process one command at a time, with no settings lock held during
        // storage, HTTP, MP3 decoding, display drawing or microphone reads.
        if let Ok(command) = self.commands.try_recv() {
            self.handle_command(command)?;
        }
        self.queue_alarm()?;
        if let Some(reply) = self.pending_alarm.take() {
            if self.network.connected() && !self.network.portal {
                self.speak_reply(&reply)?;
            }
        }
        if self.network.portal {
            if now.saturating_sub(self.last_portal_display_ms) >= 1_000 {
                self.show_portal()?;
            }
            std::thread::sleep(Duration::from_millis(10));
            return Ok(());
        }
        if !self.sleeping && now.saturating_sub(self.last_activity_ms) >= SLEEP_AFTER_MS {
            self.sleeping = true;
            self.board.expression = Expression::Sleepy;
            self.board.state = FaceState::Idle;
            self.board.focus(true);
            self.board.tick(self.now_ms())?;
            self.prepare_cached_voices();
        }
        if now < self.microphone_retry_ms {
            return Ok(());
        }
        let mut chunk = [0i16; VAD_CHUNK_SAMPLES];
        if self
            .board
            .read_microphone(&mut chunk, self.config.mic_gain)
            .is_err()
        {
            log::warn!("Microphone read failed; retrying after cooldown");
            self.microphone_retry_ms = self.now_ms() + 3_000;
            self.recover_capture();
            return Ok(());
        }
        if self.warmup_remaining == 1 {
            let peak = chunk
                .iter()
                .map(|sample| sample.unsigned_abs())
                .max()
                .unwrap_or(0);
            log::info!(
                "Microphone warmup complete: {} samples, peak={peak}",
                chunk.len()
            );
        }
        self.warmup_remaining = self.warmup_remaining.saturating_sub(1);
        let trigger = self.vad.push(&chunk, self.board.servo_noise(self.now_ms()));
        if trigger && self.config.wake_mode != 0 {
            if self.sleeping {
                self.awake();
                self.speak(WAKE_SPEECH, Expression::Happy, Some("wake"))?;
            } else if self.network.connected() {
                self.listen(true)?;
            } else {
                self.offline_notice()?;
                self.vad.reset();
                self.warmup_remaining = WARMUP_CHUNKS;
            }
        }
        Ok(())
    }

    fn poll_button(&mut self, now: u64) -> Option<ButtonAction> {
        if self.board.button_pressed() {
            let started = *self.button_since_ms.get_or_insert(now);
            if !self.button_consumed && now.saturating_sub(started) >= HOLD_FOR_PORTAL_MS {
                self.button_consumed = true;
                return Some(ButtonAction::Portal);
            }
        } else if self.button_since_ms.take().is_some() {
            let consumed = std::mem::replace(&mut self.button_consumed, false);
            if !consumed {
                return Some(ButtonAction::Tap);
            }
        }
        None
    }

    fn handle_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::SaveConfig(form) => {
                let shared = self.shared.clone();
                let submission = shared
                    .lock()
                    .map_err(|_| anyhow!("web state unavailable"))?;
                let mut next = self.config.clone();
                next.apply_form(&form).map_err(|message| anyhow!(message))?;
                // HTTP already publishes volume as soon as it accepts a
                // setting. A queued older form must not undo a newer voice or
                // Web change; persist the currently effective volume instead.
                let live = self.volume.snapshot();
                next.set_volume(live.volume);
                next.last_nonzero_volume = live.last_nonzero_volume;
                let wifi_changed = next.wifi_ssid != self.config.wifi_ssid
                    || next.wifi_pass != self.config.wifi_pass;
                let microphone_changed = next.mic_gain != self.config.mic_gain;
                // Write and commit the complete validated snapshot first.
                self.storage.save_config(&next)?;
                self.config = next;
                drop(submission);
                self.vad.threshold = i32::from(self.config.vad_threshold);
                self.sync_web()?;
                self.notice("設定を保存しました。");
                if wifi_changed {
                    self.notice("Wi-Fi設定を保存しました。本体を再起動しています。");
                    restart();
                }
                if microphone_changed {
                    self.reset_capture()?;
                } else {
                    self.vad.reset();
                    self.warmup_remaining = WARMUP_CHUNKS;
                }
            }
            Command::SetRole(role) => {
                self.storage.save_role(&role)?;
                self.conversation.set_role(&role);
                self.sync_web()?;
                self.notice("ロールを保存しました。会話の履歴を新しくしました。");
            }
            Command::Speak(text) => {
                self.awake();
                self.notice(&text);
                self.speak(&text, Expression::Happy, None)?;
            }
            Command::Chat(text) => {
                self.awake();
                self.chat(&text)?;
            }
            Command::ClearKeys => {
                let mut next = self.config.clone();
                next.wifi_ssid.clear();
                next.wifi_pass.clear();
                next.gemini_key.clear();
                next.claude_key.clear();
                next.openai_key.clear();
                next.voicevox_key.clear();
                // Commit the empty snapshot, then also clear the original
                // credential keys and disable secrets.h fallback on downgrade.
                self.storage.clear_credentials(&next)?;
                self.config = next;
                self.sync_web()?;
                self.notice("Wi-FiとAPIキーを消去しました。設定モードで再起動しています。");
                restart();
            }
            Command::SetExpression(expression) => {
                self.awake();
                self.board.expression = expression;
                self.last_activity_ms = self.now_ms();
                self.notice("表情を変更しました。");
            }
        }
        Ok(())
    }

    fn record(&mut self, preroll: bool) -> Result<Option<(Vec<u8>, usize)>> {
        self.board.state = FaceState::Listening;
        self.board.expression = Expression::Happy;
        self.board.focus(true);
        let mut chunk = [0i16; VAD_CHUNK_SAMPLES];
        // Immediate button and wake-only followups also need codec warmup;
        // elapsed time alone does not drain samples buffered by I2S DMA.
        while self.warmup_remaining > 0 {
            if self.queue_alarm()? {
                self.standby();
                return Ok(None);
            }
            self.board
                .read_microphone(&mut chunk, self.config.mic_gain)?;
            self.vad.push(&chunk, true);
            self.warmup_remaining -= 1;
            self.board.tick(self.now_ms())?;
        }
        let pre = if preroll {
            self.vad.preroll()
        } else {
            Vec::new()
        };
        let stop_peak = (i32::from(self.config.vad_threshold) / 3).max(1_000);
        let mut recording = Recording::new(&pre, stop_peak);
        // A wall-clock cap also protects against a broken I2S rate returning
        // too slowly; Recording itself independently bounds sample storage.
        let deadline = Instant::now() + Duration::from_secs(15);
        while !recording.is_finished() && Instant::now() < deadline {
            if self.queue_alarm()? {
                self.standby();
                return Ok(None);
            }
            self.board
                .read_microphone(&mut chunk, self.config.mic_gain)?;
            recording.push(&chunk);
            self.board.tick(self.now_ms())?;
            if matches!(self.poll_button(self.now_ms()), Some(ButtonAction::Portal)) {
                self.enter_portal()?;
                return Ok(None);
            }
        }
        let profile = audio::profile(
            recording.samples(),
            recording.pre_samples(),
            i32::from(self.config.vad_threshold),
        );
        // Counts and amplitudes are useful hardware diagnostics, without
        // retaining or logging the user's audio or transcription text.
        log::info!(
            "recording profile: pre_peak={} live_peak={} voiced={}/{}",
            profile.pre_peak,
            profile.live_peak,
            profile.voiced,
            profile.count
        );
        self.vad.reset();
        self.warmup_remaining = WARMUP_CHUNKS;
        Ok(Some((recording.into_wav(), profile.voiced)))
    }

    fn listen(&mut self, require_wake: bool) -> Result<()> {
        self.last_activity_ms = self.now_ms();
        let Some((wav, voiced)) = self.record(require_wake)? else {
            return Ok(());
        };
        self.board.state = FaceState::Thinking;
        self.board.expression = Expression::Doubt;
        self.board.tick(self.now_ms())?;
        let transcript = self.cloud(|services, config| services.transcribe(&wav, config));
        drop(wav);
        let transcript = match transcript {
            Ok(text) => text,
            Err(_) => {
                log::warn!("Transcription failed");
                self.notice("音声認識に失敗しました。接続とOpenAIのAPIキーを確認してください。");
                if !require_wake || self.config.wake_mode == 2 {
                    self.speak("ごめん、うまく聞き取れなかったよ", Expression::Sad, None)?;
                }
                self.standby();
                return Ok(());
            }
        };
        self.trace_voice(true, &transcript, "recognized");
        match accept_transcript(
            &transcript,
            &self.config.wake_phrase,
            self.config.wake_mode,
            require_wake,
            voiced,
        ) {
            WakeDecision::Ignore => {
                self.trace_voice(true, "", "ignored");
                if !require_wake {
                    self.speak("ごめん、うまく聞き取れなかったよ", Expression::Sad, None)?;
                }
                self.standby();
            }
            WakeDecision::Respond(text) => self.chat_input(&text, true)?,
            WakeDecision::ListenAgain => {
                self.trace_voice(true, "", "followup");
                self.speak("なあに？", Expression::Happy, None)?;
                // One followup bypasses wake matching. The manual path cannot
                // yield ListenAgain, so this recursion has a depth of one.
                self.listen(false)?;
            }
        }
        self.standby();
        Ok(())
    }

    fn chat(&mut self, text: &str) -> Result<()> {
        self.chat_input(text, false)
    }

    fn trace_voice(&self, microphone: bool, text: &str, route: &str) {
        // USB-local diagnosis only; raw recognition is never served by HTTP
        // or stored in NVS. Limit logging to volume-related utterances.
        if microphone
            && [
                "音量",
                "おんりょう",
                "オンリョウ",
                "ボリューム",
                "ミュート",
                "消音",
                "上げ",
                "下げ",
                "大き",
                "小さ",
            ]
            .iter()
            .any(|word| text.contains(word))
        {
            let bounded: String = text.chars().filter(|c| !c.is_control()).take(256).collect();
            log::info!("Voice volume input: route={route} text={bounded}");
        }
    }

    fn chat_input(&mut self, text: &str, microphone: bool) -> Result<()> {
        self.last_activity_ms = self.now_ms();
        let volume_text = strip_command_wake_prefix(text, &self.config.wake_phrase).to_owned();
        self.trace_voice(microphone, &volume_text, "chat");
        if let Some(command) = voice_volume::parse(&volume_text) {
            self.trace_voice(microphone, &volume_text, "local_volume");
            return self.voice_volume_command(command);
        }
        // Invalid explicit numbers stay local. Let conversational wording of
        // nonnumeric requests reach the AI's bounded volume action protocol.
        let has_number = volume_text.chars().any(|c| {
            c.is_ascii_digit() || "０１２３４５６７８９〇零一二三四五六七八九十百千万".contains(c)
        }) || volume_text.contains("ゼロ");
        if has_number && voice_volume::looks_like_volume_request(&volume_text) {
            self.trace_voice(microphone, &volume_text, "volume_help");
            let help = "音量は0から255で指定してね。「音量を下げて」「音量を10にして」「ミュート解除」でも変えられるよ。";
            self.notice(help);
            if self.volume.get() != 0 {
                self.speak(help, Expression::Neutral, None)?;
            }
            self.standby();
            return Ok(());
        }
        if let Some(reply) = self.conversation.local_command(text, self.now_ms() / 1_000) {
            self.trace_voice(microphone, &volume_text, "timer");
            self.notice(&reply.text);
            self.speak_reply(&reply)?;
            return Ok(());
        }
        if asks_ip(text) {
            let address = self.network.ip().replace('.', "、てん、");
            let text = format!("ぼくのアイピーアドレスは、{address}、だよ");
            self.notice(&text);
            return self.speak(&text, Expression::Happy, None);
        }
        let now = network::now_japan();
        if asks_time(text) {
            let answer = if now.is_empty() {
                "まだ時計を合わせられていないよ。ワイファイを確認してね".into()
            } else {
                format!("いまは{now}だよ")
            };
            self.notice(&answer);
            return self.speak(&answer, Expression::Happy, None);
        }
        if !self.network.connected() {
            return self.offline_notice();
        }
        self.board.state = FaceState::Thinking;
        self.board.expression = Expression::Doubt;
        self.board.focus(true);
        self.board.tick(self.now_ms())?;
        let mut context = if now.is_empty() {
            String::new()
        } else {
            format!("現在日時: {now}。")
        };
        if wants_weather(text) {
            match self.cloud(|services, config| services.weather(config)) {
                Ok(weather) => context.push_str(&weather),
                Err(_) => {
                    log::warn!("Weather lookup unavailable");
                    context.push_str("天気情報の取得に失敗しました。天気や気温を推測せず、取得できなかったと伝えてください。");
                }
            }
        }
        if self.network.portal {
            self.standby();
            return Ok(());
        }
        let messages = self.conversation.request_messages(text, &context);
        match self.cloud(|services, config| services.chat(&messages, config)) {
            Ok(raw) => {
                let action = voice_action::extract(text, &raw);
                if let Some(command) = action.command {
                    self.trace_voice(microphone, &volume_text, "ai_volume");
                    return self.voice_volume_command(command);
                }
                if action.marker_present {
                    self.trace_voice(microphone, &volume_text, "volume_action_rejected");
                    let help = "音量は変更していないよ。「音量を20にして」「音量を下げて」のように、もう一度お願い。";
                    self.notice(help);
                    self.speak(help, Expression::Doubt, None)?;
                    self.standby();
                    return Ok(());
                }
                let reply = self
                    .conversation
                    .process_reply(&action.text, self.now_ms() / 1_000);
                self.conversation.remember(text, &action.text);
                self.notice(&reply.text);
                self.speak_reply(&reply)?;
            }
            Err(_) => {
                log::warn!("Chat request failed");
                self.notice("会話サービスに接続できませんでした。選択したサービスとAPIキーを確認してください。");
                self.speak(
                    "ごめん、いまうまくお話しできないみたい。少し待って、もう一度話しかけてね",
                    Expression::Sad,
                    None,
                )?;
            }
        }
        self.standby();
        Ok(())
    }

    fn speak_reply(&mut self, reply: &Reply) -> Result<()> {
        if !reply.text.trim().is_empty() {
            self.speak(&reply.text, reply.expression, None)?;
        }
        Ok(())
    }

    fn voice_volume_command(&mut self, command: VolumeCommand) -> Result<()> {
        // Serialize settings commits with HTTP submission/rollback. PCM reads
        // remain atomic and never wait for this settings-only lock.
        let shared = self.shared.clone();
        let submission = shared
            .lock()
            .map_err(|_| anyhow!("web state unavailable"))?;
        let before = self.volume.snapshot();
        let Some(requested) =
            voice_volume::resolve(command, before.volume, before.last_nonzero_volume)
        else {
            drop(submission);
            let answer = format!("今の音量は{}だよ。0から255で調整できるよ。", before.volume);
            self.notice(&answer);
            if before.volume != 0 {
                self.speak(&answer, Expression::Neutral, None)?;
            }
            self.standby();
            return Ok(());
        };
        let mut next = self.config.clone();
        // A Web change may already be audible while its save is queued.
        next.set_volume(before.volume);
        next.last_nonzero_volume = before.last_nonzero_volume;
        next.set_volume(requested);
        if self.storage.save_config(&next).is_err() {
            drop(submission);
            self.notice("音量を保存できませんでした。音量は変更していません。");
            self.standby();
            return Ok(());
        }
        // Do not overwrite a Web request that arrived during the NVS commit.
        if !self.volume.set_if_unchanged(before, requested) {
            let live = self.volume.snapshot();
            next.set_volume(live.volume);
            next.last_nonzero_volume = live.last_nonzero_volume;
            self.storage.save_config(&next)?;
        }
        self.config = next;
        drop(submission);
        self.sync_web()?;
        let effective = self.volume.get();
        let answer = if effective == 0 {
            "消音しました。「ミュート解除」で元の音量に戻せます。".to_owned()
        } else {
            format!("音量を{effective}にしたよ。")
        };
        log::info!("Voice volume command applied: {effective}/255");
        self.notice(&answer);
        if effective != 0 {
            self.speak(&answer, Expression::Happy, None)?;
        }
        self.standby();
        Ok(())
    }

    fn speak(&mut self, text: &str, expression: Expression, cache: Option<&str>) -> Result<()> {
        if self.volume.get() == 0 {
            self.notice(text);
            self.standby();
            return Ok(());
        }
        self.awake();
        self.board.expression = expression;
        self.board.state = FaceState::Thinking;
        self.board.focus(true);
        self.board.tick(self.now_ms())?;
        let cached = cache.and_then(|name| self.storage.cached_voice(name));
        let bytes = if let Some(bytes) = cached {
            bytes
        } else if self.network.connected() {
            let bytes = self.cloud(|services, config| services.synthesize(text, config))?;
            if let Some(name) = cache {
                if self.storage.cache_voice(name, &bytes).is_err() {
                    log::warn!("Voice cache write unavailable");
                }
            }
            bytes
        } else {
            self.notice(text);
            self.standby();
            return Ok(());
        };
        self.board.state = FaceState::Speaking;
        log::info!("TTS audio ready: {} bytes", bytes.len());
        crate::diagnostics::heap("before MP3 playback");
        self.board.tick(self.now_ms())?;
        let playback = self.board.play_mp3(&bytes);
        drop(bytes);
        if playback.is_ok() {
            if let Ok(mut state) = self.shared.lock() {
                state.last_playback = self.board.last_playback();
            }
        }
        // Board restores the codec even on decoder failure. Explicitly reset
        // capture here too, so warmup and preroll state follow every exit.
        let microphone = self.reset_capture();
        self.standby();
        self.last_activity_ms = self.now_ms();
        playback?;
        microphone?;
        log::info!("Playback finished; microphone restored; standby ready");
        crate::diagnostics::heap("playback complete");
        Ok(())
    }

    fn prepare_cached_voices(&mut self) {
        if !self.network.connected() {
            return;
        }
        for (name, text) in [("wake", WAKE_SPEECH), ("nonet", OFFLINE_SPEECH)] {
            if self.network.portal || !self.network.connected() {
                break;
            }
            if self.storage.cached_voice(name).is_some() {
                continue;
            }
            match self.cloud(|services, config| services.synthesize(text, config)) {
                Ok(bytes) => {
                    if self.storage.cache_voice(name, &bytes).is_err() {
                        log::warn!("Idle voice cache write unavailable");
                    }
                }
                Err(_) => log::warn!("Idle voice cache generation unavailable"),
            }
        }
    }

    fn offline_notice(&mut self) -> Result<()> {
        self.notice("Wi-Fiに接続していません。本体のボタンを3秒押すと接続設定が開きます。");
        let now = self.now_ms();
        if self
            .last_offline_notice_ms
            .is_none_or(|previous| now.saturating_sub(previous) >= OFFLINE_NOTICE_INTERVAL_MS)
        {
            self.last_offline_notice_ms = Some(now);
            self.speak(OFFLINE_SPEECH, Expression::Doubt, Some("nonet"))?;
        }
        self.standby();
        Ok(())
    }

    fn awake(&mut self) {
        self.sleeping = false;
        self.last_activity_ms = self.now_ms();
        self.board.focus(false);
        self.board.expression = Expression::Neutral;
    }

    fn standby(&mut self) {
        self.board.state = if self.network.portal {
            FaceState::Setup
        } else {
            FaceState::Idle
        };
        self.board.expression = if self.sleeping {
            Expression::Sleepy
        } else {
            Expression::Neutral
        };
        self.board.focus(self.sleeping || self.network.portal);
    }

    fn reset_capture(&mut self) -> Result<()> {
        let result = self.board.microphone();
        self.vad.reset();
        self.warmup_remaining = WARMUP_CHUNKS;
        result
    }

    fn recover_capture(&mut self) {
        if self.reset_capture().is_err() {
            log::warn!("Microphone restore failed; waiting for retry");
        }
        self.standby();
    }

    fn enter_portal(&mut self) -> Result<()> {
        self.network.enter_portal()?;
        self.sleeping = false;
        self.board.focus(true);
        self.vad.reset();
        self.warmup_remaining = WARMUP_CHUNKS;
        self.sync_web()?;
        self.notice("設定モードです。Wi-Fi接続後に設定画面を開いてください。");
        self.show_portal()
    }

    fn show_portal(&mut self) -> Result<()> {
        self.board.focus(true);
        self.board.state = FaceState::Setup;
        self.board.expression = Expression::Neutral;
        let address = format!("http://{}/", self.network.ip());
        self.board.display.show_status(&[
            "SETUP MODE",
            network::SETUP_SSID,
            "pass: stackchan",
            &address,
        ])?;
        self.last_portal_display_ms = self.now_ms();
        Ok(())
    }

    /// HTTP/TLS runs on one scoped worker. The device keeps ownership of all
    /// hardware, animates the face, reacts to a portal hold, and sounds timers
    /// while the network request waits. No worker survives the calling scope.
    fn cloud<R: Send>(
        &mut self,
        operation: impl FnOnce(&mut Services<HttpTransport>, &Config) -> Result<R> + Send,
    ) -> Result<R> {
        crate::diagnostics::heap("cloud start");
        let mut services = std::mem::replace(&mut self.services, Services::new(HttpTransport));
        let config = self.config.clone();
        std::thread::scope(|scope| {
            let worker = std::thread::Builder::new()
                .name("cloud-request".into())
                .stack_size(24 * 1024)
                .spawn_scoped(scope, move || {
                    let result = operation(&mut services, &config);
                    (services, result)
                })
                .context("クラウド通信の処理を開始できませんでした")?;
            let mut display_error_logged = false;
            while !worker.is_finished() {
                let now = self.now_ms();
                if self.board.tick(now).is_err() && !display_error_logged {
                    display_error_logged = true;
                    log::warn!("Display update unavailable while waiting for cloud");
                }
                if self.queue_alarm().is_err() {
                    log::warn!("Local alarm hardware unavailable");
                }
                if matches!(self.poll_button(now), Some(ButtonAction::Portal)) {
                    if self.enter_portal().is_err() {
                        log::warn!("Setup mode could not start");
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let (services, result) = worker
                .join()
                .map_err(|_| anyhow!("クラウド通信の処理が中断されました"))?;
            self.services = services;
            crate::diagnostics::heap("cloud complete");
            if self.network.portal {
                return Err(anyhow!("設定モードに切り替えました"));
            }
            result
        })
    }

    fn queue_alarm(&mut self) -> Result<bool> {
        let Some(reply) = self.conversation.take_timer_alert(self.now_ms() / 1_000) else {
            return Ok(false);
        };
        self.sleeping = false;
        self.last_activity_ms = self.now_ms();
        self.notice(&reply.text);
        self.pending_alarm = Some(reply);
        let state = self.board.state;
        let expression = self.board.expression;
        // Sound the local alarm immediately. Recording callers discard the
        // interrupted clip, so the beep never gets sent to transcription.
        let alarm = self.board.beep();
        let microphone = self.reset_capture();
        self.board.state = state;
        self.board.expression = expression;
        alarm?;
        microphone?;
        Ok(true)
    }

    fn sync_web(&self) -> Result<()> {
        let mut state = self
            .shared
            .lock()
            .map_err(|_| anyhow!("web state unavailable"))?;
        state.config = self.config.clone();
        state.role = self.conversation.role().to_owned();
        state.portal = self.network.portal;
        Ok(())
    }

    fn notice(&self, text: &str) {
        if let Ok(mut state) = self.shared.lock() {
            state.notice = text.to_owned();
        }
    }
}

fn asks_ip(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("ip") || text.contains("アイピー") || text.contains("あいぴー"))
        && ["アドレス", "教え", "何", "なに", "いくつ"]
            .iter()
            .any(|word| text.contains(word))
}

fn asks_time(text: &str) -> bool {
    ["何時", "日付", "何日", "曜日", "今の時間", "いまの時間"]
        .iter()
        .any(|word| text.contains(word))
}

fn wants_weather(text: &str) -> bool {
    [
        "天気", "気温", "暑い", "寒い", "雨", "雪", "傘", "降水", "晴れ",
    ]
    .iter()
    .any(|word| text.contains(word))
}

fn restart() -> ! {
    // Let the HTTP handler finish delivering its accepted response first.
    std::thread::sleep(Duration::from_millis(800));
    unsafe { sys::esp_restart() }
}
