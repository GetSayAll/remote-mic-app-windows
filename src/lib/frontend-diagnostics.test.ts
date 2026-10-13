import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { reasonToken } from "./frontend-diagnostics";

/** Rust 侧 `diagnostic_token()` 放行的形状；其余一律记成 invalid。 */
const TOKEN = /^[A-Za-z0-9_.-]{1,64}$/;

describe("reasonToken（结构化日志只收稳定 token）", () => {
  it("布尔展开成 true/false，片段用 _ 连接", () => {
    expect(reasonToken("was_enabled", false, "switch_reverted")).toBe(
      "was_enabled_false_switch_reverted",
    );
    expect(reasonToken("enabled", true)).toBe("enabled_true");
    expect(reasonToken("bridge_phase", "failed")).toBe("bridge_phase_failed");
  });

  it("空格、=、中文等自由文本不再产出 invalid（替换成 _ 后仍是合法 token）", () => {
    const token = reasonToken("page=connection was_enabled=false error=授权未完成（UAC 被取消）");
    expect(token).toBe("page_connection_was_enabled_false_error_UAC");
    expect(token).toMatch(TOKEN);
  });

  it("去首尾下划线、截断到 64 字符、全空回落 unknown", () => {
    expect(reasonToken("  ", " x ")).toBe("x");
    expect(reasonToken("a".repeat(80))).toHaveLength(64);
    expect(reasonToken("a".repeat(80))).toMatch(TOKEN);
    expect(reasonToken(null, undefined, "")).toBe("unknown");
    expect(reasonToken(0)).toBe("0");
  });
});

/**
 * 源码守卫（2026-10-10 真机实测的回归点）：`reportFrontendEvent` 的 `reason`
 * 必须是稳定 token——写成带空格 / `=` / 中文的模板串时，后端 `diagnostic_token()`
 * 一律记 `reason=invalid`，失败原因等于没记。这里静态扫一遍所有调用点：
 * 要么是 token 字面量，要么经 `reasonToken(...)` 构造。
 */
function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "node_modules") continue;
      out.push(...sourceFiles(full));
      continue;
    }
    if (entry.endsWith(".ts") || entry.endsWith(".vue")) out.push(full);
  }
  return out;
}

describe("reportFrontendEvent 的 reason 必须是稳定 token", () => {
  it("全部调用点通过（否则后端日志里会是 reason=invalid）", () => {
    const srcRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
    const offenders: string[] = [];
    let checked = 0;

    for (const file of sourceFiles(srcRoot)) {
      const text = readFileSync(file, "utf8");
      const call = /reportFrontendEvent\(/g;
      let match: RegExpExecArray | null;
      while ((match = call.exec(text)) !== null) {
        const segment = text.slice(match.index, match.index + 900);
        const reason = /reason:\s*(`[^`]*`|"[^"]*"|reasonToken\([^\n]*?\))/.exec(segment);
        if (!reason) continue;
        const raw = reason[1];
        checked += 1;
        if (raw.startsWith("reasonToken(")) continue;
        const literal = raw.slice(1, -1);
        if (literal.includes("${") || !TOKEN.test(literal)) {
          offenders.push(`${relative(srcRoot, file)}: ${literal}`);
        }
      }
    }

    expect(checked).toBeGreaterThan(10);
    expect(offenders).toEqual([]);
  });
});
