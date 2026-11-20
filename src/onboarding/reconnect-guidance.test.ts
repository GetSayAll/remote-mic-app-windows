/**
 * 长时间重连后的可操作排查卡（issue #104，2026-11-19）。
 *
 * 现场：Windows 里只有配对记录，遥控器 HID 接口没有出现（`raw_input` 停在
 * `awaiting`），`service_discovery` 持续 `gatt_status=1`。向导只写「最多等约
 * 60 秒」，超时后没有任何可执行动作。这里锁定卡的显示边界：只在重连满 60 秒
 * （或重试次数已到阈值）且按键通道仍缺席时出现，不提前打扰正常重连。
 */
import { describe, expect, it } from "vitest";
import {
  RECONNECT_GUIDANCE_ATTEMPT,
  RECONNECT_GUIDANCE_DELAY_MS,
  shouldShowReconnectGuidance,
} from "./reconnect-guidance";

const base = {
  reconnecting: true,
  reconnectAttempt: 0,
  waitedMs: 0,
  rawInputPhase: "awaiting" as const,
  buttonObserved: false,
};

describe("shouldShowReconnectGuidance", () => {
  it("重连满 60 秒且按键通道仍未出现时显示", () => {
    expect(
      shouldShowReconnectGuidance({
        ...base,
        waitedMs: RECONNECT_GUIDANCE_DELAY_MS,
      }),
    ).toBe(true);
  });

  it("未满 60 秒不提前显示", () => {
    expect(
      shouldShowReconnectGuidance({
        ...base,
        waitedMs: RECONNECT_GUIDANCE_DELAY_MS - 1,
      }),
    ).toBe(false);
  });

  it("重试次数达到阈值时直接显示（用户中途进入向导也不必再等 60 秒）", () => {
    expect(
      shouldShowReconnectGuidance({
        ...base,
        reconnectAttempt: RECONNECT_GUIDANCE_ATTEMPT,
      }),
    ).toBe(true);
    expect(
      shouldShowReconnectGuidance({
        ...base,
        reconnectAttempt: RECONNECT_GUIDANCE_ATTEMPT - 1,
      }),
    ).toBe(false);
  });

  it("按键通道不是 awaiting（监听器已就绪或已停止）时不显示", () => {
    for (const phase of ["stopped", "starting", "ready"] as const) {
      expect(
        shouldShowReconnectGuidance({
          ...base,
          waitedMs: RECONNECT_GUIDANCE_DELAY_MS,
          rawInputPhase: phase,
        }),
        phase,
      ).toBe(false);
    }
  });

  it("已经收到遥控器按键时不显示（下一步由按键行接管）", () => {
    expect(
      shouldShowReconnectGuidance({
        ...base,
        waitedMs: RECONNECT_GUIDANCE_DELAY_MS,
        buttonObserved: true,
      }),
    ).toBe(false);
  });

  it("不在重连相位时不显示", () => {
    expect(
      shouldShowReconnectGuidance({
        ...base,
        reconnecting: false,
        waitedMs: RECONNECT_GUIDANCE_DELAY_MS,
        reconnectAttempt: RECONNECT_GUIDANCE_ATTEMPT,
      }),
    ).toBe(false);
  });
});
