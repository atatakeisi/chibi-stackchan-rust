//! A bounded LLM fallback for physical volume requests. Tags propose an action;
//! the current user's unquoted request must independently authorize it. The
//! device persists and applies accepted commands before generating its reply.

use crate::voice_volume::{self, VolumeCommand, STEP};

pub struct ActionReply {
    pub text: String,
    pub command: Option<VolumeCommand>,
    pub marker_present: bool,
}

/// Remove every volume marker from speech, even malformed or incomplete ones.
/// Only one complete terminal tag can propose an action. Rejecting a proposal
/// never prevents stripping it, and never changes a setting by itself.
pub fn extract(user_text: &str, reply: &str) -> ActionReply {
    let mut text = String::with_capacity(reply.len());
    let mut rest = reply;
    let mut count = 0;
    let mut proposed = None;
    let mut terminal = false;
    while let Some(start) = rest.to_ascii_lowercase().find("[volume") {
        text.push_str(&rest[..start]);
        count += 1;
        let Some(end) = rest[start..].find(']') else {
            proposed = None;
            terminal = false;
            rest = "";
            break;
        };
        let end = start + end;
        if count == 1 {
            proposed = tag_command(&rest[start..=end]);
            terminal = rest[end + 1..].trim().is_empty();
        }
        rest = &rest[end + 1..];
    }
    text.push_str(rest);
    let mixed_action = reply.to_ascii_lowercase().contains("[timer");
    let command = proposed.filter(|command| {
        count == 1 && terminal && !mixed_action && user_authorizes(user_text, *command)
    });
    ActionReply {
        text: text.trim().to_owned(),
        command,
        marker_present: count != 0,
    }
}

fn tag_command(tag: &str) -> Option<VolumeCommand> {
    let argument = tag.strip_prefix("[volume:")?.strip_suffix(']')?;
    match argument {
        "up" => Some(VolumeCommand::Adjust(STEP)),
        "down" => Some(VolumeCommand::Adjust(-STEP)),
        "mute" => Some(VolumeCommand::Mute),
        "unmute" => Some(VolumeCommand::Unmute),
        "status" => Some(VolumeCommand::Status),
        number if !number.is_empty() && number.bytes().all(|c| c.is_ascii_digit()) => {
            number.parse::<u8>().ok().map(VolumeCommand::Set)
        }
        _ => None,
    }
}

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

