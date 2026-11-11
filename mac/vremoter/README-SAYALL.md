# mac/vremoter — vendored macOS app（ADR 0005 修订版）

本目录是 [vRemoter](https://github.com/VincentKingHsu/vRemoter)（MIT，Copyright (c) 2026 Sima Qingfeng）
的**源码整体引入**，来源为我们本地维护的社区版 `ccc333cn-edition`（revision
`f3e310f`，含 WeType 支持、语音断连重连等 4 项改进，对应上游 PR #9–#12）。

## 为什么直接引入而不是重写

2026-11-11 产品决策修订：Mac 端 Chromecast 支持以**已验证可用的实现**为优先——
vRemoter 在 macOS 上语音（ATVV + 豆包/WeType）与按键（IOHIDManager）均已量产
验证。MIT 协议允许整体引入；在此机器（Windows）上重写 Apple FFI 无法编译验证，
重写没有收益。原 ADR 0005 的 L1–L3 Rust 重写计划**作废**，改为"引入 + 按需收敛"。

## 保持原样的东西

- `LICENSE`、`THIRD_PARTY_NOTICES.md` 原文保留（MIT 要求）。
- Swift 源码、打包脚本、SelfTests 不做改名或结构调整——保证 Mac 上
  `run-self-tests.sh` / `package-app.sh` 开箱可用。
- 上游若更新：以补丁形式同步，不在本仓库内分叉演化。

## 使用（需要 macOS）

```bash
cd mac/vremoter
./run-self-tests.sh     # 逻辑自测
./package-app.sh        # 打 .app
./build-dmg.sh          # 打安装包
```

## 后续收敛点（另行立项，不阻塞当前）

- 品牌/更新源：应用名、图标、`vincentstudio.org` 更新服务器指向自家基础设施。
- 语义统一：`sayall-core::cast_hid`（Rust）与本目录键码表已是同一张表的两个
  实现；若未来做统一配置同步，以 core 为单一事实源。
- 小米 RC001/RC003：本目录不含；协议已在 `sayall-core` 就绪，届时在 Swift 侧
  新增 BLEBridge 会话（参考 Sources/vRemote/BLEBridge.swift 骨架）。
