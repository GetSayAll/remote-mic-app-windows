//! 滚动区域学习与匹配的纯逻辑（跨平台可编译、可单测）。
//!
//! Windows 平台层（`scroll_region_windows`）只负责把 UIA 元素转换成
//! [`ScrollRegionDescriptor`]；本模块不依赖 Windows API，负责：
//!
//! - 记录目标只保留结构特征（控件类型、AutomationId、ClassName、归一化矩形），
//!   不保存控件名称、窗口标题或任何内容文本；
//! - 运行时从当前窗口的候选里选回最可信的滚动容器；
//! - 对档案做长度与数量校验，防止配置文件被异常值撑爆。
//!
//! 设计参考 macOS `ScrollRegionTarget.swift` 的四项匹配思路，但 Windows UIA
//! 不能直接复用 `RecordedFocusTarget`：滚动容器的硬门槛是 `ScrollPattern`，
//! 与输入框的 `ValuePattern` / 可聚焦判定不同。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::focus::NormalizedRect;

/// 每个 APP 最多保存的滚动区域档案数（与聚焦档案同量级）。
pub const MAX_SCROLL_REGIONS: usize = 200;
/// 单个语义字段的长度上限。
pub const MAX_SCROLL_SEMANTIC_LEN: usize = 256;
/// 学习窗口（与输入框学习一致，用户有 3 秒点击目标内容区）。
pub const SCROLL_LEARN_WINDOW_MS: u64 = 3000;
/// 学习轮询间隔。
pub const SCROLL_LEARN_POLL_MS: u64 = 40;
/// 太小或只有 2×2 的屏幕阅读器节点不作为滚动容器。
pub const MIN_SCROLL_REGION_SIDE: f64 = 32.0;

/// 用户执行翻页时请求的滚动方向/端点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollCommand {
    PageUp,
    PageDown,
    ToTop,
    ToBottom,
}

impl ScrollCommand {
    /// 结构化日志使用的稳定标识。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::ToTop => "to_top",
            Self::ToBottom => "to_bottom",
        }
    }

    pub fn allows_repeat(self) -> bool {
        matches!(self, Self::PageUp | Self::PageDown)
    }
}

/// 滚动区域后端失败原因（稳定 token；UI 负责本地化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollFailure {
    TargetExited,
    OwnProcess,
    NotAccessible,
    NoCandidate,
    PatternUnavailable,
    NotMoved,
    Backend,
}

impl ScrollFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TargetExited => "target_exited",
            Self::OwnProcess => "own_process",
            Self::NotAccessible => "not_accessible",
            Self::NoCandidate => "no_candidate",
            Self::PatternUnavailable => "pattern_unavailable",
            Self::NotMoved => "not_moved",
            Self::Backend => "backend",
        }
    }
}

impl std::fmt::Display for ScrollFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for ScrollFailure {}

/// 滚动执行路径（用于 UI 区分「真的滚了」与「只发出了滚轮请求」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollMethod {
    ScrollPattern,
    Wheel,
}

impl ScrollMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ScrollPattern => "scroll_pattern",
            Self::Wheel => "wheel",
        }
    }
}

/// 一次滚动请求的可观察结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollOutcome {
    pub method: ScrollMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_percent: Option<f64>,
    /// UIA 百分比是否真的变化；滚轮回退无法读回时保持 false。
    pub moved: bool,
    /// 已经在顶部/底部（UIA 读回判定）。
    pub at_edge: bool,
}

/// 记录下来的滚动容器结构。**不保存文本内容**。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollRegionTarget {
    pub control_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub automation_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rect: Option<NormalizedRect>,
}

