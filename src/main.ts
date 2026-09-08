import { createApp } from "vue";
import App from "./App.vue";
import { isTauriRuntime } from "./lib/bridge";
import { initializeTheme } from "./lib/theme";
import "./styles.css";

// 初始化调用在 Vue 挂载前同步应用首帧主题；异步读取设置不阻塞主界面。
void initializeTheme();
createApp(App).mount("#app");

// 桌面应用内禁用 WebView 默认右键菜单（浏览器预览保留原生菜单，便于调试）。
if (isTauriRuntime()) {
  document.addEventListener("contextmenu", (event) => event.preventDefault());
}

if (import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1") {
  void import("./runtime-simulation").then(({ runRuntimeSimulationSmoke }) =>
    runRuntimeSimulationSmoke(),
  );
}
