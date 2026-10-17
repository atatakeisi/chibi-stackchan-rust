//! Deterministic voice commands for the same 0..=255 volume used by the web UI.
//!
//! Only complete requests are accepted. A quoted example, a negated command,
//! or a sentence that merely mentions the volume must not change the speaker.

const MAX_REQUEST_BYTES: usize = 4096;
pub const STEP: i16 = 2;
pub const DEFAULT_RESTORED_VOLUME: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeCommand {
    Set(u8),
    Adjust(i16),
    Mute,
    Unmute,
    Status,
}

/// Recognize common Japanese transcription variants without involving an LLM.
/// Bare numbers use the web UI's 0..=255 scale; `%` explicitly means percent.
pub fn parse(text: &str) -> Option<VolumeCommand> {
    let normalized = normalize(text)?;
    let request = without_courtesy(without_leadin(&normalized));
    if request.is_empty() {
        return None;
    }

    // Test complete unmute phrases first: they contain the words for mute.
    if [
        "ミュート解除",
        "ミュート解除して",
        "ミュートを解除",
        "ミュートを解除して",
        "消音解除",
        "消音解除して",
        "消音を解除",
        "消音を解除して",
        "音量を戻して",
        "音量を元に戻して",
        "音を戻して",
        "声を戻して",
    ]
    .contains(&request)
    {
        return Some(VolumeCommand::Unmute);
    }
    if [
        "ミュート",
        "ミュートして",
        "ミュートにして",
        "消音",
        "消音して",
        "消音にして",
        "音を消して",
        "声を消して",
    ]
    .contains(&request)
    {
        return Some(VolumeCommand::Mute);
    }

    let status = request
        .strip_prefix("今の")
        .or_else(|| request.strip_prefix("現在の"))
        .unwrap_or(request);
    if ["音量", "ボリューム", "おんりょう", "オンリョウ"]
        .iter()
        .any(|noun| {
            status.strip_prefix(noun).is_some_and(|rest| {
                [
                    "",
                    "は",
                    "いくつ",
                    "はいくつ",
                    "は何",
                    "は何パーセント",
                    "を教えて",
                    "教えて",
                    "はどのくらい",
                    "どのくらい",
                ]
                .contains(&without_noun_separator(rest))
            })
        })
    {
        return Some(VolumeCommand::Status);
    }

    let (request, softened) = without_degree(request);
    let volume_request = without_volume_noun(request);
    if let Some(rest) = volume_request {
        let (rest, _) = without_degree(rest);
        if let Some(command) = direction(rest) {
            return Some(command);
        }
        if let Some(command) = numeric_request(rest) {
            return Some(command);
        }
    }
    // Without an explicit noun, accept only these short, unmistakable requests.
    if request == "静かにして" || (softened && matches!(request, "小さくして" | "大きくして"))
    {
        return direction(request);
    }
    None
}

/// Identify an unsupported explicit volume request so callers can explain the
/// accepted syntax rather than letting an LLM claim that a setting changed.
/// This function never authorizes a change; only `parse` supplies a command.
pub fn looks_like_volume_request(text: &str) -> bool {
    if parse(text).is_some() {
        return true;
    }
    let Some(normalized) = normalize(text) else {
        return false;
    };
    let request = without_courtesy(without_leadin(&normalized));
    let (request, _) = without_degree(request);
    let Some(rest) = without_volume_noun(request) else {
        return false;
    };
    let (rest, _) = without_degree(rest);
    // These endings identify requests, including invalid numeric settings.
    (numeric_intent(rest)
        || [
            "にして",
            "に設定",
            "に設定して",
            "上げて",
            "下げて",
            "調整して",
        ]
        .iter()
        .any(|suffix| rest.ends_with(suffix)))
        && !["ないで", "なくて", "と言", "って言", "例えば", "たとえば"]
            .iter()
            .any(|part| request.contains(part))
}

// Invalid bare settings such as "音量300" must not fall through to an LLM.
// Require the entire remainder to look numeric so questions and anecdotes are
// still ordinary conversation rather than volume operations.
fn numeric_intent(text: &str) -> bool {
    let number = text
        .strip_suffix("パーセント")
        .or_else(|| text.strip_suffix('%'))
        .unwrap_or(text);
    let is_digit = |c: char| {
        c.is_ascii_digit()
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
            )
    };
    number.chars().any(is_digit)
        && number
            .chars()
            .all(|c| is_digit(c) || matches!(c, '+' | '-' | '＋' | '－' | '−' | '.' | '．' | ','))
}

