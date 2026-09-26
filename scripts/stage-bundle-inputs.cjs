// 出包 / CI 前置：确保 tauri.conf 声明的两件 bundle 资源**真的存在**。
//
// 为什么必须有这一步：src-tauri/tauri.conf.json 的 bundle.resources 声明了
// `sayall-helper.exe` 与 `frida-gadget.dll`，而这两个文件按仓库约定**不入库**
// （见 .gitignore：gadget 是 23 MB 第三方二进制，由 vendor/frida-gadget.lock.json
// 锁定 SHA-256）。tauri-build 在**编译期**就校验资源路径是否存在，于是任何只跑
// cargo test / cargo check 的步骤都会直接失败：
//     2026-09-27 PR #127 实测：resource path `sayall-helper.exe` doesn't exist
// 结论：凡是要编译 src-tauri 的环境（CI verify、发布流程、本机出包）都必须先跑本脚本。
//
// 职责边界：**只负责"让输入存在并落地"**。
//   - 完整性与版本由 `vendor/frida-gadget.lock.json` 作唯一事实来源，
//     `vendor/fetch_frida_gadget.py` 负责下载 + 双重 SHA-256 校验（失败即非零退出）；
//   - 助手由 cargo 现场构建（产物新鲜由 cargo 的增量机制保证）。
// 这里刻意**不再重复**一遍尺寸/摘要断言：那是 fetch 的后置条件，永远不可能失败，
// 加进来只会变成"看着像防线、实际从不触发"的摆设。
const { execFileSync } = require("child_process");
const path = require("path");

const root = path.join(__dirname, "..");
const helperManifest = path.join(root, "hardware/RC003/helper/Cargo.toml");
const vendorDir = path.join(root, "hardware/RC003/helper/vendor");

console.log("[inputs] 1/3 构建助手（release）");
execFileSync("cargo", ["build", "--release", "--manifest-path", helperManifest], {
  cwd: root,
  stdio: "inherit",
});

console.log("[inputs] 2/3 按锁文件获取并校验 frida-gadget（含双重 SHA-256 校验）");
execFileSync("python", [path.join(vendorDir, "fetch_frida_gadget.py")], {
  cwd: root,
  stdio: "inherit",
});

console.log("[inputs] 3/3 落地到 src-tauri（复用 stage-bundle-resources.cjs，单一落地路径）");
execFileSync(process.execPath, [path.join(__dirname, "stage-bundle-resources.cjs")], {
  cwd: root,
  stdio: "inherit",
});