fn user_authorizes(user: &str, command: VolumeCommand) -> bool {
    if user.len() > 4096
        || user.chars().any(|c| {
            matches!(
                c,
                '「' | '」' | '『' | '』' | '"' | '\'' | '“' | '”' | '‘' | '’' | '`' | '[' | ']'
            )
        })
    {
        return false;
    }
    let mut request: String = user
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap(),
            '％' => '%',
            other => other,
        })
        .collect();
    // Observed STT homophone: spoken "二十" arrived as "二重". Normalize
    // only a volume number immediately followed by a setting request; words
    // such as "二重人格" remain ordinary text, not a numeric authorization.
    if request.contains("二重") && has(&request, &["音量", "ボリューム", "声の大きさ", "音声"])
    {
        for suffix in [
            "にして",
            "に設定",
            "に変え",
            "に調整",
            "にでき",
            "でお願い",
            "にお願い",
        ] {
            request = request.replace(&format!("二重{suffix}"), &format!("二十{suffix}"));
        }
    }
    if matches!(command, VolumeCommand::Set(_)) && request.contains("二重") {
        return false;
    }
    if has(
        &request,
        &[
            "ないで",
            "なくて",
            "しない",
            "しなく",
            "せず",
            "ずに",
            "上げない",
            "下げない",
            "変えない",
            "消さない",
            "戻さない",
            "禁止",
            "不要",
            "やめて",
            "いけない",
            "いけません",
            "だめ",
            "ダメ",
            "駄目",
            "ないよう",
            "ほしくない",
            "欲しくない",
            "したくない",
            "必要ない",
            "必要ありません",
            "と言",
            "って言",
            "と話",
            "って話",
            "と書",
            "って書",
            "と入力",
            "と認識",
            "と頼",
            "と伝",
            "とお願い",
            "ってお願い",
            "お願いした",
            "頼んだ",
            "例えば",
            "たとえば",
            "例文",
            "例として",
            "という",
            "引用",
            "発言",
            "セリフ",
            "台詞",
            "台本",
            "説明",
            "方法",
            "やり方",
            "について",
            "意味",
            "発音",
            "読み上げ",
            "言って",
            "してもいい",
            "にしたら",
            "にすると",
            "した場合",
            "したとき",
            "それから",
            "そのあと",
            "その後",
            "ついでに",
            "続けて",
            "てから",
            "たあと",
            "後で",
            "あとで",
            "かつ",
            "および",
            "及び",
            "同時に",
            "両方",
            "タイマー",
            "アラーム",
            "表情",
            "目を",
            "口を",
            "顔を",
            "サーボ",
            "天気",
            "再生",
            "しゃべって",
            "話して",
            "待って",
            "調べて",
            "答えて",
            "聞いて",
            "見て",
            "歌って",
            "マイナス",
            "－",
            "−",
            "-",
            "＋",
            "+",
        ],
    ) || request.matches("して").count() > 1
    {
        return false;
    }
    if command != VolumeCommand::Status
        && has(
            &request,
            &[
                "してる",
                "している",
                "しています",
                "していた",
                "してた",
                "してある",
                "しておいた",
                "してみた",
                "しました",
                "設定済",
                "設定した",
                "調整した",
                "変えた",
                "変えました",
                "下げた",
                "上げた",
                "下げてる",
                "上げてる",
                "下げています",
                "上げています",
                "デシベル",
                "dB",
                "db",
                "倍",
            ],
        )
    {
        return false;
    }
    // A known complete request is the strongest binding, including numeric
    // percent/kanji forms. A model cannot substitute a different value/action.
    if let Some(expected) = voice_volume::parse(&request) {
        return expected == command;
    }
    let volume_noun = has(&request, &["音量", "ボリューム", "声の大きさ", "音声"]);
    let mute_noun = has(&request, &["ミュート", "消音"]);
    if !volume_noun && !mute_noun {
        return false;
    }
    // Do not mistake the noun "声の大きさ" for a request to raise volume.
    let direction_text = request.replace("声の大きさ", "");
    let up_words = ["上げ", "あげ", "大き", "高く", "高め"];
    let down_words = [
        "下げ",
        "さげ",
        "小さ",
        "静か",
        "低く",
        "低め",
        "控えめ",
        "ひかえめ",
    ];
    let up_count = up_words
        .iter()
        .map(|word| direction_text.matches(word).count())
        .sum::<usize>();
    let down_count = down_words
        .iter()
        .map(|word| direction_text.matches(word).count())
        .sum::<usize>();
    let restore = has(&request, &["解除", "戻して", "もどして", "戻せ", "再開"]);
    let silence = mute_noun || has(&request, &["消して", "消せ"]);
    let mute_count = ["ミュート", "消音", "消して", "消せ"]
        .iter()
        .map(|word| request.matches(word).count())
        .sum::<usize>();
    if mute_count > 1 || request.matches("解除").count() > 1 {
        return false;
    }
    let spans = number_spans(&request);
    let requesting = has(
        &request,
        &[
            "して",
            "くれる",
            "ください",
            "お願い",
            "上げて",
            "下げて",
            "あげて",
            "さげて",
        ],
    );
    match command {
        VolumeCommand::Set(value) => {
            if !volume_noun
                || mute_noun
                || restore
                || up_count != 0
                || down_count != 0
                || !has(
                    &request,
                    &[
                        "にして",
                        "に設定",
                        "に変え",
                        "に調整",
                        "にでき",
                        "でお願い",
                        "にお願い",
                    ],
                )
                || spans.len() != 1
            {
                return false;
            }
            let (start, end) = spans[0];
            let unit = if request[end..].starts_with("パーセント") {
                "パーセント"
            } else if request[end..].starts_with('%') {
                "%"
            } else {
                ""
            };
            let candidate = format!("音量を{}{unit}にして", &request[start..end]);
            voice_volume::parse(&candidate) == Some(VolumeCommand::Set(value))
        }
        VolumeCommand::Adjust(delta) => {
            volume_noun
                && !mute_noun
                && !restore
                && !silence
                && spans.is_empty()
                && requesting
                && ((delta == STEP && up_count == 1 && down_count == 0)
                    || (delta == -STEP && down_count == 1 && up_count == 0))
        }
        VolumeCommand::Mute => {
            silence
                && !restore
                && up_count == 0
                && down_count == 0
                && spans.is_empty()
                && requesting
        }
        VolumeCommand::Unmute => {
            restore && up_count == 0 && down_count == 0 && spans.is_empty() && requesting
        }
        VolumeCommand::Status => {
            up_count == 0
                && down_count == 0
                && !restore
                && spans.is_empty()
                && has(
                    &request,
                    &[
                        "いくつ",
                        "どのくらい",
                        "何",
                        "教えて",
                        "おしえて",
                        "確認",
                        "ですか",
                        "かな",
                    ],
                )
        }
    }
}

