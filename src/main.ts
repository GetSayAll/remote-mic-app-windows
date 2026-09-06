import { createApp } from "vue";
import App from "./App.vue";
import { isTauriRuntime } from "./lib/bridge";
import "./styles.css";

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
