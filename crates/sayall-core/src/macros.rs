//! 组合动作（宏）数据模型与校验。
//!
//! 对齐 mac `SayAllMacroCore`（`sayall-private-platform/packages/macos-combination-actions`）
//! 的 Schema/校验语义：严格字段白名单 + 版本门控 + 保存期循环检测。纯逻辑，
//! 不依赖 Windows API / Tauri / WebView；平台执行器见 `sayall-windows`。
//!
//! 与 Mac 模型的有意差异（2026-10-15 方案定稿）：
//! - 快捷键内联存储（不建共享快捷键库，规避共享 Profile 重录串改问题）；
//! - `focus_input` 只带应用目标，聚焦档案复用按键映射的 `focus_profiles`；
//! - 不做宏库快照历史（沿用 `.bak` 备份护栏）。

use serde::{Deserialize, Serialize};

/// 组合动作文档结构版本（写入 `schemaVersion` 字段）。
pub const MACRO_SCHEMA_VERSION: &str = "0.1";
/// 宏库文件 `format_version`。
pub const MACRO_LIBRARY_FORMAT_VERSION: u32 = 1;
/// 名称长度上限（UI 同步硬截，对齐 Mac 20 字符策略）。
pub const MAX_MACRO_NAME_CHARS: usize = 20;
pub const MAX_MACRO_SUMMARY_CHARS: usize = 300;
pub const MIN_MACRO_STEPS: usize = 1;
pub const MAX_MACRO_STEPS: usize = 20;
pub const MAX_MACRO_DEFINITIONS: usize = 200;
pub const MACRO_ID_MIN_LEN: usize = 8;
pub const MACRO_ID_MAX_LEN: usize = 128;
pub const MAX_STEP_ID_LEN: usize = 80;
pub const MIN_STEP_TIMEOUT_MS: u64 = 100;
/// 普通步骤显式超时上限（嵌套类见 `MAX_NESTED_STEP_TIMEOUT_MS`）。
pub const MAX_STEP_TIMEOUT_MS: u64 = 10_000;
/// `run_macro` 步骤显式超时上限（对齐 Mac：嵌套/快捷指令/脚本类 60 秒）。
pub const MAX_NESTED_STEP_TIMEOUT_MS: u64 = 60_000;
/// 整宏默认超时（运行期参数，默认 60 秒）。
pub const DEFAULT_MACRO_OVERALL_TIMEOUT_MS: u64 = 60_000;
pub const MIN_MACRO_OVERALL_TIMEOUT_MS: u64 = 100;
pub const MAX_MACRO_OVERALL_TIMEOUT_MS: u64 = 300_000;
/// 嵌套执行深度上限（对齐 Mac）。
pub const MAX_NESTED_DEPTH: usize = 8;
pub const MAX_URL_CHARS: usize = 2048;
/// 宏库文件体积上限（与按键映射导入上限同量级）。
pub const MAX_LIBRARY_BYTES: usize = 1024 * 1024;
/// 主键长度上限（可显示字符）。
pub const MAX_CHORD_KEY_CHARS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroAction {
    /// 打开/激活应用并等待就绪（默认 8 秒）。
    OpenApplication,
    /// 等待应用达到某状态（默认 5 秒）。
    WaitForApplication,
    /// 注入单键或组合键（默认 2 秒）。
    SendShortcut,
    /// 用学习记录聚焦目标应用的输入框（默认 2 秒）。
    FocusInput,
    /// 系统默认浏览器打开 http/https 网址（默认 5 秒）。
    OpenUrl,
    /// 执行已有组合动作（嵌套，默认 60 秒）。
    RunMacro,
}

impl MacroAction {
    pub fn default_timeout_ms(self) -> u64 {
        match self {
            Self::OpenApplication => 8_000,
            Self::WaitForApplication => 5_000,
            Self::SendShortcut | Self::FocusInput => 2_000,
            Self::OpenUrl => 5_000,
            Self::RunMacro => 60_000,
        }
    }

