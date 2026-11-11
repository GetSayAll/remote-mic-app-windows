# SayAll Windows · Chromecast 与本地听写差距

范围包括已安装的 SayAll Windows、Chromecast 预览版 `b545325`、本机 Wispr Flow 1.6.1400，以及 OpenAI Whisper 的本地识别能力。Craft Anything 静态读产物，产品行为以公开说明或单独的运行验证为准。

| 能力 | SayAll Windows 的路径 | 差距和验收 |
| --- | --- | --- |
| 遥控器按键 | SayAll 已有手势与动作引擎。旧 Chromecast 预览根据报文 ID 和长度做固定解码。本机 Windows HID 描述符显示 Report 1 长度为 3，确定键的 Consumer usage 是 `0x0041`。 | Windows 应按实际设备描述符解释该报文。逐键确认真实无线报文、原生动作并发及睡眠恢复，保留小米和旧报文格式回归。 |
| 按下反馈 | 预览版已有型号图与可配置手势。 | 按下时同时高亮对应实体键和配置行。释放、断连或切换设备时清除。语音键用可观测的蓝牙串流状态。 |
| 语音识别 | 现有实时听写将遥控器 PCM 通过 VB-CABLE 交给微信输入法。新增本地 Whisper WAV 转写入口，输出可编辑草稿。 | 直接从遥控器 PCM 接本地模型需要独立的按住说话状态、分块、取消、语言选择和延迟预算。中文代码混输、长音频、设备断开单独验收。Whisper 原模型另有语言识别、英语翻译等能力。 [OpenAI Whisper](https://github.com/openai/whisper) |
| 文本处理 | SayAll 有回眸与常用语，现有常用语由按键选择。 | Wispr Flow 提供口述词典和短语、智能格式化、口误回改和命令模式。SayAll 需另做规则冲突、原稿保留、恢复和授权语义，尤其需要保留代码标识符和用户确认。 [词典与短语](https://docs.wisprflow.ai/articles/9639977157-view-shared-dictionary-words-and-snippets-in-the-admin-portal) · [智能格式化](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack) · [命令模式](https://docs.wisprflow.ai/articles/4816967992-How-to-use-Command-Mode) |
| 文本去向 | 现有听写依赖前台输入法。文件转写返回草稿供修改和复制。 | 直接写入浏览器需要校验站点、标签页和会话，失焦时保留草稿。自动化可验证草稿合同，真实焦点行为和提交需独立环境。 [Wispr Flow 听写入口](https://docs.wisprflow.ai/articles/6409258247-flow-bubble-requirements) |
| 生产交付 | x64 本机预览。 | 签名候选、安装升级回退、第三方模型分发、低配设备延迟、实体遥控器和目标应用验收。 |

## Craft Anything 取证

- `ev_eab2eb5d31b241e82209d168343b7962b418e15a76d11add1b50bf0cf443309e` 记录已安装 SayAll 的 Raw Input、映射与恢复输入工具标记。
- `ev_d150081ec66598227bbeea9e6c1c3f26b2f0236df76d17e24a12b340c70a1f55` 记录旧 Chromecast 预览的 Raw Input 与首帧 PCM 标记。
- `ev_e7b34764bc0d33fc4ca578aa2fb5b63957ef7fcc533d4ca352664e5f97c0e07b` 记录 Wispr Flow ASAR 清单。静态应用图定位了听写开始、状态和终态等通道，动态通道和节点存在分析覆盖限制。
- `ev_32304ee14dadc8176a3bbdd246cc2efd1de2c2b8156c4e9227be7c7631908334` 记录 Wispr Flow Windows Helper 的前台窗口、剪贴板和键盘接口标记。运行策略、云端实现和录音行为由公开资料或后续运行验证确认。

端到端证据见 [WindowsChromecast](../../Testing/WindowsChromecast.md)。开发与生产门禁见 [TODO](../../TODO.md)。

## 本机转写证据

用固定的 whisper.cpp Windows x64 CPU 引擎和多语 base-q5_1 模型跑实际适配器。
5.6 秒的合成中文 WAV 生成草稿耗时 2.289 秒，包含读取、校验、模型加载与推理。
样本中的「识别」被识别成「时别」。这条证据覆盖文件到草稿的实际转写，
中文准确率、说话口音、遥控器实时音频和目标应用写入需要独立验收。
