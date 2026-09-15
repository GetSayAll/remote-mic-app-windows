import { createApp } from "vue";
import App from "./App.vue";
import { isTauriRuntime } from "./lib/bridge";
import { initializeTheme } from "./lib/theme";
import { installFrontendDiagnostics, reportFrontendEvent } from "./lib/frontend-diagnostics";
import "./styles.css";

installFrontendDiagnostics();
// 初始化调用在 Vue 挂载前同步应用首帧主题；异步读取设置不阻塞主界面。
void initializeTheme();
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

// 焦点环模态跟踪（见 styles.css body.kb-nav 注释）：仅真实 Tab 导航显示
// 焦点环；遥控器注入的快捷键属于键盘模态但不该点亮焦点环，故只认 Tab。
window.addEventListener(
  "keydown",
  (event) => {
    if (event.key === "Tab") document.body.classList.add("kb-nav");
  },
  true,
);
window.addEventListener(
  "mousedown",
  () => document.body.classList.remove("kb-nav"),
  true,
);

if (import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1") {
  void import("./runtime-simulation").then(({ runRuntimeSimulationSmoke }) =>
    runRuntimeSimulationSmoke(),
  );
}
