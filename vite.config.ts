import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    // 绑定 IPv4 回环：默认只监听 ::1 时，WebView2 走 127.0.0.1 连不上 dev server，
    // 页面会一直停在 document_load（dev-only 配置，不影响生产构建）。
    host: "127.0.0.1",
    port: 2430,
    strictPort: true,
    watch: {
      ignored: ["**/target/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
