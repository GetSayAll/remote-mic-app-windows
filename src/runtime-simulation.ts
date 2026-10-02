import { invoke } from "@tauri-apps/api/core";
import {
  connectRemote,
  disconnectRemote,
  getAudioSnapshot,
  getButtonMappings,
  getDiagnosticReport,
  getRawInputSnapshot,
  getRuntimeSnapshot,
  listAudioEndpoints,
  saveButtonMappings,
  scanPairedRemotes,
  setAppIcon,
  stopRawInput,
  testButtonMapping,
  type PlatformSnapshot,
} from "./lib/bridge";
import { reportFrontendEvent } from "./lib/frontend-diagnostics";

interface RuntimeSimulationReport {
  passed: boolean;
  platform?: string;
  steps: string[];
  error?: string;
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/**
 * 阶段标记：写进诊断日志（`runtime_simulation phase=completed reason=<stage>`）。
 *
 * 2026-10-02 教训：本机仿真超时 60 秒时，报告没写、stdout/stderr 全空，日志里
 * 只有每秒一条 `tray_icon_state ... ipc_unavailable`，无法判断卡在哪一步——
 * 是因为那次构建漏了 `VITE_SAYALL_RUNTIME_SIMULATION=1`（仿真前端入口根本没
 * 编进去）。每个阶段入口留一条日志后，同样的现场一次日志拉取就能定位。
 */
function mark(stage: string): void {
  reportFrontendEvent({
    event: "runtime_simulation",
    phase: "started",
    result: "passed",
    reason: stage,
  });
}

async function waitFor<T>(read: () => T | null, description: string): Promise<T> {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    const value = read();
    if (value !== null) return value;
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  throw new Error(`等待 ${description} 超时`);
}

function buttonWithText(label: string): HTMLButtonElement | null {
  return (
    Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent?.trim().includes(label) && !button.disabled,
    ) ?? null
  );
}

async function clickButton(label: string): Promise<void> {
  const button = await waitFor(() => buttonWithText(label), `按钮“${label}”可用`);
  button.click();
}

async function openPage(label: string, heading = label): Promise<void> {
  const button = await waitFor(
    () =>
      Array.from(document.querySelectorAll<HTMLButtonElement>("nav button")).find(
        (candidate) => candidate.textContent?.trim().includes(label),
      ) ?? null,
    `导航“${label}”`,
  );
  button.click();
  await waitFor(
    () => (document.querySelector("h1")?.textContent?.trim() === heading ? heading : null),
    `页面“${heading}”`,
  );
}

/** 仿真选项：本机运行时跳过「外部入口」两步，避免反复弹出系统浏览器。 */
interface RuntimeSimulationOptions {
  skipExternalEntries: boolean;
}

async function loadSimulationOptions(): Promise<RuntimeSimulationOptions> {
  try {
    return await invoke<RuntimeSimulationOptions>("get_runtime_simulation_options");
  } catch {
    // 拿不到选项时按 CI 口径（执行外部入口），不静默降级覆盖面。
    return { skipExternalEntries: false };
  }
}

/**
 * 设置页外部入口（官网 / GitHub）的 CI 判据。
 *
 * 2026-10-02 用户指定：成功不再显示任何提示，只有失败就地给原因。因此判据是
 * "在窗口期内要么出现失败原因、要么保持静默"；capability 白名单是产品配置，
 * 被 opener 拒绝（"Not allowed to open url"）必须报红，runner 上没有可用的
 * 默认浏览器只是环境差异（失败文案会如实带出来），不制造与本产品无关的红灯。
 *
 * 本机运行（脚本未带 -IncludeExternalEntries 且非 CI）时这两步会弹出系统浏览器，
 * 打断操作人：此时**如实记为 deferred 步骤**并跳过点击（不是静默丢覆盖面）。
 */
