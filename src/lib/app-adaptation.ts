import {
  type AppFocusProfile,
  type ButtonActions,
  type ButtonMappings,
  type ButtonTrigger,
  type CustomAppPick,
  type FocusStrategy,
  type MacroDefinition,
  type PresetAppInfo,
  type RemoteButton,
} from "./bridge";
import { presetFocusDefault } from "./focus-defaults";

/** 侧栏「APP 适配」列表的一行。 */
export interface AppAdaptationRow {
  /** `open_app` 的 target：自定义应用路径 / `shell:AppsFolder\...`，或预设应用 id。 */
  target: string;
  name: string;
  /** 自定义应用的库路径，或内置预设探测到的真实可执行文件路径。 */
  path?: string;
  /** 内置预设：只允许清除显式档案，不删除应用库条目。 */
  preset: boolean;
  /** 内置预设是否已安装；自定义应用默认 true。 */
  installed: boolean;
}

export interface AppBindingRef {
  button: RemoteButton;
  trigger: ButtonTrigger;
}

export interface AppMacroReference {
  macroId: string;
  name: string;
}

const TRIGGERS: ButtonTrigger[] = ["single", "double", "long"];

function keyOf(target: string): string {
  return target.trim().toLocaleLowerCase();
}

/** 合并扫描到的应用；路径不区分大小写去重，保留已有顺序与名称。 */
export function mergeApplications(
  existing: CustomAppPick[],
  incoming: CustomAppPick[],
): CustomAppPick[] {
  const unique = new Map<string, CustomAppPick>();
  for (const app of [...existing, ...incoming]) {
    const key = keyOf(app.path);
    if (!key || unique.has(key)) continue;
    unique.set(key, app);
  }
  return [...unique.values()];
}

/**
 * 列表来源：
 * 1. 用户添加的 APP（`applications`，按添加顺序）。
 * 2. 有显式聚焦档案的内置预设（2026-11-09 审计：本机已有网易云音乐一条）。
 *
 * 预设没有 `applications` 条目，但档案一旦存在就必须可见可编辑；否则移除
 * 按键页编辑器后，它会变成隐藏配置。没有档案的预设仍不进入列表。
 */
export function buildAppRows(
  applications: readonly CustomAppPick[],
  profiles: Record<string, AppFocusProfile> | undefined,
  presetApps: readonly PresetAppInfo[],
  extraTargets: readonly string[] = [],
): AppAdaptationRow[] {
  const rows: AppAdaptationRow[] = [];
  const seen = new Set<string>();
  const extras = new Set(extraTargets.map(keyOf));

  for (const app of applications) {
    const target = app.path.trim();
    const key = keyOf(target);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    rows.push({
      target,
      name: app.name.trim() || appFileName(target),
      path: target,
      preset: false,
      installed: true,
    });
  }

  const profileKeys = new Map<string, string>();
  for (const target of Object.keys(profiles ?? {})) {
    profileKeys.set(keyOf(target), target);
  }
  for (const preset of presetApps) {
    const key = keyOf(preset.id);
    if (seen.has(key) || (!profileKeys.has(key) && !extras.has(key))) continue;
    seen.add(key);
    rows.push({
      target: preset.id,
      name: preset.name,
      path: preset.path ?? undefined,
      preset: true,
      installed: preset.installed,
    });
  }

  return rows;
}

/** 自定义应用的兜底名称：文件名去扩展名。 */
export function appFileName(target: string): string {
  const base = target.split(/[\\/]/).pop() ?? target;
  return base.replace(/\.(exe|lnk)$/i, "") || target;
}

/** 按 target 读取档案；路径 target 不区分大小写。 */
export function profileForTarget(
  profiles: Record<string, AppFocusProfile> | undefined,
  target: string,
): AppFocusProfile | null {
  if (!profiles) return null;
  const exact = profiles[target];
  if (exact) return exact;
  const expected = keyOf(target);
  const key = Object.keys(profiles).find((candidate) => keyOf(candidate) === expected);
  return key ? profiles[key] : null;
}

