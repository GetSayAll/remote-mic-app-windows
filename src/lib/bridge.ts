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
}

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
  | { type: "focus_input" };

/** 预设应用条目（list_preset_apps 返回；对齐 Mac PresetApplication）。 */
export interface PresetAppInfo {
  id: string;
  name: string;
  installed: boolean;
}

/** 打开应用后的聚焦方式（对齐 Rust `FocusStrategy`）。 */
export type FocusStrategy = "open_only" | "app_shortcut" | "recorded_element";

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

/** 「打开应用」目标的聚焦档案（键 = open_app 的 target）。 */
export interface AppFocusProfile {
  strategy: FocusStrategy;
  shortcut?: KeyChord;
  recorded?: RecordedFocusTarget;
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
}

export interface PairedRemote {
  id: string;
  name: string;
  model: RemoteModel;
  isSupportedCandidate: boolean;
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

export interface AppUpdateProgress {
  downloaded: number;
  contentLength: number | null;
  finished: boolean;
}

const browserSnapshot: RuntimeSnapshot = {
  // 浏览器预览没有安装包可读，这里跟随当前应用版本：它是预览里"设置页版本号"
  // 与侧栏底部的唯一来源，写死旧值会让预览显示一个不存在的版本。
  appVersion: "0.8.1",
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

export async function getDiagnosticReport(): Promise<DiagnosticReport> {
  if (!isTauriRuntime()) {
    return {
      schemaVersion: 2,
      appVersion: browserSnapshot.appVersion,
      appBuild: "unknown",
      sourceRevision: "unknown",
      buildChannel: "unknown",
      windowsVersion: "unknown",
      processArchitecture: "browser-preview",
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
      { id: "edge", name: "Edge 浏览器", installed: true },
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

export function actionSummary(action: ButtonAction | undefined): string {
  if (!action || action.type === "disabled") return "未设置";
  if (action.type === "focus_input") return "聚焦输入框";
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
