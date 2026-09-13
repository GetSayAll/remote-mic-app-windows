import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import type { ConnectionSnapshot } from "../lib/bridge";
import BatteryIndicator from "./BatteryIndicator.vue";

const connection = (batteryLevel?: number | null, phase: ConnectionSnapshot["phase"] = "ready"): ConnectionSnapshot => ({
  phase, batteryLevel, remoteName: "RC003", remoteModel: "rc003", capabilities: null,
  voiceState: "idle", decodedSamples: 0, generation: 0, reconnectAttempt: 0,
  powerNotificationsAvailable: true, lastError: null,
});

describe("remote battery", () => {
  it.each([0, 20, 99, 100])("shows %i percent including an empty battery", (level) => {
    const wrapper = mount(BatteryIndicator, { props: { connection: connection(level) } });
    expect(wrapper.text()).toBe(`${level}%`);
    expect(wrapper.classes("low")).toBe(level <= 20);
    expect(wrapper.attributes("title")).toContain("Windows 缓存");
  });

  it.each([undefined, null, -1, 101, 0.5, NaN, Infinity])("keeps missing/invalid %s unknown", (level) => {
    const wrapper = mount(BatteryIndicator, { props: { connection: connection(level) } });
    expect(wrapper.text()).toBe("电量未知");
    expect(wrapper.classes("low")).toBe(false);
  });

  it("clears on disconnect, suspend, reconnect and recovers from a new reading", async () => {
    const wrapper = mount(BatteryIndicator, { props: { connection: connection(99) } });
    for (const phase of ["idle", "connecting", "discovering", "disconnected", "suspended", "reconnecting", "failed"] as const) {
      await wrapper.setProps({ connection: connection(99, phase) });
      expect(wrapper.text()).toBe("电量未知");
    }
    await wrapper.setProps({ connection: connection(18, "streaming") });
    expect(wrapper.text()).toBe("18%");
    await wrapper.setProps({ connection: null });
    expect(wrapper.text()).toBe("电量未知");
  });
});
