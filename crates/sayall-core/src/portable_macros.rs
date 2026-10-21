//! 组合动作与 macOS `.sayall` 交换格式的转换。
//!
//! macOS 的“组合动作”页面导入/导出使用 `sayall-transfer 1.0`（`.sayall`），
//! 不是它的本地 `library.json`。本模块只负责边界转换：Windows 本地仍使用
//! `MacroLibraryDocument` / `combined-actions.json`，不改变运行时模型。
//!
//! 共享格式只承载可安全转换的子集：
//! - 支持 openApplication / waitForApplication / sendKeyboardShortcut /
//!   openURL / runMacro / builtInAction；
//! - 不导出或导入 focusInput / focusLearnedTarget（macOS 共享格式本身拒绝
//!   `focusInput`，学习目标还依赖本机 AX 档案）；
//! - 不导入 runShortcut / runShellScript / runAppleScript（Windows 无对应
//!   能力，且不允许执行本机脚本）；
//! - 应用作用域（`scope`）只接受 global；非 global 直接报错，避免静默放宽。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::macros::{
    latest_definitions, validate_http_url, validate_library, ApplicationWaitState, MacroAction,
    MacroChord, MacroDefinition, MacroLibraryDocument, MacroModifier, MacroParameters, MacroStep,
    MACRO_LIBRARY_FORMAT_VERSION, MACRO_SCHEMA_VERSION,
};

/// macOS `.sayall` 容器标识。
pub const PORTABLE_FORMAT: &str = "sayall-transfer";
/// macOS `.sayall` 容器版本。
pub const PORTABLE_SCHEMA_VERSION: &str = "1.0";
/// 共享格式允许的最小读取器版本。
pub const PORTABLE_MIN_READER_VERSION: &str = "1.0";
/// macOS 端支持当前共享格式的最低 App 版本（Mac 端合同里的固定值）。
pub const PORTABLE_MIN_REMOTE_MIC_VERSION: &str = "1.9.21";
/// macOS `.sayall` 文件大小上限（与 Mac `PortableTransferPackage.maximumBytes` 一致）。
pub const PORTABLE_MAX_BYTES: usize = 4 * 1024 * 1024;
/// `.sayall` 内每个组合动作的 schema 版本；macOS 共享格式要求 `1.0`。
pub const PORTABLE_MACRO_SCHEMA_VERSION: &str = "1.0";

/// 转换错误。`code` 供日志和测试使用，`detail` 面向用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableError {
    code: &'static str,
    detail: String,
}