/// Resolve against the current setting. Persistence and remembering the most
/// recent nonzero value belong to the caller. Status performs no mutation.
pub fn resolve(command: VolumeCommand, current: u8, previous_nonzero: u8) -> Option<u8> {
    Some(match command {
        VolumeCommand::Set(value) => value,
        VolumeCommand::Adjust(delta) => (i32::from(current) + i32::from(delta)).clamp(0, 255) as u8,
        VolumeCommand::Mute => 0,
        VolumeCommand::Unmute => {
            if current > 0 {
                current
            } else if previous_nonzero > 0 {
                previous_nonzero
            } else {
                DEFAULT_RESTORED_VOLUME
            }
        }
        VolumeCommand::Status => return None,
    })
}

fn normalize(text: &str) -> Option<String> {
    if text.len() > MAX_REQUEST_BYTES
        || text
            .chars()
            .any(|c| matches!(c, '「' | '」' | '『' | '』' | '"' | '\'' | '“' | '”'))
    {
        return None;
    }
    let normalized: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap(),
            '％' => '%',
            other => other,
        })
        .collect();
    Some(
        normalized
            .trim_matches(['。', '、', '！', '!', '？', '?'])
            .to_owned(),
    )
}

/// Remove only fixed attention-getters at the start of a complete request.
/// Device names are handled using the configured wake phrases by the caller.
fn without_leadin(request: &str) -> &str {
    for prefix in [
        "あのね",
        "おーい",
        "ねえ",
        "ねぇ",
        "あの",
        // Observed transcription of the user's "おーい" call. Require its
        // separator so an unrelated word beginning with 大い is not removed.
        "大い・",
        "大い、",
        "大い,",
        "大い，",
    ] {
        if let Some(rest) = request.strip_prefix(prefix) {
            return rest.trim_start_matches(['、', ',', '。', '！', '!']);
        }
    }
    request
}

fn without_courtesy(request: &str) -> &str {
    let request = ["よね", "ね", "よ"]
        .iter()
        .find_map(|suffix| request.strip_suffix(suffix))
        .unwrap_or(request);
    for suffix in [
        "いただけませんか",
        "いただけますか",
        "いただけるかな",
        "もらえませんか",
        "くれませんか",
        "もらえますか",
        "もらえるかな",
        "くれるかな",
        "お願いします",
        "くれますか",
        "ほしいです",
        "欲しいです",
        "ください",
        "下さい",
        "いただける",
        "もらえる",
        "くれる",
        "ほしい",
        "欲しい",
        "お願い",
    ] {
        if let Some(rest) = request.strip_suffix(suffix) {
            return rest.trim_end_matches(['、', ',']);
        }
    }
    request
}

fn without_degree(request: &str) -> (&str, bool) {
    for prefix in [
        "もう少しだけ",
        "もうすこしだけ",
        "もうちょっとだけ",
        "もう少し",
        "もうすこし",
        "もうちょっと",
        "ちょっとだけ",
        "すこしだけ",
        "少しだけ",
        "ちょっと",
        "すこし",
        "少し",
        "もっと",
    ] {
        if let Some(rest) = request.strip_prefix(prefix) {
            return (rest, true);
        }
    }
    (request, false)
}

fn without_volume_noun(request: &str) -> Option<&str> {
    [
        "スピーカーの音量を",
        "スピーカーの音量",
        "声の音量を",
        "声の音量",
        "声の大きさを",
        "声の大きさ",
        "ボリュームを",
        "ボリューム",
        "音量を",
        "音量",
        "おんりょうを",
        "おんりょう",
        "オンリョウを",
        "オンリョウ",
        "音声を",
        "声を",
        "音を",
    ]
    .iter()
    .find_map(|prefix| {
        let rest = without_noun_separator(request.strip_prefix(prefix)?);
        // A recognizer can place the noun's particle after its comma:
        // "音量、を20にして". Do not remove punctuation elsewhere in a request.
        let rest = if prefix.ends_with('を') {
            rest
        } else {
            rest.strip_prefix('を').unwrap_or(rest)
        };
        Some(without_noun_separator(rest))
    })
}

fn without_noun_separator(request: &str) -> &str {
    // Commas can mark a spoken pause. Sentence boundaries and separators
    // between multiple commands remain intact for complete-request matching.
    request.trim_start_matches(['、', ',', '，'])
}

fn direction(request: &str) -> Option<VolumeCommand> {
    if [
        "下げて",
        "小さくして",
        "小さく",
        "抑えて",
        "絞って",
        "静かにして",
    ]
    .contains(&request)
    {
        Some(VolumeCommand::Adjust(-STEP))
    } else if ["上げて", "大きくして", "大きく"].contains(&request) {
        Some(VolumeCommand::Adjust(STEP))
    } else {
        None
    }
}

