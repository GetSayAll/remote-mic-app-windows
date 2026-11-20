/**
 * 第②步长时间重连的排查卡（issue #104，2026-11-19）。
 *
 * 组件级测试只覆盖计时与显示条件；文案的完整含义由纯逻辑测试
 * `src/onboarding/reconnect-guidance.test.ts` 锁定。
 */
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RuntimeSnapshot } from "../../lib/bridge";
import RemoteStep from "./RemoteStep.vue";

function runtime(
  phase: "idle" | "reconnecting" | "ready",
  reconnectAttempt: number,
  rawInputPhase: "stopped" | "starting" | "ready" | "awaiting",
): RuntimeSnapshot {
  return {
    platform: {
      connection: {
        phase,
        reconnectAttempt,
        remoteModel: "unknown",
        remoteName: null,
        remoteToken: null,
      },
      rawInput: { phase: rawInputPhase },
    },
  } as unknown as RuntimeSnapshot;
}

const baseProps = {
  devices: [],
  scanning: false,
  scanMessage: "",
  connectingDeviceId: "",
  connectedDeviceId: "",
  operationMessage: "",
  buttonObserved: false,
  voiceKeyMistake: false,
  gateCode: "remote.not_connected",
  otherRemoteActivity: false,
};

afterEach(() => {
  vi.useRealTimers();
});

describe("RemoteStep 长时间重连指引", () => {
  it("重连满 60 秒且按键通道未出现时显示排查卡", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("reconnecting", 1, "awaiting") },
    });

    expect(wrapper.text()).toContain("正在自动恢复连接");
    expect(wrapper.text()).not.toContain("最多等约 60 秒");
    expect(wrapper.text()).not.toContain("遥控器还没有真正连上");

    vi.advanceTimersByTime(59_999);
    await nextTick();
    expect(wrapper.text()).not.toContain("遥控器还没有真正连上");

    vi.advanceTimersByTime(1);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器还没有真正连上");
    expect(wrapper.text()).toContain("手机或电视");
    expect(wrapper.text()).toContain("删除这支遥控器");
    expect(wrapper.findAll("button").map((node) => node.text())).toEqual(
      expect.arrayContaining(["打开蓝牙设置", "重新扫描"]),
    );
    wrapper.unmount();
  });

  it("按键通道已就绪时，即使重连很久也不显示排查卡", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("reconnecting", 1, "ready") },
    });

    vi.advanceTimersByTime(60_000);
    await nextTick();
    expect(wrapper.text()).not.toContain("遥控器还没有真正连上");
    wrapper.unmount();
  });

  it("重试次数已达阈值时立即显示，不必再等 60 秒", () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("reconnecting", 5, "awaiting") },
    });

    expect(wrapper.text()).toContain("遥控器还没有真正连上");
    wrapper.unmount();
  });
});
