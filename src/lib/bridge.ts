import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

export const VB_CABLE_DOWNLOAD_URL = "https://vb-audio.com/Cable/";

/**
 * 设置页“问题反馈”的外部入口（2026-10-01 用户指定，2026-10-02 官网地址改为
 * 带 `?from=win` 的来源标记，便于官网区分 Windows 版来客）：官网首页与 Windows
 * 版源码仓库。
 *
 * 这两个字符串同时是 `src-tauri/capabilities/default.json` 里 opener 白名单的
 * 键——改这里必须同步改那里，否则真机上点击会被插件判为 ForbiddenUrl
 * （`bridge.test.ts` 逐字锁定了这份契约，runtime simulation 另行断言不在真机
 * 上被拒绝）。
 */
export const OFFICIAL_WEBSITE_URL = "https://sayall.app/?from=win";
export const GITHUB_REPOSITORY_URL = "https://github.com/GetSayAll/remote-mic-app-windows";

/**
 * Vokie 官网（2026-10-01 Andy 提供）。
 *
 * 同样是 opener 白名单的键：新增 URL 必须同步
 * `src-tauri/capabilities/default.json` 与 `bridge.test.ts` 的白名单断言。
 */
export const VOKIE_HOMEPAGE_URL = "https://vokie.com/";

/**
 * 推荐页只加载官网（2026-10-18 用户指定），不在 Windows 客户端复制推荐内容。
 * 官网 `/recommendations/` 设置 `X-Frame-Options: DENY`，因此页面由 Tauri
 * 子 WebView 承载；这里只保留「打开官网」兜底所用的公开地址。
 */
export const RECOMMENDATIONS_URL = "https://sayall.app/recommendations/";

export type RecommendationsPageStatus = "loading" | "ready" | "failed";

export interface RecommendationsPageEvent {
  generation: number;
  status: "ready" | "failed";
  reason?: string | null;
}

export type ConnectionPhase =
  | "idle"
  | "connecting"
  | "discovering"
  | "awaiting_capabilities"
  | "ready"
  | "streaming"
  | "draining"
  | "reconnecting"
  | "suspended"
  | "disconnected"
  | "failed";

export type VoiceSessionState = "idle" | "streaming" | "draining";

export type RemoteModel = "rc001" | "rc003" | "unknown";

export type AudioPhase =
  | "unconfigured"
  | "ready"
  | "streaming"
  | "draining"
  | "failed"
  | "unsupported";

export interface AudioEndpoint {
  id: string;
  name: string;
  isVirtualCableCandidate: boolean;
}

/**
 * 界面上的“推荐”判据：值得推荐给输入法当麦克风来源的 VB-CABLE 渲染端点。
 *
 * 渲染端点的名字跨 VB-CABLE 驱动版本变化（2026-10-02 现场）：
 *   经典 2 通道：`CABLE Input (VB-Audio Virtual Cable)`
 *   新版 16 通道：`CABLE In 16 Ch (2- VB-Audio Virtual Cable)`
 * 两者都能把声音送回录音端 `CABLE Output`（`examples/cable_loopback_probe.rs`
 * 实测两个端点回环 peak 均 ≈11k），因此都推荐；VB-CABLE A/B 的
 * `CABLE-A/B Input` 不带 "VB-Audio Virtual Cable"，不推荐（它们的录音端不是
 * 输入法默认监听的 CABLE Output）。
 *
 * 后端的 `isVirtualCableCandidate` 是更宽的候选判定（含 VB-CABLE A/B 的
 * CABLE-A/B Input 与 CI 仿真端点），只用于自动选择与安装检测，不足以
 * 决定推荐标记；两者刻意分开。
 */
export function isRecommendedVoiceEndpoint(endpoint: AudioEndpoint): boolean {
  const name = endpoint.name.trim().toLowerCase();
  if (!name.includes("vb-audio virtual cable")) {
    return false;
  }
  return name.includes("cable input") || name.includes("cable in");
}

export interface AudioSnapshot {
  phase: AudioPhase;
  selectedEndpointId: string | null;
  selectedEndpointName: string | null;
  queuedSamples: number;
  submittedSamples: number;
  generation: number;
  lastError: string | null;
}

export type RawInputPhase = "stopped" | "starting" | "awaiting" | "ready" | "failed" | "unsupported";

export type RemoteButton =
  | "back"
  | "ok"
  | "tv"
  | "home"
  | "right"
  | "left"
  | "down"
  | "up"
  | "menu"
  | "power"
  | "volume_mute"
  | "volume_up"
  | "volume_down";

export type ButtonTrigger = "single" | "double" | "long";

export interface ButtonEdge {
  button: RemoteButton;
  isPressed: boolean;
}

export interface ShortcutCaptureEdge {
  key: KeyCode;
  isPressed: boolean;
  /** 边沿来源：real = 物理事件；injected = 外部钩子（输入法）吞下后重放的副本。 */
  source?: "real" | "injected";
}

export interface RawInputSnapshot {
  phase: RawInputPhase;
  matchedDeviceCount: number;
  rawEventCount: number;
  semanticEdgeCount: number;
  lastButton: RemoteButton | null;
  lastIsPressed: boolean | null;
  activeButtons: RemoteButton[];
  lastError: string | null;
  /** 报文来自遥控器但路径与绑定不符而被丢弃的次数（绑定失效的直接证据）。 */
  staleRemoteEventCount: number;
}

/**
 * 全按键增强捕获传输桥接（捕获链第 ② 段）的状态。
 *
 * 背景：RC003 的返回 / 音量± 在 Windows 侧零事件（`kbdhid` 丢弃了这三个 usage），
 * 增强模式由提权助手在报告层拦下已映射按键、再经这条桥接送回主程序。所以按键无响应有
 * 多种原因，这一份快照用来区分它们：`listening` = 主程序已就绪、在等助手；
 * `connected` 且 `edgesApplied` 增长 = 边沿真的过了桥。
 */
export interface Rc003BridgeSnapshot {
  phase: Rc003BridgePhase;
  port: number;
  helperPid: number;
  acceptedTotal: number;
  deniedTotal: number;
  replacedTotal: number;
  edgesApplied: number;
  usagesDropped: number;
  malformedTotal: number;
  watchdogReleaseTotal: number;
  pressedUsages: number[];
  lastRxAgeMs: number | null;
  targetGeneration: number;
  targetUsages: number[];
  ownedUsages: number[];
  /**
   * 「全按键支持」的**实际生效状态**（2026-10-13）：开关开着 ≠ 生效。
   *
   * Rust 的常驻对账线程发布（判据只此一处，界面只读，见 `CaptureHealth`）：
   * `off` 开关关着 / `starting` 助手没连上或还在宽限期内（不显示文案）/
   * `active` agent 已接管 / `inactive` 过了宽限仍没有所有权——此时界面显示
   * 一句「还没有生效…」的用户语言（文案见 `captureInactiveText`）。
   */
  captureHealth: Rc003CaptureHealth;
  /**
   * 启动时被应用**主动关掉**的原因（2026-10-15，issue #264 附加观察甲）：
   * `none` = 没发生；其余取值表示开关是被回落拉低的，界面要让用户看懂"为什么
   * 自己关了"（文案见 `captureRevokedText`）。token 与 Rust `CaptureRevoke::as_str`
   * 逐字对应。
   */
  captureRevoked: Rc003CaptureRevoked;
}

/** `captureHealth` 的取值（与 Rust `CaptureHealth::as_str` 逐字对应）。 */
export type Rc003CaptureHealth = "off" | "starting" | "active" | "inactive";

/** `captureRevoked` 的取值（与 Rust `CaptureRevoke::as_str` 逐字对应）。 */
export type Rc003CaptureRevoked =
  | "none"
  | "task_missing"
  | "reauth_marker_present"
  | "task_target_mismatch"
  | "task_power_policy_mismatch"
  | "unsupported";

/** `stopped` = 该平台没有这个机制（非 Windows），或桥接未启用。 */
export type Rc003BridgePhase = "stopped" | "listening" | "connected" | "failed";

export type KeyCode = string;

export interface KeyChord {
  keys: KeyCode[];
}

export type MouseClickKind = "left" | "right" | "double_left" | "middle";
export type MoveDirection = "up" | "down" | "left" | "right";
export const mouseClickLabels: Record<MouseClickKind, string> = { left: "左键单击", right: "右键单击", double_left: "左键双击", middle: "中键单击" };
export const mouseMoveLabels: Record<MoveDirection, string> = { up: "鼠标向上", down: "鼠标向下", left: "鼠标向左", right: "鼠标向右" };

export type ButtonAction =
  | { type: "disabled" }
  | { type: "shortcut"; chord: KeyChord }
  | { type: "scroll"; direction: "up" | "down"; steps?: number }
  | { type: "mouse_click"; kind: MouseClickKind }
  | { type: "mouse_move"; direction: MoveDirection; distance: number }
  | { type: "open_app"; target: string }
  | { type: "focus_input" }
  | { type: "run_macro"; macroId: string; macroVersion: string }
  | { type: "open_common_phrases" };

// ===== 组合动作（Rust `sayall_core::macros` 的 camelCase 镜像）=====

export type MacroAction =
  | "open_application"
  | "wait_for_application"
  | "send_shortcut"
  | "focus_input"
  | "focus_frontmost"
  | "system_action"
  | "open_url"
  | "run_macro";

export type MacroModifier = "control" | "alt" | "shift" | "win";

/** 修饰键固定显示顺序（对齐 Mac PRODUCT_SPEC：Control → Option → Shift → Command）。 */
export const macroModifierOrder: MacroModifier[] = ["control", "alt", "shift", "win"];
export const macroModifierLabels: Record<MacroModifier, string> = {
  control: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  win: "Win",
};

export interface MacroChord {
  modifiers: MacroModifier[];
  /** 语义键名（`n`、`return`、`escape`、`space`、方向键名、`f1`…`f12`）。 */
  key: string;
}

export type ApplicationWaitState = "running" | "visible_window";

export interface MacroParameters {
  target?: string;
  state?: ApplicationWaitState;
  chord?: MacroChord;
  url?: string;
  nestedMacroId?: string;
  nestedMacroName?: string;
}

export interface MacroStep {
  stepId: string;
  action: MacroAction;
  timeoutMs?: number;
  parameters: MacroParameters;
}

export interface MacroDefinition {
  schemaVersion: string;
  macroId: string;
  version: string;
  name: string;
  summary?: string;
  steps: MacroStep[];
}

export interface MacroLibraryDocument {
  formatVersion: number;
  definitions: MacroDefinition[];
}

export type MacroTerminalResult =
  | "completed"
  | "completed_with_unknown_result"
  | "step_failed"
  | "missing_dependency"
  | "permission_denied"
  | "timed_out"
  | "cancelled"
  | "invalid_macro"
  | "nested_too_deep"
  | "cycle_detected";

