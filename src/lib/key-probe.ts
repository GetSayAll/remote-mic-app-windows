/**
 * 临时诊断（2026-10-04）：记录 Alt / Ctrl / Shift 类键的 DOM 到达情况，
 * 排查"WebView2 窗口里输入法语音热键不响应"的键投递问题。
 *
 * 背景：最小 wry/WebView2 探针（AltProbe）里右 Alt 事件完整到达 DOM 且豆包语音条
 * 可被唤起；本应用的窗口里却唤不起/卡住。需要先取证"本应用的 WebView2 是否收到
 * 键事件"，再决定是宿主层（Tauri/wry）还是页面/进程层（TSF 等）的差异。
 *
 * 只记录按键代码与修饰键状态，不含任何文字内容；排查完成后随修复提交移除或转正。
 */
import { reportFrontendEvent } from "./frontend-diagnostics";

function token(value: string, fallback: string): string {
  const cleaned = value.replace(/[^A-Za-z0-9_-]/g, "").slice(0, 20);
  return cleaned.length > 0 ? cleaned : fallback;
}

function isTracked(event: KeyboardEvent): boolean {
  return (
    event.code === "" ||
    event.code.startsWith("Alt") ||
    event.code.startsWith("Control") ||
    event.code.startsWith("Shift") ||
    event.key === "Alt" ||
    event.key === "Control" ||
    event.key === "Shift"
  );
}

export function installKeyProbe(target: Window = window): void {
  const log = (phase: "keydown" | "keyup", event: KeyboardEvent): void => {
    if (!isTracked(event)) return;
    reportFrontendEvent({
      event: "key_probe",
      phase,
      result: "unknown",
      reason: `code_${token(event.code, "empty")}_key_${token(event.key, "none")}_loc${event.location}`,
      detail: `c${event.ctrlKey ? 1 : 0}a${event.altKey ? 1 : 0}s${event.shiftKey ? 1 : 0}m${event.metaKey ? 1 : 0}g${event.getModifierState("AltGraph") ? 1 : 0}_rep${event.repeat ? 1 : 0}`,
    });
  };
  target.addEventListener("keydown", (event) => log("keydown", event), true);
  target.addEventListener("keyup", (event) => log("keyup", event), true);
}
