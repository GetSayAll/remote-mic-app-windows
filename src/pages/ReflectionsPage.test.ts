// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  TranscriptHistorySnapshot,
  TranscriptRecordView,
  TranscriptRecordingView,
} from "../lib/bridge";
import { effectiveLanguage } from "../lib/language-state";
import ReflectionsPage from "./ReflectionsPage.vue";

const bridge = vi.hoisted(() => ({
  getTranscriptHistory: vi.fn(),
  setTranscriptHistoryEnabled: vi.fn(),
  setTranscriptAudioRecordingEnabled: vi.fn(),
  getTranscriptAudio: vi.fn(),
  exportTranscriptAudio: vi.fn(),
  deleteTranscriptRecording: vi.fn(),
  deleteTranscript: vi.fn(),
  deleteTranscriptApplication: vi.fn(),
  deleteAllTranscripts: vi.fn(),
  subscribeTranscriptCaptured: vi.fn(),
  subscribeTranscriptRecordingFinished: vi.fn(),
}));

vi.mock("../lib/bridge", () => bridge);

function record(
  id: string,
  applicationKey: string,
  applicationName: string,
  text: string,
  offsetMs: number,
): TranscriptRecordView {
  return {
    id,
    sessionId: `session-${id}`,
    endedAtMs: Date.now() - offsetMs,
    localDateKey: "2026-10-25",
    utcOffsetMinutes: 480,
    applicationKey,
    applicationName,
    text,
  };
}

function recording(
  sessionId: string,
  overrides: Partial<TranscriptRecordingView> = {},
): TranscriptRecordingView {
  return {
    sessionId,
    endedAtMs: Date.now() - 1_000,
    localDateKey: "2026-10-25",
    utcOffsetMinutes: 480,
    applicationKey: "chrome-1",
    applicationName: "Chrome",
    durationMs: 1_000,
    byteCount: 3_244,
    ...overrides,
  };
}

function snapshot(
  overrides: Partial<TranscriptHistorySnapshot> = {},
): TranscriptHistorySnapshot {
  const records = [
    record("id-1", "chrome-1", "Chrome", "帮我把发布说明压缩成三条。", 1_000),
    record("id-2", "chrome-1", "Chrome", "第二条 Chrome 记录。", 2_000),
    record("id-3", "wechat-1", "微信", "今晚七点老地方见。", 3_000),
  ];
  return {
    enabled: true,
    audioEnabled: false,
    recordings: [],
    applications: [
      {
        key: "chrome-1",
        name: "Chrome",
        count: 2,
        latestEndedAtMs: records[0].endedAtMs,
        iconDataUrl: null,
      },
      {
        key: "wechat-1",
        name: "微信",
        count: 1,
        latestEndedAtMs: records[2].endedAtMs,
        iconDataUrl: null,
      },
    ],
    records,
    totalCount: 3,
    recentCount: 3,
    archiveError: null,
    ...overrides,
  };
}