/** 实际会执行的聚焦档案：显式档案优先，预设回落到内置默认。 */
export function effectiveAppProfile(
  row: Pick<AppAdaptationRow, "target" | "preset">,
  profiles: Record<string, AppFocusProfile> | undefined,
): AppFocusProfile | null {
  const explicit = profileForTarget(profiles, row.target);
  if (explicit) return explicit;
  return presetFocusDefault(row.target, row.preset);
}

/** 列表行里的短标签；全名仍由详情和按键页使用。 */
export function compactFocusLabel(profile: AppFocusProfile | null): string {
  switch (profile?.strategy) {
    case "auto_composer":
      return "自动选择";
    case "app_shortcut":
      return "快捷键";
    case "recorded_element":
      return "已记录输入框";
    case "recorded_point":
      return "点击位置";
    default:
      return "只打开";
  }
}

/** 详情与状态句里的全名。 */
export function focusStrategyLabel(strategy: FocusStrategy): string {
  switch (strategy) {
    case "auto_composer":
      return "自动选择输入框";
    case "app_shortcut":
      return "使用 APP 快捷键聚焦";
    case "recorded_element":
      return "聚焦已记录的输入框";
    case "recorded_point":
      return "记录点击位置";
    default:
      return "只打开 APP";
  }
}

/** 反查当前基础按键配置里绑定到该 APP 的「按键 × 手势」。 */
export function findAppBindings(
  actions: Partial<Record<RemoteButton, ButtonActions>>,
  target: string,
): AppBindingRef[] {
  const expected = keyOf(target);
  const result: AppBindingRef[] = [];
  for (const [button, actionSet] of Object.entries(actions ?? {}) as Array<
    [RemoteButton, ButtonActions]
  >) {
    for (const trigger of TRIGGERS) {
      const action = actionSet?.[trigger];
      if (action?.type === "open_app" && keyOf(action.target) === expected) {
        result.push({ button, trigger });
      }
    }
  }
  return result;
}

/**
 * 组合动作里的目标引用。
 *
 * 只在删除自定义应用时使用：预设应用被删除适配时不会从应用库消失，因此
 * 组合动作仍能按原 target 打开它。
 */
export function findMacroReferences(
  definitions: readonly MacroDefinition[],
  target: string,
): AppMacroReference[] {
  const expected = keyOf(target);
  const referenced = new Map<string, AppMacroReference>();
  for (const definition of definitions) {
    const hit = definition.steps.some((step) => {
      if (
        step.action !== "open_application" &&
        step.action !== "wait_for_application" &&
        step.action !== "focus_input"
      ) {
        return false;
      }
      const stepTarget = step.parameters?.target;
      return typeof stepTarget === "string" && keyOf(stepTarget) === expected;
    });
    if (hit) {
      referenced.set(definition.macroId, {
        macroId: definition.macroId,
        name: definition.name,
      });
    }
  }
  return [...referenced.values()];
}

/**
 * 删除适配的纯数据变换。
 *
 * - 自定义应用：同时移除 `applications` 条目与 `focusProfiles` 档案。
 * - 预设应用：只移除显式档案，回落到内置默认。
 *
 * 调用方负责先做绑定 / 组合动作引用检查。
 */
export function removeAppAdaptation(
  mappings: ButtonMappings,
  target: string,
  preset: boolean,
): ButtonMappings {
  const expected = keyOf(target);
  const focusProfiles: Record<string, AppFocusProfile> = {};
  for (const [key, profile] of Object.entries(mappings.focusProfiles ?? {})) {
    if (keyOf(key) !== expected) focusProfiles[key] = profile;
  }
  const next: ButtonMappings = {
    ...mappings,
    focusProfiles: Object.keys(focusProfiles).length ? focusProfiles : undefined,
  };
  if (!preset) {
    next.applications = (mappings.applications ?? []).filter(
      (app) => keyOf(app.path) !== expected,
    );
  }
  return next;
}

/** 选中项失效（被删 / 被移除）时回退到第一项。 */
export function nextSelectedTarget(
  rows: readonly AppAdaptationRow[],
  current: string | null,
): string | null {
  if (current && rows.some((row) => row.target === current)) return current;
  return rows[0]?.target ?? null;
}
