import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";

createApp(App).mount("#app");

if (import.meta.env.VITE_SAYALL_RUNTIME_SIMULATION === "1") {
  void import("./runtime-simulation").then(({ runRuntimeSimulationSmoke }) =>
    runRuntimeSimulationSmoke(),
  );
}
