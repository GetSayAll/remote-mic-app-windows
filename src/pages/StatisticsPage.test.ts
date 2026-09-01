// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as bridge from "../lib/bridge";
import StatisticsPage from "./StatisticsPage.vue";

describe("statistics page", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows persisted period totals and recent local-only aggregates", async () => {
    vi.spyOn(bridge, "getUsageStatistics").mockResolvedValue({
      today: { buttonPresses: 2, voiceSessions: 1, voiceSeconds: 30 },
      thisWeek: { buttonPresses: 12, voiceSessions: 4, voiceSeconds: 125 },
      total: { buttonPresses: 42, voiceSessions: 8, voiceSeconds: 3_661 },
      recentDays: [
        { localDate: "2026-08-27", usage: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 } },
        { localDate: "2026-08-28", usage: { buttonPresses: 4, voiceSessions: 2, voiceSeconds: 95 } },
        { localDate: "2026-08-29", usage: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 } },
        { localDate: "2026-08-30", usage: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 } },
        { localDate: "2026-08-31", usage: { buttonPresses: 6, voiceSessions: 1, voiceSeconds: 30 } },
        { localDate: "2026-09-01", usage: { buttonPresses: 4, voiceSessions: 1, voiceSeconds: 65 } },
        { localDate: "2026-09-02", usage: { buttonPresses: 2, voiceSessions: 1, voiceSeconds: 30 } },
      ],
    });

    const wrapper = mount(StatisticsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("仅保存在本机");
    expect(wrapper.text()).toContain("2 次 · 30秒");
    expect(wrapper.findAll(".metric-card strong").map((item) => item.text())).toEqual([
      "2",
      "1",
      "30秒",
    ]);

    await wrapper.findAll(".period-switch button")[1].trigger("click");
    expect(wrapper.findAll(".metric-card strong").map((item) => item.text())).toEqual([
      "12",
      "4",
      "2分5秒",
    ]);

    await wrapper.findAll(".period-switch button")[2].trigger("click");
    expect(wrapper.findAll(".metric-card strong").map((item) => item.text())).toEqual([
      "42",
      "8",
      "1小时1分钟",
    ]);
    wrapper.unmount();
  });

  it("keeps the empty state truthful when no Windows usage has been recorded", async () => {
    vi.spyOn(bridge, "getUsageStatistics").mockResolvedValue({
      today: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
      thisWeek: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
      total: { buttonPresses: 0, voiceSessions: 0, voiceSeconds: 0 },
      recentDays: [],
    });

    const wrapper = mount(StatisticsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("暂无可展示的数据");
    expect(wrapper.text()).not.toContain("最近 7 天");
    wrapper.unmount();
  });
});
