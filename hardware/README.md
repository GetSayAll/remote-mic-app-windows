# 硬件目录

存放**真实硬件**的取证资料与探针脚本。与 `docs/`（设计文档）、`Bugs/`（缺陷记录）、`Testing/`（可执行测试）的分工是：

- `docs/`、`Bugs/` 记录**结论与推理**；
- `Testing/` 放**可以随时重跑的测试**；
- **本目录放"一次性采集的原始证据"** —— 真机按键采集日志、设备枚举原始输出，以及生成它们的探针脚本。
  这类材料的特点是**不可再生**：删掉就得重新接上真机、重新按一遍遥控器才能复现。

## 目录

| 型号 | 说明 |
| --- | --- |
| [`RC003/`](RC003/README.md) | 小米蓝牙遥控器 2 Pro（`VID_2717 / PID_32B8 / REV_00A4`）。按键通道归属、设备栈结构、驱动挂载点的取证资料 |

> 支持目标型号见仓库根 `AGENTS.md`：RC001（小米蓝牙遥控器 2）与 RC003（2 Pro）。
> RC001 的对应取证目录尚未建立（RC003 优先），建好后按同样结构放在本目录下。

## 与文档的关系

本目录的证据是下列文档的**原始出处**，阅读文档时可按需回查：

- `docs/investigations/2026-09-22-per-device-key-interception-route-selection.md`（路线选型·首轮）
- `docs/investigations/2026-09-22-per-device-key-interception-route-selection-round2.md`（路线选型·第二轮）
- `docs/investigations/evidence/2026-09-22-tv-shell-action-channel-analysis.md`（TV 键通道归属）
