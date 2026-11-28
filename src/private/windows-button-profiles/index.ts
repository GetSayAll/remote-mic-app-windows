// 键位方案可选组件的**公开空壳**（前端侧）。
//
// 正式 UI 在私有仓：
// `sayall-private-platform/packages/windows-button-profiles/ui/`
// 构建期由该包的注入脚本覆盖本目录。未注入时：
// - `profilePageComponent()` 返回 null → 侧栏不显示入口；
// - `accessDecision()` 返回 `unavailable` → 页面不可进入；
// - `createButtonProfileScope()` 返回 null → 按键页保持原有行为。
//
// 公开侧不得复制私有 UI 补齐空缺。

import type { Component, Ref } from "vue";
import type {
  ButtonAction,
  ButtonActions,
  ButtonTrigger,
  RemoteButton,
} from "../../lib/bridge";

/** 空壳标识：注入脚本与发布校验用它区分「公开空壳」与「私有实现」。 */
export const COMPONENT_MARKER = "windows-button-profiles/stub";

/** 键位方案页组件；空壳返回 null。 */
export function profilePageComponent(): Component | null {
  return null;
}

/** 页面访问决策；空壳恒为 `unavailable`。 */
export function accessDecision(): "unavailable" {
  return "unavailable";
}

export type ProfileScopeAccess =
  | "unavailable"
  | "requiresPlus"
  | "temporarilyOffline"
  | "allowed";

export interface ProfileScopeProfile {
  id: string;
  name: string;
}

export type ProfileBindingState =
  | "inherit"
  | "explicit"
  | "disabled"
  | "unavailable";

export interface ButtonProfileScopeState {
  available: boolean;
  loading: boolean;
  error: string | null;
  access: ProfileScopeAccess;
  profiles: ProfileScopeProfile[];
  activeProfileId: string | null;
  selectedProfileId: string | null;
  editing: boolean;
  saveState: "idle" | "saving" | "saved" | "failed";
}

/** 按键页作用域的稳定接缝；公开构建没有私有组件，返回 null。 */
export interface ButtonProfileScope {
  state: Ref<ButtonProfileScopeState>;
  load(): Promise<void>;
  select(profileId: string | null): void;
  beginEdit(): boolean;
  endEdit(): void;
  supportsButton(button: RemoteButton): boolean;
  displayActions(button: RemoteButton, base: ButtonActions): ButtonActions;
  bindingState(
    button: RemoteButton,
    trigger: ButtonTrigger,
  ): ProfileBindingState;
  writeAction(
    button: RemoteButton,
    trigger: ButtonTrigger,
    action: ButtonAction,
  ): Promise<void>;
  clearAction(button: RemoteButton, trigger: ButtonTrigger): Promise<void>;
  writeHold(
    button: RemoteButton,
    hold: "single" | "long" | "singleHold" | "longHold" | null,
  ): Promise<void>;
  createFromBase(
    name: string,
    entries: Array<{
      button: RemoteButton;
      trigger: ButtonTrigger;
      action: ButtonAction;
    }>,
    holds?: Array<{
      button: RemoteButton;
      hold: "single" | "long" | "singleHold" | "longHold";
    }>,
  ): Promise<string>;
  copyProfile(sourceProfileId: string, name: string): Promise<string>;
}

export function createButtonProfileScope(): ButtonProfileScope | null {
  return null;
}
