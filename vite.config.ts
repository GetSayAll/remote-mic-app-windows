import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 2430,
    strictPort: true,
    watch: {
      ignored: ["**/target/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
