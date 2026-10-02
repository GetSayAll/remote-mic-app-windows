//! 聚焦输入框的纯逻辑（跨平台可编译、可单测）。
//!
//! 平台层（`focus_windows`）负责把 UIA 元素转换成 [`FocusCandidate`]；本模块只做
//! 判定、打分、档案校验与请求门控，不依赖任何 Windows API。
//!
//! 设计依据：`docs/investigations/2026-10-01-windows-input-focus-uia-feasibility.md`
//! 与 mac 版行为规格（聚焦策略、敏感词/排除词、阈值与分差、请求作废语义）。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::send_input::KeyChord;

/// 聚焦档案条数上限（防止配置文件被无限膨胀）。
pub const MAX_FOCUS_PROFILES: usize = 200;
/// 档案里单个语义字段的长度上限。
pub const MAX_SEMANTIC_LEN: usize = 256;
/// 祖先上下文 token 数量与单个 token 长度上限。
pub const MAX_CONTEXT_TOKENS: usize = 24;
pub const MAX_CONTEXT_TOKEN_LEN: usize = 32;
/// 通用输入框打分阈值（对标 mac 的 ≥80）。
pub const COMPOSER_SCORE_THRESHOLD: i32 = 80;
/// 已记录目标匹配阈值与「与第二名的最小分差」（对标 mac 的 ≥180 / ≥40）。
pub const RECORDED_SCORE_THRESHOLD: i32 = 180;
pub const RECORDED_SCORE_MARGIN: i32 = 40;

/// 敏感字段词表（与 mac 版一致，含中英）。
const SENSITIVE_TERMS: &[&str] = &[
    "password",
    "passcode",
    "secret",
    "api key",
    "apikey",
    "token",
    "credit card",
    "密码",
    "口令",
    "密钥",
    "令牌",
    "银行卡",
];

/// 明确不该聚焦的排除词（浏览器地址栏、命令面板、重命名等）。
///
/// 注意：「搜索 / search」不在此列——网页内的搜索框按 mac 语义仍是允许的目标，
/// 只有浏览器自身的地址栏（`OmniboxViewViews` / 「地址和搜索栏」）要排除。
const EXCLUDED_TERMS: &[&str] = &[
    "address bar",
    "omnibox",
    "command palette",
    "code editor",
    "monaco",
    "rename",
    "title",
    "地址栏",
    "地址和搜索栏",
    "命令面板",
    "代码编辑器",
    "重命名",
];

/// 强语义词：命中即说明它很可能就是「对话输入框」。
const STRONG_TERMS: &[&str] = &[
    "composer",
    "prompt-editor",
    "prompt_editor",
    "chat-input",
    "chat_input",
    "message-input",
    "message_input",
    "prompt input",
    "message input",
    "消息输入",
    "输入消息",
    "发送消息",
    "说点什么",
];

/// 辅助语义词。
const SUPPORTING_TERMS: &[&str] = &[
    "message",
    "prompt",
    "reply",
    "chat",
    "ask anything",
    "提问",
    "回复",
    "输入框",
];

/// 显式允许（可接受的输入目标，但不一定是主输入框）。
const ALLOWED_TERMS: &[&str] = &[
    "search",
    "find",
    "filter",
    "settings",
    "preferences",
    "terminal",
    "console",
    "shell",
    "xterm",
    "搜索",
    "查找",
    "筛选",
    "设置",
    "偏好",
    "终端",
    "控制台",
];

/// 目标应用的聚焦方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusStrategy {
    /// 只打开应用（默认）。
    #[default]
    OpenOnly,
    /// 打开后发送该应用自己的聚焦快捷键。
    AppShortcut,
    /// 打开后聚焦用户记录的输入框。
    RecordedElement,
}

/// 相对目标顶层窗口的归一化矩形（物理像素换算成比例）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// 比例用 f64 表示；唯一构造入口 `from_bounds` 只接受有限值，因此这里可以满足
// `Eq` 的自反性要求（手写 `Eq` 只是 Marker 承诺，不改变 `PartialEq` 语义）。
// 没有它，`ButtonMappings`（derive Eq）就无法容纳聚焦档案。
impl Eq for NormalizedRect {}

