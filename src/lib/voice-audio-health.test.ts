import { describe, expect, it } from "vitest";
import {
  normalizeVoiceAudioHealth,
  shouldShowVoiceAudioDegraded,
  voiceAudioDegradedText,
} from "./voice-audio-health";

describe("voice audio health（2026-10-24 蓝牙共存降级提示）", () => {
  it("只认三个稳定 token；缺失或未知值回落 unknown（不提示）", () => {
    expect(normalizeVoiceAudioHealth("unknown")).toBe("unknown");
    expect(normalizeVoiceAudioHealth("full")).toBe("full");
    expect(normalizeVoiceAudioHealth("degraded")).toBe("degraded");
    expect(normalizeVoiceAudioHealth(undefined)).toBe("unknown");
    expect(normalizeVoiceAudioHealth(null)).toBe("unknown");
    expect(normalizeVoiceAudioHealth("")).toBe("unknown");
    expect(normalizeVoiceAudioHealth("Degraded")).toBe("unknown");
    expect(normalizeVoiceAudioHealth("nonsense")).toBe("unknown");
  });

  it("只有 degraded 显示提示；full / unknown 不显示", () => {
    expect(shouldShowVoiceAudioDegraded("degraded")).toBe(true);
    expect(shouldShowVoiceAudioDegraded("full")).toBe(false);
    expect(shouldShowVoiceAudioDegraded("unknown")).toBe(false);
  });

  it("提示说清原因与下一步，不出现内部术语", () => {
    const text = voiceAudioDegradedText();
    expect(text).toContain("会轮流使用蓝牙");
    expect(text).toContain("遥控器这次只分到一部分声音");
    expect(text).toContain("文字可能没有出现");
    expect(text).toContain("不用关闭其他设备");
    expect(text).toContain("语音输入期间会自动让出键盘、鼠标的蓝牙时间");
    expect(text).toContain("结束后恢复");
    expect(text).toContain("USB 接收器或有线连接");
    expect(text).not.toMatch(/样本率|MTU|BLE|ATVV|16\s*kHz/);
  });
});
