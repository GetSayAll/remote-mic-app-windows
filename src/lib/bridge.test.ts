import { describe, expect, it } from "vitest";
import {
  audioPhaseLabel,
  connectionPhaseLabel,
  type AudioPhase,
  type ConnectionPhase,
} from "./bridge";

describe("connection phase presentation", () => {
  it("covers every serialized Rust connection phase", () => {
    const phases: ConnectionPhase[] = [
      "idle",
      "connecting",
      "discovering",
      "awaiting_capabilities",
      "ready",
      "streaming",
      "draining",
      "disconnected",
      "failed",
    ];

    expect(phases.map(connectionPhaseLabel)).toEqual([
      "尚未连接",
      "正在打开 RC003",
      "正在发现 ATVV 服务与特征",
      "正在确认 ATVV 能力",
      "BLE / ATVV 已就绪",
      "正在接收 RC003 语音",
      "正在结束本次语音",
      "RC003 已断开",
      "连接失败",
    ]);
  });

  it("covers every serialized Rust audio phase", () => {
    const phases: AudioPhase[] = [
      "unconfigured",
      "ready",
      "streaming",
      "draining",
      "failed",
      "unsupported",
    ];

    expect(phases.map(audioPhaseLabel)).toEqual([
      "尚未选择输出端点",
      "WASAPI 已就绪",
      "正在写入 Windows 音频端点",
      "正在排空 Windows 音频缓冲",
      "WASAPI 输出失败",
      "当前环境不支持 WASAPI",
    ]);
  });
});
