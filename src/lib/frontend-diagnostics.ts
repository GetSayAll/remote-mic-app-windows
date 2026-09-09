import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./bridge";

const bootStarted = performance.now();

export interface FrontendDiagnosticEvent {
  event: string;
  phase: string;
  result: "passed" | "failed" | "unknown";
  reason: string;
  elapsedMs?: number;
}

export function reportFrontendEvent(event: FrontendDiagnosticEvent): void {
  if (!isTauriRuntime()) return;
  void invoke("report_frontend_event", {
    report: {
      ...event,
      elapsedMs: event.elapsedMs ?? Math.max(0, Math.round(performance.now() - bootStarted)),
    },
  }).catch(() => {
    // 日志 IPC 本身失败时不能递归上报；开发控制台保留一个无用户内容的信号。
    console.error("feature=frontend_diagnostics result=failed reason=ipc_unavailable");
  });
}

export function installFrontendDiagnostics(): void {
  reportFrontendEvent({
    event: "script_evaluation",
    phase: "started",
    result: "passed",
    reason: "entry_module_loaded",
  });
  window.addEventListener("error", () => {
    reportFrontendEvent({
      event: "window_error",
      phase: "completed",
      result: "failed",
      reason: "uncaught_javascript_error",
    });
  }, true);
  window.addEventListener("unhandledrejection", () => {
    reportFrontendEvent({
      event: "unhandled_rejection",
      phase: "completed",
      result: "failed",
      reason: "uncaught_async_error",
    });
  });
}
