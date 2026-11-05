import type { AgentAccessSnapshot } from "../lib/agent-access";
import type { TranscriptHistorySnapshot } from "../lib/bridge";
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
  /**
   * 最近一次读到的记录快照（仅内存，不落盘）。
   *
   * 切换侧边栏会卸载页面，重新挂载时先用它同步渲染上一次的内容，再在后台静默
   * 刷新——否则每次切回都从空白加载态开始，看起来像整页刷新、开关也会滑一下。
   */
  historySnapshot: TranscriptHistorySnapshot | null;
  /**
   * 最近一次读到的「本地 Agent 访问」快照（仅内存，不落盘）。
   *
   * 与 historySnapshot 同理：卡片每次挂载仍会现读，缓存只负责第一帧——否则
   * 重挂载先按空快照渲染（区块收起、授权列表空态），读回后整卡撑开，看起来
   * 闪一下、页面高度跳变。
   */
  agentAccessSnapshot: AgentAccessSnapshot | null;
  /** 离开页面时 `.content` 的滚动位置；重挂载后恢复。 */
  scrollTop: number;
}

function emptySession(): ReflectionsViewSession {
  return {
    selectedApplicationKey: null,
    applicationSwitcherExpanded: false,
    expandedDayKeys: new Set<string>(),
    dayExpansionSettled: false,
    historySnapshot: null,
    agentAccessSnapshot: null,
    scrollTop: 0,
  };
}

/**
 * 回眸页的浏览状态（跨侧边栏切换保留）。
 *
 * 放在模块级而不是组件内：`App.vue` 用 `<component :is>` 渲染页面，切走会卸载
 * `ReflectionsPage`，组件内 `ref` 随之销毁——选中的应用、折叠的日期、展开的应用栏
 * 都会在切回时回到默认值。这里保留"看哪儿"、最近一次快照与滚动位置；快照
 * 只是第一帧的缓存，每次挂载都会重新读取，保证与 Rust 侧一致。
 */
export const reflectionsViewSession = reactive<ReflectionsViewSession>(emptySession());

/** 测试隔离用：模块级状态会跨挂载存活，每个用例开始前必须清回默认值。 */
export function resetReflectionsViewSessionForTests(): void {
  Object.assign(reflectionsViewSession, emptySession());
}
