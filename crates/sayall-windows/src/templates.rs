//! Structured mapping-template configuration.
//!
//! This module deliberately only describes persisted user intent.  Resolving a
//! foreground application or injecting an action stays in the platform runtime.

use crate::send_input::ButtonMappings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMPLATE_ID_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// One fixed-key mapping model for every built-in and user template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MappingTemplate {
    pub id: String,
    pub name: String,
    pub mappings: ButtonMappings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationBinding {
    pub application_id: String,
    pub template_id: String,
    #[serde(default)]
    pub menu_order: u32,
    /// Local-only launch target.  It is intentionally omitted from exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_target: Option<String>,
}

pub type ButtonMappingTemplate = MappingTemplate;

pub const BUILTIN_AGENT_TEMPLATE_ID: &str = "preset-agent";
pub const BUILTIN_CHAT_TEMPLATE_ID: &str = "preset-chat";
pub const BUILTIN_BROWSER_TEMPLATE_ID: &str = "preset-browser";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCatalogKind {
    Direct,
}

/// Effective catalog: stable built-in identities may have explicitly saved edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateCatalogEntry {
    pub id: String,
    pub name: String,
    pub kind: TemplateCatalogKind,
    pub built_in: bool,
    pub button_mappings: Option<ButtonMappings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportApplicationBinding {
    pub application_id: String,
    pub template_id: String,
    pub menu_order: u32,
}