impl NormalizedRect {
    /// 由元素矩形与窗口矩形（物理像素）换算；任一矩形非有限或尺寸非正时返回 `None`。
    ///
    /// UIA 的 `BoundingRectangle` 可能是离屏值甚至 ±∞（最小化窗口、合成元素），
    /// 因此这里必须先做有限性与正尺寸校验再换算。
    pub fn from_bounds(
        element: Option<(f64, f64, f64, f64)>,
        window: Option<(f64, f64, f64, f64)>,
    ) -> Option<Self> {
        let (ex, ey, ew, eh) = element?;
        let (wx, wy, ww, wh) = window?;
        if !all_finite_and_positive(&[(ew, eh), (ww, wh)]) {
            return None;
        }
        if !all_finite(&[ex, ey, wx, wy]) {
            return None;
        }
        if ww <= 0.0 || wh <= 0.0 || ew <= 0.0 || eh <= 0.0 {
            return None;
        }
        Some(Self {
            x: (ex - wx) / ww,
            y: (ey - wy) / wh,
            width: ew / ww,
            height: eh / wh,
        })
    }
}

fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn all_finite_and_positive(pairs: &[(f64, f64)]) -> bool {
    pairs
        .iter()
        .all(|(a, b)| a.is_finite() && b.is_finite() && *a > 0.0 && *b > 0.0)
}

fn clamp_chars(value: &str, limit: usize) -> String {
    value.trim().chars().take(limit).collect()
}

/// 用户记录的输入框语义特征（不包含任何输入内容）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedFocusTarget {
    pub control_type: String,
    pub automation_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub window_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rect: Option<NormalizedRect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_tokens: Vec<String>,
}

impl RecordedFocusTarget {
    /// 规范化：裁剪长度、裁剪 token、拒绝敏感语义与「没有任何可比特征」的记录。
    pub fn normalized(mut self) -> Option<Self> {
        self.control_type = clamp_chars(&self.control_type, MAX_SEMANTIC_LEN);
        self.automation_id = clamp_chars(&self.automation_id, MAX_SEMANTIC_LEN);
        self.class_name = clamp_chars(&self.class_name, MAX_SEMANTIC_LEN);
        self.name = clamp_chars(&self.name, MAX_SEMANTIC_LEN);
        self.window_title = clamp_chars(&self.window_title, MAX_SEMANTIC_LEN);
        self.context_tokens = self
            .context_tokens
            .into_iter()
            .map(|token| clamp_chars(&token, MAX_CONTEXT_TOKEN_LEN))
            .filter(|token| !token.is_empty())
            .take(MAX_CONTEXT_TOKENS)
            .collect();

        if self.control_type.is_empty() {
            return None;
        }
        // 至少一个可比对特征，否则这份记录无法用于重新定位。
        if self.automation_id.is_empty()
            && self.class_name.is_empty()
            && self.name.is_empty()
            && self.window_title.is_empty()
        {
            return None;
        }
        // 敏感与排除词只看控件语义（不含窗口标题——标题可能是用户文档名，
        // 误判会把正常记录整条丢掉）。
        let semantic = self.semantic_text();
        if contains_sensitive_term(&semantic) || contains_excluded_term(&semantic) {
            return None;
        }
        Some(self)
    }

    /// 语义字段（不含窗口标题）的合并文本，用于敏感词判定与相似度。
    pub fn semantic_text(&self) -> String {
        format!(
            "{} {} {} {}",
            self.automation_id,
            self.class_name,
            self.name,
            self.context_tokens.join(" ")
        )
        .to_lowercase()
    }
}

/// 单个目标（预设应用 id 或自定义应用路径）的聚焦档案。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppFocusProfile {
    #[serde(default)]
    pub strategy: FocusStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<KeyChord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<RecordedFocusTarget>,
}

impl AppFocusProfile {
    /// 规范化：字段与策略必须自洽，否则整条返回 `None`（调用方丢弃并记日志）。
    pub fn normalized(mut self) -> Option<Self> {
        match self.strategy {
            FocusStrategy::OpenOnly => {
                self.shortcut = None;
                self.recorded = None;
                Some(self)
            }
            FocusStrategy::AppShortcut => {
                self.recorded = None;
                let has_keys = self
                    .shortcut
                    .as_ref()
                    .is_some_and(|chord| !chord.keys.is_empty());
                if !has_keys {
                    return None;
                }
                Some(self)
            }
            FocusStrategy::RecordedElement => {
                self.shortcut = None;
                self.recorded = Some(self.recorded.and_then(|target| target.normalized())?);
                Some(self)
            }
        }
    }
}

