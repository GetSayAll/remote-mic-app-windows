<script setup lang="ts">
import { computed } from "vue";
import type { AppIconIdentifier } from "../lib/bridge";
import type { NavIcon } from "../navigation";
import { navigationItems } from "../navigation";
import CrownIcon from "./CrownIcon.vue";
import { t, tNav } from "../lib/i18n";
import { effectiveLanguage } from "../lib/language-state";

/**
 * 侧栏文案：中文模式直接用原文；英文模式走导航专用词表（`nav.*`），避免与
 * 页面内同形文案（设备徽标「推荐」= Recommended、向导阶段「设置」= Set Up）
 * 争用同一个 key。
 */
function navLabel(item: { label: string }): string {
  return effectiveLanguage.value === "en" ? tNav(item.label) : item.label;
}

const props = withDefaults(
  defineProps<{
    activePage: string;
    /** 账号入口文案：未登录「我的」，已登录为账号显示名（对齐 Mac statistics）。 */
    profileLabel?: string;
    /** 顶部品牌区与字标使用当前应用图标。 */
    appIcon?: AppIconIdentifier;
    /** 侧栏品牌区版本行（对齐 Mac PR 803）；读不到时留空。 */
    appVersion?: string;
  }>(),
  { profileLabel: "我的", appIcon: "faceted-duck", appVersion: "" },
);
const emit = defineEmits<{ select: [page: string] }>();

const topNavigationItems = computed(() => navigationItems.filter((item) => item.id !== "profile"));
const profileItem = computed(() => navigationItems.find((item) => item.id === "profile"));
/** 未登录时 App 传「我的」；英文模式换成 Me，登录后的完整账号名原样显示。 */
const profileLabelText = computed(() =>
  props.profileLabel === "我的" || props.profileLabel === "个人中心"
    ? tNav("我的")
    : props.profileLabel,
);
const appIconPreview = computed(() =>
  props.appIcon === "standard" ? "/app-logo.webp" : "/app-icon-faceted-duck.webp",
);

/**
 * 侧栏图标：SVG path 组（24x24 视窗，描边风格），形状对齐
 * macOS SettingsSection.systemImage（keyboard/link/shield.lefthalf.filled/
 * gearshape）。Windows 无 SF Symbols，用同形 SVG 还原。
 */
