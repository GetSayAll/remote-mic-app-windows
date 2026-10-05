<script setup lang="ts">
import type { NavIcon, PageId } from "../navigation";
import { navigationItems } from "../navigation";

defineProps<{ activePage: PageId; version?: string | null }>();
const emit = defineEmits<{ select: [page: PageId] }>();

/**
 * 侧栏图标：SVG path 组（24x24 视窗，描边风格），形状对齐
 * macOS SettingsSection.systemImage（keyboard/link/shield.lefthalf.filled/
 * gearshape）。Windows 无 SF Symbols，用同形 SVG 还原。
 */
const ICON_PATHS: Record<NavIcon, { strokes: string[]; fills?: string[] }> = {
  keyboard: {
    // SF "keyboard"：圆角键盘轮廓 + 功能行点阵 + 底部长条
    strokes: [
      "M3.2 6.8h17.6a1.7 1.7 0 0 1 1.7 1.7v7a1.7 1.7 0 0 1-1.7 1.7H3.2a1.7 1.7 0 0 1-1.7-1.7v-7a1.7 1.7 0 0 1 1.7-1.7z",
      "M7 10.2h.01M10.4 10.2h.01M13.8 10.2h.01M17.2 10.2h.01",
      "M7.6 13.6h8.8",
    ],
  },
  template: {
    strokes: ["M5 3.8h11l3 3v13.4H5z", "M16 3.8v3.5h3", "M8.2 11h7.6", "M8.2 14.5h7.6", "M8.2 18h4.6"],
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
};
</script>

<template>
  <aside class="sidebar">
    <nav aria-label="设置页面">
      <button
        v-for="item in navigationItems"
        :key="item.id"
        class="nav-item"
        :class="{ active: activePage === item.id }"
        type="button"
        @click="emit('select', item.id)"
      >
        <svg class="nav-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <template v-if="ICON_PATHS[item.icon].fills">
            <path
              v-for="(path, index) in ICON_PATHS[item.icon].fills"
              :key="`f${index}`"
              :d="path"
              fill="currentColor"
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
        <span>{{ item.label }}</span>
      </button>
    </nav>

    <!-- 底部显示应用版本号（2026-10-02 用户指定：不再显示“预览版”）。版本来自
         运行快照的 package_info，与安装包/更新器同源；尚未读到时不显示占位。 -->
    <div class="sidebar-footer">
      <span v-if="version" class="sidebar-version">{{ version }}</span>
    </div>
  </aside>
</template>

<style scoped>
/* 版本号用等宽数字，避免运行中的宽度抖动。 */
.sidebar-version { font-variant-numeric: tabular-nums; letter-spacing: 0.2px; }
</style>