    /// 显式超时的上限（`run_macro` 与其它步骤不同档，对齐 Mac）。
    pub fn max_timeout_ms(self) -> u64 {
        match self {
            Self::RunMacro => MAX_NESTED_STEP_TIMEOUT_MS,
            _ => MAX_STEP_TIMEOUT_MS,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenApplication => "open_application",
            Self::WaitForApplication => "wait_for_application",
            Self::SendShortcut => "send_shortcut",
            Self::FocusInput => "focus_input",
            Self::OpenUrl => "open_url",
            Self::RunMacro => "run_macro",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationWaitState {
    /// 进程已运行。
    Running,
    /// 出现可见窗口。
    VisibleWindow,
}

/// 组合键修饰键。显示顺序固定 Control → Alt → Shift → Win（对齐 Mac
/// `PRODUCT_SPEC.md` 的 Control → Option → Shift → Command 顺序映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroModifier {
    Control,
    Alt,
    Shift,
    Win,
}

impl MacroModifier {
    /// 规范显示顺序。
    pub const DISPLAY_ORDER: [Self; 4] = [Self::Control, Self::Alt, Self::Shift, Self::Win];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Control => "Control",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Win => "Windows",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Control => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Win => "Win",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroChord {
    pub modifiers: Vec<MacroModifier>,
    /// 主键：语义键名（如 `n`、`return`、`escape`、`space`、方向键名），
    /// 由平台执行器映射到虚拟键码。
    pub key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroParameters {
    /// 应用目标（预设 id / 自定义路径 / UWP id；语义同 `ButtonAction::OpenApp`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `wait_for_application` 的等待条件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ApplicationWaitState>,
    /// `send_shortcut` 内联组合键。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chord: Option<MacroChord>,
    /// `open_url` 完整网址（仅 http/https）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// `run_macro` 被引用动作的稳定 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_macro_id: Option<String>,
    /// `run_macro` 被引用动作的名称（展示用，运行期按 ID 解析最新版本）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_macro_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroStep {
    pub step_id: String,
    pub action: MacroAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub parameters: MacroParameters,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroDefinition {
    pub schema_version: String,
    pub macro_id: String,
    /// 语义版本 `x.y.z`（每位非负整数）。
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub steps: Vec<MacroStep>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroLibraryDocument {
    pub format_version: u32,
    #[serde(default)]
    pub definitions: Vec<MacroDefinition>,
}

/// 校验问题：机器可读 code（前端映射到用户文案），不带自由文本细节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroIssue {
    UnsupportedSchemaVersion,
    InvalidMacroId,
    InvalidVersion,
    NameEmpty,
    NameTooLong,
    SummaryTooLong,
    TooFewSteps,
    TooManySteps,
    TooManyDefinitions,
    InvalidStepId,
    DuplicateStepId,
    StepTimeoutOutOfRange,
    MissingParameter(&'static str),
    UnexpectedParameter(&'static str),
    InvalidUrl,
    InvalidChord,
    InvalidWaitState,
    NestedSelf,
    NestedCycle,
    DuplicateDefinition,
    ImmutableVersionConflict,
}

impl MacroIssue {
    /// 稳定 token，IPC 传输与前端文案映射用。
    pub fn code(&self) -> String {
        match self {
            Self::UnsupportedSchemaVersion => "unsupported_schema_version".to_owned(),
            Self::InvalidMacroId => "invalid_macro_id".to_owned(),
            Self::InvalidVersion => "invalid_version".to_owned(),
            Self::NameEmpty => "name_empty".to_owned(),
            Self::NameTooLong => "name_too_long".to_owned(),
            Self::SummaryTooLong => "summary_too_long".to_owned(),
            Self::TooFewSteps => "too_few_steps".to_owned(),
            Self::TooManySteps => "too_many_steps".to_owned(),
            Self::TooManyDefinitions => "too_many_definitions".to_owned(),
            Self::InvalidStepId => "invalid_step_id".to_owned(),
            Self::DuplicateStepId => "duplicate_step_id".to_owned(),
            Self::StepTimeoutOutOfRange => "step_timeout_out_of_range".to_owned(),
            Self::MissingParameter(field) => format!("missing_parameter.{field}"),
            Self::UnexpectedParameter(field) => format!("unexpected_parameter.{field}"),
            Self::InvalidUrl => "invalid_url".to_owned(),
            Self::InvalidChord => "invalid_chord".to_owned(),
            Self::InvalidWaitState => "invalid_wait_state".to_owned(),
            Self::NestedSelf => "nested_self".to_owned(),
            Self::NestedCycle => "nested_cycle".to_owned(),
            Self::DuplicateDefinition => "duplicate_definition".to_owned(),
            Self::ImmutableVersionConflict => "immutable_version_conflict".to_owned(),
        }
    }
}

fn is_valid_macro_id(id: &str) -> bool {
    let len = id.len();
    if !(MACRO_ID_MIN_LEN..=MACRO_ID_MAX_LEN).contains(&len) {
        return false;
    }
    let mut chars = id.chars();
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_valid_step_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_STEP_ID_LEN {
        return false;
    }
    let mut chars = id.chars();
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// 语义版本校验：`x.y.z`，每位为无前导零的非负整数（`0` 本身允许）。
fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let parse_part = |part: &str| -> Option<u64> {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        part.parse().ok()
    };
    let major = parse_part(parts.next()?)?;
    let minor = parse_part(parts.next()?)?;
    let patch = parse_part(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// http/https 网址校验（不引入 url 依赖；对齐 Mac 规则）：scheme 白名单、
/// host 非空、禁 userinfo、无控制字符、长度上限。
pub fn validate_http_url(url: &str) -> Result<(), MacroIssue> {
    if url.len() > MAX_URL_CHARS || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(MacroIssue::InvalidUrl);
    }
    let rest = {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("https://") {
            &url["https://".len()..]
        } else if lower.starts_with("http://") {
            &url["http://".len()..]
        } else {
            return Err(MacroIssue::InvalidUrl);
        }
    };
    // 权限部分（authority）= `//` 后到首个 `/` `?` `#`。
    let authority_end = rest
        .find(|c| matches!(c, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(MacroIssue::InvalidUrl);
    }
    // userinfo：`@` 出现在 authority 内即拒绝（账号密码形态）。
    if authority.contains('@') {
        return Err(MacroIssue::InvalidUrl);
    }
    // host 至少一个非分隔符字符（不做完整 DNS 校验）。
    let host = authority.rsplit(':').next().unwrap_or(authority);
    if host.is_empty() {
        return Err(MacroIssue::InvalidUrl);
    }
    Ok(())
}

fn validate_chord(chord: &MacroChord) -> Result<(), MacroIssue> {
    if chord.key.is_empty()
        || chord.key.len() > MAX_CHORD_KEY_CHARS
        || chord.key.chars().any(|c| c.is_control())
    {
        return Err(MacroIssue::InvalidChord);
    }
    let mut seen = Vec::new();
    for modifier in &chord.modifiers {
        if seen.contains(modifier) {
            return Err(MacroIssue::InvalidChord);
        }
        seen.push(*modifier);
    }
    Ok(())
}

/// 该动作允许出现的参数字段（白名单；其余参数一律 `UnexpectedParameter`）。
fn allowed_parameters(action: MacroAction) -> &'static [&'static str] {
    match action {
        MacroAction::OpenApplication => &["target"],
        MacroAction::WaitForApplication => &["target", "state"],
        MacroAction::SendShortcut => &["chord"],
        MacroAction::FocusInput => &["target"],
        MacroAction::OpenUrl => &["url"],
        MacroAction::RunMacro => &["nested_macro_id", "nested_macro_name"],
    }
}

fn validate_step(step: &MacroStep) -> Vec<MacroIssue> {
    let mut issues = Vec::new();
    if !is_valid_step_id(&step.step_id) {
        issues.push(MacroIssue::InvalidStepId);
    }
    if let Some(timeout) = step.timeout_ms {
        if !(MIN_STEP_TIMEOUT_MS..=step.action.max_timeout_ms()).contains(&timeout) {
            issues.push(MacroIssue::StepTimeoutOutOfRange);
        }
    }
    let allowed = allowed_parameters(step.action);
    let p = &step.parameters;
    let unexpected = [
        ("target", p.target.is_some()),
        ("state", p.state.is_some()),
        ("chord", p.chord.is_some()),
        ("url", p.url.is_some()),
        ("nested_macro_id", p.nested_macro_id.is_some()),
        ("nested_macro_name", p.nested_macro_name.is_some()),
    ]
    .into_iter()
    .filter(|(field, present)| *present && !allowed.contains(field))
    .map(|(field, _)| MacroIssue::UnexpectedParameter(field));
    issues.extend(unexpected);
    match step.action {
        MacroAction::OpenApplication | MacroAction::FocusInput => {
            if p.target.as_deref().unwrap_or_default().trim().is_empty() {
                issues.push(MacroIssue::MissingParameter("target"));
            }
        }
        MacroAction::WaitForApplication => {
            if p.target.as_deref().unwrap_or_default().trim().is_empty() {
                issues.push(MacroIssue::MissingParameter("target"));
            }
            if p.state.is_none() {
                issues.push(MacroIssue::MissingParameter("state"));
            }
        }
        MacroAction::SendShortcut => match &p.chord {
            None => issues.push(MacroIssue::MissingParameter("chord")),
            Some(chord) => {
                if let Err(issue) = validate_chord(chord) {
                    issues.push(issue);
                }
            }
        },
        MacroAction::OpenUrl => match &p.url {
            None => issues.push(MacroIssue::MissingParameter("url")),
            Some(url) => {
                if validate_http_url(url).is_err() {
                    issues.push(MacroIssue::InvalidUrl);
                }
            }
        },
        MacroAction::RunMacro => {
            match &p.nested_macro_id {
                None => issues.push(MacroIssue::MissingParameter("nested_macro_id")),
                Some(id) => {
                    if !is_valid_macro_id(id) {
                        issues.push(MacroIssue::InvalidMacroId);
                    }
                }
            }
            if p.nested_macro_name
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                issues.push(MacroIssue::MissingParameter("nested_macro_name"));
            }
        }
    }
    issues
}

/// 单条定义校验（不含跨定义循环；`run_macro` 自引用在此拦截）。
pub fn validate_definition(definition: &MacroDefinition) -> Result<(), Vec<MacroIssue>> {
    let mut issues = Vec::new();
    if definition.schema_version != MACRO_SCHEMA_VERSION {
        issues.push(MacroIssue::UnsupportedSchemaVersion);
    }
    if !is_valid_macro_id(&definition.macro_id) {
        issues.push(MacroIssue::InvalidMacroId);
    }
    if parse_semver(&definition.version).is_none() {
        issues.push(MacroIssue::InvalidVersion);
    }
    let name = definition.name.trim();
    if name.is_empty() {
        issues.push(MacroIssue::NameEmpty);
    } else if name.chars().count() > MAX_MACRO_NAME_CHARS {
        issues.push(MacroIssue::NameTooLong);
    }
    if definition
        .summary
        .as_deref()
        .map(|s| s.chars().count() > MAX_MACRO_SUMMARY_CHARS)
        .unwrap_or(false)
    {
        issues.push(MacroIssue::SummaryTooLong);
    }
    if definition.steps.len() < MIN_MACRO_STEPS {
        issues.push(MacroIssue::TooFewSteps);
    } else if definition.steps.len() > MAX_MACRO_STEPS {
        issues.push(MacroIssue::TooManySteps);
    }
    let mut seen_step_ids = std::collections::BTreeSet::new();
    for step in &definition.steps {
        if !seen_step_ids.insert(step.step_id.clone()) {
            issues.push(MacroIssue::DuplicateStepId);
        }
        if step.action == MacroAction::RunMacro
            && step.parameters.nested_macro_id.as_deref() == Some(definition.macro_id.as_str())
        {
            issues.push(MacroIssue::NestedSelf);
        }
        issues.extend(validate_step(step));
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

/// 宏库校验：单条规则 + 条数上限 + 同 `macro_id@version` 冲突（不可变版本）。
pub fn validate_library(document: &MacroLibraryDocument) -> Result<(), Vec<MacroIssue>> {
    let mut issues = Vec::new();
    if document.definitions.len() > MAX_MACRO_DEFINITIONS {
        issues.push(MacroIssue::TooManyDefinitions);
    }
    let mut seen = std::collections::BTreeSet::new();
    for definition in &document.definitions {
        let key = format!("{}@{}", definition.macro_id, definition.version);
        if !seen.insert(key) {
            issues.push(MacroIssue::DuplicateDefinition);
        }
        if let Err(mut def_issues) = validate_definition(definition) {
            issues.append(&mut def_issues);
        }
    }
    if find_cycle(&document.definitions).is_some() {
        issues.push(MacroIssue::NestedCycle);
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

/// 跨定义循环检测（保存期防线，运行期另有 ancestry 兜底）：按 `macro_id`
/// 解析步骤引用做 DFS，找到环返回环上的 ID 路径（含收尾重复）。
pub fn find_cycle(definitions: &[MacroDefinition]) -> Option<Vec<String>> {
    fn visit(
        id: &str,
        definitions: &[MacroDefinition],
        visiting: &mut Vec<String>,
        completed: &mut std::collections::BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if completed.contains(id) {
            return None;
        }
        if let Some(pos) = visiting.iter().position(|seen| seen == id) {
            let mut cycle = visiting[pos..].to_vec();
            cycle.push(id.to_owned());
            return Some(cycle);
        }
        visiting.push(id.to_owned());
        for definition in definitions {
            if definition.macro_id != id {
                continue;
            }
            for step in &definition.steps {
                if step.action != MacroAction::RunMacro {
                    continue;
                }
                let Some(nested) = step.parameters.nested_macro_id.as_deref() else {
                    continue;
                };
                if definitions.iter().any(|d| d.macro_id == nested) {
                    if let Some(cycle) = visit(nested, definitions, visiting, completed) {
                        return Some(cycle);
                    }
                }
            }
        }
        visiting.pop();
        completed.insert(id.to_owned());
        None
    }
    let mut visiting = Vec::new();
    let mut completed = std::collections::BTreeSet::new();
    for definition in definitions {
        if let Some(cycle) = visit(
            &definition.macro_id,
            definitions,
            &mut visiting,
            &mut completed,
        ) {
            return Some(cycle);
        }
    }
    None
}

/// 按 `macro_id` 取最新已保存版本（语义版本比较；解析失败按字符串比较兜底）。
pub fn latest_definition<'a>(
    definitions: &'a [MacroDefinition],
    macro_id: &str,
) -> Option<&'a MacroDefinition> {
    definitions
        .iter()
        .filter(|d| d.macro_id == macro_id)
        .max_by(|a, b| compare_versions(&a.version, &b.version))
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (parse_semver(a), parse_semver(b)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => a.cmp(b),
    }
}

/// 库合并（导入语义）：同 `macro_id@version` 内容一致 → 跳过；内容不同 →
/// 不可变冲突报错；新条目 → 追加。任何失败都不改变现状（调用方先校验再落盘）。
pub fn merge_library(
    base: &MacroLibraryDocument,
    incoming: MacroLibraryDocument,
) -> Result<MacroLibraryDocument, MacroIssue> {
    let mut merged = base.clone();
    for definition in incoming.definitions {
        let same_version = merged
            .definitions
            .iter()
            .find(|d| d.macro_id == definition.macro_id && d.version == definition.version);
        match same_version {
            Some(existing) if *existing == definition => continue,
            Some(_) => return Err(MacroIssue::ImmutableVersionConflict),
            None => merged.definitions.push(definition),
        }
    }
    Ok(merged)
}

/// 单步有效超时 = min(步骤显式超时或动作默认, 整宏剩余时间)。
pub fn effective_step_timeout(step: &MacroStep, overall_remaining_ms: u64) -> u64 {
    let requested = step
        .timeout_ms
        .unwrap_or_else(|| step.action.default_timeout_ms());
    requested.min(overall_remaining_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(action: MacroAction, parameters: MacroParameters) -> MacroStep {
        MacroStep {
            step_id: "step.01".to_owned(),
            action,
            timeout_ms: None,
            parameters,
        }
    }

    fn definition(steps: Vec<MacroStep>) -> MacroDefinition {
        MacroDefinition {
            schema_version: MACRO_SCHEMA_VERSION.to_owned(),
            macro_id: "macro.abcdef".to_owned(),
            version: "1.0.0".to_owned(),
            name: "测试动作".to_owned(),
            summary: None,
            steps,
        }
    }

    fn open_app_steps() -> Vec<MacroStep> {
        vec![step(
            MacroAction::OpenApplication,
            MacroParameters {
                target: Some("vscode".to_owned()),
                ..MacroParameters::default()
            },
        )]
    }

    #[test]
    fn roundtrip_camel_case_wire() {
        let definition = definition(open_app_steps());
        let json = serde_json::to_value(&definition).unwrap();
        assert_eq!(json["schemaVersion"], "0.1");
        assert_eq!(json["steps"][0]["stepId"], "step.01");
        assert_eq!(json["steps"][0]["action"], "open_application");
        assert_eq!(json["steps"][0]["parameters"]["target"], "vscode");
        let parsed: MacroDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, definition);
    }

    #[test]
    fn unknown_top_level_field_rejected() {
        let definition = definition(open_app_steps());
        let mut json = serde_json::to_value(&definition).unwrap();
        json["extra"] = 1.into();
        assert!(serde_json::from_value::<MacroDefinition>(json).is_err());
    }

    #[test]
    fn unexpected_parameter_rejected_per_action() {
        let bad = definition(vec![step(
            MacroAction::OpenApplication,
            MacroParameters {
                target: Some("vscode".to_owned()),
                url: Some("https://example.com".to_owned()),
                ..MacroParameters::default()
            },
        )]);
        let issues = validate_definition(&bad).unwrap_err();
        assert!(issues.contains(&MacroIssue::UnexpectedParameter("url")));
    }

    #[test]
    fn name_bounds() {
        let mut def = definition(open_app_steps());
        def.name = "".to_owned();
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::NameEmpty));
        def.name = "长".repeat(MAX_MACRO_NAME_CHARS);
        assert!(validate_definition(&def).is_ok());
        def.name = "长".repeat(MAX_MACRO_NAME_CHARS + 1);
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::NameTooLong));
    }

    #[test]
    fn step_count_bounds() {
        let mut def = definition(open_app_steps());
        def.steps = vec![];
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::TooFewSteps));
        def.steps = (0..MAX_MACRO_STEPS)
            .map(|i| MacroStep {
                step_id: format!("step.{i:02}"),
                ..open_app_steps()[0].clone()
            })
            .collect();
        assert!(validate_definition(&def).is_ok());
        def.steps.push(open_app_steps()[0].clone());
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::TooManySteps));
    }

    #[test]
    fn step_timeout_bounds_by_action() {
        let mut def = definition(open_app_steps());
        def.steps[0].timeout_ms = Some(MIN_STEP_TIMEOUT_MS - 1);
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::StepTimeoutOutOfRange));
        def.steps[0].timeout_ms = Some(MAX_STEP_TIMEOUT_MS + 1);
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::StepTimeoutOutOfRange));
        def.steps[0].timeout_ms = Some(MAX_STEP_TIMEOUT_MS);
        assert!(validate_definition(&def).is_ok());
        // run_macro 允许到 60 秒档。
        let mut nested = definition(vec![step(
            MacroAction::RunMacro,
            MacroParameters {
                nested_macro_id: Some("macro.123456".to_owned()),
                nested_macro_name: Some("别的动作".to_owned()),
                ..MacroParameters::default()
            },
        )]);
        nested.steps[0].timeout_ms = Some(MAX_NESTED_STEP_TIMEOUT_MS);
        assert!(validate_definition(&nested).is_ok());
        nested.steps[0].timeout_ms = Some(MAX_NESTED_STEP_TIMEOUT_MS + 1);
        assert!(validate_definition(&nested)
            .unwrap_err()
            .contains(&MacroIssue::StepTimeoutOutOfRange));
    }

    #[test]
    fn url_acceptance_matrix() {
        for ok in [
            "https://example.com",
            "http://example.com/path?q=1#frag",
            "https://sub.example.com:8443/a",
        ] {
            assert!(validate_http_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "ftp://example.com",
            "file:///etc/passwd",
            "https:///missing-host",
            "https://user:pass@example.com",
            "https://example .com",
            "javascript:alert(1)",
            "https://example.com/\u{0}\u{7}",
        ] {
            assert!(validate_http_url(bad).is_err(), "{bad}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS));
        assert!(validate_http_url(&long).is_err());
    }

    #[test]
    fn chord_rules() {
        let mut def = definition(vec![step(
            MacroAction::SendShortcut,
            MacroParameters {
                chord: Some(MacroChord {
                    modifiers: vec![MacroModifier::Control, MacroModifier::Alt],
                    key: "n".to_owned(),
                }),
                ..MacroParameters::default()
            },
        )]);
        assert!(validate_definition(&def).is_ok());
        def.steps[0].parameters.chord = Some(MacroChord {
            modifiers: vec![MacroModifier::Control, MacroModifier::Control],
            key: "n".to_owned(),
        });
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::InvalidChord));
        def.steps[0].parameters.chord = Some(MacroChord {
            modifiers: vec![],
            key: String::new(),
        });
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::InvalidChord));
        def.steps[0].parameters.chord = None;
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::MissingParameter("chord")));
    }

    #[test]
    fn run_macro_self_reference_rejected() {
        let def = definition(vec![step(
            MacroAction::RunMacro,
            MacroParameters {
                nested_macro_id: Some("macro.abcdef".to_owned()),
                nested_macro_name: Some("自己".to_owned()),
                ..MacroParameters::default()
            },
        )]);
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::NestedSelf));
    }

    #[test]
    fn duplicate_step_id_rejected() {
        let mut def = definition(open_app_steps());
        def.steps.push(def.steps[0].clone());
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::DuplicateStepId));
    }

    #[test]
    fn macro_id_and_version_format() {
        let mut def = definition(open_app_steps());
        def.macro_id = "short".to_owned();
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::InvalidMacroId));
        def.macro_id = "-starts-dash".to_owned();
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::InvalidMacroId));
        def.macro_id = "macro.abcdef".to_owned();
        def.version = "1.0".to_owned();
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::InvalidVersion));
        def.version = "01.0.0".to_owned();
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::InvalidVersion));
        def.version = "1.0.0".to_owned();
        assert!(validate_definition(&def).is_ok());
    }

    #[test]
    fn library_detects_direct_and_indirect_cycles() {
        let make = |id: &str, nested: &str| {
            definition(vec![step(
                MacroAction::RunMacro,
                MacroParameters {
                    nested_macro_id: Some(nested.to_owned()),
                    nested_macro_name: Some("引用".to_owned()),
                    ..MacroParameters::default()
                },
            )])
            .macro_ided(id)
        };
        trait MacroIded {
            fn macro_ided(self, id: &str) -> MacroDefinition;
        }
        impl MacroIded for MacroDefinition {
            fn macro_ided(mut self, id: &str) -> MacroDefinition {
                self.macro_id = id.to_owned();
                self
            }
        }
        // 直接环：A → A。
        let direct = vec![make("macro.aaaaaaa", "macro.aaaaaaa")];
        assert!(find_cycle(&direct).is_some());
        // 间接环：A → B → A。
        let indirect = vec![
            make("macro.aaaaaaa", "macro.bbbbbbb"),
            make("macro.bbbbbbb", "macro.aaaaaaa"),
        ];
        let cycle = find_cycle(&indirect).unwrap();
        assert_eq!(cycle.first(), cycle.last());
        // 无环：A → B。
        let acyclic = vec![
            make("macro.aaaaaaa", "macro.bbbbbbb"),
            make("macro.bbbbbbb", "macro.ccccccc"),
        ];
        assert!(find_cycle(&acyclic).is_none());
    }

    #[test]
    fn library_rejects_immutable_conflict_and_dedupes_identical() {
        let def = definition(open_app_steps());
        let doc = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![def.clone()],
        };
        // 完全一致 → 通过。
        assert!(validate_library(&doc).is_ok());
        // 同 id@version 内容不同 → 冲突。
        let mut other = def.clone();
        other.name = "改名同版本".to_owned();
        let conflict = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![def.clone(), other],
        };
        assert!(validate_library(&conflict)
            .unwrap_err()
            .contains(&MacroIssue::DuplicateDefinition));
        // 新版本（patch 升级）→ 允许共存。
        let mut newer = def.clone();
        newer.version = "1.0.1".to_owned();
        let multi = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![def, newer],
        };
        assert!(validate_library(&multi).is_ok());
    }

    #[test]
    fn latest_definition_picks_highest_version() {
        let base = definition(open_app_steps());
        let mut older = base.clone();
        older.version = "1.0.0".to_owned();
        let mut newer = base.clone();
        newer.version = "1.2.0".to_owned();
        let defs = vec![older, newer.clone()];
        assert_eq!(
            latest_definition(&defs, "macro.abcdef").map(|d| d.version.as_str()),
            Some("1.2.0")
        );
        assert!(latest_definition(&defs, "macro.missing").is_none());
    }

    #[test]
    fn merge_library_semantics() {
        let base_def = definition(open_app_steps());
        let base = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![base_def.clone()],
        };
        // 完全一致 → 跳过，不产生重复。
        let same = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![base_def.clone()],
        };
        let merged = merge_library(&base, same).unwrap();
        assert_eq!(merged.definitions.len(), 1);
        // 新增 → 追加。
        let mut added = base_def.clone();
        added.macro_id = "macro.newadded".to_owned();
        let incoming = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![base_def, added.clone()],
        };
        let merged = merge_library(&base, incoming).unwrap();
        assert_eq!(merged.definitions.len(), 2);
        // 同版本不同内容 → 冲突。
        let mut conflicted = base.definitions[0].clone();
        conflicted.name = "冲突名".to_owned();
        let incoming = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![conflicted],
        };
        assert_eq!(
            merge_library(&base, incoming).unwrap_err(),
            MacroIssue::ImmutableVersionConflict
        );
    }

    #[test]
    fn effective_step_timeout_uses_min_of_step_and_remaining() {
        let mut s = step(
            MacroAction::OpenApplication,
            MacroParameters {
                target: Some("vscode".to_owned()),
                ..MacroParameters::default()
            },
        );
        // 默认 8 秒，剩余充足 → 8 秒。
        assert_eq!(effective_step_timeout(&s, 60_000), 8_000);
        // 剩余不足 → 截到剩余。
        assert_eq!(effective_step_timeout(&s, 3_000), 3_000);
        // 显式超时优先。
        s.timeout_ms = Some(1_500);
        assert_eq!(effective_step_timeout(&s, 60_000), 1_500);
    }

    #[test]
    fn schema_version_mismatch_rejected() {
        let mut def = definition(open_app_steps());
        def.schema_version = "0.2".to_owned();
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::UnsupportedSchemaVersion));
    }

    #[test]
    fn too_many_definitions_rejected() {
        let def = definition(open_app_steps());
        let doc = MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions: vec![def; MAX_MACRO_DEFINITIONS + 1],
        };
        assert!(validate_library(&doc)
            .unwrap_err()
            .contains(&MacroIssue::TooManyDefinitions));
    }

    #[test]
    fn wait_for_application_requires_state() {
        let def = definition(vec![step(
            MacroAction::WaitForApplication,
            MacroParameters {
                target: Some("vscode".to_owned()),
                state: None,
                ..MacroParameters::default()
            },
        )]);
        assert!(validate_definition(&def)
            .unwrap_err()
            .contains(&MacroIssue::MissingParameter("state")));
    }
}
