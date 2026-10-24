import type { ButtonMappings, RemoteButton, UsageProfileDay } from "./bridge";
import { actionSummary, buttonLabel } from "./bridge";

/** 小米遥控器物理解析顺序：与 Rust `ALL_BUTTONS` / Mac `RemoteButton.xiaomiCases` 对齐。 */
export const PHYSICAL_BUTTONS: RemoteButton[] = [
  "back",
  "ok",
  "tv",
  "home",
  "right",
  "left",
  "down",
  "up",
  "menu",
  "power",
  "volume_mute",
  "volume_up",
  "volume_down",
];

export const WEEKDAY_LABELS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

export interface RankingEntry {
  title: string;
  count: number;
}

export interface CalendarDay {
  date: string | null;
  buttonPresses: number;
  voiceSeconds: number;
  intensity: number;
  isToday: boolean;
}

export interface CalendarWeek {
  monthLabel: string | null;
  days: CalendarDay[];
}

const countFormatter = new Intl.NumberFormat("zh-CN");

/** Mac `chartDurationText` 同款：`m:ss`，超过 1 小时用 `h:mm:ss`。 */
export function formatDuration(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.round(seconds) : 0;
  const hours = Math.floor(total / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  const remainder = total % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}`;
  }
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
}

/**
 * Mac `voiceDurationText` 同款：累计语音 KPI 使用中文单位，不与单次排行的
 * `m:ss` 混用；超过 1 小时按 Mac 口径省略秒。
 */
export function formatVoiceTotal(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.round(seconds) : 0;
  const hours = Math.floor(total / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  const remainder = total % 60;
  if (hours > 0) {
    return `${countFormatter.format(hours)} 小时 ${countFormatter.format(minutes)} 分钟`;
  }
  if (minutes > 0) {
    return `${countFormatter.format(minutes)} 分钟 ${countFormatter.format(remainder)} 秒`;
  }
  return `${countFormatter.format(remainder)} 秒`;
}

/**
 * 侧栏个人入口的账号名：与 Mac 一致，只取邮箱 `@` 前最多 10 个字符。
 *
 * 完整邮箱仍只在会员页显示；侧栏和无障碍名称都不应出现完整地址。
 */
export function profileAccountLabel(
  displayName: string | null | undefined,
  fallback = "个人中心",
): string {
  const trimmed = displayName?.trim();
  if (!trimmed) return fallback;
  const localPart = trimmed.includes("@") ? trimmed.split("@")[0] : trimmed;
  return Array.from(localPart).slice(0, 10).join("") || fallback;
}

/** Mac 的日历强度口径：按键 40 次或语音 300 秒记满分。 */
export function calendarIntensity(buttonPresses: number, voiceSeconds: number): number {
  const buttons = Number.isFinite(buttonPresses) ? Math.max(0, buttonPresses) : 0;
  const voice = Number.isFinite(voiceSeconds) ? Math.max(0, voiceSeconds) : 0;
  return Math.min(1, Math.max(buttons / 40, voice / 300));
}

function buttonControlKey(button: RemoteButton): string {
  return `button.${button}`;
}

/**
 * 动作排行榜：按当前单击动作汇总历史按键数。
 *
 * 被禁用或没有单击动作的按键不计入动作榜，但仍会出现在实体按键榜；这与 Mac
 * `statisticsActionRanking` 一致，也让“改了映射后历史计数跟着新动作走”。
 */
export function buildActionRanking(
  mappings: ButtonMappings | null | undefined,
  counts: Record<string, number>,
): RankingEntry[] {
  if (!mappings) return [];
  const totals = new Map<string, number>();
  for (const button of PHYSICAL_BUTTONS) {
    const count = Math.max(0, counts[buttonControlKey(button)] ?? 0);
    if (count === 0) continue;
    const action = mappings.actions[button]?.single;
    if (!action || action.type === "disabled") continue;
    const title = actionSummary(action);
    totals.set(title, (totals.get(title) ?? 0) + count);
  }
  return [...totals.entries()]
    .map(([title, count]) => ({ title, count }))
    .sort((left, right) =>
      left.count === right.count
        ? left.title.localeCompare(right.title, "zh-CN")
        : right.count - left.count,
    )
    .slice(0, 10);
}

/** 实体按键排行榜：13 个实体键，按次数降序、同次数按名称升序。 */
export function buildButtonRanking(counts: Record<string, number>): RankingEntry[] {
  return PHYSICAL_BUTTONS.map((button) => ({
    title: buttonLabel(button),
    count: Math.max(0, counts[buttonControlKey(button)] ?? 0),
  }))
    .filter((entry) => entry.count > 0)
    .sort((left, right) =>
      left.count === right.count
        ? left.title.localeCompare(right.title, "zh-CN")
        : right.count - left.count,
    )
    .slice(0, 10);
}

function parseDayKey(key: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(year, month - 1, day, 12, 0, 0, 0);
  return Number.isNaN(date.getTime()) ? null : date;
}

function dayKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
    date.getDate(),
  ).padStart(2, "0")}`;
}

