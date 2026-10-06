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

// ── arm64 双载荷（可选，2026-10-07 issue206）────────────────────────────
// 与默认配置的关系：src-tauri/tauri.conf.json 的 bundle.resources **只声明上面两件
// x64**，理由见 stage-bundle-inputs.cjs 顶部与 RELEASING.md（tauri-build 在编译期
// 校验资源存在，而 arm64 助手要 VS 的 VC.Tools.ARM64 才能构建；写进默认配置会让
// 本机任何 cargo check / cargo test 直接失败）。这两件只有在**真的暂存成功**之后，
// 才由 src-tauri/tauri.arm64-payload.conf.json 覆盖式声明，安装包才携带它们。
//
// 缺 arm64 源时的行为（两份要么都暂存、要么都不暂存，避免半套载荷被 --config 声明）：
//   * 默认：打印警告并跳过（本机/部分环境没有 ARM64 工具集，缺它不是错误）；
//   * SAYALL_REQUIRE_ARM64_PAYLOAD=1（发布 / CI 的 installer job）：退出码 3 硬失败。
const arm64Pairs = [
  ["hardware/RC003/helper/target/aarch64-pc-windows-msvc/release/sayall-helper.exe", "sayall-helper-arm64.exe"],
  ["hardware/RC003/helper/vendor/frida-gadget-arm64.dll", "frida-gadget-arm64.dll"],
];

const missingArm64 = arm64Pairs.filter(([src]) => !fs.existsSync(path.join(root, src)));
if (missingArm64.length > 0 && process.env.SAYALL_REQUIRE_ARM64_PAYLOAD === "1") {
  console.error("[stage] SAYALL_REQUIRE_ARM64_PAYLOAD=1 但 arm64 载荷缺失，终止（本路径要求安装包必须含 arm64）：");
  for (const [src] of missingArm64) {
    console.error("[stage]   缺：" + src);
  }
  process.exit(3);
}
if (missingArm64.length > 0) {
  console.warn("[stage] 警告：arm64 载荷缺失，本次只暂存 x64 两件（与默认 tauri.conf.json 一致）。");
  for (const [src] of missingArm64) {
    console.warn("[stage]   缺：" + src);
  }
  if (missingArm64.length < arm64Pairs.length) {
    console.warn("[stage]   只到齐一半 → 两份都不暂存，避免半套载荷被 arm64 覆盖配置声明。");
  }
  console.warn("[stage]   影响：安装包不含 arm64 载荷；Windows 11 ARM64 上应用会明确提示不支持\"全按键支持\"。");
  console.warn("[stage]   补法：见 scripts/stage-bundle-inputs.cjs 的工具链检测提示。");
}

for (const [src, dst] of pairs.concat(missingArm64.length === 0 ? arm64Pairs : [])) {
  const from = path.join(root, src);
  const to = path.join(root, "src-tauri", dst);
  fs.copyFileSync(from, to);
  console.log("[stage] " + dst + " <- " + src);
}