export type MacroStepOutcome =
  | "done"
  | "unknown"
  | "missing_dependency"
  | "permission_denied"
  | "failed";

export interface MacroStepReport {
  index: number;
  stepId: string;
  action: MacroAction;
  outcome: MacroStepOutcome;
  elapsedMs: number;
}

export interface MacroRunReport {
  macroId: string;
  result: MacroTerminalResult;
  elapsedMs: number;
  steps: MacroStepReport[];
}

export interface MacroRunStatus {
  running: boolean;
  report: MacroRunReport | null;
}

/** 预设应用条目（list_preset_apps 返回；对齐 Mac PresetApplication）。 */
export interface PresetAppInfo {
  id: string;
  name: string;
  installed: boolean;
}

/** 打开应用后的聚焦方式（对齐 Rust `FocusStrategy`）。 */
export type FocusStrategy =
  | "open_only"
  | "app_shortcut"
  | "recorded_element"
  | "recorded_point";

/** 相对目标顶层窗口的归一化矩形（物理像素换算成比例）。 */
export interface NormalizedRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * 用户记录的输入框语义特征（不含任何输入内容）。
 *
 * `name` / `windowTitle` 属本地配置数据（可能含文档名等个人化信息），只存本地
 * 配置文件，不进日志、不随诊断上传。
 */
export interface RecordedFocusTarget {
  controlType: string;
  automationId: string;
  className?: string;
  name?: string;
  windowTitle?: string;
  normalizedRect?: NormalizedRect;
  contextTokens?: string[];
}

/** 「记录点击位置」：相对目标窗口的归一化坐标（0..1）与记录时的窗口尺寸。 */
export interface RecordedPoint {
  xRatio: number;
  yRatio: number;
  windowWidth: number;
  windowHeight: number;
}

/** 「打开应用」目标的聚焦档案（键 = open_app 的 target）。 */
export interface AppFocusProfile {
  strategy: FocusStrategy;
  shortcut?: KeyChord;
  recorded?: RecordedFocusTarget;
  point?: RecordedPoint;
}

/** 每键三列（单击/双击/长按），对齐 Mac 原版 ButtonTrigger。 */
export interface ButtonActions {
  single: ButtonAction;
  double: ButtonAction;
  long: ButtonAction;
  /**
   * 「按住连续触发」指定的槽位（单击或长按；缺省 = 关闭）。
   * 每键至多一个槽位，互斥由界面与 Rust `normalized()` 强制。
   */
  holdRepeat?: "single" | "long";
  /**
   * OK 键：用遥控器移动过光标后的 5 秒内，按 OK 直接点击光标位置
   * （缺省 = 关闭；只对 OK 键生效）。
   */
  okContextClick?: boolean;
}

export interface ButtonMappings {
  /** 配置结构版本（Rust 侧写入并校验；界面原样回传）。 */
  schemaVersion?: number;
  enabled: boolean;
  actions: Partial<Record<RemoteButton, ButtonActions>>;
  applications?: CustomAppPick[];
  /** 聚焦档案：键 = `open_app` 的 target（预设 id 或自定义应用路径）。 */
  focusProfiles?: Record<string, AppFocusProfile>;
}

export interface FiredGesture {
  button: RemoteButton;
  trigger: ButtonTrigger;
}

/** 「聚焦输入框」最近一次结果（focus_service::FocusReport；reason 见 FocusFailure 标识）。 */
export interface FocusReport {
  requestId: number;
  kind: string;
  focused: boolean;
  reason?: string;
  attempts: number;
  elapsedMs: number;
}

export interface ButtonMappingSnapshot {
  enabled: boolean;
  gateActive: boolean;
  listenerActive: boolean;
  swallowedEdges: number;
  leakedDowns: number;
  firedGestures: number;
  lastFired: FiredGesture | null;
  lastError: string | null;
  lastFocus?: FocusReport;
}

export interface SendInputSnapshot {
  available: boolean;
  submittedBatches: number;
  submittedEvents: number;
  lastError: string | null;
}

export interface AtvvCapabilities {
  version: number;
  codecs: number;
  interaction: number;
  frameSize: number;
  selectedCodec: number;
  sampleRate: number;
}

export interface ConnectionSnapshot {
  phase: ConnectionPhase;
  batteryLevel?: number | null;
  remoteName: string | null;
  remoteModel: RemoteModel;
  /** 当前连接设备的稳定令牌（Rust `device_token`，与扫描列表同一套算法）；老快照没有此字段。 */
  remoteToken?: string | null;
  capabilities: AtvvCapabilities | null;
  voiceState: VoiceSessionState;
  decodedSamples: number;
  generation: number;
  reconnectAttempt: number;
  powerNotificationsAvailable: boolean;
  lastError: string | null;
}

export interface PlatformSnapshot {
  platform: string;
  windowsApiAvailable: boolean;
  bleScanAvailable: boolean;
  bleVoiceReady: boolean;
  wasapiReady: boolean;
  rawInputReady: boolean;
  sendInputReady: boolean;
  verificationStatus: string;
  connection: ConnectionSnapshot;
  audio: AudioSnapshot;
  rawInput: RawInputSnapshot;
  buttonMapping: ButtonMappingSnapshot;
}

export interface RuntimeSnapshot {
  appVersion: string;
  platform: PlatformSnapshot;
}

export interface DiagnosticReport {
  schemaVersion: number;
  appVersion: string;
  /** 构建号（CI / 本地构建注入；取不到为 "unknown"）。 */
  appBuild: string;
  /** 源码修订（40 位 git SHA；取不到为 "unknown"）。 */
  sourceRevision: string;
  /** 发布通道（取不到为 "unknown"）。 */
  buildChannel: string;
  /** 运行机器的 Windows 版本（major.minor.build，如 "10.0.26100"）。 */
  windowsVersion: string;
  /** 进程架构（x86_64 等）。 */
  processArchitecture: string;
  /**
   * 本次开机的引导类型（2026-10-20）：`full` / `hiberboot` / `hibernate_resume` /
   * `unknown`。快速启动（hiberboot）开机后部分蓝牙适配器会带旧固件状态、
   * 造成链路反复断开——报障判读的第一个分流问题就是"这次开机是不是 hiberboot"。
   */
  bootType: string;
  /**
   * 「快速启动」功能是否开启；**读不到为 `null`**（不是 false——读不到不等于没开）。
   * 与 `bootType` 并列：开关开着但用户点的是「重启」时，`bootType` 仍是 `full`。
   */
  fastStartupEnabled: boolean | null;
  platform: string;
  verificationStatus: string;
  capabilities: {
    windowsApiAvailable: boolean;
    bleScanAvailable: boolean;
    bleVoiceReady: boolean;
    wasapiReady: boolean;
    rawInputReady: boolean;
    sendInputReady: boolean;
  };
  connection: {
    phase: ConnectionPhase;
    capabilitiesConfirmed: boolean;
    sampleRate: number | null;
    frameSize: number | null;
    decodedSamples: number;
    generation: number;
    reconnectAttempt: number;
    powerNotificationsAvailable: boolean;
    errorPresent: boolean;
  };
  audio: {
    phase: AudioPhase;
    endpointConfigured: boolean;
    queuedSamples: number;
    submittedSamples: number;
    generation: number;
    errorPresent: boolean;
  };
  rawInput: {
    phase: RawInputPhase;
    matchedDeviceCount: number;
    rawEventCount: number;
    semanticEdgeCount: number;
    lastButton: RemoteButton | null;
    lastIsPressed: boolean | null;
    errorPresent: boolean;
  };
  sendInput: {
    available: boolean;
    submittedBatches: number;
    submittedEvents: number;
    errorPresent: boolean;
  };
  buttonMapping: {
    enabled: boolean;
    gateActive: boolean;
    listenerActive: boolean;
    swallowedEdges: number;
    leakedDowns: number;
    firedGestures: number;
    errorPresent: boolean;
  };
  /** 输入法（2026-10-13 现场，schemaVersion 3）：诊断"语音没出字是不是没切输入法"。 */
  ime: {
    detected: boolean;
    activeName: string | null;
    installedNames: string[];
    expectedName: string | null;
    matchesExpected: boolean | null;
  };
}

export interface PairedRemote {
  id: string;
  name: string;
  model: RemoteModel;
  isSupportedCandidate: boolean;
  /** 设备稳定令牌（Rust `device_token`）：把扫描行与"当前连着的设备"对上；老快照没有此字段。 */
  token?: string;
}

/** 应用内更新（Rust updater command 契约，camelCase 对齐 src-tauri/src/updater.rs）。 */
export interface AppUpdateInfo {
  currentVersion: string;
  available: boolean;
  version: string | null;
  notes: string | null;
  date: string | null;
}

export interface AppUpdatePreferences {
  includePrereleases: boolean;
}

export type ThemePreference = "system" | "light" | "dark";

/** Windows 系统强调色（设置 > 个性化 > 颜色），Rust accent 命令契约。 */
export interface AccentRgb {
  r: number;
  g: number;
  b: number;
}

export async function getSystemAccentColor(): Promise<AccentRgb | null> {
  if (!isTauriRuntime()) return null;
  return invoke<AccentRgb | null>("get_system_accent_color");
}

export async function subscribeAccentChanges(
  handler: (color: AccentRgb) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<AccentRgb>("system-accent-changed", (event) =>
    handler(event.payload),
  );
  return () => {
    void unlisten();
  };
}

export async function getLaunchAtLogin(): Promise<boolean> {
  if (!isTauriRuntime()) return false;
  return invoke<boolean>("get_launch_at_login");
}

export async function setLaunchAtLogin(enabled: boolean): Promise<boolean> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法设置开机自启动");
  return invoke<boolean>("set_launch_at_login", { enabled });
}

/**
 * 「启动时自动打开主面板」（设置页「启动行为」，对齐 Mac `openMainWindowAtLaunch`）。
 *
 * 浏览器预览按新装默认返回 `true`，与 Rust `AppSettings::default()` 同源。
 */
export async function getOpenWindowAtLaunch(): Promise<boolean> {
  if (!isTauriRuntime()) return true;
  return invoke<boolean>("get_open_window_at_launch");
}

export async function setOpenWindowAtLaunch(enabled: boolean): Promise<boolean> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法设置启动时打开主面板");
  return invoke<boolean>("set_open_window_at_launch", { enabled });
}

/**
 * 本次启动前刚完成更新时的版本号（对齐 Mac `update.completed.*`）。
 *
 * Rust 侧取走即清空：同一次运行里只提示一次，重新挂载页面不会重复弹。
 * 浏览器预览没有更新机制，返回 `null`。
 */