impl From<&ApplicationBinding> for ExportApplicationBinding {
    fn from(binding: &ApplicationBinding) -> Self {
        Self {
            application_id: binding.application_id.clone(),
            template_id: binding.template_id.clone(),
            menu_order: binding.menu_order,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingConfiguration {
    #[serde(default)]
    pub menu_update_default: bool,
    #[serde(default)]
    pub menu_template_switch_enabled: bool,
    #[serde(default = "mapping_notice_default")]
    pub mapping_notice_enabled: bool,
    #[serde(default)]
    pub common_mappings: ButtonMappings,
    #[serde(default)]
    pub templates: Vec<MappingTemplate>,
    #[serde(default)]
    pub application_bindings: Vec<ApplicationBinding>,
    /// Application-specific ordinary-button profiles are opt-in. Bindings are
    /// retained while disabled so turning the feature back on restores the
    /// user's explicit associations without rewriting them.
    #[serde(default)]
    pub button_mapping_follow_enabled: bool,
}

/// A share-safe import preview. The token retains local launch resolutions;
/// the displayed configuration contains logical references and no local paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingConfigurationImportPreview {
    pub token: String,
    /// Only this source token can be used to prepare selected-template imports.
    /// It is never an apply token.
    pub source_token: Option<String>,
    pub format_version: u32,
    pub configuration: MappingConfiguration,
    pub builtin_template_ids: Vec<String>,
    pub template_name_conflicts: Vec<String>,
    pub unresolved_application_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateImportRequest {
    #[serde(default)]
    pub template_ids: Vec<String>,
    #[serde(default)]
    pub resolved_names: BTreeMap<String, String>,
    #[serde(default)]
    pub replace_application_bindings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTemplatePreview {
    pub source_template_id: String,
    pub template: MappingTemplate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateImportPreview {
    pub token: String,
    pub templates: Vec<ImportedTemplatePreview>,
    pub added_application_bindings: Vec<ExportApplicationBinding>,
    pub replaced_application_ids: Vec<String>,
    pub skipped_application_ids: Vec<String>,
    pub unresolved_application_ids: Vec<String>,
}

impl Default for MappingConfiguration {
    fn default() -> Self {
        Self {
            menu_update_default: false,
            menu_template_switch_enabled: false,
            mapping_notice_enabled: true,
            common_mappings: ButtonMappings::default(),
            templates: Vec::new(),
            application_bindings: Vec::new(),
            button_mapping_follow_enabled: false,
        }
    }
}

fn mapping_notice_default() -> bool {
    true
}

impl MappingConfiguration {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.common_mappings = self
            .common_mappings
            .normalized()
            .map_err(|e| format!("通用映射无效：{e}"))?;
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for template in &mut self.templates {
            if template.mappings.actions.values().any(|actions| {
                [&actions.single, &actions.double, &actions.long]
                    .iter()
                    .any(|action| matches!(action, crate::send_input::ButtonAction::OpenApp { .. }))
            }) {
                return Err("模板只支持固定按键或组合键".into());
            }
            template.id = normalized_required(&template.id, "模板 ID")?;
            template.name = normalized_required(&template.name, "模板名称")?;
            if let Some(builtin) = Self::recommended_templates()
                .into_iter()
                .find(|t| t.id == template.id)
            {
                template.name = builtin.name;
            }
            if !ids.insert(template.id.clone()) || !names.insert(template.name.clone()) {
                return Err("模板 ID 或名称重复".to_owned());
            }
            template.mappings = template
                .mappings
                .clone()
                .normalized()
                .map_err(|e| format!("按键模板无效：{e}"))?;
        }
        let mut applications = HashSet::new();
        for binding in &mut self.application_bindings {
            binding.application_id = normalized_required(&binding.application_id, "应用 ID")?;
            binding.template_id = normalized_required(&binding.template_id, "模板 ID")?;
            if !applications.insert(binding.application_id.to_lowercase()) {
                return Err("每个程序只能关联一个默认模板".to_owned());
            }
            if !ids.contains(&binding.template_id) && !is_builtin_template_id(&binding.template_id)
            {
                return Err("应用绑定引用了不存在的模板".to_owned());
            }
            binding.launch_target = binding
                .launch_target
                .take()
                .map(|v| normalized_required(&v, "程序目标"))
                .transpose()?;
        }
        Ok(self)
    }

    pub fn save_button_mapping_template(
        &mut self,
        name: String,
        mappings: ButtonMappings,
    ) -> Result<MappingTemplate, String> {
        let name = normalized_required(&name, "模板名称")?;
        if self.templates.iter().any(|t| t.name == name) {
            return Err("模板名称重复".to_owned());
        }
        let template = MappingTemplate {
            id: new_template_id(),
            name,
            mappings: mappings
                .normalized()
                .map_err(|e| format!("按键模板无效：{e}"))?,
        };
        self.templates.push(template.clone());
        Ok(template)
    }
    pub fn create_template(&mut self, name: String) -> Result<MappingTemplate, String> {
        self.save_button_mapping_template(name, ButtonMappings::default())
    }
    pub fn duplicate_template(
        &mut self,
        id: &str,
        name: String,
    ) -> Result<MappingTemplate, String> {
        let mappings = self
            .template_mappings(id)
            .ok_or_else(|| "模板不存在".to_owned())?;
        self.save_button_mapping_template(name, mappings)
    }
    pub fn update_button_mapping_template(
        &mut self,
        id: &str,
        mappings: ButtonMappings,
    ) -> Result<MappingTemplate, String> {
        let mappings = mappings
            .normalized()
            .map_err(|e| format!("按键模板无效：{e}"))?;
        if !self.templates.iter().any(|t| t.id == id) {
            let builtin = Self::recommended_templates()
                .into_iter()
                .find(|t| t.id == id)
                .ok_or_else(|| "模板不存在".to_owned())?;
            self.templates.push(builtin);
        }
        let template = self
            .templates
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| "模板不存在".to_owned())?;
        template.mappings = mappings;
        Ok(template.clone())
    }
    pub fn reset_builtin_template(&mut self, id: &str) -> Result<MappingTemplate, String> {
        let builtin = Self::recommended_templates()
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "只能复位默认模板".to_owned())?;
        self.templates.retain(|t| t.id != id);
        Ok(builtin)
    }
    pub fn template_mappings(&self, id: &str) -> Option<ButtonMappings> {
        self.templates
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.mappings.clone())
            .or_else(|| {
                Self::recommended_templates()
                    .into_iter()
                    .find(|t| t.id == id)
                    .map(|t| t.mappings)
            })
    }
    pub fn upsert_application_binding(
        &mut self,
        mut binding: ApplicationBinding,
    ) -> Result<(), String> {
        if self.template_mappings(&binding.template_id).is_none() {
            return Err("模板不存在".to_owned());
        }
        binding.menu_order = self
            .application_bindings
            .iter()
            .find(|b| {
                b.application_id
                    .eq_ignore_ascii_case(&binding.application_id)
            })
            .map(|b| b.menu_order)
            .unwrap_or_else(|| {
                self.application_bindings
                    .iter()
                    .map(|b| b.menu_order)
                    .max()
                    .map_or(0, |i| i.saturating_add(1))
            });
        self.remove_application_binding(&binding.application_id);
        self.application_bindings.push(binding);
        Ok(())
    }
    pub fn remove_application_binding(&mut self, application_id: &str) {
        self.application_bindings
            .retain(|b| !b.application_id.eq_ignore_ascii_case(application_id));
    }
    pub fn reorder_application_associations(&mut self, ids: Vec<String>) -> Result<(), String> {
        let requested: HashSet<_> = ids.iter().map(|id| id.trim().to_lowercase()).collect();
        let existing: HashSet<_> = self
            .application_bindings
            .iter()
            .map(|b| b.application_id.to_lowercase())
            .collect();
        if requested.len() != ids.len() || requested != existing {
            return Err("程序关联顺序必须完整包含当前全部且不得重复".to_owned());
        }
        for (i, id) in ids.iter().enumerate() {
            self.application_bindings
                .iter_mut()
                .find(|b| b.application_id.eq_ignore_ascii_case(id.trim()))
                .unwrap()
                .menu_order = i as u32;
        }
        Ok(())
    }
    pub fn recommended_templates() -> Vec<MappingTemplate> {
        [
            (BUILTIN_AGENT_TEMPLATE_ID, "Agent"),
            (BUILTIN_CHAT_TEMPLATE_ID, "聊天工具"),
            (BUILTIN_BROWSER_TEMPLATE_ID, "浏览器"),
        ]
        .into_iter()
        .map(|(id, name)| MappingTemplate {
            id: id.into(),
            name: name.into(),
            mappings: fixed_keys(),
        })
        .collect()
    }
    pub fn template_catalog(&self) -> Vec<TemplateCatalogEntry> {
        Self::recommended_templates()
            .into_iter()
            .map(|builtin| {
                self.templates
                    .iter()
                    .find(|t| t.id == builtin.id)
                    .cloned()
                    .unwrap_or(builtin)
            })
            .chain(
                self.templates
                    .iter()
                    .filter(|t| !is_builtin_template_id(&t.id))
                    .cloned(),
            )
            .map(|t| TemplateCatalogEntry {
                built_in: is_builtin_template_id(&t.id),
                id: t.id,
                name: t.name,
                kind: TemplateCatalogKind::Direct,
                button_mappings: Some(t.mappings),
            })
            .collect()
    }
    pub fn copy_template_catalog_entry(
        &mut self,
        id: &str,
        name: String,
    ) -> Result<TemplateCatalogEntry, String> {
        let template = self.duplicate_template(id, name)?;
        Ok(TemplateCatalogEntry {
            id: template.id,
            name: template.name,
            kind: TemplateCatalogKind::Direct,
            built_in: false,
            button_mappings: Some(template.mappings),
        })
    }
    pub fn apply_template_preset(
        &mut self,
        id: &str,
        name: String,
    ) -> Result<MappingTemplate, String> {
        if !is_builtin_template_id(id) {
            return Err("推荐模板不存在".to_owned());
        }
        self.duplicate_template(id, name)
    }
}

/// Fixed keyboard actions; system task selection uses public shell shortcuts.
fn fixed_keys() -> ButtonMappings {
    use crate::raw_input::RemoteButton;
    use crate::send_input::{ButtonAction, ButtonActions, KeyChord, KeyCode};
    let mut mappings = ButtonMappings {
        enabled: true,
        applications: Vec::new(),
        focus_profiles: BTreeMap::new(),
        actions: BTreeMap::new(),
    };
    for (button, keys) in [
        (RemoteButton::Up, vec![KeyCode::Up]),
        (RemoteButton::Down, vec![KeyCode::Down]),
        (RemoteButton::Left, vec![KeyCode::Left]),
        (RemoteButton::Right, vec![KeyCode::Right]),
        (RemoteButton::Back, vec![KeyCode::Backspace]),
        (RemoteButton::Ok, vec![KeyCode::Enter]),
        (RemoteButton::VolumeUp, vec![KeyCode::VolumeUp]),
        (RemoteButton::VolumeDown, vec![KeyCode::VolumeDown]),
        (RemoteButton::Power, vec![KeyCode::Escape]),
    ] {
        mappings.actions.insert(
            button,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord { keys },
                },
                ..Default::default()
            },
        );
    }
    let shortcut = |keys| ButtonAction::Shortcut {
        chord: KeyChord { keys },
    };
    mappings.actions.insert(
        RemoteButton::Home,
        ButtonActions {
            single: shortcut(vec![KeyCode::Home]),
            double: shortcut(vec![KeyCode::Control, KeyCode::Home]),
            long: shortcut(vec![KeyCode::Control, KeyCode::End]),
        },
    );
    mappings.actions.insert(
        RemoteButton::Tv,
        ButtonActions {
            single: shortcut(vec![KeyCode::Tab]),
            double: ButtonAction::Disabled,
            long: ButtonAction::TaskSwitch {
                view: crate::send_input::TaskSwitchView::Desktops,
            },
        },
    );
    mappings
}

