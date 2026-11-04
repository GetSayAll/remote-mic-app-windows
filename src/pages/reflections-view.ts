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

/** 旧 manifest 可能把应用写成文件名，新记录写产品名；界面统一按产品名与当前语言显示。
 *
 * 键是记录里出现过的原始名（历史记录用 `SHGetFileInfoW` 取到的文件名，去扩展名），
 * 值是对齐 `PRESET_APPS` / `docs/product-copy.md` 的产品名。
 */
const APPLICATION_NAME_ALIASES: Record<string, string> = {
  微信: "微信",
  weixin: "微信",
  wechat: "微信",
  "chrome 浏览器": "Chrome 浏览器",
  chrome: "Chrome 浏览器",
  记事本: "记事本",
  notepad: "记事本",
};

export function displayApplicationName(name: string): string {
  const product = APPLICATION_NAME_ALIASES[name.trim().toLowerCase()];
  return product ? t(product) : name;
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
