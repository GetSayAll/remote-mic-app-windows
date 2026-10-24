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
    expect(text).toContain("声音没有完整送到输入工具");
    expect(text).toContain("文字可能没有出现");
    expect(text).toContain("关闭或移开同时连接的其他蓝牙设备");
    expect(text).toContain("键盘、鼠标、耳机");
    expect(text).not.toMatch(/样本率|MTU|BLE|ATVV|16\s*kHz/);
  });
});
