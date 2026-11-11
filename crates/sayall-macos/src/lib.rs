//! SayAll macOS 后端（ADR 0005，`docs/decisions/0005-macos-support-in-repo.md`）。
//!
//! 本仓库是双平台唯一主干：Windows 走 `sayall-windows`，macOS 走本 crate，
//! 协议与映射逻辑共享 `sayall-core`。
//!
//! ## 分层状态（ADR 0005）
//!
//! - **L0 纯逻辑（本 crate 当前内容）**：macOS 侧按键合成键码表与 Chromecast
//!   HID 边沿跟踪（复用 `sayall_core::cast_hid`）。平台无关，任何机器都能
//!   `cargo test` 验证。
//! - **L1 Apple FFI（未开始，硬前置 = macOS 机器）**：CoreBluetooth（GATT/ATVV）、
//!   IOHIDManager（Chromecast HID 报告）、CGEvent（输入合成）、CoreAudio
//!   （麦克风管道）。在无法编译/链接/真机验证的机器上不编写这些代码——
//!   不交付未验证的支架（TODO MAC-02..04）。
//!
//! ## vRemoter 的关系
//!
//! vRemoter（MIT）是行为参考：`keymap` 中的 CGKeyCode 表与合成键标记移植自
//! 其 `InputTrigger.swift`；HID report-1 单键翻转语义移植自其
//! `ChromecastRemoteHIDBridge.swift`。不引入其源码，归属见 `ATTRIBUTION.md`。

pub mod keymap;

pub use keymap::{modifier_key_codes, modifier_primary_key_code, SYNTHETIC_EVENT_MARKER};
pub use sayall_core::cast_hid::{
    chromecast_button_for_consumer_usage, chromecast_button_for_legacy_index,
    consumer_usage_for_chromecast_button, CastKeyEdge, CastKeyEdgeTracker, RemoteButton,
    ALL_BUTTONS,
};
