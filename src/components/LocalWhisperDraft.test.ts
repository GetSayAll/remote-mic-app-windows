// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { effectiveLanguage } from "../lib/language-state";
import LocalWhisperDraft from "./LocalWhisperDraft.vue";
import type { LiveWhisperStatus, WhisperDraft } from "../lib/local-whisper";

const ipc = vi.hoisted(() => ({
  canUseLocalWhisper: vi.fn(),
  pickLocalWhisperFile: vi.fn(),
  transcribeLocalWhisper: vi.fn(),
  cancelLocalWhisper: vi.fn(),
  armLocalWhisper: vi.fn(),
  getLocalWhisperStatus: vi.fn(),
  cancelLiveLocalWhisper: vi.fn(),
}));

vi.mock("../lib/local-whisper", () => ipc);

function openCard() {
  const wrapper = mount(LocalWhisperDraft);
  wrapper.get("details").element.open = true;
  return wrapper;
}

async function selectFiles(wrapper: ReturnType<typeof openCard>, includeAudio = true) {
  const kinds = includeAudio ? ["executable", "model", "audio"] : ["executable", "model"];
  for (const kind of kinds) {
    await wrapper.get(`[data-whisper-file="${kind}"]`).trigger("click");
    await flushPromises();
  }
}

describe("Local Whisper draft", () => {
  const result: WhisperDraft = { text: "测试语音", durationMs: 1600, elapsedMs: 700 };

  beforeEach(() => {
    vi.resetAllMocks();
    effectiveLanguage.value = "zh-Hans";
    ipc.canUseLocalWhisper.mockReturnValue(true);
    ipc.pickLocalWhisperFile
      .mockResolvedValueOnce("C:\\whisper\\whisper-cli.exe")
      .mockResolvedValueOnce("C:\\whisper\\ggml-base-q5_1.bin")
      .mockResolvedValueOnce("D:\\audio\\example.wav");
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("starts only on request and copies the edited draft only on request", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    ipc.transcribeLocalWhisper.mockResolvedValue(result);
    const wrapper = openCard();

    expect(wrapper.get<HTMLButtonElement>("[data-whisper-start]").element.disabled).toBe(true);
    await selectFiles(wrapper);
    expect(wrapper.text()).toContain("example.wav");
    expect(wrapper.text()).not.toContain("D:\\audio\\");
    expect(ipc.transcribeLocalWhisper).not.toHaveBeenCalled();
    await wrapper.get("[data-whisper-start]").trigger("click");
    await flushPromises();

    expect(ipc.transcribeLocalWhisper).toHaveBeenCalledWith({
      executable: "C:\\whisper\\whisper-cli.exe",
      model: "C:\\whisper\\ggml-base-q5_1.bin",
      wav: "D:\\audio\\example.wav",
      language: "zh",
    });
    expect(wrapper.get<HTMLTextAreaElement>("[data-whisper-draft]").element.value).toBe(result.text);
    expect(writeText).not.toHaveBeenCalled();
    await wrapper.get("[data-whisper-draft]").setValue("改过的草稿");
    await wrapper.get("[data-whisper-copy]").trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalledOnce();
    expect(writeText).toHaveBeenCalledWith("改过的草稿");
    wrapper.unmount();
  });

  it("cancels a running request and retains the previous draft", async () => {
    let finishCancelled!: (reason: string) => void;
    ipc.transcribeLocalWhisper
      .mockResolvedValueOnce(result)
      .mockImplementationOnce(() => new Promise<WhisperDraft>((_resolve, reject) => {
        finishCancelled = reject;
      }));
    ipc.cancelLocalWhisper.mockResolvedValue(true);
    const wrapper = openCard();
    await selectFiles(wrapper);
    await wrapper.get("[data-whisper-start]").trigger("click");
    await flushPromises();
    await wrapper.get("[data-whisper-draft]").setValue("保留这段草稿");
    await wrapper.get("[data-whisper-start]").trigger("click");
    await wrapper.get("[data-whisper-cancel]").trigger("click");

    expect(wrapper.text()).toContain("正在取消转写");
    expect(ipc.cancelLocalWhisper).toHaveBeenCalledOnce();
    finishCancelled("whisper_cancelled");
    await flushPromises();
    expect(wrapper.text()).toContain("已取消转写");
    expect(wrapper.get<HTMLTextAreaElement>("[data-whisper-draft]").element.value).toBe("保留这段草稿");
    wrapper.unmount();
  });

  it("localizes a structured verification error without revealing backend details", async () => {
    effectiveLanguage.value = "en";
    ipc.transcribeLocalWhisper.mockRejectedValue("whisper_unverified_engine");
    const wrapper = openCard();
    await selectFiles(wrapper);
    await wrapper.get("[data-whisper-start]").trigger("click");
    await flushPromises();

    expect(wrapper.get("[role=alert]").text()).toContain("executable could not be verified");
    expect(wrapper.text()).not.toContain("whisper_unverified_engine");
    expect(wrapper.find("[data-whisper-draft]").exists()).toBe(false);
    wrapper.unmount();
  });

  it("arms only on request and presents each remote phase without requiring a WAV file", async () => {
    vi.useFakeTimers();
    ipc.armLocalWhisper.mockResolvedValue({ token: 42 });
    ipc.getLocalWhisperStatus
      .mockResolvedValueOnce({ phase: "armed" })
      .mockResolvedValueOnce({ phase: "capturing" })
      .mockResolvedValueOnce({ phase: "transcribing" })
      .mockResolvedValueOnce({ phase: "ready", draft: result });
    const writeText = vi.fn();
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const wrapper = openCard();

    expect(wrapper.get<HTMLButtonElement>("[data-whisper-arm]").element.disabled).toBe(true);
    await selectFiles(wrapper, false);
    expect(wrapper.get<HTMLButtonElement>("[data-whisper-arm]").element.disabled).toBe(false);
    expect(wrapper.get<HTMLButtonElement>("[data-whisper-start]").element.disabled).toBe(true);
    expect(ipc.armLocalWhisper).not.toHaveBeenCalled();
    await wrapper.get("[data-whisper-arm]").trigger("click");
    await flushPromises();
    expect(ipc.armLocalWhisper).toHaveBeenCalledWith({
      executable: "C:\\whisper\\whisper-cli.exe",
      model: "C:\\whisper\\ggml-base-q5_1.bin",
      language: "zh",
    });
    expect(wrapper.text()).toContain("请按住遥控器语音键说话。");
    await vi.advanceTimersByTimeAsync(300);
    expect(wrapper.text()).toContain("正在接收遥控器语音");
    await vi.advanceTimersByTimeAsync(300);
    expect(wrapper.text()).toContain("正在本机转写");
    await vi.advanceTimersByTimeAsync(300);
    expect(wrapper.get<HTMLTextAreaElement>("[data-whisper-draft]").element.value).toBe(result.text);
    await vi.advanceTimersByTimeAsync(600);
    expect(ipc.getLocalWhisperStatus.mock.calls).toEqual([[42], [42], [42], [42]]);
    expect(writeText).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("keeps a deliberately cleared draft until the user accepts the next remote result", async () => {
    ipc.transcribeLocalWhisper.mockResolvedValue(result);
    ipc.armLocalWhisper.mockResolvedValue({ token: 12 });
    ipc.getLocalWhisperStatus.mockResolvedValue({ phase: "ready", draft: { ...result, text: "第二次" } });
    const wrapper = openCard();
    await selectFiles(wrapper);
    await wrapper.get("[data-whisper-start]").trigger("click");
    await flushPromises();
    await wrapper.get("[data-whisper-draft]").setValue("");
    await wrapper.get("[data-whisper-arm]").trigger("click");
    await flushPromises();

    expect(wrapper.get<HTMLTextAreaElement>("[data-whisper-draft]").element.value).toBe("");
    expect(wrapper.text()).toContain("第二次");
    await wrapper.get("[data-whisper-replace]").trigger("click");
    expect(wrapper.get<HTMLTextAreaElement>("[data-whisper-draft]").element.value).toBe("第二次");
    wrapper.unmount();
  });

  it("cancels a token returned after the user leaves during arming", async () => {
    let finishArming!: (value: { token: number }) => void;
    ipc.armLocalWhisper.mockImplementation(() => new Promise((resolve) => { finishArming = resolve; }));
    ipc.cancelLiveLocalWhisper.mockResolvedValue(true);
    const wrapper = openCard();
    await selectFiles(wrapper, false);
    await wrapper.get("[data-whisper-arm]").trigger("click");
    wrapper.unmount();
    finishArming({ token: 9 });
    await flushPromises();
    expect(ipc.cancelLiveLocalWhisper).toHaveBeenCalledExactlyOnceWith(9);
    expect(ipc.getLocalWhisperStatus).not.toHaveBeenCalled();
  });

  it("cancels the in-flight arm if the user closes the Whisper panel", async () => {
    let finishArming!: (value: { token: number }) => void;
    ipc.armLocalWhisper.mockImplementation(() => new Promise((resolve) => { finishArming = resolve; }));
    ipc.cancelLiveLocalWhisper.mockResolvedValue(true);
    const wrapper = openCard();
    await selectFiles(wrapper, false);
    await wrapper.get("[data-whisper-arm]").trigger("click");
    expect(wrapper.text()).toContain("正在验证本地模型和遥控器");

    const details = wrapper.get("details");
    details.element.open = false;
    await details.trigger("toggle");
    expect(wrapper.text()).toContain("正在取消转写");
    finishArming({ token: 10 });
    await flushPromises();

    expect(ipc.cancelLiveLocalWhisper).toHaveBeenCalledExactlyOnceWith(10);
    expect(ipc.getLocalWhisperStatus).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("已取消转写");
    wrapper.unmount();
  });

  it("drops a ready response in flight when the user cancels", async () => {
    let finishPoll!: (status: LiveWhisperStatus) => void;
    ipc.armLocalWhisper.mockResolvedValue({ token: 19 });
    ipc.getLocalWhisperStatus.mockImplementation(() => new Promise((resolve) => { finishPoll = resolve; }));
    ipc.cancelLiveLocalWhisper.mockResolvedValue(false);
    const wrapper = openCard();
    await selectFiles(wrapper, false);
    await wrapper.get("[data-whisper-arm]").trigger("click");
    await flushPromises();
    await wrapper.get("[data-whisper-cancel]").trigger("click");
    await flushPromises();
    finishPoll({ phase: "ready", draft: result });
    await flushPromises();
    expect(ipc.cancelLiveLocalWhisper).toHaveBeenCalledExactlyOnceWith(19);
    expect(wrapper.find("[data-whisper-draft]").exists()).toBe(false);
    expect(wrapper.text()).toContain("已取消转写");
    wrapper.unmount();
  });

  it("shows when the remote arm expires and allows arming again", async () => {
    ipc.armLocalWhisper.mockResolvedValue({ token: 21 });
    ipc.getLocalWhisperStatus.mockResolvedValue({ phase: "expired" });
    const wrapper = openCard();
    await selectFiles(wrapper, false);
    await wrapper.get("[data-whisper-arm]").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("本次遥控器听写已超时");
    expect(wrapper.get<HTMLButtonElement>("[data-whisper-arm]").element.disabled).toBe(false);
    wrapper.unmount();
  });
});
