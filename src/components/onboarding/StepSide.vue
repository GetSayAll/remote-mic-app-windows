<script setup lang="ts">
/**
 * 向导右栏（设计稿 2026-10-05）：上半插图、下半「检查卡」。
 * 检查卡每一行 = 当前步骤的一条「继续」条件（由 `onboarding/panel.ts` 推导）。
 * 窄栏/矮窗时由页面样式把本组件收进内容列（见 OnboardingPage.vue 的媒体查询）。
 */
import type { SidePanel, StatusRowState } from "../../onboarding/panel";

const props = defineProps<{ panel: SidePanel }>();

const GLYPH: Record<StatusRowState, string> = {
  pending: "○",
  busy: "◐",
  ok: "✓",
  warn: "!",
};

/** 声波条的示意高度（装饰元素，不随状态变化）。 */
const WAVE = [14, 30, 46, 30, 54, 22, 34];
</script>

<template>
  <aside class="onboarding-side">
    <div class="side-visual">
      <img
        v-if="props.panel.visual === 'remote'"
        class="side-photo"
        src="/RC003-remote-photo@2x.png"
        alt="小米蓝牙语音遥控器示意图"
        draggable="false"
      />
      <template v-else-if="props.panel.visual === 'app'">
        <img class="side-appicon" src="/app-logo.png" alt="" />
        <span class="side-wave" aria-hidden="true">
          <i v-for="(height, index) in WAVE" :key="index" :style="{ height: `${height}px` }"></i>
        </span>
      </template>
      <span v-else class="side-logo-done">
        <img class="side-appicon" src="/app-logo.png" alt="" />
        <span class="side-done-badge" aria-hidden="true">✓</span>
      </span>
      <span v-if="props.panel.caption" class="side-caption">{{ props.panel.caption }}</span>
    </div>

    <section
      v-if="props.panel.cardTitle && props.panel.rows?.length"
      class="onboarding-check-card"
      :aria-label="props.panel.cardTitle"
    >
      <h4>{{ props.panel.cardTitle }}</h4>
      <div
        v-for="row in props.panel.rows"
        :key="row.label"
        class="onboarding-crow"
        :data-state="row.state"
      >
        <span class="crow-label">{{ row.label }}</span>
        <span v-if="row.value" class="crow-value">{{ row.value }}</span>
        <span v-else class="crow-state" aria-hidden="true">{{ GLYPH[row.state] }}</span>
      </div>
    </section>
  </aside>
</template>
