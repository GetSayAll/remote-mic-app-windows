//! 「回眸」文字记录的纯逻辑。
//!
//! 本模块不依赖 Windows、Tauri 或 WebView：记录 schema、应用键、连续差异、
//! 7 天窗口和日文件排序都在这里，平台层只负责读取目标输入框和落盘。

use serde::{Deserialize, Serialize};

/// 单条记录 schema 版本。
pub const TRANSCRIPT_SCHEMA_VERSION: u32 = 1;
/// 日文件格式版本。
pub const TRANSCRIPT_FORMAT_VERSION: u32 = 1;
/// 捕获方法版本（与 Mac 的 `captureMethodVersion` 对齐）。
pub const CAPTURE_METHOD_VERSION: u32 = 1;
/// 单条记录文字上限（与 Mac 一致）。
pub const MAX_TRANSCRIPT_CHARS: usize = 8_000;
/// 单个日文件记录上限。
pub const MAX_RECORDS_PER_DAY: usize = 2_000;
/// 整个归档的读取上限，防止异常目录把页面撑爆。
pub const MAX_RECORDS_TOTAL: usize = 10_000;
/// 全部应用视图的最近窗口：7 天。
pub const RECENT_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

/// 语音来源。当前 Windows 只有实体遥控器；保留 `unknown` 便于旧数据/未来来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSource {
    BluetoothRemote,
    Unknown,
}

impl Default for TranscriptSource {
    fn default() -> Self {
        Self::Unknown
    }
}

impl TranscriptSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BluetoothRemote => "bluetooth_remote",
            Self::Unknown => "unknown",
        }
    }
}

/// 一条「回眸」记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRecord {
    pub schema_version: u32,
    pub id: String,
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub local_date_key: String,
    pub utc_offset_minutes: i32,
    pub application_key: String,
    pub application_name: String,
    #[serde(default)]
    pub source: TranscriptSource,
    pub original_transcript: String,
    pub capture_method_version: u32,
}

impl TranscriptRecord {
    /// 校验记录是否可写入/可展示。失败只返回稳定原因，不返回文字内容。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != TRANSCRIPT_SCHEMA_VERSION {
            return Err("unsupported_schema");
        }
        if self.id.trim().is_empty() || self.session_id.trim().is_empty() {
            return Err("missing_id");
        }
        if !is_safe_application_key(&self.application_key) {
            return Err("invalid_application_key");
        }
        if !is_local_date_key(&self.local_date_key) {
            return Err("invalid_local_date_key");
        }
        if self.application_name.trim().is_empty() {
            return Err("missing_application_name");
        }
        if self.ended_at_ms < self.started_at_ms {
            return Err("invalid_time_range");
        }
        if self.original_transcript.trim().is_empty() {
            return Err("empty_transcript");
        }
        if self.original_transcript.chars().count() > MAX_TRANSCRIPT_CHARS {
            return Err("transcript_too_long");
        }
        if self.capture_method_version == 0 {
            return Err("invalid_capture_method");
        }
        Ok(())
    }
}

/// 一个应用某一天的日文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDayFile {
    pub format_version: u32,
    pub application_key: String,
    pub application_name: String,
    pub local_date_key: String,
    #[serde(default)]
    pub records: Vec<TranscriptRecord>,
}

impl TranscriptDayFile {
    pub fn new(application_key: String, application_name: String, local_date_key: String) -> Self {
        Self {
            format_version: TRANSCRIPT_FORMAT_VERSION,
            application_key,
            application_name,
            local_date_key,
            records: Vec::new(),
        }
    }

