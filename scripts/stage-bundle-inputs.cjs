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
//
// ── arm64 双载荷（2026-10-07，issue206）──────────────────────────────────
// 安装包按设计**同时携带 x64 与 arm64 两份**助手与 Gadget（主程序本身仍是 x64
// 单包、单安装器、单更新通道；运行时按系统原生架构选择用哪一份）。原因：
// Windows 11 ARM64 上"全按键支持"只能注入原生 ARM64 的 WUDFHost.exe，x64 助手
// 进不去（见 docs/decisions/0003-arm64-enhanced-capture-scope.md 与
// Bugs/2026-10-07-issue206-arm64-unsupported.md）。
//
// **为什么默认的 src-tauri/tauri.conf.json 里没有 arm64 两件**：
//   tauri-build 在**编译期**校验资源路径是否存在（见本文件开头 2026-09-27 实测），
//   而 arm64 助手要链接成 aarch64-pc-windows-msvc 就需要 VS 的
//   `Microsoft.VisualStudio.Component.VC.Tools.ARM64` 组件——本机与部分环境没有它。
//   一旦把 arm64 写进默认配置，任何 `cargo check` / `cargo test` 都会直接失败，
//   破坏全仓库的日常验证回路。
//   所以 arm64 两件走**覆盖式声明**：只有真的暂存到 src-tauri/ 之后，出包/CI 才用
//   `--config src-tauri/tauri.arm64-payload.conf.json` 把 bundle.resources 覆盖成四件
//   （tauri 的 `--config` 可重复，按 RFC 7396 顺序合并，数组整体覆盖）。详见 RELEASING.md。
//
// 三条分支：
//   1. 工具链齐备（rustup 有 aarch64 目标 + vswhere 能查到 VC.Tools.ARM64）
//      → 构建 arm64 助手、按锁文件取 arm64 Gadget，安装包含四件载荷；
//   2. 工具链缺失 → 打印明确警告（缺什么、怎么补、本包不含什么）后**继续**，
//      安装包只含 x64 两件；ARM64 机器上应用会明确提示"不支持全按键支持"；
//   3. 工具链缺失 **且** SAYALL_REQUIRE_ARM64_PAYLOAD=1 → 退出码 3 硬失败。
//      发布 / CI 的 installer job **必须**设这个变量：发布路径不允许静默退化成
//      单载荷包（见 RELEASING.md）。
//
// 诊断（不构建任何东西，只跑工具链检测与上面的报告逻辑）：
//     node scripts/stage-bundle-inputs.cjs --check-arm64
//   不带环境变量 → 打印警告、退出 0；带 SAYALL_REQUIRE_ARM64_PAYLOAD=1 → 退出 3。
const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
const helperManifest = path.join(root, "hardware/RC003/helper/Cargo.toml");
const vendorDir = path.join(root, "hardware/RC003/helper/vendor");
const fetchScript = path.join(vendorDir, "fetch_frida_gadget.py");

const ARM64_RUSTUP_TARGET = "aarch64-pc-windows-msvc";
const ARM64_VC_COMPONENT = "Microsoft.VisualStudio.Component.VC.Tools.ARM64";
// 缺 arm64 载荷但被要求必须提供时的退出码：与普通失败区分，方便 CI/人工一眼看清。
const EXIT_ARM64_REQUIRED = 3;

function run(cmd, args) {
  try {
    return { ok: true, stdout: execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }) };
  } catch (err) {
    const stdout = err && err.stdout ? String(err.stdout) : "";
    return { ok: false, stdout };
  }
}

function vswherePath() {
  const base = process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)";
  return path.join(base, "Microsoft Visual Studio", "Installer", "vswhere.exe");
}

