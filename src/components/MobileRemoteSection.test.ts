import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PhoneRemoteSnapshot } from "../lib/bridge";
import { effectiveLanguage } from "../lib/language-state";
import MobileRemoteSection from "./MobileRemoteSection.vue";

const mocks = vi.hoisted(() => ({
  startPhoneRemoteIos: vi.fn(),
  stopPhoneRemoteIos: vi.fn(),
  approvePhoneRemote: vi.fn(),
  denyPhoneRemote: vi.fn(),
  clearPhoneRemoteTrustedDevices: vi.fn(),
  startPhoneRemoteWeb: vi.fn(),
  stopPhoneRemoteWeb: vi.fn(),
  openIOSAppStore: vi.fn(),
  iosAppStoreUrl: vi.fn(() => "https://apps.apple.com/app/sayall/id6797208067"),
}));

vi.mock("../lib/bridge", async (importOriginal) => {
  const original = await importOriginal<typeof import("../lib/bridge")>();
  return {
    ...original,
    ...mocks,
  };
});

function snapshot(patch: Partial<PhoneRemoteSnapshot> = {}): PhoneRemoteSnapshot {
  return {
    availability: "available",
    membership: "active",
    ios: {
      state: "idle",
      expiresAt: null,
      deviceName: null,
      qrCode: null,
    },
    web: {
      state: "idle",
      expiresAt: null,
      deviceName: null,
      miniProgramCode: null,
    },
    trustedDeviceCount: 2,
    approval: null,
    ...patch,
  };
}

describe("MobileRemoteSection", () => {
  beforeEach(() => {
    effectiveLanguage.value = "zh-Hans";
    vi.clearAllMocks();
  });

  it("renders nothing when the private runtime is unavailable", () => {
    const wrapper = mount(MobileRemoteSection, {
      props: { snapshot: snapshot({ availability: "unavailable" }) },
    });
    expect(wrapper.find(".mobile-remote-card").exists()).toBe(false);
  });

  it("shows the membership gate on both rows and navigates to membership", async () => {
    const wrapper = mount(MobileRemoteSection, {
      props: { snapshot: snapshot({ membership: "plus_required" }) },
    });
    const buttons = wrapper.findAll(".mobile-action");
    expect(buttons).toHaveLength(2);
    expect(buttons[0].text()).toContain("点击开通会员功能");
    expect(buttons[1].text()).toContain("点击开通会员功能");
    await buttons[0].trigger("click");
    expect(wrapper.emitted("navigate")?.[0]).toEqual(["membership"]);
    expect(mocks.startPhoneRemoteIos).not.toHaveBeenCalled();
  });

  it("shows the iOS QR while waiting and stops waiting", async () => {
    const waiting = snapshot({
      ios: {
        state: "waiting",
        expiresAt: 1_900_000_000,
        deviceName: null,
        qrCode: "data:image/png;base64,AAAA",
      },
    });
    mocks.stopPhoneRemoteIos.mockResolvedValue(snapshot());
    const wrapper = mount(MobileRemoteSection, { props: { snapshot: waiting } });

    expect(wrapper.find(".mobile-qr-frame img").attributes("src")).toContain("data:image/png");
    expect(wrapper.text()).toContain("正在等待手机");
    await wrapper.findAll(".mobile-action")[0].trigger("click");
    await flushPromises();
    expect(mocks.stopPhoneRemoteIos).toHaveBeenCalledOnce();
  });

  it("shows the mini-program row as unavailable but visible", () => {
    const wrapper = mount(MobileRemoteSection, {
      props: {
        snapshot: snapshot({
          web: {
            state: "unavailable",
            expiresAt: null,
            deviceName: null,
            miniProgramCode: null,
          },
        }),
      },
    });
    expect(wrapper.text()).toContain("无线麦 SayAll 微信小程序");
    expect(wrapper.text()).toContain("尚未开放");
    expect(wrapper.findAll(".mobile-action")[1].attributes("disabled")).toBeDefined();
  });

  it("shows both connected rows independently", () => {
    const wrapper = mount(MobileRemoteSection, {
      props: {
        snapshot: snapshot({
          ios: {
            state: "connected",
            expiresAt: null,
            deviceName: "Andy 的 iPhone",
            qrCode: null,
          },
          web: {
            state: "connected",
            expiresAt: null,
            deviceName: "微信",
            miniProgramCode: "data:image/png;base64,BBBB",
          },
        }),
      },
    });
    expect(wrapper.text()).toContain("已连接");
    expect(wrapper.text()).toContain("手机遥控已连接");
    const buttons = wrapper.findAll(".mobile-action");
    expect(buttons[0].text()).toContain("取消连接");
    expect(buttons[1].text()).toContain("显示二维码");
  });

  it("opens the web mini-program code without disconnecting", async () => {
    const wrapper = mount(MobileRemoteSection, {
      props: {
        snapshot: snapshot({
          web: {
            state: "waiting_for_scan",
            expiresAt: null,
            deviceName: null,
            miniProgramCode: "data:image/png;base64,CCCC",
          },
        }),
      },
    });
    await wrapper.findAll(".mobile-action")[1].trigger("click");
    expect(wrapper.find(".mobile-web-code img").exists()).toBe(true);
    expect(wrapper.text()).toContain("使用微信扫描小程序码");
    expect(mocks.stopPhoneRemoteWeb).not.toHaveBeenCalled();
  });

  it("approves with the request id and closes the modal", async () => {
    const approval = {
      requestId: "approval-1",
      deviceName: "Andy 的 iPhone",
      pairingCode: "42",
    };
    mocks.approvePhoneRemote.mockResolvedValue(
      snapshot({
        ios: {
          state: "connected",
          expiresAt: null,
          deviceName: approval.deviceName,
          qrCode: null,
        },
      }),
    );
    const wrapper = mount(MobileRemoteSection, {
      props: {
        snapshot: snapshot({
          ios: {
            state: "waiting_for_approval",
            expiresAt: null,
            deviceName: approval.deviceName,
            qrCode: null,
          },
          approval,
        }),
      },
    });
    expect(wrapper.text()).toContain("允许“Andy 的 iPhone”连接？");
    expect(wrapper.text()).toContain("42");
    const allow = wrapper.findAll(".mobile-modal button").find((button) =>
      button.text().includes("允许连接"),
    )!;
    await allow.trigger("click");
    await flushPromises();
    expect(mocks.approvePhoneRemote).toHaveBeenCalledWith("approval-1");
    expect(wrapper.find(".mobile-modal-layer").exists()).toBe(false);
  });

  it("asks before clearing trusted devices", async () => {
    mocks.clearPhoneRemoteTrustedDevices.mockResolvedValue(
      snapshot({ trustedDeviceCount: 0 }),
    );
    const wrapper = mount(MobileRemoteSection, { props: { snapshot: snapshot() } });
    await wrapper.find(".mobile-remote-footer .link-button").trigger("click");
    expect(wrapper.text()).toContain("清除所有受信任设备？");
    const confirm = wrapper.findAll(".mobile-modal button").find((button) =>
      button.text().includes("清除受信任设备"),
    )!;
    await confirm.trigger("click");
    await flushPromises();
    expect(mocks.clearPhoneRemoteTrustedDevices).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("0 台受信任设备");
  });
});