    pub fn validate_header(&self) -> Result<(), &'static str> {
        if self.format_version != TRANSCRIPT_FORMAT_VERSION {
            return Err("unsupported_format");
        }
        if !is_safe_application_key(&self.application_key) {
            return Err("invalid_application_key");
        }
        if !is_local_date_key(&self.local_date_key) {
            return Err("invalid_local_date_key");
        }
        Ok(())
    }

    /// 追加记录；重复 id 幂等，超过日上限返回错误。写完调用 `sort_newest_first`。
    pub fn push(&mut self, record: TranscriptRecord) -> Result<(), &'static str> {
        record.validate()?;
        if self.records.iter().any(|existing| existing.id == record.id) {
            return Ok(());
        }
        if self.records.len() >= MAX_RECORDS_PER_DAY {
            return Err("day_limit_reached");
        }
        self.records.push(record);
        Ok(())
    }

    pub fn sort_newest_first(&mut self) {
        self.records.sort_by(|left, right| {
            right
                .ended_at_ms
                .cmp(&left.ended_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
    }
}

/// 一次连续文字变化。字符下标，不是字节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptTextChange {
    pub old_start: usize,
    pub old_end: usize,
    pub new_start: usize,
    pub new_end: usize,
    pub new_text: String,
}

/// 计算原文到新文的单一连续差异。
///
/// 规则：
/// - 只接受**纯新增**（原文被保留，只有中间插入了一段）。这比 Mac 的选区路径保守；
///   没有选区时不能用它猜测“替换”是否来自语音。
/// - 如果前后缀之外还有多处改动，返回 `None`。
/// - 新文字长度超过上限返回 `None`。
/// 语音期间的新增文字判定。纯插入沿用严格规则；另外接受两种已被真机日志证实的替换：
/// 1. 语音开始时整个输入框已选中，语音输入替换选区；
/// 2. 没有选区但整个输入框被替换（例如输入框自带的占位/初始文字被第一段语音覆盖）。
///
/// 替换场景只返回新的输入框文字，不返回、也不落盘任何旧文字。
/// 聊天输入框的空态占位文案。UIA 的 TextPattern 会把占位文字当成正文返回；
/// 捕获层用它区分“发送后清空”与真实听写，避免把「发送消息...」保存成记录。
const PLACEHOLDER_TEXTS: &[&str] = &[
    "发送消息",
    "输入消息",
    "输入你的消息",
    "说点什么",
    "问任何问题",
    "有什么可以帮",
    "有什么可以帮你",
    "sendamessage",
    "sendmessage",
    "typeamessage",
    "typeyourmessage",
    "messagechatgpt",
    "messageclaude",
    "askanything",
    "askaquestion",
    "writeamessage",
];

/// 判断 UIA 读到的文字是否只是输入框的空态占位（忽略空白、大小写、省略号和句末标点）。
pub fn is_placeholder_text(value: &str) -> bool {
    let normalized: String = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| if character == '…' { '.' } else { character })
        .collect();
    let trimmed = normalized.trim_end_matches(|character| {
        matches!(character, '.' | '。' | '!' | '！' | '?' | '？' | ':' | '：')
    });
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_lowercase();
    PLACEHOLDER_TEXTS
        .iter()
        .any(|placeholder| lowered == *placeholder)
}

pub fn continuous_insertion_with_selection(
    original: &str,
    updated: &str,
    selection: Option<&str>,
) -> Option<TranscriptTextChange> {
    let shape = text_change_shape(original, updated)?;
    if shape.removed_chars == 0 {
        return continuous_insertion(original, updated);
    }
    let original_chars: Vec<char> = original.chars().collect();

    if let Some(selection) = selection.filter(|value| !value.is_empty()) {
        let selection_chars: Vec<char> = selection.chars().collect();
        if selection_chars.len() == original_chars.len() && selection_chars == original_chars {
            return whole_replacement(original, updated);
        }
        // 局部选区的偏移无法从 UIA 可靠取得；只接受整个输入框被选中的替换。
        return None;
    }

    if shape.prefix_chars == 0
        && shape.suffix_chars == 0
        && shape.removed_chars == original_chars.len()
    {
        return whole_replacement(original, updated);
    }
    None
}

fn whole_replacement(original: &str, updated: &str) -> Option<TranscriptTextChange> {
    let new_chars = updated.chars().count();
    if new_chars == 0 || new_chars > MAX_TRANSCRIPT_CHARS {
        return None;
    }
    Some(TranscriptTextChange {
        old_start: 0,
        old_end: original.chars().count(),
        new_start: 0,
        new_end: new_chars,
        new_text: updated.to_owned(),
    })
}

pub fn continuous_insertion(original: &str, updated: &str) -> Option<TranscriptTextChange> {
    let shape = text_change_shape(original, updated)?;
    let original_chars: Vec<char> = original.chars().collect();
    let updated_chars: Vec<char> = updated.chars().collect();
    let old_start = shape.prefix_chars;
    let old_end = original_chars.len() - shape.suffix_chars;
    let new_start = shape.prefix_chars;
    let new_end = updated_chars.len() - shape.suffix_chars;
    if old_start != old_end {
        // 原文被删掉或被替换：没有选区证据时拒绝。
        return None;
    }
    if new_end <= new_start {
        return None;
    }
    let new_text: String = updated_chars[new_start..new_end].iter().collect();
    if new_text.chars().count() > MAX_TRANSCRIPT_CHARS {
        return None;
    }
    Some(TranscriptTextChange {
        old_start,
        old_end,
        new_start,
        new_end,
        new_text,
    })
}

/// 只描述文本差异的结构，不携带文本内容；供捕获线程写脱敏诊断日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextChangeShape {
    pub prefix_chars: usize,
    pub suffix_chars: usize,
    pub removed_chars: usize,
    pub inserted_chars: usize,
}

