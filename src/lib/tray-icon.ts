import type { RuntimeSnapshot, TrayIconState } from "./bridge";
import { reportTrayIconState } from "./bridge";
import { reportFrontendEvent } from "./frontend-diagnostics";

/**
 * 托盘图标状态（2026-10-02）：
 *
 * 样式（彩色应用图标 / 单色状态图标）由设置持久化在 Rust 侧；这里只把"当前
 * 连接与语音状态"告诉 Rust，让它对单色状态图标做与 Mac main 相同的呈现——
 * 未连接时整体变暗（对应 Mac `NSStatusBarButton.appearsDisabled`），连接后
 * 恢复，语音中保持同一形状（Mac main 的 `StatusIconActiveTemplate` 与基础
 * 模板逐字节相同，故 Windows 侧不另造活动图标，见 scripts/generate-tray-icons.py）。
 *
 * 状态源与界面同源：`get_runtime_snapshot` 每秒轮询的结果。重复状态不重复投递，
 * 投递失败保留待投递状态，下一次轮询自然重试（不断言、不阻塞界面）。
 */
const CONNECTED_PHASES = new Set(["ready", "streaming", "draining"]);

export function trayIconStateFromSnapshot(snapshot: RuntimeSnapshot | null): TrayIconState {
  const connection = snapshot?.platform.connection;
  if (!connection) return { connected: false, streaming: false };
  const connected = CONNECTED_PHASES.has(connection.phase);
  return {
    connected,
    streaming: connected && connection.voiceState === "streaming",
  };
}

/**
 * 去重上报器：只在状态变化时调用 IPC；失败保持"待重试"，不改变调用方语义。
 * 浏览器预览（非 Tauri）下 `reportTrayIconState` 直接返回，不会产生副作用。
 */
export function createTrayIconReporter(): {
  report(snapshot: RuntimeSnapshot | null): Promise<void>;
} {
  let lastApplied: string | null = null;
  return {
    async report(snapshot: RuntimeSnapshot | null): Promise<void> {
      const state = trayIconStateFromSnapshot(snapshot);
      const key = `${state.connected ? "connected" : "disconnected"}:${state.streaming ? "streaming" : "idle"}`;
      if (key === lastApplied) return;
      try {
        await reportTrayIconState(state);
        lastApplied = key;
      } catch (error) {
        // 失败不写 lastApplied：下一次轮询会重试同一次状态。日志只记结果与
        // 状态名，不带任何用户信息。
        reportFrontendEvent({
          event: "tray_icon_state",
          phase: "completed",
          result: "failed",
          reason: "ipc_unavailable",
        });
        console.error("feature=tray_icon_state result=failed reason=ipc_unavailable", error);
      }
    },
  };
}