pub fn is_builtin_template_id(id: &str) -> bool {
    matches!(
        id,
        BUILTIN_AGENT_TEMPLATE_ID | BUILTIN_CHAT_TEMPLATE_ID | BUILTIN_BROWSER_TEMPLATE_ID
    )
}

pub fn reject_builtin_template_mutation(id: &str) -> Result<(), String> {
    if is_builtin_template_id(id) {
        Err("默认模板的名称和身份固定，不能重命名或删除；可以编辑或复位映射".to_owned())
    } else {
        Ok(())
    }
}

fn normalized_required(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field}不能为空"))
    } else {
        Ok(value.to_owned())
    }
}

pub fn new_template_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEMPLATE_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("template-{nanos:x}-{sequence:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_edits_keep_identity_and_bindings_and_survive_reload() {
        let mut c = MappingConfiguration::default();
        c.upsert_application_binding(ApplicationBinding {
            application_id: "wechat".into(),
            template_id: BUILTIN_CHAT_TEMPLATE_ID.into(),
            menu_order: 0,
            launch_target: None,
        })
        .unwrap();
        let copy = c
            .duplicate_template(BUILTIN_CHAT_TEMPLATE_ID, "My copy".into())
            .unwrap();
        let bindings = c.application_bindings.clone();
        let changed = ButtonMappings::default();
        let saved = c
            .update_button_mapping_template(BUILTIN_CHAT_TEMPLATE_ID, changed.clone())
            .unwrap();
        assert_eq!(saved.id, BUILTIN_CHAT_TEMPLATE_ID);
        assert_eq!(
            c.template_mappings(BUILTIN_CHAT_TEMPLATE_ID),
            Some(changed.clone())
        );
        let reopened: MappingConfiguration =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        let reopened = reopened.normalized().unwrap();
        assert_eq!(reopened.application_bindings, bindings);
        assert_eq!(reopened.template_mappings(&copy.id), Some(copy.mappings));
        let catalog = reopened.template_catalog();
        assert_eq!(
            catalog
                .iter()
                .filter(|t| t.id == BUILTIN_CHAT_TEMPLATE_ID)
                .count(),
            1
        );
        assert_eq!(
            catalog
                .iter()
                .find(|t| t.id == BUILTIN_CHAT_TEMPLATE_ID)
                .unwrap()
                .button_mappings,
            Some(changed)
        );
        assert_eq!(catalog.len(), 4);
    }

    #[test]
    fn reset_default_preserves_other_edits_and_all_associations() {
        let mut c = MappingConfiguration::default();
        let user = c.create_template("Mine".into()).unwrap();
        c.upsert_application_binding(ApplicationBinding {
            application_id: "wechat".into(),
            template_id: BUILTIN_CHAT_TEMPLATE_ID.into(),
            menu_order: 0,
            launch_target: None,
        })
        .unwrap();
        c.update_button_mapping_template(BUILTIN_CHAT_TEMPLATE_ID, ButtonMappings::default())
            .unwrap();
        c.update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, ButtonMappings::default())
            .unwrap();
        let bindings = c.application_bindings.clone();
        c.reset_builtin_template(BUILTIN_CHAT_TEMPLATE_ID).unwrap();
        assert_eq!(c.application_bindings, bindings);
        assert_eq!(c.template_mappings(&user.id), Some(user.mappings));
        assert_eq!(
            c.template_mappings(BUILTIN_AGENT_TEMPLATE_ID),
            Some(ButtonMappings::default())
        );
        let expected = MappingConfiguration::default().template_mappings(BUILTIN_CHAT_TEMPLATE_ID);
        assert_eq!(c.template_mappings(BUILTIN_CHAT_TEMPLATE_ID), expected);
        let before = c.clone();
        assert!(c.reset_builtin_template("missing").is_err());
        assert_eq!(c, before);
    }

    #[test]
    fn defaults_keep_fast_keys_immediate_and_provide_useful_home_tv_gestures() {
        use crate::raw_input::RemoteButton;
        use crate::send_input::{ButtonAction, KeyChord, KeyCode};
        for template in MappingConfiguration::recommended_templates() {
            assert_eq!(
                template
                    .mappings
                    .action_for(RemoteButton::Ok, crate::send_input::ButtonTrigger::Single),
                ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Enter]
                    }
                }
            );
            for button in [
                RemoteButton::Ok,
                RemoteButton::Left,
                RemoteButton::Right,
                RemoteButton::Up,
                RemoteButton::Down,
                RemoteButton::Back,
                RemoteButton::VolumeUp,
                RemoteButton::VolumeDown,
            ] {
                let actions = template.mappings.actions(button);
                assert_eq!(actions.double, ButtonAction::Disabled);
                assert_eq!(actions.long, ButtonAction::Disabled);
            }
            let home = template.mappings.actions(RemoteButton::Home);
            assert_eq!(
                home.double,
                ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Control, KeyCode::Home]
                    }
                }
            );
            assert_eq!(
                home.long,
                ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Control, KeyCode::End]
                    }
                }
            );
            let tv = serde_json::to_value(template.mappings.actions(RemoteButton::Tv)).unwrap();
            assert_eq!(
                tv["single"],
                serde_json::json!({"type":"shortcut","chord":{"keys":["tab"]}})
            );
            assert_eq!(
                tv["long"],
                serde_json::json!({"type":"task_switch","view":"desktops"})
            );
            assert_eq!(tv["double"], serde_json::json!({"type":"disabled"}));
            assert!(!template.mappings.actions.contains_key(&RemoteButton::Menu));
        }
    }

    #[test]
    fn builtin_wire_fixture_matches_production_fixed_keys() {
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/ipc/template-catalog-builtins.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::to_value(MappingConfiguration::default().template_catalog()).unwrap(),
            expected
        );
    }
    #[test]
    fn every_template_uses_one_fixed_key_model_and_preserves_missing_actions() {
        use crate::raw_input::RemoteButton;
        let mut c = MappingConfiguration::default();
        let copied = c
            .duplicate_template(BUILTIN_AGENT_TEMPLATE_ID, "Copy".into())
            .unwrap();
        assert_eq!(c.template_catalog().len(), 4);
        assert!(c
            .template_catalog()
            .iter()
            .all(|t| t.button_mappings.is_some()));
        assert!(copied.mappings.actions.contains_key(&RemoteButton::Home));
        assert!(copied.mappings.actions.contains_key(&RemoteButton::Tv));
        assert_eq!(
            copied.mappings.actions[&RemoteButton::Ok].long,
            crate::send_input::ButtonAction::Disabled
        );
        let raw = serde_json::to_string(&c).unwrap();
        assert!(!raw.contains("regionActions"));
        assert_eq!(
            serde_json::from_str::<MappingConfiguration>(&raw)
                .unwrap()
                .normalized()
                .unwrap(),
            c
        );
        assert!(serde_json::from_str::<MappingTemplate>(
            r#"{"id":"x","name":"x","regionActions":{}}"#
        )
        .is_err());
    }
    #[test]
    fn one_default_per_program_preserves_template_and_binding_order() {
        let mut c = MappingConfiguration::default();
        let t = c.create_template("Empty".into()).unwrap();
        for id in [BUILTIN_AGENT_TEMPLATE_ID, t.id.as_str()] {
            c.upsert_application_binding(ApplicationBinding {
                application_id: "codex".into(),
                template_id: id.into(),
                menu_order: 99,
                launch_target: None,
            })
            .unwrap();
        }
        assert_eq!(c.application_bindings.len(), 1);
        assert_eq!(c.application_bindings[0].template_id, t.id);
        assert!(c.clone().normalized().is_ok());
        assert!(c
            .update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, ButtonMappings::default())
            .is_ok());
        let mut duplicate = c.clone();
        duplicate
            .application_bindings
            .push(c.application_bindings[0].clone());
        assert!(duplicate.normalized().is_err());
        assert!(c
            .reorder_application_associations(vec!["missing".into()])
            .is_err());
    }
}