impl ScrollRegionTarget {
    /// 规范化：裁掉过长字段；至少保留 automationId / className / 矩形之一。
    /// 只有正确类型的滚动容器才会进入档案，因此控件类型必须非空。
    pub fn normalized(mut self) -> Option<Self> {
        self.control_type = clamp_chars(&self.control_type, MAX_SCROLL_SEMANTIC_LEN);
        self.automation_id = clamp_chars(&self.automation_id, MAX_SCROLL_SEMANTIC_LEN);
        self.class_name = clamp_chars(&self.class_name, MAX_SCROLL_SEMANTIC_LEN);
        if self.control_type.is_empty() {
            return None;
        }
        if self.automation_id.is_empty()
            && self.class_name.is_empty()
            && self.normalized_rect.is_none()
        {
            return None;
        }
        if let Some(rect) = self.normalized_rect {
            let values = [rect.x, rect.y, rect.width, rect.height];
            if values.iter().any(|value| !value.is_finite())
                || rect.width <= 0.0
                || rect.height <= 0.0
                || rect.x < 0.0
                || rect.y < 0.0
                || rect.x + rect.width > 1.0
                || rect.y + rect.height > 1.0
            {
                self.normalized_rect = None;
            }
        }
        Some(self)
    }

    /// 是否有可用于匹配的结构身份（只有矩形的档案置信度较低）。
    pub fn has_identity(&self) -> bool {
        !self.automation_id.is_empty() || !self.class_name.is_empty()
    }
}

/// 平台层采集到的滚动容器候选（只含结构与几何，不含内容）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollRegionDescriptor {
    pub control_type: String,
    pub automation_id: String,
    pub class_name: String,
    pub normalized_rect: Option<NormalizedRect>,
    pub vertically_scrollable: bool,
    pub horizontally_scrollable: bool,
}

impl ScrollRegionDescriptor {
    pub fn from_target(target: &ScrollRegionTarget) -> Self {
        Self {
            control_type: target.control_type.clone(),
            automation_id: target.automation_id.clone(),
            class_name: target.class_name.clone(),
            normalized_rect: target.normalized_rect,
            vertically_scrollable: true,
            horizontally_scrollable: false,
        }
    }

    /// 学习门槛：可见、尺寸合理、至少一个方向真的能滚动。
    pub fn passes_learning_gate(&self) -> bool {
        if !self.vertically_scrollable && !self.horizontally_scrollable {
            return false;
        }
        if self.control_type.is_empty() {
            return false;
        }
        self.normalized_rect
            .is_some_and(|rect| rect.width > 0.0 && rect.height > 0.0)
    }

    pub fn to_target(&self) -> Option<ScrollRegionTarget> {
        ScrollRegionTarget {
            control_type: self.control_type.clone(),
            automation_id: self.automation_id.clone(),
            class_name: self.class_name.clone(),
            normalized_rect: self.normalized_rect,
        }
        .normalized()
    }
}

/// 单个候选与记录目标的匹配分。
///
/// 控件类型必须一致；AutomationId 与 ClassName 是强身份，归一化矩形只作几何
/// 校正。没有身份字段的旧档案只允许在高重叠度时命中。
pub fn scroll_region_score(target: &ScrollRegionTarget, candidate: &ScrollRegionDescriptor) -> i32 {
    if !candidate.vertically_scrollable && !candidate.horizontally_scrollable {
        return 0;
    }
    if target.control_type != candidate.control_type {
        return 0;
    }
    let mut score = 40;
    let mut identity = 0;
    if !target.automation_id.is_empty() && target.automation_id == candidate.automation_id {
        score += 120;
        identity += 1;
    }
    if !target.class_name.is_empty() && target.class_name == candidate.class_name {
        score += 60;
        identity += 1;
    }
    let overlap = match (target.normalized_rect, candidate.normalized_rect) {
        (Some(left), Some(right)) => normalized_overlap(left, right),
        _ => 0.0,
    };
    score += (overlap * 60.0).round() as i32;
    if identity == 0 {
        let target_coverage = match (target.normalized_rect, candidate.normalized_rect) {
            (Some(left), Some(right)) => normalized_target_coverage(left, right),
            _ => 0.0,
        };
        if overlap < 0.85 || target_coverage < 0.85 {
            return 0;
        }
    }
    score
}