const ICON_PATHS: Record<NavIcon, { strokes: string[]; fills?: string[] }> = {
  keyboard: {
    // SF "keyboard"：圆角键盘轮廓 + 三行点阵 + 底部长条。
    // 旧轮廓只有 10.4 高、宽高比接近 2:1，放在 18px 槽位里比 Mac 的 SF Symbol 扁；
    // 这里把轮廓抬到 16.4 高、宽度保持 20.4，接近 SF keyboard 的视觉比例。
    strokes: [
      "M3.2 3.8h17.6a1.7 1.7 0 0 1 1.7 1.7v12.9a1.7 1.7 0 0 1-1.7 1.7H3.2a1.7 1.7 0 0 1-1.7-1.7V5.5a1.7 1.7 0 0 1 1.7-1.7z",
      "M5.7 7.7h.01M8.4 7.7h.01M11.1 7.7h.01M13.8 7.7h.01M16.5 7.7h.01M19.2 7.7h.01",
      "M5.7 10.8h.01M8.4 10.8h.01M11.1 10.8h.01M13.8 10.8h.01M16.5 10.8h.01M19.2 10.8h.01",
      "M5.7 13.9h.01M8.4 13.9h.01M11.1 13.9h.01M13.8 13.9h.01M16.5 13.9h.01M19.2 13.9h.01",
      "M6.6 17.2h10.8",
    ],
  },
  sparkles: {
    // SF "sparkles"：一颗大四角星 + 两颗小星，语义对齐 Mac 推荐入口。
    strokes: [
      "M11.2 3.6l1.6 5.1 5.1 1.6-5.1 1.6-1.6 5.1-1.6-5.1-5.1-1.6 5.1-1.6z",
      "M18.6 3.2v3M17.1 4.7h3",
      "M5.2 17.6v2.8M3.8 19h2.8",
    ],
  },
  layers: {
    // SF "square.stack.3d.up"：层叠方块（组合动作；对齐 Mac 侧边栏图标）
    strokes: [
      "M12 3.2l9 4.6-9 4.6-9-4.6z",
      "M3.6 12.4l8.4 4.3 8.4-4.3",
      "M3.6 16.6l8.4 4.3 8.4-4.3",
    ],
  },
  link: {
    // SF "link"：两段互扣链环（对角）
    strokes: [
      "M10.4 13.2a4.6 4.6 0 0 0 7 .5l2.8-2.8a4.6 4.6 0 1 0-6.5-6.5l-1.6 1.6",
      "M13.6 10.8a4.6 4.6 0 0 0-7-.5l-2.8 2.8a4.6 4.6 0 1 0 6.5 6.5l1.6-1.6",
    ],
  },
  shield: {
    // SF "shield.lefthalf.filled"：盾形轮廓 + 左半填充
    strokes: ["M12 2.8l7.2 2.9v5.4c0 4.7-3.1 7.8-7.2 9.4-4.1-1.6-7.2-4.7-7.2-9.4V5.7z"],
    fills: ["M12 2.8L4.8 5.7v5.4c0 4.7 3.1 7.8 7.2 9.4z"],
  },
  gear: {
    // SF "gearshape"：外圈齿形 + 中央圆孔（齿形沿用 Feather 齿轮比例，与其余
    // 图标的 1.9 描边保持一致）
    strokes: [
      "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z",
      "M15.2 12a3.2 3.2 0 1 1-6.4 0 3.2 3.2 0 0 1 6.4 0z",
    ],
  },
  member: {
    // 会员图标由 CrownIcon 组件绘制（SF Symbols crown.fill）；保留空组维持类型映射。
    strokes: [],
  },
  textbubble: {
    // SF "text.bubble.fill"：实心圆角气泡 + 两行镂空文字（对齐 Mac 回眸入口）。
    // 两行文字用 evenodd 子路径挖空，避免固定写死侧栏/选中态的背景色。
    fills: [
      "M4.2 3.6h15.6a2.2 2.2 0 0 1 2.2 2.2v9.1a2.2 2.2 0 0 1-2.2 2.2h-7.1L8.1 21v-3.9H4.2A2.2 2.2 0 0 1 2 14.9V5.8a2.2 2.2 0 0 1 2.2-2.2zM6.4 7.8h11.2v1.6H6.4zM6.4 11.4h7.4v1.6H6.4z",
    ],
    strokes: [],
  },
  person: {
    // SF "person.crop.circle"：圆环 + 实心头肩。Mac 的 SF Symbol 内部是填充剪影，
    // 旧实现把头和肩也画成描边，账号入口看起来与 Mac 的图标不一致。
    strokes: ["M12 2.5a9.5 9.5 0 1 0 0 19 9.5 9.5 0 0 0 0-19z"],
    fills: [
      "M12 7.4a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4z",
      "M6.2 19.1c1.1-2.5 3.2-3.9 5.8-3.9s4.7 1.4 5.8 3.9c-1.5 1.1-3.6 1.8-5.8 1.8s-4.3-.7-5.8-1.8z",
    ],
  },
};
</script>

