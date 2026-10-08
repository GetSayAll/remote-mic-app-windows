<script setup lang="ts">
/**
 * 状态行（设计稿 2026-10-05）：图标 + 标题（+说明）+ 右侧状态词或短值。
 * 状态同时用图形与文字表达，不依赖颜色单独传达（高对比度可用）。
 */
import type { StatusRowState } from "../../onboarding/panel";

const props = defineProps<{
  state: StatusRowState;
  title: string;
  desc?: string;
  /** 右侧短值（如 "2 / 3"）；缺省显示状态词。 */
  value?: string;
  /** true 时右侧不再显示状态词（如完成页：左侧图标已表达状态，见设计稿）。 */
  hideState?: boolean;
}>();

const GLYPH: Record<StatusRowState, string> = {
  pending: "○",
  busy: "◐",
  ok: "✓",
  warn: "!",
};

const STATE_TEXT: Record<StatusRowState, string> = {
  pending: "待满足",
  busy: "进行中",
  ok: "已就绪",
  warn: "需处理",
};
</script>

<template>
  <div class="onboarding-status-row" :data-state="props.state">
    <span class="status-icon" aria-hidden="true">{{ GLYPH[props.state] }}</span>
    <span class="status-text">
      <strong>{{ props.title }}</strong>
      <small v-if="props.desc">{{ props.desc }}</small>
    </span>
    <span v-if="props.value" class="status-value">{{ props.value }}</span>
    <span v-else-if="!props.hideState" class="status-state">{{ STATE_TEXT[props.state] }}</span>
  </div>
</template>