pub fn text_change_shape(original: &str, updated: &str) -> Option<TextChangeShape> {
    if original == updated {
        return None;
    }
    let original_chars: Vec<char> = original.chars().collect();
    let updated_chars: Vec<char> = updated.chars().collect();
    let mut prefix = 0usize;
    while prefix < original_chars.len()
        && prefix < updated_chars.len()
        && original_chars[prefix] == updated_chars[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < original_chars.len().saturating_sub(prefix)
        && suffix < updated_chars.len().saturating_sub(prefix)
        && original_chars[original_chars.len() - 1 - suffix]
            == updated_chars[updated_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }
    Some(TextChangeShape {
        prefix_chars: prefix,
        suffix_chars: suffix,
        removed_chars: original_chars.len().saturating_sub(prefix + suffix),
        inserted_chars: updated_chars.len().saturating_sub(prefix + suffix),
    })
}

/// 生成安全的应用键：显示名清洗 + 稳定哈希。
///
/// `identity` 可以是 AUMID 或可执行文件路径；只用于哈希，不落盘、不写日志。
pub fn application_key(identity: &str, application_name: &str) -> String {
    let source = if identity.trim().is_empty() {
        application_name
    } else {
        identity
    };
    let mut sanitized = String::new();
    for character in application_name.chars() {
        if character.is_alphanumeric() || matches!(character, '.' | '-' | '_') {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
        if sanitized.chars().count() >= 80 {
            break;
        }
    }
    let trimmed = sanitized.trim_matches('_');
    let prefix = if trimmed.is_empty() {
        "unknown"
    } else {
        trimmed
    };
    format!("{prefix}-{:016x}", stable_hash(source))
}

/// FNV-1a 64 位稳定哈希（跨进程、跨版本一致）。
pub fn stable_hash(value: &str) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

pub fn is_safe_application_key(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

pub fn is_local_date_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..].iter().all(u8::is_ascii_digit)
        && {
            let year = value[..4].parse::<u32>().unwrap_or(0);
            let month = value[5..7].parse::<u32>().unwrap_or(0);
            let day = value[8..].parse::<u32>().unwrap_or(0);
            year >= 1970 && (1..=12).contains(&month) && (1..=31).contains(&day)
        }
}

/// 全部应用视图的 7 天过滤；指定应用时返回该应用的全部记录。
pub fn visible_records<'a>(
    records: &'a [TranscriptRecord],
    application_key: Option<&str>,
    now_ms: i64,
) -> Vec<&'a TranscriptRecord> {
    let cutoff = now_ms.saturating_sub(RECENT_WINDOW_MS);
    let mut visible: Vec<&TranscriptRecord> = records
        .iter()
        .filter(|record| match application_key {
            Some(key) => record.application_key == key,
            None => record.ended_at_ms >= cutoff,
        })
        .collect();
    visible.sort_by(|left, right| {
        right
            .ended_at_ms
            .cmp(&left.ended_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    visible
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(text: &str) -> TranscriptRecord {
        TranscriptRecord {
            schema_version: TRANSCRIPT_SCHEMA_VERSION,
            id: "id-1".to_owned(),
            session_id: "session-1".to_owned(),
            started_at_ms: 1_000,
            ended_at_ms: 2_000,
            local_date_key: "2026-10-25".to_owned(),
            utc_offset_minutes: 480,
            application_key: application_key("chrome.exe", "Chrome"),
            application_name: "Chrome".to_owned(),
            source: TranscriptSource::BluetoothRemote,
            original_transcript: text.to_owned(),
            capture_method_version: CAPTURE_METHOD_VERSION,
        }
    }

    #[test]
    fn recognizes_common_chat_placeholders() {
        for value in [
            "发送消息...\n",
            "发送消息…",
            " 发送消息 ",
            "输入消息",
            "说点什么…",
            "Send a message...",
            "Type a message…",
            "Ask anything",
        ] {
            assert!(
                is_placeholder_text(value),
                "should be placeholder: {value:?}"
            );
        }
        for value in ["发送消息给张三", "今晚七点见。", "Send", ""] {
            assert!(
                !is_placeholder_text(value),
                "should not be placeholder: {value:?}"
            );
        }
    }

    #[test]
    fn detects_one_contiguous_insertion_at_caret() {
        let change = continuous_insertion("今天天气不错。", "今天天气不错。晚上见。").unwrap();
        assert_eq!(change.old_start, 7);
        assert_eq!(change.old_end, 7);
        assert_eq!(change.new_start, 7);
        assert_eq!(change.new_end, 11);
        assert_eq!(change.new_text, "晚上见。");
    }

    #[test]
    fn rejects_replacement_without_selection_evidence() {
        assert!(continuous_insertion("旧稿", "新稿").is_none());
    }

    #[test]
    fn accepts_replacement_when_the_whole_input_was_selected() {
        let change =
            continuous_insertion_with_selection("旧稿内容", "新内容", Some("旧稿内容")).unwrap();
        assert_eq!(change.new_text, "新内容");
        assert_eq!(change.old_end, 4);
        assert_eq!(change.new_end, 3);
    }

    #[test]
    fn accepts_whole_field_replacement_without_selection() {
        let change = continuous_insertion_with_selection("输入消息", "你好", None).unwrap();
        assert_eq!(change.new_text, "你好");
    }

    #[test]
    fn rejects_partial_replacement_without_whole_selection_evidence() {
        assert!(continuous_insertion_with_selection("abc旧稿def", "abc新稿def", None).is_none());
        assert!(
            continuous_insertion_with_selection("abc旧稿def", "abc新稿def", Some("旧稿")).is_none()
        );
        assert!(
            continuous_insertion_with_selection("abc旧稿def", "abc新稿def", Some("其他")).is_none()
        );
    }

    #[test]
    fn change_shape_distinguishes_insertion_from_replacement() {
        assert_eq!(text_change_shape("abcd", "abcd"), None);
        assert_eq!(
            text_change_shape("abcd", "abXYcd"),
            Some(TextChangeShape {
                prefix_chars: 2,
                suffix_chars: 2,
                removed_chars: 0,
                inserted_chars: 2,
            })
        );
        assert_eq!(
            text_change_shape("旧稿", "新稿"),
            Some(TextChangeShape {
                prefix_chars: 0,
                suffix_chars: 1,
                removed_chars: 1,
                inserted_chars: 1,
            })
        );
        assert_eq!(
            text_change_shape("输入消息", "你好"),
            Some(TextChangeShape {
                prefix_chars: 0,
                suffix_chars: 0,
                removed_chars: 4,
                inserted_chars: 2,
            })
        );
    }

    #[test]
    fn rejects_unrelated_edits_outside_the_insertion() {
        assert!(continuous_insertion("abc", "XabcY").is_none());
    }

    #[test]
    fn rejects_no_change_and_empty_insertion() {
        assert!(continuous_insertion("abc", "abc").is_none());
        assert!(continuous_insertion("abc", "abc").is_none());
    }

    #[test]
    fn rejects_insertion_beyond_limit() {
        let inserted = "字".repeat(MAX_TRANSCRIPT_CHARS + 1);
        let updated = format!("前缀{inserted}后缀");
        assert!(continuous_insertion("前缀后缀", &updated).is_none());
    }

    #[test]
    fn application_key_is_stable_and_safe() {
        let key = application_key(
            "C:\\Program Files\\Google\\Chrome\\chrome.exe",
            "Google Chrome",
        );
        assert!(is_safe_application_key(&key));
        assert!(key.starts_with("Google_Chrome-"));
        assert_eq!(
            key,
            application_key(
                "C:\\Program Files\\Google\\Chrome\\chrome.exe",
                "Google Chrome"
            )
        );
        assert_ne!(
            key,
            application_key("C:\\Other\\chrome.exe", "Google Chrome")
        );

        let chinese = application_key("C:\\Program Files\\Weixin\\Weixin.exe", "微信");
        assert!(is_safe_application_key(&chinese));
        assert!(chinese.starts_with("微信-"));
    }

    #[test]
    fn validates_record_and_day_file_limits() {
        let mut valid = record("今天见。");
        assert_eq!(valid.validate(), Ok(()));
        valid.original_transcript = "  ".to_owned();
        assert_eq!(valid.validate(), Err("empty_transcript"));

        let mut day = TranscriptDayFile::new(
            application_key("chrome.exe", "Chrome"),
            "Chrome".to_owned(),
            "2026-10-25".to_owned(),
        );
        day.push(record("第一条")).unwrap();
        day.push(record("第一条")).unwrap();
        assert_eq!(day.records.len(), 1);
    }

    #[test]
    fn recent_window_is_seven_days_for_all_apps_but_not_for_filtered_app() {
        let mut old = record("旧记录");
        old.ended_at_ms = 1_000;
        let mut recent = record("新记录");
        recent.ended_at_ms = RECENT_WINDOW_MS + 2_000;
        recent.id = "id-2".to_owned();
        recent.application_key = application_key("wechat.exe", "微信");
        recent.application_name = "微信".to_owned();
        let records = vec![old.clone(), recent.clone()];

        let all = visible_records(&records, None, RECENT_WINDOW_MS + 3_000);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "id-2");

        let filtered = visible_records(
            &records,
            Some(old.application_key.as_str()),
            RECENT_WINDOW_MS + 3_000,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "id-1");
    }

    #[test]
    fn local_date_key_rejects_future_shapes() {
        assert!(is_local_date_key("2026-10-25"));
        assert!(is_local_date_key("2026-02-31"));
        assert!(!is_local_date_key("2026-2-25"));
        assert!(!is_local_date_key("2026/10/25"));
        assert!(!is_local_date_key("2026-13-01"));
    }
}
