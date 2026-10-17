//! Portable wake-word, dialogue, expression and timer behavior.
//!
//! The caller supplies monotonic seconds and remembers only successful AI
//! exchanges. Building a request never mutates history.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const DEFAULT_ROLE: &str = "あなたは「ちびスタックちゃん」という小さくてかわいいロボットです。一人称は「ぼく」。日本語で、話し言葉で短く(60文字以内で)答えてください。";
const EXPRESSION_RULE: &str =
    "返答の先頭に感情タグを1つだけ付けてください: [happy] [sad] [angry] [sleepy] [doubt] [neutral]";
const TIMER_RULE: &str = "タイマーを頼まれたら返答の最後に [timer:秒数] を付けてください (例: 3分なら [timer:180])。タイマーの中止を頼まれたら [timer:cancel] を、残り時間を聞かれたら [timer:status] を付けてください。タイマーと関係ない話ではこれらのタグを付けないでください。";
const SPEECH_RULE: &str = "返答は音声で読み上げられます。話し言葉で120文字以内に収めてください。";
const VOLUME_RULE: &str = "あなたは実際に動くロボットの会話担当です。この会話は既に本体への呼びかけなので、音量操作を頼まれたら『本体に話しかけて』と案内せず、返答の最後に操作タグを1つだけ付けてください。音量は0から255で、指定値なら [volume:20] のようにその数値、上げるなら [volume:up]、下げるなら [volume:down]、消音は [volume:mute]、解除は [volume:unmute]、現在値の質問は [volume:status] です。up/downは2だけ変更します。範囲外の数値を丸めたり、指定のない数値を作ったりしないでください。本体が保存と適用を確認して返答するので、自分が変更済みだと断言しないでください。引用、否定、例文、操作方法の説明、複数の操作を含む依頼や音量と無関係な会話では操作タグを付けないでください。タイマータグとは同時に使わないでください。";
const MAX_HISTORY_TURNS: usize = 5;
const MAX_HISTORY_BYTES: usize = 16 * 1024;
const MAX_MESSAGE_BYTES: usize = 4096;
const MAX_ROLE_BYTES: usize = 8192;
const MAX_TIMER_SECONDS: u64 = 12 * 3600;

pub use stackchan_avatar_face::Expression;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub expression: Expression,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WakeDecision {
    Ignore,
    ListenAgain,
    Respond(String),
}

/// `require_wake` distinguishes automatic microphone triggers from manual input.
/// Voiced-window counts remain diagnostic, as in the original firmware: a
/// genuine short utterance must not be discarded solely for its duration.
pub fn accept_transcript(
    text: &str,
    wake_phrase: &str,
    wake_mode: u8,
    require_wake: bool,
    _voiced_windows: usize,
) -> WakeDecision {
    let text = text.trim();
    if is_hallucination(text) || (require_wake && wake_mode == 0) {
        return WakeDecision::Ignore;
    }
    if !require_wake {
        return WakeDecision::Respond(text.to_owned());
    }
    if let Some(end) = find_wake_end(text, wake_phrase) {
        let rest = text[end..]
            .trim_start_matches(|c: char| c.is_whitespace() || "、。！？，!?,".contains(c))
            .trim();
        return if rest.chars().count() < 2 {
            WakeDecision::ListenAgain
        } else {
            WakeDecision::Respond(rest.to_owned())
        };
    }
    if wake_mode == 2 {
        WakeDecision::Respond(text.to_owned())
    } else {
        WakeDecision::Ignore
    }
}