/// 平台层采集到的候选输入框（只含语义与几何，不含输入内容）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusCandidate {
    /// UIA 控制类型名（`Edit` / `Document` / `Group` / 其它）。
    pub control_type: String,
    pub automation_id: String,
    pub class_name: String,
    pub name: String,
    pub window_title: String,
    pub context_tokens: Vec<String>,
    pub normalized_rect: Option<NormalizedRect>,
    pub enabled: bool,
    pub keyboard_focusable: bool,
    pub is_password: bool,
    pub read_only: Option<bool>,
    /// UIA `IsTextPatternAvailable`：控件承载可读文本。
    pub has_text_pattern: bool,
    /// 该元素当前是否持有键盘焦点（用于「优先保留用户已在用的输入框」）。
    pub focused: bool,
}

/// 现代网页编辑器的语义提示。实测（2026-10-02）：TipTap / ProseMirror 的
/// `contenteditable` 在 Windows UIA 上是 `ControlType.Group` 而不是 Edit/Document
/// （Chromium 会把 DOM class 原样暴露，例如 `tiptap ProseMirror … ProseMirror-focused`）。
/// 只认 Edit/Document 会漏掉这类输入框，因此允许「可聚焦 + 编辑器语义」的控件入候选。
const EDITOR_SEMANTIC_HINTS: &[&str] = &[
    "prosemirror",
    "tiptap",
    "contenteditable",
    "cm-editor",
    "codemirror",
    "ql-editor",
    "slate-editor",
];

impl FocusCandidate {
    /// 控制类型是否是文本输入类（mac 的 `AXTextArea` / `AXTextField` 等价物）。
    pub fn is_text_control(&self) -> bool {
        matches!(self.control_type.as_str(), "Edit" | "Document")
    }

    /// 是否带富文本编辑器语义（class / automation id / name 命中编辑器特征词）。
    fn has_editor_semantics(&self) -> bool {
        let semantic = self.semantic_text();
        EDITOR_SEMANTIC_HINTS
            .iter()
            .any(|hint| semantic.contains(hint))
    }

    /// 硬门槛：可用、可聚焦、非密码框、非只读、语义不含敏感/排除词，且属于文本输入类、
    /// 带文本模式或带编辑器语义（三者之一）。
    ///
    /// `read_only == Some(true)` 直接拒绝：Chromium 会把网页根节点暴露成
    /// `Document` 且标 `IsReadOnly=true`（实测），聚焦它既不能输入也会抢走真正的
    /// 输入框；`None`（拿不到值模式）不在此列，保持宽松。
    pub fn passes_hard_gate(&self) -> bool {
        if !self.enabled || !self.keyboard_focusable || self.is_password {
            return false;
        }
        if self.read_only == Some(true) {
            return false;
        }
        let semantic = self.semantic_text();
        if contains_sensitive_term(&semantic) || contains_excluded_term(&semantic) {
            return false;
        }
        self.is_text_control() || self.has_text_pattern || self.has_editor_semantics()
    }

    /// 语义字段（不含窗口标题）的合并文本。
    pub fn semantic_text(&self) -> String {
        format!(
            "{} {} {} {}",
            self.automation_id,
            self.class_name,
            self.name,
            self.context_tokens.join(" ")
        )
        .to_lowercase()
    }
}

/// 语义（或含窗口标题的合并文本）是否命中敏感词。
pub fn contains_sensitive_term(text: &str) -> bool {
    let lowered = text.to_lowercase();
    SENSITIVE_TERMS.iter().any(|term| lowered.contains(term))
}

/// 是否命中「明确不该聚焦」的排除词（地址栏、命令面板、重命名等）。
pub fn contains_excluded_term(text: &str) -> bool {
    let lowered = text.to_lowercase();
    EXCLUDED_TERMS.iter().any(|term| lowered.contains(term))
}

