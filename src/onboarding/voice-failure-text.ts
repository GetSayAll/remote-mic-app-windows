/**
 * 第⑤步失败卡文案（2026-10-14 现场）。
 *
 * 失败码 → 文案表从 `OnboardingPage.vue` 移到这里（随纯函数一起可单测），并新增一层
 * 分流：`voice.no_transcript` 不总是"麦克风 / 工具配置错"——「支持更多输入工具」还没
 * 生效时（`starting` 准备中 / `inactive` 过了宽限），豆包收不到报告层合成出的语音键，
 * 原文案「语音链路正常…」在此场景**不成立且误导**。按失败瞬间的捕获状态给专门解释；
 * `active` / `unknown` / `off` 时保持原排查文案（麦克风 / 语音键 / 工具三件套仍适用）。
 */
import type { Rc003CaptureHealth } from "../lib/bridge";
import type { VoiceAttemptFailureCode } from "./voice-attempt";

/** 失败码 → 「发生了什么 + 你该怎么办」（设计稿 §7；不贴内部错误原文）。 */
export const VOICE_FAILURE_COPY: Record<VoiceAttemptFailureCode, string> = {
  "voice.session_not_started":
    "没有检测到语音会话。确认遥控器已就绪，按住语音键不要放，再试一次。",
  "voice.no_samples": "收到了语音会话，但没有声音数据。对着遥控器正常说话，再试一次。",
  "voice.audio_delivery_failed":
    "声音没有完整送达语音设备。检查设备是否被其他程序占用，再试一次。",
  "voice.session_not_ended": "松开后这次语音没有正常结束。稍等一下或断开重连遥控器，再试一次。",
  "voice.no_transcript":
    "语音链路正常，但输入工具没有写出文字。检查工具麦克风是否选 CABLE Output、工具里的语音键是否和本次设置一致、工具是否在运行。",
  "voice.manual_input": "检测到键盘输入。这一步请只用遥控器语音键，不要用键盘打字。",
  "voice.input_target_not_ready": "输入框没有聚焦。先点一下输入框，再按住遥控器语音键。",
  "voice.focus_lost": "测试过程中输入框失去了焦点。点回输入框，再试一次。",
};

/**
 * 生成失败卡文案。`captureHealth` 只在 `voice.no_transcript` 上生效——取**失败瞬间**
 * 现读的捕获状态（读不到传 `"unknown"`，不猜测、不加解释句）。
 */
export function voiceFailureText(
  code: VoiceAttemptFailureCode | null,
  captureHealth: Rc003CaptureHealth | "unknown",
): string {
  if (code === "voice.no_transcript") {
    if (captureHealth === "starting") {
      return "“支持更多输入工具”还在准备中（刚开启时通常几秒）。等几秒后点“重新测试”再试一次。";
    }
    if (captureHealth === "inactive") {
      return "“支持更多输入工具”还没有生效，正在自动重试；如果一直不恢复，请把开关关掉再打开，然后重新测试。";
    }
    if (captureHealth === "off") {
      // 豆包 + 报告层关闭 = 必然收不到文字（语音键走了它按设计忽略的注入回退）。
      // 常规向导路径到不了这里（步骤④会拦），但用户中途去别的页面关掉开关是可达路径。
      return "“支持更多输入工具”现在是关闭的，请先打开它，再点“重新测试”。";
    }
  }
  return code ? VOICE_FAILURE_COPY[code] : "这次测试没有通过，再试一次。";
}