describe("reflections page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    bridge.getTranscriptHistory.mockResolvedValue(snapshot());
    bridge.setTranscriptHistoryEnabled.mockResolvedValue(snapshot({ enabled: false }));
    bridge.setTranscriptAudioRecordingEnabled.mockResolvedValue(snapshot({ audioEnabled: true }));
    bridge.getTranscriptAudio.mockResolvedValue("data:audio/wav;base64,UklGRg==");
    bridge.exportTranscriptAudio.mockResolvedValue(true);
    bridge.deleteTranscriptRecording.mockResolvedValue(snapshot());
    bridge.deleteTranscript.mockImplementation(async (id: string) => {
      const current = snapshot();
      const records = current.records.filter((item) => item.id !== id);
      return {
        ...current,
        records,
        totalCount: records.length,
        recentCount: records.length,
      };
    });
    bridge.deleteTranscriptApplication.mockResolvedValue(
      snapshot({ records: [], applications: [], totalCount: 0, recentCount: 0 }),
    );
    bridge.deleteAllTranscripts.mockResolvedValue(
      snapshot({ records: [], applications: [], totalCount: 0, recentCount: 0 }),
    );
    bridge.subscribeTranscriptCaptured.mockResolvedValue(() => undefined);
    bridge.subscribeTranscriptRecordingFinished.mockResolvedValue(() => undefined);
    Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
      configurable: true,
      value: function showModal(this: HTMLDialogElement) {
        this.setAttribute("open", "");
      },
    });
    Object.defineProperty(HTMLDialogElement.prototype, "close", {
      configurable: true,
      value: function close(this: HTMLDialogElement) {
        this.removeAttribute("open");
      },
    });
  });

  afterEach(() => {
    effectiveLanguage.value = "zh-Hans";
    vi.unstubAllGlobals();
  });

  it("renders the header switch, applications and records", async () => {
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("回眸");
    expect(wrapper.text()).toContain("记录回眸");
    expect(wrapper.text()).toContain("全部记录");
    expect(wrapper.text()).toContain("3 条记录");
    expect(wrapper.text()).toContain("Chrome");
    expect(wrapper.text()).toContain("微信");
    expect(wrapper.text()).toContain("帮我把发布说明压缩成三条。");
    expect((wrapper.get("input[type=checkbox]").element as HTMLInputElement).checked).toBe(true);
  });

  it("filters to one application and shows its complete history", async () => {
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    const chromeChip = wrapper
      .findAll("button")
      .find((button) => button.text().includes("Chrome"));
    expect(chromeChip).toBeTruthy();
    await chromeChip!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("删除当前应用");
    expect(wrapper.text()).toContain("2 条记录");
    expect(wrapper.text()).toContain("第二条 Chrome 记录。");
    expect(wrapper.text()).not.toContain("今晚七点老地方见。");
  });

  it("persists the switch change", async () => {
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    await wrapper.get("input[type=checkbox]").setValue(false);
    await flushPromises();

    expect(bridge.setTranscriptHistoryEnabled).toHaveBeenCalledWith(false);
  });

  it("reverts the switch and reports when saving fails", async () => {
    bridge.setTranscriptHistoryEnabled.mockRejectedValueOnce(new Error("disk full"));
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    const checkbox = wrapper.get("input[type=checkbox]");
    await checkbox.setValue(false);
    await flushPromises();

    expect((checkbox.element as HTMLInputElement).checked).toBe(true);
    expect(wrapper.text()).toContain("保存回眸设置失败，请稍后重试。");
  });

  it("persists the original-audio switch only as a sub-option of Reflections", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(snapshot({ audioEnabled: true }));
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    const toggles = wrapper.findAll('input[type="checkbox"]');
    expect(toggles).toHaveLength(2);
    expect((toggles[1].element as HTMLInputElement).checked).toBe(true);
    await toggles[1].setValue(false);
    await flushPromises();
    expect(bridge.setTranscriptAudioRecordingEnabled).toHaveBeenCalledWith(false);
  });

  it("does not animate the header switches on first render", async () => {
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.get(".record-toggles").classes()).not.toContain("settled");
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(wrapper.get(".record-toggles").classes()).toContain("settled");
  });

  it("does not show a loading card before a fast first load resolves", async () => {
    const wrapper = mount(ReflectionsPage);
    expect(wrapper.find(".reflections-loading").exists()).toBe(false);
    await flushPromises();
    expect(wrapper.find(".reflections-loading").exists()).toBe(false);
    expect(wrapper.text()).toContain("帮我把发布说明压缩成三条。");
  });

  it("refreshes captured data without flashing the loading card", async () => {
    let captured: (() => void) | undefined;
    bridge.subscribeTranscriptCaptured.mockImplementation(
      async (callback: () => void) => {
        captured = callback;
        return () => undefined;
      },
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();
    expect(captured).toBeTypeOf("function");

    let resolveRefresh!: (value: TranscriptHistorySnapshot) => void;
    bridge.getTranscriptHistory.mockReturnValueOnce(
      new Promise<TranscriptHistorySnapshot>((resolve) => {
        resolveRefresh = resolve;
      }),
    );
    captured!();
    await flushPromises();

    expect(wrapper.find(".reflections-loading").exists()).toBe(false);
    expect(wrapper.text()).toContain("帮我把发布说明压缩成三条。");

    resolveRefresh(snapshot());
    await flushPromises();
    expect(wrapper.find(".reflections-loading").exists()).toBe(false);
    expect(wrapper.text()).toContain("帮我把发布说明压缩成三条。");
  });

  it("keeps original audio unavailable while Reflections is off", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(snapshot({ enabled: false }));
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    const toggles = wrapper.findAll('input[type="checkbox"]');
    expect((toggles[0].element as HTMLInputElement).checked).toBe(false);
    expect((toggles[1].element as HTMLInputElement).disabled).toBe(true);
  });

  it("plays and exports a linked recording", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({
        audioEnabled: true,
        recordings: [recording("session-id-1")],
      }),
    );
    const play = vi.fn().mockResolvedValue(undefined);
    const pause = vi.fn();
    class FakeAudio {
      src = "";
      onended: (() => void) | null = null;
      onerror: (() => void) | null = null;
      play = play;
      pause = pause;
    }
    vi.stubGlobal("Audio", FakeAudio);
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    await wrapper.get('button[title="播放录音"]').trigger("click");
    await flushPromises();
    expect(bridge.getTranscriptAudio).toHaveBeenCalledWith("session-id-1");
    expect(play).toHaveBeenCalled();
    expect(wrapper.find('button[title="停止播放"]').exists()).toBe(true);

    await wrapper.get('button[title="导出录音"]').trigger("click");
    await flushPromises();
    expect(bridge.exportTranscriptAudio).toHaveBeenCalledWith("session-id-1");
  });

  it("does not report a failed play when the recording ends", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({
        audioEnabled: true,
        recordings: [recording("session-id-1")],
      }),
    );
    const instances: FakeAudio[] = [];
    class FakeAudio {
      onended: (() => void) | null = null;
      onerror: (() => void) | null = null;
      play = vi.fn().mockResolvedValue(undefined);
      pause = vi.fn();
      private currentSrc = "";
      constructor() {
        instances.push(this);
      }
      get src(): string {
        return this.currentSrc;
      }
      set src(value: string) {
        this.currentSrc = value;
        if (value === "") queueMicrotask(() => this.onerror?.());
      }
    }
    vi.stubGlobal("Audio", FakeAudio);
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    await wrapper.get('button[title="播放录音"]').trigger("click");
    await flushPromises();
    instances[0].onended?.();
    await flushPromises();

    expect(wrapper.text()).not.toContain("无法播放这段录音，请稍后重试。");
    expect(wrapper.find('button[title="播放录音"]').exists()).toBe(true);
  });

  it("refreshes when a recording finishes without a text capture", async () => {
    let finished: (() => void) | undefined;
    bridge.subscribeTranscriptRecordingFinished.mockImplementation(
      async (callback: () => void) => {
        finished = callback;
        return () => undefined;
      },
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();
    expect(finished).toBeTypeOf("function");
    expect(wrapper.text()).not.toContain("仅录音");

    bridge.getTranscriptHistory.mockResolvedValueOnce(
      snapshot({
        recordings: [
          recording("orphan-refresh", {
            applicationKey: "wechat-1",
            applicationName: "微信",
          }),
        ],
        totalCount: 4,
        recentCount: 4,
      }),
    );
    finished!();
    await flushPromises();

    expect(bridge.getTranscriptHistory).toHaveBeenCalledTimes(2);
    expect(wrapper.text()).toContain("仅录音");
    expect(wrapper.text()).toContain("微信");
  });

  it("displays a legacy Weixin app name as 微信", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({
        recordings: [
          recording("orphan-weixin", {
            applicationKey: "wechat-1",
            applicationName: "Weixin",
          }),
        ],
        applications: [
          {
            key: "wechat-1",
            name: "Weixin",
            count: 1,
            latestEndedAtMs: Date.now(),
            iconDataUrl: null,
          },
        ],
        totalCount: 4,
        recentCount: 4,
      }),
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("微信");
    expect(wrapper.text()).not.toContain("Weixin");
  });

  it("shows and operates recording-only entries", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({
        audioEnabled: true,
        recordings: [
          recording("orphan-1", {
            applicationKey: "wechat-1",
            applicationName: "微信",
            endedAtMs: Date.now() - 86_400_000,
            localDateKey: "2026-10-24",
          }),
        ],
        totalCount: 4,
        recentCount: 4,
      }),
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("仅录音");
    expect(wrapper.text()).toContain("微信");
    expect(wrapper.text()).toContain("1 段录音");
    await wrapper.get('button[title="播放录音"]').trigger("click");
    await flushPromises();
    expect(bridge.getTranscriptAudio).toHaveBeenCalledWith("orphan-1");

    await wrapper.get('button[title="导出录音"]').trigger("click");
    await flushPromises();
    expect(bridge.exportTranscriptAudio).toHaveBeenCalledWith("orphan-1");

    await wrapper.get('button[title="删除录音"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("录音将移到回收站，不会影响其他记录。");
    const confirm = wrapper
      .findAll("dialog button")
      .find((button) => button.text() === "删除");
    await confirm!.trigger("click");
    await flushPromises();
    expect(bridge.deleteTranscriptRecording).toHaveBeenCalledWith("orphan-1");
  });

  it("deletes the entry and its recording with a single button", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({
        audioEnabled: true,
        recordings: [recording("session-id-1")],
      }),
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.find('button[title="删除录音"]').exists()).toBe(false);
    await wrapper.get('button[title="删除这条记录"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("记录和关联的录音将一起移到回收站");
    const confirm = wrapper
      .findAll("dialog button")
      .find((button) => button.text() === "删除");
    await confirm!.trigger("click");
    await flushPromises();

    expect(bridge.deleteTranscript).toHaveBeenCalledWith("id-1");
    expect(bridge.deleteTranscriptRecording).not.toHaveBeenCalled();
    expect(wrapper.text()).not.toContain("帮我把发布说明压缩成三条。");
  });

  it("keeps the record and reports when deletion fails", async () => {
    bridge.deleteTranscript.mockRejectedValueOnce(new Error("locked"));
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    await wrapper.get('button[title="删除这条记录"]').trigger("click");
    await flushPromises();
    const confirm = wrapper
      .findAll("dialog button")
      .find((button) => button.text() === "删除");
    await confirm!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("删除失败，记录仍保留。请稍后重试。");
    expect(wrapper.text()).toContain("帮我把发布说明压缩成三条。");
  });

  it("shows the disabled empty state without deleting existing history", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({
        enabled: false,
        records: [],
        applications: [],
        totalCount: 0,
        recentCount: 0,
      }),
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("开启后，新的语音转文字结果会保存在本机。");
    expect(wrapper.text()).toContain("已有记录不会因关闭开关而删除。");
    expect(wrapper.text()).not.toContain("全部删除");
  });

  it("confirms a single record deletion and refreshes the snapshot", async () => {
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    const deleteButton = wrapper.get('button[title="删除这条记录"]');
    await deleteButton.trigger("click");
    await flushPromises();
    expect((wrapper.get("dialog").element as HTMLDialogElement).open).toBe(true);
    expect(wrapper.text()).toContain("删除这条语音记录？");

    const confirm = wrapper
      .findAll("dialog button")
      .find((button) => button.text().includes("删除"));
    await confirm!.trigger("click");
    await flushPromises();

    expect(bridge.deleteTranscript).toHaveBeenCalledWith("id-1");
    expect(wrapper.text()).not.toContain("帮我把发布说明压缩成三条。");
  });

  it("shows the recent-window link when older records exist", async () => {
    bridge.getTranscriptHistory.mockResolvedValue(
      snapshot({ totalCount: 5, recentCount: 3 }),
    );
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("按应用查看更多记录");
  });

  it("renders the page in English", async () => {
    effectiveLanguage.value = "en";
    const wrapper = mount(ReflectionsPage);
    await flushPromises();

    expect(wrapper.text()).toContain("Reflections");
    expect(wrapper.text()).toContain("Save to Reflections");
    expect(wrapper.text()).toContain("All Entries");
    expect(wrapper.text()).toContain("3 entries");
    expect(wrapper.text()).toContain("All Apps");
    expect(wrapper.text()).toContain("Delete All");
    expect(wrapper.get("h1").text()).toBe("Reflections");
  });
});