/// 通用输入框打分；不合格返回 `None`。
pub fn composer_candidate_score(candidate: &FocusCandidate) -> Option<i32> {
    if !candidate.passes_hard_gate() {
        return None;
    }
    let semantic = candidate.semantic_text();
    // Edit / Document 基础分更高；其它类型（实测：TipTap/ProseMirror 的 Group）
    // 需要靠语义或几何补足阈值。
    let mut score = if candidate.is_text_control() { 50 } else { 30 };
    if candidate.focused {
        // 用户当前已在用的输入框优先保留（DimAgent 实测：编辑器节点自带焦点）。
        score += 40;
    }
    if STRONG_TERMS.iter().any(|term| semantic.contains(term)) {
        score += 120;
    } else if SUPPORTING_TERMS.iter().any(|term| semantic.contains(term)) {
        score += 70;
    } else if ALLOWED_TERMS.iter().any(|term| semantic.contains(term)) {
        score += 80;
    }

    if let Some(rect) = candidate.normalized_rect {
        if rect.width >= 0.30 {
            score += 25;
        }
        if (0.02..=0.60).contains(&rect.height) {
            score += 10;
        }
        if rect.width >= 0.45 {
            score += 30;
        }
        if rect.y >= 0.55 {
            score += 20;
        } else if rect.y <= 0.25 {
            score -= 15;
        }
    }

    (score >= COMPOSER_SCORE_THRESHOLD).then_some(score)
}

/// 选出最合适的通用输入框候选（无合格候选返回 `None`）。
pub fn best_composer_index(candidates: &[FocusCandidate]) -> Option<usize> {
    let mut best: Option<(usize, i32)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(score) = composer_candidate_score(candidate) else {
            continue;
        };
        if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((index, score));
        }
    }
    best.map(|(index, _)| index)
}

/// 两个语义值是否算「同一特征」（mac 规则：相等，或长度 ≥4 且互相包含）。
fn semantic_values_match(lhs: &str, rhs: &str) -> bool {
    let lhs = lhs.trim().to_lowercase();
    let rhs = rhs.trim().to_lowercase();
    if lhs.is_empty() || rhs.is_empty() {
        return false;
    }
    if lhs == rhs {
        return true;
    }
    lhs.chars().count().min(rhs.chars().count()) >= 4 && (lhs.contains(&rhs) || rhs.contains(&lhs))
}

/// 上下文 token 的 Jaccard 相似度（mac 同款）。
fn context_similarity(lhs: &[String], rhs: &[String]) -> f64 {
    let left: std::collections::BTreeSet<String> =
        lhs.iter().flat_map(|value| tokenize(value)).collect();
    let right: std::collections::BTreeSet<String> =
        rhs.iter().flat_map(|value| tokenize(value)).collect();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count() as f64;
    intersection / left.union(&right).count().max(1) as f64
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_owned)
        .collect()
}

/// 已记录目标的匹配打分；不合格返回 `None`。
pub fn recorded_candidate_score(
    candidate: &FocusCandidate,
    target: &RecordedFocusTarget,
) -> Option<i32> {
    if !candidate.passes_hard_gate() || candidate.control_type != target.control_type {
        return None;
    }

    let mut score = 0;
    let mut strong_matches = 0;
    if semantic_values_match(&candidate.automation_id, &target.automation_id) {
        score += 500;
        strong_matches += 1;
    }
    if semantic_values_match(&candidate.name, &target.name) {
        score += 220;
        strong_matches += 1;
    }
    if semantic_values_match(&candidate.class_name, &target.class_name) {
        score += 140;
        strong_matches += 1;
    }
    if semantic_values_match(&candidate.window_title, &target.window_title) {
        score += 100;
    }

    let similarity = context_similarity(&candidate.context_tokens, &target.context_tokens);
    score += (similarity * 180.0).round() as i32;
    if similarity >= 0.5 {
        strong_matches += 1;
    }

    if let (Some(candidate_rect), Some(target_rect)) =
        (candidate.normalized_rect, target.normalized_rect)
    {
        let distance = (candidate_rect.x - target_rect.x).abs()
            + (candidate_rect.y - target_rect.y).abs()
            + (candidate_rect.width - target_rect.width).abs()
            + (candidate_rect.height - target_rect.height).abs();
        score += (120.0 - distance * 120.0).max(0.0).round() as i32;
    }

    if strong_matches == 0 {
        return None;
    }
    (score >= RECORDED_SCORE_THRESHOLD).then_some(score)
}

