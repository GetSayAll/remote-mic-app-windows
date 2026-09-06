// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetAppUpdateForTests, appUpdateProgressText, useAppUpdate } from "./app-update";
import type { AppUpdateInfo } from "./bridge";

const checkAppUpdate = vi.fn<() => Promise<AppUpdateInfo>>();
const installAppUpdate = vi.fn<() => Promise<void>>();
const progressHandlers: Array<(progress: unknown) => void> = [];
const subscribeAppUpdateProgress = vi.fn(
  (handler: (progress: unknown) => void) => {
    progressHandlers.push(handler);
    return () => {};
  },
);

vi.mock("./bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bridge")>();
  return {
    ...actual,
    checkAppUpdate: () => checkAppUpdate(),
    installAppUpdate: () => installAppUpdate(),
    subscribeAppUpdateProgress: (handler: (progress: unknown) => void) =>
      subscribeAppUpdateProgress(handler),
  };
});

function emitProgress(downloaded: number, contentLength: number | null, finished: boolean): void {
  for (const handler of progressHandlers) {
    handler({ downloaded, contentLength, finished });
  }
}

const availableInfo: AppUpdateInfo = {
  currentVersion: "0.1.0",
  available: true,
  version: "0.2.0",
  notes: "修复若干问题",
  date: "2026-09-05T12:00:00Z",
};

const noUpdateInfo: AppUpdateInfo = {
  currentVersion: "0.1.0",
  available: false,
  version: null,
  notes: null,
  date: null,
};

describe("app update shared state", () => {
  beforeEach(() => {
    resetAppUpdateForTests();
    checkAppUpdate.mockReset();
    installAppUpdate.mockReset();
    subscribeAppUpdateProgress.mockClear();
    progressHandlers.length = 0;
  });

  it("手动检查发现新版本：进入 available 且横幅可见", async () => {
    checkAppUpdate.mockResolvedValue(availableInfo);
    const { phase, info, bannerVisible, check } = useAppUpdate();
    await check(true);
    expect(phase.value).toBe("available");
    expect(info.value?.version).toBe("0.2.0");
    expect(bannerVisible.value).toBe(true);
  });

  it("忽略横幅后不再可见", async () => {
    checkAppUpdate.mockResolvedValue(availableInfo);
    const { bannerVisible, check, dismissBanner } = useAppUpdate();
    await check(true);
    expect(bannerVisible.value).toBe(true);
    dismissBanner();
    expect(bannerVisible.value).toBe(false);
  });

  it("无更新：进入 up-to-date 且无横幅", async () => {
    checkAppUpdate.mockResolvedValue(noUpdateInfo);
    const { phase, bannerVisible, check } = useAppUpdate();
    await check(true);
    expect(phase.value).toBe("up-to-date");
    expect(bannerVisible.value).toBe(false);
  });

  it("手动检查失败：进入 failed 并保留错误信息", async () => {
    checkAppUpdate.mockRejectedValue(new Error("检查更新失败：网络错误"));
    const { phase, errorMessage, check } = useAppUpdate();
    await check(true);
    expect(phase.value).toBe("failed");
    expect(errorMessage.value).toContain("网络错误");
  });

  it("启动静默检查失败：完全无声（idle、无错误信息、无横幅）", async () => {
    checkAppUpdate.mockRejectedValue(new Error("检查更新失败：网络错误"));
    const { phase, errorMessage, bannerVisible, runStartupSilentCheck } = useAppUpdate();
    await runStartupSilentCheck();
    expect(phase.value).toBe("idle");
    expect(errorMessage.value).toBe("");
    expect(bannerVisible.value).toBe(false);
  });

  it("启动静默检查仅执行一次", async () => {
    checkAppUpdate.mockResolvedValue(noUpdateInfo);
    const { runStartupSilentCheck } = useAppUpdate();
    await runStartupSilentCheck();
    await runStartupSilentCheck();
    expect(checkAppUpdate).toHaveBeenCalledTimes(1);
  });

  it("检查中重复触发不叠加请求", async () => {
    let resolveCheck: (info: AppUpdateInfo) => void = () => {};
    checkAppUpdate.mockImplementation(
      () =>
        new Promise<AppUpdateInfo>((resolve) => {
          resolveCheck = resolve;
        }),
    );
    const { phase, check } = useAppUpdate();
    const first = check(true);
    const second = check(true);
    await second;
    expect(checkAppUpdate).toHaveBeenCalledTimes(1);
    expect(phase.value).toBe("checking");
    resolveCheck(availableInfo);
    await first;
    expect(phase.value).toBe("available");
  });

  it("安装流程：downloading → 进度事件 → installing", async () => {
    checkAppUpdate.mockResolvedValue(availableInfo);
    let resolveInstall: () => void = () => {};
    installAppUpdate.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveInstall = resolve;
        }),
    );
    const { phase, progress, check, install } = useAppUpdate();
    await check(true);
    const installing = install();
    // 冲刷微任务：让 install() 推进到 await installAppUpdate()（事件处理器
    // 已注册、resolveInstall 已捕获），测试侧再送进度事件并放行安装。
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(phase.value).toBe("downloading");
    emitProgress(1_048_576, 4_194_304, false);
    expect(progress.value).toEqual({ downloaded: 1_048_576, contentLength: 4_194_304, finished: false });
    emitProgress(4_194_304, 4_194_304, true);
    expect(phase.value).toBe("installing");
    resolveInstall();
    await installing;
    expect(phase.value).toBe("installing");
  });

  it("安装失败：进入 failed 并保留错误信息", async () => {
    checkAppUpdate.mockResolvedValue(availableInfo);
    installAppUpdate.mockRejectedValue(new Error("下载或安装更新失败：签名校验失败"));
    const { phase, errorMessage, check, install } = useAppUpdate();
    await check(true);
    await install();
    expect(phase.value).toBe("failed");
    expect(errorMessage.value).toContain("签名校验失败");
  });

  it("非 available 阶段安装被拒绝", async () => {
    const { phase, install } = useAppUpdate();
    await install();
    expect(phase.value).toBe("idle");
    expect(installAppUpdate).not.toHaveBeenCalled();
  });

  it("进度文案：带总长显示双值，无总长只显示已下载", () => {
    expect(
      appUpdateProgressText({ downloaded: 1_048_576, contentLength: 4_194_304, finished: false }),
    ).toBe("1.0 MB / 4.0 MB");
    expect(appUpdateProgressText({ downloaded: 524_288, contentLength: null, finished: false })).toBe(
      "0.5 MB",
    );
  });
});
