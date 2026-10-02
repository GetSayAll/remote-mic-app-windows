import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RuntimeSnapshot } from "./bridge";
import { createTrayIconReporter, trayIconStateFromSnapshot } from "./tray-icon";

const reportTrayIconState = vi.hoisted(() => vi.fn<(state: unknown) => Promise<void>>());

vi.mock("./bridge", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bridge")>();
  return { ...actual, reportTrayIconState };
});

vi.mock("./frontend-diagnostics", () => ({
  reportFrontendEvent: vi.fn(),
}));

function snapshot(phase: string, voiceState: string): RuntimeSnapshot {
  return {
    appVersion: "0.5.0",
    platform: {
      connection: { phase, voiceState },
    },
  } as unknown as RuntimeSnapshot;
}

describe("tray icon state derivation", () => {
  it("只把已连接（含语音中）映射为 connected", () => {
    expect(trayIconStateFromSnapshot(snapshot("ready", "idle"))).toEqual({
      connected: true,
      streaming: false,
    });
    expect(trayIconStateFromSnapshot(snapshot("streaming", "streaming"))).toEqual({
      connected: true,
      streaming: true,
    });
    expect(trayIconStateFromSnapshot(snapshot("draining", "draining"))).toEqual({
      connected: true,
      streaming: false,
    });
    for (const phase of ["idle", "connecting", "reconnecting", "disconnected", "failed"]) {
      expect(trayIconStateFromSnapshot(snapshot(phase, "idle"))).toEqual({
        connected: false,
        streaming: false,
      });
    }
  });

  it("运行快照还没到手时按未连接处理", () => {
    expect(trayIconStateFromSnapshot(null)).toEqual({ connected: false, streaming: false });
  });
});

describe("tray icon reporter", () => {
  beforeEach(() => {
    reportTrayIconState.mockReset().mockResolvedValue(undefined);
  });

  it("状态未变化时不重复投递 IPC", async () => {
    const reporter = createTrayIconReporter();
    await reporter.report(snapshot("idle", "idle"));
    await reporter.report(snapshot("idle", "idle"));
    expect(reportTrayIconState).toHaveBeenCalledTimes(1);

    await reporter.report(snapshot("ready", "idle"));
    await reporter.report(snapshot("streaming", "streaming"));
    expect(reportTrayIconState.mock.calls.map(([state]) => state)).toEqual([
      { connected: false, streaming: false },
      { connected: true, streaming: false },
      { connected: true, streaming: true },
    ]);
  });

  it("投递失败时保留待投递状态，下一次轮询重试", async () => {
    reportTrayIconState.mockRejectedValueOnce(new Error("ipc failed"));
    const reporter = createTrayIconReporter();

    await reporter.report(snapshot("ready", "idle"));
    expect(reportTrayIconState).toHaveBeenCalledTimes(1);

    // 失败不写入已应用状态：同样的状态会再投一次，直到成功。
    await reporter.report(snapshot("ready", "idle"));
    expect(reportTrayIconState).toHaveBeenCalledTimes(2);

    // 成功之后才去重。
    await reporter.report(snapshot("ready", "idle"));
    expect(reportTrayIconState).toHaveBeenCalledTimes(2);
  });
});
