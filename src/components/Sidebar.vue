<script setup lang="ts">
import { computed } from "vue";
import type { NavIcon } from "../navigation";
import { navigationItems } from "../navigation";
import CrownIcon from "./CrownIcon.vue";

const props = withDefaults(
  defineProps<{
    activePage: string;
    version?: string | null;
    /** 个人中心入口文案：未登录「我的」，已登录为脱敏后的邮箱前缀。 */
    profileLabel?: string;
  }>(),
  { version: null, profileLabel: "我的" },
);
const emit = defineEmits<{ select: [page: string] }>();

const topNavigationItems = computed(() => navigationItems.filter((item) => item.id !== "profile"));
const profileItem = computed(() => navigationItems.find((item) => item.id === "profile"));

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
  person: {
    // SF "person.crop.circle"：圆形轮廓 + 头肩剪影。
    strokes: [
      "M12 2.5a9.5 9.5 0 1 0 0 19 9.5 9.5 0 0 0 0-19z",
      "M12 7.2a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4z",
      "M5.8 19.2c1.1-2.8 3.2-4.3 6.2-4.3s5.1 1.5 6.2 4.3",
    ],
  },
};
</script>

<template>
  <aside class="sidebar">
    <nav class="sidebar-nav" aria-label="设置页面">
      <button
        v-for="item in topNavigationItems"
        :key="item.id"
        class="nav-item"
        :class="{ active: activePage === item.id }"
        type="button"
        @click="emit('select', item.id)"
      >
        <CrownIcon v-if="item.icon === 'member'" class="nav-icon" />
        <svg v-else class="nav-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
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

      <!-- 个人中心固定在最底部（对齐 Mac statistics），与顶部页面区分开。 -->
      <div class="sidebar-spacer"></div>
      <button
        v-if="profileItem"
        class="nav-item profile-nav-item"
        :class="{ active: activePage === profileItem.id }"
        type="button"
        aria-label="个人中心"
        @click="emit('select', profileItem.id)"
      >
        <svg class="nav-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
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
        <span>{{ props.profileLabel || "我的" }}</span>
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
.sidebar-nav {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
  flex-direction: column;
  gap: 4px;
}

.sidebar-spacer {
  flex: 1 1 auto;
  min-height: 12px;
}

.profile-nav-item {
  margin-top: 4px;
}

/* 版本号用等宽数字，避免运行中的宽度抖动。 */
.sidebar-version { font-variant-numeric: tabular-nums; letter-spacing: 0.2px; }
</style>