impl PortableError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for PortableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for PortableError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortablePackage {
    format: String,
    schema_version: String,
    #[serde(rename = "packageID")]
    package_id: String,
    version: String,
    requirements: PortableRequirements,
    export_purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    extensions: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(rename = "type")]
    transfer_type: String,
    #[serde(default, rename = "macro", skip_serializing_if = "Option::is_none")]
    macro_definition: Option<PortableMacro>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    application: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    backup: Option<PortableBackup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependencies: Option<PortableDependencies>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableRequirements {
    minimum_reader_version: String,
    minimum_remote_mic_version: String,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableRoot {
    kind: String,
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableBackup {
    roots: Vec<PortableRoot>,
    #[serde(default)]
    macros: Vec<PortableMacro>,
    #[serde(default)]
    shortcuts: Vec<serde_json::Value>,
    #[serde(default)]
    focus_targets: Vec<serde_json::Value>,
    #[serde(default)]
    applications: Vec<serde_json::Value>,
    #[serde(default)]
    button_profiles: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host_settings: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableDependencies {
    #[serde(default)]
    macros: Vec<PortableMacro>,
    #[serde(default)]
    shortcuts: Vec<serde_json::Value>,
    #[serde(default)]
    focus_targets: Vec<serde_json::Value>,
    #[serde(default)]
    applications: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableMacro {
    schema_version: String,
    #[serde(rename = "macroID")]
    macro_id: String,
    version: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<PortableScope>,
    steps: Vec<PortableStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableScope {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle_identifiers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableStep {
    #[serde(rename = "stepID")]
    step_id: String,
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout_milliseconds: Option<u64>,
    #[serde(default)]
    parameters: PortableParameters,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableParameters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_profile_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modifiers: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcut_profile_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcut_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcut_name: Option<String>,
    #[serde(
        rename = "nestedMacroID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    nested_macro_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nested_macro_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    script_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    script_interpreter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    built_in_action: Option<String>,
}

const APP_BUNDLE_BY_PRESET: &[(&str, &str)] = &[
    ("netease_music", "com.netease.163music"),
    ("chrome", "com.google.Chrome"),
    ("wechat", "com.tencent.xinWeChat"),
    ("vscode", "com.microsoft.VSCode"),
    ("cursor", "com.todesktop.230313mzl4w4u92"),
    ("edge", "com.microsoft.edgemac"),
    ("notepad", "com.apple.TextEdit"),
    ("calc", "com.apple.calculator"),
    ("explorer", "com.apple.finder"),
    ("sayall", "com.hd838a.RemoteMic"),
];

const BUILT_IN_KEYS: &[(&str, &str)] = &[
    ("escape", "escape"),
    ("playPause", "media_play_pause"),
    ("previousTrack", "media_prev"),
    ("nextTrack", "media_next"),
    ("volumeUp", "volume_up"),
    ("volumeDown", "volume_down"),
    ("mute", "volume_mute"),
];

/// 把 Windows 本地库导出为 macOS 可导入的 `.sayall`。
///
/// 导出整库，使用 `personalBackup` 容器；只包含每个 `macro_id` 的最新版本。
/// 含 Windows 专属步骤的库会整体失败并指出具体动作，避免静默丢步骤。
pub fn export_portable_library(document: &MacroLibraryDocument) -> Result<String, PortableError> {
    validate_library(document).map_err(|issues| {
        PortableError::new(
            "invalid_library",
            format!(
                "组合动作库无效：{}",
                issues
                    .iter()
                    .map(|issue| issue.code())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        )
    })?;
    let definitions = latest_definitions(&document.definitions);
    if definitions.is_empty() {
        return Err(PortableError::new(
            "empty_library",
            "当前没有可导出的组合动作",
        ));
    }
    let mut macros = Vec::with_capacity(definitions.len());
    let mut roots = Vec::with_capacity(definitions.len());
    for definition in &definitions {
        macros.push(to_portable_macro(definition)?);
        roots.push(PortableRoot {
            kind: "macro".to_owned(),
            id: definition.macro_id.clone(),
        });
    }
    let package = PortablePackage {
        format: PORTABLE_FORMAT.to_owned(),
        schema_version: PORTABLE_SCHEMA_VERSION.to_owned(),
        package_id: "sayall.windows.combined-actions".to_owned(),
        version: "1.0.0".to_owned(),
        requirements: PortableRequirements {
            minimum_reader_version: PORTABLE_MIN_READER_VERSION.to_owned(),
            minimum_remote_mic_version: PORTABLE_MIN_REMOTE_MIC_VERSION.to_owned(),
            capabilities: Vec::new(),
        },
        export_purpose: "personalBackup".to_owned(),
        website: Some("https://sayall.app".to_owned()),
        github: Some("https://github.com/GetSayAll/remote-mic-app-windows".to_owned()),
        extensions: BTreeMap::new(),
        transfer_type: "personalBackup".to_owned(),
        macro_definition: None,
        profile: None,
        application: None,
        backup: Some(PortableBackup {
            roots,
            macros,
            shortcuts: Vec::new(),
            focus_targets: Vec::new(),
            applications: Vec::new(),
            button_profiles: Vec::new(),
            host_settings: None,
        }),
        dependencies: None,
    };
    validate_package(&package)?;
    serde_json::to_string_pretty(&package)
        .map_err(|error| PortableError::new("serialize_failed", error.to_string()))
}

/// 解析 macOS `.sayall` 并转换为 Windows 本地库。
///
/// 只接受组合动作：`type=macro`，或 `type=personalBackup` 且所有 root 都是
/// `macro`。文件里若包含按键方案、设置、脚本等 Windows 不能安全承载的内容，
/// 整体拒绝，不做部分导入。
pub fn import_portable_library(bytes: &[u8]) -> Result<MacroLibraryDocument, PortableError> {
    if bytes.len() > PORTABLE_MAX_BYTES {
        return Err(PortableError::new("too_large", "组合动作文件过大"));
    }
    let package: PortablePackage = serde_json::from_slice(bytes)
        .map_err(|error| PortableError::new("invalid_json", error.to_string()))?;
    validate_package(&package)?;
    let (macros, roots) = match package.transfer_type.as_str() {
        "macro" => {
            let macro_definition = package
                .macro_definition
                .ok_or_else(|| PortableError::new("invalid_package", "组合动作文件缺少 macro"))?;
            let mut macros = vec![macro_definition];
            if let Some(dependencies) = &package.dependencies {
                macros.extend(dependencies.macros.iter().cloned());
                reject_unsupported_dependencies(dependencies)?;
            }
            let roots = vec![PortableRoot {
                kind: "macro".to_owned(),
                id: macros[0].macro_id.clone(),
            }];
            (macros, roots)
        }
        "personalBackup" => {
            let backup = package
                .backup
                .ok_or_else(|| PortableError::new("invalid_package", "备份文件缺少 backup"))?;
            if !backup.shortcuts.is_empty()
                || !backup.focus_targets.is_empty()
                || !backup.button_profiles.is_empty()
                || backup.host_settings.is_some()
            {
                return Err(PortableError::new(
                    "unsupported_content",
                    "该文件还包含按键方案或设置；请只导出组合动作",
                ));
            }
            if backup.roots.is_empty() || backup.roots.iter().any(|root| root.kind != "macro") {
                return Err(PortableError::new(
                    "unsupported_content",
                    "该备份包含组合动作以外的内容",
                ));
            }
            let mut macros = backup.macros;
            if let Some(dependencies) = &package.dependencies {
                macros.extend(dependencies.macros.iter().cloned());
                reject_unsupported_dependencies(dependencies)?;
            }
            (macros, backup.roots)
        }
        other => {
            return Err(PortableError::new(
                "unsupported_type",
                format!("不支持的文件类型：{other}"),
            ))
        }
    };

    let root_ids: BTreeSet<&str> = roots.iter().map(|root| root.id.as_str()).collect();
    if roots.len() != root_ids.len() {
        return Err(PortableError::new("invalid_package", "组合动作 root 重复"));
    }
    let mut definitions = Vec::with_capacity(macros.len());
    let mut seen = BTreeSet::new();
    for macro_definition in macros {
        if !seen.insert(macro_definition.macro_id.clone()) {
            return Err(PortableError::new(
                "invalid_package",
                "同一个组合动作出现了多个版本",
            ));
        }
        definitions.push(from_portable_macro(&macro_definition)?);
    }
    if !root_ids.iter().all(|id| {
        definitions
            .iter()
            .any(|definition| definition.macro_id == *id)
    }) {
        return Err(PortableError::new(
            "missing_dependency",
            "组合动作文件中的 root 缺少对应动作",
        ));
    }
    let document = MacroLibraryDocument {
        format_version: MACRO_LIBRARY_FORMAT_VERSION,
        definitions,
    };
    validate_library(&document).map_err(|issues| {
        PortableError::new(
            "invalid_library",
            format!(
                "转换后的组合动作无效：{}",
                issues
                    .iter()
                    .map(|issue| issue.code())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        )
    })?;
    Ok(document)
}

fn validate_package(package: &PortablePackage) -> Result<(), PortableError> {
    if package.format != PORTABLE_FORMAT {
        return Err(PortableError::new("invalid_format", "不是 SayAll 共享格式"));
    }
    if package.schema_version != PORTABLE_SCHEMA_VERSION {
        return Err(PortableError::new(
            "unsupported_schema",
            format!("不支持的共享格式版本：{}", package.schema_version),
        ));
    }
    if package.package_id.len() < 8 || !is_semver(&package.version, 3) {
        return Err(PortableError::new(
            "invalid_package",
            "共享格式的 packageID/version 无效",
        ));
    }
    if !is_semver(&package.requirements.minimum_remote_mic_version, 3)
        || !is_semver(&package.requirements.minimum_reader_version, 2)
        || package
            .requirements
            .minimum_reader_version
            .split('.')
            .next()
            != Some("1")
        || package.requirements.capabilities.len() > 32
    {
        return Err(PortableError::new(
            "invalid_package",
            "共享格式的 requirements 无效",
        ));
    }
    if package.export_purpose != "share" && package.export_purpose != "personalBackup" {
        return Err(PortableError::new(
            "invalid_package",
            "共享格式的 exportPurpose 无效",
        ));
    }
    match package.transfer_type.as_str() {
        "macro" => {
            if package.macro_definition.is_none() || package.export_purpose != "share" {
                return Err(PortableError::new(
                    "invalid_package",
                    "单个组合动作文件必须使用 share 用途并包含 macro",
                ));
            }
        }
        "personalBackup" => {
            if package.backup.is_none() || package.export_purpose != "personalBackup" {
                return Err(PortableError::new(
                    "invalid_package",
                    "个人备份必须使用 personalBackup 用途并包含 backup",
                ));
            }
        }
        "buttonProfile" | "application" => {
            return Err(PortableError::new(
                "unsupported_type",
                "组合动作导入只接受 macro 或 personalBackup",
            ))
        }
        _ => {
            return Err(PortableError::new(
                "unsupported_type",
                "组合动作导入只接受 macro 或 personalBackup",
            ))
        }
    }
    Ok(())
}

fn reject_unsupported_dependencies(
    dependencies: &PortableDependencies,
) -> Result<(), PortableError> {
    if !dependencies.shortcuts.is_empty() || !dependencies.focus_targets.is_empty() {
        return Err(PortableError::new(
            "unsupported_dependency",
            "该组合动作依赖快捷键或聚焦档案，Windows 版不能安全导入",
        ));
    }
    Ok(())
}

fn to_portable_macro(definition: &MacroDefinition) -> Result<PortableMacro, PortableError> {
    let steps = definition
        .steps
        .iter()
        .map(|step| to_portable_step(&definition.macro_id, step))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PortableMacro {
        schema_version: PORTABLE_MACRO_SCHEMA_VERSION.to_owned(),
        macro_id: definition.macro_id.clone(),
        version: definition.version.clone(),
        name: definition.name.clone(),
        summary: definition.summary.clone(),
        scope: None,
        steps,
    })
}

fn to_portable_step(macro_id: &str, step: &MacroStep) -> Result<PortableStep, PortableError> {
    let mut parameters = PortableParameters::default();
    let action = match step.action {
        MacroAction::OpenApplication => {
            let target = required(macro_id, &step.step_id, "target", &step.parameters.target)?;
            parameters.bundle_identifier = Some(preset_to_bundle(target).ok_or_else(|| {
                unsupported(
                    macro_id,
                    &step.step_id,
                    format!("无法把应用 {target} 映射到 macOS"),
                )
            })?);
            "openApplication"
        }
        MacroAction::WaitForApplication => {
            let target = required(macro_id, &step.step_id, "target", &step.parameters.target)?;
            parameters.bundle_identifier = Some(preset_to_bundle(target).ok_or_else(|| {
                unsupported(
                    macro_id,
                    &step.step_id,
                    format!("无法把应用 {target} 映射到 macOS"),
                )
            })?);
            parameters.state = Some(
                match step
                    .parameters
                    .state
                    .unwrap_or(ApplicationWaitState::VisibleWindow)
                {
                    ApplicationWaitState::Running => "running",
                    ApplicationWaitState::VisibleWindow => "frontmostWindow",
                }
                .to_owned(),
            );
            "waitForApplication"
        }
        MacroAction::SendShortcut => {
            let chord = step
                .parameters
                .chord
                .as_ref()
                .ok_or_else(|| unsupported(macro_id, &step.step_id, "缺少快捷键组合"))?;
            let modifiers = to_portable_modifiers(macro_id, &step.step_id, &chord.modifiers)?;
            if !modifiers.is_empty() {
                parameters.modifiers = Some(modifiers);
            }
            parameters.key = Some(normalize_key(macro_id, &step.step_id, &chord.key)?);
            "sendKeyboardShortcut"
        }
        MacroAction::OpenUrl => {
            let url = required(macro_id, &step.step_id, "url", &step.parameters.url)?;
            validate_http_url(url)
                .map_err(|_| unsupported(macro_id, &step.step_id, "只支持 http/https 网址"))?;
            parameters.url_string = Some(url.to_owned());
            "openURL"
        }
        MacroAction::RunMacro => {
            let nested = required(
                macro_id,
                &step.step_id,
                "nestedMacroId",
                &step.parameters.nested_macro_id,
            )?;
            parameters.nested_macro_id = Some(nested.to_owned());
            parameters.nested_macro_name = step.parameters.nested_macro_name.clone();
            "runMacro"
        }
        MacroAction::SystemAction => {
            let chord = step
                .parameters
                .chord
                .as_ref()
                .ok_or_else(|| unsupported(macro_id, &step.step_id, "缺少系统动作按键"))?;
            if !chord.modifiers.is_empty() {
                return Err(unsupported(
                    macro_id,
                    &step.step_id,
                    "共享格式的系统动作不能带修饰键",
                ));
            }
            parameters.built_in_action = Some(
                key_to_built_in(&chord.key)
                    .ok_or_else(|| {
                        unsupported(
                            macro_id,
                            &step.step_id,
                            format!("系统动作 {} 没有 macOS 等价", chord.key),
                        )
                    })?
                    .to_owned(),
            );
            "builtInAction"
        }
        MacroAction::FocusInput | MacroAction::FocusFrontmost => {
            return Err(unsupported(
                macro_id,
                &step.step_id,
                "macOS 共享格式不支持聚焦输入框步骤",
            ))
        }
    };
    Ok(PortableStep {
        step_id: step.step_id.clone(),
        action: action.to_owned(),
        timeout_milliseconds: step.timeout_ms,
        parameters,
    })
}

fn from_portable_macro(definition: &PortableMacro) -> Result<MacroDefinition, PortableError> {
    if definition.schema_version != PORTABLE_MACRO_SCHEMA_VERSION {
        return Err(PortableError::new(
            "unsupported_schema",
            format!(
                "组合动作 {} 使用不支持的 schema：{}",
                definition.macro_id, definition.schema_version
            ),
        ));
    }
    if let Some(scope) = &definition.scope {
        let global = scope.kind == "global"
            && scope
                .bundle_identifiers
                .as_ref()
                .map(|ids| ids.is_empty())
                .unwrap_or(true);
        if !global {
            return Err(PortableError::new(
                "unsupported_scope",
                format!(
                    "组合动作 {} 限定了应用范围，Windows 版不能安全导入",
                    definition.macro_id
                ),
            ));
        }
    }
    let steps = definition
        .steps
        .iter()
        .map(|step| from_portable_step(&definition.macro_id, step))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MacroDefinition {
        schema_version: MACRO_SCHEMA_VERSION.to_owned(),
        macro_id: definition.macro_id.clone(),
        version: definition.version.clone(),
        name: definition.name.clone(),
        summary: definition.summary.clone(),
        steps,
    })
}

fn from_portable_step(macro_id: &str, step: &PortableStep) -> Result<MacroStep, PortableError> {
    let p = &step.parameters;
    let (action, parameters) = match step.action.as_str() {
        "openApplication" => {
            reject_extra(macro_id, &step.step_id, p, &["bundle_identifier"])?;
            let bundle = required(
                macro_id,
                &step.step_id,
                "bundleIdentifier",
                &p.bundle_identifier,
            )?;
            (
                MacroAction::OpenApplication,
                MacroParameters {
                    target: Some(bundle_to_preset(bundle).ok_or_else(|| {
                        unsupported(
                            macro_id,
                            &step.step_id,
                            format!("macOS 应用 {bundle} 在 Windows 没有预设"),
                        )
                    })?),
                    ..MacroParameters::default()
                },
            )
        }
        "waitForApplication" => {
            reject_extra(macro_id, &step.step_id, p, &["bundle_identifier", "state"])?;
            let bundle = required(
                macro_id,
                &step.step_id,
                "bundleIdentifier",
                &p.bundle_identifier,
            )?;
            let state = match p.state.as_deref() {
                Some("running") => ApplicationWaitState::Running,
                Some("frontmostWindow") => ApplicationWaitState::VisibleWindow,
                Some("frontmost") => {
                    return Err(unsupported(
                        macro_id,
                        &step.step_id,
                        "macOS 的 frontmost 等待条件在 Windows 没有等价",
                    ))
                }
                Some(other) => {
                    return Err(unsupported(
                        macro_id,
                        &step.step_id,
                        format!("未知的等待条件：{other}"),
                    ))
                }
                None => return Err(unsupported(macro_id, &step.step_id, "缺少等待条件")),
            };
            (
                MacroAction::WaitForApplication,
                MacroParameters {
                    target: Some(bundle_to_preset(bundle).ok_or_else(|| {
                        unsupported(
                            macro_id,
                            &step.step_id,
                            format!("macOS 应用 {bundle} 在 Windows 没有预设"),
                        )
                    })?),
                    state: Some(state),
                    ..MacroParameters::default()
                },
            )
        }
        "sendKeyboardShortcut" => {
            reject_extra(macro_id, &step.step_id, p, &["modifiers", "key"])?;
            let key = required(macro_id, &step.step_id, "key", &p.key)?;
            let modifiers =
                from_portable_modifiers(macro_id, &step.step_id, p.modifiers.as_deref());
            (
                MacroAction::SendShortcut,
                MacroParameters {
                    chord: Some(MacroChord {
                        modifiers: modifiers?,
                        key: normalize_key(macro_id, &step.step_id, key)?,
                    }),
                    ..MacroParameters::default()
                },
            )
        }
        "openURL" => {
            reject_extra(macro_id, &step.step_id, p, &["url_string"])?;
            let url = required(macro_id, &step.step_id, "urlString", &p.url_string)?;
            validate_http_url(url)
                .map_err(|_| unsupported(macro_id, &step.step_id, "只支持 http/https 网址"))?;
            (
                MacroAction::OpenUrl,
                MacroParameters {
                    url: Some(url.to_owned()),
                    ..MacroParameters::default()
                },
            )
        }
        "runMacro" => {
            reject_extra(
                macro_id,
                &step.step_id,
                p,
                &["nested_macro_id", "nested_macro_name"],
            )?;
            let nested = required(macro_id, &step.step_id, "nestedMacroID", &p.nested_macro_id)?;
            (
                MacroAction::RunMacro,
                MacroParameters {
                    nested_macro_id: Some(nested.to_owned()),
                    nested_macro_name: p.nested_macro_name.clone(),
                    ..MacroParameters::default()
                },
            )
        }
        "builtInAction" => {
            reject_extra(macro_id, &step.step_id, p, &["built_in_action"])?;
            let action = required(macro_id, &step.step_id, "builtInAction", &p.built_in_action)?;
            let key = built_in_to_key(action).ok_or_else(|| {
                unsupported(
                    macro_id,
                    &step.step_id,
                    format!("系统动作 {action} 在 Windows 没有等价"),
                )
            })?;
            (
                MacroAction::SystemAction,
                MacroParameters {
                    chord: Some(MacroChord {
                        modifiers: Vec::new(),
                        key: key.to_owned(),
                    }),
                    ..MacroParameters::default()
                },
            )
        }
        "focusInput" | "focusLearnedTarget" => {
            return Err(unsupported(
                macro_id,
                &step.step_id,
                "macOS 共享格式的聚焦步骤在 Windows 不能安全转换",
            ))
        }
        "runShortcut" | "runShellScript" | "runAppleScript" => {
            return Err(unsupported(
                macro_id,
                &step.step_id,
                "Windows 版不支持执行 macOS 快捷指令或脚本",
            ))
        }
        other => {
            return Err(unsupported(
                macro_id,
                &step.step_id,
                format!("未知的共享格式动作：{other}"),
            ))
        }
    };
    Ok(MacroStep {
        step_id: step.step_id.clone(),
        action,
        timeout_ms: step.timeout_milliseconds,
        parameters,
    })
}

fn reject_extra(
    macro_id: &str,
    step_id: &str,
    parameters: &PortableParameters,
    allowed: &[&str],
) -> Result<(), PortableError> {
    let used = [
        ("bundle_identifier", parameters.bundle_identifier.is_some()),
        ("url_string", parameters.url_string.is_some()),
        ("state", parameters.state.is_some()),
        ("local_profile_key", parameters.local_profile_key.is_some()),
        ("modifiers", parameters.modifiers.is_some()),
        ("key", parameters.key.is_some()),
        (
            "shortcut_profile_key",
            parameters.shortcut_profile_key.is_some(),
        ),
        (
            "shortcut_identifier",
            parameters.shortcut_identifier.is_some(),
        ),
        ("shortcut_name", parameters.shortcut_name.is_some()),
        ("nested_macro_id", parameters.nested_macro_id.is_some()),
        ("nested_macro_name", parameters.nested_macro_name.is_some()),
        ("script_source", parameters.script_source.is_some()),
        (
            "script_interpreter",
            parameters.script_interpreter.is_some(),
        ),
        ("built_in_action", parameters.built_in_action.is_some()),
    ];
    if let Some((name, _)) = used
        .iter()
        .find(|(name, present)| *present && !allowed.contains(name))
    {
        return Err(unsupported(
            macro_id,
            step_id,
            format!("步骤参数 {name} 在 Windows 版没有等价"),
        ));
    }
    Ok(())
}

fn to_portable_modifiers(
    macro_id: &str,
    step_id: &str,
    modifiers: &[MacroModifier],
) -> Result<Vec<String>, PortableError> {
    let mut result = Vec::new();
    for modifier in modifiers {
        let mapped = match modifier {
            MacroModifier::Control => "command",
            MacroModifier::Alt => "option",
            MacroModifier::Shift => "shift",
            MacroModifier::Win => {
                return Err(unsupported(
                    macro_id,
                    step_id,
                    "Windows 键在 macOS 没有安全等价",
                ))
            }
        };
        if !result.iter().any(|value| value == mapped) {
            result.push(mapped.to_owned());
        }
    }
    Ok(result)
}

fn from_portable_modifiers(
    macro_id: &str,
    step_id: &str,
    modifiers: Option<&[String]>,
) -> Result<Vec<MacroModifier>, PortableError> {
    let mut result = Vec::new();
    for modifier in modifiers.unwrap_or_default() {
        let mapped = match modifier.as_str() {
            "command" | "control" => MacroModifier::Control,
            "option" => MacroModifier::Alt,
            "shift" => MacroModifier::Shift,
            "function" => {
                return Err(unsupported(
                    macro_id,
                    step_id,
                    "Fn 修饰键在 Windows 版没有等价",
                ))
            }
            other => {
                return Err(unsupported(
                    macro_id,
                    step_id,
                    format!("未知修饰键：{other}"),
                ))
            }
        };
        if !result.contains(&mapped) {
            result.push(mapped);
        }
    }
    Ok(result)
}

fn normalize_key(macro_id: &str, step_id: &str, key: &str) -> Result<String, PortableError> {
    let lower = key.to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "return" | "enter" | "⏎" => "return",
        "escape" | "esc" | "⎋" => "escape",
        "space" => "space",
        "tab" | "⇥" => "tab",
        "backspace" | "⌫" => "backspace",
        "delete" => "delete",
        "insert" => "insert",
        "home" => "home",
        "end" => "end",
        "pageup" | "page_up" => "pageup",
        "pagedown" | "page_down" => "pagedown",
        "left" | "←" => "left",
        "right" | "→" => "right",
        "up" | "↑" => "up",
        "down" | "↓" => "down",
        "volume_mute" | "volumeup" | "volumedown" | "volume_up" | "volume_down" => lower.as_str(),
        _ if lower.chars().count() == 1 && lower.chars().all(|c| c.is_ascii_alphanumeric()) => {
            lower.as_str()
        }
        _ if lower.starts_with('f')
            && lower[1..]
                .parse::<u8>()
                .map(|n| (1..=12).contains(&n))
                .unwrap_or(false) =>
        {
            lower.as_str()
        }
        _ => {
            return Err(unsupported(
                macro_id,
                step_id,
                format!("按键 {key} 没有安全的跨平台名称"),
            ))
        }
    };
    Ok(normalized.to_owned())
}

fn key_to_built_in(key: &str) -> Option<&'static str> {
    let lower = key.to_ascii_lowercase();
    BUILT_IN_KEYS
        .iter()
        .find(|(_, windows_key)| *windows_key == lower.as_str())
        .map(|(mac_action, _)| *mac_action)
}

fn built_in_to_key(action: &str) -> Option<&'static str> {
    BUILT_IN_KEYS
        .iter()
        .find(|(mac_action, _)| *mac_action == action)
        .map(|(_, windows_key)| *windows_key)
}

fn preset_to_bundle(preset: &str) -> Option<String> {
    APP_BUNDLE_BY_PRESET
        .iter()
        .find(|(id, _)| *id == preset)
        .map(|(_, bundle)| (*bundle).to_owned())
}

fn bundle_to_preset(bundle: &str) -> Option<String> {
    APP_BUNDLE_BY_PRESET
        .iter()
        .find(|(_, id)| *id == bundle)
        .map(|(preset, _)| (*preset).to_owned())
}

fn required<'a>(
    macro_id: &str,
    step_id: &str,
    field: &str,
    value: &'a Option<String>,
) -> Result<&'a str, PortableError> {
    value
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| unsupported(macro_id, step_id, format!("缺少 {field}")))
}

