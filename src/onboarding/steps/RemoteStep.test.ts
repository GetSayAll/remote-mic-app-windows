/**
 * 第②步长时间重连与重置遥控器的排查卡（issue #104，2026-11-19；重置引导 2026-11-21）。
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
  remoteName: string | null = null,
): RuntimeSnapshot {
  return {
    platform: {
      connection: {
        phase,
        reconnectAttempt,
        remoteModel: "unknown",
        remoteName,
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

describe("RemoteStep 重置遥控器指引（2026-11-21）", () => {
  it("已连接 20 秒仍无按键时提示重置并重新配对", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("ready", 0, "ready") },
    });

    expect(wrapper.text()).not.toContain("遥控器已连接，但没有收到按键");
    vi.advanceTimersByTime(19_999);
    await nextTick();
    expect(wrapper.text()).not.toContain("遥控器已连接，但没有收到按键");

    vi.advanceTimersByTime(1);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器已连接，但没有收到按键");
    expect(wrapper.text()).toContain("同时长按「主页」+「菜单」");
    expect(wrapper.text()).toContain("在 Windows 蓝牙设置里删除这支遥控器。");
    wrapper.unmount();
  });

  it("收到按键后立即撤掉重置提示", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("ready", 0, "ready") },
    });
    vi.advanceTimersByTime(20_000);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器已连接，但没有收到按键");

    await wrapper.setProps({ buttonObserved: true });
    await nextTick();
    expect(wrapper.text()).not.toContain("遥控器已连接，但没有收到按键");
    wrapper.unmount();
  });

  it("只有 MI RC 名称、还没有卡住时不提示重置", () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: {
        ...baseProps,
        devices: [
          { id: "dev-1", name: "MI RC", model: "unknown", isSupportedCandidate: true },
        ],
        runtime: runtime("idle", 0, "stopped", "MI RC"),
      },
    });

    expect(wrapper.text()).not.toContain("遥控器连不上或者一直收不到按键");
    expect(wrapper.text()).not.toContain("遥控器已连接，但没有收到按键");
    wrapper.unmount();
  });

  it("已连接且名称为 MI RC、20 秒无按键时才提示重置", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: {
        ...baseProps,
        devices: [
          { id: "dev-1", name: "MI RC", model: "unknown", isSupportedCandidate: true },
        ],
        runtime: runtime("ready", 0, "ready", "MI RC"),
      },
    });

    expect(wrapper.text()).not.toContain("遥控器连不上或者一直收不到按键");
    vi.advanceTimersByTime(20_000);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器连不上或者一直收不到按键");
    const text = wrapper.text();
    const remove = text.indexOf("在 Windows 蓝牙设置里删除这支遥控器。");
    const reset = text.indexOf("同时长按「主页」+「菜单」");
    const repair = text.indexOf("配对完成后回到这里点「重新扫描」");
    expect(remove).toBeGreaterThanOrEqual(0);
    expect(remove).toBeLessThan(reset);
    expect(reset).toBeLessThan(repair);
    wrapper.unmount();
  });

  it("列表里另有一支 MI RC 旧设备时，不把它当成当前已连接的那支", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: {
        ...baseProps,
        connectedDeviceId: "dev-2",
        devices: [
          { id: "dev-1", name: "MI RC", model: "unknown", isSupportedCandidate: true },
          {
            id: "dev-2",
            name: "小米蓝牙语音遥控器 2 Pro",
            model: "rc003",
            isSupportedCandidate: true,
          },
        ],
        runtime: runtime("ready", 0, "ready"),
      },
    });

    vi.advanceTimersByTime(20_000);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器已连接，但没有收到按键");
    expect(wrapper.text()).not.toContain("遥控器连不上或者一直收不到按键");
    wrapper.unmount();
  });

  it("一直连不上时，重连排查卡给出重置步骤且不再写“不需要重新配对”", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: {
        ...baseProps,
        devices: [
          { id: "dev-1", name: "MI RC", model: "unknown", isSupportedCandidate: true },
        ],
        runtime: runtime("reconnecting", 1, "awaiting", "MI RC"),
      },
    });

    expect(wrapper.text()).toContain("不需要重新配对");
    vi.advanceTimersByTime(60_000);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器还没有真正连上");
    expect(wrapper.text()).toContain("Windows 报告的遥控器名称是 MI RC");
    expect(wrapper.text()).toContain("同时长按「主页」+「菜单」重置遥控器");
    expect(wrapper.text()).not.toContain("不需要重新配对");
    wrapper.unmount();
  });

  it("一直连不上且 Raw Input 在位时，60 秒后显示“没有连上”重置卡", async () => {
    vi.useFakeTimers();
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("reconnecting", 1, "ready") },
    });

    expect(wrapper.text()).toContain("不需要重新配对");
    vi.advanceTimersByTime(60_000);
    await nextTick();
    expect(wrapper.text()).toContain("遥控器一直没有连上");
    expect(wrapper.text()).toContain("应用已经自动重试超过约 60 秒");
    expect(wrapper.text()).toContain("同时长按「主页」+「菜单」");
    expect(wrapper.text()).not.toContain("不需要重新配对");
    wrapper.unmount();
  });

  it("只有找不到已配对遥控器时才显示 Windows「高级」提示，并与下方留间距", async () => {
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("idle", 0, "stopped") },
    });

    expect(wrapper.text()).toContain("找不到遥控器？部分电脑需要打开「高级」");
    expect(wrapper.text()).toContain("如果在蓝牙列表里找不到遥控器，可以打开高级设置");
    expect(wrapper.text()).toContain(
      "设置 → 蓝牙和其他设备 → 设备 → 向下滚动 → 蓝牙设备发现 → 高级",
    );
    expect(wrapper.text()).toContain("打开后回到这里点「扫描已配对设备」");
    expect(wrapper.find(".onboarding-advanced-card").exists()).toBe(true);

    await wrapper.setProps({ scanning: true });
    await nextTick();
    expect(wrapper.find(".onboarding-advanced-card").exists()).toBe(false);

    await wrapper.setProps({
      scanning: false,
      devices: [
        { id: "dev-1", name: "小米蓝牙语音遥控器 2 Pro", model: "rc003", isSupportedCandidate: true },
      ],
      runtime: runtime("ready", 0, "ready"),
    });
    await nextTick();
    expect(wrapper.find(".onboarding-advanced-card").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("找不到遥控器？部分电脑需要打开「高级」");
    wrapper.unmount();
  });

  it("连接页不再重复页脚的按键提示，按键行只保留短说明", () => {
    const wrapper = mount(RemoteStep, {
      props: { ...baseProps, runtime: runtime("ready", 0, "ready") },
    });
    expect(wrapper.text()).not.toContain("连接测试请用返回 / 音量+ / 音量−以外的键");
    expect(wrapper.text()).toContain(
      "按一下遥控器上的普通按键（比如主页键），不要按语音键。",
    );
    wrapper.unmount();
  });
});