<template>
  <aside class="sidebar">
    <!-- 品牌区（对齐 Mac PR 803）：44px 应用图标 + SayAll + 版本行。 -->
    <div class="sidebar-brand">
      <img class="brand-logo" :src="appIconPreview" alt="" aria-hidden="true" />
      <div class="brand-text">
        <span class="brand-name">SayAll</span>
        <span v-if="appVersion" class="brand-version">{{ appVersion }}</span>
      </div>
    </div>
    <nav class="sidebar-nav" :aria-label="t('设置页面')">
      <button
        v-for="item in topNavigationItems"
        :key="item.id"
        class="nav-item"
        :class="{ active: activePage === item.id }"
        type="button"
        @click="emit('select', item.id)"
      >
        <CrownIcon v-if="item.icon === 'member'" class="nav-icon" />
        <svg
          v-else
          class="nav-icon"
          :class="`nav-icon-${item.icon}`"
          viewBox="0 0 24 24"
          fill="none"
          aria-hidden="true"
        >
          <template v-if="ICON_PATHS[item.icon].fills">
            <path
              v-for="(path, index) in ICON_PATHS[item.icon].fills"
              :key="`f${index}`"
              :d="path"
              fill="currentColor"
              fill-rule="evenodd"
            />
          </template>
          <path
            v-for="(path, index) in ICON_PATHS[item.icon].strokes"
            :key="index"
            :d="path"
            stroke="currentColor"
            stroke-width="1.9"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <span class="nav-label">{{ navLabel(item) }}</span>
      </button>

      <!-- 个人中心固定在最底部（对齐 Mac statistics）。 -->
      <div class="sidebar-spacer"></div>
      <div class="sidebar-account">
        <button
          v-if="profileItem"
          class="nav-item profile-nav-item"
          :class="{ active: activePage === profileItem.id }"
          type="button"
          :aria-label="profileLabelText"
          @click="emit('select', profileItem.id)"
        >
          <svg class="nav-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <template v-if="ICON_PATHS.person.fills">
              <path
                v-for="(path, index) in ICON_PATHS.person.fills"
                :key="`pf${index}`"
                :d="path"
                fill="currentColor"
                fill-rule="evenodd"
              />
            </template>
            <path
              v-for="(path, index) in ICON_PATHS.person.strokes"
              :key="index"
              :d="path"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <span class="nav-label">{{ profileLabelText }}</span>
        </button>
      </div>
    </nav>
  </aside>
</template>

<style scoped>
/* 品牌区：对齐 Mac PR 803（44pt logo、16pt SayAll、12pt 版本行）。 */
.sidebar-brand {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  height: 75px;
  padding: 11px 15px 10px;
  box-sizing: border-box;
}

.brand-logo {
  width: 44px;
  height: 44px;
  flex: 0 0 auto;
  object-fit: cover;
}

.brand-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding-top: 6px;
}

.brand-name {
  font-family: var(--font-brand);
  font-size: 16px;
  font-weight: 600;
  line-height: 1.2;
}

.brand-version {
  margin-top: -2px;
  overflow: hidden;
  color: var(--muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidebar-nav {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
  flex-direction: column;
  /* Mac 侧栏没有项间留白；选中背景是整块页面单元。 */
  gap: 0;
}

.sidebar-spacer {
  flex: 1 1 auto;
  min-height: 12px;
}

/* 账号区与导航之间只留呼吸空间，不再画短横线；总高 56（40 行 + 上下 6 + 行外距 4）。 */
.sidebar-account {
  margin-top: 6px;
  padding: 6px 0;
}

/* 图标在左、文字在右的紧凑横排；文字占满剩余宽度并保持左对齐。 */
.nav-label {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  /* 与 Mac SettingsSidebarRow 的 13pt semibold 对齐。 */
  font-size: 13px;
  font-weight: 600;
}

/* SF keyboard 是横向符号，18px 槽位里仍显得比其余图标大；缩 10% 做光学配重。 */
.nav-icon-keyboard {
  transform: scale(0.9);
}

/* 账号区由 40 行 + 上下 6 内边距组成（Mac 设计稿最小 56），长账号名换行时不显拥挤。 */

/* 账号名按 Mac 允许换行；普通导航项仍保持单行省略。 */
.profile-nav-item .nav-label {
  overflow: visible;
  overflow-wrap: anywhere;
  text-overflow: clip;
  white-space: normal;
}
</style>
