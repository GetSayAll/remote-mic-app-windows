import { invoke } from "@tauri-apps/api/core";
import {
  connectRemote,
  disconnectRemote,
  getAudioSnapshot,
  getDiagnosticReport,
  getRuntimeSnapshot,
  listAudioEndpoints,
  saveButtonMappings,
  scanPairedRemotes,
  startRawInput,
  stopRawInput,
  testButtonMapping,
  type PlatformSnapshot,
} from "./lib/bridge";

interface RuntimeSimulationReport {
  passed: boolean;
  platform?: string;
  steps: string[];
  error?: string;
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
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

async function runJourney(steps: string[]): Promise<PlatformSnapshot> {
  await waitFor(
    () => (document.querySelector("h1")?.textContent?.trim() === "连接与语音" ? true : null),
    "连接与语音首页",
  );
  const runtime = await getRuntimeSnapshot();
  assert(runtime.platform.platform === "windows-ci-simulation", "应用未使用 Windows CI 仿真后端");
  steps.push("Tauri WebView 通过真实 IPC 读取仿真运行快照");

  await clickButton("扫描已配对设备");
  await waitFor(
    () => (document.body.textContent?.includes("找到 2 个已批准名称的候选设备") ? true : null),
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
  await waitFor(
    () =>
      Array.from(document.querySelectorAll<HTMLButtonElement>("button")).some((button) =>
        button.textContent?.trim().includes("当前端点"),
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

  await openPage("按键");
  await clickButton("启动监听");
  await waitFor(() => (buttonWithText("停止监听") ? true : null), "Raw Input 就绪状态");
  const rawInput = await startRawInput();
  assert(rawInput.phase === "ready", "仿真 Raw Input 没有进入就绪");
  assert(rawInput.semanticEdgeCount === 2, "仿真 Raw Input 语义边沿数量异常");
  steps.push("按键页面通过 IPC 启动并展示 Raw Input 仿真状态");

  await saveButtonMappings({
    actions: {
      ok: { type: "shortcut", chord: { keys: ["left_control", "c"] } },
    },
  });
  const sendInput = await testButtonMapping("ok");
  assert(sendInput.submittedBatches === 1, "仿真 SendInput 没有提交唯一批次");
  assert(sendInput.submittedEvents === 4, "Ctrl+C 仿真没有生成四个按下/释放事件");
  steps.push("映射保存、热加载和 SendInput 记录器通过真实 Tauri IPC");

  await openPage("权限");
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
  steps.push("权限页面生成去标识化运行诊断摘要");

  await openPage("关于");
  await openPage("连接与语音");
  steps.push("五个侧栏页面均在 Windows WebView 中完成导航和渲染");

  const voice = await invoke<PlatformSnapshot>("run_runtime_simulation_voice_session");
  assert(voice.connection.decodedSamples === 240, "40 + 80 字节语音没有解码为 240 个采样");
  assert(voice.connection.generation === 1, "首次仿真语音会话代次不是 1");
  assert(voice.connection.voiceState === "idle", "仿真语音排空后没有回到 idle");
  assert(voice.audio.submittedSamples === 240, "仿真 WASAPI 没有提交完整 240 个采样");
  steps.push("RC001 首次 STREAM_START → 40+80 AUDIO → STREAM_STOP → DRAIN 闭环完成");

  await stopRawInput();
  await disconnectRemote();
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
