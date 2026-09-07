# 交接文档（handoff）——驱动签名规避调查后续

目的：新代理 10 分钟内接手。日期：2026-09-04。调查已闭环（R5 终审 PASS），本文件指引后续执行。

## 当前系统状态（接手前必读）

1. **常驻捕获器**在 console 会话运行（`Testing\investigation\remote-capture.ps1`，检查进程+日志 `Testing\investigation\remote-capture.log`）——**用户随时按遥控器按键即采集真机数据**（协议：`Testing\investigation\REMOTE-CAPTURE-PROTOCOL.md`，含首事件 7 项验证清单与 console 会话前提）；
2. **微信登录窗特意留置前台**（Weixin.exe 4.1.13.63，QR 码待用户扫码）——扫码后按 `evidence/n/FINDINGS.md` 任务 3 复跑协议测客户端语音（仅文件传输助手）；
3. 默认录音设备=Realtek 麦克风（已恢复）；活动输入法=豆包（已恢复）；豆包快捷键=出厂右 Alt（config 已还原并验证）；
4. 机器锁协议：`Testing\investigation\machine-lock-protocol.md`（审计日志制，敏感实验必守）。

## 关键文档

| 文档 | 内容 |
|---|---|
| `2026-09-04-avoid-driver-signing-input-paths-final.md` | **最终报告**（先读这个） |
| `2026-09-04-avoid-driver-signing-input-paths.md` | 工作文档（全部轮次细节、路线图前提、边界对照表、Win+H 终验规格、环境事实） |
| `Bugs/2026-09-04-doubao-voice-hold-hotkey.md` | 豆包/WeType 全案记录（含勘误体系） |
| `docs/decisions/0002-dual-track-injection-optional-helper.md` | 双轨架构 ADR（增强轨待修宪决策） |
| `Testing/WindowsRC003Preview.md` | 产品真机测试手册 |

## 后续工作（按优先级）

1. **真机遥控器采集**（用户按键即可，零准备）：数据到手后回答 usage 表差异/0x00F1/时序窗参数 → 直接喂给吞键层实现与真机验收；
2. **WeType 产品化收尾**（天级）：连接页 UI 引导（活动输入法确认+按住 ≥0.5s 提示）、快按无文本的 UX 披露、真机遥控器端到端验收（注入段已实现+音频段已 E2E，只差遥控器本体）；
3. **Win+H 健康主机终验**（五步序列已备）：任何已激活+语音功能完整的机器；
4. **吞键层实现**（2-4 周）：公式+工程细节齐备（工作文档 C 交付节+路线图），真机数据为输入；
5. **修宪决策**（用户）：是否启动 WinUHid 增强轨（豆包唯一路径，4-8 周+OV 证书）；澄清 A2"基础 vs 增强"边界；
6. **微信客户端复测**（用户扫码后，协议已备）；STT 路线若立项先解决延迟（流式/SenseVoice）。

## 环境陷阱速查（历史教训，全部实证）

PS 5.1 无 BOM UTF-8 脚本按 GBK 读（别在脚本写中文/计数用字节级）；INPUT 结构必须 40 字节（32B 会静默失败）；TSF 激活必须 FORSESSION=0x20000000（dwFlags=0 静默无效）；活动 IME 判定用候选框行为（非 S_OK/HKL/WTSB）；OCR 用 `dim ocr recognize`（视觉模型未配置）；焦点用 SetWindowPos+点击；VB-CABLE 锁 16kHz；豆包 ASR 需网络；锁协议必守（并行实验互相污染有实证）。
