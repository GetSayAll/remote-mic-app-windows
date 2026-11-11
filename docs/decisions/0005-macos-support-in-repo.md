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
2. **（2026-11-11 当日修订）vRemoter 源码整体引入**：产品复核后认定——
   Mac 端已有量产验证的实现（vRemoter：ATVV 语音 + IOHIDManager 按键 +
   CoreAudio 混音），MIT 允许整体引入，重写 Apple FFI（在 Windows 机器上
   无法编译验证）没有收益。源码收入 `mac/vremoter/`（来源 = 本地社区版
   `ccc333cn-edition`，revision `f3e310f`，含 WeType 支持等 4 项改进），
   保持原样可构建；LICENSE 与 THIRD_PARTY_NOTICES 原文保留。
   原 L1–L3 Rust 重写计划作废，改为"引入 + 按需收敛"（收敛点见
   `mac/vremoter/README-SAYALL.md`：品牌/更新源、语义统一、小米接入）。
   vRemoter 上游仓库本身仍不改动。
3. （已被第 2 条取代）~~分层策略~~：L0 纯逻辑（保留——`sayall-core::cast_hid`
   与 `sayall-macos` keymap 仍作为跨平台语义的单一事实源，供未来配置同步
   与小米接入复用）；L1–L3 由 `mac/vremoter/` 整体引入替代。

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