function mondayOffset(date: Date): number {
  return (date.getDay() + 6) % 7;
}

function emptyCalendarDay(): CalendarDay {
  return {
    date: null,
    buttonPresses: 0,
    voiceSeconds: 0,
    intensity: 0,
    isToday: false,
  };
}

/**
 * 把后端返回的最近 26 周日期整理成按周分列的日历。
 *
 * 关键不变量（Mac 2026-09-29 Bug）：同一列是同一周，格子索引必须是
 * `column * 7 + row`；首尾空格补齐到完整周，不能把连续日期排成一行。
 */
export function buildCalendarWeeks(
  recentDays: UsageProfileDay[],
  today = new Date(),
): CalendarWeek[] {
  const parsed = recentDays
    .map((day) => ({ ...day, parsed: parseDayKey(day.date) }))
    .filter((day): day is UsageProfileDay & { parsed: Date } => day.parsed !== null);
  if (parsed.length === 0) return [];

  const todayKey = dayKey(today);
  const first = parsed[0].parsed;
  const leading = mondayOffset(first);
  const cells: CalendarDay[] = [
    ...Array.from({ length: leading }, emptyCalendarDay),
    ...parsed.map((day) => ({
      date: day.date,
      buttonPresses: Math.max(0, day.buttonPresses),
      voiceSeconds: Math.max(0, day.voiceSeconds),
      intensity: calendarIntensity(day.buttonPresses, day.voiceSeconds),
      isToday: day.date === todayKey,
    })),
  ];
  const remainder = cells.length % 7;
  if (remainder !== 0) {
    cells.push(...Array.from({ length: 7 - remainder }, emptyCalendarDay));
  }

  const columns = cells.length / 7;
  const weeks: CalendarWeek[] = Array.from({ length: columns }, () => ({
    monthLabel: null,
    days: [],
  }));
  for (let column = 0; column < columns; column += 1) {
    weeks[column].days = cells.slice(column * 7, column * 7 + 7);
  }

  // 月份标签：只标每个月的第一周；至少隔 2 列，避免窄窗口重叠。
  const monthFormatter = new Intl.DateTimeFormat("zh-CN", { month: "short" });
  const seen = new Set<string>();
  let nextAvailable = 0;
  for (let column = 0; column < columns; column += 1) {
    const firstDate = weeks[column].days.find((day) => day.date !== null)?.date;
    if (!firstDate) continue;
    const parsedDate = parseDayKey(firstDate);
    if (!parsedDate) continue;
    const label = monthFormatter.format(parsedDate);
    if (seen.has(label)) continue;
    seen.add(label);
    const displayColumn = Math.max(column, nextAvailable);
    if (displayColumn < columns) {
      weeks[displayColumn].monthLabel = label;
      nextAvailable = displayColumn + 2;
    }
  }
  return weeks;
}

/** 日历选中日期的展示：`2026年9月21日`。 */
export function formatCalendarDate(dateKey: string): string {
  const parsed = parseDayKey(dateKey);
  if (!parsed) return dateKey;
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(parsed);
}

/** 语音排行的结束时间：`2026年9月21日 23:26`。 */
export function formatVoiceSessionDate(endedAtMs: number): string {
  if (!Number.isFinite(endedAtMs) || endedAtMs <= 0) return "—";
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(endedAtMs));
}
