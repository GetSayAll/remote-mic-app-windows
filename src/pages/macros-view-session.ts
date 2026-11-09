import type { MacroDefinition } from "../lib/bridge";

/**
 * 组合动作页的跨挂载快照（2026-11-06）。
 *
 * 侧栏切页会重建页面；库读取是异步 IPC，重建后的首帧如果从空列表开始，
 * 用户会看到「我的组合动作 0」闪一下再变成真实条数。这里保存上一次读取到的
 * 去重后列表，挂载首帧直接渲染，随后 `reloadLibrary()` 仍照常静默刷新。
 *
 * 只缓存展示数据，不改变任何生命周期；页面卸载/切走时的录入清理逻辑不受影响。
 */
export const macrosViewSession: { definitions: MacroDefinition[] | null } = {
  definitions: null,
};
