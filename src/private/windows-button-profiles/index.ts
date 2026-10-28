// 键位方案可选组件的**公开空壳**（前端侧）。
//
// 正式 UI 在私有仓：
// `sayall-private-platform/packages/windows-button-profiles/ui/`
// 构建期由该包的注入脚本覆盖本目录。未注入时：
// - `profilePageComponent()` 返回 null → 侧栏不显示入口；
// - `accessDecision()` 返回 `unavailable` → 页面不可进入。
//
// 公开侧不得复制私有 UI 补齐空缺。

import type { Component } from "vue";

/** 空壳标识：注入脚本与发布校验用它区分「公开空壳」与「私有实现」。 */
export const COMPONENT_MARKER = "windows-button-profiles/stub";

/** 键位方案页组件；空壳返回 null。 */
export function profilePageComponent(): Component | null {
  return null;
}

/** 页面访问决策；空壳恒为 unavailable。 */
export function accessDecision(): "unavailable" {
  return "unavailable";
}
