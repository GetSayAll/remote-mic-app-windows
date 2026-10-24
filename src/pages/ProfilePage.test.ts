// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ButtonMappings, UsageProfile } from "../lib/bridge";
import ProfilePage from "./ProfilePage.vue";

const bridge = vi.hoisted(() => ({
  getUsageProfile: vi.fn<() => Promise<UsageProfile>>(),
  getButtonMappings: vi.fn<() => Promise<ButtonMappings>>(),
  listPresetApps: vi.fn(),
  getCombinedActions: vi.fn(),
  getCommonPhrasePanelSnapshot: vi.fn(),
}));

vi.mock("../lib/bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/bridge")>();
  return { ...actual, ...bridge };
});

function dateKey(offsetFromToday: number): string {
  const date = new Date();
  date.setHours(12, 0, 0, 0);
  date.setDate(date.getDate() + offsetFromToday);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
    date.getDate(),
  ).padStart(2, "0")}`;
}

function sampleProfile(): UsageProfile {
  return {
    totalButtonPresses: 561,
    totalVoiceSeconds: 33 * 60 + 12,
    weekButtonPresses: 9,
    longestVoiceSeconds: 85,
    buttonPressesByControl: {
      "button.back": 47,
      "button.ok": 42,
      "button.home": 45,
      voice: 132,
    },
    voiceSessions: [
      {
        endedAtMs: new Date(2026, 8, 21, 23, 26).getTime(),
        durationSeconds: 85,
        applicationName: "微信",
      },
      {
        endedAtMs: new Date(2026, 8, 20, 12, 0).getTime(),
        durationSeconds: 61,
        applicationName: null,
      },
    ],
    recentDays: Array.from({ length: 26 * 7 }, (_, index) => ({
      date: dateKey(index - 26 * 7 + 1),
      buttonPresses: index === 26 * 7 - 1 ? 4 : 0,
      voiceSeconds: 0,
    })),
  };
}

const mappings: ButtonMappings = {
  enabled: true,
  actions: {
    back: {
      single: { type: "shortcut", chord: { keys: ["escape"] } },
      double: { type: "disabled" },
      long: { type: "disabled" },
    },
    home: {
      single: { type: "shortcut", chord: { keys: ["home"] } },
      double: { type: "disabled" },
      long: { type: "disabled" },
    },
  },
};

function stubBridge(): void {
  bridge.getUsageProfile.mockResolvedValue(sampleProfile());
  bridge.getButtonMappings.mockResolvedValue(mappings);
  bridge.listPresetApps.mockResolvedValue([]);
  bridge.getCombinedActions.mockResolvedValue({ formatVersion: 1, definitions: [] });
  bridge.getCommonPhrasePanelSnapshot.mockResolvedValue({
    available: false,
    access: "unavailable",
    canUse: false,
    previewMode: false,
    notice: null,
    entries: [],
  });
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("个人中心页", () => {
  it("渲染 4 项 KPI、两个排行榜、26 周日历与语音排行", async () => {
    stubBridge();
    const wrapper = mount(ProfilePage);
    await flushPromises();
    await flushPromises();

    expect(wrapper.get("h1").text()).toBe("个人中心");
    expect(wrapper.get(".profile-header .badge").text()).toBe("数据仅存本机");
    expect(wrapper.findAll(".profile-kpi")).toHaveLength(4);
    const text = wrapper.text();
    expect(text).toContain("累计按键次数");
    expect(text).toContain("561");
    expect(text).toContain("累计语音时长");
    expect(text).toContain("33 分钟 12 秒");
    expect(text).toContain("Esc");
    expect(text).toContain("返回");
    expect(text).toContain("1:25");
    expect(wrapper.findAll(".voice-row")).toHaveLength(2);
    expect(wrapper.text()).toContain("微信");
    expect(wrapper.text()).toContain("未知应用");
    expect(wrapper.findAll(".calendar-grid button.calendar-day")).toHaveLength(26 * 7);
    expect(wrapper.text()).not.toContain("回眸");
    wrapper.unmount();
  });

  it("点击日历日期只在页面内显示当天详情", async () => {
    stubBridge();
    const wrapper = mount(ProfilePage);
    await flushPromises();
    await flushPromises();

    const day = wrapper.findAll(".calendar-day").find((cell) => cell.attributes("aria-label")?.includes("按键 4 次"));
    expect(day).toBeDefined();
    await day!.trigger("click");
    expect(wrapper.get(".calendar-detail").text()).toContain("按键 4 次");
    expect(wrapper.get(".calendar-detail").text()).not.toContain("回眸");
    wrapper.unmount();
  });

  it("读取失败时显示错误与重试，不伪造 0 数据", async () => {
    stubBridge();
    bridge.getUsageProfile.mockRejectedValueOnce(new Error("ipc failed"));
    const wrapper = mount(ProfilePage);
    await flushPromises();
    await flushPromises();

    expect(wrapper.get('[role="alert"]').text()).toContain("无法读取本机统计");
    expect(wrapper.text()).not.toContain("561");
    await wrapper.get('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("561");
    wrapper.unmount();
  });
});
