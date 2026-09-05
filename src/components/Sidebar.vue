<script setup lang="ts">
import type { NavIcon, PageId } from "../navigation";
import { navigationItems } from "../navigation";

defineProps<{ activePage: PageId }>();
const emit = defineEmits<{ select: [page: PageId] }>();

/**
 * 侧栏图标：SVG path 组（24x24 视窗，描边风格），形状对齐
 * macOS SettingsSection.systemImage（keyboard/link/shield.lefthalf.filled/
 * info.circle）。Windows 无 SF Symbols，用同形 SVG 还原。
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
  info: {
    // SF "info.circle"：圆 + i
    strokes: ["M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18z", "M12 8.1h.01", "M12 11.4v5"],
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

    <div class="sidebar-footer">
      <span class="status-dot pending"></span>
      Windows Preview
    </div>
  </aside>
</template>
