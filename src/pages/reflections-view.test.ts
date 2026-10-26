import { afterEach, describe, expect, it } from "vitest";
import type { TranscriptRecordView } from "../lib/bridge";
import { effectiveLanguage } from "../lib/language-state";
import {
  displayApplicationName,
  formatCount,
  formatDayLabel,
  formatRecordTime,
  groupTranscriptsByDay,
  newestDayKey,
  REFLECTIONS_RECENT_MS,
  visibleTranscripts,
} from "./reflections-view";

function record(
  id: string,
  applicationKey: string,
  endedAtMs: number,
  localDateKey: string,
  text = "测试文字",
): TranscriptRecordView {
  return {
    id,
    sessionId: `session-${id}`,
    endedAtMs,
    localDateKey,
    utcOffsetMinutes: 480,
    applicationKey,
    applicationName: applicationKey === "chrome-1" ? "Chrome" : "微信",
    text,
  };
}

afterEach(() => {
  effectiveLanguage.value = "zh-Hans";
});

describe("reflections view helpers", () => {
  it("shows only the recent seven days for all applications but all history for one app", () => {
    const now = REFLECTIONS_RECENT_MS + 10_000_000;
    const old = record("old", "chrome-1", 1_000, "2026-10-01");
    const recent = record("recent", "wechat-1", now - 1_000, "2026-10-25");
    const records = [old, recent];

    expect(visibleTranscripts(records, null, now).map((item) => item.id)).toEqual([
      "recent",
    ]);
    expect(visibleTranscripts(records, "chrome-1", now).map((item) => item.id)).toEqual([
      "old",
    ]);
  });

  it("groups by day newest first and keeps records newest first", () => {
    const groups = groupTranscriptsByDay([
      record("a", "chrome-1", 100, "2026-10-24"),
      record("b", "chrome-1", 300, "2026-10-25"),
      record("c", "wechat-1", 200, "2026-10-25"),
    ]);

    expect(groups.map((group) => group.key)).toEqual(["2026-10-25", "2026-10-24"]);
    expect(groups[0].records.map((item) => item.id)).toEqual(["b", "c"]);
    expect(newestDayKey(groups)).toBe("2026-10-25");
  });

  it("shows WeChat in the selected language without splitting its app key", () => {
    expect(displayApplicationName("Weixin")).toBe("微信");
    expect(displayApplicationName("WeChat")).toBe("微信");
    expect(displayApplicationName("WorkBuddy")).toBe("WorkBuddy");
    effectiveLanguage.value = "en";
    expect(displayApplicationName("Weixin")).toBe("WeChat");
  });

  it("formats dates and times without depending on the current locale", () => {
    expect(formatDayLabel("2026-10-25")).toBe("2026年10月25日");
    expect(formatDayLabel("not-a-date")).toBe("not-a-date");
    expect(
      formatRecordTime(record("time", "chrome-1", Date.UTC(2026, 9, 25, 1, 41), "2026-10-25")),
    ).toBe("09:41");
    expect(formatCount(3)).toBe("3 条记录");
  });

  it("formats dates and counts in English", () => {
    effectiveLanguage.value = "en";
    expect(formatDayLabel("2026-10-25")).toBe("October 25, 2026");
    expect(formatCount(1)).toBe("1 entry");
    expect(formatCount(3)).toBe("3 entries");
  });
});