export async function takeUpdateCompletedVersion(): Promise<string | null> {
  if (!isTauriRuntime()) return null;
  return invoke<string | null>("take_update_completed_version");
}

export interface AppUpdateProgress {
  downloaded: number;
  contentLength: number | null;
  finished: boolean;
}

const browserSnapshot: RuntimeSnapshot = {
  // 浏览器预览没有安装包可读，这里跟随当前应用版本：它是预览里"设置页版本号"
  // 与侧栏底部的唯一来源，写死旧值会让预览显示一个不存在的版本。
  appVersion: "0.8.2",
  platform: {
    platform: "browser-preview",
    windowsApiAvailable: false,
    bleScanAvailable: false,
    bleVoiceReady: false,
    wasapiReady: false,
    rawInputReady: false,
    sendInputReady: false,
    verificationStatus: "浏览器预览仅展示界面，不代表真机已通过",
    connection: {
      phase: "idle",
      remoteName: null,
      remoteModel: "unknown",
      capabilities: null,
      voiceState: "idle",
      decodedSamples: 0,
      generation: 0,
      reconnectAttempt: 0,
      powerNotificationsAvailable: false,
      lastError: null,
    },
    audio: {
      phase: "unsupported",
      selectedEndpointId: null,
      selectedEndpointName: null,
      queuedSamples: 0,
      submittedSamples: 0,
      generation: 0,
      lastError: null,
    },
    rawInput: {
      phase: "unsupported",
      matchedDeviceCount: 0,
      rawEventCount: 0,
      semanticEdgeCount: 0,
      lastButton: null,
      lastIsPressed: null,
      activeButtons: [],
      lastError: null,
      staleRemoteEventCount: 0,
    },
    buttonMapping: {
      enabled: true,
      gateActive: false,
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    },
  },
};

export function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function getRuntimeSnapshot(): Promise<RuntimeSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot;
  }
  return invoke<RuntimeSnapshot>("get_runtime_snapshot");
}

/** 设置页账号区只需要的会员摘要；不携带令牌、租约或设备标识。 */
export interface MembershipAccountSummary {
  availability: "available" | "unavailable";
  accountDisplayName: string | null;
  plusActive: boolean;
  accountSignedIn: boolean;
  syncState: "current" | "offline" | "unavailable";
}

export async function getMembershipAccountSummary(): Promise<MembershipAccountSummary> {
  if (!isTauriRuntime()) {
    return {
      availability: "unavailable",
      accountDisplayName: null,
      plusActive: false,
      accountSignedIn: false,
      syncState: "unavailable",
    };
  }
  return invoke<MembershipAccountSummary>("get_membership_snapshot");
}

export async function getDiagnosticReport(): Promise<DiagnosticReport> {
  if (!isTauriRuntime()) {
    return {
      schemaVersion: 4,
      appVersion: browserSnapshot.appVersion,
      appBuild: "unknown",
      sourceRevision: "unknown",
      buildChannel: "unknown",
      windowsVersion: "unknown",
      processArchitecture: "browser-preview",
      // 浏览器预览没有真实的开机上下文：一律 unknown / null（不是 false）。
      bootType: "unknown",
      fastStartupEnabled: null,
      platform: browserSnapshot.platform.platform,
      verificationStatus: browserSnapshot.platform.verificationStatus,
      capabilities: {
        windowsApiAvailable: false,
        bleScanAvailable: false,
        bleVoiceReady: false,
        wasapiReady: false,
        rawInputReady: false,
        sendInputReady: false,
      },
      connection: {
        phase: browserSnapshot.platform.connection.phase,
        capabilitiesConfirmed: false,
        sampleRate: null,
        frameSize: null,
        decodedSamples: 0,
        generation: 0,
        reconnectAttempt: 0,
        powerNotificationsAvailable: false,
        errorPresent: false,
      },
      audio: {
        phase: browserSnapshot.platform.audio.phase,
        endpointConfigured: false,
        queuedSamples: 0,
        submittedSamples: 0,
        generation: 0,
        errorPresent: false,
      },
      rawInput: {
        phase: browserSnapshot.platform.rawInput.phase,
        matchedDeviceCount: 0,
        rawEventCount: 0,
        semanticEdgeCount: 0,
        lastButton: null,
        lastIsPressed: null,
        errorPresent: false,
      },
      sendInput: {
        available: false,
        submittedBatches: 0,
        submittedEvents: 0,
        errorPresent: false,
      },
      buttonMapping: {
        enabled: true,
        gateActive: false,
        listenerActive: false,
        swallowedEdges: 0,
        leakedDowns: 0,
        firedGestures: 0,
        errorPresent: false,
      },
      ime: {
        detected: false,
        activeName: null,
        installedNames: [],
        expectedName: null,
        matchesExpected: null,
      },
    };
  }
  return invoke<DiagnosticReport>("get_diagnostic_report");
}

export function formatDiagnosticReport(
  report: DiagnosticReport,
  generatedAt = new Date().toISOString(),
): string {
  return JSON.stringify({ generatedAt, ...report }, null, 2);
}

/**
 * 打开诊断日志目录（关于页入口）。返回实际打开的目录供界面显示。
 *
 * 目录由 Rust 侧从日志初始化的落盘路径推导，前端不拼接、也不传路径——
 * 保留 capabilities 的最小权限边界（opener 只放行 VB-CABLE 官网、产品官网与
 * 源码仓库三个固定 URL）。
 */
export async function openLogDirectory(): Promise<string> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法打开日志目录");
  return invoke<string>("open_log_directory");
}

/**
 * Ctrl+W：关闭主窗口——隐藏到托盘驻留，语义与点标题栏“X”完全一致。
 *
 * 有意**不**调用 `@tauri-apps/api` 的 `getCurrentWindow().close()`：那条路径在
 * Windows 上究竟是触发 `CloseRequested`（→ Rust 侧 `prevent_close` + hide，
 * 即隐藏到托盘）还是直接销毁窗口，取决于 tao 的平台实现细节，跨版本可能静默
 * 改变语义；这里显式调 Rust 命令，动作与“X”的收尾是同一行代码。
 */
export async function hideMainWindow(): Promise<void> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法关闭窗口");
  await invoke("hide_main_window");
}

export async function scanPairedRemotes(): Promise<PairedRemote[]> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法读取已配对设备");
  }
  return invoke<PairedRemote[]>("scan_paired_remotes");
}

export async function getConnectionSnapshot(): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.connection;
  }
  return invoke<ConnectionSnapshot>("get_connection_snapshot");
}

export async function connectRemote(deviceId: string): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法连接遥控器");
  }
  return invoke<ConnectionSnapshot>("connect_remote", { deviceId });
}

export async function disconnectRemote(): Promise<ConnectionSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法断开遥控器");
  }
  return invoke<ConnectionSnapshot>("disconnect_remote");
}

export async function listAudioEndpoints(): Promise<AudioEndpoint[]> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法读取音频设备");
  }
  return invoke<AudioEndpoint[]>("list_audio_endpoints");
}

export async function getAudioSnapshot(): Promise<AudioSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.audio;
  }
  return invoke<AudioSnapshot>("get_audio_snapshot");
}

export async function selectAudioEndpoint(endpointId: string): Promise<AudioSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法选择音频设备");
  }
  return invoke<AudioSnapshot>("select_audio_endpoint", { endpointId });
}

/** 向导入口用到的固定 Windows 设置页（不接收任意 URI；Rust 侧另有 ms-settings 前缀校验）。 */
export type WindowsSettingsSection = "bluetooth" | "sound" | "microphone";

export async function openWindowsSettings(section: WindowsSettingsSection): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("open_windows_settings", { section });
}

/**
 * 语音增益（dB，0–24；对齐 Mac 设置页「增益」滑块，0 = 原始音量）。
 *
 * 读取走持久化值；写入返回实际保存值（越界会被钳制），界面用它回显——
 * 不假设"写进去什么就存什么"。
 */
export async function getGainDb(): Promise<number> {
  if (!isTauriRuntime()) {
    return 0;
  }
  return invoke<number>("get_gain_db");
}

export async function setGainDb(gainDb: number): Promise<number> {
  if (!isTauriRuntime()) {
    return gainDb;
  }
  return invoke<number>("set_gain_db", { gainDb });
}

export async function openVbCableDownloadPage(): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(VB_CABLE_DOWNLOAD_URL, "_blank", "noopener,noreferrer");
    return;
  }
  await openUrl(VB_CABLE_DOWNLOAD_URL);
}

/**
 * 外部链接的统一出口：浏览器预览开新标签，Tauri 运行时交给 opener 插件
 * （插件只放行 capabilities 白名单内的 URL，前端不做过滤，也不拼接参数）。
 */
async function openExternalUrl(url: string): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await openUrl(url);
}

/** 设置页顶部“官网”入口。 */
export async function openOfficialWebsite(): Promise<void> {
  await openExternalUrl(OFFICIAL_WEBSITE_URL);
}

/** 设置页“问题反馈”里的“GitHub”入口。 */
export async function openGitHubRepository(): Promise<void> {
  await openExternalUrl(GITHUB_REPOSITORY_URL);
}

/** 连接页“Vokie 未安装”提示里的官网入口（2026-10-01）。 */
export async function openVokieHomepage(): Promise<void> {
  await openExternalUrl(VOKIE_HOMEPAGE_URL);
}

/**
 * 显示推荐页（实际内容是官网页面，由 Rust 侧子 WebView 承载）。
 * 返回本次加载的 generation；迟到的 ready/failed 事件由页面按它丢弃。
 */
export async function showRecommendationsPage(
  theme: "light" | "dark",
): Promise<number> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法在应用内打开推荐页面");
  }
  return invoke<number>("show_recommendations", { theme });
}

/** 离开推荐页时隐藏子 WebView；页面本身保留，返回时可快速重载。 */
export async function hideRecommendationsPage(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("hide_recommendations");
}

/**
 * 加载失败时的「打开官网」：Rust 侧保留当前详情参数并去掉 `embed/theme`
 * 等 App 专用参数，再交给系统浏览器。浏览器预览直接打开列表页。
 */
export async function openRecommendationsWebsite(): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(RECOMMENDATIONS_URL, "_blank", "noopener,noreferrer");
    return;
  }
  await invoke("open_recommendations_website");
}

/** Vokie 检测结果：未安装时连接页显示官网入口；已安装但没运行时提示先启动它。 */
export interface VokieInstallation {
  installed: boolean;
  running: boolean;
}

export async function getVokieInstallation(): Promise<VokieInstallation> {
  if (!isTauriRuntime()) {
    return { installed: false, running: false };
  }
  return invoke<VokieInstallation>("get_vokie_installation");
}

