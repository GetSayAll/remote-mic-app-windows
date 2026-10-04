/**
 * 前端功能开关。放独立模块是为了让页面与测试读**同一个**事实来源：
 * 入口隐藏时，对应的功能用例经 `it.skipIf` 一并挂起，重新放开开关时
 * 测试自动恢复，不会出现"入口回来了、用例还在 skip"的漂移。
 */

/**
 * 连接页「修改快捷键」自定义录入入口。
 *
 * 2026-09-28 Andy 决定：该功能有问题，先隐藏入口，后续研究新方案后再放开。
 * 录入逻辑本身保留（`beginVoiceHotkeyCapture` 一族），「默认」「关闭」两个
 * 预设按钮不受影响，用户仍可一键回到默认组合或关闭快捷键。
 */
export const VOICE_HOTKEY_CUSTOM_CAPTURE_ENABLED = false;

/**
 * 按键编辑器「设备操作 / 聚焦输入框」入口。
 *
 * 2026-10-03 用户要求：先隐藏入口。动作类型与平台受理路径保留（已有映射照常生效，
 * runtime simulation 改为经 IPC 直接写入并验证受理），只是 UI 暂不暴露入口。
 */
export const DEVICE_ACTION_SECTION_ENABLED = false;

/**
 * 首次使用向导（Onboarding）激活开关。
 *
 * 流程未完整（步骤④–⑦、staged 事务、attempt 唯一终态）且真机探针未通过前保持
 * **关闭**：关闭时 App 不读取向导状态、行为与现状完全一致。向导本体、状态机与
 * 迁移逻辑已实现并有独立测试；全部就绪后打开此开关即可对未完成的用户生效。
 */
export const ONBOARDING_WIZARD_ENABLED = false;
