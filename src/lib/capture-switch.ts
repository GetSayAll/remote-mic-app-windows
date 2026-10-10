/**
 * 「支持更多输入工具」/「全按键支持」开关的状态与失败文案（2026-10-10 Andy 定稿）。
 *
 * 设计契约（写进 docs/product-copy.md，以后同类开关一律照此）：
 * - 开关与状态提示**合并成一个开关**：不再有独立的状态字、状态圆点或状态胶囊；
 * - 开关只有三种状态：关闭 / 开启 / 正在操作（进行中由开关自身的置灰 + 转圈表达，
 *   不配任何文字）；
 * - **只有开启失败（或关闭失败）才显示文案**，且必须是普通用户能读懂的原因：
 *   不出现 UAC、计划的 task、退出码、可执行文件名等内部细节（原文只进诊断日志）；
 * - 功能启动中（助手还在连）不显示文案——正在操作由开关态表达。
 */

/** 开关的可见状态：不可用 / 正在操作 / 开启 / 关闭。 */
export type CaptureSwitchState = "unavailable" | "busy" | "on" | "off";

/**
 * 开关状态判定（纯函数，单测钉住三态语义）。
 *
 * - `unavailable`：本机根本用不了（ARM64 无载荷，issue #206）——置灰；
 * - `busy`：正在操作（开启/关闭进行中）——置灰 + 转圈，无文字；
 * - `on` / `off`：可点，无文字。
 *
 * 未读到（`enabled === null`）按关闭处理：界面此时渲染同尺寸占位符，
 * 不是开关本体（2026-09-28 的无动画挂载约定）。
 */
export function captureSwitchState(input: {
  enabled: boolean | null;
  busy: boolean;
  unsupported: boolean;
}): CaptureSwitchState {
  if (input.unsupported) return "unavailable";
  if (input.busy) return "busy";
  return input.enabled === true ? "on" : "off";
}

/**
 * 失败原文 → 普通用户能读懂的一句话（`name` 由调用方按本页名称传入：
 * 按键页「全按键支持」、连接页“支持更多输入工具”）。
 *
 * 返回 `null` = 没有失败要显示（其余情况一律不显示文案）。原文映射不到的
 * 一律回落通用句——技术细节留在诊断日志，不摊给用户。
 */
export function captureFailureText(
  raw: string | null | undefined,
  name: string,
): string | null {
  const text = (raw ?? "").trim();
  if (!text) return null;
  if (text.includes("授权") || text.includes("UAC") || text.includes("提权")) {
    return `没有完成系统授权，${name}保持关闭。`;
  }
  if (text.includes("暂不支持")) {
    return `这台电脑暂不支持${name}。`;
  }
  if (text.includes("缺少") && text.includes("组件")) {
    return `安装包缺少本机需要的组件，${name}暂不可用；请重新安装本版本。`;
  }
  if (text.includes("停用") || text.includes("关闭")) {
    return `关闭没有完成，${name}仍是开启状态。`;
  }
  return `开启没有成功，${name}保持关闭。`;
}
