# ADR 0005: macOS 支持进入本仓库（双平台单仓库）

日期：2026-11-11
状态：已接受

## 背景

SayAll 的遥控器生态目前分裂在两处：

- 本仓库（`remote-mic-app-windows`）：Windows 产品，Rust + Tauri。小米 RC001/RC003 与
  Chromecast 均已支持（语音 + 按键 + 增强捕获 Helper）。
- `vRemoter`（第三方，MIT，VincentKingHsu v1.1.1 + 本地 fork 的 PR #9–#12）：
  macOS 产品，Swift。X6 与 Chromecast 已支持。

两个平台的能力互补但不重叠：vRemoter 没有小米 RC001/RC003；本仓库没有 macOS 后端。
用户决策：**以本仓库为主**，macOS 支持直接进入本仓库，不改动 vRemoter 仓库。

## 决策

1. 本仓库成为双平台唯一主干。新增 `crates/sayall-macos` crate，与
   `sayall-windows` 平级；`sayall-core`（ATVV 协议、映射引擎、编解码）继续作为
   两平台共享的纯逻辑层。
2. vRemoter 仅作 **MIT 参考实现**：移植其已验证的行为逻辑（HID report-1 单键翻转、
   CGKeyCode 表、合成键标记），在 `ATTRIBUTION.md` 标注；不引入其源码文件，
   不修改其仓库。
3. 分层策略（自下而上，每层可独立验收）：
   - **L0 纯逻辑**（本 ADR 随附）：HID 边沿跟踪、键码表——平台无关，在本机
     `cargo test` 即可验证。
   - **L1 Apple FFI**（CoreBluetooth / IOHIDManager / CGEvent / CoreAudio）：
     **硬前置 = 一台 macOS 机器**（编译、链接、真机验收都离不开）。在此之前
     不编写未编译过的 FFI 代码——不交付无法验证的支架。
   - **L2 会话接线**：ATVV 语音会话（复用 `sayall-core` 协议状态机）、
     Chromecast 按键（L0 跟踪器 + IOHIDManager）、小米 RC003 传输。
   - **L3 应用壳**：Tauri 壳按平台条件编译，或独立 mac 菜单栏壳（届时另立 ADR）。

## 证据

- Chromecast 在 macOS 上 HID 输入报告可达：vRemoter `ChromecastRemoteHIDBridge.swift`
  （IOHIDManager + report ID 0x01 + 单字节 usage），与本仓库 Windows 侧
  抓包逆向的 report-1 键码表一致（`raw_input.rs::chromecast_button_for_legacy_index`）。
- Windows 主机 GATT 缓存缺陷（见 `Testing/WindowsChromecast.md`）为 Windows 栈特有，
  macOS 每次连接重新发现并按 UUID 订阅，不存在该问题——Mac 上按键不需要 Helper。

## 后果

- 仓库名含 `-windows` 与实际范围不再相符；重命名是 GitHub 组织级操作，
  由维护者择机执行，不阻塞本 ADR。
- CI 需要新增 macOS runner 才能守住 L1+（TODO MAC-02..04 的验收前提）。
- `sayall-macos` 在 L1 落地前只包含可在本机验证的纯逻辑；Windows 全部门禁
  不受影响。