fn number_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (offset, c) in text.char_indices() {
        let numeric = c.is_ascii_digit()
            || matches!(
                c,
                '〇' | '零'
                    | '一'
                    | '二'
                    | '三'
                    | '四'
                    | '五'
                    | '六'
                    | '七'
                    | '八'
                    | '九'
                    | '十'
                    | '百'
                    | '千'
                    | '万'
            );
        if numeric {
            start.get_or_insert(offset);
        } else if let Some(start) = start.take() {
            spans.push((start, offset));
        }
    }
    if let Some(start) = start {
        spans.push((start, text.len()));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noncanonical_requests_bind_to_direction_or_explicit_value() {
        for (user, tag, expected) in [
            (
                "ちょっとボリューム小さめにしてくれる",
                "down",
                VolumeCommand::Adjust(-2),
            ),
            (
                "音声をもう少し大きめにしてほしい",
                "up",
                VolumeCommand::Adjust(2),
            ),
            ("音量20にして", "20", VolumeCommand::Set(20)),
            (
                "ボリュームをだいたい２０にしてくれる",
                "20",
                VolumeCommand::Set(20),
            ),
            (
                "声の大きさを二十に設定してくれる",
                "20",
                VolumeCommand::Set(20),
            ),
            (
                "声の大きさを五十パーセントに設定して",
                "128",
                VolumeCommand::Set(128),
            ),
            ("お願い、ミュートにしてくれる", "mute", VolumeCommand::Mute),
            ("音声を元に戻してほしい", "unmute", VolumeCommand::Unmute),
            (
                "ボリュームが今どのくらいか教えて",
                "status",
                VolumeCommand::Status,
            ),
        ] {
            let result = extract(user, &format!("[happy]わかったよ。[volume:{tag}]"));
            assert_eq!(result.command, Some(expected), "{user}");
            assert_eq!(result.text, "[happy]わかったよ。");
            assert!(result.marker_present);
        }
    }

    #[test]
    fn observed_twenty_homophone_is_bound_only_in_a_volume_setting_request() {
        for user in [
            "大い・音量二重にして",
            "大い・音量をだいたい二重に設定してくれる",
        ] {
            assert_eq!(
                extract(user, "[volume:20]").command,
                Some(VolumeCommand::Set(20)),
                "{user}"
            );
            assert_eq!(extract(user, "[volume:200]").command, None, "{user}");
        }
        for user in [
            "音量を二重人格にして",
            "二重人格の話をして",
            "「大い・音量二重にして」と言って",
            "大い・音量二重にしないで",
            "大い・音量二重にしてという例文",
        ] {
            for tag in ["2", "20"] {
                assert_eq!(
                    extract(user, &format!("[volume:{tag}]")).command,
                    None,
                    "{user}"
                );
            }
        }
    }

    #[test]
    fn tags_cannot_invent_a_value_or_reverse_a_requested_action() {
        for (user, tag) in [
            ("音量20にして", "200"),
            ("音量20にして", "up"),
            ("音量を下げて", "up"),
            ("ちょっと音声を小さめにして", "255"),
            ("音量20を教えて", "20"),
            ("音量をマイナス5にして", "5"),
            ("声の大きさを300に設定して", "255"),
            ("音量を15.5にして", "15"),
            ("音量を20デシベルにして", "20"),
            ("今、音量20にしているよ", "20"),
            ("音量20に設定したと思う", "20"),
            ("ミュート解除", "mute"),
        ] {
            assert_eq!(
                extract(user, &format!("[volume:{tag}]")).command,
                None,
                "{user}"
            );
        }
    }

    #[test]
    fn quotes_negations_examples_reports_and_multiple_operations_never_authorize() {
        for (user, tag) in [
            ("「音量20にして」と言って", "20"),
            ("音量20にしてって言ったらどうなる", "20"),
            ("例えば音量20にして", "20"),
            ("音量20にしないで", "20"),
            ("音量を下げないで", "down"),
            ("音量を20にするとどうなる", "20"),
            ("音量20にしてもいい？", "20"),
            ("音量20にしてはいけない", "20"),
            ("音量20にしてほしくない", "20"),
            ("音量20にしてとお願いした", "20"),
            ("音量20にしてという文", "20"),
            ("音量20にしてからミュートして", "20"),
            ("音量20にして、タイマーもセットして", "20"),
            ("音量20か30にして", "20"),
            ("音量20にして、目を閉じて", "20"),
            ("音量を大きくして小さくして", "up"),
            ("ミュートして消音して", "mute"),
            ("ミュート、ミュートして", "mute"),
            ("おはよう", "20"),
            ("天気を教えて", "20"),
        ] {
            assert_eq!(
                extract(user, &format!("返事[volume:{tag}]")).command,
                None,
                "{user}"
            );
        }
    }

    #[test]
    fn only_one_complete_terminal_tag_is_accepted_and_markers_never_reach_speech() {
        for reply in [
            "返事[volume:20][volume:20]",
            "返事[volume:20][volume:mute]",
            "[volume:20]返事",
            "返事[volume:256]",
            "返事[volume:-1]",
            "返事[volume:1.5]",
            "返事[volume:unknown]",
            "返事[volume: 20]",
            "返事[volume:20",
            "返事[volume]",
            "返事[volume=20]",
            "返事[volume:20][timer:10]",
            "返事[Volume:20]",
        ] {
            let result = extract("音量20にして", reply);
            assert!(result.marker_present, "{reply}");
            assert_eq!(result.command, None, "{reply}");
            assert!(
                !result.text.to_ascii_lowercase().contains("[volume"),
                "{reply}"
            );
        }
        let plain = extract("音量20にして", "普通の返事");
        assert!(!plain.marker_present);
        assert_eq!(plain.text, "普通の返事");
        assert_eq!(plain.command, None);
    }
}