/// 在候选里选最可信的一条；分数不足或与第二名差距不足时返回 `None`。
pub fn best_scroll_region_index(
    candidates: &[ScrollRegionDescriptor],
    target: &ScrollRegionTarget,
) -> Option<usize> {
    let mut scored: Vec<(usize, i32)> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (index, scroll_region_score(target, candidate)))
        .filter(|(_, score)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let (best_index, best_score) = *scored.first()?;
    let threshold = if target.has_identity() { 120 } else { 88 };
    if best_score < threshold {
        return None;
    }
    if let Some((_, second)) = scored.get(1) {
        if best_score - second < 20 {
            return None;
        }
    }
    Some(best_index)
}

/// 没有记录目标时的兜底：选面积最大、方向可滚动的可见容器。
///
/// 只用于「用户明确点了翻页键，但这个 APP 还没学习」的通用路径；有记录时
/// 必须优先用 [`best_scroll_region_index`]，避免把内容滚到错误的嵌套区域。
pub fn largest_vertical_candidate(candidates: &[ScrollRegionDescriptor]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.vertically_scrollable)
        .filter_map(|(index, candidate)| {
            let rect = candidate.normalized_rect?;
            candidate
                .passes_learning_gate()
                .then_some((index, rect.width * rect.height))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

fn intersection_area(left: NormalizedRect, right: NormalizedRect) -> f64 {
    let x1 = left.x.max(right.x);
    let y1 = left.y.max(right.y);
    let x2 = (left.x + left.width).min(right.x + right.width);
    let y2 = (left.y + left.height).min(right.y + right.height);
    (x2 - x1).max(0.0) * (y2 - y1).max(0.0)
}

fn normalized_overlap(left: NormalizedRect, right: NormalizedRect) -> f64 {
    let smaller = (left.width * left.height).min(right.width * right.height);
    if smaller <= 0.0 {
        0.0
    } else {
        (intersection_area(left, right) / smaller).clamp(0.0, 1.0)
    }
}

fn normalized_target_coverage(target: NormalizedRect, candidate: NormalizedRect) -> f64 {
    let area = target.width * target.height;
    if area <= 0.0 {
        0.0
    } else {
        (intersection_area(target, candidate) / area).clamp(0.0, 1.0)
    }
}

/// 规范化档案表：丢弃无效目标，最多保留前 `MAX_SCROLL_REGIONS` 条。
/// 返回被丢弃的条数（不返回路径，日志不涉及用户本机路径）。
pub fn normalize_scroll_regions(regions: &mut BTreeMap<String, ScrollRegionTarget>) -> usize {
    let before = regions.len();
    let mut normalized = BTreeMap::new();
    for (key, target) in std::mem::take(regions) {
        if key.trim().is_empty() || key.chars().count() > 2048 {
            continue;
        }
        if let Some(target) = target.normalized() {
            normalized.insert(key, target);
        }
    }
    if normalized.len() > MAX_SCROLL_REGIONS {
        let keep: Vec<String> = normalized
            .keys()
            .take(MAX_SCROLL_REGIONS)
            .cloned()
            .collect();
        normalized.retain(|key, _| keep.iter().any(|kept| kept == key));
    }
    *regions = normalized;
    before.saturating_sub(regions.len())
}

fn clamp_chars(value: &str, limit: usize) -> String {
    value.trim().chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NormalizedRect {
        NormalizedRect {
            x,
            y,
            width,
            height,
        }
    }

    fn target() -> ScrollRegionTarget {
        ScrollRegionTarget {
            control_type: "Document".to_owned(),
            automation_id: "RootWebArea".to_owned(),
            class_name: String::new(),
            normalized_rect: Some(rect(0.0, 0.1, 1.0, 0.8)),
        }
        .normalized()
        .unwrap()
    }

    fn candidate() -> ScrollRegionDescriptor {
        ScrollRegionDescriptor {
            control_type: "Document".to_owned(),
            automation_id: "RootWebArea".to_owned(),
            class_name: String::new(),
            normalized_rect: Some(rect(0.0, 0.1, 1.0, 0.8)),
            vertically_scrollable: true,
            horizontally_scrollable: false,
        }
    }

    #[test]
    fn target_requires_structure_and_clamps_fields() {
        let long = "x".repeat(MAX_SCROLL_SEMANTIC_LEN + 10);
        let normalized = ScrollRegionTarget {
            control_type: long.clone(),
            automation_id: long.clone(),
            class_name: String::new(),
            normalized_rect: Some(rect(0.0, 0.0, 1.0, 1.0)),
        }
        .normalized()
        .unwrap();
        assert_eq!(normalized.control_type.len(), MAX_SCROLL_SEMANTIC_LEN);
        assert_eq!(normalized.automation_id.len(), MAX_SCROLL_SEMANTIC_LEN);
        assert!(ScrollRegionTarget::default().normalized().is_none());
    }

    #[test]
    fn out_of_bounds_rect_is_dropped_but_identity_keeps_target() {
        let target = ScrollRegionTarget {
            control_type: "Pane".to_owned(),
            automation_id: "ScrollViewer".to_owned(),
            class_name: "ScrollViewer".to_owned(),
            normalized_rect: Some(rect(0.9, 0.9, 0.5, 0.5)),
        }
        .normalized()
        .unwrap();
        assert!(target.normalized_rect.is_none());
        assert!(target.has_identity());
    }

    #[test]
    fn exact_identity_wins_with_geometry() {
        let target = target();
        let index = best_scroll_region_index(&[candidate()], &target);
        assert_eq!(index, Some(0));
        assert!(scroll_region_score(&target, &candidate()) >= 120);
    }

    #[test]
    fn wrong_control_type_is_rejected() {
        let mut other = candidate();
        other.control_type = "Pane".to_owned();
        assert_eq!(best_scroll_region_index(&[other], &target()), None);
    }

    #[test]
    fn ambiguous_candidates_are_rejected() {
        let target = target();
        let mut first = candidate();
        first.normalized_rect = Some(rect(0.0, 0.0, 1.0, 1.0));
        let mut second = candidate();
        second.normalized_rect = Some(rect(0.0, 0.0, 1.0, 1.0));
        assert_eq!(best_scroll_region_index(&[first, second], &target), None);
    }

    #[test]
    fn rect_only_target_requires_strong_overlap() {
        let target = ScrollRegionTarget {
            control_type: "Pane".to_owned(),
            automation_id: String::new(),
            class_name: String::new(),
            normalized_rect: Some(rect(0.1, 0.1, 0.8, 0.8)),
        }
        .normalized()
        .unwrap();
        let mut candidate = ScrollRegionDescriptor {
            control_type: "Pane".to_owned(),
            normalized_rect: Some(rect(0.1, 0.1, 0.8, 0.8)),
            vertically_scrollable: true,
            ..Default::default()
        };
        assert_eq!(
            best_scroll_region_index(&[candidate.clone()], &target),
            Some(0)
        );
        candidate.normalized_rect = Some(rect(0.1, 0.1, 0.4, 0.4));
        assert_eq!(best_scroll_region_index(&[candidate], &target), None);
    }

    #[test]
    fn non_scrollable_candidates_are_rejected() {
        let mut candidate = candidate();
        candidate.vertically_scrollable = false;
        candidate.horizontally_scrollable = false;
        assert!(!candidate.passes_learning_gate());
        assert_eq!(best_scroll_region_index(&[candidate], &target()), None);
    }

    #[test]
    fn largest_vertical_candidate_skips_tiny_nodes() {
        let mut tiny = candidate();
        tiny.normalized_rect = Some(rect(0.0, 0.0, 0.002, 0.002));
        let big = ScrollRegionDescriptor {
            control_type: "Document".to_owned(),
            normalized_rect: Some(rect(0.0, 0.0, 1.0, 1.0)),
            vertically_scrollable: true,
            ..Default::default()
        };
        assert_eq!(largest_vertical_candidate(&[tiny, big]), Some(1));
    }

    #[test]
    fn scroll_commands_mark_only_page_steps_as_repeatable() {
        assert!(ScrollCommand::PageUp.allows_repeat());
        assert!(ScrollCommand::PageDown.allows_repeat());
        assert!(!ScrollCommand::ToTop.allows_repeat());
        assert!(!ScrollCommand::ToBottom.allows_repeat());
    }
}
