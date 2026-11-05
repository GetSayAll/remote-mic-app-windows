import { reactive } from "vue";

export interface ReflectionsViewSession {
  /** 当前选中的应用；`null` = 全部应用。 */
  selectedApplicationKey: string | null;
  /** 应用栏是否展开成网格。 */
  applicationSwitcherExpanded: boolean;
  /** 已展开的日期分组 key（文字分组与"仅录音"分组共用同一命名空间）。 */
  expandedDayKeys: Set<string>;
  /**
   * 展开集合是否已经确定过。
   *
   * 空集合有两种含义：「还没展开过」和「用户把全部日期折叠了」。只有前者才允许
   * 默认展开最新一天，否则用户折叠后切走再回来会被重新展开。
   */
  dayExpansionSettled: boolean;
}

function emptySession(): ReflectionsViewSession {
  return {
    selectedApplicationKey: null,
    applicationSwitcherExpanded: false,
    expandedDayKeys: new Set<string>(),
    dayExpansionSettled: false,
  };
}

/**
 * 回眸页的浏览状态（跨侧边栏切换保留）。
 *
 * 放在模块级而不是组件内：`App.vue` 用 `<component :is>` 渲染页面，切走会卸载
 * `ReflectionsPage`，组件内 `ref` 随之销毁——选中的应用、折叠的日期、展开的应用栏
 * 都会在切回时回到默认值。这里只保留"看哪儿"，不缓存记录快照：重进仍然重新读取，
 * 新记录不会因为缓存而看不到。
 */
export const reflectionsViewSession = reactive<ReflectionsViewSession>(emptySession());

/** 测试隔离用：模块级状态会跨挂载存活，每个用例开始前必须清回默认值。 */
export function resetReflectionsViewSessionForTests(): void {
  Object.assign(reflectionsViewSession, emptySession());
}
