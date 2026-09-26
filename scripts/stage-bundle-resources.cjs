// 出包前把助手与 Gadget 暂存到 src-tauri 根目录。
// 为什么：tauri 资源的落盘路径 = 安装目录 + 源相对路径（实测：
// map 目标对单文件不可靠、../ 会变成 _up_ 嵌套目录）。要让文件落在
// 安装根目录（主程序与助手同目录的产品化布局），唯一可靠的做法是
// 让源文件就在 src-tauri 根——资源条目写裸文件名。
// 暂存产物不入库（.gitignore）。node __dirname 定位，与调用时 cwd 无关。
const fs = require("fs");
const path = require("path");
const { execFileSync } = require("child_process");

const root = path.join(__dirname, "..");

// ── 出包前必须重新构建前端（2026-09-25 血泪教训）──────────────────────
// 前端产物 `dist/` 只在 vite build 时更新。出包流程若绕过 `tauri build`
// （直接 cargo build + tauri bundle，见本机为避免编译冻结的做法），就没人
// 跑这一步——结果：**Vue 改动全部没有进过安装包**，界面类修复看起来
// 「改了却没生效」，排查成本极高。放在这里，出包一定带最新前端。
execFileSync(process.execPath, [path.join(root, "node_modules", "vite", "bin", "vite.js"), "build"], {
  cwd: root,
  stdio: "inherit",
});
const pairs = [
  ["hardware/RC003/helper/target/release/sayall-helper.exe", "sayall-helper.exe"],
  ["hardware/RC003/helper/vendor/frida-gadget.dll", "frida-gadget.dll"],
];

for (const [src, dst] of pairs) {
  const from = path.join(root, src);
  const to = path.join(root, "src-tauri", dst);
  fs.copyFileSync(from, to);
  console.log("[stage] " + dst + " <- " + src);
}
