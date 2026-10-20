// 常用语可选组件的**公开空壳**（前端侧）。
//
// 正式 UI（圆盘面板、常用语管理页）在私有仓库
// `sayall-private-platform/packages/windows-common-phrases/ui/`，
// 构建期由该包的注入脚本覆盖本目录。未注入时：
// - `panelComponent()` 返回 null → 浮层窗口页面渲染空容器（宿主也不会打开该窗口）；
// - `settingsComponent()` 返回 null → 设置页不显示常用语入口。
//
// 公开侧不得复制私有 UI 补齐空缺。
import type { Component } from "vue";

/** 空壳标识：注入脚本与发布校验用它区分「公开空壳」与「私有实现」。 */
export const COMPONENT_MARKER = "windows-common-phrases/stub";

/** 浮层窗口要挂载的面板组件；空壳返回 null。 */
export function panelComponent(): Component | null {
  return null;
}

/** 设置页里的常用语页组件；空壳返回 null。 */
export function settingsComponent(): Component | null {
  return null;
}