// arm64 助手能否在本机构建。返回值 { ready, missing }，missing 是给人看的"缺什么"清单。
// 判据只有两条（都是构建它的**必要条件**，不做多余猜测）：
//   * rustup 的已装目标里有 aarch64-pc-windows-msvc（否则连 std 都没有）；
//   * vswhere 能查到 VC.Tools.ARM64 组件（否则没有 arm64 的 link.exe/lib）。
// 注意 vswhere 在"没有匹配实例"时**仍以 0 退出**并把输出留空，所以判据是 stdout
// 非空，而不是退出码（2026-10-07 本机实测：缺组件时退出码 0、输出为空）。
function detectArm64Toolchain() {
  const missing = [];
  const target = run("rustup", ["target", "list", "--installed"]);
  if (!target.ok) {
    missing.push("rustup 不可用，无法确认 " + ARM64_RUSTUP_TARGET + " 已安装（补：安装 rustup 后 rustup target add " + ARM64_RUSTUP_TARGET + "）");
  } else if (!target.stdout.split(/\r?\n/).map((line) => line.trim()).includes(ARM64_RUSTUP_TARGET)) {
    missing.push("rustup 目标 " + ARM64_RUSTUP_TARGET + " 未安装（补：rustup target add " + ARM64_RUSTUP_TARGET + "）");
  }
  const vswhere = vswherePath();
  if (!fs.existsSync(vswhere)) {
    missing.push("找不到 vswhere（" + vswhere + "），无法确认 VS 的 ARM64 工具集");
  } else {
    const vs = run(vswhere, ["-latest", "-products", "*", "-requires", ARM64_VC_COMPONENT, "-property", "installationPath"]);
    if (!vs.ok || vs.stdout.trim() === "") {
      missing.push("Visual Studio 缺 " + ARM64_VC_COMPONENT + "（补：Visual Studio Installer → 修改 → 单个组件 → 勾选 \"MSVC v143 - VS 2022 C++ ARM64/ARM64EC 生成工具\"）");
    }
  }
  return { ready: missing.length === 0, missing };
}

// 缺 arm64 时的唯一报告出口：警告 + 影响 + 补法；被要求时必须硬失败。
function reportArm64Unavailable(missing) {
  console.warn("[inputs] 警告：arm64 载荷不可用，本次安装包**只含 x64 两件**（sayall-helper.exe / frida-gadget.dll）。");
  for (const item of missing) {
    console.warn("[inputs]   缺：" + item);
  }
  console.warn("[inputs]   影响：Windows 11 ARM64 上无法启用\"全按键支持\"（该功能只能注入原生 ARM64 的 WUDFHost.exe）；");
  console.warn("[inputs]         语音路径与 x64 Windows 不受影响。应用会在 ARM64 上明确提示不支持，而不是静默失败。");
  if (process.env.SAYALL_REQUIRE_ARM64_PAYLOAD === "1") {
    console.error("[inputs] SAYALL_REQUIRE_ARM64_PAYLOAD=1：本路径要求安装包必须含 arm64 载荷，硬失败退出。");
    process.exit(EXIT_ARM64_REQUIRED);
  }
  console.warn("[inputs]   （出包/CI 若要强制包含 arm64 载荷，设 SAYALL_REQUIRE_ARM64_PAYLOAD=1。）");
}

const arm64Toolchain = detectArm64Toolchain();

if (process.argv.includes("--check-arm64")) {
  console.log("[inputs] --check-arm64：只检测 arm64 工具链并复现上面的报告逻辑，不构建、不暂存。");
  if (arm64Toolchain.ready) {
    console.log("[inputs] arm64 工具链齐备：正式出包会构建 arm64 助手并取 arm64 Gadget。");
  } else {
    reportArm64Unavailable(arm64Toolchain.missing);
  }
  process.exit(0);
}

console.log("[inputs] 1/4 构建助手（release，x86_64-pc-windows-msvc）");
execFileSync("cargo", ["build", "--release", "--manifest-path", helperManifest], {
  cwd: root,
  stdio: "inherit",
});

if (arm64Toolchain.ready) {
  console.log("[inputs] 2/4 构建助手（release，aarch64-pc-windows-msvc）");
  execFileSync(
    "cargo",
    ["build", "--release", "--target", ARM64_RUSTUP_TARGET, "--manifest-path", helperManifest],
    { cwd: root, stdio: "inherit" },
  );
} else {
  reportArm64Unavailable(arm64Toolchain.missing);
}

console.log(
  "[inputs] 3/4 按锁文件获取并校验 frida-gadget（" +
    (arm64Toolchain.ready ? "x86_64 + arm64：--all" : "仅 x86_64：默认，保持向后兼容") +
    "）",
);
execFileSync("python", arm64Toolchain.ready ? [fetchScript, "--all"] : [fetchScript], {
  cwd: root,
  stdio: "inherit",
});

console.log("[inputs] 4/4 落地到 src-tauri（复用 stage-bundle-resources.cjs，单一落地路径）");
execFileSync(process.execPath, [path.join(__dirname, "stage-bundle-resources.cjs")], {
  cwd: root,
  stdio: "inherit",
});