/**
 * 打开 Vokie（连接页第 ② 步「打开 Vokie」按钮）：装了但没运行时一键叫起来。
 * 失败时抛错（调用方把原因显示给用户）。
 */
export async function launchVokie(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("launch_vokie");
}

/**
 * 「其他工具」面板记住的按键：`null` = 从未选过（保持现状），`[]` = 明确选了
 * 「不按键」。选中「其他工具」时恢复它，用户改选时写回。
 */
export async function getOtherVoiceHotkey(): Promise<KeyCode[] | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<KeyCode[] | null>("get_other_voice_hotkey");
}

export async function setOtherVoiceHotkey(keys: KeyCode[]): Promise<KeyCode[] | null> {
  if (!isTauriRuntime()) {
    return keys;
  }
  return invoke<KeyCode[] | null>("set_other_voice_hotkey", { keys });
}

/**
 * 首次使用向导状态（Rust `onboarding.json`，设计稿 §6）。
 *
 * `isActive` = 未完成当前流程版本；完成前不允许进入主界面。步骤 token 由
 * `src/onboarding/flow.ts` 的 `normalizeStep` 归一化（未知值 → welcome）。
 */
export interface OnboardingState {
  flowVersion: number;
  completedVersion: number;
  step: string;
  isActive: boolean;
}

/**
 * 浏览器预览（`pnpm dev`）兜底：默认「已完成」，预览环境用于页面开发，
 * 不把开发者锁进向导；真机/仿真环境始终走 Rust 状态文件。
 */
const BROWSER_ONBOARDING_STATE: OnboardingState = {
  flowVersion: 1,
  completedVersion: 1,
  step: "complete",
  isActive: false,
};

export async function getOnboardingState(): Promise<OnboardingState> {
  if (!isTauriRuntime()) {
    return { ...BROWSER_ONBOARDING_STATE };
  }
  return invoke<OnboardingState>("get_onboarding_state");
}

export async function saveOnboardingStep(step: string): Promise<OnboardingState> {
  if (!isTauriRuntime()) {
    return { ...BROWSER_ONBOARDING_STATE };
  }
  return invoke<OnboardingState>("save_onboarding_step", { step });
}

/** 设置页「重新运行设置向导」：只重置向导进度，不清除设备/映射/音频/其他设置。 */
export async function restartOnboarding(): Promise<OnboardingState> {
  if (!isTauriRuntime()) {
    return { ...BROWSER_ONBOARDING_STATE };
  }
  return invoke<OnboardingState>("restart_onboarding");
}

export async function completeOnboarding(): Promise<OnboardingState> {
  if (!isTauriRuntime()) {
    return { ...BROWSER_ONBOARDING_STATE };
  }
  return invoke<OnboardingState>("complete_onboarding");
}

/**
 * 第④步暂存语音绑定：落回滚快照 + 应用正式配置与运行时（设计稿 §5.4）。
 * 只有向导进行中可调用；退出未完成流程/重跑向导会回滚到进入向导前的配置。
 */
export async function stageOnboardingVoiceBinding(
  tool: VoiceInputTool,
  hotkey: KeyChord | null,
): Promise<OnboardingState> {
  if (!isTauriRuntime()) {
    return { ...BROWSER_ONBOARDING_STATE };
  }
  return invoke<OnboardingState>("stage_onboarding_voice_binding", { tool, hotkey });
}

export async function getRawInputSnapshot(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    return browserSnapshot.platform.rawInput;
  }
  return invoke<RawInputSnapshot>("get_raw_input_snapshot");
}

/**
 * 向导第⑥步：按键映射临时暂挂（只观察、不注入；内存态、不改用户配置）。
 * 进入「普通按键体验」时暂挂，离开时恢复；进程退出后自然复位。
 */
export async function setMappingSuspension(suspended: boolean): Promise<boolean> {
  if (!isTauriRuntime()) {
    return suspended;
  }
  return invoke<boolean>("set_mapping_suspension", { suspended });
}

/**
 * 向导第⑤步前置（探针④）：打开物理键观察窗口。
 * 返回窗口 id；0 = 当前环境不可用（仿真/钩子未运行），调用方按未知处理。
 * excludeVks：不计入的虚拟键码——「按住说话」和弦由报告层以非注入形态
 * 送进 OS（必须到达输入法），不属于手动输入。
 */
export async function beginKeyObservation(excludeVks: number[]): Promise<number> {
  if (!isTauriRuntime()) {
    return 0;
  }
  return invoke<number>("begin_key_observation", { excludeVks });
}

/**
 * 向导第⑤步前置（探针④）：关闭物理键观察窗口并取回计数。
 * 计数 = 窗口内「可能进入 OS 的物理按下沿」（非注入、未被门控吞下、不在
 * 排除集内）。返回 null = 计量不可靠（窗口过期/钩子停止）——fail-open，
 * 不得据此判定手动输入。
 */
export async function endKeyObservation(windowId: number): Promise<number | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<number | null>("end_key_observation", { windowId });
}

/** 浏览器 / 仿真环境没有这个机制：恒为 "不存在"，前端据此不显示这一条目。 */
const BROWSER_RC003_BRIDGE: Rc003BridgeSnapshot = {
  phase: "stopped",
  port: 0,
  helperPid: 0,
  acceptedTotal: 0,
  deniedTotal: 0,
  replacedTotal: 0,
  edgesApplied: 0,
  usagesDropped: 0,
  malformedTotal: 0,
  watchdogReleaseTotal: 0,
  pressedUsages: [],
  lastRxAgeMs: null,
  targetGeneration: 0,
  targetUsages: [],
  ownedUsages: [],
  captureHealth: "off",
  captureRevoked: "none",
};

export async function getRc003BridgeSnapshot(): Promise<Rc003BridgeSnapshot> {
  // `typeof window` 这一层不能省：组件卸载后轮询仍可能再触发一次，
  // 而测试环境在 teardown 之后 window 已不可用 —— 直接调 `isTauriRuntime()`
  // 会抛 ReferenceError，表现为一个与被测功能无关的 unhandled rejection。
  if (typeof window === "undefined" || !isTauriRuntime()) {
    return BROWSER_RC003_BRIDGE;
  }
  return invoke<Rc003BridgeSnapshot>("get_rc003_bridge_snapshot");
}

/**
 * 三键捕获的计划任务状态。`installed` = 用户已授权（任务在系统里），
 * 这是开关的真相源——主程序每次启动时会自动拉起已授权的助手，
 * 所以**不需要**额外的持久化字段。
 */
export interface Rc003TaskStatus {
  installed: boolean;
  /**
   * 这次打开开关会触发系统授权（UAC）。2026-10-03 起恒为 true：每次开启都会
   * 重新注册任务并弹一次 Windows 授权窗口。字段保留给诊断与类型兼容，
   * 前端弹窗判据已不依赖它（每次开启都弹确认，只有关闭方向直接执行）。
   */
  authorizationRequired: boolean;
  /** 用户意图（持久化，默认关闭）。开关显示读它，而不是读 installed。 */
  enabled: boolean;
  helperPath: string | null;
  lastError: string | null;
}

/**
 * 开关打开：**每次都重新授权**（弹一次 UAC 重新注册任务，主程序会等它完成），
 * 然后触发助手；开关关闭：结束助手，任务留在系统里但授权视为作废
 * （提权任务普通权限删不掉），下次开启必弹 UAC（2026-10-03 Andy 定稿）。
 */
export async function getRc003TaskStatus(): Promise<Rc003TaskStatus> {
  if (typeof window === "undefined" || !isTauriRuntime()) {
    return { installed: false, authorizationRequired: true, enabled: false, helperPath: null, lastError: null };
  }
  return invoke<Rc003TaskStatus>("get_rc003_task_status");
}

export async function enableRc003Capture(): Promise<Rc003TaskStatus> {
  if (typeof window === "undefined" || !isTauriRuntime()) {
    return { installed: false, authorizationRequired: true, enabled: false, helperPath: null, lastError: null };
  }
  return invoke<Rc003TaskStatus>("enable_rc003_capture");
}

export async function disableRc003Capture(): Promise<Rc003TaskStatus> {
  if (typeof window === "undefined" || !isTauriRuntime()) {
    return { installed: false, authorizationRequired: true, enabled: false, helperPath: null, lastError: null };
  }
  return invoke<Rc003TaskStatus>("disable_rc003_capture");
}

/** 本机无法使用「全按键支持」的原因（`available` 为 false 时一定有值）。 */
export type CaptureUnsupportedReason = "arch_unsupported" | "helper_missing";

/**
 * 「全按键支持」在这台电脑上是否可用（2026-10-07 issue #206）。
 *
 * 背景：Windows 11 ARM64 上增强捕获起不来（产品只提供 x64 载荷，ARM64 上加载
 * 不了），后端因此在启动时把已持久化的开启意图回落为关闭、并停止重试空转；
 * 界面据此把开关与三键卡片置灰、说明原因与恢复方式，不再让页面永久停在
 * 「正在启动」。`available=false` 时桥接相位是 `stopped`（状态点不显示）。
 */
export interface CaptureSupport {
  /** 本机原生架构。 */
  nativeArch: "x64" | "arm64" | "unknown";
  /** 本机架构对应的载荷文件名（诊断信息，不进用户可见文案）。 */
  helperExpected: string;
  /** false = 本机无法使用「全按键支持」：界面必须置灰入口并说明原因。 */
  available: boolean;
  reason: CaptureUnsupportedReason | null;
}

/** 浏览器预览没有真实平台：按「可用」渲染，保证界面能完整走查。 */
const BROWSER_CAPTURE_SUPPORT: CaptureSupport = {
  nativeArch: "unknown",
  helperExpected: "sayall-helper.exe",
  available: true,
  reason: null,
};

/**
 * 读取本机的支持情况（挂载时一次 + 随页面既有轮询刷新）。
 *
 * 与 `getRc003TaskStatus` 同口径：非 Tauri 环境返回浏览器预览值，Tauri 环境
 * IPC 失败时抛错，由调用方 `.catch` 后保持上一次的值（读不到就不置灰，
 * 不因为一次 IPC 失败把功能锁死）。
 */
export async function getCaptureSupport(): Promise<CaptureSupport> {
  if (typeof window === "undefined" || !isTauriRuntime()) {
    return BROWSER_CAPTURE_SUPPORT;
  }
  return invoke<CaptureSupport>("get_capture_support");
}

export async function startRawInput(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法启动按键监听");
  }
  return invoke<RawInputSnapshot>("start_raw_input");
}

export async function stopRawInput(): Promise<RawInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法停止按键监听");
  }
  return invoke<RawInputSnapshot>("stop_raw_input");
}

