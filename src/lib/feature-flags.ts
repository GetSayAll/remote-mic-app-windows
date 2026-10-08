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
 * 2026-10-04：步骤①–⑦、staged 事务、attempt 唯一终态与「重新运行向导」入口
 * 已全部实现；打开开关，供现场验收（遥控器实时语音 + 三种输入工具 + 键盘
 * 阴性对照）使用。验收通过前本分支不合入 main（main 保持关闭）。
 */
export const ONBOARDING_WIZARD_ENABLED = true;
