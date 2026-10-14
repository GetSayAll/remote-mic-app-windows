/**
 * 第⑤步失败卡文案（2026-10-14 现场）：`voice.no_transcript` 不总是"麦克风/工具配置错"。
 * 「支持更多输入工具」还没生效时（`starting` 准备中 / `inactive` 过了宽限），豆包收不到
 * 报告层合成出的语音键——原文案「语音链路正常…」在此场景**不成立且误导**。按失败瞬间的
 * 捕获状态分流；`active` / `unknown` 时不改原行为（三件套排查仍然适用）。
 */
import { describe, expect, it } from "vitest";
import { VOICE_FAILURE_COPY, voiceFailureText } from "./voice-failure-text";

describe("voiceFailureText（失败解释按捕获状态分流）", () => {
  it("no_transcript + starting：解释为「还在准备中」并引导稍后重试", () => {
    const text = voiceFailureText("voice.no_transcript", "starting");
    expect(text).toContain("还在准备中");
    expect(text).toContain("重新测试");
    expect(text).not.toContain("语音链路正常");
  });

  it("no_transcript + inactive：给出「关掉再打开」的既有指引", () => {
    const text = voiceFailureText("voice.no_transcript", "inactive");
    expect(text).toContain("还没有生效");
    expect(text).toContain("关掉再打开");
    expect(text).not.toContain("语音链路正常");
  });

  it("no_transcript + off：提示开关未打开（豆包 + 关闭是必然失败）", () => {
    const text = voiceFailureText("voice.no_transcript", "off");
    expect(text).toContain("关闭");
    expect(text).toContain("重新测试");
    expect(text).not.toContain("语音链路正常");
  });

  it("no_transcript + active/unknown：保留原有排查文案", () => {
    for (const health of ["active", "unknown"] as const) {
      expect(voiceFailureText("voice.no_transcript", health)).toBe(
        VOICE_FAILURE_COPY["voice.no_transcript"],
      );
    }
  });

  it("其它失败码不受捕获状态影响", () => {
    expect(voiceFailureText("voice.no_samples", "starting")).toBe(
      VOICE_FAILURE_COPY["voice.no_samples"],
    );
    expect(voiceFailureText("voice.input_target_not_ready", "inactive")).toBe(
      VOICE_FAILURE_COPY["voice.input_target_not_ready"],
    );
  });

  it("无失败码：默认重试句", () => {
    expect(voiceFailureText(null, "unknown")).toBe("这次测试没有通过，再试一次。");
  });
});