fn numeric_request(request: &str) -> Option<VolumeCommand> {
    // Match the longest endings first so the amount remains a pure number.
    for (suffix, relative) in [
        ("だけ下げて", -1),
        ("だけ上げて", 1),
        ("に設定して", 0),
        ("に下げて", 0),
        ("に上げて", 0),
        ("に設定", 0),
        ("にして", 0),
        ("下げて", -1),
        ("上げて", 1),
        ("で", 0),
        ("に", 0),
        ("", 0),
    ] {
        if let Some(amount) = request.strip_suffix(suffix) {
            if let Some(value) = volume_number(amount) {
                return Some(if relative == 0 {
                    VolumeCommand::Set(value)
                } else {
                    VolumeCommand::Adjust(i16::from(value) * relative)
                });
            }
        }
    }
    None
}

fn volume_number(text: &str) -> Option<u8> {
    let (number, percent) = if let Some(number) = text
        .strip_suffix("パーセント")
        .or_else(|| text.strip_suffix('%'))
    {
        (number, true)
    } else {
        (text, false)
    };
    // Observed STT output for "二十" in an explicit numeric volume slot.
    // Match the complete amount only; never replace this text in a sentence.
    let value = if number == "二重" {
        20
    } else {
        integer(number)?
    };
    if percent {
        (value <= 100).then(|| ((value * 255 + 50) / 100) as u8)
    } else {
        u8::try_from(value).ok()
    }
}

