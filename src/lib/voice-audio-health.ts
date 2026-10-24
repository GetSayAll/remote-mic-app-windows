/**
 * 语音音频完整度的用户提示（2026-10-24 现场）。
 *
 * 现场事实：遥控器与另一支 BLE 键盘同时连接时，ATVV 音频只剩满速的约 1/3；
 * 豆包语音条能出现，但转写不出文字。应用不自动断开用户的其它蓝牙设备，
 * 只在**最近一次会话确实降级**时给一句可执行的提示（文案规范见
 * docs/product-copy.md：说清发生了什么 + 用户该做什么，不贴内部错误）。
 */
import type { VoiceAudioHealth } from "./bridge";
import { t } from "./i18n";

/** 载荷规范化：缺失/未知值按"读不到"处理，不提示。 */
export function normalizeVoiceAudioHealth(raw: unknown): VoiceAudioHealth {
  return raw === "unknown" || raw === "full" || raw === "degraded"
    ? raw
    : "unknown";
}

/** 只有明确降级才提示；满速与未知都不显示。 */
export function shouldShowVoiceAudioDegraded(health: VoiceAudioHealth): boolean {
  return health === "degraded";
}

/**
 * 一句用户语言：不出现样本率、MTU、BLE 调度等内部词。
 * 连接页与向导第⑤步共用，保证同一事实同一说法。
 */
export function voiceAudioDegradedText(): string {
  return t(
    "最近一次语音的声音没有完整送到输入工具，文字可能没有出现。请关闭或移开同时连接的其他蓝牙设备（键盘、鼠标、耳机等），再试一次。",
  );
}
