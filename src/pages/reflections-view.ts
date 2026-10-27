import type { TranscriptRecordView } from "../lib/bridge";
import { effectiveLanguage } from "../lib/language-state";
import { t } from "../lib/i18n";

/** 全部应用视图的最近窗口：7 天。与应用存储层/后端快照的统计口径一致。 */
export const REFLECTIONS_RECENT_MS = 7 * 24 * 60 * 60 * 1_000;

export interface TranscriptDayGroup {
  key: string;
  records: TranscriptRecordView[];
}

export function visibleTranscripts(
  records: readonly TranscriptRecordView[],
  applicationKey: string | null,
  nowMs: number,
): TranscriptRecordView[] {
  const cutoff = nowMs - REFLECTIONS_RECENT_MS;
  return records
    .filter((record) =>
      applicationKey
        ? record.applicationKey === applicationKey
        : record.endedAtMs >= cutoff,
    )
    .slice()
    .sort(compareRecords);
}

export function groupTranscriptsByDay(
  records: readonly TranscriptRecordView[],
): TranscriptDayGroup[] {
  const groups = new Map<string, TranscriptRecordView[]>();
  for (const record of records) {
    const existing = groups.get(record.localDateKey) ?? [];
    existing.push(record);
    groups.set(record.localDateKey, existing);
  }
  return [...groups.entries()]
    .map(([key, groupRecords]) => ({
      key,
      records: groupRecords.slice().sort(compareRecords),
    }))
    .sort((left, right) => {
      const leftNewest = left.records[0]?.endedAtMs ?? 0;
      const rightNewest = right.records[0]?.endedAtMs ?? 0;
      return rightNewest - leftNewest;
    });
}

export function newestDayKey(groups: readonly TranscriptDayGroup[]): string | null {
  return groups[0]?.key ?? null;
}

export function formatDayLabel(localDateKey: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(localDateKey);
  if (!match) return localDateKey;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (effectiveLanguage.value === "en") {
    return new Intl.DateTimeFormat("en-US", {
      year: "numeric",
      month: "long",
      day: "numeric",
      timeZone: "UTC",
    }).format(new Date(Date.UTC(year, month - 1, day)));
  }
  return `${year}年${month}月${day}日`;
}

/** 用记录发生时的本地偏移显示时间；不跟随用户之后的时区变化。 */
export function formatTimestamp(endedAtMs: number, utcOffsetMinutes: number): string {
  const local = new Date(endedAtMs + utcOffsetMinutes * 60_000);
  const hours = String(local.getUTCHours()).padStart(2, "0");
  const minutes = String(local.getUTCMinutes()).padStart(2, "0");
  return `${hours}:${minutes}`;
}

export function formatRecordTime(record: TranscriptRecordView): string {
  return formatTimestamp(record.endedAtMs, record.utcOffsetMinutes);
}

/** 旧 manifest 可能把微信写成 Weixin/WeChat；界面统一按当前语言显示。 */
export function displayApplicationName(name: string): string {
  if (name === "微信" || name === "Weixin" || name === "WeChat") {
    return t("微信");
  }
  return name;
}

export function formatRecordingCount(count: number): string {
  return count === 1 ? t("1 段录音") : t("{count} 段录音", { count });
}

export function formatRecordingDuration(durationMs: number): string {
  return t("{count} 秒", { count: Math.max(1, Math.round(durationMs / 1_000)) });
}

export function formatCount(count: number): string {
  return count === 1 ? t("1 条记录") : t("{count} 条记录", { count });
}

export function compareRecords(
  left: TranscriptRecordView,
  right: TranscriptRecordView,
): number {
  return right.endedAtMs - left.endedAtMs || right.id.localeCompare(left.id);
}
