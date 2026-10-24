import { describe, expect, it } from "vitest";
import type { ButtonMappings, UsageProfileDay } from "./bridge";
import {
  PHYSICAL_BUTTONS,
  buildActionRanking,
  buildButtonRanking,
  buildCalendarWeeks,
  calendarIntensity,
  formatDuration,
  formatVoiceSessionDate,
  formatVoiceTotal,
  profileAccountLabel,
} from "./profile";

function disabled() {
  return { type: "disabled" as const };
}

function mappings(actions: ButtonMappings["actions"]): ButtonMappings {
  return { enabled: true, actions };
}

function days(from: string, count: number): UsageProfileDay[] {
  const start = new Date(`${from}T12:00:00`);
  return Array.from({ length: count }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
      date.getDate(),
    ).padStart(2, "0")}`;
    return {
      date: key,
      buttonPresses: index + 1,
      voiceSeconds: index * 10,
    };
  });
}

describe("formatDuration", () => {
  it("uses m:ss below one hour and h:mm:ss above it", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(59)).toBe("0:59");
    expect(formatDuration(60)).toBe("1:00");
    expect(formatDuration(85)).toBe("1:25");
    expect(formatDuration(3_661)).toBe("1:01:01");
  });

  it("treats invalid and negative durations as zero", () => {
    expect(formatDuration(Number.NaN)).toBe("0:00");
    expect(formatDuration(-1)).toBe("0:00");
    expect(formatDuration(Number.POSITIVE_INFINITY)).toBe("0:00");
  });
});

describe("formatVoiceTotal", () => {
  it("uses Mac's Chinese duration units", () => {
    expect(formatVoiceTotal(0)).toBe("0 秒");
    expect(formatVoiceTotal(32)).toBe("32 秒");
    expect(formatVoiceTotal(1_992)).toBe("33 分钟 12 秒");
    expect(formatVoiceTotal(3_723)).toBe("1 小时 2 分钟");
  });

  it("treats invalid and negative durations as zero", () => {
    expect(formatVoiceTotal(Number.NaN)).toBe("0 秒");
    expect(formatVoiceTotal(-1)).toBe("0 秒");
  });
});

describe("profileAccountLabel", () => {
  it("keeps only the first ten characters before the @", () => {
    expect(profileAccountLabel("abcdefghijk@example.com")).toBe("abcdefghij");
    expect(profileAccountLabel("abc@example.com")).toBe("abc");
    expect(profileAccountLabel("i***@example.com")).toBe("i***");
  });

  it("falls back to 个人中心 when no display name is available", () => {
    expect(profileAccountLabel(null)).toBe("个人中心");
    expect(profileAccountLabel("   ")).toBe("个人中心");
    expect(profileAccountLabel(undefined)).toBe("个人中心");
  });
});

describe("rankings", () => {
  it("groups historical button counts by the current single-click action", () => {
    const ranking = buildActionRanking(
      mappings({
        back: {
          single: { type: "shortcut", chord: { keys: ["escape"] } },
          double: disabled(),
          long: disabled(),
        },
        ok: { single: disabled(), double: disabled(), long: disabled() },
        home: {
          single: { type: "shortcut", chord: { keys: ["home"] } },
          double: disabled(),
          long: disabled(),
        },
      }),
      {
        "button.back": 47,
        "button.ok": 42,
        "button.home": 45,
      },
    );

    expect(ranking).toEqual([
      { title: "Esc", count: 47 },
      { title: "Home", count: 45 },
    ]);
  });

  it("ranks only physical buttons that have been pressed", () => {
    const ranking = buildButtonRanking({
      "button.back": 47,
      "button.ok": 42,
      "button.home": 45,
      voice: 100,
    });

    expect(ranking).toEqual([
      { title: "返回", count: 47 },
      { title: "主页", count: 45 },
      { title: "确定", count: 42 },
    ]);
  });

  it("keeps at most ten rows", () => {
    const counts = Object.fromEntries(
      PHYSICAL_BUTTONS.map((button, index) => [`button.${button}`, index + 1]),
    );
    expect(buildButtonRanking(counts)).toHaveLength(10);
    expect(buildActionRanking(mappings({}), counts)).toEqual([]);
  });
});

describe("calendar", () => {
  it("uses the Mac intensity thresholds", () => {
    expect(calendarIntensity(0, 0)).toBe(0);
    expect(calendarIntensity(20, 0)).toBe(0.5);
    expect(calendarIntensity(40, 0)).toBe(1);
    expect(calendarIntensity(0, 300)).toBe(1);
    expect(calendarIntensity(Number.NaN, -10)).toBe(0);
  });

  it("keeps Monday-first rows and pads leading blank days", () => {
    const weeks = buildCalendarWeeks(days("2026-10-21", 5), new Date(2026, 9, 25, 12));
    expect(weeks).toHaveLength(1);
    expect(weeks[0].days).toHaveLength(7);
    expect(weeks[0].days[0].date).toBeNull();
    expect(weeks[0].days[1].date).toBeNull();
    expect(weeks[0].days[2].date).toBe("2026-10-21");
    expect(weeks[0].days[6].date).toBe("2026-10-25");
  });

  it("uses column * 7 + row for a full 26-week window", () => {
    const recent = days("2026-04-26", 182);
    const weeks = buildCalendarWeeks(recent, new Date(2026, 9, 24, 12));
    expect(weeks).toHaveLength(27);
    expect(weeks.every((week) => week.days.length === 7)).toBe(true);
    expect(weeks[0].monthLabel).toBe("4月");
    expect(weeks.flatMap((week) => week.days).filter((day) => day.date !== null)).toHaveLength(
      182,
    );
    expect(weeks.flatMap((week) => week.days).filter((day) => day.isToday)).toHaveLength(1);
  });
});

describe("voice session date", () => {
  it("renders a Chinese local date and time", () => {
    const text = formatVoiceSessionDate(new Date(2026, 8, 21, 23, 26).getTime());
    expect(text).toContain("2026");
    expect(text).toContain("23:26");
  });

  it("does not fabricate a date for a missing timestamp", () => {
    expect(formatVoiceSessionDate(0)).toBe("—");
  });
});
