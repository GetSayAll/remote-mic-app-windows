import { createApp } from "vue";
import App from "./App.vue";
import { isTauriRuntime } from "./lib/bridge";
import { initializeAccentColor } from "./lib/accent";
import { initializeTheme } from "./lib/theme";
import { installFrontendDiagnostics, reportFrontendEvent } from "./lib/frontend-diagnostics";
import { installFocusModalityTracking } from "./lib/focus-modality";
import { installKeyProbe } from "./lib/key-probe";
import "./styles.css";

installFrontendDiagnostics();
// 初始化调用在 Vue 挂载前同步应用首帧主题；异步读取设置不阻塞主界面。
void initializeTheme();
// 系统强调色：拉取一次并注入 --accent* 变量；后续系统侧变化由
// "system-accent-changed" 事件实时推送（Rust WM_SETTINGCHANGE 监听），
// 用户在系统设置里换强调色无需重启应用。读取失败时保留 styles.css
// 内置默认色，不影响启动。
void initializeAccentColor();
try {
  const app = createApp(App);
  app.config.errorHandler = () => {
    reportFrontendEvent({
      event: "vue_runtime_error",
      phase: "completed",
      result: "failed",
      reason: "component_render_or_lifecycle_failed",
    });
  };
  app.mount("#app");
  reportFrontendEvent({
    event: "vue_mount",
    phase: "completed",
    result: "passed",
    reason: "root_component_mounted",
  });
} catch {
  reportFrontendEvent({
    event: "vue_mount",
    phase: "completed",
    result: "failed",
    reason: "root_component_mount_failed",
  });
  const root = document.querySelector<HTMLElement>("#app");
  if (root) root.textContent = "无线麦启动失败。请将诊断日志发送给技术支持。";
}

// 桌面应用内禁用 WebView 默认右键菜单（浏览器预览保留原生菜单，便于调试）。
if (isTauriRuntime()) {
  document.addEventListener("contextmenu", (event) => event.preventDefault());
}

// 焦点环模态跟踪（实现见 src/lib/focus-modality.ts，样式见 styles.css）：
// 默认保留原生焦点指示，仅在最近一次交互是指针时抑制，消除遥控器按键在
// 鼠标点过的控件上凭空点亮的幽灵焦点环。
installFocusModalityTracking();

// 临时诊断（2026-10-04）：Alt 类键 DOM 到达取证（WebView2 键投递排查），
// 完成后随修复提交移除或转正。
installKeyProbe();

if (import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1") {
  void import("./runtime-simulation").then(({ runRuntimeSimulationSmoke }) =>
    runRuntimeSimulationSmoke(),
  );
}