export async function getButtonMappings(): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    return { enabled: true, actions: {} };
  }
  return invoke<ButtonMappings>("get_button_mappings");
}

export async function saveButtonMappings(mappings: ButtonMappings): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法保存按键映射");
  }
  return invoke<ButtonMappings>("save_button_mappings", { mappings });
}

export async function resetButtonMappings(): Promise<ButtonMappings> {
  if (!isTauriRuntime()) {
    return { enabled: true, actions: {} };
  }
  return invoke<ButtonMappings>("reset_button_mappings");
}

/** 返回 false 表示用户在系统文件选择器中取消。 */
export async function exportButtonMappingConfiguration(): Promise<boolean> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法导出按键映射配置");
  }
  return invoke<boolean>("export_button_mapping_configuration");
}

/** 返回 null 表示用户在系统文件选择器中取消。 */
export async function importButtonMappingConfiguration(): Promise<ButtonMappings | null> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法导入按键映射配置");
  }
  return invoke<ButtonMappings | null>("import_button_mapping_configuration");
}

export async function testButtonMapping(
  button: RemoteButton,
  trigger: ButtonTrigger,
): Promise<SendInputSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法执行按键测试");
  }
  return invoke<SendInputSnapshot>("test_button_mapping", { button, trigger });
}

export async function listPresetApps(): Promise<PresetAppInfo[]> {
  if (!isTauriRuntime()) {
    // 浏览器预览：展示完整预设表（仅渲染验证）。
    return [
      { id: "sayall", name: "无线麦", installed: true },
      { id: "wechat", name: "微信", installed: true },
      { id: "workbuddy", name: "WorkBuddy", installed: true },
      { id: "chatgpt", name: "ChatGPT", installed: true },
      { id: "chrome", name: "Chrome 浏览器", installed: true },
      { id: "notepad", name: "记事本", installed: true },
      { id: "calc", name: "计算器", installed: true },
      { id: "explorer", name: "文件资源管理器", installed: true },
      { id: "netease_music", name: "网易云音乐", installed: true },
      // 2026-10-02 扩充的预设（与 Rust PRESET_APPS 同步，仅浏览器预览用）。
      { id: "vokie", name: "Vokie", installed: true },
      { id: "vscode", name: "Visual Studio Code", installed: true },
      { id: "cursor", name: "Cursor", installed: true },
      { id: "dimagent", name: "DimAgent", installed: true },
      { id: "qq", name: "QQ", installed: true },
      { id: "feishu", name: "飞书", installed: true },
      { id: "hermes", name: "Hermes", installed: true },
    ];
  }
  return invoke<PresetAppInfo[]>("list_preset_apps");
}

/**
 * 「学习输入框」：3 秒窗口内轮询系统焦点，返回捕获到的可编辑目标特征。
 *
 * 调用方需先让目标应用成为前台（本命令不会切换窗口）。
 */
export async function learnFocusTarget(): Promise<RecordedFocusTarget> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法学习输入框");
  }
  return invoke<RecordedFocusTarget>("learn_focus_target");
}

/**
 * 「记录点击位置」：把当前鼠标所在的相对位置记到目标应用上。
 *
 * 只读取光标坐标与目标窗口矩形，不读任何内容；光标不在目标窗口内时抛
 * `cursor_outside_target`（见 `focusPointErrorLabel`）。
 */
export async function learnFocusPoint(target: string): Promise<RecordedPoint> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法记录点击位置");
  }
  return invoke<RecordedPoint>("learn_focus_point", { target });
}

/** 「测试打开与聚焦」：按目标与聚焦档案走一次生产路径（异步受理）。 */
export async function testAppFocus(target: string): Promise<void> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法测试打开与聚焦");
  }
  return invoke<void>("test_app_focus", { target });
}

export async function getButtonMappingSnapshot(): Promise<ButtonMappingSnapshot> {
  if (!isTauriRuntime()) {
    return {
      enabled: true,
      gateActive: false,
      listenerActive: false,
      swallowedEdges: 0,
      leakedDowns: 0,
      firedGestures: 0,
      lastFired: null,
      lastError: null,
    };
  }
  return invoke<ButtonMappingSnapshot>("get_button_mapping_snapshot");
}

/** 订阅语义按键边沿（画布高亮数据源）；浏览器预览下为空订阅。 */
export async function subscribeButtonEdges(
  handler: (edge: ButtonEdge) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<ButtonEdge>("button-edge", (event) => handler(event.payload));
  return () => {
    void unlisten();
  };
}

/** 订阅已触发手势（单击/双击/长按反馈）；浏览器预览下为空订阅。 */
export async function subscribeButtonGestures(
  handler: (gesture: FiredGesture) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<FiredGesture>("button-gesture", (event) => handler(event.payload));
  return () => {
    void unlisten();
  };
}

/** 开始 OS 级快捷键录入；返回录入开始时仍被按住的键（preheld）。
 *  preheld 键的边沿对录入不可见（防粘键：其 DOWN 已进 OS，UP 必须放行），
 *  后端会等它们全部松开后才开始投递边沿——前端据此提示用户先松手，
 *  避免"按住中打开录入"被静默截断成半截组合。 */
export async function startShortcutCapture(): Promise<KeyCode[]> {
  if (!isTauriRuntime()) return [];
  const preheld = await invoke<KeyCode[]>("start_shortcut_capture");
  return preheld ?? [];
}

/** 微信输入法语音是否在录入会话期间被触发（观测其麦克风 ConsentStore）。 */
export type WetypeVoiceVerdict = "observed" | "not_observed" | "unknown";

export interface ShortcutCaptureStopResult {
  /** "unknown" 表示观测不可用，调用方不得据此推断用户按了什么。 */
  wetypeVoice: WetypeVoiceVerdict;
}

export async function stopShortcutCapture(): Promise<ShortcutCaptureStopResult | null> {
  if (!isTauriRuntime()) return null;
  const result = await invoke<ShortcutCaptureStopResult | null>("stop_shortcut_capture");
  return result ?? null;
}

/** 原生低级钩子录入边沿；Win+L 等系统组合在到达 Shell 前已成对吞下。 */
export async function subscribeShortcutCaptureEdges(
  handler: (edge: ShortcutCaptureEdge) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<ShortcutCaptureEdge>("shortcut-capture-edge", (event) =>
    handler(event.payload),
  );
  return () => {
    void unlisten();
  };
}

export async function getSendInputSnapshot(): Promise<SendInputSnapshot> {
  if (!isTauriRuntime()) {
    return { available: false, submittedBatches: 0, submittedEvents: 0, lastError: null };
  }
  return invoke<SendInputSnapshot>("get_send_input_snapshot");
}

export async function getVoiceHoldHotkey(): Promise<KeyChord | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<KeyChord | null>("get_voice_hold_hotkey");
}

export async function setVoiceHoldHotkey(hotkey: KeyChord | null): Promise<KeyChord | null> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法保存按住说话快捷键");
  }
  return invoke<KeyChord | null>("set_voice_hold_hotkey", { hotkey });
}

/**
 * 连接页选择的输入工具（2026-09-30 设计稿 v3）。
 *
 * 它只决定连接页展示哪套引导（快捷键建议、准备清单、是否显示"支持更多
 * 输入工具"开关）；真正生效的语音路径永远是"按住说话快捷键"本身。
 * `null` = 用户从未选择过（老配置）：界面按当前快捷键推断一次后落存。
 */
export type VoiceInputTool = "wechat" | "doubao" | "vokie" | "other";

export async function getVoiceInputTool(): Promise<VoiceInputTool | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<VoiceInputTool | null>("get_voice_input_tool");
}

export async function setVoiceInputTool(tool: VoiceInputTool): Promise<VoiceInputTool> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法保存输入工具设置");
  }
  const saved = await invoke<VoiceInputTool | null>("set_voice_input_tool", { tool });
  return saved ?? tool;
}

/**
 * 「锁定当前按键」（按键页页脚开关，2026-10-18 持久化到应用设置）。
 *
 * 开启时操作实体遥控器不会切换正在编辑的按键；默认开启（对齐 Mac
 * `MappingSelectionPolicy` 的默认值）。设置文件是权威值：页面切换会重建
 * 组件，挂载时必须从这里读回；浏览器预览回落"开启"且不落盘（设置类一致口径）。
 */
export async function getLockMappingSelection(): Promise<boolean> {
  if (!isTauriRuntime()) {
    return true;
  }
  return invoke<boolean>("get_lock_mapping_selection");
}

export async function saveLockMappingSelection(locked: boolean): Promise<boolean> {
  if (!isTauriRuntime()) {
    return locked;
  }
  return invoke<boolean>("set_lock_mapping_selection", { locked });
}

/**
 * 输入法状态（2026-10-13 现场）：诊断"语音没出字是不是因为没切输入法"。
 *
 * - `detected=false`：读不到输入法状态——界面**不提示**（fail-open，宁可不说也不能瞎说）；
 * - `expected=null`：当前的输入工具不需要切输入法（如 Vokie / 其他工具）；
 * - `matchesExpected`：`null` = 判定不出来，同样是"不提示"。
 */
export interface ImeStatus {
  detected: boolean;
  active: string | null;
  installed: string[];
  expected: string | null;
  matchesExpected: boolean | null;
}


/**
 * **只读**查询输入法状态（不切输入法）。复制诊断信息时用它，避免"复制一下顺手改了
 * 用户输入法"。浏览器预览下返回"读不到"。
 */
export async function getImeStatus(): Promise<ImeStatus> {
  if (!isTauriRuntime()) {
    return { detected: false, active: null, installed: [], expected: null, matchesExpected: null };
  }
  return invoke<ImeStatus>("get_ime_status");
}

/** 检查应用更新；浏览器预览下返回"无更新"占位（不发起网络请求）。 */
export async function checkAppUpdate(): Promise<AppUpdateInfo> {
  if (!isTauriRuntime()) {
    return {
      currentVersion: browserSnapshot.appVersion,
      available: false,
      version: null,
      notes: null,
      date: null,
    };
  }
  return invoke<AppUpdateInfo>("check_app_update");
}

export async function getAppUpdatePreferences(): Promise<AppUpdatePreferences> {
  if (!isTauriRuntime()) {
    return { includePrereleases: false };
  }
  return invoke<AppUpdatePreferences>("get_app_update_preferences");
}

export async function setAppUpdatePreferences(
  includePrereleases: boolean,
): Promise<AppUpdatePreferences> {
  if (!isTauriRuntime()) {
    return { includePrereleases };
  }
  return invoke<AppUpdatePreferences>("set_app_update_preferences", { includePrereleases });
}

export async function getThemePreference(operationId: string): Promise<ThemePreference> {
  if (!isTauriRuntime()) {
    return "system";
  }
  return invoke<ThemePreference>("get_theme_preference", { operationId });
}