fn is_hallucination(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.is_empty()
        || [
            "ご視聴",
            "ご清聴",
            "視聴ありがと",
            "視聴してくださ",
            "ご覧いただき",
            "字幕",
            "チャンネル登録",
            "高評価",
            "最後までご覧",
            "見てくれてありがと",
            "thank you for watching",
            "thanks for watching",
            "please subscribe",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
}

/// Keep original UTF-8 offsets while ignoring long vowels and spacing.
fn normalized_chars(text: &str) -> Vec<(char, usize)> {
    text.char_indices()
        .filter_map(|(offset, c)| {
            if c.is_whitespace() || matches!(c, 'ー' | '〜' | '～') {
                None
            } else {
                let c = match c {
                    'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap(),
                    other => other.to_ascii_lowercase(),
                };
                Some((c, offset))
            }
        })
        .collect()
}

/// Remove an addressed device name before parsing a local command, including
/// manual/follow-up input where wake-word activation was not required. Match
/// only a configured alias at the start; never search inside quoted speech or
/// discard an unrelated sentence before a name.
pub fn strip_command_wake_prefix<'a>(text: &'a str, aliases: &str) -> &'a str {
    let mut candidate = text.trim();
    for prefix in ["あのね", "おーい", "ねえ", "ねぇ", "あの"] {
        if let Some(rest) = candidate.strip_prefix(prefix) {
            candidate =
                rest.trim_start_matches(|c: char| c.is_whitespace() || "、。！？，!?,".contains(c));
            break;
        }
    }
    let normalized = normalized_chars(candidate);
    let end = aliases
        .split([',', '、'])
        .filter_map(|alias| {
            let pattern = normalized_chars(alias.trim());
            if pattern.is_empty()
                || !normalized
                    .iter()
                    .map(|&(c, _)| c)
                    .take(pattern.len())
                    .eq(pattern.iter().map(|&(c, _)| c))
            {
                return None;
            }
            Some(
                normalized
                    .get(pattern.len())
                    .map_or(candidate.len(), |&(_, offset)| offset),
            )
        })
        .max();
    match end {
        Some(end) => candidate[end..]
            .trim_start_matches(|c: char| c.is_whitespace() || "、。！？，!?,".contains(c)),
        None => text.trim(),
    }
}

fn find_wake_end(text: &str, aliases: &str) -> Option<usize> {
    let normalized = normalized_chars(text);
    let chars: Vec<char> = normalized.iter().map(|&(c, _)| c).collect();
    for alias in aliases.split([',', '、']) {
        let pattern: Vec<char> = normalized_chars(alias).iter().map(|&(c, _)| c).collect();
        if pattern.is_empty() {
            continue;
        }
        let distance = usize::from(pattern.len() >= 5);
        if let Some(end) = fuzzy_end(&chars, &pattern, distance) {
            return Some(
                normalized
                    .get(end)
                    .map_or(text.len(), |&(_, offset)| offset),
            );
        }
    }
    None
}

/// Levenshtein substring search in Unicode scalar values, using two DP rows.
fn fuzzy_end(text: &[char], pattern: &[char], max_distance: usize) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let mut previous = vec![0; text.len() + 1];
    let mut current = vec![0; text.len() + 1];
    for (i, &p) in pattern.iter().enumerate() {
        current[0] = i + 1;
        for (j, &t) in text.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(p != t))
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
        .iter()
        .enumerate()
        .skip(1)
        .min_by_key(|&(end, distance)| (*distance, end))
        .filter(|&(_, &distance)| distance <= max_distance)
        .map(|(end, _)| end)
}

#[derive(Clone, Copy)]
struct Timer {
    started: u64,
    seconds: u64,
}

impl Timer {
    fn remaining(self, now: u64) -> u64 {
        self.seconds
            .saturating_sub(now.saturating_sub(self.started))
    }
}

pub struct Conversation {
    role: String,
    history: VecDeque<(String, String)>,
    timer: Option<Timer>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new(DEFAULT_ROLE)
    }
}