async function recordExternalEntry(
  label: string,
  steps: string[],
  skip: boolean,
): Promise<void> {
  if (skip) {
    // 本机运行：跳过点击（避免弹出系统浏览器），如实记为 deferred。
    steps.push(`设置页“${label}”入口本机跳过（避免弹出浏览器）：capability 往返未验证`);
    return;
  }
  await clickButton(label);
  const deadline = Date.now() + 3_000;
  let message: string | null = null;
  while (Date.now() < deadline) {
    const text = document.querySelector(".link-message")?.textContent?.trim();
    if (text) {
      message = text;
      break;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  }
  assert(
    message === null || !message.includes("Not allowed to open url"),
    `设置页“${label}”入口被 opener capability 拒绝：${message}`,
  );
  steps.push(
    message === null
      ? `设置页“${label}”入口经真实 IPC 交由系统浏览器打开（成功不显示提示）`
      : `设置页“${label}”入口返回不可用（deferred）：${message}`,
  );
}

async function runJourney(steps: string[]): Promise<PlatformSnapshot> {
  mark("journey_start");
  const simulationOptions = await loadSimulationOptions();
  // 应用默认打开"按键"页（对齐 Mac 页序），先导航到连接页完成连接旅程。
  await openPage("连接");
  await waitFor(
    () => (document.querySelector("h1")?.textContent?.trim() === "连接" ? true : null),
    "连接首页",
  );
  const runtime = await getRuntimeSnapshot();
  assert(runtime.platform.platform === "windows-ci-simulation", "应用未使用 Windows CI 仿真后端");
  steps.push("Tauri WebView 通过真实 IPC 读取仿真运行快照");

  await clickButton("扫描已配对设备");
  await waitFor(
    () => (document.body.textContent?.includes("找到 2 个已配对的小米遥控器") ? true : null),
    "RC001/RC003 扫描结果",
  );
  const remotes = await scanPairedRemotes();
  assert(remotes.length === 2, "仿真扫描没有同时返回 RC001 和 RC003");
  assert(remotes.some((remote) => remote.model === "rc001"), "仿真扫描缺少 RC001");
  assert(remotes.some((remote) => remote.model === "rc003"), "仿真扫描缺少 RC003");
  steps.push("连接页面渲染 RC001/RC003 扫描结果");

  const rc001 = remotes.find((remote) => remote.model === "rc001");
  assert(rc001, "找不到 RC001 仿真设备");
  const connection = await connectRemote(rc001.id);
  assert(connection.phase === "ready", "RC001 仿真连接没有进入 ATVV 就绪");
  assert(connection.capabilities?.sampleRate === 16_000, "RC001 仿真能力不是 16 kHz");
  steps.push("RC001 连接 command 返回 16 kHz ATVV 就绪状态");

  await waitFor(
    () => (document.body.textContent?.includes("CABLE Input (CI Simulation)") ? true : null),
    "仿真音频端点",
  );
  // 端点列表默认收起（用户每次只用一个）：自动选择后以"更换设备"入口呈现。
  await waitFor(
    () =>
      Array.from(document.querySelectorAll<HTMLButtonElement>("button")).some((button) =>
        button.textContent?.trim().includes("更换设备"),
      )
        ? true
        : null,
    "自动选择仿真 CABLE Input",
  );
  const endpoints = await listAudioEndpoints();
  assert(endpoints.length === 1, "仿真音频端点数量异常");
  const audio = await getAudioSnapshot();
  assert(audio.phase === "ready", "仿真音频端点没有进入 WASAPI 就绪");
  assert(audio.selectedEndpointId === endpoints[0].id, "仿真 CABLE Input 没有被自动选择");
  steps.push("连接页面首次检测并自动选择唯一的仿真 CABLE Input");

  mark("buttons_page");
  await openPage("按键", "按键映射");
  await waitFor(
    () => (document.body.textContent?.includes("按键监听已就绪") ? true : null),
    "Raw Input 就绪状态（随应用自愈启动）",
  );
  const rawInput = await getRawInputSnapshot();
  assert(rawInput.phase === "ready", "仿真 Raw Input 没有进入就绪");
  assert(rawInput.semanticEdgeCount === 2, "仿真 Raw Input 语义边沿数量异常");
  steps.push("按键页面随应用启动展示 Raw Input 仿真状态");
  assert(
    document.body.textContent?.includes("语音键"),
    "按键画布没有渲染语音键卡片",
  );
  assert(
    Array.from(document.querySelectorAll(".mapping-cell")).length === 36,
    "按键画布没有渲染 12 键 × 3 触发方式的单元格",
  );
  steps.push("按键映射画布渲染 12 张按键卡与三列触发单元格");

  await saveButtonMappings({
    enabled: true,
    actions: {
      ok: {
        single: { type: "shortcut", chord: { keys: ["left_control", "c"] } },
        double: { type: "disabled" },
        long: { type: "disabled" },
      },
    },
  });
  const sendInput = await testButtonMapping("ok", "single");
  assert(sendInput.submittedBatches === 1, "仿真 SendInput 没有提交唯一批次");
  assert(sendInput.submittedEvents === 4, "Ctrl+C 仿真没有生成四个按下/释放事件");
  steps.push("映射保存、热加载和 SendInput 记录器通过真实 Tauri IPC");

  // 2026-10-03 关闭态语义：仿真设置目录每轮重建（「全按键支持」默认关闭），
  // 返回 / 音量+ / 音量− 三张卡片整卡置灰、格子禁用、悬停给出开启指引——
  // 这条在真实 WebView 里钉住（单元测试另有覆盖），也是下面改用「确定」键的原因。
  // 用 waitFor 而不是即时断言：真实 WebView 的 localStorage 可能残留上一轮的
  // 开关缓存（本机仿真就命中过：首帧按缓存渲染成开启、首次对账后才回落关闭），
  // 权威状态落地前的过渡帧不是本步要验证的东西。
  const gatedBackCard = await waitFor(() => {
    const card = Array.from(document.querySelectorAll<HTMLElement>(".mapping-card")).find(
      (candidate) => candidate.querySelector(".mapping-card-title strong")?.textContent === "返回",
    );
    return card?.classList.contains("is-locked") ? card : null;
  }, "关闭态返回卡片整卡置灰");
  const gatedBackCell = gatedBackCard.querySelector<HTMLButtonElement>(".mapping-cell");
  assert(gatedBackCell?.disabled === true, "关闭态返回格子没有禁用");
  assert(
    (gatedBackCell?.title ?? "").includes("需要开启全按键支持才能使用，开启后恢复正常"),
    `关闭态返回格子悬停提示异常：${gatedBackCell?.title ?? ""}`,
  );
  steps.push("「全按键支持」关闭时返回/音量± 整卡置灰禁用并给出开启指引");

  // 「设备操作 / 聚焦输入框」芯片自 2026-10-03 起按用户要求隐藏（见
  // src/lib/feature-flags.ts DEVICE_ACTION_SECTION_ENABLED=false），动作与平台受理路径
  // 保留：这里钉住芯片缺席，映射改为经真实 IPC 直接写入，再验证平台受理。
  // 打开编辑器仍走「确定 · 单击」格子——关闭态下三键不可点击，上一步已钉住。
  const focusCell = await waitFor(() => {
    const cell = Array.from(document.querySelectorAll<HTMLButtonElement>(".mapping-cell")).find(
      (candidate) => candidate.title.startsWith("确定 · 单击"),
    );
    return cell ?? null;
  }, "确定键「单击」映射格");
  focusCell.click();
  await waitFor(() => (document.querySelector(".mapping-editor") ? true : null), "按键编辑器");
  const editorButtons = Array.from(
    document.querySelectorAll<HTMLButtonElement>(".mapping-editor button"),
  );
  assert(
    !editorButtons.some((button) => button.textContent?.trim() === "聚焦输入框"),
    "「聚焦输入框」芯片应已隐藏",
  );
  const currentMappings = await getButtonMappings();
  const currentOk = currentMappings.actions.ok;
  assert(currentOk !== undefined, "确定键映射缺失（仿真前置步骤应已写入）");
  await saveButtonMappings({
    ...currentMappings,
    actions: {
      ...currentMappings.actions,
      ok: { ...currentOk, single: { type: "focus_input" } },
    },
  });
  const focusSnapshot = await testButtonMapping("ok", "single");
  assert(
    focusSnapshot.submittedBatches === 2,
    "「聚焦输入框」动作没有分发到平台的聚焦受理路径",
  );
  steps.push("「聚焦输入框」动作经真实 IPC 写入并分发到平台聚焦受理（UI 芯片按用户要求隐藏）");

  // 「打开应用 + 聚焦方式」：选预设应用 → 切到「聚焦已记录的输入框」→ 学习一次 →
  // 读回档案 → 测试打开与聚焦。学习与测试都走真实 IPC（仿真返回固定样本）。
  await clickButton("记事本");
  await waitFor(
    () =>
      document.body.textContent?.includes("打开后聚焦方式") &&
      document.body.textContent?.includes("聚焦已记录的输入框")
        ? true
        : null,
    "聚焦方式面板",
  );
  mark("focus_profile_panel");
  await clickButton("聚焦已记录的输入框");
  await clickButton("开始学习输入框");
  await waitFor(
    () => (document.body.textContent?.includes("只记录控件特征，不含输入内容") ? true : null),
    "学习结果提示",
  );
  await waitFor(
    () => (document.body.textContent?.includes("已记录输入框") ? true : null),
    "聚焦档案呈现为已记录输入框",
  );
  const savedMappings = await getButtonMappings();
  const profile = savedMappings.focusProfiles?.["notepad"];
  assert(profile?.strategy === "recorded_element", "聚焦档案没有保存为已记录输入框");
  assert(
    profile?.recorded?.automationId === "ci-simulation-input",
    "聚焦档案没有记录到仿真输入框",
  );
  await clickButton("测试打开与聚焦");
  steps.push("打开应用的聚焦方式三选一、学习输入框与测试打开聚焦经真实 IPC 闭环");

  mark("permissions_page");
  await openPage("权限");
  // 诊断摘要 2026-10-01 从关于页迁回权限页（用户指定）：状态与取证入口同页，
  // 用户看到某一项不对时不必跳页。
  await clickButton("生成摘要");
  await waitFor(
    () =>
      document.querySelector(".diagnostic-output")?.textContent?.includes("windows-ci-simulation")
        ? true
        : null,
    "诊断摘要渲染",
  );
  const diagnostic = await getDiagnosticReport();
  assert(diagnostic.platform === "windows-ci-simulation", "诊断摘要没有来自仿真平台");
  assert(diagnostic.capabilities.bleVoiceReady, "诊断摘要没有反映 ATVV 就绪");
  assert(diagnostic.capabilities.wasapiReady, "诊断摘要没有反映 WASAPI 就绪");
  assert(diagnostic.capabilities.rawInputReady, "诊断摘要没有反映 Raw Input 就绪");
  steps.push("权限页呈现蓝牙/按键/音频三项状态并生成去标识化运行诊断摘要");

  // "打开日志目录"刻意**不作成败断言**：它经 ShellExecuteW 交给资源管理器，
  // CI runner 是否有可用的 shell 桌面不在本仓库控制范围内，拿它当门禁只会
  // 制造与本产品无关的红灯。这里只证明 WebView → IPC → Rust → shell 的往返
  // 真的走通（按钮回到可用、消息落在终态），并把结果如实记入步骤文本。
  await clickButton("打开日志目录");
  const logDirectoryMessage = await waitFor(
    () => {
      const text = document.querySelector(".log-directory-message")?.textContent?.trim();
      return text && text !== "正在打开日志目录…" ? text : null;
    },
    "日志目录入口返回终态",
  );
  steps.push(
    logDirectoryMessage.startsWith("已打开日志目录")
      ? "权限页“打开日志目录”经真实 IPC 交由资源管理器打开"
      : `权限页“打开日志目录”返回不可用（deferred）：${logDirectoryMessage}`,
  );

  await openPage("设置");
  mark("settings_page");
  // 2026-10-02 设置页改版（对齐 Mac 新设置页）：顶部模块 = 应用标识 + 版本 +
  // 检查更新；通用 / 问题反馈两个分组在卡外有分组标题。
  const settingsOverview = document.querySelector<HTMLElement>("article.settings-overview");
  assert(settingsOverview !== null, "设置页缺少顶部标识与检查更新模块");
  assert(
    settingsOverview.textContent?.includes("当前版本") === true,
    "设置页顶部没有显示当前版本",
  );
  assert(
    document.querySelector("article.settings-overview .check-button") !== null,
    "设置页顶部没有检查更新入口",
  );
  const sectionTitles = Array.from(
    document.querySelectorAll<HTMLElement>(".settings-section .section-title"),
  ).map((element) => element.textContent?.trim());
  assert(
    sectionTitles.join(" / ") === "通用 / 问题反馈",
    `设置页分组标题异常：${sectionTitles.join(" / ")}`,
  );
  const entryLabels = Array.from(
    document.querySelectorAll<HTMLButtonElement>(".settings-section .button-row button"),
  ).map((button) => button.textContent?.trim());
  assert(
    entryLabels.length === 2 && entryLabels[0] === "官网" && entryLabels[1] === "GitHub",
    `设置页问题反馈入口异常：${entryLabels.join(" / ")}`,
  );
  steps.push("设置页顶部为应用标识/版本/检查更新，问题反馈分组提供官网与 GitHub 入口");
  await recordExternalEntry("官网", steps, simulationOptions.skipExternalEntries);
  await recordExternalEntry("GitHub", steps, simulationOptions.skipExternalEntries);

  const darkTheme = await waitFor(
    () =>
      document.querySelector<HTMLInputElement>('input[name="theme-preference"][value="dark"]:not(:disabled)'),
    "深色外观选项可用",
  );
  darkTheme.click();
  await waitFor(
    () => (document.documentElement.dataset.theme === "dark" ? true : null),
    "深色外观应用",
  );
  assert(darkTheme.checked, "深色外观保存后没有保持选中");
  assert(!document.querySelector('[role="alert"]'), "深色外观保存后显示错误");

  const systemTheme = await waitFor(
    () =>
      document.querySelector<HTMLInputElement>('input[name="theme-preference"][value="system"]:not(:disabled)'),
    "系统外观选项可用",
  );
  systemTheme.click();
  await waitFor(() => (systemTheme.checked && !systemTheme.disabled ? true : null), "系统外观恢复");
  assert(!document.querySelector('[role="alert"]'), "恢复系统外观后显示错误");
  steps.push("设置页深色/系统外观经 Windows WebView、Tauri capability 与设置持久化闭环");

  // 应用图标（2026-10-02）：仿真后端不建托盘，这里证明选项、IPC 与持久化往返
  // 可用，并且窗口图标接口不报错（真实托盘/任务栏换图属真机验收，见
  // Testing/WindowsRC003Preview.md 用例十四）。
  const appIconOption = document.querySelector<HTMLInputElement>(
    'input[name="app-icon"][value="faceted-duck"]:not(:disabled)',
  );
  assert(appIconOption !== null, "设置页缺少应用图标选项");
  appIconOption.click();
  await waitFor(() => (appIconOption.checked ? true : null), "应用图标切换");
  assert(!document.querySelector('[role="alert"]'), "切换应用图标后显示错误");
  // 直达断言：命令参数契约（前端 `{ identifier }` ↔ Rust 命令参数名）。名字不匹配
  // 时 Tauri 判成缺参，前端只会看到 "IPC 不可用"（2026-10-02 本机仿真现场教训）。
  const appliedIcon = await setAppIcon("faceted-duck");
  assert(appliedIcon === "faceted-duck", `仿真切换应用图标没有生效：${appliedIcon}`);
  const restoredIcon = await setAppIcon("standard");
  assert(restoredIcon === "standard", `仿真还原应用图标没有生效：${restoredIcon}`);
  steps.push("设置页应用图标选项经真实 IPC 与设置持久化往返");

  await openPage("连接");
  steps.push("四个侧栏页面均在 Windows WebView 中完成导航和渲染");

  mark("voice_session");
  const voice = await invoke<PlatformSnapshot>("run_runtime_simulation_voice_session");
  assert(voice.connection.decodedSamples === 240, "40 + 80 字节语音没有解码为 240 个采样");
  assert(voice.connection.generation === 1, "首次仿真语音会话代次不是 1");
  assert(voice.connection.voiceState === "idle", "仿真语音排空后没有回到 idle");
  assert(voice.audio.submittedSamples === 240, "仿真 WASAPI 没有提交完整 240 个采样");
  steps.push("RC001 首次 STREAM_START → 40+80 AUDIO → STREAM_STOP → DRAIN 闭环完成");

  await stopRawInput();
  await disconnectRemote();
  mark("cleanup");
  const finalSnapshot = await getRuntimeSnapshot();
  assert(finalSnapshot.platform.connection.phase === "disconnected", "仿真连接没有释放");
  assert(finalSnapshot.platform.rawInput.phase === "stopped", "仿真 Raw Input 没有停止");
  steps.push("断开和停止 command 完成资源状态释放");
  return voice;
}

export async function runRuntimeSimulationSmoke(): Promise<void> {
  const report: RuntimeSimulationReport = { passed: false, steps: [] };
  try {
    const snapshot = await runJourney(report.steps);
    report.passed = true;
    report.platform = snapshot.platform;
  } catch (error) {
    report.error = error instanceof Error ? error.message : String(error);
  }
  await invoke("complete_runtime_simulation_smoke", { result: report });
}
