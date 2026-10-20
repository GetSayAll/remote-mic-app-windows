import { ref } from "vue";
import type { MacroDefinition } from "../lib/bridge";

export type MacroEditorStep = "basic" | "steps" | "review";

export interface MacroEditorSession {
  draft: MacroDefinition;
  isNew: boolean;
  editorStep: MacroEditorStep;
  selectedStepId: string | null;
}

/**
 * 组合动作编辑会话（跨页面切换保留进度，对齐 Mac 端 Settings 标签切换不丢草稿）。
 *
 * 放在模块级而不是组件内：页面切换会卸载 `MacrosPage`，组件内 `ref` 会随之销毁。
 * 保存 / 取消编辑时由页面显式清空；只有"编辑到一半切走"才恢复。
 */
const session = ref<MacroEditorSession | null>(null);

export function readMacroEditorSession(): MacroEditorSession | null {
  return session.value;
}

export function writeMacroEditorSession(value: MacroEditorSession | null): void {
  session.value = value ? (JSON.parse(JSON.stringify(value)) as MacroEditorSession) : null;
}

export function clearMacroEditorSession(): void {
  session.value = null;
}