export async function saveThemePreference(
  preference: ThemePreference,
  operationId: string,
): Promise<ThemePreference> {
  if (!isTauriRuntime()) {
    return preference;
  }
  return invoke<ThemePreference>("set_theme_preference", { preference, operationId });
}

export interface ThemeResultReport {
  operationId: string;
  action: "initialize" | "change";
  preference: ThemePreference;
  resolvedTheme: "light" | "dark";
  terminalResult: "passed" | "failed";
  reason:
    | "applied"
    | "preference_load_failed"
    | "native_apply_failed"
    | "apply_or_save_failed";
  elapsedMs: number;
}

export async function reportThemeResult(report: ThemeResultReport): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("report_theme_result", { report });
}

/**
 * 应用图标（2026-10-02 用户指定；对齐 Mac main `AppIconController`/`AppIconCatalog`）：
 *
 * - `standard`：「默认」水彩鸭（可切换的另一风格，也是老配置与认不出的 ID 的落点）；
 * - `faceted-duck`：来自 Mac `Resources/AppIcons/faceted-duck.png` 的「几何鸭」，
 *   2026-10-04 起为新装默认，exe / 安装包自身的图标也用它。
 *
 * 切换后由 Rust 同时更换**主窗口图标（任务栏 / Alt-Tab / 标题栏）、托盘图标，
 * 以及开始菜单 / 桌面 / 固定到任务栏的快捷方式图标**；设置页顶部标识与选项预览用
 * 同一 ID 实时渲染。exe 与安装包自身的图标是安装产物，运行期不变。
 */
export type AppIconIdentifier = "standard" | "faceted-duck";

export async function getAppIcon(): Promise<AppIconIdentifier> {
  if (!isTauriRuntime()) return "standard";
  return invoke<AppIconIdentifier>("get_app_icon");
}

export async function setAppIcon(identifier: AppIconIdentifier): Promise<AppIconIdentifier> {
  if (!isTauriRuntime()) return identifier;
  return invoke<AppIconIdentifier>("set_app_icon", { identifier });
}

/** 下载并安装已检查到的更新（Windows 上安装成功时应用会退出并由安装器重启）。 */
export async function installAppUpdate(): Promise<void> {
  if (!isTauriRuntime()) {
    throw new Error("当前是浏览器预览，无法安装更新");
  }
  await invoke("install_app_update");
}

/** 订阅更新下载进度；浏览器预览下为空订阅。 */
export async function subscribeAppUpdateProgress(
  handler: (progress: AppUpdateProgress) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<AppUpdateProgress>("app-update-progress", (event) =>
    handler(event.payload),
  );
  return () => {
    void unlisten();
  };
}

const voiceHotkeyKeyLabels: Record<string, string> = {
  control: "Ctrl",
  left_control: "左 Ctrl",
  right_control: "右 Ctrl",
  shift: "Shift",
  left_shift: "左 Shift",
  right_shift: "右 Shift",
  alt: "Alt",
  left_alt: "左 Alt",
  right_alt: "右 Alt",
  left_windows: "左 Win",
  right_windows: "右 Win",
  enter: "Enter",
  escape: "Esc",
  space: "空格",
  tab: "Tab",
  apps: "右键菜单",
  oem1: ";",
  oem_plus: "=",
  oem_comma: ",",
  oem_minus: "-",
  oem_period: ".",
  oem2: "/",
  oem3: "`",
  oem4: "[",
  oem5: "\\",
  oem6: "]",
  oem7: "'",
  oem102: "\\",
  numpad0: "小键盘 0",
  numpad1: "小键盘 1",
  numpad2: "小键盘 2",
  numpad3: "小键盘 3",
  numpad4: "小键盘 4",
  numpad5: "小键盘 5",
  numpad6: "小键盘 6",
  numpad7: "小键盘 7",
  numpad8: "小键盘 8",
  numpad9: "小键盘 9",
  numpad_multiply: "小键盘 ×",
  numpad_add: "小键盘 +",
  numpad_subtract: "小键盘 −",
  numpad_decimal: "小键盘 .",
  numpad_divide: "小键盘 ÷",
  numlock: "Num Lock",
};

function voiceHotkeyKeyLabel(code: string): string {
  const known = voiceHotkeyKeyLabels[code];
  if (known) return known;
  const digit = /^digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  return code.toUpperCase();
}

export function voiceHoldHotkeyLabel(hotkey: KeyChord | null): string {
  if (!hotkey || hotkey.keys.length === 0) return "关闭";
  return hotkey.keys.map(voiceHotkeyKeyLabel).join(" + ");
}

export function connectionPhaseLabel(phase: ConnectionPhase): string {
  return {
    idle: "尚未连接",
    connecting: "正在连接遥控器",
    discovering: "正在连接遥控器",
    awaiting_capabilities: "正在确认语音功能",
    ready: "已连接",
    streaming: "正在接收语音",
    draining: "正在结束本次语音",
    reconnecting: "正在等待遥控器重连",
    suspended: "电脑已进入睡眠",
    disconnected: "遥控器已断开",
    failed: "连接失败",
  }[phase];
}

export function remoteModelLabel(model: RemoteModel): string {
  return {
    rc001: "小米蓝牙语音遥控器 2",
    rc003: "小米蓝牙语音遥控器 2 Pro",
    unknown: "连接后显示",
  }[model];
}

export function audioPhaseLabel(phase: AudioPhase): string {
  return {
    unconfigured: "尚未选择设备",
    ready: "已就绪",
    streaming: "正在写入语音",
    draining: "正在结束",
    failed: "语音设备出错",
    unsupported: "当前环境不支持语音设备",
  }[phase];
}

export const buttonLabels: Record<RemoteButton, string> = {
  back: "返回",
  ok: "确定",
  tv: "TV",
  home: "主页",
  right: "右",
  left: "左",
  down: "下",
  up: "上",
  menu: "菜单",
  power: "电源",
  volume_mute: "静音",
  volume_up: "音量+",
  volume_down: "音量−",
};

export function buttonLabel(button: RemoteButton): string {
  return buttonLabels[button];
}

export function buttonTriggerLabel(trigger: ButtonTrigger): string {
  return {
    single: "单击",
    double: "双击",
    long: "长按",
  }[trigger];
}

/**
 * 武装族按键的"同键映射"表（对齐 crates/sayall-windows/src/send_input.rs
 * 的 native_key）：映射动作与原生动作相同时，映射引擎的泄漏对冲保证
 * 冷首按单响应（原生动作已交付，引擎跳过注入）。
 */
export const identityShortcutByButton: Partial<Record<RemoteButton, KeyCode>> = {
  ok: "enter",
  up: "up",
  down: "down",
  left: "left",
  right: "right",
  home: "home",
};

export type ShortcutCapability = "all" | "identity" | "none";

/**
 * 按键 × 触发 × 型号 的"单响应能力"判定（2026-09-06 定稿；注入链路已由
 * examples/preset_inject_probe.rs 真机验证 36/36 全部正确——所有可见按键
 * 的所有配置均真实生效，本矩阵**只用于编辑器的信息提示**，不做门控）：
 *
 * - **all**（直接归因族：电源 VK 0xFF/0x5F、菜单 VK_APPS）：原始键
 *   从不泄漏 → 任意配置严格单响应；
 * - **identity**（武装族常见物理 VK：确定/方向）：孤立冷首按原始键
 *   必泄漏（结构性武装死锁，公开 API 内不可根除）→ 同键映射由泄漏对冲
 *   保证单响应，其他映射"配置动作正常执行 + 冷首按附带一次原生动作"；
 * - **none**：TV（OEM_3 `~/~，同键映射不可表达）。返回/音量±现在保留为
 *   可配置按键；实际是否能收到边沿由底层设备捕获能力决定。
 *
 * 2026-09-07 增补（方案 C"遥控器优先"落地，key_gate 常驻抑制族）：
 * Home/TV 已配置映射且遥控器连接期间原生按键被接管——任意按压（含孤立
 * 冷首按）严格单响应，本矩阵的 identity/none 标注对这两键仅剩编辑参考
 * 意义（见 ButtonsPage capabilityNote 的接管提示）。左键自 2026-09-08
 * 起恢复为与上/下/右/确定相同的逐键武装与泄漏对冲机制。
 */
export function shortcutCapability(
  button: RemoteButton,
  trigger: ButtonTrigger,
  _model: RemoteModel,
): ShortcutCapability {
  if (button === "power" || button === "menu") {
    return "all";
  }
  if (button === "tv") {
    // TV 无同键映射可表达。
    return "none";
  }
  if (button === "back" || button === "volume_up" || button === "volume_down") {
    return "all";
  }
  // 武装族（确定/方向）：单击可配同键映射（对冲单响应）。
  return trigger === "single" ? "identity" : "none";
}

const keyLabels: Record<string, string> = {
  ...voiceHotkeyKeyLabels,
  // 组合动作用 `return` 作为 Enter 的稳定存储名；界面统一显示键帽上的 "Enter"。
  return: "Enter",
  // 用厂商印在键帽上的英文名，避免“退格/删除”在中文里被混为一谈。
  backspace: "Backspace",
  home: "Home",
  page_up: "Page Up",
  page_down: "Page Down",
  end: "End",
  insert: "Insert",
  delete: "Delete",
  left: "←",
  up: "↑",
  right: "→",
  down: "↓",
  volume_mute: "静音",
  volume_down: "音量−",
  volume_up: "音量+",
  media_play_pause: "播放/暂停",
  media_prev: "上一首",
  media_next: "下一首",
  f1: "F1",
  f2: "F2",
  f3: "F3",
  f4: "F4",
  f5: "F5",
  f6: "F6",
  f7: "F7",
  f8: "F8",
  f9: "F9",
  f10: "F10",
  f11: "F11",
  f12: "F12",
};

export function keyLabel(code: KeyCode): string {
  const known = keyLabels[code];
  if (known) return known;
  const digit = /^digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  return code.toUpperCase();
}

export function chordLabel(chord: KeyChord): string {
  return chord.keys.map(keyLabel).join(" + ");
}

/** 预设应用显示名（页面加载 listPresetApps 后更新；测试可注入）。 */
const presetAppNames: Map<string, string> = new Map();

export function registerPresetAppNames(apps: Array<{ id: string; name: string }>): void {
  presetAppNames.clear();
  for (const app of apps) {
    presetAppNames.set(app.id, app.name);
  }
}

/** 组合动作名（页面加载库后更新；`actionSummary` 显示引用名）。 */
const macroNames: Map<string, string> = new Map();

export function registerMacroNames(definitions: Array<{ macroId: string; name: string }>): void {
  macroNames.clear();
  for (const definition of definitions) {
    macroNames.set(definition.macroId, definition.name);
  }
}