fn unsupported(macro_id: &str, step_id: &str, detail: impl Into<String>) -> PortableError {
    PortableError::new(
        "unsupported_action",
        format!("组合动作 {macro_id} / 步骤 {step_id}：{}", detail.into()),
    )
}

fn is_semver(value: &str, count: usize) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == count
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::{MacroAction, MacroChord, MacroModifier, MacroParameters, MacroStep};

    fn definition(id: &str, steps: Vec<MacroStep>) -> MacroDefinition {
        MacroDefinition {
            schema_version: MACRO_SCHEMA_VERSION.to_owned(),
            macro_id: id.to_owned(),
            version: "1.0.0".to_owned(),
            name: id.to_owned(),
            summary: None,
            steps,
        }
    }

    fn step(action: MacroAction, parameters: MacroParameters) -> MacroStep {
        step_with_id("step.01", action, parameters)
    }

    fn step_with_id(id: &str, action: MacroAction, parameters: MacroParameters) -> MacroStep {
        MacroStep {
            step_id: id.to_owned(),
            action,
            timeout_ms: None,
            parameters,
        }
    }

    fn library(definitions: Vec<MacroDefinition>) -> MacroLibraryDocument {
        MacroLibraryDocument {
            format_version: MACRO_LIBRARY_FORMAT_VERSION,
            definitions,
        }
    }

    #[test]
    fn exports_mac_shape_for_supported_subset() {
        let document = library(vec![definition(
            "macro.aaaaaaa",
            vec![
                step(
                    MacroAction::OpenApplication,
                    MacroParameters {
                        target: Some("chrome".to_owned()),
                        ..MacroParameters::default()
                    },
                ),
                step_with_id(
                    "step.02",
                    MacroAction::SendShortcut,
                    MacroParameters {
                        chord: Some(MacroChord {
                            modifiers: vec![MacroModifier::Control],
                            key: "c".to_owned(),
                        }),
                        ..MacroParameters::default()
                    },
                ),
                step_with_id(
                    "step.03",
                    MacroAction::SystemAction,
                    MacroParameters {
                        chord: Some(MacroChord {
                            modifiers: Vec::new(),
                            key: "media_play_pause".to_owned(),
                        }),
                        ..MacroParameters::default()
                    },
                ),
            ],
        )]);
        let text = export_portable_library(&document).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["format"], "sayall-transfer");
        assert_eq!(value["type"], "personalBackup");
        assert_eq!(value["backup"]["roots"][0]["id"], "macro.aaaaaaa");
        assert_eq!(value["backup"]["macros"][0]["schemaVersion"], "1.0");
        assert_eq!(
            value["backup"]["macros"][0]["steps"][0]["action"],
            "openApplication"
        );
        assert_eq!(
            value["backup"]["macros"][0]["steps"][0]["parameters"]["bundleIdentifier"],
            "com.google.Chrome"
        );
        assert_eq!(
            value["backup"]["macros"][0]["steps"][1]["parameters"]["modifiers"][0],
            "command"
        );
        assert_eq!(
            value["backup"]["macros"][0]["steps"][2]["parameters"]["builtInAction"],
            "playPause"
        );
    }

    #[test]
    fn round_trips_supported_macro_library() {
        let document = library(vec![
            definition(
                "macro.parent",
                vec![
                    step(
                        MacroAction::OpenApplication,
                        MacroParameters {
                            target: Some("vscode".to_owned()),
                            ..MacroParameters::default()
                        },
                    ),
                    step_with_id(
                        "step.02",
                        MacroAction::RunMacro,
                        MacroParameters {
                            nested_macro_id: Some("macro.child".to_owned()),
                            nested_macro_name: Some("Child".to_owned()),
                            ..MacroParameters::default()
                        },
                    ),
                ],
            ),
            definition(
                "macro.child",
                vec![step(
                    MacroAction::OpenUrl,
                    MacroParameters {
                        url: Some("https://example.com".to_owned()),
                        ..MacroParameters::default()
                    },
                )],
            ),
        ]);
        let text = export_portable_library(&document).unwrap();
        let imported = import_portable_library(text.as_bytes()).unwrap();
        assert_eq!(imported.definitions.len(), 2);
        let parent = imported
            .definitions
            .iter()
            .find(|definition| definition.macro_id == "macro.parent")
            .unwrap();
        assert_eq!(parent.steps[0].parameters.target.as_deref(), Some("vscode"));
        assert_eq!(
            parent.steps[1].parameters.nested_macro_id.as_deref(),
            Some("macro.child")
        );
    }

    #[test]
    fn imports_single_macro_mac_package() {
        let json = r#"{
  "format": "sayall-transfer",
  "schemaVersion": "1.0",
  "packageID": "example.macro.1",
  "version": "1.0.0",
  "requirements": {
    "minimumReaderVersion": "1.0",
    "minimumRemoteMicVersion": "1.9.21",
    "capabilities": []
  },
  "exportPurpose": "share",
  "type": "macro",
  "macro": {
    "schemaVersion": "1.0",
    "macroID": "macro.mac.1",
    "version": "1.0.0",
    "name": "Mac 动作",
    "steps": [
      {
        "stepID": "step.01",
        "action": "openApplication",
        "parameters": {"bundleIdentifier": "com.google.Chrome"}
      },
      {
        "stepID": "step.02",
        "action": "builtInAction",
        "parameters": {"builtInAction": "playPause"}
      }
    ]
  },
  "dependencies": {"macros": [], "shortcuts": [], "focusTargets": [], "applications": [{"id": "app.chrome", "bundleIdentifier": "com.google.Chrome", "displayName": "Chrome", "focusStrategy": "none"}]}
}"#;
        let imported = import_portable_library(json.as_bytes()).unwrap();
        assert_eq!(imported.definitions.len(), 1);
        assert_eq!(imported.definitions[0].macro_id, "macro.mac.1");
        assert_eq!(
            imported.definitions[0].steps[0]
                .parameters
                .target
                .as_deref(),
            Some("chrome")
        );
        assert_eq!(
            imported.definitions[0].steps[1]
                .parameters
                .chord
                .as_ref()
                .unwrap()
                .key,
            "media_play_pause"
        );
    }

    #[test]
    fn rejects_windows_only_focus_and_unsupported_mac_actions() {
        let focus = library(vec![definition(
            "macro.focus",
            vec![step(
                MacroAction::FocusInput,
                MacroParameters {
                    target: Some("vscode".to_owned()),
                    ..MacroParameters::default()
                },
            )],
        )]);
        assert_eq!(
            export_portable_library(&focus).unwrap_err().code(),
            "unsupported_action"
        );
        let mac_script = r#"{
  "format": "sayall-transfer",
  "schemaVersion": "1.0",
  "packageID": "example.script",
  "version": "1.0.0",
  "requirements": {"minimumReaderVersion": "1.0", "minimumRemoteMicVersion": "1.9.21", "capabilities": []},
  "exportPurpose": "share",
  "type": "macro",
  "macro": {
    "schemaVersion": "1.0", "macroID": "macro.script", "version": "1.0.0", "name": "Script",
    "steps": [{"stepID": "step.01", "action": "runShellScript", "parameters": {"scriptSource": "echo"}}]
  }
}"#;
        assert_eq!(
            import_portable_library(mac_script.as_bytes())
                .unwrap_err()
                .code(),
            "unsupported_action"
        );
    }

    #[test]
    fn rejects_non_global_scope_and_unmapped_app() {
        let scoped = r#"{
  "format": "sayall-transfer",
  "schemaVersion": "1.0",
  "packageID": "example.scope",
  "version": "1.0.0",
  "requirements": {"minimumReaderVersion": "1.0", "minimumRemoteMicVersion": "1.9.21", "capabilities": []},
  "exportPurpose": "share",
  "type": "macro",
  "macro": {
    "schemaVersion": "1.0", "macroID": "macro.scope", "version": "1.0.0", "name": "Scoped",
    "scope": {"kind": "application", "bundleIdentifiers": ["com.apple.Safari"]},
    "steps": [{"stepID": "step.01", "action": "openURL", "parameters": {"urlString": "https://example.com"}}]
  }
}"#;
        assert_eq!(
            import_portable_library(scoped.as_bytes())
                .unwrap_err()
                .code(),
            "unsupported_scope"
        );
        let unmapped = library(vec![definition(
            "macro.app",
            vec![step(
                MacroAction::OpenApplication,
                MacroParameters {
                    target: Some("C:\\Tools\\app.exe".to_owned()),
                    ..MacroParameters::default()
                },
            )],
        )]);
        assert_eq!(
            export_portable_library(&unmapped).unwrap_err().code(),
            "unsupported_action"
        );
    }

    #[test]
    fn rejects_non_macro_backup_content() {
        let json = r#"{
  "format": "sayall-transfer",
  "schemaVersion": "1.0",
  "packageID": "example.backup",
  "version": "1.0.0",
  "requirements": {"minimumReaderVersion": "1.0", "minimumRemoteMicVersion": "1.9.21", "capabilities": []},
  "exportPurpose": "personalBackup",
  "type": "personalBackup",
  "backup": {
    "roots": [{"kind": "macro", "id": "macro.aaaaaaa"}],
    "macros": [{"schemaVersion": "1.0", "macroID": "macro.aaaaaaa", "version": "1.0.0", "name": "A", "steps": [{"stepID": "step.01", "action": "openURL", "parameters": {"urlString": "https://example.com"}}]}],
    "shortcuts": [], "focusTargets": [], "applications": [], "buttonProfiles": [],
    "hostSettings": {"id": "host.settings"}
  }
}"#;
        assert_eq!(
            import_portable_library(json.as_bytes()).unwrap_err().code(),
            "unsupported_content"
        );
    }
}