/// Arabic digits or a small Japanese integer, with no signs or separators.
fn integer(text: &str) -> Option<u16> {
    if text.is_empty() || text.chars().count() > 5 {
        return None;
    }
    let digit = |c| match c {
        '0'..='9' => Some(c as u16 - '0' as u16),
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
    };
    if text == "ゼロ" {
        return Some(0);
    }
    if !text.contains(['十', '百']) {
        return text.chars().try_fold(0u16, |value, c| {
            value.checked_mul(10)?.checked_add(digit(c)?)
        });
    }
    let mut total = 0u16;
    let mut pending = None;
    let mut last_unit = 1000u16;
    for c in text.chars() {
        let unit = match c {
            '百' => 100,
            '十' => 10,
            _ => {
                let value = digit(c)?;
                if pending.is_some() || value == 0 {
                    return None;
                }
                pending = Some(value);
                continue;
            }
        };
        if unit >= last_unit {
            return None;
        }
        total = total.checked_add(pending.take().unwrap_or(1) * unit)?;
        last_unit = unit;
    }
    total.checked_add(pending.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gentle_relative_commands_cover_common_speech() {
        for text in [
            "音量を下げて",
            "音量下げてください。",
            "声を小さくして",
            "もう少し小さくして",
            "もう少し音量を下げて",
            "音量をちょっと下げてくれますか？",
            "音量を下げて、お願いします。",
            "もうちょっと小さくして。",
            "静かにして",
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Adjust(-2)), "{text}");
        }
        for text in ["音量を上げて", "声を大きくして", "もう少し大きくして"] {
            assert_eq!(parse(text), Some(VolumeCommand::Adjust(2)), "{text}");
        }
    }

    #[test]
    fn attention_getters_and_spoken_courtesy_preserve_complete_requests() {
        for (text, command) in [
            ("おーい、ミュートして", VolumeCommand::Mute),
            ("おーい！ミュート解除してよ。", VolumeCommand::Unmute),
            ("ねえ、音量をもう少し下げて。", VolumeCommand::Adjust(-2)),
            ("ねぇ、ボリュームを上げてよ", VolumeCommand::Adjust(2)),
            ("あの、音量15にしてよ", VolumeCommand::Set(15)),
            ("あのね、音量を15にしてくださいね", VolumeCommand::Set(15)),
            ("音量を下げていただけますか？", VolumeCommand::Adjust(-2)),
            ("音量を下げていただけませんか？", VolumeCommand::Adjust(-2)),
            ("もう少し小さくしてくれるかな", VolumeCommand::Adjust(-2)),
            ("音量を上げてもらえるかな", VolumeCommand::Adjust(2)),
            ("音量を小さくしてほしいです", VolumeCommand::Adjust(-2)),
            ("音量を小さくして欲しいです", VolumeCommand::Adjust(-2)),
            ("音量を15にしてよね", VolumeCommand::Set(15)),
            ("ねえ、今の音量を教えてよ", VolumeCommand::Status),
        ] {
            assert_eq!(parse(text), Some(command), "{text}");
        }
    }

    #[test]
    fn volume_synonyms_and_small_degrees_are_understood() {
        for text in [
            "音声を小さくして",
            "声の大きさを少し下げて",
            "音量をもうちょっとだけ下げて",
            "音量をもう少しだけ下げて",
            "音量をすこしだけ下げて",
            "ちょっとだけ音量を下げて",
            "音量もうちょっと小さくして",
            "音量を小さくしてください",
            "ボリュームを少し下げてくれる？",
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Adjust(-2)), "{text}");
        }
        assert_eq!(parse("音声を大きくして"), Some(VolumeCommand::Adjust(2)));
        assert_eq!(
            parse("音量を15にしてください。"),
            Some(VolumeCommand::Set(15))
        );
    }

    #[test]
    fn transcription_commas_are_allowed_only_after_the_volume_noun() {
        for (text, command) in [
            ("音量、20にして", VolumeCommand::Set(20)),
            ("音量、上げて", VolumeCommand::Adjust(2)),
            ("音量,下げて", VolumeCommand::Adjust(-2)),
            ("音量を，20にして", VolumeCommand::Set(20)),
            ("音量、を20にして", VolumeCommand::Set(20)),
            ("ボリューム、少し下げて", VolumeCommand::Adjust(-2)),
            ("ねえ、音量、20にしてください", VolumeCommand::Set(20)),
            ("音量、いくつ？", VolumeCommand::Status),
            ("おんりょう20にして", VolumeCommand::Set(20)),
            ("オンリョウ、下げて", VolumeCommand::Adjust(-2)),
        ] {
            assert_eq!(parse(text), Some(command), "{text}");
        }
        for text in [
            "音量。上げて",
            "音量！上げて",
            "音量？上げて",
            "音量、上げて、下げて",
            "音量、20にして、10にして",
            "音量、下げて。いや上げて",
            "音量、下げないで",
            "音量、を上げないで",
            "音量、上げてと言って",
            "音量、『20にして』",
            "おんりょう、上げてから下げて",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    #[test]
    fn absolute_values_use_the_same_units_as_the_web_ui() {
        for (text, value) in [
            ("音量を10にして", 10),
            ("音量１０", 10),
            ("音量を ５ にしてください。", 5),
            ("ボリュームを255に設定して", 255),
            ("音量を十にして", 10),
            ("音量を二十にして", 20),
            ("音量を百二十八にして", 128),
            ("音量を二百五十五にして", 255),
            ("音量を二五五にして", 255),
            ("音量をゼロにして", 0),
            ("音量を〇にして", 0),
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Set(value)), "{text}");
        }
    }

    #[test]
    fn observed_twenty_homophone_is_confined_to_a_numeric_volume_amount() {
        for text in [
            "音量二重にして",
            "大い・音量二重にして",
            "大い、音量二重にして",
            "大い,音量二重にして",
            "大い，音量二重にして",
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Set(20)), "{text}");
        }
        for text in [
            "二重にして",
            "大い音量二重にして",
            "音量二重人格にして",
            "音量二重人格の話をして",
            "音量二重にしないで",
            "大い・音量二重にしないで",
            "音量二重にしてと言って",
            "音量二重にして、三十にして",
            "大い・音量二重にして、ミュートして",
            "「大い・音量二重にして」と言って",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    #[test]
    fn percent_rounds_and_endpoints_are_exact() {
        for (text, value) in [
            ("音量を0%にして", 0),
            ("音量を１％にして", 3),
            ("音量を10パーセントにして", 26),
            ("音量を五十パーセントにして", 128),
            ("音量を100%にして", 255),
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Set(value)), "{text}");
        }
    }

    #[test]
    fn numeric_relative_changes_and_absolute_direction_differ() {
        for (text, command) in [
            ("音量を3下げて", VolumeCommand::Adjust(-3)),
            ("音量を二だけ上げて", VolumeCommand::Adjust(2)),
            ("音量を10%下げて", VolumeCommand::Adjust(-26)),
            ("音量を10に下げて", VolumeCommand::Set(10)),
            ("音量を20に上げて", VolumeCommand::Set(20)),
        ] {
            assert_eq!(parse(text), Some(command), "{text}");
        }
    }

    #[test]
    fn unmute_does_not_match_mute_substrings() {
        for text in [
            "ミュート解除",
            "ミュートを解除して",
            "消音を解除してください",
            "音量を戻して",
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Unmute), "{text}");
        }
        for text in ["ミュートして", "消音して", "音を消して"] {
            assert_eq!(parse(text), Some(VolumeCommand::Mute), "{text}");
        }
    }

    #[test]
    fn status_queries_cannot_change_settings() {
        for text in [
            "音量はいくつ？",
            "今の音量を教えて",
            "現在の音量は何パーセント？",
            "音量は？",
        ] {
            assert_eq!(parse(text), Some(VolumeCommand::Status), "{text}");
        }
        assert_eq!(resolve(VolumeCommand::Status, 10, 5), None);
    }

    #[test]
    fn malformed_and_out_of_range_numbers_do_not_become_commands() {
        for text in [
            "音量を-1にして",
            "音量を+10にして",
            "音量を256にして",
            "音量を二百五十六にして",
            "音量を101%にして",
            "音量を1.5にして",
            "音量を十百にして",
            "音量を一二十にして",
            "音量を10と20にして",
            "音量を99999にして",
        ] {
            assert_eq!(parse(text), None, "{text}");
            assert!(looks_like_volume_request(text), "{text}");
        }
    }

    #[test]
    fn invalid_bare_settings_receive_local_help_instead_of_llm_acknowledgement() {
        for text in [
            "音量300",
            "音量を３００",
            "音量を三百",
            "音量101%",
            "ボリューム101パーセント",
            "音量-1",
            "音量＋10",
            "音量1.5",
            "音量ちょっと99999",
        ] {
            assert_eq!(parse(text), None, "{text}");
            assert!(looks_like_volume_request(text), "{text}");
        }
        for text in [
            "音量10って何？",
            "音量300は大きすぎる",
            "音量101%は使える？",
            "音量-1とはどういう意味？",
            "音量を三百にしないで",
            "「音量300」と言って",
        ] {
            assert_eq!(parse(text), None, "{text}");
            assert!(!looks_like_volume_request(text), "{text}");
        }
    }

    #[test]
    fn other_conversation_negation_and_multiple_commands_are_inert() {
        for text in [
            "3分のタイマーをセットして",
            "大きな声で説明して",
            "小さな声は聞き取りにくい",
            "音量を下げないで",
            "音量を上げなくていい",
            "ミュートしないで",
            "音量を上げて下げて",
            "音量を10にして、20にして",
            "音量を上げてと言った",
            "「音量を上げて」と言って",
            "例えば音量を10にして",
            "音量を上げてもいい？",
            "音量を調整できますか？",
            "昨日音量を下げていた",
            "ねえ、音量を下げないで",
            "あの、音量を下げてと言って",
            "音量15にしてよと言われた",
            "音声を小さくして説明して",
            "音量を下げられる？",
            "音量を下げて、タイマー3分",
            "音量を上げてから下げて",
            "ねえ、「音量を15にして」と言って",
            "おーい、ミュートしないで",
            "おーい、音量を上げて下げて",
            "",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    #[test]
    fn unsupported_spoken_requests_receive_help_without_changing_settings() {
        for text in [
            "あの、音量を300にしてよ",
            "ねぇ、音量を101パーセントにしてくださいね",
            "おーい、音量を半分にしてよ",
        ] {
            assert_eq!(parse(text), None, "{text}");
            assert!(looks_like_volume_request(text), "{text}");
        }
    }

    #[test]
    fn adjustments_saturate_even_for_arbitrary_deltas() {
        assert_eq!(resolve(VolumeCommand::Adjust(-2), 1, 1), Some(0));
        assert_eq!(resolve(VolumeCommand::Adjust(2), 254, 254), Some(255));
        assert_eq!(resolve(VolumeCommand::Adjust(i16::MIN), 255, 10), Some(0));
        assert_eq!(resolve(VolumeCommand::Adjust(i16::MAX), 0, 10), Some(255));
        assert_eq!(resolve(VolumeCommand::Adjust(-2), 10, 10), Some(8));
    }

    #[test]
    fn mute_restores_previous_setting_without_an_unexpected_jump() {
        assert_eq!(resolve(VolumeCommand::Mute, 5, 5), Some(0));
        assert_eq!(resolve(VolumeCommand::Unmute, 0, 5), Some(5));
        assert_eq!(resolve(VolumeCommand::Unmute, 7, 20), Some(7));
        assert_eq!(resolve(VolumeCommand::Unmute, 0, 0), Some(10));
    }

    #[test]
    fn long_and_quoted_requests_are_rejected() {
        let long = format!("{}音量を下げて", " ".repeat(MAX_REQUEST_BYTES));
        assert_eq!(parse(&long), None);
        assert!(!looks_like_volume_request(&long));
        assert_eq!(parse("音量を\"10\"にして"), None);
    }
}
