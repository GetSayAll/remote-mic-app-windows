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

if (import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1") {
  void import("./runtime-simulation").then(({ runRuntimeSimulationSmoke }) =>
    runRuntimeSimulationSmoke(),
  );
}
