import type { Component } from "vue";
import type { NavIcon } from "../navigation";

/**
 * 私有功能页的注册表。
 *
 * 公开构建保持本文件原样：**空数组**，侧栏与页面表都只有公开的四页
 * （仓库规则：未实现的页面不显示空入口）。
 *
 * 私有构建在打包前用同名文件替换本文件内容，导出私有页面（例如会员页）。
 * 契约：
 *
 * 1. 只依赖本文件与 `../navigation` 的公开类型；
 * 2. `id` 不得与公开页面冲突（`buttons` / `connection` / `permissions` / `settings`）；
 * 3. 页面组件自行负责 IPC、文案与失败态，公开代码不感知其存在。
 *
 * 见 `docs/architecture/private-component-boundary.md`。
 */
export interface PrivateFeaturePage {
  id: string;
  label: string;
  icon: NavIcon;
  component: Component;
  /** 插入到该公开页面之后；缺省/未命中时追加到末尾。 */
  after?: string;
}

export const privateFeaturePages: readonly PrivateFeaturePage[] = [];

/** `activePage` 到组件的映射片段，与公开页面表合并后交给主界面。 */
export function privatePageComponents(): Record<string, Component> {
  return Object.fromEntries(
    privateFeaturePages.map((page) => [page.id, page.component]),
  );
}