export function macroNameById(macroId: string): string | null {
  return macroNames.get(macroId) ?? null;
}

/**
 * 同一条组合动作只保留最新版本（面向用户的是「动作」而不是「版本」）。
 *
 * 库文件按不可变版本累积（编辑一次 = 新版本追加，绑定引用按最新版解析）；
 * 列表 / 选择器必须先用本函数去重，否则编辑一次就会多出一行
 * （2026-10-15 现场：1.0.0 与 1.0.1 同时显示成两个动作）。
 * 顺序 = 首次出现的顺序（列表稳定，编辑后不跳位）。
 */
export function latestMacroDefinitions(definitions: MacroDefinition[]): MacroDefinition[] {
  const parse = (version: string): [number, number, number] | null => {
    const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
    return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
  };
  const greater = (candidate: string, current: string): boolean => {
    const left = parse(candidate);
    const right = parse(current);
    if (left && right) {
      for (let index = 0; index < 3; index += 1) {
        if (left[index] !== right[index]) return left[index] > right[index];
      }
      return false;
    }
    return candidate > current;
  };
  const latest: MacroDefinition[] = [];
  for (const definition of definitions) {
    const index = latest.findIndex((item) => item.macroId === definition.macroId);
    if (index === -1) {
      latest.push(definition);
    } else if (greater(definition.version, latest[index].version)) {
      latest[index] = definition;
    }
  }
  return latest;
}

/** 常用语组件是否可用（按键页加载快照后注册；测试可注入）。 */
let commonPhrasesAvailable = false;

/**
 * 注册常用语组件可用性（按键页加载 `getCommonPhrasePanelSnapshot()` 后调用）。
 *
 * 为什么用模块级缓存而不是每次问 IPC：`actionSummary` 是同步的渲染路径函数，
 * 不能等待异步快照——与预设应用名的处理方式一致。
 */
export function registerCommonPhraseAvailability(available: boolean): void {
  commonPhrasesAvailable = available;
}

export function actionSummary(action: ButtonAction | undefined): string {
  if (!action || action.type === "disabled") return "未设置";
  if (action.type === "focus_input") return "聚焦输入框";
  if (action.type === "open_common_phrases") {
    // 缺私有组件（公开构建）时如实标注，不静默替换成含义不同的动作。
    return commonPhrasesAvailable ? "打开常用语" : "打开常用语（当前不可用）";
  }
  if (action.type === "scroll") {
    const label = action.direction === "up" ? "滚轮向上" : "滚轮向下";
    return (action.steps ?? 1) === 1 ? label : `${label} ${action.steps} 格`;
  }
  if (action.type === "mouse_click") return mouseClickLabels[action.kind];
  if (action.type === "mouse_move") return `${mouseMoveLabels[action.direction]} ${action.distance} px`;
  if (action.type === "open_app") {
    const known = presetAppNames.get(action.target);
    if (known) return `打开${known}`;
    // 自定义应用：target 为路径，取文件名去扩展名作展示名。
    const base = action.target.split(/[\\/]/).pop() ?? action.target;
    const stem = base.replace(/\.(exe|lnk)$/i, "");
    return `打开${stem || action.target}`;
  }
  if (action.type === "run_macro") {
    const known = macroNameById(action.macroId);
    return known ? `执行${known}` : "执行组合动作";
  }
  return chordLabel(action.chord);
}

/** 自定义应用选择结果（pick_custom_app 命令返回）。 */
export interface CustomAppPick {
  name: string;
  path: string;
}

export async function scanRegisteredApps(): Promise<CustomAppPick[]> {
  if (!isTauriRuntime()) throw new Error("应用扫描需要在 Windows 客户端中使用");
  return invoke<CustomAppPick[]>("scan_registered_apps");
}

/**
 * 打开原生文件选择器选择自定义应用（.exe/.lnk）。
 * 用户取消或浏览器预览环境返回 null。
 */
export async function pickCustomApp(): Promise<CustomAppPick | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  try {
    return await invoke<CustomAppPick | null>("pick_custom_app");
  } catch {
    return null;
  }
}

// ===== 组合动作 IPC =====

/** 浏览器预览用的演示库（只服务界面预览与快照断言，不进入生产路径）。 */
const previewMacroLibrary: MacroLibraryDocument = {
  formatVersion: 1,
  definitions: [
    {
      schemaVersion: "0.1",
      macroId: "macro.preview01",
      version: "1.0.0",
      name: "打开记事本并聚焦",
      steps: [
        {
          stepId: "step.01",
          action: "open_application",
          parameters: { target: "notepad" },
        },
        {
          stepId: "step.02",
          action: "focus_input",
          parameters: { target: "notepad" },
        },
      ],
    },
  ],
};

export async function getCombinedActions(): Promise<MacroLibraryDocument> {
  if (!isTauriRuntime()) {
    return structuredClone(previewMacroLibrary);
  }
  return invoke<MacroLibraryDocument>("get_combined_actions");
}

export async function saveMacro(definition: MacroDefinition): Promise<MacroLibraryDocument> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法保存组合动作");
  return invoke<MacroLibraryDocument>("save_macro", { definition });
}

export interface MacroDeleteResult {
  library: MacroLibraryDocument;
  removedMacroIds: string[];
  rewrittenMacroIds: string[];
}

export async function deleteMacro(macroId: string): Promise<MacroDeleteResult> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法删除组合动作");
  return invoke<MacroDeleteResult>("delete_macro", { macroId });
}

/** 删除前的引用清单（按键 + 嵌套引用）。 */
export interface MacroButtonReference {
  button: string;
  trigger: string;
}

export interface MacroReferenceReport {
  buttons: MacroButtonReference[];
  nestedIn: string[];
}

/** 查询引用该组合动作的按键与组合动作（删除确认用）。 */
export async function macroReferences(macroId: string): Promise<MacroReferenceReport> {
  if (!isTauriRuntime()) {
    return { buttons: [], nestedIn: [] };
  }
  return invoke<MacroReferenceReport>("macro_references", { macroId });
}

/** 导出组合动作库；用户取消返回 false。 */
export async function exportCombinedActions(): Promise<boolean> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法导出组合动作");
  return invoke<boolean>("export_combined_actions");
}

/** 导入组合动作（合并入库）；用户取消返回 null。 */
export async function importCombinedActions(): Promise<MacroLibraryDocument | null> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法导入组合动作");
  return invoke<MacroLibraryDocument | null>("import_combined_actions");
}

/** 测试运行已保存的组合动作（按最新版本；异步受理）。 */
export async function testMacro(macroId: string): Promise<void> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法测试组合动作");
  return invoke<void>("test_macro", { macroId });
}

/** 测试运行未保存的草稿定义；返回本次运行 ID（用于轮询状态）。 */
export async function testMacroDefinition(definition: MacroDefinition): Promise<string> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法测试组合动作");
  return invoke<string>("test_macro_definition", { definition });
}

/** 测试运行草稿里的单个步骤；返回本次运行 ID。 */
export async function testMacroStep(
  definition: MacroDefinition,
  stepId: string,
): Promise<string> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法测试步骤");
  return invoke<string>("test_macro_step", { definition, stepId });
}

export async function cancelMacro(macroId: string): Promise<boolean> {
  if (!isTauriRuntime()) throw new Error("当前是浏览器预览，无法取消组合动作");
  return invoke<boolean>("cancel_macro", { macroId });
}

export async function getMacroRunStatus(macroId: string): Promise<MacroRunStatus> {
  if (!isTauriRuntime()) {
    return { running: false, report: null };
  }
  return invoke<MacroRunStatus>("get_macro_run_status", { macroId });
}

/** 组合动作步骤类型的中文名（对齐 Mac `macro.step.*` 文案）。 */
export const macroActionLabels: Record<MacroAction, string> = {
  open_application: "打开应用",
  wait_for_application: "等待应用",
  send_shortcut: "单键或快捷键",
  focus_input: "聚焦学习过的输入框",
  focus_frontmost: "聚焦输入框",
  system_action: "系统与媒体动作",
  open_url: "打开网址",
  run_macro: "执行已有组合动作",
};

export function macroActionLabel(action: MacroAction): string {
  return macroActionLabels[action] ?? action;
}

/** 组合键显示（修饰键固定顺序，`Ctrl + Alt + N`；与按键页文本惯例一致）。 */
export function macroChordLabel(chord: MacroChord): string {
  const modifiers = macroModifierOrder
    .filter((modifier) => chord.modifiers.includes(modifier))
    .map((modifier) => macroModifierLabels[modifier]);
  const key = chord.key.length === 1 ? chord.key.toUpperCase() : keyLabel(chord.key);
  return [...modifiers, key].join(" + ");
}

/** 步骤摘要（列表与详情共用一行说明）。 */
export function macroStepSummary(step: MacroStep): string {
  const parameters = step.parameters;
  switch (step.action) {
    case "open_application":
      return appTargetLabel(parameters.target);
    case "wait_for_application":
      return `${appTargetLabel(parameters.target)} · ${
        parameters.state === "running" ? "App 已打开" : "App 窗口已出现"
      }`;
    case "send_shortcut":
      return parameters.chord ? macroChordLabel(parameters.chord) : "尚未录入按键";
    case "focus_input":
      return `${appTargetLabel(parameters.target)} · 聚焦学习过的输入框`;
    case "focus_frontmost":
      return "当前前台应用 · 聚焦输入框";
    case "system_action":
      return parameters.chord ? macroChordLabel(parameters.chord) : "尚未选择系统动作";
    case "open_url":
      return parameters.url ?? "尚未填写网址";
    case "run_macro":
      return parameters.nestedMacroName ?? "尚未选择组合动作";
  }
}

function appTargetLabel(target: string | undefined): string {
  if (!target) return "尚未选择 App";
  const known = presetAppNames.get(target);
  if (known) return known;
  if (!target.includes("\\") && !target.includes("/")) return target;
  const base = target.split(/[\\/]/).pop() ?? target;
  const stem = base.replace(/\.(exe|lnk)$/i, "");
  return stem || target;
}

/** 聚焦学习失败的机器码 → 用户文案（底层错误形如 `SendInput failed: no_candidate`）。 */
export function focusLearnErrorLabel(message: string): string {
  const code = message.replace(/^SendInput failed:\s*/i, "").trim();
  switch (code) {
    case "no_candidate":
      return "没有采到可用的输入框：请在倒计时里切到目标 App 并点击它的输入框，然后重试";
    case "self_foreground":
      return "学习期间无线麦在窗口最前面：请先切到目标 App 再重试";
    case "not_foreground":
      return "目标 App 不在最前面：请先切到它再重试";
    case "target_exited":
      return "目标 App 已经退出：请先打开它再学习";
    case "not_accessible":
      return "这个输入框读不到（可能是管理员权限窗口或受限应用）";
    case "timeout":
      return "学习超时，请重试";
    case "cancelled":
      return "学习已取消";
    default:
      return message;
  }
}