/// 在候选中选出已记录目标的最佳匹配；要求达到阈值且与第二名分差足够。
pub fn best_recorded_index(
    candidates: &[FocusCandidate],
    target: &RecordedFocusTarget,
) -> Option<usize> {
    let mut scored: Vec<(usize, i32)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            recorded_candidate_score(candidate, target).map(|score| (index, score))
        })
        .collect();
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|left, right| right.1.cmp(&left.1));
    let (best_index, best_score) = scored[0];
    if let Some((_, runner_up)) = scored.get(1) {
        if best_score - runner_up < RECORDED_SCORE_MARGIN {
            return None;
        }
    }
    Some(best_index)
}

/// 请求代次门控：新请求作废旧请求，迟到回调不得污染新状态。
#[derive(Debug, Default)]
pub struct FocusRequestGate {
    current: AtomicU64,
}

impl FocusRequestGate {
    pub fn new() -> Self {
        Self {
            current: AtomicU64::new(0),
        }
    }

    /// 开启一代新请求，返回其 id。
    pub fn begin(&self) -> u64 {
        self.current.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// 该 id 是否仍是当前代次。
    pub fn is_current(&self, request_id: u64) -> bool {
        self.current.load(Ordering::Acquire) == request_id
    }
}

/// 规范化整份聚焦档案表：丢弃非法条目，并把条数压到上限内。
///
/// 返回被丢弃的键（供调用方记日志，便于用户排查「为什么设置没保存」）。
pub fn normalize_focus_profiles(profiles: &mut BTreeMap<String, AppFocusProfile>) -> Vec<String> {
    let mut dropped = Vec::new();
    let keys: Vec<String> = profiles.keys().cloned().collect();
    for key in keys {
        let normalized = profiles
            .get(&key)
            .cloned()
            .and_then(AppFocusProfile::normalized);
        match normalized {
            Some(profile) => {
                profiles.insert(key, profile);
            }
            None => {
                profiles.remove(&key);
                dropped.push(key);
            }
        }
    }
    while profiles.len() > MAX_FOCUS_PROFILES {
        let Some(last) = profiles.keys().next_back().cloned() else {
            break;
        };
        profiles.remove(&last);
        dropped.push(last);
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(control_type: &str, automation_id: &str) -> FocusCandidate {
        FocusCandidate {
            control_type: control_type.to_owned(),
            automation_id: automation_id.to_owned(),
            enabled: true,
            keyboard_focusable: true,
            ..Default::default()
        }
    }

    fn window_rect() -> NormalizedRect {
        NormalizedRect {
            x: 0.05,
            y: 0.62,
            width: 0.80,
            height: 0.25,
        }
    }

    #[test]
    fn strategy_serializes_as_snake_case() {
        let open = AppFocusProfile::default();
        let json = serde_json::to_string(&open).unwrap();
        assert_eq!(json, "{\"strategy\":\"open_only\"}");

        let mut shortcut = AppFocusProfile {
            strategy: FocusStrategy::AppShortcut,
            ..Default::default()
        };
        shortcut.shortcut = Some(KeyChord {
            keys: vec![crate::send_input::KeyCode::RightAlt],
        });
        let json = serde_json::to_string(&shortcut).unwrap();
        assert!(json.contains("\"strategy\":\"app_shortcut\""));
        assert!(json.contains("shortcut"));
    }

    #[test]
    fn legacy_profile_json_without_new_fields_still_deserializes() {
        let profile: AppFocusProfile = serde_json::from_str("{}").unwrap();
        assert_eq!(profile.strategy, FocusStrategy::OpenOnly);
        assert!(profile.shortcut.is_none());
        assert!(profile.recorded.is_none());
    }

    #[test]
    fn profile_requires_fields_matching_its_strategy() {
        // app_shortcut 没给快捷键 → 整条丢弃
        let invalid = AppFocusProfile {
            strategy: FocusStrategy::AppShortcut,
            ..Default::default()
        };
        assert!(invalid.normalized().is_none());

        // recorded_element 没给记录 → 整条丢弃
        let invalid = AppFocusProfile {
            strategy: FocusStrategy::RecordedElement,
            ..Default::default()
        };
        assert!(invalid.normalized().is_none());

        // open_only 永远可用（忽略附带字段）
        let open = AppFocusProfile {
            strategy: FocusStrategy::OpenOnly,
            shortcut: Some(KeyChord {
                keys: vec![crate::send_input::KeyCode::RightAlt],
            }),
            ..Default::default()
        };
        let normalized = open.normalized().unwrap();
        assert_eq!(normalized.strategy, FocusStrategy::OpenOnly);
        assert!(normalized.shortcut.is_none());
        assert!(normalized.recorded.is_none());
    }

    #[test]
    fn profile_normalization_caps_semantic_fields() {
        let long = "a".repeat(MAX_SEMANTIC_LEN + 40);
        let target = RecordedFocusTarget {
            control_type: "Edit".to_owned(),
            automation_id: long.clone(),
            class_name: long.clone(),
            name: long.clone(),
            window_title: long.clone(),
            normalized_rect: None,
            context_tokens: (0..(MAX_CONTEXT_TOKENS + 10))
                .map(|index| format!("{index}-{}", "t".repeat(MAX_CONTEXT_TOKEN_LEN + 5)))
                .collect(),
        };
        let profile = AppFocusProfile {
            strategy: FocusStrategy::RecordedElement,
            shortcut: None,
            recorded: Some(target),
        }
        .normalized()
        .expect("合法记录必须保留");

        let recorded = profile.recorded.unwrap();
        assert!(recorded.automation_id.chars().count() <= MAX_SEMANTIC_LEN);
        assert!(recorded.class_name.chars().count() <= MAX_SEMANTIC_LEN);
        assert!(recorded.name.chars().count() <= MAX_SEMANTIC_LEN);
        assert!(recorded.window_title.chars().count() <= MAX_SEMANTIC_LEN);
        assert!(recorded.context_tokens.len() <= MAX_CONTEXT_TOKENS);
        assert!(recorded
            .context_tokens
            .iter()
            .all(|token| token.chars().count() <= MAX_CONTEXT_TOKEN_LEN));
    }

    #[test]
    fn recorded_target_without_any_anchor_is_rejected() {
        let empty = RecordedFocusTarget {
            control_type: "Edit".to_owned(),
            ..Default::default()
        };
        let profile = AppFocusProfile {
            strategy: FocusStrategy::RecordedElement,
            shortcut: None,
            recorded: Some(empty),
        };
        assert!(profile.normalized().is_none());
    }

    #[test]
    fn recorded_target_rejects_sensitive_or_excluded_semantics() {
        for semantic in ["Password", "密码输入框", "API Key", "地址栏"] {
            let target = RecordedFocusTarget {
                control_type: "Edit".to_owned(),
                automation_id: semantic.to_owned(),
                ..Default::default()
            };
            let profile = AppFocusProfile {
                strategy: FocusStrategy::RecordedElement,
                shortcut: None,
                recorded: Some(target),
            };
            assert!(
                profile.normalized().is_none(),
                "含「{semantic}」的记录必须被拒绝"
            );
        }
    }

    #[test]
    fn sensitive_terms_cover_chinese_and_english() {
        for text in [
            "Password",
            "PASSWORD",
            "api key",
            "token",
            "密码",
            "密钥",
            "银行卡",
        ] {
            assert!(contains_sensitive_term(text), "{text} 应判为敏感");
        }
        assert!(!contains_sensitive_term("消息输入框"));
        assert!(!contains_sensitive_term("composer"));
    }

    #[test]
    fn excluded_terms_cover_browser_chrome_and_editors() {
        for text in [
            "address bar",
            "OmniboxViewViews",
            "地址栏",
            "地址和搜索栏",
            "command palette",
            "命令面板",
            "code editor",
            "重命名",
        ] {
            assert!(contains_excluded_term(text), "{text} 应被排除");
        }
        // 网页搜索框按 mac 语义是允许的目标，不能被排除词误伤
        assert!(!contains_excluded_term("搜索"));
        assert!(!contains_excluded_term("消息输入"));
    }

    #[test]
    fn candidate_hard_gate_requires_usable_non_secret_field() {
        assert!(candidate("Edit", "chat-input").passes_hard_gate());

        let mut disabled = candidate("Edit", "chat-input");
        disabled.enabled = false;
        assert!(!disabled.passes_hard_gate());

        let mut not_focusable = candidate("Edit", "chat-input");
        not_focusable.keyboard_focusable = false;
        assert!(!not_focusable.passes_hard_gate());

        let mut secret = candidate("Edit", "password-box");
        secret.is_password = true;
        assert!(!secret.passes_hard_gate());

        let mut sensitive_name = candidate("Edit", "chat-input");
        sensitive_name.name = "手机令牌".to_owned();
        assert!(!sensitive_name.passes_hard_gate());

        // 非文本输入类控件（如微信的 XButton）不进入候选
        assert!(!candidate("Button", "ok").passes_hard_gate());
        // 可聚焦但没有文本模式、也没有编辑器语义的 Group 同样不进候选
        assert!(!candidate("Group", "").passes_hard_gate());

        // 只读控件（实测：Chromium 把网页根节点暴露为 Document + IsReadOnly=true）
        // 不能输入，直接拒绝；拿不到只读值时保持宽松。
        let mut read_only = candidate("Document", "RootWebArea");
        read_only.read_only = Some(true);
        assert!(!read_only.passes_hard_gate());

        let mut writable = candidate("Document", "RootWebArea");
        writable.read_only = Some(false);
        assert!(writable.passes_hard_gate());
    }

    #[test]
    fn contenteditable_editor_group_is_accepted_as_candidate() {
        // 真实样本（2026-10-02 DimAgent 实测）：TipTap/ProseMirror contenteditable
        // 在 UIA 上是 ControlType.Group，Chromium 把 DOM class 原样暴露。
        let editor = FocusCandidate {
            control_type: "Group".to_owned(),
            class_name: "tiptap ProseMirror outline-none ProseMirror-focused".to_owned(),
            normalized_rect: Some(NormalizedRect {
                x: 0.30,
                y: 0.75,
                width: 0.55,
                height: 0.10,
            }),
            focused: true,
            ..candidate("Group", "")
        };
        assert!(editor.passes_hard_gate(), "编辑器 Group 必须可入候选");
        assert!(
            composer_candidate_score(&editor).is_some(),
            "编辑器 Group 必须能过打分阈值"
        );

        // 有文本模式但无编辑器语义的可聚焦控件也允许（例如自定义富文本容器）
        let text_container = FocusCandidate {
            has_text_pattern: true,
            ..candidate("Custom", "custom-text-host")
        };
        assert!(text_container.passes_hard_gate());
    }

    #[test]
    fn focused_candidate_wins_over_identical_unfocused_one() {
        let rect = NormalizedRect {
            x: 0.05,
            y: 0.62,
            width: 0.80,
            height: 0.25,
        };
        let unfocused = FocusCandidate {
            normalized_rect: Some(rect),
            ..candidate("Edit", "composer")
        };
        let focused = FocusCandidate {
            focused: true,
            ..unfocused.clone()
        };
        let candidates = vec![unfocused, focused];
        assert_eq!(
            best_composer_index(&candidates),
            Some(1),
            "同分时保留用户当前已在用的输入框"
        );
    }

    #[test]
    fn composer_score_rejects_excluded_and_offers_reasonably_high_for_composer_terms() {
        let plain = FocusCandidate {
            normalized_rect: Some(window_rect()),
            ..candidate("Document", "RootWebArea")
        };
        assert!(composer_candidate_score(&plain).is_some());

        let omnibox = FocusCandidate {
            control_type: "Edit".to_owned(),
            class_name: "OmniboxViewViews".to_owned(),
            name: "地址和搜索栏".to_owned(),
            ..candidate("Edit", "view_1012")
        };
        assert_eq!(composer_candidate_score(&omnibox), None);

        let composer = FocusCandidate {
            name: "消息输入".to_owned(),
            normalized_rect: Some(window_rect()),
            ..candidate("Edit", "chat-input")
        };
        let composer_score = composer_candidate_score(&composer).expect("composer 应合格");
        let plain_document_score =
            composer_candidate_score(&plain).expect("普通 Document 也应合格");
        assert!(composer_score > plain_document_score);
    }

    #[test]
    fn best_composer_index_prefers_lower_composer_over_page_document() {
        let candidates = vec![
            candidate("Document", "RootWebArea"),
            FocusCandidate {
                automation_id: "chat-input".to_owned(),
                name: "消息输入".to_owned(),
                ..candidate("Edit", "chat-input")
            },
        ];
        assert_eq!(best_composer_index(&candidates), Some(1));
        assert_eq!(best_composer_index(&[]), None);
    }

    #[test]
    fn recorded_match_requires_threshold_and_margin() {
        let target = RecordedFocusTarget {
            control_type: "Edit".to_owned(),
            automation_id: "composer".to_owned(),
            class_name: String::new(),
            name: String::new(),
            window_title: String::new(),
            normalized_rect: None,
            context_tokens: Vec::new(),
        };
        let strong = FocusCandidate {
            automation_id: "composer".to_owned(),
            ..candidate("Edit", "composer")
        };
        let weaker = FocusCandidate {
            name: "composer".to_owned(),
            ..candidate("Edit", "other")
        };
        assert!(recorded_candidate_score(&strong, &target).is_some());
        assert_eq!(recorded_candidate_score(&weaker, &target), None);

        // 强匹配唯一 → 选中
        let only = vec![strong.clone()];
        assert_eq!(best_recorded_index(&only, &target), Some(0));

        // 两个强匹配分差不足 → 放弃（避免点错相近控件）
        let ambiguous = vec![strong.clone(), strong.clone()];
        assert_eq!(best_recorded_index(&ambiguous, &target), None);
    }

    #[test]
    fn normalized_rect_requires_finite_positive_bounds() {
        assert!(NormalizedRect::from_bounds(None, Some((0.0, 0.0, 100.0, 100.0))).is_none());
        assert!(NormalizedRect::from_bounds(
            Some((0.0, 0.0, 10.0, 10.0)),
            Some((0.0, 0.0, 0.0, 100.0))
        )
        .is_none());
        assert!(NormalizedRect::from_bounds(
            Some((f64::NEG_INFINITY, 0.0, 10.0, 10.0)),
            Some((0.0, 0.0, 100.0, 100.0))
        )
        .is_none());
        assert!(NormalizedRect::from_bounds(
            Some((0.0, 0.0, -10.0, 10.0)),
            Some((0.0, 0.0, 100.0, 100.0))
        )
        .is_none());

        let rect = NormalizedRect::from_bounds(
            Some((20.0, 30.0, 40.0, 25.0)),
            Some((10.0, 10.0, 200.0, 100.0)),
        )
        .expect("正常矩形应可换算");
        assert!((rect.x - 0.05).abs() < 1e-9);
        assert!((rect.y - 0.2).abs() < 1e-9);
        assert!((rect.width - 0.2).abs() < 1e-9);
        assert!((rect.height - 0.25).abs() < 1e-9);
    }

    #[test]
    fn request_gate_supersedes_older_requests() {
        let gate = FocusRequestGate::new();
        let first = gate.begin();
        assert!(gate.is_current(first));

        let second = gate.begin();
        assert!(gate.is_current(second));
        assert!(!gate.is_current(first), "新请求必须作废旧请求");
        assert_ne!(first, second);
    }

    #[test]
    fn focus_profile_map_drops_invalid_entries_and_caps_size() {
        let mut profiles: BTreeMap<String, AppFocusProfile> = BTreeMap::new();
        profiles.insert(
            "wechat".to_owned(),
            AppFocusProfile {
                strategy: FocusStrategy::AppShortcut,
                ..Default::default()
            },
        );
        profiles.insert("notepad".to_owned(), AppFocusProfile::default());

        let dropped = normalize_focus_profiles(&mut profiles);
        assert_eq!(dropped, vec!["wechat".to_owned()]);
        assert_eq!(profiles.len(), 1);
        assert!(profiles.contains_key("notepad"));

        let mut many: BTreeMap<String, AppFocusProfile> = BTreeMap::new();
        for index in 0..(MAX_FOCUS_PROFILES + 5) {
            many.insert(format!("target-{index:04}"), AppFocusProfile::default());
        }
        let dropped = normalize_focus_profiles(&mut many);
        assert_eq!(many.len(), MAX_FOCUS_PROFILES);
        assert_eq!(dropped.len(), 5);
        // 被丢弃的是字典序末尾的键，保留前缀确定
        assert!(many.contains_key("target-0000"));
        assert!(!many.contains_key(&format!("target-{:04}", MAX_FOCUS_PROFILES + 4)));
    }
}
