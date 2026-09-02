# ADR 0001：Windows 使用独立 Tauri/Rust 架构

- 状态：Accepted
- 日期：2026-09-01

## 背景

现有 macOS 产品深度依赖 Apple 平台 API，不适合为了 Windows 重写。已有 Windows 候选分别使用 Python/Qt、C# 和 Rust/Tauri，证明小米 ATVV 语音遥控器的 Windows 路线可行，但各自包含不同的维护、权限和来源风险。

## 决策

- Windows 在 `GetSayAll/remote-mic-app-windows` 独立维护；
- 使用 Rust、Tauri 2、Vue 3；
- UI 以 macOS 原版为产品基准；
- 基础语音只使用 Windows 公共 API，不依赖管理员权限或进程注入；
- 完整 HID 作为可选实验能力。

## 结果

两端不能共享最终 UI、二进制、驱动或发布流水线，但未来可以在协议测试和配置兼容上保持一致。Windows 初始开发成本高于直接发布 Python 候选，长期运行时体积、类型安全和平台维护边界更清晰。