/** 「记录点击位置」失败的机器码 → 用户文案（底层错误同样可能带 `SendInput failed:` 前缀）。 */
export function focusPointErrorLabel(message: string): string {
  const code = message.replace(/^SendInput failed:\s*/i, "").trim();
  switch (code) {
    case "cursor_outside_target":
      return "鼠标不在目标应用的窗口里：把鼠标停在要点击的位置（例如输入框）上再试";
    case "target_window_missing":
      return "目标应用没有可见窗口：先打开它再试";
    case "not_foreground":
      return "目标应用没有在最前面：先切到它再试";
    case "window_size_changed":
      return "目标窗口的大小和记录时相差太大：请重新记录一次位置";
    case "cursor_unavailable":
      return "读不到鼠标位置：请重试";
    default:
      return message;
  }
}

/** 组合动作启动失败的机器码 → 用户文案（Rust 侧只给稳定 token）。 */
export function macroStartErrorLabel(code: string): string | null {
  if (code === "already_running") return "这个组合动作正在执行，请稍后再试";
  if (code === "not_found") return "找不到这个组合动作，请重新打开页面";
  if (code.startsWith("invalid:")) {
    const issues = code.slice("invalid:".length).split(",");
    if (issues.includes("name_empty")) return "请先填写组合动作名称";
    if (issues.includes("name_too_long")) return "组合动作名称太长，最多 20 个字符";
    if (issues.includes("too_few_steps") || issues.includes("too_many_steps")) {
      return "步骤数量不对，请检查后重试";
    }
    if (issues.some((issue) => issue.startsWith("missing_parameter"))) {
      return "有步骤还没填写完整，请检查每一步的参数";
    }
    return "这条组合动作的设置不完整，请检查名称与步骤";
  }
  return null;
}

/** 整宏终态的用户文案（对齐 Mac `macro.status.execution.*`）。 */
export function macroTerminalLabel(result: MacroTerminalResult): string {
  switch (result) {
    case "completed":
      return "组合动作执行成功";
    case "completed_with_unknown_result":
      return "组合动作已执行，但无法确认第三方 App 的结果";
    case "cancelled":
      return "组合动作已取消";
    case "timed_out":
      return "组合动作执行超时";
    case "invalid_macro":
      return "这条组合动作的设置有问题";
    case "missing_dependency":
      return "需要使用的 App 或输入框记录不在这台电脑上";
    case "permission_denied":
      return "组合动作缺少系统权限";
    case "nested_too_deep":
      return "组合动作嵌套超过 8 层";
    case "cycle_detected":
      return "组合动作不能直接或间接执行自己";
    case "step_failed":
      return "组合动作中的步骤执行失败";
  }
}

/** 单步结果文案（对齐 Mac `macro.test.step.*`）。 */
export function macroStepOutcomeLabel(outcome: MacroStepOutcome): string {
  switch (outcome) {
    case "done":
      return "步骤已完成";
    case "unknown":
      return "请求已发送，请在目标 App 确认结果";
    case "missing_dependency":
      return "所需动作或设置不可用";
    case "permission_denied":
      return "权限未授权";
    case "failed":
      return "执行失败，后续步骤已停止";
  }
}

/** 生成新的组合动作 ID（与库的稳定 ID 规则一致：`macro.` + 32 位十六进制）。 */
export function newMacroId(): string {
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    crypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `macro.${hex}`;
}

/** 生成新的步骤 ID（宏内唯一）。 */
export function newStepId(): string {
  const bytes = new Uint8Array(8);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    crypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  return `step.${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

/** 下一个补丁版本号（编辑保存总是新版本，对齐不可变版本语义）。 */
export function nextMacroVersion(version: string): string {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
  if (!match) return "1.0.0";
  return `${match[1]}.${match[2]}.${Number(match[3]) + 1}`;
}

/** 常用语门禁状态（与 Rust 侧 CommonPhraseAccess 一一对应）。 */
export type CommonPhraseAccess = "unavailable" | "denied" | "allowed";

/** 浮层面板一个位置的数据：只含短显示，不含完整句。 */
export interface CommonPhrasePanelEntry {
  button: string;
  label: string;
}

/** 浮层面板快照（渲染圆盘用）。 */
export interface CommonPhrasePanelSnapshot {
  /** 私有组件是否已注入（未注入时公开构建隐藏入口）。 */
  available: boolean;
  access: CommonPhraseAccess;
  canUse: boolean;
  /** 「常用语示例」只读预览模式（前端据此显示"登录会员后可使用"）。 */
  previewMode: boolean;
  /** 一次性失败提示的原因 key（**读取即消费**，Rust 侧读取后清空）；无提示为 null。 */
  notice: string | null;
  entries: CommonPhrasePanelEntry[];
}

/** 空快照：未注入私有组件或浏览器预览环境的回落值。 */
export function emptyCommonPhrasePanelSnapshot(): CommonPhrasePanelSnapshot {
  return {
    available: false,
    access: "unavailable",
    canUse: false,
    previewMode: false,
    notice: null,
    entries: [],
  };
}

/** 读取浮层面板数据。未注入私有组件时 available=false、entries 为空。 */
export async function getCommonPhrasePanelSnapshot(): Promise<CommonPhrasePanelSnapshot> {
  if (!isTauriRuntime()) {
    return emptyCommonPhrasePanelSnapshot();
  }
  try {
    return await invoke<CommonPhrasePanelSnapshot>("common_phrase_panel_snapshot");
  } catch {
    return emptyCommonPhrasePanelSnapshot();
  }
}

/** 关闭浮层面板（右上角关闭按钮；返回键由遥控器路径关闭）。 */
export async function closeCommonPhrasePanel(reason?: string): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    await invoke("close_common_phrase_panel", { reason: reason ?? null });
  } catch {
    // 关闭失败不阻塞界面：面板会在下一次按键或收尾路径里被关掉。
  }
}

/**
 * 订阅「面板显示」事件（宿主每次 show 面板都会广播）。
 *
 * 存在的理由：面板窗口是 NOACTIVATE，focus/visibility 变化不保证触发，
 * 仅靠前端事件会在"编辑常用语后再打开面板"时显示旧数据（2026-10-16 实测）。
 * 返回取消订阅函数。
 */
export async function onCommonPhrasePanelShown(handler: () => void): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen("common-phrases-panel-shown", () => handler());
  return unlisten;
}

/** 鼠标点选圆盘某个位置（与遥控器按下等价）。返回是否已受理。 */
export async function selectCommonPhrase(button: string): Promise<boolean> {
  if (!isTauriRuntime()) return false;
  try {
    return await invoke<boolean>("select_common_phrase", { button });
  } catch {
    return false;
  }
}

/** 下行命令的返回体（`{"ok":true,"value":...}` / `{"ok":false,"error":"..."}`）。 */
interface CommonPhraseDispatchResult {
  ok: boolean;
  value?: string;
  error?: string;
}

/**
 * 下行命令通道（编辑器"调整常用语"、示例入口等）。未注入组件（公开构建）或
 * 未知命令会抛错——调用方必须按错误处理，不得静默当成功。
 */
async function dispatchCommonPhrase(command: string, payload?: unknown): Promise<string> {
  if (!isTauriRuntime()) {
    throw new Error("unavailable");
  }
  const raw = await invoke<string>("common_phrase_dispatch", {
    command,
    payload: payload === undefined ? null : JSON.stringify(payload),
  });
  const result = JSON.parse(raw) as CommonPhraseDispatchResult;
  if (!result.ok) {
    throw new Error(result.error ?? "edit_failed");
  }
  return result.value ?? "";
}

/** 编辑器（“调整常用语”）条目。 */
export interface CommonPhraseEditorEntry {
  id: string;
  chineseText: string;
  chineseLabel: string;
  englishText: string;
  englishLabel: string;
  /** 内置条目（编辑后自动落成用户副本）。 */
  builtIn: boolean;
}

/** 编辑器快照（“调整常用语”页数据源）。 */
export interface CommonPhraseEditorSnapshot {
  storeAvailable: boolean;
  loadFailed: boolean;
  /** 会员门禁：false 时界面显示会员提示、禁用编辑。 */
  allowed: boolean;
  entries: CommonPhraseEditorEntry[];
  /** 五个位置（ok/left/up/right/down）→ 条目 id；无分配的键位缺键。 */
  assignments: Record<string, string>;
}

/** 读取编辑器快照。未接通时抛错（调用方按会员提示处理）。 */
export async function getCommonPhraseEditorSnapshot(): Promise<CommonPhraseEditorSnapshot> {
  return JSON.parse(await dispatchCommonPhrase("editor_snapshot")) as CommonPhraseEditorSnapshot;
}

/** 编辑器保存输入：不给 id = 新建；给 id = 替换该条目。 */
export interface CommonPhraseEditInput {
  id?: string | null;
  chineseText: string;
  chineseLabel: string;
  englishText?: string;
  englishLabel?: string;
}

/** 保存一条常用语；返回条目 id（新建时为新用户副本 id）。 */
export async function saveCommonPhrase(input: CommonPhraseEditInput): Promise<string> {
  return dispatchCommonPhrase("editor_save", {
    id: input.id ?? null,
    chineseText: input.chineseText,
    chineseLabel: input.chineseLabel,
    englishText: input.englishText ?? "",
    englishLabel: input.englishLabel ?? "",
  });
}

/** 把某个面板位置分配给指定条目（立即保存）。 */
export async function assignCommonPhrase(button: string, id: string): Promise<void> {
  await dispatchCommonPhrase("editor_assign", { button, id });
}

/** 删除一条常用语（同时清掉相关分配）。 */
export async function deleteCommonPhrase(id: string): Promise<void> {
  await dispatchCommonPhrase("editor_delete", { id });
}

/** 按偏移量调整顺序（越界不留痕）。 */
export async function moveCommonPhrase(id: string, offset: number): Promise<void> {
  await dispatchCommonPhrase("editor_move", { id, offset });
}

/** 导入备份（整份替换；被改动的内置项转用户副本保留）。 */
export async function importCommonPhrases(json: string): Promise<void> {
  await dispatchCommonPhrase("editor_import", { json });
}

/** 导出备份（JSON 文本）。 */
export async function exportCommonPhrases(): Promise<string> {
  return dispatchCommonPhrase("editor_export");
}

/** 打开「常用语示例」（会员=普通面板、非会员=只读预览）。 */
export async function openCommonPhraseExample(): Promise<void> {
  await dispatchCommonPhrase("open_example");
}
