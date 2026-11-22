/**
 * 语音音频完整度的用户提示（2026-10-24 现场，2026-11-22 修订）。
 *
 * 现场事实：遥控器与键盘、鼠标同时连接时，适配器会在多条链路之间轮流调度；
 * 三链路下遥控器的 ATVV 音频只剩满速的约 1/3，豆包语音条能出现但转写不出文字。
 * 2026-11-22 只读取回：遥控器 15 ms/延迟 0，键盘与鼠标均为 7.5 ms/延迟 44。
 * 应用不能改变其他设备的调度，也不自动断开它们；提示只说明现象与可选做法，
 * 不再要求用户关闭设备（文案规范见 docs/product-copy.md）。
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
 * 连接页与向导第⑥步共用，保证同一事实同一说法。
 */
export function voiceAudioDegradedText(): string {
  return t(
    "电脑同时连接了键盘、鼠标等蓝牙设备时，会轮流使用蓝牙，遥控器这次只分到一部分声音，文字可能没有出现。不用关闭其他设备：语音输入期间会自动让出键盘、鼠标的蓝牙时间，结束后恢复；如果这次仍不完整，可以再试一次，或把键盘、鼠标改用 USB 接收器或有线连接。",
  );
}