impl Conversation {
    pub fn new(role: &str) -> Self {
        Self {
            role: role_text(role),
            history: VecDeque::new(),
            timer: None,
        }
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn set_role(&mut self, role: &str) {
        self.role = role_text(role);
        self.history.clear();
    }

    pub fn request_messages(&self, user: &str, context: &str) -> Vec<Message> {
        let mut messages = Vec::with_capacity(self.history.len() * 2 + 2);
        messages.push(Message {
            role: "system".into(),
            content: format!(
                "{}\n{}\n{}\n{}\n{}",
                self.role, EXPRESSION_RULE, TIMER_RULE, SPEECH_RULE, VOLUME_RULE
            ),
        });
        for (user, reply) in &self.history {
            messages.push(Message {
                role: "user".into(),
                content: user.clone(),
            });
            messages.push(Message {
                role: "assistant".into(),
                content: reply.clone(),
            });
        }
        let mut content = bounded(user.trim(), MAX_MESSAGE_BYTES);
        if !context.trim().is_empty() {
            content.push_str("\n(参考情報: ");
            content.push_str(&bounded(context.trim(), MAX_MESSAGE_BYTES));
            content.push(')');
        }
        messages.push(Message {
            role: "user".into(),
            content,
        });
        messages
    }

    pub fn remember(&mut self, user: &str, reply: &str) {
        if user.trim().is_empty() || reply.trim().is_empty() {
            return;
        }
        self.history.push_back((
            bounded(user.trim(), MAX_MESSAGE_BYTES),
            bounded(reply.trim(), MAX_MESSAGE_BYTES),
        ));
        while self.history.len() > MAX_HISTORY_TURNS
            || self
                .history
                .iter()
                .map(|(user, reply)| user.len() + reply.len())
                .sum::<usize>()
                > MAX_HISTORY_BYTES
        {
            self.history.pop_front();
        }
    }

    pub fn local_command(&mut self, text: &str, now_secs: u64) -> Option<Reply> {
        let mentions_timer = text.contains("タイマー") || text.contains("たいまー");
        if mentions_timer && contains_any(text, &["やめ", "止め", "ストップ", "キャンセル", "中止"])
        {
            self.timer = None;
            return Some(reply("タイマーをやめたよ", Expression::Neutral));
        }
        if mentions_timer && contains_any(text, &["あと", "残り", "何分", "何秒"]) {
            return Some(reply(self.timer_status(now_secs), Expression::Neutral));
        }
        if mentions_timer || contains_any(text, &["はかって", "計って", "測って"]) {
            let seconds = parse_duration(text);
            if seconds > 0 {
                let replaced = self.timer.is_some();
                self.start_timer(seconds, now_secs);
                let ending = if replaced {
                    "セットし直したよ"
                } else {
                    "セットしたよ"
                };
                return Some(reply(
                    format!("{}のタイマーに{ending}", format_duration(seconds)),
                    Expression::Happy,
                ));
            }
        }
        None
    }

    pub fn process_reply(&mut self, text: &str, now_secs: u64) -> Reply {
        let expression = pick_expression(text);
        let mut spoken = strip_expression_tags(text);
        let (without_tags, argument) = strip_timer_tags(&spoken);
        spoken = without_tags;
        if let Some(argument) = argument {
            match argument.trim().to_ascii_lowercase().as_str() {
                "cancel" => {
                    self.timer = None;
                    if spoken.is_empty() {
                        spoken = "タイマーをやめたよ".into();
                    }
                }
                "status" => spoken = self.timer_status(now_secs),
                argument => {
                    if let Some(seconds) = positive_seconds(argument) {
                        let replaced = self.timer.is_some();
                        self.start_timer(seconds, now_secs);
                        if spoken.is_empty() {
                            spoken =
                                format!("{}のタイマーをセットしたよ", format_duration(seconds));
                        } else if replaced {
                            spoken.push_str("。前のタイマーはやめてセットし直したよ");
                        }
                    }
                }
            }
        }
        reply(spoken, expression)
    }

    pub fn take_timer_alert(&mut self, now_secs: u64) -> Option<Reply> {
        let timer = self.timer?;
        if timer.remaining(now_secs) != 0 {
            return None;
        }
        self.timer = None;
        Some(reply(
            format!(
                "ピピピッ、ピピピッ！{}たったよ！",
                format_duration(timer.seconds)
            ),
            Expression::Happy,
        ))
    }

    fn start_timer(&mut self, seconds: u64, now_secs: u64) {
        self.timer = Some(Timer {
            started: now_secs,
            seconds: seconds.min(MAX_TIMER_SECONDS),
        });
    }

    fn timer_status(&self, now_secs: u64) -> String {
        self.timer.map_or_else(
            || "いまタイマーは動いてないよ".into(),
            |timer| format!("あと{}だよ", format_duration(timer.remaining(now_secs))),
        )
    }
}

fn role_text(role: &str) -> String {
    bounded(
        if role.trim().is_empty() {
            DEFAULT_ROLE
        } else {
            role.trim()
        },
        MAX_ROLE_BYTES,
    )
}

fn bounded(text: &str, max_bytes: usize) -> String {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn reply(text: impl Into<String>, expression: Expression) -> Reply {
    Reply {
        text: text.into(),
        expression,
    }
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| text.contains(pattern))
}

const EXPRESSION_TAGS: [(&str, Expression); 6] = [
    ("[happy]", Expression::Happy),
    ("[sad]", Expression::Sad),
    ("[angry]", Expression::Angry),
    ("[sleepy]", Expression::Sleepy),
    ("[doubt]", Expression::Doubt),
    ("[neutral]", Expression::Neutral),
];

fn pick_expression(text: &str) -> Expression {
    let lower = text.to_ascii_lowercase();
    for (tag, expression) in EXPRESSION_TAGS {
        if lower.contains(tag) {
            return expression;
        }
    }
    for (words, expression) in [
        (
            &["ごめん", "残念", "悲し", "つら", "さみし"][..],
            Expression::Sad,
        ),
        (&["怒", "だめ", "ダメ", "こら"][..], Expression::Angry),
        (&["眠", "ねむ", "おやすみ"][..], Expression::Sleepy),
        (
            &["？", "?", "わからない", "かな", "はて"][..],
            Expression::Doubt,
        ),
        (
            &[
                "！",
                "!",
                "嬉し",
                "楽し",
                "ありがと",
                "やった",
                "すごい",
                "好き",
            ][..],
            Expression::Happy,
        ),
    ] {
        if contains_any(text, words) {
            return expression;
        }
    }
    Expression::Neutral
}

fn strip_expression_tags(text: &str) -> String {
    let mut text = text.replace(['\n', '\r'], " ");
    for (tag, _) in EXPRESSION_TAGS {
        while let Some(start) = text.to_ascii_lowercase().find(tag) {
            text.replace_range(start..start + tag.len(), "");
        }
    }
    text.trim().to_owned()
}

/// Execute only the first tag, like the C++ firmware, but hide all timer tags
/// from TTS. Malformed/incomplete arguments never start a timer.
fn strip_timer_tags(text: &str) -> (String, Option<String>) {
    let mut spoken = String::new();
    let mut rest = text;
    let mut argument = None;
    while let Some(start) = rest.to_ascii_lowercase().find("[timer:") {
        spoken.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(']') else {
            return (spoken.trim().to_owned(), argument);
        };
        let end = start + end;
        if argument.is_none() {
            argument = Some(rest[start + 7..end].to_owned());
        }
        rest = &rest[end + 1..];
    }
    spoken.push_str(rest);
    (spoken.trim().to_owned(), argument)
}

fn format_duration(seconds: u64) -> String {
    match (seconds / 60, seconds % 60) {
        (0, seconds) => format!("{seconds}秒"),
        (minutes, 0) => format!("{minutes}分"),
        (minutes, seconds) => format!("{minutes}分{seconds}秒"),
    }
}

fn positive_seconds(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let seconds = text.bytes().fold(0u64, |n, c| {
        n.saturating_mul(10).saturating_add(u64::from(c - b'0'))
    });
    (seconds > 0).then_some(seconds.min(MAX_TIMER_SECONDS))
}

fn digit(c: char) -> Option<u64> {
    match c {
        '0'..='9' => Some(c as u64 - '0' as u64),
        '０'..='９' => Some(c as u64 - '０' as u64),
        '〇' | '零' => Some(0),
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn numeral(c: char) -> bool {
    digit(c).is_some() || matches!(c, '十' | '百' | '千')
}

fn number(chars: &[char]) -> u64 {
    let (mut total, mut pending) = (0u64, 0u64);
    for &c in chars {
        if let Some(value) = digit(c) {
            pending = pending.saturating_mul(10).saturating_add(value);
        } else {
            let unit = match c {
                '十' => 10,
                '百' => 100,
                '千' => 1000,
                _ => 0,
            };
            total = total.saturating_add(pending.max(1).saturating_mul(unit));
            pending = 0;
        }
    }
    total.saturating_add(pending)
}

fn parse_duration(text: &str) -> u64 {
    let chars: Vec<char> = text.chars().collect();
    let (mut i, mut total) = (0, 0u64);
    while i < chars.len() {
        if !numeral(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && numeral(chars[i]) {
            i += 1;
        }
        let value = number(&chars[start..i]);
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let (unit, used) = match chars.get(i..) {
            Some(['時', '間', ..]) => (3600, 2),
            Some(['分', ..]) => (60, 1),
            Some(['秒', ..]) => (1, 1),
            _ => (0, 0),
        };
        if unit != 0 {
            if start > 0 && matches!(chars[start - 1], '-' | '−' | 'ー' | '.' | '．') {
                return 0;
            }
            total = total.saturating_add(value.saturating_mul(unit));
            i += used;
            if chars.get(i) == Some(&'半') {
                total = total.saturating_add(unit / 2);
                i += 1;
            }
        }
    }
    total.min(MAX_TIMER_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addressed_volume_commands_work_in_automatic_and_manual_listening() {
        use crate::voice_volume::{self, VolumeCommand};
        for require_wake in [true, false] {
            for (text, expected) in [
                ("スタックちゃん音量下げて", VolumeCommand::Adjust(-2)),
                ("スタックチャン音量上げて", VolumeCommand::Adjust(2)),
                ("おーい、ミュートして", VolumeCommand::Mute),
                ("おーい、音量20にして", VolumeCommand::Set(20)),
                (
                    "ねえ、スタックちゃん、音量15にしてよ",
                    VolumeCommand::Set(15),
                ),
            ] {
                let WakeDecision::Respond(command) =
                    accept_transcript(text, "スタックちゃん", 2, require_wake, 2)
                else {
                    panic!("command ignored: {text}");
                };
                assert_eq!(
                    voice_volume::parse(strip_command_wake_prefix(&command, "スタックちゃん")),
                    Some(expected),
                    "{text}, require_wake={require_wake}"
                );
            }
        }
    }

    #[test]
    fn command_wake_prefix_is_anchored_and_uses_configured_aliases() {
        assert_eq!(
            accept_transcript("スタックチャン音量上げて", "スタックちゃん", 1, true, 2),
            WakeDecision::Respond("音量上げて".into())
        );
        assert_eq!(
            strip_command_wake_prefix("ちび、音量下げて", "スタックちゃん、ちび"),
            "音量下げて"
        );
        assert_eq!(
            strip_command_wake_prefix("STACK chan, 音量15にして", "stackchan"),
            "音量15にして"
        );
        for text in [
            "例はスタックちゃん音量下げて",
            "「スタックちゃん音量下げて」と言って",
            "他の子、音量下げて",
        ] {
            assert_eq!(strip_command_wake_prefix(text, "スタックちゃん"), text);
        }
    }

    #[test]
    fn addressed_one_minute_timer_keeps_its_existing_alarm() {
        let original = "スタックちゃんタイマー1分";
        for require_wake in [true, false] {
            let WakeDecision::Respond(text) =
                accept_transcript(original, "スタックちゃん", 2, require_wake, 2)
            else {
                panic!("timer ignored")
            };
            let volume_text = strip_command_wake_prefix(&text, "スタックちゃん");
            assert_eq!(crate::voice_volume::parse(volume_text), None);
            assert!(!crate::voice_volume::looks_like_volume_request(volume_text));
            let mut conversation = Conversation::default();
            if conversation.local_command(&text, 100).is_none() {
                // Existing conversation fallback: the AI returns the timer
                // tag, which is consumed locally before the reply is spoken.
                conversation.process_reply("[happy]1分のタイマーをセットしたよ[timer:60]", 100);
            }
            assert!(conversation.take_timer_alert(159).is_none());
            assert!(conversation
                .take_timer_alert(160)
                .unwrap()
                .text
                .contains("ピピピッ"));
            assert!(conversation.take_timer_alert(161).is_none());
        }
    }

    #[test]
    fn wake_matching_preserves_unicode_offsets_and_aliases() {
        assert_eq!(
            accept_transcript(
                "ねえ、スタックちゃーん　、今日の天気は？",
                "スタックちゃん",
                1,
                true,
                2
            ),
            WakeDecision::Respond("今日の天気は？".into())
        );
        assert_eq!(
            accept_transcript("スタッフちゃん、こんにちは", "スタックちゃん", 1, true, 2),
            WakeDecision::Respond("こんにちは".into())
        );
        assert_eq!(
            accept_transcript("🤖ちび、元気？", "スタックちゃん、ちび", 1, true, 1),
            WakeDecision::Respond("元気？".into())
        );
        assert_eq!(
            accept_transcript("ちぶ、元気？", "ちび", 1, true, 1),
            WakeDecision::Ignore
        );
        assert_eq!(
            accept_transcript("スタックちゃーん！", "スタックちゃん", 1, true, 1),
            WakeDecision::ListenAgain
        );
        assert_eq!(
            accept_transcript("スタックちゃん、一", "スタックちゃん", 1, true, 1),
            WakeDecision::ListenAgain
        );
        assert_eq!(
            accept_transcript("STACK chan, hello", "stackchan", 1, true, 1),
            WakeDecision::Respond("hello".into())
        );
    }

    #[test]
    fn wake_modes_and_hallucinations_match_activation_intent() {
        assert_eq!(
            accept_transcript("こんにちは", "スタックちゃん", 1, true, 10),
            WakeDecision::Ignore
        );
        assert_eq!(
            accept_transcript("こんにちは", "スタックちゃん", 2, true, 1),
            WakeDecision::Respond("こんにちは".into())
        );
        assert_eq!(
            accept_transcript("こんにちは", "スタックちゃん", 0, false, 0),
            WakeDecision::Respond("こんにちは".into())
        );
        assert_eq!(
            accept_transcript("スタックちゃん", "スタックちゃん", 0, true, 10),
            WakeDecision::Ignore
        );
        for text in [
            "",
            "　\n",
            "ご視聴ありがとうございました",
            "THANKS FOR WATCHING",
        ] {
            assert_eq!(
                accept_transcript(text, "スタックちゃん", 2, true, 120),
                WakeDecision::Ignore
            );
        }
    }

    #[test]
    fn expressions_prefer_tags_and_never_speak_control_tags() {
        let mut conversation = Conversation::default();
        let result = conversation.process_reply("[HAPPY]ごめんね。\nまた遊ぼう！[happy]", 10);
        assert_eq!(result, reply("ごめんね。 また遊ぼう！", Expression::Happy));
        assert_eq!(
            conversation.process_reply("おやすみ", 0).expression,
            Expression::Sleepy
        );
        assert_eq!(
            conversation.process_reply("どうしたの？", 0).expression,
            Expression::Doubt
        );
        assert_eq!(
            conversation.process_reply("ごめんね", 0).expression,
            Expression::Sad
        );
    }

    #[test]
    fn local_timer_supports_japanese_fullwidth_and_compound_durations() {
        for (text, expected) in [
            ("三分のタイマー", 180),
            ("タイマーを１時間２０分３０秒", 4830),
            ("三十分はかって", 1800),
            ("一分半測って", 90),
            ("二百三十秒のタイマー", 230),
            ("999999999999999999999999999時間のタイマー", 43200),
        ] {
            let mut conversation = Conversation::default();
            assert!(conversation.local_command(text, 100).is_some(), "{text}");
            assert!(
                conversation.take_timer_alert(100 + expected - 1).is_none(),
                "{text}"
            );
            assert!(
                conversation.take_timer_alert(100 + expected).is_some(),
                "{text}"
            );
            assert!(conversation.take_timer_alert(100 + expected).is_none());
        }
        let mut conversation = Conversation::default();
        assert!(conversation.local_command("タイマーを-3分", 0).is_none());
        assert!(conversation.local_command("タイマーを1.5分", 0).is_none());
        assert!(conversation
            .local_command("三分の曲を知っている？", 0)
            .is_none());
    }

    #[test]
    fn timer_tags_status_replacement_cancellation_and_expiry_are_monotonic() {
        let mut conversation = Conversation::default();
        assert_eq!(
            conversation
                .process_reply("[happy]セットしたよ[timer:90]", 100)
                .text,
            "セットしたよ"
        );
        assert_eq!(
            conversation
                .local_command("タイマーあと何秒？", 130)
                .unwrap()
                .text,
            "あと1分だよ"
        );
        assert_eq!(
            conversation.process_reply("[timer:status]", 191).text,
            "あと0秒だよ"
        );
        assert!(conversation.take_timer_alert(191).is_some());
        assert_eq!(
            conversation.process_reply("[timer:status]", 191).text,
            "いまタイマーは動いてないよ"
        );
        conversation.process_reply("[timer:10]", 200);
        assert!(conversation
            .local_command("20秒のタイマー", 201)
            .unwrap()
            .text
            .contains("セットし直したよ"));
        assert_eq!(
            conversation.process_reply("[timer:cancel]", 202).text,
            "タイマーをやめたよ"
        );
        assert!(conversation.take_timer_alert(9999).is_none());
        conversation.local_command("10秒のタイマー", u64::MAX - 5);
        assert!(conversation.take_timer_alert(u64::MAX).is_none());
    }

    #[test]
    fn malformed_timer_tags_cannot_schedule_or_reach_speech() {
        for tag in [
            "[timer:-3]",
            "[timer:1.5]",
            "[timer:12abc]",
            "[timer:0]",
            "[timer:",
        ] {
            let mut conversation = Conversation::default();
            assert_eq!(
                conversation.process_reply(&format!("返事{tag}"), 0).text,
                "返事"
            );
            assert!(conversation.take_timer_alert(100000).is_none());
        }
        let mut conversation = Conversation::default();
        conversation.process_reply("[timer:999999999999999999999999999]", 0);
        assert!(conversation.take_timer_alert(43199).is_none());
        assert!(conversation.take_timer_alert(43200).is_some());
        conversation.process_reply("[timer:10][timer:cancel]", 100);
        assert!(conversation.take_timer_alert(110).is_some());
    }

    #[test]
    fn requests_keep_only_successful_bounded_exchanges_and_current_context() {
        let mut conversation = Conversation::new("先生だよ");
        let first = conversation.request_messages("質問", "天気は晴れ");
        assert_eq!(first.len(), 2);
        assert!(first[0].content.contains("先生だよ"));
        assert!(first[0].content.contains("[timer:秒数]"));
        assert_eq!(first[1].content, "質問\n(参考情報: 天気は晴れ)");
        assert_eq!(conversation.request_messages("再試行", "").len(), 2);
        conversation.remember("質問", "");
        assert_eq!(conversation.request_messages("再試行", "").len(), 2);
        for i in 0..8 {
            conversation.remember(&format!("質問{i}"), &format!("返事{i}"));
        }
        let request = conversation.request_messages("今の質問", "");
        assert_eq!(request.len(), 12);
        assert_eq!(request[1].content, "質問3");
        assert_eq!(request[10].content, "返事7");
        assert!(!request
            .iter()
            .any(|message| message.content.contains("天気は晴れ")));
        conversation.set_role("");
        assert_eq!(conversation.role(), DEFAULT_ROLE);
        assert_eq!(conversation.request_messages("質問", "").len(), 2);
    }

    #[test]
    fn history_byte_limits_preserve_utf8_and_alternating_roles() {
        let mut conversation = Conversation::default();
        for _ in 0..20 {
            conversation.remember(&"あ".repeat(5000), &"🤖".repeat(5000));
        }
        let messages = conversation.request_messages("次", "");
        assert!(
            conversation
                .history
                .iter()
                .map(|(a, b)| a.len() + b.len())
                .sum::<usize>()
                <= MAX_HISTORY_BYTES
        );
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[2].role, "assistant");
        assert!(messages[1].content.ends_with('あ'));
        assert!(messages[2].content.ends_with('🤖'));
    }
}
