// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { effectiveLanguage } from "../lib/language-state";
import LocalWhisperDraft from "./LocalWhisperDraft.vue";
import type { WhisperDraft } from "../lib/local-whisper";

const ipc = vi.hoisted(() => ({
  canUseLocalWhisper: vi.fn(),
  pickLocalWhisperFile: vi.fn(),
  transcribeLocalWhisper: vi.fn(),
  cancelLocalWhisper: vi.fn(),
}));

vi.mock("../lib/local-whisper", () => ipc);

function openCard() {
  const wrapper = mount(LocalWhisperDraft);
  wrapper.get("details").element.open = true;
  return wrapper;
}

async function selectFiles(wrapper: ReturnType<typeof openCard>) {
  for (const kind of ["executable", "model", "audio"]) {
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
    expect(wrapper.get("textarea").element.value).toBe(result.text);
    expect(writeText).not.toHaveBeenCalled();
    await wrapper.get("textarea").setValue("改过的草稿");
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
    await wrapper.get("textarea").setValue("保留这段草稿");
    await wrapper.get("[data-whisper-start]").trigger("click");
    await wrapper.get("[data-whisper-cancel]").trigger("click");

    expect(wrapper.text()).toContain("正在取消转写");
    expect(ipc.cancelLocalWhisper).toHaveBeenCalledOnce();
    finishCancelled("whisper_cancelled");
    await flushPromises();
    expect(wrapper.text()).toContain("已取消转写");
    expect(wrapper.get("textarea").element.value).toBe("保留这段草稿");
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
    expect(wrapper.find("textarea").exists()).toBe(false);
    wrapper.unmount();
  });
});
